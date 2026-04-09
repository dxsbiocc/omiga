//! Session management commands

use super::CommandResult;
use crate::app_state::OmigaAppState;
use crate::domain::session::{
    Message as DomainMessage, MessageTokenUsage, ToolCall as DomainToolCall,
};
use crate::domain::session_codec::SessionCodec;
use crate::errors::OmigaError;
use serde::{Deserialize, Serialize};
use tauri::State;

/// Back-end global state (repo + chat runtime). Same managed type as `OmigaAppState`.
pub type AppState = OmigaAppState;

/// List all sessions
#[tauri::command]
pub async fn list_sessions(
    state: State<'_, OmigaAppState>,
) -> CommandResult<Vec<SessionSummary>> {
    let repo = state.repo.lock().await;

    let sessions = repo
        .list_sessions()
        .await
        .map_err(|e| OmigaError::Persistence(format!("Failed to list sessions: {}", e)))?;

    Ok(sessions
        .into_iter()
        .map(|s| SessionSummary {
            id: s.id,
            name: s.name,
            message_count: s.message_count as usize,
            updated_at: s.updated_at,
        })
        .collect())
}

/// Load a session by ID
#[tauri::command]
pub async fn load_session(
    state: State<'_, AppState>,
    session_id: String,
) -> CommandResult<SessionData> {
    let repo = state.repo.lock().await;

    let session = repo
        .get_session(&session_id)
        .await
        .map_err(|e| OmigaError::Persistence(format!("Failed to load session: {}", e)))?;

    let Some(session) = session else {
        return Err(OmigaError::NotFound {
            resource: format!("Session {}", session_id),
        });
    };

    // Use SessionCodec for conversion (eliminates duplication)
    let domain_session = SessionCodec::db_to_domain(session);

    // Convert domain messages to command messages
    let messages: Vec<Message> = domain_session
        .messages
        .into_iter()
        .map(|m| match m {
            DomainMessage::User { content } => Message::User { content },
            DomainMessage::Assistant {
                content,
                tool_calls,
                token_usage,
                reasoning_content,
            } => Message::Assistant {
                content,
                tool_calls: tool_calls.map(|tc| {
                    tc.into_iter()
                        .map(|t| ToolCall {
                            id: t.id,
                            name: t.name,
                            arguments: t.arguments,
                        })
                        .collect()
                }),
                token_usage,
                reasoning_content,
            },
            DomainMessage::Tool { tool_call_id, output } => Message::Tool {
                tool_call_id,
                output,
            },
        })
        .collect();

    Ok(SessionData {
        id: domain_session.id,
        name: domain_session.name,
        messages,
        project_path: domain_session.project_path,
        created_at: domain_session.created_at.to_rfc3339(),
        updated_at: domain_session.updated_at.to_rfc3339(),
    })
}

/// Save a session (upsert)
#[tauri::command]
pub async fn save_session(
    state: State<'_, AppState>,
    session: SessionData,
) -> CommandResult<()> {
    let repo = state.repo.lock().await;

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
        let msg_id = uuid::Uuid::new_v4().to_string();

        // Convert command Message to domain Message for codec
        let domain_msg = match message {
            Message::User { content } => DomainMessage::User { content: content.clone() },
            Message::Assistant {
                content,
                tool_calls,
                token_usage,
                reasoning_content,
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
            },
            Message::Tool { tool_call_id, output } => DomainMessage::Tool {
                tool_call_id: tool_call_id.clone(),
                output: output.clone(),
            },
        };

        // Use SessionCodec for serialization (single source of truth)
        let (
            id,
            session_id,
            role,
            content,
            tool_calls,
            tool_call_id,
            token_usage_json,
            reasoning_content,
        ) = SessionCodec::message_to_record(&domain_msg, &msg_id, &session.id);

        repo.save_message(
            &id,
            &session_id,
            &role,
            &content,
            tool_calls.as_deref(),
            tool_call_id.as_deref(),
            token_usage_json.as_deref(),
            reasoning_content.as_deref(),
        )
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
    let repo = state.repo.lock().await;

    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();

    repo.create_session(&id, &name, &project_path)
        .await
        .map_err(|e| OmigaError::Persistence(format!("Failed to create session: {}", e)))?;

    Ok(SessionData {
        id,
        name,
        messages: vec![],
        project_path,
        created_at: now.clone(),
        updated_at: now,
    })
}

/// Delete a session
#[tauri::command]
pub async fn delete_session(
    state: State<'_, AppState>,
    session_id: String,
) -> CommandResult<()> {
    let repo = state.repo.lock().await;

    repo.delete_session(&session_id)
        .await
        .map_err(|e| OmigaError::Persistence(format!("Failed to delete session: {}", e)))?;

    Ok(())
}

/// Rename a session
#[tauri::command]
pub async fn rename_session(
    state: State<'_, AppState>,
    session_id: String,
    name: String,
) -> CommandResult<()> {
    let repo = state.repo.lock().await;

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
    let repo = state.repo.lock().await;

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
    let repo = state.repo.lock().await;
    let msg_id = uuid::Uuid::new_v4().to_string();

    // Convert command Message to domain Message for codec
    let domain_msg = match message {
        Message::User { content } => DomainMessage::User { content },
        Message::Assistant {
            content,
            tool_calls,
            token_usage,
            reasoning_content,
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
        },
        Message::Tool { tool_call_id, output } => DomainMessage::Tool {
            tool_call_id,
            output,
        },
    };

    // Use SessionCodec for serialization (single source of truth)
    let (
        id,
        sid,
        role,
        content,
        tool_calls,
        tool_call_id,
        token_usage_json,
        reasoning_content,
    ) = SessionCodec::message_to_record(&domain_msg, &msg_id, &session_id);

    repo.save_message(
        &id,
        &sid,
        &role,
        &content,
        tool_calls.as_deref(),
        tool_call_id.as_deref(),
        token_usage_json.as_deref(),
        reasoning_content.as_deref(),
    )
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
    let repo = state.repo.lock().await;

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
pub async fn get_setting(
    state: State<'_, AppState>,
    key: String,
) -> CommandResult<Option<String>> {
    let repo = state.repo.lock().await;

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
    let repo = state.repo.lock().await;

    repo.set_setting(&key, &value)
        .await
        .map_err(|e| OmigaError::Persistence(format!("Failed to set setting: {}", e)))?;

    Ok(())
}

/// Session summary (for listing)
#[derive(Debug, Serialize, Deserialize)]
pub struct SessionSummary {
    pub id: String,
    pub name: String,
    pub message_count: usize,
    pub updated_at: String,
}

/// A chat message
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "role")]
pub enum Message {
    #[serde(rename = "user")]
    User { content: String },
    #[serde(rename = "assistant")]
    Assistant {
        content: String,
        tool_calls: Option<Vec<ToolCall>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        token_usage: Option<MessageTokenUsage>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reasoning_content: Option<String>,
    },
    #[serde(rename = "tool")]
    Tool { tool_call_id: String, output: String },
}

/// A tool call
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
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
}
