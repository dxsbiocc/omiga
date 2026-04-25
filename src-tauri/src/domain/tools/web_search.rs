//! Web search — Tavily Search API (preferred) or DuckDuckGo fallback
//!
//! Upstream `WebSearchTool` uses Anthropic server-side `web_search`; Omiga runs a real HTTP search.
//!
//! DuckDuckGo: the public `api.duckduckgo.com` endpoint is an **instant-answer** API, not full web
//! search — many queries return empty `AbstractURL` / `RelatedTopics`. HTML results at
//! `html.duckduckgo.com` are used when the JSON API yields nothing. Result links are often
//! `duckduckgo.com/l/?uddg=…` redirects; we unwrap `uddg` to the real destination URL.

use super::{ToolContext, ToolError, ToolSchema};
use crate::domain::mcp::client::{
    call_tool_on_server, call_tool_via_peer, connect_mcp_server_legacy, list_tools_for_server,
};
use crate::domain::mcp::config::merged_mcp_servers;
use crate::infrastructure::streaming::{StreamOutput, StreamOutputItem};
use async_trait::async_trait;
use lazy_static::lazy_static;
use regex::Regex;
use rmcp::model::CallToolResult;
use serde::{Deserialize, Serialize};
use serde_json::{Map as JsonMap, Value as JsonValue};
use std::collections::HashSet;
use std::pin::Pin;
use std::time::{Duration, Instant};

/// Hard cap for safety (Tavily / HTML parsing).
const MAX_RESULTS_CAP: usize = 12;
const MAX_OUTPUT_CHARS: usize = 100_000;
/// Per-result snippet cap (Hermes-style: keep tool output lean).
const MAX_SNIPPET_CHARS: usize = 512;
const MAX_TITLE_CHARS: usize = 220;
const DDG_HTML_RETRY_DELAY_MS: u64 = 450;
const TAVILY_RETRY_DELAY_MS: u64 = 400;

pub const DESCRIPTION: &str = r#"Search the public web for up-to-date information.

- Provider order when keys are set (Omiga Settings → Advanced, or env): Tavily → Exa → Firecrawl → Parallel → DuckDuckGo. Env vars: `OMIGA_TAVILY_API_KEY` / `TAVILY_API_KEY`, `OMIGA_EXA_API_KEY` / `EXA_API_KEY`, `OMIGA_FIRECRAWL_API_KEY` / `FIRECRAWL_API_KEY`, optional `OMIGA_FIRECRAWL_API_URL` / `FIRECRAWL_API_URL` for self-hosted Firecrawl base, `OMIGA_PARALLEL_API_KEY` / `PARALLEL_API_KEY`. Settings overrides env when non-empty.
- If every configured provider fails or returns no results, falls back to DuckDuckGo (instant-answer JSON when available, then HTML; no API key). HTML fetch retries once on transient errors.
- Optional `allowed_domains` or `blocked_domains` filter result URLs (not both).
- `max_results` (default 5, max 10) limits how many hits are returned.
- Optional `search_url` overrides the DuckDuckGo HTML endpoint (for private search proxies or tests).
- Unsafe/private result URLs are filtered out; `search_url` must also be a public-safe HTTP(S) URL.
- After answering, cite sources with markdown links when you use this tool."#;

