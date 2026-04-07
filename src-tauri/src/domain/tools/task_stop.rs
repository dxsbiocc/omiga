//! Stop a background task — aligned with `TaskStopTool` (aliases: `KillShell`).

use super::{ToolContext, ToolError, ToolSchema};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

pub const DESCRIPTION: &str = r#"Stop a running background task by `task_id` (legacy: `shell_id`).

**Note:** Omiga does not yet expose a unified background task registry from the Tauri shell; use session UI or cancel stream where available."#;

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

        Err(ToolError::ExecutionFailed {
            message: format!(
                "Background task stop is not wired in Omiga (task_id={}). Use chat cancel or the terminal panel if available.",
                id
            ),
        })
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
