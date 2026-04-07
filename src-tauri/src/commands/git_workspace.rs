//! Git workspace info for the chat composer (branch list, current branch).

use super::CommandResult;
use crate::errors::AppError;
use git2::{BranchType, Repository};
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitWorkspaceInfo {
    pub is_git: bool,
    pub current_branch: String,
    pub branches: Vec<String>,
    /// Absolute or resolved path for display
    pub display_path: String,
}

/// Returns branch list and current branch for a workspace directory. Non-git or missing dirs yield `is_git: false`.
#[tauri::command]
pub fn git_workspace_info(path: String) -> CommandResult<GitWorkspaceInfo> {
    let path = path.trim();
    if path.is_empty() {
        return Ok(GitWorkspaceInfo {
            is_git: false,
            current_branch: String::new(),
            branches: Vec::new(),
            display_path: String::new(),
        });
    }

    let p = Path::new(path);
    if !p.exists() {
        return Ok(GitWorkspaceInfo {
            is_git: false,
            current_branch: String::new(),
            branches: Vec::new(),
            display_path: path.to_string(),
        });
    }

    let display_path = p
        .canonicalize()
        .unwrap_or_else(|_| p.to_path_buf())
        .display()
        .to_string();

    let repo = match Repository::open(p) {
        Ok(r) => r,
        Err(_) => {
            return Ok(GitWorkspaceInfo {
                is_git: false,
                current_branch: String::new(),
                branches: Vec::new(),
                display_path,
            });
        }
    };

    let head = repo.head().map_err(|e| {
        AppError::Unknown(format!("git head: {}", e))
    })?;
    let current_branch = head
        .shorthand()
        .map(|s| s.to_string())
        .unwrap_or_else(|| "HEAD".to_string());

    let mut branches = Vec::new();
    let br_iter = repo.branches(Some(BranchType::Local)).map_err(|e| {
        AppError::Unknown(format!("git branches: {}", e))
    })?;
    for br in br_iter {
        let (branch, _) = br.map_err(|e| AppError::Unknown(format!("git branch: {}", e)))?;
        if let Ok(Some(name)) = branch.name() {
            branches.push(name.to_string());
        }
    }
    branches.sort();

    Ok(GitWorkspaceInfo {
        is_git: true,
        current_branch,
        branches,
        display_path,
    })
}
