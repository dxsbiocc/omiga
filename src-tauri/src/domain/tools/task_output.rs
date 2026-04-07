//! Read background task output — aligned with `TaskOutputTool` (wire name `TaskOutput`).

use super::{ToolContext, ToolError, ToolSchema};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

pub const DESCRIPTION: &str = r#"Retrieve output from a background task. Supports waiting with `block` and `timeout` (ms).

**Note:** Task output polling is not integrated with Omiga's shell bridge yet."#;

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
        Err(ToolError::ExecutionFailed {
            message: format!(
                "TaskOutput is not available in Omiga yet (task_id={}, block={}, timeout_ms={}).",
                args.task_id, args.block, args.timeout
            ),
        })
    }
}

pub fn schema() -> ToolSchema {
    ToolSchema::new(
        "TaskOutput",
        DESCRIPTION,
        serde_json::json!({
            "type": "object",
            "properties": {
                "task_id": { "type": "string" },
                "block": { "type": "boolean", "description": "Wait for completion" },
                "timeout": { "type": "number", "description": "Max wait ms (0–600000)" }
            },
            "required": ["task_id"]
        }),
    )
}
