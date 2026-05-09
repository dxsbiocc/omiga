use super::{ToolContext, ToolError, ToolImpl, ToolSchema};
use crate::domain::execution_records::ExecutionRecord;
use crate::infrastructure::streaming::{stream_single, StreamOutputItem};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::BTreeMap;

pub const DESCRIPTION: &str =
    "Analyze project-scoped ExecutionRecords and recommend archive, promote, fix, or cleanup actions.";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionArchiveAdvisorArgs {
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default, rename = "includeRecords")]
    pub include_records: bool,
    #[serde(default, rename = "includeLowPriority")]
    pub include_low_priority: bool,
}

pub struct ExecutionArchiveAdvisorTool;

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
struct ArchiveAdviceSummary {
    scanned_record_count: usize,
    recommendation_count: usize,
    high_priority_count: usize,
    medium_priority_count: usize,
    low_priority_count: usize,
    status_counts: BTreeMap<String, usize>,
    kind_counts: BTreeMap<String, usize>,
    action_counts: BTreeMap<String, usize>,
    records_with_artifacts: usize,
    records_with_preflight: usize,
    fallback_run_count: usize,
    child_record_count: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct ArchiveRecommendation {
    record_id: String,
    action: String,
    priority: String,
    reason: String,
    kind: String,
    status: String,
    unit_id: Option<String>,
    canonical_id: Option<String>,
    parent_execution_id: Option<String>,
    child_count: usize,
    execution_mode: Option<String>,
    run_dir: Option<String>,
    provenance_path: Option<String>,
    artifact_paths: Vec<String>,
    param_source_summary: BTreeMap<String, usize>,
    preflight_question_count: usize,
}

#[async_trait]
impl ToolImpl for ExecutionArchiveAdvisorTool {
    type Args = ExecutionArchiveAdvisorArgs;

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

        let mut child_counts = BTreeMap::new();
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
            if !children.is_empty() {
                child_counts.insert(record.id.clone(), children.len());
            }
        }

        let mut summary = summarize_records(&records);
        let mut recommendations = records
            .iter()
            .flat_map(|record| recommend_for_record(record, &child_counts))
            .collect::<Vec<_>>();
        if !args.include_low_priority {
            recommendations.retain(|recommendation| recommendation.priority != "low");
        }
        recommendations.sort_by(compare_recommendations);

        summary.recommendation_count = recommendations.len();
        for recommendation in &recommendations {
            increment(&mut summary.action_counts, &recommendation.action);
            match recommendation.priority.as_str() {
                "high" => summary.high_priority_count += 1,
                "medium" => summary.medium_priority_count += 1,
                "low" => summary.low_priority_count += 1,
                _ => {}
            }
        }

        let output = serde_json::json!({
            "database": crate::domain::execution_records::execution_db_path(&ctx.project_root),
            "summary": summary,
            "recommendations": recommendations,
            "records": if args.include_records {
                serde_json::to_value(&records).unwrap_or_else(|_| serde_json::json!([]))
            } else {
                JsonValue::Null
            },
            "markdownSummary": markdown_summary(&summary),
            "note": "Read-only archive advisor. It does not delete, move, or write artifacts; use it to decide what to preserve, promote, fix, or clean."
        });
        Ok(stream_single(StreamOutputItem::Text(
            serde_json::to_string_pretty(&output).unwrap_or_else(|_| "{}".to_string()),
        )))
    }
}

pub fn schema() -> ToolSchema {
    ToolSchema::new(
        "execution_archive_advisor",
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
                "includeRecords": {
                    "type": "boolean",
                    "description": "When true, include raw ExecutionRecord rows in the response."
                },
                "includeLowPriority": {
                    "type": "boolean",
                    "description": "When true, include low-priority cleanup suggestions; defaults to false."
                }
            }
        }),
    )
}

