//! File system commands

use super::CommandResult;
use crate::app_state::OmigaAppState;
use crate::errors::{AppError, FsError};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use serde::Serialize;
use std::{
    ffi::OsString,
    path::{Component, Path, PathBuf},
};
use tauri::State;

/// Read file as raw bytes - fastest way to get content for Monaco
pub async fn read_file_bytes_scoped(
    path: String,
    workspace_root: Option<String>,
) -> CommandResult<Vec<u8>> {
    let canonical = canonicalize_scoped_read_target(&path, workspace_root.as_deref())?;

    if !canonical.is_file() {
        return Err(AppError::Fs(FsError::InvalidPath { path: path.clone() }));
    }

    let meta = tokio::fs::metadata(&canonical)
        .await
        .map_err(|e: std::io::Error| AppError::Fs(FsError::from(e)))?;

    const MAX_BYTES: u64 = 10 * 1024 * 1024; // 10 MB limit
    if meta.len() > MAX_BYTES {
        return Err(AppError::Fs(FsError::FileTooLarge {
            path: path.clone(),
            size: meta.len(),
            max: MAX_BYTES,
        }));
    }

    let bytes = tokio::fs::read(&canonical)
        .await
        .map_err(|e| AppError::Fs(FsError::from(e)))?;

    Ok(bytes)
}

/// Read a file with optional line-based pagination (`offset` / `limit` lines).
pub async fn read_file_scoped(
    path: String,
    offset: Option<usize>,
    limit: Option<usize>,
    workspace_root: Option<String>,
) -> CommandResult<FileReadResponse> {
    let canonical = canonicalize_scoped_read_target(&path, workspace_root.as_deref())?;

    if !canonical.is_file() {
        return Err(AppError::Fs(FsError::InvalidPath { path: path.clone() }));
    }

    let meta = tokio::fs::metadata(&canonical)
        .await
        .map_err(|e: std::io::Error| AppError::Fs(FsError::from(e)))?;

    const MAX_BYTES: u64 = 2 * 1024 * 1024;
    if meta.len() > MAX_BYTES {
        return Err(AppError::Fs(FsError::FileTooLarge {
            path: path.clone(),
            size: meta.len(),
            max: MAX_BYTES,
        }));
    }

    let full = tokio::fs::read_to_string(&canonical)
        .await
        .map_err(|e: std::io::Error| AppError::Fs(FsError::from(e)))?;

    let lines: Vec<&str> = full.lines().collect();
    let total_lines = lines.len();
    let off = offset.unwrap_or(0);
    let lim = limit.unwrap_or(usize::MAX);
    let slice: Vec<&str> = lines.iter().skip(off).take(lim).copied().collect();
    let content = slice.join("\n");
    let returned = slice.len();
    let has_more = off + returned < total_lines;

    Ok(FileReadResponse {
        content,
        total_lines,
        has_more,
    })
}

/// Write a file
pub async fn write_file_scoped(
    path: String,
    content: String,
    _expected_hash: Option<String>,
    workspace_root: String,
) -> CommandResult<FileWriteResponse> {
    let workspace_root = canonicalize_mutation_scope(&workspace_root)?;
    // Require the file to already exist and stay inside the selected workspace.
    let canonical = canonicalize_existing_mutation_target(&path, &workspace_root)?;
    if !canonical.is_file() {
        return Err(AppError::Fs(FsError::InvalidPath { path: path.clone() }));
    }
    let bytes = content.as_bytes();
    tokio::fs::write(&canonical, bytes)
        .await
        .map_err(|e| AppError::Fs(FsError::from(e)))?;
    Ok(FileWriteResponse {
        bytes_written: bytes.len(),
        new_hash: String::new(),
    })
}

