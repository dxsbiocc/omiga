//! Unified search tool — web, literature, and extension-ready source routing
//!
//! Public fallback engines:
//! - DuckDuckGo: the public `api.duckduckgo.com` endpoint is an **instant-answer** API, not full
//!   web search — many queries return empty `AbstractURL` / `RelatedTopics`. HTML results at
//!   `html.duckduckgo.com` are used when the JSON API yields nothing.
//! - Bing / Google: lightweight HTML fallbacks used after configured APIs.
//!
//! Result links are often search-engine redirects (`duckduckgo.com/l/?uddg=…`,
//! `bing.com/ck/a?u=…`, `google.com/url?q=…`); we unwrap them to the real destination URL.

use super::{ToolContext, ToolError, ToolSchema};
use crate::infrastructure::streaming::{StreamOutput, StreamOutputItem};
use async_trait::async_trait;
use base64::{
    engine::general_purpose::{STANDARD as BASE64_STANDARD, URL_SAFE, URL_SAFE_NO_PAD},
    Engine as _,
};
use lazy_static::lazy_static;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};
use std::collections::HashSet;
use std::future::Future;
use std::pin::Pin;
use std::time::{Duration, Instant};

/// Hard cap for safety (Tavily / HTML parsing).
const MAX_RESULTS_CAP: usize = 12;
const MAX_OUTPUT_CHARS: usize = 100_000;
/// Per-result snippet cap (Hermes-style: keep tool output lean).
const MAX_SNIPPET_CHARS: usize = 512;
const MAX_TITLE_CHARS: usize = 220;
const SEARCH_METHOD_RETRY_DELAY_MS: u64 = 400;
const SEARCH_METHOD_MAX_ATTEMPTS: usize = 3;
/// `search` is an interactive chat tool: a search that cannot return within
/// this budget is not useful enough to keep the turn blocked.
const SEARCH_MAX_TIMEOUT_SECS: u64 = 30;

pub const DESCRIPTION: &str = r#"Search across typed data-source categories and return formatted results. Web/literature/dataset/social return SerpAPI-style JSON; knowledge returns recall excerpts.

- `category` is required. Categories: `literature`, `dataset` (`data` alias), `knowledge`, `web`, `social`.
- `source` is optional and defaults to `auto`. Web sources: `auto`, `tavily`, `exa`, `firecrawl`, `parallel`, `google`, `bing`, `ddg`. Literature sources: `auto`, `pubmed`, `arxiv`, `crossref`, `openalex`, `biorxiv`, `medrxiv`, `semantic_scholar` (opt-in, API key required). Dataset sources: `auto`, `geo`, `ena`, `ena_run`, `ena_experiment`, `ena_sample`, `ena_analysis`, `ena_assembly`, `ena_sequence`, `cbioportal`, `gtex`. Knowledge sources/scopes: `all`, `wiki`, `implicit`, `long_term`, `permanent`, `sources`. Social sources: `wechat` (opt-in).
- `subcategory` is optional. Prefer `query(category="dataset", operation="search", …)` for structured dataset/database lookup; this dataset path remains as a compatibility search wrapper.
- `source=auto` uses Settings → Search priority for web, PubMed for literature, and a combined GEO + ENA query for dataset.
- Results are returned as formatted JSON with a top-level `results` array and SerpAPI-style fields (`position`, `title`, `name`, `link`, `url`, `displayed_link`, `favicon`, `snippet`, `metadata`).
- Optional `allowed_domains` or `blocked_domains` filter web result URLs (not both).
- `max_results` (default 5, max 10) limits how many hits are returned.
- Optional `search_url` overrides the DuckDuckGo HTML endpoint (for private search proxies or tests).
- Unsafe/private result URLs are filtered out; `search_url` must also be a public-safe HTTP(S) URL.
- After answering, cite sources with markdown links when you use this tool."#;

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
struct SearchHit {
    title: String,
    url: String,
    /// Populated for DuckDuckGo HTML / Tavily `content` when available.
    snippet: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SearchApiProvider {
    Tavily,
    Exa,
    Firecrawl,
    Parallel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SearchMethod {
    Tavily,
    Exa,
    Firecrawl,
    Parallel,
    Ddg,
    Bing,
    Google,
}

#[derive(Debug, Clone)]
struct SearchExecution {
    hits: Vec<SearchHit>,
    source_labels: Vec<String>,
    effective_source: Option<String>,
    notes: Vec<String>,
}

struct SearchMethodRequest<'a> {
    query: &'a str,
    allowed: &'a Option<Vec<String>>,
    blocked: &'a Option<Vec<String>>,
    max_results: usize,
    search_url: Option<&'a str>,
}

impl SearchExecution {
    fn new() -> Self {
        Self {
            hits: Vec::new(),
            source_labels: Vec::new(),
            effective_source: None,
            notes: Vec::new(),
        }
    }

    fn push_source(&mut self, label: impl Into<String>) {
        let label = label.into();
        if !label.trim().is_empty() && !self.source_labels.iter().any(|s| s == &label) {
            self.source_labels.push(label);
        }
    }

    fn push_note(&mut self, note: impl Into<String>) {
        let note = note.into();
        if !note.trim().is_empty() && !self.notes.iter().any(|s| s == &note) {
            self.notes.push(note);
        }
    }
}

const DDG_HTML_SEARCH_BASE: &str = "https://html.duckduckgo.com/html/";
const BING_SEARCH_BASE: &str = "https://www.bing.com/search";
const GOOGLE_SEARCH_BASE: &str = "https://www.google.com/search";

fn join_labels(labels: &[String]) -> String {
    if labels.is_empty() {
        "Unknown source".to_string()
    } else {
        labels.join(" + ")
    }
}

fn api_provider_label(provider: SearchApiProvider) -> &'static str {
    match provider {
        SearchApiProvider::Tavily => "Tavily Search API",
        SearchApiProvider::Exa => "Exa Search API",
        SearchApiProvider::Firecrawl => "Firecrawl Search API",
        SearchApiProvider::Parallel => "Parallel Search API",
    }
}

fn search_method_from_setting(value: &str) -> Option<SearchMethod> {
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

fn search_method_label(method: SearchMethod) -> &'static str {
    match method {
        SearchMethod::Tavily => "Tavily",
        SearchMethod::Exa => "Exa",
        SearchMethod::Firecrawl => "Firecrawl",
        SearchMethod::Parallel => "Parallel",
        SearchMethod::Ddg => "DuckDuckGo",
        SearchMethod::Bing => "Bing",
        SearchMethod::Google => "Google",
    }
}

fn default_search_methods() -> Vec<SearchMethod> {
    vec![SearchMethod::Ddg, SearchMethod::Google, SearchMethod::Bing]
}

fn ordered_search_methods(settings: &[String], legacy_preferred_engine: &str) -> Vec<SearchMethod> {
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

fn search_method_missing_config(ctx: &ToolContext, method: SearchMethod) -> Option<&'static str> {
    match method {
        SearchMethod::Tavily if resolve_tavily_api_key(ctx).is_none() => {
            Some("Tavily API key is not configured")
        }
        SearchMethod::Exa if resolve_exa_api_key(ctx).is_none() => {
            Some("Exa API key is not configured")
        }
        SearchMethod::Firecrawl if resolve_firecrawl_api_key(ctx).is_none() => {
            Some("Firecrawl API key is not configured")
        }
        SearchMethod::Parallel if resolve_parallel_api_key(ctx).is_none() => {
            Some("Parallel API key is not configured")
        }
        _ => None,
    }
}

fn search_api_provider_for_method(method: SearchMethod) -> Option<SearchApiProvider> {
    match method {
        SearchMethod::Tavily => Some(SearchApiProvider::Tavily),
        SearchMethod::Exa => Some(SearchApiProvider::Exa),
        SearchMethod::Firecrawl => Some(SearchApiProvider::Firecrawl),
        SearchMethod::Parallel => Some(SearchApiProvider::Parallel),
        SearchMethod::Ddg | SearchMethod::Bing | SearchMethod::Google => None,
    }
}

fn search_method_source_label(method: SearchMethod) -> String {
    if let Some(provider) = search_api_provider_for_method(method) {
        api_provider_label(provider).to_string()
    } else {
        format!("{} public search", search_method_label(method))
    }
}

fn effective_max_results(args: &SearchArgs) -> usize {
    let m = args.max_results.unwrap_or(5).clamp(1, 10) as usize;
    m.min(MAX_RESULTS_CAP)
}

fn effective_search_timeout(timeout_secs: u64) -> Duration {
    Duration::from_secs(timeout_secs.clamp(5, SEARCH_MAX_TIMEOUT_SECS))
}

fn search_timeout_error(timeout: Duration) -> ToolError {
    let timeout_secs = timeout.as_secs().max(1);
    ToolError::ExecutionFailed {
        message: format!(
            "search timed out after {}s. Try a narrower query or configure a faster search API provider.",
            timeout_secs
        ),
    }
}

