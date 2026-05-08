//! Public JSON output compatibility for retrieval responses.
//!
//! `RetrievalResponse` is an internal, source-neutral shape. These helpers keep
//! the model-visible `search`, `fetch`, and `query` tools compatible with their
//! existing SerpAPI-style/detail JSON outputs while adding lightweight routing
//! audit fields.

use super::types::{
    public_category, RetrievalItem, RetrievalOperation, RetrievalProviderKind, RetrievalRequest,
    RetrievalResponse,
};
use serde_json::{json, Map, Value as JsonValue};

pub fn search_json(request: &RetrievalRequest, response: &RetrievalResponse) -> JsonValue {
    if let Some(raw) = response
        .raw
        .as_ref()
        .filter(|raw| raw.get("error").is_some())
    {
        return annotate_top_level(raw.clone(), request, response);
    }

    let results = response
        .items
        .iter()
        .enumerate()
        .map(|(idx, item)| item_to_search_result(item, response, idx + 1))
        .collect::<Vec<_>>();

    let mut out = json!({
        "query": request.query.as_deref().unwrap_or_default(),
        "category": response.public_category(),
        "source": response.source,
        "effective_source": response.effective_source,
        "provider": response.provider.as_str(),
        "total": response.total.unwrap_or(results.len() as u64),
        "count": results.len(),
        "route_notes": response.notes,
        "results": results,
    });
    if let Some(plugin) = response.plugin.as_deref() {
        out["plugin"] = json!(plugin);
    }
    if response.provider == RetrievalProviderKind::Builtin {
        merge_raw_builtin_top_level(&mut out, response.raw.as_ref());
    }
    out
}

pub fn fetch_json(request: &RetrievalRequest, response: &RetrievalResponse) -> JsonValue {
    if let Some(raw) = response.raw.as_ref().filter(|_| response.detail.is_none()) {
        return annotate_top_level(raw.clone(), request, response);
    }

    let detail = response
        .detail
        .as_ref()
        .or_else(|| response.items.first())
        .cloned()
        .unwrap_or_default();
    let title = title_for_item(&detail);
    let url = detail.url.clone().unwrap_or_default();
    let mut out = json!({
        "category": response.public_category(),
        "source": response.source,
        "effective_source": response.effective_source,
        "provider": response.provider.as_str(),
        "id": detail.id.as_deref(),
        "accession": detail.accession.as_deref(),
        "title": title,
        "name": title,
        "link": url,
        "url": url,
        "displayed_link": displayed_link_for_url(detail.url.as_deref().unwrap_or_default()),
        "favicon": detail.favicon.clone().or_else(|| favicon_for_url(detail.url.as_deref().unwrap_or_default())),
        "snippet": detail.snippet.as_deref().unwrap_or_default(),
        "content": detail.content.as_deref().unwrap_or_default(),
        "prompt": request.prompt.as_deref().unwrap_or_default(),
        "metadata": metadata_with_raw(&detail),
        "route_notes": response.notes,
    });
    if let Some(requested_url) = request.url.as_deref() {
        out["requested_url"] = json!(requested_url);
    }
    if let Some(plugin) = response.plugin.as_deref() {
        out["plugin"] = json!(plugin);
    }
    if response.provider == RetrievalProviderKind::Builtin {
        merge_raw_builtin_top_level(&mut out, response.raw.as_ref());
    }
    out
}

pub fn query_json(request: &RetrievalRequest, response: &RetrievalResponse) -> JsonValue {
    let mut out = match response.operation {
        RetrievalOperation::Search | RetrievalOperation::Query => search_json(request, response),
        RetrievalOperation::Fetch | RetrievalOperation::Resolve => fetch_json(request, response),
        RetrievalOperation::DownloadSummary => response
            .raw
            .clone()
            .map(|raw| annotate_top_level(raw, request, response))
            .unwrap_or_else(|| fetch_json(request, response)),
    };

    if let Some(obj) = out.as_object_mut() {
        obj.insert("tool".to_string(), json!("query"));
        obj.insert("operation".to_string(), json!(response.operation.as_str()));
        obj.entry("category".to_string())
            .or_insert_with(|| json!(public_category(&response.category)));
    }
    out
}

