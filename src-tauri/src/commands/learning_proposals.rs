//! Tauri commands for lightweight user-facing learning proposal confirmation.

use crate::commands::CommandResult;
use crate::domain::learning_proposals::{
    self, LearningProposal, LearningProposalDecision, LearningProposalKind,
};
use crate::errors::AppError;
use serde::Serialize;
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
pub struct LearningProposalPrompt {
    pub proposal_id: String,
    pub kind: String,
    pub title: String,
    pub message: String,
    pub actions: Vec<LearningProposalPromptAction>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LearningProposalActionResult {
    pub proposal_id: String,
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

fn prompt_from_proposal(proposal: LearningProposal) -> LearningProposalPrompt {
    LearningProposalPrompt {
        proposal_id: proposal.id,
        kind: kind_label(&proposal.kind),
        title: proposal.title,
        message: proposal.user_message,
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

fn normalize_prompt_action(action: &str) -> Option<&'static str> {
    match action.trim().to_ascii_lowercase().as_str() {
        "approve_apply" | "approve" | "save" | "保存" => Some("approve_apply"),
        "snooze" | "later" | "稍后" | "稍后提醒" => Some("snooze"),
        "dismiss" | "ignore" | "忽略" => Some("dismiss"),
        _ => None,
    }
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
    Ok(list.proposals.into_iter().next().map(prompt_from_proposal))
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
                    "preflight": {"answeredParams": [{"param": "method"}]}
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

        let result = learning_proposal_respond(
            Some(tmp.path().to_string_lossy().into_owned()),
            prompt.proposal_id,
            "approve_apply".to_string(),
        )
        .await
        .unwrap();
        assert_eq!(result.status, "applied");
        assert!(result.notification.contains("已固化学习建议"));

        let next =
            learning_proposal_next(Some(tmp.path().to_string_lossy().into_owned()), Some(false))
                .await
                .unwrap();
        assert!(next.is_none());
    }
}
