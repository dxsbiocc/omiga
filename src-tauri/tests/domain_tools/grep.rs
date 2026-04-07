//! Tests for `grep`.

use futures::StreamExt;
use omiga_lib::domain::tools::grep::{GrepArgs, GrepTool};
use omiga_lib::domain::tools::{ToolContext, ToolImpl};
use omiga_lib::infrastructure::streaming::StreamOutputItem;
use std::fs;
use tempfile::tempdir;

async fn drain_tool(mut stream: omiga_lib::infrastructure::streaming::StreamOutputBox) {
    while stream.next().await.is_some() {}
}

#[tokio::test]
async fn grep_finds_pattern_in_project_file() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("sample.rs"), "fn hello() {}\n").unwrap();

    let ctx = ToolContext::new(dir.path());
    let args = GrepArgs {
        pattern: "hello".to_string(),
        path_pattern: None,
        case_insensitive: false,
        max_results: 100,
    };
    drain_tool(GrepTool::execute(&ctx, args).await.unwrap()).await;
}

#[tokio::test]
async fn grep_respects_ignore_file() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join(".ignore"), "ignored.txt\n").unwrap();
    fs::write(dir.path().join("tracked.txt"), "secret_word\n").unwrap();
    fs::write(dir.path().join("ignored.txt"), "secret_word\n").unwrap();

    let ctx = ToolContext::new(dir.path());
    let args = GrepArgs {
        pattern: "secret_word".to_string(),
        path_pattern: None,
        case_insensitive: false,
        max_results: 100,
    };
    let mut stream = GrepTool::execute(&ctx, args).await.unwrap();
    let mut grep_matches = 0usize;
    while let Some(item) = stream.next().await {
        if let StreamOutputItem::GrepMatch(_) = item {
            grep_matches += 1;
        }
    }
    assert_eq!(
        grep_matches, 1,
        "only tracked.txt should match; ignored.txt is excluded by .ignore"
    );
}

#[tokio::test]
async fn glob_filters_to_rs_only() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("a.rs"), "foo\n").unwrap();
    fs::write(dir.path().join("b.md"), "foo\n").unwrap();

    let ctx = ToolContext::new(dir.path());
    let args = GrepArgs {
        pattern: "foo".to_string(),
        path_pattern: Some("*.rs".to_string()),
        case_insensitive: false,
        max_results: 100,
    };
    let mut stream = GrepTool::execute(&ctx, args).await.unwrap();
    let mut files = Vec::new();
    while let Some(item) = stream.next().await {
        if let StreamOutputItem::GrepMatch(m) = item {
            files.push(m.file.replace('\\', "/"));
        }
    }
    assert_eq!(files.len(), 1);
    assert!(files[0].ends_with("a.rs"), "got {:?}", files);
}
