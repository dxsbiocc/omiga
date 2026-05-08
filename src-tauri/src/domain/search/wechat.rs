//! Optional WeChat public-account search via Sogou Weixin search.
//!
//! This adapter is intentionally opt-in. The endpoint is a public HTML page,
//! can change without notice, and may rate-limit or require CAPTCHA.

use crate::domain::tools::ToolContext;
use chrono::{LocalResult, TimeZone, Utc};
use lazy_static::lazy_static;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as Json};
use std::time::Duration;

const DEFAULT_BASE_URL: &str = "https://weixin.sogou.com/weixin";
const DEFAULT_MAX_RESULTS: u32 = 5;
const MAX_RESULTS_CAP: u32 = 10;
const WECHAT_FAVICON: &str = "https://weixin.sogou.com/favicon.ico";
const WECHAT_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct WechatSearchArgs {
    #[serde(alias = "q")]
    pub query: String,
    #[serde(default, alias = "maxResults", alias = "limit")]
    pub max_results: Option<u32>,
    #[serde(default)]
    pub page: Option<u32>,
}

impl WechatSearchArgs {
    pub fn normalized_max_results(&self) -> u32 {
        self.max_results
            .unwrap_or(DEFAULT_MAX_RESULTS)
            .clamp(1, MAX_RESULTS_CAP)
    }

