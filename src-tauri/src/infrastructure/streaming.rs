//! Streaming output abstraction
//!
//! Design decision (from eng review):
//! - Unified StreamOutput trait for all streaming outputs
//! - Handles: Claude API SSE, Bash stdout/stderr, Grep results, File reads
//! - Enables real-time UI updates via Tauri events

use futures::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use std::pin::Pin;

/// A single item in a streaming output
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum StreamOutputItem {
    /// Stream started
    Start,
    /// Text content chunk (for LLM streaming)
    #[serde(rename = "text")]
    Text(String),
    /// Tool use started (may be sent twice: placeholder then full `arguments` at block end)
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        arguments: String,
    },
    /// Tool result chunk
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        name: String,
        input: String,
        output: String,
        is_error: bool,
    },
    /// `ask_user_question` is waiting for the user to submit choices in the chat UI.
    #[serde(rename = "ask_user_pending")]
    AskUserPending {
        session_id: String,
        message_id: String,
        tool_use_id: String,
        questions: serde_json::Value,
    },
    /// Stdout line from bash
    #[serde(rename = "stdout")]
    Stdout(String),
    /// Stderr line from bash
    #[serde(rename = "stderr")]
    Stderr(String),
    /// Exit code from bash
    #[serde(rename = "exit_code")]
    ExitCode(i32),
    /// Grep match result
    #[serde(rename = "grep_match")]
    GrepMatch(GrepMatch),
    /// Glob match result
    #[serde(rename = "glob_match")]
    GlobMatch(GlobMatch),
    /// Complete file/directory listing
    #[serde(rename = "file_list")]
    FileList(Vec<FileEntry>),
    /// Complete content block
    #[serde(rename = "content")]
    Content(String),
    /// Metadata key-value pair
    #[serde(rename = "metadata")]
    Metadata { key: String, value: String },
    /// Thinking/reasoning content
    #[serde(rename = "thinking")]
    Thinking(String),
    /// Error occurred
    #[serde(rename = "error")]
    Error {
        message: String,
        code: Option<String>,
    },
    /// Stream was cancelled by user
    #[serde(rename = "cancelled")]
    Cancelled,
    /// Optional short recap after the turn (independent LLM; omitted when model skips)
    #[serde(rename = "turn_summary")]
    TurnSummary { text: Option<String> },
    /// Suggested follow-up prompts for the composer (independent LLM; emitted before [`Complete`] when generation succeeds)
    #[serde(rename = "follow_up_suggestions")]
    FollowUpSuggestions(Vec<FollowUpSuggestion>),
    /// Indicator that follow-up suggestions are being generated in the background
    #[serde(rename = "suggestions_generating")]
    SuggestionsGenerating,
    /// Aggregated LLM token usage for this user turn (main agent only; excludes post-turn summary / follow-up LLM calls)
    #[serde(rename = "token_usage")]
    TokenUsage {
        prompt_tokens: u32,
        completion_tokens: u32,
        total_tokens: u32,
        provider: String,
    },
    /// Stream completed successfully
    #[serde(rename = "complete")]
    Complete,
}

/// One quick-reply row: short UI label + full text to place in the composer
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FollowUpSuggestion {
    pub label: String,
    pub prompt: String,
}

/// A grep match result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrepMatch {
    pub file: String,
    pub line: usize,
    pub column: usize,
    pub content: String,
}

/// A glob match result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobMatch {
    pub path: String,
    pub is_file: bool,
    pub size: u64,
}

/// A file/directory entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub is_directory: bool,
    pub size: Option<u64>,
    pub modified: Option<String>,
}

/// Trait for types that can produce a stream of output items
///
/// This is implemented by:
/// - BashOutput (streams stdout/stderr lines)
/// - ClaudeStream (streams LLM response chunks)
/// - GrepOutput (streams match results)
/// - GlobOutput (streams file matches)
/// - FileReadOutput (streams content chunks)
pub trait StreamOutput: Send {
    /// Convert this output into a stream of items
    fn into_stream(self) -> Pin<Box<dyn Stream<Item = StreamOutputItem> + Send>>;
}

/// Adapter to convert any Stream<Item = StreamOutputItem> into StreamOutput
pub struct StreamAdapter<S>(S);

impl<S> StreamOutput for StreamAdapter<S>
where
    S: Stream<Item = StreamOutputItem> + Send + 'static,
{
    fn into_stream(self) -> Pin<Box<dyn Stream<Item = StreamOutputItem> + Send>> {
        Box::pin(self.0)
    }
}

/// A boxed stream output type alias
pub type StreamOutputBox = Pin<Box<dyn Stream<Item = StreamOutputItem> + Send>>;

/// Helper to create a stream from an iterator
pub fn stream_from_iter<I>(items: I) -> StreamOutputBox
where
    I: IntoIterator<Item = StreamOutputItem> + Send + 'static,
    I::IntoIter: Send,
{
    Box::pin(futures::stream::iter(items))
}

/// Helper to create a single-item stream
pub fn stream_single(item: StreamOutputItem) -> StreamOutputBox {
    stream_from_iter(vec![item, StreamOutputItem::Complete])
}

