//! OpenAI-compatible API client
//!
//! Supports OpenAI, Azure OpenAI, and any OpenAI-compatible endpoint (Ollama, vLLM, etc.)

use super::{
    LlmClient, LlmConfig, LlmContent, LlmMessage, LlmProvider, LlmRole, LlmStreamChunk, TokenUsage,
};
use crate::domain::tools::ToolSchema;
use crate::errors::ApiError;
use async_trait::async_trait;
use futures::{Stream, StreamExt};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::pin::Pin;

const LARGE_REQUEST_WARN_BYTES: usize = 900_000;
const VERY_LARGE_REQUEST_WARN_BYTES: usize = 2_500_000;

/// OpenAI-compatible client
pub struct OpenAiCompatibleClient {
    client: Client,
    config: LlmConfig,
}

#[derive(Debug, Clone, Copy)]
struct RequestDiagnostics {
    body_bytes: usize,
    message_count: usize,
    tool_count: usize,
}

fn format_bytes(bytes: usize) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.2} MiB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.1} KiB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes} B")
    }
}

fn request_size_advice(diag: RequestDiagnostics) -> String {
    let severity = if diag.body_bytes >= VERY_LARGE_REQUEST_WARN_BYTES {
        "很大"
    } else if diag.body_bytes >= LARGE_REQUEST_WARN_BYTES {
        "偏大"
    } else {
        "正常"
    };
    let mut text = format!(
        "[Omiga] 本次 LLM 请求体：{}（{}），messages={}，tools={}。",
        format_bytes(diag.body_bytes),
        severity,
        diag.message_count,
        diag.tool_count
    );
    if diag.body_bytes >= LARGE_REQUEST_WARN_BYTES {
        text.push_str(
            "\n请求体偏大时，代理/网关可能在上传阶段断开连接，表现为 `error sending request`。\
             建议先压缩/开启新会话，减少长工具输出、Trace、队友记录后再重试。",
        );
    }
    text
}

fn request_diagnostics(body: &serde_json::Value) -> RequestDiagnostics {
    let body_bytes = serde_json::to_vec(body).map(|v| v.len()).unwrap_or(0);
    let message_count = body
        .get("messages")
        .and_then(|v| v.as_array())
        .map(|v| v.len())
        .unwrap_or(0);
    let tool_count = body
        .get("tools")
        .and_then(|v| v.as_array())
        .map(|v| v.len())
        .unwrap_or(0);
    RequestDiagnostics {
        body_bytes,
        message_count,
        tool_count,
    }
}

/// Moonshot returns 403 when the user selects a **Kimi For Coding** model or coding-only endpoint,
/// which is restricted to approved clients (Kimi CLI, Claude Code, etc.). Omiga uses the generic API.
fn enrich_openai_compatible_http_error(status: u16, body: &str) -> String {
    if status == 403 && body.contains("Kimi For Coding") {
        return format!(
            "{}\n\n\
            [Omiga] \"Kimi For Coding\" models are limited to specific tools (Kimi CLI, Claude Code, Roo Code, …). \
            In Settings → Model, use a **general** Kimi id such as `kimi-k2-0905-preview` or `kimi-k2.5`, \
            and the default `https://api.moonshot.ai/v1/chat/completions` base (not the coding-only API).",
            body
        );
    }
    body.to_string()
}

fn enrich_openai_compatible_network_error(
    config: &LlmConfig,
    url: &str,
    diagnostics: RequestDiagnostics,
    err: reqwest::Error,
) -> ApiError {
    let original = err.to_string();
    let size_advice = request_size_advice(diagnostics);
    let mut message = format!("{original}\n\n{size_advice}");

    if matches!(config.provider, LlmProvider::Deepseek) {
        message = format!(
            "{message}\n\n\
            [Omiga] 无法连接 DeepSeek API（{url}）。这通常是本机网络/DNS/TLS/代理问题，而不是模型返回错误。\n\
            建议检查：\n\
            1. 在同一台机器运行：curl -I https://api.deepseek.com\n\
            2. 如在需要代理的网络中，确保启动 Omiga 的进程继承了 HTTPS_PROXY/HTTP_PROXY，或在终端设置代理后再启动应用。\n\
            3. DeepSeek 官方 base_url 是 https://api.deepseek.com；OpenAI 兼容的 https://api.deepseek.com/v1 也可用。若在设置中填了 base_url，请不要填错域名。\n\
            4. 如果浏览器可访问但桌面应用不可访问，请检查系统代理/防火墙是否允许 Omiga 访问 api.deepseek.com:443。"
        );
    }

    if err.is_timeout() {
        ApiError::Network {
            message: format!("Request timeout while sending request. {message}"),
        }
    } else {
        ApiError::Network { message }
    }
}

