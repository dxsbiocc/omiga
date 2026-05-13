use super::super::{ToolContext, ToolError};
use super::common::*;
use serde_json::{json, Value as JsonValue};
use std::future::Future;
use std::time::{Duration, Instant};

mod html_sources;
mod providers;
mod web_common;

const SEARCH_METHOD_RETRY_DELAY_MS: u64 = 400;
const SEARCH_METHOD_MAX_ATTEMPTS: usize = 3;
const SEARCH_MAX_TIMEOUT_SECS: u64 = 30;

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
            providers::search_tavily_once(
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
            providers::search_exa_once(client, &key, request.query, max_u32).await
        }
        SearchMethod::Firecrawl => {
            let key = resolve_firecrawl_api_key(ctx).ok_or_else(|| ToolError::ExecutionFailed {
                message: "Firecrawl API key is not configured".to_string(),
            })?;
            let base = resolve_firecrawl_base_url(ctx);
            providers::search_firecrawl_once(client, &key, &base, request.query, max_u32).await
        }
        SearchMethod::Parallel => {
            let key = resolve_parallel_api_key(ctx).ok_or_else(|| ToolError::ExecutionFailed {
                message: "Parallel API key is not configured".to_string(),
            })?;
            providers::search_parallel_once(client, &key, request.query, max_u32).await
        }
        SearchMethod::Ddg => {
            html_sources::search_ddg(
                client,
                request.query,
                request.max_results,
                request.search_url,
            )
            .await
        }
        SearchMethod::Bing => {
            html_sources::search_bing(client, request.query, request.max_results).await
        }
        SearchMethod::Google => {
            html_sources::search_google(client, request.query, request.max_results).await
        }
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
                    let filtered = web_common::filter_hits(
                        &ctx.project_root,
                        hits,
                        allowed,
                        blocked,
                        max_results,
                    );
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
    let host = web_common::host_of_url(url)?;
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

pub(super) async fn search_web_json(
    ctx: &ToolContext,
    args: &SearchArgs,
    source: &str,
    max_n: usize,
    start: Instant,
) -> Result<JsonValue, ToolError> {
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
        crate::domain::tools::web_safety::validate_public_http_url(
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
                source,
            );
            tokio::select! {
                _ = ctx.cancel.cancelled() => Err(ToolError::Cancelled),
                r = strategy_fut => r,
            }
        }) => r?,
    };

    let processed = web_common::dedupe_hits_preserve_order(
        execution
            .hits
            .iter()
            .cloned()
            .map(web_common::sanitize_hit)
            .collect::<Vec<_>>(),
    );
    let hits = web_common::filter_hits(&ctx.project_root, processed, allowed, blocked, max_n);
    let duration = start.elapsed().as_secs_f32();
    Ok(web_results_json(args, source, execution, hits, duration))
}

#[cfg(test)]
mod ddg_tests {
    use super::html_sources::{
        normalize_result_url, parse_bing_html_results, parse_ddg_html_results_openharness,
        parse_ddg_instant_answer_json, parse_google_html_results, search_ddg,
    };
    use super::web_common::{dedupe_hits_preserve_order, filter_hits, sanitize_search_text};
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
