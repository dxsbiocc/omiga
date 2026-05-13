use super::{
    learning_self_evolution_report::{
        LearningSelfEvolutionReportArgs, LearningSelfEvolutionReportTool,
    },
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
    "Write non-applied draft scaffolds from self-evolution report candidates for human review.";

const DRAFT_DIR_RELATIVE: &str = ".omiga/learning/self-evolution-drafts";
const SAFETY_NOTE: &str = "Draft-only self-evolution. This tool writes review files under .omiga/learning/self-evolution-drafts and does not register, enable, publish, or modify Operators, Templates, Skills, defaults, or archives.";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LearningSelfEvolutionDraftWriteArgs {
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default, rename = "maxDrafts")]
    pub max_drafts: Option<usize>,
    #[serde(default, rename = "candidateKinds")]
    pub candidate_kinds: Vec<String>,
    #[serde(default, rename = "reviewNote")]
    pub review_note: Option<String>,
}

impl Default for LearningSelfEvolutionDraftWriteArgs {
    fn default() -> Self {
        Self {
            limit: None,
            max_drafts: Some(5),
            candidate_kinds: Vec::new(),
            review_note: None,
        }
    }
}

pub struct LearningSelfEvolutionDraftWriteTool;

#[async_trait]
impl ToolImpl for LearningSelfEvolutionDraftWriteTool {
    type Args = LearningSelfEvolutionDraftWriteArgs;

    const DESCRIPTION: &'static str = DESCRIPTION;

    async fn execute(
        ctx: &ToolContext,
        args: Self::Args,
    ) -> Result<crate::infrastructure::streaming::StreamOutputBox, ToolError> {
        let report = report_value(ctx, args.limit).await?;
        let generated_at = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        let candidates = select_candidates(&report, &args);
        let batch_dir = ctx.project_root.join(DRAFT_DIR_RELATIVE).join(format!(
            "draft-batch-{}-{}",
            Utc::now().format("%Y%m%dT%H%M%SZ"),
            Uuid::new_v4().simple()
        ));
        tokio::fs::create_dir_all(&batch_dir)
            .await
            .map_err(|err| ToolError::ExecutionFailed {
                message: format!("create self-evolution draft batch dir: {err}"),
            })?;

        let mut drafts = Vec::new();
        for (index, candidate) in candidates.iter().enumerate() {
            let draft = write_candidate_draft(
                ctx,
                &batch_dir,
                index + 1,
                candidate,
                &generated_at,
                args.review_note.as_deref(),
            )
            .await?;
            drafts.push(draft);
        }

        let index_markdown =
            render_batch_index(&generated_at, &drafts, args.review_note.as_deref());
        let index_path = batch_dir.join("README.md");
        tokio::fs::write(&index_path, index_markdown)
            .await
            .map_err(|err| ToolError::ExecutionFailed {
                message: format!("write self-evolution draft batch index: {err}"),
            })?;

        let output = serde_json::json!({
            "status": "succeeded",
            "generatedAt": generated_at,
            "batchDir": project_relative_path(&ctx.project_root, &batch_dir),
            "indexPath": project_relative_path(&ctx.project_root, &index_path),
            "draftCount": drafts.len(),
            "drafts": drafts,
            "sourceReportSummary": report.get("summary").cloned().unwrap_or(JsonValue::Null),
            "safetyNote": SAFETY_NOTE,
        });
        Ok(stream_single(StreamOutputItem::Text(
            serde_json::to_string_pretty(&output).unwrap_or_else(|_| "{}".to_string()),
        )))
    }
}

pub fn schema() -> ToolSchema {
    ToolSchema::new(
        "learning_self_evolution_draft_write",
        DESCRIPTION,
        serde_json::json!({
            "type": "object",
            "properties": {
                "limit": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 200,
                    "description": "Maximum recent ExecutionRecords to scan before generating draft scaffolds; defaults to 100."
                },
                "maxDrafts": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 20,
                    "description": "Maximum candidate drafts to write; defaults to 5."
                },
                "candidateKinds": {
                    "type": "array",
                    "items": {
                        "type": "string",
                        "enum": ["template_candidate", "operator_candidate", "project_preference_candidate", "archive_candidate"]
                    },
                    "description": "Optional candidate kinds to include; when empty, includes all supported kinds."
                },
                "reviewNote": {
                    "type": "string",
                    "description": "Optional note copied into every draft README for reviewer context."
                }
            }
        }),
    )
}

