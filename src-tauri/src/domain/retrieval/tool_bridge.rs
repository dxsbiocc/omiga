//! Tool-entry bridge for unified retrieval routing.
//!
//! `search`, `fetch`, and `query` keep their public schemas, then enter
//! `RetrievalCore` here. Normalized provider responses are rendered through
//! shared output adapters; the only stream passthrough left is the local
//! knowledge-recall compatibility route.

use super::normalize;
use super::output;
use super::providers::routing::{has_plugin_source, RoutedRetrievalProvider};
use super::types::{RetrievalError, RetrievalProviderOutput, RetrievalRequest, RetrievalResponse};
use super::RetrievalCore;
use crate::domain::plugins::{enabled_plugin_retrieval_plugins, PluginRetrievalRegistration};
use crate::domain::tools::{fetch::FetchArgs, query::QueryArgs, search::SearchArgs, ToolContext};
use crate::errors::ToolError;
use crate::infrastructure::streaming::StreamOutputBox;
use serde_json::Value as JsonValue;

pub async fn execute_search(
    ctx: &ToolContext,
    args: SearchArgs,
) -> Result<StreamOutputBox, ToolError> {
    crate::domain::tools::search::validate_search_args(&args)?;
    let request = normalize::search_request(&args).map_err(ToolError::from)?;
    execute_with_registrations(
        ctx,
        request,
        enabled_plugin_retrieval_plugins(),
        output::search_json,
    )
    .await
}

pub async fn execute_fetch(
    ctx: &ToolContext,
    args: FetchArgs,
) -> Result<StreamOutputBox, ToolError> {
    let request = normalize::fetch_request(&args).map_err(ToolError::from)?;
    execute_with_registrations(
        ctx,
        request,
        enabled_plugin_retrieval_plugins(),
        output::fetch_json,
    )
    .await
}

pub async fn execute_query(
    ctx: &ToolContext,
    args: QueryArgs,
) -> Result<StreamOutputBox, ToolError> {
    let request = normalize::query_request(&args).map_err(ToolError::from)?;
    execute_with_registrations(
        ctx,
        request,
        enabled_plugin_retrieval_plugins(),
        output::query_json,
    )
    .await
}

async fn execute_with_registrations(
    ctx: &ToolContext,
    request: RetrievalRequest,
    registrations: Vec<PluginRetrievalRegistration>,
    render: fn(&RetrievalRequest, &RetrievalResponse) -> JsonValue,
) -> Result<StreamOutputBox, ToolError> {
    let explicit_plugin_source = has_plugin_source(&registrations, &request);
    let core = RetrievalCore::new(RoutedRetrievalProvider::new(registrations));
    match core.execute(ctx, request.clone()).await {
        Ok(RetrievalProviderOutput::Stream(stream)) => Ok(stream),
        Ok(RetrievalProviderOutput::Response(response)) => {
            Ok(json_stream(render(&request, &response)))
        }
        Err(RetrievalError::Cancelled) => Err(ToolError::Cancelled),
        Err(error) if explicit_plugin_source => {
            Ok(json_stream(output::structured_error_json(&request, &error)))
        }
        Err(error) => Err(ToolError::from(error)),
    }
}

fn json_stream(value: JsonValue) -> StreamOutputBox {
    let text = serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string());
    crate::infrastructure::streaming::stream_from_iter(vec![
        crate::infrastructure::streaming::StreamOutputItem::Start,
        crate::infrastructure::streaming::StreamOutputItem::Content(text),
        crate::infrastructure::streaming::StreamOutputItem::Complete,
    ])
}

