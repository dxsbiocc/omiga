//! Read background task output — aligned with `TaskOutputTool` (wire name `TaskOutput`).

use super::{ToolContext, ToolError, ToolSchema};
use crate::infrastructure::streaming::{StreamOutput, StreamOutputItem};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::pin::Pin;

pub const DESCRIPTION: &str = r#"Retrieve output from a background agent task by `task_id`.

- `block` (default: true): wait until the task completes or `timeout` ms elapses.
- `timeout` (default: 30000 ms, max 600000 ms): how long to wait when `block=true`.

Returns the task result when complete, or current status when non-blocking / timed out."#;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskOutputArgs {
    pub task_id: String,
    #[serde(default = "default_block")]
    pub block: bool,
    #[serde(default = "default_timeout_ms")]
    pub timeout: u64,
}

fn default_block() -> bool {
    true
}

fn default_timeout_ms() -> u64 {
    30_000
}

pub struct TaskOutputTool;

#[async_trait]
impl super::ToolImpl for TaskOutputTool {
    type Args = TaskOutputArgs;

    const DESCRIPTION: &'static str = DESCRIPTION;

    async fn execute(
        _ctx: &ToolContext,
        args: Self::Args,
    ) -> Result<crate::infrastructure::streaming::StreamOutputBox, ToolError> {
        if args.task_id.trim().is_empty() {
            return Err(ToolError::InvalidArguments {
                message: "`task_id` must be non-empty.".to_string(),
            });
        }

        use crate::domain::agents::background::{get_background_agent_manager, BackgroundAgentStatus};

        let manager = get_background_agent_manager();
        let timeout_ms = args.timeout.min(600_000);

        if args.block {
            let deadline = tokio::time::Instant::now()
                + tokio::time::Duration::from_millis(timeout_ms);

            loop {
                match manager.get_task(&args.task_id).await {
                    None => {
                        return Err(ToolError::ExecutionFailed {
                            message: format!("Task '{}' not found.", args.task_id),
                        });
                    }
                    Some(task) => match task.status {
                        BackgroundAgentStatus::Completed => {
                            let output = read_task_output(&task).await;
                            return Ok(TextOutput(output).into_stream());
                        }
                        BackgroundAgentStatus::Failed => {
                            return Err(ToolError::ExecutionFailed {
                                message: task
                                    .error_message
                                    .unwrap_or_else(|| "Task failed.".to_string()),
                            });
                        }
                        BackgroundAgentStatus::Cancelled => {
                            return Err(ToolError::ExecutionFailed {
                                message: "Task was cancelled.".to_string(),
                            });
                        }
                        BackgroundAgentStatus::Pending | BackgroundAgentStatus::Running => {}
                    },
                }

                if tokio::time::Instant::now() >= deadline {
                    // Return status snapshot on timeout instead of hard-erroring.
                    let status_json = build_status_json(manager, &args.task_id).await;
                    let msg = format!(
                        "Timed out after {} ms waiting for task '{}'. Current status:\n{}",
                        timeout_ms, args.task_id, status_json
                    );
                    return Err(ToolError::ExecutionFailed { message: msg });
                }

                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
            }
        } else {
            // Non-blocking: return current snapshot.
            let status_json = build_status_json(manager, &args.task_id).await;
            Ok(TextOutput(status_json).into_stream())
        }
    }
}

/// Read output from the task's file, falling back to `result_summary`.
async fn read_task_output(task: &crate::domain::agents::background::BackgroundAgentTask) -> String {
    if let Some(path) = &task.output_path {
        if let Ok(content) = tokio::fs::read_to_string(path).await {
            return content;
        }
    }
    task.result_summary.clone().unwrap_or_default()
}

async fn build_status_json(
    manager: &crate::domain::agents::background::BackgroundAgentManager,
    task_id: &str,
) -> String {
    match manager.get_task(task_id).await {
        None => format!("{{\"error\": \"Task '{}' not found.\"}}", task_id),
        Some(task) => serde_json::to_string_pretty(&serde_json::json!({
            "task_id": task.task_id,
            "agent_type": task.agent_type,
            "description": task.description,
            "status": format!("{:?}", task.status),
            "result_summary": task.result_summary,
            "error_message": task.error_message,
            "created_at": task.created_at,
            "started_at": task.started_at,
            "completed_at": task.completed_at,
        }))
        .unwrap_or_default(),
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
        "TaskOutput",
        DESCRIPTION,
        serde_json::json!({
            "type": "object",
            "properties": {
                "task_id": {
                    "type": "string",
                    "description": "Background task id returned by the Agent tool"
                },
                "block": {
                    "type": "boolean",
                    "description": "Wait for the task to complete (default: true)"
                },
                "timeout": {
                    "type": "number",
                    "description": "Max wait in ms when block=true (default: 30000, max: 600000)"
                }
            },
            "required": ["task_id"]
        }),
    )
}
