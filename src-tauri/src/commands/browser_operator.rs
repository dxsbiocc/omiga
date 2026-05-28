//! Tauri commands for Browser Operator backend management.

use crate::app_state::OmigaAppState;
use serde_json::{Map, Value};
use tauri::State;

const BROWSER_OPERATOR_INSTALL_TOOL_NAME: &str = "browser_operator_install_backend";
const BROWSER_OPERATOR_INSTALL_CONFIRMATION_REASON: &str =
    "missing explicit install confirmation; Browser Operator backend installer was not executed";
const BROWSER_OPERATOR_INSTALL_STARTED_REASON: &str = "Browser Operator backend install started";
const BROWSER_OPERATOR_INSTALL_COMPLETED_REASON: &str =
    "Browser Operator backend install completed";
const BROWSER_OPERATOR_INSTALL_FAILURE_REASON_PREFIX: &str =
    "Browser Operator backend install failed: ";
const BROWSER_OPERATOR_INSTALL_AUDIT_REASON_MAX_CHARS: usize = 120;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BrowserOperatorInstallAuditPhase {
    NotStarted,
    Started,
    Completed,
    Failed,
}

impl BrowserOperatorInstallAuditPhase {
    fn as_str(self) -> &'static str {
        match self {
            Self::NotStarted => "not_started",
            Self::Started => "started",
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }
}

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

fn truncate_audit_reason_fragment(value: &str, max_chars: usize) -> String {
    let collapsed = value.split_whitespace().collect::<Vec<_>>().join(" ");
    let char_count = collapsed.chars().count();
    if char_count <= max_chars {
        collapsed
    } else {
        let truncated = collapsed
            .chars()
            .take(max_chars.saturating_sub(1))
            .collect::<String>();
        format!("{truncated}…")
    }
}

fn browser_operator_install_audit_status(
    decision: &str,
    phase: BrowserOperatorInstallAuditPhase,
) -> &'static str {
    if decision != "approved" {
        "denied"
    } else {
        phase.as_str()
    }
}

fn browser_operator_install_failure_reason(
    err: &crate::domain::browser_operator::BrowserOperatorError,
) -> String {
    let message = truncate_audit_reason_fragment(
        &err.message,
        BROWSER_OPERATOR_INSTALL_AUDIT_REASON_MAX_CHARS,
    );
    let message_lower = message.to_ascii_lowercase();
    let unsafe_message = message.is_empty()
        || message.contains('{')
        || message.contains('[')
        || message_lower.contains("stdout")
        || message_lower.contains("stderr")
        || message_lower.contains("traceback");
    let safe_detail = if unsafe_message {
        err.code.clone()
    } else {
        format!("{} ({message})", err.code)
    };

    format!("{BROWSER_OPERATOR_INSTALL_FAILURE_REASON_PREFIX}{safe_detail}")
}

