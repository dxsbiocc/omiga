//! Common types for LLM API abstraction

use serde::{Deserialize, Serialize};

/// Role in a conversation
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LlmRole {
    System,
    User,
    Assistant,
    Tool,
}

impl LlmRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            LlmRole::System => "system",
            LlmRole::User => "user",
            LlmRole::Assistant => "assistant",
            LlmRole::Tool => "tool",
        }
    }
}

/// Content block in a message
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LlmContent {
    /// Text content
    Text { text: String },
    /// Tool use (function call)
    ToolUse {
        id: String,
        name: String,
        arguments: serde_json::Value,
    },
    /// Tool result
    ToolResult {
        tool_use_id: String,
        content: String,
        is_error: Option<bool>,
    },
    /// Image (for vision models)
    Image {
        source: ImageSource,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ImageSource {
    /// Base64 encoded image
    Base64 {
        media_type: String,
        data: String,
    },
    /// URL to image
    Url {
        url: String,
    },
}

impl LlmContent {
    /// Create text content
    pub fn text(text: impl Into<String>) -> Self {
        LlmContent::Text { text: text.into() }
    }

    /// Create tool use content
    pub fn tool_use(id: impl Into<String>, name: impl Into<String>, arguments: serde_json::Value) -> Self {
        LlmContent::ToolUse {
            id: id.into(),
            name: name.into(),
            arguments,
        }
    }

    /// Create tool result content
    pub fn tool_result(tool_use_id: impl Into<String>, content: impl Into<String>) -> Self {
        LlmContent::ToolResult {
            tool_use_id: tool_use_id.into(),
            content: content.into(),
            is_error: None,
        }
    }

    /// Get text content if present
    pub fn as_text(&self) -> Option<&str> {
        match self {
            LlmContent::Text { text } => Some(text.as_str()),
            _ => None,
        }
    }

    /// Check if content is empty
    pub fn is_empty(&self) -> bool {
        match self {
            LlmContent::Text { text } => text.is_empty(),
            LlmContent::ToolResult { content, .. } => content.is_empty(),
            _ => false,
        }
    }
}

/// Message in a conversation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmMessage {
    pub role: LlmRole,
    pub content: Vec<LlmContent>,
    /// Optional name (for tool messages)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Tool calls (for assistant messages with tool use)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
}

impl LlmMessage {
    /// Create a system message
    pub fn system(text: impl Into<String>) -> Self {
        Self {
            role: LlmRole::System,
            content: vec![LlmContent::text(text)],
            name: None,
            tool_calls: None,
        }
    }

    /// Create a user message
    pub fn user(text: impl Into<String>) -> Self {
        Self {
            role: LlmRole::User,
            content: vec![LlmContent::text(text)],
            name: None,
            tool_calls: None,
        }
    }

    /// Create an assistant message
    pub fn assistant(text: impl Into<String>) -> Self {
        Self {
            role: LlmRole::Assistant,
            content: vec![LlmContent::text(text)],
            name: None,
            tool_calls: None,
        }
    }

    /// Create a tool message
    pub fn tool(tool_use_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: LlmRole::Tool,
            content: vec![LlmContent::tool_result(tool_use_id, content)],
            name: None,
            tool_calls: None,
        }
    }

    /// Get combined text content
    pub fn text_content(&self) -> String {
        self.content
            .iter()
            .filter_map(|c| c.as_text())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Tool call representation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: String,
    pub function: FunctionCall,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: String, // JSON string
}

/// Streaming response chunk
#[derive(Debug, Clone)]
pub enum LlmStreamChunk {
    /// Text delta
    Text(String),
    /// Tool use started
    ToolStart { id: String, name: String },
    /// Tool arguments (JSON delta for streaming)
    ToolArguments(String),
    /// Content block completed
    BlockStop,
    /// Provider-reported token usage (may appear once or multiple times per stream; last wins per request)
    Usage(TokenUsage),
    /// Message completed
    Stop { stop_reason: Option<String> },
    /// Keep-alive ping
    Ping,
    /// Error occurred
    Error(String),
}

impl LlmStreamChunk {
    /// Check if this is a stop chunk
    pub fn is_stop(&self) -> bool {
        matches!(self, LlmStreamChunk::Stop { .. })
    }

    /// Get text if this is a text chunk
    pub fn as_text(&self) -> Option<&str> {
        match self {
            LlmStreamChunk::Text(t) => Some(t.as_str()),
            _ => None,
        }
    }
}

/// Token usage information
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TokenUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

/// Complete response (non-streaming)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmResponse {
    pub id: String,
    pub model: String,
    pub content: Vec<LlmContent>,
    pub usage: TokenUsage,
    pub stop_reason: Option<String>,
}

