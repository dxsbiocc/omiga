use crate::domain::chat_state::{PendingToolCall, SessionRuntimeState};
use crate::domain::session::MessageTokenUsage;
use crate::domain::tools::ToolSchema;
use crate::errors::{ApiError, ChatError, OmigaError};
use crate::infrastructure::streaming::StreamOutputItem;
use crate::llm::{LlmClient, LlmMessage, LlmStreamChunk, TokenUsage};
use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use tauri::{AppHandle, Emitter};
use tokio::sync::{Mutex, RwLock};

#[derive(Debug, Clone, Serialize)]
pub(super) struct ActivityOperationPayload {
    pub session_id: String,
    pub operation_id: String,
    pub label: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

pub(super) fn emit_activity_operation(
    app: &AppHandle,
    session_id: &str,
    operation_id: &str,
    label: &str,
    status: &str,
    detail: Option<String>,
) {
    let _ = app.emit(
        "omiga-activity-step",
        ActivityOperationPayload {
            session_id: session_id.to_string(),
            operation_id: operation_id.to_string(),
            label: label.to_string(),
            status: status.to_string(),
            detail,
        },
    );
}

/// Emit full `ToolUse` and append to `completed_tool_calls` when a tool block ends.
/// Call when `BlockStop` fires, when a new `ToolStart` supersedes the previous tool, or when the stream ends without `BlockStop` (provider quirk).
pub(super) async fn finalize_pending_tool_by_id(
    app: &AppHandle,
    message_id: &str,
    pending_tools: &Arc<Mutex<HashMap<String, PendingToolCall>>>,
    id: &str,
    completed_tool_calls: &mut Vec<(String, String, String)>,
) -> bool {
    let tool = {
        let mut pending = pending_tools.lock().await;
        pending.remove(id)
    };
    let Some(tool) = tool else {
        return false;
    };
    let args = tool.arguments.join("");
    completed_tool_calls.push((tool.id.clone(), tool.name.clone(), args.clone()));
    let _ = app.emit(
        &format!("chat-stream-{}", message_id),
        &StreamOutputItem::ToolUse {
            id: tool.id.clone(),
            name: tool.name.clone(),
            arguments: args,
        },
    );
    true
}

/// Merge per-request usage into a running total for one user turn (multi tool-round).
pub(super) fn merge_turn_token_usage(acc: &mut Option<TokenUsage>, stream: Option<TokenUsage>) {
    let Some(s) = stream else {
        return;
    };
    let t = acc.get_or_insert(TokenUsage::default());
    t.prompt_tokens = t.prompt_tokens.saturating_add(s.prompt_tokens);
    t.completion_tokens = t.completion_tokens.saturating_add(s.completion_tokens);
    t.total_tokens = t.prompt_tokens.saturating_add(t.completion_tokens);
}

pub(super) struct StreamLlmRequest<'a> {
    pub client: &'a dyn LlmClient,
    pub app: &'a AppHandle,
    pub message_id: &'a str,
    pub round_id: &'a str,
    pub messages: &'a [LlmMessage],
    pub tools: &'a [ToolSchema],
    pub emit_text_chunks: bool,
    pub pending_tools: &'a Arc<Mutex<HashMap<String, PendingToolCall>>>,
    pub cancel_flag: &'a Arc<RwLock<bool>>,
    pub repo: Arc<crate::domain::persistence::SessionRepository>,
}

const MAX_RETRIES: u32 = 3;
const BASE_BACKOFF_SECS: u64 = 1;

fn is_retryable(err: &ApiError) -> (bool, u64) {
    match err {
        ApiError::RateLimited { retry_after } => (true, (*retry_after).max(BASE_BACKOFF_SECS)),
        _ => (false, 0),
    }
}