/// Kimi OpenAPI (`KimiK25ChatRequest`): `thinking` must be an object `{"type":"enabled"|"disabled"}`,
/// not a boolean — otherwise: "expected type object ... bool is not acceptable".
/// See <https://platform.moonshot.ai/docs/api/chat> (kimi-k2.5 only; other Moonshot models omit `thinking`).
fn kimi_thinking_request_value(enabled: bool) -> serde_json::Value {
    serde_json::json!({
        "type": if enabled { "enabled" } else { "disabled" }
    })
}

/// Only `kimi-k2.5` (and id variants containing that substring) document the `thinking` object field.
fn moonshot_model_accepts_thinking_object(model: &str) -> bool {
    model.to_lowercase().contains("kimi-k2.5")
}

fn base_url_looks_like_moonshot(base: Option<&str>) -> bool {
    base.map(|u| {
        let u = u.to_lowercase();
        u.contains("moonshot") || u.contains("kimi.moonshot")
    })
    .unwrap_or(false)
}

/// Attach Kimi `thinking` body when the model + endpoint support it; otherwise omit the field.
fn maybe_attach_kimi_thinking_body(body: &mut serde_json::Value, config: &LlmConfig) {
    let enabled = config.thinking.unwrap_or(false);
    match config.provider {
        LlmProvider::Moonshot => {
            if moonshot_model_accepts_thinking_object(&config.model) {
                body["thinking"] = kimi_thinking_request_value(enabled);
            }
        }
        LlmProvider::Custom => {
            if base_url_looks_like_moonshot(config.base_url.as_deref())
                && moonshot_model_accepts_thinking_object(&config.model)
            {
                body["thinking"] = kimi_thinking_request_value(enabled);
            }
        }
        _ => {}
    }
}

/// Build the DeepSeek `thinking` object.
/// When enabled, `reasoning_effort` ("high" or "max") is included; default is "high".
fn deepseek_thinking_request_value(
    enabled: bool,
    reasoning_effort: Option<&str>,
) -> serde_json::Value {
    if enabled {
        serde_json::json!({
            "type": "enabled",
            "reasoning_effort": reasoning_effort.unwrap_or("high")
        })
    } else {
        serde_json::json!({ "type": "disabled" })
    }
}

/// Attach DeepSeek `thinking` body only when thinking is explicitly enabled.
///
/// Omitting the field (thinking disabled / not configured) is equivalent to
/// `{"type": "disabled"}` per the API default, and avoids breaking old configs
/// or sessions that never used thinking mode.
fn maybe_attach_deepseek_thinking_body(body: &mut serde_json::Value, config: &LlmConfig) {
    if config.provider != LlmProvider::Deepseek || config.thinking != Some(true) {
        return;
    }
    body["thinking"] = deepseek_thinking_request_value(true, config.reasoning_effort.as_deref());
}

impl OpenAiCompatibleClient {
    pub fn new(config: LlmConfig) -> Self {
        let client = Client::builder()
            .connect_timeout(std::time::Duration::from_secs(60))
            .timeout(std::time::Duration::from_secs(config.timeout_secs))
            .build()
            .unwrap_or_else(|_| Client::new());

        Self { client, config }
    }