async fn report_value(ctx: &ToolContext, limit: Option<usize>) -> Result<JsonValue, ToolError> {
    let mut stream = LearningSelfEvolutionReportTool::execute(
        ctx,
        LearningSelfEvolutionReportArgs {
            limit,
            write_report: false,
            include_records: false,
        },
    )
    .await?;
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
            message: "learning_self_evolution_report returned no JSON output".to_string(),
        });
    }
    serde_json::from_str(&text).map_err(|err| ToolError::ExecutionFailed {
        message: format!("parse learning_self_evolution_report output: {err}"),
    })
}

fn select_candidates(
    report: &JsonValue,
    args: &LearningSelfEvolutionDraftWriteArgs,
) -> Vec<JsonValue> {
    let max = args.max_drafts.unwrap_or(5).clamp(1, 20);
    let allowed = args
        .candidate_kinds
        .iter()
        .map(|kind| kind.trim().to_ascii_lowercase())
        .filter(|kind| !kind.is_empty())
        .collect::<std::collections::HashSet<_>>();
    report
        .get("candidates")
        .and_then(JsonValue::as_array)
        .into_iter()
        .flatten()
        .filter(|candidate| {
            if allowed.is_empty() {
                return true;
            }
            candidate
                .get("kind")
                .and_then(JsonValue::as_str)
                .map(|kind| allowed.contains(&kind.to_ascii_lowercase()))
                .unwrap_or(false)
        })
        .take(max)
        .cloned()
        .collect()
}

async fn write_candidate_draft(
    ctx: &ToolContext,
    batch_dir: &Path,
    index: usize,
    candidate: &JsonValue,
    generated_at: &str,
    review_note: Option<&str>,
) -> Result<JsonValue, ToolError> {
    let candidate_id = string_field(candidate, "id").unwrap_or("candidate");
    let kind = string_field(candidate, "kind").unwrap_or("candidate");
    let draft_dir = batch_dir.join(format!(
        "{index:02}-{}-{}",
        safe_slug(kind),
        safe_slug(candidate_id)
    ));
    tokio::fs::create_dir_all(&draft_dir)
        .await
        .map_err(|err| ToolError::ExecutionFailed {
            message: format!("create candidate draft dir: {err}"),
        })?;

    let mut files = Vec::new();
    let readme_path = draft_dir.join("DRAFT.md");
    tokio::fs::write(
        &readme_path,
        render_candidate_readme(candidate, generated_at, review_note),
    )
    .await
    .map_err(|err| ToolError::ExecutionFailed {
        message: format!("write candidate draft README: {err}"),
    })?;
    files.push(project_relative_path(&ctx.project_root, &readme_path));

    let candidate_json_path = draft_dir.join("candidate.json");
    let candidate_json =
        serde_json::to_string_pretty(candidate).map_err(|err| ToolError::ExecutionFailed {
            message: format!("serialize candidate JSON: {err}"),
        })?;
    tokio::fs::write(&candidate_json_path, candidate_json)
        .await
        .map_err(|err| ToolError::ExecutionFailed {
            message: format!("write candidate JSON: {err}"),
        })?;
    files.push(project_relative_path(
        &ctx.project_root,
        &candidate_json_path,
    ));

    if let Some((filename, contents)) = specialized_draft_file(candidate) {
        let path = draft_dir.join(filename);
        tokio::fs::write(&path, contents)
            .await
            .map_err(|err| ToolError::ExecutionFailed {
                message: format!("write specialized candidate draft: {err}"),
            })?;
        files.push(project_relative_path(&ctx.project_root, &path));
    }

    Ok(serde_json::json!({
        "candidateId": candidate_id,
        "kind": kind,
        "title": string_field(candidate, "title"),
        "draftDir": project_relative_path(&ctx.project_root, &draft_dir),
        "files": files,
        "safetyNote": SAFETY_NOTE,
    }))
}

