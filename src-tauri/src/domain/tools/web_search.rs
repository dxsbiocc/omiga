//! Web search — Brave Search API (preferred) or DuckDuckGo JSON fallback
//!
//! Upstream `WebSearchTool` uses Anthropic server-side `web_search`; Omiga runs a real HTTP search.

use super::{ToolContext, ToolError, ToolSchema};
use crate::infrastructure::streaming::{StreamOutput, StreamOutputItem};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::pin::Pin;
use std::time::Instant;

const MAX_RESULTS: usize = 12;
const MAX_OUTPUT_CHARS: usize = 100_000;

pub const DESCRIPTION: &str = r#"Search the public web for up-to-date information.

- If a Brave Search API key is set (Omiga Settings → Advanced, or `OMIGA_BRAVE_API_KEY` / `BRAVE_API_KEY` env), uses Brave Search API (best quality). Settings overrides env when non-empty.
- Otherwise uses DuckDuckGo's JSON API (no key; fewer / less precise hits).
- Optional `allowed_domains` or `blocked_domains` filter result URLs (not both).
- After answering, cite sources with markdown links when you use this tool."#;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSearchArgs {
    pub query: String,
    #[serde(default)]
    pub allowed_domains: Option<Vec<String>>,
    #[serde(default)]
    pub blocked_domains: Option<Vec<String>>,
}

pub struct WebSearchTool;

/// Settings key wins, then env vars.
fn resolve_brave_api_key(ctx: &ToolContext) -> Option<String> {
    if let Some(ref k) = ctx.brave_search_api_key {
        let t = k.trim();
        if !t.is_empty() {
            return Some(t.to_string());
        }
    }
    std::env::var("OMIGA_BRAVE_API_KEY")
        .ok()
        .or_else(|| std::env::var("BRAVE_API_KEY").ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
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
    Ok(())
}

fn host_of_url(url: &str) -> Option<String> {
    reqwest::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(|h| h.to_ascii_lowercase()))
}

fn domain_matches(host: &str, pattern: &str) -> bool {
    let p = pattern.trim().trim_start_matches("www.").to_ascii_lowercase();
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
    hits: Vec<(String, String)>,
    allowed: &Option<Vec<String>>,
    blocked: &Option<Vec<String>>,
) -> Vec<(String, String)> {
    hits.into_iter()
        .filter(|(_, u)| url_passes_filters(u, allowed, blocked))
        .take(MAX_RESULTS)
        .collect()
}

async fn search_brave(
    client: &reqwest::Client,
    key: &str,
    query: &str,
) -> Result<Vec<(String, String)>, ToolError> {
    let resp = client
        .get("https://api.search.brave.com/res/v1/web/search")
        .query(&[("q", query), ("count", "15")])
        .header("Accept", "application/json")
        .header("X-Subscription-Token", key)
        .send()
        .await
        .map_err(|e| ToolError::ExecutionFailed {
            message: format!("Brave request failed: {}", e),
        })?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(ToolError::ExecutionFailed {
            message: format!(
                "Brave HTTP {}: {}",
                status,
                body.chars().take(500).collect::<String>()
            ),
        });
    }

    let v: serde_json::Value = resp.json().await.map_err(|e| ToolError::ExecutionFailed {
        message: format!("Brave JSON: {}", e),
    })?;

    let mut out = Vec::new();
    if let Some(arr) = v.get("web").and_then(|w| w.get("results")).and_then(|r| r.as_array()) {
        for r in arr {
            let title = r
                .get("title")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            let url = r.get("url").and_then(|x| x.as_str()).unwrap_or("");
            if !url.is_empty() {
                out.push((title, url.to_string()));
            }
        }
    }
    Ok(out)
}

async fn search_ddg(
    client: &reqwest::Client,
    query: &str,
) -> Result<Vec<(String, String)>, ToolError> {
    let resp = client
        .get("https://api.duckduckgo.com/")
        .query(&[
            ("q", query),
            ("format", "json"),
            ("no_html", "1"),
            ("skip_disambig", "1"),
        ])
        .send()
        .await
        .map_err(|e| ToolError::ExecutionFailed {
            message: format!("DuckDuckGo request failed: {}", e),
        })?;

    if !resp.status().is_success() {
        return Err(ToolError::ExecutionFailed {
            message: format!("DuckDuckGo HTTP {}", resp.status()),
        });
    }

    let v: serde_json::Value = resp.json().await.map_err(|e| ToolError::ExecutionFailed {
        message: format!("DuckDuckGo JSON: {}", e),
    })?;

    let mut out = Vec::new();

    if let Some(u) = v.get("AbstractURL").and_then(|x| x.as_str()) {
        if !u.is_empty() {
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
            out.push((title, u.to_string()));
        }
    }

    if let Some(topics) = v.get("RelatedTopics").and_then(|x| x.as_array()) {
        collect_ddg_topics(topics, &mut out);
    }

    Ok(out)
}

fn collect_ddg_topics(topics: &[serde_json::Value], out: &mut Vec<(String, String)>) {
    for item in topics {
        if out.len() >= MAX_RESULTS {
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
                    out.push((title, u.to_string()));
                }
                continue;
            }
        }
        if let Some(nested) = item.get("Topics").and_then(|x| x.as_array()) {
            collect_ddg_topics(nested, out);
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

        let key_opt = resolve_brave_api_key(ctx);
        let used_brave = key_opt.is_some();
        let raw = if let Some(key) = key_opt {
            let fut = search_brave(&client, &key, args.query.trim());
            tokio::select! {
                _ = ctx.cancel.cancelled() => return Err(ToolError::Cancelled),
                r = fut => r?,
            }
        } else {
            let fut = search_ddg(&client, args.query.trim());
            tokio::select! {
                _ = ctx.cancel.cancelled() => return Err(ToolError::Cancelled),
                r = fut => r?,
            }
        };

        let hits = filter_hits(raw, allowed, blocked);
        let duration = start.elapsed().as_secs_f32();

        let mut text = String::new();
        text.push_str(&format!(
            "Web search results for query: \"{}\"\n\n",
            args.query.trim()
        ));
        if used_brave {
            text.push_str("Source: Brave Search API\n");
        } else {
            text.push_str("Source: DuckDuckGo (fallback; set OMIGA_BRAVE_API_KEY for Brave Search)\n");
        }
        text.push_str(&format!("Duration: {:.2}s\n\n", duration));

        if hits.is_empty() {
            text.push_str("No links matched the filters or the search returned no results.\n");
        } else {
            text.push_str("Results:\n");
            for (i, (title, url)) in hits.iter().enumerate() {
                let t = if title.is_empty() { url.as_str() } else { title.as_str() };
                text.push_str(&format!("{}. {} — {}\n", i + 1, t, url));
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
                }
            },
            "required": ["query"]
        }),
    )
}
