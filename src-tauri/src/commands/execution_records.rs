//! Read-only Tauri commands for project-scoped Operator / Template ExecutionRecords.

use super::CommandResult;
use crate::domain::execution_records::ExecutionRecord;
use crate::errors::AppError;
use serde::Serialize;
use serde_json::{json, Value as JsonValue};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionRecordLineageSummary {
    pub returned_records: usize,
    pub returned_root_records: usize,
    pub returned_records_with_parent: usize,
    pub included_child_records: usize,
    pub status_counts: BTreeMap<String, usize>,
    pub kind_counts: BTreeMap<String, usize>,
    pub execution_mode_counts: BTreeMap<String, usize>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionRecordListResponse {
    pub database: String,
    pub count: usize,
    pub records: Vec<ExecutionRecord>,
    pub lineage_summary: ExecutionRecordLineageSummary,
    pub note: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionRecordDetailResponse {
    pub found: bool,
    pub record_id: String,
    pub record: Option<ExecutionRecord>,
    pub parsed: JsonValue,
    pub children: Vec<ExecutionRecord>,
    pub lineage: JsonValue,
    pub database: String,
    pub note: String,
}

fn execution_record_error(error: String) -> AppError {
    AppError::Config(error)
}

fn resolve_project_root(project_root: Option<String>) -> PathBuf {
    let raw = project_root.unwrap_or_default();
    let trimmed = raw.trim();
    let path = if trimmed.is_empty() || trimmed == "." {
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
    } else {
        PathBuf::from(trimmed)
    };
    path.canonicalize().unwrap_or(path)
}

fn parse_json(raw: Option<&str>) -> JsonValue {
    raw.and_then(|value| serde_json::from_str::<JsonValue>(value).ok())
        .unwrap_or(JsonValue::Null)
}

fn count_by(
    records: &[ExecutionRecord],
    value: impl Fn(&ExecutionRecord) -> String,
) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for record in records {
        *counts.entry(value(record)).or_insert(0) += 1;
    }
    counts
}

fn execution_mode(record: &ExecutionRecord) -> String {
    for raw in [
        record.metadata_json.as_deref(),
        record.runtime_json.as_deref(),
    ] {
        let parsed = parse_json(raw);
        if let Some(mode) = parsed.get("executionMode").and_then(JsonValue::as_str) {
            return mode.to_string();
        }
    }
    "unknown".to_string()
}

fn lineage_summary(records: &[ExecutionRecord]) -> ExecutionRecordLineageSummary {
    ExecutionRecordLineageSummary {
        returned_records: records.len(),
        returned_root_records: records
            .iter()
            .filter(|record| record.parent_execution_id.is_none())
            .count(),
        returned_records_with_parent: records
            .iter()
            .filter(|record| record.parent_execution_id.is_some())
            .count(),
        included_child_records: 0,
        status_counts: count_by(records, |record| record.status.clone()),
        kind_counts: count_by(records, |record| record.kind.clone()),
        execution_mode_counts: count_by(records, execution_mode),
    }
}

#[tauri::command]
pub async fn list_execution_records(
    project_root: Option<String>,
    limit: Option<usize>,
    session_id: Option<String>,
) -> CommandResult<ExecutionRecordListResponse> {
    let project_root = resolve_project_root(project_root);
    let mut records = crate::domain::execution_records::list_recent_execution_records(
        &project_root,
        limit.unwrap_or(50),
    )
    .await
    .map_err(execution_record_error)?;
    if let Some(session_id) = session_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        records.retain(|record| record.session_id.as_deref() == Some(session_id));
    }
    let summary = lineage_summary(&records);
    Ok(ExecutionRecordListResponse {
        database: crate::domain::execution_records::execution_db_path(&project_root)
            .to_string_lossy()
            .into_owned(),
        count: records.len(),
        records,
        lineage_summary: summary,
        note: "Read-only ExecutionRecord browser data. This command does not mutate runs, artifacts, proposals, or archives.".to_string(),
    })
}

