use super::{
    execution_archive_advisor::{ExecutionArchiveAdvisorArgs, ExecutionArchiveAdvisorTool},
    ToolContext, ToolError, ToolImpl, ToolSchema,
};
use crate::infrastructure::streaming::{stream_single, StreamOutputItem};
use async_trait::async_trait;
use chrono::Utc;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::path::Path;
use uuid::Uuid;

pub const DESCRIPTION: &str =
    "Write a human-reviewable archive suggestion report from ExecutionRecords under .omiga/execution/archive-suggestions without moving or deleting artifacts.";

const REPORT_DIR_RELATIVE: &str = ".omiga/execution/archive-suggestions";
const SAFETY_NOTE: &str = "No artifact mutation was performed. This tool only writes a Markdown report and JSON snapshot under .omiga/execution/archive-suggestions.";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionArchiveSuggestionWriteArgs {
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default, rename = "includeRecords")]
    pub include_records: bool,
    #[serde(default, rename = "includeLowPriority")]
    pub include_low_priority: bool,
    #[serde(default, rename = "reviewNote")]
    pub review_note: Option<String>,
}

pub struct ExecutionArchiveSuggestionWriteTool;

#[async_trait]
impl ToolImpl for ExecutionArchiveSuggestionWriteTool {
    type Args = ExecutionArchiveSuggestionWriteArgs;

    const DESCRIPTION: &'static str = DESCRIPTION;

    async fn execute(
        ctx: &ToolContext,
        args: Self::Args,
    ) -> Result<crate::infrastructure::streaming::StreamOutputBox, ToolError> {
        let advisor_args = ExecutionArchiveAdvisorArgs {
            limit: args.limit,
            include_records: args.include_records,
            include_low_priority: args.include_low_priority,
        };
        let advisor = advisor_value(ctx, advisor_args).await?;
        let generated_at = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        let report_dir = ctx.project_root.join(REPORT_DIR_RELATIVE);
        tokio::fs::create_dir_all(&report_dir)
            .await
            .map_err(|err| ToolError::ExecutionFailed {
                message: format!("create archive suggestion report dir: {err}"),
            })?;

        let file_stem = format!(
            "archive-suggestions-{}-{}",
            Utc::now().format("%Y%m%dT%H%M%SZ"),
            Uuid::new_v4().simple()
        );
        let report_path = report_dir.join(format!("{file_stem}.md"));
        let json_path = report_dir.join(format!("{file_stem}.json"));
        let report = render_markdown_report(&advisor, &generated_at, args.review_note.as_deref());
        let snapshot = serde_json::json!({
            "status": "succeeded",
            "generatedAt": generated_at,
            "reportPath": project_relative_path(&ctx.project_root, &report_path),
            "jsonPath": project_relative_path(&ctx.project_root, &json_path),
            "safetyNote": SAFETY_NOTE,
            "reviewNote": args.review_note,
            "advisor": advisor,
        });

        let snapshot_text =
            serde_json::to_string_pretty(&snapshot).map_err(|err| ToolError::ExecutionFailed {
                message: format!("serialize archive suggestion snapshot: {err}"),
            })?;
        tokio::fs::write(&report_path, report)
            .await
            .map_err(|err| ToolError::ExecutionFailed {
                message: format!("write archive suggestion report: {err}"),
            })?;
        tokio::fs::write(&json_path, snapshot_text)
            .await
            .map_err(|err| ToolError::ExecutionFailed {
                message: format!("write archive suggestion JSON snapshot: {err}"),
            })?;

        let advisor_ref = snapshot.get("advisor").unwrap_or(&JsonValue::Null);
        let summary = advisor_ref.get("summary").unwrap_or(&JsonValue::Null);
        let output = serde_json::json!({
            "status": "succeeded",
            "generatedAt": snapshot.get("generatedAt"),
            "reportPath": snapshot.get("reportPath"),
            "jsonPath": snapshot.get("jsonPath"),
            "recommendationCount": recommendation_count(advisor_ref),
            "highPriorityCount": summary_usize(summary, "highPriorityCount"),
            "mediumPriorityCount": summary_usize(summary, "mediumPriorityCount"),
            "lowPriorityCount": summary_usize(summary, "lowPriorityCount"),
            "markdownSummary": advisor_ref.get("markdownSummary"),
            "safetyNote": SAFETY_NOTE,
        });
        Ok(stream_single(StreamOutputItem::Text(
            serde_json::to_string_pretty(&output).unwrap_or_else(|_| "{}".to_string()),
        )))
    }
}

pub fn schema() -> ToolSchema {
    ToolSchema::new(
        "execution_archive_suggestion_write",
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
                    "description": "When true, include raw ExecutionRecord rows in the JSON snapshot report."
                },
                "includeLowPriority": {
                    "type": "boolean",
                    "description": "When true, include low-priority cleanup suggestions; defaults to false."
                },
                "reviewNote": {
                    "type": "string",
                    "description": "Optional human note to include in the written report."
                }
            }
        }),
    )
}