/// Tool definition for API
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value, // JSON Schema
}

impl From<crate::domain::tools::ToolSchema> for LlmToolDefinition {
    fn from(schema: crate::domain::tools::ToolSchema) -> Self {
        Self {
            name: schema.name,
            description: schema.description,
            parameters: schema.parameters,
        }
    }
}

/// Streaming options
#[derive(Debug, Clone, Default)]
pub struct StreamOptions {
    pub include_usage: bool,
}

/// Request builder for LLM calls
#[derive(Debug, Clone, Default)]
pub struct LlmRequest {
    pub messages: Vec<LlmMessage>,
    pub tools: Vec<LlmToolDefinition>,
    pub stream: bool,
    pub stream_options: Option<StreamOptions>,
}

impl LlmRequest {
    pub fn new(messages: Vec<LlmMessage>) -> Self {
        Self {
            messages,
            tools: Vec::new(),
            stream: true,
            stream_options: None,
        }
    }

    pub fn with_tools(mut self, tools: Vec<LlmToolDefinition>) -> Self {
        self.tools = tools;
        self
    }

    pub fn non_streaming(mut self) -> Self {
        self.stream = false;
        self
    }

    pub fn with_usage(mut self) -> Self {
        self.stream_options = Some(StreamOptions { include_usage: true });
        self
    }
}

/// Convert from domain Message to LLM Message
impl From<crate::api::Message> for LlmMessage {
    fn from(msg: crate::api::Message) -> Self {
        let content: Vec<LlmContent> = msg
            .content
            .into_iter()
            .map(|block| match block {
                crate::api::ContentBlock::Text { text } => LlmContent::Text { text },
                crate::api::ContentBlock::ToolUse { id, name, input } => {
                    LlmContent::ToolUse { id, name, arguments: input }
                }
                crate::api::ContentBlock::ToolResult { tool_use_id, content, is_error } => {
                    LlmContent::ToolResult { tool_use_id, content, is_error }
                }
            })
            .collect();

        Self {
            role: match msg.role {
                crate::api::Role::User => LlmRole::User,
                crate::api::Role::Assistant => LlmRole::Assistant,
            },
            content,
            name: None,
            tool_calls: None,
        }
    }
}

/// Convert from LLM Message to domain Message (lossy - only text content)
impl From<LlmMessage> for crate::api::Message {
    fn from(msg: LlmMessage) -> Self {
        let content: Vec<crate::api::ContentBlock> = msg
            .content
            .into_iter()
            .map(|block| match block {
                LlmContent::Text { text } => crate::api::ContentBlock::Text { text },
                LlmContent::ToolUse { id, name, arguments } => {
                    crate::api::ContentBlock::ToolUse { id, name, input: arguments }
                }
                LlmContent::ToolResult { tool_use_id, content, is_error } => {
                    crate::api::ContentBlock::ToolResult { tool_use_id, content, is_error }
                }
                LlmContent::Image { .. } => {
                    // Images not supported in domain model
                    crate::api::ContentBlock::Text { text: "[Image]".to_string() }
                }
            })
            .collect();

        Self {
            role: match msg.role {
                LlmRole::System => crate::api::Role::User, // System goes as user prefix
                LlmRole::User => crate::api::Role::User,
                LlmRole::Assistant => crate::api::Role::Assistant,
                LlmRole::Tool => crate::api::Role::User, // Tool results go as user
            },
            content,
        }
    }
}
