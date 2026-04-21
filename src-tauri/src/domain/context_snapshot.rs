//! Context snapshot index — lists and reads `.omiga/context/*.md` snapshots.
//! Snapshot files are written by Ralph/Team skills at session start and are
//! human-readable markdown capturing goal, environment, and project state.

use serde::Serialize;
use std::path::Path;
use tokio::fs;

/// Lightweight metadata for a context snapshot file.
#[derive(Debug, Clone, Serialize)]
pub struct ContextSnapshotMeta {
    /// Filename stem (without `.md`), e.g. `run-deseq2-20260417-143022`.
    pub name: String,
    /// Absolute path to the snapshot file.
    pub path: String,
    /// ISO-8601 last-modified timestamp.
    pub modified_at: String,
    /// File size in bytes.
    pub size_bytes: u64,
}

fn context_dir(project_root: &Path) -> std::path::PathBuf {
    project_root.join(".omiga").join("context")
}

/// List all context snapshots for `project_root`, newest first.
pub async fn list_snapshots(project_root: &Path) -> Vec<ContextSnapshotMeta> {
    let dir = context_dir(project_root);
    let Ok(mut entries) = fs::read_dir(&dir).await else {
        return vec![];
    };

    let mut metas: Vec<(std::time::SystemTime, ContextSnapshotMeta)> = vec![];

    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let Ok(meta) = entry.metadata().await else {
            continue;
        };
        let modified = meta.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        let modified_at = chrono::DateTime::<chrono::Utc>::from(modified).to_rfc3339();
        let name = path
            .file_stem()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();

        metas.push((
            modified,
            ContextSnapshotMeta {
                name,
                path: path.to_string_lossy().to_string(),
                modified_at,
                size_bytes: meta.len(),
            },
        ));
    }

    metas.sort_by(|a, b| b.0.cmp(&a.0));
    metas.into_iter().map(|(_, m)| m).collect()
}

/// Read the full content of a snapshot file.
pub async fn read_snapshot(path: &Path) -> Option<String> {
    fs::read_to_string(path).await.ok()
}

/// Delete a specific snapshot by its path. Returns true if the file was removed.
pub async fn delete_snapshot(path: &Path) -> bool {
    fs::remove_file(path).await.is_ok()
}

/// Delete all snapshots for `project_root`. Returns the count removed.
pub async fn clear_all_snapshots(project_root: &Path) -> usize {
    let metas = list_snapshots(project_root).await;
    let mut count = 0;
    for m in &metas {
        if delete_snapshot(Path::new(&m.path)).await {
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
    async fn list_empty_dir() {
        let dir = tempdir().unwrap();
        let snaps = list_snapshots(dir.path()).await;
        assert!(snaps.is_empty());
    }

    #[tokio::test]
    async fn list_and_delete() {
        let dir = tempdir().unwrap();
        let ctx_dir = dir.path().join(".omiga").join("context");
        fs::create_dir_all(&ctx_dir).await.unwrap();

        for i in 0..3 {
            let p = ctx_dir.join(format!("snap-{i}.md"));
            fs::write(&p, format!("# Snapshot {i}")).await.unwrap();
        }

        let snaps = list_snapshots(dir.path()).await;
        assert_eq!(snaps.len(), 3);

        let removed = clear_all_snapshots(dir.path()).await;
        assert_eq!(removed, 3);
        assert!(list_snapshots(dir.path()).await.is_empty());
    }
}
