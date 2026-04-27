//! Orchestration mode lifecycle helpers and implicit-memory indexing.
//!
//! This module owns the thin wrappers that call into `domain::orchestration::*`
//! at phase boundaries (begin / update / complete / fail) for Ralph, Autopilot,
//! and Team modes, plus the post-turn chat-to-memory indexer.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tauri::{AppHandle, Emitter};
use tokio::sync::RwLock;

use crate::domain::chat_state::SessionRuntimeState;
use crate::domain::persistence::NewOrchestrationEventRecord;

pub(super) struct ModeLifecycleContext<'a> {
    pub is_active: bool,
    pub sessions: &'a Arc<RwLock<HashMap<String, SessionRuntimeState>>>,
    pub repo: &'a crate::domain::persistence::SessionRepository,
    pub project_root: &'a Path,
    pub session_id: &'a str,
    pub env_label: Option<String>,
    pub round_id: Option<&'a str>,
}

async fn append_orchestration_mode_event(
    repo: &crate::domain::persistence::SessionRepository,
    session_id: &str,
    round_id: Option<&str>,
    mode: &str,
    event_type: &str,
    phase: Option<&str>,
    payload: serde_json::Value,
) {
    let payload_json = serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_string());
    if let Err(e) = repo
        .append_orchestration_event(NewOrchestrationEventRecord {
            session_id,
            round_id,
            message_id: None,
            mode: Some(mode),
            event_type,
            phase,
            task_id: None,
            payload_json: &payload_json,
        })
        .await
    {
        tracing::warn!(target: "omiga::orchestration_events", session_id, mode, event_type, error = %e, "append_orchestration_mode_event failed");
    }
}

// ── Ralph ────────────────────────────────────────────────────────────────────

pub(super) fn ralph_runtime_env_label(
    execution_environment: &str,
    ssh_server: Option<&str>,
    local_venv_type: &str,
    local_venv_name: &str,
) -> Option<String> {
    if !local_venv_name.trim().is_empty() {
        let kind = if local_venv_type.trim().is_empty() {
            "env"
        } else {
            local_venv_type.trim()
        };
        return Some(format!("{kind}:{}", local_venv_name.trim()));
    }
    match execution_environment {
        "ssh" => Some(format!("ssh:{}", ssh_server.unwrap_or("unknown"))),
        "" => None,
        other => Some(other.to_string()),
    }
}

pub(super) async fn begin_ralph_turn_if_needed(ctx: ModeLifecycleContext<'_>, goal: &str) {
    if !ctx.is_active {
        return;
    }
    if let Err(e) = crate::domain::orchestration::ralph::RalphOrchestrator::begin(
        ctx.sessions,
        ctx.project_root,
        ctx.session_id,
        goal,
        ctx.env_label,
    )
    .await
    {
        tracing::warn!(target: "omiga::ralph", session_id = ctx.session_id, error = %e, "Failed to begin Ralph turn");
    }
    append_orchestration_mode_event(
        ctx.repo,
        ctx.session_id,
        ctx.round_id,
        "ralph",
        "phase_changed",
        Some("planning"),
        serde_json::json!({ "goal": goal }),
    )
    .await;
}

pub(super) async fn update_ralph_phase_if_needed(
    ctx: ModeLifecycleContext<'_>,
    phase: crate::domain::ralph_state::RalphPhase,
) {
    if !ctx.is_active {
        return;
    }
    if let Err(e) = crate::domain::orchestration::ralph::RalphOrchestrator::set_phase(
        ctx.sessions,
        ctx.project_root,
        ctx.session_id,
        phase,
        ctx.env_label,
    )
    .await
    {
        tracing::warn!(target: "omiga::ralph", session_id = ctx.session_id, error = %e, "Failed to update Ralph phase");
    }
    let phase_s = phase.to_string();
    append_orchestration_mode_event(
        ctx.repo,
        ctx.session_id,
        ctx.round_id,
        "ralph",
        "phase_changed",
        Some(&phase_s),
        serde_json::json!({}),
    )
    .await;
}

pub(super) async fn complete_ralph_turn_if_needed(
    is_ralph_turn: bool,
    sessions: &Arc<RwLock<HashMap<String, SessionRuntimeState>>>,
    repo: &crate::domain::persistence::SessionRepository,
    project_root: &Path,
    session_id: &str,
    round_id: Option<&str>,
) {
    if !is_ralph_turn {
        return;
    }
    if let Err(e) = crate::domain::orchestration::ralph::RalphOrchestrator::complete(
        sessions,
        project_root,
        session_id,
    )
    .await
    {
        tracing::warn!(target: "omiga::ralph", session_id, error = %e, "Failed to complete Ralph turn");
    }
    append_orchestration_mode_event(
        repo,
        session_id,
        round_id,
        "ralph",
        "mode_completed",
        Some("complete"),
        serde_json::json!({}),
    )
    .await;
}

