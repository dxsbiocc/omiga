//! Tauri commands for context snapshot management.
//! Snapshots are `.omiga/context/*.md` files written by Ralph/Team skills.

use crate::commands::CommandResult;
use crate::domain::context_snapshot::{self, ContextSnapshotMeta};

fn validate_project_root(project_root: &str) -> Result<std::path::PathBuf, String> {
    let canonical = std::path::PathBuf::from(project_root)
        .canonicalize()
        .map_err(|_| format!("project_root not accessible: {project_root}"))?;
    let home = dirs::home_dir().ok_or_else(|| "cannot determine home directory".to_string())?;
    if !canonical.starts_with(&home) {
        return Err(format!(
            "project_root must be under home directory: {project_root}"
        ));
    }
    Ok(canonical)
}

/// List all context snapshots for a project root, newest first.
#[tauri::command]
pub async fn list_context_snapshots(
    project_root: String,
) -> CommandResult<Vec<ContextSnapshotMeta>> {
    let root =
        validate_project_root(&project_root).map_err(|e| crate::errors::AppError::Unknown(e))?;
    Ok(context_snapshot::list_snapshots(&root).await)
}

/// Validate that `path` resolves to a file inside `project_root/.omiga/context/`.
fn validate_snapshot_path(path: &str, project_root: &str) -> Result<std::path::PathBuf, String> {
    let p = std::path::PathBuf::from(path);
    let allowed = std::path::PathBuf::from(project_root)
        .join(".omiga")
        .join("context");
    let canonical_p = p
        .canonicalize()
        .map_err(|_| format!("path not found: {path}"))?;
    let canonical_allowed = allowed
        .canonicalize()
        .map_err(|_| "context dir not found".to_string())?;
    if !canonical_p.starts_with(&canonical_allowed) {
        return Err(format!("path escapes context directory: {path}"));
    }
    Ok(canonical_p)
}

/// Read the full markdown content of a specific snapshot.
#[tauri::command]
pub async fn read_context_snapshot(
    path: String,
    project_root: String,
) -> CommandResult<Option<String>> {
    let safe = validate_snapshot_path(&path, &project_root)
        .map_err(|e| crate::errors::AppError::Unknown(e))?;
    Ok(context_snapshot::read_snapshot(&safe).await)
}

/// Delete a specific snapshot file.
#[tauri::command]
pub async fn delete_context_snapshot(path: String, project_root: String) -> CommandResult<bool> {
    let safe = validate_snapshot_path(&path, &project_root)
        .map_err(|e| crate::errors::AppError::Unknown(e))?;
    Ok(context_snapshot::delete_snapshot(&safe).await)
}

/// Delete all context snapshots for a project (used by cancel skill or cleanup).
#[tauri::command]
pub async fn clear_all_context_snapshots(project_root: String) -> CommandResult<usize> {
    let root =
        validate_project_root(&project_root).map_err(|e| crate::errors::AppError::Unknown(e))?;
    Ok(context_snapshot::clear_all_snapshots(&root).await)
}
