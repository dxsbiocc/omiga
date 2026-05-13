use super::{ToolContext, ToolError, ToolImpl, ToolSchema};
use crate::infrastructure::streaming::{stream_single, StreamOutputItem};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

pub const DESCRIPTION: &str =
    "List installed and enabled Omiga operators. This is read-only and cannot enable or install operators.";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct OperatorListArgs {
    /// enabled | installed | all. MVP treats installed/all equivalently and marks exposed aliases.
    #[serde(default)]
    pub scope: Option<String>,
}

pub struct OperatorListTool;

#[async_trait]
impl ToolImpl for OperatorListTool {
    type Args = OperatorListArgs;

    const DESCRIPTION: &'static str = DESCRIPTION;

    async fn execute(
        _ctx: &ToolContext,
        args: Self::Args,
    ) -> Result<crate::infrastructure::streaming::StreamOutputBox, ToolError> {
        let scope = args.scope.as_deref().unwrap_or("all");
        let mut operators = crate::domain::operators::list_operator_summaries();
        if scope == "enabled" {
            operators.retain(|operator| operator.exposed);
        }
        let output = serde_json::json!({
            "scope": scope,
            "operators": operators,
            "note": "operator_list is read-only; enable/install operators from the plugin/operator settings UI."
        });
        Ok(stream_single(StreamOutputItem::Text(
            serde_json::to_string_pretty(&output).unwrap_or_else(|_| "{}".to_string()),
        )))
    }
}

pub fn schema() -> ToolSchema {
    ToolSchema::new(
        "operator_list",
        DESCRIPTION,
        serde_json::json!({
            "type": "object",
            "properties": {
                "scope": {
                    "type": "string",
                    "enum": ["enabled", "installed", "all"],
                    "description": "Which operator set to list."
                }
            }
        }),
    )
}
