use super::permissions::{
    ask_user_waiter_key, cancel_ask_user_waiters_for_message,
    cancel_permission_tool_waiters_for_message, cancel_permission_tool_waiters_for_session,
};
use super::CommandResult;
use crate::app_state::OmigaAppState;
use crate::errors::{ChatError, OmigaError};
use std::sync::Arc;
use tauri::State;

/// Submit answers for a blocked `ask_user_question` tool call (chat UI).
#[tauri::command]
pub async fn submit_ask_user_answer(
    app_state: State<'_, OmigaAppState>,
    session_id: String,
    message_id: String,
    tool_use_id: String,
    answers: serde_json::Value,
) -> CommandResult<()> {
    let key = ask_user_waiter_key(&session_id, &message_id, &tool_use_id);
    let mut map = app_state.chat.ask_user_waiters.lock().await;
    let Some(waiter) = map.remove(&key) else {
        return Err(OmigaError::Chat(ChatError::StreamError(
            "No pending ask_user_question for this tool call (already answered or expired)."
                .to_string(),
        )));
    };
    drop(map);
    let _ = waiter.tx.send(Ok(answers));
    Ok(())
}

/// Cancel an in-progress stream by message_id
#[tauri::command]
pub async fn cancel_stream(
    app_state: State<'_, OmigaAppState>,
    message_id: String,
) -> CommandResult<()> {
    // Kill foreground/background bash and any tool that listens on `ToolContext.cancel` for this round.
    {
        let ar = app_state.chat.active_rounds.lock().await;
        if let Some(rs) = ar.get(&message_id) {
            rs.round_cancel.cancel();
        }
    }

    let session_for_waiters = {
        let ar = app_state.chat.active_rounds.lock().await;
        ar.get(&message_id).map(|r| r.session_id.clone())
    };
    if let Some(ref sid) = session_for_waiters {
        cancel_ask_user_waiters_for_message(&app_state.chat.ask_user_waiters, sid, &message_id)
            .await;
        cancel_permission_tool_waiters_for_message(
            &app_state.chat.permission_tool_waiters,
            sid,
            &message_id,
        )
        .await;
    } else {
        let repo = &*app_state.repo;
        if let Ok(Some(round)) = repo.get_round_by_message_id(&message_id).await {
            cancel_ask_user_waiters_for_message(
                &app_state.chat.ask_user_waiters,
                &round.session_id,
                &message_id,
            )
            .await;
            cancel_permission_tool_waiters_for_message(
                &app_state.chat.permission_tool_waiters,
                &round.session_id,
                &message_id,
            )
            .await;
        }
    }

    // Look up the round by message_id
    let repo = &*app_state.repo;

    // Find active round
    if let Ok(Some(round)) = repo.get_round_by_message_id(&message_id).await {
        if round.is_active() {
            // Cancel in database
            repo.cancel_round(&round.id, Some("User requested cancellation"))
                .await
                .map_err(|e| {
                    OmigaError::Chat(ChatError::StreamError(format!(
                        "Failed to cancel round: {}",
                        e
                    )))
                })?;

            // Set cancellation flag for in-memory tracking
            let active_rounds = app_state.chat.active_rounds.lock().await;
            if let Some(round_state) = active_rounds.get(&message_id) {
                let mut cancelled = round_state.cancelled.write().await;
                *cancelled = true;
            }

            tracing::info!("Cancelled round {} for message {}", round.id, message_id);
        }
    } else {
        // Try to cancel by looking up in active rounds directly
        let active_rounds = app_state.chat.active_rounds.lock().await;
        if let Some(round_state) = active_rounds.get(&message_id) {
            let mut cancelled = round_state.cancelled.write().await;
            *cancelled = true;
            drop(cancelled);

            // Also mark in database
            let round_id = round_state.round_id.clone();
            drop(active_rounds);

            let repo = &*app_state.repo;
            let _ = repo
                .cancel_round(&round_id, Some("User requested cancellation"))
                .await;
        }
    }

    Ok(())
}

