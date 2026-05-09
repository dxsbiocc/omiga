use super::{ToolContext, ToolError, ToolImpl, ToolSchema};
use crate::infrastructure::streaming::{stream_single, StreamOutputItem};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const DESCRIPTION: &str =
    "Summarize project-scoped ExecutionRecord parent/child lineage into a read-only lineage report.";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionLineageReportArgs {
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default, rename = "includeRoots")]
    pub include_roots: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RootLineageSummary {
    id: String,
    kind: String,
    status: String,
    unit_id: Option<String>,
    canonical_id: Option<String>,
    child_count: usize,
    child_status_counts: BTreeMap<String, usize>,
    child_kind_counts: BTreeMap<String, usize>,
    execution_mode: Option<String>,
}

pub struct ExecutionLineageReportTool;

#[async_trait]
impl ToolImpl for ExecutionLineageReportTool {
    type Args = ExecutionLineageReportArgs;

    const DESCRIPTION: &'static str = DESCRIPTION;

    async fn execute(
        ctx: &ToolContext,
        args: Self::Args,
    ) -> Result<crate::infrastructure::streaming::StreamOutputBox, ToolError> {
        let limit = args.limit.unwrap_or(100).clamp(1, 200);
        let records = crate::domain::execution_records::list_recent_execution_records(
            &ctx.project_root,
            limit,
        )
        .await
        .map_err(|message| ToolError::ExecutionFailed { message })?;

        let mut status_counts = BTreeMap::new();
        let mut kind_counts = BTreeMap::new();
        let mut execution_mode_counts = BTreeMap::new();
        let mut parent_ids = BTreeSet::new();
        let mut child_records = 0usize;
        let mut fallback_runs = 0usize;

        for record in &records {
            increment(&mut status_counts, &record.status);
            increment(&mut kind_counts, &record.kind);
            if let Some(parent) = record.parent_execution_id.as_deref() {
                child_records += 1;
                parent_ids.insert(parent.to_string());
            }
            if let Some(mode) = execution_mode(record) {
                if mode == "fallbackMigrationTarget" {
                    fallback_runs += 1;
                }
                increment(&mut execution_mode_counts, &mode);
            }
        }

        let roots = if args.include_roots {
            let mut roots = Vec::new();
            for record in records
                .iter()
                .filter(|record| record.parent_execution_id.is_none())
            {
                let children = crate::domain::execution_records::list_child_execution_records(
                    &ctx.project_root,
                    &record.id,
                    limit,
                )
                .await
                .map_err(|message| ToolError::ExecutionFailed { message })?;
                roots.push(root_summary(record, &children));
            }
            serde_json::to_value(roots).unwrap_or_else(|_| serde_json::json!([]))
        } else {
            serde_json::Value::Null
        };

        let output = serde_json::json!({
            "database": crate::domain::execution_records::execution_db_path(&ctx.project_root),
            "scannedRecordCount": records.len(),
            "rootRecordCount": records.len().saturating_sub(child_records),
            "childRecordCount": child_records,
            "knownParentCount": parent_ids.len(),
            "fallbackRunCount": fallback_runs,
            "statusCounts": status_counts,
            "kindCounts": kind_counts,
            "executionModeCounts": execution_mode_counts,
            "roots": roots,
            "note": "Read-only V4 lineage report built from recent project-scoped ExecutionRecords. Use execution_record_list for raw rows."
        });
        Ok(stream_single(StreamOutputItem::Text(
            serde_json::to_string_pretty(&output).unwrap_or_else(|_| "{}".to_string()),
        )))
    }
}

pub fn schema() -> ToolSchema {
    ToolSchema::new(
        "execution_lineage_report",
        DESCRIPTION,
        serde_json::json!({
            "type": "object",
            "properties": {
                "limit": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 200,
                    "description": "Maximum recent ExecutionRecords to scan; defaults to 100."
                },
                "includeRoots": {
                    "type": "boolean",
                    "description": "When true, include per-root child summaries."
                }
            }
        }),
    )
}

fn root_summary(
    record: &crate::domain::execution_records::ExecutionRecord,
    children: &[crate::domain::execution_records::ExecutionRecord],
) -> RootLineageSummary {
    let mut child_status_counts = BTreeMap::new();
    let mut child_kind_counts = BTreeMap::new();
    for child in children {
        increment(&mut child_status_counts, &child.status);
        increment(&mut child_kind_counts, &child.kind);
    }
    RootLineageSummary {
        id: record.id.clone(),
        kind: record.kind.clone(),
        status: record.status.clone(),
        unit_id: record.unit_id.clone(),
        canonical_id: record.canonical_id.clone(),
        child_count: children.len(),
        child_status_counts,
        child_kind_counts,
        execution_mode: execution_mode(record),
    }
}

fn increment(counts: &mut BTreeMap<String, usize>, key: &str) {
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
    async fn summarizes_parent_child_lineage() {
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
                metadata_json: Some(
                    json!({"execution": {"executionMode": "fallbackMigrationTarget"}}),
                ),
            },
        )
        .await
        .expect("parent");
        crate::domain::execution_records::record_execution(
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
        let value = execute_to_json(
            &ctx,
            ExecutionLineageReportArgs {
                limit: Some(10),
                include_roots: true,
            },
        )
        .await;

        assert_eq!(value["scannedRecordCount"], 2);
        assert_eq!(value["rootRecordCount"], 1);
        assert_eq!(value["childRecordCount"], 1);
        assert_eq!(value["fallbackRunCount"], 1);
        assert_eq!(value["roots"][0]["childCount"], 1);
        assert_eq!(value["roots"][0]["childStatusCounts"]["succeeded"], 1);
    }

    async fn execute_to_json(
        ctx: &ToolContext,
        args: ExecutionLineageReportArgs,
    ) -> serde_json::Value {
        let mut stream = ExecutionLineageReportTool::execute(ctx, args)
            .await
            .expect("execute");
        while let Some(item) = stream.next().await {
            if let StreamOutputItem::Text(text) = item {
                return serde_json::from_str(&text).expect("json");
            }
        }
        panic!("execution_lineage_report did not return text output");
    }
}
