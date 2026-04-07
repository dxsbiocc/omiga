//! Tests for `file_edit`.

use futures::StreamExt;
use omiga_lib::domain::tools::file_edit::{FileEditArgs, FileEditTool};
use omiga_lib::domain::tools::{ToolContext, ToolImpl};

#[tokio::test]
async fn file_edit_replacen_single() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let f = root.join("a.txt");
    tokio::fs::write(&f, "hello world\n").await.unwrap();

    let ctx = ToolContext::new(root);
    let args = FileEditArgs {
        file_path: "a.txt".to_string(),
        old_string: "world".to_string(),
        new_string: "Rust".to_string(),
        replace_all: false,
    };
    let mut stream = FileEditTool::execute(&ctx, args).await.unwrap();
    while stream.next().await.is_some() {}

    let out = tokio::fs::read_to_string(&f).await.unwrap();
    assert_eq!(out, "hello Rust\n");
}

#[tokio::test]
async fn file_edit_rejects_non_unique_without_replace_all() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let f = root.join("b.txt");
    tokio::fs::write(&f, "x x x\n").await.unwrap();

    let ctx = ToolContext::new(root);
    let args = FileEditArgs {
        file_path: "b.txt".to_string(),
        old_string: "x".to_string(),
        new_string: "y".to_string(),
        replace_all: false,
    };
    assert!(FileEditTool::execute(&ctx, args).await.is_err());
}
