//! Project-scoped learning proposals distilled from execution records.
//!
//! This layer is intentionally proposal-first: it records what the agent
//! believes is worth crystallizing, but it does not mutate operators,
//! templates, skills, or archived result folders by itself. UI and autonomous
//! learning agents can use the stored proposals to ask for confirmation and
//! then perform a concrete apply step in a later flow.

use crate::domain::execution_records::ExecutionRecord;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

const LEARNING_PROPOSALS_RELATIVE_PATH: &str = ".omiga/learning/proposals.json";
const LEARNING_APPLIED_RELATIVE_PATH: &str = ".omiga/learning/applied.json";
const LEARNING_PREFERENCE_CANDIDATES_RELATIVE_PATH: &str =
    ".omiga/learning/preference-candidates.json";
const LEARNING_PROJECT_PREFERENCES_RELATIVE_PATH: &str = ".omiga/learning/project-preferences.json";
const LEARNING_ARCHIVE_MARKERS_RELATIVE_PATH: &str = ".omiga/learning/archive-markers.json";
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LearningAppliedTarget {
    pub target_type: String,
    pub path: String,
    pub record_id: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LearningApplyRecord {
    pub id: String,
    pub proposal_id: String,
    pub kind: LearningProposalKind,
    pub status: String,
    pub source_record_ids: Vec<String>,
    pub targets: Vec<LearningAppliedTarget>,
    pub evidence: JsonValue,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    pub applied_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LearningApplyStore {
    pub schema_version: u32,
    pub records: Vec<LearningApplyRecord>,
}

impl Default for LearningApplyStore {
    fn default() -> Self {
        Self {
            schema_version: STORE_SCHEMA_VERSION,
            records: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LearningPreferenceCandidate {
    pub id: String,
    pub proposal_id: String,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canonical_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_plugin: Option<String>,
    pub answered_params: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub selected_params: BTreeMap<String, JsonValue>,
    pub param_source_summary: BTreeMap<String, usize>,
    pub source_record_ids: Vec<String>,
    pub evidence: JsonValue,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LearningPreferenceCandidateStore {
    pub schema_version: u32,
    pub candidates: Vec<LearningPreferenceCandidate>,
}

impl Default for LearningPreferenceCandidateStore {
    fn default() -> Self {
        Self {
            schema_version: STORE_SCHEMA_VERSION,
            candidates: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LearningProjectPreference {
    pub id: String,
    pub candidate_id: String,
    pub proposal_id: String,
    pub status: String,
    pub scope: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canonical_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_plugin: Option<String>,
    pub params: BTreeMap<String, JsonValue>,
    pub answered_params: Vec<String>,
    pub source_record_ids: Vec<String>,
    pub evidence: JsonValue,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    pub promoted_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LearningProjectPreferenceStore {
    pub schema_version: u32,
    pub preferences: Vec<LearningProjectPreference>,
}

impl Default for LearningProjectPreferenceStore {
    fn default() -> Self {
        Self {
            schema_version: STORE_SCHEMA_VERSION,
            preferences: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LearningPreferenceCandidateSummary {
    pub total_count: usize,
    pub candidate_count: usize,
    pub promoted_count: usize,
    pub missing_selected_params_count: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LearningPreferenceCandidateList {
    pub store_path: String,
    pub project_preferences_path: String,
    pub summary: LearningPreferenceCandidateSummary,
    pub candidates: Vec<LearningPreferenceCandidate>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LearningPreferencePromotionResult {
    pub candidate_store_path: String,
    pub project_preferences_path: String,
    pub candidate: LearningPreferenceCandidate,
    pub preference: LearningProjectPreference,
    pub notification: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LearningProjectPreferenceHint {
    pub id: String,
    pub status: String,
    pub scope: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canonical_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_plugin: Option<String>,
    pub match_reasons: Vec<String>,
    pub params: BTreeMap<String, JsonValue>,
    pub answered_params: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    pub updated_at: String,
    pub summary: String,
    pub safety_note: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LearningProjectPreferenceHintList {
    pub store_path: String,
    pub count: usize,
    pub hints: Vec<LearningProjectPreferenceHint>,
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LearningArchiveMarker {
    pub id: String,
    pub proposal_id: String,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canonical_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_plugin: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_dir: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance_path: Option<String>,
    pub artifact_paths: Vec<String>,
    pub source_record_ids: Vec<String>,
    pub evidence: JsonValue,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LearningArchiveMarkerStore {
    pub schema_version: u32,
    pub markers: Vec<LearningArchiveMarker>,
}

impl Default for LearningArchiveMarkerStore {
    fn default() -> Self {
        Self {
            schema_version: STORE_SCHEMA_VERSION,
            markers: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LearningProposalApplyResult {
    pub proposal_store_path: String,
    pub apply_store_path: String,
    pub proposal: LearningProposal,
    pub apply_record: LearningApplyRecord,
    pub notification: String,
}

pub fn learning_proposals_path(project_root: &Path) -> PathBuf {
    project_root.join(LEARNING_PROPOSALS_RELATIVE_PATH)
}

pub fn learning_applied_path(project_root: &Path) -> PathBuf {
    project_root.join(LEARNING_APPLIED_RELATIVE_PATH)
}

pub fn learning_preference_candidates_path(project_root: &Path) -> PathBuf {
    project_root.join(LEARNING_PREFERENCE_CANDIDATES_RELATIVE_PATH)
}

pub fn learning_project_preferences_path(project_root: &Path) -> PathBuf {
    project_root.join(LEARNING_PROJECT_PREFERENCES_RELATIVE_PATH)
}

pub fn learning_archive_markers_path(project_root: &Path) -> PathBuf {
    project_root.join(LEARNING_ARCHIVE_MARKERS_RELATIVE_PATH)
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

pub fn apply_learning_proposal(
    project_root: &Path,
    proposal_id: &str,
    allow_unapproved: bool,
    note: Option<String>,
) -> Result<LearningProposalApplyResult, String> {
    let mut proposal_store = load_store(project_root)?;
    let now = now_rfc3339();
    let proposal = proposal_store
        .proposals
        .iter_mut()
        .find(|proposal| proposal.id == proposal_id)
        .ok_or_else(|| format!("learning proposal `{proposal_id}` not found"))?;

    if proposal.status != LearningProposalStatus::Approved
        && proposal.status != LearningProposalStatus::Applied
        && !allow_unapproved
    {
        return Err(format!(
            "learning proposal `{proposal_id}` must be approved before apply"
        ));
    }

    let previous_status = proposal.status.clone();
    let apply_record = apply_record_for_proposal(project_root, proposal, note.clone(), &now)?;
    upsert_apply_record(project_root, apply_record.clone())?;

    proposal.status = LearningProposalStatus::Applied;
    proposal.updated_at = now.clone();
    if let Some(note) = note {
        proposal.decision_note = Some(note);
    }
    let proposal = proposal.clone();
    save_store(project_root, &proposal_store)?;

    Ok(LearningProposalApplyResult {
        proposal_store_path: learning_proposals_path(project_root)
            .to_string_lossy()
            .into_owned(),
        apply_store_path: learning_applied_path(project_root)
            .to_string_lossy()
            .into_owned(),
        notification: apply_notification(&proposal, previous_status),
        proposal,
        apply_record,
    })
}

pub fn list_learning_preference_candidates(
    project_root: &Path,
    include_promoted: bool,
) -> Result<LearningPreferenceCandidateList, String> {
    let path = learning_preference_candidates_path(project_root);
    let mut store = load_json_or_default::<LearningPreferenceCandidateStore>(&path)?;
    sort_preference_candidates(&mut store.candidates);
    let mut candidates = store.candidates;
    if !include_promoted {
        candidates.retain(|candidate| candidate.status != "promoted");
    }
    let summary = summarize_preference_candidates(&candidates);
    Ok(LearningPreferenceCandidateList {
        store_path: path.to_string_lossy().into_owned(),
        project_preferences_path: learning_project_preferences_path(project_root)
            .to_string_lossy()
            .into_owned(),
        summary,
        candidates,
    })
}

pub fn promote_learning_preference_candidate(
    project_root: &Path,
    candidate_id: &str,
    note: Option<String>,
) -> Result<LearningPreferencePromotionResult, String> {
    let candidate_id = candidate_id.trim();
    if candidate_id.is_empty() {
        return Err("candidate id must not be empty".to_string());
    }

    let candidate_path = learning_preference_candidates_path(project_root);
    let mut candidate_store =
        load_json_or_default::<LearningPreferenceCandidateStore>(&candidate_path)?;
    let now = now_rfc3339();
    let candidate = candidate_store
        .candidates
        .iter_mut()
        .find(|candidate| candidate.id == candidate_id)
        .ok_or_else(|| format!("learning preference candidate `{candidate_id}` not found"))?;
    if candidate.selected_params.is_empty() {
        return Err(format!(
            "learning preference candidate `{candidate_id}` has no selectedParams to promote"
        ));
    }

    candidate.status = "promoted".to_string();
    candidate.updated_at = now.clone();
    if let Some(note) = note.clone() {
        candidate.note = Some(note);
    }
    let candidate = candidate.clone();
    sort_preference_candidates(&mut candidate_store.candidates);
    save_json_file(&candidate_path, &candidate_store)?;

    let preference = project_preference_for_candidate(&candidate, note, &now);
    upsert_project_preference(project_root, preference.clone())?;

    Ok(LearningPreferencePromotionResult {
        candidate_store_path: candidate_path.to_string_lossy().into_owned(),
        project_preferences_path: learning_project_preferences_path(project_root)
            .to_string_lossy()
            .into_owned(),
        notification: format!(
            "已将 `{}` 提升为项目偏好，后续 agent 可优先复用 {} 个参数。",
            candidate.id,
            preference.params.len()
        ),
        candidate,
        preference,
    })
}

pub fn matching_learning_project_preference_hints(
    project_root: &Path,
    unit_id: Option<&str>,
    canonical_id: Option<&str>,
    provider_plugin: Option<&str>,
) -> Result<LearningProjectPreferenceHintList, String> {
    let path = learning_project_preferences_path(project_root);
    let store = load_json_or_default::<LearningProjectPreferenceStore>(&path)?;
    let mut hints = store
        .preferences
        .into_iter()
        .filter(|preference| preference.status == "active" && !preference.params.is_empty())
        .filter_map(|preference| {
            project_preference_match_reasons(&preference, unit_id, canonical_id, provider_plugin)
                .map(|reasons| project_preference_hint(preference, reasons))
        })
        .collect::<Vec<_>>();
    sort_project_preference_hints(&mut hints);
    Ok(LearningProjectPreferenceHintList {
        store_path: path.to_string_lossy().into_owned(),
        count: hints.len(),
        hints,
        note: "项目偏好只作为推荐，不会自动改写模板代码或覆盖本次明确参数；若与用户当前要求冲突，以当前要求为准。"
            .to_string(),
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

fn apply_record_for_proposal(
    project_root: &Path,
    proposal: &LearningProposal,
    note: Option<String>,
    now: &str,
) -> Result<LearningApplyRecord, String> {
    let mut targets = Vec::new();
    match proposal.kind {
        LearningProposalKind::ReusableChoice => {
            let candidate = preference_candidate_for_proposal(proposal, note.clone(), now);
            let path = learning_preference_candidates_path(project_root);
            upsert_preference_candidate(project_root, candidate.clone())?;
            targets.push(LearningAppliedTarget {
                target_type: "preference_candidate".to_string(),
                path: path.to_string_lossy().into_owned(),
                record_id: candidate.id,
                description:
                    "Saved as a project-scoped preference candidate; not an active default yet."
                        .to_string(),
            });
        }
        LearningProposalKind::ArchiveResult => {
            let marker = archive_marker_for_proposal(proposal, note.clone(), now);
            let path = learning_archive_markers_path(project_root);
            upsert_archive_marker(project_root, marker.clone())?;
            targets.push(LearningAppliedTarget {
                target_type: "archive_marker".to_string(),
                path: path.to_string_lossy().into_owned(),
                record_id: marker.id,
                description:
                    "Saved as a project-scoped archive marker; artifacts are not moved or deleted."
                        .to_string(),
            });
        }
    }

    Ok(LearningApplyRecord {
        id: format!("apply_{}", proposal.id),
        proposal_id: proposal.id.clone(),
        kind: proposal.kind.clone(),
        status: "applied".to_string(),
        source_record_ids: proposal.source_record_ids.clone(),
        targets,
        evidence: proposal.evidence.clone(),
        note,
        applied_at: now.to_string(),
        updated_at: now.to_string(),
    })
}

fn preference_candidate_for_proposal(
    proposal: &LearningProposal,
    note: Option<String>,
    now: &str,
) -> LearningPreferenceCandidate {
    LearningPreferenceCandidate {
        id: format!("pref_{}", proposal.id),
        proposal_id: proposal.id.clone(),
        status: "candidate".to_string(),
        unit_id: evidence_string(proposal, "unitId"),
        canonical_id: evidence_string(proposal, "canonicalId"),
        provider_plugin: evidence_string(proposal, "providerPlugin"),
        answered_params: evidence_string_vec(proposal, "answeredParams"),
        selected_params: evidence_json_map(proposal, "selectedParams"),
        param_source_summary: evidence_usize_map(proposal, "paramSourceSummary"),
        source_record_ids: proposal.source_record_ids.clone(),
        evidence: proposal.evidence.clone(),
        note,
        created_at: now.to_string(),
        updated_at: now.to_string(),
    }
}

fn project_preference_for_candidate(
    candidate: &LearningPreferenceCandidate,
    note: Option<String>,
    now: &str,
) -> LearningProjectPreference {
    LearningProjectPreference {
        id: format!("project_{}", candidate.id),
        candidate_id: candidate.id.clone(),
        proposal_id: candidate.proposal_id.clone(),
        status: "active".to_string(),
        scope: "project".to_string(),
        unit_id: candidate.unit_id.clone(),
        canonical_id: candidate.canonical_id.clone(),
        provider_plugin: candidate.provider_plugin.clone(),
        params: candidate.selected_params.clone(),
        answered_params: candidate.answered_params.clone(),
        source_record_ids: candidate.source_record_ids.clone(),
        evidence: candidate.evidence.clone(),
        note,
        promoted_at: now.to_string(),
        updated_at: now.to_string(),
    }
}

fn archive_marker_for_proposal(
    proposal: &LearningProposal,
    note: Option<String>,
    now: &str,
) -> LearningArchiveMarker {
    LearningArchiveMarker {
        id: format!("archive_{}", proposal.id),
        proposal_id: proposal.id.clone(),
        status: "marked".to_string(),
        unit_id: evidence_string(proposal, "unitId"),
        canonical_id: evidence_string(proposal, "canonicalId"),
        provider_plugin: evidence_string(proposal, "providerPlugin"),
        run_dir: evidence_string(proposal, "runDir"),
        provenance_path: evidence_string(proposal, "provenancePath"),
        artifact_paths: evidence_string_vec(proposal, "artifactPaths"),
        source_record_ids: proposal.source_record_ids.clone(),
        evidence: proposal.evidence.clone(),
        note,
        created_at: now.to_string(),
        updated_at: now.to_string(),
    }
}

fn upsert_apply_record(project_root: &Path, mut record: LearningApplyRecord) -> Result<(), String> {
    let path = learning_applied_path(project_root);
    let mut store = load_json_or_default::<LearningApplyStore>(&path)?;
    if let Some(existing) = store
        .records
        .iter_mut()
        .find(|existing| existing.id == record.id)
    {
        record.applied_at.clone_from(&existing.applied_at);
        *existing = record;
    } else {
        store.records.push(record);
    }
    sort_apply_records(&mut store.records);
    save_json_file(&path, &store)
}

fn upsert_project_preference(
    project_root: &Path,
    mut preference: LearningProjectPreference,
) -> Result<(), String> {
    let path = learning_project_preferences_path(project_root);
    let mut store = load_json_or_default::<LearningProjectPreferenceStore>(&path)?;
    if let Some(existing) = store
        .preferences
        .iter_mut()
        .find(|existing| existing.id == preference.id)
    {
        preference.promoted_at.clone_from(&existing.promoted_at);
        *existing = preference;
    } else {
        store.preferences.push(preference);
    }
    store
        .preferences
        .sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
    save_json_file(&path, &store)
}

fn project_preference_match_reasons(
    preference: &LearningProjectPreference,
    unit_id: Option<&str>,
    canonical_id: Option<&str>,
    provider_plugin: Option<&str>,
) -> Option<Vec<String>> {
    let mut reasons = Vec::new();
    if optional_str_eq(preference.canonical_id.as_deref(), canonical_id) {
        reasons.push("canonicalId".to_string());
    }
    if optional_str_eq(preference.unit_id.as_deref(), unit_id) {
        reasons.push("unitId".to_string());
    }
    let has_specific_scope = preference.canonical_id.is_some() || preference.unit_id.is_some();
    if !has_specific_scope
        && optional_str_eq(preference.provider_plugin.as_deref(), provider_plugin)
    {
        reasons.push("providerPlugin".to_string());
    }
    if reasons.is_empty() {
        None
    } else {
        Some(reasons)
    }
}

fn project_preference_hint(
    preference: LearningProjectPreference,
    match_reasons: Vec<String>,
) -> LearningProjectPreferenceHint {
    let summary = format!(
        "建议优先考虑：{}。",
        summarize_param_pairs(&preference.params)
    );
    LearningProjectPreferenceHint {
        id: preference.id,
        status: preference.status,
        scope: preference.scope,
        unit_id: preference.unit_id,
        canonical_id: preference.canonical_id,
        provider_plugin: preference.provider_plugin,
        match_reasons,
        params: preference.params,
        answered_params: preference.answered_params,
        note: preference.note,
        updated_at: preference.updated_at,
        summary,
        safety_note: "仅作为推荐；不要静默覆盖本次用户明确给出的参数。".to_string(),
    }
}

fn summarize_param_pairs(params: &BTreeMap<String, JsonValue>) -> String {
    let pairs = params
        .iter()
        .take(6)
        .map(|(key, value)| format!("{key}={}", compact_json_value(value)))
        .collect::<Vec<_>>();
    let suffix = if params.len() > pairs.len() {
        format!("，另有 {} 项", params.len() - pairs.len())
    } else {
        String::new()
    };
    format!("{}{}", pairs.join(", "), suffix)
}

fn compact_json_value(value: &JsonValue) -> String {
    match value {
        JsonValue::String(value) => value.clone(),
        JsonValue::Bool(value) => value.to_string(),
        JsonValue::Number(value) => value.to_string(),
        JsonValue::Null => "null".to_string(),
        other => serde_json::to_string(other).unwrap_or_else(|_| "<complex>".to_string()),
    }
}

fn optional_str_eq(left: Option<&str>, right: Option<&str>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => left == right,
        _ => false,
    }
}

fn upsert_preference_candidate(
    project_root: &Path,
    mut candidate: LearningPreferenceCandidate,
) -> Result<(), String> {
    let path = learning_preference_candidates_path(project_root);
    let mut store = load_json_or_default::<LearningPreferenceCandidateStore>(&path)?;
    if let Some(existing) = store
        .candidates
        .iter_mut()
        .find(|existing| existing.id == candidate.id)
    {
        candidate.created_at.clone_from(&existing.created_at);
        *existing = candidate;
    } else {
        store.candidates.push(candidate);
    }
    sort_preference_candidates(&mut store.candidates);
    save_json_file(&path, &store)
}

fn upsert_archive_marker(
    project_root: &Path,
    mut marker: LearningArchiveMarker,
) -> Result<(), String> {
    let path = learning_archive_markers_path(project_root);
    let mut store = load_json_or_default::<LearningArchiveMarkerStore>(&path)?;
    if let Some(existing) = store
        .markers
        .iter_mut()
        .find(|existing| existing.id == marker.id)
    {
        marker.created_at.clone_from(&existing.created_at);
        *existing = marker;
    } else {
        store.markers.push(marker);
    }
    store
        .markers
        .sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
    save_json_file(&path, &store)
}

fn load_json_or_default<T>(path: &Path) -> Result<T, String>
where
    T: Default + DeserializeOwned,
{
    if !path.is_file() {
        return Ok(T::default());
    }
    let raw = fs::read_to_string(path)
        .map_err(|err| format!("read JSON store `{}`: {err}", path.display()))?;
    serde_json::from_str::<T>(&raw)
        .map_err(|err| format!("parse JSON store `{}`: {err}", path.display()))
}

fn save_json_file<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("create JSON store dir `{}`: {err}", parent.display()))?;
    }
    let raw = serde_json::to_string_pretty(value)
        .map_err(|err| format!("serialize JSON store `{}`: {err}", path.display()))?;
    fs::write(path, raw).map_err(|err| format!("write JSON store `{}`: {err}", path.display()))
}

fn evidence_string(proposal: &LearningProposal, key: &str) -> Option<String> {
    proposal
        .evidence
        .get(key)
        .and_then(JsonValue::as_str)
        .map(ToOwned::to_owned)
}

fn evidence_string_vec(proposal: &LearningProposal, key: &str) -> Vec<String> {
    proposal
        .evidence
        .get(key)
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

fn evidence_usize_map(proposal: &LearningProposal, key: &str) -> BTreeMap<String, usize> {
    let mut out = BTreeMap::new();
    if let Some(map) = proposal.evidence.get(key).and_then(JsonValue::as_object) {
        for (key, value) in map {
            if let Some(count) = value.as_u64().and_then(|count| usize::try_from(count).ok()) {
                out.insert(key.clone(), count);
            }
        }
    }
    out
}

fn evidence_json_map(proposal: &LearningProposal, key: &str) -> BTreeMap<String, JsonValue> {
    proposal
        .evidence
        .get(key)
        .and_then(JsonValue::as_object)
        .map(|map| {
            map.iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect()
        })
        .unwrap_or_default()
}

fn apply_notification(
    proposal: &LearningProposal,
    previous_status: LearningProposalStatus,
) -> String {
    if previous_status == LearningProposalStatus::Applied {
        return format!("学习建议已处于固化状态：{}。", proposal.title);
    }
    match proposal.kind {
        LearningProposalKind::ReusableChoice => {
            format!(
                "已固化学习建议：{}。已写入项目偏好候选，后续任务可由学习 agent 决定是否提升为模板默认值。",
                proposal.title
            )
        }
        LearningProposalKind::ArchiveResult => {
            format!(
                "已固化学习建议：{}。已写入结果封存标记，后续任务可按 marker 引用或执行真实归档。",
                proposal.title
            )
        }
    }
}

async fn refresh_store_from_execution_records(
    project_root: &Path,
    limit: usize,
    store: &mut LearningProposalStore,
) -> Result<usize, String> {
    let records =
        crate::domain::execution_records::list_recent_execution_records(project_root, limit)
            .await?;
    let mut existing = store
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
        existing.insert(candidate.id.clone(), candidate.status.clone());
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

fn summarize_preference_candidates(
    candidates: &[LearningPreferenceCandidate],
) -> LearningPreferenceCandidateSummary {
    let mut summary = LearningPreferenceCandidateSummary {
        total_count: candidates.len(),
        ..LearningPreferenceCandidateSummary::default()
    };
    for candidate in candidates {
        if candidate.status == "promoted" {
            summary.promoted_count += 1;
        } else {
            summary.candidate_count += 1;
        }
        if candidate.selected_params.is_empty() {
            summary.missing_selected_params_count += 1;
        }
    }
    summary
}

fn generate_learning_proposals_from_records(
    records: &[ExecutionRecord],
    now: &str,
) -> Vec<LearningProposal> {
    let mut proposals = Vec::new();
    let mut reusable_choice_keys = HashSet::new();
    for record in records.iter().filter(|record| record.status == "succeeded") {
        let metadata = parse_json(record.metadata_json.as_deref());
        let runtime = parse_json(record.runtime_json.as_deref());
        let output_summary = parse_json(record.output_summary_json.as_deref());
        let metadata_ref = metadata.as_ref();
        let output_ref = output_summary.as_ref();
        let param_sources = param_source_summary(metadata_ref);
        let answered_params = preflight_answered_params(metadata_ref);
        let selected_params = selected_param_values(metadata_ref, &answered_params);
        let has_user_preflight = param_sources.get("user_preflight").copied().unwrap_or(0) > 0
            || !answered_params.is_empty();
        let artifact_paths = artifact_paths(record, metadata_ref, output_ref);
        let run_dir = string_at(metadata_ref, &["/runDir", "/run_dir"])
            .or_else(|| string_at(runtime.as_ref(), &["/runDir", "/run_dir"]));
        let provenance_path = string_at(metadata_ref, &["/provenancePath", "/provenance_path"]);
        let unit_label = unit_label(record);

        if has_user_preflight
            && reusable_choice_keys.insert(reusable_choice_signature(record, &answered_params))
        {
            proposals.push(reusable_choice_proposal(
                record,
                &unit_label,
                &param_sources,
                &answered_params,
                &selected_params,
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

fn reusable_choice_signature(record: &ExecutionRecord, answered_params: &[String]) -> String {
    let mut sorted_params = answered_params.to_vec();
    sorted_params.sort();
    format!(
        "{}\0{}\0{}\0{}",
        record
            .canonical_id
            .as_deref()
            .or(record.unit_id.as_deref())
            .unwrap_or(record.kind.as_str()),
        record.provider_plugin.as_deref().unwrap_or_default(),
        record.param_hash.as_deref().unwrap_or(record.id.as_str()),
        sorted_params.join(",")
    )
}

#[allow(clippy::too_many_arguments)]
fn reusable_choice_proposal(
    record: &ExecutionRecord,
    unit_label: &str,
    param_sources: &BTreeMap<String, usize>,
    answered_params: &[String],
    selected_params: &BTreeMap<String, JsonValue>,
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
            "selectedParams": selected_params,
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

    if kind == "reusable_choice" {
        let unit_key = record
            .canonical_id
            .as_deref()
            .or(record.unit_id.as_deref())
            .or(record.provider_plugin.as_deref())
            .unwrap_or(&record.kind);
        hasher.update(unit_key.as_bytes());
        hasher.update(b"\0");
        if let Some(param_hash) = record.param_hash.as_deref() {
            hasher.update(param_hash.as_bytes());
        } else {
            // Without a parameter signature we cannot safely deduplicate across
            // runs; fall back to the specific record to avoid collapsing
            // unrelated user choices.
            hasher.update(record.id.as_bytes());
        }
    } else {
        // Archive proposals are intentionally record-specific: each successful
        // run may produce a distinct result folder/provenance bundle.
        hasher.update(record.id.as_bytes());
        hasher.update(b"\0");
        if let Some(param_hash) = record.param_hash.as_deref() {
            hasher.update(param_hash.as_bytes());
        }
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
    let answered = answered_params.iter().collect::<HashSet<_>>();
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

fn sort_proposals(proposals: &mut [LearningProposal]) {
    proposals.sort_by(|left, right| {
        status_rank(&left.status)
            .cmp(&status_rank(&right.status))
            .then_with(|| right.updated_at.cmp(&left.updated_at))
            .then_with(|| left.id.cmp(&right.id))
    });
}

fn sort_apply_records(records: &mut [LearningApplyRecord]) {
    records.sort_by(|left, right| {
        right
            .updated_at
            .cmp(&left.updated_at)
            .then_with(|| left.id.cmp(&right.id))
    });
}

fn sort_preference_candidates(candidates: &mut [LearningPreferenceCandidate]) {
    candidates.sort_by(|left, right| {
        left.status
            .cmp(&right.status)
            .then_with(|| right.updated_at.cmp(&left.updated_at))
            .then_with(|| left.id.cmp(&right.id))
    });
}

fn sort_project_preference_hints(hints: &mut [LearningProjectPreferenceHint]) {
    hints.sort_by(|left, right| {
        project_preference_match_rank(&left.match_reasons)
            .cmp(&project_preference_match_rank(&right.match_reasons))
            .then_with(|| right.updated_at.cmp(&left.updated_at))
            .then_with(|| left.id.cmp(&right.id))
    });
}

fn project_preference_match_rank(reasons: &[String]) -> u8 {
    if reasons.iter().any(|reason| reason == "canonicalId") {
        0
    } else if reasons.iter().any(|reason| reason == "unitId") {
        1
    } else {
        2
    }
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
    async fn refresh_deduplicates_reusable_choices_by_unit_and_param_signature() {
        let tmp = tempfile::tempdir().unwrap();
        for (record_id, run_dir) in [
            ("execrec_first", ".omiga/runs/plot_1"),
            ("execrec_second", ".omiga/runs/plot_2"),
        ] {
            crate::domain::execution_records::record_execution_with_id(
                tmp.path(),
                record_id.to_string(),
                ExecutionRecordInput {
                    kind: "template".to_string(),
                    unit_id: Some("visualization-r/scatter".to_string()),
                    canonical_id: Some("plugin/template/visualization-r/scatter".to_string()),
                    provider_plugin: Some("visualization-r".to_string()),
                    status: "succeeded".to_string(),
                    session_id: Some("session-plot".to_string()),
                    parent_execution_id: None,
                    started_at: Some("2026-05-09T00:00:00Z".to_string()),
                    ended_at: Some("2026-05-09T00:00:01Z".to_string()),
                    input_hash: Some("sha256:input".to_string()),
                    param_hash: Some("sha256:shared-style".to_string()),
                    output_summary_json: Some(json!({
                        "outputs": [{"path": format!("{run_dir}/figure.png")}]
                    })),
                    runtime_json: None,
                    metadata_json: Some(json!({
                        "runDir": run_dir,
                        "paramSources": {
                            "palette": "user_preflight",
                            "point_size": "user_preflight"
                        },
                        "preflight": {
                            "answeredParams": [
                                {"param": "palette"},
                                {"param": "point_size"}
                            ]
                        }
                    })),
                },
            )
            .await
            .unwrap();
        }

        let listed = refresh_and_list_learning_proposals(tmp.path(), 50, false)
            .await
            .unwrap();
        let reusable_choices = listed
            .proposals
            .iter()
            .filter(|proposal| proposal.kind == LearningProposalKind::ReusableChoice)
            .count();
        let archive_results = listed
            .proposals
            .iter()
            .filter(|proposal| proposal.kind == LearningProposalKind::ArchiveResult)
            .count();

        assert_eq!(reusable_choices, 1);
        assert_eq!(archive_results, 2);
        assert_eq!(listed.summary.generated_count, 3);
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

    #[tokio::test]
    async fn apply_writes_preference_candidates_and_archive_markers_idempotently() {
        let tmp = tempfile::tempdir().unwrap();
        record_execution(
            tmp.path(),
            ExecutionRecordInput {
                kind: "operator".to_string(),
                unit_id: Some("demo".to_string()),
                canonical_id: Some("plugin/operator/demo".to_string()),
                provider_plugin: Some("omics".to_string()),
                status: "succeeded".to_string(),
                session_id: None,
                parent_execution_id: None,
                started_at: Some("2026-05-10T00:00:00Z".to_string()),
                ended_at: Some("2026-05-10T00:00:02Z".to_string()),
                input_hash: None,
                param_hash: Some("sha256:param".to_string()),
                output_summary_json: Some(json!({
                    "outputs": {
                        "report": {"path": "report.html"}
                    }
                })),
                runtime_json: None,
                metadata_json: Some(json!({
                    "runDir": ".omiga/runs/oprun_2",
                    "provenancePath": ".omiga/runs/oprun_2/provenance.json",
                    "paramSources": {"method": "user_preflight"},
                    "preflight": {"answeredParams": [{"param": "method"}]},
                    "selectedParams": {"method": "deseq2"}
                })),
            },
        )
        .await
        .unwrap();

        let listed = refresh_and_list_learning_proposals(tmp.path(), 50, false)
            .await
            .unwrap();
        let reusable_id = listed
            .proposals
            .iter()
            .find(|proposal| proposal.kind == LearningProposalKind::ReusableChoice)
            .unwrap()
            .id
            .clone();
        let archive_id = listed
            .proposals
            .iter()
            .find(|proposal| proposal.kind == LearningProposalKind::ArchiveResult)
            .unwrap()
            .id
            .clone();

        let unapproved = apply_learning_proposal(tmp.path(), &reusable_id, false, None);
        assert!(unapproved
            .unwrap_err()
            .contains("must be approved before apply"));

        decide_learning_proposal(
            tmp.path(),
            &reusable_id,
            LearningProposalDecision::Approve,
            Some("confirm reuse".to_string()),
        )
        .unwrap();
        let applied = apply_learning_proposal(
            tmp.path(),
            &reusable_id,
            false,
            Some("solidify reusable choice".to_string()),
        )
        .unwrap();
        assert_eq!(applied.proposal.status, LearningProposalStatus::Applied);
        assert_eq!(
            applied.apply_record.targets[0].target_type,
            "preference_candidate"
        );
        assert!(applied.notification.contains("已固化学习建议"));

        let preferences = load_json_or_default::<LearningPreferenceCandidateStore>(
            &learning_preference_candidates_path(tmp.path()),
        )
        .unwrap();
        assert_eq!(preferences.candidates.len(), 1);
        assert_eq!(preferences.candidates[0].answered_params, vec!["method"]);
        assert_eq!(
            preferences.candidates[0].selected_params["method"],
            "deseq2"
        );

        apply_learning_proposal(tmp.path(), &reusable_id, false, None).unwrap();
        let apply_store =
            load_json_or_default::<LearningApplyStore>(&learning_applied_path(tmp.path())).unwrap();
        assert_eq!(apply_store.records.len(), 1);

        decide_learning_proposal(
            tmp.path(),
            &archive_id,
            LearningProposalDecision::Approve,
            None,
        )
        .unwrap();
        let archive_applied =
            apply_learning_proposal(tmp.path(), &archive_id, false, None).unwrap();
        assert_eq!(
            archive_applied.apply_record.targets[0].target_type,
            "archive_marker"
        );
        let archive_markers = load_json_or_default::<LearningArchiveMarkerStore>(
            &learning_archive_markers_path(tmp.path()),
        )
        .unwrap();
        assert_eq!(archive_markers.markers.len(), 1);
        assert_eq!(
            archive_markers.markers[0].provenance_path.as_deref(),
            Some(".omiga/runs/oprun_2/provenance.json")
        );
    }

    #[tokio::test]
    async fn promotes_preference_candidate_to_project_preference_store() {
        let tmp = tempfile::tempdir().unwrap();
        record_execution(
            tmp.path(),
            ExecutionRecordInput {
                kind: "operator".to_string(),
                unit_id: Some("demo".to_string()),
                canonical_id: Some("plugin/operator/demo".to_string()),
                provider_plugin: Some("omics".to_string()),
                status: "succeeded".to_string(),
                session_id: None,
                parent_execution_id: None,
                started_at: Some("2026-05-10T00:00:00Z".to_string()),
                ended_at: Some("2026-05-10T00:00:02Z".to_string()),
                input_hash: None,
                param_hash: Some("sha256:param".to_string()),
                output_summary_json: None,
                runtime_json: None,
                metadata_json: Some(json!({
                    "paramSources": {"method": "user_preflight", "alpha": "user_preflight"},
                    "preflight": {
                        "answeredParams": [{"param": "method"}, {"param": "alpha"}]
                    },
                    "selectedParams": {"method": "deseq2", "alpha": 0.05}
                })),
            },
        )
        .await
        .unwrap();

        let listed = refresh_and_list_learning_proposals(tmp.path(), 50, false)
            .await
            .unwrap();
        let reusable_id = listed.proposals[0].id.clone();
        decide_learning_proposal(
            tmp.path(),
            &reusable_id,
            LearningProposalDecision::Approve,
            None,
        )
        .unwrap();
        apply_learning_proposal(tmp.path(), &reusable_id, false, None).unwrap();

        let candidates = list_learning_preference_candidates(tmp.path(), false).unwrap();
        assert_eq!(candidates.summary.candidate_count, 1);
        assert_eq!(candidates.summary.missing_selected_params_count, 0);
        let candidate_id = candidates.candidates[0].id.clone();

        let promoted = promote_learning_preference_candidate(
            tmp.path(),
            &candidate_id,
            Some("make project default".to_string()),
        )
        .unwrap();
        assert_eq!(promoted.candidate.status, "promoted");
        assert_eq!(promoted.preference.status, "active");
        assert_eq!(promoted.preference.params["method"], "deseq2");
        assert_eq!(promoted.preference.params["alpha"], 0.05);

        let active = load_json_or_default::<LearningProjectPreferenceStore>(
            &learning_project_preferences_path(tmp.path()),
        )
        .unwrap();
        assert_eq!(active.preferences.len(), 1);
        assert_eq!(active.preferences[0].candidate_id, candidate_id);

        let remaining = list_learning_preference_candidates(tmp.path(), false).unwrap();
        assert_eq!(remaining.summary.total_count, 0);
        let all = list_learning_preference_candidates(tmp.path(), true).unwrap();
        assert_eq!(all.summary.promoted_count, 1);
    }
}
