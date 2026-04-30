//! Embedded terminal sessions for the Terminal tab.
//!
//! This intentionally does **not** open the operating system Terminal app. It spawns a
//! shell/SSH/container bridge with piped stdin/stdout/stderr and streams chunks back to
//! the WebView, similar to VS Code's integrated terminal panel (without full PTY
//! emulation or terminal-control dependencies).

use super::CommandResult;
use crate::app_state::OmigaAppState;
use crate::commands::execution_envs::get_merged_ssh_configs;
use crate::domain::tools::{env_store::EnvStore, ToolContext};
use crate::errors::{AppError, FsError};
use crate::execution::ExternalTerminalCommand;
use crate::llm::config::SshExecConfig;
use crate::utils::shell::shell_single_quote;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::sync::OnceLock;
use tauri::{AppHandle, Emitter, State};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{ChildStdin, Command};
use tokio::sync::Mutex;

const TERMINAL_ENV_TIMEOUT_MS: u64 = 60_000;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalStartRequest {
    pub terminal_id: String,
    pub cwd: Option<String>,
    pub execution_environment: Option<String>,
    pub ssh_profile_name: Option<String>,
    pub sandbox_backend: Option<String>,
    pub session_id: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalStartResponse {
    pub terminal_id: String,
    pub cwd: String,
    pub label: String,
    pub execution_environment: String,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TerminalOutputEvent {
    pub terminal_id: String,
    pub stream: String,
    pub data: String,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TerminalExitEvent {
    pub terminal_id: String,
    pub code: Option<i32>,
}

struct TerminalSession {
    stdin: Arc<Mutex<ChildStdin>>,
    pid: Option<u32>,
}

static TERMINAL_SESSIONS: OnceLock<Mutex<HashMap<String, TerminalSession>>> = OnceLock::new();

fn terminal_sessions() -> &'static Mutex<HashMap<String, TerminalSession>> {
    TERMINAL_SESSIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn fs_io_error(message: impl Into<String>) -> AppError {
    AppError::Fs(FsError::IoError {
        message: message.into(),
    })
}

fn validate_terminal_id(id: &str) -> CommandResult<String> {
    let trimmed = id.trim();
    if trimmed.is_empty()
        || trimmed.len() > 96
        || !trimmed
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(AppError::Fs(FsError::InvalidPath {
            path: id.to_string(),
        }));
    }
    Ok(trimmed.to_string())
}

fn canonical_local_cwd(cwd: &str) -> CommandResult<PathBuf> {
    let trimmed = cwd.trim();
    if trimmed.is_empty() || trimmed == "." {
        return Err(fs_io_error("请先选择本地工作区，再打开内嵌终端"));
    }

    let canonical = PathBuf::from(trimmed)
        .canonicalize()
        .map_err(|e| fs_io_error(format!("无法解析终端工作区 `{trimmed}`: {e}")))?;
    if !canonical.is_dir() {
        return Err(AppError::Fs(FsError::InvalidPath {
            path: format!("终端工作区不是目录: {}", canonical.display()),
        }));
    }
    Ok(canonical)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TerminalShell {
    Bash,
    Zsh,
    Powershell,
}

impl TerminalShell {
    fn default_for_environment(execution_environment: &str) -> Self {
        if execution_environment == "local" || execution_environment.is_empty() {
            if cfg!(target_os = "windows") {
                Self::Powershell
            } else {
                Self::Zsh
            }
        } else {
            Self::Bash
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Bash => "bash",
            Self::Zsh => "zsh",
            Self::Powershell => "PowerShell",
        }
    }

    fn local_program_and_args(self) -> CommandResult<(String, Vec<String>)> {
        match self {
            Self::Bash => Ok(("bash".to_string(), vec!["-l".to_string()])),
            Self::Zsh => Ok(("zsh".to_string(), vec!["-l".to_string()])),
            Self::Powershell if cfg!(target_os = "windows") => {
                Ok(("powershell.exe".to_string(), vec!["-NoLogo".to_string()]))
            }
            Self::Powershell => Err(fs_io_error("当前平台不支持本地 PowerShell 终端")),
        }
    }

    fn unix_program(self) -> CommandResult<&'static str> {
        match self {
            Self::Bash => Ok("bash"),
            Self::Zsh => Ok("zsh"),
            Self::Powershell => Err(fs_io_error("PowerShell 不能用于 SSH 或容器终端")),
        }
    }
}

fn validate_remote_cwd(cwd: &str) -> CommandResult<String> {
    let trimmed = cwd.trim();
    if trimmed.is_empty() || trimmed == "." {
        return Err(fs_io_error("请先选择远端工作区，再打开 SSH 终端"));
    }
    if trimmed.contains('\n') || trimmed.contains('\0') {
        return Err(AppError::Fs(FsError::InvalidPath {
            path: cwd.to_string(),
        }));
    }
    if trimmed == "~" {
        return Ok("~".to_string());
    }
    if let Some(rest) = trimmed.strip_prefix("~/") {
        if rest.split('/').any(|seg| seg == "..") {
            return Err(AppError::Fs(FsError::PathTraversal {
                path: cwd.to_string(),
            }));
        }
        return Ok(format!("~/{}", rest));
    }
    if !trimmed.starts_with('/') {
        return Err(AppError::Fs(FsError::InvalidPath {
            path: cwd.to_string(),
        }));
    }
    let mut out = PathBuf::new();
    for component in Path::new(trimmed).components() {
        match component {
            Component::RootDir => out.push("/"),
            Component::ParentDir => {
                return Err(AppError::Fs(FsError::PathTraversal {
                    path: cwd.to_string(),
                }));
            }
            Component::Normal(part) => {
                out.push(part);
            }
            _ => {}
        }
    }
    Ok(out.to_string_lossy().replace('\\', "/"))
}

fn remote_cd_target(path: &str) -> String {
    if path == "~" {
        "~".to_string()
    } else if let Some(rest) = path.strip_prefix("~/") {
        format!("\"$HOME\"/{}", shell_single_quote(rest))
    } else {
        shell_single_quote(path)
    }
}

fn expand_tilde_identity(path: &str) -> String {
    if let Some(rest) = path.strip_prefix("~/") {
        dirs::home_dir()
            .map(|home| home.join(rest).to_string_lossy().to_string())
            .unwrap_or_else(|| path.to_string())
    } else {
        path.to_string()
    }
}

fn local_shell_command(
    cwd: &Path,
    shell: TerminalShell,
) -> CommandResult<TerminalStartResponseAndCommand> {
    let (program, args) = shell.local_program_and_args()?;
    Ok(TerminalStartResponseAndCommand {
        cwd: cwd.to_string_lossy().to_string(),
        label: format!("本地 · {}", shell.label()),
        execution_environment: "local".to_string(),
        command: ExternalTerminalCommand::new(program, args, format!("本地 · {}", shell.label())),
        current_dir: Some(cwd.to_path_buf()),
        wrap_in_host_pty: shell != TerminalShell::Powershell,
    })
}

fn ssh_terminal_command(
    profile_name: &str,
    cfg: &SshExecConfig,
    remote_cwd: &str,
    shell: TerminalShell,
) -> CommandResult<ExternalTerminalCommand> {
    let host = cfg
        .effective_hostname()
        .ok_or_else(|| fs_io_error(format!("SSH profile `{profile_name}` has no HostName")))?;
    let user = cfg
        .user
        .as_ref()
        .filter(|u| !u.trim().is_empty())
        .ok_or_else(|| fs_io_error(format!("SSH profile `{profile_name}` has no User")))?;

    let mut args = vec![
        "-tt".to_string(),
        "-o".to_string(),
        "ConnectTimeout=15".to_string(),
    ];
    if cfg.port != 22 {
        args.push("-p".to_string());
        args.push(cfg.port.to_string());
    }
    if let Some(identity) = &cfg.identity_file {
        args.push("-i".to_string());
        args.push(expand_tilde_identity(identity));
    }
    args.push(format!("{}@{}", user, host));

    let shell_program = shell.unix_program()?;
    let cd_target = remote_cd_target(remote_cwd);
    let script = format!("cd {cd_target} || exit 126; exec {shell_program} -l");
    args.push(format!(
        "{shell_program} -lc {}",
        shell_single_quote(&script)
    ));

    Ok(ExternalTerminalCommand::new(
        "ssh",
        args,
        format!("SSH · {profile_name} · {}", shell.label()),
    ))
}

async fn session_env_store(app_state: &OmigaAppState, session_id: &str) -> CommandResult<EnvStore> {
    let trimmed = session_id.trim();
    if trimmed.is_empty() {
        return Err(fs_io_error("打开容器终端需要有效会话 ID"));
    }
    let sessions = app_state.chat.sessions.read().await;
    let runtime = sessions.get(trimmed).ok_or_else(|| {
        fs_io_error(format!(
            "会话 `{}` 尚未初始化，无法连接到对应容器终端",
            trimmed
        ))
    })?;
    Ok(runtime.env_store.clone())
}

async fn sandbox_terminal_command(
    app_state: &OmigaAppState,
    session_id: &str,
    backend: &str,
    shell: TerminalShell,
) -> CommandResult<ExternalTerminalCommand> {
    let env_store = session_env_store(app_state, session_id).await?;
    let ctx = ToolContext::new("/workspace")
        .with_execution_environment("sandbox")
        .with_sandbox_backend(backend)
        .with_env_store(Some(env_store.clone()));
    let env = env_store
        .get_or_create(&ctx, TERMINAL_ENV_TIMEOUT_MS)
        .await
        .map_err(|e| fs_io_error(format!("容器环境初始化失败 ({backend}): {}", e)))?;
    let guard = env.lock().await;
    guard
        .embedded_terminal_command(shell.unix_program()?)
        .ok_or_else(|| {
            fs_io_error(format!(
                "{} 暂不支持内嵌交互终端（后端尚未提供可管道连接的 shell）",
                backend
            ))
        })
}

struct TerminalStartResponseAndCommand {
    cwd: String,
    label: String,
    execution_environment: String,
    command: ExternalTerminalCommand,
    current_dir: Option<PathBuf>,
    wrap_in_host_pty: bool,
}

fn shell_command_line(command: &ExternalTerminalCommand) -> String {
    std::iter::once(command.program.as_str())
        .chain(command.args.iter().map(String::as_str))
        .map(shell_single_quote)
        .collect::<Vec<_>>()
        .join(" ")
}

fn wrap_in_host_pty(command: ExternalTerminalCommand) -> ExternalTerminalCommand {
    #[cfg(unix)]
    {
        if cfg!(target_os = "macos") {
            let mut args = vec!["-q".to_string(), "/dev/null".to_string(), command.program];
            args.extend(command.args);
            return ExternalTerminalCommand::new("script", args, command.display_name);
        }

        ExternalTerminalCommand::new(
            "script",
            vec![
                "-q".to_string(),
                "-c".to_string(),
                shell_command_line(&command),
                "/dev/null".to_string(),
            ],
            command.display_name,
        )
    }

    #[cfg(not(unix))]
    {
        command
    }
}

async fn resolve_terminal_command(
    app_state: &OmigaAppState,
    request: &TerminalStartRequest,
) -> CommandResult<TerminalStartResponseAndCommand> {
    let execution_environment = request
        .execution_environment
        .as_deref()
        .map(str::trim)
        .unwrap_or("local");
    let shell = TerminalShell::default_for_environment(execution_environment);

    match execution_environment {
        "ssh" => {
            let profile_name = request
                .ssh_profile_name
                .as_deref()
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .ok_or_else(|| fs_io_error("请先在执行环境中选择 SSH 服务器"))?;
            let remote_cwd = validate_remote_cwd(request.cwd.as_deref().unwrap_or(""))?;
            let configs = get_merged_ssh_configs()
                .map_err(|e| fs_io_error(format!("读取 SSH 配置失败: {e}")))?;
            let cfg = configs
                .get(profile_name)
                .ok_or_else(|| fs_io_error(format!("找不到 SSH 配置 `{profile_name}`")))?;
            Ok(TerminalStartResponseAndCommand {
                cwd: remote_cwd.clone(),
                label: format!("SSH · {profile_name} · {}", shell.label()),
                execution_environment: "ssh".to_string(),
                command: ssh_terminal_command(profile_name, cfg, &remote_cwd, shell)?,
                current_dir: None,
                wrap_in_host_pty: false,
            })
        }
        "sandbox" | "remote" => {
            let session_id = request
                .session_id
                .as_deref()
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .ok_or_else(|| fs_io_error("打开容器终端需要当前会话 ID"))?;
            let backend = request
                .sandbox_backend
                .as_deref()
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .unwrap_or("docker");
            let command = sandbox_terminal_command(app_state, session_id, backend, shell).await?;
            Ok(TerminalStartResponseAndCommand {
                cwd: request
                    .cwd
                    .as_deref()
                    .map(str::trim)
                    .filter(|v| !v.is_empty() && *v != ".")
                    .unwrap_or("/workspace")
                    .to_string(),
                label: format!("容器 · {backend} · {}", shell.label()),
                execution_environment: format!("sandbox:{backend}"),
                command,
                current_dir: None,
                wrap_in_host_pty: true,
            })
        }
        _ => {
            let cwd = canonical_local_cwd(request.cwd.as_deref().unwrap_or(""))?;
            local_shell_command(&cwd, shell)
        }
    }
}

fn emit_output(app: &AppHandle, terminal_id: &str, stream: &str, data: impl Into<String>) {
    let event = format!("terminal-output-{terminal_id}");
    let _ = app.emit(
        &event,
        TerminalOutputEvent {
            terminal_id: terminal_id.to_string(),
            stream: stream.to_string(),
            data: data.into(),
        },
    );
}

async fn spawn_reader<R>(app: AppHandle, terminal_id: String, stream: &'static str, mut reader: R)
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    let mut buf = [0_u8; 4096];
    loop {
        match reader.read(&mut buf).await {
            Ok(0) => break,
            Ok(n) => emit_output(
                &app,
                &terminal_id,
                stream,
                String::from_utf8_lossy(&buf[..n]),
            ),
            Err(e) => {
                emit_output(
                    &app,
                    &terminal_id,
                    "system",
                    format!("\n[terminal read error: {e}]\n"),
                );
                break;
            }
        }
    }
}

#[tauri::command]
pub async fn terminal_start(
    app: AppHandle,
    app_state: State<'_, OmigaAppState>,
    request: TerminalStartRequest,
) -> CommandResult<TerminalStartResponse> {
    let terminal_id = validate_terminal_id(&request.terminal_id)?;

    // Replace an existing session with the same id before starting a new process.
    terminal_stop(terminal_id.clone()).await?;

    let mut resolved = resolve_terminal_command(&app_state, &request).await?;
    if resolved.wrap_in_host_pty {
        resolved.command = wrap_in_host_pty(resolved.command);
    }
    let mut command = Command::new(&resolved.command.program);
    command
        .args(&resolved.command.args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(current_dir) = &resolved.current_dir {
        command.current_dir(current_dir);
    }

    let mut child = command.spawn().map_err(|e| {
        fs_io_error(format!(
            "无法启动内嵌终端 `{}`: {}",
            resolved.command.display_name, e
        ))
    })?;

    let pid = child.id();
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| fs_io_error("无法连接终端 stdin"))?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let stdin = Arc::new(Mutex::new(stdin));

    terminal_sessions().lock().await.insert(
        terminal_id.clone(),
        TerminalSession {
            stdin: stdin.clone(),
            pid,
        },
    );

    if let Some(stdout) = stdout {
        tokio::spawn(spawn_reader(
            app.clone(),
            terminal_id.clone(),
            "stdout",
            stdout,
        ));
    }
    if let Some(stderr) = stderr {
        tokio::spawn(spawn_reader(
            app.clone(),
            terminal_id.clone(),
            "stderr",
            stderr,
        ));
    }

    let wait_app = app.clone();
    let wait_id = terminal_id.clone();
    tokio::spawn(async move {
        let code = match child.wait().await {
            Ok(status) => status.code(),
            Err(_) => None,
        };
        terminal_sessions().lock().await.remove(&wait_id);
        let _ = wait_app.emit(
            format!("terminal-exit-{wait_id}").as_str(),
            TerminalExitEvent {
                terminal_id: wait_id,
                code,
            },
        );
    });

    emit_output(
        &app,
        &terminal_id,
        "system",
        format!(
            "Omiga embedded terminal connected: {} ({})\n",
            resolved.label, resolved.cwd
        ),
    );

    Ok(TerminalStartResponse {
        terminal_id,
        cwd: resolved.cwd,
        label: resolved.label,
        execution_environment: resolved.execution_environment,
    })
}

#[tauri::command]
pub async fn terminal_write(terminal_id: String, data: String) -> CommandResult<()> {
    let terminal_id = validate_terminal_id(&terminal_id)?;
    let stdin = {
        let sessions = terminal_sessions().lock().await;
        sessions
            .get(&terminal_id)
            .map(|s| s.stdin.clone())
            .ok_or_else(|| fs_io_error(format!("终端 `{terminal_id}` 不存在或已退出")))?
    };
    let mut guard = stdin.lock().await;
    guard
        .write_all(data.as_bytes())
        .await
        .map_err(|e| fs_io_error(format!("写入终端失败: {e}")))?;
    guard
        .flush()
        .await
        .map_err(|e| fs_io_error(format!("刷新终端失败: {e}")))?;
    Ok(())
}

#[tauri::command]
pub async fn terminal_stop(terminal_id: String) -> CommandResult<()> {
    let terminal_id = validate_terminal_id(&terminal_id)?;
    let session = terminal_sessions().lock().await.remove(&terminal_id);
    if let Some(session) = session {
        if let Ok(mut stdin) = session.stdin.try_lock() {
            let _ = stdin.write_all(b"exit\n").await;
            let _ = stdin.flush().await;
        }
        if let Some(pid) = session.pid {
            #[cfg(unix)]
            {
                let _ = nix::sys::signal::kill(
                    nix::unistd::Pid::from_raw(pid as i32),
                    nix::sys::signal::Signal::SIGTERM,
                );
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_id_validation_is_strict() {
        assert!(validate_terminal_id("term_123-abc").is_ok());
        assert!(validate_terminal_id("").is_err());
        assert!(validate_terminal_id("../bad").is_err());
    }

    #[test]
    fn remote_cwd_rejects_traversal() {
        assert!(validate_remote_cwd("/tmp/project").is_ok());
        assert!(validate_remote_cwd("~/project").is_ok());
        assert!(validate_remote_cwd("relative").is_err());
        assert!(validate_remote_cwd("/tmp/../secret").is_err());
        assert!(validate_remote_cwd("~/../secret").is_err());
    }

    #[test]
    fn remote_cd_quotes_paths() {
        assert_eq!(
            remote_cd_target("/tmp/it has spaces"),
            "'/tmp/it has spaces'"
        );
    }

    #[test]
    fn terminal_shell_defaults_by_environment() {
        let remote_default = TerminalShell::default_for_environment("ssh");
        assert_eq!(remote_default, TerminalShell::Bash);

        let local_default = TerminalShell::default_for_environment("local");
        if cfg!(target_os = "windows") {
            assert_eq!(local_default, TerminalShell::Powershell);
        } else {
            assert_eq!(local_default, TerminalShell::Zsh);
        }
    }
}