fn render_batch_index(
    generated_at: &str,
    drafts: &[JsonValue],
    review_note: Option<&str>,
) -> String {
    let mut out = String::new();
    out.push_str("# Self-Evolution Draft Batch\n\n");
    out.push_str(&format!("- Generated at: `{generated_at}`\n"));
    out.push_str(&format!("- Safety: {SAFETY_NOTE}\n"));
    if let Some(note) = review_note.filter(|note| !note.trim().is_empty()) {
        out.push_str(&format!("- Review note: {}\n", inline(note)));
    }
    out.push_str(&format!("- Draft count: `{}`\n\n", drafts.len()));
    for draft in drafts {
        out.push_str(&format!(
            "- `{}` · `{}` · {}\n",
            string_field(draft, "candidateId").unwrap_or("candidate"),
            string_field(draft, "kind").unwrap_or("candidate"),
            string_field(draft, "draftDir").unwrap_or("")
        ));
    }
    out
}

fn render_candidate_readme(
    candidate: &JsonValue,
    generated_at: &str,
    review_note: Option<&str>,
) -> String {
    let mut out = String::new();
    out.push_str("# Self-Evolution Candidate Draft\n\n");
    out.push_str(&format!("- Generated at: `{generated_at}`\n"));
    out.push_str(&format!("- Safety: {SAFETY_NOTE}\n"));
    if let Some(note) = review_note.filter(|note| !note.trim().is_empty()) {
        out.push_str(&format!("- Review note: {}\n", inline(note)));
    }
    out.push('\n');
    out.push_str(&format!(
        "## {}\n\n",
        inline(string_field(candidate, "title").unwrap_or("Candidate"))
    ));
    out.push_str(&format!(
        "- Candidate id: `{}`\n",
        inline(string_field(candidate, "id").unwrap_or("candidate"))
    ));
    out.push_str(&format!(
        "- Kind: `{}`\n",
        inline(string_field(candidate, "kind").unwrap_or("candidate"))
    ));
    out.push_str(&format!(
        "- Priority: `{}`\n",
        inline(string_field(candidate, "priority").unwrap_or("unknown"))
    ));
    if let Some(rationale) = string_field(candidate, "rationale") {
        out.push_str(&format!("- Rationale: {}\n", inline(rationale)));
    }
    if let Some(next_step) = string_field(candidate, "proposedNextStep") {
        out.push_str(&format!("- Proposed next step: {}\n", inline(next_step)));
    }
    if let Some(ids) = candidate
        .get("sourceRecordIds")
        .and_then(JsonValue::as_array)
    {
        let joined = ids
            .iter()
            .filter_map(JsonValue::as_str)
            .map(inline)
            .collect::<Vec<_>>()
            .join("`, `");
        if !joined.is_empty() {
            out.push_str(&format!("- Source records: `{joined}`\n"));
        }
    }
    out.push_str("\n## Reviewer checklist\n\n");
    out.push_str("- [ ] Confirm this candidate is still relevant.\n");
    out.push_str("- [ ] Inspect provenance and artifact paths in `candidate.json`.\n");
    out.push_str("- [ ] Add or update deterministic fixtures before publishing any real unit.\n");
    out.push_str("- [ ] Keep generated defaults subordinate to explicit user params.\n");
    out.push_str("- [ ] For Operator/Template candidates, invoke `self-evolution-unit-creator` after bootstrapping it with `learning_self_evolution_creator`.\n");
    out.push_str("- [ ] Use a separate reviewed change to promote this draft into active project code/config.\n");
    out
}

fn specialized_draft_file(candidate: &JsonValue) -> Option<(&'static str, String)> {
    match string_field(candidate, "kind")? {
        "template_candidate" => Some(("template.yaml.draft", template_yaml_draft(candidate))),
        "operator_candidate" => Some(("operator.yaml.draft", operator_yaml_draft(candidate))),
        "project_preference_candidate" => Some((
            "project-preference.json.draft",
            project_preference_draft(candidate),
        )),
        "archive_candidate" => Some(("archive-marker.json.draft", archive_marker_draft(candidate))),
        _ => None,
    }
}

