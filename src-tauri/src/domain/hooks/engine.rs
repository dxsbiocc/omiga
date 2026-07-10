use super::schema::{HookConfigFile, HookDeclaration, HookEvent};
use serde::Serialize;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

const DEFAULT_HOOK_TIMEOUT_MS: u64 = 10_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreHookOutcome {
    Proceed,
    Block { reason: String },
    ModifyArgs { new_args_json: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PostHookOutcome {
    Keep,
    AppendFeedback { text: String },
}

#[derive(Debug, Clone, Default)]
pub struct HookEngine {
    declarations: Arc<Vec<HookDeclaration>>,
}

#[derive(Debug)]
struct CommandOutput {
    exit_code: Option<i32>,
    stdout: String,
    stderr: String,
}

#[derive(Debug)]
enum CommandRunError {
    Spawn(String),
    Timeout,
    Wait(String),
}

#[derive(Debug, Serialize)]
struct PreToolUseInput<'a> {
    event: &'static str,
    tool_name: &'a str,
    args_json: &'a str,
    args: Value,
}

#[derive(Debug, Serialize)]
struct PostToolUseInput<'a> {
    event: &'static str,
    tool_name: &'a str,
    args_json: &'a str,
    args: Value,
    output: &'a str,
    is_error: bool,
}

impl HookEngine {
    pub fn new(declarations: Vec<HookDeclaration>) -> Self {
        Self {
            declarations: Arc::new(declarations),
        }
    }

    pub fn empty() -> Self {
        Self::default()
    }

