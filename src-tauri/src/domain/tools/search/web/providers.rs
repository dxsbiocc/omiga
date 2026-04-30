use super::super::common::SearchHit;
use crate::domain::tools::ToolError;
use serde::Serialize;

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

pub(super) async fn search_tavily_once(
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

pub(super) async fn search_exa_once(
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

pub(super) async fn search_firecrawl_once(
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

pub(super) async fn search_parallel_once(
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