/// Read an image file and return it as a base64-encoded data URL.
/// Supports common raster/vector formats; rejects files larger than 20 MB.
pub async fn read_image_base64_scoped(
    path: String,
    workspace_root: Option<String>,
) -> CommandResult<ImageReadResponse> {
    let canonical = canonicalize_scoped_read_target(&path, workspace_root.as_deref())?;
    if !canonical.is_file() {
        return Err(AppError::Fs(FsError::InvalidPath { path: path.clone() }));
    }

    let meta = tokio::fs::metadata(&canonical)
        .await
        .map_err(|e| AppError::Fs(FsError::from(e)))?;

    const MAX_BYTES: u64 = 20 * 1024 * 1024; // 20 MB
    if meta.len() > MAX_BYTES {
        return Err(AppError::Fs(FsError::FileTooLarge {
            path: path.clone(),
            size: meta.len(),
            max: MAX_BYTES,
        }));
    }

    let bytes = tokio::fs::read(&canonical)
        .await
        .map_err(|e| AppError::Fs(FsError::from(e)))?;

    let ext = canonical
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    let mime = match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "bmp" => "image/bmp",
        "ico" => "image/x-icon",
        "tiff" | "tif" => "image/tiff",
        "avif" => "image/avif",
        _ => "application/octet-stream",
    };

    let data = BASE64.encode(&bytes);
    Ok(ImageReadResponse {
        data,
        mime_type: mime.to_string(),
    })
}

#[derive(Debug, Serialize)]
pub struct ImageReadResponse {
    pub data: String,
    pub mime_type: String,
}

/// Absolute path to the Rust agent tool sources (`src/domain/tools`).
#[tauri::command]
pub fn agent_tools_directory() -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src/domain/tools")
        .to_string_lossy()
        .into_owned()
}

/// List directory contents
pub async fn list_directory_scoped(
    path: String,
    _offset: Option<usize>,
    _limit: Option<usize>,
    workspace_root: Option<String>,
) -> CommandResult<DirectoryListResponse> {
    let canonical = canonicalize_scoped_read_target(&path, workspace_root.as_deref())?;

    if !canonical.is_dir() {
        return Err(AppError::Fs(FsError::InvalidPath { path: path.clone() }));
    }

    let mut read_dir = tokio::fs::read_dir(&canonical).await.map_err(|e| {
        AppError::Fs(FsError::IoError {
            message: e.to_string(),
        })
    })?;

    let mut entries = Vec::new();
    while let Some(entry) = read_dir.next_entry().await.map_err(|e| {
        AppError::Fs(FsError::IoError {
            message: e.to_string(),
        })
    })? {
        let meta = entry.metadata().await.map_err(|e| {
            AppError::Fs(FsError::IoError {
                message: e.to_string(),
            })
        })?;
        let name = entry.file_name().to_string_lossy().to_string();
        let full = entry.path();
        let path_str = full.to_string_lossy().to_string();
        let modified = meta.modified().ok().map(|st| {
            let dt: chrono::DateTime<chrono::Utc> = st.into();
            dt.to_rfc3339()
        });
        entries.push(DirectoryEntry {
            name,
            path: path_str,
            is_directory: meta.is_dir(),
            size: if meta.is_file() {
                Some(meta.len())
            } else {
                None
            },
            modified,
        });
    }

    entries.sort_by(|a, b| match (a.is_directory, b.is_directory) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
    });

    let total = entries.len();
    let directory = canonical.to_string_lossy().into_owned();
    Ok(DirectoryListResponse {
        entries,
        total,
        has_more: false,
        directory,
    })
}

/// Response from read_file
#[derive(Debug, Serialize)]
pub struct FileReadResponse {
    pub content: String,
    pub total_lines: usize,
    pub has_more: bool,
}

/// Response from write_file
#[derive(Debug, Serialize)]
pub struct FileWriteResponse {
    pub bytes_written: usize,
    pub new_hash: String,
}

/// Directory entry
#[derive(Debug, Serialize)]
pub struct DirectoryEntry {
    pub name: String,
    pub path: String,
    pub is_directory: bool,
    pub size: Option<u64>,
    /// RFC3339 timestamp from `metadata.modified()`, when available.
    pub modified: Option<String>,
}

/// Response from list_directory
#[derive(Debug, Serialize)]
pub struct DirectoryListResponse {
    /// Canonical absolute path of the listed directory (same as `path` after resolve).
    pub directory: String,
    pub entries: Vec<DirectoryEntry>,
    pub total: usize,
    pub has_more: bool,
}

async fn workspace_root_from_session(
    state: &OmigaAppState,
    session_id: &str,
) -> CommandResult<String> {
    let trimmed = session_id.trim();
    if trimmed.is_empty() {
        return Err(fs_io_error("Session id must not be empty"));
    }
    let session = state
        .repo
        .get_session_meta(trimmed)
        .await
        .map_err(|e| AppError::Persistence(format!("Failed to load session: {}", e)))?
        .ok_or_else(|| AppError::NotFound {
            resource: format!("Session {}", trimmed),
        })?;
    let root = session.project_path.trim();
    if root.is_empty() || root == "." {
        return Err(fs_io_error(format!(
            "Session '{}' does not have a local workspace root",
            trimmed
        )));
    }
    Ok(root.to_string())
}

