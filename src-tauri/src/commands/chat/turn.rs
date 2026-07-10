use crate::domain::chat_state::{PendingToolCall, SessionRuntimeState};
use crate::domain::session::MessageTokenUsage;
use crate::domain::telemetry::{TurnMetrics, TurnMetricsRecorder};
use crate::domain::tools::{
    normalize_legacy_retrieval_tool_arguments, normalize_legacy_retrieval_tool_name, ToolSchema,
};
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
use tokio::time::{Duration, Instant};

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

pub(super) trait ChatStreamEmitter: Send + Sync {
    fn emit_stream_item(&self, message_id: &str, item: &StreamOutputItem);
}

impl ChatStreamEmitter for AppHandle {
    fn emit_stream_item(&self, message_id: &str, item: &StreamOutputItem) {
        let _ = self.emit(&format!("chat-stream-{}", message_id), item);
    }
}

#[async_trait::async_trait]
trait RoundStatusStore: Send + Sync {
    async fn mark_round_partial(&self, round_id: &str);
    async fn cancel_round(&self, round_id: &str, reason: Option<&str>);
}

struct RepositoryRoundStatusStore {
    repo: Arc<crate::domain::persistence::SessionRepository>,
}

#[async_trait::async_trait]
impl RoundStatusStore for RepositoryRoundStatusStore {
    async fn mark_round_partial(&self, round_id: &str) {
        let _ = self.repo.mark_round_partial(round_id, None).await;
    }

    async fn cancel_round(&self, round_id: &str, reason: Option<&str>) {
        let _ = self.repo.cancel_round(round_id, reason).await;
    }
}

/// Emit full `ToolUse` and append to `completed_tool_calls` when a tool block ends.
/// Call when `BlockStop` fires, when a new `ToolStart` supersedes the previous tool, or when the stream ends without `BlockStop` (provider quirk).
pub(super) async fn finalize_pending_tool_by_id(
    emitter: &dyn ChatStreamEmitter,
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
    let args =
        normalize_stream_tool_arguments(&tool.original_name, &tool.name, &tool.arguments.join(""));
    completed_tool_calls.push((tool.id.clone(), tool.name.clone(), args.clone()));
    emitter.emit_stream_item(
        message_id,
        &StreamOutputItem::ToolUse {
            id: tool.id.clone(),
            name: tool.name.clone(),
            arguments: args,
        },
    );
    true
}

fn normalize_stream_tool_name(tool_name: &str) -> String {
    normalize_legacy_retrieval_tool_name(tool_name)
}

fn normalize_stream_tool_arguments(
    original_tool_name: &str,
    normalized_tool_name: &str,
    arguments: &str,
) -> String {
    normalize_legacy_retrieval_tool_arguments(original_tool_name, normalized_tool_name, arguments)
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
    t.cache_creation_input_tokens =
        add_optional_token_count(t.cache_creation_input_tokens, s.cache_creation_input_tokens);
    t.cache_read_input_tokens =
        add_optional_token_count(t.cache_read_input_tokens, s.cache_read_input_tokens);
}

fn add_optional_token_count(current: Option<u32>, next: Option<u32>) -> Option<u32> {
    match (current, next) {
        (Some(current), Some(next)) => Some(current.saturating_add(next)),
        (Some(current), None) => Some(current),
        (None, Some(next)) => Some(next),
        (None, None) => None,
    }
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

struct StreamLlmCoreRequest<'a> {
    client: &'a dyn LlmClient,
    emitter: &'a dyn ChatStreamEmitter,
    message_id: &'a str,
    round_id: &'a str,
    messages: &'a [LlmMessage],
    tools: &'a [ToolSchema],
    emit_text_chunks: bool,
    pending_tools: &'a Arc<Mutex<HashMap<String, PendingToolCall>>>,
    cancel_flag: &'a Arc<RwLock<bool>>,
    round_status: &'a dyn RoundStatusStore,
}

const MAX_RETRIES: u32 = 3;
const BASE_BACKOFF_SECS: u64 = 1;
const STREAM_EMIT_FLUSH_INTERVAL: Duration = Duration::from_millis(30);
const STREAM_EMIT_FLUSH_BYTES: usize = 2 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VisibleChunkKind {
    Text,
    Thinking,
}

struct VisibleChunkBuffer {
    kind: Option<VisibleChunkKind>,
    text: String,
    last_flush: Instant,
}

impl VisibleChunkBuffer {
    fn new(now: Instant) -> Self {
        Self {
            kind: None,
            text: String::new(),
            last_flush: now,
        }
    }

    fn push(&mut self, kind: VisibleChunkKind, text: &str, now: Instant) -> Vec<StreamOutputItem> {
        if text.is_empty() {
            return Vec::new();
        }

        let mut emitted = Vec::new();
        if self.kind.is_some_and(|pending| pending != kind) {
            if let Some(item) = self.flush(now) {
                emitted.push(item);
            }
        }

        if self.kind.is_none() {
            self.kind = Some(kind);
        }
        self.text.push_str(text);

        if self.text.len() >= STREAM_EMIT_FLUSH_BYTES
            || now.duration_since(self.last_flush) >= STREAM_EMIT_FLUSH_INTERVAL
        {
            if let Some(item) = self.flush(now) {
                emitted.push(item);
            }
        }

        emitted
    }