    /// Determine the `reasoning_content` to include in a replayed assistant message.
    ///
    /// DeepSeek (docs): "When tool calls occur, reasoning_content MUST be returned to the API
    /// in all subsequent rounds — omitting it causes a 400 error."
    /// Moonshot/Kimi: same requirement when thinking is enabled.
    fn assistant_reasoning_for_api(
        config: &LlmConfig,
        has_tool_calls: bool,
        stored: Option<String>,
    ) -> Option<String> {
        let thinking_on = config.thinking == Some(true);

        if matches!(config.provider, LlmProvider::Deepseek) {
            if has_tool_calls {
                if thinking_on {
                    // Thinking enabled + tool calls: MUST include even when empty.
                    return Some(stored.unwrap_or_default());
                }
                // Thinking disabled but stored value is non-empty (e.g. deepseek-reasoner
                // produced reasoning before thinking was explicitly configured): pass it back.
                return stored.filter(|s| !s.is_empty());
            }
            // No tool calls: API ignores reasoning_content; only include if non-empty.
            return stored.filter(|s| !s.is_empty());
        }

        // Moonshot / Custom
        if !thinking_on {
            return stored.filter(|s| !s.is_empty());
        }
        if has_tool_calls {
            return Some(stored.unwrap_or_default());
        }
        stored.filter(|s| !s.is_empty())
    }

    /// Build request headers
    fn build_headers(&self) -> reqwest::header::HeaderMap {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::AUTHORIZATION,
            format!("Bearer {}", self.config.api_key).parse().unwrap(),
        );
        headers.insert(
            reqwest::header::CONTENT_TYPE,
            "application/json".parse().unwrap(),
        );

        // Add any extra headers from config
        if let Some(extra) = &self.config.extra_headers {
            for (key, value) in extra {
                if let Ok(header_name) = reqwest::header::HeaderName::from_bytes(key.as_bytes()) {
                    if let Ok(header_value) = value.parse() {
                        headers.insert(header_name, header_value);
                    }
                }
            }
        }

