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

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    ToolUse { id: String, name: String, input: serde_json::Value },
    #[serde(rename = "tool_result")]
    ToolResult { tool_use_id: String, content: String, is_error: Option<bool> },
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

/// Stream chunk for UI
#[derive(Debug, Clone)]
pub enum StreamChunk {
    Text(String),
    ToolStart { id: String, name: String },
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
    ContentBlockStart { index: usize, content_block: ContentBlockStart },
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
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
enum ContentBlockStart {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse { id: String, name: String, input: serde_json::Value },
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
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .expect("Failed to create HTTP client");
        Self { config, client }
    }

    pub async fn send_message_streaming(
        &self,
        messages: Vec<Message>,
        tools: Vec<ToolDefinition>,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamChunk, ApiError>> + Send>>, ApiError> {
        #[derive(Serialize)]
        struct Request {
            model: String,
            max_tokens: u32,
            messages: Vec<Message>,
            #[serde(skip_serializing_if = "Option::is_none")]
            system: Option<String>,
            #[serde(skip_serializing_if = "Option::is_none")]
            temperature: Option<f32>,
            tools: Vec<ToolDefinition>,
            stream: bool,
        }

        let request = Request {
            model: self.config.model.clone(),
            max_tokens: self.config.max_tokens,
            messages,
            system: self.config.system.clone(),
            temperature: self.config.temperature,
            tools,
            stream: true,
        };

        let response = self
            .client
            .post(&self.config.api_url)
            .headers(self.config.headers())
            .json(&request)
            .send()
            .await
            .map_err(|e| ApiError::Network { message: e.to_string() })?;

        let status = response.status();
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            return Err(ApiError::Http {
                status: status.as_u16(),
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

                            if let Err(_) =
                                process_sse_event(&event, &tx, &mut usage_acc).await
                            {
                                return; // Channel closed
                            }
                        }
                    }
                    Err(_) => {
                        let _ = tx.send(Err(ApiError::SseParse {
                            message: "Invalid UTF-8 in stream".to_string(),
                        })).await;
                        return;
                    }
                }
            }
            Err(e) => {
                let _ = tx.send(Err(ApiError::Network {
                    message: format!("Stream error: {}", e),
                })).await;
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
        if line.starts_with("data: ") {
            data = Some(&line[6..]);
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
                        if let Some(i) = u.input_tokens {
                            usage_acc.prompt_tokens = i;
                        }
                        if let Some(o) = u.output_tokens {
                            usage_acc.completion_tokens = o;
                        }
                    }
                    Ok(())
                }
                StreamEvent::ContentBlockStart { content_block, .. } => {
                    match content_block {
                        ContentBlockStart::Text { .. } => {
                            // Text block started - no special handling
                            Ok(())
                        }
                        ContentBlockStart::ToolUse { id, name, .. } => {
                            tx.send(Ok(StreamChunk::ToolStart { id, name }))
                                .await
                                .map_err(|_| ())
                        }
                    }
                }
                StreamEvent::ContentBlockDelta { delta, .. } => {
                    match delta {
                        ContentDelta::TextDelta { text } => {
                            tx.send(Ok(StreamChunk::Text(text)))
                                .await
                                .map_err(|_| ())
                        }
                        ContentDelta::InputJsonDelta { partial_json } => {
                            tx.send(Ok(StreamChunk::ToolJson(partial_json)))
                                .await
                                .map_err(|_| ())
                        }
                    }
                }
                StreamEvent::ContentBlockStop { .. } => {
                    tx.send(Ok(StreamChunk::BlockStop))
                        .await
                        .map_err(|_| ())
                }
                StreamEvent::MessageDelta { usage, .. } => {
                    if let Some(u) = usage {
                        if let Some(i) = u.input_tokens {
                            usage_acc.prompt_tokens = i;
                        }
                        if let Some(o) = u.output_tokens {
                            usage_acc.completion_tokens = o;
                        }
                    }
                    Ok(())
                }
                StreamEvent::MessageStop => {
                    usage_acc.total_tokens = usage_acc
                        .prompt_tokens
                        .saturating_add(usage_acc.completion_tokens);
                    if usage_acc.prompt_tokens > 0 || usage_acc.completion_tokens > 0 {
                        let _ = tx
                            .send(Ok(StreamChunk::Usage(usage_acc.clone())))
                            .await;
                    }
                    tx.send(Ok(StreamChunk::Stop)).await.map_err(|_| ())
                }
                StreamEvent::Ping => {
                    tx.send(Ok(StreamChunk::Ping))
                        .await
                        .map_err(|_| ())
                }
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
                message: format!("Parse error: {} | Data: {}", e, data.chars().take(200).collect::<String>()),
            }))
            .await
            .map_err(|_| ())
        }
    }
}

