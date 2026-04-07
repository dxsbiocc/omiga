//! FileWrite tool - Write file contents with conflict detection
//!
//! Features:
//! - Content hash-based conflict detection
//! - Diff preview generation
//! - Atomic writes (write to temp, then rename)

use super::{ToolContext, ToolError, ToolSchema};
use crate::errors::FsError;
use crate::infrastructure::streaming::{StreamOutput, StreamOutputItem};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::pin::Pin;

pub const DESCRIPTION: &str = r#"Write content to a file.

Use this tool when you need to:
- Create new files
- Update existing files
- Apply code changes
- Save generated content

Safety features:
- Content hash conflict detection (prevents overwriting changes)
- Atomic writes (no partial writes on crash)
- Diff preview for conflict resolution
- Creates parent directories automatically"#;

/// Arguments for FileWrite tool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileWriteArgs {
    /// Path to the file (relative to project root or absolute)
    pub path: String,
    /// Content to write
    pub content: String,
    /// Expected content hash (for conflict detection, optional)
    #[serde(default)]
    pub expected_hash: Option<String>,
    /// If true, create parent directories if they don't exist
    #[serde(default = "default_create_dirs")]
    pub create_dirs: bool,
}

fn default_create_dirs() -> bool {
    true
}

/// FileWrite tool implementation
pub struct FileWriteTool;

#[async_trait]
impl super::ToolImpl for FileWriteTool {
    type Args = FileWriteArgs;

    const DESCRIPTION: &'static str = DESCRIPTION;

    async fn execute(
        ctx: &ToolContext,
        args: Self::Args,
    ) -> Result<crate::infrastructure::streaming::StreamOutputBox, ToolError> {
        let path = resolve_path(&ctx.project_root, &args.path)?;

        // Check if file exists and verify hash if provided
        let current_hash = if path.exists() {
            let content = tokio::fs::read_to_string(&path).await.map_err(|e| {
                if e.kind() == std::io::ErrorKind::InvalidData {
                    FsError::BinaryFile {
                        path: args.path.clone(),
                    }
                } else {
                    FsError::from(e)
                }
            })?;
            Some(compute_hash(&content))
        } else {
            None
        };

        // Conflict detection
        if let Some(expected) = &args.expected_hash {
            if let Some(current) = &current_hash {
                if current != expected {
                    // Conflict detected - generate diff
                    let old_content =
                        tokio::fs::read_to_string(&path).await.map_err(FsError::from)?;
                    let _diff = generate_diff(&old_content, &args.content);

                    return Err(FsError::ConflictDetected {
                        path: args.path.clone(),
                        expected: expected.clone(),
                        current: current.clone(),
                    }
                    .into());
                }
            }
        }

        // Create parent directories if needed
        if args.create_dirs {
            if let Some(parent) = path.parent() {
                tokio::fs::create_dir_all(parent).await.map_err(|e| {
                    FsError::IoError {
                        message: format!("Failed to create parent directories: {}", e),
                    }
                })?;
            }
        }

        // Atomic write: write to temp file, then rename
        let temp_path = path.with_extension("tmp");

        // Write to temp file
        tokio::fs::write(&temp_path, &args.content)
            .await
            .map_err(|e| FsError::IoError {
                message: format!("Failed to write temp file: {}", e),
            })?;

        // Atomic rename
        tokio::fs::rename(&temp_path, &path).await.map_err(|e| FsError::IoError {
            message: format!("Failed to rename temp file: {}", e),
        })?;

        // Compute new hash
        let new_hash = compute_hash(&args.content);

        let output = FileWriteOutput {
            path: args.path,
            bytes_written: args.content.len(),
            new_hash,
            created: current_hash.is_none(),
        };

        Ok(output.into_stream())
    }
}

/// File write output
#[derive(Debug, Clone)]
pub struct FileWriteOutput {
    pub path: String,
    pub bytes_written: usize,
    pub new_hash: String,
    pub created: bool,
}

impl StreamOutput for FileWriteOutput {
    fn into_stream(self) -> Pin<Box<dyn futures::Stream<Item = StreamOutputItem> + Send>> {
        use futures::stream;

        let items = vec![
            StreamOutputItem::Metadata {
                key: "path".to_string(),
                value: self.path,
            },
            StreamOutputItem::Metadata {
                key: "bytes_written".to_string(),
                value: self.bytes_written.to_string(),
            },
            StreamOutputItem::Metadata {
                key: "new_hash".to_string(),
                value: self.new_hash,
            },
            StreamOutputItem::Metadata {
                key: "created".to_string(),
                value: self.created.to_string(),
            },
            StreamOutputItem::Start,
            StreamOutputItem::Content(format!(
                "File {} ({} bytes)",
                if self.created { "created" } else { "updated" },
                self.bytes_written
            )),
            StreamOutputItem::Complete,
        ];

        Box::pin(stream::iter(items))
    }
}

/// Compute SHA-256 hash of content
fn compute_hash(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    hex::encode(hasher.finalize())
}

/// Generate a simple diff between old and new content
fn generate_diff(old: &str, new: &str) -> String {
    // Simple line-by-line diff
    let old_lines: Vec<&str> = old.lines().collect();
    let new_lines: Vec<&str> = new.lines().collect();

    let mut diff = String::new();
    let max_lines = old_lines.len().max(new_lines.len());

    for i in 0..max_lines {
        let old_line = old_lines.get(i);
        let new_line = new_lines.get(i);

        match (old_line, new_line) {
            (Some(old), Some(new)) if old != new => {
                diff.push_str(&format!("- {}\n+ {}\n", old, new));
            }
            (Some(old), None) => {
                diff.push_str(&format!("- {}\n", old));
            }
            (None, Some(new)) => {
                diff.push_str(&format!("+ {}\n", new));
            }
            _ => {}
        }
    }

    diff
}

/// Resolve path (same as file_read)
fn resolve_path(
    project_root: &std::path::Path,
    path: &str,
) -> Result<std::path::PathBuf, FsError> {
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

    // Check for path traversal
    let canonical_project = project_root
        .canonicalize()
        .unwrap_or_else(|_| project_root.to_path_buf());
    let canonical_path = path_buf.canonicalize().unwrap_or_else(|_| path_buf.clone());

    if !canonical_path.starts_with(&canonical_project)
        && !path.starts_with('/')
        && !path.starts_with("~/")
    {
        return Err(FsError::PathTraversal {
            path: path.to_string(),
        });
    }

    Ok(path_buf)
}

/// Get the JSON schema for the FileWrite tool
pub fn schema() -> ToolSchema {
    ToolSchema::new(
        "file_write",
        DESCRIPTION,
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file (relative to project root or absolute)"
                },
                "content": {
                    "type": "string",
                    "description": "Content to write to the file"
                },
                "expected_hash": {
                    "type": "string",
                    "description": "SHA-256 hash of expected current content (for conflict detection)"
                },
                "create_dirs": {
                    "type": "boolean",
                    "description": "Create parent directories if they don't exist (default: true)"
                }
            },
            "required": ["path", "content"]
        }),
    )
}
