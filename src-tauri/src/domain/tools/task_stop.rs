//! Stop a background task — aligned with `TaskStopTool` (aliases: `KillShell`).

use super::{ToolContext, ToolError, ToolSchema};
use crate::infrastructure::streaming::{StreamOutput, StreamOutputItem};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::pin::Pin;

pub const DESCRIPTION: &str = r#"Stop a running background task by `task_id` (legacy: `shell_id`).

Cancels the background agent task and returns its final status. Has no effect if the task is already completed, failed, or cancelled."#;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskStopArgs {
    #[serde(default)]
    pub task_id: Option<String>,
    #[serde(default)]
    pub shell_id: Option<String>,
}

pub struct TaskStopTool;

#[async_trait]
impl super::ToolImpl for TaskStopTool {
    type Args = TaskStopArgs;

    const DESCRIPTION: &'static str = DESCRIPTION;

    async fn execute(
        _ctx: &ToolContext,
        args: Self::Args,
    ) -> Result<crate::infrastructure::streaming::StreamOutputBox, ToolError> {
        let id = args
            .task_id
            .as_ref()
            .or(args.shell_id.as_ref())
            .map(|s| s.trim())
            .filter(|s| !s.is_empty());
        let Some(id) = id else {
            return Err(ToolError::InvalidArguments {
                message: "Missing `task_id` (or deprecated `shell_id`).".to_string(),
            });
        };

        let manager = crate::domain::agents::background::get_background_agent_manager();

        // If already finished, just return its current status without modifying it.
        if let Some(existing) = manager.get_task(id).await {
            use crate::domain::agents::background::BackgroundAgentStatus;
            match existing.status {
                BackgroundAgentStatus::Completed
                | BackgroundAgentStatus::Failed
                | BackgroundAgentStatus::Cancelled => {
                    let text = serde_json::to_string_pretty(&serde_json::json!({
                        "task_id": id,
                        "agent_type": existing.agent_type,
                        "description": existing.description,
                        "status": format!("{:?}", existing.status),
                        "note": "Task was already finished; no action taken."
                    }))
                    .unwrap_or_default();
                    return Ok(TextOutput(text).into_stream());
                }
                _ => {}
            }
        } else {
            return Err(ToolError::ExecutionFailed {
                message: format!("Background task '{}' not found.", id),
            });
        }

        match manager.cancel_task(id).await {
            Some(task) => {
                let text = serde_json::to_string_pretty(&serde_json::json!({
                    "task_id": id,
                    "agent_type": task.agent_type,
                    "description": task.description,
                    "status": "Cancelled",
                }))
                .unwrap_or_default();
                Ok(TextOutput(text).into_stream())
            }
            None => Err(ToolError::ExecutionFailed {
                message: format!("Failed to cancel task '{}'.", id),
            }),
        }
    }
}

struct TextOutput(String);

impl StreamOutput for TextOutput {
    fn into_stream(self) -> Pin<Box<dyn futures::Stream<Item = StreamOutputItem> + Send>> {
        use futures::stream;
        Box::pin(stream::iter(vec![
            StreamOutputItem::Start,
            StreamOutputItem::Content(self.0),
            StreamOutputItem::Complete,
        ]))
    }
}

pub fn schema() -> ToolSchema {
    ToolSchema::new(
        "TaskStop",
        DESCRIPTION,
        serde_json::json!({
            "type": "object",
            "properties": {
                "task_id": { "type": "string", "description": "Background task id" },
                "shell_id": { "type": "string", "description": "Deprecated: use task_id" }
            }
        }),
    )
}
