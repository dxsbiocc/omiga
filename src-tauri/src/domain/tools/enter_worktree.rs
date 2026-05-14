use super::{ToolContext, ToolError, ToolSchema};
use crate::infrastructure::streaming::{StreamOutput, StreamOutputItem};
use async_trait::async_trait;
use git2::{BranchType, Repository, WorktreeAddOptions};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::pin::Pin;

pub const DESCRIPTION: &str = r#"Create or reuse a git worktree for isolated branch work."#;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnterWorktreeArgs {
    pub branch: String,
    pub worktree_name: Option<String>,
}

pub struct EnterWorktreeTool;

#[async_trait]
impl super::ToolImpl for EnterWorktreeTool {
    type Args = EnterWorktreeArgs;

    const DESCRIPTION: &'static str = DESCRIPTION;

    async fn execute(
        ctx: &ToolContext,
        args: Self::Args,
    ) -> Result<crate::infrastructure::streaming::StreamOutputBox, ToolError> {
        let repo_root = repo_root(ctx)?;
        let repo = Repository::discover(&repo_root).map_err(|e| ToolError::ExecutionFailed {
            message: format!("Git repository not found: {}", e),
        })?;
        let root = repo_root_path(&repo);
        let name = args
            .worktree_name
            .as_deref()
            .map(sanitize_worktree_name)
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| sanitize_worktree_name(&args.branch));
        let path = root.join(".claude").join("worktrees").join(&name);

        if let Some(existing_path) = existing_worktree_path(&repo, &name)? {
            let body = serde_json::json!({
                "worktree_path": existing_path.display().to_string(),
                "branch": args.branch,
                "message": format!("Worktree reused at {}", existing_path.display()),
                "reused": true
            });
            let text = serialize_json(body)?;
            return Ok(EnterWorktreeOutput { text }.into_stream());
        }

        std::fs::create_dir_all(path.parent().unwrap_or(&root)).map_err(|e| {
            ToolError::ExecutionFailed {
                message: format!("create worktree parent: {}", e),
            }
        })?;

        let branch = ensure_branch(&repo, &args.branch)?;
        let reference = branch.into_reference();
        let mut opts = WorktreeAddOptions::new();
        opts.reference(Some(&reference));
        repo.worktree(&name, &path, Some(&opts))
            .map_err(|e| ToolError::ExecutionFailed {
                message: format!("create worktree: {}", e),
            })?;

        let body = serde_json::json!({
            "worktree_path": path.display().to_string(),
            "branch": args.branch,
            "message": format!("Worktree created at {}", path.display()),
            "reused": false
        });
        let text = serialize_json(body)?;
        Ok(EnterWorktreeOutput { text }.into_stream())
    }
}

fn repo_root(ctx: &ToolContext) -> Result<PathBuf, ToolError> {
    if ctx.cwd.exists() {
        return Ok(ctx.cwd.clone());
    }
    std::env::current_dir().map_err(|e| ToolError::ExecutionFailed {
        message: format!("current directory: {}", e),
    })
}

fn repo_root_path(repo: &Repository) -> PathBuf {
    repo.workdir()
        .or_else(|| repo.path().parent())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| repo.path().to_path_buf())
}

fn sanitize_worktree_name(value: &str) -> String {
    let sanitized = value
        .chars()
        .filter_map(|c| match c {
            c if c.is_ascii_alphanumeric() || c == '-' => Some(c),
            '/' | '.' | ' ' => Some('-'),
            c if c.is_whitespace() => Some('-'),
            _ => None,
        })
        .collect::<String>();

    if sanitized.is_empty() {
        "worktree".to_string()
    } else {
        sanitized
    }
}

fn existing_worktree_path(repo: &Repository, name: &str) -> Result<Option<PathBuf>, ToolError> {
    let worktrees = repo.worktrees().map_err(|e| ToolError::ExecutionFailed {
        message: format!("list worktrees: {}", e),
    })?;
    if worktrees.iter().flatten().any(|existing| existing == name) {
        let wt = repo
            .find_worktree(name)
            .map_err(|e| ToolError::ExecutionFailed {
                message: format!("find worktree: {}", e),
            })?;
        Ok(Some(wt.path().to_path_buf()))
    } else {
        Ok(None)
    }
}

fn ensure_branch<'repo>(
    repo: &'repo Repository,
    branch_name: &str,
) -> Result<git2::Branch<'repo>, ToolError> {
    match repo.find_branch(branch_name, BranchType::Local) {
        Ok(branch) => Ok(branch),
        Err(_) => {
            let head = repo.head().map_err(|e| ToolError::ExecutionFailed {
                message: format!("read HEAD: {}", e),
            })?;
            let head_commit = head
                .peel_to_commit()
                .map_err(|e| ToolError::ExecutionFailed {
                    message: format!("read HEAD commit: {}", e),
                })?;
            repo.branch(branch_name, &head_commit, false)
                .map_err(|e| ToolError::ExecutionFailed {
                    message: format!("create branch: {}", e),
                })
        }
    }
}

fn serialize_json(value: serde_json::Value) -> Result<String, ToolError> {
    serde_json::to_string_pretty(&value).map_err(|e| ToolError::ExecutionFailed {
        message: e.to_string(),
    })
}

struct EnterWorktreeOutput {
    text: String,
}

impl StreamOutput for EnterWorktreeOutput {
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
        "EnterWorktree",
        DESCRIPTION,
        serde_json::json!({
            "type": "object",
            "properties": {
                "branch": { "type": "string" },
                "worktree_name": { "type": "string" }
            },
            "required": ["branch"]
        }),
    )
}