fn summarize_records(records: &[ExecutionRecord]) -> ArchiveAdviceSummary {
    let mut summary = ArchiveAdviceSummary {
        scanned_record_count: records.len(),
        child_record_count: records
            .iter()
            .filter(|record| record.parent_execution_id.is_some())
            .count(),
        ..ArchiveAdviceSummary::default()
    };
    for record in records {
        increment(&mut summary.status_counts, &record.status);
        increment(&mut summary.kind_counts, &record.kind);
        let metadata = parse_json(record.metadata_json.as_deref());
        if record_has_artifacts(record, metadata.as_ref()) {
            summary.records_with_artifacts += 1;
        }
        if preflight_question_count(metadata.as_ref()) > 0 {
            summary.records_with_preflight += 1;
        }
        if execution_mode(record, metadata.as_ref()).as_deref() == Some("fallbackMigrationTarget") {
            summary.fallback_run_count += 1;
        }
    }
    summary
}

fn recommend_for_record(
    record: &ExecutionRecord,
    child_counts: &BTreeMap<String, usize>,
) -> Vec<ArchiveRecommendation> {
    let metadata = parse_json(record.metadata_json.as_deref());
    let output_summary = parse_json(record.output_summary_json.as_deref());
    let runtime = parse_json(record.runtime_json.as_deref());
    let metadata_ref = metadata.as_ref();
    let artifact_paths = artifact_paths(record, metadata_ref, output_summary.as_ref());
    let param_sources = param_source_summary(metadata_ref);
    let preflight_questions = preflight_question_count(metadata_ref);
    let child_count = child_counts.get(&record.id).copied().unwrap_or(0);
    let execution_mode = execution_mode(record, metadata_ref);
    let run_dir = string_at(metadata_ref, &["/runDir", "/run_dir"])
        .or_else(|| string_at(runtime.as_ref(), &["/runDir", "/run_dir"]));
    let provenance_path = string_at(metadata_ref, &["/provenancePath", "/provenance_path"]);

    let mut out = Vec::new();
    if record.status != "succeeded" {
        out.push(base_recommendation(
            record,
            "fix_before_archive",
            "high",
            failure_reason(record, run_dir.as_deref()),
            child_count,
            execution_mode.clone(),
            run_dir.clone(),
            provenance_path.clone(),
            artifact_paths.clone(),
            param_sources.clone(),
            preflight_questions,
        ));
        return out;
    }

    if record.parent_execution_id.is_some() {
        out.push(base_recommendation(
            record,
            "cleanup_candidate",
            "low",
            "This is a successful child execution; keep it until the parent lineage is archived, then consider pruning its transient run directory.".to_string(),
            child_count,
            execution_mode.clone(),
            run_dir.clone(),
            provenance_path.clone(),
            artifact_paths.clone(),
            param_sources.clone(),
            preflight_questions,
        ));
    }

    if !artifact_paths.is_empty() || run_dir.is_some() || provenance_path.is_some() {
        out.push(base_recommendation(
            record,
            "archive_result",
            if record.parent_execution_id.is_none() {
                "high"
            } else {
                "medium"
            },
            archive_reason(record, &artifact_paths, child_count),
            child_count,
            execution_mode.clone(),
            run_dir.clone(),
            provenance_path.clone(),
            artifact_paths.clone(),
            param_sources.clone(),
            preflight_questions,
        ));
    }

    if preflight_questions > 0 || param_sources.get("user_preflight").copied().unwrap_or(0) > 0 {
        out.push(base_recommendation(
            record,
            "promote_reusable_choice",
            "medium",
            "User preflight choices were captured; review whether this parameter set should become a reusable Template default, example, or smoke fixture.".to_string(),
            child_count,
            execution_mode.clone(),
            run_dir.clone(),
            provenance_path.clone(),
            artifact_paths.clone(),
            param_sources.clone(),
            preflight_questions,
        ));
    }

    if execution_mode.as_deref() == Some("fallbackMigrationTarget") {
        out.push(base_recommendation(
            record,
            "inspect_lineage",
            "medium",
            "Template execution used fallback migration target; archive parent and child together until rendered-template parity is verified.".to_string(),
            child_count,
            execution_mode,
            run_dir,
            provenance_path,
            artifact_paths,
            param_sources,
            preflight_questions,
        ));
    }

    out
}

