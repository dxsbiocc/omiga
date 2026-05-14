use super::{ToolContext, ToolError, ToolSchema};
use crate::infrastructure::streaming::{StreamOutput, StreamOutputItem};
use async_trait::async_trait;
use git2::{Repository, WorktreePruneOptions};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::pin::Pin;

pub const DESCRIPTION: &str = r#"Exit the current Omiga git worktree and optionally prune it."#;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExitWorktreeArgs {
    pub keep: Option<bool>,
}

pub struct ExitWorktreeTool;

#[async_trait]
impl super::ToolImpl for ExitWorktreeTool {
    type Args = ExitWorktreeArgs;

    const DESCRIPTION: &'static str = DESCRIPTION;

    async fn execute(
        ctx: &ToolContext,
        args: Self::Args,
    ) -> Result<crate::infrastructure::streaming::StreamOutputBox, ToolError> {
        let worktree = active_worktree(&ctx.cwd).map(Ok).unwrap_or_else(|| {
            let cwd = std::env::current_dir().map_err(|e| ToolError::ExecutionFailed {
                message: format!("current directory: {}", e),
            })?;
            active_worktree(&cwd).ok_or_else(|| ToolError::ExecutionFailed {
                message: "No active worktree to exit".to_string(),
            })
        })?;
        // find_worktree must be called on the principal repo, not the linked worktree repo.
        // Convention: worktree.path = .../repo/.claude/worktrees/<name> — go up 3 levels.
        let main_repo_root = worktree
            .path
            .parent()
            .and_then(|p| p.parent())
            .and_then(|p| p.parent())
            .ok_or_else(|| ToolError::ExecutionFailed {
                message: "cannot navigate to main repository from worktree path".to_string(),
            })?;
        let repo =
            Repository::discover(main_repo_root).map_err(|e| ToolError::ExecutionFailed {
                message: format!("open main repository: {}", e),
            })?;
        // Verify the discovered repo actually contains the worktree we intend to prune.
        // This guards against accidentally operating on a parent repository when the
        // .claude/worktrees/ layout is not followed.
        if repo.find_worktree(&worktree.name).is_err() {
            return Err(ToolError::ExecutionFailed {
                message: format!(
                    "worktree '{}' not found in repository at '{}'; \
                     cannot safely prune",
                    worktree.name,
                    main_repo_root.display()
                ),
            });
        }

        let pruned = if args.keep != Some(true) {
            let wt =
                repo.find_worktree(&worktree.name)
                    .map_err(|e| ToolError::ExecutionFailed {
                        message: format!("find worktree: {}", e),
                    })?;
            let mut opts = WorktreePruneOptions::new();
            opts.valid(true).working_tree(true);
            wt.prune(Some(&mut opts))
                .map_err(|e| ToolError::ExecutionFailed {
                    message: format!("prune worktree: {}", e),
                })?;
            true
        } else {
            false
        };

        let body = serde_json::json!({
            "exited_path": worktree.path.display().to_string(),
            "pruned": pruned
        });
        let text = serde_json::to_string_pretty(&body).map_err(|e| ToolError::ExecutionFailed {
            message: e.to_string(),
        })?;
        Ok(ExitWorktreeOutput { text }.into_stream())
    }
}

struct ActiveWorktree {
    name: String,
    path: PathBuf,
}

fn active_worktree(cwd: &Path) -> Option<ActiveWorktree> {
    let components = cwd
        .components()
        .map(|component| component.as_os_str().to_string_lossy().to_string())
        .collect::<Vec<_>>();

    components
        .windows(3)
        .position(|window| window[0] == ".claude" && window[1] == "worktrees")
        .and_then(|idx| {
            let name = components.get(idx + 2)?.clone();
            let path = components
                .iter()
                .take(idx + 3)
                .fold(PathBuf::new(), |path, component| path.join(component));
            Some(ActiveWorktree { name, path })
        })
}

struct ExitWorktreeOutput {
    text: String,
}

impl StreamOutput for ExitWorktreeOutput {
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
        "ExitWorktree",
        DESCRIPTION,
        serde_json::json!({
            "type": "object",
            "properties": {
                "keep": { "type": "boolean" }
            }
        }),
    )
}
