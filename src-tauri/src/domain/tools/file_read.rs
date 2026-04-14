//! FileRead tool - Read file contents with pagination support
//!
//! Features:
//! - Pagination for large files (offset/limit)
//! - Binary file detection
//! - Line number preservation

use super::{ToolContext, ToolError, ToolSchema};
use crate::errors::FsError;
use crate::infrastructure::streaming::{StreamOutput, StreamOutputItem};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::pin::Pin;

pub const DESCRIPTION: &str = r#"Read the contents of a file.

Use this tool when you need to:
- View source code
- Read configuration files
- Check file contents
- Read logs (consider pagination for large files)

Features:
- Automatically handles text encoding
- Supports pagination for large files
- Detects binary files
- Returns line numbers"#;

/// Default max file size (10MB)
const DEFAULT_MAX_SIZE: u64 = 10 * 1024 * 1024;
/// Default chunk size for pagination (100 lines)
const DEFAULT_CHUNK_LINES: usize = 100;

/// Arguments for FileRead tool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileReadArgs {
    /// Path to the file (relative to project root or absolute)
    pub path: String,
    /// Line offset for pagination (0-indexed)
    #[serde(default)]
    pub offset: usize,
    /// Maximum number of lines to read
    #[serde(default = "default_chunk_lines")]
    pub limit: usize,
}

fn default_chunk_lines() -> usize {
    DEFAULT_CHUNK_LINES
}

/// FileRead tool implementation
pub struct FileReadTool;

#[async_trait]
impl super::ToolImpl for FileReadTool {
    type Args = FileReadArgs;

    const DESCRIPTION: &'static str = DESCRIPTION;

    async fn execute(
        ctx: &ToolContext,
        args: Self::Args,
    ) -> Result<crate::infrastructure::streaming::StreamOutputBox, ToolError> {
        // Remote/SSH/sandbox: use shell-based file ops through the cached environment
        if ctx.execution_environment != "local" {
            if let Some(ref store) = ctx.env_store {
                let remote_path = crate::domain::tools::env_store::remote_path(ctx, &args.path);
                let env_arc = store.get_or_create(ctx, 30_000).await?;
                let result = {
                    let mut guard = env_arc.lock().await;
                    let mut ops = crate::domain::tools::shell_file_ops::ShellFileOps::new(&mut *guard);
                    ops.read_file(&remote_path, args.offset, args.limit).await?
                };
                let output = FileReadOutput {
                    path: args.path,
                    content: result.content,
                    offset: args.offset,
                    total_lines: result.total_lines,
                    has_more: result.has_more,
                };
                return Ok(output.into_stream());
            }
        }

        let path = resolve_path(&ctx.project_root, &args.path)?;

        // Check file metadata
        let metadata = tokio::fs::metadata(&path).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                FsError::NotFound {
                    path: args.path.clone(),
                }
            } else {
                FsError::from(e)
            }
        })?;

        if !metadata.is_file() {
            return Err(FsError::InvalidPath {
                path: args.path.clone(),
            }
            .into());
        }

        // Check file size
        let size = metadata.len();
        if size > DEFAULT_MAX_SIZE {
            return Err(FsError::FileTooLarge {
                path: args.path.clone(),
                size,
                max: DEFAULT_MAX_SIZE,
            }
            .into());
        }

        // Check for binary file
        let is_binary = is_binary_file(&path).await?;
        if is_binary {
            return Err(FsError::BinaryFile {
                path: args.path.clone(),
            }
            .into());
        }

        // Read file with pagination
        let content = read_file_paginated(&path, args.offset, args.limit).await?;
        let total_lines = count_lines(&path).await?;

        let output = FileReadOutput {
            path: args.path,
            content,
            offset: args.offset,
            total_lines,
            has_more: args.offset + args.limit < total_lines,
        };

        Ok(output.into_stream())
    }
}

/// File read output
#[derive(Debug, Clone)]
pub struct FileReadOutput {
    pub path: String,
    pub content: String,
    pub offset: usize,
    pub total_lines: usize,
    pub has_more: bool,
}

