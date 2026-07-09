//! Claude API client with full SSE streaming

use crate::domain::tools::ToolSchema;
use crate::errors::ApiError;
use futures::Stream;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::pin::Pin;

const DEFAULT_API_URL: &str = "https://api.anthropic.com/v1/messages";
const DEFAULT_MODEL: &str = "claude-3-5-sonnet-20241022";
const DEFAULT_MAX_TOKENS: u32 = 4096;
const ANTHROPIC_CACHE_CONTROL_MAX_BREAKPOINTS: usize = 4;

fn default_prompt_cache() -> bool {
    true
}

/// Claude API configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeConfig {
    pub api_key: String,
    pub api_url: String,
    pub model: String,
    pub max_tokens: u32,
    pub temperature: Option<f32>,
    pub system: Option<String>,
    pub version: String,
    /// Request timeout in seconds (total including streaming)
    pub timeout: u64,
    /// Enable Anthropic prompt-cache cache_control breakpoints.
    #[serde(default = "default_prompt_cache")]
    pub prompt_cache: bool,
}

impl ClaudeConfig {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            api_url: DEFAULT_API_URL.to_string(),
            model: DEFAULT_MODEL.to_string(),
            max_tokens: DEFAULT_MAX_TOKENS,
            temperature: None,
            system: None,
            version: "2023-06-01".to_string(),
            timeout: 600,
            prompt_cache: true,
        }
    }

    pub fn headers(&self) -> reqwest::header::HeaderMap {
        use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};

        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", self.api_key)).unwrap(),
        );
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert(
            "anthropic-version",
            HeaderValue::from_str(&self.version).unwrap(),
        );
        headers.insert(
            "anthropic-beta",
            HeaderValue::from_static("tools-2024-04-04"),
        );
        headers
    }
}

/// API message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: Vec<ContentBlock>,
    /// Moonshot/Kimi: required on replay when `thinking` is enabled and the turn has tool calls.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: String,
        is_error: Option<bool>,
    },
}

impl ContentBlock {
    pub fn text(text: impl Into<String>) -> Self {
        ContentBlock::Text { text: text.into() }
    }

    pub fn tool_result(tool_use_id: impl Into<String>, content: impl Into<String>) -> Self {
        ContentBlock::ToolResult {
            tool_use_id: tool_use_id.into(),
            content: content.into(),
            is_error: None,
        }
    }
}

/// Tool definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

#[derive(Debug, Clone, Copy, Serialize)]
struct CacheControl {
    #[serde(rename = "type")]
    cache_type: &'static str,
}

