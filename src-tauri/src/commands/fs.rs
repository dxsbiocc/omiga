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

// ─── Filesystem mutation commands ─────────────────────────────────────────────

/// Create a new empty directory (all intermediate parent directories are created).
#[tauri::command]
pub async fn create_directory(path: String) -> CommandResult<String> {
    let p = PathBuf::from(&path);
    tokio::fs::create_dir_all(&p)
        .await
        .map_err(|e| AppError::Fs(FsError::IoError { message: e.to_string() }))?;
    let canonical = p
        .canonicalize()
        .map_err(|e| AppError::Fs(FsError::IoError { message: e.to_string() }))?;
    Ok(canonical.to_string_lossy().into_owned())
}

/// Create a new empty file. Fails if the file already exists.
#[tauri::command]
pub async fn create_file(path: String) -> CommandResult<String> {
    let p = PathBuf::from(&path);
    if p.exists() {
        return Err(AppError::Fs(FsError::IoError {
            message: format!("'{}' already exists", path),
        }));
    }
    // Create parent directories if missing.
    if let Some(parent) = p.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| AppError::Fs(FsError::IoError { message: e.to_string() }))?;
        }
    }
    tokio::fs::File::create(&p)
        .await
        .map_err(|e| AppError::Fs(FsError::IoError { message: e.to_string() }))?;
    let canonical = p
        .canonicalize()
        .map_err(|e| AppError::Fs(FsError::IoError { message: e.to_string() }))?;
    Ok(canonical.to_string_lossy().into_owned())
}

/// Delete a file or directory. Directories are removed recursively.
#[tauri::command]
pub async fn delete_fs_entry(path: String) -> CommandResult<()> {
    let p = PathBuf::from(&path);
    if !p.exists() {
        return Err(AppError::Fs(FsError::IoError {
            message: format!("'{}' does not exist", path),
        }));
    }
    if p.is_dir() {
        tokio::fs::remove_dir_all(&p)
            .await
            .map_err(|e| AppError::Fs(FsError::IoError { message: e.to_string() }))?;
    } else {
        tokio::fs::remove_file(&p)
            .await
            .map_err(|e| AppError::Fs(FsError::IoError { message: e.to_string() }))?;
    }
    Ok(())
}

/// Rename (or move) a filesystem entry. `to_path` must not already exist.
#[tauri::command]
pub async fn rename_fs_entry(from_path: String, to_path: String) -> CommandResult<String> {
    let from = PathBuf::from(&from_path);
    let to = PathBuf::from(&to_path);
    if !from.exists() {
        return Err(AppError::Fs(FsError::IoError {
            message: format!("'{}' does not exist", from_path),
        }));
    }
    if to.exists() {
        return Err(AppError::Fs(FsError::IoError {
            message: format!("'{}' already exists", to_path),
        }));
    }
    tokio::fs::rename(&from, &to)
        .await
        .map_err(|e| AppError::Fs(FsError::IoError { message: e.to_string() }))?;
    let canonical = to
        .canonicalize()
        .map_err(|e| AppError::Fs(FsError::IoError { message: e.to_string() }))?;
    Ok(canonical.to_string_lossy().into_owned())
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
#[tauri::command]
pub async fn read_local_file_for_view(path: String) -> CommandResult<LocalFileViewResponse> {
    let path_buf = PathBuf::from(&path);
    let canonical = path_buf.canonicalize().map_err(|e| {
        AppError::Fs(FsError::IoError {
            message: format!("{}: {}", path, e),
        })
    })?;

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
            const MAX: u64 = 1 * 1024 * 1024;
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