/// Connect to the LLM with exponential backoff retry on rate-limit / overload errors.
async fn connect_with_retry(
    client: &dyn LlmClient,
    messages: Vec<LlmMessage>,
    tools: Vec<ToolSchema>,
) -> Result<Pin<Box<dyn futures::Stream<Item = Result<LlmStreamChunk, ApiError>> + Send>>, OmigaError>
{
    let mut attempt = 0u32;
    loop {
        match client
            .send_message_streaming(messages.clone(), tools.clone())
            .await
        {
            Ok(stream) => return Ok(stream),
            Err(e) => {
                let (retryable, hint_secs) = is_retryable(&e);
                if retryable && attempt < MAX_RETRIES {
                    let backoff = hint_secs.max(BASE_BACKOFF_SECS << attempt);
                    tracing::warn!(
                        target: "omiga::chat",
                        "API rate limited, retrying in {}s (attempt {}/{})",
                        backoff, attempt + 1, MAX_RETRIES
                    );
                    tokio::time::sleep(tokio::time::Duration::from_secs(backoff)).await;
                    attempt += 1;
                } else {
                    return Err(OmigaError::Chat(ChatError::StreamError(e.to_string())));
                }
            }
        }
    }
}

/// Stream LLM response and collect tool calls with cancellation support
/// Returns: (tool_calls, assistant_text, reasoning_content, was_cancelled, usage_this_request)
pub(super) async fn stream_llm_response_with_cancel(
    request: StreamLlmRequest<'_>,
) -> Result<
    (
        Vec<(String, String, String)>,
        String,
        String,
        bool,
        Option<TokenUsage>,
    ),
    OmigaError,
> {
    use futures::StreamExt;

    let stream = connect_with_retry(
        request.client,
        request.messages.to_vec(),
        request.tools.to_vec(),
    )
    .await?;

    let mut stream = stream;
    let mut assistant_text = String::new();
    let mut reasoning_content = String::new();
    let mut completed_tool_calls: Vec<(String, String, String)> = Vec::new();
    let mut current_tool_id: Option<String> = None;
    let mut was_cancelled = false;
    let mut usage_this_request: Option<TokenUsage> = None;

    // Mark round as partial after receiving first chunk
    let mut marked_partial = false;

    while let Some(result) = stream.next().await {
        // Check cancellation flag
        if *request.cancel_flag.read().await {
            was_cancelled = true;
            // Mark round as cancelled in database
            let _ = request
                .repo
                .cancel_round(request.round_id, Some("User cancelled"))
                .await;
            break;
        }

        match result {
            Ok(chunk) => match chunk {
                LlmStreamChunk::Text(text) => {
                    if !marked_partial && !text.is_empty() {
                        // Mark as partial in database
                        let _ = request
                            .repo
                            .mark_round_partial(request.round_id, None)
                            .await;
                        marked_partial = true;
                    }
                    assistant_text.push_str(&text);
                    if request.emit_text_chunks {
                        let _ = request.app.emit(
                            &format!("chat-stream-{}", request.message_id),
                            &StreamOutputItem::Text(text),
                        );
                    }
                }
                LlmStreamChunk::ReasoningContent(text) => {
                    reasoning_content.push_str(&text);
                    let _ = request.app.emit(
                        &format!("chat-stream-{}", request.message_id),
                        &StreamOutputItem::Thinking(text),
                    );
                }
                LlmStreamChunk::ToolStart { id, name } => {
                    // Some streams start the next tool without BlockStop; finalize the previous one.
                    if let Some(prev_id) = current_tool_id.take() {
                        if prev_id != id {
                            let _ = finalize_pending_tool_by_id(
                                request.app,
                                request.message_id,
                                request.pending_tools,
                                &prev_id,
                                &mut completed_tool_calls,
                            )
                            .await;
                        }
                    }
                    let mut pending = request.pending_tools.lock().await;
                    pending.insert(
                        id.clone(),
                        PendingToolCall {
                            id: id.clone(),
                            name: name.clone(),
                            arguments: Vec::new(),
                        },
                    );
                    current_tool_id = Some(id.clone());

                    let _ = request.app.emit(
                        &format!("chat-stream-{}", request.message_id),
                        &StreamOutputItem::ToolUse {
                            id: id.clone(),
                            name: name.clone(),
                            arguments: String::new(),
                        },
                    );
                }
                LlmStreamChunk::ToolArguments(json) => {
                    // Collect JSON fragments
                    if let Some(ref id) = current_tool_id {
                        let mut pending = request.pending_tools.lock().await;
                        if let Some(tool) = pending.get_mut(id) {
                            tool.arguments.push(json);
                        }
                    }
                }
                LlmStreamChunk::BlockStop => {
                    if let Some(id) = current_tool_id.take() {
                        let _ = finalize_pending_tool_by_id(
                            request.app,
                            request.message_id,
                            request.pending_tools,
                            &id,
                            &mut completed_tool_calls,
                        )
                        .await;
                    }
                }
                LlmStreamChunk::Usage(u) => {
                    usage_this_request = Some(u);
                }
                LlmStreamChunk::Stop { stop_reason: _ } => break,
                _ => {}
            },
            Err(e) => {
                return Err(OmigaError::Chat(ChatError::StreamError(e.to_string())));
            }
        }
    }

    // Stream ended without BlockStop for the last tool (e.g. OpenAI sends [DONE] before finish_reason in some buffers).
    if !was_cancelled {
        let leftover_ids: Vec<String> = {
            let pending = request.pending_tools.lock().await;
            pending.keys().cloned().collect()
        };
        for lid in leftover_ids {
            let _ = finalize_pending_tool_by_id(
                request.app,
                request.message_id,
                request.pending_tools,
                &lid,
                &mut completed_tool_calls,
            )
            .await;
        }
    }

    Ok((
        completed_tool_calls,
        assistant_text,
        reasoning_content,
        was_cancelled,
        usage_this_request,
    ))
}

