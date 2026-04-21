//! Shared blackboard — persisted to `.omiga/context/blackboard-{session_id}.json`
//!
//! Allows parallel Team workers to post structured results that sibling agents
//! and the Architect aggregator can query.  Written after every update so a
//! crash can read the partial board and resume.

use chrono::{DateTime, Utc};
use lazy_static::lazy_static;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use tokio::fs;
use tokio::sync::Mutex as AsyncMutex;

// Per-session write lock: prevents concurrent post_blackboard_entry calls from
// overwriting each other's entries via the read→modify→write pattern.
lazy_static! {
    static ref SESSION_LOCKS: Mutex<HashMap<String, Arc<AsyncMutex<()>>>> =
        Mutex::new(HashMap::new());
}

fn session_lock(session_id: &str) -> Arc<AsyncMutex<()>> {
    let mut map = SESSION_LOCKS.lock().unwrap();
    map.entry(session_id.to_string())
        .or_insert_with(|| Arc::new(AsyncMutex::new(())))
        .clone()
}

/// A single entry posted by one worker.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlackboardEntry {
    /// Subtask that posted this entry
    pub subtask_id: String,
    /// Agent role that executed the subtask
    pub agent_type: String,
    /// Logical key (e.g. "result", "error_summary", "artifact_path")
    pub key: String,
    /// Value — plain text or JSON-encoded structured data
    pub value: String,
    pub posted_at: DateTime<Utc>,
}

/// Session-scoped shared blackboard.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Blackboard {
    pub version: u8,
    pub session_id: String,
    pub entries: Vec<BlackboardEntry>,
    pub updated_at: DateTime<Utc>,
}

impl Blackboard {
    pub fn new(session_id: String) -> Self {
        Self {
            version: 1,
            session_id,
            entries: vec![],
            updated_at: Utc::now(),
        }
    }

    pub fn post(&mut self, entry: BlackboardEntry) {
        self.entries.push(entry);
        self.updated_at = Utc::now();
    }

