//! Send a user-visible message — aligned with `BriefTool` / `SendUserMessage` (TypeScript), legacy `Brief`.
//!
//! Resolves optional attachment paths (cwd-relative or absolute), returns metadata. Omiga chat already shows assistant text; this tool gives the model a structured “delivered” handoff like Claude Code.

use super::{ToolContext, ToolError, ToolSchema};
use crate::errors::FsError;
use crate::infrastructure::streaming::{StreamOutput, StreamOutputItem};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::pin::Pin;

const MAX_ATTACHMENTS: usize = 20;
const MAX_ATTACHMENT_BYTES: u64 = 10 * 1024 * 1024;

pub const DESCRIPTION: &str = r#"Send a message the user should read. `message` supports markdown.

Optional `attachments`: file paths (absolute or relative to the session working directory) for logs, screenshots, diffs.

`status`: `normal` when replying to the user; `proactive` when surfacing something they did not ask for.

In Omiga, the chat transcript shows assistant output; this tool still records a structured delivery receipt for the model."#;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum UserMessageStatus {
    #[default]
    Normal,
    Proactive,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendUserMessageArgs {
    pub message: String,
    #[serde(default)]
    pub attachments: Option<Vec<String>>,
    #[serde(default)]
    pub status: Option<UserMessageStatus>,
}

#[derive(Debug, Clone, Serialize)]
struct ResolvedAttachment {
    path: String,
    size: u64,
    is_image: bool,
}

pub struct SendUserMessageTool;

fn is_image_path(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .map(|e| {
            matches!(
                e.to_ascii_lowercase().as_str(),
                "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "bmp" | "ico"
            )
        })
        .unwrap_or(false)
}

/// Resolve path: absolute / `~/` / relative to `cwd` (matches Brief using cwd for relative paths).
fn resolve_attachment_path(
    project_root: &Path,
    cwd: &Path,
    path: &str,
) -> Result<PathBuf, FsError> {
    let path_buf = if path.starts_with('/') || path.starts_with("~/") {
        if path.starts_with("~/") {
            let home = std::env::var("HOME")
                .map_err(|_| FsError::InvalidPath { path: path.to_string() })?;
            PathBuf::from(path.replacen("~", &home, 1))
        } else {
            PathBuf::from(path)
        }
    } else {
        cwd.join(path)
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

#[async_trait]
impl super::ToolImpl for SendUserMessageTool {
    type Args = SendUserMessageArgs;

    const DESCRIPTION: &'static str = DESCRIPTION;

    async fn execute(
        ctx: &ToolContext,
        args: Self::Args,
    ) -> Result<crate::infrastructure::streaming::StreamOutputBox, ToolError> {
        let msg = args.message.trim();
        if msg.is_empty() {
            return Err(ToolError::InvalidArguments {
                message: "`message` must be non-empty.".to_string(),
            });
        }

        let status = args.status.unwrap_or_default();
        let proactive = status == UserMessageStatus::Proactive;

        let mut resolved: Vec<ResolvedAttachment> = Vec::new();
        if let Some(paths) = &args.attachments {
            if paths.len() > MAX_ATTACHMENTS {
                return Err(ToolError::InvalidArguments {
                    message: format!("At most {} attachments.", MAX_ATTACHMENTS),
                });
            }
            for raw in paths {
                let p = resolve_attachment_path(&ctx.project_root, &ctx.cwd, raw.trim())
                    .map_err(|e: FsError| ToolError::InvalidArguments {
                        message: format!("Attachment \"{}\": {}", raw, e),
                    })?;

                let meta = tokio::fs::metadata(&p).await.map_err(|e| {
                    ToolError::InvalidArguments {
                        message: format!("Attachment \"{}\": {}", raw, e),
                    }
                })?;
                if !meta.is_file() {
                    return Err(ToolError::InvalidArguments {
                        message: format!("Attachment \"{}\" is not a regular file.", raw),
                    });
                }
                let size = meta.len();
                if size > MAX_ATTACHMENT_BYTES {
                    return Err(ToolError::InvalidArguments {
                        message: format!(
                            "Attachment \"{}\" is too large (max {} bytes).",
                            raw, MAX_ATTACHMENT_BYTES
                        ),
                    });
                }

                resolved.push(ResolvedAttachment {
                    path: p.to_string_lossy().to_string(),
                    size,
                    is_image: is_image_path(&p),
                });
            }
        }

        let sent_at = chrono::Utc::now().to_rfc3339();
        let n_attach = resolved.len();

        let out = serde_json::json!({
            "message": msg,
            "sentAt": sent_at,
            "status": if proactive { "proactive" } else { "normal" },
            "attachments": resolved,
            "_omiga": "Message recorded. The user sees the chat; use this tool when instructions require an explicit user-facing handoff."
        });

        let text = serde_json::to_string_pretty(&out).map_err(|e| ToolError::ExecutionFailed {
            message: format!("serialize: {}", e),
        })?;

        let summary = if n_attach == 0 {
            "Message delivered to user.".to_string()
        } else {
            format!(
                "Message delivered to user ({} attachment(s)).",
                n_attach
            )
        };

        Ok(
            SendUserMessageOutput {
                text: format!("{}\n\n{}", summary, text),
            }
            .into_stream(),
        )
    }
}

struct SendUserMessageOutput {
    text: String,
}

impl StreamOutput for SendUserMessageOutput {
    fn into_stream(self) -> Pin<Box<dyn futures::Stream<Item = StreamOutputItem> + Send>> {
        use futures::stream;
        Box::pin(stream::iter(vec![
            StreamOutputItem::Start,
            StreamOutputItem::Content(self.text),
            StreamOutputItem::Complete,
        ]))
    }
}

pub fn schema() -> ToolSchema {
    ToolSchema::new(
        "SendUserMessage",
        DESCRIPTION,
        serde_json::json!({
            "type": "object",
            "properties": {
                "message": {
                    "type": "string",
                    "description": "Markdown message for the user"
                },
                "attachments": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Optional file paths (absolute or cwd-relative)"
                },
                "status": {
                    "type": "string",
                    "enum": ["normal", "proactive"],
                    "description": "normal = reply; proactive = unsolicited update"
                }
            },
            "required": ["message"]
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_relative_to_cwd() {
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path().join("proj");
        std::fs::create_dir_all(&project).unwrap();
        let cwd = project.join("sub");
        std::fs::create_dir_all(&cwd).unwrap();
        let f = cwd.join("a.txt");
        std::fs::write(&f, b"x").unwrap();

        let p = resolve_attachment_path(&project, &cwd, "a.txt").unwrap();
        assert_eq!(p, f);
    }
}
