use super::super::ToolError;
use crate::infrastructure::streaming::{StreamOutput, StreamOutputItem};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};
use std::pin::Pin;

pub(super) const MAX_RESULTS_CAP: usize = 12;
pub(super) const MAX_OUTPUT_CHARS: usize = 100_000;

fn default_max_results() -> Option<u32> {
    Some(5)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchArgs {
    pub category: String,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default, alias = "subCategory", alias = "dataset_type", alias = "type")]
    pub subcategory: Option<String>,
    pub query: String,
    #[serde(default)]
    pub allowed_domains: Option<Vec<String>>,
    #[serde(default)]
    pub blocked_domains: Option<Vec<String>>,
    /// Maximum hits to return (1–10). Default 5.
    #[serde(default = "default_max_results")]
    pub max_results: Option<u32>,
    /// Override HTML search base URL (e.g. `https://html.duckduckgo.com/html/`).
    #[serde(default)]
    pub search_url: Option<String>,
}

#[derive(Debug, Clone)]
pub(in crate::domain::tools::search) struct SearchHit {
    pub(in crate::domain::tools::search) title: String,
    pub(in crate::domain::tools::search) url: String,
    /// Populated for DuckDuckGo HTML / Tavily `content` when available.
    pub(in crate::domain::tools::search) snippet: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::domain::tools::search) enum SearchApiProvider {
    Tavily,
    Exa,
    Firecrawl,
    Parallel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::domain::tools::search) enum SearchMethod {
    Tavily,
    Exa,
    Firecrawl,
    Parallel,
    Ddg,
    Bing,
    Google,
}

#[derive(Debug, Clone)]
pub(in crate::domain::tools::search) struct SearchExecution {
    pub(in crate::domain::tools::search) hits: Vec<SearchHit>,
    pub(in crate::domain::tools::search) source_labels: Vec<String>,
    pub(in crate::domain::tools::search) effective_source: Option<String>,
    pub(in crate::domain::tools::search) notes: Vec<String>,
}

pub(in crate::domain::tools::search) struct SearchMethodRequest<'a> {
    pub(in crate::domain::tools::search) query: &'a str,
    pub(in crate::domain::tools::search) allowed: &'a Option<Vec<String>>,
    pub(in crate::domain::tools::search) blocked: &'a Option<Vec<String>>,
    pub(in crate::domain::tools::search) max_results: usize,
    pub(in crate::domain::tools::search) search_url: Option<&'a str>,
}

impl SearchExecution {
    pub(super) fn new() -> Self {
        Self {
            hits: Vec::new(),
            source_labels: Vec::new(),
            effective_source: None,
            notes: Vec::new(),
        }
    }

    pub(super) fn push_source(&mut self, label: impl Into<String>) {
        let label = label.into();
        if !label.trim().is_empty() && !self.source_labels.iter().any(|s| s == &label) {
            self.source_labels.push(label);
        }
    }

    pub(super) fn push_note(&mut self, note: impl Into<String>) {
        let note = note.into();
        if !note.trim().is_empty() && !self.notes.iter().any(|s| s == &note) {
            self.notes.push(note);
        }
    }
}

pub(super) fn search_method_from_setting(value: &str) -> Option<SearchMethod> {
    match value.trim().to_ascii_lowercase().as_str() {
        "tavily" => Some(SearchMethod::Tavily),
        "exa" => Some(SearchMethod::Exa),
        "firecrawl" => Some(SearchMethod::Firecrawl),
        "parallel" => Some(SearchMethod::Parallel),
        "google" => Some(SearchMethod::Google),
        "bing" => Some(SearchMethod::Bing),
        "duckduckgo" | "duck-duck-go" | "ddg" => Some(SearchMethod::Ddg),
        _ => None,
    }
}

pub(super) fn default_search_methods() -> Vec<SearchMethod> {
    vec![SearchMethod::Ddg, SearchMethod::Google, SearchMethod::Bing]
}

