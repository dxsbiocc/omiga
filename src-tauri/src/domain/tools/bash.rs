//! Bash tool — run shell commands with timeouts, cancellation, and safety checks
//!
//! Aligned with main-repo [`src/tools/BashTool/BashTool.tsx`] and [`src/utils/timeouts.ts`]:
//! - Optional **`timeout` in milliseconds** (same as upstream); falls back to **`timeout_secs`** or env
//! - Default **120s** unless `BASH_DEFAULT_TIMEOUT_MS` is set; max **600s** unless `BASH_MAX_TIMEOUT_MS`
//! - Optional fields: `description`, `run_in_background`, `dangerously_disable_sandbox` (parity with Zod schema)
//! - Uses **`bash -l -c`** so the environment matches login-shell init (see `bashProvider.ts` `getSpawnArgs`)
//!
//! Working directory resolution matches filesystem tools: project-relative paths,
//! absolute paths, and `~/` are allowed; relative paths must stay under the project root.

use super::{ToolContext, ToolError, ToolSchema};
use crate::errors::{BashError, FsError};
use crate::infrastructure::streaming::{StreamOutput, StreamOutputItem};
use async_trait::async_trait;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::time::timeout;

/// Total stdout+stderr cap to avoid huge tool results / memory use (≈2 MiB).
const MAX_OUTPUT_BYTES: usize = 2 * 1024 * 1024;

pub const DESCRIPTION: &str = r#"Execute a bash command in the given working directory.

Defaults (same as upstream Claude Code bash / `src/tools/BashTool`):
- Optional **`timeout`** in **milliseconds** (preferred). If omitted, `timeout_secs` or `BASH_DEFAULT_TIMEOUT_MS` applies (default **120s**).
- Maximum duration is **600s** (10 minutes), overridable via `BASH_MAX_TIMEOUT_MS`.
- Optional **`description`**: short human summary of what the command does (shown in UI / metadata).
- **`run_in_background`**: when true, the command runs in a **detached** task (like `spawnShellTask`); the tool returns immediately with a task id and output file path. Completion is emitted as Tauri event `background-shell-complete`.
- **`dangerously_disable_sandbox`**: ignored (no sandbox layer in Omiga); kept for API compatibility.
- `cwd` is optional: omit to use the session working directory (usually the project root), or set a path relative to the project root, or an absolute path / `~/...`.

Safety: fork bombs, writing raw disk devices, and `rm -rf /` (root filesystem) patterns are blocked. Leading `sleep N` (N ≥ 2) is blocked unless you use sub-second sleep for pacing — use normal execution instead of polling with sleep. Output is truncated if it exceeds a large byte limit."#;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BashArgs {
    pub command: String,
    /// Short human-readable summary (same role as `BashTool` Zod `description`)
    #[serde(default)]
    pub description: Option<String>,
    /// Relative to project root, absolute, or `~/...`; omit for session `cwd`
    #[serde(default)]
    pub cwd: Option<String>,
    /// Milliseconds — matches upstream `timeout` on the bash tool (takes precedence over `timeout_secs`)
    #[serde(default)]
    pub timeout: Option<u64>,
    /// Seconds when `timeout` (ms) is omitted. Defaults from env / 120s when unset.
    #[serde(default)]
    pub timeout_secs: Option<u64>,
    /// Kept for API compatibility; execution always reads stdout/stderr concurrently (no pipe deadlock)
    #[serde(default = "default_stream")]
    pub stream: bool,
    #[serde(default)]
    pub run_in_background: Option<bool>,
    #[serde(default)]
    pub dangerously_disable_sandbox: Option<bool>,
}

fn default_stream() -> bool {
    true
}

