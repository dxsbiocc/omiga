use super::super::dispatch::ToolDispatchContext;

pub(super) fn is_web_tool(tool_name: &str) -> bool {
    matches!(
        tool_name,
        "fetch" | "Fetch" | "search" | "Search" | "web_search" | "web_fetch"
    )
}

pub(super) async fn handle_web_tool(ctx: &ToolDispatchContext<'_>) -> (String, String, bool) {
    super::execute_domain_tool(ctx).await
}

/// Fire-and-forget: register a web source after a successful tool call.
/// Spawns a Tokio task so it never blocks the tool execution pipeline.
pub(super) fn fetch_source_url_from_args(arguments: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(arguments)
        .ok()
        .and_then(|value| source_url_from_fetch_value(&value))
}

pub(super) fn fetch_source_url_from_args_and_output(
    arguments: &str,
    output: &str,
) -> Option<String> {
    fetch_source_url_from_args(arguments).or_else(|| {
        parse_json_value_from_tool_output(output)
            .and_then(|value| source_url_from_fetch_value(&value))
    })
}

fn parse_json_value_from_tool_output(output: &str) -> Option<serde_json::Value> {
    serde_json::from_str::<serde_json::Value>(output)
        .ok()
        .or_else(|| {
            let start = output.find('{')?;
            let end = output.rfind('}')?;
            if end <= start {
                return None;
            }
            serde_json::from_str::<serde_json::Value>(&output[start..=end]).ok()
        })
}

fn source_url_from_fetch_value(value: &serde_json::Value) -> Option<String> {
    string_field(value, &["url", "link", "href"])
        .or_else(|| value.get("result").and_then(source_url_from_fetch_value))
        .or_else(|| pubmed_url_from_fetch_value(value))
}

fn pubmed_url_from_fetch_value(value: &serde_json::Value) -> Option<String> {
    let category = string_field(value, &["category"]).unwrap_or_default();
    let source = string_field(value, &["source", "effective_source"]).unwrap_or_default();
    let looks_pubmed =
        category.eq_ignore_ascii_case("literature") || source.eq_ignore_ascii_case("pubmed");
    let pmid = string_field(value, &["id", "pmid"])
        .or_else(|| {
            value
                .get("metadata")
                .and_then(|metadata| string_field(metadata, &["pmid"]))
        })
        .filter(|id| id.chars().all(|c| c.is_ascii_digit()));
    if looks_pubmed {
        pmid.map(|id| format!("https://pubmed.ncbi.nlm.nih.gov/{id}/"))
    } else {
        None
    }
}

fn string_field(value: &serde_json::Value, keys: &[&str]) -> Option<String> {
    for key in keys {
        let candidate = value.get(*key).and_then(serde_json::Value::as_str);
        if let Some(value) = candidate.map(str::trim).filter(|s| !s.is_empty()) {
            return Some(value.to_string());
        }
    }
    None
}

pub(super) fn register_web_source_async(
    tool_name: &str,
    arguments: &str,
    output_text: &str,
    session_id: &str,
    project_root: &std::path::Path,
) {
    let tool_name = tool_name.to_string();
    let arguments = arguments.to_string();
    let output = output_text.to_string();
    let session_id = session_id.to_string();
    let project_root = project_root.to_path_buf();

    tokio::spawn(async move {
        let Ok(cfg) = crate::domain::memory::load_resolved_config(&project_root).await else {
            return;
        };
        let lt_root = cfg.long_term_path(&project_root);

        match tool_name.as_str() {
            "fetch" | "Fetch" => {
                // Extract URL from args or the structured result. `fetch` also accepts
                // search-result objects and PubMed PMIDs, so top-level `url` is not enough.
                let url = fetch_source_url_from_args_and_output(&arguments, &output);
                if let Some(url) = url {
                    let entry = crate::domain::memory::source_registry::entry_from_fetch(
                        &url,
                        &output,
                        Some(&session_id),
                        None,
                    );
                    crate::domain::memory::source_registry::upsert_source(&lt_root, entry).await;
                }
            }
            "search" | "Search" => {
                // Extract query from args: {"query":"..."}
                let query = serde_json::from_str::<serde_json::Value>(&arguments)
                    .ok()
                    .and_then(|v| v.get("query").and_then(|q| q.as_str()).map(str::to_owned))
                    .unwrap_or_default();
                let entries = crate::domain::memory::source_registry::entries_from_search_output(
                    &output,
                    Some(&session_id),
                    &query,
                );
                for entry in entries {
                    crate::domain::memory::source_registry::upsert_source(&lt_root, entry).await;
                }
            }
            _ => {}
        }
    });
}

#[cfg(test)]
mod tests {
    use super::{fetch_source_url_from_args, fetch_source_url_from_args_and_output};

    #[test]
    fn fetch_source_url_resolves_search_result_locator() {
        let args = r#"{
            "category": "web",
            "result": {
                "title": "Article",
                "link": "https://example.org/article",
                "favicon": "https://www.google.com/s2/favicons?domain=example.org&sz=64"
            }
        }"#;

        assert_eq!(
            fetch_source_url_from_args(args).as_deref(),
            Some("https://example.org/article")
        );
    }

    #[test]
    fn fetch_source_url_resolves_pubmed_id_locator() {
        let args = r#"{"category":"literature","source":"pubmed","id":"12345678"}"#;

        assert_eq!(
            fetch_source_url_from_args(args).as_deref(),
            Some("https://pubmed.ncbi.nlm.nih.gov/12345678/")
        );
    }

    #[test]
    fn fetch_source_url_falls_back_to_structured_fetch_output() {
        let args = r#"{"category":"web","prompt":"summarize"}"#;
        let output = r#"{
            "category": "web",
            "title": "Fetched",
            "url": "https://example.net/final",
            "favicon": "https://www.google.com/s2/favicons?domain=example.net&sz=64"
        }"#;

        assert_eq!(
            fetch_source_url_from_args_and_output(args, output).as_deref(),
            Some("https://example.net/final")
        );
    }
}
