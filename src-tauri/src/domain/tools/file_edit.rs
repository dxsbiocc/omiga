//! FileEdit tool — exact string replacement in an existing file
//!
//! Aligns with `src/tools/FileEditTool` (TypeScript): `file_path`, `old_string`,
//! `new_string`, optional `replace_all`. Omiga uses the tool name `file_edit`
//! (snake_case) for LLM function calling, consistent with `file_read` / `file_write`.

use super::{ToolContext, ToolError, ToolSchema};
use crate::errors::FsError;
use crate::infrastructure::streaming::{StreamOutput, StreamOutputItem};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::pin::Pin;

/// Same cap as `file_read` for predictable memory use
const MAX_FILE_BYTES: u64 = 10 * 1024 * 1024;

pub const DESCRIPTION: &str = r#"Performs exact string replacements in a file.

Use when you need to modify part of a file without rewriting the whole file.
- `old_string` must match the file contents exactly (including whitespace).
- If `old_string` is not unique and `replace_all` is false, the edit fails — include more context in `old_string` or set `replace_all` to true.
- Prefer this over full-file writes when changing a small region."#;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEditArgs {
    /// Absolute path, or path relative to the project root
    pub file_path: String,
    pub old_string: String,
    pub new_string: String,
    #[serde(default)]
    pub replace_all: bool,
}

pub struct FileEditTool;

#[async_trait]
impl super::ToolImpl for FileEditTool {
    type Args = FileEditArgs;

    const DESCRIPTION: &'static str = DESCRIPTION;

    async fn execute(
        ctx: &ToolContext,
        args: Self::Args,
    ) -> Result<crate::infrastructure::streaming::StreamOutputBox, ToolError> {
        // Remote/SSH/sandbox: delegate to shell-based edit through the cached environment
        if ctx.execution_environment != "local" {
            if let Some(ref store) = ctx.env_store {
                let remote_path = crate::domain::tools::env_store::remote_path(ctx, &args.file_path);
                let env_arc = store.get_or_create(ctx, 30_000).await?;
                {
                    let mut guard = env_arc.lock().await;
                    let mut ops = crate::domain::tools::shell_file_ops::ShellFileOps::new(&mut *guard);
                    ops.edit_file(&remote_path, &args.old_string, &args.new_string, args.replace_all).await?;
                }
                return Ok(
                    FileEditOutput {
                        path: args.file_path,
                        replaced: 1,
                        replace_all: args.replace_all,
                        created: false,
                        created_bytes: 0,
                    }
                    .into_stream(),
                );
            }
        }

        let path = resolve_path(&ctx.project_root, &args.file_path)?;

        if args.old_string == args.new_string {
            return Err(
                FsError::IoError {
                    message: "No changes to make: old_string and new_string are identical."
                        .to_string(),
                }
                .into(),
            );
        }

        let meta = tokio::fs::metadata(&path).await;

        match meta {
            Ok(m) => {
                if !m.is_file() {
                    return Err(
                        FsError::InvalidPath {
                            path: args.file_path.clone(),
                        }
                        .into(),
                    );
                }
                if m.len() > MAX_FILE_BYTES {
                    return Err(
                        FsError::FileTooLarge {
                            path: args.file_path.clone(),
                            size: m.len(),
                            max: MAX_FILE_BYTES,
                        }
                        .into(),
                    );
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // TS: empty old_string + missing file => create file
                if args.old_string.is_empty() {
                    if let Some(parent) = path.parent() {
                        tokio::fs::create_dir_all(parent).await.map_err(|e| {
                            FsError::IoError {
                                message: format!("Failed to create parent directories: {}", e),
                            }
                        })?;
                    }
                    let temp_path = path.with_extension("tmp");
                    tokio::fs::write(&temp_path, args.new_string.as_bytes())
                        .await
                        .map_err(|e| FsError::IoError {
                            message: format!("Failed to write temp file: {}", e),
                        })?;
                    tokio::fs::rename(&temp_path, &path).await.map_err(|e| {
                        FsError::IoError {
                            message: format!("Failed to rename temp file: {}", e),
                        }
                    })?;
                    let byte_len = args.new_string.len();
                    return Ok(
                        FileEditOutput {
                            path: args.file_path,
                            replaced: 0,
                            replace_all: args.replace_all,
                            created: true,
                            created_bytes: byte_len,
                        }
                        .into_stream(),
                    );
                }
                return Err(
                    FsError::NotFound {
                        path: args.file_path.clone(),
                    }
                    .into(),
                );
            }
            Err(e) => return Err(FsError::from(e).into()),
        }

        if path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("ipynb"))
            == Some(true)
        {
            return Err(
                FsError::IoError {
                    message: "Jupyter Notebook (.ipynb) files require a dedicated notebook editor; \
                              avoid editing JSON with file_edit."
                        .to_string(),
                }
                .into(),
            );
        }

        let raw = tokio::fs::read(&path).await.map_err(FsError::from)?;
        if raw.len() > MAX_FILE_BYTES as usize {
            return Err(
                FsError::FileTooLarge {
                    path: args.file_path.clone(),
                    size: raw.len() as u64,
                    max: MAX_FILE_BYTES,
                }
                .into(),
            );
        }
        if raw.iter().take(8192).any(|&b| b == 0) {
            return Err(
                FsError::BinaryFile {
                    path: args.file_path.clone(),
                }
                .into(),
            );
        }

        let mut content = String::from_utf8(raw).map_err(|_| FsError::BinaryFile {
            path: args.file_path.clone(),
        })?;

        // Normalize newlines like TS (CRLF -> LF) for stable matching
        if content.contains("\r\n") {
            content = content.replace("\r\n", "\n");
        }

        if args.old_string.is_empty() {
            if !content.is_empty() {
                return Err(
                    FsError::IoError {
                        message: "Cannot use empty old_string: file already has content."
                            .to_string(),
                    }
                    .into(),
                );
            }
            // empty file: write new_string
        } else {
            let count = content.matches(args.old_string.as_str()).count();
            if count == 0 {
                return Err(
                    FsError::IoError {
                        message: format!(
                            "String to replace not found in file.\nExpected snippet ({} bytes).",
                            args.old_string.len()
                        ),
                    }
                    .into(),
                );
            }
            if count > 1 && !args.replace_all {
                return Err(
                    FsError::IoError {
                        message: format!(
                            "old_string is not unique in file ({} matches). Use a larger unique snippet or set replace_all to true.",
                            count
                        ),
                    }
                    .into(),
                );
            }
        }

        let new_content = if args.old_string.is_empty() {
            args.new_string.clone()
        } else if args.replace_all {
            content.replace(&args.old_string, &args.new_string)
        } else {
            content.replacen(&args.old_string, &args.new_string, 1)
        };

        let replaced = if args.old_string.is_empty() {
            1
        } else if args.replace_all {
            content.matches(args.old_string.as_str()).count()
        } else {
            1
        };

        let temp_path = path.with_extension("tmp");
        tokio::fs::write(&temp_path, new_content.as_bytes())
            .await
            .map_err(|e| FsError::IoError {
                message: format!("Failed to write temp file: {}", e),
            })?;
        tokio::fs::rename(&temp_path, &path).await.map_err(|e| FsError::IoError {
            message: format!("Failed to rename temp file: {}", e),
        })?;

        Ok(
            FileEditOutput {
                path: args.file_path,
                replaced,
                replace_all: args.replace_all,
                created: false,
                created_bytes: 0,
            }
            .into_stream(),
        )
    }
}