fn default_timeout_ms() -> u64 {
    std::env::var("BASH_DEFAULT_TIMEOUT_MS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .filter(|&ms| ms > 0)
        .unwrap_or(120_000)
}

fn max_timeout_ms() -> u64 {
    let from_env = std::env::var("BASH_MAX_TIMEOUT_MS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .filter(|&ms| ms > 0);
    let floor = default_timeout_ms();
    let default_cap = 600_000u64;
    from_env
        .map(|m| m.max(floor))
        .unwrap_or_else(|| default_cap.max(floor))
}

/// True when the command is `rm -rf /` or `rm -rf /*` (root wipe), not `rm -rf /home/...`.
fn is_rm_rf_root_deletion(cmd_lower: &str) -> bool {
    const NEEDLE: &str = "rm -rf /";
    let Some(idx) = cmd_lower.find(NEEDLE) else {
        return false;
    };
    let after = &cmd_lower[idx + NEEDLE.len()..];
    if after.trim().is_empty() {
        return true;
    }
    after.trim_start().starts_with('*')
}

/// Split on `&&` / `;` at list level (respects `'...'` and `"..."` with basic `\"` escapes in double quotes).
/// Returns `(first_segment, rest_after_first_separator)`.
fn split_first_list_segment(s: &str) -> (&str, &str) {
    let s = s.trim();
    let mut in_single = false;
    let mut in_double = false;
    let mut escaped = false;
    for (i, c) in s.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if in_double && c == '\\' {
            escaped = true;
            continue;
        }
        if !in_double && c == '\'' {
            in_single = !in_single;
            continue;
        }
        if !in_single && c == '"' {
            in_double = !in_double;
            continue;
        }
        if in_single || in_double {
            continue;
        }
        if s[i..].starts_with("&&") {
            return (s[..i].trim_end(), s[i + 2..].trim_start());
        }
        if c == ';' {
            return (s[..i].trim_end(), s[i + 1..].trim_start());
        }
    }
    (s, "")
}

fn sleep_first_segment_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^sleep\s+(\d+)\s*$").expect("regex"))
}

/// Mirrors [`src/tools/BashTool/BashTool.tsx`] `detectBlockedSleepPattern` when `MONITOR_TOOL` is on.
fn detect_blocked_sleep_pattern(command: &str, run_in_background: bool) -> Option<String> {
    if run_in_background {
        return None;
    }
    let (first, rest_after) = split_first_list_segment(command);
    let re = sleep_first_segment_re();
    let caps = re.captures(first)?;
    let secs: u32 = caps.get(1)?.as_str().parse().ok()?;
    if secs < 2 {
        return None;
    }
    let rest = rest_after.trim();
    if rest.is_empty() {
        Some(format!("standalone sleep {}", secs))
    } else {
        Some(format!("sleep {} followed by: {}", secs, rest))
    }
}

fn re_git_reset_hard() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\bgit\s+reset\s+--hard\b").expect("regex"))
}

fn re_git_push_force() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"\bgit\s+push\b[^;&|\n]*[ \t](--force|--force-with-lease|-f)\b").expect("regex")
    })
}

fn re_kubectl_delete() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\bkubectl\s+delete\b").expect("regex"))
}

fn re_terraform_destroy() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\bterraform\s+destroy\b").expect("regex"))
}

fn re_sql_drop() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)\b(DROP|TRUNCATE)\s+(TABLE|DATABASE|SCHEMA)\b").expect("regex")
    })
}

/// Informational warnings (same spirit as `destructiveCommandWarning.ts`); does not block execution.
fn destructive_command_warning(command: &str) -> Option<&'static str> {
    if re_git_reset_hard().is_match(command) {
        return Some("Note: may discard uncommitted changes");
    }
    if re_git_push_force().is_match(command) {
        return Some("Note: may overwrite remote history");
    }
    if re_kubectl_delete().is_match(command) {
        return Some("Note: may delete Kubernetes resources");
    }
    if re_terraform_destroy().is_match(command) {
        return Some("Note: may destroy Terraform infrastructure");
    }
    if re_sql_drop().is_match(command) {
        return Some("Note: may drop or truncate database objects");
    }
    None
}

