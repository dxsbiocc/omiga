//! Bash tool — run shell commands with timeouts, cancellation, and safety checks
//!
//! Aligned with main-repo [`src/tools/BashTool/BashTool.tsx`] and [`src/utils/timeouts.ts`]:
//! - Optional **`timeout` in milliseconds** (same as upstream); falls back to **`timeout_secs`** or env
//! - Default **120s** unless `BASH_DEFAULT_TIMEOUT_MS` is set; max **600s** unless `BASH_MAX_TIMEOUT_MS`
//! - Optional fields: `description`, `exec_session_id`, `run_in_background`, `dangerously_disable_sandbox` (parity with Zod schema)
//! - Uses **`bash -l -c`** so the environment matches login-shell init (see `bashProvider.ts` `getSpawnArgs`)
//!
//! Working directory resolution matches filesystem tools: project-relative paths,
//! absolute paths, and `~/` are allowed; relative paths must stay under the project root.

use super::ssh_paths::resolve_bash_cwd_ssh;
use super::{ToolContext, ToolError, ToolSchema};
use crate::errors::{BashError, FsError};
use crate::execution::{create_environment, EnvironmentConfig, EnvironmentType, ExecOptions};
use crate::infrastructure::streaming::{StreamOutput, StreamOutputItem};
use crate::llm::config::merged_ssh_configs;
use crate::llm::config::{load_config_file, LlmConfigFile, SshExecConfig};
use crate::utils::shell::shell_single_quote;
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
const SANDBOX_DENIED_PREFIX: &str = "SANDBOX_DENIED:";

pub const DESCRIPTION: &str = r#"Execute a bash command in the given working directory.

Defaults (same as upstream Claude Code bash / `src/tools/BashTool`):
- Optional **`timeout`** in **milliseconds** (preferred). If omitted, `timeout_secs` or `BASH_DEFAULT_TIMEOUT_MS` applies (default **120s**).
- Maximum duration is **600s** (10 minutes), overridable via `BASH_MAX_TIMEOUT_MS`.
- Optional **`description`**: short human summary of what the command does (shown in UI / metadata).
- **`exec_session_id`**: optional local-only persistent bash session id. When set, commands run in a reused piped `bash -l` process so shell state like exported variables can persist across calls. No PTY is allocated, so interactive/TUI programs are not supported.
- **`run_in_background`**: when true, the command runs in a **detached** task (like `spawnShellTask`); the tool returns immediately with a task id and output file path. Completion is emitted as Tauri event `background-shell-complete`.
- **`dangerously_disable_sandbox`**: when true, skip the local platform sandbox escape hatch. Background shell callers cannot pass this per-call flag, so they default to sandboxed execution and can use `OMIGA_SANDBOX_DISABLE=1` only as a process-wide emergency escape.
- `cwd` is optional: omit to use the session working directory (usually the project root), or set a path relative to the project root, or an absolute path / `~/...`.
- Workspace hygiene: when inspecting a user-provided data/input directory, treat that directory as read-only by default. For commands that create scripts, notebooks, logs, temporary files, figures, or result tables, run from the session working directory and pass input data paths explicitly unless the user asked to write inside the input directory.

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
    /// Local-only persistent bash session id.
    #[serde(default)]
    pub exec_session_id: Option<String>,
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

fn first_shell_word(segment: &str) -> Option<&str> {
    let mut parts = segment.split_whitespace();
    loop {
        let word = parts.next()?;
        if word.contains('=')
            && !word.starts_with('-')
            && word
                .split_once('=')
                .is_some_and(|(key, _)| key.chars().all(|c| c == '_' || c.is_ascii_alphanumeric()))
        {
            continue;
        }
        return Some(word.trim_matches(|c: char| c == '\'' || c == '"'));
    }
}