/// Persist aggregated main-agent token usage on the final assistant DB row for this turn, then emit for live UI.
pub(super) async fn persist_and_emit_turn_token_usage(
    app: &AppHandle,
    repo: &Arc<crate::domain::persistence::SessionRepository>,
    last_assistant_message_id: &str,
    stream_message_id: &str,
    usage: &Option<TokenUsage>,
    provider: &str,
) {
    let Some(u) = usage else {
        return;
    };
    if u.prompt_tokens == 0 && u.completion_tokens == 0 {
        return;
    }
    let total = if u.total_tokens > 0 {
        u.total_tokens
    } else {
        u.prompt_tokens.saturating_add(u.completion_tokens)
    };
    let payload = MessageTokenUsage {
        input: u.prompt_tokens,
        output: u.completion_tokens,
        total: Some(total),
        provider: Some(provider.to_string()),
    };
    let json = match serde_json::to_string(&payload) {
        Ok(s) => Some(s),
        Err(e) => {
            tracing::warn!(target: "omiga::chat", "token usage json: {}", e);
            None
        }
    };
    if let Some(ref j) = json {
        let r = &**repo;
        if let Err(e) = r
            .update_message_token_usage(last_assistant_message_id, Some(j.as_str()))
            .await
        {
            tracing::warn!("Failed to persist token usage on message: {}", e);
        }
    }
    let _ = app.emit(
        &format!("chat-stream-{}", stream_message_id),
        &StreamOutputItem::TokenUsage {
            prompt_tokens: u.prompt_tokens,
            completion_tokens: u.completion_tokens,
            total_tokens: total,
            provider: provider.to_string(),
        },
    );
}

pub(super) struct MemorySyncRequest<'a> {
    pub app: &'a AppHandle,
    pub sessions: &'a Arc<RwLock<HashMap<String, SessionRuntimeState>>>,
    pub repo: &'a Arc<crate::domain::persistence::SessionRepository>,
    pub session_id: &'a str,
    pub client: &'a dyn LlmClient,
    pub user_message: &'a str,
    pub assistant_reply: &'a str,
    pub allow_long_term_promotion: bool,
}

/// Every N turns, distill current working memory into a `SessionSummary` long-term entry.
const SESSION_SUMMARY_INTERVAL: u32 = 15;

