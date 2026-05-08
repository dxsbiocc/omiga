//! Autopilot session state — persisted to `.omiga/state/autopilot-{session_id}.json`
//! so end-to-end autonomous executions can surface current phase and resume points.

use crate::domain::session::{TodoItem, TodoStatus};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::Path;
use tokio::fs;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutopilotPhase {
    Intake,
    Interview,
    Expansion,
    Design,
    Plan,
    Implementation,
    Qa,
    Validation,
    Complete,
}

impl std::fmt::Display for AutopilotPhase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Intake => "intake",
            Self::Interview => "interview",
            Self::Expansion => "expansion",
            Self::Design => "design",
            Self::Plan => "plan",
            Self::Implementation => "implementation",
            Self::Qa => "qa",
            Self::Validation => "validation",
            Self::Complete => "complete",
        };
        f.write_str(s)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutopilotState {
    pub version: u8,
    pub session_id: String,
    pub goal: String,
    pub phase: AutopilotPhase,
    pub project_root: String,
    pub qa_cycles: u32,
    pub max_qa_cycles: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env: Option<String>,
    #[serde(default)]
    pub todos_completed: Vec<String>,
    #[serde(default)]
    pub todos_pending: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    pub started_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl AutopilotState {
    pub fn new(session_id: String, goal: String, project_root: String) -> Self {
        let now = Utc::now();
        Self {
            version: 1,
            session_id,
            goal,
            phase: AutopilotPhase::Intake,
            project_root,
            qa_cycles: 0,
            max_qa_cycles: 5,
            env: None,
            todos_completed: vec![],
            todos_pending: vec![],
            last_error: None,
            started_at: now,
            updated_at: now,
        }
    }

    pub fn touch(&mut self) {
        self.updated_at = Utc::now();
    }

    pub fn sync_todos(&mut self, todos: &[TodoItem]) {
        self.todos_completed = todos
            .iter()
            .filter(|t| matches!(t.status, TodoStatus::Completed))
            .map(|t| t.content.clone())
            .collect();
        self.todos_pending = todos
            .iter()
            .filter(|t| !matches!(t.status, TodoStatus::Completed))
            .map(|t| t.content.clone())
            .collect();
        self.touch();
    }

    pub fn qa_limit_reached(&self) -> bool {
        self.qa_cycles > self.max_qa_cycles
    }
}

fn validate_session_id(session_id: &str) -> Result<(), String> {
    if session_id.is_empty() || session_id.len() > 128 {
        return Err("invalid session_id length".to_string());
    }
    if session_id
        .chars()
        .any(|c| !c.is_alphanumeric() && c != '-' && c != '_')
    {
        return Err(format!("invalid session_id: '{}'", session_id));
    }
    Ok(())
}

fn state_dir(project_root: &Path) -> std::path::PathBuf {
    project_root.join(".omiga").join("state")
}

fn state_path(project_root: &Path, session_id: &str) -> std::path::PathBuf {
    state_dir(project_root).join(format!("autopilot-{}.json", session_id))
}

pub async fn write_state(project_root: &Path, state: &AutopilotState) -> std::io::Result<()> {
    validate_session_id(&state.session_id)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
    let dir = state_dir(project_root);
    fs::create_dir_all(&dir).await?;
    let json = serde_json::to_string_pretty(state)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    fs::write(state_path(project_root, &state.session_id), json).await
}

pub async fn read_state(project_root: &Path, session_id: &str) -> Option<AutopilotState> {
    validate_session_id(session_id).ok()?;
    let json = fs::read_to_string(state_path(project_root, session_id))
        .await
        .ok()?;
    serde_json::from_str(&json).ok()
}

pub async fn list_states(project_root: &Path) -> Vec<AutopilotState> {
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
        if !stem.starts_with("autopilot-") {
            continue;
        }
        if let Ok(json) = fs::read_to_string(&path).await {
            if let Ok(state) = serde_json::from_str::<AutopilotState>(&json) {
                states.push(state);
            }
        }
    }
    states.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    states
}