fn assert_trusted_unscoped_root(canonical_root: &Path, original: &str) -> CommandResult<()> {
    for trusted in trusted_unscoped_read_roots() {
        if canonical_root == trusted || canonical_root.starts_with(&trusted) {
            return Ok(());
        }
    }
    Err(fs_io_error(format!(
        "Workspace root '{}' requires a valid session id",
        original
    )))
}

async fn resolve_read_workspace_root(
    state: &OmigaAppState,
    session_id: Option<String>,
    workspace_root: Option<String>,
) -> CommandResult<Option<String>> {
    if let Some(session_id) = session_id
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        return workspace_root_from_session(state, session_id)
            .await
            .map(Some);
    }

    if let Some(root) = workspace_root
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        let canonical = canonicalize_read_scope(root)?;
        assert_trusted_unscoped_root(&canonical, root)?;
        return Ok(Some(root.to_string()));
    }

    Ok(None)
}

async fn resolve_mutation_workspace_root(
    state: &OmigaAppState,
    session_id: String,
) -> CommandResult<String> {
    workspace_root_from_session(state, &session_id).await
}

#[tauri::command]
pub async fn read_file_bytes(
    path: String,
    session_id: Option<String>,
    workspace_root: Option<String>,
    state: State<'_, OmigaAppState>,
) -> CommandResult<Vec<u8>> {
    let workspace_root = resolve_read_workspace_root(&state, session_id, workspace_root).await?;
    read_file_bytes_scoped(path, workspace_root).await
}

#[tauri::command]
pub async fn read_file_bytes_fast(
    path: String,
    session_id: Option<String>,
    workspace_root: Option<String>,
    state: State<'_, OmigaAppState>,
) -> CommandResult<Vec<u8>> {
    let workspace_root = resolve_read_workspace_root(&state, session_id, workspace_root).await?;
    read_file_bytes_scoped(path, workspace_root).await
}

#[tauri::command]
pub async fn read_file(
    path: String,
    offset: Option<usize>,
    limit: Option<usize>,
    session_id: Option<String>,
    workspace_root: Option<String>,
    state: State<'_, OmigaAppState>,
) -> CommandResult<FileReadResponse> {
    let workspace_root = resolve_read_workspace_root(&state, session_id, workspace_root).await?;
    read_file_scoped(path, offset, limit, workspace_root).await
}

#[tauri::command]
pub async fn write_file(
    path: String,
    content: String,
    expected_hash: Option<String>,
    session_id: String,
    state: State<'_, OmigaAppState>,
) -> CommandResult<FileWriteResponse> {
    let workspace_root = resolve_mutation_workspace_root(&state, session_id).await?;
    write_file_scoped(path, content, expected_hash, workspace_root).await
}

#[tauri::command]
pub async fn read_image_base64(
    path: String,
    session_id: Option<String>,
    workspace_root: Option<String>,
    state: State<'_, OmigaAppState>,
) -> CommandResult<ImageReadResponse> {
    let workspace_root = resolve_read_workspace_root(&state, session_id, workspace_root).await?;
    read_image_base64_scoped(path, workspace_root).await
}

#[tauri::command]
pub async fn list_directory(
    path: String,
    offset: Option<usize>,
    limit: Option<usize>,
    session_id: Option<String>,
    workspace_root: Option<String>,
    state: State<'_, OmigaAppState>,
) -> CommandResult<DirectoryListResponse> {
    let workspace_root = resolve_read_workspace_root(&state, session_id, workspace_root).await?;
    list_directory_scoped(path, offset, limit, workspace_root).await
}

// ─── Filesystem mutation commands ─────────────────────────────────────────────

fn fs_io_error(message: impl Into<String>) -> AppError {
    AppError::Fs(FsError::IoError {
        message: message.into(),
    })
}

fn normal_component_count(path: &Path) -> usize {
    path.components()
        .filter(|component| matches!(component, Component::Normal(_)))
        .count()
}

fn contains_parent_dir(path: &Path) -> bool {
    path.components()
        .any(|component| matches!(component, Component::ParentDir))
}

fn project_root() -> Option<PathBuf> {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
}

