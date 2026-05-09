//! Tauri commands for the guarded Computer Use facade.

use crate::commands::CommandResult;
use crate::domain::computer_use::{ComputerUseAuditSummary, ComputerUseStopStatus};
use crate::errors::AppError;
use serde::Serialize;
use std::path::PathBuf;

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
) -> CommandResult<ComputerUseAuditSummary> {
    let project_root = resolve_project_root(project_root)?;
    crate::domain::computer_use::summarize_audit(&project_root).map_err(|error| {
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
    session_id: String,
) -> CommandResult<ComputerUseStopStatus> {
    Ok(crate::domain::computer_use::stop_active_run(&session_id))
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