pub(super) async fn sync_memory_layers_after_turn(request: MemorySyncRequest<'_>) {
    let project_root = {
        let sessions_guard = request.sessions.read().await;
        sessions_guard
            .get(request.session_id)
            .map(|runtime| super::resolve_session_project_root(&runtime.session.project_path))
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
    };

    match crate::domain::memory::working_memory::sync_after_turn(
        request.repo,
        request.session_id,
        request.client,
        request.user_message,
        request.assistant_reply,
    )
    .await
    {
        Ok(state) => {
            if !state.dirty {
                let op_id = format!("memory-sync-{}", uuid::Uuid::new_v4());
                emit_activity_operation(
                    request.app,
                    request.session_id,
                    &op_id,
                    "更新工作记忆",
                    "done",
                    Some("已提炼并更新 session scratchpad".to_string()),
                );
            }

            if !request.allow_long_term_promotion {
                return;
            }

            // Load config once for both promotion and session summary.
            let config = match crate::domain::memory::load_resolved_config(&project_root).await {
                Ok(cfg) => cfg,
                Err(e) => {
                    tracing::warn!(target: "omiga::working_memory", "load_resolved_config: {}", e);
                    return;
                }
            };

            // Promote high-signal working memory items to long-term.
            let candidate_count = crate::domain::memory::long_term::promotion_candidate_count(
                request.session_id,
                &state,
            );
            if candidate_count > 0 {
                let op_id = format!("memory-promote-{}", uuid::Uuid::new_v4());
                emit_activity_operation(
                    request.app,
                    request.session_id,
                    &op_id,
                    "晋升长期记忆",
                    "running",
                    Some(format!("准备晋升 {} 条候选摘要", candidate_count)),
                );
                let promoted = crate::domain::memory::long_term::promote_from_working_memory(
                    &config,
                    &project_root,
                    request.session_id,
                    &state,
                )
                .await;
                emit_activity_operation(
                    request.app,
                    request.session_id,
                    &op_id,
                    "晋升长期记忆",
                    "done",
                    Some(format!("已晋升 {} 条长期记忆", promoted)),
                );
            }

            // Every SESSION_SUMMARY_INTERVAL turns, snapshot working memory as a session summary.
            if state.user_turn_count > 0
                && state.user_turn_count % SESSION_SUMMARY_INTERVAL == 0
            {
                let lt_path = config.long_term_path(&project_root);
                maybe_archive_session_summary(
                    request.app,
                    request.session_id,
                    &lt_path,
                    &state,
                )
                .await;
            }
        }
        Err(e) => {
            tracing::warn!(target: "omiga::working_memory", "sync_after_turn: {}", e);
            let op_id = format!("memory-sync-{}", uuid::Uuid::new_v4());
            emit_activity_operation(
                request.app,
                request.session_id,
                &op_id,
                "更新工作记忆",
                "error",
                Some(e),
            );
        }
    }
}

/// Archive the current working memory state as a `SessionSummary` long-term entry.
async fn maybe_archive_session_summary(
    app: &AppHandle,
    session_id: &str,
    lt_path: &std::path::Path,
    state: &crate::domain::memory::working_memory::WorkingMemoryState,
) {
    let op_id = format!("memory-summary-{}", uuid::Uuid::new_v4());
    emit_activity_operation(
        app,
        session_id,
        &op_id,
        "归档会话摘要",
        "running",
        Some("正在提炼本次会话精华…".to_string()),
    );
    match crate::domain::memory::long_term::create_session_summary(lt_path, session_id, state)
        .await
    {
        Some(_) => emit_activity_operation(
            app,
            session_id,
            &op_id,
            "归档会话摘要",
            "done",
            Some("已归档会话摘要到长期记忆".to_string()),
        ),
        None => emit_activity_operation(
            app,
            session_id,
            &op_id,
            "归档会话摘要",
            "done",
            Some("内容不足，跳过摘要归档".to_string()),
        ),
    }
}

/// After the visible assistant reply is finalized: optional recap (independent LLM), then follow-up chips (independent LLM), then [`StreamOutputItem::Complete`].
///
/// - `skip_summary`：跳过摘要 LLM 调用（preflight 判定为无需摘要，或本轮已通过 SendUserMessage 直接交付内容）。
/// - `suggestions_reply`：生成 follow-up suggestions 所用的文本；当本轮使用了 SendUserMessage 时传其 message 内容，
///   而非 LLM 的空壳收尾文本；其余情况与 `final_reply` 相同。
pub(super) struct PostTurnCompletionRequest<'a> {
    pub app: &'a AppHandle,
    pub stream_message_id: &'a str,
    pub assistant_message_id: &'a str,
    pub client: &'a dyn LlmClient,
    pub final_reply: &'a str,
    pub skip_summary: bool,
    pub suggestions_reply: &'a str,
    pub repo: Arc<crate::domain::persistence::SessionRepository>,
}