        headers
    }

    /// Convert LlmMessage to OpenAI Chat Completions format.
    ///
    /// Critical: tool results must use `role: "tool"` + `tool_call_id` (not a user message with
    /// empty content). Assistant turns with `tool_use` blocks must become `tool_calls`, or the
    /// model never sees prior tool output and will re-invoke tools in a loop.
    fn convert_messages(&self, messages: Vec<LlmMessage>) -> Vec<OpenAiMessage> {
        // Pre-validate: ensure tool_calls/tool message pairing is correct
        let messages = Self::validate_message_history(messages);

        let mut out = Vec::new();

        for msg in messages {
            match msg.role {
                LlmRole::System => {
                    let text: String = msg
                        .content
                        .iter()
                        .filter_map(|c| {
                            if let LlmContent::Text { text } = c {
                                Some(text.as_str())
                            } else {
                                None
                            }
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    out.push(OpenAiMessage {
                        role: "system".to_string(),
                        content: serde_json::Value::String(text),
                        name: None,
                        tool_calls: None,
                        tool_call_id: None,
                        reasoning_content: None,
                    });
                }
                LlmRole::User => {
                    if msg
                        .content
                        .iter()
                        .all(|c| matches!(c, LlmContent::ToolResult { .. }))
                    {
                        for block in msg.content {
                            if let LlmContent::ToolResult {
                                tool_use_id,
                                content,
                                ..
                            } = block
                            {
                                out.push(OpenAiMessage {
                                    role: "tool".to_string(),
                                    content: serde_json::Value::String(content),
                                    name: None,
                                    tool_calls: None,
                                    tool_call_id: Some(tool_use_id),
                                    reasoning_content: None,
                                });
                            }
                        }
                    } else {
                        let text: String = msg
                            .content
                            .iter()
                            .filter_map(|c| {
                                if let LlmContent::Text { text } = c {
                                    Some(text.as_str())
                                } else {
                                    None
                                }
                            })
                            .collect::<Vec<_>>()
                            .join("\n");
                        out.push(OpenAiMessage {
                            role: "user".to_string(),
                            content: serde_json::Value::String(text),
                            name: None,
                            tool_calls: None,
                            tool_call_id: None,
                            reasoning_content: None,
                        });
                    }
                }
                LlmRole::Assistant => {
                    let mut text_parts: Vec<String> = Vec::new();
                    let mut tool_calls_vec: Vec<OpenAiToolCall> = Vec::new();
                    for c in &msg.content {
                        match c {
                            LlmContent::Text { text } => {
                                if !text.is_empty() {
                                    text_parts.push(text.clone());
                                }
                            }
                            LlmContent::ToolUse {
                                id,
                                name,
                                arguments,
                            } => {
                                let args_str = match arguments {
                                    serde_json::Value::String(s) => s.clone(),
                                    other => other.to_string(),
                                };
                                tool_calls_vec.push(OpenAiToolCall {
                                    id: id.clone(),
                                    r#type: "function".to_string(),
                                    function: OpenAiToolFunction {
                                        name: name.clone(),
                                        arguments: args_str,
                                    },
                                });
                            }
                            _ => {}
                        }
                    }
                    let content = if text_parts.is_empty() {
                        serde_json::Value::String(String::new())
                    } else {
                        serde_json::Value::String(text_parts.join("\n"))
                    };
                    let has_tool_calls = !tool_calls_vec.is_empty();
                    let reasoning_content = Self::assistant_reasoning_for_api(
                        &self.config,
                        has_tool_calls,
                        msg.reasoning_content.clone(),
                    );
                    out.push(OpenAiMessage {
                        role: "assistant".to_string(),
                        content,
                        name: None,
                        tool_calls: if has_tool_calls {
                            Some(tool_calls_vec)
                        } else {
                            None
                        },
                        tool_call_id: None,
                        reasoning_content,
                    });
                }
                LlmRole::Tool => {
                    if let Some(LlmContent::ToolResult {
                        tool_use_id,
                        content,
                        ..
                    }) = msg.content.first()
                    {
                        out.push(OpenAiMessage {
                            role: "tool".to_string(),
                            content: serde_json::Value::String(content.clone()),
                            name: None,
                            tool_calls: None,
                            tool_call_id: Some(tool_use_id.clone()),
                            reasoning_content: None,
                        });
                    }
                }
            }
        }

        out
    }

    /// Validate message history to ensure OpenAI compatibility:
    /// - Assistant messages with tool_calls must be immediately followed by corresponding tool messages
    /// - Remove orphaned tool messages that don't have a matching assistant tool_call
    ///
    /// This intentionally enforces the stricter OpenAI Chat Completions invariant rather than
    /// just "the matching tool result appears later". Cancelled/retried Omiga rounds can leave
    /// persisted transcripts like `assistant(tool_calls) -> assistant(text) -> tool(result)`;
    /// OpenAI rejects that with HTTP 400, so we strip the stale tool-use blocks and drop the
    /// delayed orphan tool results before constructing the request body.
    fn validate_message_history(messages: Vec<LlmMessage>) -> Vec<LlmMessage> {
        use std::collections::HashSet;

        let mut result = Vec::new();
        let mut i = 0usize;

        while i < messages.len() {
            let msg = &messages[i];
            let tool_use_ids = assistant_tool_use_ids(msg);

            if !tool_use_ids.is_empty() {
                let mut needed: HashSet<String> = tool_use_ids.iter().cloned().collect();
                let mut tool_messages = Vec::new();
                let mut j = i + 1;

                while j < messages.len() && is_tool_result_message(&messages[j]) {
                    if let Some(filtered) =
                        filter_tool_result_message_for_ids(&messages[j], &mut needed)
                    {
                        tool_messages.push(filtered);
                    }
                    j += 1;
                    if needed.is_empty() {
                        while j < messages.len() && is_tool_result_message(&messages[j]) {
                            j += 1;
                        }
                        break;
                    }
                }

                if needed.is_empty() {
                    result.push(msg.clone());
                    result.extend(tool_messages);
                } else {
                    tracing::warn!(
                        target: "omiga::openai",
                        missing_tool_call_ids = ?needed,
                        "Assistant tool_calls were not followed immediately by matching tool results; stripping stale tool calls"
                    );
                    result.push(strip_tool_uses(msg));
                }

                i = j.max(i + 1);
                continue;
            }

            if is_tool_result_message(msg) {
                tracing::warn!(
                    target: "omiga::openai",
                    "Dropping orphaned tool result message before OpenAI request"
                );
                i += 1;
                continue;
            }

            result.push(msg.clone());
            i += 1;
        }

        result
    }

    /// Convert ToolSchema to OpenAI tool format
    fn convert_tools(&self, tools: Vec<ToolSchema>) -> Vec<OpenAiTool> {
        tools
            .into_iter()
            .map(|t| OpenAiTool {
                r#type: "function".to_string(),
                function: OpenAiFunction {
                    name: t.name,
                    description: Some(t.description),
                    parameters: Some(t.parameters),
                },
            })
            .collect()
    }
}

fn assistant_tool_use_ids(msg: &LlmMessage) -> Vec<String> {
    if msg.role != LlmRole::Assistant {
        return Vec::new();
    }
    msg.content
        .iter()
        .filter_map(|content| match content {
            LlmContent::ToolUse { id, .. } => Some(id.clone()),
            _ => None,
        })
        .collect()
}

fn is_tool_result_message(msg: &LlmMessage) -> bool {
    matches!(msg.role, LlmRole::Tool | LlmRole::User)
        && !msg.content.is_empty()
        && msg
            .content
            .iter()
            .all(|content| matches!(content, LlmContent::ToolResult { .. }))
}

fn filter_tool_result_message_for_ids(
    msg: &LlmMessage,
    needed: &mut std::collections::HashSet<String>,
) -> Option<LlmMessage> {
    let mut filtered = msg.clone();
    filtered.content = msg
        .content
        .iter()
        .filter_map(|content| match content {
            LlmContent::ToolResult { tool_use_id, .. } if needed.remove(tool_use_id) => {
                Some(content.clone())
            }
            _ => None,
        })
        .collect();

    if filtered.content.is_empty() {
        None
    } else {
        Some(filtered)
    }
}

fn strip_tool_uses(msg: &LlmMessage) -> LlmMessage {
    let mut stripped = msg.clone();
    stripped
        .content
        .retain(|content| !matches!(content, LlmContent::ToolUse { .. }));
    stripped.tool_calls = None;
    stripped
}

#[async_trait]
impl LlmClient for OpenAiCompatibleClient {
    async fn send_message_streaming(
        &self,
        messages: Vec<LlmMessage>,
        tools: Vec<ToolSchema>,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<LlmStreamChunk, ApiError>> + Send>>, ApiError>
    {
        let url = self.config.api_url();
        let headers = self.build_headers();

        let openai_messages = self.convert_messages(messages);
        let openai_tools = if tools.is_empty() {
            None
        } else {
            Some(self.convert_tools(tools))
        };

        let mut body = serde_json::json!({
            "model": self.config.model,
            "messages": openai_messages,
            "stream": true,
            "max_tokens": self.config.max_tokens,
        });

        if let Some(temp) = self.config.temperature {
            body["temperature"] = serde_json::json!(temp);
        }

        if let Some(tools) = openai_tools {
            body["tools"] = serde_json::json!(tools);
            body["tool_choice"] = serde_json::json!("auto");
        }

        // OpenAI: include usage in the last stream chunks. Moonshot/Kimi rejects unknown fields with errors.
        if !matches!(self.config.provider, LlmProvider::Moonshot) {
            body["stream_options"] = serde_json::json!({ "include_usage": true });
        }

        // Add extra query params if any
        if let Some(extra_query) = &self.config.extra_query {
            for (key, value) in extra_query {
                body[key] = serde_json::json!(value);
            }
        }

        maybe_attach_kimi_thinking_body(&mut body, &self.config);
        maybe_attach_deepseek_thinking_body(&mut body, &self.config);
        let diagnostics = request_diagnostics(&body);
        if diagnostics.body_bytes >= LARGE_REQUEST_WARN_BYTES {
            tracing::warn!(
                target: "omiga::openai",
                provider = %self.config.provider,
                model = %self.config.model,
                body_bytes = diagnostics.body_bytes,
                message_count = diagnostics.message_count,
                tool_count = diagnostics.tool_count,
                "large LLM request body before send"
            );
        }

        let response = self
            .client
            .post(&url)
            .headers(headers)
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                enrich_openai_compatible_network_error(&self.config, &url, diagnostics, e)
            })?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let text = response.text().await.unwrap_or_default();
            let message = enrich_openai_compatible_http_error(status, &text);
            return Err(ApiError::Http { status, message });
        }

        let stream = response
            .bytes_stream()
            .filter_map(|result| async move {
                match result {
                    Ok(bytes) => {
                        let text = String::from_utf8_lossy(&bytes);
                        Some(Ok::<_, ApiError>(text.to_string()))
                    }
                    Err(e) => Some(Err(ApiError::from(e))),
                }
            })
            .flat_map(|result| {
                futures::stream::iter(match result {
                    Ok(text) => parse_sse_events(&text),
                    Err(e) => vec![Err(e)],
                })
            });

        Ok(Box::pin(stream))
    }

    async fn health_check(&self) -> Result<bool, ApiError> {
        // Simple check - just verify config is valid
        Ok(!self.config.api_key.is_empty())
    }

    fn config(&self) -> &LlmConfig {
        &self.config
    }
}

