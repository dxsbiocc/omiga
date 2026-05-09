use super::{ToolContext, ToolError, ToolImpl, ToolSchema};
use crate::infrastructure::streaming::{stream_single, StreamOutputItem};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

pub const DESCRIPTION: &str =
    "List recent project-scoped Operator/Template ExecutionRecords from .omiga/execution/executions.sqlite.";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionRecordListArgs {
    #[serde(default)]
    pub limit: Option<usize>,
}

pub struct ExecutionRecordListTool;

#[async_trait]
impl ToolImpl for ExecutionRecordListTool {
    type Args = ExecutionRecordListArgs;

    const DESCRIPTION: &'static str = DESCRIPTION;

    async fn execute(
        ctx: &ToolContext,
        args: Self::Args,
    ) -> Result<crate::infrastructure::streaming::StreamOutputBox, ToolError> {
        let limit = args.limit.unwrap_or(25);
        let records = crate::domain::execution_records::list_recent_execution_records(
            &ctx.project_root,
            limit,
        )
        .await
        .map_err(|message| ToolError::ExecutionFailed { message })?;
        let output = serde_json::json!({
            "database": crate::domain::execution_records::execution_db_path(&ctx.project_root),
            "count": records.len(),
            "records": records,
        });
        Ok(stream_single(StreamOutputItem::Text(
            serde_json::to_string_pretty(&output).unwrap_or_else(|_| "{}".to_string()),
        )))
    }
}

pub fn schema() -> ToolSchema {
    ToolSchema::new(
        "execution_record_list",
        DESCRIPTION,
        serde_json::json!({
            "type": "object",
            "properties": {
                "limit": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 200,
                    "description": "Maximum records to return; defaults to 25."
                }
            }
        }),
    )
}