pub(super) async fn emit_post_turn_meta_then_complete(request: PostTurnCompletionRequest<'_>) {
    let flags = crate::domain::post_turn_settings::load_post_turn_meta_flags(&request.repo)
        .await
        .unwrap_or((true, true));
    let (summary_enabled, follow_enabled) = flags;

    // Run summary synchronously (needed before Complete so the frontend can render it)
    let summary_res = if request.skip_summary {
        Ok(None)
    } else {
        crate::domain::agents::output_formatter::run_turn_summary_pass(
            request.client,
            request.final_reply,
            summary_enabled,
        )
        .await
    };

    let summary_text = match summary_res {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(target: "omiga::post_turn", "post-turn summary: {}", e);
            None
        }
    };
    let _ = request.app.emit(
        &format!("chat-stream-{}", request.stream_message_id),
        &StreamOutputItem::TurnSummary {
            text: summary_text.clone(),
        },
    );

    if let Some(summary) = summary_text.as_deref() {
        if let Err(e) = request
            .repo
            .update_message_turn_summary(request.assistant_message_id, Some(summary))
            .await
        {
            tracing::warn!("Failed to persist turn summary: {}", e);
        }
    }

    if !follow_enabled {
        let _ = request.app.emit(
            &format!("chat-stream-{}", request.stream_message_id),
            &StreamOutputItem::Complete,
        );
        return;
    }

    // Emit indicator that suggestions are being generated in background
    let _ = request.app.emit(
        &format!("chat-stream-{}", request.stream_message_id),
        &StreamOutputItem::SuggestionsGenerating,
    );

    // Spawn background task for follow-up suggestions so they don't block the frontend
    let app_bg = request.app.clone();
    let stream_id = request.stream_message_id.to_string();
    let assistant_id = request.assistant_message_id.to_string();
    let suggestions_text = request.suggestions_reply.to_string();
    let repo = request.repo.clone();
    // Clone client config for background use
    let client_config = request.client.config().clone();
    tokio::spawn(async move {
        let bg_client = match crate::llm::create_client(client_config) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(target: "omiga::follow_up", "failed to create bg client: {}", e);
                let _ = app_bg.emit(
                    &format!("chat-stream-{}", stream_id),
                    &StreamOutputItem::SuggestionsComplete {
                        generated: false,
                        error: Some(e.to_string()),
                    },
                );
                let _ = app_bg.emit(
                    &format!("chat-stream-{}", stream_id),
                    &StreamOutputItem::Complete,
                );
                return;
            }
        };
        let follow_res = crate::domain::suggestions::generate_follow_up_suggestions(
            bg_client.as_ref(),
            &suggestions_text,
            follow_enabled,
        )
        .await;

        match follow_res {
            Ok(items) if !items.is_empty() => {
                let _ = app_bg.emit(
                    &format!("chat-stream-{}", stream_id),
                    &StreamOutputItem::FollowUpSuggestions(items.clone()),
                );
                // Persist to database
                if let Ok(json) = serde_json::to_string(&items) {
                    if let Err(e) = repo
                        .update_message_follow_up_suggestions(&assistant_id, Some(&json))
                        .await
                    {
                        tracing::warn!("Failed to persist follow-up suggestions: {}", e);
                    }
                }
                let _ = app_bg.emit(
                    &format!("chat-stream-{}", stream_id),
                    &StreamOutputItem::SuggestionsComplete {
                        generated: true,
                        error: None,
                    },
                );
                let _ = app_bg.emit(
                    &format!("chat-stream-{}", stream_id),
                    &StreamOutputItem::Complete,
                );
            }
            Ok(_) => {
                let _ = app_bg.emit(
                    &format!("chat-stream-{}", stream_id),
                    &StreamOutputItem::SuggestionsComplete {
                        generated: false,
                        error: None,
                    },
                );
                let _ = app_bg.emit(
                    &format!("chat-stream-{}", stream_id),
                    &StreamOutputItem::Complete,
                );
            }
            Err(e) => {
                tracing::warn!(target: "omiga::follow_up", "follow-up suggestions: {}", e);
                let _ = app_bg.emit(
                    &format!("chat-stream-{}", stream_id),
                    &StreamOutputItem::SuggestionsComplete {
                        generated: false,
                        error: Some(e.to_string()),
                    },
                );
                let _ = app_bg.emit(
                    &format!("chat-stream-{}", stream_id),
                    &StreamOutputItem::Complete,
                );
            }
        }
    });
}
