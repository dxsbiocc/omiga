//! Tauri commands for Omiga-managed tool permission deny list (`.omiga/permissions.json`).

use crate::commands::CommandResult;
use std::path::PathBuf;

/// Returns deny rules from `<project_root>/.omiga/permissions.json` only (for the Settings UI).
#[tauri::command]
pub fn get_omiga_permission_denies(project_root: String) -> Vec<String> {
    let root = PathBuf::from(project_root.trim());
    crate::domain::tool_permission_rules::read_omiga_permissions_file(&root)
}

/// Saves deny rules to `<project_root>/.omiga/permissions.json` (merged at runtime with `~/.claude` / `.claude`).
#[tauri::command]
pub fn save_omiga_permission_denies(
    project_root: String,
    deny: Vec<String>,
) -> CommandResult<()> {
    let root = PathBuf::from(project_root.trim());
    crate::domain::tool_permission_rules::write_omiga_permissions_file(&root, &deny)
        .map_err(|e| crate::errors::AppError::Config(e))
}