    /// Load project hooks from `<project>/.omiga/hooks.toml`.
    ///
    /// Hooks are intentionally kept in their own project-scoped TOML file
    /// instead of the LLM provider config so the tool lifecycle can be changed
    /// without touching provider settings or widening config structs outside the
    /// allowed G11 files. A missing file returns an empty engine.
    pub fn load_for_project(project_root: &Path) -> Self {
        let path = hook_config_path(project_root);
        if !path.exists() {
            return Self::empty();
        }

        match std::fs::read_to_string(&path)
            .map_err(|err| err.to_string())
            .and_then(|content| {
                toml::from_str::<HookConfigFile>(&content).map_err(|err| err.to_string())
            }) {
            Ok(config) => Self::new(config.hooks),
            Err(err) => {
                tracing::warn!(
                    path = %path.display(),
                    error = %err,
                    "Failed to load hook config; continuing without hooks"
                );
                Self::empty()
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.declarations.is_empty()
    }

    pub async fn run_pre_tool_use(&self, tool_name: &str, args_json: &str) -> PreHookOutcome {
        if self.declarations.is_empty() {
            return PreHookOutcome::Proceed;
        }

        let mut current_args_json = args_json.to_string();
        for declaration in self.matching(HookEvent::PreToolUse, tool_name) {
            let payload = pre_tool_payload(tool_name, &current_args_json);
            let output = match run_hook_command(declaration, &payload).await {
                Ok(output) => output,
                Err(err) => {
                    return PreHookOutcome::Block {
                        reason: command_error_reason(err),
                    }
                }
            };

            if output.exit_code.unwrap_or(1) != 0 {
                return PreHookOutcome::Block {
                    reason: non_zero_reason(&output),
                };
            }

            let stdout = output.stdout.trim();
            if stdout.is_empty() {
                continue;
            }

            match serde_json::from_str::<Value>(stdout) {
                Ok(Value::Object(_)) => current_args_json = stdout.to_string(),
                Ok(_) => {
                    return PreHookOutcome::Block {
                        reason: "PreToolUse hook stdout must be a JSON object when non-empty"
                            .to_string(),
                    }
                }
                Err(err) => {
                    return PreHookOutcome::Block {
                        reason: format!("PreToolUse hook returned invalid JSON: {err}"),
                    }
                }
            }
        }

        if current_args_json == args_json {
            PreHookOutcome::Proceed
        } else {
            PreHookOutcome::ModifyArgs {
                new_args_json: current_args_json,
            }
        }
    }

    pub async fn run_post_tool_use(
        &self,
        tool_name: &str,
        args_json: &str,
        output: &str,
        is_error: bool,
    ) -> PostHookOutcome {
        if self.declarations.is_empty() {
            return PostHookOutcome::Keep;
        }

        let mut feedback = Vec::new();
        for declaration in self.matching(HookEvent::PostToolUse, tool_name) {
            let payload = post_tool_payload(tool_name, args_json, output, is_error);
            match run_hook_command(declaration, &payload).await {
                Ok(command_output) => {
                    if command_output.exit_code.unwrap_or(1) == 0 {
                        let text = command_output.stdout.trim();
                        if !text.is_empty() {
                            feedback.push(text.to_string());
                        }
                    } else {
                        tracing::warn!(
                            tool = %tool_name,
                            status = ?command_output.exit_code,
                            stderr = %command_output.stderr.trim(),
                            "PostToolUse hook failed; leaving tool output unchanged"
                        );
                    }
                }
                Err(err) => {
                    tracing::warn!(
                        tool = %tool_name,
                        error = %command_error_reason(err),
                        "PostToolUse hook failed; leaving tool output unchanged"
                    );
                }
            }
        }

        if feedback.is_empty() {
            PostHookOutcome::Keep
        } else {
            PostHookOutcome::AppendFeedback {
                text: feedback.join("\n"),
            }
        }
    }

    fn matching<'a>(
        &'a self,
        event: HookEvent,
        tool_name: &'a str,
    ) -> impl Iterator<Item = &'a HookDeclaration> + 'a {
        self.declarations.iter().filter(move |declaration| {
            declaration.event == event && declaration.matcher.matches_tool(tool_name)
        })
    }
}

pub fn hook_config_path(project_root: &Path) -> PathBuf {
    project_root.join(".omiga").join("hooks.toml")
}

fn pre_tool_payload(tool_name: &str, args_json: &str) -> String {
    let input = PreToolUseInput {
        event: "PreToolUse",
        tool_name,
        args_json,
        args: parse_json_or_raw(args_json),
    };
    serde_json::to_string(&input).unwrap_or_else(|_| "{}".to_string())
}

fn post_tool_payload(tool_name: &str, args_json: &str, output: &str, is_error: bool) -> String {
    let input = PostToolUseInput {
        event: "PostToolUse",
        tool_name,
        args_json,
        args: parse_json_or_raw(args_json),
        output,
        is_error,
    };
    serde_json::to_string(&input).unwrap_or_else(|_| "{}".to_string())
}

fn parse_json_or_raw(raw: &str) -> Value {
    serde_json::from_str(raw).unwrap_or_else(|_| serde_json::json!({ "raw": raw }))
}

async fn run_hook_command(
    declaration: &HookDeclaration,
    payload: &str,
) -> Result<CommandOutput, CommandRunError> {
    let mut child = Command::new("bash")
        .arg("-c")
        .arg(&declaration.command)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|err| CommandRunError::Spawn(err.to_string()))?;

    if let Some(mut stdin) = child.stdin.take() {
        let payload = payload.to_string();
        tokio::spawn(async move {
            let _ = stdin.write_all(payload.as_bytes()).await;
        });
    }

    let timeout = Duration::from_millis(declaration.timeout_ms.unwrap_or(DEFAULT_HOOK_TIMEOUT_MS));
    let output = tokio::time::timeout(timeout, child.wait_with_output())
        .await
        .map_err(|_| CommandRunError::Timeout)?
        .map_err(|err| CommandRunError::Wait(err.to_string()))?;

    Ok(CommandOutput {
        exit_code: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
    })
}

fn non_zero_reason(output: &CommandOutput) -> String {
    let stderr = output.stderr.trim();
    if !stderr.is_empty() {
        return stderr.to_string();
    }

    let stdout = output.stdout.trim();
    if !stdout.is_empty() {
        return stdout.to_string();
    }

    match output.exit_code {
        Some(code) => format!("PreToolUse hook exited with status {code}"),
        None => "PreToolUse hook terminated without an exit status".to_string(),
    }
}

