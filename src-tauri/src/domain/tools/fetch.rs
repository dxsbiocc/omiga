//! Unified fetch tool — retrieve details for web pages or source-specific records.
//!
//! The public model-visible function is `fetch`; first-version adapters are:
//! - `category="web"`: safe public HTTP(S) fetch and text extraction.
//! - `category="literature", source="pubmed"`: official NCBI EFetch by PMID.

use super::{ToolContext, ToolError, ToolSchema};
use crate::infrastructure::streaming::{StreamOutput, StreamOutputItem};
use async_trait::async_trait;
use lazy_static::lazy_static;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};
use std::io::Cursor;
use std::pin::Pin;
use std::time::{Duration, Instant};

const MAX_BODY_BYTES: u64 = 10 * 1024 * 1024;
const MAX_TEXT_CHARS: usize = 100_000;
const BROWSER_FETCH_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";
const PUBMED_HOST: &str = "pubmed.ncbi.nlm.nih.gov";

pub const DESCRIPTION: &str = r#"Fetch one document/detail from a typed data source and return formatted JSON.

- `category` is required. First-version categories: `web`, `literature`, `social`.
- `source` is optional and defaults to `auto`. First-version concrete sources: `web.auto`, `literature.pubmed`, optional `social.wechat`.
- Locate the document with one of: `url`, `id` + `source`, or a full `result` object returned by `search`.
- `web` fetch sends a safe public HTTP(S) GET, follows public-safe redirects, blocks private/loopback targets, converts HTML to text, and pretty-prints JSON.
- `literature.pubmed` fetch expects a numeric PMID in `id` (or a PubMed URL / search result) and uses official NCBI EFetch.
- `social.wechat` is disabled by default; when enabled it fetches the article URL with the safe web fetcher.
- Results are returned as formatted JSON with `title`, `link`, `url`, `favicon`, `content`, and `metadata`."#;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FetchArgs {
    pub category: String,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub result: Option<JsonValue>,
    #[serde(default)]
    pub prompt: Option<String>,
}

pub struct FetchTool;

fn browser_fetch_headers() -> reqwest::header::HeaderMap {
    let mut h = reqwest::header::HeaderMap::new();
    h.insert(
        reqwest::header::USER_AGENT,
        reqwest::header::HeaderValue::from_static(BROWSER_FETCH_USER_AGENT),
    );
    h.insert(
        reqwest::header::ACCEPT,
        reqwest::header::HeaderValue::from_static(
            "text/html,application/xhtml+xml,application/xml;q=0.9,application/json;q=0.8,*/*;q=0.7",
        ),
    );
    h.insert(
        reqwest::header::HeaderName::from_static("accept-language"),
        reqwest::header::HeaderValue::from_static("en-US,en;q=0.9"),
    );
    h
}

fn clean_inline_base64_images(text: &str) -> String {
    lazy_static! {
        static ref RE_BASE64_PARENS: Regex =
            Regex::new(r"\(data:image/[^;]+;base64,[A-Za-z0-9+/=]+\)").expect("regex");
        static ref RE_BASE64_PLAIN: Regex =
            Regex::new(r"data:image/[^;]+;base64,[A-Za-z0-9+/=]+").expect("regex");
    }
    let without_parens = RE_BASE64_PARENS.replace_all(text, "[image omitted]");
    RE_BASE64_PLAIN
        .replace_all(without_parens.as_ref(), "[image omitted]")
        .to_string()
}

#[async_trait]
impl super::ToolImpl for FetchTool {
    type Args = FetchArgs;

    const DESCRIPTION: &'static str = DESCRIPTION;