fn default_max_results() -> Option<u32> {
    Some(5)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSearchArgs {
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
enum SearchIntent {
    General,
    Research,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SearchApiProvider {
    Tavily,
    Exa,
    Firecrawl,
    Parallel,
}

#[derive(Debug, Clone)]
struct SearchExecution {
    hits: Vec<SearchHit>,
    source_labels: Vec<String>,
    notes: Vec<String>,
}

impl SearchExecution {
    fn new() -> Self {
        Self {
            hits: Vec::new(),
            source_labels: Vec::new(),
            notes: Vec::new(),
        }
    }

    fn with_hits(mut self, hits: Vec<SearchHit>) -> Self {
        self.hits = hits;
        self
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

#[derive(Debug, Clone)]
struct McpToolCandidate {
    tool_name: String,
    args: JsonMap<String, JsonValue>,
    score: i32,
}

#[derive(Debug, Clone)]
struct BrowserMcpAction {
    tool_name: String,
    schema: JsonValue,
    score: i32,
}

#[derive(Debug, Clone)]
struct BrowserMcpBundle {
    server_name: String,
    navigate: BrowserMcpAction,
    evaluate: BrowserMcpAction,
    wait: Option<BrowserMcpAction>,
    score: i32,
}

const PUBMED_SERVER: &str = "pubmed";
const PUBMED_HOST: &str = "pubmed.ncbi.nlm.nih.gov";
const BIORXIV_HOST: &str = "biorxiv.org";
const DDG_HTML_SEARCH_BASE: &str = "https://html.duckduckgo.com/html/";
const MCP_BROWSER_MIN_SCORE: i32 = 55;
const MCP_PUBMED_MIN_SCORE: i32 = 60;
const PLAYWRIGHT_BROWSER_MIN_SCORE: i32 = 45;
const BROWSER_SEARCH_EXTRACTION_SCRIPT: &str = r#"(() => {
  const normalize = (value) => (value || '').replace(/\s+/g, ' ').trim();
  const hits = [];
  const seen = new Set();
  const pushHit = (title, url, snippet) => {
    const cleanTitle = normalize(title).slice(0, 220);
    const cleanUrl = normalize(url);
    const cleanSnippet = normalize(snippet).slice(0, 320);
    if (!cleanTitle || !/^https?:/i.test(cleanUrl) || seen.has(cleanUrl)) return;
    seen.add(cleanUrl);
    hits.push({ title: cleanTitle, url: cleanUrl, snippet: cleanSnippet });
  };

  const resultNodes = Array.from(
    new Set(
      [
        ...document.querySelectorAll('[data-testid="result"], .result, article, main li'),
      ]
    )
  );

  for (const node of resultNodes) {
    const anchor = node.querySelector('a[href]');
    if (!anchor) continue;
    const title = anchor.textContent || anchor.getAttribute('aria-label') || '';
    const url = anchor.href || '';
    const snippet = (node.innerText || '').replace(anchor.textContent || '', '');
    pushHit(title, url, snippet);
    if (hits.length >= 12) return hits;
  }

  for (const anchor of document.querySelectorAll('a[href]')) {
    const href = anchor.href || '';
    if (!/^https?:/i.test(href)) continue;
    if (/duckduckgo\.com\/(y\.js|settings|\/html\/?$)/i.test(href)) continue;
    const title = anchor.textContent || anchor.getAttribute('aria-label') || '';
    if (!normalize(title)) continue;
    const container = anchor.closest('article, li, div') || anchor.parentElement || anchor;
    const snippet = container ? container.innerText || '' : '';
    pushHit(title, href, snippet);
    if (hits.length >= 12) break;
  }

  return hits;
})()"#;

fn has_explicit_domain_override(args: &WebSearchArgs) -> bool {
    args.allowed_domains
        .as_ref()
        .map(|v| !v.is_empty())
        .unwrap_or(false)
        || args
            .blocked_domains
            .as_ref()
            .map(|v| !v.is_empty())
            .unwrap_or(false)
}

fn detect_search_intent(query: &str) -> SearchIntent {
    let q = query.to_ascii_lowercase();
    let is_research = [
        "pubmed",
        "pmid",
        "doi",
        "biorxiv",
        "bioarxiv",
        "arxiv",
        "medline",
        "scholar",
        "paper",
        "papers",
        "study",
        "studies",
        "literature",
        "journal",
        "preprint",
        "abstract",
        "meta-analysis",
        "meta analysis",
        "systematic review",
        "clinical trial",
        "citation",
        "reference",
        "文献",
        "论文",
        "综述",
        "预印本",
        "期刊",
        "学术",
        "doi",
        "单细胞",
        "转录组",
        "基因组",
        "蛋白组",
    ]
    .iter()
    .any(|needle| q.contains(needle));
    if is_research {
        SearchIntent::Research
    } else {
        SearchIntent::General
    }
}

fn scoped_site_query(query: &str, domain: &str) -> String {
    let q = query.trim();
    if q.to_ascii_lowercase().contains(domain) || q.to_ascii_lowercase().contains("site:") {
        q.to_string()
    } else {
        format!("{q} site:{domain}")
    }
}

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

fn value_object(value: &JsonValue) -> Option<&serde_json::Map<String, JsonValue>> {
    value.as_object()
}

fn schema_properties(schema: &JsonValue) -> Option<&serde_json::Map<String, JsonValue>> {
    schema
        .get("properties")
        .and_then(JsonValue::as_object)
        .or_else(|| value_object(schema))
}

fn first_matching_key<'a>(
    props: &'a serde_json::Map<String, JsonValue>,
    candidates: &[&str],
) -> Option<&'a str> {
    props.keys().find_map(|key| {
        let lower: String = key
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .collect::<String>()
            .to_ascii_lowercase();
        candidates
            .iter()
            .find(|candidate| {
                let normalized = candidate
                    .chars()
                    .filter(|c| c.is_ascii_alphanumeric())
                    .collect::<String>()
                    .to_ascii_lowercase();
                lower == normalized || lower.contains(&normalized)
            })
            .map(|_| key.as_str())
    })
}

fn first_string_field(map: &serde_json::Map<String, JsonValue>, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        map.iter().find_map(|(actual, value)| {
            if actual.eq_ignore_ascii_case(key) {
                value
                    .as_str()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
            } else {
                None
            }
        })
    })
}

fn build_query_args_from_schema(
    schema: &JsonValue,
    query: &str,
    max_results: u32,
) -> Option<JsonMap<String, JsonValue>> {
    let props = schema_properties(schema)?;
    let mut args = JsonMap::new();

    if let Some(key) = first_matching_key(
        props,
        &[
            "query",
            "q",
            "term",
            "search",
            "search_query",
            "search_term",
            "keywords",
            "text",
            "objective",
            "prompt",
            "question",
        ],
    ) {
        args.insert(key.to_string(), JsonValue::String(query.to_string()));
    }

    if let Some(key) = first_matching_key(
        props,
        &[
            "max_results",
            "limit",
            "count",
            "size",
            "top_k",
            "retmax",
            "n",
        ],
    ) {
        args.insert(
            key.to_string(),
            JsonValue::Number(serde_json::Number::from(max_results.max(1))),
        );
    }

    if let Some(key) = first_matching_key(props, &["offset", "start", "page", "from"]) {
        args.insert(
            key.to_string(),
            JsonValue::Number(serde_json::Number::from(0)),
        );
    }

    if args.is_empty() {
        None
    } else {
        Some(args)
    }
}

fn browser_context_score(server_name: &str, tool_name: &str, description: &str) -> i32 {
    let mut score = 0;
    let haystack = format!(
        "{} {} {}",
        server_name.to_ascii_lowercase(),
        tool_name.to_ascii_lowercase(),
        description.to_ascii_lowercase()
    );
    for needle in [
        "browser",
        "browse",
        "playwright",
        "chrome",
        "chromium",
        "puppeteer",
        "web page",
        "webpage",
        "navigation",
        "navigate",
    ] {
        if haystack.contains(needle) {
            score += 20;
        }
    }
    score
}

