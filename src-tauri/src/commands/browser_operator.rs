//! Tauri commands for Browser Operator backend management.

use crate::app_state::OmigaAppState;
use crate::domain::browser_operator::BrowserOperatorError;
use serde_json::{Map, Value};
use std::future::Future;
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
    browser_operator_install_backend_impl(
        &state,
        confirm_install_intent,
        skip_browser_install,
        project_root,
        session_id,
        crate::domain::browser_operator::install_managed_backend,
    )
    .await
}

async fn browser_operator_install_backend_impl<F, Fut>(
    state: &OmigaAppState,
    confirm_install_intent: Option<bool>,
    skip_browser_install: Option<bool>,
    project_root: Option<String>,
    session_id: Option<String>,
    installer: F,
) -> Result<Value, String>
where
    F: FnOnce(bool) -> Fut,
    Fut: Future<Output = Result<Value, BrowserOperatorError>>,
{
    let skip_browser_install = skip_browser_install.unwrap_or(false);

    if !has_explicit_install_confirmation(confirm_install_intent) {
        append_browser_operator_install_audit_event(
            state,
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
        state,
        session_id.as_deref(),
        project_root.as_deref(),
        "approved",
        Some(BROWSER_OPERATOR_INSTALL_STARTED_REASON),
        skip_browser_install,
        BrowserOperatorInstallAuditPhase::Started,
    )
    .await;

    match installer(skip_browser_install).await {
        Ok(result) => {
            append_browser_operator_install_audit_event(
                state,
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
                state,
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
        append_browser_operator_install_audit_event, browser_operator_install_audit_arguments,
        browser_operator_install_audit_status, browser_operator_install_backend_impl,
        browser_operator_install_failure_reason, browser_operator_install_kind,
        has_explicit_install_confirmation, BrowserOperatorInstallAuditPhase,
        BROWSER_OPERATOR_INSTALL_COMPLETED_REASON, BROWSER_OPERATOR_INSTALL_CONFIRMATION_REASON,
        BROWSER_OPERATOR_INSTALL_STARTED_REASON, BROWSER_OPERATOR_INSTALL_TOOL_NAME,
    };
    use crate::app_state::OmigaAppState;
    use crate::domain::browser_operator::BrowserOperatorError;
    use crate::domain::persistence::{init_db, SessionRepository};
    use serde_json::{json, Value};
    use std::collections::HashMap;

    async fn list_browser_operator_install_events_by_phase(
        state: &OmigaAppState,
        project_root: &str,
        expected_count: i64,
    ) -> HashMap<String, (String, Option<String>, Value)> {
        let page = state
            .repo
            .list_recent_permission_audit_events_page(
                10,
                0,
                Some(project_root),
                None,
                Some("browser_operator"),
            )
            .await
            .expect("list browser operator audit events");

        assert_eq!(page.total_count, expected_count);
        page.events
            .into_iter()
            .map(|event| {
                assert_eq!(event.tool_name, BROWSER_OPERATOR_INSTALL_TOOL_NAME);
                let arguments: Value =
                    serde_json::from_str(&event.arguments_json).expect("valid audit arguments");
                let phase = arguments
                    .get("phase")
                    .and_then(Value::as_str)
                    .expect("phase")
                    .to_string();
                (phase, (event.decision, event.reason, arguments))
            })
            .collect()
    }

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

    #[tokio::test]
    async fn browser_operator_install_audit_events_persist_phase_status_and_facets_after_reopen() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("omiga-test.db");
        let pool = init_db(&db_path).await.expect("init db");
        let state = OmigaAppState::new(SessionRepository::new(pool));
        let project_root = dir.path().join("project");
        let project_root = project_root.to_string_lossy().to_string();
        let failure = BrowserOperatorError {
            code: "install_timeout".to_string(),
            message: "Browser Operator backend install timed out after 60s.".to_string(),
            details: None,
        };
        let failure_reason = browser_operator_install_failure_reason(&failure);

        append_browser_operator_install_audit_event(
            &state,
            Some("session-browser"),
            Some(project_root.as_str()),
            "denied",
            Some(BROWSER_OPERATOR_INSTALL_CONFIRMATION_REASON),
            false,
            BrowserOperatorInstallAuditPhase::NotStarted,
        )
        .await;
        append_browser_operator_install_audit_event(
            &state,
            Some("session-browser"),
            Some(project_root.as_str()),
            "approved",
            Some(BROWSER_OPERATOR_INSTALL_STARTED_REASON),
            true,
            BrowserOperatorInstallAuditPhase::Started,
        )
        .await;
        append_browser_operator_install_audit_event(
            &state,
            Some("session-browser"),
            Some(project_root.as_str()),
            "approved",
            Some(BROWSER_OPERATOR_INSTALL_COMPLETED_REASON),
            true,
            BrowserOperatorInstallAuditPhase::Completed,
        )
        .await;
        append_browser_operator_install_audit_event(
            &state,
            Some("session-browser"),
            Some(project_root.as_str()),
            "approved",
            Some(failure_reason.as_str()),
            true,
            BrowserOperatorInstallAuditPhase::Failed,
        )
        .await;

        state.repo.pool().close().await;
        drop(state);

        let reopened_pool = init_db(&db_path).await.expect("reopen db");
        let reopened_repo = SessionRepository::new(reopened_pool);
        let page = reopened_repo
            .list_recent_permission_audit_events_page(
                10,
                0,
                Some(project_root.as_str()),
                None,
                Some("browser_operator"),
            )
            .await
            .expect("list browser operator audit events");

        assert_eq!(page.total_count, 4);
        assert_eq!(page.facets.approved_count, 3);
        assert_eq!(page.facets.denied_count, 1);

        let by_phase = page
            .events
            .iter()
            .map(|event| {
                assert_eq!(event.session_id, "session-browser");
                assert_eq!(event.project_root.as_deref(), Some(project_root.as_str()));
                assert_eq!(event.tool_name, BROWSER_OPERATOR_INSTALL_TOOL_NAME);
                let arguments: Value =
                    serde_json::from_str(&event.arguments_json).expect("valid audit arguments");
                let phase = arguments
                    .get("phase")
                    .and_then(Value::as_str)
                    .expect("phase")
                    .to_string();
                (phase, (event, arguments))
            })
            .collect::<HashMap<_, _>>();

        let not_started = by_phase.get("not_started").expect("not_started event");
        assert_eq!(not_started.0.decision, "denied");
        assert_eq!(not_started.0.tool_name, BROWSER_OPERATOR_INSTALL_TOOL_NAME);
        assert_eq!(
            not_started.0.reason.as_deref(),
            Some(BROWSER_OPERATOR_INSTALL_CONFIRMATION_REASON)
        );
        assert_eq!(not_started.1["status"], "denied");
        assert_eq!(not_started.1["installKind"], "full");

        let started = by_phase.get("started").expect("started event");
        assert_eq!(started.0.decision, "approved");
        assert_eq!(
            started.0.reason.as_deref(),
            Some(BROWSER_OPERATOR_INSTALL_STARTED_REASON)
        );
        assert_eq!(started.1["status"], "started");
        assert_eq!(started.1["installKind"], "packages-only");

        let completed = by_phase.get("completed").expect("completed event");
        assert_eq!(completed.0.decision, "approved");
        assert_eq!(
            completed.0.reason.as_deref(),
            Some(BROWSER_OPERATOR_INSTALL_COMPLETED_REASON)
        );
        assert_eq!(completed.1["status"], "completed");

        let failed = by_phase.get("failed").expect("failed event");
        assert_eq!(failed.0.decision, "approved");
        assert_eq!(failed.0.reason.as_deref(), Some(failure_reason.as_str()));
        assert_eq!(failed.1["status"], "failed");
    }

    #[tokio::test]
    async fn browser_operator_install_backend_impl_records_success_audit_events() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("omiga-test.db");
        let pool = init_db(&db_path).await.expect("init db");
        let state = OmigaAppState::new(SessionRepository::new(pool));
        let project_root = dir.path().join("project").to_string_lossy().to_string();

        let result = browser_operator_install_backend_impl(
            &state,
            Some(true),
            Some(true),
            Some(project_root.clone()),
            Some("session-success".to_string()),
            |skip_browser_install| async move {
                Ok(json!({
                    "ok": true,
                    "skipBrowserInstall": skip_browser_install,
                    "home": "/tmp/omiga-browser-operator",
                }))
            },
        )
        .await
        .expect("install succeeds");

        assert_eq!(result["ok"], true);
        assert_eq!(result["skipBrowserInstall"], true);

        let by_phase =
            list_browser_operator_install_events_by_phase(&state, &project_root, 2).await;

        let started = by_phase.get("started").expect("started event");
        assert_eq!(started.0, "approved");
        assert_eq!(
            started.1.as_deref(),
            Some(BROWSER_OPERATOR_INSTALL_STARTED_REASON)
        );
        assert_eq!(started.2["status"], "started");
        assert_eq!(started.2["installKind"], "packages-only");
        assert_eq!(started.2["skipBrowserInstall"], true);
        assert_eq!(started.2["projectRoot"], project_root);
        assert_eq!(started.2["sessionId"], "session-success");

        let completed = by_phase.get("completed").expect("completed event");
        assert_eq!(completed.0, "approved");
        assert_eq!(
            completed.1.as_deref(),
            Some(BROWSER_OPERATOR_INSTALL_COMPLETED_REASON)
        );
        assert_eq!(completed.2["status"], "completed");
        assert_eq!(completed.2["installKind"], "packages-only");
        assert_eq!(completed.2["skipBrowserInstall"], true);
    }

    #[tokio::test]
    async fn browser_operator_install_backend_impl_records_failed_execution_without_denial() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("omiga-test.db");
        let pool = init_db(&db_path).await.expect("init db");
        let state = OmigaAppState::new(SessionRepository::new(pool));
        let project_root = dir.path().join("project").to_string_lossy().to_string();
        let failure = BrowserOperatorError {
            code: "install_timeout".to_string(),
            message: "Browser Operator backend install timed out after 60s.".to_string(),
            details: None,
        };
        let failure_reason = browser_operator_install_failure_reason(&failure);

        let error = browser_operator_install_backend_impl(
            &state,
            Some(true),
            Some(false),
            Some(project_root.clone()),
            Some("session-failure".to_string()),
            move |_| {
                let failure = failure.clone();
                async move { Err(failure) }
            },
        )
        .await
        .expect_err("install fails");

        assert!(error.contains("install_timeout"));

        let by_phase =
            list_browser_operator_install_events_by_phase(&state, &project_root, 2).await;

        let started = by_phase.get("started").expect("started event");
        assert_eq!(started.0, "approved");
        assert_eq!(started.2["status"], "started");
        assert_eq!(started.2["installKind"], "full");

        let failed = by_phase.get("failed").expect("failed event");
        assert_eq!(failed.0, "approved");
        assert_eq!(failed.1.as_deref(), Some(failure_reason.as_str()));
        assert_eq!(failed.2["status"], "failed");
        assert_eq!(failed.2["installKind"], "full");
        assert_eq!(failed.2["skipBrowserInstall"], false);
    }

    #[tokio::test]
    async fn browser_operator_install_backend_impl_denies_without_calling_installer() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("omiga-test.db");
        let pool = init_db(&db_path).await.expect("init db");
        let state = OmigaAppState::new(SessionRepository::new(pool));
        let project_root = dir.path().join("project").to_string_lossy().to_string();

        let error = browser_operator_install_backend_impl(
            &state,
            None,
            Some(false),
            Some(project_root.clone()),
            Some("session-denied".to_string()),
            |_| async {
                panic!("installer must not run without explicit confirmation");
                #[allow(unreachable_code)]
                Ok::<Value, BrowserOperatorError>(json!({ "ok": true }))
            },
        )
        .await
        .expect_err("install is denied");

        assert!(error.contains("confirmInstallIntent=true"));

        let by_phase =
            list_browser_operator_install_events_by_phase(&state, &project_root, 1).await;
        let denied = by_phase.get("not_started").expect("denied event");
        assert_eq!(denied.0, "denied");
        assert_eq!(
            denied.1.as_deref(),
            Some(BROWSER_OPERATOR_INSTALL_CONFIRMATION_REASON)
        );
        assert_eq!(denied.2["status"], "denied");
        assert_eq!(denied.2["installKind"], "full");
    }
}