async fn enforce_search_timeout<F>(timeout: Duration, fut: F) -> Result<SearchExecution, ToolError>
where
    F: Future<Output = Result<SearchExecution, ToolError>>,
{
    match tokio::time::timeout(timeout, fut).await {
        Ok(result) => result,
        Err(_) => Err(search_timeout_error(timeout)),
    }
}

pub struct SearchTool;

/// Settings key wins, then env vars.
fn resolve_tavily_api_key(ctx: &ToolContext) -> Option<String> {
    if let Some(ref k) = ctx.web_search_api_keys.tavily {
        let t = k.trim();
        if !t.is_empty() {
            return Some(t.to_string());
        }
    }
    std::env::var("OMIGA_TAVILY_API_KEY")
        .ok()
        .or_else(|| std::env::var("TAVILY_API_KEY").ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn resolve_exa_api_key(ctx: &ToolContext) -> Option<String> {
    if let Some(ref k) = ctx.web_search_api_keys.exa {
        let t = k.trim();
        if !t.is_empty() {
            return Some(t.to_string());
        }
    }
    std::env::var("OMIGA_EXA_API_KEY")
        .ok()
        .or_else(|| std::env::var("EXA_API_KEY").ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn resolve_parallel_api_key(ctx: &ToolContext) -> Option<String> {
    if let Some(ref k) = ctx.web_search_api_keys.parallel {
        let t = k.trim();
        if !t.is_empty() {
            return Some(t.to_string());
        }
    }
    std::env::var("OMIGA_PARALLEL_API_KEY")
        .ok()
        .or_else(|| std::env::var("PARALLEL_API_KEY").ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn resolve_firecrawl_api_key(ctx: &ToolContext) -> Option<String> {
    if let Some(ref k) = ctx.web_search_api_keys.firecrawl {
        let t = k.trim();
        if !t.is_empty() {
            return Some(t.to_string());
        }
    }
    std::env::var("OMIGA_FIRECRAWL_API_KEY")
        .ok()
        .or_else(|| std::env::var("FIRECRAWL_API_KEY").ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn resolve_firecrawl_base_url(ctx: &ToolContext) -> String {
    if let Some(ref u) = ctx.web_search_api_keys.firecrawl_url {
        let t = u.trim();
        if !t.is_empty() {
            return t.trim_end_matches('/').to_string();
        }
    }
    std::env::var("OMIGA_FIRECRAWL_API_URL")
        .ok()
        .or_else(|| std::env::var("FIRECRAWL_API_URL").ok())
        .map(|s| s.trim().trim_end_matches('/').to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "https://api.firecrawl.dev".to_string())
}

async fn search_once_with_method(
    client: &reqwest::Client,
    ctx: &ToolContext,
    method: SearchMethod,
    request: &SearchMethodRequest<'_>,
) -> Result<Vec<SearchHit>, ToolError> {
    let max_u32 = request.max_results.min(20) as u32;
    match method {
        SearchMethod::Tavily => {
            let key = resolve_tavily_api_key(ctx).ok_or_else(|| ToolError::ExecutionFailed {
                message: "Tavily API key is not configured".to_string(),
            })?;
            search_tavily_once(
                client,
                &key,
                request.query,
                request.allowed,
                request.blocked,
                max_u32,
            )
            .await
        }
        SearchMethod::Exa => {
            let key = resolve_exa_api_key(ctx).ok_or_else(|| ToolError::ExecutionFailed {
                message: "Exa API key is not configured".to_string(),
            })?;
            search_exa_once(client, &key, request.query, max_u32).await
        }
        SearchMethod::Firecrawl => {
            let key = resolve_firecrawl_api_key(ctx).ok_or_else(|| ToolError::ExecutionFailed {
                message: "Firecrawl API key is not configured".to_string(),
            })?;
            let base = resolve_firecrawl_base_url(ctx);
            search_firecrawl_once(client, &key, &base, request.query, max_u32).await
        }
        SearchMethod::Parallel => {
            let key = resolve_parallel_api_key(ctx).ok_or_else(|| ToolError::ExecutionFailed {
                message: "Parallel API key is not configured".to_string(),
            })?;
            search_parallel_once(client, &key, request.query, max_u32).await
        }
        SearchMethod::Ddg => {
            search_ddg(
                client,
                request.query,
                request.max_results,
                request.search_url,
            )
            .await
        }
        SearchMethod::Bing => search_bing(client, request.query, request.max_results).await,
        SearchMethod::Google => search_google(client, request.query, request.max_results).await,
    }
}

#[allow(clippy::too_many_arguments)]
async fn search_ordered_methods(
    client: &reqwest::Client,
    ctx: &ToolContext,
    query: &str,
    allowed: &Option<Vec<String>>,
    blocked: &Option<Vec<String>>,
    max_results: usize,
    search_url: Option<&str>,
    source: &str,
) -> Result<SearchExecution, ToolError> {
    let methods = web_methods_for_source(ctx, source)?;
    let mut exec = SearchExecution::new();
    exec.push_note(format!(
        "Search order: {}",
        methods
            .iter()
            .map(|m| search_method_label(*m))
            .collect::<Vec<_>>()
            .join(" → ")
    ));

    let request = SearchMethodRequest {
        query,
        allowed,
        blocked,
        max_results,
        search_url,
    };

    for method in methods {
        let label = search_method_source_label(method);
        if let Some(reason) = search_method_missing_config(ctx, method) {
            exec.push_note(format!("{label} skipped: {reason}."));
            continue;
        }

        let mut last_error: Option<String> = None;
        let mut empty_attempts = 0usize;

        for attempt in 1..=SEARCH_METHOD_MAX_ATTEMPTS {
            let result = search_once_with_method(client, ctx, method, &request).await;
            match result {
                Ok(hits) => {
                    let filtered =
                        filter_hits(&ctx.project_root, hits, allowed, blocked, max_results);
                    if !filtered.is_empty() {
                        exec.hits = filtered;
                        exec.effective_source = Some(search_method_source_key(method).to_string());
                        exec.push_source(if attempt == 1 {
                            label
                        } else {
                            format!("{label} (attempt {attempt})")
                        });
                        return Ok(exec);
                    }
                    empty_attempts += 1;
                }
                Err(err) => {
                    last_error = Some(err.to_string());
                }
            }

            if attempt < SEARCH_METHOD_MAX_ATTEMPTS {
                tokio::time::sleep(Duration::from_millis(SEARCH_METHOD_RETRY_DELAY_MS)).await;
            }
        }

        if let Some(err) = last_error {
            exec.push_note(format!(
                "{label} failed after {SEARCH_METHOD_MAX_ATTEMPTS} attempts: {err}"
            ));
        } else if empty_attempts > 0 {
            exec.push_note(format!(
                "{label} returned no usable hits after {empty_attempts} attempts."
            ));
        }
    }

    exec.push_source("No configured search method returned usable hits");
    Ok(exec)
}

fn validate(args: &SearchArgs) -> Result<(), ToolError> {
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

fn host_of_url(url: &str) -> Option<String> {
    reqwest::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(|h| h.to_ascii_lowercase()))
}

fn domain_matches(host: &str, pattern: &str) -> bool {
    let p = pattern
        .trim()
        .trim_start_matches("www.")
        .to_ascii_lowercase();
    let h = host.trim().trim_start_matches("www.");
    h == p || h.ends_with(&format!(".{}", p))
}

fn url_passes_filters(
    url: &str,
    allowed: &Option<Vec<String>>,
    blocked: &Option<Vec<String>>,
) -> bool {
    let Some(host) = host_of_url(url) else {
        return false;
    };
    if let Some(blocks) = blocked {
        for d in blocks {
            if domain_matches(&host, d) {
                return false;
            }
        }
    }
    if let Some(allows) = allowed {
        if allows.is_empty() {
            return true;
        }
        return allows.iter().any(|d| domain_matches(&host, d));
    }
    true
}

fn filter_hits(
    project_root: &std::path::Path,
    hits: Vec<SearchHit>,
    allowed: &Option<Vec<String>>,
    blocked: &Option<Vec<String>>,
    limit: usize,
) -> Vec<SearchHit> {
    hits.into_iter()
        .filter(|h| url_passes_filters(&h.url, allowed, blocked))
        .filter(|h| super::web_safety::is_safe_result_url(project_root, &h.url))
        .take(limit)
        .collect()
}

fn normalize_url_for_dedup(url: &str) -> String {
    url.trim().trim_end_matches('/').to_lowercase()
}

/// Strip inline base64 image payloads (Hermes `clean_base64_images`) and cap length.
fn sanitize_search_text(s: &str, max_chars: usize) -> String {
    lazy_static! {
        static ref RE_BASE64_PARENS: Regex =
            Regex::new(r"\(data:image/[^;]+;base64,[A-Za-z0-9+/=]+\)").expect("regex");
        static ref RE_BASE64_PLAIN: Regex =
            Regex::new(r"data:image/[^;]+;base64,[A-Za-z0-9+/=]+").expect("regex");
        static ref WS: Regex = Regex::new(r"\s+").expect("ws");
    }
    let t = RE_BASE64_PARENS.replace_all(s, "[image omitted]");
    let t = RE_BASE64_PLAIN.replace_all(t.as_ref(), "[image omitted]");
    let t = WS.replace_all(t.trim(), " ");
    let t = t.trim();
    if t.len() <= max_chars {
        t.to_string()
    } else {
        format!(
            "{}…",
            t.chars()
                .take(max_chars.saturating_sub(1))
                .collect::<String>()
        )
    }
}

fn sanitize_hit(h: SearchHit) -> SearchHit {
    SearchHit {
        title: sanitize_search_text(&h.title, MAX_TITLE_CHARS),
        url: h.url.trim().to_string(),
        snippet: sanitize_search_text(&h.snippet, MAX_SNIPPET_CHARS),
    }
}

fn dedupe_hits_preserve_order(hits: Vec<SearchHit>) -> Vec<SearchHit> {
    let mut seen = HashSet::new();
    let mut out = Vec::with_capacity(hits.len());
    for h in hits {
        let k = normalize_url_for_dedup(&h.url);
        if k.is_empty() {
            continue;
        }
        if seen.insert(k) {
            out.push(h);
        }
    }
    out
}

#[derive(Serialize)]
struct TavilySearchRequest<'a> {
    api_key: &'a str,
    query: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    include_domains: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    exclude_domains: Option<Vec<String>>,
    max_results: u32,
    search_depth: &'static str,
    /// Hermes-style: smaller payloads, fewer parse edge cases.
    include_raw_content: bool,
    include_images: bool,
}

async fn search_tavily_once(
    client: &reqwest::Client,
    key: &str,
    query: &str,
    allowed: &Option<Vec<String>>,
    blocked: &Option<Vec<String>>,
    max_results: u32,
) -> Result<Vec<SearchHit>, ToolError> {
    let include_domains = allowed
        .as_ref()
        .filter(|v| !v.is_empty())
        .map(|v| v.iter().map(|s| s.trim().to_string()).collect::<Vec<_>>());
    let exclude_domains = blocked
        .as_ref()
        .filter(|v| !v.is_empty())
        .map(|v| v.iter().map(|s| s.trim().to_string()).collect::<Vec<_>>());

    let body = TavilySearchRequest {
        api_key: key,
        query,
        include_domains,
        exclude_domains,
        max_results: max_results.min(20),
        search_depth: "basic",
        include_raw_content: false,
        include_images: false,
    };

    let resp = client
        .post("https://api.tavily.com/search")
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| ToolError::ExecutionFailed {
            message: format!("Tavily request failed: {}", e),
        })?;

    let status = resp.status();
    if !status.is_success() {
        let body_text = resp.text().await.unwrap_or_default();
        return Err(ToolError::ExecutionFailed {
            message: format!(
                "Tavily HTTP {}: {}",
                status,
                body_text.chars().take(500).collect::<String>()
            ),
        });
    }

    let v: serde_json::Value = resp.json().await.map_err(|e| ToolError::ExecutionFailed {
        message: format!("Tavily JSON: {}", e),
    })?;

    let mut out = Vec::new();
    if let Some(arr) = v.get("results").and_then(|r| r.as_array()) {
        for r in arr {
            let title = r
                .get("title")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            let url = r.get("url").and_then(|x| x.as_str()).unwrap_or("");
            let snippet = r
                .get("content")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            if !url.is_empty() {
                out.push(SearchHit {
                    title,
                    url: url.to_string(),
                    snippet,
                });
            }
        }
    }
    Ok(out)
}

async fn search_exa_once(
    client: &reqwest::Client,
    api_key: &str,
    query: &str,
    max_results: u32,
) -> Result<Vec<SearchHit>, ToolError> {
    let n = max_results.clamp(1, 20);
    let body = serde_json::json!({
        "query": query,
        "numResults": n,
        "contents": { "text": true }
    });
    let resp = client
        .post("https://api.exa.ai/search")
        .header("x-api-key", api_key)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| ToolError::ExecutionFailed {
            message: format!("Exa request failed: {}", e),
        })?;
    let status = resp.status();
    if !status.is_success() {
        let body_text = resp.text().await.unwrap_or_default();
        return Err(ToolError::ExecutionFailed {
            message: format!(
                "Exa HTTP {}: {}",
                status,
                body_text.chars().take(500).collect::<String>()
            ),
        });
    }
    let v: serde_json::Value = resp.json().await.map_err(|e| ToolError::ExecutionFailed {
        message: format!("Exa JSON: {}", e),
    })?;
    let mut out = Vec::new();
    if let Some(arr) = v.get("results").and_then(|r| r.as_array()) {
        for r in arr {
            let title = r
                .get("title")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            let url = r.get("url").and_then(|x| x.as_str()).unwrap_or("");
            let snippet = r
                .get("text")
                .and_then(|x| x.as_str())
                .or_else(|| r.get("snippet").and_then(|x| x.as_str()))
                .unwrap_or("")
                .to_string();
            if !url.is_empty() {
                out.push(SearchHit {
                    title,
                    url: url.to_string(),
                    snippet,
                });
            }
        }
    }
    Ok(out)
}

async fn search_firecrawl_once(
    client: &reqwest::Client,
    api_key: &str,
    base_url: &str,
    query: &str,
    max_results: u32,
) -> Result<Vec<SearchHit>, ToolError> {
    let n = max_results.clamp(1, 20);
    let url = format!("{}/v1/search", base_url);
    let body = serde_json::json!({
        "query": query,
        "limit": n
    });
    let resp = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| ToolError::ExecutionFailed {
            message: format!("Firecrawl request failed: {}", e),
        })?;
    let status = resp.status();
    if !status.is_success() {
        let body_text = resp.text().await.unwrap_or_default();
        return Err(ToolError::ExecutionFailed {
            message: format!(
                "Firecrawl HTTP {}: {}",
                status,
                body_text.chars().take(500).collect::<String>()
            ),
        });
    }
    let v: serde_json::Value = resp.json().await.map_err(|e| ToolError::ExecutionFailed {
        message: format!("Firecrawl JSON: {}", e),
    })?;
    let mut out = Vec::new();
    let data = v
        .get("data")
        .and_then(|d| d.as_array())
        .or_else(|| v.get("results").and_then(|r| r.as_array()));
    if let Some(arr) = data {
        for r in arr {
            let title = r
                .get("title")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            let url = r.get("url").and_then(|x| x.as_str()).unwrap_or("");
            let snippet = r
                .get("description")
                .and_then(|x| x.as_str())
                .or_else(|| r.get("markdown").and_then(|x| x.as_str()))
                .or_else(|| r.get("snippet").and_then(|x| x.as_str()))
                .unwrap_or("")
                .to_string();
            if !url.is_empty() {
                out.push(SearchHit {
                    title,
                    url: url.to_string(),
                    snippet,
                });
            }
        }
    }
    Ok(out)
}

async fn search_parallel_once(
    client: &reqwest::Client,
    api_key: &str,
    query: &str,
    max_results: u32,
) -> Result<Vec<SearchHit>, ToolError> {
    let n = max_results.clamp(1, 20);
    let body = serde_json::json!({
        "objective": query,
        "search_queries": [query],
        "mode": "fast",
        "max_results": n
    });
    let resp = client
        .post("https://api.parallel.ai/v1beta/search")
        .header("x-api-key", api_key)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| ToolError::ExecutionFailed {
            message: format!("Parallel request failed: {}", e),
        })?;
    let status = resp.status();
    if !status.is_success() {
        let body_text = resp.text().await.unwrap_or_default();
        return Err(ToolError::ExecutionFailed {
            message: format!(
                "Parallel HTTP {}: {}",
                status,
                body_text.chars().take(500).collect::<String>()
            ),
        });
    }
    let v: serde_json::Value = resp.json().await.map_err(|e| ToolError::ExecutionFailed {
        message: format!("Parallel JSON: {}", e),
    })?;
    let mut out = Vec::new();
    let items = v
        .pointer("/results")
        .and_then(|x| x.as_array())
        .or_else(|| v.get("search_results").and_then(|x| x.as_array()))
        .or_else(|| v.as_array());
    if let Some(arr) = items {
        for r in arr {
            if r.is_object() {
                let title = r
                    .get("title")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string();
                let url = r.get("url").and_then(|x| x.as_str()).unwrap_or("");
                let snippet = r
                    .get("excerpt")
                    .and_then(|x| x.as_str())
                    .or_else(|| r.get("snippet").and_then(|x| x.as_str()))
                    .or_else(|| r.get("text").and_then(|x| x.as_str()))
                    .unwrap_or("")
                    .to_string();
                if !url.is_empty() {
                    out.push(SearchHit {
                        title,
                        url: url.to_string(),
                        snippet,
                    });
                }
            }
        }
    }
    Ok(out)
}

/// Browser-like UA; DDG often returns non-JSON or empty payloads for generic `reqwest` defaults.
const DDG_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

fn ddg_api_headers() -> reqwest::header::HeaderMap {
    let mut h = reqwest::header::HeaderMap::new();
    h.insert(
        reqwest::header::USER_AGENT,
        reqwest::header::HeaderValue::from_static(DDG_USER_AGENT),
    );
    h.insert(
        reqwest::header::ACCEPT,
        reqwest::header::HeaderValue::from_static("application/json, text/javascript, */*;q=0.1"),
    );
    h.insert(
        reqwest::header::REFERER,
        reqwest::header::HeaderValue::from_static("https://duckduckgo.com/html/"),
    );
    h.insert(
        reqwest::header::HeaderName::from_static("accept-language"),
        reqwest::header::HeaderValue::from_static("en-US,en;q=0.9"),
    );
    h
}

fn ddg_html_headers() -> reqwest::header::HeaderMap {
    let mut h = reqwest::header::HeaderMap::new();
    h.insert(
        reqwest::header::USER_AGENT,
        reqwest::header::HeaderValue::from_static(DDG_USER_AGENT),
    );
    h.insert(
        reqwest::header::ACCEPT,
        reqwest::header::HeaderValue::from_static(
            "text/html,application/xhtml+xml;q=0.9,*/*;q=0.8",
        ),
    );
    h.insert(
        reqwest::header::REFERER,
        reqwest::header::HeaderValue::from_static("https://duckduckgo.com/html/"),
    );
    h.insert(
        reqwest::header::HeaderName::from_static("accept-language"),
        reqwest::header::HeaderValue::from_static("en-US,en;q=0.9"),
    );
    h
}

fn bing_html_headers() -> reqwest::header::HeaderMap {
    let mut h = ddg_html_headers();
    h.insert(
        reqwest::header::REFERER,
        reqwest::header::HeaderValue::from_static("https://www.bing.com/"),
    );
    h
}

fn google_html_headers() -> reqwest::header::HeaderMap {
    let mut h = ddg_html_headers();
    h.insert(
        reqwest::header::REFERER,
        reqwest::header::HeaderValue::from_static("https://www.google.com/"),
    );
    h
}

fn ddg_href_for_parse(raw: &str) -> String {
    raw.replace("&amp;", "&")
}

fn decode_bing_redirect_candidate(value: &str) -> Option<String> {
    let trimmed = value.trim();
    let candidates = if let Some(stripped) = trimmed.strip_prefix("a1") {
        vec![trimmed, stripped]
    } else {
        vec![trimmed]
    };

    for candidate in candidates {
        for decoded in [
            URL_SAFE_NO_PAD.decode(candidate.as_bytes()),
            URL_SAFE.decode(candidate.as_bytes()),
            BASE64_STANDARD.decode(candidate.as_bytes()),
        ] {
            let Ok(bytes) = decoded else {
                continue;
            };
            let Ok(text) = String::from_utf8(bytes) else {
                continue;
            };
            if text.starts_with("http://") || text.starts_with("https://") {
                return Some(text);
            }
        }
    }

    None
}

/// Match OpenHarness: unwrap `uddg` on DDG `/l/` redirects; otherwise return the URL unchanged.
fn normalize_result_url(raw_url: &str) -> String {
    let raw = ddg_href_for_parse(raw_url.trim());
    let fixed = if raw.starts_with("//") {
        format!("https:{}", raw)
    } else if raw.starts_with('/') {
        format!("https://www.google.com{}", raw)
    } else {
        raw
    };
    let Ok(u) = reqwest::Url::parse(&fixed) else {
        return raw_url.to_string();
    };
    let host = u.host_str().unwrap_or("").to_ascii_lowercase();
    if host.ends_with("duckduckgo.com") && u.path().starts_with("/l") {
        for (k, v) in u.query_pairs() {
            if k == "uddg" && !v.is_empty() {
                return v.into_owned();
            }
        }
    }
    if host.ends_with("bing.com") && u.path().starts_with("/ck/") {
        for (k, v) in u.query_pairs() {
            if k == "u" && !v.is_empty() {
                if let Some(decoded) = decode_bing_redirect_candidate(&v) {
                    return decoded;
                }
            }
        }
    }
    if host.ends_with("google.com") && u.path().starts_with("/url") {
        for (k, v) in u.query_pairs() {
            if k == "q" && !v.is_empty() {
                return v.into_owned();
            }
        }
    }
    fixed
}

fn clean_html_fragment(fragment: &str) -> String {
    lazy_static! {
        static ref TAGS: Regex = Regex::new(r"(?s)<[^>]+>").expect("regex");
        static ref WS: Regex = Regex::new(r"\s+").expect("ws");
    }
    let t = TAGS.replace_all(fragment, " ");
    let t = t
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&nbsp;", " ");
    WS.replace_all(t.trim(), " ").to_string()
}

fn collect_ddg_topics(topics: &[serde_json::Value], out: &mut Vec<SearchHit>, max: usize) {
    for item in topics {
        if out.len() >= max {
            break;
        }
        if let Some(obj) = item.as_object() {
            if let (Some(u), Some(text)) = (
                obj.get("FirstURL").and_then(|x| x.as_str()),
                obj.get("Text").and_then(|x| x.as_str()),
            ) {
                if !u.is_empty() {
                    let title = if text.len() > 160 {
                        format!("{}…", &text[..160])
                    } else {
                        text.to_string()
                    };
                    out.push(SearchHit {
                        title,
                        url: u.to_string(),
                        snippet: String::new(),
                    });
                }
                continue;
            }
        }
        if let Some(nested) = item.get("Topics").and_then(|x| x.as_array()) {
            collect_ddg_topics(nested, out, max);
        }
    }
}

/// Parse instant-answer JSON from `api.duckduckgo.com`.
fn parse_ddg_instant_answer_json(body: &str, max: usize) -> Result<Vec<SearchHit>, ToolError> {
    let t = body.trim_start();
    if !t.starts_with('{') {
        return Err(ToolError::ExecutionFailed {
            message: "DuckDuckGo API returned non-JSON body (likely blocked or HTML).".to_string(),
        });
    }
    let v: serde_json::Value =
        serde_json::from_str(body).map_err(|e| ToolError::ExecutionFailed {
            message: format!("DuckDuckGo JSON parse: {}", e),
        })?;

    let mut out = Vec::new();

    if let Some(u) = v.get("AbstractURL").and_then(|x| x.as_str()) {
        if !u.is_empty() && out.len() < max {
            let title = v
                .get("Heading")
                .and_then(|x| x.as_str())
                .or_else(|| v.get("Abstract").and_then(|x| x.as_str()))
                .unwrap_or(u)
                .to_string();
            let title = if title.len() > 160 {
                format!("{}…", &title[..160])
            } else {
                title
            };
            out.push(SearchHit {
                title,
                url: u.to_string(),
                snippet: String::new(),
            });
        }
    }

    if let Some(topics) = v.get("RelatedTopics").and_then(|x| x.as_array()) {
        collect_ddg_topics(topics, &mut out, max);
    }

    Ok(out)
}

lazy_static! {
    /// OpenHarness-style: `result__snippet` / `result-snippet` blocks.
    static ref RE_DDG_SNIPPET: Regex = Regex::new(
        r#"(?is)<(?:a|div|span)[^>]+class="[^"]*(?:result__snippet|result-snippet)[^"]*"[^>]*>(?P<snippet>.*?)</(?:a|div|span)>"#,
    )
    .expect("regex");
    /// All `<a …>title</a>` for class scan + href extraction (aligned with OpenHarness).
    static ref RE_DDG_ANCHOR: Regex =
        Regex::new(r#"(?is)<a(?P<attrs>[^>]+)>(?P<title>.*?)</a>"#).expect("regex");
    static ref RE_CLASS_ATTR: Regex = Regex::new(r#"(?i)class="(?P<class>[^"]+)""#).expect("regex");
    static ref RE_HREF_ATTR: Regex = Regex::new(r#"(?i)href="(?P<href>[^"]+)""#).expect("regex");
    static ref RE_BING_BLOCK: Regex =
        Regex::new(r#"(?is)<li[^>]+class="[^"]*\bb_algo\b[^"]*"[^>]*>(?P<body>.*?)</li>"#)
            .expect("regex");
    static ref RE_BING_SNIPPET: Regex =
        Regex::new(r#"(?is)<p[^>]*>(?P<snippet>.*?)</p>"#).expect("regex");
}

fn extract_ddg_snippets(body: &str) -> Vec<String> {
    RE_DDG_SNIPPET
        .captures_iter(body)
        .filter_map(|c| c.name("snippet").map(|m| clean_html_fragment(m.as_str())))
        .collect()
}

/// HTML result page parser (OpenHarness-style): `result__a` / `result-link`, snippets, `uddg` URLs.
fn parse_ddg_html_results_openharness(body: &str, limit: usize) -> Vec<SearchHit> {
    let snippets = extract_ddg_snippets(body);
    let mut results = Vec::new();
    let mut snip_i = 0usize;

    for cap in RE_DDG_ANCHOR.captures_iter(body) {
        if results.len() >= limit {
            break;
        }
        let attrs = cap.name("attrs").map(|m| m.as_str()).unwrap_or("");
        let Some(class_m) = RE_CLASS_ATTR.captures(attrs) else {
            continue;
        };
        let class_names = class_m.name("class").map(|m| m.as_str()).unwrap_or("");
        if !class_names.contains("result__a") && !class_names.contains("result-link") {
            continue;
        }
        let Some(href_m) = RE_HREF_ATTR.captures(attrs) else {
            continue;
        };
        let raw_href = href_m.name("href").map(|m| m.as_str()).unwrap_or("");
        let title_raw = cap.name("title").map(|m| m.as_str()).unwrap_or("");
        let title = clean_html_fragment(title_raw);
        let url = normalize_result_url(raw_href);
        let snippet = snippets
            .get(snip_i)
            .map(|s| s.as_str())
            .unwrap_or("")
            .to_string();
        snip_i += 1;

        if title.is_empty() || url.is_empty() {
            continue;
        }
        let title = if title.len() > 160 {
            format!("{}…", &title[..160])
        } else {
            title
        };
        results.push(SearchHit {
            title,
            url,
            snippet,
        });
    }

    results
}

fn parse_bing_html_results(body: &str, limit: usize) -> Vec<SearchHit> {
    let mut results = Vec::new();

    for block in RE_BING_BLOCK.captures_iter(body) {
        if results.len() >= limit {
            break;
        }
        let block = block.name("body").map(|m| m.as_str()).unwrap_or("");
        let Some(anchor) = RE_DDG_ANCHOR.captures(block) else {
            continue;
        };
        let attrs = anchor.name("attrs").map(|m| m.as_str()).unwrap_or("");
        let Some(href_m) = RE_HREF_ATTR.captures(attrs) else {
            continue;
        };
        let raw_href = href_m.name("href").map(|m| m.as_str()).unwrap_or("");
        let title_raw = anchor.name("title").map(|m| m.as_str()).unwrap_or("");
        let title = clean_html_fragment(title_raw);
        let url = normalize_result_url(raw_href);
        let snippet = RE_BING_SNIPPET
            .captures(block)
            .and_then(|c| c.name("snippet").map(|m| clean_html_fragment(m.as_str())))
            .unwrap_or_default();

        if title.is_empty() || url.is_empty() {
            continue;
        }
        let title = if title.len() > 160 {
            format!("{}…", &title[..160])
        } else {
            title
        };
        results.push(SearchHit {
            title,
            url,
            snippet,
        });
    }

    dedupe_hits_preserve_order(results.into_iter().map(sanitize_hit).collect())
}

fn parse_google_html_results(body: &str, limit: usize) -> Vec<SearchHit> {
    let mut results = Vec::new();

    for anchor in RE_DDG_ANCHOR.captures_iter(body) {
        if results.len() >= limit {
            break;
        }
        let attrs = anchor.name("attrs").map(|m| m.as_str()).unwrap_or("");
        let Some(href_m) = RE_HREF_ATTR.captures(attrs) else {
            continue;
        };
        let raw_href = href_m.name("href").map(|m| m.as_str()).unwrap_or("");
        let title_raw = anchor.name("title").map(|m| m.as_str()).unwrap_or("");
        let title = clean_html_fragment(title_raw);
        let url = normalize_result_url(raw_href);
        let host = host_of_url(&url).unwrap_or_default();

        if title.is_empty()
            || url.is_empty()
            || !url.starts_with("http")
            || host.ends_with("google.com")
        {
            continue;
        }
        let title = if title.len() > 160 {
            format!("{}…", &title[..160])
        } else {
            title
        };
        results.push(SearchHit {
            title,
            url,
            snippet: String::new(),
        });
    }

    dedupe_hits_preserve_order(results.into_iter().map(sanitize_hit).collect())
}

async fn fetch_ddg_instant_answer_body(
    client: &reqwest::Client,
    query: &str,
) -> Result<String, ToolError> {
    let resp = client
        .get("https://api.duckduckgo.com/")
        .query(&[
            ("q", query),
            ("format", "json"),
            ("no_html", "1"),
            ("skip_disambig", "1"),
        ])
        .headers(ddg_api_headers())
        .send()
        .await
        .map_err(|e| ToolError::ExecutionFailed {
            message: format!("DuckDuckGo API request failed: {}", e),
        })?;

    if !resp.status().is_success() {
        return Err(ToolError::ExecutionFailed {
            message: format!("DuckDuckGo API HTTP {}", resp.status()),
        });
    }

    resp.text().await.map_err(|e| ToolError::ExecutionFailed {
        message: format!("DuckDuckGo API read body: {}", e),
    })
}

async fn fetch_ddg_html_body(
    client: &reqwest::Client,
    query: &str,
    base: &str,
) -> Result<String, ToolError> {
    let url = reqwest::Url::parse(base.trim()).map_err(|_| ToolError::ExecutionFailed {
        message: "Invalid search_url for DuckDuckGo HTML fetch".to_string(),
    })?;
    let resp = client
        .get(url)
        .query(&[("q", query)])
        .headers(ddg_html_headers())
        .send()
        .await
        .map_err(|e| ToolError::ExecutionFailed {
            message: format!("DuckDuckGo HTML request failed: {}", e),
        })?;

    if !resp.status().is_success() {
        return Err(ToolError::ExecutionFailed {
            message: format!("DuckDuckGo HTML HTTP {}", resp.status()),
        });
    }

    resp.text().await.map_err(|e| ToolError::ExecutionFailed {
        message: format!("DuckDuckGo HTML read body: {}", e),
    })
}

async fn fetch_bing_html_body(client: &reqwest::Client, query: &str) -> Result<String, ToolError> {
    let resp = client
        .get(BING_SEARCH_BASE)
        .query(&[("q", query), ("count", "10")])
        .headers(bing_html_headers())
        .send()
        .await
        .map_err(|e| ToolError::ExecutionFailed {
            message: format!("Bing HTML request failed: {}", e),
        })?;

    if !resp.status().is_success() {
        return Err(ToolError::ExecutionFailed {
            message: format!("Bing HTML HTTP {}", resp.status()),
        });
    }

    resp.text().await.map_err(|e| ToolError::ExecutionFailed {
        message: format!("Bing HTML read body: {}", e),
    })
}

async fn fetch_google_html_body(
    client: &reqwest::Client,
    query: &str,
) -> Result<String, ToolError> {
    let resp = client
        .get(GOOGLE_SEARCH_BASE)
        .query(&[("q", query), ("num", "10")])
        .headers(google_html_headers())
        .send()
        .await
        .map_err(|e| ToolError::ExecutionFailed {
            message: format!("Google HTML request failed: {}", e),
        })?;

    if !resp.status().is_success() {
        return Err(ToolError::ExecutionFailed {
            message: format!("Google HTML HTTP {}", resp.status()),
        });
    }

    resp.text().await.map_err(|e| ToolError::ExecutionFailed {
        message: format!("Google HTML read body: {}", e),
    })
}

/// Instant answer JSON; if empty or unusable, HTML result page (`search_url` or default DDG HTML).
async fn search_ddg(
    client: &reqwest::Client,
    query: &str,
    max_results: usize,
    search_url: Option<&str>,
) -> Result<Vec<SearchHit>, ToolError> {
    let html_base = search_url
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(DDG_HTML_SEARCH_BASE);

    let mut hits = match fetch_ddg_instant_answer_body(client, query).await {
        Ok(body) => parse_ddg_instant_answer_json(&body, max_results).unwrap_or_default(),
        Err(_) => Vec::new(),
    };

    if hits.is_empty() {
        let html = fetch_ddg_html_body(client, query, html_base).await?;
        hits = parse_ddg_html_results_openharness(&html, max_results);
    }

    Ok(hits)
}

async fn search_bing(
    client: &reqwest::Client,
    query: &str,
    max_results: usize,
) -> Result<Vec<SearchHit>, ToolError> {
    let html = fetch_bing_html_body(client, query).await?;
    Ok(parse_bing_html_results(&html, max_results))
}

async fn search_google(
    client: &reqwest::Client,
    query: &str,
    max_results: usize,
) -> Result<Vec<SearchHit>, ToolError> {
    let html = fetch_google_html_body(client, query).await?;
    Ok(parse_google_html_results(&html, max_results))
}

#[cfg(test)]
mod ddg_tests {
    use super::*;

    #[test]
    fn normalize_result_url_extracts_uddg() {
        let u = normalize_result_url(
            "https://duckduckgo.com/l/?uddg=https%3A%2F%2Frust-lang.org%2F&rut=abc",
        );
        assert_eq!(u, "https://rust-lang.org/");
    }

    #[test]
    fn normalize_result_url_extracts_bing_redirect() {
        let u = normalize_result_url(
            "https://www.bing.com/ck/a?u=a1aHR0cHM6Ly9leGFtcGxlLm9yZy9wYXRo&ntb=1",
        );
        assert_eq!(u, "https://example.org/path");
    }

    #[test]
    fn normalize_result_url_extracts_google_redirect() {
        let u = normalize_result_url("/url?q=https%3A%2F%2Fexample.net%2Fpaper&sa=U");
        assert_eq!(u, "https://example.net/paper");
    }

    #[test]
    fn normalize_result_url_passes_through_https() {
        let u = normalize_result_url("https://example.com/path");
        assert_eq!(u, "https://example.com/path");
    }

    #[test]
    fn normalize_result_url_non_l_ddg_unchanged() {
        let u = normalize_result_url("https://duckduckgo.com/settings");
        assert_eq!(u, "https://duckduckgo.com/settings");
    }

    #[test]
    fn parse_ddg_html_extracts_result_a_and_uddg() {
        let html = r##"
<a href="https://duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com%2Fpage&amp;rut=x" class="result__a">Example Page Title</a>
"##;
        let v = parse_ddg_html_results_openharness(html, 10);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].url, "https://example.com/page");
        assert!(v[0].title.contains("Example"));
    }

    #[test]
    fn parse_ddg_instant_answer_json_topic() {
        let j = r#"{
            "AbstractURL": "",
            "RelatedTopics": [
                { "Text": "Hello world snippet text here", "FirstURL": "https://example.org/a" }
            ]
        }"#;
        let v = parse_ddg_instant_answer_json(j, 10).expect("parse");
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].url, "https://example.org/a");
    }

    #[test]
    fn parse_ddg_html_class_before_href() {
        let html = r#"<a class="result__a" href="https://duckduckgo.com/l/?uddg=https%3A%2F%2Ffoo.com%2F">Foo</a>"#;
        let v = parse_ddg_html_results_openharness(html, 10);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].url, "https://foo.com/");
    }

    #[test]
    fn sanitize_search_text_strips_inline_base64() {
        let s = "Intro data:image/png;base64,abcdabcd+/= tail";
        let t = sanitize_search_text(s, 500);
        assert!(t.contains("[image omitted]"));
        assert!(!t.contains("base64"));
    }

    #[test]
    fn sanitize_search_text_truncates_long_snippet() {
        let s = "x".repeat(900);
        let t = sanitize_search_text(&s, 100);
        assert!(t.chars().count() <= 100);
        assert!(t.ends_with('…'));
    }

    #[test]
    fn dedupe_drops_duplicate_urls() {
        let hits = vec![
            SearchHit {
                title: "a".into(),
                url: "https://Example.com/path".into(),
                snippet: "".into(),
            },
            SearchHit {
                title: "b".into(),
                url: "https://example.com/path/".into(),
                snippet: "".into(),
            },
            SearchHit {
                title: "c".into(),
                url: "https://other.com/".into(),
                snippet: "".into(),
            },
        ];
        let d = dedupe_hits_preserve_order(hits);
        assert_eq!(d.len(), 2);
    }

    #[test]
    fn parse_ddg_html_snippet_paired_with_result() {
        let html = r#"
<div class="result__snippet">A short snippet text.</div>
<a class="result__a" href="https://duckduckgo.com/l/?uddg=https%3A%2F%2Fbar.com%2F">Bar Title</a>
"#;
        let v = parse_ddg_html_results_openharness(html, 10);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].url, "https://bar.com/");
        assert!(v[0].snippet.contains("snippet"));
    }

    #[test]
    fn parse_bing_html_extracts_result_block() {
        let html = r#"
<ol id="b_results">
  <li class="b_algo">
    <h2><a href="https://example.org/paper">Example Paper</a></h2>
    <div class="b_caption"><p>Readable summary &amp; details.</p></div>
  </li>
</ol>
"#;
        let v = parse_bing_html_results(html, 10);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].url, "https://example.org/paper");
        assert_eq!(v[0].title, "Example Paper");
        assert!(v[0].snippet.contains("summary & details"));
    }

    #[test]
    fn parse_google_html_extracts_url_redirect() {
        let html = r#"
<a href="/url?q=https%3A%2F%2Fexample.net%2Fpaper&amp;sa=U"><h3>Google Result</h3></a>
"#;
        let v = parse_google_html_results(html, 10);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].url, "https://example.net/paper");
        assert_eq!(v[0].title, "Google Result");
    }

    #[test]
    fn search_method_order_uses_user_selection() {
        assert_eq!(
            ordered_search_methods(&["tavily".into(), "google".into(), "ddg".into()], "bing"),
            vec![
                SearchMethod::Tavily,
                SearchMethod::Google,
                SearchMethod::Ddg
            ]
        );
        assert_eq!(
            ordered_search_methods(&["google".into(), "google".into(), "unknown".into()], "ddg"),
            vec![SearchMethod::Google]
        );
        assert_eq!(
            ordered_search_methods(&[], "bing"),
            vec![SearchMethod::Bing, SearchMethod::Ddg, SearchMethod::Google]
        );
        assert_eq!(
            ordered_search_methods(&[], "unknown"),
            vec![SearchMethod::Ddg, SearchMethod::Google, SearchMethod::Bing]
        );
    }

    /// Live network (optional): `cargo test -p omiga ddg_live_smoke -- --ignored --nocapture`
    /// Skips assertion if DDG is unreachable (firewall / region); succeeds when API+HTML return hits.
    #[tokio::test]
    #[ignore]
    async fn ddg_live_smoke_search_ddg_returns_hits() {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(45))
            .no_proxy()
            .user_agent(concat!("Omiga/", env!("CARGO_PKG_VERSION"), " Search"))
            .build()
            .expect("client");
        let hits = match search_ddg(&client, "rust programming language", 10, None).await {
            Ok(h) => h,
            Err(e) => {
                eprintln!("ddg_live_smoke: skipped (network or DDG blocked): {}", e);
                return;
            }
        };
        assert!(
            !hits.is_empty(),
            "DDG instant answer or HTML fallback should return at least one link"
        );
    }

    #[test]
    fn filter_hits_drops_unsafe_result_urls() {
        let tmp = tempfile::TempDir::new().expect("tempdir");
        let hits = vec![
            SearchHit {
                title: "local".into(),
                url: "http://127.0.0.1:3000".into(),
                snippet: "".into(),
            },
            SearchHit {
                title: "public".into(),
                url: "https://example.org".into(),
                snippet: "".into(),
            },
        ];
        let filtered = filter_hits(tmp.path(), hits, &None, &None, 10);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].url, "https://example.org");
    }

    #[test]
    fn search_timeout_is_hard_capped_for_interactive_use() {
        assert_eq!(effective_search_timeout(600), Duration::from_secs(30));
        assert_eq!(effective_search_timeout(1), Duration::from_secs(5));
    }

    #[tokio::test]
    async fn search_global_timeout_returns_error() {
        let result = enforce_search_timeout(Duration::from_millis(1), async {
            tokio::time::sleep(Duration::from_millis(50)).await;
            Ok(SearchExecution::new())
        })
        .await;

        match result {
            Err(ToolError::ExecutionFailed { message }) => {
                assert!(message.contains("search timed out after 1s"));
            }
            other => panic!("expected timeout execution failure, got {other:?}"),
        }
    }
}