fn browser_search_candidate_from_schema(
    server_name: &str,
    tool_name: &str,
    description: &str,
    schema: &JsonValue,
    query: &str,
    max_results: u32,
) -> Option<McpToolCandidate> {
    let context_score = browser_context_score(server_name, tool_name, description);
    if context_score == 0 {
        return None;
    }

    let props = schema_properties(schema)?;
    let args = build_query_args_from_schema(schema, query, max_results)?;
    let name = tool_name.to_ascii_lowercase();
    let desc = description.to_ascii_lowercase();
    let mut score = context_score;
    if name.contains("search") || name.contains("query") || name.contains("find") {
        score += 25;
    }
    if desc.contains("search") || desc.contains("results") || desc.contains("web") {
        score += 15;
    }
    if first_matching_key(
        props,
        &[
            "query",
            "q",
            "term",
            "search",
            "search_query",
            "search_term",
            "keywords",
            "objective",
        ],
    )
    .is_some()
    {
        score += 20;
    }
    if first_matching_key(props, &["max_results", "limit", "count", "size", "top_k"]).is_some() {
        score += 10;
    }
    (score >= MCP_BROWSER_MIN_SCORE).then(|| McpToolCandidate {
        tool_name: tool_name.to_string(),
        args,
        score,
    })
}

fn browser_action_candidate(
    server_name: &str,
    tool_name: &str,
    description: &str,
    schema: &JsonValue,
    action_keywords: &[&str],
) -> Option<BrowserMcpAction> {
    let context_score = browser_context_score(server_name, tool_name, description);
    if context_score == 0 {
        return None;
    }
    let name = tool_name.to_ascii_lowercase();
    let desc = description.to_ascii_lowercase();
    let mut score = context_score;
    for needle in action_keywords {
        if name.contains(needle) {
            score += 25;
        }
        if desc.contains(needle) {
            score += 10;
        }
    }
    (score >= PLAYWRIGHT_BROWSER_MIN_SCORE).then(|| BrowserMcpAction {
        tool_name: tool_name.to_string(),
        schema: schema.clone(),
        score,
    })
}

fn build_url_args_from_schema(schema: &JsonValue, url: &str) -> Option<JsonMap<String, JsonValue>> {
    let props = schema_properties(schema)?;
    let mut args = JsonMap::new();
    if let Some(key) = first_matching_key(
        props,
        &[
            "url",
            "uri",
            "href",
            "target_url",
            "targeturl",
            "page_url",
            "pageurl",
        ],
    ) {
        args.insert(key.to_string(), JsonValue::String(url.to_string()));
    }
    (!args.is_empty()).then_some(args)
}

fn build_browser_evaluate_args(
    schema: &JsonValue,
    script: &str,
) -> Option<JsonMap<String, JsonValue>> {
    let props = schema_properties(schema)?;
    let mut args = JsonMap::new();
    if let Some(key) = first_matching_key(
        props,
        &[
            "function",
            "expression",
            "script",
            "javascript",
            "code",
            "js",
        ],
    ) {
        args.insert(key.to_string(), JsonValue::String(script.to_string()));
    }
    if let Some(key) = first_matching_key(props, &["selector", "element", "locator"]) {
        args.insert(key.to_string(), JsonValue::String("body".to_string()));
    }
    (!args.is_empty()).then_some(args)
}

fn build_browser_wait_args(schema: &JsonValue) -> Option<JsonMap<String, JsonValue>> {
    let props = schema_properties(schema)?;
    let mut args = JsonMap::new();
    if let Some(key) = first_matching_key(
        props,
        &[
            "state",
            "wait_until",
            "waituntil",
            "load",
            "load_state",
            "loadstate",
        ],
    ) {
        args.insert(
            key.to_string(),
            JsonValue::String("networkidle".to_string()),
        );
    }
    if let Some(key) = first_matching_key(
        props,
        &[
            "timeout",
            "timeout_ms",
            "timeoutms",
            "milliseconds",
            "ms",
            "duration",
        ],
    ) {
        args.insert(
            key.to_string(),
            JsonValue::Number(serde_json::Number::from(5000)),
        );
    }
    Some(args)
}

fn extract_text_blobs(value: &JsonValue, out: &mut Vec<String>, depth: usize) {
    if depth > 8 {
        return;
    }
    match value {
        JsonValue::String(s) => {
            if !s.trim().is_empty() {
                out.push(s.trim().to_string());
            }
        }
        JsonValue::Array(items) => {
            for item in items {
                extract_text_blobs(item, out, depth + 1);
            }
        }
        JsonValue::Object(map) => {
            if let Some(text) = map.get("text").and_then(JsonValue::as_str) {
                if !text.trim().is_empty() {
                    out.push(text.trim().to_string());
                }
            }
            for child in map.values() {
                extract_text_blobs(child, out, depth + 1);
            }
        }
        _ => {}
    }
}

fn search_hits_from_browser_result(value: &JsonValue, limit: usize) -> Vec<SearchHit> {
    if let Some(structured) = value.get("structuredContent") {
        let hits = search_hits_from_any_json(structured, limit);
        if !hits.is_empty() {
            return hits;
        }
    }

    let mut blobs = Vec::new();
    extract_text_blobs(value, &mut blobs, 0);
    for blob in blobs {
        if let Ok(parsed) = serde_json::from_str::<JsonValue>(&blob) {
            let hits = search_hits_from_any_json(&parsed, limit);
            if !hits.is_empty() {
                return hits;
            }
        }
    }

    search_hits_from_any_json(value, limit)
}

fn browser_search_url(query: &str) -> String {
    let mut url =
        reqwest::Url::parse(DDG_HTML_SEARCH_BASE).expect("duckduckgo html search base is valid");
    url.query_pairs_mut().append_pair("q", query.trim());
    url.to_string()
}

