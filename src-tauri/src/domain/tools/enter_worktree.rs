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

#[derive(Debug, Clone)]
pub struct PreparedWorktree {
    pub path: PathBuf,
    pub branch: String,
    pub reused: bool,
    pub name: String,
}

/// Create or reuse the deterministic worktree owned by one chat session.
///
/// This helper is intentionally stricter than the public `EnterWorktree` tool:
/// it only accepts an existing local branch and validates that a reused worktree
/// is still checked out on the requested branch.
pub fn prepare_session_worktree(
    repo_root: &Path,
    session_id: &str,
    requested_branch: Option<&str>,
) -> Result<PreparedWorktree, ToolError> {
    let repo = Repository::discover(repo_root).map_err(|e| ToolError::ExecutionFailed {
        message: format!("Git repository not found: {}", e),
    })?;
    let root = repo_root_path(&repo);
    let branch = requested_branch
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| current_branch_name(&repo))
        .ok_or_else(|| ToolError::ExecutionFailed {
            message: "Cannot determine current branch for worktree".to_string(),
        })?;
    let branch_ref = repo
        .find_branch(&branch, BranchType::Local)
        .map_err(|e| ToolError::ExecutionFailed {
            message: format!("Local branch '{}' not found: {}", branch, e),
        })?
        .into_reference();
    let name = session_worktree_name(session_id, &branch);
    let path = root.join(".claude").join("worktrees").join(&name);

    if let Some(existing_path) = existing_worktree_path(&repo, &name)? {
        validate_existing_worktree_branch(&existing_path, &branch)?;
        return Ok(PreparedWorktree {
            path: existing_path,
            branch,
            reused: true,
            name,
        });
    }

    std::fs::create_dir_all(path.parent().unwrap_or(&root)).map_err(|e| {
        ToolError::ExecutionFailed {
            message: format!("create worktree parent: {}", e),
        }
    })?;

    let mut opts = WorktreeAddOptions::new();
    opts.reference(Some(&branch_ref));
    repo.worktree(&name, &path, Some(&opts))
        .map_err(|e| ToolError::ExecutionFailed {
            message: format!("create worktree: {}", e),
        })?;

    Ok(PreparedWorktree {
        path,
        branch,
        reused: false,
        name,
    })
}

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

fn current_branch_name(repo: &Repository) -> Option<String> {
    repo.head()
        .ok()
        .and_then(|head| head.shorthand().map(str::to_string))
}

fn session_worktree_name(session_id: &str, branch: &str) -> String {
    let session_short: String = session_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(8)
        .collect();
    let mut branch_part = sanitize_worktree_name(branch);
    if branch_part.len() > 40 {
        branch_part.truncate(40);
    }
    format!(
        "omiga-{}-{}-{:08x}",
        if session_short.is_empty() {
            "session"
        } else {
            session_short.as_str()
        },
        branch_part,
        stable_hash32(branch.as_bytes())
    )
}

fn stable_hash32(bytes: &[u8]) -> u32 {
    bytes.iter().fold(0x811c9dc5u32, |hash, byte| {
        (hash ^ u32::from(*byte)).wrapping_mul(0x01000193)
    })
}

fn validate_existing_worktree_branch(path: &Path, branch: &str) -> Result<(), ToolError> {
    let repo = Repository::open(path).map_err(|e| ToolError::ExecutionFailed {
        message: format!("open existing worktree: {}", e),
    })?;
    let actual = current_branch_name(&repo).ok_or_else(|| ToolError::ExecutionFailed {
        message: format!(
            "Existing worktree at '{}' is not on a named branch",
            path.display()
        ),
    })?;
    if actual != branch {
        return Err(ToolError::ExecutionFailed {
            message: format!(
                "Existing worktree at '{}' is on branch '{}' instead of '{}'",
                path.display(),
                actual,
                branch
            ),
        });
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    use git2::{Repository, Signature};

    fn init_repo_with_branch() -> (tempfile::TempDir, Repository) {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = Repository::init(dir.path()).expect("init repo");
        let sig = Signature::now("Omiga", "omiga@example.com").expect("signature");
        std::fs::write(dir.path().join("README.md"), "hello\n").expect("write readme");
        let mut index = repo.index().expect("index");
        index.add_path(Path::new("README.md")).expect("add readme");
        let tree_id = index.write_tree().expect("write tree");
        let tree = repo.find_tree(tree_id).expect("find tree");
        let commit_id = repo
            .commit(Some("HEAD"), &sig, &sig, "initial", &tree, &[])
            .expect("commit");
        let commit = repo.find_commit(commit_id).expect("find commit");
        repo.branch("feature/worktree", &commit, false)
            .expect("create branch");
        drop(commit);
        drop(tree);
        (dir, repo)
    }

    #[test]
    fn prepare_session_worktree_creates_and_reuses_branch_worktree() {
        let (dir, repo) = init_repo_with_branch();
        drop(repo);

        let first =
            prepare_session_worktree(dir.path(), "session-abcdef123456", Some("feature/worktree"))
                .expect("prepare first worktree");
        assert!(!first.reused);
        assert_eq!(first.branch, "feature/worktree");
        assert!(first.path.exists());
        assert!(first.name.starts_with("omiga-sessiona-"));

        let second =
            prepare_session_worktree(dir.path(), "session-abcdef123456", Some("feature/worktree"))
                .expect("prepare second worktree");
        assert!(second.reused);
        assert_eq!(second.path, first.path);
        assert_eq!(second.branch, first.branch);
    }
}