fn base_recommendation(
    record: &ExecutionRecord,
    action: &str,
    priority: &str,
    reason: String,
    child_count: usize,
    execution_mode: Option<String>,
    run_dir: Option<String>,
    provenance_path: Option<String>,
    artifact_paths: Vec<String>,
    param_source_summary: BTreeMap<String, usize>,
    preflight_question_count: usize,
) -> ArchiveRecommendation {
    ArchiveRecommendation {
        record_id: record.id.clone(),
        action: action.to_string(),
        priority: priority.to_string(),
        reason,
        kind: record.kind.clone(),
        status: record.status.clone(),
        unit_id: record.unit_id.clone(),
        canonical_id: record.canonical_id.clone(),
        parent_execution_id: record.parent_execution_id.clone(),
        child_count,
        execution_mode,
        run_dir,
        provenance_path,
        artifact_paths,
        param_source_summary,
        preflight_question_count,
    }
}

fn failure_reason(record: &ExecutionRecord, run_dir: Option<&str>) -> String {
    match run_dir {
        Some(path) => format!(
            "Execution `{}` did not succeed; inspect `{}/logs` and provenance before archiving or deleting artifacts.",
            record.id, path
        ),
        None => format!(
            "Execution `{}` did not succeed and has no runDir metadata; inspect the raw record before archiving.",
            record.id
        ),
    }
}

fn archive_reason(
    record: &ExecutionRecord,
    artifact_paths: &[String],
    child_count: usize,
) -> String {
    if !artifact_paths.is_empty() {
        return format!(
            "Successful {} execution produced {} artifact path(s); preserve outputs and provenance as the task result.",
            record.kind,
            artifact_paths.len()
        );
    }
    if child_count > 0 {
        return format!(
            "Successful {} root execution has {} child execution(s); archive the lineage together.",
            record.kind, child_count
        );
    }
    format!(
        "Successful {} execution has run/provenance metadata; preserve it if it supports the user-facing answer.",
        record.kind
    )
}

fn record_has_artifacts(record: &ExecutionRecord, metadata: Option<&JsonValue>) -> bool {
    string_at(metadata, &["/runDir", "/provenancePath", "/exportDir"]).is_some()
        || record.output_summary_json.is_some()
}

fn artifact_paths(
    record: &ExecutionRecord,
    metadata: Option<&JsonValue>,
    output_summary: Option<&JsonValue>,
) -> Vec<String> {
    let mut out = Vec::new();
    for pointer in ["/exportDir", "/provenancePath", "/runDir"] {
        if let Some(value) = string_at(metadata, &[pointer]) {
            push_unique(&mut out, value);
        }
    }
    if let Some(outputs) = output_summary.and_then(|value| value.get("outputs")) {
        collect_output_paths(outputs, &mut out);
    }
    if out.is_empty() && record.output_summary_json.is_some() {
        push_unique(&mut out, "output_summary_json".to_string());
    }
    out
}

fn collect_output_paths(value: &JsonValue, out: &mut Vec<String>) {
    match value {
        JsonValue::String(path) => push_unique(out, path.clone()),
        JsonValue::Array(values) => {
            for value in values {
                collect_output_paths(value, out);
            }
        }
        JsonValue::Object(map) => {
            for key in ["path", "file", "href", "uri"] {
                if let Some(path) = map.get(key).and_then(JsonValue::as_str) {
                    push_unique(out, path.to_string());
                }
            }
            for value in map.values() {
                if value.is_array() || value.is_object() {
                    collect_output_paths(value, out);
                }
            }
        }
        _ => {}
    }
}

fn param_source_summary(metadata: Option<&JsonValue>) -> BTreeMap<String, usize> {
    let mut out = BTreeMap::new();
    if let Some(params) = metadata
        .and_then(|value| value.get("paramSources"))
        .and_then(JsonValue::as_object)
    {
        for source in params.values().filter_map(JsonValue::as_str) {
            increment(&mut out, source);
        }
    }
    out
}

fn preflight_question_count(metadata: Option<&JsonValue>) -> usize {
    metadata
        .and_then(|value| value.get("preflight"))
        .and_then(|preflight| preflight.get("answeredParams"))
        .and_then(JsonValue::as_array)
        .map(Vec::len)
        .unwrap_or(0)
}

