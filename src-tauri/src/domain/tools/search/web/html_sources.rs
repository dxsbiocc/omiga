use super::super::common::SearchHit;
use super::web_common::{dedupe_hits_preserve_order, host_of_url, sanitize_hit};
use crate::domain::tools::ToolError;
use base64::{
    engine::general_purpose::{STANDARD as BASE64_STANDARD, URL_SAFE, URL_SAFE_NO_PAD},
    Engine as _,
};
use lazy_static::lazy_static;
use regex::Regex;

const DDG_HTML_SEARCH_BASE: &str = "https://html.duckduckgo.com/html/";
const BING_SEARCH_BASE: &str = "https://www.bing.com/search";
const GOOGLE_SEARCH_BASE: &str = "https://www.google.com/search";

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
pub(super) fn normalize_result_url(raw_url: &str) -> String {
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
pub(super) fn parse_ddg_instant_answer_json(
    body: &str,
    max: usize,
) -> Result<Vec<SearchHit>, ToolError> {
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
pub(super) fn parse_ddg_html_results_openharness(body: &str, limit: usize) -> Vec<SearchHit> {
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

pub(super) fn parse_bing_html_results(body: &str, limit: usize) -> Vec<SearchHit> {
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

pub(super) fn parse_google_html_results(body: &str, limit: usize) -> Vec<SearchHit> {
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
pub(super) async fn search_ddg(
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

pub(super) async fn search_bing(
    client: &reqwest::Client,
    query: &str,
    max_results: usize,
) -> Result<Vec<SearchHit>, ToolError> {
    let html = fetch_bing_html_body(client, query).await?;
    Ok(parse_bing_html_results(&html, max_results))
}

pub(super) async fn search_google(
    client: &reqwest::Client,
    query: &str,
    max_results: usize,
) -> Result<Vec<SearchHit>, ToolError> {
    let html = fetch_google_html_body(client, query).await?;
    Ok(parse_google_html_results(&html, max_results))
}
