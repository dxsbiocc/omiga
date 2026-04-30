use super::super::common::SearchHit;
use lazy_static::lazy_static;
use regex::Regex;
use std::collections::HashSet;

const MAX_SNIPPET_CHARS: usize = 512;
const MAX_TITLE_CHARS: usize = 220;

pub(super) fn host_of_url(url: &str) -> Option<String> {
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

pub(super) fn filter_hits(
    project_root: &std::path::Path,
    hits: Vec<SearchHit>,
    allowed: &Option<Vec<String>>,
    blocked: &Option<Vec<String>>,
    limit: usize,
) -> Vec<SearchHit> {
    hits.into_iter()
        .filter(|h| url_passes_filters(&h.url, allowed, blocked))
        .filter(|h| crate::domain::tools::web_safety::is_safe_result_url(project_root, &h.url))
        .take(limit)
        .collect()
}

fn normalize_url_for_dedup(url: &str) -> String {
    url.trim().trim_end_matches('/').to_lowercase()
}

/// Strip inline base64 image payloads (Hermes `clean_base64_images`) and cap length.
pub(super) fn sanitize_search_text(s: &str, max_chars: usize) -> String {
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

pub(super) fn sanitize_hit(h: SearchHit) -> SearchHit {
    SearchHit {
        title: sanitize_search_text(&h.title, MAX_TITLE_CHARS),
        url: h.url.trim().to_string(),
        snippet: sanitize_search_text(&h.snippet, MAX_SNIPPET_CHARS),
    }
}

pub(super) fn dedupe_hits_preserve_order(hits: Vec<SearchHit>) -> Vec<SearchHit> {
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
