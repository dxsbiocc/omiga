//! Tests for `notebook_edit`.

use futures::StreamExt;
use omiga_lib::domain::tools::notebook_edit::{NotebookEditArgs, NotebookEditTool};
use omiga_lib::domain::tools::{ToolContext, ToolImpl};
use serde_json::Value;
use tempfile::tempdir;

async fn drain_tool(mut stream: omiga_lib::infrastructure::streaming::StreamOutputBox) {
    while stream.next().await.is_some() {}
}

fn minimal_ipynb() -> &'static str {
    r#"{
  "nbformat": 4,
  "nbformat_minor": 5,
  "metadata": {},
  "cells": [
    {
      "cell_type": "code",
      "id": "a1",
      "metadata": {},
      "source": "print(1)",
      "outputs": [],
      "execution_count": null
    }
  ]
}"#
}

#[tokio::test]
async fn notebook_replace_by_cell_id_updates_source() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("t.ipynb");
    std::fs::write(&path, minimal_ipynb()).unwrap();

    let ctx = ToolContext::new(dir.path());
    let args = NotebookEditArgs {
        notebook_path: "t.ipynb".to_string(),
        cell_id: Some("a1".to_string()),
        new_source: "print(2)".to_string(),
        cell_type: None,
        edit_mode: Some("replace".to_string()),
    };
    drain_tool(NotebookEditTool::execute(&ctx, args).await.unwrap()).await;

    let s = std::fs::read_to_string(&path).unwrap();
    let v: Value = serde_json::from_str(&s).unwrap();
    assert_eq!(v["cells"][0]["source"].as_str(), Some("print(2)"));
}

#[tokio::test]
async fn notebook_replace_by_cell_index() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("t.ipynb");
    std::fs::write(&path, minimal_ipynb()).unwrap();

    let ctx = ToolContext::new(dir.path());
    let args = NotebookEditArgs {
        notebook_path: "t.ipynb".to_string(),
        cell_id: Some("cell-0".to_string()),
        new_source: "x".to_string(),
        cell_type: None,
        edit_mode: None,
    };
    drain_tool(NotebookEditTool::execute(&ctx, args).await.unwrap()).await;

    let s = std::fs::read_to_string(&path).unwrap();
    let v: Value = serde_json::from_str(&s).unwrap();
    assert_eq!(v["cells"][0]["source"].as_str(), Some("x"));
}

#[tokio::test]
async fn notebook_insert_after_cell_0() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("t.ipynb");
    std::fs::write(&path, minimal_ipynb()).unwrap();

    let ctx = ToolContext::new(dir.path());
    let args = NotebookEditArgs {
        notebook_path: "t.ipynb".to_string(),
        cell_id: Some("cell-0".to_string()),
        new_source: "# hi".to_string(),
        cell_type: Some("markdown".to_string()),
        edit_mode: Some("insert".to_string()),
    };
    drain_tool(NotebookEditTool::execute(&ctx, args).await.unwrap()).await;

    let s = std::fs::read_to_string(&path).unwrap();
    let v: Value = serde_json::from_str(&s).unwrap();
    let cells = v["cells"].as_array().unwrap();
    assert_eq!(cells.len(), 2);
    assert_eq!(cells[1]["cell_type"].as_str(), Some("markdown"));
    assert_eq!(cells[1]["source"].as_str(), Some("# hi"));
}

#[tokio::test]
async fn notebook_delete_cell_0() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("t.ipynb");
    std::fs::write(&path, minimal_ipynb()).unwrap();

    let ctx = ToolContext::new(dir.path());
    let args = NotebookEditArgs {
        notebook_path: "t.ipynb".to_string(),
        cell_id: Some("cell-0".to_string()),
        new_source: String::new(),
        cell_type: None,
        edit_mode: Some("delete".to_string()),
    };
    drain_tool(NotebookEditTool::execute(&ctx, args).await.unwrap()).await;

    let s = std::fs::read_to_string(&path).unwrap();
    let v: Value = serde_json::from_str(&s).unwrap();
    assert_eq!(v["cells"].as_array().unwrap().len(), 0);
}