fn protected_mutation_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(home) = dirs::home_dir() {
        if let Ok(canonical) = home.canonicalize() {
            roots.push(canonical);
        }
    }
    if let Some(project) = project_root() {
        if let Ok(canonical) = project.canonicalize() {
            roots.push(canonical);
        }
    }
    roots
}

fn push_canonical_if_dir(roots: &mut Vec<PathBuf>, path: PathBuf) {
    if let Ok(canonical) = path.canonicalize() {
        if canonical.is_dir() && !roots.iter().any(|root| root == &canonical) {
            roots.push(canonical);
        }
    }
}

fn trusted_unscoped_read_roots() -> Vec<PathBuf> {
    trusted_unscoped_read_roots_from(dirs::home_dir(), dirs::data_dir(), dirs::cache_dir())
}

fn trusted_unscoped_read_roots_from(
    home_dir: Option<PathBuf>,
    data_dir: Option<PathBuf>,
    cache_dir: Option<PathBuf>,
) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(home) = home_dir {
        // Skill previews may read installed skill bundles, but not the whole
        // ~/.codex / ~/.omiga trees where sessions and credentials can live.
        push_canonical_if_dir(&mut roots, home.join(".codex").join("skills"));
        push_canonical_if_dir(&mut roots, home.join(".codex").join("plugins"));
        push_canonical_if_dir(&mut roots, home.join(".omiga").join("skills"));
    }
    if let Some(data_dir) = data_dir {
        // Background agent output is written under the Tauri app data dir.
        for app_dir in ["com.omiga.desktop", "com.omiga.app", "Omiga", "omiga"] {
            push_canonical_if_dir(&mut roots, data_dir.join(app_dir));
        }
    }
    if let Some(cache_dir) = cache_dir {
        for app_dir in ["com.omiga.desktop", "com.omiga.app", "Omiga", "omiga"] {
            push_canonical_if_dir(&mut roots, cache_dir.join(app_dir));
        }
    }
    roots
}

fn validate_read_path_shape(path: &str, p: &Path) -> CommandResult<()> {
    validate_mutation_path_shape(path, p)
}

fn canonicalize_read_scope(workspace_root: &str) -> CommandResult<PathBuf> {
    canonicalize_mutation_scope(workspace_root)
}

fn ensure_read_target_in_scope(
    canonical: &Path,
    workspace_root: Option<&Path>,
    original: &str,
) -> CommandResult<()> {
    if let Some(root) = workspace_root {
        if canonical == root || canonical.starts_with(root) {
            return Ok(());
        }
        return Err(fs_io_error(format!(
            "Filesystem read '{}' is outside workspace root '{}'",
            original,
            root.to_string_lossy()
        )));
    }

    for root in trusted_unscoped_read_roots() {
        if canonical == root || canonical.starts_with(&root) {
            return Ok(());
        }
    }

    Err(fs_io_error(format!(
        "Filesystem read '{}' requires a workspace root",
        original
    )))
}

fn canonicalize_scoped_read_target(
    path: &str,
    workspace_root: Option<&str>,
) -> CommandResult<PathBuf> {
    let p = PathBuf::from(path);
    validate_read_path_shape(path, &p)?;
    let canonical = p
        .canonicalize()
        .map_err(|e| fs_io_error(format!("Could not resolve read target '{}': {}", path, e)))?;
    if canonical.parent().is_none() || normal_component_count(&canonical) < 2 {
        return Err(fs_io_error(format!(
            "Refusing to read high-level filesystem path '{}'",
            path
        )));
    }

    let root = match workspace_root.and_then(|root| {
        let trimmed = root.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    }) {
        Some(root) => Some(canonicalize_read_scope(root)?),
        None => None,
    };
    ensure_read_target_in_scope(&canonical, root.as_deref(), path)?;
    Ok(canonical)
}

fn canonicalize_mutation_scope(workspace_root: &str) -> CommandResult<PathBuf> {
    let root = PathBuf::from(workspace_root);
    validate_mutation_path_shape(workspace_root, &root)?;
    let canonical = root.canonicalize().map_err(|e| {
        fs_io_error(format!(
            "Could not resolve workspace root '{}': {}",
            workspace_root, e
        ))
    })?;
    if !canonical.is_dir() {
        return Err(fs_io_error(format!(
            "Workspace root '{}' is not a directory",
            workspace_root
        )));
    }
    if let Some(home) = dirs::home_dir().and_then(|path| path.canonicalize().ok()) {
        if canonical == home {
            return Err(fs_io_error(format!(
                "Refusing to use home directory '{}' as a mutation root",
                workspace_root
            )));
        }
    }
    Ok(canonical)
}