impl StreamOutput for FileReadOutput {
    fn into_stream(self) -> Pin<Box<dyn futures::Stream<Item = StreamOutputItem> + Send>> {
        use futures::stream;

        let items = vec![
            StreamOutputItem::Metadata {
                key: "path".to_string(),
                value: self.path,
            },
            StreamOutputItem::Metadata {
                key: "offset".to_string(),
                value: self.offset.to_string(),
            },
            StreamOutputItem::Metadata {
                key: "total_lines".to_string(),
                value: self.total_lines.to_string(),
            },
            StreamOutputItem::Metadata {
                key: "has_more".to_string(),
                value: self.has_more.to_string(),
            },
            StreamOutputItem::Start,
            StreamOutputItem::Content(self.content),
            StreamOutputItem::Complete,
        ];

        Box::pin(stream::iter(items))
    }
}

/// Resolve path (handle relative and absolute)
fn resolve_path(
    project_root: &std::path::Path,
    path: &str,
) -> Result<std::path::PathBuf, FsError> {
    let path_buf = if path.starts_with('/') || path.starts_with("~/") {
        // Absolute path or home directory
        if path.starts_with("~/") {
            let home = std::env::var("HOME")
                .map_err(|_| FsError::InvalidPath { path: path.to_string() })?;
            std::path::PathBuf::from(path.replacen("~", &home, 1))
        } else {
            std::path::PathBuf::from(path)
        }
    } else {
        // Relative to project root
        project_root.join(path)
    };

    // Check for path traversal
    let canonical_project = project_root.canonicalize().unwrap_or_else(|_| project_root.to_path_buf());
    let canonical_path = path_buf.canonicalize().unwrap_or_else(|_| path_buf.clone());

    if !canonical_path.starts_with(&canonical_project) && !path.starts_with('/') && !path.starts_with("~/") {
        // Allow paths outside project only if explicitly absolute
        return Err(FsError::PathTraversal { path: path.to_string() });
    }

    Ok(path_buf)
}

/// Check if file is binary
async fn is_binary_file(path: &std::path::Path) -> Result<bool, FsError> {
    // Read first 8KB to detect binary
    let header = tokio::fs::read(&path)
        .await
        .map_err(FsError::from)?;

    // Check for null bytes (common in binary files)
    let sample_size = header.len().min(8192);
    if header[..sample_size].contains(&0) {
        return Ok(true);
    }

    // Additional checks can be added here (file extension, MIME type, etc.)

    Ok(false)
}

/// Read file with pagination
async fn read_file_paginated(
    path: &std::path::Path,
    offset: usize,
    limit: usize,
) -> Result<String, FsError> {
    use tokio::io::{AsyncBufReadExt, BufReader};

    let file = tokio::fs::File::open(path).await.map_err(FsError::from)?;
    let reader = BufReader::new(file);
    let mut lines = reader.lines();

    let mut result = String::new();
    let mut current_line = 0;
    let mut lines_read = 0;

    while let Some(line) = lines.next_line().await.map_err(|e| FsError::IoError {
        message: e.to_string(),
    })? {
        if current_line >= offset {
            result.push_str(&line);
            result.push('\n');
            lines_read += 1;

            if lines_read >= limit {
                break;
            }
        }
        current_line += 1;
    }

    Ok(result)
}

/// Count total lines in file
async fn count_lines(path: &std::path::Path) -> Result<usize, FsError> {
    use tokio::io::{AsyncBufReadExt, BufReader};

    let file = tokio::fs::File::open(path).await.map_err(FsError::from)?;
    let reader = BufReader::new(file);
    let mut lines = reader.lines();

    let mut count = 0;
    while let Ok(Some(_)) = lines.next_line().await {
        count += 1;
    }

    Ok(count)
}

/// Get the JSON schema for the FileRead tool
pub fn schema() -> ToolSchema {
    ToolSchema::new(
        "file_read",
        DESCRIPTION,
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file (relative to project root or absolute)"
                },
                "offset": {
                    "type": "integer",
                    "description": "Line offset for pagination (0-indexed, default: 0)",
                    "minimum": 0
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of lines to read (default: 100)",
                    "minimum": 1,
                    "maximum": 10000
                }
            },
            "required": ["path"]
        }),
    )
}