#[allow(dead_code)]
fn _assert_error_send_sync(_: RetrievalError) {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::retrieval::plugin::manifest::load_plugin_retrieval_manifest;
    use crate::domain::tools::WebSearchApiKeys;
    use crate::infrastructure::streaming::StreamOutputItem;
    use futures::StreamExt;
    use serde_json::json;
    use std::collections::HashMap;
    use std::fs;

    #[cfg(unix)]
    fn make_executable(path: &std::path::Path) {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(path).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(path, perms).unwrap();
    }

    fn executable_registration() -> (tempfile::TempDir, PluginRetrievalRegistration) {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("mock_plugin.py");
        fs::write(&script, MOCK_PLUGIN).unwrap();
        #[cfg(unix)]
        make_executable(&script);
        let manifest = load_plugin_retrieval_manifest(
            dir.path(),
            json!({
                "protocolVersion": 1,
                "runtime": {
                    "command": "./mock_plugin.py",
                    "requestTimeoutMs": 5_000,
                    "concurrency": 1
                },
                "sources": [{
                    "id": "mock_source",
                    "category": "dataset",
                    "capabilities": ["search", "fetch", "query"]
                }]
            }),
        )
        .unwrap();
        (
            dir,
            PluginRetrievalRegistration {
                plugin_id: "mock".to_string(),
                plugin_root: manifest.runtime.cwd.clone(),
                retrieval: manifest,
            },
        )
    }

    fn enabled_ctx() -> ToolContext {
        let mut enabled = HashMap::new();
        enabled.insert("dataset".to_string(), vec!["mock_source".to_string()]);
        ToolContext::new("/tmp").with_web_search_api_keys(WebSearchApiKeys {
            enabled_sources_by_category: Some(enabled),
            ..WebSearchApiKeys::default()
        })
    }

    fn search_args(source: &str) -> SearchArgs {
        SearchArgs {
            category: "dataset".to_string(),
            source: Some(source.to_string()),
            subcategory: None,
            query: "hello".to_string(),
            allowed_domains: None,
            blocked_domains: None,
            max_results: Some(3),
            search_url: None,
        }
    }

    fn fetch_args(source: &str) -> FetchArgs {
        FetchArgs {
            category: "dataset".to_string(),
            source: Some(source.to_string()),
            subcategory: None,
            url: None,
            id: Some("mock-1".to_string()),
            result: None,
            prompt: Some("summarize bridge detail".to_string()),
        }
    }

    fn query_search_args(source: &str) -> QueryArgs {
        QueryArgs {
            category: "dataset".to_string(),
            source: Some(source.to_string()),
            operation: Some("search".to_string()),
            subcategory: None,
            query: Some("hello".to_string()),
            id: None,
            url: None,
            result: None,
            params: None,
            max_results: Some(3),
        }
    }

    fn query_fetch_args(source: &str) -> QueryArgs {
        QueryArgs {
            category: "dataset".to_string(),
            source: Some(source.to_string()),
            operation: Some("fetch".to_string()),
            subcategory: None,
            query: None,
            id: Some("mock-1".to_string()),
            url: None,
            result: None,
            params: None,
            max_results: None,
        }
    }

    async fn stream_text(mut stream: StreamOutputBox) -> String {
        let mut out = String::new();
        while let Some(item) = stream.next().await {
            match item {
                StreamOutputItem::Text(text) | StreamOutputItem::Content(text) => {
                    out.push_str(&text)
                }
                _ => {}
            }
        }
        out
    }

    const MOCK_PLUGIN: &str = r#"#!/usr/bin/env python3
import json
import sys

for line in sys.stdin:
    msg = json.loads(line)
    if msg.get("type") == "initialize":
        print(json.dumps({
            "id": msg["id"],
            "type": "initialized",
            "protocolVersion": 1,
            "sources": [{"category":"dataset", "id":"mock_source", "capabilities":["search", "fetch", "query"]}]
        }), flush=True)
    elif msg.get("type") == "execute":
        req = msg["request"]
        operation = req.get("operation")
        if operation in ("search", "query"):
            response = {
                "ok": True,
                "operation": operation,
                "category": req.get("category"),
                "source": req.get("source"),
                "effectiveSource": req.get("source"),
                "items": [{
                    "id": "mock-1",
                    "title": "Bridge Result",
                    "url": "https://example.test/a",
                    "snippet": "Plugin search snippet",
                    "metadata": {"mode": operation}
                }],
                "total": 1,
                "notes": ["plugin search route"]
            }
        elif operation == "fetch":
            response = {
                "ok": True,
                "operation": "fetch",
                "category": req.get("category"),
                "source": req.get("source"),
                "effectiveSource": req.get("source"),
                "detail": {
                    "id": req.get("id") or "mock-1",
                    "title": "Bridge Detail",
                    "url": "https://example.test/detail",
                    "snippet": "Plugin detail snippet",
                    "content": "Plugin detail body",
                    "metadata": {
                        "requested_id": req.get("id"),
                        "prompt": req.get("prompt")
                    },
                    "raw": {"kind": "detail"}
                },
                "total": 1,
                "notes": ["plugin fetch route"]
            }
        else:
            response = {
                "ok": False,
                "operation": operation or "unknown",
                "category": req.get("category"),
                "source": req.get("source")
            }
        print(json.dumps({
            "id": msg["id"],
            "type": "result",
            "response": response
        }), flush=True)
    elif msg.get("type") == "shutdown":
        print(json.dumps({"id": msg["id"], "type": "shutdown"}), flush=True)
        break
"#;

    #[tokio::test]
    async fn bridge_executes_plugin_search_and_renders_search_json() {
        let (_dir, registration) = executable_registration();
        let request = normalize::search_request(&search_args("mock_source")).unwrap();

        let stream = execute_with_registrations(
            &enabled_ctx(),
            request,
            vec![registration],
            output::search_json,
        )
        .await
        .unwrap();
        let value: JsonValue = serde_json::from_str(&stream_text(stream).await).unwrap();

        assert_eq!(value["provider"], json!("plugin"));
        assert_eq!(value["plugin"], json!("mock"));
        assert_eq!(value["results"][0]["title"], json!("Bridge Result"));
    }

    #[tokio::test]
    async fn bridge_executes_plugin_fetch_and_renders_fetch_json() {
        let (_dir, registration) = executable_registration();
        let request = normalize::fetch_request(&fetch_args("mock_source")).unwrap();

        let stream = execute_with_registrations(
            &enabled_ctx(),
            request,
            vec![registration],
            output::fetch_json,
        )
        .await
        .unwrap();
        let value: JsonValue = serde_json::from_str(&stream_text(stream).await).unwrap();

        assert_eq!(value["provider"], json!("plugin"));
        assert_eq!(value["plugin"], json!("mock"));
        assert_eq!(value["category"], json!("data"));
        assert_eq!(value["source"], json!("mock_source"));
        assert_eq!(value["id"], json!("mock-1"));
        assert_eq!(value["title"], json!("Bridge Detail"));
        assert_eq!(value["content"], json!("Plugin detail body"));
        assert_eq!(value["prompt"], json!("summarize bridge detail"));
        assert_eq!(value["metadata"]["requested_id"], json!("mock-1"));
        assert_eq!(
            value["metadata"]["source_specific"]["kind"],
            json!("detail")
        );
    }

    #[tokio::test]
    async fn bridge_executes_plugin_query_search_and_renders_query_json() {
        let (_dir, registration) = executable_registration();
        let request = normalize::query_request(&query_search_args("mock_source")).unwrap();

        let stream = execute_with_registrations(
            &enabled_ctx(),
            request,
            vec![registration],
            output::query_json,
        )
        .await
        .unwrap();
        let value: JsonValue = serde_json::from_str(&stream_text(stream).await).unwrap();

        assert_eq!(value["provider"], json!("plugin"));
        assert_eq!(value["plugin"], json!("mock"));
        assert_eq!(value["tool"], json!("query"));
        assert_eq!(value["operation"], json!("search"));
        assert_eq!(value["results"][0]["title"], json!("Bridge Result"));
        assert_eq!(value["results"][0]["metadata"]["mode"], json!("search"));
    }

    #[tokio::test]
    async fn bridge_executes_plugin_query_fetch_and_renders_query_json() {
        let (_dir, registration) = executable_registration();
        let request = normalize::query_request(&query_fetch_args("mock_source")).unwrap();

        let stream = execute_with_registrations(
            &enabled_ctx(),
            request,
            vec![registration],
            output::query_json,
        )
        .await
        .unwrap();
        let value: JsonValue = serde_json::from_str(&stream_text(stream).await).unwrap();

        assert_eq!(value["provider"], json!("plugin"));
        assert_eq!(value["plugin"], json!("mock"));
        assert_eq!(value["tool"], json!("query"));
        assert_eq!(value["operation"], json!("fetch"));
        assert_eq!(value["id"], json!("mock-1"));
        assert_eq!(value["title"], json!("Bridge Detail"));
        assert_eq!(value["content"], json!("Plugin detail body"));
    }

    #[tokio::test]
    async fn bridge_returns_structured_error_for_disabled_plugin_source() {
        let (_dir, registration) = executable_registration();
        let request = normalize::search_request(&search_args("mock_source")).unwrap();

        let stream = execute_with_registrations(
            &ToolContext::new("/tmp"),
            request,
            vec![registration],
            output::search_json,
        )
        .await
        .unwrap();
        let value: JsonValue = serde_json::from_str(&stream_text(stream).await).unwrap();

        assert_eq!(value["error"], json!("source_disabled"));
        assert_eq!(value["results"], json!([]));
    }
}