pub(super) fn ordered_search_methods(
    settings: &[String],
    legacy_preferred_engine: &str,
) -> Vec<SearchMethod> {
    let mut out = Vec::new();
    for value in settings {
        let Some(method) = search_method_from_setting(value) else {
            continue;
        };
        if !out.contains(&method) {
            out.push(method);
        }
    }
    if out.is_empty() {
        if let Some(method) = search_method_from_setting(legacy_preferred_engine) {
            out.push(method);
        }
        for method in default_search_methods() {
            if !out.contains(&method) {
                out.push(method);
            }
        }
    }
    out
}

pub(super) fn effective_max_results(args: &SearchArgs) -> usize {
    let m = args.max_results.unwrap_or(5).clamp(1, 10) as usize;
    m.min(MAX_RESULTS_CAP)
}

pub(super) fn validate(args: &SearchArgs) -> Result<(), ToolError> {
    if args.query.trim().len() < 2 {
        return Err(ToolError::InvalidArguments {
            message: "query must be at least 2 characters".to_string(),
        });
    }
    if args.allowed_domains.is_some() && args.blocked_domains.is_some() {
        return Err(ToolError::InvalidArguments {
            message: "Cannot specify both allowed_domains and blocked_domains".to_string(),
        });
    }
    if let Some(ref u) = args.search_url {
        let t = u.trim();
        if !t.is_empty() {
            if !t.starts_with("http://") && !t.starts_with("https://") {
                return Err(ToolError::InvalidArguments {
                    message: "search_url must be an http(s) URL".to_string(),
                });
            }
            if reqwest::Url::parse(t).is_err() {
                return Err(ToolError::InvalidArguments {
                    message: "search_url is not a valid URL".to_string(),
                });
            }
        }
    }
    Ok(())
}

pub(super) fn normalized_category(value: &str) -> String {
    match value.trim().to_ascii_lowercase().replace('-', "_").as_str() {
        "dataset" | "datasets" => "data".to_string(),
        "knowledge_base" | "kb" | "memory" => "knowledge".to_string(),
        other => other.to_string(),
    }
}

pub(super) fn normalized_source(value: Option<&str>) -> String {
    value
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("auto")
        .to_ascii_lowercase()
        .replace('-', "_")
}

pub(super) fn normalized_subcategory(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_ascii_lowercase().replace(['-', ' '], "_"))
}

pub(super) fn recall_scope_for_source(source: &str) -> String {
    match source {
        "auto" => "all",
        "memory" => "implicit",
        "knowledge" | "knowledge_base" => "wiki",
        "session" | "sessions" => "implicit",
        "source" => "sources",
        "all" | "implicit" | "wiki" | "long_term" | "permanent" | "sources" => source,
        _ => "all",
    }
    .to_string()
}

pub(super) fn structured_error_json(
    code: &str,
    category: &str,
    source: &str,
    message: impl Into<String>,
) -> JsonValue {
    json!({
        "error": code,
        "category": category,
        "source": source,
        "message": message.into(),
        "results": [],
    })
}

pub(super) fn json_stream(value: JsonValue) -> crate::infrastructure::streaming::StreamOutputBox {
    let mut text = serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string());
    if text.len() > MAX_OUTPUT_CHARS {
        text.truncate(MAX_OUTPUT_CHARS);
        text.push_str("\n/* Output truncated */");
    }
    SearchOutput { text }.into_stream()
}

#[derive(Debug, Clone)]
struct SearchOutput {
    text: String,
}

impl StreamOutput for SearchOutput {
    fn into_stream(self) -> Pin<Box<dyn futures::Stream<Item = StreamOutputItem> + Send>> {
        use futures::stream;
        Box::pin(stream::iter(vec![
            StreamOutputItem::Start,
            StreamOutputItem::Content(self.text),
            StreamOutputItem::Complete,
        ]))
    }
}