pub fn structured_error_json(
    request: &RetrievalRequest,
    error: &super::types::RetrievalError,
) -> JsonValue {
    error.structured_json(request.public_category(), &request.source)
}

fn item_to_search_result(
    item: &RetrievalItem,
    response: &RetrievalResponse,
    position: usize,
) -> JsonValue {
    let title = title_for_item(item);
    let url = item.url.clone().unwrap_or_default();
    let category =
        raw_string(item, "category").unwrap_or_else(|| response.public_category().to_string());
    let source = raw_string(item, "source").unwrap_or_else(|| response.source.clone());
    let effective_source =
        raw_string(item, "effective_source").unwrap_or_else(|| response.effective_source.clone());
    let displayed_link =
        raw_string(item, "displayed_link").unwrap_or_else(|| displayed_link_for_url(&url));
    let favicon = item
        .favicon
        .clone()
        .or_else(|| raw_string(item, "favicon"))
        .or_else(|| favicon_for_url(&url));
    let mut out = json!({
        "position": position,
        "category": category,
        "source": source,
        "effective_source": effective_source,
        "provider": response.provider.as_str(),
        "title": title,
        "name": title,
        "link": url,
        "url": url,
        "displayed_link": displayed_link,
        "favicon": favicon,
        "snippet": item.snippet.clone().unwrap_or_default(),
        "id": item.id.as_deref(),
        "accession": item.accession.as_deref(),
        "metadata": metadata_with_raw(item),
    });
    if let Some(plugin) = response.plugin.as_deref() {
        out["plugin"] = json!(plugin);
    }
    out
}

fn raw_string(item: &RetrievalItem, key: &str) -> Option<String> {
    item.raw
        .as_ref()
        .and_then(|raw| raw.get(key))
        .and_then(JsonValue::as_str)
        .map(ToString::to_string)
        .filter(|value| !value.trim().is_empty())
}

fn merge_raw_builtin_top_level(out: &mut JsonValue, raw: Option<&JsonValue>) {
    let (Some(out_obj), Some(raw_obj)) = (out.as_object_mut(), raw.and_then(JsonValue::as_object))
    else {
        return;
    };
    for (key, value) in raw_obj {
        if matches!(
            key.as_str(),
            "results" | "error" | "message" | "provider" | "plugin"
        ) {
            continue;
        }
        out_obj.insert(key.clone(), value.clone());
    }
}

fn annotate_top_level(
    mut value: JsonValue,
    request: &RetrievalRequest,
    response: &RetrievalResponse,
) -> JsonValue {
    if let Some(obj) = value.as_object_mut() {
        obj.entry("category".to_string())
            .or_insert_with(|| json!(response.public_category()));
        obj.entry("source".to_string())
            .or_insert_with(|| json!(response.source));
        obj.entry("effective_source".to_string())
            .or_insert_with(|| json!(response.effective_source));
        obj.insert("provider".to_string(), json!(response.provider.as_str()));
        if let Some(plugin) = response.plugin.as_deref() {
            obj.insert("plugin".to_string(), json!(plugin));
        }
        if let Some(prompt) = request.prompt.as_deref() {
            obj.entry("prompt".to_string())
                .or_insert_with(|| json!(prompt));
        }
    }
    value
}

fn metadata_with_raw(item: &RetrievalItem) -> JsonValue {
    let mut metadata = item.metadata.clone();
    if !metadata.is_object() {
        metadata = json!({ "value": metadata });
    }
    if let (Some(obj), Some(raw)) = (metadata.as_object_mut(), item.raw.as_ref()) {
        obj.entry("source_specific".to_string())
            .or_insert_with(|| raw.clone());
    }
    metadata
}

fn title_for_item(item: &RetrievalItem) -> String {
    item.title
        .as_deref()
        .or(item.id.as_deref())
        .or(item.accession.as_deref())
        .or(item.url.as_deref())
        .unwrap_or_default()
        .to_string()
}

