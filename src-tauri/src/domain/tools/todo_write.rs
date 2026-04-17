//! TodoWrite — session task checklist (matches `src/tools/TodoWriteTool`)

use super::{ToolContext, ToolError, ToolSchema};
use crate::domain::session::{TodoItem, TodoStatus};
use crate::infrastructure::streaming::{StreamOutput, StreamOutputItem};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::pin::Pin;

const MAX_TODOS: usize = 100;

pub const DESCRIPTION: &str = r#"Update the session todo list for multi-step coding work.

Each item requires:
- `content`: imperative description (e.g. "Run tests")
- `activeForm`: present continuous (e.g. "Running tests")
- `status`: pending | in_progress | completed

Guidelines: keep exactly one task `in_progress` when the list is non-empty; mark completed as soon as work finishes; when all tasks are completed the list is cleared automatically.

Use proactively for complex tasks; skip for single trivial steps."#;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoWriteArgs {
    pub todos: Vec<TodoItem>,
}

pub struct TodoWriteTool;

fn validate_todos(todos: &[TodoItem]) -> Result<(), ToolError> {
    if todos.len() > MAX_TODOS {
        return Err(ToolError::InvalidArguments {
            message: format!("Too many todos (max {})", MAX_TODOS),
        });
    }
    for t in todos {
        if t.content.trim().is_empty() {
            return Err(ToolError::InvalidArguments {
                message: "Each todo must have non-empty `content`".to_string(),
            });
        }
        if t.active_form.trim().is_empty() {
            return Err(ToolError::InvalidArguments {
                message: "Each todo must have non-empty `activeForm`".to_string(),
            });
        }
    }
    Ok(())
}

#[async_trait]
impl super::ToolImpl for TodoWriteTool {
    type Args = TodoWriteArgs;

    const DESCRIPTION: &'static str = DESCRIPTION;

    async fn execute(
        ctx: &ToolContext,
        args: Self::Args,
    ) -> Result<crate::infrastructure::streaming::StreamOutputBox, ToolError> {
        validate_todos(&args.todos)?;

        let store = ctx
            .todos
            .as_ref()
            .ok_or_else(|| ToolError::ExecutionFailed {
                message: "Todo list is not available in this context (no active chat session)."
                    .to_string(),
            })?;

        let input = args.todos;
        let all_done = input
            .iter()
            .all(|t| matches!(t.status, TodoStatus::Completed));

        // Match TS: when all completed (including empty input), clear stored list
        let stored: Vec<TodoItem> = if all_done { vec![] } else { input.clone() };

        let old = {
            let mut g = store.lock().await;
            let old = g.clone();
            *g = stored.clone();
            old
        };

        let mut text = String::from(
            "Todos have been modified successfully. Ensure that you continue to use the todo list to track your progress. Please proceed with the current tasks if applicable.\n\n",
        );

        text.push_str(&format!(
            "Previous tasks: {} | Current tasks stored: {}\n",
            old.len(),
            stored.len()
        ));

        if !input.is_empty() && !all_done {
            let in_progress = input
                .iter()
                .filter(|t| matches!(t.status, TodoStatus::InProgress))
                .count();
            if in_progress != 1 {
                text.push_str(&format!(
                    "Note: Prefer exactly one task as `in_progress` at a time (now: {}).\n",
                    in_progress
                ));
            }
        }

        if stored.is_empty() && !input.is_empty() && all_done {
            text.push_str("All tasks completed; session todo list cleared.\n");
        } else if !stored.is_empty() {
            text.push_str("\nCurrent tasks:\n");
            for t in &stored {
                let st = match t.status {
                    TodoStatus::Pending => "pending",
                    TodoStatus::InProgress => "in_progress",
                    TodoStatus::Completed => "completed",
                };
                text.push_str(&format!("- [{}] {} — {}\n", st, t.content, t.active_form));
            }
        }

        Ok(TodoWriteOutput { text }.into_stream())
    }
}

#[derive(Debug, Clone)]
struct TodoWriteOutput {
    text: String,
}

impl StreamOutput for TodoWriteOutput {
    fn into_stream(self) -> Pin<Box<dyn futures::Stream<Item = StreamOutputItem> + Send>> {
        use futures::stream;
        Box::pin(stream::iter(vec![
            StreamOutputItem::Start,
            StreamOutputItem::Content(self.text),
            StreamOutputItem::Complete,
        ]))
    }
}

pub fn schema() -> ToolSchema {
    ToolSchema::new(
        "todo_write",
        DESCRIPTION,
        serde_json::json!({
            "type": "object",
            "properties": {
                "todos": {
                    "type": "array",
                    "description": "Full replacement list for this session",
                    "items": {
                        "type": "object",
                        "properties": {
                            "content": { "type": "string", "description": "Imperative task description" },
                            "status": {
                                "type": "string",
                                "enum": ["pending", "in_progress", "completed"]
                            },
                            "activeForm": { "type": "string", "description": "Present continuous label while working" }
                        },
                        "required": ["content", "status", "activeForm"]
                    }
                }
            },
            "required": ["todos"]
        }),
    )
}
