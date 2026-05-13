use super::{ToolContext, ToolError, ToolImpl, ToolSchema};
use crate::domain::execution_records::ExecutionRecord;
use crate::infrastructure::streaming::{stream_single, StreamOutputItem};
use async_trait::async_trait;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use uuid::Uuid;

pub const DESCRIPTION: &str =
    "Generate a non-applying self-evolution report from Operator/Template ExecutionRecord lineage.";

const REPORT_DIR_RELATIVE: &str = ".omiga/learning/self-evolution-reports";
const SAFETY_NOTE: &str = "Report-only self-evolution. This tool does not create, modify, or publish Operators, Templates, Skills, defaults, or archives.";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LearningSelfEvolutionReportArgs {
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default = "default_write_report", rename = "writeReport")]
    pub write_report: bool,
    #[serde(default, rename = "includeRecords")]
    pub include_records: bool,
}

impl Default for LearningSelfEvolutionReportArgs {
    fn default() -> Self {
        Self {
            limit: None,
            write_report: default_write_report(),
            include_records: false,
        }
    }
}

pub struct LearningSelfEvolutionReportTool;

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
struct SelfEvolutionSummary {
    scanned_record_count: usize,
    candidate_count: usize,
    template_candidate_count: usize,
    operator_candidate_count: usize,
    preference_candidate_count: usize,
    archive_candidate_count: usize,
    lineage_candidate_count: usize,
    repeated_unit_candidate_count: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SelfEvolutionCandidate {
    id: String,
    kind: String,
    priority: String,
    title: String,
    rationale: String,
    proposed_next_step: String,
    source_record_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    unit_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    canonical_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    provider_plugin: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    evidence: BTreeMap<String, JsonValue>,
}

#[async_trait]
impl ToolImpl for LearningSelfEvolutionReportTool {
    type Args = LearningSelfEvolutionReportArgs;

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

        let child_counts = child_counts_for_roots(&ctx.project_root, &records, limit).await?;
        let candidates = generate_candidates(&records, &child_counts);
        let summary = summarize_candidates(records.len(), &candidates);
        let generated_at = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);

        let mut report_path = None;
        let mut json_path = None;
        if args.write_report {
            let report_dir = ctx.project_root.join(REPORT_DIR_RELATIVE);
            tokio::fs::create_dir_all(&report_dir)
                .await
                .map_err(|err| ToolError::ExecutionFailed {
                    message: format!("create self-evolution report dir: {err}"),
                })?;
            let file_stem = format!(
                "self-evolution-{}-{}",
                Utc::now().format("%Y%m%dT%H%M%SZ"),
                Uuid::new_v4().simple()
            );
            let markdown_path = report_dir.join(format!("{file_stem}.md"));
            let snapshot_path = report_dir.join(format!("{file_stem}.json"));
            let markdown =
                render_markdown_report(&generated_at, &summary, &candidates, args.include_records);
            let snapshot = serde_json::json!({
                "status": "succeeded",
                "generatedAt": generated_at,
                "summary": summary.clone(),
                "candidates": candidates.clone(),
                "records": if args.include_records {
                    serde_json::to_value(&records).unwrap_or_else(|_| serde_json::json!([]))
                } else {
                    JsonValue::Null
                },
                "safetyNote": SAFETY_NOTE,
            });
            let snapshot_text = serde_json::to_string_pretty(&snapshot).map_err(|err| {
                ToolError::ExecutionFailed {
                    message: format!("serialize self-evolution report snapshot: {err}"),
                }
            })?;
            tokio::fs::write(&markdown_path, markdown)
                .await
                .map_err(|err| ToolError::ExecutionFailed {
                    message: format!("write self-evolution report: {err}"),
                })?;
            tokio::fs::write(&snapshot_path, snapshot_text)
                .await
                .map_err(|err| ToolError::ExecutionFailed {
                    message: format!("write self-evolution report JSON snapshot: {err}"),
                })?;
            report_path = Some(project_relative_path(&ctx.project_root, &markdown_path));
            json_path = Some(project_relative_path(&ctx.project_root, &snapshot_path));
        }

