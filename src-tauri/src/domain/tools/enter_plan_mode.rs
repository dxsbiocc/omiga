//! Enter plan mode — aligned with `EnterPlanModeTool` (wire name `EnterPlanMode`).
//!
//! Tool prompt text is `src/tools/EnterPlanModeTool/prompt.ts` (`getEnterPlanModeToolPromptExternal`).

use super::{ToolContext, ToolError, ToolSchema};
use crate::constants::plan_mode_prompt::{
    ENTER_PLAN_MODE_SUCCESS_MESSAGE, ENTER_PLAN_MODE_TOOL_PROMPT, ENTER_PLAN_MODE_TOOL_RESULT_FOLLOWUP,
};
use crate::infrastructure::streaming::{StreamOutput, StreamOutputItem};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::pin::Pin;

pub const DESCRIPTION: &str = ENTER_PLAN_MODE_TOOL_PROMPT;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EnterPlanModeArgs {}

pub struct EnterPlanModeTool;

#[async_trait]
impl super::ToolImpl for EnterPlanModeTool {
    type Args = EnterPlanModeArgs;

    const DESCRIPTION: &'static str = DESCRIPTION;

    async fn execute(
        ctx: &ToolContext,
        _args: Self::Args,
    ) -> Result<crate::infrastructure::streaming::StreamOutputBox, ToolError> {
        if let Some(pm) = &ctx.plan_mode {
            *pm.lock().await = true;
        }
        let text = serde_json::json!({
            "message": ENTER_PLAN_MODE_SUCCESS_MESSAGE,
            "instructions": ENTER_PLAN_MODE_TOOL_RESULT_FOLLOWUP,
            "_omiga": "No per-turn plan_mode attachment like Claude Code — follow `instructions` for plan-file workflow. Session `plan_mode` flag is set for sub-agent ExitPlanMode parity."
        });
        let s = serde_json::to_string_pretty(&text).map_err(|e| ToolError::ExecutionFailed {
            message: e.to_string(),
        })?;
        Ok(EnterPlanModeOutput { text: s }.into_stream())
    }
}

struct EnterPlanModeOutput {
    text: String,
}

impl StreamOutput for EnterPlanModeOutput {
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
        "EnterPlanMode",
        DESCRIPTION,
        serde_json::json!({
            "type": "object",
            "properties": {}
        }),
    )
}
