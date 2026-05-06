//! Session management commands

use super::CommandResult;
use crate::app_state::OmigaAppState;
use crate::domain::persistence::MessageRecord;
use crate::domain::persistence::RuntimeConstraintEventRecord;
use crate::domain::persistence::RuntimeConstraintRoundTraceRecord;
use crate::domain::session::{
    delete_session_config, load_session_config, save_session_config, SessionCodec, SessionConfig,
};
use crate::domain::session::{
    Message as DomainMessage, MessageTokenUsage, ToolCall as DomainToolCall,
};
use crate::errors::OmigaError;
use serde::{Deserialize, Serialize};
use tauri::State;

/// Back-end global state (repo + chat runtime). Same managed type as `OmigaAppState`.
pub type AppState = OmigaAppState;

fn message_record_to_api(rec: MessageRecord) -> Message {
    let id = Some(rec.id.clone());
    let created_at = Some(rec.created_at.clone());
    match rec.role.as_str() {
        "assistant" => {
            let tool_calls = rec
                .tool_calls
                .and_then(|tc| serde_json::from_str::<Vec<ToolCall>>(&tc).ok());
            let token_usage = rec
                .token_usage_json
                .as_ref()
                .and_then(|j| serde_json::from_str::<MessageTokenUsage>(j).ok());
            let follow_up_suggestions = rec
                .follow_up_suggestions_json
                .and_then(|j| serde_json::from_str::<Vec<FollowUpSuggestion>>(&j).ok());
            Message::Assistant {
                content: rec.content,
                tool_calls,
                token_usage,
                reasoning_content: rec.reasoning_content,
                follow_up_suggestions,
                turn_summary: rec.turn_summary,
                id,
                created_at,
            }
        }
        "tool" => Message::Tool {
            tool_call_id: rec.tool_call_id.unwrap_or_default(),
            output: rec.content,
            id,
            created_at,
        },
        _ => Message::User {
            content: rec.content,
            id,
            created_at,
        },
    }
}

