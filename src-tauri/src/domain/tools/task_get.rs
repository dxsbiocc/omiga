//! Get a V2 task by id — aligned with `TaskGetTool` (`TaskGet`).

use super::{ToolContext, ToolError, ToolSchema};
use crate::infrastructure::streaming::{StreamOutput, StreamOutputItem};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::pin::Pin;

pub const DESCRIPTION: &str = r#"Retrieve one task by id from the session task list. Returns `null` if missing."#;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskGetArgs {
    #[serde(rename = "taskId")]
    pub task_id: String,
}

pub struct TaskGetTool;

#[async_trait]
impl super::ToolImpl for TaskGetTool {
    type Args = TaskGetArgs;

    const DESCRIPTION: &'static str = DESCRIPTION;

    async fn execute(
        ctx: &ToolContext,
        args: Self::Args,
    ) -> Result<crate::infrastructure::streaming::StreamOutputBox, ToolError> {
        let Some(store) = ctx.agent_tasks.as_ref() else {
            return Err(ToolError::ExecutionFailed {
                message: "Task list is not available in this context (no session task store)."
                    .to_string(),
            });
        };

        let id = args.task_id.trim();
        if id.is_empty() {
            return Err(ToolError::InvalidArguments {
                message: "`taskId` must not be empty.".to_string(),
            });
        }

        let g = store.lock().await;
        let found = g.iter().find(|t| t.id == id);

        let task_json = found.map(|t| {
            serde_json::json!({
                "id": t.id,
                "subject": t.subject,
                "description": t.description,
                "status": t.status,
                "blocks": t.blocks,
                "blockedBy": t.blocked_by,
            })
        });

        let text = serde_json::json!({ "task": task_json });
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
        "TaskGet",
        DESCRIPTION,
        serde_json::json!({
            "type": "object",
            "properties": {
                "taskId": { "type": "string", "description": "The ID of the task to retrieve" }
            },
            "required": ["taskId"]
        }),
    )
}