fn normalized_category(value: &str) -> String {
    match value.trim().to_ascii_lowercase().replace('-', "_").as_str() {
        "dataset" | "datasets" => "data".to_string(),
        "knowledge_base" | "kb" | "memory" => "knowledge".to_string(),
        other => other.to_string(),
    }
}

fn normalized_source(value: Option<&str>) -> String {
    value
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("auto")
        .to_ascii_lowercase()
        .replace('-', "_")
}

fn normalized_subcategory(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_ascii_lowercase().replace(['-', ' '], "_"))
}

async fn dataset_auto_search(
    ctx: &ToolContext,
    client: &crate::domain::search::data::PublicDataClient,
    data_args: crate::domain::search::data::DataSearchArgs,
) -> Result<crate::domain::search::data::DataSearchResponse, ToolError> {
    let enabled = ctx.web_search_api_keys.enabled_query_dataset_sources();
    let mut sources = Vec::new();
    if enabled.iter().any(|source| source == "geo") {
        sources.push(crate::domain::search::data::PublicDataSource::Geo);
    }
    if enabled.iter().any(|source| source == "ena") {
        sources.push(crate::domain::search::data::PublicDataSource::EnaStudy);
    }
    if enabled.iter().any(|source| source == "cbioportal") {
        sources.push(crate::domain::search::data::PublicDataSource::CbioPortal);
    }
    if enabled.iter().any(|source| source == "gtex") {
        sources.push(crate::domain::search::data::PublicDataSource::Gtex);
    }
    if sources.is_empty() {
        return Ok(crate::domain::search::data::DataSearchResponse {
            query: data_args.query.trim().to_string(),
            source: "auto".to_string(),
            total: Some(0),
            results: Vec::new(),
            notes: vec!["All dataset sources are disabled in Settings → Search.".to_string()],
        });
    }
    let mut results = Vec::new();
    let mut total = 0u64;
    let mut saw_total = false;
    let mut notes = vec!["Combined enabled dataset-source search".to_string()];
    for source in sources {
        let response = tokio::select! {
            _ = ctx.cancel.cancelled() => return Err(ToolError::Cancelled),
            r = client.search(source, data_args.clone()) => r,
        };
        match response {
            Ok(response) => {
                if let Some(count) = response.total {
                    total = total.saturating_add(count);
                    saw_total = true;
                }
                notes.extend(response.notes);
                results.extend(response.results);
            }
            Err(err) => notes.push(format!("{} source failed: {err}", source.as_str())),
        }
    }
    results.truncate(data_args.normalized_max_results() as usize);
    Ok(crate::domain::search::data::DataSearchResponse {
        query: data_args.query.trim().to_string(),
        source: "auto".to_string(),
        total: saw_total.then_some(total),
        results,
        notes,
    })
}

