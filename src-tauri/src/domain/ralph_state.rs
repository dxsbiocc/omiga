//! Ralph session state — persisted to `.omiga/state/ralph-{session_id}.json`
//! so long-running analyses can resume after a crash or session restart.

use crate::domain::session::{TodoItem, TodoStatus};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::Path;
use tokio::fs;

/// Lifecycle phase of a Ralph run, matching the skill step numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RalphPhase {
    /// Step 0: task intake and planning
    Planning,
    /// Step 1: environment verification
    EnvCheck,
    /// Step 2: analysis execution (main loop)
    Executing,
    /// Step 3: output quality checks
    QualityCheck,
    /// Step 4: Architect verification
    Verifying,
    /// Step 5: loop deciding or done
    Complete,
}

impl std::fmt::Display for RalphPhase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            RalphPhase::Planning => "planning",
            RalphPhase::EnvCheck => "env_check",
            RalphPhase::Executing => "executing",
            RalphPhase::QualityCheck => "quality_check",
            RalphPhase::Verifying => "verifying",
            RalphPhase::Complete => "complete",
        };
        f.write_str(s)
    }
}

/// Persisted state for a Ralph execution session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RalphState {
    /// Schema version — bump when the struct changes incompatibly.
    pub version: u8,
    /// Unique session identifier (matches the background task or chat session ID).
    pub session_id: String,
    /// The original user goal / task description.
    pub goal: String,
    /// Current lifecycle phase.
    pub phase: RalphPhase,
    /// How many full execute→verify loops have completed.
    pub iteration: u32,
    /// Consecutive identical errors — stop and report when this reaches 3.
    pub consecutive_errors: u32,
    /// Absolute path to the project root.
    pub project_root: String,
    /// Active conda/venv environment name, if known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env: Option<String>,
    /// Todo items already marked completed.
    #[serde(default)]
    pub todos_completed: Vec<String>,
    /// Todo items still pending.
    #[serde(default)]
    pub todos_pending: Vec<String>,
    /// Last error message seen, for deduplication.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    pub started_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl RalphState {
    pub fn new(session_id: String, goal: String, project_root: String) -> Self {
        let now = Utc::now();
        Self {
            version: 1,
            session_id,
            goal,
            phase: RalphPhase::Planning,
            iteration: 1,
            consecutive_errors: 0,
            project_root,
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

    pub fn clear_error(&mut self) {
        self.last_error = None;
        self.consecutive_errors = 0;
        self.touch();
    }

    pub fn record_error(&mut self, error: &str) {
        let next_fp = fingerprint(error);
        let prev_fp = self.last_error.as_deref().map(fingerprint);
        if prev_fp.as_deref() == Some(next_fp.as_str()) {
            self.consecutive_errors = self.consecutive_errors.saturating_add(1);
        } else {
            self.consecutive_errors = 1;
        }
        self.last_error = Some(error.chars().take(500).collect());
        self.touch();
    }
}

fn fingerprint(msg: &str) -> String {
    use sha1::{Digest, Sha1};

    let mut normalized = msg.to_string();
    let line_re = regex::Regex::new(r"(?i)\bline\s+\d+\b").unwrap();
    normalized = line_re.replace_all(&normalized, "line N").to_string();
    let colon_re = regex::Regex::new(r":\d+:").unwrap();
    normalized = colon_re.replace_all(&normalized, ":N:").to_string();
    let hex_re = regex::Regex::new(r"0x[0-9a-fA-F]+").unwrap();
    normalized = hex_re.replace_all(&normalized, "0xADDR").to_string();
    let ts_re = regex::Regex::new(r"\d{4}-\d{2}-\d{2}[T ]\d{2}:\d{2}:\d{2}").unwrap();
    normalized = ts_re.replace_all(&normalized, "TIMESTAMP").to_string();
    normalized = normalized.split_whitespace().collect::<Vec<_>>().join(" ");
    let prefix: String = normalized.chars().take(200).collect();

    let mut hasher = Sha1::new();
    hasher.update(prefix.as_bytes());
    let digest = hasher.finalize();
    format!("{:x}", digest)[..16].to_string()
}

pub async fn begin_turn(
    project_root: &Path,
    session_id: &str,
    goal: &str,
    env: Option<String>,
    todos: &[TodoItem],
) -> std::io::Result<RalphState> {
    let existing = read_state(project_root, session_id).await;
    let mut state = existing.clone().unwrap_or_else(|| {
        RalphState::new(
            session_id.to_string(),
            goal.to_string(),
            project_root.to_string_lossy().to_string(),
        )
    });

    if existing.is_some() && state.session_id == session_id && state.goal == goal {
        state.iteration = state.iteration.saturating_add(1).max(1);
    } else {
        state.goal = goal.to_string();
        state.iteration = 1;
    }

    state.project_root = project_root.to_string_lossy().to_string();
    state.phase = RalphPhase::Planning;
    state.env = env;
    state.sync_todos(todos);
    write_state(project_root, &state).await?;
    Ok(state)
}

pub async fn update_phase(
    project_root: &Path,
    session_id: &str,
    phase: RalphPhase,
    env: Option<String>,
    todos: &[TodoItem],
) -> std::io::Result<Option<RalphState>> {
    let Some(mut state) = read_state(project_root, session_id).await else {
        return Ok(None);
    };
    state.phase = phase;
    if env.is_some() {
        state.env = env;
    }
    state.sync_todos(todos);
    write_state(project_root, &state).await?;
    Ok(Some(state))
}

pub async fn complete_turn(
    project_root: &Path,
    session_id: &str,
    todos: &[TodoItem],
) -> std::io::Result<Option<RalphState>> {
    let Some(mut state) = read_state(project_root, session_id).await else {
        return Ok(None);
    };
    state.phase = RalphPhase::Complete;
    state.sync_todos(todos);
    state.clear_error();
    write_state(project_root, &state).await?;
    Ok(Some(state))
}

pub async fn fail_turn(
    project_root: &Path,
    session_id: &str,
    phase: RalphPhase,
    todos: &[TodoItem],
    error: &str,
) -> std::io::Result<Option<RalphState>> {
    let Some(mut state) = read_state(project_root, session_id).await else {
        return Ok(None);
    };
    state.phase = phase;
    state.sync_todos(todos);
    state.record_error(error);
    write_state(project_root, &state).await?;
    Ok(Some(state))
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
    state_dir(project_root).join(format!("ralph-{}.json", session_id))
}

pub async fn write_state(project_root: &Path, state: &RalphState) -> std::io::Result<()> {
    validate_session_id(&state.session_id)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
    let dir = state_dir(project_root);
    fs::create_dir_all(&dir).await?;
    let json = serde_json::to_string_pretty(state)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    fs::write(state_path(project_root, &state.session_id), json).await
}

pub async fn read_state(project_root: &Path, session_id: &str) -> Option<RalphState> {
    validate_session_id(session_id).ok()?;
    let json = fs::read_to_string(state_path(project_root, session_id))
        .await
        .ok()?;
    serde_json::from_str(&json).ok()
}

/// List all ralph state files under `project_root/.omiga/state/`, newest first.
pub async fn list_states(project_root: &Path) -> Vec<RalphState> {
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
        if !stem.starts_with("ralph-") {
            continue;
        }
        if let Ok(json) = fs::read_to_string(&path).await {
            if let Ok(state) = serde_json::from_str::<RalphState>(&json) {
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

/// Remove all ralph state files; returns how many were deleted.
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
    use crate::domain::session::{TodoItem, TodoStatus};
    use tempfile::tempdir;

    #[tokio::test]
    async fn round_trip_state() {
        let dir = tempdir().unwrap();
        let state = RalphState::new(
            "sess-abc".to_string(),
            "Run DESeq2 analysis".to_string(),
            dir.path().to_string_lossy().to_string(),
        );
        write_state(dir.path(), &state).await.unwrap();
        let loaded = read_state(dir.path(), "sess-abc").await.unwrap();
        assert_eq!(loaded.session_id, "sess-abc");
        assert_eq!(loaded.goal, "Run DESeq2 analysis");
        assert_eq!(loaded.phase, RalphPhase::Planning);
    }

    #[tokio::test]
    async fn list_and_clear() {
        let dir = tempdir().unwrap();
        for i in 0..3 {
            let s = RalphState::new(
                format!("sess-{i}"),
                format!("goal {i}"),
                dir.path().to_string_lossy().to_string(),
            );
            write_state(dir.path(), &s).await.unwrap();
        }
        let states = list_states(dir.path()).await;
        assert_eq!(states.len(), 3);

        let removed = clear_all_states(dir.path()).await;
        assert_eq!(removed, 3);
        assert!(list_states(dir.path()).await.is_empty());
    }

    #[test]
    fn record_error_deduplicates_similar_errors() {
        let mut state = RalphState::new(
            "sess-err".to_string(),
            "goal".to_string(),
            "/tmp".to_string(),
        );
        state.record_error("line 42 failed at 2026-04-20T10:00:00 with addr 0x1234");
        assert_eq!(state.consecutive_errors, 1);
        state.record_error("line 99 failed at 2026-04-20T10:30:00 with addr 0xabcd");
        assert_eq!(state.consecutive_errors, 2);
        state.record_error("different root cause");
        assert_eq!(state.consecutive_errors, 1);
    }

    #[test]
    fn sync_todos_splits_completed_and_pending() {
        let mut state = RalphState::new(
            "sess-todo".to_string(),
            "goal".to_string(),
            "/tmp".to_string(),
        );
        state.sync_todos(&[
            TodoItem {
                content: "done".to_string(),
                status: TodoStatus::Completed,
                active_form: "doing done".to_string(),
            },
            TodoItem {
                content: "doing".to_string(),
                status: TodoStatus::InProgress,
                active_form: "doing".to_string(),
            },
        ]);
        assert_eq!(state.todos_completed, vec!["done".to_string()]);
        assert_eq!(state.todos_pending, vec!["doing".to_string()]);
    }

    #[tokio::test]
    async fn begin_update_complete_turn_round_trip() {
        let dir = tempdir().unwrap();
        let todos = vec![TodoItem {
            content: "env check".to_string(),
            status: TodoStatus::InProgress,
            active_form: "checking env".to_string(),
        }];
        let state = begin_turn(
            dir.path(),
            "sess-flow",
            "Investigate runtime",
            Some("conda:test".to_string()),
            &todos,
        )
        .await
        .unwrap();
        assert_eq!(state.phase, RalphPhase::Planning);
        assert_eq!(state.iteration, 1);

        let updated = update_phase(dir.path(), "sess-flow", RalphPhase::Executing, None, &todos)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.phase, RalphPhase::Executing);

        let completed = complete_turn(dir.path(), "sess-flow", &todos)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(completed.phase, RalphPhase::Complete);
        assert_eq!(completed.consecutive_errors, 0);
    }
}