pub async fn clear_state(project_root: &Path, session_id: &str) -> bool {
    if validate_session_id(session_id).is_err() {
        return false;
    }
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

pub async fn begin_turn(
    project_root: &Path,
    session_id: &str,
    goal: &str,
    env: Option<String>,
    todos: &[TodoItem],
) -> std::io::Result<AutopilotState> {
    let existing = read_state(project_root, session_id).await;
    let was_complete = existing
        .as_ref()
        .map(|s| matches!(s.phase, AutopilotPhase::Complete))
        .unwrap_or(false);
    let mut state = existing.clone().unwrap_or_else(|| {
        AutopilotState::new(
            session_id.to_string(),
            goal.to_string(),
            project_root.to_string_lossy().to_string(),
        )
    });
    state.goal = goal.to_string();
    state.project_root = project_root.to_string_lossy().to_string();
    state.phase = AutopilotPhase::Expansion;
    if existing.is_none() || was_complete {
        state.qa_cycles = 0;
        state.last_error = None;
    }
    state.env = env;
    state.sync_todos(todos);
    write_state(project_root, &state).await?;
    Ok(state)
}

pub async fn update_phase(
    project_root: &Path,
    session_id: &str,
    phase: AutopilotPhase,
    env: Option<String>,
    todos: &[TodoItem],
) -> std::io::Result<Option<AutopilotState>> {
    let Some(mut state) = read_state(project_root, session_id).await else {
        return Ok(None);
    };
    let previous_phase = state.phase;
    state.phase = phase;
    if env.is_some() {
        state.env = env;
    }
    if phase == AutopilotPhase::Qa && previous_phase != AutopilotPhase::Qa {
        state.qa_cycles = state.qa_cycles.saturating_add(1);
    }
    state.sync_todos(todos);
    write_state(project_root, &state).await?;
    Ok(Some(state))
}

pub async fn complete_turn(
    project_root: &Path,
    session_id: &str,
    todos: &[TodoItem],
) -> std::io::Result<Option<AutopilotState>> {
    let Some(mut state) = read_state(project_root, session_id).await else {
        return Ok(None);
    };
    state.phase = AutopilotPhase::Complete;
    state.last_error = None;
    state.sync_todos(todos);
    write_state(project_root, &state).await?;
    Ok(Some(state))
}

pub async fn fail_turn(
    project_root: &Path,
    session_id: &str,
    phase: AutopilotPhase,
    todos: &[TodoItem],
    error: &str,
) -> std::io::Result<Option<AutopilotState>> {
    let Some(mut state) = read_state(project_root, session_id).await else {
        return Ok(None);
    };
    state.phase = phase;
    state.last_error = Some(error.chars().take(500).collect());
    state.sync_todos(todos);
    write_state(project_root, &state).await?;
    Ok(Some(state))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn sample_todos() -> Vec<TodoItem> {
        vec![
            TodoItem {
                content: "spec".to_string(),
                status: TodoStatus::Completed,
                active_form: "writing spec".to_string(),
            },
            TodoItem {
                content: "qa".to_string(),
                status: TodoStatus::InProgress,
                active_form: "running qa".to_string(),
            },
        ]
    }

    #[tokio::test]
    async fn begin_update_complete_round_trip() {
        let dir = tempdir().unwrap();
        let todos = sample_todos();
        let state = begin_turn(
            dir.path(),
            "auto-1",
            "Build feature",
            Some("conda:test".to_string()),
            &todos,
        )
        .await
        .unwrap();
        assert_eq!(state.phase, AutopilotPhase::Expansion);
        assert_eq!(state.todos_completed, vec!["spec".to_string()]);

        let qa = update_phase(dir.path(), "auto-1", AutopilotPhase::Qa, None, &todos)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(qa.qa_cycles, 1);

        let done = complete_turn(dir.path(), "auto-1", &todos)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(done.phase, AutopilotPhase::Complete);
        assert!(done.last_error.is_none());
    }

    #[tokio::test]
    async fn fail_turn_records_error() {
        let dir = tempdir().unwrap();
        let todos = sample_todos();
        let _ = begin_turn(dir.path(), "auto-2", "Goal", None, &todos)
            .await
            .unwrap();
        let failed = fail_turn(
            dir.path(),
            "auto-2",
            AutopilotPhase::Implementation,
            &todos,
            "tests failed",
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(failed.phase, AutopilotPhase::Implementation);
        assert_eq!(failed.last_error.as_deref(), Some("tests failed"));
    }

    #[tokio::test]
    async fn qa_cycles_increment_only_on_entering_qa() {
        let dir = tempdir().unwrap();
        let todos = sample_todos();
        let _ = begin_turn(dir.path(), "auto-3", "Goal", None, &todos)
            .await
            .unwrap();

        let qa1 = update_phase(dir.path(), "auto-3", AutopilotPhase::Qa, None, &todos)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(qa1.qa_cycles, 1);

        let qa_same = update_phase(dir.path(), "auto-3", AutopilotPhase::Qa, None, &todos)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(qa_same.qa_cycles, 1);

        let _ = update_phase(
            dir.path(),
            "auto-3",
            AutopilotPhase::Implementation,
            None,
            &todos,
        )
        .await
        .unwrap();
        let qa2 = update_phase(dir.path(), "auto-3", AutopilotPhase::Qa, None, &todos)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(qa2.qa_cycles, 2);
    }

    #[tokio::test]
    async fn qa_limit_reached_after_exceeding_max_cycles() {
        let dir = tempdir().unwrap();
        let todos = sample_todos();
        let mut state = begin_turn(dir.path(), "auto-4", "Goal", None, &todos)
            .await
            .unwrap();
        state.max_qa_cycles = 1;
        write_state(dir.path(), &state).await.unwrap();

        let qa1 = update_phase(dir.path(), "auto-4", AutopilotPhase::Qa, None, &todos)
            .await
            .unwrap()
            .unwrap();
        assert!(!qa1.qa_limit_reached());

        let _ = update_phase(
            dir.path(),
            "auto-4",
            AutopilotPhase::Implementation,
            None,
            &todos,
        )
        .await
        .unwrap();
        let qa2 = update_phase(dir.path(), "auto-4", AutopilotPhase::Qa, None, &todos)
            .await
            .unwrap()
            .unwrap();
        assert!(qa2.qa_limit_reached());
    }

    #[tokio::test]
    async fn list_and_clear_all_states() {
        let dir = tempdir().unwrap();
        let todos = sample_todos();
        let _ = begin_turn(dir.path(), "auto-a", "Goal A", None, &todos)
            .await
            .unwrap();
        let _ = begin_turn(dir.path(), "auto-b", "Goal B", None, &todos)
            .await
            .unwrap();
        assert_eq!(list_states(dir.path()).await.len(), 2);
        assert_eq!(clear_all_states(dir.path()).await, 2);
        assert!(list_states(dir.path()).await.is_empty());
    }
}