fn displayed_link_for_url(url: &str) -> String {
    let Ok(parsed) = reqwest::Url::parse(url) else {
        return url.to_string();
    };
    let host = parsed
        .host_str()
        .unwrap_or_default()
        .trim_start_matches("www.");
    let mut out = host.to_string();
    let path = parsed.path().trim_end_matches('/');
    if !path.is_empty() && path != "/" {
        out.push_str(path);
    }
    out
}

fn favicon_for_url(url: &str) -> Option<String> {
    let parsed = reqwest::Url::parse(url).ok()?;
    let host = parsed.host_str()?.to_ascii_lowercase();
    Some(format!(
        "https://www.google.com/s2/favicons?domain={}&sz=64",
        host.trim_start_matches("www.")
    ))
}

#[allow(dead_code)]
fn empty_metadata() -> JsonValue {
    JsonValue::Object(Map::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::retrieval::types::{
        RetrievalProviderKind, RetrievalTool, RetrievalWebOptions,
    };

    fn request(tool: RetrievalTool, operation: RetrievalOperation) -> RetrievalRequest {
        RetrievalRequest {
            request_id: "req-1".to_string(),
            tool,
            operation,
            category: "dataset".to_string(),
            source: "mock_source".to_string(),
            subcategory: None,
            query: Some("brca1".to_string()),
            id: None,
            url: None,
            result: None,
            params: None,
            max_results: Some(5),
            prompt: Some("extract".to_string()),
            web: Some(RetrievalWebOptions::default()),
        }
    }

    fn response(operation: RetrievalOperation) -> RetrievalResponse {
        RetrievalResponse {
            operation,
            category: "dataset".to_string(),
            source: "mock_source".to_string(),
            effective_source: "mock_source".to_string(),
            provider: RetrievalProviderKind::Plugin,
            plugin: Some("mock@tests".to_string()),
            items: vec![RetrievalItem {
                id: Some("mock-1".to_string()),
                accession: Some("ACC1".to_string()),
                title: Some("Mock Result".to_string()),
                url: Some("https://example.test/mock-1".to_string()),
                snippet: Some("Snippet".to_string()),
                content: Some("Content".to_string()),
                favicon: None,
                metadata: json!({"organism":"human"}),
                raw: Some(json!({"native": true})),
            }],
            detail: None,
            total: Some(9),
            notes: vec!["handled by mock".to_string()],
            raw: None,
        }
    }

    #[test]
    fn search_json_preserves_serpapi_shape_and_audit_fields() {
        let req = request(RetrievalTool::Search, RetrievalOperation::Search);
        let res = response(RetrievalOperation::Search);

        let json = search_json(&req, &res);

        assert_eq!(json["query"], "brca1");
        assert_eq!(json["category"], "data");
        assert_eq!(json["provider"], "plugin");
        assert_eq!(json["plugin"], "mock@tests");
        assert_eq!(json["total"], 9);
        assert_eq!(json["results"][0]["position"], 1);
        assert_eq!(json["results"][0]["title"], "Mock Result");
        assert_eq!(json["results"][0]["name"], "Mock Result");
        assert_eq!(json["results"][0]["link"], "https://example.test/mock-1");
        assert_eq!(json["results"][0]["displayed_link"], "example.test/mock-1");
        assert_eq!(
            json["results"][0]["metadata"]["source_specific"]["native"],
            true
        );
    }

    #[test]
    fn fetch_json_uses_detail_shape() {
        let mut res = response(RetrievalOperation::Fetch);
        res.detail = res.items.first().cloned();
        res.items.clear();
        let req = request(RetrievalTool::Fetch, RetrievalOperation::Fetch);

        let json = fetch_json(&req, &res);

        assert_eq!(json["category"], "data");
        assert_eq!(json["title"], "Mock Result");
        assert_eq!(json["content"], "Content");
        assert_eq!(json["prompt"], "extract");
        assert_eq!(json["provider"], "plugin");
    }

    #[test]
    fn query_json_adds_tool_and_operation_annotations() {
        let req = request(RetrievalTool::Query, RetrievalOperation::Search);
        let res = response(RetrievalOperation::Search);

        let json = query_json(&req, &res);

        assert_eq!(json["tool"], "query");
        assert_eq!(json["operation"], "search");
        assert!(json["results"].is_array());
    }
}
