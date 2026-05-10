use super::{ToolContext, ToolError, ToolImpl, ToolSchema};
use crate::infrastructure::streaming::{stream_single, StreamOutputItem};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

pub const DESCRIPTION: &str =
    "Apply an approved learning proposal by writing project-scoped solidification records.";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LearningProposalApplyArgs {
    #[serde(rename = "proposalId")]
    pub proposal_id: String,
    #[serde(default, rename = "allowUnapproved")]
    pub allow_unapproved: bool,
    #[serde(default)]
    pub note: Option<String>,
}

pub struct LearningProposalApplyTool;

#[async_trait]
impl ToolImpl for LearningProposalApplyTool {
    type Args = LearningProposalApplyArgs;

    const DESCRIPTION: &'static str = DESCRIPTION;

    async fn execute(
        ctx: &ToolContext,
        args: Self::Args,
    ) -> Result<crate::infrastructure::streaming::StreamOutputBox, ToolError> {
        let result = crate::domain::learning_proposals::apply_learning_proposal(
            &ctx.project_root,
            &args.proposal_id,
            args.allow_unapproved,
            args.note,
        )
        .map_err(|message| ToolError::ExecutionFailed { message })?;
        let output = serde_json::json!({
            "proposalStore": result.proposal_store_path,
            "applyStore": result.apply_store_path,
            "proposal": result.proposal,
            "applyRecord": result.apply_record,
            "notification": result.notification,
            "note": "Apply writes project-scoped learning records only. It does not silently rewrite operators, templates, skills, or move artifacts."
        });
        Ok(stream_single(StreamOutputItem::Text(
            serde_json::to_string_pretty(&output).unwrap_or_else(|_| "{}".to_string()),
        )))
    }
}

pub fn schema() -> ToolSchema {
    ToolSchema::new(
        "learning_proposal_apply",
        DESCRIPTION,
        serde_json::json!({
            "type": "object",
            "required": ["proposalId"],
            "properties": {
                "proposalId": {
                    "type": "string",
                    "description": "Approved learning proposal id returned by learning_proposal_list."
                },
                "allowUnapproved": {
                    "type": "boolean",
                    "description": "When true, apply without a prior approve decision. Defaults to false to preserve user-confirmation semantics."
                },
                "note": {
                    "type": "string",
                    "description": "Optional human-readable rationale or context stored with the applied record."
                }
            }
        }),
    )
}
