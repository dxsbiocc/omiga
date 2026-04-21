//! Tauri commands for Ralph session state management.
//! Used by the frontend to display active/stale ralph sessions and by the cancel skill.

use crate::commands::CommandResult;
use crate::domain::{autopilot_state, ralph_state};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct RalphSessionInfo {
    pub session_id: String,
    pub goal: String,
    pub phase: String,
    pub iteration: u32,
    pub consecutive_errors: u32,
    pub todos_completed: Vec<String>,
    pub todos_pending: Vec<String>,
    pub started_at: String,
    pub updated_at: String,
}

impl From<ralph_state::RalphState> for RalphSessionInfo {
    fn from(s: ralph_state::RalphState) -> Self {
        Self {
            session_id: s.session_id,
            goal: s.goal,
            phase: s.phase.to_string(),
            iteration: s.iteration,
            consecutive_errors: s.consecutive_errors,
            todos_completed: s.todos_completed,
            todos_pending: s.todos_pending,
            started_at: s.started_at.to_rfc3339(),
            updated_at: s.updated_at.to_rfc3339(),
        }
    }
}

/// List all active (non-complete) Ralph sessions for a project root.
#[tauri::command]
pub async fn list_ralph_sessions(project_root: String) -> CommandResult<Vec<RalphSessionInfo>> {
    let root = std::path::Path::new(&project_root);
    let states = ralph_state::list_states(root).await;
    let infos = states
        .into_iter()
        .filter(|s| !matches!(s.phase, ralph_state::RalphPhase::Complete))
        .map(RalphSessionInfo::from)
        .collect();
    Ok(infos)
}

/// Clear the state file for a specific Ralph session.
#[tauri::command]
pub async fn clear_ralph_session(project_root: String, session_id: String) -> CommandResult<bool> {
    let root = std::path::Path::new(&project_root);
    Ok(ralph_state::clear_state(root, &session_id).await)
}

/// Clear all Ralph state files in a project (called by cancel skill or on project close).
#[tauri::command]
pub async fn clear_all_ralph_sessions(project_root: String) -> CommandResult<usize> {
    let root = std::path::Path::new(&project_root);
    Ok(ralph_state::clear_all_states(root).await)
}

// ─── Autopilot session commands ─────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct AutopilotSessionInfo {
    pub session_id: String,
    pub goal: String,
    pub phase: String,
    pub qa_cycles: u32,
    pub max_qa_cycles: u32,
    pub todos_completed: Vec<String>,
    pub todos_pending: Vec<String>,
    pub started_at: String,
    pub updated_at: String,
}

impl From<autopilot_state::AutopilotState> for AutopilotSessionInfo {
    fn from(s: autopilot_state::AutopilotState) -> Self {
        Self {
            session_id: s.session_id,
            goal: s.goal,
            phase: s.phase.to_string(),
            qa_cycles: s.qa_cycles,
            max_qa_cycles: s.max_qa_cycles,
            todos_completed: s.todos_completed,
            todos_pending: s.todos_pending,
            started_at: s.started_at.to_rfc3339(),
            updated_at: s.updated_at.to_rfc3339(),
        }
    }
}

/// List all active (non-complete) Autopilot sessions for a project root.
#[tauri::command]
pub async fn list_autopilot_sessions(
    project_root: String,
) -> CommandResult<Vec<AutopilotSessionInfo>> {
    let root = std::path::Path::new(&project_root);
    let states = autopilot_state::list_states(root).await;
    let infos = states
        .into_iter()
        .filter(|s| !matches!(s.phase, autopilot_state::AutopilotPhase::Complete))
        .map(AutopilotSessionInfo::from)
        .collect();
    Ok(infos)
}

/// Clear the state file for a specific Autopilot session.
#[tauri::command]
pub async fn clear_autopilot_session(
    project_root: String,
    session_id: String,
) -> CommandResult<bool> {
    let root = std::path::Path::new(&project_root);
    Ok(autopilot_state::clear_state(root, &session_id).await)
}

/// Clear all Autopilot state files in a project.
#[tauri::command]
pub async fn clear_all_autopilot_sessions(project_root: String) -> CommandResult<usize> {
    let root = std::path::Path::new(&project_root);
    Ok(autopilot_state::clear_all_states(root).await)
}

// ─── Team session commands ───────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct TeamSessionInfo {
    pub session_id: String,
    pub goal: String,
    pub phase: String,
    pub subtask_count: usize,
    pub completed_count: usize,
    pub failed_count: usize,
    pub running_count: usize,
    pub started_at: String,
    pub updated_at: String,
}

impl From<crate::domain::team_state::TeamState> for TeamSessionInfo {
    fn from(s: crate::domain::team_state::TeamState) -> Self {
        let completed = s.completed_count();
        let failed = s.failed_count();
        let running = s.running_count();
        let total = s.subtasks.len();
        Self {
            session_id: s.session_id,
            goal: s.goal,
            phase: s.phase.to_string(),
            subtask_count: total,
            completed_count: completed,
            failed_count: failed,
            running_count: running,
            started_at: s.started_at.to_rfc3339(),
            updated_at: s.updated_at.to_rfc3339(),
        }
    }
}