fn execution_mode(record: &ExecutionRecord, metadata: Option<&JsonValue>) -> Option<String> {
    metadata
        .and_then(|value| {
            value
                .pointer("/execution/executionMode")
                .or_else(|| value.get("executionMode"))
        })
        .and_then(JsonValue::as_str)
        .map(ToOwned::to_owned)
        .or_else(|| {
            if record.parent_execution_id.is_some() {
                Some("child".to_string())
            } else {
                None
            }
        })
}

fn string_at(value: Option<&JsonValue>, pointers: &[&str]) -> Option<String> {
    let value = value?;
    pointers
        .iter()
        .find_map(|pointer| value.pointer(pointer).and_then(JsonValue::as_str))
        .map(ToOwned::to_owned)
}

fn parse_json(raw: Option<&str>) -> Option<JsonValue> {
    raw.and_then(|value| serde_json::from_str::<JsonValue>(value).ok())
}

fn push_unique(out: &mut Vec<String>, value: String) {
    if !value.trim().is_empty() && !out.iter().any(|existing| existing == &value) {
        out.push(value);
    }
}

fn increment(counts: &mut BTreeMap<String, usize>, key: &str) {
    *counts.entry(key.to_string()).or_insert(0) += 1;
}

fn compare_recommendations(
    left: &ArchiveRecommendation,
    right: &ArchiveRecommendation,
) -> std::cmp::Ordering {
    priority_rank(&left.priority)
        .cmp(&priority_rank(&right.priority))
        .then_with(|| left.record_id.cmp(&right.record_id))
        .then_with(|| left.action.cmp(&right.action))
}

fn priority_rank(priority: &str) -> u8 {
    match priority {
        "high" => 0,
        "medium" => 1,
        "low" => 2,
        _ => 3,
    }
}