impl BashArgs {
    pub fn validate(&self) -> Result<(), BashError> {
        let dangerous_patterns = [
            ":(){ :|:& };:",
            "> /dev/sda",
            "dd if=/dev/zero of=/dev",
        ];

        let cmd_lower = self.command.to_lowercase();
        if self.command.trim().is_empty() {
            return Err(BashError::ExecutionFailed {
                message: "command must not be empty".to_string(),
            });
        }
        if is_rm_rf_root_deletion(&cmd_lower) {
            return Err(BashError::DangerousCommandBlocked {
                command: self.command.clone(),
            });
        }
        for pattern in &dangerous_patterns {
            if cmd_lower.contains(pattern) {
                return Err(BashError::DangerousCommandBlocked {
                    command: self.command.clone(),
                });
            }
        }

        if let Some(detail) = detect_blocked_sleep_pattern(
            &self.command,
            self.run_in_background == Some(true),
        ) {
            return Err(BashError::ExecutionFailed {
                message: format!(
                    "Blocked: {}. Run long-running work without polling sleep, use a sub-second sleep only for pacing, or set `run_in_background: true` for long waits.",
                    detail
                ),
            });
        }

        Ok(())
    }

    fn effective_timeout_ms(&self) -> u64 {
        let cap = max_timeout_ms();
        if let Some(ms) = self.timeout {
            return ms.max(1).min(cap);
        }
        let secs = self
            .timeout_secs
            .unwrap_or_else(|| default_timeout_ms().div_ceil(1000));
        secs
            .max(1)
            .min(max_timeout_secs())
            .saturating_mul(1000)
            .min(cap)
    }
}

fn max_timeout_secs() -> u64 {
    (max_timeout_ms() + 999) / 1000
}

pub struct BashTool;

#[async_trait]
impl super::ToolImpl for BashTool {
    type Args = BashArgs;

    const DESCRIPTION: &'static str = DESCRIPTION;

    async fn execute(
        ctx: &ToolContext,
        args: Self::Args,
    ) -> Result<crate::infrastructure::streaming::StreamOutputBox, ToolError> {
        if args.dangerously_disable_sandbox == Some(true) {
            tracing::debug!("bash: dangerously_disable_sandbox ignored (no sandbox in Omiga)");
        }

        args.validate().map_err(|e| ToolError::ExecutionFailed {
            message: e.to_string(),
        })?;

        let cwd = resolve_bash_cwd(ctx, args.cwd.as_deref())?;
        if !cwd.exists() {
            return Err(BashError::WorkingDirNotFound {
                path: cwd.display().to_string(),
            }
            .into());
        }
        if !cwd.is_dir() {
            return Err(ToolError::InvalidArguments {
                message: format!("cwd is not a directory: {}", cwd.display()),
            });
        }

        let timeout_ms = args.effective_timeout_ms();
        let command = args.command.clone();
        let description = args.description.clone();
        let destructive = destructive_command_warning(&args.command).map(str::to_string);

        if args.run_in_background == Some(true) {
            let Some(bg) = ctx.background_shell.clone() else {
                return Err(ToolError::InvalidArguments {
                    message: "run_in_background requires an active chat session (invoke from the Omiga chat UI).".to_string(),
                });
            };
            let task_id = uuid::Uuid::new_v4().to_string();
            let output_path = ctx
                .background_output_dir
                .clone()
                .unwrap_or_else(|| std::path::PathBuf::from("."))
                .join(format!("bg-{task_id}.txt"));
            let desc_text = description
                .clone()
                .unwrap_or_else(|| truncate_command_summary(&command));
            crate::domain::background_shell::spawn_background_bash_task(
                bg,
                cwd.clone(),
                command.clone(),
                timeout_ms,
                output_path.clone(),
                task_id.clone(),
                desc_text,
            );
            let msg = format!(
                "Command running in background with task ID: {}\nOutput will be written to: {}\nYou will receive a notification when the command completes.",
                task_id,
                output_path.display()
            );
            return Ok(BashOutput {
                command,
                description,
                destructive_warning: destructive,
                exit_code: 0,
                stdout: vec![msg],
                stderr: vec![],
                background_task_id: Some(task_id),
                output_file: Some(output_path.to_string_lossy().to_string()),
            }
            .into_stream());
        }

        let cancel = ctx.cancel.clone();
        let output = run_bash_command(
            &cwd,
            &command,
            cancel,
            timeout_ms,
            args.stream,
        )
        .await?;

        Ok(BashOutput {
            command,
            description,
            destructive_warning: destructive,
            exit_code: output.exit_code,
            stdout: output.stdout,
            stderr: output.stderr,
            background_task_id: None,
            output_file: None,
        }
        .into_stream())
    }
}

