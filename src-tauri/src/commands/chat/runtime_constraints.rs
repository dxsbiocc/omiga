use super::llm_bridge::{completed_to_tool_calls, tool_calls_json_opt};
use super::orchestration::spawn_chat_indexing;
use super::permissions::{execute_ask_user_question_interactive, AskUserQuestionExecution};
use super::tool_output::persist_session_tool_state;
use super::turn::{
    emit_post_turn_meta_then_complete, persist_and_emit_turn_token_usage, spawn_memory_sync,
    stream_llm_response_with_cancel, MemorySyncRequest, PostTurnCompletionRequest,
    StreamLlmRequest,
};
use crate::domain::chat_state::{AskUserWaiter, PendingToolCall, SessionRuntimeState};
use crate::domain::persistence::NewMessageRecord;
use crate::domain::session::ToolCall;
use crate::errors::OmigaError;
use crate::infrastructure::streaming::StreamOutputItem;
use crate::llm::{LlmClient, LlmMessage};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tauri::{AppHandle, Emitter};
use tokio::sync::{Mutex, RwLock};

pub(super) fn emit_buffered_assistant_text(app: &AppHandle, message_id: &str, text: &str) {
    if text.is_empty() {
        return;
    }
    let _ = app.emit(
        &format!("chat-stream-{}", message_id),
        &StreamOutputItem::Text(text.to_string()),
    );
}

pub(super) async fn emit_runtime_constraint_metadata(
    app: &AppHandle,
    repo: &Arc<crate::domain::persistence::SessionRepository>,
    session_id: &str,
    round_id: &str,
    message_id: &str,
    key: &str,
    payload: serde_json::Value,
) {
    let payload_string = payload.to_string();
    let _ = app.emit(
        &format!("chat-stream-{}", message_id),
        &StreamOutputItem::Metadata {
            key: key.to_string(),
            value: payload_string.clone(),
        },
    );
    let constraint_id = payload
        .get("id")
        .and_then(|v| v.as_str())
        .or_else(|| payload.get("constraint_id").and_then(|v| v.as_str()));
    if let Err(e) = repo
        .append_runtime_constraint_event(
            session_id,
            round_id,
            message_id,
            key,
            constraint_id,
            &payload_string,
        )
        .await
    {
        tracing::warn!("Failed to persist runtime constraint metadata: {}", e);
    }
}

pub(super) struct RuntimeConstraintBlockRequest<'a> {
    pub(super) app: &'a AppHandle,
    pub(super) client: &'a dyn LlmClient,
    pub(super) repo: Arc<crate::domain::persistence::SessionRepository>,
    pub(super) sessions: &'a Arc<RwLock<HashMap<String, SessionRuntimeState>>>,
    pub(super) session_id: &'a str,
    pub(super) round_id: &'a str,
    pub(super) message_id: &'a str,
    pub(super) user_message: &'a str,
    pub(super) assistant_text: &'a str,
    pub(super) assistant_reasoning: &'a str,
    pub(super) tool_calls: &'a [(String, String, String)],
    pub(super) block: &'a crate::domain::runtime_constraints::ConstraintToolBlock,
    pub(super) tool_results_dir: &'a Path,
    pub(super) ask_user_waiters: Arc<Mutex<HashMap<String, AskUserWaiter>>>,
    pub(super) cancel_flag: Arc<RwLock<bool>>,
    pub(super) preflight_skip_turn_summary: bool,
    pub(super) turn_token_usage: &'a Option<crate::llm::TokenUsage>,
    pub(super) provider_name: &'a str,
    pub(super) persist_original_assistant: bool,
}

