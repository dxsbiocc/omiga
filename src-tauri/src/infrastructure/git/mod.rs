//! Git utilities

use git2::{Repository, StatusOptions};
use std::path::Path;

/// Get git status for a directory
pub fn get_status(path: &Path) -> Result<GitStatus, git2::Error> {
    let repo = Repository::open(path)?;

    let mut opts = StatusOptions::new();
    opts.include_untracked(true)
        .renames_head_to_index(true)
        .renames_index_to_workdir(true);

    let statuses = repo.statuses(Some(&mut opts))?;

    let mut modified = Vec::new();
    let mut added = Vec::new();
    let mut deleted = Vec::new();
    let mut untracked = Vec::new();

    for entry in statuses.iter() {
        let status = entry.status();
        let path = entry.path().unwrap_or("").to_string();

        if status.is_index_new() || status.is_wt_new() {
            added.push(path.clone());
            untracked.push(path);
        } else if status.is_index_deleted() || status.is_wt_deleted() {
            deleted.push(path);
        } else if status.is_index_modified() || status.is_wt_modified() {
            modified.push(path);
        } else if status.is_index_renamed() || status.is_wt_renamed() {
            modified.push(path);
        }
    }

    // Get current branch
    let head = repo.head()?;
    let branch = head
        .shorthand()
        .map(|s| s.to_string())
        .unwrap_or_else(|| "HEAD".to_string());

    Ok(GitStatus {
        branch,
        modified,
        added,
        deleted,
        untracked,
    })
}

/// Git status information
#[derive(Debug, Clone)]
pub struct GitStatus {
    pub branch: String,
    pub modified: Vec<String>,
    pub added: Vec<String>,
    pub deleted: Vec<String>,
    pub untracked: Vec<String>,
}

/// Check if a directory is a git repository
pub fn is_repo(path: &Path) -> bool {
    Repository::open(path).is_ok()
}
