//! Project-scoped learning proposals distilled from execution records.
//!
//! This layer is intentionally proposal-first: it records what the agent
//! believes is worth crystallizing, but it does not mutate operators,
//! templates, skills, or archived result folders by itself. UI and autonomous
//! learning agents can use the stored proposals to ask for confirmation and
//! then perform a concrete apply step in a later flow.

use crate::domain::execution_records::ExecutionRecord;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

const LEARNING_PROPOSALS_RELATIVE_PATH: &str = ".omiga/learning/proposals.json";
const STORE_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LearningProposalStatus {
    Proposed,
    Approved,
    Applied,
    Dismissed,
    Snoozed,
}

impl LearningProposalStatus {
    pub fn is_pending(&self) -> bool {
        matches!(self, Self::Proposed)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LearningProposalKind {
    ReusableChoice,
    ArchiveResult,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LearningProposalDecision {
    Approve,
    Dismiss,
    Snooze,
    MarkApplied,
}

impl LearningProposalDecision {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "approve" | "approved" => Some(Self::Approve),
            "dismiss" | "dismissed" => Some(Self::Dismiss),
            "snooze" | "snoozed" => Some(Self::Snooze),
            "mark_applied" | "markapplied" | "applied" => Some(Self::MarkApplied),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LearningProposalAction {
    pub id: String,
    pub label: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LearningProposal {
    pub id: String,
    pub status: LearningProposalStatus,
    pub kind: LearningProposalKind,
    pub title: String,
    pub summary: String,
    pub user_message: String,
    pub proposed_action: String,
    pub source_record_ids: Vec<String>,
    pub recommendation_actions: Vec<String>,
    pub evidence: JsonValue,
    pub actions: Vec<LearningProposalAction>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision_note: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LearningProposalSummary {
    pub total_count: usize,
    pub pending_count: usize,
    pub approved_count: usize,
    pub applied_count: usize,
    pub dismissed_count: usize,
    pub snoozed_count: usize,
    pub generated_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LearningProposalStore {
    pub schema_version: u32,
    pub proposals: Vec<LearningProposal>,
}

impl Default for LearningProposalStore {
    fn default() -> Self {
        Self {
            schema_version: STORE_SCHEMA_VERSION,
            proposals: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LearningProposalList {
    pub store_path: String,
    pub summary: LearningProposalSummary,
    pub proposals: Vec<LearningProposal>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LearningProposalDecisionResult {
    pub store_path: String,
    pub proposal: LearningProposal,
    pub notification: String,
}

pub fn learning_proposals_path(project_root: &Path) -> PathBuf {
    project_root.join(LEARNING_PROPOSALS_RELATIVE_PATH)
}

pub async fn refresh_and_list_learning_proposals(
    project_root: &Path,
    limit: usize,
    include_decided: bool,
) -> Result<LearningProposalList, String> {
    let mut store = load_store(project_root)?;
    let generated_count =
        refresh_store_from_execution_records(project_root, limit, &mut store).await?;
    save_store(project_root, &store)?;
    Ok(list_from_store(
        project_root,
        store,
        include_decided,
        generated_count,
    ))
}

pub fn list_learning_proposals(
    project_root: &Path,
    include_decided: bool,
) -> Result<LearningProposalList, String> {
    let store = load_store(project_root)?;
    Ok(list_from_store(project_root, store, include_decided, 0))
}

pub fn decide_learning_proposal(
    project_root: &Path,
    proposal_id: &str,
    decision: LearningProposalDecision,
    note: Option<String>,
) -> Result<LearningProposalDecisionResult, String> {
    let mut store = load_store(project_root)?;
    let now = now_rfc3339();
    let proposal = store
        .proposals
        .iter_mut()
        .find(|proposal| proposal.id == proposal_id)
        .ok_or_else(|| format!("learning proposal `{proposal_id}` not found"))?;
    proposal.status = match decision {
        LearningProposalDecision::Approve => LearningProposalStatus::Approved,
        LearningProposalDecision::Dismiss => LearningProposalStatus::Dismissed,
        LearningProposalDecision::Snooze => LearningProposalStatus::Snoozed,
        LearningProposalDecision::MarkApplied => LearningProposalStatus::Applied,
    };
    proposal.updated_at = now;
    proposal.decision_note = note;
    let proposal = proposal.clone();
    save_store(project_root, &store)?;
    let notification = decision_notification(&proposal, decision);
    Ok(LearningProposalDecisionResult {
        store_path: learning_proposals_path(project_root)
            .to_string_lossy()
            .into_owned(),
        proposal,
        notification,
    })
}

fn load_store(project_root: &Path) -> Result<LearningProposalStore, String> {
    let path = learning_proposals_path(project_root);
    if !path.is_file() {
        return Ok(LearningProposalStore::default());
    }
    let raw = fs::read_to_string(&path)
        .map_err(|err| format!("read learning proposal store `{}`: {err}", path.display()))?;
    serde_json::from_str::<LearningProposalStore>(&raw)
        .or_else(|_| {
            serde_json::from_str::<Vec<LearningProposal>>(&raw).map(|proposals| {
                LearningProposalStore {
                    schema_version: STORE_SCHEMA_VERSION,
                    proposals,
                }
            })
        })
        .map_err(|err| format!("parse learning proposal store `{}`: {err}", path.display()))
}

fn save_store(project_root: &Path, store: &LearningProposalStore) -> Result<(), String> {
    let path = learning_proposals_path(project_root);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("create learning proposal dir `{}`: {err}", parent.display()))?;
    }
    let raw = serde_json::to_string_pretty(store)
        .map_err(|err| format!("serialize learning proposal store: {err}"))?;
    fs::write(&path, raw)
        .map_err(|err| format!("write learning proposal store `{}`: {err}", path.display()))
}

async fn refresh_store_from_execution_records(
    project_root: &Path,
    limit: usize,
    store: &mut LearningProposalStore,
) -> Result<usize, String> {
    let records =
        crate::domain::execution_records::list_recent_execution_records(project_root, limit)
            .await?;
    let existing = store
        .proposals
        .iter()
        .map(|proposal| (proposal.id.clone(), proposal.status.clone()))
        .collect::<HashMap<_, _>>();

    let now = now_rfc3339();
    let mut generated = 0;
    for mut candidate in generate_learning_proposals_from_records(&records, &now) {
        if let Some(status) = existing.get(&candidate.id) {
            candidate.status = status.clone();
            continue;
        }
        store.proposals.push(candidate);
        generated += 1;
    }
    sort_proposals(&mut store.proposals);
    Ok(generated)
}

fn list_from_store(
    project_root: &Path,
    store: LearningProposalStore,
    include_decided: bool,
    generated_count: usize,
) -> LearningProposalList {
    let mut proposals = store.proposals;
    sort_proposals(&mut proposals);
    if !include_decided {
        proposals.retain(|proposal| proposal.status.is_pending());
    }
    let mut summary = summarize_proposals(&proposals);
    summary.generated_count = generated_count;
    LearningProposalList {
        store_path: learning_proposals_path(project_root)
            .to_string_lossy()
            .into_owned(),
        summary,
        proposals,
    }
}

fn summarize_proposals(proposals: &[LearningProposal]) -> LearningProposalSummary {
    let mut summary = LearningProposalSummary {
        total_count: proposals.len(),
        ..LearningProposalSummary::default()
    };
    for proposal in proposals {
        match proposal.status {
            LearningProposalStatus::Proposed => summary.pending_count += 1,
            LearningProposalStatus::Approved => summary.approved_count += 1,
            LearningProposalStatus::Applied => summary.applied_count += 1,
            LearningProposalStatus::Dismissed => summary.dismissed_count += 1,
            LearningProposalStatus::Snoozed => summary.snoozed_count += 1,
        }
    }
    summary
}

fn generate_learning_proposals_from_records(
    records: &[ExecutionRecord],
    now: &str,
) -> Vec<LearningProposal> {
    let mut proposals = Vec::new();
    for record in records.iter().filter(|record| record.status == "succeeded") {
        let metadata = parse_json(record.metadata_json.as_deref());
        let runtime = parse_json(record.runtime_json.as_deref());
        let output_summary = parse_json(record.output_summary_json.as_deref());
        let metadata_ref = metadata.as_ref();
        let output_ref = output_summary.as_ref();
        let param_sources = param_source_summary(metadata_ref);
        let answered_params = preflight_answered_params(metadata_ref);
        let has_user_preflight = param_sources.get("user_preflight").copied().unwrap_or(0) > 0
            || !answered_params.is_empty();
        let artifact_paths = artifact_paths(record, metadata_ref, output_ref);
        let run_dir = string_at(metadata_ref, &["/runDir", "/run_dir"])
            .or_else(|| string_at(runtime.as_ref(), &["/runDir", "/run_dir"]));
        let provenance_path = string_at(metadata_ref, &["/provenancePath", "/provenance_path"]);
        let unit_label = unit_label(record);

        if has_user_preflight {
            proposals.push(reusable_choice_proposal(
                record,
                &unit_label,
                &param_sources,
                &answered_params,
                &artifact_paths,
                run_dir.as_deref(),
                provenance_path.as_deref(),
                now,
            ));
        }

        if record.parent_execution_id.is_none()
            && (!artifact_paths.is_empty() || run_dir.is_some() || provenance_path.is_some())
        {
            proposals.push(archive_result_proposal(
                record,
                &unit_label,
                &artifact_paths,
                run_dir.as_deref(),
                provenance_path.as_deref(),
                now,
            ));
        }
    }
    proposals
}

#[allow(clippy::too_many_arguments)]
fn reusable_choice_proposal(
    record: &ExecutionRecord,
    unit_label: &str,
    param_sources: &BTreeMap<String, usize>,
    answered_params: &[String],
    artifact_paths: &[String],
    run_dir: Option<&str>,
    provenance_path: Option<&str>,
    now: &str,
) -> LearningProposal {
    let answered = if answered_params.is_empty() {
        "用户在预检中确认了参数".to_string()
    } else {
        format!("用户确认了参数：{}", answered_params.join(", "))
    };
    LearningProposal {
        id: stable_proposal_id("reusable_choice", record),
        status: LearningProposalStatus::Proposed,
        kind: LearningProposalKind::ReusableChoice,
        title: format!("保存 `{unit_label}` 的参数选择"),
        summary: format!(
            "{answered}。建议让用户确认是否固化为项目偏好、模板默认值候选或示例参数。"
        ),
        user_message: format!(
            "我发现本次 `{unit_label}` 运行包含可复用的用户选择。是否把这些选择保存为项目学习记录，供后续类似任务优先复用？"
        ),
        proposed_action: "save_project_preference_candidate".to_string(),
        source_record_ids: vec![record.id.clone()],
        recommendation_actions: vec!["promote_reusable_choice".to_string()],
        evidence: serde_json::json!({
            "unitId": record.unit_id,
            "canonicalId": record.canonical_id,
            "providerPlugin": record.provider_plugin,
            "paramSourceSummary": param_sources,
            "answeredParams": answered_params,
            "runDir": run_dir,
            "provenancePath": provenance_path,
            "artifactPaths": artifact_paths,
        }),
        actions: confirmation_actions(),
        decision_note: None,
        created_at: now.to_string(),
        updated_at: now.to_string(),
    }
}

fn archive_result_proposal(
    record: &ExecutionRecord,
    unit_label: &str,
    artifact_paths: &[String],
    run_dir: Option<&str>,
    provenance_path: Option<&str>,
    now: &str,
) -> LearningProposal {
    LearningProposal {
        id: stable_proposal_id("archive_result", record),
        status: LearningProposalStatus::Proposed,
        kind: LearningProposalKind::ArchiveResult,
        title: format!("封存 `{unit_label}` 的成功结果"),
        summary: "本次运行已成功并产生了可追溯输出。建议让用户确认是否把结果封存为项目记录。"
            .to_string(),
        user_message: format!(
            "本次 `{unit_label}` 已成功产生结果。是否将输出和 provenance 封存为项目记录，便于后续任务自动引用？"
        ),
        proposed_action: "archive_result_candidate".to_string(),
        source_record_ids: vec![record.id.clone()],
        recommendation_actions: vec!["archive_result".to_string()],
        evidence: serde_json::json!({
            "unitId": record.unit_id,
            "canonicalId": record.canonical_id,
            "providerPlugin": record.provider_plugin,
            "runDir": run_dir,
            "provenancePath": provenance_path,
            "artifactPaths": artifact_paths,
        }),
        actions: confirmation_actions(),
        decision_note: None,
        created_at: now.to_string(),
        updated_at: now.to_string(),
    }
}

fn confirmation_actions() -> Vec<LearningProposalAction> {
    vec![
        LearningProposalAction {
            id: "approve".to_string(),
            label: "保存".to_string(),
            description: "确认这是值得固化的项目学习记录；后续 apply 流程可再落到模板/偏好/归档。"
                .to_string(),
        },
        LearningProposalAction {
            id: "dismiss".to_string(),
            label: "忽略".to_string(),
            description: "本次不保存该学习建议。".to_string(),
        },
        LearningProposalAction {
            id: "snooze".to_string(),
            label: "稍后提醒".to_string(),
            description: "暂缓处理，避免打断当前分析。".to_string(),
        },
    ]
}

fn decision_notification(
    proposal: &LearningProposal,
    decision: LearningProposalDecision,
) -> String {
    match decision {
        LearningProposalDecision::Approve => format!(
            "已保存学习建议：{}。后续可由学习 agent 将其应用为模板默认值、项目偏好或结果封存动作。",
            proposal.title
        ),
        LearningProposalDecision::Dismiss => format!("已忽略学习建议：{}。", proposal.title),
        LearningProposalDecision::Snooze => {
            format!("已暂缓学习建议：{}，后续可再次提醒。", proposal.title)
        }
        LearningProposalDecision::MarkApplied => format!("已标记为完成固化：{}。", proposal.title),
    }
}

fn stable_proposal_id(kind: &str, record: &ExecutionRecord) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(kind.as_bytes());
    hasher.update(b"\0");
    hasher.update(record.id.as_bytes());
    hasher.update(b"\0");
    if let Some(param_hash) = record.param_hash.as_deref() {
        hasher.update(param_hash.as_bytes());
    }
    let digest = hasher.finalize();
    let short = digest
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("learn_{short}")
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

fn sort_proposals(proposals: &mut [LearningProposal]) {
    proposals.sort_by(|left, right| {
        status_rank(&left.status)
            .cmp(&status_rank(&right.status))
            .then_with(|| right.updated_at.cmp(&left.updated_at))
            .then_with(|| left.id.cmp(&right.id))
    });
}

fn status_rank(status: &LearningProposalStatus) -> u8 {
    match status {
        LearningProposalStatus::Proposed => 0,
        LearningProposalStatus::Snoozed => 1,
        LearningProposalStatus::Approved => 2,
        LearningProposalStatus::Applied => 3,
        LearningProposalStatus::Dismissed => 4,
    }
}

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::execution_records::{record_execution, ExecutionRecordInput};
    use serde_json::json;

    #[tokio::test]
    async fn refresh_generates_user_facing_learning_proposals() {
        let tmp = tempfile::tempdir().unwrap();
        record_execution(
            tmp.path(),
            ExecutionRecordInput {
                kind: "operator".to_string(),
                unit_id: Some("operator-differential-expression-basic".to_string()),
                canonical_id: Some(
                    "plugin/operator/operator-differential-expression-basic".to_string(),
                ),
                provider_plugin: Some("omics".to_string()),
                status: "succeeded".to_string(),
                session_id: Some("session-1".to_string()),
                parent_execution_id: None,
                started_at: Some("2026-05-09T00:00:00Z".to_string()),
                ended_at: Some("2026-05-09T00:00:01Z".to_string()),
                input_hash: Some("sha256:input".to_string()),
                param_hash: Some("sha256:param".to_string()),
                output_summary_json: Some(json!({
                    "outputs": {
                        "table": {"path": "de-results.tsv"}
                    }
                })),
                runtime_json: None,
                metadata_json: Some(json!({
                    "runDir": ".omiga/runs/oprun_1",
                    "provenancePath": ".omiga/runs/oprun_1/provenance.json",
                    "paramSources": {
                        "method": "user_preflight",
                        "fdr": "default"
                    },
                    "preflight": {
                        "answeredParams": [{"param": "method"}]
                    }
                })),
            },
        )
        .await
        .unwrap();

        let listed = refresh_and_list_learning_proposals(tmp.path(), 50, false)
            .await
            .unwrap();
        assert_eq!(listed.summary.generated_count, 2);
        assert_eq!(listed.summary.pending_count, 2);
        assert!(listed
            .proposals
            .iter()
            .any(|proposal| proposal.kind == LearningProposalKind::ReusableChoice));
        assert!(listed
            .proposals
            .iter()
            .any(|proposal| proposal.kind == LearningProposalKind::ArchiveResult));

        let second = refresh_and_list_learning_proposals(tmp.path(), 50, false)
            .await
            .unwrap();
        assert_eq!(second.summary.generated_count, 0);
        assert_eq!(second.summary.pending_count, 2);
    }

    #[tokio::test]
    async fn decision_updates_proposal_status_and_notification() {
        let tmp = tempfile::tempdir().unwrap();
        record_execution(
            tmp.path(),
            ExecutionRecordInput {
                kind: "operator".to_string(),
                unit_id: Some("demo".to_string()),
                canonical_id: None,
                provider_plugin: None,
                status: "succeeded".to_string(),
                session_id: None,
                parent_execution_id: None,
                started_at: None,
                ended_at: None,
                input_hash: None,
                param_hash: Some("sha256:param".to_string()),
                output_summary_json: None,
                runtime_json: None,
                metadata_json: Some(json!({
                    "paramSources": {"method": "user_preflight"},
                    "preflight": {"answeredParams": [{"param": "method"}]}
                })),
            },
        )
        .await
        .unwrap();
        let listed = refresh_and_list_learning_proposals(tmp.path(), 50, false)
            .await
            .unwrap();
        let id = listed.proposals[0].id.clone();

        let decided = decide_learning_proposal(
            tmp.path(),
            &id,
            LearningProposalDecision::Approve,
            Some("good default".to_string()),
        )
        .unwrap();

        assert_eq!(decided.proposal.status, LearningProposalStatus::Approved);
        assert!(decided.notification.contains("已保存学习建议"));
        let pending = list_learning_proposals(tmp.path(), false).unwrap();
        assert_eq!(pending.summary.pending_count, 0);
        let all = list_learning_proposals(tmp.path(), true).unwrap();
        assert_eq!(all.summary.approved_count, 1);
    }
}
