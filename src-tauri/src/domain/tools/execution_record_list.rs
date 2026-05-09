use super::{ToolContext, ToolError, ToolImpl, ToolSchema};
use crate::infrastructure::streaming::{stream_single, StreamOutputItem};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const DESCRIPTION: &str =
    "List recent project-scoped Operator/Template ExecutionRecords from .omiga/execution/executions.sqlite.";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionRecordListArgs {
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default, rename = "parentExecutionId")]
    pub parent_execution_id: Option<String>,
    #[serde(default, rename = "includeChildren")]
    pub include_children: bool,
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
        let records = if let Some(parent_id) = args
            .parent_execution_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            crate::domain::execution_records::list_child_execution_records(
                &ctx.project_root,
                parent_id,
                limit,
            )
            .await
        } else {
            crate::domain::execution_records::list_recent_execution_records(
                &ctx.project_root,
                limit,
            )
            .await
        }
        .map_err(|message| ToolError::ExecutionFailed { message })?;
        let children_by_parent = if args.include_children {
            let mut children = BTreeMap::new();
            for record in &records {
                let child_records = crate::domain::execution_records::list_child_execution_records(
                    &ctx.project_root,
                    &record.id,
                    limit,
                )
                .await
                .map_err(|message| ToolError::ExecutionFailed { message })?;
                if !child_records.is_empty() {
                    children.insert(record.id.clone(), child_records);
                }
            }
            serde_json::to_value(children).unwrap_or_else(|_| serde_json::json!({}))
        } else {
            serde_json::Value::Null
        };
        let output = serde_json::json!({
            "database": crate::domain::execution_records::execution_db_path(&ctx.project_root),
            "count": records.len(),
            "parentExecutionId": args.parent_execution_id,
            "records": records,
            "childrenByParent": children_by_parent,
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
                },
                "parentExecutionId": {
                    "type": "string",
                    "description": "When set, return only child records whose parentExecutionId matches this execution record id."
                },
                "includeChildren": {
                    "type": "boolean",
                    "description": "When true, include a childrenByParent object for the returned parent records."
                }
            }
        }),
    )
}
