use super::common::{displayed_link_for_url, favicon_for_url, resolve_url, title_from_result};
use super::FetchArgs;
use crate::domain::tools::{ToolContext, ToolError};
use lazy_static::lazy_static;
use regex::Regex;
use serde_json::{json, Value as JsonValue};
use std::io::Cursor;
use std::time::{Duration, Instant};

const MAX_BODY_BYTES: u64 = 10 * 1024 * 1024;
const MAX_TEXT_CHARS: usize = 100_000;
const BROWSER_FETCH_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";
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

pub(super) async fn fetch_web_json(
    ctx: &ToolContext,
    args: &FetchArgs,
    requested_source: &str,
) -> Result<JsonValue, ToolError> {
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

    crate::domain::tools::web_safety::validate_public_http_url(&ctx.project_root, url.trim(), true)
        .map_err(|message| ToolError::InvalidArguments { message })?;

    let timeout = Duration::from_secs(ctx.timeout_secs.clamp(5, 120));

    let project_root_for_redirect = ctx.project_root.clone();
    let mut client_builder = reqwest::Client::builder()
        .timeout(timeout)
        .redirect(reqwest::redirect::Policy::custom(move |attempt| {
            if attempt.previous().len() >= 10 {
                return attempt.error("Too many redirects");
            }
            match crate::domain::tools::web_safety::validate_public_http_url(
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
    crate::domain::tools::web_safety::validate_public_http_url(&ctx.project_root, &final_url, true)
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
    Ok(value)
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
}