fn template_yaml_draft(candidate: &JsonValue) -> String {
    let title = string_field(candidate, "title").unwrap_or("Draft Template");
    let slug = safe_slug(title);
    let source_records = string_array(candidate.get("sourceRecordIds"));
    format!(
        r#"# REVIEW DRAFT ONLY — not loaded by Omiga until moved into a real plugin template.yaml.
apiVersion: omiga.ai/template/v1alpha1
kind: Template
metadata:
  id: {slug}
  version: 0.1.0
  name: {title:?}
  description: {description:?}
  tags: [self-evolution-draft]
classification:
  category: draft/self-evolution
runtime:
  type: command
template:
  engine: jinja2
  entry: template.sh.j2
execution:
  mode: rendered
review:
  sourceRecordIds: {source_records:?}
  safetyNote: {SAFETY_NOTE:?}
"#,
        description = string_field(candidate, "rationale")
            .unwrap_or("Draft generated from ExecutionRecord lineage for human review.")
    )
}

fn operator_yaml_draft(candidate: &JsonValue) -> String {
    let title = string_field(candidate, "title").unwrap_or("Draft Operator");
    let slug = safe_slug(title);
    let source_records = string_array(candidate.get("sourceRecordIds"));
    format!(
        r#"# REVIEW DRAFT ONLY — not loaded by Omiga until moved into a real plugin operator.yaml.
apiVersion: omiga.ai/operator/v1alpha1
kind: Operator
metadata:
  id: {slug}
  version: 0.1.0
  name: {title:?}
  description: {description:?}
  tags: [self-evolution-draft]
runtime:
  type: local
  command: echo
  args: ["Replace this draft with a reviewed implementation"]
parameters: []
inputs: []
outputs: []
review:
  sourceRecordIds: {source_records:?}
  safetyNote: {SAFETY_NOTE:?}
"#,
        description = string_field(candidate, "rationale")
            .unwrap_or("Draft generated from ExecutionRecord evidence for human review.")
    )
}

fn project_preference_draft(candidate: &JsonValue) -> String {
    let evidence = candidate
        .get("evidence")
        .cloned()
        .unwrap_or(JsonValue::Null);
    serde_json::to_string_pretty(&serde_json::json!({
        "status": "draft",
        "scope": "project",
        "unitId": evidence.get("unitId").cloned().unwrap_or(JsonValue::Null),
        "canonicalId": evidence.get("canonicalId").cloned().unwrap_or(JsonValue::Null),
        "providerPlugin": evidence.get("providerPlugin").cloned().unwrap_or(JsonValue::Null),
        "params": evidence.get("selectedParams").cloned().unwrap_or_else(|| serde_json::json!({})),
        "answeredParams": evidence.get("answeredParams").cloned().unwrap_or_else(|| serde_json::json!([])),
        "sourceRecordIds": candidate.get("sourceRecordIds").cloned().unwrap_or_else(|| serde_json::json!([])),
        "safetyNote": SAFETY_NOTE,
    }))
    .unwrap_or_else(|_| "{}".to_string())
}

fn archive_marker_draft(candidate: &JsonValue) -> String {
    let evidence = candidate
        .get("evidence")
        .cloned()
        .unwrap_or(JsonValue::Null);
    serde_json::to_string_pretty(&serde_json::json!({
        "status": "draft",
        "unitId": evidence.get("unitId").cloned().unwrap_or(JsonValue::Null),
        "canonicalId": evidence.get("canonicalId").cloned().unwrap_or(JsonValue::Null),
        "providerPlugin": evidence.get("providerPlugin").cloned().unwrap_or(JsonValue::Null),
        "runDir": evidence.get("runDir").cloned().unwrap_or(JsonValue::Null),
        "provenancePath": evidence.get("provenancePath").cloned().unwrap_or(JsonValue::Null),
        "artifactPaths": evidence.get("artifactPaths").cloned().unwrap_or_else(|| serde_json::json!([])),
        "sourceRecordIds": candidate.get("sourceRecordIds").cloned().unwrap_or_else(|| serde_json::json!([])),
        "safetyNote": SAFETY_NOTE,
    }))
    .unwrap_or_else(|_| "{}".to_string())
}