    fn flush(&mut self, now: Instant) -> Option<StreamOutputItem> {
        let kind = self.kind.take()?;
        if self.text.is_empty() {
            return None;
        }
        let text = std::mem::take(&mut self.text);
        self.last_flush = now;
        Some(match kind {
            VisibleChunkKind::Text => StreamOutputItem::Text(text),
            VisibleChunkKind::Thinking => StreamOutputItem::Thinking(text),
        })
    }
}

fn emit_stream_items(
    emitter: &dyn ChatStreamEmitter,
    message_id: &str,
    items: Vec<StreamOutputItem>,
) -> bool {
    let emitted_any = !items.is_empty();
    for item in items {
        emitter.emit_stream_item(message_id, &item);
    }
    emitted_any
}

fn flush_visible_buffer(
    buffer: &mut VisibleChunkBuffer,
    emitter: &dyn ChatStreamEmitter,
    message_id: &str,
) -> bool {
    match buffer.flush(Instant::now()) {
        Some(item) => {
            emitter.emit_stream_item(message_id, &item);
            true
        }
        None => false,
    }
}

fn is_retryable(err: &ApiError) -> (bool, u64) {
    match err {
        ApiError::RateLimited { retry_after } => (true, (*retry_after).max(BASE_BACKOFF_SECS)),
        ApiError::Network { .. } | ApiError::Timeout | ApiError::Server { .. } => (true, 0),
        ApiError::Http { status, .. } if *status == 429 || *status >= 500 => (true, 0),
        _ => (false, 0),
    }
}

fn retry_backoff_secs(err: &ApiError, attempt: u32) -> Option<u64> {
    let (retryable, hint_secs) = is_retryable(err);
    if !retryable {
        return None;
    }
    match err {
        ApiError::RateLimited { .. } => Some(hint_secs.max(BASE_BACKOFF_SECS)),
        _ => Some(BASE_BACKOFF_SECS << attempt),
    }
}

fn emit_turn_metrics(
    span: &tracing::Span,
    message_id: &str,
    round_id: &str,
    metrics: &TurnMetrics,
) {
    span.in_scope(|| {
        tracing::info!(
            target: "omiga::telemetry",
            message_id = %message_id,
            round_id = %round_id,
            connect_ms = ?metrics.connect_ms,
            ttft_ms = ?metrics.ttft_ms,
            stream_total_ms = ?metrics.stream_total_ms,
            tool_calls = metrics.tool_calls,
            input_tokens = ?metrics.input_tokens,
            output_tokens = ?metrics.output_tokens,
            cache_read_tokens = ?metrics.cache_read_tokens,
            retries = metrics.retries,
            "llm_turn_metrics"
        );
    });
}

fn finish_turn_metrics(
    recorder: &mut TurnMetricsRecorder,
    span: &tracing::Span,
    message_id: &str,
    round_id: &str,
) {
    recorder.on_stream_end(Instant::now());
    let metrics = recorder.metrics();
    emit_turn_metrics(span, message_id, round_id, &metrics);
}

