//! bioRxiv/medRxiv JSON response parser.

use super::{
    clean_html_text, normalize_doi, LiteraturePaper, PublicLiteratureSource,
    PREPRINT_SEARCH_WINDOW_DAYS,
};
use chrono::{Duration as ChronoDuration, Utc};
use serde_json::{json, Map as JsonMap, Value as Json};

pub fn parse_preprint_json(
    source: PublicLiteratureSource,
    root: &Json,
    query: &str,
) -> Vec<LiteraturePaper> {
    let Some(items) = root.get("collection").and_then(Json::as_array) else {
        return Vec::new();
    };
    let query_terms = normalized_query_terms(query);
    items
        .iter()
        .filter(|item| preprint_matches_query(item, &query_terms))
        .filter_map(|item| {
            let title = item
                .get("title")
                .and_then(Json::as_str)
                .map(clean_html_text)?;
            if title.is_empty() {
                return None;
            }
            let doi = item
                .get("doi")
                .and_then(Json::as_str)
                .map(normalize_doi)
                .filter(|s| !s.is_empty());
            let version = item
                .get("version")
                .and_then(Json::as_str)
                .unwrap_or("1")
                .trim_start_matches('v');
            let id = doi.clone().unwrap_or_else(|| title.clone());
            let host = match source {
                PublicLiteratureSource::Biorxiv => "www.biorxiv.org",
                PublicLiteratureSource::Medrxiv => "www.medrxiv.org",
                _ => "www.biorxiv.org",
            };
            let url = doi
                .as_ref()
                .map(|d| format!("https://{host}/content/{d}v{version}"))
                .unwrap_or_default();
            let pdf_url = doi
                .as_ref()
                .map(|d| format!("https://{host}/content/{d}v{version}.full.pdf"));
            let authors = item
                .get("authors")
                .and_then(Json::as_str)
                .map(|s| {
                    s.split(';')
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_string)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let abstract_text = item
                .get("abstract")
                .and_then(Json::as_str)
                .map(clean_html_text)
                .filter(|s| !s.is_empty());
            let published_date = item.get("date").and_then(Json::as_str).map(str::to_string);
            let category = item
                .get("category")
                .and_then(Json::as_str)
                .unwrap_or_default();
            let mut extra = JsonMap::new();
            for key in ["version", "type", "license", "published", "server"] {
                if let Some(value) = item.get(key).and_then(Json::as_str) {
                    if !value.is_empty() {
                        extra.insert(key.to_string(), json!(value));
                    }
                }
            }
            Some(LiteraturePaper {
                id,
                source,
                title,
                authors,
                abstract_text,
                url,
                pdf_url,
                doi,
                published_date,
                updated_date: None,
                venue: Some(source.as_str().to_string()),
                categories: if category.is_empty() {
                    Vec::new()
                } else {
                    vec![category.to_string()]
                },
                citation_count: None,
                extra,
            })
        })
        .collect()
}

fn preprint_matches_query(item: &Json, terms: &[String]) -> bool {
    if terms.is_empty() {
        return true;
    }
    let haystack = ["title", "abstract", "category", "authors", "doi"]
        .iter()
        .filter_map(|key| item.get(*key).and_then(Json::as_str))
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase();
    terms.iter().all(|term| haystack.contains(term))
}

fn normalized_query_terms(query: &str) -> Vec<String> {
    query
        .split(|ch: char| !ch.is_alphanumeric())
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| s.len() >= 2)
        .take(6)
        .collect()
}

#[allow(dead_code)]
fn preprint_recent_start_date() -> String {
    (Utc::now().date_naive() - ChronoDuration::days(PREPRINT_SEARCH_WINDOW_DAYS))
        .format("%Y-%m-%d")
        .to_string()
}