fn string_field<'a>(value: &'a JsonValue, field: &str) -> Option<&'a str> {
    value.get(field).and_then(JsonValue::as_str)
}

fn string_array(value: Option<&JsonValue>) -> Vec<String> {
    value
        .and_then(JsonValue::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(JsonValue::as_str)
                .map(ToOwned::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn safe_slug(value: &str) -> String {
    let slug = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if slug.is_empty() {
        "draft".to_string()
    } else {
        slug.chars().take(80).collect()
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
    use serde_json::json;

    #[tokio::test]
    async fn writes_review_only_self_evolution_draft_scaffolds() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let parent_id = record_execution(
            tmp.path(),
            ExecutionRecordInput {
                kind: "template".to_string(),
                unit_id: Some("draft_demo".to_string()),
                canonical_id: Some("plugin/template/draft_demo".to_string()),
                provider_plugin: Some("omics".to_string()),
                status: "succeeded".to_string(),
                session_id: Some("session-1".to_string()),
                parent_execution_id: None,
                started_at: Some("2026-05-10T00:00:00Z".to_string()),
                ended_at: Some("2026-05-10T00:00:02Z".to_string()),
                input_hash: Some("sha256:input".to_string()),
                param_hash: Some("sha256:param".to_string()),
                output_summary_json: Some(json!({
                    "outputs": [{"path": ".omiga/runs/draft-demo/out/report.md"}]
                })),
                runtime_json: None,
                metadata_json: Some(json!({
                    "runDir": ".omiga/runs/draft-demo",
                    "provenancePath": ".omiga/runs/draft-demo/provenance.json",
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
                unit_id: Some("draft_child".to_string()),
                canonical_id: Some("plugin/operator/draft_child".to_string()),
                provider_plugin: Some("omics".to_string()),
                status: "succeeded".to_string(),
                session_id: Some("session-1".to_string()),
                parent_execution_id: Some(parent_id),
                started_at: Some("2026-05-10T00:00:01Z".to_string()),
                ended_at: Some("2026-05-10T00:00:02Z".to_string()),
                input_hash: None,
                param_hash: None,
                output_summary_json: None,
                runtime_json: None,
                metadata_json: Some(json!({"runDir": ".omiga/runs/draft-child"})),
            },
        )
        .await
        .expect("child");

        let value = execute_to_json(
            &ToolContext::new(tmp.path()),
            LearningSelfEvolutionDraftWriteArgs {
                limit: Some(20),
                max_drafts: Some(3),
                candidate_kinds: Vec::new(),
                review_note: Some("phase 11".to_string()),
            },
        )
        .await;

        assert_eq!(value["status"], "succeeded");
        assert!(value["draftCount"].as_u64().unwrap() >= 3);
        let batch_dir = tmp.path().join(value["batchDir"].as_str().unwrap());
        assert!(batch_dir.exists());
        assert!(batch_dir.join("README.md").exists());
        let first = &value["drafts"].as_array().unwrap()[0];
        let draft_dir = tmp.path().join(first["draftDir"].as_str().unwrap());
        assert!(draft_dir.join("DRAFT.md").exists());
        assert!(draft_dir.join("candidate.json").exists());
        let readme = std::fs::read_to_string(draft_dir.join("DRAFT.md")).expect("draft readme");
        assert!(readme.contains("Draft-only self-evolution"));
        assert!(readme.contains("phase 11"));
    }

    async fn execute_to_json(
        ctx: &ToolContext,
        args: LearningSelfEvolutionDraftWriteArgs,
    ) -> JsonValue {
        let mut stream = LearningSelfEvolutionDraftWriteTool::execute(ctx, args)
            .await
            .expect("execute draft write");
        while let Some(item) = stream.next().await {
            if let StreamOutputItem::Text(text) = item {
                return serde_json::from_str(&text).expect("json");
            }
        }
        panic!("learning_self_evolution_draft_write did not return text output");
    }
}