#[derive(Debug, Clone)]
struct FileEditOutput {
    path: String,
    replaced: usize,
    replace_all: bool,
    created: bool,
    created_bytes: usize,
}

impl StreamOutput for FileEditOutput {
    fn into_stream(self) -> Pin<Box<dyn futures::Stream<Item = StreamOutputItem> + Send>> {
        use futures::stream;
        let summary = if self.created {
            format!("Created file ({} bytes)", self.created_bytes)
        } else {
            format!(
                "Edited {} ({} replacement{}, replace_all={})",
                self.path,
                self.replaced,
                if self.replaced == 1 { "" } else { "s" },
                self.replace_all
            )
        };
        let items = vec![
            StreamOutputItem::Metadata {
                key: "path".to_string(),
                value: self.path,
            },
            StreamOutputItem::Metadata {
                key: "replace_all".to_string(),
                value: self.replace_all.to_string(),
            },
            StreamOutputItem::Start,
            StreamOutputItem::Content(summary),
            StreamOutputItem::Complete,
        ];
        Box::pin(stream::iter(items))
    }
}

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

pub fn schema() -> ToolSchema {
    ToolSchema::new(
        "file_edit",
        DESCRIPTION,
        serde_json::json!({
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "Path to the file (relative to project root or absolute)"
                },
                "old_string": {
                    "type": "string",
                    "description": "Exact text to replace (must appear in the file unless empty for new empty file)"
                },
                "new_string": {
                    "type": "string",
                    "description": "Replacement text"
                },
                "replace_all": {
                    "type": "boolean",
                    "description": "Replace every occurrence of old_string (default false)"
                }
            },
            "required": ["file_path", "old_string", "new_string"]
        }),
    )
}