pub(super) async fn handle_runtime_constraint_block_main(
    request: RuntimeConstraintBlockRequest<'_>,
) {
    let RuntimeConstraintBlockRequest {
        app,
        client,
        repo,
        sessions,
        session_id,
        round_id,
        message_id,
        user_message,
        assistant_text,
        assistant_reasoning,
        tool_calls,
        block,
        tool_results_dir,
        ask_user_waiters,
        cancel_flag,
        preflight_skip_turn_summary,
        turn_token_usage,
        provider_name,
        persist_original_assistant,
    } = request;
    let assistant_msg_id = uuid::Uuid::new_v4().to_string();
    let tool_calls_json = tool_calls_json_opt(tool_calls);
    let reasoning_save = (!assistant_reasoning.is_empty()).then_some(assistant_reasoning);
    if persist_original_assistant {
        if let Err(e) = repo
            .save_message(NewMessageRecord {
                id: &assistant_msg_id,
                session_id,
                role: "assistant",
                content: assistant_text,
                tool_calls: tool_calls_json.as_deref(),
                tool_call_id: None,
                token_usage_json: None,
                reasoning_content: reasoning_save,
                follow_up_suggestions_json: None,
                turn_summary: None,
            })
            .await
        {
            tracing::warn!(
                "Failed to save assistant message before runtime constraint block: {}",
                e
            );
        }

        {
            let mut sessions_guard = sessions.write().await;
            if let Some(runtime) = sessions_guard.get_mut(session_id) {
                let tc = completed_to_tool_calls(tool_calls);
                let rc = (!assistant_reasoning.is_empty()).then(|| assistant_reasoning.to_string());
                runtime
                    .session
                    .add_assistant_message_with_tools(assistant_text, tc, rc);
            }
        }

        let blocked_batch: Vec<(String, String, bool, Option<String>)> = tool_calls
            .iter()
            .map(|(id, _name, _arguments)| {
                (
                    id.clone(),
                    block.tool_result_message.to_string(),
                    true,
                    None,
                )
            })
            .collect();
        if let Err(e) = repo
            .save_tool_results_batch(session_id, &blocked_batch)
            .await
        {
            tracing::warn!(
                "Failed to save runtime-constraint blocked tool results batch: {}",
                e
            );
        }

        {
            let mut sessions_guard = sessions.write().await;
            if let Some(runtime) = sessions_guard.get_mut(session_id) {
                for (tool_use_id, tool_name, arguments) in tool_calls {
                    runtime.session.add_tool_result_with_error(
                        tool_use_id.clone(),
                        block.tool_result_message.to_string(),
                        Some(true),
                    );
                    let _ = app.emit(
                        &format!("chat-stream-{}", message_id),
                        &StreamOutputItem::ToolResult {
                            tool_use_id: tool_use_id.clone(),
                            name: tool_name.clone(),
                            input: arguments.clone(),
                            output: block.tool_result_message.to_string(),
                            is_error: true,
                        },
                    );
                }
            }
        }
    }

    let mut last_assistant_id = assistant_msg_id;
    let mut final_response = block.assistant_response.clone();

    if let Some(ref question_args) = block.interactive_question {
        let tool_use_id = format!("constraint-ask-user-{}", uuid::Uuid::new_v4());
        let tool_name = "ask_user_question".to_string();
        let ask_arguments = match serde_json::to_string(question_args) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("Failed to serialize runtime clarification question: {}", e);
                String::new()
            }
        };

        if !ask_arguments.is_empty() {
            let ask_assistant_id = uuid::Uuid::new_v4().to_string();
            let ask_tool_call = ToolCall {
                id: tool_use_id.clone(),
                name: tool_name.clone(),
                arguments: ask_arguments.clone(),
            };
            let ask_tool_calls = vec![ask_tool_call.clone()];

            let ask_tool_calls_json = serde_json::to_string(&ask_tool_calls).ok();
            if let Err(e) = repo
                .save_message(NewMessageRecord {
                    id: &ask_assistant_id,
                    session_id,
                    role: "assistant",
                    content: "",
                    tool_calls: ask_tool_calls_json.as_deref(),
                    tool_call_id: None,
                    token_usage_json: None,
                    reasoning_content: None,
                    follow_up_suggestions_json: None,
                    turn_summary: None,
                })
                .await
            {
                tracing::warn!(
                    "Failed to save runtime clarification assistant message: {}",
                    e
                );
            }
            {
                let mut sessions_guard = sessions.write().await;
                if let Some(runtime) = sessions_guard.get_mut(session_id) {
                    runtime.session.add_assistant_message_with_tools(
                        String::new(),
                        Some(ask_tool_calls),
                        None,
                    );
                }
            }

            let (returned_tool_id, output, is_error) =
                execute_ask_user_question_interactive(AskUserQuestionExecution {
                    tool_use_id: tool_use_id.clone(),
                    tool_name,
                    arguments: ask_arguments,
                    app: app.clone(),
                    message_id: message_id.to_string(),
                    session_id: session_id.to_string(),
                    tool_results_dir,
                    waiters: ask_user_waiters,
                    cancel_flag: Some(cancel_flag),
                })
                .await;

            if let Err(e) = repo
                .save_tool_results_batch(
                    session_id,
                    &[(returned_tool_id.clone(), output.clone(), is_error, None)],
                )
                .await
            {
                tracing::warn!("Failed to save runtime clarification tool result: {}", e);
            }
            {
                let mut sessions_guard = sessions.write().await;
                if let Some(runtime) = sessions_guard.get_mut(session_id) {
                    runtime.session.add_tool_result_with_error(
                        returned_tool_id,
                        output,
                        Some(is_error),
                    );
                }
            }

            last_assistant_id = ask_assistant_id;

            if !is_error {
                if let Some(ref post_answer_response) = block.post_answer_response {
                    final_response = post_answer_response.clone();
                    let follow_up_id = uuid::Uuid::new_v4().to_string();
                    if let Err(e) = repo
                        .save_message(NewMessageRecord {
                            id: &follow_up_id,
                            session_id,
                            role: "assistant",
                            content: post_answer_response,
                            tool_calls: None,
                            tool_call_id: None,
                            token_usage_json: None,
                            reasoning_content: None,
                            follow_up_suggestions_json: None,
                            turn_summary: None,
                        })
                        .await
                    {
                        tracing::warn!(
                            "Failed to save runtime clarification follow-up message: {}",
                            e
                        );
                    }
                    {
                        let mut sessions_guard = sessions.write().await;
                        if let Some(runtime) = sessions_guard.get_mut(session_id) {
                            runtime
                                .session
                                .add_assistant_message(post_answer_response.clone());
                        }
                    }
                    let _ = app.emit(
                        &format!("chat-stream-{}", message_id),
                        &StreamOutputItem::Text(format!("\n\n{}", post_answer_response)),
                    );
                    last_assistant_id = follow_up_id;
                }
            }
        }
    } else {
        let _ = app.emit(
            &format!("chat-stream-{}", message_id),
            &StreamOutputItem::Text(format!("\n\n{}", block.assistant_response)),
        );
        let clarification_id = uuid::Uuid::new_v4().to_string();
        if let Err(e) = repo
            .save_message(NewMessageRecord {
                id: &clarification_id,
                session_id,
                role: "assistant",
                content: &block.assistant_response,
                tool_calls: None,
                tool_call_id: None,
                token_usage_json: None,
                reasoning_content: None,
                follow_up_suggestions_json: None,
                turn_summary: None,
            })
            .await
        {
            tracing::warn!(
                "Failed to save runtime-constraint clarification message: {}",
                e
            );
        }
        {
            let mut sessions_guard = sessions.write().await;
            if let Some(runtime) = sessions_guard.get_mut(session_id) {
                runtime
                    .session
                    .add_assistant_message(block.assistant_response.clone());
            }
        }
        last_assistant_id = clarification_id;
    }

    persist_session_tool_state(sessions, &repo, session_id).await;

    if let Err(e) = repo
        .complete_round(round_id, Some(&last_assistant_id))
        .await
    {
        tracing::warn!(
            "Failed to complete round after runtime constraint block: {}",
            e
        );
    }

    persist_and_emit_turn_token_usage(
        app,
        &repo,
        &last_assistant_id,
        message_id,
        turn_token_usage,
        provider_name,
    )
    .await;
    spawn_memory_sync(MemorySyncRequest {
        app,
        sessions,
        repo: &repo,
        session_id,
        client,
        user_message,
        assistant_reply: &final_response,
        allow_long_term_promotion: false,
    });
    emit_post_turn_meta_then_complete(PostTurnCompletionRequest {
        app,
        session_id,
        stream_message_id: message_id,
        assistant_message_id: &last_assistant_id,
        client,
        final_reply: &final_response,
        skip_summary: preflight_skip_turn_summary,
        skip_follow_up: false,
        user_request: user_message,
        suggestions_reply: &final_response,
        repo: repo.clone(),
    })
    .await;
    spawn_chat_indexing(app, sessions, &repo, session_id);
}

