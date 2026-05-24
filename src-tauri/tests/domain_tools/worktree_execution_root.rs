//! Regression coverage for session worktree execution roots.

use futures::StreamExt;
use git2::{Repository, Signature};
use omiga_lib::domain::tools::enter_worktree::prepare_session_worktree;
use omiga_lib::domain::tools::file_write::{FileWriteArgs, FileWriteTool};
use omiga_lib::domain::tools::{ToolContext, ToolImpl};
use std::path::Path;

fn init_repo_with_branch() -> tempfile::TempDir {
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
    drop(repo);
    dir
}

#[tokio::test]
async fn worktree_file_write_uses_prepared_execution_root() {
    let dir = init_repo_with_branch();
    let prepared = prepare_session_worktree(
        dir.path(),
        "session-worktree-root",
        Some("feature/worktree"),
    )
    .expect("prepare worktree");

    let ctx = ToolContext::new(prepared.path.clone());
    let args = FileWriteArgs {
        path: "scratch/result.txt".to_string(),
        content: "written from isolated worktree\n".to_string(),
        expected_hash: None,
        create_dirs: true,
    };
    let mut stream = FileWriteTool::execute(&ctx, args)
        .await
        .expect("write in worktree");
    while stream.next().await.is_some() {}

    let worktree_file = prepared.path.join("scratch/result.txt");
    let canonical_file = dir.path().join("scratch/result.txt");
    assert_eq!(
        tokio::fs::read_to_string(&worktree_file).await.unwrap(),
        "written from isolated worktree\n"
    );
    assert!(
        !canonical_file.exists(),
        "relative writes must not land in the canonical repository root"
    );
}