        let output = serde_json::json!({
            "status": "succeeded",
            "generatedAt": generated_at,
            "summary": summary,
            "candidates": candidates,
            "records": if args.include_records {
                serde_json::to_value(&records).unwrap_or_else(|_| serde_json::json!([]))
            } else {
                JsonValue::Null
            },
            "reportPath": report_path,
            "jsonPath": json_path,
            "safetyNote": SAFETY_NOTE,
        });
        Ok(stream_single(StreamOutputItem::Text(
            serde_json::to_string_pretty(&output).unwrap_or_else(|_| "{}".to_string()),
        )))
    }
}

pub fn schema() -> ToolSchema {
    ToolSchema::new(
        "learning_self_evolution_report",
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
                "writeReport": {
                    "type": "boolean",
                    "description": "When true, write Markdown and JSON report files under .omiga/learning/self-evolution-reports. Defaults to true."
                },
                "includeRecords": {
                    "type": "boolean",
                    "description": "When true, include raw ExecutionRecord rows in the JSON output and snapshot."
                }
            }
        }),
    )
}

fn default_write_report() -> bool {
    true
}

async fn child_counts_for_roots(
    project_root: &Path,
    records: &[ExecutionRecord],
    limit: usize,
) -> Result<BTreeMap<String, usize>, ToolError> {
    let mut child_counts = BTreeMap::new();
    for record in records
        .iter()
        .filter(|record| record.parent_execution_id.is_none())
    {
        let children = crate::domain::execution_records::list_child_execution_records(
            project_root,
            &record.id,
            limit,
        )
        .await
        .map_err(|message| ToolError::ExecutionFailed { message })?;
        if !children.is_empty() {
            child_counts.insert(record.id.clone(), children.len());
        }
    }
    Ok(child_counts)
}

fn generate_candidates(
    records: &[ExecutionRecord],
    child_counts: &BTreeMap<String, usize>,
) -> Vec<SelfEvolutionCandidate> {
    let mut candidates = Vec::new();
    let mut repeated_groups: HashMap<String, Vec<&ExecutionRecord>> = HashMap::new();

    for record in records.iter().filter(|record| record.status == "succeeded") {
        let metadata = parse_json(record.metadata_json.as_deref());
        let output_summary = parse_json(record.output_summary_json.as_deref());
        let metadata_ref = metadata.as_ref();
        let output_ref = output_summary.as_ref();
        let unit = unit_label(record);
        let artifact_paths = artifact_paths(record, metadata_ref, output_ref);
        let param_sources = param_source_summary(metadata_ref);
        let answered_params = preflight_answered_params(metadata_ref);
        let selected_params = selected_param_values(metadata_ref, &answered_params);
        let child_count = child_counts.get(&record.id).copied().unwrap_or(0);
        let run_dir = string_at(metadata_ref, &["/runDir", "/run_dir"]);
        let provenance_path = string_at(metadata_ref, &["/provenancePath", "/provenance_path"]);

        if record.parent_execution_id.is_none() && child_count > 0 {
            candidates.push(lineage_template_candidate(
                record,
                &unit,
                child_count,
                run_dir.as_deref(),
                provenance_path.as_deref(),
            ));
        }

        if !answered_params.is_empty()
            || param_sources.get("user_preflight").copied().unwrap_or(0) > 0
        {
            candidates.push(preference_candidate(
                record,
                &unit,
                &answered_params,
                &selected_params,
                &param_sources,
            ));
        }

        if record.parent_execution_id.is_none()
            && (!artifact_paths.is_empty() || run_dir.is_some() || provenance_path.is_some())
        {
            candidates.push(archive_candidate(
                record,
                &unit,
                &artifact_paths,
                run_dir.as_deref(),
                provenance_path.as_deref(),
            ));
        }

        if record.parent_execution_id.is_none()
            && record
                .param_hash
                .as_deref()
                .is_some_and(|hash| !hash.trim().is_empty())
        {
            repeated_groups
                .entry(repeated_unit_key(record))
                .or_default()
                .push(record);
        }
    }

    for group in repeated_groups
        .into_values()
        .filter(|group| group.len() >= 2)
    {
        candidates.push(repeated_unit_template_candidate(&group));
    }

    candidates.sort_by(|left, right| {
        priority_rank(&left.priority)
            .cmp(&priority_rank(&right.priority))
            .then_with(|| left.kind.cmp(&right.kind))
            .then_with(|| left.id.cmp(&right.id))
    });
    candidates
}