fn dataset_subcategory_id(subcategory: Option<&str>) -> Result<Option<&'static str>, ToolError> {
    let Some(subcategory) = subcategory else {
        return Ok(None);
    };
    match subcategory {
        "expression" | "gene_expression" | "transcriptomics" | "transcriptome" => {
            Ok(Some("expression"))
        }
        "sequencing" | "sequence_reads" | "raw_reads" | "reads" | "sra" => Ok(Some("sequencing")),
        "genomics" | "genome" | "genomes" | "assembly" | "assemblies" => Ok(Some("genomics")),
        "sample_metadata" | "sample" | "samples" | "metadata" => Ok(Some("sample_metadata")),
        "multi_omics" | "multiomics" | "projects" | "project" => Ok(Some("multi_omics")),
        other => Err(ToolError::InvalidArguments {
            message: format!("Unsupported dataset subcategory: {other}"),
        }),
    }
}

fn dataset_source_for_subcategory(
    subcategory: Option<&str>,
) -> Result<Option<crate::domain::search::data::PublicDataSource>, ToolError> {
    let Some(subcategory) = subcategory else {
        return Ok(None);
    };
    match subcategory {
        "expression" | "gene_expression" | "transcriptomics" | "transcriptome" => {
            Ok(Some(crate::domain::search::data::PublicDataSource::Geo))
        }
        "sequencing" | "sequence_reads" | "raw_reads" | "reads" | "sra" => {
            Ok(Some(crate::domain::search::data::PublicDataSource::EnaRun))
        }
        "genomics" | "genome" | "genomes" | "assembly" | "assemblies" => Ok(Some(
            crate::domain::search::data::PublicDataSource::EnaAssembly,
        )),
        "sample_metadata" | "sample" | "samples" | "metadata" => Ok(Some(
            crate::domain::search::data::PublicDataSource::EnaSample,
        )),
        "multi_omics" | "multiomics" | "projects" | "project" => Ok(Some(
            crate::domain::search::data::PublicDataSource::CbioPortal,
        )),
        other => Err(ToolError::InvalidArguments {
            message: format!("Unsupported dataset subcategory: {other}"),
        }),
    }
}