pub(super) async fn fail_ralph_turn_if_needed(
    ctx: ModeLifecycleContext<'_>,
    phase: crate::domain::ralph_state::RalphPhase,
    error: &str,
) {
    if !ctx.is_active {
        return;
    }
    if let Err(e) = crate::domain::orchestration::ralph::RalphOrchestrator::fail(
        ctx.sessions,
        ctx.project_root,
        ctx.session_id,
        phase,
        error,
    )
    .await
    {
        tracing::warn!(target: "omiga::ralph", session_id = ctx.session_id, error = %e, "Failed to record Ralph failure");
    }
    let phase_s = phase.to_string();
    append_orchestration_mode_event(
        ctx.repo,
        ctx.session_id,
        ctx.round_id,
        "ralph",
        "mode_failed",
        Some(&phase_s),
        serde_json::json!({ "error": error }),
    )
    .await;
}

// ── Autopilot ────────────────────────────────────────────────────────────────

pub(super) async fn begin_autopilot_turn_if_needed(ctx: ModeLifecycleContext<'_>, goal: &str) {
    if !ctx.is_active {
        return;
    }
    if let Err(e) = crate::domain::orchestration::autopilot::AutopilotOrchestrator::begin(
        ctx.sessions,
        ctx.project_root,
        ctx.session_id,
        goal,
        ctx.env_label,
    )
    .await
    {
        tracing::warn!(target: "omiga::autopilot", session_id = ctx.session_id, error = %e, "Failed to begin Autopilot turn");
    }
    append_orchestration_mode_event(
        ctx.repo,
        ctx.session_id,
        ctx.round_id,
        "autopilot",
        "phase_changed",
        Some("intake"),
        serde_json::json!({ "goal": goal }),
    )
    .await;
}

pub(super) async fn update_autopilot_phase_if_needed(
    ctx: ModeLifecycleContext<'_>,
    phase: crate::domain::autopilot_state::AutopilotPhase,
) -> Option<crate::domain::autopilot_state::AutopilotState> {
    if !ctx.is_active {
        return None;
    }
    let result = match crate::domain::orchestration::autopilot::AutopilotOrchestrator::set_phase(
        ctx.sessions,
        ctx.project_root,
        ctx.session_id,
        phase,
        ctx.env_label,
    )
    .await
    {
        Ok(state) => state,
        Err(e) => {
            tracing::warn!(target: "omiga::autopilot", session_id = ctx.session_id, error = %e, "Failed to update Autopilot phase");
            None
        }
    };
    let phase_s = phase.to_string();
    if result.is_some() {
        append_orchestration_mode_event(
            ctx.repo,
            ctx.session_id,
            ctx.round_id,
            "autopilot",
            "phase_changed",
            Some(&phase_s),
            serde_json::json!({}),
        )
        .await;
    }
    result
}

pub(super) async fn complete_autopilot_turn_if_needed(
    is_autopilot_turn: bool,
    sessions: &Arc<RwLock<HashMap<String, SessionRuntimeState>>>,
    repo: &crate::domain::persistence::SessionRepository,
    project_root: &Path,
    session_id: &str,
    round_id: Option<&str>,
) {
    if !is_autopilot_turn {
        return;
    }
    if let Err(e) = crate::domain::orchestration::autopilot::AutopilotOrchestrator::complete(
        sessions,
        project_root,
        session_id,
    )
    .await
    {
        tracing::warn!(target: "omiga::autopilot", session_id, error = %e, "Failed to complete Autopilot turn");
    }
    append_orchestration_mode_event(
        repo,
        session_id,
        round_id,
        "autopilot",
        "mode_completed",
        Some("complete"),
        serde_json::json!({}),
    )
    .await;
}

pub(super) async fn fail_autopilot_turn_if_needed(
    ctx: ModeLifecycleContext<'_>,
    phase: crate::domain::autopilot_state::AutopilotPhase,
    error: &str,
) {
    if !ctx.is_active {
        return;
    }
    if let Err(e) = crate::domain::orchestration::autopilot::AutopilotOrchestrator::fail(
        ctx.sessions,
        ctx.project_root,
        ctx.session_id,
        phase,
        error,
    )
    .await
    {
        tracing::warn!(target: "omiga::autopilot", session_id = ctx.session_id, error = %e, "Failed to record Autopilot failure");
    }
    let phase_s = phase.to_string();
    append_orchestration_mode_event(
        ctx.repo,
        ctx.session_id,
        ctx.round_id,
        "autopilot",
        "mode_failed",
        Some(&phase_s),
        serde_json::json!({ "error": error }),
    )
    .await;
}

// ── Team ─────────────────────────────────────────────────────────────────────

pub(super) async fn begin_team_turn_if_needed(
    is_team_turn: bool,
    repo: &crate::domain::persistence::SessionRepository,
    project_root: &Path,
    session_id: &str,
    goal: &str,
    round_id: Option<&str>,
) {
    if !is_team_turn {
        return;
    }
    if let Err(e) =
        crate::domain::orchestration::team::TeamOrchestrator::begin(project_root, session_id, goal)
            .await
    {
        tracing::warn!(target: "omiga::team", session_id, error = %e, "Failed to begin Team turn");
    }
    append_orchestration_mode_event(
        repo,
        session_id,
        round_id,
        "team",
        "phase_changed",
        Some("planning"),
        serde_json::json!({ "goal": goal }),
    )
    .await;
}

