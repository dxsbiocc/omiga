//! Crossref Works response parser.

use super::{clean_html_text, normalize_doi, LiteraturePaper, PublicLiteratureSource};
use serde_json::{json, Map as JsonMap, Value as Json};

pub fn parse_crossref_json(root: &Json) -> Vec<LiteraturePaper> {
    let items = root
        .pointer("/message/items")
        .and_then(Json::as_array)
        .cloned()
        .unwrap_or_default();
    items.iter().filter_map(parse_crossref_item).collect()
}

pub(in crate::domain::search::literature) fn parse_crossref_item(
    item: &Json,
) -> Option<LiteraturePaper> {
    let title = first_json_string_array(item.get("title"))
        .or_else(|| item.get("title").and_then(Json::as_str).map(str::to_string))?;
    let title = clean_html_text(&title);
    if title.is_empty() {
        return None;
    }
    let doi = item
        .get("DOI")
        .and_then(Json::as_str)
        .map(normalize_doi)
        .filter(|s| !s.is_empty());
    let id = doi.clone().unwrap_or_else(|| {
        item.get("URL")
            .and_then(Json::as_str)
            .unwrap_or("")
            .to_string()
    });
    let url = item
        .get("URL")
        .and_then(Json::as_str)
        .map(str::to_string)
        .or_else(|| doi.as_ref().map(|d| format!("https://doi.org/{d}")))
        .unwrap_or_default();
    let authors = item
        .get("author")
        .and_then(Json::as_array)
        .map(|authors| {
            authors
                .iter()
                .filter_map(crossref_author_name)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let abstract_text = item
        .get("abstract")
        .and_then(Json::as_str)
        .map(clean_html_text)
        .filter(|s| !s.is_empty());
    let published_date = crossref_date(item, "published")
        .or_else(|| crossref_date(item, "issued"))
        .or_else(|| crossref_date(item, "created"));
    let venue = first_json_string_array(item.get("container-title"));
    let pdf_url = item.get("link").and_then(Json::as_array).and_then(|links| {
        links.iter().find_map(|link| {
            let content_type = link
                .get("content-type")
                .and_then(Json::as_str)
                .unwrap_or_default()
                .to_ascii_lowercase();
            if content_type.contains("pdf") {
                link.get("URL").and_then(Json::as_str).map(str::to_string)
            } else {
                None
            }
        })
    });
    let categories = item
        .get("subject")
        .and_then(Json::as_array)
        .map(|items| json_string_vec(items))
        .unwrap_or_else(|| {
            item.get("type")
                .and_then(Json::as_str)
                .map(|s| vec![s.to_string()])
                .unwrap_or_default()
        });
    let citation_count = item.get("is-referenced-by-count").and_then(Json::as_u64);
    let mut extra = JsonMap::new();
    for key in ["publisher", "type", "volume", "issue", "page"] {
        if let Some(value) = item.get(key).and_then(Json::as_str) {
            if !value.is_empty() {
                extra.insert(key.to_string(), json!(value));
            }
        }
    }
    Some(LiteraturePaper {
        id,
        source: PublicLiteratureSource::Crossref,
        title,
        authors,
        abstract_text,
        url,
        pdf_url,
        doi,
        published_date,
        updated_date: None,
        venue,
        categories,
        citation_count,
        extra,
    })
}

fn first_json_string_array(value: Option<&Json>) -> Option<String> {
    value
        .and_then(Json::as_array)
        .and_then(|items| items.iter().find_map(Json::as_str))
        .map(str::to_string)
}

fn json_string_vec(items: &[Json]) -> Vec<String> {
    items
        .iter()
        .filter_map(Json::as_str)
        .map(str::to_string)
        .collect()
}

fn crossref_author_name(author: &Json) -> Option<String> {
    let given = author
        .get("given")
        .and_then(Json::as_str)
        .unwrap_or("")
        .trim();
    let family = author
        .get("family")
        .and_then(Json::as_str)
        .unwrap_or("")
        .trim();
    let name = match (given.is_empty(), family.is_empty()) {
        (false, false) => format!("{given} {family}"),
        (true, false) => family.to_string(),
        (false, true) => given.to_string(),
        (true, true) => author
            .get("name")
            .and_then(Json::as_str)
            .unwrap_or("")
            .to_string(),
    };
    if name.trim().is_empty() {
        None
    } else {
        Some(name)
    }
}

fn crossref_date(item: &Json, field: &str) -> Option<String> {
    let parts = item
        .get(field)?
        .get("date-parts")?
        .as_array()?
        .first()?
        .as_array()?;
    let year = parts.first()?.as_i64()?;
    let month = parts
        .get(1)
        .and_then(Json::as_i64)
        .unwrap_or(1)
        .clamp(1, 12);
    let day = parts
        .get(2)
        .and_then(Json::as_i64)
        .unwrap_or(1)
        .clamp(1, 31);
    Some(format!("{year:04}-{month:02}-{day:02}"))
}
