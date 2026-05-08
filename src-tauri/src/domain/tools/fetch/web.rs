use super::common::{displayed_link_for_url, favicon_for_url, resolve_url, title_from_result};
use super::FetchArgs;
use crate::domain::tools::{ToolContext, ToolError};
use futures::StreamExt;
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
            match validate_fetch_redirect_target(&project_root_for_redirect, attempt.url().as_str())
            {
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
    reject_oversized_content_length(content_length, MAX_BODY_BYTES)?;
    let body_bytes =
        read_body_limited(response, content_length, MAX_BODY_BYTES, &ctx.cancel).await?;

    let ct_lower = content_type.to_ascii_lowercase();
    let sniff_html = sniff_likely_html(&body_bytes);
    let text = if ct_lower.contains("html") || sniff_html {
        html2text::from_read(Cursor::new(body_bytes.as_slice()), 120).map_err(|e| {
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

fn validate_fetch_redirect_target(project_root: &std::path::Path, url: &str) -> Result<(), String> {
    crate::domain::tools::web_safety::validate_public_http_url(project_root, url, true)
}

fn body_too_large_error(bytes: u64, max_bytes: u64) -> ToolError {
    ToolError::ExecutionFailed {
        message: format!("Response body too large ({bytes} bytes, max {max_bytes} bytes)"),
    }
}

fn reject_oversized_content_length(
    content_length: Option<u64>,
    max_bytes: u64,
) -> Result<(), ToolError> {
    if let Some(bytes) = content_length {
        if bytes > max_bytes {
            return Err(body_too_large_error(bytes, max_bytes));
        }
    }
    Ok(())
}

async fn read_body_limited(
    response: reqwest::Response,
    content_length: Option<u64>,
    max_bytes: u64,
    cancel: &tokio_util::sync::CancellationToken,
) -> Result<Vec<u8>, ToolError> {
    let capacity = content_length.unwrap_or_default().min(max_bytes) as usize;
    let mut body = Vec::with_capacity(capacity);
    let mut stream = response.bytes_stream();

    while let Some(chunk) = tokio::select! {
        _ = cancel.cancelled() => return Err(ToolError::Cancelled),
        chunk = stream.next() => chunk,
    } {
        let chunk = chunk.map_err(|e| ToolError::ExecutionFailed {
            message: format!("Read body: {}", e),
        })?;
        let next_len = (body.len() as u64)
            .checked_add(chunk.len() as u64)
            .ok_or_else(|| ToolError::ExecutionFailed {
                message: "Response body size overflowed".to_string(),
            })?;
        if next_len > max_bytes {
            return Err(body_too_large_error(next_len, max_bytes));
        }
        body.extend_from_slice(&chunk);
    }

    Ok(body)
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
    fn rejects_oversized_content_length_before_reading_body() {
        let err = reject_oversized_content_length(Some(101), 100).expect_err("oversized");
        assert!(err.to_string().contains("Response body too large"));
    }

    #[test]
    fn redirect_validation_resolves_dns_before_following() {
        let root = tempfile::TempDir::new().expect("tempdir");
        let err = validate_fetch_redirect_target(root.path(), "http://example.invalid/")
            .expect_err("redirect target should require DNS resolution");
        assert!(err.contains("DNS resolution failed"));
    }

    #[tokio::test]
    async fn streaming_body_reader_enforces_running_limit() {
        use std::io::Write;
        use std::io::{BufRead, BufReader};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            {
                let mut reader = BufReader::new(stream.try_clone().expect("clone stream"));
                let mut line = String::new();
                while reader.read_line(&mut line).expect("read request") > 0 {
                    if line == "\r\n" {
                        break;
                    }
                    line.clear();
                }
            }
            let body = "abcdef";
            let response = format!(
                "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n{:x}\r\n{}\r\n0\r\n\r\n",
                body.len(),
                body
            );
            stream.write_all(response.as_bytes()).expect("write");
            stream.flush().expect("flush");
        });

        let client = reqwest::Client::builder()
            .no_proxy()
            .build()
            .expect("client");
        let response = client
            .get(format!("http://{addr}/"))
            .send()
            .await
            .expect("response");
        let err = read_body_limited(
            response,
            None,
            5,
            &tokio_util::sync::CancellationToken::new(),
        )
        .await
        .expect_err("body should exceed streaming limit");
        server.join().expect("server thread");
        assert!(err.to_string().contains("Response body too large"));
    }

    #[test]
    fn truncate_chars_preserves_boundaries() {
        let s = "é".repeat(10);
        let (t, note) = truncate_chars(&s, 7);
        assert!(t.is_char_boundary(t.len()));
        assert!(note.is_some());
    }
}