/// Parse SSE events from OpenAI format
fn parse_sse_events(text: &str) -> Vec<Result<LlmStreamChunk, ApiError>> {
    let mut results = Vec::new();

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with(":") {
            continue;
        }

        if !line.starts_with("data: ") {
            continue;
        }

        let data = &line[6..]; // Remove "data: " prefix

        if data == "[DONE]" {
            results.push(Ok(LlmStreamChunk::Stop {
                stop_reason: Some("complete".to_string()),
            }));
            continue;
        }

        match serde_json::from_str::<OpenAiStreamResponse>(data) {
            Ok(response) => {
                if let Some(u) = response.usage {
                    let mut tu = TokenUsage::default();
                    tu.prompt_tokens = u.prompt_tokens.unwrap_or(0);
                    tu.completion_tokens = u.completion_tokens.unwrap_or(0);
                    tu.total_tokens = u
                        .total_tokens
                        .unwrap_or(tu.prompt_tokens.saturating_add(tu.completion_tokens));
                    if tu.prompt_tokens > 0 || tu.completion_tokens > 0 {
                        results.push(Ok(LlmStreamChunk::Usage(tu)));
                    }
                }
                if let Some(choice) = response.choices.first() {
                    if let Some(delta) = &choice.delta {
                        if let Some(rc) = &delta.reasoning_content {
                            results.push(Ok(LlmStreamChunk::ReasoningContent(rc.clone())));
                        }
                        if let Some(content) = &delta.content {
                            results.push(Ok(LlmStreamChunk::Text(content.clone())));
                        }

                        if let Some(tool_calls) = &delta.tool_calls {
                            for tool_call in tool_calls {
                                if let Some(id) = &tool_call.id {
                                    results.push(Ok(LlmStreamChunk::ToolStart {
                                        id: id.clone(),
                                        name: tool_call
                                            .function
                                            .as_ref()
                                            .and_then(|f| f.name.clone())
                                            .unwrap_or_default(),
                                    }));
                                }
                                if let Some(func) = &tool_call.function {
                                    if let Some(args) = &func.arguments {
                                        results
                                            .push(Ok(LlmStreamChunk::ToolArguments(args.clone())));
                                    }
                                }
                            }
                        }
                    }

                    if let Some(finish_reason) = &choice.finish_reason {
                        if finish_reason == "stop" || finish_reason == "tool_calls" {
                            results.push(Ok(LlmStreamChunk::BlockStop));
                        }
                    }
                }
            }
            Err(e) => {
                // Silently skip parse errors for malformed chunks
                tracing::debug!("Failed to parse SSE chunk: {}", e);
            }
        }
    }

    results
}

