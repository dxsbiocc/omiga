//! List V2 tasks — aligned with `TaskListTool` (`TaskList`).

use super::{ToolContext, ToolError, ToolSchema};
use crate::domain::session::TaskV2Status;
use crate::infrastructure::streaming::{StreamOutput, StreamOutputItem};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::pin::Pin;

pub const DESCRIPTION: &str = r#"List all tasks in the session task list (summary fields). Internal tasks (`metadata._internal`) are omitted."#;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TaskListArgs {}

pub struct TaskListTool;

#[async_trait]
impl super::ToolImpl for TaskListTool {
    type Args = TaskListArgs;

    const DESCRIPTION: &'static str = DESCRIPTION;

    async fn execute(
        ctx: &ToolContext,
        _args: Self::Args,
    ) -> Result<crate::infrastructure::streaming::StreamOutputBox, ToolError> {
        let Some(store) = ctx.agent_tasks.as_ref() else {
            return Err(ToolError::ExecutionFailed {
                message: "Task list is not available in this context (no session task store)."
                    .to_string(),
            });
        };

        let g = store.lock().await;
        let visible: Vec<_> = g.iter().filter(|t| !t.is_internal()).collect();

        let resolved_completed: HashSet<&str> = visible
            .iter()
            .filter(|t| t.status == TaskV2Status::Completed)
            .map(|t| t.id.as_str())
            .collect();

        let tasks: Vec<serde_json::Value> = visible
            .iter()
            .map(|task| {
                let blocked_by: Vec<&str> = task
                    .blocked_by
                    .iter()
                    .filter(|id| !resolved_completed.contains(id.as_str()))
                    .map(|s| s.as_str())
                    .collect();
                serde_json::json!({
                    "id": task.id,
                    "subject": task.subject,
                    "status": task.status,
                    "owner": task.owner,
                    "blockedBy": blocked_by,
                })
            })
            .collect();

        let text = serde_json::json!({ "tasks": tasks });
        let s = serde_json::to_string_pretty(&text).map_err(|e| ToolError::ExecutionFailed {
            message: e.to_string(),
        })?;
        Ok(JsonOut { text: s }.into_stream())
    }
}

struct JsonOut {
    text: String,
}

impl StreamOutput for JsonOut {
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
        "TaskList",
        DESCRIPTION,
        serde_json::json!({
            "type": "object",
            "properties": {}
        }),
    )
}