pub(super) async fn complete_team_turn_if_needed(
    is_team_turn: bool,
    repo: &crate::domain::persistence::SessionRepository,
    project_root: &Path,
    session_id: &str,
    round_id: Option<&str>,
) {
    if !is_team_turn {
        return;
    }
    if let Err(e) =
        crate::domain::orchestration::team::TeamOrchestrator::complete(project_root, session_id)
            .await
    {
        tracing::warn!(target: "omiga::team", session_id, error = %e, "Failed to complete Team turn");
    }
    append_orchestration_mode_event(
        repo,
        session_id,
        round_id,
        "team",
        "mode_completed",
        Some("complete"),
        serde_json::json!({}),
    )
    .await;
}

pub(super) async fn fail_team_turn_if_needed(
    is_team_turn: bool,
    repo: &crate::domain::persistence::SessionRepository,
    project_root: &Path,
    session_id: &str,
    error: &str,
    round_id: Option<&str>,
) {
    if !is_team_turn {
        return;
    }
    if let Err(e) =
        crate::domain::orchestration::team::TeamOrchestrator::fail(project_root, session_id, error)
            .await
    {
        tracing::warn!(target: "omiga::team", session_id, error = %e, "Failed to record Team failure");
    }
    append_orchestration_mode_event(
        repo,
        session_id,
        round_id,
        "team",
        "mode_failed",
        Some("failed"),
        serde_json::json!({ "error": error }),
    )
    .await;
}

// ── Implicit memory indexing ──────────────────────────────────────────────────

/// Index a completed chat session into PageIndex implicit memory.
/// Emits `chat-index-start`, `chat-index-complete`, or `chat-index-error` events.
pub(super) async fn index_chat_to_implicit_memory(
    app: &AppHandle,
    project_path: &str,
    session_id: &str,
    session_name: &str,
    repo: &crate::domain::persistence::SessionRepository,
) {
    let _ = app.emit(
        "chat-index-start",
        serde_json::json!({ "session_id": session_id }),
    );

    let session_with_messages = match repo.get_session(session_id).await {
        Ok(Some(s)) => s,
        _ => {
            tracing::debug!("Session {} not found for indexing", session_id);
            let _ = app.emit(
                "chat-index-error",
                serde_json::json!({ "session_id": session_id, "error": "Session not found" }),
            );
            return;
        }
    };

    let messages: Vec<crate::domain::memory::ChatMessage> = session_with_messages
        .messages
        .into_iter()
        .map(|msg| crate::domain::memory::ChatMessage {
            id: msg.id,
            session_id: msg.session_id,
            role: match msg.role.as_str() {
                "assistant" => crate::domain::memory::ChatRole::Assistant,
                "tool" => crate::domain::memory::ChatRole::Tool,
                _ => crate::domain::memory::ChatRole::User,
            },
            content: msg.content,
            timestamp: chrono::DateTime::parse_from_rfc3339(&msg.created_at)
                .map(|dt| dt.timestamp())
                .unwrap_or_else(|_| chrono::Utc::now().timestamp()),
            tool_calls: msg.tool_calls.and_then(|tc| serde_json::from_str(&tc).ok()),
        })
        .collect();

    if messages.is_empty() {
        let _ = app.emit(
            "chat-index-complete",
            serde_json::json!({ "session_id": session_id, "document_count": 0 }),
        );
        return;
    }

    let project_root = super::resolve_session_project_root(project_path);
    let memory_dir = match crate::domain::memory::load_resolved_config(&project_root).await {
        Ok(cfg) => cfg.implicit_path(&project_root),
        Err(_) => project_root.join(".omiga/memory/implicit"),
    };

    let mut indexer = crate::domain::memory::ChatIndexer::new(&memory_dir);
    if let Err(e) = indexer.init().await {
        tracing::warn!("Failed to init chat indexer: {}", e);
        let _ = app.emit(
            "chat-index-error",
            serde_json::json!({ "session_id": session_id, "error": format!("Failed to init indexer: {e}") }),
        );
        return;
    }
    if let Err(e) = indexer.load().await {
        tracing::warn!("Failed to load chat indexer: {}", e);
        let _ = app.emit(
            "chat-index-error",
            serde_json::json!({ "session_id": session_id, "error": format!("Failed to load indexer: {e}") }),
        );
        return;
    }

    match indexer
        .index_session(session_id, session_name, &messages)
        .await
    {
        Ok(_) => {
            tracing::info!("Indexed chat session {} into implicit memory", session_id);
            crate::domain::memory::touch_project_registry(&project_root).await;
            let _ = app.emit(
                "chat-index-complete",
                serde_json::json!({ "session_id": session_id, "document_count": indexer.document_count() }),
            );
        }
        Err(e) => {
            tracing::warn!("Failed to index chat session: {}", e);
            let _ = app.emit(
                "chat-index-error",
                serde_json::json!({ "session_id": session_id, "error": format!("Failed to index: {e}") }),
            );
        }
    }
}