    async fn execute(
        ctx: &ToolContext,
        args: Self::Args,
    ) -> Result<crate::infrastructure::streaming::StreamOutputBox, ToolError> {
        let category = normalized_category(&args.category);
        let source = normalized_source(args.source.as_deref());
        match category.as_str() {
            "web" => fetch_web(ctx, &args, &source).await,
            "literature" => match source.as_str() {
                "auto" | "pubmed" => fetch_pubmed(ctx, &args).await,
                "semantic_scholar" | "semanticscholar" => {
                    if !ctx.web_search_api_keys.semantic_scholar_enabled {
                        return Ok(json_stream(structured_error_json(
                            "source_disabled",
                            "literature",
                            &source,
                            "literature.semantic_scholar is disabled. Enable it and configure an API key in Settings → Search.",
                        )));
                    }
                    Ok(json_stream(structured_error_json(
                        "source_not_implemented",
                        "literature",
                        &source,
                        format!("literature source `{source}` fetch is registered but not implemented in the first version"),
                    )))
                }
                "arxiv" | "openalex" => {
                    Ok(json_stream(structured_error_json(
                        "source_not_implemented",
                        "literature",
                        &source,
                        format!("literature source `{source}` is registered but not implemented in the first version"),
                    )))
                }
                other => Err(ToolError::InvalidArguments {
                    message: format!("Unsupported literature fetch source: {other}"),
                }),
            },
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
                    fetch_web(ctx, &args, &source).await
                }
                other => Err(ToolError::InvalidArguments {
                    message: format!("Unsupported social fetch source: {other}"),
                }),
            },
            other => Err(ToolError::InvalidArguments {
                message: format!("Unsupported fetch category: {other}"),
            }),
        }
    }
}

async fn fetch_pubmed(
    ctx: &ToolContext,
    args: &FetchArgs,
) -> Result<crate::infrastructure::streaming::StreamOutputBox, ToolError> {
    let pmid = resolve_pubmed_pmid(args).ok_or_else(|| ToolError::InvalidArguments {
        message: "PubMed fetch expects a numeric PMID via `id`, a PubMed `url`, or a PubMed search `result`. DOI-to-PMID resolution is planned for a later version.".to_string(),
    })?;
    let client = crate::domain::search::pubmed::EntrezClient::from_tool_context(ctx)
        .map_err(|message| ToolError::ExecutionFailed { message })?;
    let detail = tokio::select! {
        _ = ctx.cancel.cancelled() => return Err(ToolError::Cancelled),
        r = client.fetch_by_pmid(&pmid) => r.map_err(|message| ToolError::ExecutionFailed { message })?,
    };
    Ok(json_stream(crate::domain::search::pubmed::detail_to_json(
        &detail,
    )))
}

