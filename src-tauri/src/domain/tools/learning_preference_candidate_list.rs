use super::{ToolContext, ToolError, ToolImpl, ToolSchema};
use crate::infrastructure::streaming::{stream_single, StreamOutputItem};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

pub const DESCRIPTION: &str =
    "List project learning preference candidates that can be promoted to active project preferences.";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LearningPreferenceCandidateListArgs {
    #[serde(default, rename = "includePromoted")]
    pub include_promoted: bool,
}

pub struct LearningPreferenceCandidateListTool;

#[async_trait]
impl ToolImpl for LearningPreferenceCandidateListTool {
    type Args = LearningPreferenceCandidateListArgs;

    const DESCRIPTION: &'static str = DESCRIPTION;

    async fn execute(
        ctx: &ToolContext,
        args: Self::Args,
    ) -> Result<crate::infrastructure::streaming::StreamOutputBox, ToolError> {
        let list = crate::domain::learning_proposals::list_learning_preference_candidates(
            &ctx.project_root,
            args.include_promoted,
        )
        .map_err(|message| ToolError::ExecutionFailed { message })?;
        let output = serde_json::json!({
            "store": list.store_path,
            "projectPreferences": list.project_preferences_path,
            "summary": list.summary,
            "candidates": list.candidates,
            "note": "Candidates are inactive until promoted. Promote only after user confirmation or an explicit autonomous-learning policy permits it."
        });
        Ok(stream_single(StreamOutputItem::Text(
            serde_json::to_string_pretty(&output).unwrap_or_else(|_| "{}".to_string()),
        )))
    }
}

pub fn schema() -> ToolSchema {
    ToolSchema::new(
        "learning_preference_candidate_list",
        DESCRIPTION,
        serde_json::json!({
            "type": "object",
            "properties": {
                "includePromoted": {
                    "type": "boolean",
                    "description": "When true, include candidates that were already promoted. Defaults to false."
                }
            }
        }),
    )
}