fn pubmed_candidate_from_schema(
    tool_name: &str,
    description: &str,
    schema: &JsonValue,
    query: &str,
    max_results: u32,
) -> Option<McpToolCandidate> {
    let args = build_query_args_from_schema(schema, query, max_results)?;
    let name = tool_name.to_ascii_lowercase();
    let desc = description.to_ascii_lowercase();
    let mut score = 0;
    if name.contains("pubmed") {
        score += 20;
    }
    if name.contains("search") {
        score += 30;
    }
    if name.contains("article") {
        score += 15;
    }
    if desc.contains("search") || desc.contains("article") || desc.contains("pubmed") {
        score += 20;
    }
    (score >= MCP_PUBMED_MIN_SCORE).then(|| McpToolCandidate {
        tool_name: tool_name.to_string(),
        args,
        score,
    })
}

fn object_to_generic_hit(map: &serde_json::Map<String, JsonValue>) -> Option<SearchHit> {
    let url = first_string_field(
        map,
        &["url", "link", "href", "sourceUrl", "pubmedUrl", "uri"],
    )?;
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return None;
    }
    let title = first_string_field(
        map,
        &["title", "name", "heading", "paperTitle", "label", "source"],
    )
    .unwrap_or_else(|| url.clone());
    let snippet = first_string_field(
        map,
        &[
            "snippet",
            "summary",
            "description",
            "text",
            "content",
            "abstract",
            "markdown",
        ],
    )
    .unwrap_or_default();
    Some(SearchHit {
        title,
        url,
        snippet,
    })
}

fn collect_generic_search_hits(
    value: &JsonValue,
    out: &mut Vec<SearchHit>,
    limit: usize,
    depth: usize,
) {
    if out.len() >= limit || depth > 8 {
        return;
    }
    match value {
        JsonValue::Array(items) => {
            for item in items {
                collect_generic_search_hits(item, out, limit, depth + 1);
                if out.len() >= limit {
                    break;
                }
            }
        }
        JsonValue::Object(map) => {
            if let Some(hit) = object_to_generic_hit(map) {
                out.push(hit);
                if out.len() >= limit {
                    return;
                }
            }
            for child in map.values() {
                collect_generic_search_hits(child, out, limit, depth + 1);
                if out.len() >= limit {
                    break;
                }
            }
        }
        _ => {}
    }
}

fn search_hits_from_any_json(value: &JsonValue, limit: usize) -> Vec<SearchHit> {
    let mut hits = Vec::new();
    collect_generic_search_hits(value, &mut hits, limit, 0);
    dedupe_hits_preserve_order(hits.into_iter().map(sanitize_hit).collect())
}

fn search_hits_from_pubmed_result(value: &JsonValue, limit: usize) -> Vec<SearchHit> {
    let mut out = Vec::new();
    if let Some(items) = value
        .pointer("/structuredContent/summaries")
        .and_then(JsonValue::as_array)
    {
        for item in items {
            let Some(map) = item.as_object() else {
                continue;
            };
            let Some(url) = first_string_field(map, &["pubmedUrl", "url", "link"]) else {
                continue;
            };
            let title = first_string_field(map, &["title"]).unwrap_or_else(|| url.clone());
            let authors = first_string_field(map, &["authors"]).unwrap_or_default();
            let source = first_string_field(map, &["source"]).unwrap_or_default();
            let pub_date =
                first_string_field(map, &["pubDate", "published", "date"]).unwrap_or_default();
            let mut snippet_bits = Vec::new();
            if !authors.is_empty() {
                snippet_bits.push(authors);
            }
            if !source.is_empty() {
                snippet_bits.push(source);
            }
            if !pub_date.is_empty() {
                snippet_bits.push(pub_date);
            }
            out.push(SearchHit {
                title,
                url,
                snippet: snippet_bits.join(" | "),
            });
            if out.len() >= limit {
                break;
            }
        }
    }
    if out.is_empty() {
        search_hits_from_any_json(value, limit)
    } else {
        dedupe_hits_preserve_order(out.into_iter().map(sanitize_hit).collect())
    }
}

fn call_tool_result_to_json(result: &CallToolResult) -> Option<JsonValue> {
    serde_json::to_value(result).ok()
}

fn effective_max_results(args: &WebSearchArgs) -> usize {
    let m = args.max_results.unwrap_or(5).clamp(1, 10) as usize;
    m.min(MAX_RESULTS_CAP)
}

pub struct WebSearchTool;

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

fn has_any_search_key(ctx: &ToolContext) -> bool {
    resolve_tavily_api_key(ctx).is_some()
        || resolve_exa_api_key(ctx).is_some()
        || resolve_firecrawl_api_key(ctx).is_some()
        || resolve_parallel_api_key(ctx).is_some()
}

async fn search_via_configured_apis(
    client: &reqwest::Client,
    ctx: &ToolContext,
    query: &str,
    allowed: &Option<Vec<String>>,
    blocked: &Option<Vec<String>>,
    max_results: usize,
) -> Result<Option<(Vec<SearchHit>, SearchApiProvider)>, ToolError> {
    let max_u32 = max_results.min(20) as u32;

    if let Some(key) = resolve_tavily_api_key(ctx) {
        if let Ok(hits) = search_tavily(client, &key, query, allowed, blocked, max_u32).await {
            if !hits.is_empty() {
                return Ok(Some((hits, SearchApiProvider::Tavily)));
            }
        }
    }

    if let Some(key) = resolve_exa_api_key(ctx) {
        if let Ok(hits) = search_exa_once(client, &key, query, max_u32).await {
            let filtered = filter_hits(&ctx.project_root, hits, allowed, blocked, max_results);
            if !filtered.is_empty() {
                return Ok(Some((filtered, SearchApiProvider::Exa)));
            }
        }
    }

    if let Some(key) = resolve_firecrawl_api_key(ctx) {
        let base = resolve_firecrawl_base_url(ctx);
        if let Ok(hits) = search_firecrawl_once(client, &key, &base, query, max_u32).await {
            let filtered = filter_hits(&ctx.project_root, hits, allowed, blocked, max_results);
            if !filtered.is_empty() {
                return Ok(Some((filtered, SearchApiProvider::Firecrawl)));
            }
        }
    }

    if let Some(key) = resolve_parallel_api_key(ctx) {
        if let Ok(hits) = search_parallel_once(client, &key, query, max_u32).await {
            let filtered = filter_hits(&ctx.project_root, hits, allowed, blocked, max_results);
            if !filtered.is_empty() {
                return Ok(Some((filtered, SearchApiProvider::Parallel)));
            }
        }
    }

    Ok(None)
}

