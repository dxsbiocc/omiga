//! Tauri commands for lightweight user-facing learning proposal confirmation.

use crate::commands::CommandResult;
use crate::domain::learning_proposals::{
    self, LearningPreferenceCandidate, LearningProposal, LearningProposalDecision,
    LearningProposalKind,
};
use crate::errors::AppError;
use serde::Serialize;
use serde_json::Value as JsonValue;
use std::path::Path;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LearningProposalPromptAction {
    pub id: String,
    pub label: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LearningProposalPromptDetail {
    pub label: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LearningProposalPrompt {
    pub proposal_id: String,
    pub kind: String,
    pub title: String,
    pub message: String,
    pub details: Vec<LearningProposalPromptDetail>,
    pub actions: Vec<LearningProposalPromptAction>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LearningProposalActionResult {
    pub proposal_id: String,
    pub status: String,
    pub notification: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LearningPreferenceCandidateSummaryView {
    pub total_count: usize,
    pub candidate_count: usize,
    pub promoted_count: usize,
    pub missing_selected_params_count: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LearningPreferenceCandidateView {
    pub candidate_id: String,
    pub status: String,
    pub title: String,
    pub message: String,
    pub can_promote: bool,
    pub details: Vec<LearningProposalPromptDetail>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LearningPreferenceCandidateOverview {
    pub store_path: String,
    pub project_preferences_path: String,
    pub summary: LearningPreferenceCandidateSummaryView,
    pub candidates: Vec<LearningPreferenceCandidateView>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LearningPreferencePromotionActionResult {
    pub candidate_id: String,
    pub status: String,
    pub notification: String,
}

fn learning_error(error: String) -> AppError {
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

fn kind_label(kind: &LearningProposalKind) -> String {
    match kind {
        LearningProposalKind::ReusableChoice => "reusable_choice",
        LearningProposalKind::ArchiveResult => "archive_result",
    }
    .to_string()
}

fn evidence_string(proposal: &LearningProposal, key: &str) -> Option<String> {
    proposal
        .evidence
        .get(key)
        .and_then(JsonValue::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(ToOwned::to_owned)
}

fn evidence_string_list(proposal: &LearningProposal, key: &str) -> Vec<String> {
    proposal
        .evidence
        .get(key)
        .and_then(JsonValue::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(JsonValue::as_str)
                .filter(|value| !value.trim().is_empty())
                .map(ToOwned::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn push_detail(details: &mut Vec<LearningProposalPromptDetail>, label: &str, value: String) {
    if value.trim().is_empty() {
        return;
    }
    details.push(LearningProposalPromptDetail {
        label: label.to_string(),
        value,
    });
}

fn proposal_details(root: &Path, proposal: &LearningProposal) -> Vec<LearningProposalPromptDetail> {
    let mut details = Vec::new();
    push_detail(
        &mut details,
        "建议类型",
        match proposal.kind {
            LearningProposalKind::ReusableChoice => "项目偏好候选".to_string(),
            LearningProposalKind::ArchiveResult => "结果封存候选".to_string(),
        },
    );
    if let Some(canonical_id) = evidence_string(proposal, "canonicalId") {
        push_detail(&mut details, "来源单元", canonical_id);
    } else if let Some(unit_id) = evidence_string(proposal, "unitId") {
        push_detail(&mut details, "来源单元", unit_id);
    }
    if let Some(plugin) = evidence_string(proposal, "providerPlugin") {
        push_detail(&mut details, "插件", plugin);
    }
    let answered_params = evidence_string_list(proposal, "answeredParams");
    if !answered_params.is_empty() {
        push_detail(&mut details, "用户确认参数", answered_params.join(", "));
    }
    let artifact_paths = evidence_string_list(proposal, "artifactPaths");
    if !artifact_paths.is_empty() {
        push_detail(
            &mut details,
            "相关产物",
            format!("{} 个产物/路径", artifact_paths.len()),
        );
    }
    if let Some(run_dir) = evidence_string(proposal, "runDir") {
        push_detail(&mut details, "运行目录", run_dir);
    }
    let save_path = match proposal.kind {
        LearningProposalKind::ReusableChoice => {
            learning_proposals::learning_preference_candidates_path(root)
        }
        LearningProposalKind::ArchiveResult => {
            learning_proposals::learning_archive_markers_path(root)
        }
    };
    push_detail(
        &mut details,
        "保存位置",
        save_path.to_string_lossy().into_owned(),
    );
    push_detail(
        &mut details,
        "安全边界",
        "只写项目学习记录；不会静默修改 operator、template 或移动产物文件。".to_string(),
    );
    details
}

fn prompt_from_proposal(root: &Path, proposal: LearningProposal) -> LearningProposalPrompt {
    let details = proposal_details(root, &proposal);
    LearningProposalPrompt {
        proposal_id: proposal.id,
        kind: kind_label(&proposal.kind),
        title: proposal.title,
        message: proposal.user_message,
        details,
        actions: vec![
            LearningProposalPromptAction {
                id: "approve_apply".to_string(),
                label: "保存".to_string(),
                description: "确认并保存为项目学习记录。".to_string(),
            },
            LearningProposalPromptAction {
                id: "snooze".to_string(),
                label: "稍后".to_string(),
                description: "暂时不处理，避免打断当前工作。".to_string(),
            },
            LearningProposalPromptAction {
                id: "dismiss".to_string(),
                label: "忽略".to_string(),
                description: "不保存这条学习建议。".to_string(),
            },
        ],
    }
}

fn status_label(status: &str) -> String {
    match status {
        "candidate" => "候选".to_string(),
        "promoted" => "已提升为项目偏好".to_string(),
        other => other.to_string(),
    }
}

fn candidate_title(candidate: &LearningPreferenceCandidate) -> String {
    let unit = candidate
        .canonical_id
        .as_deref()
        .or(candidate.unit_id.as_deref())
        .unwrap_or("未知单元");
    format!("{}：{}", status_label(&candidate.status), unit)
}

fn preference_candidate_details(
    candidate: &LearningPreferenceCandidate,
) -> Vec<LearningProposalPromptDetail> {
    let mut details = Vec::new();
    push_detail(&mut details, "状态", status_label(&candidate.status));
    if let Some(canonical_id) = candidate.canonical_id.clone() {
        push_detail(&mut details, "来源单元", canonical_id);
    } else if let Some(unit_id) = candidate.unit_id.clone() {
        push_detail(&mut details, "来源单元", unit_id);
    }
    if let Some(plugin) = candidate.provider_plugin.clone() {
        push_detail(&mut details, "插件", plugin);
    }
    if !candidate.answered_params.is_empty() {
        push_detail(
            &mut details,
            "用户确认参数",
            candidate.answered_params.join(", "),
        );
    }
    push_detail(
        &mut details,
        "可复用参数",
        if candidate.selected_params.is_empty() {
            "暂无可直接提升的参数；需要 agent 进一步整理。".to_string()
        } else {
            format!("{} 个参数", candidate.selected_params.len())
        },
    );
    push_detail(
        &mut details,
        "安全边界",
        "这是项目偏好候选；不会覆盖默认模板，除非用户后续明确提升。".to_string(),
    );
    details
}

fn preference_candidate_view(
    candidate: LearningPreferenceCandidate,
) -> LearningPreferenceCandidateView {
    let selected_count = candidate.selected_params.len();
    LearningPreferenceCandidateView {
        candidate_id: candidate.id.clone(),
        status: candidate.status.clone(),
        title: candidate_title(&candidate),
        message: if selected_count == 0 {
            "已保存为学习记录，但缺少可直接提升的参数，需要 agent 进一步整理。".to_string()
        } else {
            format!(
                "包含 {} 个可复用参数，可在确认后提升为项目偏好。",
                selected_count
            )
        },
        can_promote: candidate.status != "promoted" && selected_count > 0,
        details: preference_candidate_details(&candidate),
    }
}

fn normalize_prompt_action(action: &str) -> Option<&'static str> {
    match action.trim().to_ascii_lowercase().as_str() {
        "approve_apply" | "approve" | "save" | "保存" => Some("approve_apply"),
        "snooze" | "later" | "稍后" | "稍后提醒" => Some("snooze"),
        "dismiss" | "ignore" | "忽略" => Some("dismiss"),
        _ => None,
    }
}

#[tauri::command]
pub async fn learning_preference_candidates(
    project_root: Option<String>,
    include_promoted: Option<bool>,
) -> CommandResult<LearningPreferenceCandidateOverview> {
    let root = resolve_project_root(project_root);
    let list = learning_proposals::list_learning_preference_candidates(
        &root,
        include_promoted.unwrap_or(true),
    )
    .map_err(learning_error)?;
    Ok(LearningPreferenceCandidateOverview {
        store_path: list.store_path,
        project_preferences_path: list.project_preferences_path,
        summary: LearningPreferenceCandidateSummaryView {
            total_count: list.summary.total_count,
            candidate_count: list.summary.candidate_count,
            promoted_count: list.summary.promoted_count,
            missing_selected_params_count: list.summary.missing_selected_params_count,
        },
        candidates: list
            .candidates
            .into_iter()
            .map(preference_candidate_view)
            .collect(),
    })
}

#[tauri::command]
pub async fn learning_preference_candidate_promote(
    project_root: Option<String>,
    candidate_id: String,
) -> CommandResult<LearningPreferencePromotionActionResult> {
    let root = resolve_project_root(project_root);
    let candidate_id = candidate_id.trim();
    if candidate_id.is_empty() {
        return Err(AppError::Config(
            "candidate_id must not be empty".to_string(),
        ));
    }
    let promoted = learning_proposals::promote_learning_preference_candidate(
        &root,
        candidate_id,
        Some("promoted from saved learning records dialog".to_string()),
    )
    .map_err(learning_error)?;
    Ok(LearningPreferencePromotionActionResult {
        candidate_id: promoted.candidate.id,
        status: promoted.preference.status,
        notification: promoted.notification,
    })
}

#[tauri::command]
pub async fn learning_proposal_next(
    project_root: Option<String>,
    refresh: Option<bool>,
) -> CommandResult<Option<LearningProposalPrompt>> {
    let root = resolve_project_root(project_root);
    let list = if refresh.unwrap_or(false) {
        learning_proposals::refresh_and_list_learning_proposals(&root, 100, false).await
    } else {
        learning_proposals::list_learning_proposals(&root, false)
    }
    .map_err(learning_error)?;
    Ok(list
        .proposals
        .into_iter()
        .next()
        .map(|proposal| prompt_from_proposal(&root, proposal)))
}

#[tauri::command]
pub async fn learning_proposal_respond(
    project_root: Option<String>,
    proposal_id: String,
    action: String,
) -> CommandResult<LearningProposalActionResult> {
    let root = resolve_project_root(project_root);
    let proposal_id = proposal_id.trim();
    if proposal_id.is_empty() {
        return Err(AppError::Config(
            "proposal_id must not be empty".to_string(),
        ));
    }
    match normalize_prompt_action(&action) {
        Some("approve_apply") => {
            learning_proposals::decide_learning_proposal(
                &root,
                proposal_id,
                LearningProposalDecision::Approve,
                Some("approved from lightweight confirmation prompt".to_string()),
            )
            .map_err(learning_error)?;
            let applied = learning_proposals::apply_learning_proposal(
                &root,
                proposal_id,
                false,
                Some("applied from lightweight confirmation prompt".to_string()),
            )
            .map_err(learning_error)?;
            Ok(LearningProposalActionResult {
                proposal_id: proposal_id.to_string(),
                status: "applied".to_string(),
                notification: applied.notification,
            })
        }
        Some("snooze") => {
            let decided = learning_proposals::decide_learning_proposal(
                &root,
                proposal_id,
                LearningProposalDecision::Snooze,
                Some("snoozed from lightweight confirmation prompt".to_string()),
            )
            .map_err(learning_error)?;
            Ok(LearningProposalActionResult {
                proposal_id: proposal_id.to_string(),
                status: "snoozed".to_string(),
                notification: decided.notification,
            })
        }
        Some("dismiss") => {
            let decided = learning_proposals::decide_learning_proposal(
                &root,
                proposal_id,
                LearningProposalDecision::Dismiss,
                Some("dismissed from lightweight confirmation prompt".to_string()),
            )
            .map_err(learning_error)?;
            Ok(LearningProposalActionResult {
                proposal_id: proposal_id.to_string(),
                status: "dismissed".to_string(),
                notification: decided.notification,
            })
        }
        _ => Err(AppError::Config(
            "action must be approve_apply, snooze, or dismiss".to_string(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::execution_records::{record_execution, ExecutionRecordInput};
    use serde_json::json;

    #[test]
    fn normalizes_prompt_actions() {
        assert_eq!(normalize_prompt_action("保存"), Some("approve_apply"));
        assert_eq!(normalize_prompt_action("later"), Some("snooze"));
        assert_eq!(normalize_prompt_action("ignore"), Some("dismiss"));
        assert_eq!(normalize_prompt_action("unknown"), None);
    }

    #[tokio::test]
    async fn next_and_respond_apply_first_pending_prompt() {
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
                ended_at: Some("2026-05-10T00:00:01Z".to_string()),
                input_hash: None,
                param_hash: Some("sha256:param".to_string()),
                output_summary_json: None,
                runtime_json: None,
                metadata_json: Some(json!({
                    "paramSources": {"method": "user_preflight"},
                    "preflight": {"answeredParams": [{"param": "method"}]},
                    "selectedParams": {"method": "deseq2"}
                })),
            },
        )
        .await
        .unwrap();

        let prompt =
            learning_proposal_next(Some(tmp.path().to_string_lossy().into_owned()), Some(true))
                .await
                .unwrap()
                .expect("pending learning prompt");
        assert_eq!(prompt.actions[0].id, "approve_apply");
        assert!(prompt
            .details
            .iter()
            .any(|detail| detail.label == "保存位置"));
        assert!(prompt
            .details
            .iter()
            .any(|detail| detail.label == "安全边界"));

        let result = learning_proposal_respond(
            Some(tmp.path().to_string_lossy().into_owned()),
            prompt.proposal_id,
            "approve_apply".to_string(),
        )
        .await
        .unwrap();
        assert_eq!(result.status, "applied");
        assert!(result.notification.contains("已固化学习建议"));

        let saved = learning_preference_candidates(
            Some(tmp.path().to_string_lossy().into_owned()),
            Some(true),
        )
        .await
        .unwrap();
        assert_eq!(saved.summary.total_count, 1);
        assert!(saved.candidates[0].title.contains("候选"));
        assert!(saved.candidates[0]
            .details
            .iter()
            .any(|detail| detail.label == "安全边界"));
        assert!(saved.candidates[0].can_promote);

        let promoted = learning_preference_candidate_promote(
            Some(tmp.path().to_string_lossy().into_owned()),
            saved.candidates[0].candidate_id.clone(),
        )
        .await
        .unwrap();
        assert_eq!(promoted.status, "active");
        assert!(promoted.notification.contains("项目偏好"));

        let next =
            learning_proposal_next(Some(tmp.path().to_string_lossy().into_owned()), Some(false))
                .await
                .unwrap();
        assert!(next.is_none());
    }
}
