//! Anthropic Claude API client adapter
//!
//! Adapts the existing Claude API implementation to the LlmClient trait

use super::{LlmClient, LlmConfig, LlmMessage, LlmStreamChunk};
use crate::api::{ClaudeClient as InnerClient, ClaudeConfig as InnerConfig, ContentBlock, Message, Role, StreamChunk};
use crate::domain::tools::ToolSchema;
use crate::errors::ApiError;
use async_trait::async_trait;
use futures::{Stream, StreamExt};
use std::pin::Pin;

/// Anthropic Claude client
pub struct AnthropicClient {
    inner: InnerClient,
    config: LlmConfig,
}

impl AnthropicClient {
    pub fn new(config: LlmConfig) -> Self {
        let inner_config = InnerConfig {
            api_key: config.api_key.clone(),
            api_url: config.api_url(),
            model: config.model.clone(),
            max_tokens: config.max_tokens,
            temperature: config.temperature,
            system: config.system_prompt.clone(),
            version: "2023-06-01".to_string(),
        };

        Self {
            inner: InnerClient::new(inner_config),
            config,
        }
    }
}

#[async_trait]
impl LlmClient for AnthropicClient {
    async fn send_message_streaming(
        &self,
        messages: Vec<LlmMessage>,
        tools: Vec<ToolSchema>,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<LlmStreamChunk, ApiError>> + Send>>, ApiError> {
        // Convert LlmMessage to inner Message format
        let inner_messages: Vec<Message> = messages
            .into_iter()
            .map(|msg| Message {
                role: match msg.role {
                    super::LlmRole::User => Role::User,
                    super::LlmRole::Assistant => Role::Assistant,
                    _ => Role::User, // System and Tool map to User
                },
                content: msg
                    .content
                    .into_iter()
                    .filter_map(|c| match c {
                        super::LlmContent::Text { text } => Some(ContentBlock::Text { text }),
                        super::LlmContent::ToolUse { id, name, arguments } => {
                            Some(ContentBlock::ToolUse { id, name, input: arguments })
                        }
                        super::LlmContent::ToolResult {
                            tool_use_id,
                            content,
                            is_error,
                        } => Some(ContentBlock::ToolResult {
                            tool_use_id,
                            content,
                            is_error,
                        }),
                        _ => None,
                    })
                    .collect(),
            })
            .collect();

        // Convert tools - pass ToolSchema directly to tools_to_definitions
        let tool_defs = InnerClient::tools_to_definitions(&tools);

        // Call inner client
        let stream = self
            .inner
            .send_message_streaming(inner_messages, tool_defs)
            .await?;

        // Convert StreamChunk to LlmStreamChunk
        let converted = stream.map(|result| {
            result.map(|chunk| match chunk {
                StreamChunk::Text(text) => LlmStreamChunk::Text(text),
                StreamChunk::ToolStart { id, name } => LlmStreamChunk::ToolStart { id, name },
                StreamChunk::ToolJson(json) => LlmStreamChunk::ToolArguments(json),
                StreamChunk::BlockStop => LlmStreamChunk::BlockStop,
                StreamChunk::Usage(u) => LlmStreamChunk::Usage(u),
                StreamChunk::Stop => LlmStreamChunk::Stop { stop_reason: None },
                StreamChunk::Ping => LlmStreamChunk::Ping,
            })
        });

        Ok(Box::pin(converted))
    }

    async fn health_check(&self) -> Result<bool, ApiError> {
        // Simple health check - try to make a minimal request
        // In production, you might want a dedicated endpoint
        Ok(true) // Assume healthy if client created successfully
    }

    fn config(&self) -> &LlmConfig {
        &self.config
    }
}