fn markdown_summary(summary: &ArchiveAdviceSummary) -> String {
    if summary.scanned_record_count == 0 {
        return "No ExecutionRecords found for this project.".to_string();
    }
    format!(
        "Scanned {} ExecutionRecords. Recommendations: {} high, {} medium, {} low. Artifacts: {} record(s). Preflight decisions: {} record(s). Fallback runs: {}.",
        summary.scanned_record_count,
        summary.high_priority_count,
        summary.medium_priority_count,
        summary.low_priority_count,
        summary.records_with_artifacts,
        summary.records_with_preflight,
        summary.fallback_run_count
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::execution_records::{record_execution, ExecutionRecordInput};
    use crate::infrastructure::streaming::StreamOutputItem;
    use futures::StreamExt;
    use serde_json::json;

    #[tokio::test]
    async fn advises_archive_promote_fix_and_cleanup_actions() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let parent_id = record_execution(
            tmp.path(),
            ExecutionRecordInput {
                kind: "template".to_string(),
                unit_id: Some("diff_expr".to_string()),
                canonical_id: Some("builtin/template/diff_expr".to_string()),
                provider_plugin: Some("builtin".to_string()),
                status: "succeeded".to_string(),
                session_id: Some("session-1".to_string()),
                parent_execution_id: None,
                started_at: Some("2026-05-09T00:00:00Z".to_string()),
                ended_at: Some("2026-05-09T00:00:05Z".to_string()),
                input_hash: Some("sha256:input".to_string()),
                param_hash: Some("sha256:param".to_string()),
                output_summary_json: Some(json!({
                    "outputs": [{"path": ".omiga/runs/oprun/out/report.md"}]
                })),
                runtime_json: None,
                metadata_json: Some(json!({
                    "runDir": ".omiga/runs/template-run",
                    "provenancePath": ".omiga/runs/template-run/provenance.json",
                    "paramSources": {
                        "de_method": "user_preflight",
                        "alpha": "default"
                    },
                    "preflight": {
                        "answeredParams": [
                            {"param": "de_method", "question": "Which method?"}
                        ]
                    },
                    "execution": {
                        "executionMode": "fallbackMigrationTarget"
                    }
                })),
            },
        )
        .await
        .expect("parent");

        record_execution(
            tmp.path(),
            ExecutionRecordInput {
                kind: "operator".to_string(),
                unit_id: Some("diff_expr_operator".to_string()),
                canonical_id: Some("builtin/operator/diff_expr_operator".to_string()),
                provider_plugin: Some("builtin".to_string()),
                status: "succeeded".to_string(),
                session_id: Some("session-1".to_string()),
                parent_execution_id: Some(parent_id.clone()),
                started_at: Some("2026-05-09T00:00:01Z".to_string()),
                ended_at: Some("2026-05-09T00:00:04Z".to_string()),
                input_hash: None,
                param_hash: None,
                output_summary_json: None,
                runtime_json: None,
                metadata_json: Some(json!({
                    "runDir": ".omiga/runs/operator-child",
                    "provenancePath": ".omiga/runs/operator-child/provenance.json"
                })),
            },
        )
        .await
        .expect("child");

        record_execution(
            tmp.path(),
            ExecutionRecordInput {
                kind: "operator".to_string(),
                unit_id: Some("broken".to_string()),
                canonical_id: Some("builtin/operator/broken".to_string()),
                provider_plugin: Some("builtin".to_string()),
                status: "failed".to_string(),
                session_id: Some("session-1".to_string()),
                parent_execution_id: None,
                started_at: Some("2026-05-09T00:01:00Z".to_string()),
                ended_at: Some("2026-05-09T00:01:01Z".to_string()),
                input_hash: None,
                param_hash: None,
                output_summary_json: None,
                runtime_json: None,
                metadata_json: Some(json!({
                    "runDir": ".omiga/runs/broken"
                })),
            },
        )
        .await
        .expect("failed");

        let value = execute_to_json(
            &ToolContext::new(tmp.path()),
            ExecutionArchiveAdvisorArgs {
                limit: Some(25),
                include_records: false,
                include_low_priority: true,
            },
        )
        .await;

        assert_eq!(value["summary"]["scannedRecordCount"], 3);
        assert_eq!(value["summary"]["childRecordCount"], 1);
        assert_eq!(value["summary"]["fallbackRunCount"], 1);
        assert_eq!(value["summary"]["recordsWithPreflight"], 1);
        let actions = value["recommendations"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|entry| entry["action"].as_str())
            .collect::<Vec<_>>();
        assert!(actions.contains(&"archive_result"));
        assert!(actions.contains(&"promote_reusable_choice"));
        assert!(actions.contains(&"inspect_lineage"));
        assert!(actions.contains(&"fix_before_archive"));
        assert!(actions.contains(&"cleanup_candidate"));
    }

    #[tokio::test]
    async fn hides_low_priority_cleanup_by_default() {
        let tmp = tempfile::tempdir().expect("tempdir");
        record_execution(
            tmp.path(),
            ExecutionRecordInput {
                kind: "operator".to_string(),
                unit_id: Some("child".to_string()),
                canonical_id: None,
                provider_plugin: None,
                status: "succeeded".to_string(),
                session_id: None,
                parent_execution_id: Some("parent".to_string()),
                started_at: Some("2026-05-09T00:00:00Z".to_string()),
                ended_at: Some("2026-05-09T00:00:01Z".to_string()),
                input_hash: None,
                param_hash: None,
                output_summary_json: None,
                runtime_json: None,
                metadata_json: Some(json!({"runDir": ".omiga/runs/child"})),
            },
        )
        .await
        .expect("record");

        let value = execute_to_json(
            &ToolContext::new(tmp.path()),
            ExecutionArchiveAdvisorArgs::default(),
        )
        .await;
        let actions = value["recommendations"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|entry| entry["action"].as_str())
            .collect::<Vec<_>>();

        assert!(!actions.contains(&"cleanup_candidate"));
        assert!(actions.contains(&"archive_result"));
    }

    async fn execute_to_json(ctx: &ToolContext, args: ExecutionArchiveAdvisorArgs) -> JsonValue {
        let mut stream = ExecutionArchiveAdvisorTool::execute(ctx, args)
            .await
            .expect("execute advisor");
        while let Some(item) = stream.next().await {
            if let StreamOutputItem::Text(text) = item {
                return serde_json::from_str(&text).expect("json");
            }
        }
        panic!("execution_archive_advisor did not return text output");
    }
}