async fn sleep_before_retry(backoff_secs: u64) {
    #[cfg(test)]
    {
        let _ = backoff_secs;
    }

    #[cfg(not(test))]
    tokio::time::sleep(Duration::from_secs(backoff_secs)).await;
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
                if let Some(backoff) =
                    retry_backoff_secs(&e, attempt).filter(|_| attempt < MAX_RETRIES)
                {
                    tracing::warn!(
                        target: "omiga::chat",
                        "API stream connect failed, retrying in {}s (attempt {}/{})",
                        backoff, attempt + 1, MAX_RETRIES
                    );
                    sleep_before_retry(backoff).await;
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
    let round_status = RepositoryRoundStatusStore {
        repo: request.repo.clone(),
    };
    stream_llm_response_with_cancel_core(StreamLlmCoreRequest {
        client: request.client,
        emitter: request.app,
        message_id: request.message_id,
        round_id: request.round_id,
        messages: request.messages,
        tools: request.tools,
        emit_text_chunks: request.emit_text_chunks,
        pending_tools: request.pending_tools,
        cancel_flag: request.cancel_flag,
        round_status: &round_status,
    })
    .await
}

async fn stream_llm_response_with_cancel_core(
    request: StreamLlmCoreRequest<'_>,
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

    let mut stream_retry_attempt = 0u32;
    let mut metrics_recorder = TurnMetricsRecorder::new(Instant::now());
    let turn_span = tracing::info_span!(
        "llm_turn",
        message_id = %request.message_id,
        round_id = %request.round_id
    );

    'request_retry: loop {
        if *request.cancel_flag.read().await {
            request
                .round_status
                .cancel_round(request.round_id, Some("User cancelled"))
                .await;
            finish_turn_metrics(
                &mut metrics_recorder,
                &turn_span,
                request.message_id,
                request.round_id,
            );
            return Ok((Vec::new(), String::new(), String::new(), true, None));
        }

        let stream = match connect_with_retry(
            request.client,
            request.messages.to_vec(),
            request.tools.to_vec(),
        )
        .await
        {
            Ok(stream) => {
                metrics_recorder.on_connected(Instant::now());
                stream
            }
            Err(e) => {
                finish_turn_metrics(
                    &mut metrics_recorder,
                    &turn_span,
                    request.message_id,
                    request.round_id,
                );
                return Err(e);
            }
        };

        let mut stream = stream;
        let mut assistant_text = String::new();
        let mut reasoning_content = String::new();
        let mut completed_tool_calls: Vec<(String, String, String)> = Vec::new();
        let mut current_tool_id: Option<String> = None;
        let mut was_cancelled = false;
        let mut usage_this_request: Option<TokenUsage> = None;
        let mut visible_buffer = VisibleChunkBuffer::new(Instant::now());
        let mut emitted_any_visible = false;

        // Mark round as partial after receiving first text chunk for this attempt.
        let mut marked_partial = false;

        while let Some(result) = stream.next().await {
            // Check cancellation flag
            if *request.cancel_flag.read().await {
                let _ =
                    flush_visible_buffer(&mut visible_buffer, request.emitter, request.message_id);
                was_cancelled = true;
                request
                    .round_status
                    .cancel_round(request.round_id, Some("User cancelled"))
                    .await;
                break;
            }

            match result {
                Ok(chunk) => match chunk {
                    LlmStreamChunk::Text(text) => {
                        if !text.is_empty() {
                            metrics_recorder.on_first_visible(Instant::now());
                        }
                        if !marked_partial && !text.is_empty() {
                            request
                                .round_status
                                .mark_round_partial(request.round_id)
                                .await;
                            marked_partial = true;
                        }
                        assistant_text.push_str(&text);
                        if request.emit_text_chunks {
                            let items =
                                visible_buffer.push(VisibleChunkKind::Text, &text, Instant::now());
                            emitted_any_visible |=
                                emit_stream_items(request.emitter, request.message_id, items);
                        }
                    }
                    LlmStreamChunk::ReasoningContent(text) => {
                        if !text.is_empty() {
                            metrics_recorder.on_first_visible(Instant::now());
                        }
                        reasoning_content.push_str(&text);
                        let items =
                            visible_buffer.push(VisibleChunkKind::Thinking, &text, Instant::now());
                        emitted_any_visible |=
                            emit_stream_items(request.emitter, request.message_id, items);
                    }
                    LlmStreamChunk::ToolStart { id, name } => {
                        emitted_any_visible |= flush_visible_buffer(
                            &mut visible_buffer,
                            request.emitter,
                            request.message_id,
                        );
                        let original_name = name.clone();
                        let name = normalize_stream_tool_name(&name);
                        // Some streams start the next tool without BlockStop; finalize the previous one.
                        if let Some(prev_id) = current_tool_id.take() {
                            if prev_id != id {
                                emitted_any_visible |= flush_visible_buffer(
                                    &mut visible_buffer,
                                    request.emitter,
                                    request.message_id,
                                );
                                if finalize_pending_tool_by_id(
                                    request.emitter,
                                    request.message_id,
                                    request.pending_tools,
                                    &prev_id,
                                    &mut completed_tool_calls,
                                )
                                .await
                                {
                                    metrics_recorder.inc_tool();
                                }
                            }
                        }
                        let mut pending = request.pending_tools.lock().await;
                        pending.insert(
                            id.clone(),
                            PendingToolCall {
                                id: id.clone(),
                                original_name,
                                name: name.clone(),
                                arguments: Vec::new(),
                            },
                        );
                        current_tool_id = Some(id.clone());

                        request.emitter.emit_stream_item(
                            request.message_id,
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
                        emitted_any_visible |= flush_visible_buffer(
                            &mut visible_buffer,
                            request.emitter,
                            request.message_id,
                        );
                        if let Some(id) = current_tool_id.take() {
                            if finalize_pending_tool_by_id(
                                request.emitter,
                                request.message_id,
                                request.pending_tools,
                                &id,
                                &mut completed_tool_calls,
                            )
                            .await
                            {
                                metrics_recorder.inc_tool();
                            }
                        }
                    }
                    LlmStreamChunk::Usage(u) => {
                        emitted_any_visible |= flush_visible_buffer(
                            &mut visible_buffer,
                            request.emitter,
                            request.message_id,
                        );
                        metrics_recorder.set_usage(
                            u.prompt_tokens,
                            u.completion_tokens,
                            u.cache_read_input_tokens,
                        );
                        usage_this_request = Some(u);
                    }
                    LlmStreamChunk::Stop { stop_reason: _ } => {
                        let _ = flush_visible_buffer(
                            &mut visible_buffer,
                            request.emitter,
                            request.message_id,
                        );
                        break;
                    }
                    _ => {}
                },
                Err(e) => {
                    let can_retry_without_frontend_reset =
                        completed_tool_calls.is_empty() && !emitted_any_visible;
                    let retry_delay = retry_backoff_secs(&e, stream_retry_attempt).filter(|_| {
                        stream_retry_attempt < MAX_RETRIES && can_retry_without_frontend_reset
                    });

                    if let Some(backoff) = retry_delay {
                        // There is no existing reset/replace StreamOutputItem, and this task
                        // cannot add cross-file event variants. Retry only before Text/Thinking
                        // has been emitted, so the UI never has to discard partial visible output.
                        if *request.cancel_flag.read().await {
                            let _ = flush_visible_buffer(
                                &mut visible_buffer,
                                request.emitter,
                                request.message_id,
                            );
                            request
                                .round_status
                                .cancel_round(request.round_id, Some("User cancelled"))
                                .await;
                            finish_turn_metrics(
                                &mut metrics_recorder,
                                &turn_span,
                                request.message_id,
                                request.round_id,
                            );
                            return Ok((
                                completed_tool_calls,
                                assistant_text,
                                reasoning_content,
                                true,
                                usage_this_request,
                            ));
                        }
                        request.pending_tools.lock().await.clear();
                        stream_retry_attempt += 1;
                        metrics_recorder.inc_retry();
                        tracing::warn!(
                            target: "omiga::chat",
                            "LLM stream disconnected before visible output; retrying in {}s (attempt {}/{})",
                            backoff, stream_retry_attempt, MAX_RETRIES
                        );
                        sleep_before_retry(backoff).await;
                        continue 'request_retry;
                    }

                    let _ = flush_visible_buffer(
                        &mut visible_buffer,
                        request.emitter,
                        request.message_id,
                    );
                    finish_turn_metrics(
                        &mut metrics_recorder,
                        &turn_span,
                        request.message_id,
                        request.round_id,
                    );
                    return Err(OmigaError::Chat(ChatError::StreamError(e.to_string())));
                }
            }
        }

        let _ = flush_visible_buffer(&mut visible_buffer, request.emitter, request.message_id);

        // Stream ended without BlockStop for the last tool (e.g. OpenAI sends [DONE] before finish_reason in some buffers).
        if !was_cancelled {
            let leftover_ids: Vec<String> = {
                let pending = request.pending_tools.lock().await;
                pending.keys().cloned().collect()
            };
            for lid in leftover_ids {
                if finalize_pending_tool_by_id(
                    request.emitter,
                    request.message_id,
                    request.pending_tools,
                    &lid,
                    &mut completed_tool_calls,
                )
                .await
                {
                    metrics_recorder.inc_tool();
                }
            }
        }

        finish_turn_metrics(
            &mut metrics_recorder,
            &turn_span,
            request.message_id,
            request.round_id,
        );
        return Ok((
            completed_tool_calls,
            assistant_text,
            reasoning_content,
            was_cancelled,
            usage_this_request,
        ));
    }
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
    if !u.has_any_tokens() {
        return;
    }
    let payload = message_token_usage_from_llm_usage(u, provider);
    let total = payload
        .total
        .unwrap_or_else(|| payload.input.saturating_add(payload.output));
    let prompt_tokens = payload.input;
    let completion_tokens = payload.output;
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
    // Cache usage is persisted above. The live stream event shape is defined outside this task's
    // allowed files, so frontend emission remains prompt/completion/total/provider only.
    let _ = app.emit(
        &format!("chat-stream-{}", stream_message_id),
        &StreamOutputItem::TokenUsage {
            prompt_tokens,
            completion_tokens,
            total_tokens: total,
            provider: provider.to_string(),
        },
    );
}

fn message_token_usage_from_llm_usage(u: &TokenUsage, provider: &str) -> MessageTokenUsage {
    let total = if u.total_tokens > 0 {
        u.total_tokens
    } else {
        u.prompt_tokens.saturating_add(u.completion_tokens)
    };
    MessageTokenUsage {
        input: u.prompt_tokens,
        output: u.completion_tokens,
        total: Some(total),
        provider: Some(provider.to_string()),
        cache_read: u.cache_read_input_tokens,
        cache_creation: u.cache_creation_input_tokens,
    }
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

/// Owned version of MemorySyncRequest — required to move the sync work into a background task.
struct MemorySyncOwned {
    app: AppHandle,
    sessions: Arc<RwLock<HashMap<String, SessionRuntimeState>>>,
    repo: Arc<crate::domain::persistence::SessionRepository>,
    session_id: String,
    client_config: crate::llm::LlmConfig,
    user_message: String,
    assistant_reply: String,
    allow_long_term_promotion: bool,
}

/// Spawn memory sync as a fire-and-forget background task so it never blocks turn completion.
/// Activity-operation events are emitted on `omiga-activity-step` so the UI shows progress.
pub(super) fn spawn_memory_sync(request: MemorySyncRequest<'_>) {
    let owned = MemorySyncOwned {
        app: request.app.clone(),
        sessions: request.sessions.clone(),
        repo: request.repo.clone(),
        session_id: request.session_id.to_string(),
        client_config: request.client.config().clone(),
        user_message: request.user_message.to_string(),
        assistant_reply: request.assistant_reply.to_string(),
        allow_long_term_promotion: request.allow_long_term_promotion,
    };
    tokio::spawn(async move {
        let client = match crate::llm::create_client(owned.client_config) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(target: "omiga::working_memory", "bg memory sync: client error: {}", e);
                return;
            }
        };
        // Reuse the existing sync logic with the spawned client.
        sync_memory_layers_after_turn(MemorySyncRequest {
            app: &owned.app,
            sessions: &owned.sessions,
            repo: &owned.repo,
            session_id: &owned.session_id,
            client: client.as_ref(),
            user_message: &owned.user_message,
            assistant_reply: &owned.assistant_reply,
            allow_long_term_promotion: owned.allow_long_term_promotion,
        })
        .await;
    });
}

