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

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
struct LineageSummary {
    returned_records: usize,
    returned_root_records: usize,
    returned_records_with_parent: usize,
    included_child_records: usize,
    parents_with_included_children: usize,
    returned_status_counts: BTreeMap<String, usize>,
    returned_kind_counts: BTreeMap<String, usize>,
    included_child_status_counts: BTreeMap<String, usize>,
    included_child_kind_counts: BTreeMap<String, usize>,
    execution_mode_counts: BTreeMap<String, usize>,
    children_by_parent_count: BTreeMap<String, usize>,
}

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
            children
        } else {
            BTreeMap::new()
        };
        let lineage_summary = lineage_summary(&records, &children_by_parent);
        let children_by_parent_json = if args.include_children {
            serde_json::to_value(&children_by_parent).unwrap_or_else(|_| serde_json::json!({}))
        } else {
            serde_json::Value::Null
        };
        let output = serde_json::json!({
            "database": crate::domain::execution_records::execution_db_path(&ctx.project_root),
            "count": records.len(),
            "parentExecutionId": args.parent_execution_id,
            "records": records,
            "childrenByParent": children_by_parent_json,
            "lineageSummary": lineage_summary,
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

fn lineage_summary(
    records: &[crate::domain::execution_records::ExecutionRecord],
    children_by_parent: &BTreeMap<String, Vec<crate::domain::execution_records::ExecutionRecord>>,
) -> LineageSummary {
    let mut summary = LineageSummary {
        returned_records: records.len(),
        ..LineageSummary::default()
    };

    for record in records {
        if record.parent_execution_id.is_some() {
            summary.returned_records_with_parent += 1;
        } else {
            summary.returned_root_records += 1;
        }
        increment_count(&mut summary.returned_status_counts, &record.status);
        increment_count(&mut summary.returned_kind_counts, &record.kind);
        if let Some(mode) = execution_mode(record) {
            increment_count(&mut summary.execution_mode_counts, &mode);
        }
    }

    for (parent_id, children) in children_by_parent {
        if children.is_empty() {
            continue;
        }
        summary.parents_with_included_children += 1;
        summary
            .children_by_parent_count
            .insert(parent_id.clone(), children.len());
        summary.included_child_records += children.len();
        for child in children {
            increment_count(&mut summary.included_child_status_counts, &child.status);
            increment_count(&mut summary.included_child_kind_counts, &child.kind);
            if let Some(mode) = execution_mode(child) {
                increment_count(&mut summary.execution_mode_counts, &mode);
            }
        }
    }

    summary
}

fn increment_count(counts: &mut BTreeMap<String, usize>, key: &str) {
    *counts.entry(key.to_string()).or_insert(0) += 1;
}

fn execution_mode(record: &crate::domain::execution_records::ExecutionRecord) -> Option<String> {
    let metadata = record.metadata_json.as_deref()?;
    let value = serde_json::from_str::<serde_json::Value>(metadata).ok()?;
    value
        .pointer("/execution/executionMode")
        .or_else(|| value.get("executionMode"))
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::streaming::StreamOutputItem;
    use futures::StreamExt;
    use serde_json::json;

    #[tokio::test]
    async fn lists_children_by_parent_and_filters_child_records() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let parent_id = crate::domain::execution_records::record_execution(
            tmp.path(),
            crate::domain::execution_records::ExecutionRecordInput {
                kind: "template".to_string(),
                unit_id: Some("template_a".to_string()),
                canonical_id: Some("provider/template/template_a".to_string()),
                provider_plugin: Some("provider".to_string()),
                status: "succeeded".to_string(),
                session_id: Some("session-1".to_string()),
                parent_execution_id: None,
                started_at: Some("2026-05-09T00:00:00Z".to_string()),
                ended_at: Some("2026-05-09T00:00:03Z".to_string()),
                input_hash: None,
                param_hash: None,
                output_summary_json: Some(json!({"status": "succeeded"})),
                runtime_json: None,
                metadata_json: Some(json!({
                    "execution": {
                        "executionMode": "fallbackMigrationTarget"
                    }
                })),
            },
        )
        .await
        .expect("parent");
        let child_id = crate::domain::execution_records::record_execution(
            tmp.path(),
            crate::domain::execution_records::ExecutionRecordInput {
                kind: "operator".to_string(),
                unit_id: Some("operator_a".to_string()),
                canonical_id: Some("provider/operator/operator_a".to_string()),
                provider_plugin: Some("provider".to_string()),
                status: "succeeded".to_string(),
                session_id: Some("session-1".to_string()),
                parent_execution_id: Some(parent_id.clone()),
                started_at: Some("2026-05-09T00:00:01Z".to_string()),
                ended_at: Some("2026-05-09T00:00:02Z".to_string()),
                input_hash: None,
                param_hash: None,
                output_summary_json: Some(json!({"status": "succeeded"})),
                runtime_json: None,
                metadata_json: None,
            },
        )
        .await
        .expect("child");
        let ctx = ToolContext::new(tmp.path());

        let children = execute_to_json(
            &ctx,
            ExecutionRecordListArgs {
                limit: Some(10),
                parent_execution_id: Some(parent_id.clone()),
                include_children: false,
            },
        )
        .await;
        assert_eq!(children["count"], 1);
        assert_eq!(children["records"][0]["id"], child_id);
        assert_eq!(children["records"][0]["parentExecutionId"], parent_id);
        assert_eq!(children["lineageSummary"]["returnedRecords"], 1);
        assert_eq!(children["lineageSummary"]["returnedRecordsWithParent"], 1);
        assert_eq!(
            children["lineageSummary"]["returnedKindCounts"]["operator"],
            1
        );

        let parents_with_children = execute_to_json(
            &ctx,
            ExecutionRecordListArgs {
                limit: Some(10),
                parent_execution_id: None,
                include_children: true,
            },
        )
        .await;
        assert_eq!(
            parents_with_children["childrenByParent"][parent_id.as_str()][0]["id"],
            child_id
        );
        assert_eq!(
            parents_with_children["lineageSummary"]["returnedRecords"],
            2
        );
        assert_eq!(
            parents_with_children["lineageSummary"]["returnedRootRecords"],
            1
        );
        assert_eq!(
            parents_with_children["lineageSummary"]["returnedRecordsWithParent"],
            1
        );
        assert_eq!(
            parents_with_children["lineageSummary"]["includedChildRecords"],
            1
        );
        assert_eq!(
            parents_with_children["lineageSummary"]["childrenByParentCount"][parent_id.as_str()],
            1
        );
        assert_eq!(
            parents_with_children["lineageSummary"]["executionModeCounts"]
                ["fallbackMigrationTarget"],
            1
        );
    }

    async fn execute_to_json(
        ctx: &ToolContext,
        args: ExecutionRecordListArgs,
    ) -> serde_json::Value {
        let mut stream = ExecutionRecordListTool::execute(ctx, args)
            .await
            .expect("execute");
        while let Some(item) = stream.next().await {
            if let StreamOutputItem::Text(text) = item {
                return serde_json::from_str(&text).expect("json");
            }
        }
        panic!("execution_record_list did not return text output");
    }
}