/// Wrap a stream with metadata prefix
pub fn stream_with_metadata<S>(
    metadata: Vec<(String, String)>,
    stream: S,
) -> Pin<Box<dyn Stream<Item = StreamOutputItem> + Send>>
where
    S: Stream<Item = StreamOutputItem> + Send + 'static,
{
    let metadata_items: Vec<StreamOutputItem> = metadata
        .into_iter()
        .map(|(k, v)| StreamOutputItem::Metadata { key: k, value: v })
        .collect();

    Box::pin(futures::stream::iter(metadata_items).chain(stream))
}

/// SSE (Server-Sent Events) parser for Claude API
pub mod sse {
    use super::StreamOutputItem;
    use eventsource_stream::Event;
    use futures::Stream;

    /// Parse SSE events into stream items
    pub fn parse_sse_stream<S>(stream: S) -> impl Stream<Item = Result<StreamOutputItem, String>>
    where
        S: Stream<Item = Result<Event, reqwest::Error>> + Send + 'static,
    {
        use futures::StreamExt;
        stream
            .map(|event| event.map_err(|e| e.to_string()))
            .filter_map(|event| async move {
                match event {
                    Ok(e) if !e.data.is_empty() => match parse_sse_event(&e.data) {
                        Ok(Some(item)) => Some(Ok(item)),
                        Ok(None) => None,
                        Err(err) => Some(Err(err)),
                    },
                    _ => None,
                }
            })
    }

    /// Parse a single SSE event
    fn parse_sse_event(data: &str) -> Result<Option<StreamOutputItem>, String> {
        // Parse Claude API SSE events
        // Events look like:
        // event: content_block_delta
        // data: {"type": "content_block_delta", ...}

        if data.starts_with("{event: ") {
            // Extract event type and data
            let parts: Vec<&str> = data.splitn(2, "\ndata: ").collect();
            if parts.len() == 2 {
                let event_type = parts[0].trim_start_matches("{event: ").trim();
                let event_data = parts[1];

                return match event_type {
                    "content_block_delta" => parse_content_delta(event_data),
                    "message_start" => parse_message_start(event_data),
                    "message_stop" => Ok(Some(StreamOutputItem::Complete)),
                    _ => Ok(None),
                };
            }
        }

        // Try parsing as JSON directly
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(data) {
            if let Some(event_type) = json.get("type").and_then(|t| t.as_str()) {
                return match event_type {
                    "content_block_delta" => parse_content_delta_value(&json),
                    "message_start" => Ok(None), // Skip
                    "message_stop" => Ok(Some(StreamOutputItem::Complete)),
                    _ => Ok(None),
                };
            }
        }

        Ok(None)
    }

    /// Parse content delta event
    fn parse_content_delta(data: &str) -> Result<Option<StreamOutputItem>, String> {
        let value: serde_json::Value =
            serde_json::from_str(data).map_err(|e| format!("Failed to parse delta: {}", e))?;

        parse_content_delta_value(&value)
    }

    fn parse_content_delta_value(
        value: &serde_json::Value,
    ) -> Result<Option<StreamOutputItem>, String> {
        if let Some(delta) = value.get("delta") {
            // Text delta
            if let Some(text) = delta.get("text").and_then(|t| t.as_str()) {
                return Ok(Some(StreamOutputItem::Text(text.to_string())));
            }

            // Thinking delta
            if let Some(thinking) = delta.get("thinking").and_then(|t| t.as_str()) {
                return Ok(Some(StreamOutputItem::Thinking(thinking.to_string())));
            }

            // Tool use
            if let Some(tool_use) = value.get("tool_use") {
                let id = tool_use
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let name = tool_use
                    .get("name")
                    .and_then(|n| n.as_str())
                    .unwrap_or("unknown");
                let arguments = tool_use
                    .get("input")
                    .map(|i| i.to_string())
                    .unwrap_or_else(|| "{}".to_string());
                return Ok(Some(StreamOutputItem::ToolUse {
                    id,
                    name: name.to_string(),
                    arguments,
                }));
            }
        }

        Ok(None)
    }

    fn parse_message_start(_data: &str) -> Result<Option<StreamOutputItem>, String> {
        // Message start event - just signal start
        Ok(Some(StreamOutputItem::Start))
    }
}

/// Channel-based streaming for Tauri events
pub mod tauri_stream {
    use super::StreamOutputItem;
    use std::sync::Arc;
    use tokio::sync::mpsc;

    /// A channel-based stream that can be sent across Tauri commands
    pub struct TauriStream {
        receiver: Arc<tokio::sync::Mutex<mpsc::Receiver<StreamOutputItem>>>,
    }

    impl TauriStream {
        /// Create a new stream with given buffer size
        pub fn new(buffer: usize) -> (Self, mpsc::Sender<StreamOutputItem>) {
            let (sender, receiver) = mpsc::channel(buffer);
            let stream = Self {
                receiver: Arc::new(tokio::sync::Mutex::new(receiver)),
            };
            (stream, sender)
        }

        /// Get the receiver
        pub async fn recv(&self) -> Option<StreamOutputItem> {
            let mut receiver = self.receiver.lock().await;
            receiver.recv().await
        }
    }
}

#[cfg(test)]
#[path = "streaming_tests.rs"]
mod tests;