fn reject_protected_mutation_target(path: &Path, original: &str) -> CommandResult<()> {
    if path.parent().is_none() || normal_component_count(path) < 2 {
        return Err(fs_io_error(format!(
            "Refusing to mutate high-level filesystem path '{}'",
            original
        )));
    }

    for protected in protected_mutation_roots() {
        if path == protected {
            return Err(fs_io_error(format!(
                "Refusing to mutate protected directory '{}'",
                original
            )));
        }
    }
    Ok(())
}

fn validate_mutation_path_shape(path: &str, p: &Path) -> CommandResult<()> {
    if path.trim().is_empty() {
        return Err(fs_io_error("Path must not be empty"));
    }
    if !p.is_absolute() {
        return Err(fs_io_error(format!(
            "Filesystem mutations require an absolute path: '{}'",
            path
        )));
    }
    if contains_parent_dir(p) {
        return Err(fs_io_error(format!(
            "Filesystem mutation paths must not contain '..': '{}'",
            path
        )));
    }
    if normal_component_count(p) < 2 {
        return Err(fs_io_error(format!(
            "Refusing to mutate high-level filesystem path '{}'",
            path
        )));
    }
    Ok(())
}

fn reject_outside_mutation_scope(
    path: &Path,
    workspace_root: &Path,
    original: &str,
) -> CommandResult<()> {
    if path == workspace_root {
        return Err(fs_io_error(format!(
            "Refusing to mutate workspace root '{}'",
            original
        )));
    }
    if !path.starts_with(workspace_root) {
        return Err(fs_io_error(format!(
            "Filesystem mutation '{}' is outside workspace root '{}'",
            original,
            workspace_root.to_string_lossy()
        )));
    }
    Ok(())
}

fn canonicalize_existing_mutation_target(
    path: &str,
    workspace_root: &Path,
) -> CommandResult<PathBuf> {
    let p = PathBuf::from(path);
    validate_mutation_path_shape(path, &p)?;
    let canonical = p.canonicalize().map_err(|e| fs_io_error(e.to_string()))?;
    reject_outside_mutation_scope(&canonical, workspace_root, path)?;
    reject_protected_mutation_target(&canonical, path)?;
    Ok(canonical)
}

fn resolve_mutation_target(path: &str, workspace_root: &Path) -> CommandResult<PathBuf> {
    let p = PathBuf::from(path);
    validate_mutation_path_shape(path, &p)?;

    if p.exists() {
        return canonicalize_existing_mutation_target(path, workspace_root);
    }

    let mut cursor = p.as_path();
    let mut suffix: Vec<OsString> = Vec::new();
    while !cursor.exists() {
        let name = cursor.file_name().ok_or_else(|| {
            fs_io_error(format!(
                "Could not resolve parent directory for mutation path '{}'",
                path
            ))
        })?;
        suffix.push(name.to_os_string());
        cursor = cursor.parent().ok_or_else(|| {
            fs_io_error(format!(
                "Could not resolve parent directory for mutation path '{}'",
                path
            ))
        })?;
    }

    let mut resolved = cursor
        .canonicalize()
        .map_err(|e| fs_io_error(e.to_string()))?;
    for part in suffix.iter().rev() {
        resolved.push(part);
    }
    reject_outside_mutation_scope(&resolved, workspace_root, path)?;
    reject_protected_mutation_target(&resolved, path)?;
    Ok(resolved)
}

/// Create a new empty directory (all intermediate parent directories are created).
pub async fn create_directory_scoped(
    path: String,
    workspace_root: String,
) -> CommandResult<String> {
    let workspace_root = canonicalize_mutation_scope(&workspace_root)?;
    let target = resolve_mutation_target(&path, &workspace_root)?;
    tokio::fs::create_dir_all(&target)
        .await
        .map_err(|e| fs_io_error(e.to_string()))?;
    let canonical = canonicalize_existing_mutation_target(&path, &workspace_root)?;
    Ok(canonical.to_string_lossy().into_owned())
}

