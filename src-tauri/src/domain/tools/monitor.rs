//! `Monitor` tool — aligned with `MonitorTool` in Claude Code.
//!
//! Watches a background task's output file for an exit condition pattern or task completion.
//! Distinct from `TaskOutput` (which only returns on task completion):
//! Monitor returns as soon as a matching line appears in the output, even mid-run.

use super::{ToolContext, ToolError, ToolSchema};
use crate::infrastructure::streaming::{StreamOutput, StreamOutputItem};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::pin::Pin;

pub const DESCRIPTION: &str = r#"Watch a background task's live output and return as soon as a condition is met.

- `task_id`: the background task to monitor (from `Agent` or `TaskCreate`).
- `exit_condition` (optional): a substring or regex pattern; Monitor returns when a line containing this appears.
- `timeout_ms` (default 30000, max 600000): maximum wait in milliseconds.

Returns the accumulated output at the moment the condition triggers (or on timeout / task completion)."#;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitorArgs {
    pub task_id: String,
    /// Substring to look for in the task's output lines. Returns as soon as a match is found.
    pub exit_condition: Option<String>,
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u64,
}

fn default_timeout_ms() -> u64 {
    30_000
}

pub struct MonitorTool;

#[async_trait]
impl super::ToolImpl for MonitorTool {
    type Args = MonitorArgs;

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

        use crate::domain::agents::background::{
            get_background_agent_manager, BackgroundAgentStatus,
        };

        let manager = get_background_agent_manager();
        let timeout_ms = args.timeout_ms.min(600_000);
        let deadline =
            tokio::time::Instant::now() + tokio::time::Duration::from_millis(timeout_ms);

        loop {
            match manager.get_task(&args.task_id).await {
                None => {
                    return Err(ToolError::ExecutionFailed {
                        message: format!("Task '{}' not found.", args.task_id),
                    });
                }
                Some(task) => {
                    let output = read_output_so_far(&task).await;

                    // Check exit condition against accumulated output lines
                    if let Some(ref pattern) = args.exit_condition {
                        if output.lines().any(|line| line.contains(pattern.as_str())) {
                            let result = serde_json::to_string_pretty(&serde_json::json!({
                                "task_id": args.task_id,
                                "trigger": "exit_condition",
                                "matched_pattern": pattern,
                                "output": output,
                            }))
                            .unwrap_or_default();
                            return Ok(MonitorOutput(result).into_stream());
                        }
                    }

                    // Return on terminal task states
                    match task.status {
                        BackgroundAgentStatus::Completed => {
                            let result = serde_json::to_string_pretty(&serde_json::json!({
                                "task_id": args.task_id,
                                "trigger": "completed",
                                "output": output,
                            }))
                            .unwrap_or_default();
                            return Ok(MonitorOutput(result).into_stream());
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
                    }
                }
            }

            if tokio::time::Instant::now() >= deadline {
                let task = manager.get_task(&args.task_id).await;
                let output = if let Some(t) = task {
                    read_output_so_far(&t).await
                } else {
                    String::new()
                };
                let result = serde_json::to_string_pretty(&serde_json::json!({
                    "task_id": args.task_id,
                    "trigger": "timeout",
                    "timeout_ms": timeout_ms,
                    "output": output,
                }))
                .unwrap_or_default();
                return Ok(MonitorOutput(result).into_stream());
            }

            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        }
    }
}

async fn read_output_so_far(
    task: &crate::domain::agents::background::BackgroundAgentTask,
) -> String {
    if let Some(path) = &task.output_path {
        if let Ok(content) = tokio::fs::read_to_string(path).await {
            return content;
        }
    }
    task.result_summary.clone().unwrap_or_default()
}

struct MonitorOutput(String);

impl StreamOutput for MonitorOutput {
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
        "Monitor",
        DESCRIPTION,
        serde_json::json!({
            "type": "object",
            "properties": {
                "task_id": {
                    "type": "string",
                    "description": "Background task ID to monitor"
                },
                "exit_condition": {
                    "type": "string",
                    "description": "Substring to watch for in task output. Returns as soon as matched."
                },
                "timeout_ms": {
                    "type": "number",
                    "description": "Max wait in milliseconds (default: 30000, max: 600000)"
                }
            },
            "required": ["task_id"]
        }),
    )
}