/// Cancel all active rounds for a session (used when closing session)
#[tauri::command]
pub async fn cancel_session_rounds(
    app_state: State<'_, OmigaAppState>,
    session_id: String,
) -> CommandResult<Vec<String>> {
    let repo = &*app_state.repo;

    // Get all active rounds for this session
    let active_rounds_db = repo.get_active_rounds(&session_id).await.map_err(|e| {
        OmigaError::Chat(ChatError::StreamError(format!(
            "Failed to get active rounds: {}",
            e
        )))
    })?;

    let mut cancelled_round_ids = Vec::new();

    for round in active_rounds_db {
        // Cancel in database
        if let Err(e) = repo.cancel_round(&round.id, Some("Session closed")).await {
            tracing::warn!("Failed to cancel round {}: {}", round.id, e);
        } else {
            cancelled_round_ids.push(round.id.clone());
        }

        // Set cancellation flag + stop tool subprocesses
        let active_rounds = app_state.chat.active_rounds.lock().await;
        if let Some(round_state) = active_rounds.get(&round.message_id) {
            round_state.round_cancel.cancel();
            let mut cancelled = round_state.cancelled.write().await;
            *cancelled = true;
        }
    }

    // Clean up in-memory session cache — shut down remote env connections first
    {
        let env_store = {
            let sessions = app_state.chat.sessions.read().await;
            sessions.get(&session_id).map(|s| s.env_store.clone())
        };
        if let Some(store) = env_store {
            store.shutdown().await;
        }
        let mut sessions = app_state.chat.sessions.write().await;
        sessions.remove(&session_id);
    }

    app_state
        .permission_manager
        .remove_session_composer_stance(&session_id)
        .await;

    cancel_permission_tool_waiters_for_session(
        &app_state.chat.permission_tool_waiters,
        &session_id,
    )
    .await;

    Ok(cancelled_round_ids)
}

/// Return the list of files the AI wrote or edited during a session.
///
/// The registry is in-memory only (cleared when the session is unloaded) and is
/// populated by the sequential tool execution loop whenever `file_write` or
/// `file_edit` completes successfully.
#[tauri::command]
pub async fn get_session_artifacts(
    session_id: String,
    app_state: State<'_, Arc<OmigaAppState>>,
) -> Result<Vec<crate::domain::session::artifacts::ArtifactEntry>, String> {
    let sessions = app_state.chat.sessions.read().await;
    if let Some(runtime) = sessions.get(&session_id) {
        Ok(runtime.artifact_registry.list())
    } else {
        Ok(vec![])
    }
}

/// Export a session's messages as a Markdown string.
///
/// Fetches all messages for the given session from the database and formats
/// them as Markdown. System messages are skipped. Tool results longer than
/// 500 characters are truncated. The returned string can be saved as a `.md`
/// file by the frontend.
#[tauri::command]
pub async fn export_session_markdown(
    app_state: State<'_, OmigaAppState>,
    session_id: String,
) -> Result<String, String> {
    let repo = &*app_state.repo;

    let records = sqlx::query_as::<_, crate::domain::persistence::MessageRecord>(
        r#"
        SELECT id, session_id, role, content, tool_calls, tool_call_id,
               token_usage_json, reasoning_content, follow_up_suggestions_json,
               turn_summary, created_at
        FROM messages
        WHERE session_id = ?
        ORDER BY created_at ASC, id ASC
        "#,
    )
    .bind(&session_id)
    .fetch_all(repo.pool())
    .await
    .map_err(|e| format!("Failed to load messages: {e}"))?;

    let date = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let mut md = format!(
        "# Session Export\n\n> Exported from Omiga on {date}\n\n---\n\n"
    );

    for record in &records {
        match record.role.as_str() {
            "system" => continue,
            "user" => {
                // Wrap in a fenced block to prevent user content from injecting
                // Markdown headings or HTML into the exported document structure.
                md.push_str("## User\n\n```\n");
                md.push_str(&record.content.replace("```", "` ` `"));
                md.push_str("\n```\n\n");
            }
            "assistant" => {
                md.push_str("## Assistant\n\n");
                if !record.content.is_empty() {
                    md.push_str(&record.content);
                    md.push('\n');
                }
                if let Some(tc_json) = &record.tool_calls {
                    if let Ok(calls) =
                        serde_json::from_str::<Vec<serde_json::Value>>(tc_json)
                    {
                        for call in &calls {
                            // Allowlist characters in tool names — they are internal identifiers.
                            let raw_name = call
                                .get("name")
                                .and_then(|v| v.as_str())
                                .unwrap_or("unknown_tool");
                            let name: String = raw_name
                                .chars()
                                .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
                                .collect();
                            md.push_str(&format!("\n**Tool call:** `{name}`\n"));
                        }
                    }
                }
                md.push('\n');
            }
            "tool" => {
                let output = &record.content;
                let truncated = if output.chars().count() > 500 {
                    let s: String = output.chars().take(500).collect();
                    format!("{s}\n\n*[output truncated]*")
                } else {
                    output.clone()
                };
                // Tool output is already wrapped in a fenced code block — safe.
                let safe = truncated.replace("```", "` ` `");
                md.push_str("**Tool result:**\n\n```\n");
                md.push_str(&safe);
                md.push_str("\n```\n\n");
            }
            // Skip unknown roles entirely rather than interpolating them as headings.
            _ => continue,
        }
    }

    Ok(md)
}