impl CacheControl {
    fn ephemeral() -> Self {
        Self {
            cache_type: "ephemeral",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
struct Request {
    model: String,
    max_tokens: u32,
    messages: Vec<RequestMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<Vec<RequestSystemBlock>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    tools: Vec<RequestToolDefinition>,
    stream: bool,
}

#[derive(Debug, Clone, Serialize)]
struct RequestMessage {
    role: Role,
    content: Vec<RequestContentBlock>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    reasoning_content: Option<String>,
}

impl From<Message> for RequestMessage {
    fn from(message: Message) -> Self {
        Self {
            role: message.role,
            content: message
                .content
                .into_iter()
                .map(RequestContentBlock::from)
                .collect(),
            reasoning_content: message.reasoning_content,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
enum RequestSystemBlock {
    #[serde(rename = "text")]
    Text {
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        cache_control: Option<CacheControl>,
    },
}

impl RequestSystemBlock {
    fn text(text: impl Into<String>) -> Self {
        Self::Text {
            text: text.into(),
            cache_control: None,
        }
    }

    fn set_cache_control(&mut self, cache_control: CacheControl) {
        match self {
            Self::Text {
                cache_control: target,
                ..
            } => *target = Some(cache_control),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
enum RequestContentBlock {
    #[serde(rename = "text")]
    Text {
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        cache_control: Option<CacheControl>,
    },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
        #[serde(skip_serializing_if = "Option::is_none")]
        cache_control: Option<CacheControl>,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: String,
        is_error: Option<bool>,
        #[serde(skip_serializing_if = "Option::is_none")]
        cache_control: Option<CacheControl>,
    },
}

impl RequestContentBlock {
    fn set_cache_control(&mut self, cache_control: CacheControl) {
        match self {
            Self::Text {
                cache_control: target,
                ..
            }
            | Self::ToolUse {
                cache_control: target,
                ..
            }
            | Self::ToolResult {
                cache_control: target,
                ..
            } => *target = Some(cache_control),
        }
    }
}

impl From<ContentBlock> for RequestContentBlock {
    fn from(block: ContentBlock) -> Self {
        match block {
            ContentBlock::Text { text } => Self::Text {
                text,
                cache_control: None,
            },
            ContentBlock::ToolUse { id, name, input } => Self::ToolUse {
                id,
                name,
                input,
                cache_control: None,
            },
            ContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => Self::ToolResult {
                tool_use_id,
                content,
                is_error,
                cache_control: None,
            },
        }
    }
}

#[derive(Debug, Clone, Serialize)]
struct RequestToolDefinition {
    name: String,
    description: String,
    input_schema: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    cache_control: Option<CacheControl>,
}

impl From<ToolDefinition> for RequestToolDefinition {
    fn from(tool: ToolDefinition) -> Self {
        Self {
            name: tool.name,
            description: tool.description,
            input_schema: tool.input_schema,
            cache_control: None,
        }
    }
}

fn build_request(
    config: &ClaudeConfig,
    messages: Vec<Message>,
    tools: Vec<ToolDefinition>,
) -> Request {
    let mut request_messages: Vec<RequestMessage> =
        messages.into_iter().map(RequestMessage::from).collect();
    let mut request_tools: Vec<RequestToolDefinition> =
        tools.into_iter().map(RequestToolDefinition::from).collect();
    let mut system = config
        .system
        .clone()
        .map(|text| vec![RequestSystemBlock::text(text)]);

    if config.prompt_cache {
        let mut breakpoints = 0usize;

        if let Some(last_system_block) = system.as_mut().and_then(|blocks| blocks.last_mut()) {
            last_system_block.set_cache_control(CacheControl::ephemeral());
            breakpoints += 1;
        }

        if let Some(last_tool) = request_tools.last_mut() {
            last_tool.cache_control = Some(CacheControl::ephemeral());
            breakpoints += 1;
        }

        if let Some(last_user_message) = request_messages
            .iter_mut()
            .rfind(|message| message.role == Role::User)
        {
            if let Some(last_content_block) = last_user_message.content.last_mut() {
                last_content_block.set_cache_control(CacheControl::ephemeral());
                breakpoints += 1;
            }
        }

        debug_assert!(breakpoints <= ANTHROPIC_CACHE_CONTROL_MAX_BREAKPOINTS);
    }

    Request {
        model: config.model.clone(),
        max_tokens: config.max_tokens,
        messages: request_messages,
        system,
        temperature: config.temperature,
        tools: request_tools,
        stream: true,
    }
}

/// Stream chunk for UI
#[derive(Debug, Clone)]
pub enum StreamChunk {
    Text(String),
    ToolStart {
        id: String,
        name: String,
    },
    ToolJson(String),
    BlockStop,
    /// Final usage for this Anthropic stream (emitted before [`Stop`])
    Usage(crate::llm::TokenUsage),
    Stop,
    Ping,
}

/// Claude API streaming events (deserialized from SSE; fields are for serde shape)
#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
enum StreamEvent {
    #[serde(rename = "message_start")]
    MessageStart { message: MessageStart },
    #[serde(rename = "content_block_start")]
    ContentBlockStart {
        index: usize,
        content_block: ContentBlockStart,
    },
    #[serde(rename = "content_block_delta")]
    ContentBlockDelta { index: usize, delta: ContentDelta },
    #[serde(rename = "content_block_stop")]
    ContentBlockStop { index: usize },
    #[serde(rename = "message_delta")]
    MessageDelta {
        delta: MessageDeltaBody,
        #[serde(default)]
        usage: Option<AnthropicUsageFields>,
    },
    #[serde(rename = "message_stop")]
    MessageStop,
    #[serde(rename = "ping")]
    Ping,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
struct MessageStart {
    id: String,
    #[serde(rename = "type")]
    _type: String,
    role: Role,
    content: Vec<ContentBlock>,
    model: String,
    #[serde(default)]
    usage: Option<AnthropicUsageFields>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct AnthropicUsageFields {
    #[serde(default)]
    input_tokens: Option<u32>,
    #[serde(default)]
    output_tokens: Option<u32>,
    #[serde(default)]
    cache_creation_input_tokens: Option<u32>,
    #[serde(default)]
    cache_read_input_tokens: Option<u32>,
}

impl AnthropicUsageFields {
    fn apply_to(&self, usage_acc: &mut crate::llm::TokenUsage) {
        if let Some(input_tokens) = self.input_tokens {
            usage_acc.prompt_tokens = input_tokens;
        }
        if let Some(output_tokens) = self.output_tokens {
            usage_acc.completion_tokens = output_tokens;
        }
        if let Some(cache_creation_input_tokens) = self.cache_creation_input_tokens {
            usage_acc.cache_creation_input_tokens = Some(cache_creation_input_tokens);
        }
        if let Some(cache_read_input_tokens) = self.cache_read_input_tokens {
            usage_acc.cache_read_input_tokens = Some(cache_read_input_tokens);
        }
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
enum ContentBlockStart {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
enum ContentDelta {
    #[serde(rename = "text_delta")]
    TextDelta { text: String },
    #[serde(rename = "input_json_delta")]
    InputJsonDelta { partial_json: String },
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
struct MessageDeltaBody {
    stop_reason: Option<String>,
    stop_sequence: Option<String>,
}

/// Claude API client
pub struct ClaudeClient {
    config: ClaudeConfig,
    client: Client,
}

impl ClaudeClient {
    pub fn new(config: ClaudeConfig) -> Self {
        let client = Client::builder()
            .connect_timeout(std::time::Duration::from_secs(60))
            .timeout(std::time::Duration::from_secs(config.timeout))
            .build()
            .expect("Failed to create HTTP client");
        Self { config, client }
    }

    pub async fn send_message_streaming(
        &self,
        messages: Vec<Message>,
        tools: Vec<ToolDefinition>,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamChunk, ApiError>> + Send>>, ApiError> {
        let request = build_request(&self.config, messages, tools);

        let response = self
            .client
            .post(&self.config.api_url)
            .headers(self.config.headers())
            .json(&request)
            .send()
            .await
            .map_err(|e| ApiError::Network {
                message: e.to_string(),
            })?;

        let status = response.status();
        if !status.is_success() {
            let status_u16 = status.as_u16();
            // 429 = rate limit, 529 = Anthropic engine overloaded — both are retryable
            if status_u16 == 429 || status_u16 == 529 {
                let retry_after = response
                    .headers()
                    .get("retry-after")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|s| s.parse::<u64>().ok())
                    .unwrap_or(0);
                return Err(ApiError::RateLimited { retry_after });
            }
            let text = response.text().await.unwrap_or_default();
            return Err(ApiError::Http {
                status: status_u16,
                message: text,
            });
        }

        // Create streaming channel
        let (tx, rx) = tokio::sync::mpsc::channel::<Result<StreamChunk, ApiError>>(100);

        // Spawn task to parse SSE and send chunks
        tokio::spawn(parse_sse_stream(response, tx));

        let stream = tokio_stream::wrappers::ReceiverStream::new(rx);
        Ok(Box::pin(stream))
    }

    pub fn tools_to_definitions(schemas: &[ToolSchema]) -> Vec<ToolDefinition> {
        schemas
            .iter()
            .map(|schema| ToolDefinition {
                name: schema.name.clone(),
                description: schema.description.clone(),
                input_schema: schema.parameters.clone(),
            })
            .collect()
    }
}

/// Parse SSE stream from Claude API
async fn parse_sse_stream(
    response: reqwest::Response,
    tx: tokio::sync::mpsc::Sender<Result<StreamChunk, ApiError>>,
) {
    use futures::StreamExt;

    let mut stream = response.bytes_stream();
    let mut buffer = String::new();
    let mut usage_acc = crate::llm::TokenUsage::default();

    while let Some(chunk) = stream.next().await {
        match chunk {
            Ok(bytes) => {
                // Convert bytes to string and append to buffer
                match String::from_utf8(bytes.to_vec()) {
                    Ok(text) => {
                        buffer.push_str(&text);

                        // Process complete lines
                        while let Some(pos) = buffer.find("\n\n") {
                            let event = buffer[..pos].to_string();
                            buffer = buffer[pos + 2..].to_string();

                            if process_sse_event(&event, &tx, &mut usage_acc)
                                .await
                                .is_err()
                            {
                                return; // Channel closed
                            }
                        }
                    }
                    Err(_) => {
                        let _ = tx
                            .send(Err(ApiError::SseParse {
                                message: "Invalid UTF-8 in stream".to_string(),
                            }))
                            .await;
                        return;
                    }
                }
            }
            Err(e) => {
                let _ = tx
                    .send(Err(ApiError::Network {
                        message: format!("Stream error: {}", e),
                    }))
                    .await;
                return;
            }
        }
    }

    // Process any remaining data in buffer
    if !buffer.is_empty() {
        let _ = process_sse_event(&buffer, &tx, &mut usage_acc).await;
    }

    // Send stop signal
    let _ = tx.send(Ok(StreamChunk::Stop)).await;
}

/// Process a single SSE event
async fn process_sse_event(
    event: &str,
    tx: &tokio::sync::mpsc::Sender<Result<StreamChunk, ApiError>>,
    usage_acc: &mut crate::llm::TokenUsage,
) -> Result<(), ()> {
    // Parse SSE format (data: {...})
    let mut data = None;
    for line in event.lines() {
        if let Some(stripped) = line.strip_prefix("data: ") {
            data = Some(stripped);
            break;
        }
    }

    let Some(data) = data else {
        return Ok(()); // No data line, skip
    };

    // Handle special [DONE] marker
    if data == "[DONE]" {
        return tx.send(Ok(StreamChunk::Stop)).await.map_err(|_| ());
    }

    // Parse JSON event
    match serde_json::from_str::<StreamEvent>(data) {
        Ok(event) => {
            match event {
                StreamEvent::MessageStart { message } => {
                    if let Some(u) = message.usage {
                        u.apply_to(usage_acc);
                    }
                    Ok(())
                }
                StreamEvent::ContentBlockStart { content_block, .. } => {
                    match content_block {
                        ContentBlockStart::Text { .. } => {
                            // Text block started - no special handling
                            Ok(())
                        }
                        ContentBlockStart::ToolUse { id, name, .. } => tx
                            .send(Ok(StreamChunk::ToolStart { id, name }))
                            .await
                            .map_err(|_| ()),
                    }
                }
                StreamEvent::ContentBlockDelta { delta, .. } => match delta {
                    ContentDelta::TextDelta { text } => {
                        tx.send(Ok(StreamChunk::Text(text))).await.map_err(|_| ())
                    }
                    ContentDelta::InputJsonDelta { partial_json } => tx
                        .send(Ok(StreamChunk::ToolJson(partial_json)))
                        .await
                        .map_err(|_| ()),
                },
                StreamEvent::ContentBlockStop { .. } => {
                    tx.send(Ok(StreamChunk::BlockStop)).await.map_err(|_| ())
                }
                StreamEvent::MessageDelta { usage, .. } => {
                    if let Some(u) = usage {
                        u.apply_to(usage_acc);
                    }
                    Ok(())
                }
                StreamEvent::MessageStop => {
                    usage_acc.total_tokens = usage_acc
                        .prompt_tokens
                        .saturating_add(usage_acc.completion_tokens);
                    if usage_acc.has_any_tokens() {
                        let _ = tx.send(Ok(StreamChunk::Usage(usage_acc.clone()))).await;
                    }
                    tx.send(Ok(StreamChunk::Stop)).await.map_err(|_| ())
                }
                StreamEvent::Ping => tx.send(Ok(StreamChunk::Ping)).await.map_err(|_| ()),
            }
        }
        Err(e) => {
            // Try to parse as error response
            if let Ok(error_obj) = serde_json::from_str::<serde_json::Value>(data) {
                if let Some(error) = error_obj.get("error") {
                    let message = error
                        .get("message")
                        .and_then(|m| m.as_str())
                        .unwrap_or("Unknown API error")
                        .to_string();
                    return tx
                        .send(Err(ApiError::Server { message }))
                        .await
                        .map_err(|_| ());
                }
            }

            tx.send(Err(ApiError::SseParse {
                message: format!(
                    "Parse error: {} | Data: {}",
                    e,
                    data.chars().take(200).collect::<String>()
                ),
            }))
            .await
            .map_err(|_| ())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn tool(name: &str) -> ToolDefinition {
        ToolDefinition {
            name: name.to_string(),
            description: format!("{name} tool"),
            input_schema: json!({"type": "object"}),
        }
    }

    fn user_message(content: Vec<ContentBlock>) -> Message {
        Message {
            role: Role::User,
            content,
            reasoning_content: None,
        }
    }

    fn count_cache_control_breakpoints(value: &Value) -> usize {
        match value {
            Value::Object(map) => {
                usize::from(map.contains_key("cache_control"))
                    + map
                        .values()
                        .map(count_cache_control_breakpoints)
                        .sum::<usize>()
            }
            Value::Array(values) => values.iter().map(count_cache_control_breakpoints).sum(),
            _ => 0,
        }
    }

    #[test]
    fn anthropic_request_serializes_prompt_cache_breakpoints_in_expected_positions() {
        let mut config = ClaudeConfig::new("test-key");
        config.system = Some("system prompt".to_string());

        let request = build_request(
            &config,
            vec![
                user_message(vec![ContentBlock::text("first user")]),
                Message {
                    role: Role::Assistant,
                    content: vec![ContentBlock::text("assistant")],
                    reasoning_content: None,
                },
                user_message(vec![
                    ContentBlock::text("latest user text"),
                    ContentBlock::tool_result("tool_1", "tool result"),
                ]),
            ],
            vec![tool("first_tool"), tool("last_tool")],
        );
        let value = serde_json::to_value(request).unwrap();

        assert_eq!(
            value["system"][0]["cache_control"],
            json!({"type": "ephemeral"})
        );
        assert!(value["tools"][0].get("cache_control").is_none());
        assert_eq!(
            value["tools"][1]["cache_control"],
            json!({"type": "ephemeral"})
        );
        assert!(value["messages"][0]["content"][0]
            .get("cache_control")
            .is_none());
        assert!(value["messages"][2]["content"][0]
            .get("cache_control")
            .is_none());
        assert_eq!(
            value["messages"][2]["content"][1]["cache_control"],
            json!({"type": "ephemeral"})
        );

        let breakpoint_count = count_cache_control_breakpoints(&value);
        assert_eq!(breakpoint_count, 3);
        assert!(breakpoint_count <= ANTHROPIC_CACHE_CONTROL_MAX_BREAKPOINTS);
    }

    #[test]
    fn anthropic_request_omits_prompt_cache_breakpoints_when_disabled() {
        let mut config = ClaudeConfig::new("test-key");
        config.system = Some("system prompt".to_string());
        config.prompt_cache = false;

        let request = build_request(
            &config,
            vec![user_message(vec![ContentBlock::text("user")])],
            vec![tool("only_tool")],
        );
        let value = serde_json::to_value(request).unwrap();

        assert_eq!(count_cache_control_breakpoints(&value), 0);
    }

    #[tokio::test]
    async fn anthropic_sse_usage_parses_prompt_cache_fields() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(4);
        let mut usage_acc = crate::llm::TokenUsage::default();

        process_sse_event(
            r#"data: {"type":"message_start","message":{"id":"msg_1","type":"message","role":"assistant","content":[],"model":"claude","usage":{"input_tokens":10,"cache_creation_input_tokens":4}}}"#,
            &tx,
            &mut usage_acc,
        )
        .await
        .unwrap();
        process_sse_event(
            r#"data: {"type":"message_delta","delta":{"stop_reason":null,"stop_sequence":null},"usage":{"output_tokens":7,"cache_read_input_tokens":3}}"#,
            &tx,
            &mut usage_acc,
        )
        .await
        .unwrap();
        process_sse_event(r#"data: {"type":"message_stop"}"#, &tx, &mut usage_acc)
            .await
            .unwrap();

        let usage = loop {
            match rx.recv().await.unwrap().unwrap() {
                StreamChunk::Usage(usage) => break usage,
                _ => {}
            }
        };

        assert_eq!(usage.prompt_tokens, 10);
        assert_eq!(usage.completion_tokens, 7);
        assert_eq!(usage.total_tokens, 17);
        assert_eq!(usage.cache_creation_input_tokens, Some(4));
        assert_eq!(usage.cache_read_input_tokens, Some(3));
    }
}
