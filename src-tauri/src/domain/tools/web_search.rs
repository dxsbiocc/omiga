//! Web search — Tavily Search API (preferred) or DuckDuckGo fallback
//!
//! Upstream `WebSearchTool` uses Anthropic server-side `web_search`; Omiga runs a real HTTP search.
//!
//! DuckDuckGo: the public `api.duckduckgo.com` endpoint is an **instant-answer** API, not full web
//! search — many queries return empty `AbstractURL` / `RelatedTopics`. HTML results at
//! `html.duckduckgo.com` are used when the JSON API yields nothing. Result links are often
//! `duckduckgo.com/l/?uddg=…` redirects; we unwrap `uddg` to the real destination URL.

use super::{ToolContext, ToolError, ToolSchema};
use crate::infrastructure::streaming::{StreamOutput, StreamOutputItem};
use async_trait::async_trait;
use lazy_static::lazy_static;
use regex::Regex;
use serde::{Deserialize, Serialize};
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

/// Returns true if the URL's host resolves to a loopback, link-local, or RFC-1918 private
/// address — prevents SSRF via LLM-controlled `search_url` pointing at internal services
/// (AWS metadata 169.254.169.254, localhost, 10/8, 172.16/12, 192.168/16, etc.).
fn is_ssrf_blocked_url(url_str: &str) -> bool {
    let Ok(parsed) = reqwest::Url::parse(url_str) else {
        return false;
    };
    let Some(host) = parsed.host_str() else {
        return true; // no host → block
    };
    let host_lower = host.to_lowercase();

    // Localhost by name
    if host_lower == "localhost" || host_lower.ends_with(".localhost") {
        return true;
    }
    // mDNS .local
    if host_lower == "local" || host_lower.ends_with(".local") {
        return true;
    }

    // IPv4 literal — use std::net for precise range checks
    if let Ok(v4) = host_lower.parse::<std::net::Ipv4Addr>() {
        return v4.is_loopback()     // 127.0.0.0/8
            || v4.is_link_local()   // 169.254.0.0/16
            || v4.is_private()      // 10/8, 172.16/12, 192.168/16
            || v4.is_broadcast()    // 255.255.255.255
            || v4.octets()[0] == 0; // 0.x.x.x
    }

    // IPv6 literal (may be bracketed in the URL host, reqwest strips brackets)
    if let Ok(v6) = host_lower.parse::<std::net::Ipv6Addr>() {
        return v6.is_loopback() // ::1
            || v6.is_multicast() // ff00::/8
            // link-local fe80::/10
            || (v6.segments()[0] & 0xffc0) == 0xfe80;
    }

    false
}

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
            if is_ssrf_blocked_url(t) {
                return Err(ToolError::InvalidArguments {
                    message:
                        "search_url must not point to a loopback, link-local, or private address"
                            .to_string(),
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
    hits: Vec<SearchHit>,
    allowed: &Option<Vec<String>>,
    blocked: &Option<Vec<String>>,
    limit: usize,
) -> Vec<SearchHit> {
    hits.into_iter()
        .filter(|h| url_passes_filters(&h.url, allowed, blocked))
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WebSearchSource {
    Tavily,
    Exa,
    Firecrawl,
    Parallel,
    DuckDuckGo,
    /// At least one paid/search API key was set but none returned usable hits.
    DuckDuckGoFallback,
}

fn source_line(src: WebSearchSource) -> &'static str {
    match src {
        WebSearchSource::Tavily => "Source: Tavily Search API\n",
        WebSearchSource::Exa => "Source: Exa Search API\n",
        WebSearchSource::Firecrawl => "Source: Firecrawl Search API\n",
        WebSearchSource::Parallel => "Source: Parallel Search API\n",
        WebSearchSource::DuckDuckGo => {
            "Source: DuckDuckGo (instant-answer API + HTML; add keys in Settings → Advanced for Tavily/Exa/Firecrawl/Parallel)\n"
        }
        WebSearchSource::DuckDuckGoFallback => {
            "Source: DuckDuckGo (fallback after configured search APIs returned no results or failed)\n"
        }
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
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .no_proxy()
            .user_agent(concat!("Omiga/", env!("CARGO_PKG_VERSION"), " WebSearch"))
            .build()
            .map_err(|e| ToolError::ExecutionFailed {
                message: format!("HTTP client: {}", e),
            })?;

        let allowed = &args.allowed_domains;
        let blocked = &args.blocked_domains;
        let max_n = effective_max_results(&args);
        let max_u32 = max_n.min(20) as u32;
        let search_url = args.search_url.as_deref().filter(|s| !s.trim().is_empty());

        let had_any_search_key = resolve_tavily_api_key(ctx).is_some()
            || resolve_exa_api_key(ctx).is_some()
            || resolve_firecrawl_api_key(ctx).is_some()
            || resolve_parallel_api_key(ctx).is_some();

        let mut raw: Vec<SearchHit> = Vec::new();
        let mut source = WebSearchSource::DuckDuckGo;

        if let Some(key) = resolve_tavily_api_key(ctx) {
            let tavily_fut =
                search_tavily(&client, &key, args.query.trim(), allowed, blocked, max_u32);
            let tavily_res = tokio::select! {
                _ = ctx.cancel.cancelled() => return Err(ToolError::Cancelled),
                r = tavily_fut => r,
            };
            if let Ok(hits) = tavily_res {
                if !hits.is_empty() {
                    raw = hits;
                    source = WebSearchSource::Tavily;
                }
            }
        }

        if raw.is_empty() {
            if let Some(key) = resolve_exa_api_key(ctx) {
                let exa_fut = search_exa_once(&client, &key, args.query.trim(), max_u32);
                let exa_res = tokio::select! {
                    _ = ctx.cancel.cancelled() => return Err(ToolError::Cancelled),
                    r = exa_fut => r,
                };
                if let Ok(hits) = exa_res {
                    if !hits.is_empty() {
                        raw = hits;
                        source = WebSearchSource::Exa;
                    }
                }
            }
        }

        if raw.is_empty() {
            if let Some(key) = resolve_firecrawl_api_key(ctx) {
                let base = resolve_firecrawl_base_url(ctx);
                let fc_fut =
                    search_firecrawl_once(&client, &key, &base, args.query.trim(), max_u32);
                let fc_res = tokio::select! {
                    _ = ctx.cancel.cancelled() => return Err(ToolError::Cancelled),
                    r = fc_fut => r,
                };
                if let Ok(hits) = fc_res {
                    if !hits.is_empty() {
                        raw = hits;
                        source = WebSearchSource::Firecrawl;
                    }
                }
            }
        }

        if raw.is_empty() {
            if let Some(key) = resolve_parallel_api_key(ctx) {
                let par_fut = search_parallel_once(&client, &key, args.query.trim(), max_u32);
                let par_res = tokio::select! {
                    _ = ctx.cancel.cancelled() => return Err(ToolError::Cancelled),
                    r = par_fut => r,
                };
                if let Ok(hits) = par_res {
                    if !hits.is_empty() {
                        raw = hits;
                        source = WebSearchSource::Parallel;
                    }
                }
            }
        }

        if raw.is_empty() {
            let ddg_fut = search_ddg(&client, args.query.trim(), max_n, search_url);
            let h = tokio::select! {
                _ = ctx.cancel.cancelled() => return Err(ToolError::Cancelled),
                r = ddg_fut => r?,
            };
            raw = h;
            source = if had_any_search_key {
                WebSearchSource::DuckDuckGoFallback
            } else {
                WebSearchSource::DuckDuckGo
            };
        }

        let processed = dedupe_hits_preserve_order(raw.into_iter().map(sanitize_hit).collect());
        let hits = filter_hits(processed, allowed, blocked, max_n);
        let duration = start.elapsed().as_secs_f32();

        let mut text = String::new();
        text.push_str(&format!(
            "Web search results for query: \"{}\"\n\n",
            args.query.trim()
        ));
        text.push_str(source_line(source));
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