async fn fetch_web(
    ctx: &ToolContext,
    args: &FetchArgs,
    requested_source: &str,
) -> Result<crate::infrastructure::streaming::StreamOutputBox, ToolError> {
    let start = Instant::now();
    let url = resolve_url(args).ok_or_else(|| ToolError::InvalidArguments {
        message: "fetch(category=web) requires `url` or a search `result` with `url`/`link`"
            .to_string(),
    })?;

    let parsed = reqwest::Url::parse(&url).map_err(|e| ToolError::InvalidArguments {
        message: format!("Invalid URL: {}", e),
    })?;

    match parsed.scheme() {
        "http" | "https" => {}
        other => {
            return Err(ToolError::InvalidArguments {
                message: format!("Unsupported URL scheme: {}", other),
            })
        }
    }

    super::web_safety::validate_public_http_url(&ctx.project_root, url.trim(), true)
        .map_err(|message| ToolError::InvalidArguments { message })?;

    let timeout = Duration::from_secs(ctx.timeout_secs.clamp(5, 120));

    let project_root_for_redirect = ctx.project_root.clone();
    let mut client_builder = reqwest::Client::builder()
        .timeout(timeout)
        .redirect(reqwest::redirect::Policy::custom(move |attempt| {
            if attempt.previous().len() >= 10 {
                return attempt.error("Too many redirects");
            }
            match super::web_safety::validate_public_http_url(
                &project_root_for_redirect,
                attempt.url().as_str(),
                false,
            ) {
                Ok(_) => attempt.follow(),
                Err(message) => attempt.error(message),
            }
        }))
        .default_headers(browser_fetch_headers());
    if !ctx.web_use_proxy {
        client_builder = client_builder.no_proxy();
    }
    let client = client_builder
        .build()
        .map_err(|e| ToolError::ExecutionFailed {
            message: format!("HTTP client: {}", e),
        })?;

    let send_fut = client.get(url.clone()).send();

    let response = tokio::select! {
        _ = ctx.cancel.cancelled() => return Err(ToolError::Cancelled),
        res = send_fut => res.map_err(|e| ToolError::ExecutionFailed {
            message: format!("HTTP error: {}", e),
        })?,
    };

    let status = response.status();
    let code = status.as_u16();
    let code_text = status.canonical_reason().unwrap_or("Unknown").to_string();

    let final_url = response.url().to_string();
    super::web_safety::validate_public_http_url(&ctx.project_root, &final_url, true)
        .map_err(|message| ToolError::ExecutionFailed { message })?;
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_string();

    let content_length = response.content_length();

    let body_bytes = tokio::select! {
        _ = ctx.cancel.cancelled() => return Err(ToolError::Cancelled),
        b = response.bytes() => b.map_err(|e| ToolError::ExecutionFailed {
            message: format!("Read body: {}", e),
        })?,
    };

    if body_bytes.len() as u64 > MAX_BODY_BYTES {
        return Err(ToolError::ExecutionFailed {
            message: format!(
                "Response body too large ({} bytes, max {} bytes)",
                body_bytes.len(),
                MAX_BODY_BYTES
            ),
        });
    }

    let ct_lower = content_type.to_ascii_lowercase();
    let sniff_html = sniff_likely_html(&body_bytes);
    let text = if ct_lower.contains("html") || sniff_html {
        html2text::from_read(Cursor::new(body_bytes.as_ref()), 120).map_err(|e| {
            ToolError::ExecutionFailed {
                message: format!("HTML conversion: {}", e),
            }
        })?
    } else if ct_lower.contains("json") {
        let s = String::from_utf8(body_bytes.to_vec()).map_err(|_| ToolError::ExecutionFailed {
            message: "Response body is not valid UTF-8".to_string(),
        })?;
        serde_json::from_str::<serde_json::Value>(&s)
            .ok()
            .and_then(|v| serde_json::to_string_pretty(&v).ok())
            .unwrap_or(s)
    } else if ct_lower.starts_with("text/")
        || ct_lower.contains("javascript")
        || ct_lower.contains("xml")
    {
        String::from_utf8(body_bytes.to_vec()).map_err(|_| ToolError::ExecutionFailed {
            message: "Text response is not valid UTF-8".to_string(),
        })?
    } else {
        return Err(ToolError::ExecutionFailed {
            message: format!(
                "Unsupported content type for text extraction: {}. Try a URL that returns HTML, JSON, or plain text.",
                content_type
            ),
        });
    };

    let cleaned = clean_inline_base64_images(&text);
    let (content, truncated_note) = truncate_chars(&cleaned, MAX_TEXT_CHARS);
    let title = title_from_result(args).unwrap_or_else(|| final_url.clone());
    let value = json!({
        "category": "web",
        "source": requested_source,
        "effective_source": "http",
        "title": title,
        "name": title,
        "link": final_url,
        "url": final_url,
        "requested_url": url,
        "displayed_link": displayed_link_for_url(&final_url),
        "favicon": favicon_for_url(&final_url),
        "status": code,
        "status_text": code_text,
        "content_type": content_type,
        "content_length": content_length,
        "duration_ms": start.elapsed().as_millis(),
        "prompt": args.prompt.as_deref().unwrap_or(""),
        "content": content,
        "truncated_note": truncated_note,
        "metadata": {
            "bytes_decoded_text": text.len(),
        }
    });
    Ok(json_stream(value))
}

fn normalized_category(value: &str) -> String {
    value.trim().to_ascii_lowercase().replace('-', "_")
}

fn normalized_source(value: Option<&str>) -> String {
    value
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("auto")
        .to_ascii_lowercase()
        .replace('-', "_")
}

fn resolve_url(args: &FetchArgs) -> Option<String> {
    args.url
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| string_from_result(args, &["url", "link", "href"]))
}

fn title_from_result(args: &FetchArgs) -> Option<String> {
    string_from_result(args, &["title", "name"])
}

fn string_from_result(args: &FetchArgs, keys: &[&str]) -> Option<String> {
    let object = args.result.as_ref()?.as_object()?;
    for key in keys {
        let value = object
            .get(*key)
            .and_then(JsonValue::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty());
        if let Some(value) = value {
            return Some(value.to_string());
        }
    }
    None
}

fn resolve_pubmed_pmid(args: &FetchArgs) -> Option<String> {
    let raw = args
        .id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| string_from_result(args, &["id", "pmid"]))
        .or_else(|| {
            args.result
                .as_ref()
                .and_then(|v| v.get("metadata"))
                .and_then(|m| m.get("pmid"))
                .and_then(JsonValue::as_str)
                .map(str::to_string)
        })
        .or_else(|| resolve_url(args).and_then(|url| pmid_from_pubmed_url(&url)))?;
    let trimmed = raw.trim().trim_start_matches("PMID:").trim();
    trimmed
        .chars()
        .all(|c| c.is_ascii_digit())
        .then(|| trimmed.to_string())
}

