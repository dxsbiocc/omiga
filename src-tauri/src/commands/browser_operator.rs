//! Tauri commands for Browser Operator backend management.

use crate::app_state::OmigaAppState;
use serde_json::{Map, Value};
use tauri::State;

const BROWSER_OPERATOR_INSTALL_TOOL_NAME: &str = "browser_operator_install_backend";
const BROWSER_OPERATOR_INSTALL_CONFIRMATION_REASON: &str =
    "missing explicit install confirmation; Browser Operator backend installer was not executed";

#[tauri::command]
pub async fn browser_operator_backend_status() -> Result<Value, String> {
    Ok(crate::domain::browser_operator::backend_status())
}

fn browser_operator_install_kind(skip_browser_install: bool) -> &'static str {
    if skip_browser_install {
        "packages-only"
    } else {
        "full"
    }
}

fn has_explicit_install_confirmation(confirm_install_intent: Option<bool>) -> bool {
    confirm_install_intent.unwrap_or(false)
}

fn normalized_optional_str(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn browser_operator_install_audit_arguments(
    skip_browser_install: bool,
    project_root: Option<&str>,
    session_id: Option<&str>,
) -> String {
    let mut arguments = Map::new();
    arguments.insert(
        "skipBrowserInstall".to_string(),
        Value::Bool(skip_browser_install),
    );
    arguments.insert(
        "installKind".to_string(),
        Value::String(browser_operator_install_kind(skip_browser_install).to_string()),
    );
    if let Some(project_root) = normalized_optional_str(project_root) {
        arguments.insert("projectRoot".to_string(), Value::String(project_root));
    }
    if let Some(session_id) = normalized_optional_str(session_id) {
        arguments.insert("sessionId".to_string(), Value::String(session_id));
    }
    Value::Object(arguments).to_string()
}

async fn append_browser_operator_install_audit_event(
    app_state: &OmigaAppState,
    session_id: Option<&str>,
    project_root: Option<&str>,
    decision: &str,
    reason: Option<&str>,
    skip_browser_install: bool,
) {
    let normalized_session_id = normalized_optional_str(session_id);
    let normalized_project_root = normalized_optional_str(project_root);
    let arguments_json = browser_operator_install_audit_arguments(
        skip_browser_install,
        normalized_project_root.as_deref(),
        normalized_session_id.as_deref(),
    );

    crate::commands::permissions::append_permission_audit_event(
        app_state,
        normalized_session_id
            .as_deref()
            .unwrap_or(BROWSER_OPERATOR_INSTALL_TOOL_NAME),
        None,
        normalized_project_root.as_deref(),
        decision,
        BROWSER_OPERATOR_INSTALL_TOOL_NAME,
        None,
        reason,
        &arguments_json,
    )
    .await;
}

#[tauri::command]
pub async fn browser_operator_install_backend(
    state: State<'_, OmigaAppState>,
    confirm_install_intent: Option<bool>,
    skip_browser_install: Option<bool>,
    project_root: Option<String>,
    session_id: Option<String>,
) -> Result<Value, String> {
    let skip_browser_install = skip_browser_install.unwrap_or(false);

    if !has_explicit_install_confirmation(confirm_install_intent) {
        append_browser_operator_install_audit_event(
            &state,
            session_id.as_deref(),
            project_root.as_deref(),
            "denied",
            Some(BROWSER_OPERATOR_INSTALL_CONFIRMATION_REASON),
            skip_browser_install,
        )
        .await;
        return Err(
            "Browser Operator 安装需要显式确认；缺少 confirmInstallIntent=true，已拒绝执行安装。"
                .to_string(),
        );
    }

    append_browser_operator_install_audit_event(
        &state,
        session_id.as_deref(),
        project_root.as_deref(),
        "approved",
        None,
        skip_browser_install,
    )
    .await;

    crate::domain::browser_operator::install_managed_backend(skip_browser_install)
        .await
        .map_err(|err| err.to_string())
}

#[cfg(test)]
mod tests {
    use super::{
        browser_operator_install_audit_arguments, browser_operator_install_kind,
        has_explicit_install_confirmation,
    };
    use serde_json::{json, Value};

    #[test]
    fn browser_operator_install_kind_matches_skip_flag() {
        assert_eq!(browser_operator_install_kind(true), "packages-only");
        assert_eq!(browser_operator_install_kind(false), "full");
    }

    #[test]
    fn browser_operator_install_requires_explicit_confirmation() {
        assert!(!has_explicit_install_confirmation(None));
        assert!(!has_explicit_install_confirmation(Some(false)));
        assert!(has_explicit_install_confirmation(Some(true)));
    }

    #[test]
    fn browser_operator_install_audit_arguments_include_safe_context() {
        let arguments = browser_operator_install_audit_arguments(
            true,
            Some("/tmp/workspace"),
            Some("session-123"),
        );
        let parsed: Value = serde_json::from_str(&arguments).expect("valid json");

        assert_eq!(
            parsed,
            json!({
                "skipBrowserInstall": true,
                "installKind": "packages-only",
                "projectRoot": "/tmp/workspace",
                "sessionId": "session-123",
            })
        );
    }

    #[test]
    fn browser_operator_install_audit_arguments_omit_blank_optional_fields() {
        let arguments = browser_operator_install_audit_arguments(false, Some("  "), None);
        let parsed: Value = serde_json::from_str(&arguments).expect("valid json");

        assert_eq!(
            parsed,
            json!({
                "skipBrowserInstall": false,
                "installKind": "full",
            })
        );
    }
}