fn command_error_reason(err: CommandRunError) -> String {
    match err {
        CommandRunError::Spawn(err) => format!("Failed to spawn hook command: {err}"),
        CommandRunError::Timeout => "Hook command timed out".to_string(),
        CommandRunError::Wait(err) => format!("Failed to wait for hook command: {err}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::hooks::schema::{HookEvent, HookMatcher};
    use std::fs;

    fn hook(event: HookEvent, tool_name: &str, command: String) -> HookDeclaration {
        HookDeclaration {
            event,
            matcher: HookMatcher {
                tool_name: tool_name.to_string(),
            },
            command,
            timeout_ms: Some(2_000),
        }
    }

    fn script(dir: &tempfile::TempDir, name: &str, body: &str) -> String {
        let path = dir.path().join(name);
        fs::write(&path, body).expect("write test hook script");
        format!("sh {}", shell_words::quote(&path.to_string_lossy()))
    }

    #[tokio::test]
    async fn hooks_pre_tool_use_non_zero_blocks() {
        let dir = tempfile::tempdir().expect("tempdir");
        let command = script(&dir, "block.sh", "printf 'blocked by hook' >&2\nexit 7\n");
        let engine = HookEngine::new(vec![hook(HookEvent::PreToolUse, "bash", command)]);

        let outcome = engine
            .run_pre_tool_use("bash", r#"{"cmd":"echo hi"}"#)
            .await;

        assert_eq!(
            outcome,
            PreHookOutcome::Block {
                reason: "blocked by hook".to_string()
            }
        );
    }

    #[tokio::test]
    async fn hooks_pre_tool_use_json_stdout_modifies_args() {
        let dir = tempfile::tempdir().expect("tempdir");
        let command = script(
            &dir,
            "modify.sh",
            "printf '%s' '{\"cmd\":\"echo changed\"}'\n",
        );
        let engine = HookEngine::new(vec![hook(HookEvent::PreToolUse, "bash", command)]);

        let outcome = engine
            .run_pre_tool_use("bash", r#"{"cmd":"echo old"}"#)
            .await;

        assert_eq!(
            outcome,
            PreHookOutcome::ModifyArgs {
                new_args_json: r#"{"cmd":"echo changed"}"#.to_string()
            }
        );
    }

    #[tokio::test]
    async fn hooks_pre_tool_use_empty_stdout_proceeds() {
        let dir = tempfile::tempdir().expect("tempdir");
        let command = script(&dir, "proceed.sh", "exit 0\n");
        let engine = HookEngine::new(vec![hook(HookEvent::PreToolUse, "bash", command)]);

        let outcome = engine
            .run_pre_tool_use("bash", r#"{"cmd":"echo hi"}"#)
            .await;

        assert_eq!(outcome, PreHookOutcome::Proceed);
    }

    #[tokio::test]
    async fn hooks_post_tool_use_stdout_appends_feedback() {
        let dir = tempfile::tempdir().expect("tempdir");
        let command = script(&dir, "feedback.sh", "printf 'lint: ok'\n");
        let engine = HookEngine::new(vec![hook(HookEvent::PostToolUse, "bash", command)]);

        let outcome = engine
            .run_post_tool_use("bash", r#"{"cmd":"echo hi"}"#, "hello", false)
            .await;

        assert_eq!(
            outcome,
            PostHookOutcome::AppendFeedback {
                text: "lint: ok".to_string()
            }
        );
    }

    #[tokio::test]
    async fn hooks_matcher_only_runs_for_named_tool() {
        let dir = tempfile::tempdir().expect("tempdir");
        let command = script(&dir, "block.sh", "printf 'blocked' >&2\nexit 1\n");
        let engine = HookEngine::new(vec![hook(HookEvent::PreToolUse, "bash", command)]);

        let outcome = engine.run_pre_tool_use("read", r#"{"path":"a"}"#).await;

        assert_eq!(outcome, PreHookOutcome::Proceed);
    }

    #[tokio::test]
    async fn hooks_missing_config_returns_proceed_and_keep() {
        let dir = tempfile::tempdir().expect("tempdir");
        let engine = HookEngine::load_for_project(dir.path());

        assert!(engine.is_empty());
        assert_eq!(
            engine
                .run_pre_tool_use("bash", r#"{"cmd":"echo hi"}"#)
                .await,
            PreHookOutcome::Proceed
        );
        assert_eq!(
            engine
                .run_post_tool_use("bash", r#"{"cmd":"echo hi"}"#, "hello", false)
                .await,
            PostHookOutcome::Keep
        );
    }
}