fn recall_scope_for_source(source: &str) -> String {
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

fn search_method_source_key(method: SearchMethod) -> &'static str {
    match method {
        SearchMethod::Tavily => "tavily",
        SearchMethod::Exa => "exa",
        SearchMethod::Firecrawl => "firecrawl",
        SearchMethod::Parallel => "parallel",
        SearchMethod::Ddg => "ddg",
        SearchMethod::Bing => "bing",
        SearchMethod::Google => "google",
    }
}

fn web_methods_for_source(ctx: &ToolContext, source: &str) -> Result<Vec<SearchMethod>, ToolError> {
    match source {
        "auto" => Ok(ordered_search_methods(
            &ctx.web_search_methods,
            &ctx.web_search_engine,
        )),
        "tavily" => Ok(vec![SearchMethod::Tavily]),
        "exa" => Ok(vec![SearchMethod::Exa]),
        "firecrawl" => Ok(vec![SearchMethod::Firecrawl]),
        "parallel" => Ok(vec![SearchMethod::Parallel]),
        "google" => Ok(vec![SearchMethod::Google]),
        "bing" => Ok(vec![SearchMethod::Bing]),
        "ddg" | "duckduckgo" | "duck_duck_go" => Ok(vec![SearchMethod::Ddg]),
        other => Err(ToolError::InvalidArguments {
            message: format!("Unsupported web search source: {other}"),
        }),
    }
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
    let host = host_of_url(url)?;
    Some(format!(
        "https://www.google.com/s2/favicons?domain={}&sz=64",
        host.trim_start_matches("www.")
    ))
}

