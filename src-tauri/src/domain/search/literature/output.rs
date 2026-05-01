//! SerpAPI-shaped literature search and detail JSON output.

use super::{truncate_chars, LiteraturePaper, LiteratureSearchResponse};
use serde_json::{json, Value as Json};

pub fn search_response_to_json(response: &LiteratureSearchResponse) -> Json {
    let results: Vec<Json> = response
        .results
        .iter()
        .enumerate()
        .map(|(idx, item)| paper_to_serp_result(item, idx + 1))
        .collect();
    json!({
        "query": response.query,
        "category": "literature",
        "source": response.source.as_str(),
        "effective_source": response.source.as_str(),
        "count": results.len(),
        "total": response.total,
        "route_notes": response.notes,
        "results": results,
    })
}

fn paper_to_serp_result(item: &LiteraturePaper, position: usize) -> Json {
    json!({
        "position": position,
        "category": "literature",
        "source": item.source.as_str(),
        "title": item.title,
        "name": item.title,
        "link": item.url,
        "url": item.url,
        "displayed_link": displayed_link_for_url(&item.url),
        "favicon": item.source.favicon(),
        "snippet": paper_snippet(item),
        "id": item.id,
        "metadata": {
            "authors": item.authors,
            "author_names": item.authors,
            "article_title": item.title,
            "paper_title": item.title,
            "abstract": item.abstract_text,
            "doi": item.doi,
            "published_date": item.published_date,
            "updated_date": item.updated_date,
            "venue": item.venue,
            "pdf_url": item.pdf_url,
            "categories": item.categories,
            "citation_count": item.citation_count,
            "source_specific": item.extra,
        }
    })
}

pub fn paper_to_detail_json(item: &LiteraturePaper) -> Json {
    json!({
        "category": "literature",
        "source": item.source.as_str(),
        "effective_source": item.source.as_str(),
        "title": item.title,
        "name": item.title,
        "article_title": item.title,
        "paper_title": item.title,
        "link": item.url,
        "url": item.url,
        "displayed_link": displayed_link_for_url(&item.url),
        "favicon": item.source.favicon(),
        "id": item.id,
        "authors": item.authors,
        "content": paper_detail_content(item),
        "metadata": {
            "authors": item.authors,
            "author_names": item.authors,
            "article_title": item.title,
            "paper_title": item.title,
            "abstract": item.abstract_text,
            "doi": item.doi,
            "published_date": item.published_date,
            "updated_date": item.updated_date,
            "venue": item.venue,
            "pdf_url": item.pdf_url,
            "categories": item.categories,
            "citation_count": item.citation_count,
            "source_specific": item.extra,
        }
    })
}

fn paper_snippet(item: &LiteraturePaper) -> String {
    let mut pieces = Vec::new();
    if !item.authors.is_empty() {
        let mut authors = item
            .authors
            .iter()
            .take(3)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        if item.authors.len() > 3 {
            authors.push_str(", et al.");
        }
        pieces.push(authors);
    }
    if let Some(date) = &item.published_date {
        pieces.push(date.clone());
    }
    if let Some(venue) = &item.venue {
        if !venue.is_empty() {
            pieces.push(venue.clone());
        }
    }
    if let Some(abstract_text) = &item.abstract_text {
        pieces.push(truncate_chars(abstract_text, 360));
    }
    pieces.join(" — ")
}

fn paper_detail_content(item: &LiteraturePaper) -> String {
    let mut out = String::new();
    out.push_str(&item.title);
    out.push_str("\n\n");
    if !item.authors.is_empty() {
        out.push_str("Authors: ");
        out.push_str(&item.authors.join(", "));
        out.push('\n');
    }
    if let Some(venue) = item.venue.as_deref().filter(|s| !s.trim().is_empty()) {
        out.push_str("Venue: ");
        out.push_str(venue);
        out.push('\n');
    }
    if let Some(date) = item
        .published_date
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        out.push_str("Published: ");
        out.push_str(date);
        out.push('\n');
    }
    if let Some(doi) = item.doi.as_deref().filter(|s| !s.trim().is_empty()) {
        out.push_str("DOI: ");
        out.push_str(doi);
        out.push('\n');
    }
    if !item.categories.is_empty() {
        out.push_str("Categories: ");
        out.push_str(&item.categories.join(", "));
        out.push('\n');
    }
    if let Some(pdf) = item.pdf_url.as_deref().filter(|s| !s.trim().is_empty()) {
        out.push_str("PDF: ");
        out.push_str(pdf);
        out.push('\n');
    }
    if let Some(abstract_text) = item
        .abstract_text
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        out.push_str("\nAbstract\n");
        out.push_str(abstract_text);
    }
    out.trim().to_string()
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