fn resolve_bash_cwd(ctx: &ToolContext, cwd: Option<&str>) -> Result<PathBuf, FsError> {
    match cwd {
        None => Ok(ctx.cwd.clone()),
        Some(p) if p.trim().is_empty() => Ok(ctx.cwd.clone()),
        Some(p) => resolve_path(&ctx.project_root, p),
    }
}

fn truncate_command_summary(cmd: &str) -> String {
    const MAX_CHARS: usize = 80;
    let t = cmd.trim();
    if t.chars().count() <= MAX_CHARS {
        return t.to_string();
    }
    let mut s = String::new();
    for (i, c) in t.chars().enumerate() {
        if i + 3 >= MAX_CHARS {
            break;
        }
        s.push(c);
    }
    s.push_str("...");
    s
}

fn resolve_path(project_root: &Path, path: &str) -> Result<PathBuf, FsError> {
    let path_buf = if path.starts_with('/') || path.starts_with("~/") {
        if path.starts_with("~/") {
            let home = std::env::var("HOME")
                .map_err(|_| FsError::InvalidPath { path: path.to_string() })?;
            PathBuf::from(path.replacen("~", &home, 1))
        } else {
            PathBuf::from(path)
        }
    } else {
        project_root.join(path)
    };

    let canonical_project = project_root
        .canonicalize()
        .unwrap_or_else(|_| project_root.to_path_buf());
    let canonical_path = path_buf.canonicalize().unwrap_or_else(|_| path_buf.clone());

    if !canonical_path.starts_with(&canonical_project)
        && !path.starts_with('/')
        && !path.starts_with("~/")
    {
        return Err(FsError::PathTraversal {
            path: path.to_string(),
        });
    }

    Ok(path_buf)
}

pub(crate) struct BashRawOutput {
    pub(crate) exit_code: i32,
    pub(crate) stdout: Vec<String>,
    pub(crate) stderr: Vec<String>,
}

pub(crate) async fn run_bash_command(
    cwd: &Path,
    command: &str,
    cancel: tokio_util::sync::CancellationToken,
    timeout_ms: u64,
    _stream: bool,
) -> Result<BashRawOutput, ToolError> {
    let mut cmd = Command::new("bash");
    // Login shell + command, matching `bashProvider.ts` `getSpawnArgs` when no snapshot is used.
    cmd.arg("-l").arg("-c").arg(command);
    cmd.current_dir(cwd);
    cmd.stdin(std::process::Stdio::null());
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    #[cfg(unix)]
    cmd.process_group(0);

    let mut child = cmd.spawn().map_err(|e| ToolError::ExecutionFailed {
        message: format!("Failed to spawn process: {}", e),
    })?;

    let pid = child.id();

    let stdout = child.stdout.take().ok_or_else(|| ToolError::ExecutionFailed {
        message: "Failed to capture stdout".to_string(),
    })?;
    let stderr = child.stderr.take().ok_or_else(|| ToolError::ExecutionFailed {
        message: "Failed to capture stderr".to_string(),
    })?;

    let counter = Arc::new(AtomicUsize::new(0));
    let c_out = counter.clone();
    let c_err = counter;

    let read_out = read_lines_capped(BufReader::new(stdout), c_out);
    let read_err = read_lines_capped(BufReader::new(stderr), c_err);
    let wait_child = async move { child.wait().await };

    let work = async move {
        tokio::try_join!(read_out, read_err, wait_child)
    };

    let timeout_secs = (timeout_ms + 999) / 1000;
    let outcome = tokio::select! {
        _ = cancel.cancelled() => {
            if let Some(pid) = pid {
                kill_process_tree(pid).await;
            }
            return Err(BashError::Cancelled.into());
        }
        r = timeout(Duration::from_millis(timeout_ms), work) => {
            match r {
                Ok(Ok((stdout, stderr, status))) => Ok((stdout, stderr, status)),
                Ok(Err(e)) => Err(ToolError::ExecutionFailed {
                    message: format!("bash io error: {}", e),
                }),
                Err(_) => {
                    if let Some(pid) = pid {
                        kill_process_tree(pid).await;
                    }
                    Err(BashError::Timeout { seconds: timeout_secs }.into())
                }
            }
        }
    };

    let (stdout, stderr, status) = outcome?;

    let exit_code = status.code().unwrap_or(-1);

    Ok(BashRawOutput {
        exit_code,
        stdout,
        stderr,
    })
}