fn lineage_template_candidate(
    record: &ExecutionRecord,
    unit: &str,
    child_count: usize,
    run_dir: Option<&str>,
    provenance_path: Option<&str>,
) -> SelfEvolutionCandidate {
    let mut evidence = base_evidence(record);
    evidence.insert("childCount".to_string(), serde_json::json!(child_count));
    insert_optional(&mut evidence, "runDir", run_dir);
    insert_optional(&mut evidence, "provenancePath", provenance_path);
    SelfEvolutionCandidate {
        id: stable_candidate_id("lineage_template", &[&record.id]),
        kind: "template_candidate".to_string(),
        priority: "high".to_string(),
        title: format!("Crystallize `{unit}` lineage into a reusable Template"),
        rationale: format!(
            "Successful root execution has {child_count} child execution(s); this is a candidate for a documented Template or workflow preset."
        ),
        proposed_next_step:
            "Review parent/child provenance, identify stable inputs/params, then draft a Template manifest and fixture."
                .to_string(),
        source_record_ids: vec![record.id.clone()],
        unit_id: record.unit_id.clone(),
        canonical_id: record.canonical_id.clone(),
        provider_plugin: record.provider_plugin.clone(),
        evidence,
    }
}

fn preference_candidate(
    record: &ExecutionRecord,
    unit: &str,
    answered_params: &[String],
    selected_params: &BTreeMap<String, JsonValue>,
    param_sources: &BTreeMap<String, usize>,
) -> SelfEvolutionCandidate {
    let mut evidence = base_evidence(record);
    evidence.insert(
        "answeredParams".to_string(),
        serde_json::json!(answered_params),
    );
    evidence.insert(
        "selectedParams".to_string(),
        serde_json::json!(selected_params),
    );
    evidence.insert(
        "paramSourceSummary".to_string(),
        serde_json::json!(param_sources),
    );
    SelfEvolutionCandidate {
        id: stable_candidate_id("project_preference", &[&record.id]),
        kind: "project_preference_candidate".to_string(),
        priority: "medium".to_string(),
        title: format!("Promote reusable choices for `{unit}`"),
        rationale:
            "Execution metadata contains user preflight decisions or user_preflight param sources."
                .to_string(),
        proposed_next_step:
            "Ask for confirmation, then use learning proposal/preference flows to save project-scoped defaults."
                .to_string(),
        source_record_ids: vec![record.id.clone()],
        unit_id: record.unit_id.clone(),
        canonical_id: record.canonical_id.clone(),
        provider_plugin: record.provider_plugin.clone(),
        evidence,
    }
}

fn archive_candidate(
    record: &ExecutionRecord,
    unit: &str,
    artifact_paths: &[String],
    run_dir: Option<&str>,
    provenance_path: Option<&str>,
) -> SelfEvolutionCandidate {
    let mut evidence = base_evidence(record);
    evidence.insert(
        "artifactPaths".to_string(),
        serde_json::json!(artifact_paths),
    );
    insert_optional(&mut evidence, "runDir", run_dir);
    insert_optional(&mut evidence, "provenancePath", provenance_path);
    SelfEvolutionCandidate {
        id: stable_candidate_id("archive", &[&record.id]),
        kind: "archive_candidate".to_string(),
        priority: "medium".to_string(),
        title: format!("Mark `{unit}` result for project archive"),
        rationale: "Successful root execution produced run/provenance/output artifacts."
            .to_string(),
        proposed_next_step:
            "Write an archive suggestion report, then preserve artifacts only after human review."
                .to_string(),
        source_record_ids: vec![record.id.clone()],
        unit_id: record.unit_id.clone(),
        canonical_id: record.canonical_id.clone(),
        provider_plugin: record.provider_plugin.clone(),
        evidence,
    }
}