#[tauri::command]
pub async fn read_execution_record(
    project_root: Option<String>,
    record_id: String,
    include_children: Option<bool>,
) -> CommandResult<ExecutionRecordDetailResponse> {
    let project_root = resolve_project_root(project_root);
    let record_id = record_id.trim().to_string();
    if record_id.is_empty() {
        return Err(AppError::Config("recordId must not be empty".to_string()));
    }

    let record = crate::domain::execution_records::get_execution_record(&project_root, &record_id)
        .await
        .map_err(execution_record_error)?;
    let children = if include_children.unwrap_or(true) {
        crate::domain::execution_records::list_child_execution_records(
            &project_root,
            &record_id,
            50,
        )
        .await
        .map_err(execution_record_error)?
    } else {
        Vec::new()
    };
    let parsed = record
        .as_ref()
        .map(|record| {
            json!({
                "metadata": parse_json(record.metadata_json.as_deref()),
                "runtime": parse_json(record.runtime_json.as_deref()),
                "outputSummary": parse_json(record.output_summary_json.as_deref()),
            })
        })
        .unwrap_or(JsonValue::Null);
    let lineage = json!({
        "parentExecutionId": record.as_ref().and_then(|record| record.parent_execution_id.clone()),
        "childCount": children.len(),
    });

    Ok(ExecutionRecordDetailResponse {
        found: record.is_some(),
        record_id,
        record,
        parsed,
        children,
        lineage,
        database: crate::domain::execution_records::execution_db_path(&project_root)
            .to_string_lossy()
            .into_owned(),
        note: "Read-only ExecutionRecord detail. This command does not mutate runs, artifacts, proposals, or archives.".to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::execution_records::{record_execution_with_id, ExecutionRecordInput};

    #[tokio::test]
    async fn lists_and_reads_execution_records_without_mutation() {
        let tmp = tempfile::tempdir().unwrap();
        record_execution_with_id(
            tmp.path(),
            "execrec_parent".to_string(),
            ExecutionRecordInput {
                kind: "template".to_string(),
                unit_id: Some("bulk_de".to_string()),
                canonical_id: Some("plugin/template/bulk_de".to_string()),
                provider_plugin: Some("plugin".to_string()),
                status: "success".to_string(),
                session_id: Some("session-1".to_string()),
                parent_execution_id: None,
                started_at: Some("2026-05-10T00:00:00Z".to_string()),
                ended_at: Some("2026-05-10T00:01:00Z".to_string()),
                input_hash: None,
                param_hash: Some("sha256:param".to_string()),
                output_summary_json: Some(json!({"outputs": {"table": ["de.tsv"]}})),
                runtime_json: Some(json!({"executionMode": "renderedTemplate"})),
                metadata_json: Some(json!({
                    "paramSources": {"method": "user_preflight"},
                    "preflight": {"answeredParams": [{"param": "method"}]}
                })),
            },
        )
        .await
        .unwrap();
        record_execution_with_id(
            tmp.path(),
            "execrec_child".to_string(),
            ExecutionRecordInput {
                kind: "operator".to_string(),
                unit_id: Some("bulk_de_operator".to_string()),
                canonical_id: None,
                provider_plugin: Some("plugin".to_string()),
                status: "success".to_string(),
                session_id: Some("session-1".to_string()),
                parent_execution_id: Some("execrec_parent".to_string()),
                started_at: None,
                ended_at: None,
                input_hash: None,
                param_hash: None,
                output_summary_json: None,
                runtime_json: None,
                metadata_json: None,
            },
        )
        .await
        .unwrap();

        let list = list_execution_records(
            Some(tmp.path().to_string_lossy().into_owned()),
            Some(10),
            Some("session-1".to_string()),
        )
        .await
        .unwrap();
        assert_eq!(list.count, 2);
        assert_eq!(list.lineage_summary.returned_records_with_parent, 1);
        assert_eq!(
            list.lineage_summary
                .execution_mode_counts
                .get("renderedTemplate"),
            Some(&1)
        );

        let detail = read_execution_record(
            Some(tmp.path().to_string_lossy().into_owned()),
            "execrec_parent".to_string(),
            Some(true),
        )
        .await
        .unwrap();
        assert!(detail.found);
        assert_eq!(detail.children.len(), 1);
        assert_eq!(
            detail.parsed["metadata"]["paramSources"]["method"],
            "user_preflight"
        );
        assert_eq!(detail.lineage["childCount"], 1);
    }
}