    fn normalized_page(&self) -> u32 {
        self.page.unwrap_or(1).clamp(1, 10)
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct WechatArticle {
    pub title: String,
    pub url: String,
    pub snippet: String,
    pub account_name: Option<String>,
    pub published_at: Option<String>,
    pub page: u32,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct WechatSearchResponse {
    pub query: String,
    pub page: u32,
    pub results: Vec<WechatArticle>,
}

#[derive(Clone)]
pub struct WechatClient {
    http: reqwest::Client,
    base_url: String,
}

impl WechatClient {
    pub fn from_tool_context(ctx: &ToolContext) -> Result<Self, String> {
        if !ctx.web_search_api_keys.wechat_search_enabled {
            return Err(
                "WeChat public-account search is disabled. Enable it in Settings → Search."
                    .to_string(),
            );
        }

        let mut builder = reqwest::Client::builder()
            .timeout(Duration::from_secs(ctx.timeout_secs.clamp(5, 30)))
            .user_agent(WECHAT_USER_AGENT);
        if !ctx.web_use_proxy {
            builder = builder.no_proxy();
        }
        let http = builder
            .build()
            .map_err(|e| format!("build WeChat search HTTP client: {e}"))?;
        Ok(Self {
            http,
            base_url: DEFAULT_BASE_URL.to_string(),
        })
    }

    #[cfg(test)]
    pub fn with_base_url(base_url: impl Into<String>) -> Result<Self, String> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent(WECHAT_USER_AGENT)
            .build()
            .map_err(|e| format!("build WeChat search HTTP client: {e}"))?;
        Ok(Self {
            http,
            base_url: base_url.into(),
        })
    }

    pub async fn search(&self, args: WechatSearchArgs) -> Result<WechatSearchResponse, String> {
        if args.query.trim().is_empty() {
            return Err("WeChat search query must not be empty".to_string());
        }

        let page = args.normalized_page();
        let response = self
            .http
            .get(&self.base_url)
            .header(reqwest::header::REFERER, "https://weixin.sogou.com/")
            .query(&[
                ("type", "2".to_string()),
                ("query", args.query.trim().to_string()),
                ("ie", "utf8".to_string()),
                ("page", page.to_string()),
            ])
            .send()
            .await
            .map_err(|e| format!("WeChat search request failed: {e}"))?;
        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| format!("read WeChat search response: {e}"))?;
        if !status.is_success() {
            return Err(format!(
                "WeChat search returned HTTP {}: {}",
                status.as_u16(),
                truncate_chars(&body, 300)
            ));
        }
        if looks_like_antispider_page(&body) {
            return Err("WeChat/Sogou search returned an anti-bot verification page; try again later or disable this source.".to_string());
        }

        let mut results = parse_wechat_search_html(&body, page);
        results.truncate(args.normalized_max_results() as usize);
        Ok(WechatSearchResponse {
            query: args.query.trim().to_string(),
            page,
            results,
        })
    }
}

pub fn search_response_to_json(response: &WechatSearchResponse) -> Json {
    let results: Vec<Json> = response
        .results
        .iter()
        .enumerate()
        .map(|(idx, item)| article_to_serp_result(item, idx + 1))
        .collect();
    json!({
        "query": response.query,
        "category": "social",
        "source": "wechat",
        "effective_source": "wechat",
        "page": response.page,
        "count": results.len(),
        "results": results,
    })
}

fn article_to_serp_result(item: &WechatArticle, position: usize) -> Json {
    json!({
        "position": position,
        "category": "social",
        "source": "wechat",
        "title": item.title,
        "name": item.title,
        "link": item.url,
        "url": item.url,
        "displayed_link": displayed_link_for_url(&item.url),
        "favicon": WECHAT_FAVICON,
        "snippet": item.snippet,
        "id": Json::Null,
        "metadata": {
            "platform": "wechat",
            "account_name": item.account_name,
            "published_at": item.published_at,
            "page": item.page,
        }
    })
}

fn parse_wechat_search_html(html: &str, page: u32) -> Vec<WechatArticle> {
    lazy_static! {
        static ref RE_LI: Regex = Regex::new(r#"(?is)<li\b[^>]*>(.*?)</li>"#).expect("regex");
        static ref RE_TITLE_A: Regex =
            Regex::new(r#"(?is)<h3[^>]*>.*?<a\b[^>]*href=["']([^"']+)["'][^>]*>(.*?)</a>"#)
                .expect("regex");
        static ref RE_ANY_A: Regex =
            Regex::new(r#"(?is)<a\b[^>]*href=["']([^"']+)["'][^>]*>(.*?)</a>"#).expect("regex");
        static ref RE_SNIPPET: Regex =
            Regex::new(r#"(?is)<p\b[^>]*class=["'][^"']*txt-info[^"']*["'][^>]*>(.*?)</p>"#)
                .expect("regex");
        static ref RE_ACCOUNT: Regex = Regex::new(
            r#"(?is)<a\b[^>]*class=["'][^"']*(?:account|wx-name|name)[^"']*["'][^>]*>(.*?)</a>"#
        )
        .expect("regex");
        static ref RE_TIMESTAMP: Regex =
            Regex::new(r#"(?is)\bt=["'](\d{10,13})["']"#).expect("regex");
    }

    let mut out = Vec::new();
    for cap in RE_LI.captures_iter(html) {
        let block = cap.get(1).map(|m| m.as_str()).unwrap_or_default();
        let Some((url, title_html)) = RE_TITLE_A
            .captures(block)
            .or_else(|| RE_ANY_A.captures(block))
            .and_then(|c| Some((c.get(1)?.as_str(), c.get(2)?.as_str())))
        else {
            continue;
        };
        let title = clean_html_text(title_html);
        if title.is_empty() {
            continue;
        }
        let url = normalize_wechat_url(url);
        if url.is_empty() {
            continue;
        }
        let snippet = RE_SNIPPET
            .captures(block)
            .and_then(|c| c.get(1))
            .map(|m| clean_html_text(m.as_str()))
            .unwrap_or_default();
        let account_name = RE_ACCOUNT
            .captures(block)
            .and_then(|c| c.get(1))
            .map(|m| clean_html_text(m.as_str()))
            .filter(|s| !s.is_empty());
        let published_at = RE_TIMESTAMP
            .captures(block)
            .and_then(|c| c.get(1))
            .and_then(|m| timestamp_to_rfc3339(m.as_str()));

        out.push(WechatArticle {
            title,
            url,
            snippet,
            account_name,
            published_at,
            page,
        });
    }
    out
}

fn normalize_wechat_url(value: &str) -> String {
    let decoded = decode_html_entities(value).trim().to_string();
    if decoded.starts_with("http://") || decoded.starts_with("https://") {
        decoded
    } else if decoded.starts_with("//") {
        format!("https:{decoded}")
    } else if decoded.starts_with('/') {
        format!("https://weixin.sogou.com{decoded}")
    } else {
        String::new()
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

fn clean_html_text(value: &str) -> String {
    lazy_static! {
        static ref RE_TAG: Regex = Regex::new(r#"(?is)<[^>]+>"#).expect("regex");
        static ref RE_WS: Regex = Regex::new(r#"\s+"#).expect("regex");
    }
    let without_tags = RE_TAG.replace_all(value, "");
    let decoded = decode_html_entities(without_tags.as_ref());
    RE_WS.replace_all(decoded.trim(), " ").to_string()
}

fn decode_html_entities(value: &str) -> String {
    value
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#34;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&nbsp;", " ")
}

fn timestamp_to_rfc3339(value: &str) -> Option<String> {
    let mut n = value.parse::<i64>().ok()?;
    if value.len() == 13 {
        n /= 1000;
    }
    match Utc.timestamp_opt(n, 0) {
        LocalResult::Single(dt) => Some(dt.to_rfc3339()),
        _ => None,
    }
}

fn looks_like_antispider_page(html: &str) -> bool {
    let lower = html.to_ascii_lowercase();
    lower.contains("antispider")
        || html.contains("请输入验证码")
        || html.contains("访问过于频繁")
        || html.contains("验证码")
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for (idx, ch) in value.chars().enumerate() {
        if idx >= max_chars {
            out.push('…');
            return out;
        }
        out.push(ch);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_wechat_search_html_fixture() {
        let html = r#"
        <ul>
          <li>
            <div class="txt-box">
              <h3><a href="/link?url=abc&amp;type=1">AI &amp; 医疗<em>观察</em></a></h3>
              <p class="txt-info">这是一段摘要&nbsp;内容</p>
              <div class="s-p" t="1700000000"><a class="account">示例公众号</a></div>
            </div>
          </li>
        </ul>
        "#;
        let parsed = parse_wechat_search_html(html, 2);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].title, "AI & 医疗观察");
        assert_eq!(
            parsed[0].url,
            "https://weixin.sogou.com/link?url=abc&type=1"
        );
        assert_eq!(parsed[0].snippet, "这是一段摘要 内容");
        assert_eq!(parsed[0].account_name.as_deref(), Some("示例公众号"));
        assert_eq!(parsed[0].page, 2);
        assert!(parsed[0].published_at.is_some());
    }

    #[test]
    fn wechat_json_uses_serpapi_shape() {
        let response = WechatSearchResponse {
            query: "人工智能".to_string(),
            page: 1,
            results: vec![WechatArticle {
                title: "标题".to_string(),
                url: "https://weixin.sogou.com/link?url=abc".to_string(),
                snippet: "摘要".to_string(),
                account_name: Some("公众号".to_string()),
                published_at: None,
                page: 1,
            }],
        };
        let json = search_response_to_json(&response);
        assert_eq!(json["category"], "social");
        assert_eq!(json["source"], "wechat");
        assert_eq!(json["results"][0]["metadata"]["account_name"], "公众号");
    }
}