fn repeated_unit_template_candidate(records: &[&ExecutionRecord]) -> SelfEvolutionCandidate {
    let first = records[0];
    let source_record_ids = records
        .iter()
        .map(|record| record.id.clone())
        .collect::<Vec<_>>();
    let mut source_refs = records
        .iter()
        .map(|record| record.id.as_str())
        .collect::<Vec<_>>();
    source_refs.sort_unstable();
    let unit = unit_label(first);
    let mut evidence = base_evidence(first);
    evidence.insert("runCount".to_string(), serde_json::json!(records.len()));
    evidence.insert(
        "sourceRecordIds".to_string(),
        serde_json::json!(source_record_ids),
    );
    SelfEvolutionCandidate {
        id: stable_candidate_id("repeated_unit_template", &source_refs),
        kind: "template_candidate".to_string(),
        priority: "medium".to_string(),
        title: format!("Review repeated `{unit}` executions for Template defaults"),
        rationale: format!(
            "{} successful root executions share the same unit/parameter signature.",
            records.len()
        ),
        proposed_next_step:
            "Compare selected params and artifacts; if stable, create a Template example/default candidate."
                .to_string(),
        source_record_ids: records.iter().map(|record| record.id.clone()).collect(),
        unit_id: first.unit_id.clone(),
        canonical_id: first.canonical_id.clone(),
        provider_plugin: first.provider_plugin.clone(),
        evidence,
    }
}

fn summarize_candidates(
    scanned_record_count: usize,
    candidates: &[SelfEvolutionCandidate],
) -> SelfEvolutionSummary {
    let mut summary = SelfEvolutionSummary {
        scanned_record_count,
        candidate_count: candidates.len(),
        ..SelfEvolutionSummary::default()
    };
    for candidate in candidates {
        match candidate.kind.as_str() {
            "template_candidate" => summary.template_candidate_count += 1,
            "operator_candidate" => summary.operator_candidate_count += 1,
            "project_preference_candidate" => summary.preference_candidate_count += 1,
            "archive_candidate" => summary.archive_candidate_count += 1,
            _ => {}
        }
        if candidate.evidence.contains_key("childCount") {
            summary.lineage_candidate_count += 1;
        }
        if candidate.evidence.contains_key("runCount") {
            summary.repeated_unit_candidate_count += 1;
        }
    }
    summary
}

fn render_markdown_report(
    generated_at: &str,
    summary: &SelfEvolutionSummary,
    candidates: &[SelfEvolutionCandidate],
    include_records: bool,
) -> String {
    let mut out = String::new();
    out.push_str("# Learning Self-Evolution Report\n\n");
    out.push_str(&format!("- Generated at: `{generated_at}`\n"));
    out.push_str(&format!("- Safety: {SAFETY_NOTE}\n"));
    out.push_str(&format!(
        "- Raw records included in JSON: `{include_records}`\n\n"
    ));
    out.push_str("## Summary\n\n");
    out.push_str(&format!(
        "- Scanned records: {}\n- Candidates: {}\n- Template candidates: {}\n- Preference candidates: {}\n- Archive candidates: {}\n\n",
        summary.scanned_record_count,
        summary.candidate_count,
        summary.template_candidate_count,
        summary.preference_candidate_count,
        summary.archive_candidate_count
    ));
    out.push_str("## Candidates\n\n");
    if candidates.is_empty() {
        out.push_str("No self-evolution candidates were generated for the scanned records.\n");
        return out;
    }
    for (index, candidate) in candidates.iter().enumerate() {
        out.push_str(&format!(
            "### {}. {} · {} · {}\n\n",
            index + 1,
            inline(&candidate.priority),
            inline(&candidate.kind),
            inline(&candidate.title)
        ));
        out.push_str(&format!("- Rationale: {}\n", inline(&candidate.rationale)));
        out.push_str(&format!(
            "- Next step: {}\n",
            inline(&candidate.proposed_next_step)
        ));
        out.push_str(&format!(
            "- Source records: `{}`\n",
            candidate.source_record_ids.join("`, `")
        ));
        if let Some(canonical_id) = candidate.canonical_id.as_deref() {
            out.push_str(&format!("- Canonical unit: `{}`\n", inline(canonical_id)));
        }
        out.push('\n');
    }
    out
}