fn dedicated_tool_preference_violation(command: &str) -> Option<&'static str> {
    let trimmed = command.trim();
    let (first_segment, _) = split_first_list_segment(trimmed);
    let executable = first_shell_word(first_segment)?;
    let executable = executable.rsplit('/').next().unwrap_or(executable);
    let executable = executable.to_ascii_lowercase();

    match executable.as_str() {
        "grep" | "egrep" | "fgrep" | "rg" | "ripgrep" => {
            return Some(
                "Blocked: this is file/content search through bash. Use the dedicated `ripgrep` tool (`pattern`, optional `path`) so results are structured, bounded, and reviewable. If this task may need a workflow, call `list_skills` / `skill_view` before falling back to bash.",
            );
        }
        "cat" | "head" | "tail" | "less" | "more" => {
            return Some(
                "Blocked: this reads file content through bash. Use the dedicated `file_read` tool instead. If this task may need a workflow, call `list_skills` / `skill_view` first.",
            );
        }
        "sed" if first_segment.contains(" -n") || first_segment.starts_with("sed -n") => {
            return Some(
                "Blocked: this is a file-read style `sed` command. Use `file_read` for bounded file inspection, or `file_edit` for edits.",
            );
        }
        "python" | "python3" | "node" | "ruby" | "perl" | "rscript" => {
            let lower = trimmed.to_ascii_lowercase();
            if lower.contains(" -c ")
                || lower.contains(" -e ")
                || lower.contains(" <<")
                || trimmed.len() > 240
                || trimmed.contains('\n')
            {
                return Some(
                    "Blocked: long inline code through bash is fragile and caused the repeated quoting/corruption failures. Write the code with `file_write` / `file_edit` (or `notebook_edit` for notebooks), then run the saved script with bash.",
                );
            }
        }
        _ => {}
    }

    let lower = trimmed.to_ascii_lowercase();
    let looks_like_shell_write = lower.contains("<<")
        || lower.starts_with("cat >")
        || lower.contains(" cat >")
        || lower.starts_with("tee ")
        || lower.contains(" tee ")
        || lower.starts_with("echo ")
        || lower.starts_with("printf ");
    if looks_like_shell_write
        && (lower.contains(" >") || lower.contains(">>") || lower.contains("<<"))
    {
        return Some(
            "Blocked: this writes code/content through shell quoting. Use `file_write` or `file_edit` to create/update the file, then run it with bash if execution is needed.",
        );
    }

    None
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
        let dangerous_patterns = [":(){ :|:& };:", "> /dev/sda", "dd if=/dev/zero of=/dev"];

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

        if let Some(detail) =
            detect_blocked_sleep_pattern(&self.command, self.run_in_background == Some(true))
        {
            return Err(BashError::ExecutionFailed {
                message: format!(
                    "Blocked: {}. Run long-running work without polling sleep, use a sub-second sleep only for pacing, or set `run_in_background: true` for long waits.",
                    detail
                ),
            });
        }

        if let Some(message) = dedicated_tool_preference_violation(&self.command) {
            return Err(BashError::ExecutionFailed {
                message: message.to_string(),
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
        secs.max(1)
            .min(max_timeout_secs())
            .saturating_mul(1000)
            .min(cap)
    }
}

fn max_timeout_secs() -> u64 {
    max_timeout_ms().div_ceil(1000)
}

pub(crate) struct BashRawOutput {
    pub(crate) exit_code: i32,
    pub(crate) stdout: Vec<String>,
    pub(crate) stderr: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LocalSandbox {
    None,
    #[cfg(any(target_os = "macos", test))]
    Seatbelt,
    #[cfg(any(target_os = "linux", test))]
    Landlock,
}

impl LocalSandbox {
    fn is_enabled(self) -> bool {
        !matches!(self, Self::None)
    }

    fn treats_permission_denied_as_sandbox_denial(self) -> bool {
        #[cfg(any(target_os = "linux", test))]
        {
            matches!(self, Self::Landlock)
        }
        #[cfg(not(any(target_os = "linux", test)))]
        {
            let _ = self;
            false
        }
    }
}

fn active_local_sandbox() -> LocalSandbox {
    #[cfg(target_os = "macos")]
    {
        return LocalSandbox::Seatbelt;
    }
    #[cfg(target_os = "linux")]
    {
        return LocalSandbox::Landlock;
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        LocalSandbox::None
    }
}

/// Map local cwd under `project_root` to the active SSH session workspace.
fn ssh_remote_cwd(project_root: &Path, local_cwd: &Path) -> String {
    let root = crate::domain::tools::env_store::ssh_remote_root_for_project(project_root);
    let rel = match local_cwd.strip_prefix(project_root) {
        Ok(p) if p.as_os_str().is_empty() => {
            return root;
        }
        Ok(p) => p
            .to_string_lossy()
            .replace('\\', "/")
            .trim_start_matches('/')
            .to_string(),
        Err(_) => String::new(),
    };
    if rel.is_empty() {
        root
    } else {
        format!("{}/{}", root.trim_end_matches('/'), rel)
    }
}

fn pick_ssh_profile(cfg: &LlmConfigFile) -> Option<(&String, &SshExecConfig)> {
    let ssh_map = cfg.execution_envs.as_ref()?.ssh.as_ref()?;
    if let Ok(name) = std::env::var("OMIGA_SSH_PROFILE") {
        if let Some(c) = ssh_map.get(&name) {
            if c.enabled
                && c.effective_hostname().is_some()
                && c.user.as_ref().is_some_and(|u| !u.is_empty())
            {
                return ssh_map.get_key_value(&name);
            }
        }
    }
    let mut pairs: Vec<_> = ssh_map.iter().filter(|(_, c)| c.enabled).collect();
    pairs.sort_by(|a, b| a.0.cmp(b.0));
    for (name, c) in pairs {
        if c.effective_hostname().is_some() && c.user.as_ref().is_some_and(|u| !u.is_empty()) {
            return Some((name, c));
        }
    }
    None
}

#[derive(Clone, Copy)]
enum RemoteBackend {
    Ssh,
    Docker,
    Singularity,
}

fn pick_remote_backend(cfg: &LlmConfigFile, ctx: &ToolContext) -> Result<RemoteBackend, String> {
    let sb = ctx.sandbox_backend.trim().to_lowercase();
    if sb != "auto" && !sb.is_empty() {
        return match sb.as_str() {
            "ssh" => Ok(RemoteBackend::Ssh),
            "docker" => Ok(RemoteBackend::Docker),
            "singularity" => Ok(RemoteBackend::Singularity),
            "modal" | "daytona" => Err(format!(
                "sandbox backend `{}` is not available in this build",
                sb
            )),
            _ => Err(format!("unknown sandbox backend: {}", sb)),
        };
    }
    if let Ok(s) = std::env::var("OMIGA_REMOTE_BACKEND") {
        return match s.to_lowercase().as_str() {
            "ssh" => Ok(RemoteBackend::Ssh),
            "docker" => Ok(RemoteBackend::Docker),
            "singularity" => Ok(RemoteBackend::Singularity),
            "modal" | "daytona" => Err(format!(
                "OMIGA_REMOTE_BACKEND={} is not available in this build",
                s
            )),
            _ => Err(format!("unknown OMIGA_REMOTE_BACKEND={}", s)),
        };
    }
    if pick_ssh_profile(cfg).is_some() {
        return Ok(RemoteBackend::Ssh);
    }
    Err(
        "「远程」bash 需要可用的远端执行配置：在 omiga.yaml 的 execution_envs.ssh 下添加已启用且含 HostName/User 的主机；\
         或在沙箱菜单选择 SSH / Docker / Singularity，\
         或设置 OMIGA_REMOTE_BACKEND=docker|singularity 并配置对应环境。\
         可选环境变量：OMIGA_SSH_PROFILE（指定 ssh 配置名）。"
            .to_string(),
    )
}

fn exec_result_to_bash_raw(exec_result: crate::execution::ExecResult) -> BashRawOutput {
    BashRawOutput {
        exit_code: exec_result.returncode,
        stdout: if exec_result.output.is_empty() {
            vec![]
        } else {
            exec_result.output.lines().map(String::from).collect()
        },
        stderr: vec![],
    }
}

fn ssh_config_for_context(ctx: &ToolContext) -> Result<SshExecConfig, ToolError> {
    let name = ctx
        .ssh_server
        .as_ref()
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| ToolError::ExecutionFailed {
            message: "未选择 SSH 服务器：请在执行环境菜单中选择 SSH 主机。".to_string(),
        })?;
    let merged = merged_ssh_configs().map_err(|e| ToolError::ExecutionFailed { message: e })?;
    merged
        .get(name)
        .cloned()
        .ok_or_else(|| ToolError::ExecutionFailed {
            message: format!("找不到 SSH 配置: {}", name),
        })
}

async fn run_remote_bash_ssh(
    ctx: &ToolContext,
    ssh_cfg: &SshExecConfig,
    remote_cwd: String,
    command: &str,
    timeout_ms: u64,
) -> Result<BashRawOutput, ToolError> {
    let host = ssh_cfg
        .effective_hostname()
        .ok_or_else(|| ToolError::ExecutionFailed {
            message: "SSH: 配置缺少 HostName 或 Host".to_string(),
        })?;
    let user = ssh_cfg
        .user
        .as_ref()
        .ok_or_else(|| ToolError::ExecutionFailed {
            message: "SSH: 配置缺少 User".to_string(),
        })?;
    let config = EnvironmentConfig {
        r#type: EnvironmentType::Ssh,
        cwd: remote_cwd.clone(),
        timeout: timeout_ms.max(1_000),
        ssh_host: Some(host.to_string()),
        ssh_user: Some(user.clone()),
        ssh_port: ssh_cfg.port,
        ssh_key_path: ssh_cfg.identity_file.clone(),
        ssh_project_root: Some(ctx.project_root.clone()),
        task_id: format!("omiga-{}", uuid::Uuid::new_v4()),
        ..Default::default()
    };
    let env = create_environment(config)
        .await
        .map_err(|e| ToolError::ExecutionFailed {
            message: format!("远程 SSH 环境: {}", e),
        })?;
    let exec_opts = ExecOptions {
        timeout: Some(timeout_ms),
        cwd: Some(remote_cwd),
        stdin_data: None,
    };
    let exec_result = {
        let mut guard = env.lock().await;
        let r = guard.execute(command, exec_opts).await;
        let _ = guard.cleanup().await;
        r
    }
    .map_err(|e| ToolError::ExecutionFailed {
        message: format!("远程 SSH 执行: {}", e),
    })?;
    Ok(exec_result_to_bash_raw(exec_result))
}

async fn run_remote_bash_docker(
    _cfg: &LlmConfigFile,
    command: &str,
    timeout_ms: u64,
) -> Result<BashRawOutput, ToolError> {
    let image = std::env::var("OMIGA_DOCKER_IMAGE").unwrap_or_else(|_| "ubuntu:22.04".to_string());
    let cwd = "/workspace".to_string();
    let config = EnvironmentConfig {
        r#type: EnvironmentType::Docker,
        image: Some(image),
        cwd: cwd.clone(),
        timeout: timeout_ms.max(1_000),
        task_id: format!("omiga-{}", uuid::Uuid::new_v4()),
        ..Default::default()
    };
    let env = create_environment(config)
        .await
        .map_err(|e| ToolError::ExecutionFailed {
            message: format!("Docker 远程: {}", e),
        })?;
    let exec_opts = ExecOptions {
        timeout: Some(timeout_ms),
        cwd: Some(cwd),
        stdin_data: None,
    };
    let exec_result = {
        let mut guard = env.lock().await;
        let r = guard.execute(command, exec_opts).await;
        let _ = guard.cleanup().await;
        r
    }
    .map_err(|e| ToolError::ExecutionFailed {
        message: format!("Docker 远程执行: {}", e),
    })?;
    Ok(exec_result_to_bash_raw(exec_result))
}

async fn run_remote_bash_singularity(
    _cfg: &LlmConfigFile,
    command: &str,
    timeout_ms: u64,
) -> Result<BashRawOutput, ToolError> {
    let image = std::env::var("OMIGA_SINGULARITY_IMAGE")
        .unwrap_or_else(|_| "docker://ubuntu:22.04".to_string());
    let cwd = "/workspace".to_string();
    let config = EnvironmentConfig {
        r#type: EnvironmentType::Singularity,
        image: Some(image),
        cwd: cwd.clone(),
        timeout: timeout_ms.max(1_000),
        task_id: format!("omiga-{}", uuid::Uuid::new_v4()),
        network: true,
        ..Default::default()
    };
    let env = create_environment(config)
        .await
        .map_err(|e| ToolError::ExecutionFailed {
            message: format!("Singularity 远程: {}", e),
        })?;
    let exec_opts = ExecOptions {
        timeout: Some(timeout_ms),
        cwd: Some(cwd),
        stdin_data: None,
    };
    let exec_result = {
        let mut guard = env.lock().await;
        let r = guard.execute(command, exec_opts).await;
        let _ = guard.cleanup().await;
        r
    }
    .map_err(|e| ToolError::ExecutionFailed {
        message: format!("Singularity 远程执行: {}", e),
    })?;
    Ok(exec_result_to_bash_raw(exec_result))
}

async fn run_remote_bash(
    ctx: &ToolContext,
    cfg: &LlmConfigFile,
    local_cwd: &Path,
    command: &str,
    timeout_ms: u64,
) -> Result<BashRawOutput, ToolError> {
    let backend =
        pick_remote_backend(cfg, ctx).map_err(|m| ToolError::ExecutionFailed { message: m })?;
    match backend {
        RemoteBackend::Ssh => {
            let profile = pick_ssh_profile(cfg).ok_or_else(|| ToolError::ExecutionFailed {
                message:
                    "未找到可用的 SSH 配置（execution_envs.ssh：enabled，且设置 HostName/User）。"
                        .to_string(),
            })?;
            let remote_cwd = ssh_remote_cwd(&ctx.project_root, local_cwd);
            run_remote_bash_ssh(ctx, profile.1, remote_cwd, command, timeout_ms).await
        }
        RemoteBackend::Docker => run_remote_bash_docker(cfg, command, timeout_ms).await,
        RemoteBackend::Singularity => run_remote_bash_singularity(cfg, command, timeout_ms).await,
    }
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
            tracing::debug!("bash: sandbox disabled by caller");
        }

        args.validate().map_err(|e| ToolError::ExecutionFailed {
            message: e.to_string(),
        })?;

        let timeout_ms = args.effective_timeout_ms();
        let command = args.command.clone();
        let description = args.description.clone();
        let destructive = destructive_command_warning(&args.command).map(str::to_string);

        if ctx.execution_environment == "ssh" {
            if args.exec_session_id.is_some() {
                return Err(ToolError::InvalidArguments {
                    message: "exec_session_id is only supported for local bash execution."
                        .to_string(),
                });
            }
            if args.run_in_background == Some(true) {
                return Err(ToolError::InvalidArguments {
                    message:
                        "run_in_background is not supported when execution environment is SSH."
                            .to_string(),
                });
            }
            let remote_cwd = resolve_bash_cwd_ssh(&ctx.project_root, &ctx.cwd, args.cwd.as_deref())
                .map_err(ToolError::from)?;
            let output = if let Some(ref store) = ctx.env_store {
                // Session-cached path — no per-call connect/disconnect
                let env_arc = store.get_or_create(ctx, timeout_ms).await?;
                let exec_opts = ExecOptions {
                    timeout: Some(timeout_ms),
                    cwd: Some(remote_cwd),
                    stdin_data: None,
                };
                let exec_result = {
                    let mut guard = env_arc.lock().await;
                    guard.execute(&command, exec_opts).await
                }
                .map_err(|e| ToolError::ExecutionFailed {
                    message: format!("远程 SSH 执行: {}", e),
                })?;
                exec_result_to_bash_raw(exec_result)
            } else {
                // Fallback: legacy per-call path
                let cfg = ssh_config_for_context(ctx)?;
                run_remote_bash_ssh(ctx, &cfg, remote_cwd, &command, timeout_ms).await?
            };
            return Ok(BashOutput {
                command,
                description,
                destructive_warning: destructive,
                exit_code: output.exit_code,
                stdout: output.stdout,
                stderr: output.stderr,
                background_task_id: None,
                output_file: None,
            }
            .into_stream());
        }

        // For sandbox environments, use env_store if available to avoid per-call env create/destroy
        if ctx.execution_environment == "sandbox" || ctx.execution_environment == "remote" {
            if args.exec_session_id.is_some() {
                return Err(ToolError::InvalidArguments {
                    message: "exec_session_id is only supported for local bash execution."
                        .to_string(),
                });
            }
            if args.run_in_background == Some(true) {
                return Err(ToolError::InvalidArguments {
                    message: "run_in_background is not supported when execution environment is remote/sandbox."
                        .to_string(),
                });
            }
            let output = if let Some(ref store) = ctx.env_store {
                let env_arc = store.get_or_create(ctx, timeout_ms).await?;
                // Respect args.cwd; fall back to /workspace (sandbox default root).
                let remote_cwd = args
                    .cwd
                    .as_deref()
                    .filter(|s| !s.trim().is_empty())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "/workspace".to_string());
                let exec_opts = ExecOptions {
                    timeout: Some(timeout_ms),
                    cwd: Some(remote_cwd),
                    stdin_data: None,
                };
                let exec_result = {
                    let mut guard = env_arc.lock().await;
                    guard.execute(&command, exec_opts).await
                }
                .map_err(|e| ToolError::ExecutionFailed {
                    message: format!("远程执行: {}", e),
                })?;
                exec_result_to_bash_raw(exec_result)
            } else {
                // Fallback: legacy per-call path
                let cwd = resolve_bash_cwd(ctx, args.cwd.as_deref())?;
                let cfg = load_config_file().map_err(|e| ToolError::ExecutionFailed {
                    message: format!("Failed to load config: {}", e),
                })?;
                run_remote_bash(ctx, &cfg, &cwd, &command, timeout_ms).await?
            };
            return Ok(BashOutput {
                command,
                description,
                destructive_warning: destructive,
                exit_code: output.exit_code,
                stdout: output.stdout,
                stderr: output.stderr,
                background_task_id: None,
                output_file: None,
            }
            .into_stream());
        }

        // Wrap command with virtual-env activation when configured.
        let command = prepend_venv_activation(&ctx.local_venv_type, &ctx.local_venv_name, &command);

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

        if let Some(exec_session_id) = args.exec_session_id.as_ref() {
            if args.run_in_background == Some(true) {
                return Err(ToolError::InvalidArguments {
                    message: "exec_session_id cannot be combined with run_in_background."
                        .to_string(),
                });
            }

            let session_command = if args.cwd.as_deref().is_some_and(|s| !s.trim().is_empty()) {
                let cwd_string = cwd.to_string_lossy().to_string();
                format!(
                    "cd {} && {{\n{}\n}}",
                    shell_escape_arg(&cwd_string),
                    command
                )
            } else {
                command.clone()
            };
            let result = crate::domain::tools::exec_session::ExecSessionManager::global()
                .exec_with_initial_cwd(
                    exec_session_id,
                    &session_command,
                    Duration::from_millis(timeout_ms),
                    Some(&cwd),
                )
                .await
                .map_err(exec_session_error_to_tool_error)?;

            match result {
                crate::domain::tools::exec_session::ExecSessionResult::Completed(output) => {
                    return Ok(BashOutput {
                        command,
                        description,
                        destructive_warning: destructive,
                        exit_code: output.exit_code,
                        stdout: output.stdout,
                        stderr: output.stderr,
                        background_task_id: None,
                        output_file: None,
                    }
                    .into_stream());
                }
                crate::domain::tools::exec_session::ExecSessionResult::Timeout(t) => {
                    return Err(BashError::Timeout { seconds: t.seconds }.into());
                }
            }
        }

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
                crate::domain::background_shell::BackgroundBashTask {
                    handle: bg,
                    cwd: cwd.clone(),
                    command: command.clone(),
                    timeout_ms,
                    output_path: output_path.clone(),
                    task_id: task_id.clone(),
                    description: desc_text,
                    cancel: ctx.cancel.clone(),
                },
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
        let output = run_bash_command_inner(
            &cwd,
            &command,
            cancel,
            timeout_ms,
            args.stream,
            args.dangerously_disable_sandbox == Some(true),
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

/// Build the activation preamble for a local virtual environment.
/// Returns the original command unchanged when no venv is configured.
pub(crate) fn prepend_venv_activation(venv_type: &str, venv_name: &str, command: &str) -> String {
    let name = venv_name.trim();
    if name.is_empty() || venv_type == "none" || venv_type.is_empty() {
        return command.to_string();
    }
    let preamble = match venv_type {
        "conda" => format!(
            // Source conda init script so `conda activate` is available even in non-interactive shells.
            "source \"$(conda info --base)/etc/profile.d/conda.sh\"; \
             conda activate {name}; ",
            name = shell_escape_arg(name),
        ),
        "venv" => format!(
            "source {path}/bin/activate; ",
            path = shell_escape_arg(name),
        ),
        "pyenv" => format!("export PYENV_VERSION={ver}; ", ver = shell_escape_arg(name),),
        _ => return command.to_string(),
    };
    format!("{}{}", preamble, command)
}

/// Single-quote a shell argument. Delegates to the shared crate utility.
fn shell_escape_arg(s: &str) -> String {
    shell_single_quote(s)
}

fn exec_session_error_to_tool_error(
    err: crate::domain::tools::exec_session::ExecSessionError,
) -> ToolError {
    match err {
        crate::domain::tools::exec_session::ExecSessionError::InvalidSessionId => {
            ToolError::InvalidArguments {
                message: "exec_session_id must be non-empty, at most 128 bytes, and contain no newlines or NUL bytes.".to_string(),
            }
        }
        other => ToolError::ExecutionFailed {
            message: other.to_string(),
        },
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
            let home = std::env::var("HOME").map_err(|_| FsError::InvalidPath {
                path: path.to_string(),
            })?;
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

pub(crate) async fn run_bash_command(
    cwd: &Path,
    command: &str,
    cancel: tokio_util::sync::CancellationToken,
    timeout_ms: u64,
    _stream: bool,
) -> Result<BashRawOutput, ToolError> {
    // Compatibility path for background-shell callers. They cannot currently
    // pass the per-call `dangerously_disable_sandbox` flag without changing the
    // background module API, so the safer default is sandboxed execution with a
    // process-wide `OMIGA_SANDBOX_DISABLE=1` escape hatch.
    run_bash_command_inner(cwd, command, cancel, timeout_ms, _stream, false).await
}

async fn run_bash_command_inner(
    cwd: &Path,
    command: &str,
    cancel: tokio_util::sync::CancellationToken,
    timeout_ms: u64,
    _stream: bool,
    sandbox_disabled: bool,
) -> Result<BashRawOutput, ToolError> {
    let (mut cmd, sandbox) = local_bash_command(command, cwd, sandbox_disabled)?;
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

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| ToolError::ExecutionFailed {
            message: "Failed to capture stdout".to_string(),
        })?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| ToolError::ExecutionFailed {
            message: "Failed to capture stderr".to_string(),
        })?;

    let counter = Arc::new(AtomicUsize::new(0));
    let c_out = counter.clone();
    let c_err = counter;

    let read_out = read_lines_capped(BufReader::new(stdout), c_out);
    let read_err = read_lines_capped(BufReader::new(stderr), c_err);
    let wait_child = async move { child.wait().await };

    let work = async move { tokio::try_join!(read_out, read_err, wait_child) };

    let timeout_secs = timeout_ms.div_ceil(1000);
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

    if sandbox.is_enabled() && exit_code != 0 && output_looks_like_sandbox_denial(&stderr, sandbox)
    {
        let message = sandbox_denied_message(&stderr);
        return Err(ToolError::ExecutionFailed { message });
    }

    Ok(BashRawOutput {
        exit_code,
        stdout,
        stderr,
    })
}

fn local_bash_command(
    command: &str,
    cwd: &Path,
    sandbox_disabled: bool,
) -> Result<(Command, LocalSandbox), ToolError> {
    // Read the process-global disable flag here, then delegate the decision to
    // the pure `build_local_bash_command`. Tests exercise the pure builder with
    // an explicit flag so they never call `setenv`, which is not thread-safe
    // against concurrent `getenv` under parallel `cargo test`.
    build_local_bash_command(command, cwd, sandbox_disabled || sandbox_disabled_by_env())
}

fn build_local_bash_command(
    command: &str,
    cwd: &Path,
    disabled: bool,
) -> Result<(Command, LocalSandbox), ToolError> {
    if disabled {
        return Ok((raw_bash_command(command), LocalSandbox::None));
    }

    if super::sandbox::is_supported() {
        let policy = super::sandbox::SandboxPolicy::from_env();
        let writable_roots = super::sandbox::default_writable_roots(cwd);
        let backend = super::sandbox::backend_name();
        tracing::debug!(
            network = ?policy.network,
            "bash: using {backend} local sandbox"
        );
        let command = super::sandbox::wrap_local_command(&policy, &writable_roots, command)
            .map_err(|message| ToolError::ExecutionFailed {
                message: format!("Failed to prepare local sandbox: {message}"),
            })?;
        return Ok((command, active_local_sandbox()));
    }

    tracing::debug!("bash: {}", super::sandbox::unavailable_reason());
    Ok((raw_bash_command(command), LocalSandbox::None))
}

fn sandbox_disabled_by_env() -> bool {
    parse_sandbox_disable_flag(std::env::var("OMIGA_SANDBOX_DISABLE").ok().as_deref())
}

fn parse_sandbox_disable_flag(raw: Option<&str>) -> bool {
    raw.map(str::trim)
        .is_some_and(|value| matches!(value, "1" | "true" | "TRUE" | "yes" | "YES"))
}

fn raw_bash_command(command: &str) -> Command {
    let mut cmd = Command::new("bash");
    // Login shell + command, matching `bashProvider.ts` `getSpawnArgs` when no snapshot is used.
    cmd.arg("-l").arg("-c").arg(command);
    cmd
}

fn output_looks_like_sandbox_denial(stderr: &[String], sandbox: LocalSandbox) -> bool {
    stderr.iter().any(|line| {
        let lower = line.to_ascii_lowercase();
        lower.contains("operation not permitted")
            || lower.contains("sandbox")
            || lower.contains("deny")
            || lower.contains("not permitted")
            || (sandbox.treats_permission_denied_as_sandbox_denial()
                && lower.contains("permission denied"))
    })
}

fn sandbox_denied_message(stderr: &[String]) -> String {
    let stderr_text = stderr.join("\n");
    if stderr_text.trim().is_empty() {
        format!(
            "{} local sandbox denied this command. Single-run unsandboxed retry requires explicit approval by a higher layer.",
            SANDBOX_DENIED_PREFIX
        )
    } else {
        format!(
            "{} local sandbox denied this command. Single-run unsandboxed retry requires explicit approval by a higher layer.\nstderr:\n{}",
            SANDBOX_DENIED_PREFIX, stderr_text
        )
    }
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
                    "description": "Working directory: omit for session cwd, or project-relative / absolute / ~/ path. For commands that create scripts, notebooks, logs, figures, result tables, or temp files, use the session cwd and pass data/input paths explicitly unless the user asked to write inside that input directory."
                },
                "exec_session_id": {
                    "type": "string",
                    "description": "Optional local-only persistent bash session id. Commands with the same id reuse a piped bash process so shell state can persist. No PTY is allocated; interactive/TUI programs are not supported. On timeout, the session is terminated and the next call starts fresh."
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
                    "description": "When true, skip the local platform sandbox. Background shell callers default to sandboxed execution and can only use OMIGA_SANDBOX_DISABLE=1 as a process-wide escape hatch."
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
    use std::sync::MutexGuard;

    // Shared with the sandbox network/seatbelt test modules: `local_bash_command`
    // reads the `OMIGA_SANDBOX_*` env vars, so all sandbox env tests must
    // serialize through one process-wide lock (see `sandbox::sandbox_env_test_lock`).
    fn bash_env_lock() -> MutexGuard<'static, ()> {
        crate::domain::tools::sandbox::sandbox_env_test_lock()
    }

    fn restore_env(key: &str, value: Option<std::ffi::OsString>) {
        match value {
            Some(value) => std::env::set_var(key, value),
            None => std::env::remove_var(key),
        }
    }

    fn args(cmd: &str) -> BashArgs {
        BashArgs {
            command: cmd.to_string(),
            description: None,
            cwd: None,
            exec_session_id: None,
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
    fn background_compat_path_defaults_to_sandbox_when_supported() {
        // Pure builder: not disabled -> sandbox wrapper when the platform
        // supports it. No `setenv`, so this is safe under parallel test runs.
        let dir = tempfile::tempdir().unwrap();
        let (cmd, sandbox) = build_local_bash_command("true", dir.path(), false).unwrap();

        if crate::domain::tools::sandbox::is_supported() {
            assert!(sandbox.is_enabled());
            #[cfg(target_os = "macos")]
            {
                assert_eq!(sandbox, LocalSandbox::Seatbelt);
                assert!(
                    format!("{:?}", cmd).contains("sandbox-exec"),
                    "expected sandbox-exec wrapper, got {:?}",
                    cmd
                );
            }
            #[cfg(target_os = "linux")]
            {
                assert_eq!(sandbox, LocalSandbox::Landlock);
                assert!(
                    format!("{:?}", cmd).contains("bash"),
                    "expected bash command with Landlock pre_exec, got {:?}",
                    cmd
                );
            }
        } else {
            assert_eq!(sandbox, LocalSandbox::None);
        }
    }

    #[test]
    fn global_sandbox_disable_env_bypasses_local_wrapper() {
        // The disable flag short-circuits the wrapper regardless of platform.
        let dir = tempfile::tempdir().unwrap();
        let (_cmd, sandbox) = build_local_bash_command("true", dir.path(), true).unwrap();
        assert_eq!(sandbox, LocalSandbox::None);
    }

    #[test]
    fn sandbox_disable_flag_parsing_matches_truthy_values() {
        assert!(parse_sandbox_disable_flag(Some("1")));
        assert!(parse_sandbox_disable_flag(Some(" true ")));
        assert!(parse_sandbox_disable_flag(Some("YES")));
        assert!(!parse_sandbox_disable_flag(Some("0")));
        assert!(!parse_sandbox_disable_flag(Some("")));
        assert!(!parse_sandbox_disable_flag(None));
    }

    #[test]
    fn sandbox_denied_message_has_machine_readable_prefix() {
        let message = sandbox_denied_message(&["Operation not permitted".to_string()]);
        assert!(message.starts_with(SANDBOX_DENIED_PREFIX));
    }

    #[test]
    fn sandbox_denial_detection_includes_landlock_permission_denied() {
        assert!(output_looks_like_sandbox_denial(
            &["bash: /root/blocked: Permission denied".to_string(),],
            LocalSandbox::Landlock
        ));
    }

    #[test]
    fn sandbox_denial_detection_excludes_plain_permission_denied_without_landlock() {
        let stderr = ["bash: /root/blocked: Permission denied".to_string()];
        assert!(!output_looks_like_sandbox_denial(
            &stderr,
            LocalSandbox::None
        ));
        assert!(!output_looks_like_sandbox_denial(
            &stderr,
            LocalSandbox::Seatbelt
        ));
    }

    #[test]
    fn sandbox_denial_detection_keeps_seatbelt_terms() {
        assert!(output_looks_like_sandbox_denial(
            &["Operation not permitted".to_string()],
            LocalSandbox::Seatbelt
        ));
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    // Heavyweight: spawns a real sandboxed bash process. Under the default
    // multi-threaded suite it holds the env lock across `.await` while other
    // threads read `OMIGA_SANDBOX_*` via libc `getenv` (not thread-safe against
    // the `setenv` a locked writer performs), which makes it flaky. Gated to
    // run on demand; the `SANDBOX_DENIED:` prefix logic is covered
    // deterministically by `sandbox_denied_message_has_machine_readable_prefix`.
    #[ignore = "run on demand: spawns a real sandbox-exec bash process"]
    async fn sandbox_denial_returns_structured_error() {
        let _guard = bash_env_lock();
        let previous = std::env::var_os("OMIGA_SANDBOX_DISABLE");
        std::env::remove_var("OMIGA_SANDBOX_DISABLE");

        if !crate::domain::tools::sandbox::is_supported() {
            restore_env("OMIGA_SANDBOX_DISABLE", previous);
            return;
        }

        let dir = tempfile::tempdir().unwrap();
        let deny_path =
            Path::new(&std::env::var("HOME").expect("HOME")).join("omiga_sb_denied_signal.txt");
        let _ = std::fs::remove_file(&deny_path);

        let result = run_bash_command_inner(
            dir.path(),
            r#"echo x > "$HOME/omiga_sb_denied_signal.txt""#,
            tokio_util::sync::CancellationToken::new(),
            30_000,
            true,
            false,
        )
        .await;

        match result {
            Err(ToolError::ExecutionFailed { message }) => {
                assert!(message.starts_with(SANDBOX_DENIED_PREFIX), "{message}");
            }
            Ok(output) => panic!(
                "expected structured sandbox denial, got successful exit code {}",
                output.exit_code
            ),
            Err(other) => panic!("expected structured sandbox denial, got error {other}"),
        }
        assert!(!deny_path.exists());

        restore_env("OMIGA_SANDBOX_DISABLE", previous);
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
    fn file_search_and_read_are_rejected_when_typed_tools_exist() {
        let grep = args(r#"grep -rn -i "slurm" src 2>/dev/null | head -30"#);
        let grep_err = grep.validate().unwrap_err().to_string();
        assert!(grep_err.contains("ripgrep"));

        let cat = args("cat src/main.rs");
        let cat_err = cat.validate().unwrap_err().to_string();
        assert!(cat_err.contains("file_read"));
    }

    #[test]
    fn shell_quoting_writes_and_inline_scripts_are_rejected() {
        let heredoc = args("python3 - <<'PY'\nprint('hello')\nPY");
        let heredoc_err = heredoc.validate().unwrap_err().to_string();
        assert!(heredoc_err.contains("file_write"));

        let echo_write = args("echo 'hello' > output.txt");
        let echo_err = echo_write.validate().unwrap_err().to_string();
        assert!(echo_err.contains("file_write"));

        let saved_script = args("python3 scripts/analyse.py");
        assert!(saved_script.validate().is_ok());
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

    // ─── prepend_venv_activation tests ────────────────────────────────────────

    #[test]
    fn venv_type_none_returns_command_unchanged() {
        let cmd = prepend_venv_activation("none", "myenv", "python main.py");
        assert_eq!(cmd, "python main.py");
    }

    #[test]
    fn venv_type_empty_returns_command_unchanged() {
        let cmd = prepend_venv_activation("", "myenv", "python main.py");
        assert_eq!(cmd, "python main.py");
    }

    #[test]
    fn venv_name_empty_returns_command_unchanged() {
        let cmd = prepend_venv_activation("conda", "", "python main.py");
        assert_eq!(cmd, "python main.py");
    }

    #[test]
    fn venv_name_whitespace_only_returns_command_unchanged() {
        let cmd = prepend_venv_activation("conda", "   ", "python main.py");
        assert_eq!(cmd, "python main.py");
    }

    #[test]
    fn conda_env_prepends_source_and_activate() {
        let cmd = prepend_venv_activation("conda", "myenv", "python main.py");
        assert!(cmd.contains("conda.sh"), "should source conda init");
        assert!(
            cmd.contains("conda activate 'myenv'"),
            "should activate env"
        );
        assert!(cmd.ends_with("python main.py"), "original command appended");
    }

    #[test]
    fn conda_env_name_with_shell_metacharacters_is_quoted() {
        // An env named "env; rm -rf /" should appear inside single quotes so the
        // semicolon is inert — it must NOT appear as a bare unquoted token.
        let cmd = prepend_venv_activation("conda", "env; rm -rf /", "ls");
        // The entire name must be single-quoted
        assert!(
            cmd.contains("'env; rm -rf /'"),
            "name must be single-quoted"
        );
        // The activate call must end with the quoted name (no bare semicolon after it
        // that would let the shell treat "; rm -rf /" as a separate command segment)
        let activate_pos = cmd.find("conda activate").expect("activate missing");
        let after_activate = &cmd[activate_pos..];
        // After "conda activate " the very next char must be a single quote
        let name_start = after_activate.find("activate ").unwrap() + "activate ".len();
        assert_eq!(
            &after_activate[name_start..name_start + 1],
            "'",
            "name must start with single quote"
        );
    }

    #[test]
    fn venv_path_prepends_source_activate() {
        let cmd = prepend_venv_activation("venv", "/home/user/project/.venv", "python main.py");
        assert!(
            cmd.contains("source '/home/user/project/.venv'/bin/activate"),
            "should source activate script"
        );
        assert!(cmd.ends_with("python main.py"));
    }

    #[test]
    fn venv_path_with_spaces_is_safely_quoted() {
        let cmd = prepend_venv_activation("venv", "/home/my user/proj/.venv", "ls");
        assert!(
            cmd.contains("'/home/my user/proj/.venv'"),
            "path must be single-quoted"
        );
    }

    #[test]
    fn pyenv_version_sets_env_var() {
        let cmd = prepend_venv_activation("pyenv", "3.11.5", "python --version");
        assert!(
            cmd.contains("PYENV_VERSION='3.11.5'"),
            "should set PYENV_VERSION"
        );
        assert!(cmd.ends_with("python --version"));
    }

    #[test]
    fn pyenv_version_with_shell_special_chars_quoted() {
        let cmd = prepend_venv_activation("pyenv", "3.11; malicious", "ls");
        assert!(
            !cmd.contains("malicious;"),
            "must not execute injected code bare"
        );
        assert!(cmd.contains("'3.11; malicious'"), "version must be quoted");
    }

    #[test]
    fn unknown_venv_type_returns_command_unchanged() {
        let cmd = prepend_venv_activation("nix", "myshell", "ls");
        assert_eq!(cmd, "ls");
    }
}