fn browser_operator_install_audit_arguments(
    skip_browser_install: bool,
    project_root: Option<&str>,
    session_id: Option<&str>,
    phase: BrowserOperatorInstallAuditPhase,
    status: &str,
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
    arguments.insert(
        "phase".to_string(),
        Value::String(phase.as_str().to_string()),
    );
    arguments.insert("status".to_string(), Value::String(status.to_string()));
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
    phase: BrowserOperatorInstallAuditPhase,
) {
    let normalized_session_id = normalized_optional_str(session_id);
    let normalized_project_root = normalized_optional_str(project_root);
    let arguments_json = browser_operator_install_audit_arguments(
        skip_browser_install,
        normalized_project_root.as_deref(),
        normalized_session_id.as_deref(),
        phase,
        browser_operator_install_audit_status(decision, phase),
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
            BrowserOperatorInstallAuditPhase::NotStarted,
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
        Some(BROWSER_OPERATOR_INSTALL_STARTED_REASON),
        skip_browser_install,
        BrowserOperatorInstallAuditPhase::Started,
    )
    .await;

    match crate::domain::browser_operator::install_managed_backend(skip_browser_install).await {
        Ok(result) => {
            append_browser_operator_install_audit_event(
                &state,
                session_id.as_deref(),
                project_root.as_deref(),
                "approved",
                Some(BROWSER_OPERATOR_INSTALL_COMPLETED_REASON),
                skip_browser_install,
                BrowserOperatorInstallAuditPhase::Completed,
            )
            .await;
            Ok(result)
        }
        Err(err) => {
            let failure_reason = browser_operator_install_failure_reason(&err);
            // The permission decision remains approved: the user allowed the installer,
            // but the approved installer process failed. The failed execution outcome is
            // carried by the phase/status fields and reason instead of polluting denial
            // counts in the permission audit UI.
            append_browser_operator_install_audit_event(
                &state,
                session_id.as_deref(),
                project_root.as_deref(),
                "approved",
                Some(failure_reason.as_str()),
                skip_browser_install,
                BrowserOperatorInstallAuditPhase::Failed,
            )
            .await;
            Err(err.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        browser_operator_install_audit_arguments, browser_operator_install_audit_status,
        browser_operator_install_failure_reason, browser_operator_install_kind,
        has_explicit_install_confirmation, BrowserOperatorInstallAuditPhase,
    };
    use crate::domain::browser_operator::BrowserOperatorError;
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
    fn browser_operator_install_audit_status_tracks_install_phase_after_approval() {
        assert_eq!(
            browser_operator_install_audit_status(
                "approved",
                BrowserOperatorInstallAuditPhase::Started
            ),
            "started"
        );
        assert_eq!(
            browser_operator_install_audit_status(
                "approved",
                BrowserOperatorInstallAuditPhase::Failed
            ),
            "failed"
        );
        assert_eq!(
            browser_operator_install_audit_status(
                "denied",
                BrowserOperatorInstallAuditPhase::NotStarted
            ),
            "denied"
        );
    }

    #[test]
    fn browser_operator_install_audit_arguments_include_safe_context() {
        let arguments = browser_operator_install_audit_arguments(
            true,
            Some("/tmp/workspace"),
            Some("session-123"),
            BrowserOperatorInstallAuditPhase::Completed,
            browser_operator_install_audit_status(
                "approved",
                BrowserOperatorInstallAuditPhase::Completed,
            ),
        );
        let parsed: Value = serde_json::from_str(&arguments).expect("valid json");

        assert_eq!(
            parsed,
            json!({
                "skipBrowserInstall": true,
                "installKind": "packages-only",
                "phase": "completed",
                "status": "completed",
                "projectRoot": "/tmp/workspace",
                "sessionId": "session-123",
            })
        );
    }

    #[test]
    fn browser_operator_install_audit_arguments_omit_blank_optional_fields() {
        let arguments = browser_operator_install_audit_arguments(
            false,
            Some("  "),
            None,
            BrowserOperatorInstallAuditPhase::NotStarted,
            browser_operator_install_audit_status(
                "denied",
                BrowserOperatorInstallAuditPhase::NotStarted,
            ),
        );
        let parsed: Value = serde_json::from_str(&arguments).expect("valid json");

        assert_eq!(
            parsed,
            json!({
                "skipBrowserInstall": false,
                "installKind": "full",
                "phase": "not_started",
                "status": "denied",
            })
        );
    }

    #[test]
    fn browser_operator_install_failure_reason_keeps_short_safe_message() {
        let err = BrowserOperatorError {
            code: "install_timeout".to_string(),
            message: "Browser Operator backend install timed out after 60s.".to_string(),
            details: None,
        };

        assert_eq!(
            browser_operator_install_failure_reason(&err),
            "Browser Operator backend install failed: install_timeout (Browser Operator backend install timed out after 60s.)"
        );
    }

    #[test]
    fn browser_operator_install_failure_reason_omits_output_heavy_message() {
        let err = BrowserOperatorError {
            code: "sidecar_io_error".to_string(),
            message: "Browser Operator backend install failed: {\"stdout\":\"very long output\",\"stderr\":\"Traceback: ...\"}".to_string(),
            details: None,
        };

        assert_eq!(
            browser_operator_install_failure_reason(&err),
            "Browser Operator backend install failed: sidecar_io_error"
        );
    }
}