async fn search_pubmed_mcp(
    ctx: &ToolContext,
    query: &str,
    max_results: usize,
) -> Result<Option<SearchExecution>, ToolError> {
    if !merged_mcp_servers(&ctx.project_root).contains_key(PUBMED_SERVER) {
        return Ok(None);
    }

    let timeout = Duration::from_secs(ctx.timeout_secs.clamp(5, 120));
    let tools = match list_tools_for_server(&ctx.project_root, PUBMED_SERVER, timeout).await {
        Ok(tools) => tools,
        Err(err) => {
            let mut exec = SearchExecution::new();
            exec.push_note(format!("PubMed MCP unavailable: {err}"));
            return Ok(Some(exec));
        }
    };

    let mut best: Option<McpToolCandidate> = None;
    for tool in tools {
        let schema =
            serde_json::to_value(&*tool.input_schema).unwrap_or_else(|_| serde_json::json!({}));
        let Some(candidate) = pubmed_candidate_from_schema(
            tool.name.as_ref(),
            tool.description.as_deref().unwrap_or(""),
            &schema,
            query,
            max_results as u32,
        ) else {
            continue;
        };
        if best
            .as_ref()
            .map(|current| candidate.score > current.score)
            .unwrap_or(true)
        {
            best = Some(candidate);
        }
    }

    let Some(candidate) = best else {
        return Ok(None);
    };

    let result = match call_tool_on_server(
        &ctx.project_root,
        PUBMED_SERVER,
        &candidate.tool_name,
        Some(candidate.args),
        timeout,
    )
    .await
    {
        Ok(result) => result,
        Err(err) => {
            let mut exec = SearchExecution::new();
            exec.push_note(format!("PubMed MCP call failed: {err}"));
            return Ok(Some(exec));
        }
    };

    let Some(value) = call_tool_result_to_json(&result) else {
        return Ok(None);
    };
    let hits = search_hits_from_pubmed_result(&value, max_results);
    if hits.is_empty() {
        return Ok(None);
    }

    let mut exec = SearchExecution::new().with_hits(hits);
    exec.push_source(format!(
        "PubMed MCP (`{}` on `{}`)",
        candidate.tool_name, PUBMED_SERVER
    ));
    Ok(Some(exec))
}