async fn read_lines_capped(
    reader: BufReader<impl tokio::io::AsyncRead + Unpin>,
    counter: Arc<AtomicUsize>,
) -> Result<Vec<String>, std::io::Error> {
    let mut lines = Vec::new();
    let mut buf = reader.lines();
    while let Some(line) = buf.next_line().await? {
        let len = line.len().saturating_add(1);
        let prev = counter.fetch_add(len, Ordering::Relaxed);
        if prev + len > MAX_OUTPUT_BYTES {
            lines.push(format!(
                "... [bash output truncated: exceeded {} bytes total]",
                MAX_OUTPUT_BYTES
            ));
            break;
        }
        lines.push(line);
    }
    Ok(lines)
}

async fn kill_process_tree(pid: u32) {
    #[cfg(unix)]
    {
        use nix::sys::signal::{killpg, Signal};
        use nix::unistd::Pid;
        let _ = killpg(Pid::from_raw(pid as i32), Signal::SIGTERM);
        tokio::time::sleep(Duration::from_millis(500)).await;
        let _ = killpg(Pid::from_raw(pid as i32), Signal::SIGKILL);
    }

    #[cfg(windows)]
    {
        let _ = Command::new("taskkill")
            .args(["/F", "/T", "/PID", &pid.to_string()])
            .output()
            .await;
    }
}

#[derive(Debug, Clone)]
pub struct BashOutput {
    pub command: String,
    pub description: Option<String>,
    pub destructive_warning: Option<String>,
    pub exit_code: i32,
    pub stdout: Vec<String>,
    pub stderr: Vec<String>,
    pub background_task_id: Option<String>,
    pub output_file: Option<String>,
}

impl StreamOutput for BashOutput {
    fn into_stream(self) -> Pin<Box<dyn futures::Stream<Item = StreamOutputItem> + Send>> {
        use futures::stream;

        let mut items: Vec<StreamOutputItem> = vec![
            StreamOutputItem::Metadata {
                key: "command".to_string(),
                value: self.command.clone(),
            },
            StreamOutputItem::Start,
        ];

        if let Some(ref d) = self.description {
            items.push(StreamOutputItem::Metadata {
                key: "description".to_string(),
                value: d.clone(),
            });
        }
        if let Some(ref w) = self.destructive_warning {
            items.push(StreamOutputItem::Metadata {
                key: "destructive_warning".to_string(),
                value: w.clone(),
            });
        }
        if let Some(ref id) = self.background_task_id {
            items.push(StreamOutputItem::Metadata {
                key: "background_task_id".to_string(),
                value: id.clone(),
            });
        }
        if let Some(ref p) = self.output_file {
            items.push(StreamOutputItem::Metadata {
                key: "output_file".to_string(),
                value: p.clone(),
            });
        }

        items.extend([
            StreamOutputItem::Stdout(self.stdout.join("\n")),
            StreamOutputItem::Stderr(self.stderr.join("\n")),
            StreamOutputItem::ExitCode(self.exit_code),
            StreamOutputItem::Complete,
        ]);

        Box::pin(stream::iter(items))
    }
}