async fn advisor_value(
    ctx: &ToolContext,
    args: ExecutionArchiveAdvisorArgs,
) -> Result<JsonValue, ToolError> {
    let mut stream = ExecutionArchiveAdvisorTool::execute(ctx, args).await?;
    let mut text = String::new();
    while let Some(item) = stream.next().await {
        match item {
            StreamOutputItem::Text(chunk)
            | StreamOutputItem::Content(chunk)
            | StreamOutputItem::Stdout(chunk) => text.push_str(&chunk),
            StreamOutputItem::Error { message, .. } => {
                return Err(ToolError::ExecutionFailed { message })
            }
            _ => {}
        }
    }
    if text.trim().is_empty() {
        return Err(ToolError::ExecutionFailed {
            message: "execution_archive_advisor returned no JSON output".to_string(),
        });
    }
    serde_json::from_str(&text).map_err(|err| ToolError::ExecutionFailed {
        message: format!("parse execution archive advisor output: {err}"),
    })
}

fn render_markdown_report(
    advisor: &JsonValue,
    generated_at: &str,
    review_note: Option<&str>,
) -> String {
    let mut out = String::new();
    out.push_str("# Execution Archive Suggestions\n\n");
    out.push_str(&format!("- Generated at: `{generated_at}`\n"));
    out.push_str(&format!(
        "- Database: `{}`\n",
        string_field(advisor, "database").unwrap_or("unknown")
    ));
    out.push_str(&format!("- Safety: {SAFETY_NOTE}\n"));
    if let Some(note) = review_note.filter(|value| !value.trim().is_empty()) {
        out.push_str(&format!("- Review note: {}\n", inline(note)));
    }
    out.push('\n');

    if let Some(summary) = advisor.get("markdownSummary").and_then(JsonValue::as_str) {
        out.push_str("## Summary\n\n");
        out.push_str(summary);
        out.push_str("\n\n");
    }

    if let Some(summary) = advisor.get("summary") {
        out.push_str("## Counts\n\n");
        out.push_str(&format!(
            "- Scanned records: {}\n",
            summary_usize(summary, "scannedRecordCount")
        ));
        out.push_str(&format!(
            "- Recommendations: {}\n",
            summary_usize(summary, "recommendationCount")
        ));
        out.push_str(&format!(
            "- Priority split: high {}, medium {}, low {}\n",
            summary_usize(summary, "highPriorityCount"),
            summary_usize(summary, "mediumPriorityCount"),
            summary_usize(summary, "lowPriorityCount")
        ));
        out.push_str(&format!(
            "- Records with artifacts: {}\n",
            summary_usize(summary, "recordsWithArtifacts")
        ));
        out.push_str(&format!(
            "- Records with preflight decisions: {}\n",
            summary_usize(summary, "recordsWithPreflight")
        ));
        out.push('\n');
    }

    out.push_str("## Recommendations\n\n");
    match advisor.get("recommendations").and_then(JsonValue::as_array) {
        Some(recommendations) if !recommendations.is_empty() => {
            for (index, recommendation) in recommendations.iter().enumerate() {
                render_recommendation(&mut out, index + 1, recommendation);
            }
        }
        _ => {
            out.push_str("No archive recommendations were generated for the scanned records.\n");
        }
    }

    out
}

fn render_recommendation(out: &mut String, number: usize, recommendation: &JsonValue) {
    let priority = string_field(recommendation, "priority").unwrap_or("unknown");
    let action = string_field(recommendation, "action").unwrap_or("unknown");
    let record_id = string_field(recommendation, "recordId").unwrap_or("unknown");
    out.push_str(&format!(
        "### {number}. {} · {} · `{}`\n\n",
        inline(priority),
        inline(action),
        inline(record_id)
    ));
    out.push_str(&format!(
        "- Kind/status: {} / {}\n",
        inline(string_field(recommendation, "kind").unwrap_or("unknown")),
        inline(string_field(recommendation, "status").unwrap_or("unknown"))
    ));
    if let Some(unit_id) = string_field(recommendation, "unitId") {
        out.push_str(&format!("- Unit: `{}`\n", inline(unit_id)));
    }
    if let Some(canonical_id) = string_field(recommendation, "canonicalId") {
        out.push_str(&format!("- Canonical unit: `{}`\n", inline(canonical_id)));
    }
    if let Some(parent_id) = string_field(recommendation, "parentExecutionId") {
        out.push_str(&format!("- Parent execution: `{}`\n", inline(parent_id)));
    }
    out.push_str(&format!(
        "- Child executions: {}\n",
        recommendation
            .get("childCount")
            .and_then(JsonValue::as_u64)
            .unwrap_or(0)
    ));
    if let Some(mode) = string_field(recommendation, "executionMode") {
        out.push_str(&format!("- Execution mode: `{}`\n", inline(mode)));
    }
    if let Some(run_dir) = string_field(recommendation, "runDir") {
        out.push_str(&format!("- Run directory: `{}`\n", inline(run_dir)));
    }
    if let Some(provenance_path) = string_field(recommendation, "provenancePath") {
        out.push_str(&format!("- Provenance: `{}`\n", inline(provenance_path)));
    }
    if let Some(reason) = string_field(recommendation, "reason") {
        out.push_str(&format!("- Reason: {}\n", inline(reason)));
    }
    if let Some(param_sources) = recommendation
        .get("paramSourceSummary")
        .and_then(JsonValue::as_object)
        .filter(|map| !map.is_empty())
    {
        let joined = param_sources
            .iter()
            .map(|(key, value)| format!("{}={}", inline(key), value.as_u64().unwrap_or(0)))
            .collect::<Vec<_>>()
            .join(", ");
        out.push_str(&format!("- Param sources: {joined}\n"));
    }
    if let Some(paths) = recommendation
        .get("artifactPaths")
        .and_then(JsonValue::as_array)
        .filter(|paths| !paths.is_empty())
    {
        out.push_str("- Artifact paths:\n");
        for path in paths.iter().filter_map(JsonValue::as_str) {
            out.push_str(&format!("  - `{}`\n", inline(path)));
        }
    }
    out.push('\n');
}

