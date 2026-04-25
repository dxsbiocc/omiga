//! WebFetch — HTTP GET a URL and return text for the model
//!
//! Aligns with `src/tools/WebFetchTool`: `url` + `prompt`. The upstream app runs a
//! secondary model on the markdown; Omiga returns fetched text plus the prompt so
//! the **main** model can answer in the next turn without a nested LLM call.

use super::{ToolContext, ToolError, ToolSchema};
use crate::infrastructure::streaming::{StreamOutput, StreamOutputItem};
use async_trait::async_trait;
use lazy_static::lazy_static;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::io::Cursor;
use std::pin::Pin;
use std::time::{Duration, Instant};

/// Same ballpark as TS `MAX_HTTP_CONTENT_LENGTH`
const MAX_BODY_BYTES: u64 = 10 * 1024 * 1024;
/// Same as TS `MAX_MARKDOWN_LENGTH`
const MAX_TEXT_CHARS: usize = 100_000;
const BROWSER_FETCH_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

pub const DESCRIPTION: &str = r#"Fetch public web content over HTTP(S).

- Sends a GET request to the given URL (redirects followed, up to 10 hops).
- Converts HTML pages to plain text; returns JSON and plain text as UTF-8 when possible.
- The `prompt` field is included in the tool result so you can answer using the fetched text in your next message (this build does not run a separate summarization model inside the tool).
- Will fail for authenticated or private pages; prefer specialized tools or MCP when available.
- Blocks loopback/private-network targets and URLs that appear to embed credentials or secret-bearing query params.
- Read-only; does not write project files."#;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebFetchArgs {
    /// Fully qualified http(s) URL
    pub url: String,
    /// What to extract or how to use the page — echoed in the result for follow-up reasoning
    pub prompt: String,
}

pub struct WebFetchTool;

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
impl super::ToolImpl for WebFetchTool {
    type Args = WebFetchArgs;

    const DESCRIPTION: &'static str = DESCRIPTION;

    async fn execute(
        ctx: &ToolContext,
        args: Self::Args,
    ) -> Result<crate::infrastructure::streaming::StreamOutputBox, ToolError> {
        let start = Instant::now();

        let parsed = reqwest::Url::parse(&args.url).map_err(|e| ToolError::InvalidArguments {
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

        super::web_safety::validate_public_http_url(&ctx.project_root, args.url.trim(), true)
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

        let send_fut = client.get(args.url.clone()).send();

        let response = tokio::select! {
            _ = ctx.cancel.cancelled() => {
                return Err(ToolError::Cancelled);
            }
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
            _ = ctx.cancel.cancelled() => {
                return Err(ToolError::Cancelled);
            }
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
            let s =
                String::from_utf8(body_bytes.to_vec()).map_err(|_| ToolError::ExecutionFailed {
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
        let (truncated, truncated_note) = truncate_chars(&cleaned, MAX_TEXT_CHARS);

        let mut result = String::new();
        result.push_str(&format!("HTTP {} {}\n", code, code_text));
        result.push_str(&format!("Final URL: {}\n", final_url));
        if final_url != args.url {
            result.push_str(&format!("Requested URL: {}\n", args.url));
        }
        result.push_str(&format!("Content-Type: {}\n", content_type));
        if let Some(len) = content_length {
            result.push_str(&format!("Content-Length: {}\n", len));
        }
        result.push_str(&format!("Bytes (decoded text): {}\n", text.len()));
        result.push_str(&format!("Duration: {}ms\n\n", start.elapsed().as_millis()));
        result.push_str("--- fetched text ---\n");
        result.push_str(&truncated);
        if let Some(note) = truncated_note {
            result.push_str(&note);
        }
        result.push_str("\n\n--- instruction ---\n");
        result.push_str(&args.prompt);

        Ok(WebFetchOutput { text: result }.into_stream())
    }
}

/// When servers send `application/octet-stream` or omit Content-Type, still treat obvious HTML.
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
        "\n\n[Truncated to {} characters; full decoded text was {} characters]",
        end,
        s.len()
    );
    (s[..end].to_string(), Some(note))
}

#[derive(Debug, Clone)]
struct WebFetchOutput {
    text: String,
}

impl StreamOutput for WebFetchOutput {
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
        "web_fetch",
        DESCRIPTION,
        serde_json::json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "Fully qualified http(s) URL to fetch"
                },
                "prompt": {
                    "type": "string",
                    "description": "What you want to know or extract from the page (included in the tool result for your follow-up reasoning)"
                }
            },
            "required": ["url", "prompt"]
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
        let (truncated, note) = truncate_chars("你好hello", 3);
        assert!(truncated.chars().count() <= 3);
        assert!(note.is_some());
    }
}
