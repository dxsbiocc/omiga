//! Server-Sent Events (SSE) parsing for Claude API streaming

use super::models::*;
use futures::{Stream, TryStreamExt};
use serde::Deserialize;

/// Parse an SSE stream into StreamEvents
pub fn parse_sse_stream<S>(
    stream: S,
) -> impl Stream<Item = Result<StreamEvent, SseError>>
where
    S: Stream<Item = Result<eventsource_stream::Event, reqwest::Error>>,
{
    stream.try_filter_map(|event| async move {
        if let Some(data) = event.data {
            parse_event(&data)
        } else {
            Ok(None)
        }
    })
}

/// Parse a single SSE event
fn parse_event(data: &str) -> Result<Option<StreamEvent>, SseError> {
    // Handle special events
    if data == "[DONE]" {
        return Ok(None);
    }

    // Parse as JSON
    match serde_json::from_str::<StreamEvent>(data) {
        Ok(event) => Ok(Some(event)),
        Err(e) => {
            // Try to extract useful error info
            if let Ok(error_obj) = serde_json::from_str::<serde_json::Value>(data) {
                if let Some(error) = error_obj.get("error") {
                    return Err(SseError::ApiError {
                        message: error.get("message")
                            .and_then(|m| m.as_str())
                            .unwrap_or("Unknown error")
                            .to_string(),
                        code: error.get("type")
                            .and_then(|t| t.as_str())
                            .map(|s| s.to_string()),
                    });
                }
            }
            Err(SseError::ParseError(format!("Failed to parse SSE event: {}", e)))
        }
    }
}

/// Convert StreamEvent to StreamChunk (simplified for UI consumption)
pub fn event_to_chunk(event: StreamEvent) -> Option<StreamChunk> {
    match event {
        StreamEvent::MessageStart { message } => {
            Some(StreamChunk::Start(message))
        }
        StreamEvent::ContentBlockDelta { delta, .. } => {
            match delta {
                ContentDelta::TextDelta { text } => {
                    Some(StreamChunk::Text(text))
                }
                ContentDelta::InputJsonDelta { partial_json } => {
                    Some(StreamChunk::ToolJson(partial_json))
                }
            }
        }
        StreamEvent::ContentBlockStart { content_block, .. } => {
            match content_block {
                ContentBlock::ToolUse { id, name, .. } => {
                    Some(StreamChunk::ToolStart { id, name })
                }
                _ => None,
            }
        }
        StreamEvent::ContentBlockStop { .. } => {
            Some(StreamChunk::BlockStop)
        }
        StreamEvent::MessageStop => {
            Some(StreamChunk::Stop)
        }
        StreamEvent::Ping => {
            Some(StreamChunk::Ping)
        }
        _ => None,
    }
}

/// SSE parsing errors
#[derive(Debug, thiserror::Error)]
pub enum SseError {
    #[error("API error: {message} (code: {code:?})")]
    ApiError { message: String, code: Option<String> },
    #[error("Parse error: {0}")]
    ParseError(String),
    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),
}

/// Helper to collect streaming text into a complete message
pub async fn collect_stream<S>(stream: S) -> Result<(String, Vec<ToolCall>), SseError>
where
    S: Stream<Item = Result<StreamEvent, SseError>>,
{
    use futures::StreamExt;

    let mut text_parts = Vec::new();
    let mut current_tool: Option<(String, String)> = None;
    let mut tool_calls = Vec::new();
    let mut tool_json_parts = Vec::new();

    let mut stream = stream;

    while let Some(result) = stream.next().await {
        let event = result?;

        match event {
            StreamEvent::ContentBlockDelta { delta, .. } => {
                match delta {
                    ContentDelta::TextDelta { text } => {
                        text_parts.push(text);
                    }
                    ContentDelta::InputJsonDelta { partial_json } => {
                        tool_json_parts.push(partial_json);
                    }
                }
            }
            StreamEvent::ContentBlockStart { content_block, .. } => {
                if let ContentBlock::ToolUse { id, name, .. } = content_block {
                    current_tool = Some((id, name));
                    tool_json_parts.clear();
                }
            }
            StreamEvent::ContentBlockStop { .. } => {
                if let Some((id, name)) = current_tool.take() {
                    let json = tool_json_parts.join("");
                    tool_calls.push(ToolCall { id, name, arguments: json });
                    tool_json_parts.clear();
                }
            }
            StreamEvent::MessageStop => {
                break;
            }
            _ => {}
        }
    }

    let text = text_parts.join("");
    Ok((text, tool_calls))
}

/// Tool call extracted from stream
#[derive(Debug, Clone)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}