fn base_evidence(record: &ExecutionRecord) -> BTreeMap<String, JsonValue> {
    let mut evidence = BTreeMap::new();
    insert_optional(&mut evidence, "unitId", record.unit_id.as_deref());
    insert_optional(&mut evidence, "canonicalId", record.canonical_id.as_deref());
    insert_optional(
        &mut evidence,
        "providerPlugin",
        record.provider_plugin.as_deref(),
    );
    evidence.insert("kind".to_string(), serde_json::json!(&record.kind));
    evidence
}

fn insert_optional(evidence: &mut BTreeMap<String, JsonValue>, key: &str, value: Option<&str>) {
    if let Some(value) = value.filter(|value| !value.trim().is_empty()) {
        evidence.insert(key.to_string(), JsonValue::String(value.to_string()));
    }
}

fn repeated_unit_key(record: &ExecutionRecord) -> String {
    format!(
        "{}\0{}\0{}",
        record
            .canonical_id
            .as_deref()
            .or(record.unit_id.as_deref())
            .unwrap_or(&record.kind),
        record.provider_plugin.as_deref().unwrap_or_default(),
        record.param_hash.as_deref().unwrap_or_default()
    )
}

fn stable_candidate_id(kind: &str, record_ids: &[&str]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(kind.as_bytes());
    for record_id in record_ids {
        hasher.update(b"\0");
        hasher.update(record_id.as_bytes());
    }
    let digest = hasher.finalize();
    let short = digest
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("evolve_{short}")
}

fn unit_label(record: &ExecutionRecord) -> String {
    record
        .canonical_id
        .as_deref()
        .or(record.unit_id.as_deref())
        .unwrap_or(&record.kind)
        .to_string()
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
            *out.entry(source.to_string()).or_insert(0) += 1;
        }
    }
    out
}

fn preflight_answered_params(metadata: Option<&JsonValue>) -> Vec<String> {
    metadata
        .and_then(|value| value.get("preflight"))
        .and_then(|preflight| preflight.get("answeredParams"))
        .and_then(JsonValue::as_array)
        .map(|params| {
            params
                .iter()
                .filter_map(|entry| {
                    entry
                        .get("param")
                        .or_else(|| entry.get("id"))
                        .and_then(JsonValue::as_str)
                        .or_else(|| entry.as_str())
                        .map(ToOwned::to_owned)
                })
                .collect()
        })
        .unwrap_or_default()
}