fn summary_usize(summary: &JsonValue, field: &str) -> usize {
    summary
        .get(field)
        .and_then(JsonValue::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .unwrap_or(0)
}

fn recommendation_count(advisor: &JsonValue) -> usize {
    advisor
        .get("recommendations")
        .and_then(JsonValue::as_array)
        .map(Vec::len)
        .unwrap_or(0)
}

fn string_field<'a>(value: &'a JsonValue, field: &str) -> Option<&'a str> {
    value.get(field).and_then(JsonValue::as_str)
}

fn inline(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn project_relative_path(project_root: &Path, path: &Path) -> String {
    path.strip_prefix(project_root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::execution_records::{record_execution, ExecutionRecordInput};
    use serde_json::json;

    #[tokio::test]
    async fn writes_markdown_and_json_archive_suggestion_report_without_artifact_mutation() {
        let tmp = tempfile::tempdir().expect("tempdir");
        record_execution(
            tmp.path(),
            ExecutionRecordInput {
                kind: "template".to_string(),
                unit_id: Some("template_demo".to_string()),
                canonical_id: Some("builtin/template/template_demo".to_string()),
                provider_plugin: Some("builtin".to_string()),
                status: "succeeded".to_string(),
                session_id: Some("session-1".to_string()),
                parent_execution_id: None,
                started_at: Some("2026-05-10T00:00:00Z".to_string()),
                ended_at: Some("2026-05-10T00:00:02Z".to_string()),
                input_hash: Some("sha256:input".to_string()),
                param_hash: Some("sha256:param".to_string()),
                output_summary_json: Some(json!({
                    "outputs": [{"path": ".omiga/runs/template-demo/out/report.md"}]
                })),
                runtime_json: None,
                metadata_json: Some(json!({
                    "runDir": ".omiga/runs/template-demo",
                    "provenancePath": ".omiga/runs/template-demo/provenance.json",
                    "paramSources": {
                        "method": "user_preflight",
                        "alpha": "default"
                    },
                    "preflight": {
                        "answeredParams": [
                            {"param": "method", "question": "Which method?"}
                        ]
                    }
                })),
            },
        )
        .await
        .expect("record");

        let value = execute_to_json(
            &ToolContext::new(tmp.path()),
            ExecutionArchiveSuggestionWriteArgs {
                limit: Some(10),
                include_records: false,
                include_low_priority: true,
                review_note: Some("phase 5 smoke".to_string()),
            },
        )
        .await;

        assert_eq!(value["status"], "succeeded");
        assert_eq!(value["recommendationCount"], 2);
        let report_path = tmp.path().join(value["reportPath"].as_str().unwrap());
        let json_path = tmp.path().join(value["jsonPath"].as_str().unwrap());
        assert!(report_path.exists(), "report path should exist");
        assert!(json_path.exists(), "json path should exist");

        let report = std::fs::read_to_string(report_path).expect("report");
        assert!(report.contains("# Execution Archive Suggestions"));
        assert!(report.contains("No artifact mutation was performed"));
        assert!(report.contains("phase 5 smoke"));
        assert!(report.contains("archive_result"));
        assert!(report.contains("promote_reusable_choice"));

        let snapshot: JsonValue =
            serde_json::from_str(&std::fs::read_to_string(json_path).expect("snapshot"))
                .expect("json snapshot");
        assert_eq!(snapshot["status"], "succeeded");
        assert_eq!(snapshot["advisor"]["summary"]["scannedRecordCount"], 1);
    }

    async fn execute_to_json(
        ctx: &ToolContext,
        args: ExecutionArchiveSuggestionWriteArgs,
    ) -> JsonValue {
        let mut stream = ExecutionArchiveSuggestionWriteTool::execute(ctx, args)
            .await
            .expect("execute write tool");
        while let Some(item) = stream.next().await {
            if let StreamOutputItem::Text(text) = item {
                return serde_json::from_str(&text).expect("json");
            }
        }
        panic!("execution_archive_suggestion_write did not return text output");
    }
}
