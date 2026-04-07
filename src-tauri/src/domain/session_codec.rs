//! Session codec - Message serialization/deserialization between database and domain
//!
//! This module eliminates duplication by centralizing all record <-> domain conversions.

use super::persistence::{MessageRecord, SessionWithMessages};
use super::session::{Message, Session, ToolCall};
use crate::api::{ContentBlock, Message as ApiMessage, Role};

/// Codec for session-related conversions
pub struct SessionCodec;

impl SessionCodec {
    /// Convert database session with messages to domain Session
    pub fn db_to_domain(db_session: SessionWithMessages) -> Session {
        let messages: Vec<Message> = db_session
            .messages
            .into_iter()
            .map(Self::record_to_message)
            .collect();

        Session {
            id: db_session.id,
            name: db_session.name,
            project_path: db_session.project_path,
            messages,
            created_at: chrono::DateTime::parse_from_rfc3339(&db_session.created_at)
                .map(|dt| dt.with_timezone(&chrono::Utc))
                .unwrap_or_else(|_| chrono::Utc::now()),
            updated_at: chrono::DateTime::parse_from_rfc3339(&db_session.updated_at)
                .map(|dt| dt.with_timezone(&chrono::Utc))
                .unwrap_or_else(|_| chrono::Utc::now()),
        }
    }

    /// Convert a database MessageRecord to domain Message
    pub fn record_to_message(record: MessageRecord) -> Message {
        match record.role.as_str() {
            "assistant" => {
                let tool_calls = record.tool_calls.and_then(|tc| {
                    serde_json::from_str::<Vec<ToolCall>>(&tc).ok()
                });
                Message::Assistant {
                    content: record.content,
                    tool_calls,
                }
            }
            "tool" => Message::Tool {
                tool_call_id: record.tool_call_id.unwrap_or_default(),
                output: record.content,
            },
            _ => Message::User {
                content: record.content,
            },
        }
    }

    /// Convert a domain Message to database-ready tuple
    /// Returns: (id, session_id, role, content, tool_calls_json, tool_call_id)
    pub fn message_to_record(
        message: &Message,
        id: &str,
        session_id: &str,
    ) -> (String, String, String, String, Option<String>, Option<String>) {
        match message {
            Message::User { content } => (
                id.to_string(),
                session_id.to_string(),
                "user".to_string(),
                content.clone(),
                None,
                None,
            ),
            Message::Assistant { content, tool_calls } => {
                let tool_calls_json = tool_calls.as_ref().map(|tc| {
                    serde_json::to_string(tc).unwrap_or_default()
                });
                (
                    id.to_string(),
                    session_id.to_string(),
                    "assistant".to_string(),
                    content.clone(),
                    tool_calls_json,
                    None,
                )
            }
            Message::Tool { tool_call_id, output } => (
                id.to_string(),
                session_id.to_string(),
                "tool".to_string(),
                output.clone(),
                None,
                Some(tool_call_id.clone()),
            ),
        }
    }

    /// Convert domain Session messages to Claude API format
    pub fn to_api_messages(messages: &[Message]) -> Vec<ApiMessage> {
        messages
            .iter()
            .filter_map(|msg| match msg {
                Message::User { content } => Some(ApiMessage {
                    role: Role::User,
                    content: vec![ContentBlock::text(content.clone())],
                }),
                Message::Assistant { content, tool_calls } => {
                    let mut blocks: Vec<ContentBlock> = vec![ContentBlock::text(content.clone())];

                    // Add tool use blocks if present
                    if let Some(calls) = tool_calls {
                        for call in calls {
                            blocks.push(ContentBlock::ToolUse {
                                id: call.id.clone(),
                                name: call.name.clone(),
                                input: serde_json::from_str(&call.arguments).unwrap_or_default(),
                            });
                        }
                    }

                    Some(ApiMessage {
                        role: Role::Assistant,
                        content: blocks,
                    })
                }
                Message::Tool { tool_call_id, output } => Some(ApiMessage {
                    role: Role::User,
                    content: vec![ContentBlock::ToolResult {
                        tool_use_id: tool_call_id.clone(),
                        content: output.clone(),
                        is_error: None,
                    }],
                }),
            })
            .collect()
    }

    /// Extract tool calls from assistant message for database storage
    pub fn extract_tool_calls(message: &Message) -> Option<String> {
        match message {
            Message::Assistant { tool_calls, .. } => {
                tool_calls.as_ref().map(|tc| {
                    serde_json::to_string(tc).unwrap_or_default()
                })
            }
            _ => None,
        }
    }

    /// Parse tool calls from JSON string
    pub fn parse_tool_calls(json: &str) -> Option<Vec<ToolCall>> {
        serde_json::from_str(json).ok()
    }

    /// Build a Message::Assistant from content and optional tool calls JSON
    pub fn build_assistant_message(content: &str, tool_calls_json: Option<&str>) -> Message {
        let tool_calls = tool_calls_json.and_then(|tc| Self::parse_tool_calls(tc));
        Message::Assistant {
            content: content.to_string(),
            tool_calls,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_user_message_roundtrip() {
        let msg = Message::User {
            content: "Hello".to_string(),
        };
        let (id, session_id, role, content, tool_calls, tool_call_id) =
            SessionCodec::message_to_record(&msg, "msg-1", "sess-1");

        assert_eq!(id, "msg-1");
        assert_eq!(session_id, "sess-1");
        assert_eq!(role, "user");
        assert_eq!(content, "Hello");
        assert!(tool_calls.is_none());
        assert!(tool_call_id.is_none());
    }

    #[test]
    fn test_assistant_message_with_tool_calls() {
        let tool_calls = vec![ToolCall {
            id: "call-1".to_string(),
            name: "read_file".to_string(),
            arguments: r#"{"path": "test.txt"}"#.to_string(),
        }];
        let msg = Message::Assistant {
            content: "Let me read that file".to_string(),
            tool_calls: Some(tool_calls),
        };

        let (_, _, role, content, tool_calls_json, _) =
            SessionCodec::message_to_record(&msg, "msg-1", "sess-1");

        assert_eq!(role, "assistant");
        assert_eq!(content, "Let me read that file");
        assert!(tool_calls_json.is_some());

        // Verify we can parse it back
        let parsed = SessionCodec::parse_tool_calls(&tool_calls_json.unwrap());
        assert!(parsed.is_some());
        assert_eq!(parsed.unwrap().len(), 1);
    }

    #[test]
    fn test_record_to_message_conversion() {
        let record = MessageRecord {
            id: "msg-1".to_string(),
            session_id: "sess-1".to_string(),
            role: "tool".to_string(),
            content: "File contents".to_string(),
            tool_calls: None,
            tool_call_id: Some("call-1".to_string()),
            created_at: chrono::Utc::now().to_rfc3339(),
        };

        let msg = SessionCodec::record_to_message(record);
        match msg {
            Message::Tool { tool_call_id, output } => {
                assert_eq!(tool_call_id, "call-1");
                assert_eq!(output, "File contents");
            }
            _ => panic!("Expected Tool message"),
        }
    }
}
