use super::{ToolContext, ToolError, ToolImpl, ToolSchema};
use crate::infrastructure::streaming::{stream_single, StreamOutputItem};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

pub const DESCRIPTION: &str =
    "Promote a confirmed learning preference candidate into active project-scoped preference records.";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LearningPreferenceCandidatePromoteArgs {
    #[serde(rename = "candidateId")]
    pub candidate_id: String,
    #[serde(default)]
    pub note: Option<String>,
}

pub struct LearningPreferenceCandidatePromoteTool;

#[async_trait]
impl ToolImpl for LearningPreferenceCandidatePromoteTool {
    type Args = LearningPreferenceCandidatePromoteArgs;

    const DESCRIPTION: &'static str = DESCRIPTION;

    async fn execute(
        ctx: &ToolContext,
        args: Self::Args,
    ) -> Result<crate::infrastructure::streaming::StreamOutputBox, ToolError> {
        let result = crate::domain::learning_proposals::promote_learning_preference_candidate(
            &ctx.project_root,
            &args.candidate_id,
            args.note,
        )
        .map_err(|message| ToolError::ExecutionFailed { message })?;
        let output = serde_json::json!({
            "candidateStore": result.candidate_store_path,
            "projectPreferences": result.project_preferences_path,
            "candidate": result.candidate,
            "preference": result.preference,
            "notification": result.notification,
            "note": "Promotion writes an auditable project preference sidecar. It does not edit bundled operators/templates or move result artifacts."
        });
        Ok(stream_single(StreamOutputItem::Text(
            serde_json::to_string_pretty(&output).unwrap_or_else(|_| "{}".to_string()),
        )))
    }
}

pub fn schema() -> ToolSchema {
    ToolSchema::new(
        "learning_preference_candidate_promote",
        DESCRIPTION,
        serde_json::json!({
            "type": "object",
            "required": ["candidateId"],
            "properties": {
                "candidateId": {
                    "type": "string",
                    "description": "Candidate id returned by learning_preference_candidate_list."
                },
                "note": {
                    "type": "string",
                    "description": "Optional rationale stored with the promoted project preference."
                }
            }
        }),
    )
}
