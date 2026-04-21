//! Team session state — persisted to `.omiga/state/team-{session_id}.json`
//! so parallel Team executions can be inspected and resumed after a crash.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::Path;
use tokio::fs;

/// Lifecycle phase of a Team run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamPhase {
    /// Leader 分析需求，制定任务计划
    Planning,
    /// Workers 并行执行
    Executing,
    /// Verification agent 核查 Worker 输出
    Verifying,
    /// Debugger 修复验证发现的问题（最多 3 次）
    Fixing,
    /// Leader 汇总 Worker 输出，生成最终回复
    Synthesizing,
    /// Complete
    Complete,
    /// Aborted due to critical failure
    Failed,
}

impl std::fmt::Display for TeamPhase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            TeamPhase::Planning => "planning",
            TeamPhase::Executing => "executing",
            TeamPhase::Verifying => "verifying",
            TeamPhase::Fixing => "fixing",
            TeamPhase::Synthesizing => "synthesizing",
            TeamPhase::Complete => "complete",
            TeamPhase::Failed => "failed",
        };
        f.write_str(s)
    }
}

/// Persisted state for one subtask within a Team run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamSubtaskState {
    pub id: String,
    pub description: String,
    pub agent_type: String,
    /// "pending" | "running" | "completed" | "failed"
    pub status: String,
    pub attempt: u32,
    pub max_retries: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bg_task_id: Option<String>,
}

/// Persisted state for a Team execution session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamState {
    pub version: u8,
    pub session_id: String,
    pub goal: String,
    pub phase: TeamPhase,
    pub project_root: String,
    pub subtasks: Vec<TeamSubtaskState>,
    pub started_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl TeamState {
    pub fn new(session_id: String, goal: String, project_root: String) -> Self {
        let now = Utc::now();
        Self {
            version: 1,
            session_id,
            goal,
            phase: TeamPhase::Planning,
            project_root,
            subtasks: vec![],
            started_at: now,
            updated_at: now,
        }
    }

    pub fn touch(&mut self) {
        self.updated_at = Utc::now();
    }

    pub fn completed_count(&self) -> usize {
        self.subtasks
            .iter()
            .filter(|s| s.status == "completed")
            .count()
    }

    pub fn failed_count(&self) -> usize {
        self.subtasks
            .iter()
            .filter(|s| s.status == "failed")
            .count()
    }

    pub fn running_count(&self) -> usize {
        self.subtasks
            .iter()
            .filter(|s| s.status == "running")
            .count()
    }
}

fn state_dir(project_root: &Path) -> std::path::PathBuf {
    project_root.join(".omiga").join("state")
}

fn state_path(project_root: &Path, session_id: &str) -> std::path::PathBuf {
    state_dir(project_root).join(format!("team-{}.json", session_id))
}

pub async fn write_state(project_root: &Path, state: &TeamState) -> std::io::Result<()> {
    let dir = state_dir(project_root);
    fs::create_dir_all(&dir).await?;
    let json = serde_json::to_string_pretty(state)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    fs::write(state_path(project_root, &state.session_id), json).await
}

pub async fn read_state(project_root: &Path, session_id: &str) -> Option<TeamState> {
    let json = fs::read_to_string(state_path(project_root, session_id))
        .await
        .ok()?;
    serde_json::from_str(&json).ok()
}

/// List all team state files under `project_root/.omiga/state/`, newest first.
pub async fn list_states(project_root: &Path) -> Vec<TeamState> {
    let dir = state_dir(project_root);
    let Ok(mut entries) = fs::read_dir(&dir).await else {
        return vec![];
    };
    let mut states = vec![];
    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let stem = path.file_stem().and_then(|n| n.to_str()).unwrap_or("");
        if !stem.starts_with("team-") {
            continue;
        }
        if let Ok(json) = fs::read_to_string(&path).await {
            if let Ok(state) = serde_json::from_str::<TeamState>(&json) {
                states.push(state);
            }
        }
    }
    states.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    states
}

pub async fn clear_state(project_root: &Path, session_id: &str) -> bool {
    fs::remove_file(state_path(project_root, session_id))
        .await
        .is_ok()
}

pub async fn clear_all_states(project_root: &Path) -> usize {
    let states = list_states(project_root).await;
    let mut count = 0;
    for state in &states {
        if clear_state(project_root, &state.session_id).await {
            count += 1;
        }
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn round_trip_team_state() {
        let dir = tempdir().unwrap();
        let mut state = TeamState::new(
            "team-abc".to_string(),
            "Run parallel analysis".to_string(),
            dir.path().to_string_lossy().to_string(),
        );
        state.subtasks.push(TeamSubtaskState {
            id: "t1".to_string(),
            description: "Run DESeq2".to_string(),
            agent_type: "executor".to_string(),
            status: "completed".to_string(),
            attempt: 0,
            max_retries: 2,
            error: None,
            bg_task_id: Some("bg-123".to_string()),
        });
        write_state(dir.path(), &state).await.unwrap();
        let loaded = read_state(dir.path(), "team-abc").await.unwrap();
        assert_eq!(loaded.session_id, "team-abc");
        assert_eq!(loaded.subtasks.len(), 1);
        assert_eq!(loaded.completed_count(), 1);
    }

    #[tokio::test]
    async fn list_and_clear_team_states() {
        let dir = tempdir().unwrap();
        for i in 0..3 {
            let s = TeamState::new(
                format!("team-{i}"),
                format!("goal {i}"),
                dir.path().to_string_lossy().to_string(),
            );
            write_state(dir.path(), &s).await.unwrap();
        }
        assert_eq!(list_states(dir.path()).await.len(), 3);
        assert_eq!(clear_all_states(dir.path()).await, 3);
        assert!(list_states(dir.path()).await.is_empty());
    }
}