/// List active (non-complete) Team sessions for a project root.
#[tauri::command]
pub async fn list_team_sessions(project_root: String) -> CommandResult<Vec<TeamSessionInfo>> {
    use crate::domain::team_state;
    let root = std::path::Path::new(&project_root);
    let states = team_state::list_states(root).await;
    let infos = states
        .into_iter()
        .filter(|s| !matches!(s.phase, team_state::TeamPhase::Complete))
        .map(TeamSessionInfo::from)
        .collect();
    Ok(infos)
}

/// Clear the state file for a specific Team session.
#[tauri::command]
pub async fn clear_team_session(project_root: String, session_id: String) -> CommandResult<bool> {
    use crate::domain::team_state;
    let root = std::path::Path::new(&project_root);
    Ok(team_state::clear_state(root, &session_id).await)
}

/// Check if a Ralph session is stuck (consecutive_errors >= threshold).
/// Returns the session info if stuck, or null if healthy / not found.
#[tauri::command]
pub async fn check_ralph_stuck(
    project_root: String,
    session_id: String,
) -> CommandResult<Option<RalphSessionInfo>> {
    const STUCK_THRESHOLD: u32 = 3;
    let root = std::path::Path::new(&project_root);
    let Some(state) = ralph_state::read_state(root, &session_id).await else {
        return Ok(None);
    };
    if state.consecutive_errors >= STUCK_THRESHOLD {
        Ok(Some(RalphSessionInfo::from(state)))
    } else {
        Ok(None)
    }
}

// ─── Bulk cancel ─────────────────────────────────────────────────────────────

#[derive(Debug, serde::Serialize)]
pub struct CancelAllResult {
    /// Background agents cancelled
    pub agents_cancelled: usize,
    /// Ralph state files cleared
    pub ralph_sessions_cleared: usize,
    /// Autopilot state files cleared
    pub autopilot_sessions_cleared: usize,
    /// Team state files cleared
    pub team_sessions_cleared: usize,
    /// Blackboard files cleared
    pub blackboards_cleared: usize,
}

/// Cancel all running background agents and clear all Ralph/Team state files
/// for the given project root. This is the "emergency stop" used by the cancel skill.
#[tauri::command]
pub async fn cancel_all(project_root: String) -> CommandResult<CancelAllResult> {
    use crate::domain::agents::background::get_background_agent_manager;
    use crate::domain::{autopilot_state, ralph_state, team_state};

    let root = std::path::Path::new(&project_root);

    // 1. Cancel all running background agents
    let manager = get_background_agent_manager();
    let agents_cancelled = manager.cancel_all_running().await;

    // 2. Clear Ralph state files
    let ralph_sessions_cleared = ralph_state::clear_all_states(root).await;

    // 3. Clear Autopilot state files
    let autopilot_sessions_cleared = autopilot_state::clear_all_states(root).await;

    // 4. Clear Team state files
    let team_sessions_cleared = team_state::clear_all_states(root).await;

    // 5. Clear blackboard files
    let blackboards_cleared = clear_all_blackboards(root).await;

    Ok(CancelAllResult {
        agents_cancelled,
        ralph_sessions_cleared,
        autopilot_sessions_cleared,
        team_sessions_cleared,
        blackboards_cleared,
    })
}

/// Cancel all background agents associated with a specific Team session's subtasks,
/// then clear the Team state and blackboard for that session.
#[tauri::command]
pub async fn cancel_team_session(project_root: String, session_id: String) -> CommandResult<usize> {
    use crate::domain::agents::background::{get_background_agent_manager, BackgroundAgentStatus};
    use crate::domain::{blackboard, team_state};

    let root = std::path::Path::new(&project_root);
    let manager = get_background_agent_manager();
    let mut cancelled = 0;

    // Read the team state to find bg_task_ids for each subtask
    if let Some(state) = team_state::read_state(root, &session_id).await {
        for subtask in &state.subtasks {
            if let Some(bg_id) = &subtask.bg_task_id {
                if let Some(task) = manager.get_task(bg_id).await {
                    if matches!(
                        task.status,
                        BackgroundAgentStatus::Running | BackgroundAgentStatus::Pending
                    ) {
                        manager.cancel_task(bg_id).await;
                        cancelled += 1;
                    }
                }
            }
        }
    }

    // Clean up state and blackboard
    team_state::clear_state(root, &session_id).await;
    blackboard::clear_board(root, &session_id).await;

    Ok(cancelled)
}

/// Helper: remove all blackboard-*.json files under .omiga/context/
async fn clear_all_blackboards(project_root: &std::path::Path) -> usize {
    let dir = project_root.join(".omiga").join("context");
    let Ok(mut entries) = tokio::fs::read_dir(&dir).await else {
        return 0;
    };
    let mut count = 0;
    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if name.starts_with("blackboard-") && name.ends_with(".json") {
            if tokio::fs::remove_file(&path).await.is_ok() {
                count += 1;
            }
        }
    }
    count
}
