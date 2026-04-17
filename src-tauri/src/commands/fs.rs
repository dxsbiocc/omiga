//! File system commands

use super::CommandResult;
use crate::errors::{AppError, FsError};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use serde::Serialize;
use std::path::PathBuf;

/// Read file as raw bytes - fastest way to get content for Monaco
#[tauri::command]
pub async fn read_file_bytes_fast(path: String) -> CommandResult<Vec<u8>> {
    let path_buf = PathBuf::from(&path);
    let canonical = path_buf.canonicalize().map_err(|e| {
        AppError::Fs(FsError::IoError {
            message: format!("{}: {}", path, e),
        })
    })?;

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
#[tauri::command]
pub async fn read_file(
    path: String,
    offset: Option<usize>,
    limit: Option<usize>,
) -> CommandResult<FileReadResponse> {
    let path_buf = PathBuf::from(&path);
    let canonical = path_buf.canonicalize().map_err(|e| {
        AppError::Fs(FsError::IoError {
            message: format!("{}: {}", path, e),
        })
    })?;

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
#[tauri::command]
pub async fn write_file(
    path: String,
    content: String,
    _expected_hash: Option<String>,
) -> CommandResult<FileWriteResponse> {
    let path_buf = PathBuf::from(&path);
    // Require the file to already exist (no creating arbitrary paths)
    let canonical = path_buf.canonicalize().map_err(|e| {
        AppError::Fs(FsError::IoError {
            message: format!("{}: {}", path, e),
        })
    })?;
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
#[tauri::command]
pub async fn read_image_base64(path: String) -> CommandResult<ImageReadResponse> {
    let path_buf = PathBuf::from(&path);
    let canonical = path_buf.canonicalize().map_err(|e| {
        AppError::Fs(FsError::IoError {
            message: format!("{}: {}", path, e),
        })
    })?;
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
#[tauri::command]
pub async fn list_directory(
    path: String,
    _offset: Option<usize>,
    _limit: Option<usize>,
) -> CommandResult<DirectoryListResponse> {
    let path_buf = PathBuf::from(&path);
    let canonical = path_buf.canonicalize().map_err(|e| {
        AppError::Fs(FsError::IoError {
            message: format!("{}: {}", path, e),
        })
    })?;

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
