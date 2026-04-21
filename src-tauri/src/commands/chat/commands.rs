use super::permissions::{
    ask_user_waiter_key, cancel_ask_user_waiters_for_message,
    cancel_permission_tool_waiters_for_message, cancel_permission_tool_waiters_for_session,
};
use super::CommandResult;
use crate::app_state::OmigaAppState;
use crate::errors::{ChatError, OmigaError};
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