/// Periodic session-summary interval (aligns with the link's recommended 6-10 turn cadence).
const SESSION_SUMMARY_INTERVAL: u32 = 8;

pub(super) async fn sync_memory_layers_after_turn(request: MemorySyncRequest<'_>) {
    let project_root = {
        let sessions_guard = request.sessions.read().await;
        sessions_guard
            .get(request.session_id)
            .map(|runtime| super::resolve_session_project_root(&runtime.session.project_path))
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
    };

    // 30-second wall-clock cap: post-turn memory sync must never block turn completion
    // indefinitely. If the LLM is slow/hung, the heuristic inside extract_draft fires
    // first (20s), but this outer timeout is the final safety net.
    match tokio::time::timeout(
        std::time::Duration::from_secs(30),
        crate::domain::memory::working_memory::sync_after_turn(
            request.repo,
            request.session_id,
            request.client,
            request.user_message,
            request.assistant_reply,
        ),
    )
    .await
    .unwrap_or_else(|_| {
        tracing::warn!(
            target: "omiga::working_memory",
            session_id = %request.session_id,
            "sync_memory_layers_after_turn timed out (>30s); skipping memory sync this turn"
        );
        Err("timeout".to_string())
    }) {
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

            // Archive session summary on periodic interval OR on task completion signal.
            let is_periodic =
                state.user_turn_count > 0 && state.user_turn_count % SESSION_SUMMARY_INTERVAL == 0;
            let is_task_done =
                crate::domain::memory::working_memory::contains_task_completion_signal(
                    request.assistant_reply,
                );
            if is_periodic || is_task_done {
                let lt_path = config.long_term_path(&project_root);
                maybe_archive_session_summary(request.app, request.session_id, &lt_path, &state)
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

/// Archive the current working memory state as a `SessionSummary` long-term entry,
/// then update the project dossier for the active topic.
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
    match crate::domain::memory::long_term::create_session_summary(lt_path, session_id, state).await
    {
        Some(entry) => {
            emit_activity_operation(
                app,
                session_id,
                &op_id,
                "归档会话摘要",
                "done",
                Some("已归档会话摘要到长期记忆".to_string()),
            );
            // Update the project dossier for this topic in the background.
            let decisions: Vec<String> = state
                .decisions
                .iter()
                .filter(|d| d.confidence >= 0.70)
                .map(|d| d.text.clone())
                .collect();
            let beliefs: Vec<String> = state
                .working_facts
                .iter()
                .filter(|f| f.confidence >= 0.75)
                .map(|f| f.text.clone())
                .collect();
            let questions: Vec<String> = state
                .open_questions
                .iter()
                .map(|q| q.text.clone())
                .collect();
            let next_steps: Vec<String> = state.next_steps.iter().map(|s| s.text.clone()).collect();
            crate::domain::memory::dossier::update_project_dossier(
                lt_path,
                &entry.topic,
                decisions,
                beliefs,
                questions,
                next_steps,
            )
            .await;
        }
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
/// - `user_request`：用户本轮原始请求，供 follow-up 模型区分“完成但仍值得继续探索”的实质性回复。
/// - `suggestions_reply`：生成 follow-up suggestions 所用的文本；当本轮使用了 SendUserMessage 时传其 message 内容，
///   而非 LLM 的空壳收尾文本；其余情况与 `final_reply` 相同。
pub(super) struct PostTurnCompletionRequest<'a> {
    pub app: &'a AppHandle,
    pub session_id: &'a str,
    pub stream_message_id: &'a str,
    pub assistant_message_id: &'a str,
    pub client: &'a dyn LlmClient,
    pub final_reply: &'a str,
    pub skip_summary: bool,
    pub skip_follow_up: bool,
    pub user_request: &'a str,
    pub suggestions_reply: &'a str,
    pub repo: Arc<crate::domain::persistence::SessionRepository>,
}

/// Emit Complete immediately, then run all post-turn LLM work in parallel background tasks.
///
/// Previous architecture (sequential, blocking):
///   main reply → sync_memory → turn_summary → follow_up → Complete
///   (could block 60+ seconds on slow LLM or large context)
///
/// New architecture (parallel, non-blocking):
///   main reply → Complete (UI unlocked immediately)
///            ↘ [parallel background]
///              ├─ turn_summary      → emits TurnSummary event + persists
///              └─ follow_up chips   → emits FollowUpSuggestions + SuggestionsComplete
pub(super) async fn emit_post_turn_meta_then_complete(request: PostTurnCompletionRequest<'_>) {
    let flags = crate::domain::post_turn_settings::load_post_turn_meta_flags(&request.repo)
        .await
        .unwrap_or((true, true));
    let (summary_enabled, follow_enabled) = flags;
    let follow_enabled = follow_enabled && !request.skip_follow_up;

    // ── Emit Complete immediately — UI input is unlocked right here ───────────
    let _ = request.app.emit(
        &format!("chat-stream-{}", request.stream_message_id),
        &StreamOutputItem::Complete,
    );

    // Emit SuggestionsGenerating indicator so UI shows the spinner promptly.
    if follow_enabled {
        let _ = request.app.emit(
            &format!("chat-stream-{}", request.stream_message_id),
            &StreamOutputItem::SuggestionsGenerating,
        );
    }

    // ── Clone everything needed for background tasks ───────────────────────────
    let app_bg = request.app.clone();
    let session_id = request.session_id.to_string();
    let stream_id = request.stream_message_id.to_string();
    let assistant_id = request.assistant_message_id.to_string();
    let final_reply_bg = request.final_reply.to_string();
    let user_request_bg = request.user_request.to_string();
    let suggestions_text = request.suggestions_reply.to_string();
    let repo = request.repo.clone();
    let client_config = request.client.config().clone();
    let skip_summary = request.skip_summary;

    // ── Single spawn: run summary and suggestions in parallel inside ──────────
    tokio::spawn(async move {
        let summary_op_id = format!("post-turn-summary-{}", uuid::Uuid::new_v4());
        let follow_op_id = format!("post-turn-suggestions-{}", uuid::Uuid::new_v4());

        if !skip_summary && summary_enabled {
            emit_activity_operation(
                &app_bg,
                &session_id,
                &summary_op_id,
                "生成本轮要点",
                "running",
                Some("后台独立 LLM 正在提炼本轮要点".to_string()),
            );
        }
        if follow_enabled {
            emit_activity_operation(
                &app_bg,
                &session_id,
                &follow_op_id,
                "生成下一步建议",
                "running",
                Some("后台独立 LLM 正在生成下一步建议".to_string()),
            );
        }

        let bg_client = match crate::llm::create_client(client_config) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(target: "omiga::post_turn", "bg client creation failed: {}", e);
                if !skip_summary && summary_enabled {
                    emit_activity_operation(
                        &app_bg,
                        &session_id,
                        &summary_op_id,
                        "生成本轮要点",
                        "error",
                        Some(format!("创建后台 LLM 客户端失败：{e}")),
                    );
                }
                if follow_enabled {
                    emit_activity_operation(
                        &app_bg,
                        &session_id,
                        &follow_op_id,
                        "生成下一步建议",
                        "error",
                        Some(format!("创建后台 LLM 客户端失败：{e}")),
                    );
                    let _ = app_bg.emit(
                        &format!("chat-stream-{}", stream_id),
                        &StreamOutputItem::SuggestionsComplete {
                            generated: false,
                            error: Some(e.to_string()),
                        },
                    );
                }
                return;
            }
        };

        // Run turn summary and follow-up suggestions in parallel.
        tokio::join!(
            // ── Turn summary ─────────────────────────────────────────────────
            async {
                if skip_summary || !summary_enabled {
                    return;
                }
                let summary_text = match tokio::time::timeout(
                    std::time::Duration::from_secs(15),
                    crate::domain::agents::output_formatter::run_turn_summary_pass(
                        bg_client.as_ref(),
                        &final_reply_bg,
                        summary_enabled,
                    ),
                )
                .await
                {
                    Ok(Ok(v)) => v,
                    Ok(Err(e)) => {
                        tracing::warn!(target: "omiga::post_turn", "turn summary error: {}", e);
                        let _ = app_bg.emit(
                            &format!("chat-stream-{}", stream_id),
                            &StreamOutputItem::TurnSummary { text: None },
                        );
                        emit_activity_operation(
                            &app_bg,
                            &session_id,
                            &summary_op_id,
                            "生成本轮要点",
                            "error",
                            Some(format!("本轮要点生成失败：{e}")),
                        );
                        return;
                    }
                    Err(_) => {
                        tracing::warn!(target: "omiga::post_turn", "turn summary timed out");
                        let _ = app_bg.emit(
                            &format!("chat-stream-{}", stream_id),
                            &StreamOutputItem::TurnSummary { text: None },
                        );
                        emit_activity_operation(
                            &app_bg,
                            &session_id,
                            &summary_op_id,
                            "生成本轮要点",
                            "error",
                            Some("本轮要点生成超时".to_string()),
                        );
                        return;
                    }
                };
                let _ = app_bg.emit(
                    &format!("chat-stream-{}", stream_id),
                    &StreamOutputItem::TurnSummary {
                        text: summary_text.clone(),
                    },
                );
                if let Some(summary) = summary_text.as_deref() {
                    if let Err(e) = repo
                        .update_message_turn_summary(&assistant_id, Some(summary))
                        .await
                    {
                        tracing::warn!("Failed to persist turn summary: {}", e);
                    }
                }
                emit_activity_operation(
                    &app_bg,
                    &session_id,
                    &summary_op_id,
                    "生成本轮要点",
                    "done",
                    Some(
                        if summary_text
                            .as_deref()
                            .is_some_and(|s| !s.trim().is_empty())
                        {
                            "已生成本轮要点"
                        } else {
                            "后台模型未返回可用要点"
                        }
                        .to_string(),
                    ),
                );
            },
            // ── Follow-up suggestions ─────────────────────────────────────────
            async {
                if !follow_enabled {
                    return;
                }
                let follow_res = crate::domain::suggestions::generate_follow_up_suggestions(
                    bg_client.as_ref(),
                    Some(&user_request_bg),
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
                        emit_activity_operation(
                            &app_bg,
                            &session_id,
                            &follow_op_id,
                            "生成下一步建议",
                            "done",
                            Some(format!("已生成 {} 条下一步建议", items.len())),
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
                        emit_activity_operation(
                            &app_bg,
                            &session_id,
                            &follow_op_id,
                            "生成下一步建议",
                            "done",
                            Some("后台模型未返回可用建议".to_string()),
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
                        emit_activity_operation(
                            &app_bg,
                            &session_id,
                            &follow_op_id,
                            "生成下一步建议",
                            "error",
                            Some(format!("下一步建议生成失败：{e}")),
                        );
                    }
                }
            }
        );
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::{LlmConfig, LlmProvider};
    use futures::{stream, Stream};
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex as StdMutex;

    #[test]
    fn message_token_usage_mapping_preserves_cache_fields() {
        let usage = TokenUsage {
            prompt_tokens: 1_000,
            completion_tokens: 200,
            total_tokens: 1_200,
            cache_creation_input_tokens: Some(300),
            cache_read_input_tokens: Some(5_000),
        };

        let persisted = message_token_usage_from_llm_usage(&usage, "anthropic");

        assert_eq!(persisted.input, 1_000);
        assert_eq!(persisted.output, 200);
        assert_eq!(persisted.total, Some(1_200));
        assert_eq!(persisted.provider, Some("anthropic".to_string()));
        assert_eq!(persisted.cache_read, Some(5_000));
        assert_eq!(persisted.cache_creation, Some(300));
    }

    #[test]
    fn merge_turn_token_usage_accumulates_cache_fields() {
        let mut acc = Some(TokenUsage {
            prompt_tokens: 100,
            completion_tokens: 20,
            total_tokens: 120,
            cache_creation_input_tokens: Some(10),
            cache_read_input_tokens: Some(1_000),
        });

        merge_turn_token_usage(
            &mut acc,
            Some(TokenUsage {
                prompt_tokens: 200,
                completion_tokens: 30,
                total_tokens: 230,
                cache_creation_input_tokens: Some(15),
                cache_read_input_tokens: Some(2_000),
            }),
        );

        let usage = acc.unwrap();
        assert_eq!(usage.prompt_tokens, 300);
        assert_eq!(usage.completion_tokens, 50);
        assert_eq!(usage.total_tokens, 350);
        assert_eq!(usage.cache_creation_input_tokens, Some(25));
        assert_eq!(usage.cache_read_input_tokens, Some(3_000));
    }

    #[test]
    fn stream_normalizes_legacy_web_tool_names_and_arguments() {
        assert_eq!(normalize_stream_tool_name("web_search"), "search");
        assert_eq!(normalize_stream_tool_name("web_fetch"), "fetch");

        let search_args =
            normalize_stream_tool_arguments("web_search", "search", r#"{"q":"TP53"}"#);
        let value: serde_json::Value = serde_json::from_str(&search_args).unwrap();
        assert_eq!(value["category"], "web");
        assert_eq!(value["query"], "TP53");

        let fetch_args = normalize_stream_tool_arguments(
            "web_fetch",
            "fetch",
            r#"{"url":"https://example.com"}"#,
        );
        let value: serde_json::Value = serde_json::from_str(&fetch_args).unwrap();
        assert_eq!(value["category"], "web");
        assert_eq!(value["url"], "https://example.com");
    }

    #[test]
    fn stream_normalizes_legacy_pubmed_mcp_tool_to_unified_search() {
        assert_eq!(
            normalize_stream_tool_name("mcp__pubmed__pubmed_search_articles"),
            "search"
        );

        let search_args = normalize_stream_tool_arguments(
            "mcp__pubmed__pubmed_search_articles",
            "search",
            r#"{"term":"BRCA2","retmax":2}"#,
        );
        let value: serde_json::Value = serde_json::from_str(&search_args).unwrap();
        assert_eq!(value["category"], "literature");
        assert_eq!(value["source"], "pubmed");
        assert_eq!(value["query"], "BRCA2");
        assert_eq!(value["max_results"], 2);
    }

    struct RecordingEmitter {
        items: StdMutex<Vec<StreamOutputItem>>,
    }

    impl RecordingEmitter {
        fn new() -> Self {
            Self {
                items: StdMutex::new(Vec::new()),
            }
        }

        fn items(&self) -> Vec<StreamOutputItem> {
            self.items.lock().unwrap().clone()
        }
    }

    impl ChatStreamEmitter for RecordingEmitter {
        fn emit_stream_item(&self, _message_id: &str, item: &StreamOutputItem) {
            self.items.lock().unwrap().push(item.clone());
        }
    }

    struct NoopRoundStatus {
        partial_calls: AtomicUsize,
        cancel_calls: AtomicUsize,
    }

    impl NoopRoundStatus {
        fn new() -> Self {
            Self {
                partial_calls: AtomicUsize::new(0),
                cancel_calls: AtomicUsize::new(0),
            }
        }
    }

    #[async_trait::async_trait]
    impl RoundStatusStore for NoopRoundStatus {
        async fn mark_round_partial(&self, _round_id: &str) {
            self.partial_calls.fetch_add(1, Ordering::SeqCst);
        }

        async fn cancel_round(&self, _round_id: &str, _reason: Option<&str>) {
            self.cancel_calls.fetch_add(1, Ordering::SeqCst);
        }
    }

    struct SequencedClient {
        config: LlmConfig,
        streams: StdMutex<VecDeque<Vec<Result<LlmStreamChunk, ApiError>>>>,
        calls: AtomicUsize,
    }

    impl SequencedClient {
        fn new(streams: Vec<Vec<Result<LlmStreamChunk, ApiError>>>) -> Self {
            Self {
                config: LlmConfig::new(LlmProvider::Anthropic, "test"),
                streams: StdMutex::new(streams.into()),
                calls: AtomicUsize::new(0),
            }
        }

        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    #[async_trait::async_trait]
    impl LlmClient for SequencedClient {
        async fn send_message_streaming(
            &self,
            _messages: Vec<LlmMessage>,
            _tools: Vec<ToolSchema>,
        ) -> Result<Pin<Box<dyn Stream<Item = Result<LlmStreamChunk, ApiError>> + Send>>, ApiError>
        {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let chunks = self
                .streams
                .lock()
                .unwrap()
                .pop_front()
                .expect("mock stream exhausted");
            Ok(Box::pin(stream::iter(chunks)))
        }

        async fn health_check(&self) -> Result<bool, ApiError> {
            Ok(true)
        }

        fn config(&self) -> &LlmConfig {
            &self.config
        }
    }

    async fn run_core_stream(
        client: &SequencedClient,
        emitter: &RecordingEmitter,
        round_status: &NoopRoundStatus,
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
        let messages = vec![LlmMessage::user("hello")];
        let tools = Vec::<ToolSchema>::new();
        let pending_tools = Arc::new(Mutex::new(HashMap::new()));
        let cancel_flag = Arc::new(RwLock::new(false));

        stream_llm_response_with_cancel_core(StreamLlmCoreRequest {
            client,
            emitter,
            message_id: "message-1",
            round_id: "round-1",
            messages: &messages,
            tools: &tools,
            emit_text_chunks: true,
            pending_tools: &pending_tools,
            cancel_flag: &cancel_flag,
            round_status,
        })
        .await
    }

    #[tokio::test]
    async fn stream_retries_network_error_before_visible_output() {
        let client = SequencedClient::new(vec![
            vec![Err(ApiError::Network {
                message: "dropped".to_string(),
            })],
            vec![
                Ok(LlmStreamChunk::Text("recovered".to_string())),
                Ok(LlmStreamChunk::Stop { stop_reason: None }),
            ],
        ]);
        let emitter = RecordingEmitter::new();
        let round_status = NoopRoundStatus::new();

        let (_, assistant_text, _, was_cancelled, _) =
            run_core_stream(&client, &emitter, &round_status)
                .await
                .unwrap();

        assert_eq!(client.calls(), 2);
        assert_eq!(assistant_text, "recovered");
        assert!(!was_cancelled);
        let items = emitter.items();
        assert_eq!(items.len(), 1);
        match &items[0] {
            StreamOutputItem::Text(text) => assert_eq!(text, "recovered"),
            other => panic!("expected text event, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn stream_does_not_retry_after_visible_text_was_emitted() {
        let visible_text = "x".repeat(STREAM_EMIT_FLUSH_BYTES);
        let client = SequencedClient::new(vec![
            vec![
                Ok(LlmStreamChunk::Text(visible_text.clone())),
                Err(ApiError::Network {
                    message: "dropped".to_string(),
                }),
            ],
            vec![
                Ok(LlmStreamChunk::Text("must not be used".to_string())),
                Ok(LlmStreamChunk::Stop { stop_reason: None }),
            ],
        ]);
        let emitter = RecordingEmitter::new();
        let round_status = NoopRoundStatus::new();

        let result = run_core_stream(&client, &emitter, &round_status).await;

        assert!(result.is_err());
        assert_eq!(client.calls(), 1);
        let items = emitter.items();
        assert_eq!(items.len(), 1);
        match &items[0] {
            StreamOutputItem::Text(text) => assert_eq!(text, &visible_text),
            other => panic!("expected text event, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn stream_does_not_retry_after_completed_tool_call() {
        let client = SequencedClient::new(vec![
            vec![
                Ok(LlmStreamChunk::ToolStart {
                    id: "tool-1".to_string(),
                    name: "custom_tool".to_string(),
                }),
                Ok(LlmStreamChunk::ToolArguments(r#"{"ok":true}"#.to_string())),
                Ok(LlmStreamChunk::BlockStop),
                Err(ApiError::Network {
                    message: "dropped".to_string(),
                }),
            ],
            vec![
                Ok(LlmStreamChunk::Text("must not be used".to_string())),
                Ok(LlmStreamChunk::Stop { stop_reason: None }),
            ],
        ]);
        let emitter = RecordingEmitter::new();
        let round_status = NoopRoundStatus::new();

        let result = run_core_stream(&client, &emitter, &round_status).await;

        assert!(result.is_err());
        assert_eq!(client.calls(), 1);
        let items = emitter.items();
        assert!(matches!(
            items.first(),
            Some(StreamOutputItem::ToolUse { .. })
        ));
        assert!(matches!(
            items.get(1),
            Some(StreamOutputItem::ToolUse { .. })
        ));
    }

    #[test]
    fn visible_buffer_coalesces_small_text_chunks_and_flushes_tail() {
        let now = Instant::now();
        let mut buffer = VisibleChunkBuffer::new(now);
        let mut emitted = Vec::new();

        emitted.extend(buffer.push(VisibleChunkKind::Text, "a", now));
        emitted.extend(buffer.push(VisibleChunkKind::Text, "b", now));
        emitted.extend(buffer.push(VisibleChunkKind::Text, "c", now));

        assert!(emitted.is_empty());
        let tail = buffer.flush(now).expect("tail text should flush");
        match tail {
            StreamOutputItem::Text(text) => assert_eq!(text, "abc"),
            other => panic!("expected text event, got {other:?}"),
        }
    }

    #[test]
    fn visible_buffer_flushes_on_time_threshold() {
        let now = Instant::now();
        let mut buffer = VisibleChunkBuffer::new(now);

        assert!(buffer.push(VisibleChunkKind::Thinking, "a", now).is_empty());
        let emitted = buffer.push(
            VisibleChunkKind::Thinking,
            "b",
            now + STREAM_EMIT_FLUSH_INTERVAL,
        );

        assert_eq!(emitted.len(), 1);
        match &emitted[0] {
            StreamOutputItem::Thinking(text) => assert_eq!(text, "ab"),
            other => panic!("expected thinking event, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn stream_flushes_text_before_tool_start() {
        let client = SequencedClient::new(vec![vec![
            Ok(LlmStreamChunk::Text("before tool".to_string())),
            Ok(LlmStreamChunk::ToolStart {
                id: "tool-1".to_string(),
                name: "custom_tool".to_string(),
            }),
            Ok(LlmStreamChunk::Stop { stop_reason: None }),
        ]]);
        let emitter = RecordingEmitter::new();
        let round_status = NoopRoundStatus::new();

        let (_, assistant_text, _, _, _) = run_core_stream(&client, &emitter, &round_status)
            .await
            .unwrap();

        assert_eq!(assistant_text, "before tool");
        let items = emitter.items();
        assert!(items.len() >= 2);
        match &items[0] {
            StreamOutputItem::Text(text) => assert_eq!(text, "before tool"),
            other => panic!("expected text event first, got {other:?}"),
        }
        assert!(matches!(items[1], StreamOutputItem::ToolUse { .. }));
    }
}
