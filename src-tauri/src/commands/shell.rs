//! UI-triggered shell helpers (e.g. R Markdown `rmarkdown::render`, Quarto CLI).
//!
//! `.qmd` rendering prefers the `quarto` CLI when it is on `PATH` and reports a version ≥
//! [`min_quarto_cli_version`] (default **1.3.0**, override with env **`OMIGA_MIN_QUARTO_VERSION`**
//! e.g. `1.4.0`). Otherwise falls back to `Rscript -e 'quarto::quarto_render(...)'` (R package **quarto**).

use super::CommandResult;
use crate::app_state::OmigaAppState;
use crate::commands::execution_envs::get_merged_ssh_configs;
use crate::domain::tools::{env_store::EnvStore, ToolContext};
use crate::errors::{AppError, FsError};
use crate::execution::ExternalTerminalCommand;
use crate::llm::config::SshExecConfig;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::{Command as StdCommand, Stdio};
use std::sync::OnceLock;
use tokio::process::Command;
use tokio::time::{timeout, Duration};

/// Knit / render can be slow for large docs.
const RENDER_TIMEOUT: Duration = Duration::from_secs(600);
const VERSION_PROBE_TIMEOUT: Duration = Duration::from_secs(8);
const MAX_OUTPUT_CHARS: usize = 2 * 1024 * 1024;

/// Default minimum Quarto CLI `(major, minor, patch)` — below this we use R `quarto::quarto_render`.
fn min_quarto_cli_version() -> (u32, u32, u32) {
    std::env::var("OMIGA_MIN_QUARTO_VERSION")
        .ok()
        .and_then(|s| parse_version_triple(&s))
        .unwrap_or((1, 3, 0))
}

fn parse_version_triple(s: &str) -> Option<(u32, u32, u32)> {
    let mut parts = s.trim().split('.');
    let a = parts.next()?.parse().ok()?;
    let b = parts.next()?.parse().ok()?;
    let c = parts.next()?.parse().ok()?;
    Some((a, b, c))
}

/// First `major.minor.patch` in text (from `quarto --version` output).
fn parse_quarto_version_line(text: &str) -> Option<(u32, u32, u32)> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"(\d+)\.(\d+)\.(\d+)").expect("regex"));
    let cap = re.captures(text)?;
    Some((
        cap[1].parse().ok()?,
        cap[2].parse().ok()?,
        cap[3].parse().ok()?,
    ))
}

fn version_lt(a: (u32, u32, u32), b: (u32, u32, u32)) -> bool {
    (a.0, a.1, a.2) < (b.0, b.1, b.2)
}