fn selected_param_values(
    metadata: Option<&JsonValue>,
    answered_params: &[String],
) -> BTreeMap<String, JsonValue> {
    let mut out = BTreeMap::new();
    let Some(selected) = metadata
        .and_then(|value| value.get("selectedParams"))
        .or_else(|| {
            metadata
                .and_then(|value| value.get("preflight"))
                .and_then(|preflight| preflight.get("selectedParams"))
        })
        .and_then(JsonValue::as_object)
    else {
        return out;
    };
    let answered = answered_params
        .iter()
        .collect::<std::collections::HashSet<_>>();
    for (param, value) in selected {
        if answered.is_empty() || answered.contains(param) {
            out.insert(param.clone(), value.clone());
        }
    }
    out
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

fn priority_rank(priority: &str) -> u8 {
    match priority {
        "high" => 0,
        "medium" => 1,
        "low" => 2,
        _ => 3,
    }
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
    use futures::StreamExt;
    use serde_json::json;

    #[tokio::test]
    async fn reports_lineage_preference_archive_and_repeated_unit_candidates() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let parent_id = record_execution(
            tmp.path(),
            ExecutionRecordInput {
                kind: "template".to_string(),
                unit_id: Some("bulk_de_template".to_string()),
                canonical_id: Some("plugin/template/bulk_de_template".to_string()),
                provider_plugin: Some("omics".to_string()),
                status: "succeeded".to_string(),
                session_id: Some("session-1".to_string()),
                parent_execution_id: None,
                started_at: Some("2026-05-10T00:00:00Z".to_string()),
                ended_at: Some("2026-05-10T00:00:02Z".to_string()),
                input_hash: Some("sha256:input".to_string()),
                param_hash: Some("sha256:param".to_string()),
                output_summary_json: Some(json!({
                    "outputs": [{"path": ".omiga/runs/template/out/report.md"}]
                })),
                runtime_json: None,
                metadata_json: Some(json!({
                    "runDir": ".omiga/runs/template",
                    "provenancePath": ".omiga/runs/template/provenance.json",
                    "paramSources": {"method": "user_preflight"},
                    "preflight": {"answeredParams": [{"param": "method"}]},
                    "selectedParams": {"method": "limma"}
                })),
            },
        )
        .await
        .expect("parent");

        record_execution(
            tmp.path(),
            ExecutionRecordInput {
                kind: "operator".to_string(),
                unit_id: Some("bulk_de_operator".to_string()),
                canonical_id: Some("plugin/operator/bulk_de_operator".to_string()),
                provider_plugin: Some("omics".to_string()),
                status: "succeeded".to_string(),
                session_id: Some("session-1".to_string()),
                parent_execution_id: Some(parent_id.clone()),
                started_at: Some("2026-05-10T00:00:01Z".to_string()),
                ended_at: Some("2026-05-10T00:00:02Z".to_string()),
                input_hash: None,
                param_hash: None,
                output_summary_json: None,
                runtime_json: None,
                metadata_json: Some(json!({"runDir": ".omiga/runs/child"})),
            },
        )
        .await
        .expect("child");

        record_execution(
            tmp.path(),
            ExecutionRecordInput {
                kind: "template".to_string(),
                unit_id: Some("bulk_de_template".to_string()),
                canonical_id: Some("plugin/template/bulk_de_template".to_string()),
                provider_plugin: Some("omics".to_string()),
                status: "succeeded".to_string(),
                session_id: Some("session-2".to_string()),
                parent_execution_id: None,
                started_at: Some("2026-05-10T00:01:00Z".to_string()),
                ended_at: Some("2026-05-10T00:01:02Z".to_string()),
                input_hash: Some("sha256:input2".to_string()),
                param_hash: Some("sha256:param".to_string()),
                output_summary_json: None,
                runtime_json: None,
                metadata_json: Some(json!({
                    "paramSources": {"method": "default"}
                })),
            },
        )
        .await
        .expect("repeat");

        let value = execute_to_json(
            &ToolContext::new(tmp.path()),
            LearningSelfEvolutionReportArgs {
                limit: Some(20),
                write_report: true,
                include_records: false,
            },
        )
        .await;

        assert_eq!(value["status"], "succeeded");
        assert!(value["summary"]["candidateCount"].as_u64().unwrap() >= 4);
        assert_eq!(value["summary"]["lineageCandidateCount"], 1);
        assert_eq!(value["summary"]["repeatedUnitCandidateCount"], 1);
        let kinds = value["candidates"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|candidate| candidate["kind"].as_str())
            .collect::<Vec<_>>();
        assert!(kinds.contains(&"template_candidate"));
        assert!(kinds.contains(&"project_preference_candidate"));
        assert!(kinds.contains(&"archive_candidate"));

        let report_path = tmp.path().join(value["reportPath"].as_str().unwrap());
        let json_path = tmp.path().join(value["jsonPath"].as_str().unwrap());
        assert!(report_path.exists());
        assert!(json_path.exists());
        let report = std::fs::read_to_string(report_path).expect("report");
        assert!(report.contains("# Learning Self-Evolution Report"));
        assert!(report.contains("Report-only self-evolution"));
    }

    async fn execute_to_json(
        ctx: &ToolContext,
        args: LearningSelfEvolutionReportArgs,
    ) -> JsonValue {
        let mut stream = LearningSelfEvolutionReportTool::execute(ctx, args)
            .await
            .expect("execute self-evolution report");
        while let Some(item) = stream.next().await {
            if let StreamOutputItem::Text(text) = item {
                return serde_json::from_str(&text).expect("json");
            }
        }
        panic!("learning_self_evolution_report did not return text output");
    }
}
