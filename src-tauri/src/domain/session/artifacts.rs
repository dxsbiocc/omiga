//! Session-scoped file artifact tracking.
//!
//! Records which files the AI wrote or edited during a session so the frontend
//! can display what was created or modified.

use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

/// A single file operation recorded during a session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactEntry {
    /// Absolute or project-relative file path.
    pub path: String,
    /// `"write"` or `"edit"`.
    pub operation: String,
    /// RFC3339 UTC timestamp of the most recent operation on this path.
    pub ts: String,
}

/// Session-scoped in-memory registry.  Cleared when the session is unloaded.
#[derive(Debug, Default, Clone)]
pub struct ArtifactRegistry(pub Arc<Mutex<Vec<ArtifactEntry>>>);

impl ArtifactRegistry {
    /// Record a file write or edit.  Deduplicates by path: if the path already
    /// exists in the list the existing entry's operation and timestamp are
    /// updated in place rather than appending a duplicate.
    pub fn record(&self, path: impl Into<String>, operation: impl Into<String>) {
        // Recover from a poisoned mutex rather than silently dropping the record.
        if let Ok(mut v) = self.0.lock().or_else(|e| Ok::<_, ()>(e.into_inner())) {
            let entry = ArtifactEntry {
                path: path.into(),
                operation: operation.into(),
                ts: chrono::Utc::now().to_rfc3339(),
            };
            if let Some(existing) = v.iter_mut().find(|e| e.path == entry.path) {
                existing.operation = entry.operation;
                existing.ts = entry.ts;
            } else {
                v.push(entry);
            }
        }
    }

    /// Return all recorded artifacts (cloned snapshot).
    pub fn list(&self) -> Vec<ArtifactEntry> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}
