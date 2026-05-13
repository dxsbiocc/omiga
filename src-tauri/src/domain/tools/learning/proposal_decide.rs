use super::{ToolContext, ToolError, ToolImpl, ToolSchema};
use crate::infrastructure::streaming::{stream_single, StreamOutputItem};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

pub const DESCRIPTION: &str =
    "Approve, dismiss, snooze, or mark-applied a project learning proposal.";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LearningProposalDecideArgs {
    #[serde(rename = "proposalId")]
    pub proposal_id: String,
    pub decision: String,
    #[serde(default)]
    pub note: Option<String>,
}

pub struct LearningProposalDecideTool;

#[async_trait]
impl ToolImpl for LearningProposalDecideTool {
    type Args = LearningProposalDecideArgs;

    const DESCRIPTION: &'static str = DESCRIPTION;

    async fn execute(
        ctx: &ToolContext,
        args: Self::Args,
    ) -> Result<crate::infrastructure::streaming::StreamOutputBox, ToolError> {
        let decision =
            crate::domain::learning_proposals::LearningProposalDecision::parse(&args.decision)
                .ok_or_else(|| ToolError::InvalidArguments {
                    message: "decision must be one of approve, dismiss, snooze, mark_applied"
                        .to_string(),
                })?;
        let result = crate::domain::learning_proposals::decide_learning_proposal(
            &ctx.project_root,
            &args.proposal_id,
            decision,
            args.note,
        )
        .map_err(|message| ToolError::ExecutionFailed { message })?;
        let output = serde_json::json!({
            "store": result.store_path,
            "proposal": result.proposal,
            "notification": result.notification,
            "note": "This records the user's decision. Concrete application to templates, project preferences, or archives is a separate apply step."
        });
        Ok(stream_single(StreamOutputItem::Text(
            serde_json::to_string_pretty(&output).unwrap_or_else(|_| "{}".to_string()),
        )))
    }
}

pub fn schema() -> ToolSchema {
    ToolSchema::new(
        "learning_proposal_decide",
        DESCRIPTION,
        serde_json::json!({
            "type": "object",
            "required": ["proposalId", "decision"],
            "properties": {
                "proposalId": {
                    "type": "string",
                    "description": "Learning proposal id returned by learning_proposal_list."
                },
                "decision": {
                    "type": "string",
                    "enum": ["approve", "dismiss", "snooze", "mark_applied"],
                    "description": "User decision to persist on the proposal."
                },
                "note": {
                    "type": "string",
                    "description": "Optional human-readable rationale or context for the decision."
                }
            }
        }),
    )
}
