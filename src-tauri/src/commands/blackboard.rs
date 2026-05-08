//! Tauri commands for the shared Team blackboard.
//! The blackboard lets parallel workers post structured results that the
//! Architect and the frontend can query in real-time.

use crate::commands::CommandResult;
use crate::domain::blackboard::{self, BlackboardEntry};
use chrono::Utc;
use serde::{Deserialize, Serialize};

fn validate_project_root(project_root: &str) -> Result<std::path::PathBuf, String> {
    let p = std::path::PathBuf::from(project_root);
    let canonical = p
        .canonicalize()
        .map_err(|_| format!("project_root not accessible: {project_root}"))?;
    let home = dirs::home_dir().ok_or_else(|| "cannot determine home directory".to_string())?;
    if !canonical.starts_with(&home) {
        return Err(format!(
            "project_root must be under home directory: {}",
            project_root
        ));
    }
    Ok(canonical)
}

// ─── DTOs ────────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct BlackboardEntryDto {
    pub subtask_id: String,
    pub agent_type: String,
    pub key: String,
    pub value: String,
    pub posted_at: String,
}

impl From<BlackboardEntry> for BlackboardEntryDto {
    fn from(e: BlackboardEntry) -> Self {
        Self {
            subtask_id: e.subtask_id,
            agent_type: e.agent_type,
            key: e.key,
            value: e.value,
            posted_at: e.posted_at.to_rfc3339(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct BlackboardDto {
    pub session_id: String,
    pub entries: Vec<BlackboardEntryDto>,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
pub struct PostEntryRequest {
    pub subtask_id: String,
    pub agent_type: String,
    pub key: String,
    pub value: String,
}

// ─── Commands ─────────────────────────────────────────────────────────────────

/// Read the full blackboard for a session.
#[tauri::command]
pub async fn get_blackboard(
    project_root: String,
    session_id: String,
) -> CommandResult<Option<BlackboardDto>> {
    let root = validate_project_root(&project_root).map_err(|e| anyhow::anyhow!(e))?;
    let Some(board) = blackboard::read_board(&root, &session_id).await else {
        return Ok(None);
    };
    Ok(Some(BlackboardDto {
        session_id: board.session_id,
        entries: board
            .entries
            .into_iter()
            .map(BlackboardEntryDto::from)
            .collect(),
        updated_at: board.updated_at.to_rfc3339(),
    }))
}

/// Post a new entry to the blackboard (called by agents or skills).
#[tauri::command]
pub async fn post_blackboard_entry(
    project_root: String,
    session_id: String,
    entry: PostEntryRequest,
) -> CommandResult<()> {
    let root = validate_project_root(&project_root).map_err(|e| anyhow::anyhow!(e))?;
    blackboard::post_entry(
        &root,
        &session_id,
        BlackboardEntry {
            subtask_id: entry.subtask_id,
            agent_type: entry.agent_type,
            key: entry.key,
            value: entry.value,
            posted_at: Utc::now(),
        },
    )
    .await
    .map_err(|e| anyhow::anyhow!(e))?;
    Ok(())
}

/// Clear the blackboard for a completed or cancelled session.
#[tauri::command]
pub async fn clear_blackboard(project_root: String, session_id: String) -> CommandResult<bool> {
    let root = validate_project_root(&project_root).map_err(|e| anyhow::anyhow!(e))?;
    Ok(blackboard::clear_board(&root, &session_id).await)
}