async fn search_via_browser_mcp(
    ctx: &ToolContext,
    query: &str,
    max_results: usize,
) -> Result<Option<SearchExecution>, ToolError> {
    let timeout = Duration::from_secs(ctx.timeout_secs.clamp(5, 120));
    let merged = merged_mcp_servers(&ctx.project_root);
    if merged.is_empty() {
        return Ok(None);
    }

    let mut best_server = String::new();
    let mut best_candidate: Option<McpToolCandidate> = None;
    let mut best_bundle: Option<BrowserMcpBundle> = None;

    let mut server_names: Vec<String> = merged.keys().cloned().collect();
    server_names.sort();
    for server_name in server_names {
        let Ok(tools) = list_tools_for_server(&ctx.project_root, &server_name, timeout).await
        else {
            continue;
        };
        let mut navigate_candidate: Option<BrowserMcpAction> = None;
        let mut evaluate_candidate: Option<BrowserMcpAction> = None;
        let mut wait_candidate: Option<BrowserMcpAction> = None;
        for tool in tools {
            let schema =
                serde_json::to_value(&*tool.input_schema).unwrap_or_else(|_| serde_json::json!({}));
            if let Some(candidate) = browser_search_candidate_from_schema(
                &server_name,
                tool.name.as_ref(),
                tool.description.as_deref().unwrap_or(""),
                &schema,
                query,
                max_results as u32,
            ) {
                let should_replace = best_candidate
                    .as_ref()
                    .map(|current| candidate.score > current.score)
                    .unwrap_or(true);
                if should_replace {
                    best_server = server_name.clone();
                    best_candidate = Some(candidate);
                }
            }

            if let Some(candidate) = browser_action_candidate(
                &server_name,
                tool.name.as_ref(),
                tool.description.as_deref().unwrap_or(""),
                &schema,
                &["navigate", "goto", "open"],
            ) {
                if navigate_candidate
                    .as_ref()
                    .map(|current| candidate.score > current.score)
                    .unwrap_or(true)
                {
                    navigate_candidate = Some(candidate);
                }
            }

            if let Some(candidate) = browser_action_candidate(
                &server_name,
                tool.name.as_ref(),
                tool.description.as_deref().unwrap_or(""),
                &schema,
                &["evaluate", "eval", "javascript", "script", "code"],
            ) {
                if evaluate_candidate
                    .as_ref()
                    .map(|current| candidate.score > current.score)
                    .unwrap_or(true)
                {
                    evaluate_candidate = Some(candidate);
                }
            }

            if let Some(candidate) = browser_action_candidate(
                &server_name,
                tool.name.as_ref(),
                tool.description.as_deref().unwrap_or(""),
                &schema,
                &["wait", "load"],
            ) {
                if wait_candidate
                    .as_ref()
                    .map(|current| candidate.score > current.score)
                    .unwrap_or(true)
                {
                    wait_candidate = Some(candidate);
                }
            }
        }

        if let (Some(navigate), Some(evaluate)) = (navigate_candidate, evaluate_candidate) {
            let score = navigate.score
                + evaluate.score
                + wait_candidate.as_ref().map(|c| c.score).unwrap_or(0);
            let bundle = BrowserMcpBundle {
                server_name: server_name.clone(),
                navigate,
                evaluate,
                wait: wait_candidate,
                score,
            };
            if best_bundle
                .as_ref()
                .map(|current| bundle.score > current.score)
                .unwrap_or(true)
            {
                best_bundle = Some(bundle);
            }
        }
    }

    if let Some(candidate) = best_candidate {
        let result = match call_tool_on_server(
            &ctx.project_root,
            &best_server,
            &candidate.tool_name,
            Some(candidate.args),
            timeout,
        )
        .await
        {
            Ok(result) => Some(result),
            Err(_) => None,
        };
        if let Some(result) = result {
            if let Some(value) = call_tool_result_to_json(&result) {
                let hits = search_hits_from_browser_result(&value, max_results);
                if !hits.is_empty() {
                    let mut exec = SearchExecution::new().with_hits(hits);
                    exec.push_source(format!(
                        "Agent-browser MCP (`{}` on `{}`)",
                        candidate.tool_name, best_server
                    ));
                    return Ok(Some(exec));
                }
            }
        }
    }

    let Some(bundle) = best_bundle else {
        return Ok(None);
    };
    let navigate_args =
        match build_url_args_from_schema(&bundle.navigate.schema, &browser_search_url(query)) {
            Some(args) => args,
            None => return Ok(None),
        };
    let connection =
        match connect_mcp_server_legacy(&ctx.project_root, &bundle.server_name, timeout).await {
            Ok(conn) => conn,
            Err(_) => return Ok(None),
        };
    if call_tool_via_peer(
        &connection.peer,
        &bundle.navigate.tool_name,
        Some(navigate_args),
        timeout,
    )
    .await
    .is_err()
    {
        return Ok(None);
    }

    if let Some(wait_tool) = &bundle.wait {
        let wait_args = build_browser_wait_args(&wait_tool.schema).unwrap_or_default();
        let _ = call_tool_via_peer(
            &connection.peer,
            &wait_tool.tool_name,
            Some(wait_args),
            timeout,
        )
        .await;
    }

    let eval_args = match build_browser_evaluate_args(
        &bundle.evaluate.schema,
        BROWSER_SEARCH_EXTRACTION_SCRIPT,
    ) {
        Some(args) => args,
        None => return Ok(None),
    };
    let result = match call_tool_via_peer(
        &connection.peer,
        &bundle.evaluate.tool_name,
        Some(eval_args),
        timeout,
    )
    .await
    {
        Ok(result) => result,
        Err(_) => return Ok(None),
    };
    let Some(value) = call_tool_result_to_json(&result) else {
        return Ok(None);
    };
    let hits = search_hits_from_browser_result(&value, max_results);
    if hits.is_empty() {
        return Ok(None);
    }

    let mut exec = SearchExecution::new().with_hits(hits);
    exec.push_source(format!(
        "Agent-browser MCP bundle (`{}` + `{}` on `{}`)",
        bundle.navigate.tool_name, bundle.evaluate.tool_name, bundle.server_name
    ));
    Ok(Some(exec))
}

async fn run_research_search(
    client: &reqwest::Client,
    ctx: &ToolContext,
    args: &WebSearchArgs,
    max_results: usize,
    search_url: Option<&str>,
) -> Result<SearchExecution, ToolError> {
    let mut execution = SearchExecution::new();

    if let Some(pubmed_exec) = search_pubmed_mcp(ctx, args.query.trim(), max_results).await? {
        for label in pubmed_exec.source_labels {
            execution.push_source(label);
        }
        for note in pubmed_exec.notes {
            execution.push_note(note);
        }
        execution.hits.extend(pubmed_exec.hits);
    }

    if execution.hits.is_empty() {
        let pubmed_query = scoped_site_query(args.query.trim(), PUBMED_HOST);
        if let Some((hits, provider)) = search_via_configured_apis(
            client,
            ctx,
            &pubmed_query,
            &Some(vec![PUBMED_HOST.to_string()]),
            &None,
            max_results,
        )
        .await?
        {
            execution.push_source(format!("PubMed via {}", api_provider_label(provider)));
            execution.hits.extend(hits);
        }
    }

    let biorxiv_query = scoped_site_query(args.query.trim(), BIORXIV_HOST);
    if let Some((hits, provider)) = search_via_configured_apis(
        client,
        ctx,
        &biorxiv_query,
        &Some(vec![BIORXIV_HOST.to_string()]),
        &None,
        max_results,
    )
    .await?
    {
        execution.push_source(format!("bioRxiv via {}", api_provider_label(provider)));
        execution.hits.extend(hits);
    }

    let processed = dedupe_hits_preserve_order(
        execution
            .hits
            .into_iter()
            .map(sanitize_hit)
            .collect::<Vec<_>>(),
    );
    execution.hits = filter_hits(
        &ctx.project_root,
        processed,
        &args.allowed_domains,
        &args.blocked_domains,
        max_results,
    );
    if !execution.hits.is_empty() {
        return Ok(execution);
    }

    let scoped_browser_query = format!(
        "{} (site:{} OR site:{})",
        args.query.trim(),
        PUBMED_HOST,
        BIORXIV_HOST
    );
    if let Some(browser_exec) =
        search_via_browser_mcp(ctx, &scoped_browser_query, max_results).await?
    {
        return Ok(browser_exec);
    }

    let ddg_query = scoped_browser_query;
    let hits = search_ddg(
        client,
        &ddg_query,
        max_results,
        search_url.or(Some(DDG_HTML_SEARCH_BASE)),
    )
    .await?;
    execution.hits = filter_hits(
        &ctx.project_root,
        dedupe_hits_preserve_order(hits),
        &args.allowed_domains,
        &args.blocked_domains,
        max_results,
    );
    execution.push_source("DuckDuckGo scoped fallback (PubMed/bioRxiv)");
    Ok(execution)
}