/// Create a new empty file. Fails if the file already exists.
pub async fn create_file_scoped(path: String, workspace_root: String) -> CommandResult<String> {
    let workspace_root = canonicalize_mutation_scope(&workspace_root)?;
    let target = resolve_mutation_target(&path, &workspace_root)?;
    if target.exists() {
        return Err(fs_io_error(format!("'{}' already exists", path)));
    }
    if let Some(parent) = target.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| fs_io_error(e.to_string()))?;
    }
    tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&target)
        .await
        .map_err(|e| fs_io_error(e.to_string()))?;
    let canonical = canonicalize_existing_mutation_target(&path, &workspace_root)?;
    Ok(canonical.to_string_lossy().into_owned())
}

/// Delete a file or directory. Directories are removed recursively.
pub async fn delete_fs_entry_scoped(path: String, workspace_root: String) -> CommandResult<()> {
    let workspace_root = canonicalize_mutation_scope(&workspace_root)?;
    let target = canonicalize_existing_mutation_target(&path, &workspace_root)?;
    if target.is_dir() {
        tokio::fs::remove_dir_all(&target)
            .await
            .map_err(|e| fs_io_error(e.to_string()))?;
    } else {
        tokio::fs::remove_file(&target)
            .await
            .map_err(|e| fs_io_error(e.to_string()))?;
    }
    Ok(())
}

/// Rename (or move) a filesystem entry. `to_path` must not already exist.
pub async fn rename_fs_entry_scoped(
    from_path: String,
    to_path: String,
    workspace_root: String,
) -> CommandResult<String> {
    let workspace_root = canonicalize_mutation_scope(&workspace_root)?;
    let from = canonicalize_existing_mutation_target(&from_path, &workspace_root)?;
    let to = resolve_mutation_target(&to_path, &workspace_root)?;
    if to.exists() {
        return Err(fs_io_error(format!("'{}' already exists", to_path)));
    }
    if let Some(parent) = to.parent() {
        if !parent.exists() {
            return Err(fs_io_error(format!(
                "Parent directory '{}' does not exist",
                parent.to_string_lossy()
            )));
        }
    }
    tokio::fs::rename(&from, &to)
        .await
        .map_err(|e| fs_io_error(e.to_string()))?;
    let canonical = to.canonicalize().map_err(|e| fs_io_error(e.to_string()))?;
    Ok(canonical.to_string_lossy().into_owned())
}

#[tauri::command]
pub async fn create_directory(
    path: String,
    session_id: String,
    state: State<'_, OmigaAppState>,
) -> CommandResult<String> {
    let workspace_root = resolve_mutation_workspace_root(&state, session_id).await?;
    create_directory_scoped(path, workspace_root).await
}

#[tauri::command]
pub async fn create_file(
    path: String,
    session_id: String,
    state: State<'_, OmigaAppState>,
) -> CommandResult<String> {
    let workspace_root = resolve_mutation_workspace_root(&state, session_id).await?;
    create_file_scoped(path, workspace_root).await
}

#[tauri::command]
pub async fn delete_fs_entry(
    path: String,
    session_id: String,
    state: State<'_, OmigaAppState>,
) -> CommandResult<()> {
    let workspace_root = resolve_mutation_workspace_root(&state, session_id).await?;
    delete_fs_entry_scoped(path, workspace_root).await
}

#[tauri::command]
pub async fn rename_fs_entry(
    from_path: String,
    to_path: String,
    session_id: String,
    state: State<'_, OmigaAppState>,
) -> CommandResult<String> {
    let workspace_root = resolve_mutation_workspace_root(&state, session_id).await?;
    rename_fs_entry_scoped(from_path, to_path, workspace_root).await
}

// ─── Local file viewer ────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
#[serde(tag = "kind")]
pub enum LocalFileViewResponse {
    #[serde(rename = "html")]
    Html { content: String },
    #[serde(rename = "image")]
    Image { data_uri: String, mime: String },
    #[serde(rename = "pdf")]
    Pdf { data_uri: String },
    #[serde(rename = "unsupported")]
    Unsupported { ext: String },
}