#[derive(Debug, Serialize)]
pub struct RmdRenderResponse {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

#[derive(Debug, Serialize)]
pub struct OpenSystemTerminalResponse {
    pub cwd: String,
    pub terminal: String,
    pub execution_environment: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenSystemTerminalRequest {
    pub cwd: Option<String>,
    pub execution_environment: Option<String>,
    pub ssh_profile_name: Option<String>,
    pub sandbox_backend: Option<String>,
    pub session_id: Option<String>,
}

fn canonical_file_with_ext(path: &str, expected_ext: &str) -> Result<PathBuf, AppError> {
    let path_buf = PathBuf::from(path);
    let canonical = path_buf.canonicalize().map_err(|e| {
        AppError::Fs(FsError::IoError {
            message: format!("{}: {}", path, e),
        })
    })?;

    if !canonical.is_file() {
        return Err(AppError::Fs(FsError::InvalidPath {
            path: path.to_string(),
        }));
    }

    let ext = canonical
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    if ext != expected_ext {
        return Err(AppError::Fs(FsError::InvalidPath {
            path: format!("expected .{expected_ext} file: {path}"),
        }));
    }

    Ok(canonical)
}

/// Escape a path for use inside an R double-quoted string literal.
fn r_double_quoted_path(path: &str) -> String {
    let escaped = path.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{}\"", escaped)
}

fn truncate(s: &str) -> String {
    if s.chars().count() <= MAX_OUTPUT_CHARS {
        return s.to_string();
    }
    let mut out = s.chars().take(MAX_OUTPUT_CHARS).collect::<String>();
    out.push_str("\n… [输出已截断]");
    out
}

/// Run `rmarkdown::render()` on an `.Rmd` file via `Rscript` (R must be on `PATH`).
#[tauri::command]
pub async fn render_rmarkdown(path: String) -> CommandResult<RmdRenderResponse> {
    let canonical = canonical_file_with_ext(&path, "rmd")?;
    let path_str = canonical.to_string_lossy().to_string();
    let r_expr = format!("rmarkdown::render({})", r_double_quoted_path(&path_str));

    let run = Command::new("Rscript")
        .arg("-e")
        .arg(&r_expr)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output();

    let output = timeout(RENDER_TIMEOUT, run)
        .await
        .map_err(|_| {
            AppError::Fs(FsError::IoError {
                message: "R Markdown 渲染超时（超过 10 分钟）".to_string(),
            })
        })?
        .map_err(|e| {
            AppError::Fs(FsError::IoError {
                message: format!("无法运行 Rscript（请确认已安装 R，且 `rmarkdown` 包可用）: {e}"),
            })
        })?;

    let exit_code = output.status.code().unwrap_or(-1);
    let stdout = truncate(&String::from_utf8_lossy(&output.stdout));
    let stderr = truncate(&String::from_utf8_lossy(&output.stderr));

    Ok(RmdRenderResponse {
        stdout,
        stderr,
        exit_code,
    })
}

/// Run `quarto render` on a `.qmd` file. Uses the `quarto` CLI when available and new enough;
/// otherwise falls back to `quarto::quarto_render()` via `Rscript`.
#[tauri::command]
pub async fn render_quarto(path: String) -> CommandResult<RmdRenderResponse> {
    let canonical = canonical_file_with_ext(&path, "qmd")?;

    let probe = Command::new("quarto")
        .arg("--version")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output();

    match timeout(VERSION_PROBE_TIMEOUT, probe).await {
        Ok(Ok(out)) if out.status.success() => {
            let text = format!(
                "{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
            if let Some(ver) = parse_quarto_version_line(&text) {
                let min = min_quarto_cli_version();
                if version_lt(ver, min) {
                    return render_quarto_via_r(&canonical).await;
                }
            }
        }
        Ok(Err(e)) if e.kind() == ErrorKind::NotFound => {
            return render_quarto_via_r(&canonical).await;
        }
        _ => {
            // Version probe failed or timed out — still try `quarto render` below.
        }
    }

    let run = Command::new("quarto")
        .arg("render")
        .arg(&canonical)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output();

    let output = match timeout(RENDER_TIMEOUT, run).await {
        Err(_) => {
            return Err(AppError::Fs(FsError::IoError {
                message: "Quarto 渲染超时（超过 10 分钟）".to_string(),
            }));
        }
        Ok(Ok(out)) => out,
        Ok(Err(e)) if e.kind() == ErrorKind::NotFound => {
            return render_quarto_via_r(&canonical).await;
        }
        Ok(Err(e)) => {
            return Err(AppError::Fs(FsError::IoError {
                message: format!("无法启动 quarto（请确认已安装 Quarto CLI 且在 PATH 中）: {e}"),
            }));
        }
    };

    let exit_code = output.status.code().unwrap_or(-1);
    let stdout = truncate(&String::from_utf8_lossy(&output.stdout));
    let stderr = truncate(&String::from_utf8_lossy(&output.stderr));

    Ok(RmdRenderResponse {
        stdout,
        stderr,
        exit_code,
    })
}

/// `quarto::quarto_render()` — requires R and the **quarto** R package (`install.packages("quarto")`).
async fn render_quarto_via_r(canonical: &Path) -> CommandResult<RmdRenderResponse> {
    let path_str = canonical.to_string_lossy().to_string();
    let r_expr = format!("quarto::quarto_render({})", r_double_quoted_path(&path_str));

    let run = Command::new("Rscript")
        .arg("-e")
        .arg(&r_expr)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output();

    let output = timeout(RENDER_TIMEOUT, run)
        .await
        .map_err(|_| {
            AppError::Fs(FsError::IoError {
                message: "Quarto 渲染超时（超过 10 分钟）".to_string(),
            })
        })?
        .map_err(|e| {
            AppError::Fs(FsError::IoError {
                message: format!(
                    "未找到 quarto 命令，且无法运行 Rscript（请安装 R，并安装 R 包 quarto：`install.packages(\"quarto\")`）: {e}"
                ),
            })
        })?;

    let exit_code = output.status.code().unwrap_or(-1);
    let stdout = truncate(&String::from_utf8_lossy(&output.stdout));
    let stderr = truncate(&String::from_utf8_lossy(&output.stderr));

    Ok(RmdRenderResponse {
        stdout,
        stderr,
        exit_code,
    })
}

fn fs_io_error(message: impl Into<String>) -> AppError {
    AppError::Fs(FsError::IoError {
        message: message.into(),
    })
}

fn canonical_terminal_cwd(cwd: &str) -> CommandResult<PathBuf> {
    let trimmed = cwd.trim();
    if trimmed.is_empty() || trimmed == "." {
        return Err(fs_io_error("请先选择本地工作区，再打开系统终端"));
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct TerminalLaunchCommand {
    program: String,
    args: Vec<String>,
    display_name: String,
}

impl TerminalLaunchCommand {
    fn new(program: &str, args: Vec<String>, display_name: &str) -> Self {
        Self {
            program: program.to_string(),
            args,
            display_name: display_name.to_string(),
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows", test))]
fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(target_os = "macos")]
fn applescript_string(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

#[cfg(target_os = "macos")]
fn macos_terminal_script(command_line: &str) -> String {
    format!(
        "tell application \"Terminal\"\n  activate\n  do script {}\nend tell",
        applescript_string(command_line)
    )
}

#[cfg(target_os = "windows")]
fn windows_cmd_quote(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\\\""))
}

fn shell_command_line(command: &ExternalTerminalCommand) -> String {
    let mut parts = Vec::with_capacity(command.args.len() + 1);
    parts.push(shell_single_quote(&command.program));
    parts.extend(command.args.iter().map(|arg| shell_single_quote(arg)));
    parts.join(" ")
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn local_terminal_command_line(cwd: &Path) -> String {
    let cwd = cwd.to_string_lossy();
    format!(
        "cd {}; exec \"${{SHELL:-/bin/sh}}\"",
        shell_single_quote(&cwd)
    )
}

#[cfg(target_os = "windows")]
fn local_terminal_command_line(cwd: &Path) -> String {
    let cwd = cwd.to_string_lossy();
    format!("cd /d {} && cmd", windows_cmd_quote(&cwd))
}

fn terminal_launch_commands(command_line: &str, display_name: &str) -> Vec<TerminalLaunchCommand> {
    #[cfg(target_os = "macos")]
    {
        return vec![TerminalLaunchCommand::new(
            "osascript",
            vec!["-e".to_string(), macos_terminal_script(command_line)],
            display_name,
        )];
    }

    #[cfg(target_os = "windows")]
    {
        return vec![
            TerminalLaunchCommand::new(
                "wt.exe",
                vec![
                    "cmd".to_string(),
                    "/K".to_string(),
                    command_line.to_string(),
                ],
                display_name,
            ),
            TerminalLaunchCommand::new(
                "cmd.exe",
                vec![
                    "/C".to_string(),
                    "start".to_string(),
                    "".to_string(),
                    "cmd.exe".to_string(),
                    "/K".to_string(),
                    command_line.to_string(),
                ],
                display_name,
            ),
        ];
    }

    #[cfg(target_os = "linux")]
    {
        return vec![
            TerminalLaunchCommand::new(
                "xdg-terminal-exec",
                vec![
                    "sh".to_string(),
                    "-lc".to_string(),
                    command_line.to_string(),
                ],
                display_name,
            ),
            TerminalLaunchCommand::new(
                "x-terminal-emulator",
                vec![
                    "-e".to_string(),
                    "sh".to_string(),
                    "-lc".to_string(),
                    command_line.to_string(),
                ],
                display_name,
            ),
            TerminalLaunchCommand::new(
                "gnome-terminal",
                vec![
                    "--".to_string(),
                    "sh".to_string(),
                    "-lc".to_string(),
                    command_line.to_string(),
                ],
                display_name,
            ),
            TerminalLaunchCommand::new(
                "konsole",
                vec![
                    "-e".to_string(),
                    "sh".to_string(),
                    "-lc".to_string(),
                    command_line.to_string(),
                ],
                display_name,
            ),
            TerminalLaunchCommand::new(
                "xfce4-terminal",
                vec![
                    "--command".to_string(),
                    format!("sh -lc {}", shell_single_quote(command_line)),
                ],
                display_name,
            ),
            TerminalLaunchCommand::new(
                "kitty",
                vec![
                    "sh".to_string(),
                    "-lc".to_string(),
                    command_line.to_string(),
                ],
                display_name,
            ),
            TerminalLaunchCommand::new(
                "alacritty",
                vec![
                    "-e".to_string(),
                    "sh".to_string(),
                    "-lc".to_string(),
                    command_line.to_string(),
                ],
                display_name,
            ),
            TerminalLaunchCommand::new(
                "wezterm",
                vec![
                    "start".to_string(),
                    "--".to_string(),
                    "sh".to_string(),
                    "-lc".to_string(),
                    command_line.to_string(),
                ],
                display_name,
            ),
            TerminalLaunchCommand::new(
                "xterm",
                vec![
                    "-e".to_string(),
                    "sh".to_string(),
                    "-lc".to_string(),
                    command_line.to_string(),
                ],
                display_name,
            ),
        ];
    }

    #[allow(unreachable_code)]
    let _ = (command_line, display_name);
    Vec::new()
}

fn launch_system_terminal(command_line: &str, display_name: &str) -> CommandResult<String> {
    let commands = terminal_launch_commands(command_line, display_name);
    if commands.is_empty() {
        return Err(fs_io_error("当前平台暂不支持自动打开系统终端"));
    }

    let mut not_found: Vec<String> = Vec::new();
    for command in commands {
        let mut child = StdCommand::new(&command.program);
        child
            .args(&command.args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());

        #[cfg(target_os = "macos")]
        {
            match child.status() {
                Ok(status) if status.success() => return Ok(command.display_name),
                Ok(status) => {
                    return Err(fs_io_error(format!(
                        "{} 启动失败，退出码 {}",
                        command.display_name,
                        status.code().unwrap_or(-1)
                    )));
                }
                Err(e) if e.kind() == ErrorKind::NotFound => {
                    not_found.push(command.program);
                    continue;
                }
                Err(e) => {
                    return Err(fs_io_error(format!(
                        "无法启动 {}: {}",
                        command.display_name, e
                    )));
                }
            }
        }

        #[cfg(not(target_os = "macos"))]
        {
            match child.spawn() {
                Ok(_) => return Ok(command.display_name),
                Err(e) if e.kind() == ErrorKind::NotFound => {
                    not_found.push(command.program);
                    continue;
                }
                Err(e) => {
                    return Err(fs_io_error(format!(
                        "无法启动 {}: {}",
                        command.display_name, e
                    )));
                }
            }
        }
    }

    Err(fs_io_error(format!(
        "未找到可用系统终端（已尝试：{}）",
        not_found.join(", ")
    )))
}

fn validate_remote_terminal_cwd(cwd: &str) -> CommandResult<String> {
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
            std::path::Component::RootDir => out.push("/"),
            std::path::Component::Normal(part) => {
                if part == ".." {
                    return Err(AppError::Fs(FsError::PathTraversal {
                        path: cwd.to_string(),
                    }));
                }
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

fn ssh_terminal_command(
    profile_name: &str,
    cfg: &SshExecConfig,
    remote_cwd: &str,
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
        "-t".to_string(),
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

    let cd_target = remote_cd_target(remote_cwd);
    let script = format!("cd {cd_target} || exit 126; exec \"${{SHELL:-/bin/bash}}\"");
    args.push(format!("bash -lc {}", shell_single_quote(&script)));

    Ok(ExternalTerminalCommand::new(
        "ssh",
        args,
        format!("SSH · {profile_name}"),
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
    sandbox_backend: &str,
) -> CommandResult<ExternalTerminalCommand> {
    let backend = sandbox_backend.trim();
    if backend.is_empty() {
        return Err(fs_io_error("请先选择容器/沙箱后端"));
    }

    let env_store = session_env_store(app_state, session_id).await?;
    let ctx = ToolContext::new("/workspace")
        .with_execution_environment("sandbox")
        .with_sandbox_backend(backend)
        .with_env_store(Some(env_store.clone()));

    let env = env_store
        .get_or_create(&ctx, 60_000)
        .await
        .map_err(|e| fs_io_error(format!("容器环境初始化失败 ({backend}): {}", e)))?;
    let guard = env.lock().await;
    guard
        .external_terminal_command()
        .ok_or_else(|| fs_io_error(format!("{} 暂不支持交互式系统终端连接", backend)))
}

fn open_local_terminal(cwd: &str) -> CommandResult<OpenSystemTerminalResponse> {
    let canonical = canonical_terminal_cwd(cwd)?;
    let command_line = local_terminal_command_line(&canonical);
    let terminal = launch_system_terminal(&command_line, "系统终端")?;
    Ok(OpenSystemTerminalResponse {
        cwd: canonical.to_string_lossy().to_string(),
        terminal,
        execution_environment: "local".to_string(),
    })
}

fn open_ssh_terminal(
    request: &OpenSystemTerminalRequest,
) -> CommandResult<OpenSystemTerminalResponse> {
    let profile_name = request
        .ssh_profile_name
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| fs_io_error("请先在执行环境中选择 SSH 服务器"))?;
    let remote_cwd = validate_remote_terminal_cwd(request.cwd.as_deref().unwrap_or(""))?;
    let configs =
        get_merged_ssh_configs().map_err(|e| fs_io_error(format!("读取 SSH 配置失败: {e}")))?;
    let cfg = configs
        .get(profile_name)
        .ok_or_else(|| fs_io_error(format!("找不到 SSH 配置 `{profile_name}`")))?;
    let command = ssh_terminal_command(profile_name, cfg, &remote_cwd)?;
    let terminal = launch_system_terminal(&shell_command_line(&command), &command.display_name)?;

    Ok(OpenSystemTerminalResponse {
        cwd: remote_cwd,
        terminal,
        execution_environment: "ssh".to_string(),
    })
}

async fn open_sandbox_terminal(
    app_state: &OmigaAppState,
    request: &OpenSystemTerminalRequest,
) -> CommandResult<OpenSystemTerminalResponse> {
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
    let command = sandbox_terminal_command(app_state, session_id, backend).await?;
    let terminal = launch_system_terminal(&shell_command_line(&command), &command.display_name)?;

    Ok(OpenSystemTerminalResponse {
        cwd: request
            .cwd
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty() && *v != ".")
            .unwrap_or("/workspace")
            .to_string(),
        terminal,
        execution_environment: format!("sandbox:{backend}"),
    })
}

/// Open the selected execution surface in the operating system's terminal app.
#[tauri::command]
pub async fn open_system_terminal(
    app_state: tauri::State<'_, OmigaAppState>,
    request: OpenSystemTerminalRequest,
) -> CommandResult<OpenSystemTerminalResponse> {
    match request
        .execution_environment
        .as_deref()
        .map(str::trim)
        .unwrap_or("local")
    {
        "ssh" => open_ssh_terminal(&request),
        "sandbox" | "remote" => open_sandbox_terminal(&app_state, &request).await,
        _ => open_local_terminal(request.cwd.as_deref().unwrap_or("")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn parse_quarto_version_line_finds_semver() {
        assert_eq!(
            parse_quarto_version_line("quarto 1.5.23\n"),
            Some((1, 5, 23))
        );
        assert_eq!(parse_quarto_version_line("1.2.475"), Some((1, 2, 475)));
    }

    #[test]
    fn version_lt_orders() {
        assert!(version_lt((1, 2, 9), (1, 3, 0)));
        assert!(!version_lt((1, 3, 0), (1, 3, 0)));
    }

    #[test]
    fn parse_version_triple_env_style() {
        assert_eq!(parse_version_triple("1.4.0"), Some((1, 4, 0)));
        assert_eq!(parse_version_triple("1.4"), None);
    }

    #[test]
    fn shell_single_quote_handles_embedded_quotes() {
        assert_eq!(shell_single_quote("/tmp/it's here"), "'/tmp/it'\\''s here'");
    }

    #[test]
    fn canonical_terminal_cwd_rejects_unset_paths() {
        assert!(canonical_terminal_cwd("").is_err());
        assert!(canonical_terminal_cwd(".").is_err());
    }

    #[test]
    fn canonical_terminal_cwd_accepts_existing_directory() {
        let dir = tempdir().expect("tempdir");
        let canonical = canonical_terminal_cwd(&dir.path().to_string_lossy()).expect("cwd");
        assert!(canonical.is_dir());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_terminal_script_runs_shell_command() {
        let command = format!("cd {}", shell_single_quote("/tmp/Project \"A\""));
        let script = macos_terminal_script(&command);
        assert!(script.contains("do script"));
        assert!(script.contains("cd '/tmp/Project \\\"A\\\"'"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_terminal_attempts_include_standard_fallbacks() {
        let commands = terminal_launch_commands("echo hello", "system terminal");
        let programs: Vec<_> = commands.iter().map(|cmd| cmd.program.as_str()).collect();
        assert!(programs.contains(&"xdg-terminal-exec"));
        assert!(programs.contains(&"x-terminal-emulator"));
        assert!(programs.contains(&"xterm"));
    }
}