    pub fn query_by_key<'a>(&'a self, key: &str) -> Vec<&'a BlackboardEntry> {
        self.entries.iter().filter(|e| e.key == key).collect()
    }

    /// Exact-match lookup (primary path, fast).
    pub fn query_by_subtask<'a>(&'a self, subtask_id: &str) -> Vec<&'a BlackboardEntry> {
        let exact: Vec<_> = self
            .entries
            .iter()
            .filter(|e| e.subtask_id == subtask_id)
            .collect();
        if !exact.is_empty() {
            return exact;
        }
        // Fallback: case-insensitive match, then prefix match.
        // Handles planner ID drift (e.g. "Phase-Explore" vs "phase-explore").
        let lower = subtask_id.to_lowercase();
        let ci: Vec<_> = self
            .entries
            .iter()
            .filter(|e| e.subtask_id.to_lowercase() == lower)
            .collect();
        if !ci.is_empty() {
            return ci;
        }
        // Prefix match — e.g. dep "explore" matches subtask_id "explore-1".
        // Require minimum 6 chars to avoid short IDs (e.g. "t") matching unrelated subtasks.
        if lower.len() < 6 {
            return vec![];
        }
        self.entries
            .iter()
            .filter(|e| {
                let eid = e.subtask_id.to_lowercase();
                eid.len() >= 6 && (eid.starts_with(&lower) || lower.starts_with(&eid))
            })
            .collect()
    }

    /// Render board as a markdown section suitable for inclusion in the
    /// Architect's aggregation prompt.
    pub fn snapshot_markdown(&self) -> String {
        if self.entries.is_empty() {
            return String::new();
        }
        let mut out = String::from("## Worker Results\n\n");
        for e in &self.entries {
            out.push_str(&format!(
                "### [{subtask}] {agent}\n{value}\n\n",
                subtask = e.subtask_id,
                agent = e.agent_type,
                value = e.value.trim(),
            ));
        }
        out
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Reject session IDs that could escape the context directory via path traversal.
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

fn board_path(project_root: &Path, session_id: &str) -> std::path::PathBuf {
    project_root
        .join(".omiga")
        .join("context")
        .join(format!("blackboard-{}.json", session_id))
}

/// Atomically append an entry to the persisted board for `session_id`.
///
/// Acquires a per-session async lock before the read→modify→write cycle so
/// that concurrent calls from parallel worker agents cannot overwrite each
/// other's entries.
pub async fn post_entry(
    project_root: &Path,
    session_id: &str,
    entry: BlackboardEntry,
) -> std::io::Result<()> {
    validate_session_id(session_id)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
    let lock = session_lock(session_id);
    let _guard = lock.lock().await;

    let mut board = read_board(project_root, session_id)
        .await
        .unwrap_or_else(|| Blackboard::new(session_id.to_string()));
    board.post(entry);
    write_board(project_root, &board).await
}

pub async fn write_board(project_root: &Path, board: &Blackboard) -> std::io::Result<()> {
    validate_session_id(&board.session_id)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
    let dir = project_root.join(".omiga").join("context");
    fs::create_dir_all(&dir).await?;
    let json = serde_json::to_string_pretty(board)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    fs::write(board_path(project_root, &board.session_id), json).await
}

pub async fn read_board(project_root: &Path, session_id: &str) -> Option<Blackboard> {
    validate_session_id(session_id).ok()?;
    let json = fs::read_to_string(board_path(project_root, session_id))
        .await
        .ok()?;
    serde_json::from_str(&json).ok()
}

pub async fn clear_board(project_root: &Path, session_id: &str) -> bool {
    if validate_session_id(session_id).is_err() {
        return false;
    }
    let removed = fs::remove_file(board_path(project_root, session_id))
        .await
        .is_ok();
    // Clean up the per-session write lock so SESSION_LOCKS doesn't grow unboundedly.
    SESSION_LOCKS.lock().unwrap().remove(session_id);
    removed
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn round_trip_blackboard() {
        let dir = tempdir().unwrap();
        let mut board = Blackboard::new("sess-1".to_string());
        board.post(BlackboardEntry {
            subtask_id: "t1".to_string(),
            agent_type: "executor".to_string(),
            key: "result".to_string(),
            value: "all tests pass".to_string(),
            posted_at: Utc::now(),
        });
        write_board(dir.path(), &board).await.unwrap();
        let loaded = read_board(dir.path(), "sess-1").await.unwrap();
        assert_eq!(loaded.entries.len(), 1);
        assert_eq!(loaded.entries[0].key, "result");
    }

    #[test]
    fn snapshot_markdown_format() {
        let mut board = Blackboard::new("s".to_string());
        board.post(BlackboardEntry {
            subtask_id: "t1".to_string(),
            agent_type: "executor".to_string(),
            key: "result".to_string(),
            value: "done".to_string(),
            posted_at: Utc::now(),
        });
        let md = board.snapshot_markdown();
        assert!(md.contains("## Worker Results"));
        assert!(md.contains("[t1]"));
    }

    #[test]
    fn query_by_subtask_case_insensitive_fallback() {
        let mut board = Blackboard::new("s".to_string());
        board.post(BlackboardEntry {
            subtask_id: "Phase-Explore".to_string(),
            agent_type: "Explore".to_string(),
            key: "result".to_string(),
            value: "output".to_string(),
            posted_at: Utc::now(),
        });
        // Exact match succeeds
        assert_eq!(board.query_by_subtask("Phase-Explore").len(), 1);
        // Case-insensitive fallback
        assert_eq!(board.query_by_subtask("phase-explore").len(), 1);
        // No match for unrelated id
        assert_eq!(board.query_by_subtask("other").len(), 0);
    }

    #[test]
    fn query_by_subtask_prefix_fallback() {
        let mut board = Blackboard::new("s".to_string());
        board.post(BlackboardEntry {
            subtask_id: "explore".to_string(),
            agent_type: "Explore".to_string(),
            key: "result".to_string(),
            value: "out".to_string(),
            posted_at: Utc::now(),
        });
        // Dep "explore" matches stored "explore-1" (prefix)
        assert_eq!(board.query_by_subtask("explore").len(), 1);
    }

    #[tokio::test]
    async fn clear_board_removes_file() {
        let dir = tempdir().unwrap();
        let board = Blackboard::new("sess-2".to_string());
        write_board(dir.path(), &board).await.unwrap();
        assert!(clear_board(dir.path(), "sess-2").await);
        assert!(read_board(dir.path(), "sess-2").await.is_none());
    }
}