// OpenAI API types

#[derive(Debug, Serialize)]
struct OpenAiMessage {
    role: String,
    content: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<OpenAiToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_content: Option<String>,
}

#[derive(Debug, Serialize)]
struct OpenAiTool {
    r#type: String,
    function: OpenAiFunction,
}

#[derive(Debug, Serialize)]
struct OpenAiFunction {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    parameters: Option<serde_json::Value>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct OpenAiStreamResponse {
    id: String,
    object: String,
    created: i64,
    model: String,
    #[serde(default)]
    choices: Vec<OpenAiChoice>,
    #[serde(default)]
    usage: Option<OpenAiUsageChunk>,
}

#[derive(Debug, Deserialize)]
struct OpenAiUsageChunk {
    prompt_tokens: Option<u32>,
    completion_tokens: Option<u32>,
    total_tokens: Option<u32>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct OpenAiChoice {
    index: i32,
    delta: Option<OpenAiDelta>,
    finish_reason: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct OpenAiDelta {
    role: Option<String>,
    content: Option<String>,
    #[serde(default)]
    reasoning_content: Option<String>,
    tool_calls: Option<Vec<OpenAiToolCallDelta>>,
}

#[derive(Debug, Deserialize, Serialize)]
struct OpenAiToolCall {
    id: String,
    r#type: String,
    function: OpenAiToolFunction,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct OpenAiToolCallDelta {
    index: i32,
    id: Option<String>,
    r#type: Option<String>,
    function: Option<OpenAiToolFunctionDelta>,
}

#[derive(Debug, Deserialize, Serialize)]
struct OpenAiToolFunction {
    name: String,
    arguments: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct OpenAiToolFunctionDelta {
    name: Option<String>,
    arguments: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn assistant_with_tool(text: &str, id: &str) -> LlmMessage {
        LlmMessage {
            role: LlmRole::Assistant,
            content: vec![
                LlmContent::text(text),
                LlmContent::tool_use(id, "search", json!({"query": "q"})),
            ],
            name: None,
            tool_calls: None,
            reasoning_content: None,
        }
    }

    fn tool_ids(messages: &[LlmMessage]) -> Vec<String> {
        messages
            .iter()
            .flat_map(|msg| msg.content.iter())
            .filter_map(|content| match content {
                LlmContent::ToolResult { tool_use_id, .. } => Some(tool_use_id.clone()),
                _ => None,
            })
            .collect()
    }

    fn has_tool_use(message: &LlmMessage) -> bool {
        message
            .content
            .iter()
            .any(|content| matches!(content, LlmContent::ToolUse { .. }))
    }

    #[test]
    fn validate_message_history_preserves_immediate_tool_result() {
        let messages = vec![
            LlmMessage::user("question"),
            assistant_with_tool("checking", "call_1"),
            LlmMessage::tool("call_1", "result"),
            LlmMessage::assistant("done"),
        ];

        let sanitized = OpenAiCompatibleClient::validate_message_history(messages);

        assert_eq!(sanitized.len(), 4);
        assert!(has_tool_use(&sanitized[1]));
        assert_eq!(tool_ids(&sanitized), vec!["call_1"]);
    }

    #[test]
    fn validate_message_history_strips_interleaved_tool_call_and_drops_delayed_result() {
        let messages = vec![
            LlmMessage::user("question"),
            assistant_with_tool("checking", "call_1"),
            LlmMessage::assistant("final answer from cancelled overlapping round"),
            LlmMessage::tool("call_1", "late result"),
        ];

        let sanitized = OpenAiCompatibleClient::validate_message_history(messages);

        assert_eq!(sanitized.len(), 3);
        assert!(!has_tool_use(&sanitized[1]));
        assert_eq!(sanitized[1].text_content(), "checking");
        assert_eq!(
            sanitized[2].text_content(),
            "final answer from cancelled overlapping round"
        );
        assert!(tool_ids(&sanitized).is_empty());
    }

    #[test]
    fn validate_message_history_drops_unknown_tool_results_inside_contiguous_block() {
        let messages = vec![
            assistant_with_tool("checking", "call_1"),
            LlmMessage::tool("orphan", "orphan result"),
            LlmMessage::tool("call_1", "real result"),
            LlmMessage::assistant("done"),
        ];

        let sanitized = OpenAiCompatibleClient::validate_message_history(messages);

        assert_eq!(sanitized.len(), 3);
        assert!(has_tool_use(&sanitized[0]));
        assert_eq!(tool_ids(&sanitized), vec!["call_1"]);
        assert_eq!(sanitized[2].text_content(), "done");
    }
}
