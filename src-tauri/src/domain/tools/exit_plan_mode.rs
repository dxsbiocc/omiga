//! Exit plan mode — aligned with `ExitPlanModeV2Tool` (wire name `ExitPlanMode`).
//!
//! Tool prompt text is `src/tools/ExitPlanModeTool/prompt.ts` (`EXIT_PLAN_MODE_V2_TOOL_PROMPT`).

use super::{ToolContext, ToolError, ToolSchema};
use crate::constants::plan_mode_prompt::EXIT_PLAN_MODE_DESCRIPTION;
use crate::infrastructure::streaming::{StreamOutput, StreamOutputItem};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::pin::Pin;

pub const DESCRIPTION: &str = EXIT_PLAN_MODE_DESCRIPTION;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AllowedPrompt {
    pub tool: String,
    pub prompt: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExitPlanModeArgs {
    #[serde(default, rename = "allowedPrompts")]
    pub allowed_prompts: Option<Vec<AllowedPrompt>>,
}

pub struct ExitPlanModeTool;

#[async_trait]
impl super::ToolImpl for ExitPlanModeTool {
    type Args = ExitPlanModeArgs;

    const DESCRIPTION: &'static str = DESCRIPTION;

    async fn execute(
        ctx: &ToolContext,
        args: Self::Args,
    ) -> Result<crate::infrastructure::streaming::StreamOutputBox, ToolError> {
        if let Some(pm) = &ctx.plan_mode {
            *pm.lock().await = false;
        }
        let body = serde_json::json!({
            "status": "ready_for_review",
            "allowedPrompts": args.allowed_prompts,
            "_omiga": "User reviews the plan in chat. Wait for explicit approval before implementation work."
        });
        let text = serde_json::to_string_pretty(&body).map_err(|e| ToolError::ExecutionFailed {
            message: format!("serialize: {}", e),
        })?;
        Ok(
            ExitPlanModeOutput { text: format!("Exit plan mode (review requested).\n\n{}", text) }.into_stream(),
        )
    }
}

struct ExitPlanModeOutput {
    text: String,
}

impl StreamOutput for ExitPlanModeOutput {
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
        "ExitPlanMode",
        DESCRIPTION,
        serde_json::json!({
            "type": "object",
            "properties": {
                "allowedPrompts": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "tool": { "type": "string" },
                            "prompt": { "type": "string" }
                        },
                        "required": ["tool", "prompt"]
                    }
                }
            }
        }),
    )
}
