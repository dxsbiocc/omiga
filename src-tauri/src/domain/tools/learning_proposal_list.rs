use super::{ToolContext, ToolError, ToolImpl, ToolSchema};
use crate::infrastructure::streaming::{stream_single, StreamOutputItem};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

pub const DESCRIPTION: &str =
    "List project learning proposals and optionally refresh them from recent ExecutionRecords.";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LearningProposalListArgs {
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default)]
    pub refresh: bool,
    #[serde(default, rename = "includeDecided")]
    pub include_decided: bool,
}

pub struct LearningProposalListTool;

#[async_trait]
impl ToolImpl for LearningProposalListTool {
    type Args = LearningProposalListArgs;

    const DESCRIPTION: &'static str = DESCRIPTION;

    async fn execute(
        ctx: &ToolContext,
        args: Self::Args,
    ) -> Result<crate::infrastructure::streaming::StreamOutputBox, ToolError> {
        let limit = args.limit.unwrap_or(100).clamp(1, 200);
        let list = if args.refresh {
            crate::domain::learning_proposals::refresh_and_list_learning_proposals(
                &ctx.project_root,
                limit,
                args.include_decided,
            )
            .await
        } else {
            crate::domain::learning_proposals::list_learning_proposals(
                &ctx.project_root,
                args.include_decided,
            )
        }
        .map_err(|message| ToolError::ExecutionFailed { message })?;
        let output = serde_json::json!({
            "store": list.store_path,
            "summary": list.summary,
            "proposals": list.proposals,
            "note": "Proposal-first learning flow. This tool can persist new proposals when refresh=true, but it does not mutate operators, templates, skills, or result archives."
        });
        Ok(stream_single(StreamOutputItem::Text(
            serde_json::to_string_pretty(&output).unwrap_or_else(|_| "{}".to_string()),
        )))
    }
}

pub fn schema() -> ToolSchema {
    ToolSchema::new(
        "learning_proposal_list",
        DESCRIPTION,
        serde_json::json!({
            "type": "object",
            "properties": {
                "limit": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 200,
                    "description": "Maximum recent ExecutionRecords to scan when refresh=true; defaults to 100."
                },
                "refresh": {
                    "type": "boolean",
                    "description": "When true, scan recent ExecutionRecords and persist new learning proposals before listing."
                },
                "includeDecided": {
                    "type": "boolean",
                    "description": "When true, include approved/applied/dismissed/snoozed proposals; defaults to pending proposals only."
                }
            }
        }),
    )
}