/// Returns true if the URL's host resolves to a loopback, link-local, or RFC-1918 private
/// address — prevents SSRF via LLM-controlled `search_url` pointing at internal services
/// (AWS metadata 169.254.169.254, localhost, 10/8, 172.16/12, 192.168/16, etc.).
fn validate(args: &WebSearchArgs) -> Result<(), ToolError> {
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

/// One automatic retry on failure (transient network / rate limits).
async fn search_tavily(
    client: &reqwest::Client,
    key: &str,
    query: &str,
    allowed: &Option<Vec<String>>,
    blocked: &Option<Vec<String>>,
    max_results: u32,
) -> Result<Vec<SearchHit>, ToolError> {
    match search_tavily_once(client, key, query, allowed, blocked, max_results).await {
        Ok(v) => Ok(v),
        Err(_) => {
            tokio::time::sleep(Duration::from_millis(TAVILY_RETRY_DELAY_MS)).await;
            search_tavily_once(client, key, query, allowed, blocked, max_results).await
        }
    }
}

async fn search_exa_once(
    client: &reqwest::Client,
    api_key: &str,
    query: &str,
    max_results: u32,
) -> Result<Vec<SearchHit>, ToolError> {
    let n = max_results.min(20).max(1);
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
    let n = max_results.min(20).max(1);
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
    let n = max_results.min(20).max(1);
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

fn ddg_href_for_parse(raw: &str) -> String {
    raw.replace("&amp;", "&")
}

/// Match OpenHarness: unwrap `uddg` on DDG `/l/` redirects; otherwise return the URL unchanged.
fn normalize_result_url(raw_url: &str) -> String {
    let raw = ddg_href_for_parse(raw_url.trim());
    let fixed = if raw.starts_with("//") {
        format!("https:{}", raw)
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
        .unwrap_or("https://html.duckduckgo.com/html/");

    let mut hits = match fetch_ddg_instant_answer_body(client, query).await {
        Ok(body) => match parse_ddg_instant_answer_json(&body, max_results) {
            Ok(h) => h,
            Err(_) => Vec::new(),
        },
        Err(_) => Vec::new(),
    };

    if hits.is_empty() {
        let html = match fetch_ddg_html_body(client, query, html_base).await {
            Ok(s) => s,
            Err(_) => {
                tokio::time::sleep(Duration::from_millis(DDG_HTML_RETRY_DELAY_MS)).await;
                fetch_ddg_html_body(client, query, html_base).await?
            }
        };
        hits = parse_ddg_html_results_openharness(&html, max_results);
    }

    Ok(hits)
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

    /// Live network (optional): `cargo test -p omiga ddg_live_smoke -- --ignored --nocapture`
    /// Skips assertion if DDG is unreachable (firewall / region); succeeds when API+HTML return hits.
    #[tokio::test]
    #[ignore]
    async fn ddg_live_smoke_search_ddg_returns_hits() {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(45))
            .no_proxy()
            .user_agent(concat!("Omiga/", env!("CARGO_PKG_VERSION"), " WebSearch"))
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
    fn detect_research_intent_from_academic_keywords() {
        assert_eq!(
            detect_search_intent("检索肝癌单细胞文献"),
            SearchIntent::Research
        );
        assert_eq!(
            detect_search_intent("latest pubmed papers about fibrosis"),
            SearchIntent::Research
        );
        assert_eq!(
            detect_search_intent("weather in shanghai tomorrow"),
            SearchIntent::General
        );
    }

    #[test]
    fn scoped_site_query_adds_domain_once() {
        assert_eq!(
            scoped_site_query("cholangiocarcinoma review", PUBMED_HOST),
            "cholangiocarcinoma review site:pubmed.ncbi.nlm.nih.gov"
        );
        assert_eq!(
            scoped_site_query(
                "cholangiocarcinoma site:pubmed.ncbi.nlm.nih.gov",
                PUBMED_HOST
            ),
            "cholangiocarcinoma site:pubmed.ncbi.nlm.nih.gov"
        );
    }

    #[test]
    fn generic_json_hit_parser_finds_nested_hits() {
        let value = serde_json::json!({
            "results": [
                {
                    "title": "Example paper",
                    "url": "https://example.org/paper",
                    "snippet": "summary"
                }
            ]
        });
        let hits = search_hits_from_any_json(&value, 5);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].url, "https://example.org/paper");
        assert_eq!(hits[0].title, "Example paper");
    }

    #[test]
    fn browser_candidate_prefers_browser_context_and_query_args() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "query": { "type": "string" },
                "max_results": { "type": "integer" }
            }
        });
        let candidate = browser_search_candidate_from_schema(
            "browser-use",
            "search_web",
            "Search the web in a browser session",
            &schema,
            "gene therapy",
            5,
        )
        .expect("candidate");
        assert_eq!(
            candidate.args.get("query").and_then(JsonValue::as_str),
            Some("gene therapy")
        );
        assert_eq!(
            candidate
                .args
                .get("max_results")
                .and_then(JsonValue::as_u64),
            Some(5)
        );
    }

    #[test]
    fn browser_search_url_builds_duckduckgo_query() {
        let url = browser_search_url("liver fibrosis review");
        assert!(url.starts_with(DDG_HTML_SEARCH_BASE));
        assert!(
            url.contains("q=liver+fibrosis+review") || url.contains("q=liver%20fibrosis%20review")
        );
    }

    #[test]
    fn browser_result_parser_reads_json_text_payload() {
        let payload = serde_json::json!({
            "content": [
                {
                    "type": "text",
                    "text": "[{\"title\":\"Paper A\",\"url\":\"https://example.org/a\",\"snippet\":\"summary\"}]"
                }
            ]
        });
        let hits = search_hits_from_browser_result(&payload, 5);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].title, "Paper A");
        assert_eq!(hits[0].url, "https://example.org/a");
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
}