fn clean_session_search_snippet(snippet: String) -> String {
    snippet.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// List all sessions
#[tauri::command]
pub async fn list_sessions(state: State<'_, OmigaAppState>) -> CommandResult<Vec<SessionSummary>> {
    let repo = &*state.repo;

    let sessions = repo
        .list_sessions()
        .await
        .map_err(|e| OmigaError::Persistence(format!("Failed to list sessions: {}", e)))?;

    Ok(sessions
        .into_iter()
        .map(|s| SessionSummary {
            id: s.id,
            name: s.name,
            project_path: s.project_path,
            message_count: s.message_count as usize,
            updated_at: s.updated_at,
        })
        .collect())
}

/// Search sessions by title, project path, or message body.
#[tauri::command]
pub async fn search_sessions(
    state: State<'_, OmigaAppState>,
    query: String,
    limit: Option<i64>,
) -> CommandResult<Vec<SessionSearchSummary>> {
    let q = query.trim();
    if q.is_empty() {
        return Ok(Vec::new());
    }

    let repo = &*state.repo;
    let sessions = repo
        .search_sessions(q, limit.unwrap_or(50))
        .await
        .map_err(|e| OmigaError::Persistence(format!("Failed to search sessions: {}", e)))?;

    Ok(sessions
        .into_iter()
        .map(|s| SessionSearchSummary {
            id: s.id,
            name: s.name,
            project_path: s.project_path,
            message_count: s.message_count as usize,
            updated_at: s.updated_at,
            match_snippet: s.match_snippet.map(clean_session_search_snippet),
        })
        .collect())
}

/// Default page size for initial session load.  Older messages are fetched on demand
/// via `load_more_messages` when the user scrolls to the top.
///
/// 30 is enough to fill the viewport and gives context for the conversation.
/// Reducing from 100 cuts IPC payload size by ~3x, which is the main source of
/// session-switch latency (JSON serialisation through the WebView bridge).
const DEFAULT_MSG_PAGE_SIZE: i64 = 30;

/// Load a session by ID.
///
/// Only the most-recent `limit` messages are returned (default: 100).
/// `SessionData.has_more_messages` is `true` when the session has older messages.
#[tauri::command]
pub async fn load_session(
    state: State<'_, AppState>,
    session_id: String,
    limit: Option<i64>, // override page size; None → DEFAULT_MSG_PAGE_SIZE
) -> CommandResult<SessionData> {
    let start = std::time::Instant::now();
    tracing::info!(target: "omiga::perf", "load_session started: {}", session_id);

    let repo = &*state.repo;
    let page = limit.unwrap_or(DEFAULT_MSG_PAGE_SIZE).max(1);

    // Run all three reads in parallel:
    //   • session meta (DB)
    //   • message rows (DB, WAL + pool supports concurrent reads)
    //   • session config YAML (blocking file I/O via spawn_blocking)
    let sid_for_cfg = session_id.clone();
    let (session_result, raw_messages_result, config_result) = tokio::join!(
        repo.get_session_meta(&session_id),
        repo.get_session_messages_paged(&session_id, page + 1, 0),
        tokio::task::spawn_blocking(move || load_session_config(&sid_for_cfg))
    );

    let session = session_result
        .map_err(|e| OmigaError::Persistence(format!("Failed to load session: {}", e)))?;

    let Some(session) = session else {
        return Err(OmigaError::NotFound {
            resource: format!("Session {}", session_id),
        });
    };

    let active_provider_entry_name = session.active_provider_entry_name.clone();

    // Load messages only — provider restoration is done lazily in send_message so it
    // never blocks the session switch UI.  The active_provider_entry_name is returned
    // to the frontend so the ProviderSwitcher chip can update immediately.
    let raw_messages = raw_messages_result
        .map_err(|e| OmigaError::Persistence(format!("Failed to load messages: {}", e)))?;

    let has_more_messages = raw_messages.len() as i64 > page;
    // Drop the sentinel row if present; reverse so oldest-first for the UI.
    let raw_messages: Vec<_> = raw_messages
        .into_iter()
        .take(page as usize)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();

    let db_loaded = start.elapsed();
    tracing::info!(target: "omiga::perf", "db query completed: {:?}, msg_count: {}, has_more: {}",
        db_loaded, raw_messages.len(), has_more_messages);

    let messages: Vec<Message> = raw_messages
        .into_iter()
        .map(message_record_to_api)
        .collect();

    // spawn_blocking only fails if the thread panicked — fall back to defaults.
    let session_config = config_result.unwrap_or_else(|_| load_session_config(&session_id));
    let config_response = SessionConfigResponse::from(session_config);

    let total = start.elapsed();
    tracing::info!(target: "omiga::perf", "load_session completed: {:?}", total);

    Ok(SessionData {
        id: session.id,
        name: session.name,
        messages,
        project_path: session.project_path,
        created_at: session.created_at,
        updated_at: session.updated_at,
        active_provider_entry_name,
        has_more_messages,
        session_config: config_response,
    })
}

/// Load older messages for a session (pagination: scroll-to-top).
///
/// Returns messages strictly older than `before_id`, newest-first then reversed,
/// so the caller can prepend them to the existing list in correct chronological order.
#[tauri::command]
pub async fn load_more_messages(
    state: State<'_, AppState>,
    session_id: String,
    before_id: String, // oldest message id currently loaded
    limit: Option<i64>,
) -> CommandResult<Vec<Message>> {
    let repo = &*state.repo;
    let page = limit.unwrap_or(DEFAULT_MSG_PAGE_SIZE).max(1);

    let raw = repo
        .get_messages_before(&session_id, &before_id, page)
        .await
        .map_err(|e| OmigaError::Persistence(format!("Failed to load more messages: {}", e)))?;

    // raw is DESC order (newest first) — reverse to chronological for the UI.
    Ok(raw.into_iter().rev().map(message_record_to_api).collect())
}

/// Save a session (upsert)
#[tauri::command]
pub async fn save_session(state: State<'_, AppState>, session: SessionData) -> CommandResult<()> {
    let repo = &*state.repo;

    // Check if session exists
    let existing = repo
        .get_session(&session.id)
        .await
        .map_err(|e| OmigaError::Persistence(format!("Failed to check session: {}", e)))?;

    if existing.is_none() {
        // Create new session
        repo.create_session(&session.id, &session.name, &session.project_path)
            .await
            .map_err(|e| OmigaError::Persistence(format!("Failed to create session: {}", e)))?;
    }

    // Update timestamp
    repo.touch_session(&session.id)
        .await
        .map_err(|e| OmigaError::Persistence(format!("Failed to update session: {}", e)))?;

    // Save all messages using SessionCodec (eliminates duplication)
    for message in &session.messages {
        let msg_id = match message {
            Message::User { id: Some(i), .. }
            | Message::Assistant { id: Some(i), .. }
            | Message::Tool { id: Some(i), .. } => i.clone(),
            _ => uuid::Uuid::new_v4().to_string(),
        };

        // Convert command Message to domain Message for codec
        let domain_msg = match message {
            Message::User { content, .. } => DomainMessage::User {
                content: content.clone(),
            },
            Message::Assistant {
                content,
                tool_calls,
                token_usage,
                reasoning_content,
                follow_up_suggestions,
                turn_summary,
                ..
            } => DomainMessage::Assistant {
                content: content.clone(),
                tool_calls: tool_calls.as_ref().map(|tc| {
                    tc.iter()
                        .map(|t| DomainToolCall {
                            id: t.id.clone(),
                            name: t.name.clone(),
                            arguments: t.arguments.clone(),
                        })
                        .collect()
                }),
                token_usage: token_usage.clone(),
                reasoning_content: reasoning_content.clone(),
                follow_up_suggestions: follow_up_suggestions.as_ref().map(|items| {
                    items
                        .iter()
                        .map(|item| crate::domain::session::FollowUpSuggestion {
                            label: item.label.clone(),
                            prompt: item.prompt.clone(),
                        })
                        .collect()
                }),
                turn_summary: turn_summary.clone(),
            },
            Message::Tool {
                tool_call_id,
                output,
                ..
            } => DomainMessage::Tool {
                tool_call_id: tool_call_id.clone(),
                output: output.clone(),
            },
        };

        // Use SessionCodec for serialization (single source of truth)
        let record = SessionCodec::message_to_record(&domain_msg, &msg_id, &session.id);

        repo.save_message(record.as_insert())
            .await
            .map_err(|e| OmigaError::Persistence(format!("Failed to save message: {}", e)))?;
    }

    Ok(())
}

/// Create a new session
#[tauri::command]
pub async fn create_session(
    state: State<'_, AppState>,
    name: String,
    project_path: String,
) -> CommandResult<SessionData> {
    let repo = &*state.repo;

    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();

    repo.create_session(&id, &name, &project_path)
        .await
        .map_err(|e| OmigaError::Persistence(format!("Failed to create session: {}", e)))?;

    let session_config = SessionConfig::default_for_new();
    let _ = save_session_config(&id, &session_config);

    Ok(SessionData {
        id,
        name,
        messages: vec![],
        project_path,
        created_at: now.clone(),
        updated_at: now,
        active_provider_entry_name: None,
        has_more_messages: false,
        session_config: session_config.into(),
    })
}

/// Delete a session
#[tauri::command]
pub async fn delete_session(state: State<'_, AppState>, session_id: String) -> CommandResult<()> {
    let repo = &*state.repo;

    repo.delete_session(&session_id)
        .await
        .map_err(|e| OmigaError::Persistence(format!("Failed to delete session: {}", e)))?;

    delete_session_config(&session_id);

    Ok(())
}

/// Rename a session
#[tauri::command]
pub async fn rename_session(
    state: State<'_, AppState>,
    session_id: String,
    name: String,
) -> CommandResult<()> {
    let repo = &*state.repo;

    repo.rename_session(&session_id, &name)
        .await
        .map_err(|e| OmigaError::Persistence(format!("Failed to rename session: {}", e)))?;

    Ok(())
}

/// Update session working directory (project path)
#[tauri::command]
pub async fn update_session_project_path(
    state: State<'_, AppState>,
    session_id: String,
    project_path: String,
) -> CommandResult<()> {
    let repo = &*state.repo;

    repo.update_session_project_path(&session_id, &project_path)
        .await
        .map_err(|e| {
            OmigaError::Persistence(format!("Failed to update session project path: {}", e))
        })?;

    Ok(())
}

/// Save a single message to a session
#[tauri::command]
pub async fn save_message(
    state: State<'_, AppState>,
    session_id: String,
    message: Message,
) -> CommandResult<()> {
    let repo = &*state.repo;
    let msg_id = match &message {
        Message::User { id: Some(i), .. }
        | Message::Assistant { id: Some(i), .. }
        | Message::Tool { id: Some(i), .. } => i.clone(),
        _ => uuid::Uuid::new_v4().to_string(),
    };

    // Convert command Message to domain Message for codec
    let domain_msg = match message {
        Message::User { content, .. } => DomainMessage::User { content },
        Message::Assistant {
            content,
            tool_calls,
            token_usage,
            reasoning_content,
            follow_up_suggestions,
            turn_summary,
            ..
        } => DomainMessage::Assistant {
            content,
            tool_calls: tool_calls.map(|tc| {
                tc.into_iter()
                    .map(|t| DomainToolCall {
                        id: t.id,
                        name: t.name,
                        arguments: t.arguments,
                    })
                    .collect()
            }),
            token_usage,
            reasoning_content,
            follow_up_suggestions: follow_up_suggestions.map(|items| {
                items
                    .into_iter()
                    .map(|item| crate::domain::session::FollowUpSuggestion {
                        label: item.label,
                        prompt: item.prompt,
                    })
                    .collect()
            }),
            turn_summary,
        },
        Message::Tool {
            tool_call_id,
            output,
            ..
        } => DomainMessage::Tool {
            tool_call_id,
            output,
        },
    };

    // Use SessionCodec for serialization (single source of truth)
    let record = SessionCodec::message_to_record(&domain_msg, &msg_id, &session_id);

    repo.save_message(record.as_insert())
        .await
        .map_err(|e| OmigaError::Persistence(format!("Failed to save message: {}", e)))?;

    // Update session timestamp
    repo.touch_session(&session_id)
        .await
        .map_err(|e| OmigaError::Persistence(format!("Failed to update session: {}", e)))?;

    Ok(())
}

/// Clear all messages from a session
#[tauri::command]
pub async fn clear_session_messages(
    state: State<'_, AppState>,
    session_id: String,
) -> CommandResult<()> {
    let repo = &*state.repo;

    repo.clear_messages(&session_id)
        .await
        .map_err(|e| OmigaError::Persistence(format!("Failed to clear messages: {}", e)))?;

    Ok(())
}

/// Refresh MCP connections for a new session or after /clear
///
/// This triggers session boundary detection in the MCP connection manager:
/// - Stale connections (> 5 min idle) are closed
/// - stdio connections from different sessions are reconnected (avoiding zombie processes)
/// - Remote connections are health-checked
/// - Configuration is reloaded to pick up changes
#[tauri::command]
pub async fn refresh_session_mcp_connections(
    state: State<'_, AppState>,
    session_id: String,
    project_path: String,
) -> CommandResult<McpRefreshResult> {
    use std::path::PathBuf;

    let project_root = PathBuf::from(project_path);

    // Get the manager for this project, which will trigger session refresh
    let manager = state
        .chat
        .mcp_manager
        .get_manager(project_root.clone(), session_id.clone())
        .await;

    // Refresh connections for the new session
    manager.refresh_for_new_session(session_id.clone()).await;

    // Get stats after refresh
    let stats = manager.stats().await;

    Ok(McpRefreshResult {
        project_path: project_root.to_string_lossy().to_string(),
        session_id,
        connections_total: stats.total,
        connections_stdio: stats.stdio,
        connections_remote: stats.remote,
        connections_idle_closed: stats.idle,
    })
}

/// MCP refresh result statistics
#[derive(Debug, Serialize, Deserialize)]
pub struct McpRefreshResult {
    pub project_path: String,
    pub session_id: String,
    pub connections_total: usize,
    pub connections_stdio: usize,
    pub connections_remote: usize,
    pub connections_idle_closed: usize,
}

/// Get MCP connection statistics for all projects
#[tauri::command]
pub async fn get_mcp_connection_stats(
    state: State<'_, AppState>,
) -> CommandResult<Vec<McpRefreshResult>> {
    let all_stats = state.chat.mcp_manager.all_stats().await;

    let results: Vec<McpRefreshResult> = all_stats
        .into_iter()
        .map(|(path, stats)| McpRefreshResult {
            project_path: path.to_string_lossy().to_string(),
            session_id: stats.current_session,
            connections_total: stats.total,
            connections_stdio: stats.stdio,
            connections_remote: stats.remote,
            connections_idle_closed: stats.idle,
        })
        .collect();

    Ok(results)
}

/// Get or create settings value
#[tauri::command]
pub async fn get_setting(state: State<'_, AppState>, key: String) -> CommandResult<Option<String>> {
    let repo = &*state.repo;

    let value = repo
        .get_setting(&key)
        .await
        .map_err(|e| OmigaError::Persistence(format!("Failed to get setting: {}", e)))?;

    Ok(value)
}

/// Set a setting value
#[tauri::command]
pub async fn set_setting(
    state: State<'_, AppState>,
    key: String,
    value: String,
) -> CommandResult<()> {
    let repo = &*state.repo;

    repo.set_setting(&key, &value)
        .await
        .map_err(|e| OmigaError::Persistence(format!("Failed to set setting: {}", e)))?;

    Ok(())
}

/// Serializable per-session config for the frontend.
#[derive(Debug, Serialize, Deserialize)]
pub struct SessionConfigResponse {
    pub active_provider_entry_name: Option<String>,
    pub permission_mode: String,
    pub composer_agent_type: String,
    pub execution_environment: String,
    pub ssh_server: Option<String>,
    pub sandbox_backend: String,
    pub local_venv_type: String,
    pub local_venv_name: String,
    pub use_worktree: bool,
    pub runtime_constraints: Option<crate::domain::runtime_constraints::RuntimeConstraintConfig>,
}

impl From<SessionConfig> for SessionConfigResponse {
    fn from(cfg: SessionConfig) -> Self {
        Self {
            active_provider_entry_name: cfg.active_provider_entry_name,
            permission_mode: cfg.permission_mode,
            composer_agent_type: cfg.composer_agent_type,
            execution_environment: cfg.execution_environment,
            ssh_server: cfg.ssh_server,
            sandbox_backend: cfg.sandbox_backend,
            local_venv_type: cfg.local_venv_type,
            local_venv_name: cfg.local_venv_name,
            use_worktree: cfg.use_worktree,
            runtime_constraints: cfg.runtime_constraints,
        }
    }
}

/// Load per-session config from `~/.omiga/sessions/<session_id>.yaml`.
#[tauri::command]
pub async fn get_session_config(session_id: String) -> CommandResult<SessionConfigResponse> {
    let cfg = load_session_config(&session_id);
    Ok(cfg.into())
}

/// Save per-session config to `~/.omiga/sessions/<session_id>.yaml`.
#[tauri::command]
pub async fn save_session_config_command(
    session_id: String,
    config: SessionConfigResponse,
) -> CommandResult<()> {
    let existing = load_session_config(&session_id);
    let cfg = SessionConfig {
        active_provider_entry_name: config.active_provider_entry_name,
        permission_mode: config.permission_mode,
        composer_agent_type: config.composer_agent_type,
        execution_environment: config.execution_environment,
        ssh_server: config.ssh_server,
        sandbox_backend: config.sandbox_backend,
        local_venv_type: config.local_venv_type,
        local_venv_name: config.local_venv_name,
        use_worktree: config.use_worktree,
        runtime_constraints: config.runtime_constraints.or(existing.runtime_constraints),
    };
    save_session_config(&session_id, &cfg)
        .map_err(|e| OmigaError::Persistence(format!("Failed to save session config: {}", e)))?;
    Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RuntimeConstraintConfigSnapshot {
    pub project_config: crate::domain::runtime_constraints::RuntimeConstraintConfig,
    pub session_config: Option<crate::domain::runtime_constraints::RuntimeConstraintConfig>,
    pub resolved_enabled: bool,
    pub resolved_buffer_responses: bool,
    pub resolved_policy_pack: crate::domain::runtime_constraints::ConstraintPolicyPack,
    pub registry: Vec<RuntimeConstraintRuleStatus>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RuntimeConstraintRuleStatus {
    pub id: String,
    pub description: String,
    pub severity: crate::domain::runtime_constraints::ConstraintSeverity,
    pub enabled: bool,
    pub phases: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RuntimeConstraintTraceEventResponse {
    pub id: String,
    pub session_id: String,
    pub round_id: String,
    pub message_id: String,
    pub event_type: String,
    pub constraint_id: Option<String>,
    pub payload_json: String,
    pub created_at: String,
}

impl From<RuntimeConstraintEventRecord> for RuntimeConstraintTraceEventResponse {
    fn from(v: RuntimeConstraintEventRecord) -> Self {
        Self {
            id: v.id,
            session_id: v.session_id,
            round_id: v.round_id,
            message_id: v.message_id,
            event_type: v.event_type,
            constraint_id: v.constraint_id,
            payload_json: v.payload_json,
            created_at: v.created_at,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RuntimeConstraintTraceRoundResponse {
    pub round_id: String,
    pub session_id: String,
    pub message_id: String,
    pub event_count: usize,
    pub first_event_at: String,
    pub last_event_at: String,
}

impl From<RuntimeConstraintRoundTraceRecord> for RuntimeConstraintTraceRoundResponse {
    fn from(v: RuntimeConstraintRoundTraceRecord) -> Self {
        Self {
            round_id: v.round_id,
            session_id: v.session_id,
            message_id: v.message_id,
            event_count: v.event_count.max(0) as usize,
            first_event_at: v.first_event_at,
            last_event_at: v.last_event_at,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RuntimeConstraintTraceSummary {
    pub round_id: String,
    pub session_id: String,
    pub message_id: String,
    pub total_events: usize,
    pub first_event_at: Option<String>,
    pub last_event_at: Option<String>,
    pub event_type_counts: std::collections::BTreeMap<String, usize>,
    pub constraint_counts: std::collections::BTreeMap<String, usize>,
    pub noticed_constraints: Vec<String>,
    pub gate_constraints: Vec<String>,
    pub retry_constraints: Vec<String>,
    pub commit_phases: Vec<String>,
    pub config_payload: Option<serde_json::Value>,
}

fn summarize_runtime_constraint_events(
    rows: &[RuntimeConstraintEventRecord],
) -> Option<RuntimeConstraintTraceSummary> {
    let first = rows.first()?;
    let mut event_type_counts = std::collections::BTreeMap::<String, usize>::new();
    let mut constraint_counts = std::collections::BTreeMap::<String, usize>::new();
    let mut noticed = std::collections::BTreeSet::<String>::new();
    let mut gated = std::collections::BTreeSet::<String>::new();
    let mut retried = std::collections::BTreeSet::<String>::new();
    let mut commit_phases = std::collections::BTreeSet::<String>::new();
    let mut config_payload = None;

    for row in rows {
        *event_type_counts.entry(row.event_type.clone()).or_insert(0) += 1;
        if let Some(ref cid) = row.constraint_id {
            *constraint_counts.entry(cid.clone()).or_insert(0) += 1;
        }

        let parsed = serde_json::from_str::<serde_json::Value>(&row.payload_json).ok();
        match row.event_type.as_str() {
            "runtime_constraints.notices" => {
                if let Some(ids) = parsed
                    .as_ref()
                    .and_then(|v| v.get("ids"))
                    .and_then(|v| v.as_array())
                {
                    for id in ids.iter().filter_map(|v| v.as_str()) {
                        noticed.insert(id.to_string());
                    }
                }
            }
            "runtime_constraints.gate" => {
                if let Some(id) = parsed
                    .as_ref()
                    .and_then(|v| v.get("id"))
                    .and_then(|v| v.as_str())
                {
                    gated.insert(id.to_string());
                }
            }
            "runtime_constraint_retry" => {
                if let Some(id) = parsed
                    .as_ref()
                    .and_then(|v| v.get("id"))
                    .and_then(|v| v.as_str())
                {
                    retried.insert(id.to_string());
                }
            }
            "runtime_constraints.commit" => {
                if let Some(phase) = parsed
                    .as_ref()
                    .and_then(|v| v.get("phase"))
                    .and_then(|v| v.as_str())
                {
                    commit_phases.insert(phase.to_string());
                }
            }
            "runtime_constraints.config" => {
                if config_payload.is_none() {
                    config_payload = parsed;
                }
            }
            _ => {}
        }
    }

    Some(RuntimeConstraintTraceSummary {
        round_id: first.round_id.clone(),
        session_id: first.session_id.clone(),
        message_id: first.message_id.clone(),
        total_events: rows.len(),
        first_event_at: rows.first().map(|r| r.created_at.clone()),
        last_event_at: rows.last().map(|r| r.created_at.clone()),
        event_type_counts,
        constraint_counts,
        noticed_constraints: noticed.into_iter().collect(),
        gate_constraints: gated.into_iter().collect(),
        retry_constraints: retried.into_iter().collect(),
        commit_phases: commit_phases.into_iter().collect(),
        config_payload,
    })
}

#[tauri::command]
pub async fn get_runtime_constraints_config(
    session_id: Option<String>,
    project_path: String,
) -> CommandResult<RuntimeConstraintConfigSnapshot> {
    let project_root = std::path::PathBuf::from(project_path);
    let project_cfg =
        crate::domain::runtime_constraints::load_project_runtime_constraint_config(&project_root);
    let session_cfg = session_id
        .as_deref()
        .map(load_session_config)
        .and_then(|cfg| cfg.runtime_constraints);
    let resolved = crate::domain::runtime_constraints::resolve_runtime_constraint_config(
        &project_root,
        session_cfg.as_ref(),
    );
    let harness =
        crate::domain::runtime_constraints::RuntimeConstraintHarness::from_config(resolved.clone());

    Ok(RuntimeConstraintConfigSnapshot {
        project_config: project_cfg,
        session_config: session_cfg,
        resolved_enabled: resolved.enabled,
        resolved_buffer_responses: resolved.buffer_responses,
        resolved_policy_pack: resolved.policy_pack,
        registry: harness
            .registry()
            .into_iter()
            .map(|m| RuntimeConstraintRuleStatus {
                id: m.id.to_string(),
                description: m.description.to_string(),
                severity: m.severity,
                enabled: m.enabled,
                phases: m.phases.iter().map(|p| format!("{:?}", p)).collect(),
            })
            .collect(),
    })
}

#[tauri::command]
pub async fn save_project_runtime_constraints_config(
    project_path: String,
    config: crate::domain::runtime_constraints::RuntimeConstraintConfig,
) -> CommandResult<()> {
    crate::domain::runtime_constraints::save_project_runtime_constraint_config(
        std::path::Path::new(&project_path),
        &config,
    )
    .map_err(|e| {
        OmigaError::Persistence(format!("Failed to save project runtime constraints: {}", e))
    })?;
    Ok(())
}

#[tauri::command]
pub async fn save_session_runtime_constraints_config(
    session_id: String,
    config: Option<crate::domain::runtime_constraints::RuntimeConstraintConfig>,
) -> CommandResult<()> {
    let mut session_cfg = load_session_config(&session_id);
    session_cfg.runtime_constraints = config;
    save_session_config(&session_id, &session_cfg).map_err(|e| {
        OmigaError::Persistence(format!("Failed to save session runtime constraints: {}", e))
    })?;
    Ok(())
}

#[tauri::command]
pub async fn get_runtime_constraint_trace(
    state: State<'_, AppState>,
    round_id: String,
) -> CommandResult<Vec<RuntimeConstraintTraceEventResponse>> {
    let repo = &*state.repo;
    let rows = repo
        .list_runtime_constraint_events_for_round(&round_id)
        .await
        .map_err(|e| {
            OmigaError::Persistence(format!("Failed to load runtime constraint trace: {}", e))
        })?;
    Ok(rows.into_iter().map(Into::into).collect())
}

#[tauri::command]
pub async fn list_runtime_constraint_trace_rounds(
    state: State<'_, AppState>,
    session_id: String,
    limit: Option<usize>,
) -> CommandResult<Vec<RuntimeConstraintTraceRoundResponse>> {
    let repo = &*state.repo;
    let rows = repo
        .list_runtime_constraint_rounds_for_session(&session_id, limit.unwrap_or(20) as i64)
        .await
        .map_err(|e| {
            OmigaError::Persistence(format!(
                "Failed to list runtime constraint trace rounds: {}",
                e
            ))
        })?;
    Ok(rows.into_iter().map(Into::into).collect())
}

#[tauri::command]
pub async fn summarize_runtime_constraint_trace(
    state: State<'_, AppState>,
    round_id: String,
) -> CommandResult<Option<RuntimeConstraintTraceSummary>> {
    let repo = &*state.repo;
    let rows = repo
        .list_runtime_constraint_events_for_round(&round_id)
        .await
        .map_err(|e| {
            OmigaError::Persistence(format!(
                "Failed to summarize runtime constraint trace: {}",
                e
            ))
        })?;
    Ok(summarize_runtime_constraint_events(&rows))
}

/// Pre-warm LLM config and MCP/integrations/permission caches immediately after a session
/// switch — before the user sends their first message.
///
/// Inspired by codex's `session_startup_prewarm` pattern: start expensive async I/O the
/// moment the user switches sessions so the cold-cache penalty is paid in the background,
/// not at the start of the first `send_message` call.
///
/// This command is fire-and-forget from the frontend; errors are logged but not surfaced.
#[tauri::command]
pub async fn prewarm_session(
    state: State<'_, AppState>,
    project_path: String,
    active_provider_entry_name: Option<String>,
) -> CommandResult<()> {
    use crate::app_state::IntegrationsConfigCacheSlot;
    use crate::domain::chat_state::{
        McpToolCache, PermissionDenyCache, MCP_TOOL_CACHE_TTL, PERMISSION_DENY_CACHE_TTL,
    };
    use std::path::PathBuf;

    let project_root = PathBuf::from(&project_path);

    // 1. Pre-warm provider config and cached_config_file.
    //    apply_named_provider_runtime also warms the config file cache as a side-effect.
    if let Some(ref provider_name) = active_provider_entry_name {
        let name = provider_name.trim();
        if !name.is_empty() {
            let current = state.chat.active_provider_entry_name.lock().await.clone();
            let already_active = current.as_deref().map(str::trim) == Some(name);
            drop(current);
            if !already_active {
                if let Err(e) =
                    crate::commands::chat::apply_named_provider_runtime(&state, name).await
                {
                    tracing::debug!(target: "omiga::prewarm", "provider warm skipped: {}", e);
                }
            } else {
                // Provider already matches — still warm the config file cache.
                let _ = crate::commands::chat::get_config_file(&state).await;
            }
        }
    } else {
        let _ = crate::commands::chat::get_config_file(&state).await;
    }

    // 2. Pre-warm integrations config cache (file read, synchronous but fast).
    {
        let hit = state
            .integrations_config_cache
            .lock()
            .expect("integrations config cache poisoned")
            .get(&project_root)
            .filter(|s| s.cached_at.elapsed() < crate::app_state::INTEGRATIONS_CONFIG_CACHE_TTL)
            .is_some();

        if !hit {
            let cfg = crate::domain::integrations_config::load_integrations_config(&project_root);
            state
                .integrations_config_cache
                .lock()
                .expect("integrations config cache poisoned")
                .insert(
                    project_root.clone(),
                    IntegrationsConfigCacheSlot {
                        config: cfg,
                        cached_at: std::time::Instant::now(),
                    },
                );
        }
    }

    // 3. Pre-warm permission deny rules cache (reads up to 4 settings files).
    {
        let hit = state
            .chat
            .permission_deny_cache
            .lock()
            .await
            .get(&project_root)
            .filter(|c| c.cached_at.elapsed() < PERMISSION_DENY_CACHE_TTL)
            .is_some();

        if !hit {
            let entries =
                crate::domain::permissions::load_merged_permission_deny_rule_entries(&project_root);
            state.chat.permission_deny_cache.lock().await.insert(
                project_root.clone(),
                PermissionDenyCache {
                    entries,
                    cached_at: std::time::Instant::now(),
                },
            );
        }
    }

    // 4. Pre-warm MCP tool schema cache as a background task — discovery can take seconds
    //    when stdio servers start cold.  We spawn and forget; send_message will reuse the
    //    warm cache if it completes in time, or re-discover if not.
    {
        let current_mcp_config_signature =
            crate::domain::mcp::merged_mcp_servers_signature(&project_root);
        let hit = state
            .chat
            .mcp_tool_cache
            .lock()
            .await
            .get(&project_root)
            .filter(|c| {
                c.cached_at.elapsed() < MCP_TOOL_CACHE_TTL
                    && c.config_signature == current_mcp_config_signature
            })
            .is_some();

        if !hit {
            let mcp_tool_cache = state.chat.mcp_tool_cache.clone();
            let root = project_root.clone();
            tokio::spawn(async move {
                let mcp_timeout = std::time::Duration::from_secs(10);
                let config_signature = crate::domain::mcp::merged_mcp_servers_signature(&root);
                let schemas =
                    crate::domain::mcp::tool_pool::discover_mcp_tool_schemas(&root, mcp_timeout)
                        .await;
                mcp_tool_cache.lock().await.insert(
                    root,
                    McpToolCache {
                        schemas,
                        cached_at: std::time::Instant::now(),
                        config_signature,
                    },
                );
            });
        }
    }

    tracing::debug!(target: "omiga::prewarm", "session prewarm enqueued for {}", project_path);
    Ok(())
}

/// Session summary (for listing)
#[derive(Debug, Serialize, Deserialize)]
pub struct SessionSummary {
    pub id: String,
    pub name: String,
    pub project_path: String,
    pub message_count: usize,
    pub updated_at: String,
}

/// Session search summary (for search modal)
#[derive(Debug, Serialize, Deserialize)]
pub struct SessionSearchSummary {
    pub id: String,
    pub name: String,
    pub project_path: String,
    pub message_count: usize,
    pub updated_at: String,
    pub match_snippet: Option<String>,
}

/// A chat message
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "role")]
pub enum Message {
    #[serde(rename = "user")]
    User {
        content: String,
        /// SQLite `messages.id` when loaded from DB; omitted for legacy JSON.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        /// RFC3339 creation timestamp from DB; used by frontend for elapsed-time display.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        created_at: Option<String>,
    },
    #[serde(rename = "assistant")]
    Assistant {
        content: String,
        tool_calls: Option<Vec<ToolCall>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        token_usage: Option<MessageTokenUsage>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reasoning_content: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        follow_up_suggestions: Option<Vec<FollowUpSuggestion>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        turn_summary: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        /// RFC3339 creation timestamp from DB; used by frontend for elapsed-time display.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        created_at: Option<String>,
    },
    #[serde(rename = "tool")]
    Tool {
        tool_call_id: String,
        output: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        /// RFC3339 creation timestamp from DB; used by frontend for elapsed-time display.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        created_at: Option<String>,
    },
}

/// A tool call
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

/// A follow-up suggestion
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FollowUpSuggestion {
    pub label: String,
    pub prompt: String,
}

/// Full session data
#[derive(Debug, Serialize, Deserialize)]
pub struct SessionData {
    pub id: String,
    pub name: String,
    pub messages: Vec<Message>,
    pub project_path: String,
    pub created_at: String,
    pub updated_at: String,
    /// The provider entry name stored for this session (from DB).
    /// Frontend uses this to update the ProviderSwitcher chip without a round-trip.
    pub active_provider_entry_name: Option<String>,
    /// True when there are older messages not included in this response (pagination).
    pub has_more_messages: bool,
    /// Per-session composer/runtime config read from `~/.omiga/sessions/<id>.yaml`.
    pub session_config: SessionConfigResponse,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summarize_runtime_constraint_events_collects_core_signals() {
        let rows = vec![
            RuntimeConstraintEventRecord {
                id: "1".into(),
                session_id: "s".into(),
                round_id: "r".into(),
                message_id: "m".into(),
                event_type: "runtime_constraints.config".into(),
                constraint_id: None,
                payload_json: serde_json::json!({
                    "enabled": true,
                    "buffer_responses": true,
                    "policy_pack": "balanced"
                })
                .to_string(),
                created_at: "2026-04-17T00:00:00Z".into(),
            },
            RuntimeConstraintEventRecord {
                id: "2".into(),
                session_id: "s".into(),
                round_id: "r".into(),
                message_id: "m".into(),
                event_type: "runtime_constraints.notices".into(),
                constraint_id: None,
                payload_json: serde_json::json!({ "ids": ["evidence_first"] }).to_string(),
                created_at: "2026-04-17T00:00:01Z".into(),
            },
            RuntimeConstraintEventRecord {
                id: "3".into(),
                session_id: "s".into(),
                round_id: "r".into(),
                message_id: "m".into(),
                event_type: "runtime_constraint_retry".into(),
                constraint_id: Some("evidence_first".into()),
                payload_json: serde_json::json!({ "id": "evidence_first" }).to_string(),
                created_at: "2026-04-17T00:00:02Z".into(),
            },
            RuntimeConstraintEventRecord {
                id: "4".into(),
                session_id: "s".into(),
                round_id: "r".into(),
                message_id: "m".into(),
                event_type: "runtime_constraints.commit".into(),
                constraint_id: None,
                payload_json: serde_json::json!({ "phase": "final" }).to_string(),
                created_at: "2026-04-17T00:00:03Z".into(),
            },
        ];

        let summary = summarize_runtime_constraint_events(&rows).expect("summary");
        assert_eq!(summary.round_id, "r");
        assert_eq!(summary.total_events, 4);
        assert_eq!(
            summary.event_type_counts.get("runtime_constraint_retry"),
            Some(&1)
        );
        assert_eq!(summary.constraint_counts.get("evidence_first"), Some(&1));
        assert_eq!(
            summary.noticed_constraints,
            vec!["evidence_first".to_string()]
        );
        assert_eq!(
            summary.retry_constraints,
            vec!["evidence_first".to_string()]
        );
        assert_eq!(summary.commit_phases, vec!["final".to_string()]);
        assert!(summary.config_payload.is_some());
    }
}