fn web_results_json(
    args: &SearchArgs,
    requested_source: &str,
    execution: SearchExecution,
    hits: Vec<SearchHit>,
    duration: f32,
) -> JsonValue {
    let effective_source = execution
        .effective_source
        .clone()
        .unwrap_or_else(|| requested_source.to_string());
    let results: Vec<JsonValue> = hits
        .iter()
        .enumerate()
        .map(|(idx, h)| {
            let title = if h.title.trim().is_empty() {
                &h.url
            } else {
                &h.title
            };
            json!({
                "position": idx + 1,
                "category": "web",
                "source": effective_source,
                "title": title,
                "name": title,
                "link": h.url,
                "url": h.url,
                "displayed_link": displayed_link_for_url(&h.url),
                "favicon": favicon_for_url(&h.url),
                "snippet": h.snippet,
                "id": JsonValue::Null,
                "metadata": {},
            })
        })
        .collect();
    json!({
        "query": args.query.trim(),
        "category": "web",
        "source": requested_source,
        "effective_source": effective_source,
        "source_label": join_labels(&execution.source_labels),
        "duration_seconds": duration,
        "route_notes": execution.notes,
        "results": results,
    })
}

fn structured_error_json(
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

fn json_stream(value: JsonValue) -> crate::infrastructure::streaming::StreamOutputBox {
    let mut text = serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string());
    if text.len() > MAX_OUTPUT_CHARS {
        text.truncate(MAX_OUTPUT_CHARS);
        text.push_str("\n/* Output truncated */");
    }
    SearchOutput { text }.into_stream()
}