#[async_trait]
impl super::ToolImpl for WebSearchTool {
    type Args = WebSearchArgs;

    const DESCRIPTION: &'static str = DESCRIPTION;

    async fn execute(
        ctx: &ToolContext,
        args: Self::Args,
    ) -> Result<crate::infrastructure::streaming::StreamOutputBox, ToolError> {
        validate(&args)?;
        let start = Instant::now();

        let timeout = std::time::Duration::from_secs(ctx.timeout_secs.clamp(5, 120));
        let mut client_builder = reqwest::Client::builder()
            .timeout(timeout)
            .user_agent(concat!("Omiga/", env!("CARGO_PKG_VERSION"), " WebSearch"));
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
        let max_n = effective_max_results(&args);
        let search_url = args.search_url.as_deref().filter(|s| !s.trim().is_empty());
        if let Some(search_url) = search_url {
            super::web_safety::validate_public_http_url(&ctx.project_root, search_url, false)
                .map_err(|message| ToolError::InvalidArguments { message })?;
        }
        let had_any_search_key = has_any_search_key(ctx);
        let intent = if has_explicit_domain_override(&args) {
            SearchIntent::General
        } else {
            detect_search_intent(args.query.trim())
        };

        let execution = match intent {
            SearchIntent::Research => {
                let research_fut = run_research_search(&client, ctx, &args, max_n, search_url);
                tokio::select! {
                    _ = ctx.cancel.cancelled() => return Err(ToolError::Cancelled),
                    r = research_fut => r?,
                }
            }
            SearchIntent::General => {
                let mut exec = SearchExecution::new();

                let api_fut = search_via_configured_apis(
                    &client,
                    ctx,
                    args.query.trim(),
                    allowed,
                    blocked,
                    max_n,
                );
                if let Some((hits, provider)) = tokio::select! {
                    _ = ctx.cancel.cancelled() => return Err(ToolError::Cancelled),
                    r = api_fut => r?,
                } {
                    exec.hits = hits;
                    exec.push_source(api_provider_label(provider));
                }

                if exec.hits.is_empty() {
                    let browser_fut = search_via_browser_mcp(ctx, args.query.trim(), max_n);
                    if let Some(browser_exec) = tokio::select! {
                        _ = ctx.cancel.cancelled() => return Err(ToolError::Cancelled),
                        r = browser_fut => r?,
                    } {
                        exec = browser_exec;
                        if had_any_search_key {
                            exec.push_note(
                                "Configured search APIs returned no usable hits; used Agent-browser fallback.",
                            );
                        } else {
                            exec.push_note(
                                "No search API keys configured; used Agent-browser fallback.",
                            );
                        }
                    }
                }

                if exec.hits.is_empty() {
                    let ddg_fut = search_ddg(
                        &client,
                        args.query.trim(),
                        max_n,
                        search_url.or(Some(DDG_HTML_SEARCH_BASE)),
                    );
                    let hits = tokio::select! {
                        _ = ctx.cancel.cancelled() => return Err(ToolError::Cancelled),
                        r = ddg_fut => r?,
                    };
                    exec.hits = filter_hits(&ctx.project_root, hits, allowed, blocked, max_n);
                    if had_any_search_key {
                        exec.push_source(
                            "DuckDuckGo fallback after configured search APIs and Agent-browser returned no usable hits",
                        );
                    } else {
                        exec.push_source(
                            "DuckDuckGo fallback (no search API keys configured and no Agent-browser route matched)",
                        );
                    }
                }

                exec
            }
        };

        let processed = dedupe_hits_preserve_order(
            execution
                .hits
                .into_iter()
                .map(sanitize_hit)
                .collect::<Vec<_>>(),
        );
        let hits = filter_hits(&ctx.project_root, processed, allowed, blocked, max_n);
        let duration = start.elapsed().as_secs_f32();

        let mut text = String::new();
        text.push_str(&format!(
            "Web search results for query: \"{}\"\n\n",
            args.query.trim()
        ));
        text.push_str(&format!(
            "Intent: {}\n",
            match intent {
                SearchIntent::Research => "research",
                SearchIntent::General => "general",
            }
        ));
        text.push_str(&format!(
            "Source: {}\n",
            join_labels(&execution.source_labels)
        ));
        if !execution.notes.is_empty() {
            text.push_str(&format!("Route notes: {}\n", execution.notes.join(" | ")));
        }
        text.push_str(&format!("Duration: {:.2}s\n\n", duration));

        if hits.is_empty() {
            text.push_str("No links matched the filters or the search returned no results.\n");
        } else {
            text.push_str("Results:\n");
            for (i, h) in hits.iter().enumerate() {
                let t = if h.title.is_empty() {
                    h.url.as_str()
                } else {
                    h.title.as_str()
                };
                text.push_str(&format!("{}. {}\n", i + 1, t));
                text.push_str(&format!("   URL: {}\n", h.url));
                if !h.snippet.trim().is_empty() {
                    text.push_str(&format!("   {}\n", h.snippet.trim()));
                }
            }
        }

        text.push_str(
            "\nREMINDER: Include a Sources section with markdown links when you use these URLs in your answer.",
        );

        if text.len() > MAX_OUTPUT_CHARS {
            text.truncate(MAX_OUTPUT_CHARS);
            text.push_str("\n\n[Output truncated]");
        }

        Ok(WebSearchOutput { text }.into_stream())
    }
}

#[derive(Debug, Clone)]
struct WebSearchOutput {
    text: String,
}

impl StreamOutput for WebSearchOutput {
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
        "web_search",
        DESCRIPTION,
        serde_json::json!({
            "type": "object",
            "properties": {
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
            "required": ["query"]
        }),
    )
}