/// Read a local file for inline rendering in the chat UI.
///
/// - HTML  → returns text content (rendered in sandboxed iframe)
/// - Images (PNG/JPG/GIF/WebP/SVG/BMP/ICO) → returns base64 data URI
/// - PDF   → returns base64 data URI for iframe embed
///
/// Size caps: HTML 1 MB · images 20 MB · PDF 50 MB.
pub async fn read_local_file_for_view_scoped(
    path: String,
    workspace_root: Option<String>,
) -> CommandResult<LocalFileViewResponse> {
    let canonical = canonicalize_scoped_read_target(&path, workspace_root.as_deref())?;

    if !canonical.is_file() {
        return Err(AppError::Fs(FsError::InvalidPath { path: path.clone() }));
    }

    let ext = canonical
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    let meta = tokio::fs::metadata(&canonical)
        .await
        .map_err(|e: std::io::Error| AppError::Fs(FsError::from(e)))?;

    match ext.as_str() {
        "html" | "htm" => {
            const MAX: u64 = 1024 * 1024;
            if meta.len() > MAX {
                return Err(AppError::Fs(FsError::FileTooLarge {
                    path: path.clone(),
                    size: meta.len(),
                    max: MAX,
                }));
            }
            let content = tokio::fs::read_to_string(&canonical)
                .await
                .map_err(|e: std::io::Error| AppError::Fs(FsError::from(e)))?;
            Ok(LocalFileViewResponse::Html { content })
        }

        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "ico" => {
            const MAX: u64 = 20 * 1024 * 1024;
            if meta.len() > MAX {
                return Err(AppError::Fs(FsError::FileTooLarge {
                    path: path.clone(),
                    size: meta.len(),
                    max: MAX,
                }));
            }
            let bytes = tokio::fs::read(&canonical)
                .await
                .map_err(|e: std::io::Error| AppError::Fs(FsError::from(e)))?;
            let mime = match ext.as_str() {
                "jpg" | "jpeg" => "image/jpeg",
                "png" => "image/png",
                "gif" => "image/gif",
                "webp" => "image/webp",
                "bmp" => "image/bmp",
                "ico" => "image/x-icon",
                _ => "image/png",
            };
            let data_uri = format!("data:{};base64,{}", mime, BASE64.encode(&bytes));
            Ok(LocalFileViewResponse::Image {
                data_uri,
                mime: mime.to_string(),
            })
        }

        "svg" => {
            const MAX: u64 = 2 * 1024 * 1024;
            if meta.len() > MAX {
                return Err(AppError::Fs(FsError::FileTooLarge {
                    path: path.clone(),
                    size: meta.len(),
                    max: MAX,
                }));
            }
            let bytes = tokio::fs::read(&canonical)
                .await
                .map_err(|e: std::io::Error| AppError::Fs(FsError::from(e)))?;
            let data_uri = format!("data:image/svg+xml;base64,{}", BASE64.encode(&bytes));
            Ok(LocalFileViewResponse::Image {
                data_uri,
                mime: "image/svg+xml".to_string(),
            })
        }

        "pdf" => {
            const MAX: u64 = 50 * 1024 * 1024;
            if meta.len() > MAX {
                return Err(AppError::Fs(FsError::FileTooLarge {
                    path: path.clone(),
                    size: meta.len(),
                    max: MAX,
                }));
            }
            let bytes = tokio::fs::read(&canonical)
                .await
                .map_err(|e: std::io::Error| AppError::Fs(FsError::from(e)))?;
            let data_uri = format!("data:application/pdf;base64,{}", BASE64.encode(&bytes));
            Ok(LocalFileViewResponse::Pdf { data_uri })
        }

        _ => Ok(LocalFileViewResponse::Unsupported { ext }),
    }
}

#[tauri::command]
pub async fn read_local_file_for_view(
    path: String,
    session_id: Option<String>,
    workspace_root: Option<String>,
    state: State<'_, OmigaAppState>,
) -> CommandResult<LocalFileViewResponse> {
    let workspace_root = resolve_read_workspace_root(&state, session_id, workspace_root).await?;
    read_local_file_for_view_scoped(path, workspace_root).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trusted_unscoped_read_roots_include_user_omiga_skills() {
        let home = tempfile::tempdir().expect("temp home");
        let skills = home.path().join(".omiga").join("skills");
        std::fs::create_dir_all(&skills).expect("create skills root");
        let canonical_skills = skills.canonicalize().expect("canonical skills root");

        let roots = trusted_unscoped_read_roots_from(Some(home.path().to_path_buf()), None, None);

        assert!(
            roots.iter().any(|root| root == &canonical_skills),
            "expected trusted roots to include user Omiga skills root {canonical_skills:?}; got {roots:?}",
        );
    }
}