#[async_trait]
impl super::ToolImpl for SearchTool {
    type Args = SearchArgs;

    const DESCRIPTION: &'static str = DESCRIPTION;

    async fn execute(
        ctx: &ToolContext,
        args: Self::Args,
    ) -> Result<crate::infrastructure::streaming::StreamOutputBox, ToolError> {
        validate(&args)?;
        let start = Instant::now();
        let category = normalized_category(&args.category);
        let source = normalized_source(args.source.as_deref());
        let subcategory = normalized_subcategory(args.subcategory.as_deref());
        let max_n = effective_max_results(&args);

        match category.as_str() {
            "web" => {
                let timeout = effective_search_timeout(ctx.timeout_secs);
                let mut client_builder = reqwest::Client::builder()
                    .timeout(timeout)
                    .user_agent(concat!("Omiga/", env!("CARGO_PKG_VERSION"), " Search"));
                if !ctx.web_use_proxy {
                    client_builder = client_builder.no_proxy();
                }
                let client = client_builder
                    .build()
                    .map_err(|e| ToolError::ExecutionFailed {
                        message: format!("HTTP client: {}", e),
                    })?;

                let allowed = &args.allowed_domains;
                let blocked = &args.blocked_domains;
                let search_url = args.search_url.as_deref().filter(|s| !s.trim().is_empty());
                if let Some(search_url) = search_url {
                    super::web_safety::validate_public_http_url(
                        &ctx.project_root,
                        search_url,
                        false,
                    )
                    .map_err(|message| ToolError::InvalidArguments { message })?;
                }

                let execution = tokio::select! {
                    _ = ctx.cancel.cancelled() => return Err(ToolError::Cancelled),
                    r = enforce_search_timeout(timeout, async {
                        let strategy_fut = search_ordered_methods(
                            &client,
                            ctx,
                            args.query.trim(),
                            allowed,
                            blocked,
                            max_n,
                            search_url,
                            &source,
                        );
                        tokio::select! {
                            _ = ctx.cancel.cancelled() => Err(ToolError::Cancelled),
                            r = strategy_fut => r,
                        }
                    }) => r?,
                };

                let processed = dedupe_hits_preserve_order(
                    execution
                        .hits
                        .iter()
                        .cloned()
                        .map(sanitize_hit)
                        .collect::<Vec<_>>(),
                );
                let hits = filter_hits(&ctx.project_root, processed, allowed, blocked, max_n);
                let duration = start.elapsed().as_secs_f32();
                Ok(json_stream(web_results_json(
                    &args, &source, execution, hits, duration,
                )))
            }
            "literature" => match source.as_str() {
                "auto" | "pubmed" => {
                    let client =
                        crate::domain::search::pubmed::EntrezClient::from_tool_context(ctx)
                            .map_err(|message| ToolError::ExecutionFailed { message })?;
                    let response = tokio::select! {
                        _ = ctx.cancel.cancelled() => return Err(ToolError::Cancelled),
                        r = client.search(crate::domain::search::pubmed::PubmedSearchArgs {
                            query: args.query.trim().to_string(),
                            max_results: args.max_results,
                            ret_start: None,
                            sort: None,
                            date_type: None,
                            mindate: None,
                            maxdate: None,
                        }) => r.map_err(|message| ToolError::ExecutionFailed { message })?,
                    };
                    Ok(json_stream(
                        crate::domain::search::pubmed::search_response_to_json(&response),
                    ))
                }
                "semantic_scholar" | "semanticscholar" | "s2" => {
                    if !ctx.web_search_api_keys.semantic_scholar_enabled {
                        return Ok(json_stream(structured_error_json(
                            "source_disabled",
                            "literature",
                            &source,
                            "literature.semantic_scholar is disabled. Enable it and configure an API key in Settings → Search.",
                        )));
                    }
                    let client =
                        crate::domain::search::semantic_scholar::SemanticScholarClient::from_tool_context(ctx)
                            .map_err(|message| ToolError::ExecutionFailed { message })?;
                    let response = tokio::select! {
                        _ = ctx.cancel.cancelled() => return Err(ToolError::Cancelled),
                        r = client.search(crate::domain::search::semantic_scholar::SemanticScholarSearchArgs {
                            query: args.query.trim().to_string(),
                            max_results: args.max_results,
                            token: None,
                        }) => r.map_err(|message| ToolError::ExecutionFailed { message })?,
                    };
                    Ok(json_stream(
                        crate::domain::search::semantic_scholar::search_response_to_json(&response),
                    ))
                }
                "arxiv" | "crossref" | "openalex" | "biorxiv" | "bio_rxiv" | "medrxiv"
                | "med_rxiv" => {
                    let source_kind =
                        crate::domain::search::literature::PublicLiteratureSource::parse(&source)
                            .ok_or_else(|| ToolError::InvalidArguments {
                            message: format!("Unsupported literature search source: {source}"),
                        })?;
                    let client =
                        crate::domain::search::literature::PublicLiteratureClient::from_tool_context(ctx)
                            .map_err(|message| ToolError::ExecutionFailed { message })?;
                    let response = tokio::select! {
                        _ = ctx.cancel.cancelled() => return Err(ToolError::Cancelled),
                        r = client.search(source_kind, crate::domain::search::literature::LiteratureSearchArgs {
                            query: args.query.trim().to_string(),
                            max_results: args.max_results,
                        }) => r.map_err(|message| ToolError::ExecutionFailed { message })?,
                    };
                    Ok(json_stream(
                        crate::domain::search::literature::search_response_to_json(&response),
                    ))
                }
                other => Err(ToolError::InvalidArguments {
                    message: format!("Unsupported literature search source: {other}"),
                }),
            },
            "knowledge" => {
                <super::recall::RecallTool as super::ToolImpl>::execute(
                    ctx,
                    super::recall::RecallArgs {
                        query: args.query.trim().to_string(),
                        limit: max_n.clamp(1, 20),
                        scope: recall_scope_for_source(&source),
                    },
                )
                .await
            }
            "data" => {
                if let Some(type_id) = dataset_subcategory_id(subcategory.as_deref())? {
                    if !ctx
                        .web_search_api_keys
                        .is_query_dataset_type_enabled(type_id)
                    {
                        return Ok(json_stream(structured_error_json(
                            "source_disabled",
                            "data",
                            &source,
                            format!("data.{type_id} is disabled. Enable it in Settings → Search.",),
                        )));
                    }
                }
                match source.as_str() {
                    "auto" => {
                        let client =
                            crate::domain::search::data::PublicDataClient::from_tool_context(ctx)
                                .map_err(|message| ToolError::ExecutionFailed { message })?;
                        let data_args = crate::domain::search::data::DataSearchArgs {
                            query: args.query.trim().to_string(),
                            max_results: args.max_results,
                            params: None,
                        };
                        let response = if let Some(source_kind) =
                            dataset_source_for_subcategory(subcategory.as_deref())?
                        {
                            if !ctx
                                .web_search_api_keys
                                .is_query_dataset_source_enabled(source_kind.as_str())
                            {
                                return Ok(json_stream(structured_error_json(
                                    "source_disabled",
                                    "data",
                                    source_kind.as_str(),
                                    format!(
                                        "data.{} is disabled. Enable it in Settings → Search.",
                                        source_kind.as_str()
                                    ),
                                )));
                            }
                            tokio::select! {
                                _ = ctx.cancel.cancelled() => return Err(ToolError::Cancelled),
                                r = client.search(source_kind, data_args) => r.map_err(|message| ToolError::ExecutionFailed { message })?,
                            }
                        } else {
                            dataset_auto_search(ctx, &client, data_args).await?
                        };
                        Ok(json_stream(
                            crate::domain::search::data::search_response_to_json(&response),
                        ))
                    }
                    source
                        if crate::domain::search::data::PublicDataSource::parse(source)
                            .is_some() =>
                    {
                        let source_kind =
                            crate::domain::search::data::PublicDataSource::parse(source)
                                .ok_or_else(|| ToolError::InvalidArguments {
                                    message: format!("Unsupported data search source: {source}"),
                                })?;
                        if !ctx
                            .web_search_api_keys
                            .is_query_dataset_source_enabled(source_kind.as_str())
                        {
                            return Ok(json_stream(structured_error_json(
                                "source_disabled",
                                "data",
                                source_kind.as_str(),
                                format!(
                                    "data.{} is disabled. Enable it in Settings → Search.",
                                    source_kind.as_str()
                                ),
                            )));
                        }
                        let client =
                            crate::domain::search::data::PublicDataClient::from_tool_context(ctx)
                                .map_err(|message| ToolError::ExecutionFailed { message })?;
                        let response = tokio::select! {
                            _ = ctx.cancel.cancelled() => return Err(ToolError::Cancelled),
                            r = client.search(source_kind, crate::domain::search::data::DataSearchArgs {
                                query: args.query.trim().to_string(),
                                max_results: args.max_results,
                                params: None,
                            }) => r.map_err(|message| ToolError::ExecutionFailed { message })?,
                        };
                        Ok(json_stream(
                            crate::domain::search::data::search_response_to_json(&response),
                        ))
                    }
                    other => Err(ToolError::InvalidArguments {
                        message: format!("Unsupported data search source: {other}"),
                    }),
                }
            }
            "social" => match source.as_str() {
                "auto" | "wechat" => {
                    if !ctx.web_search_api_keys.wechat_search_enabled {
                        return Ok(json_stream(structured_error_json(
                            "source_disabled",
                            "social",
                            &source,
                            "social.wechat is disabled. Enable WeChat public-account search in Settings → Search.",
                        )));
                    }
                    let client =
                        crate::domain::search::wechat::WechatClient::from_tool_context(ctx)
                            .map_err(|message| ToolError::ExecutionFailed { message })?;
                    let response = tokio::select! {
                        _ = ctx.cancel.cancelled() => return Err(ToolError::Cancelled),
                        r = client.search(crate::domain::search::wechat::WechatSearchArgs {
                            query: args.query.trim().to_string(),
                            max_results: args.max_results,
                            page: None,
                        }) => r.map_err(|message| ToolError::ExecutionFailed { message })?,
                    };
                    Ok(json_stream(
                        crate::domain::search::wechat::search_response_to_json(&response),
                    ))
                }
                other => Err(ToolError::InvalidArguments {
                    message: format!("Unsupported social search source: {other}"),
                }),
            },
            other => Err(ToolError::InvalidArguments {
                message: format!("Unsupported search category: {other}"),
            }),
        }
    }
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

pub fn schema() -> ToolSchema {
    ToolSchema::new(
        "search",
        DESCRIPTION,
        serde_json::json!({
            "type": "object",
            "properties": {
                "category": {
                    "type": "string",
                    "description": "Source category. Supports literature, dataset (alias: data), knowledge, web, social."
                },
                "source": {
                    "type": "string",
                    "description": "Source within the category. Defaults to auto. Examples: google, ddg, bing, tavily, pubmed, arxiv, crossref, openalex, biorxiv, medrxiv, semantic_scholar (opt-in), geo, ena, ena_run, ena_experiment, ena_sample, ena_analysis, ena_assembly, ena_sequence, cbioportal, gtex, wiki, implicit, long_term, sources, wechat (opt-in)."
                },
                "subcategory": {
                    "type": "string",
                    "description": "Optional subcategory. For dataset: expression, sequencing, genomics, sample_metadata, multi_omics. With source=auto, supported dataset subcategories choose the best built-in source automatically."
                },
                "query": {
                    "type": "string",
                    "description": "Search query (at least 2 characters)"
                },
                "allowed_domains": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Only include results whose URL host matches one of these domains"
                },
                "blocked_domains": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Exclude results from these domains"
                },
                "max_results": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 10,
                    "description": "Maximum number of results (default 5)"
                },
                "search_url": {
                    "type": "string",
                    "description": "Optional override for the DuckDuckGo HTML search endpoint (http(s) URL)"
                }
            },
            "required": ["category", "query"]
        }),
    )
}
