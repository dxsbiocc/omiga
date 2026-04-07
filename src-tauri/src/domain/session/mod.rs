//! Session management

pub mod agent_task;

use serde::{Deserialize, Serialize};

pub use agent_task::{AgentTask, TaskV2Status};

/// A chat session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub name: String,
    pub project_path: String,
    pub messages: Vec<Message>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl Session {
    /// Create a new session
    pub fn new(name: impl Into<String>, project_path: impl Into<String>) -> Self {
        let now = chrono::Utc::now();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.into(),
            project_path: project_path.into(),
            messages: vec![],
            created_at: now,
            updated_at: now,
        }
    }

    /// Add a user message
    pub fn add_user_message(&mut self, content: impl Into<String>) {
        self.messages.push(Message::User {
            content: content.into(),
        });
        self.updated_at = chrono::Utc::now();
    }

    /// Add an assistant message (no tool calls)
    pub fn add_assistant_message(&mut self, content: impl Into<String>) {
        self.add_assistant_message_with_tools(content, None);
    }

    /// Add an assistant message, optionally with tool calls (matches API / DB shape)
    pub fn add_assistant_message_with_tools(
        &mut self,
        content: impl Into<String>,
        tool_calls: Option<Vec<ToolCall>>,
    ) {
        self.messages.push(Message::Assistant {
            content: content.into(),
            tool_calls,
        });
        self.updated_at = chrono::Utc::now();
    }

    /// Add a tool result
    pub fn add_tool_result(&mut self, tool_call_id: impl Into<String>, output: impl Into<String>) {
        self.messages.push(Message::Tool {
            tool_call_id: tool_call_id.into(),
            output: output.into(),
        });
        self.updated_at = chrono::Utc::now();
    }

    /// Convert messages to Claude API format
    pub fn to_api_messages(&self) -> Vec<crate::api::Message> {
        self.messages
            .iter()
            .map(|msg| match msg {
                Message::User { content } => crate::api::Message {
                    role: crate::api::Role::User,
                    content: vec![crate::api::ContentBlock::text(content.clone())],
                },
                Message::Assistant { content, .. } => crate::api::Message {
                    role: crate::api::Role::Assistant,
                    content: vec![crate::api::ContentBlock::text(content.clone())],
                },
                Message::Tool { output, .. } => crate::api::Message {
                    role: crate::api::Role::User,
                    content: vec![crate::api::ContentBlock::text(output.clone())],
                },
            })
            .collect()
    }
}

/// A chat message
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "role", rename_all = "lowercase")]
pub enum Message {
    User { content: String },
    Assistant {
        content: String,
        tool_calls: Option<Vec<ToolCall>>,
    },
    Tool {
        tool_call_id: String,
        output: String,
    },
}

/// Task status for session todo list (matches `src/utils/todo/types.ts`)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TodoStatus {
    Pending,
    InProgress,
    Completed,
}

/// One row in the session todo list
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TodoItem {
    pub content: String,
    pub status: TodoStatus,
    /// Present continuous label (TS `activeForm`)
    #[serde(rename = "activeForm", alias = "active_form")]
    pub active_form: String,
}

/// A tool call
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

/// Convert messages to Anthropic API format
pub fn to_anthropic_messages(
    messages: &[Message],
) -> Vec<serde_json::Value> {
    messages
        .iter()
        .map(|msg| match msg {
            Message::User { content } => serde_json::json!({
                "role": "user",
                "content": content
            }),
            Message::Assistant { content, tool_calls } => {
                let mut msg = serde_json::json!({
                    "role": "assistant",
                    "content": content
                });
                if let Some(calls) = tool_calls {
                    msg["tool_calls"] = serde_json::json!(calls);
                }
                msg
            }
            Message::Tool { tool_call_id, output } => serde_json::json!({
                "role": "tool",
                "tool_call_id": tool_call_id,
                "content": output
            }),
        })
        .collect()
}
