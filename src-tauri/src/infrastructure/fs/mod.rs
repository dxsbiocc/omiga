//! File system utilities

use crate::errors::FsError;
use std::path::Path;

/// Check if a path is within the project
pub fn is_within_project(project_root: &Path, path: &Path) -> bool {
    let canonical_project = project_root.canonicalize().unwrap_or_else(|_| project_root.to_path_buf());
    let canonical_path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());

    canonical_path.starts_with(canonical_project)
}

/// Resolve a path relative to project root
pub fn resolve_path(project_root: &Path, path: &str) -> Result<std::path::PathBuf, FsError> {
    let path_buf = if path.starts_with('/') || path.starts_with("~/") {
        if path.starts_with("~/") {
            let home = std::env::var("HOME")
                .map_err(|_| FsError::InvalidPath { path: path.to_string() })?;
            std::path::PathBuf::from(path.replacen("~", &home, 1))
        } else {
            std::path::PathBuf::from(path)
        }
    } else {
        project_root.join(path)
    };

    Ok(path_buf)
}

/// File tree traversal with pagination
pub struct FileTree {
    root: std::path::PathBuf,
}

impl FileTree {
    /// Create a new file tree
    pub fn new(root: impl Into<std::path::PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// List entries in a directory (paginated)
    pub async fn list_directory(
        &self,
        relative_path: &str,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<FileEntry>, FsError> {
        let full_path = self.root.join(relative_path);

        if !full_path.exists() {
            return Err(FsError::NotFound {
                path: relative_path.to_string(),
            });
        }

        if !full_path.is_dir() {
            return Err(FsError::InvalidPath {
                path: relative_path.to_string(),
            });
        }

        let mut entries = tokio::fs::read_dir(&full_path).await.map_err(FsError::from)?;
        let mut result = Vec::new();
        let mut index = 0;

        while let Some(entry) = entries.next_entry().await.map_err(|e| FsError::IoError {
            message: e.to_string(),
        })? {
            if index >= offset && result.len() < limit {
                let metadata = entry.metadata().await.ok();
                let name = entry.file_name().to_string_lossy().to_string();
                let path = entry.path();
                let relative = path
                    .strip_prefix(&self.root)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .to_string();

                result.push(FileEntry {
                    name,
                    path: relative,
                    is_directory: metadata.as_ref().map(|m| m.is_dir()).unwrap_or(false),
                    size: metadata.as_ref().map(|m| m.len()),
                    modified: metadata
                        .as_ref()
                        .and_then(|m| m.modified().ok())
                        .map(|t| chrono::DateTime::<chrono::Local>::from(t).to_rfc3339()),
                });
            }
            index += 1;
        }

        Ok(result)
    }
}

/// A file entry
#[derive(Debug, Clone)]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub is_directory: bool,
    pub size: Option<u64>,
    pub modified: Option<String>,
}