pub(super) struct PostResponseRetryRequest<'a> {
    pub client: &'a dyn LlmClient,
    pub app: &'a AppHandle,
    pub message_id: &'a str,
    pub round_id: &'a str,
    pub base_messages: &'a [LlmMessage],
    pub instruction: &'a str,
    pub pending_tools: &'a Arc<Mutex<HashMap<String, PendingToolCall>>>,
    pub cancel_flag: &'a Arc<RwLock<bool>>,
    pub repo: Arc<crate::domain::persistence::SessionRepository>,
}

pub(super) async fn run_post_response_retry_text_only(
    request: PostResponseRetryRequest<'_>,
) -> Result<(String, String, Option<crate::llm::TokenUsage>), OmigaError> {
    let PostResponseRetryRequest {
        client,
        app,
        message_id,
        round_id,
        base_messages,
        instruction,
        pending_tools,
        cancel_flag,
        repo,
    } = request;
    let mut retry_messages = base_messages.to_vec();
    retry_messages.push(LlmMessage::system(format!(
        "## Runtime validator correction\n{}",
        instruction
    )));
    let (tool_calls, text, reasoning, cancelled, usage) =
        stream_llm_response_with_cancel(StreamLlmRequest {
            client,
            app,
            message_id,
            round_id,
            messages: &retry_messages,
            tools: &[],
            emit_text_chunks: false,
            pending_tools,
            cancel_flag,
            repo,
        })
        .await?;

    if cancelled {
        return Ok((String::new(), String::new(), usage));
    }
    if !tool_calls.is_empty() {
        tracing::warn!(
            "Runtime post-response retry unexpectedly produced tool calls; ignoring them."
        );
    }
    Ok((text, reasoning, usage))
}
