//! Tests for `todo_write`.

use futures::StreamExt;
use omiga_lib::domain::session::{TodoItem, TodoStatus};
use omiga_lib::domain::tools::todo_write::{TodoWriteArgs, TodoWriteTool};
use omiga_lib::domain::tools::{ToolContext, ToolImpl};
use omiga_lib::infrastructure::streaming::StreamOutputItem;
use std::sync::Arc;

#[tokio::test]
async fn todo_write_updates_session_store() {
    let store = Arc::new(tokio::sync::Mutex::new(vec![]));
    let ctx = ToolContext::new(std::env::temp_dir()).with_todos(Some(store.clone()));

    let args = TodoWriteArgs {
        todos: vec![TodoItem {
            content: "Do work".to_string(),
            status: TodoStatus::InProgress,
            active_form: "Doing work".to_string(),
        }],
    };

    let mut stream = TodoWriteTool::execute(&ctx, args).await.unwrap();
    let mut out = String::new();
    while let Some(item) = stream.next().await {
        if let StreamOutputItem::Content(s) = item {
            out.push_str(&s);
        }
    }

    assert!(out.contains("Todos have been modified"));
    let g = store.lock().await;
    assert_eq!(g.len(), 1);
    assert_eq!(g[0].status, TodoStatus::InProgress);
}

#[tokio::test]
async fn todo_write_clears_when_all_completed() {
    let store = Arc::new(tokio::sync::Mutex::new(vec![TodoItem {
        content: "Old".to_string(),
        status: TodoStatus::Pending,
        active_form: "Olding".to_string(),
    }]));
    let ctx = ToolContext::new(std::env::temp_dir()).with_todos(Some(store.clone()));

    let args = TodoWriteArgs {
        todos: vec![TodoItem {
            content: "Old".to_string(),
            status: TodoStatus::Completed,
            active_form: "Olding".to_string(),
        }],
    };

    let mut stream = TodoWriteTool::execute(&ctx, args).await.unwrap();
    while stream.next().await.is_some() {}

    let g = store.lock().await;
    assert!(g.is_empty());
}