fn pmid_from_pubmed_url(url: &str) -> Option<String> {
    let parsed = reqwest::Url::parse(url).ok()?;
    let host = parsed.host_str()?.to_ascii_lowercase();
    if !host.ends_with(PUBMED_HOST) {
        return None;
    }
    parsed
        .path_segments()?
        .find(|segment| !segment.is_empty() && segment.chars().all(|c| c.is_ascii_digit()))
        .map(str::to_string)
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
    })
}

fn json_stream(value: JsonValue) -> crate::infrastructure::streaming::StreamOutputBox {
    let text = serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string());
    FetchOutput { text }.into_stream()
}

fn sniff_likely_html(bytes: &[u8]) -> bool {
    let take = bytes.len().min(512);
    let slice = &bytes[..take];
    let Ok(s) = std::str::from_utf8(slice) else {
        return false;
    };
    let t = s.trim_start();
    let l = t.to_ascii_lowercase();
    l.starts_with("<!doctype html") || l.starts_with("<html")
}

fn truncate_chars(s: &str, max: usize) -> (String, Option<String>) {
    if s.len() <= max {
        return (s.to_string(), None);
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    let note = format!(
        "Truncated to {} characters; full decoded text was {} characters",
        end,
        s.len()
    );
    (s[..end].to_string(), Some(note))
}

#[derive(Debug, Clone)]
struct FetchOutput {
    text: String,
}

impl StreamOutput for FetchOutput {
    fn into_stream(self) -> Pin<Box<dyn futures::Stream<Item = StreamOutputItem> + Send>> {
        use futures::stream;
        let items = vec![
            StreamOutputItem::Start,
            StreamOutputItem::Content(self.text),
            StreamOutputItem::Complete,
        ];
        Box::pin(stream::iter(items))
    }
}

pub fn schema() -> ToolSchema {
    ToolSchema::new(
        "fetch",
        DESCRIPTION,
        serde_json::json!({
            "type": "object",
            "properties": {
                "category": {
                    "type": "string",
                    "description": "Data-source category. First version supports web, literature, social."
                },
                "source": {
                    "type": "string",
                    "description": "Source within the category. Defaults to auto. For literature, first version supports pubmed."
                },
                "url": {
                    "type": "string",
                    "description": "Fully qualified URL to fetch, or a PubMed URL for literature.pubmed"
                },
                "id": {
                    "type": "string",
                    "description": "Source-specific identifier. PubMed fetch currently expects PMID."
                },
                "result": {
                    "type": "object",
                    "description": "A single result object returned by search; fetch will read url/link/id/metadata.pmid from it."
                },
                "prompt": {
                    "type": "string",
                    "description": "What to extract or how to use the fetched document (included in JSON output)"
                }
            },
            "required": ["category"]
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_inline_base64_images_replaces_payloads() {
        let raw = "before data:image/png;base64,abcd1234+/= after";
        let cleaned = clean_inline_base64_images(raw);
        assert!(cleaned.contains("[image omitted]"));
        assert!(!cleaned.contains("base64"));
    }

    #[test]
    fn truncate_chars_preserves_boundaries() {
        let s = "é".repeat(10);
        let (t, note) = truncate_chars(&s, 7);
        assert!(t.is_char_boundary(t.len()));
        assert!(note.is_some());
    }

    #[test]
    fn resolves_url_from_search_result_link() {
        let args = FetchArgs {
            category: "web".into(),
            source: None,
            url: None,
            id: None,
            result: Some(json!({"title":"A","link":"https://example.org/a"})),
            prompt: None,
        };
        assert_eq!(resolve_url(&args).as_deref(), Some("https://example.org/a"));
        assert_eq!(title_from_result(&args).as_deref(), Some("A"));
    }

    #[test]
    fn resolves_pubmed_pmid_from_url_or_metadata() {
        let from_url = FetchArgs {
            category: "literature".into(),
            source: Some("pubmed".into()),
            url: Some("https://pubmed.ncbi.nlm.nih.gov/12345678/".into()),
            id: None,
            result: None,
            prompt: None,
        };
        assert_eq!(resolve_pubmed_pmid(&from_url).as_deref(), Some("12345678"));

        let from_result = FetchArgs {
            category: "literature".into(),
            source: Some("pubmed".into()),
            url: None,
            id: None,
            result: Some(json!({"metadata":{"pmid":"42"}})),
            prompt: None,
        };
        assert_eq!(resolve_pubmed_pmid(&from_result).as_deref(), Some("42"));
    }
}
