//! Tauri commands for the guarded Computer Use facade.

use crate::commands::CommandResult;
use crate::domain::computer_use::{ComputerUseAuditSummary, ComputerUseStopStatus};
use crate::errors::AppError;
use serde::Serialize;
use serde_json::json;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;
use tauri::Manager;

fn resolve_project_root(project_root: String) -> CommandResult<PathBuf> {
    let trimmed = project_root.trim();
    if trimmed.is_empty() || trimmed == "." {
        return Err(AppError::Config(
            "Computer Use requires an active project path.".to_string(),
        ));
    }
    let path = PathBuf::from(trimmed);
    Ok(path.canonicalize().unwrap_or(path))
}

#[tauri::command]
pub async fn computer_use_audit_summary(
    project_root: String,
    retention_days: Option<u32>,
) -> CommandResult<ComputerUseAuditSummary> {
    let project_root = resolve_project_root(project_root)?;
    match retention_days {
        Some(retention_days) => crate::domain::computer_use::summarize_audit_with_retention(
            &project_root,
            retention_days,
        ),
        None => crate::domain::computer_use::summarize_audit(&project_root),
    }
    .map_err(|error| {
        AppError::Persistence(format!(
            "Failed to summarize Computer Use audit log: {error}"
        ))
    })
}

#[tauri::command]
pub async fn computer_use_clear_audit(
    project_root: String,
) -> CommandResult<ComputerUseAuditSummary> {
    let project_root = resolve_project_root(project_root)?;
    crate::domain::computer_use::clear_audit_runs(&project_root).map_err(|error| {
        AppError::Persistence(format!("Failed to clear Computer Use audit log: {error}"))
    })
}

#[tauri::command]
pub async fn computer_use_stop_active_run(
    app: tauri::AppHandle,
    session_id: String,
    project_root: Option<String>,
) -> CommandResult<ComputerUseStopStatus> {
    let mut status = crate::domain::computer_use::stop_active_run(&session_id);
    let Some(run_id) = status.run_id.clone() else {
        return Ok(status);
    };
    let Some(project_root) = project_root
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty() && *value != ".")
        .map(PathBuf::from)
    else {
        status.backend_message =
            Some("Backend stop skipped because no active project path was available.".to_string());
        return Ok(status);
    };

    let project_root = project_root.canonicalize().unwrap_or(project_root);
    let mcp_manager = app
        .try_state::<crate::app_state::OmigaAppState>()
        .map(|state| state.chat.mcp_manager.clone());
    let args = json!({
        "runId": run_id,
        "reason": "user_stop",
    })
    .to_string();
    match crate::domain::mcp::tool_dispatch::execute_mcp_tool_call(
        &project_root,
        &crate::domain::computer_use::ComputerFacadeTool::Stop.backend_mcp_name(),
        &args,
        Duration::from_secs(10),
        mcp_manager,
        None,
        Some(session_id),
    )
    .await
    {
        Ok((backend_output, backend_is_error)) => {
            status.backend_stopped = Some(!backend_is_error);
            status.backend_message = Some(backend_output);
        }
        Err(error) => {
            status.backend_stopped = Some(false);
            status.backend_error = Some(error);
        }
    }
    Ok(status)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComputerUsePermissionStatus {
    pub platform: String,
    pub supported: bool,
    pub accessibility: String,
    pub screen_recording: String,
    pub message: String,
}

#[tauri::command]
pub async fn computer_use_permission_status() -> CommandResult<ComputerUsePermissionStatus> {
    Ok(read_permission_status())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComputerUseBackendStatus {
    pub platform: String,
    pub runtime: String,
    pub wrapper_path: String,
    pub wrapper_installed: bool,
    pub wrapper_executable: bool,
    pub python_backend_path: String,
    pub python_backend_installed: bool,
    pub python_backend_executable: bool,
    pub message: String,
}

#[tauri::command]
pub async fn computer_use_backend_status() -> CommandResult<ComputerUseBackendStatus> {
    Ok(read_backend_status())
}

fn read_backend_status() -> ComputerUseBackendStatus {
    let platform = std::env::consts::OS.to_string();
    let plugin_dir =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("bundled_plugins/plugins/computer-use");
    let bin_dir = plugin_dir.join("bin");
    let wrapper_path = bin_dir.join("computer-use");
    let python_backend_path = bin_dir.join("computer-use-macos.py");

    let wrapper_installed = wrapper_path.is_file();
    let wrapper_executable = is_executable_file(&wrapper_path);
    let python_backend_installed = python_backend_path.is_file();
    let python_backend_executable = is_executable_file(&python_backend_path);

    let message = "Python Computer Use backend is active.".to_string();

    ComputerUseBackendStatus {
        platform,
        runtime: "python".to_string(),
        wrapper_path: path_display(&wrapper_path),
        wrapper_installed,
        wrapper_executable,
        python_backend_path: path_display(&python_backend_path),
        python_backend_installed,
        python_backend_executable,
        message,
    }
}

fn path_display(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

#[cfg(unix)]
fn is_executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    path.metadata()
        .map(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable_file(path: &Path) -> bool {
    path.metadata()
        .map(|metadata| metadata.is_file())
        .unwrap_or(false)
}

#[cfg(target_os = "macos")]
fn read_permission_status() -> ComputerUsePermissionStatus {
    use std::process::Command;

    let accessibility_output = Command::new("osascript")
        .arg("-e")
        .arg(r#"tell application "System Events" to count application processes"#)
        .output();
    let accessibility = match accessibility_output {
        Ok(output) if output.status.success() => "granted",
        Ok(_) => "blocked",
        Err(_) => "unknown",
    }
    .to_string();

    let screenshot_path = std::env::temp_dir().join(format!(
        "omiga-computer-use-permission-{}.png",
        uuid::Uuid::new_v4()
    ));
    let screen_output = Command::new("screencapture")
        .arg("-x")
        .arg("-t")
        .arg("png")
        .arg(&screenshot_path)
        .output();
    let screen_recording = match screen_output {
        Ok(output) if output.status.success() && screenshot_path.exists() => "granted",
        Ok(_) => "blocked",
        Err(_) => "unknown",
    }
    .to_string();
    let _ = std::fs::remove_file(&screenshot_path);

    let message = if accessibility == "granted" && screen_recording == "granted" {
        "Computer Use macOS permissions look available.".to_string()
    } else {
        "Grant Accessibility and Screen Recording to Omiga/Terminal in macOS Privacy & Security."
            .to_string()
    };

    ComputerUsePermissionStatus {
        platform: "macos".to_string(),
        supported: true,
        accessibility,
        screen_recording,
        message,
    }
}

#[cfg(not(target_os = "macos"))]
fn read_permission_status() -> ComputerUsePermissionStatus {
    ComputerUsePermissionStatus {
        platform: std::env::consts::OS.to_string(),
        supported: false,
        accessibility: "unsupported".to_string(),
        screen_recording: "unsupported".to_string(),
        message: "Computer Use Phase 10 backend supports macOS only.".to_string(),
    }
}
