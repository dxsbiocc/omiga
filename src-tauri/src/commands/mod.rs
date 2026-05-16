//! Tauri command handlers
//!
//! These are the entry points from the frontend. Each command:
//! - Deserializes arguments from frontend
//! - Delegates to domain layer
//! - Returns structured errors for frontend handling

pub mod app_skin;
pub mod blackboard;
pub mod bridge;
pub mod chat;
pub mod citation;
pub mod claude_import;
pub mod computer_use;
pub mod connectors;
pub mod context_snapshot;
pub mod cron;
pub mod execution_envs;
pub mod execution_records;
pub mod extensions;
pub mod external_integrations;
pub mod fs;
pub mod git_workspace;
pub mod integrations_settings;
pub mod learning_proposals;
pub mod local_envs;
pub mod memory;
pub mod notebook;
pub mod operators;
pub mod permissions;
pub mod plugins;
pub mod ralph;
pub mod sandbox_fs;
pub mod search;
pub mod self_evolution_drafts;
pub mod session;
pub mod shell;
pub mod ssh_fs;
pub mod terminal;
pub mod tools;

use crate::errors::AppError;

/// Standard command result type
pub type CommandResult<T> = Result<T, AppError>;

/// Send notification via osascript (macOS fallback for dev mode)
fn send_notification_via_osascript(title: &str, body: &str) -> Result<(), String> {
    use std::process::Command;

    let script = format!(
        r#"display notification "{}" with title "{}""#,
        body.replace('"', "\\\""),
        title.replace('"', "\\\"")
    );

    let output = Command::new("osascript")
        .arg("-e")
        .arg(&script)
        .output()
        .map_err(|e| format!("Failed to execute osascript: {}", e))?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!("osascript failed: {}", stderr))
    }
}

/// Convert permission state to string representation
fn permission_state_to_string(state: &tauri_plugin_notification::PermissionState) -> &'static str {
    match state {
        tauri_plugin_notification::PermissionState::Granted => "granted",
        tauri_plugin_notification::PermissionState::Denied => "denied",
        tauri_plugin_notification::PermissionState::Prompt => "prompt",
        _ => "unknown",
    }
}

/// Test notification command for debugging
#[tauri::command]
pub async fn test_notification(app: tauri::AppHandle) -> Result<String, String> {
    use tauri_plugin_notification::NotificationExt;

    tracing::debug!("Sending test notification");

    let permission_state = app.notification().permission_state();
    let has_permission = match &permission_state {
        Ok(state) => {
            tracing::debug!(?state, "Permission state");
            matches!(state, tauri_plugin_notification::PermissionState::Granted)
        }
        Err(e) => {
            tracing::warn!(?e, "Failed to check permission");
            false
        }
    };

    if has_permission
        && app
            .notification()
            .builder()
            .title("测试通知 (Backend)")
            .body("这是一条来自后端的测试通知")
            .show()
            .is_ok()
    {
        return Ok("Native notification sent".to_string());
    }

    #[cfg(target_os = "macos")]
    {
        match send_notification_via_osascript(
            "测试通知 (osascript)",
            "这是一条测试通知（开发模式备选）",
        ) {
            Ok(_) => return Ok("osascript notification sent (dev mode fallback)".to_string()),
            Err(e) => tracing::warn!(?e, "osascript fallback failed"),
        }
    }

    Err("All notification methods failed".to_string())
}

/// Return the path to today's audit log file (creates parent dirs on demand).
#[tauri::command]
pub fn get_audit_log_path() -> String {
    crate::domain::audit::audit_log_path()
        .to_string_lossy()
        .to_string()
}

/// Send notification with fallback (for production use)
#[tauri::command]
pub async fn send_notification(
    app: tauri::AppHandle,
    title: String,
    body: String,
) -> Result<String, String> {
    use tauri_plugin_notification::NotificationExt;

    let permission_state = app.notification().permission_state();
    let has_permission = matches!(
        permission_state,
        Ok(tauri_plugin_notification::PermissionState::Granted)
    );

    if has_permission
        && app
            .notification()
            .builder()
            .title(&title)
            .body(&body)
            .show()
            .is_ok()
    {
        return Ok("native".to_string());
    }

    #[cfg(target_os = "macos")]
    {
        if send_notification_via_osascript(&title, &body).is_ok() {
            return Ok("osascript".to_string());
        }
    }

    Err("Failed to send notification".to_string())
}

/// Get notification permission status
#[tauri::command]
pub fn get_notification_permission_status(app: tauri::AppHandle) -> &'static str {
    use tauri_plugin_notification::NotificationExt;

    match app.notification().permission_state() {
        Ok(state) => permission_state_to_string(&state),
        Err(_) => "error",
    }
}

/// Request notification permission
#[tauri::command]
pub fn request_notification_permission(app: tauri::AppHandle) -> &'static str {
    use tauri_plugin_notification::NotificationExt;

    match app.notification().request_permission() {
        Ok(state) => permission_state_to_string(&state),
        Err(_) => "error",
    }
}

/// Setup status returned to the frontend on first launch.
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupStatus {
    /// Whether omiga.yaml (or equivalent) was found.
    pub config_file_found: bool,
    /// Resolved path to the config file, if found.
    pub config_file_path: Option<String>,
    /// Whether at least one provider is enabled and has a non-empty API key.
    pub has_enabled_provider: bool,
    /// Human-readable guidance shown in the setup dialog.
    pub setup_hint: String,
}

/// Check whether the app is configured and ready to use.
///
/// Called by the frontend on startup to decide whether to show the
/// first-launch setup guide.
#[tauri::command]
pub fn get_setup_status() -> SetupStatus {
    let config_file = crate::llm::config::find_config_file();
    let config_file_found = config_file.is_some();
    let config_file_path = config_file.map(|p| p.to_string_lossy().into_owned());

    let has_enabled_provider = crate::llm::config::load_config_file()
        .ok()
        .and_then(|cf| {
            let providers = cf.providers?;
            let placeholder = |s: &str| {
                s.is_empty()
                    || s == "${DEEPSEEK_API_KEY}"
                    || s == "${OPENAI_API_KEY}"
                    || s == "${GOOGLE_API_KEY}"
                    || s == "${LLM_API_KEY}"
            };
            let any = providers.values().any(|p| {
                p.enabled
                    && p.api_key
                        .as_deref()
                        .map(|k| !placeholder(k))
                        .unwrap_or(false)
            });
            Some(any)
        })
        .unwrap_or(false);

    let setup_hint = if !config_file_found {
        "No configuration file found. Copy config.example.yaml to omiga.yaml in your \
         project root, then set your API key:\n\
         • DeepSeek:  export DEEPSEEK_API_KEY=\"sk-...\"\n\
         • OpenAI:    export OPENAI_API_KEY=\"sk-...\"\n\
         • Google:    export GOOGLE_API_KEY=\"AIza...\"\n\
         Restart the app after saving."
            .to_string()
    } else if !has_enabled_provider {
        "Configuration file found but no provider has a valid API key. \
         Set the environment variable for your chosen provider and restart:\n\
         • DeepSeek:  export DEEPSEEK_API_KEY=\"sk-...\"\n\
         • OpenAI:    export OPENAI_API_KEY=\"sk-...\"\n\
         • Google:    export GOOGLE_API_KEY=\"AIza...\"\n\
         Or enter your key directly in Settings → Providers."
            .to_string()
    } else {
        String::new()
    };

    SetupStatus {
        config_file_found,
        config_file_path,
        has_enabled_provider,
        setup_hint,
    }
}