pub fn schema() -> ToolSchema {
    ToolSchema::new(
        "bash",
        DESCRIPTION,
        serde_json::json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "Shell command executed via bash -l -c (login shell)"
                },
                "description": {
                    "type": "string",
                    "description": "Short description of what the command does (shown in UI / metadata)"
                },
                "cwd": {
                    "type": "string",
                    "description": "Working directory: omit for session cwd, or project-relative / absolute / ~/ path"
                },
                "timeout": {
                    "type": "integer",
                    "description": "Timeout in milliseconds (preferred; max from BASH_MAX_TIMEOUT_MS or 600000). Takes precedence over timeout_secs."
                },
                "timeout_secs": {
                    "type": "integer",
                    "description": "Timeout in seconds when `timeout` (ms) is omitted (default from BASH_DEFAULT_TIMEOUT_MS or 120)"
                },
                "stream": {
                    "type": "boolean",
                    "description": "Reserved for future incremental streaming (default: true)"
                },
                "run_in_background": {
                    "type": "boolean",
                    "description": "When true, run the command in a detached task; returns immediately with background_task_id and output path. Completion: Tauri event `background-shell-complete`."
                },
                "dangerously_disable_sandbox": {
                    "type": "boolean",
                    "description": "Ignored in Omiga (no sandbox); kept for API compatibility with Claude Code"
                }
            },
            "required": ["command"]
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::super::ToolError;
    use super::*;
    use crate::domain::tools::ToolImpl;
    use futures::StreamExt;

    fn args(cmd: &str) -> BashArgs {
        BashArgs {
            command: cmd.to_string(),
            description: None,
            cwd: None,
            timeout: None,
            timeout_secs: Some(60),
            stream: true,
            run_in_background: None,
            dangerously_disable_sandbox: None,
        }
    }

    #[tokio::test]
    async fn bash_runs_echo_smoke() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ToolContext::new(dir.path());
        let mut a = args("echo hello");
        a.timeout_secs = Some(30);
        let mut stream = BashTool::execute(&ctx, a).await.unwrap();
        while stream.next().await.is_some() {}
    }

    #[tokio::test]
    async fn run_in_background_without_chat_context_errors() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ToolContext::new(dir.path());
        let a = BashArgs {
            run_in_background: Some(true),
            ..args("echo hi")
        };
        let r = BashTool::execute(&ctx, a).await;
        match r {
            Err(ToolError::InvalidArguments { message }) => {
                assert!(message.contains("run_in_background"));
            }
            Err(e) => panic!("expected InvalidArguments, got {:?}", e),
            Ok(_) => panic!("expected error when background_shell is unset"),
        }
    }

    #[test]
    fn dangerous_rm_root_blocked() {
        let ok = args("rm -rf /home/user");
        assert!(ok.validate().is_ok());

        let bad = args("rm -rf /");
        assert!(bad.validate().is_err());
    }

    #[test]
    fn empty_command_rejected() {
        let bad = args("   ");
        assert!(bad.validate().is_err());
    }

    #[test]
    fn sleep_leading_blocked() {
        let blocked = args("sleep 5");
        assert!(blocked.validate().is_err());
        let ok_short = args("sleep 1");
        assert!(ok_short.validate().is_ok());
        let ok_bg = BashArgs {
            run_in_background: Some(true),
            ..args("sleep 5")
        };
        assert!(ok_bg.validate().is_ok());
    }

    #[test]
    fn timeout_ms_precedence() {
        let a = BashArgs {
            timeout: Some(5_000),
            timeout_secs: Some(999),
            ..args("true")
        };
        assert_eq!(a.effective_timeout_ms(), 5_000);
    }

    #[test]
    fn destructive_warning_metadata() {
        let w = destructive_command_warning("git reset --hard");
        assert!(w.is_some());
    }
}
