//! Public literature-source adapters used by the built-in `search` tool.
//!
//! This module follows the free-first connector shape from `paper-search-mcp`:
//! source-specific HTTP details stay behind one normalized `LiteraturePaper`
//! record and one SerpAPI-style JSON serializer. The first batch focuses on
//! public metadata APIs that do not need paid credentials.

use chrono::{Duration as ChronoDuration, NaiveDate, Utc};
use lazy_static::lazy_static;
use regex::Regex;
use serde_json::{json, Map as JsonMap, Value as Json};

const DEFAULT_MAX_RESULTS: u32 = 10;
const MAX_RESULTS_CAP: u32 = 25;
const ARXIV_API_URL: &str = "https://export.arxiv.org/api/query";
const CROSSREF_WORKS_URL: &str = "https://api.crossref.org/works";
const OPENALEX_WORKS_URL: &str = "https://api.openalex.org/works";
const BIORXIV_API_URL: &str = "https://api.biorxiv.org/details/biorxiv";
const MEDRXIV_API_URL: &str = "https://api.medrxiv.org/details/medrxiv";
const PREPRINT_SEARCH_WINDOW_DAYS: i64 = 365;
const PREPRINT_MAX_SCAN_PAGES: u32 = 5;

mod arxiv;
mod client;
mod operations;
mod output;
mod routing;

pub use arxiv::parse_arxiv_atom;
pub use client::PublicLiteratureClient;
pub use output::{paper_to_detail_json, search_response_to_json};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublicLiteratureSource {
    Arxiv,
    Crossref,
    OpenAlex,
    Biorxiv,
    Medrxiv,
}

impl PublicLiteratureSource {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().replace('-', "_").as_str() {
            "arxiv" | "ar_xiv" => Some(Self::Arxiv),
            "crossref" | "cross_ref" => Some(Self::Crossref),
            "openalex" | "open_alex" => Some(Self::OpenAlex),
            "biorxiv" | "bio_rxiv" => Some(Self::Biorxiv),
            "medrxiv" | "med_rxiv" => Some(Self::Medrxiv),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Arxiv => "arxiv",
            Self::Crossref => "crossref",
            Self::OpenAlex => "openalex",
            Self::Biorxiv => "biorxiv",
            Self::Medrxiv => "medrxiv",
        }
    }

    fn favicon(self) -> &'static str {
        match self {
            Self::Arxiv => "https://arxiv.org/favicon.ico",
            Self::Crossref => "https://www.crossref.org/favicon.ico",
            Self::OpenAlex => "https://openalex.org/favicon.ico",
            Self::Biorxiv => "https://www.biorxiv.org/favicon.ico",
            Self::Medrxiv => "https://www.medrxiv.org/favicon.ico",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiteratureSearchArgs {
    pub query: String,
    pub max_results: Option<u32>,
}

impl LiteratureSearchArgs {
    pub fn normalized_max_results(&self) -> u32 {
        self.max_results
            .unwrap_or(DEFAULT_MAX_RESULTS)
            .clamp(1, MAX_RESULTS_CAP)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiteraturePaper {
    pub id: String,
    pub source: PublicLiteratureSource,
    pub title: String,
    pub authors: Vec<String>,
    pub abstract_text: Option<String>,
    pub url: String,
    pub pdf_url: Option<String>,
    pub doi: Option<String>,
    pub published_date: Option<String>,
    pub updated_date: Option<String>,
    pub venue: Option<String>,
    pub categories: Vec<String>,
    pub citation_count: Option<u64>,
    pub extra: JsonMap<String, Json>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiteratureSearchResponse {
    pub query: String,
    pub source: PublicLiteratureSource,
    pub total: Option<u64>,
    pub results: Vec<LiteraturePaper>,
    pub notes: Vec<String>,
}

pub fn parse_crossref_json(root: &Json) -> Vec<LiteraturePaper> {
    let items = root
        .pointer("/message/items")
        .and_then(Json::as_array)
        .cloned()
        .unwrap_or_default();
    items.iter().filter_map(parse_crossref_item).collect()
}

fn parse_crossref_item(item: &Json) -> Option<LiteraturePaper> {
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

pub fn parse_openalex_json(root: &Json) -> Vec<LiteraturePaper> {
    let items = root
        .get("results")
        .and_then(Json::as_array)
        .cloned()
        .unwrap_or_default();
    items.iter().filter_map(parse_openalex_item).collect()
}

fn parse_openalex_item(item: &Json) -> Option<LiteraturePaper> {
    let title = item
        .get("title")
        .or_else(|| item.get("display_name"))
        .and_then(Json::as_str)
        .map(clean_html_text)?;
    if title.is_empty() {
        return None;
    }
    let openalex_id = item.get("id").and_then(Json::as_str).unwrap_or_default();
    let id = openalex_id
        .trim_start_matches("https://openalex.org/")
        .to_string();
    let doi = item
        .get("doi")
        .and_then(Json::as_str)
        .map(normalize_doi)
        .filter(|s| !s.is_empty());
    let primary_location = item.get("primary_location");
    let url = primary_location
        .and_then(|v| v.get("landing_page_url"))
        .and_then(Json::as_str)
        .map(str::to_string)
        .or_else(|| doi.as_ref().map(|d| format!("https://doi.org/{d}")))
        .unwrap_or_else(|| openalex_id.to_string());
    let pdf_url = primary_location
        .and_then(|v| v.get("pdf_url"))
        .and_then(Json::as_str)
        .map(str::to_string)
        .or_else(|| {
            item.pointer("/open_access/oa_url")
                .and_then(Json::as_str)
                .map(str::to_string)
        });
    let authors = item
        .get("authorships")
        .and_then(Json::as_array)
        .map(|authors| {
            authors
                .iter()
                .filter_map(|a| a.pointer("/author/display_name").and_then(Json::as_str))
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let abstract_text = item
        .get("abstract_inverted_index")
        .and_then(reconstruct_openalex_abstract)
        .filter(|s| !s.is_empty());
    let published_date = item
        .get("publication_date")
        .and_then(Json::as_str)
        .map(str::to_string)
        .or_else(|| {
            item.get("publication_year")
                .and_then(Json::as_u64)
                .map(|year| year.to_string())
        });
    let venue = item
        .pointer("/primary_location/source/display_name")
        .and_then(Json::as_str)
        .map(str::to_string)
        .or_else(|| {
            item.get("host_venue")
                .and_then(|v| v.get("display_name"))
                .and_then(Json::as_str)
                .map(str::to_string)
        });
    let categories = item
        .get("concepts")
        .and_then(Json::as_array)
        .map(|concepts| {
            concepts
                .iter()
                .filter_map(|c| c.get("display_name").and_then(Json::as_str))
                .take(6)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let mut extra = JsonMap::new();
    if let Some(work_type) = item.get("type").and_then(Json::as_str) {
        extra.insert("type".to_string(), json!(work_type));
    }
    if let Some(is_oa) = item.pointer("/open_access/is_oa").and_then(Json::as_bool) {
        extra.insert("is_open_access".to_string(), json!(is_oa));
    }
    Some(LiteraturePaper {
        id,
        source: PublicLiteratureSource::OpenAlex,
        title,
        authors,
        abstract_text,
        url,
        pdf_url,
        doi,
        published_date,
        updated_date: item
            .get("updated_date")
            .and_then(Json::as_str)
            .map(str::to_string),
        venue,
        categories,
        citation_count: item.get("cited_by_count").and_then(Json::as_u64),
        extra,
    })
}

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

pub(super) fn first_xml_tag(block: &str, tag: &str) -> Option<String> {
    let pattern = format!(
        r#"(?is)<(?:[A-Za-z0-9_-]+:)?{}\b[^>]*>(.*?)</(?:[A-Za-z0-9_-]+:)?{}>"#,
        regex::escape(tag),
        regex::escape(tag)
    );
    Regex::new(&pattern)
        .ok()?
        .captures(block)?
        .get(1)
        .map(|m| m.as_str().to_string())
}

pub(super) fn attr_value(attrs: &str, name: &str) -> Option<String> {
    let pattern = format!(r#"(?is)\b{}\s*=\s*["']([^"']*)["']"#, regex::escape(name));
    Regex::new(&pattern)
        .ok()?
        .captures(attrs)?
        .get(1)
        .map(|m| decode_html_entities(m.as_str()).trim().to_string())
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

fn reconstruct_openalex_abstract(value: &Json) -> Option<String> {
    let map = value.as_object()?;
    let mut positions = Vec::<(usize, String)>::new();
    for (word, indexes) in map {
        let Some(indexes) = indexes.as_array() else {
            continue;
        };
        for index in indexes {
            if let Some(pos) = index.as_u64() {
                positions.push((pos as usize, word.clone()));
            }
        }
    }
    positions.sort_by_key(|(pos, _)| *pos);
    Some(
        positions
            .into_iter()
            .map(|(_, word)| word)
            .collect::<Vec<_>>()
            .join(" "),
    )
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

pub(super) fn clean_xml_text(value: &str) -> String {
    clean_html_text(value)
}

fn clean_html_text(value: &str) -> String {
    lazy_static! {
        static ref RE_TAG: Regex = Regex::new(r#"(?is)<[^>]+>"#).unwrap();
        static ref RE_WS: Regex = Regex::new(r#"\s+"#).unwrap();
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

pub(super) fn normalize_doi(value: &str) -> String {
    let trimmed = value.trim();
    let lower = trimmed.to_ascii_lowercase();
    for prefix in ["https://doi.org/", "http://doi.org/", "doi:"] {
        if lower.starts_with(prefix) {
            return trimmed[prefix.len()..].trim().to_string();
        }
    }
    trimmed.to_string()
}

fn normalize_arxiv_identifier(value: &str) -> Option<String> {
    let mut value = value.trim().trim_end_matches('/').to_string();
    if value.is_empty() {
        return None;
    }
    if let Ok(parsed) = reqwest::Url::parse(&value) {
        let host = parsed.host_str().unwrap_or_default().to_ascii_lowercase();
        if host.ends_with("arxiv.org") {
            let path = parsed.path().trim_matches('/');
            value = path
                .strip_prefix("abs/")
                .or_else(|| path.strip_prefix("pdf/"))
                .unwrap_or(path)
                .trim_end_matches(".pdf")
                .to_string();
        }
    }
    value = value.trim_end_matches(".pdf").trim().to_string();
    let lower = value.to_ascii_lowercase();
    if lower.starts_with("arxiv:") {
        value = value["arxiv:".len()..].trim().to_string();
    }
    (!value.is_empty()).then_some(value)
}

fn normalize_openalex_identifier(value: &str) -> Option<String> {
    let value = value.trim().trim_end_matches('/').to_string();
    if value.is_empty() {
        return None;
    }
    if value.to_ascii_lowercase().starts_with("doi:")
        || value.to_ascii_lowercase().starts_with("pmid:")
        || value.to_ascii_lowercase().starts_with("pmcid:")
        || value.to_ascii_uppercase().starts_with('W')
    {
        return Some(value);
    }
    if let Ok(parsed) = reqwest::Url::parse(&value) {
        let host = parsed.host_str().unwrap_or_default().to_ascii_lowercase();
        if host.ends_with("openalex.org") {
            return parsed
                .path_segments()
                .and_then(|mut segments| segments.next_back())
                .filter(|s| !s.is_empty())
                .map(str::to_string);
        }
        if host == "doi.org" || host.ends_with(".doi.org") {
            return Some(format!("doi:{}", normalize_doi(&value)));
        }
    }
    let doi = normalize_doi(&value);
    if doi.contains('/') {
        Some(format!("doi:{doi}"))
    } else {
        Some(value)
    }
}

fn encode_path_segment(value: &str) -> String {
    let mut out = String::new();
    for byte in value.as_bytes() {
        match *byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(*byte as char)
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

pub(super) fn normalize_date_string(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    if value.len() >= 10 {
        let candidate = &value[..10];
        if NaiveDate::parse_from_str(candidate, "%Y-%m-%d").is_ok() {
            return Some(candidate.to_string());
        }
    }
    Some(value.to_string())
}

pub(super) fn arxiv_id_from_url(url: &str) -> Option<String> {
    let trimmed = url.trim().trim_end_matches('/');
    trimmed
        .rsplit('/')
        .next()
        .map(str::to_string)
        .filter(|s| !s.is_empty())
}

pub(super) fn truncate_chars(value: &str, max_chars: usize) -> String {
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

fn clean_optional(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|s| !s.is_empty())
}

#[allow(dead_code)]
fn preprint_recent_start_date() -> String {
    (Utc::now().date_naive() - ChronoDuration::days(PREPRINT_SEARCH_WINDOW_DAYS))
        .format("%Y-%m-%d")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_arxiv_atom_fixture() {
        let xml = r#"
        <feed xmlns="http://www.w3.org/2005/Atom" xmlns:arxiv="http://arxiv.org/schemas/atom">
          <entry>
            <id>http://arxiv.org/abs/2401.01234v1</id>
            <updated>2024-01-02T00:00:00Z</updated>
            <published>2024-01-01T00:00:00Z</published>
            <title> Test &amp; Paper </title>
            <summary> A useful abstract. </summary>
            <author><name>Alice Smith</name></author>
            <author><name>Bob Jones</name></author>
            <link title="pdf" href="http://arxiv.org/pdf/2401.01234v1" type="application/pdf"/>
            <category term="cs.CL" />
            <arxiv:doi>10.1000/example</arxiv:doi>
          </entry>
        </feed>
        "#;
        let parsed = parse_arxiv_atom(xml);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].id, "2401.01234v1");
        assert_eq!(parsed[0].title, "Test & Paper");
        assert_eq!(parsed[0].authors, vec!["Alice Smith", "Bob Jones"]);
        assert_eq!(parsed[0].doi.as_deref(), Some("10.1000/example"));
        assert_eq!(parsed[0].categories, vec!["cs.CL"]);
    }

    #[test]
    fn parses_crossref_json_fixture() {
        let value = json!({
            "message": {
                "total-results": 1,
                "items": [{
                    "DOI": "10.1000/example",
                    "title": ["<i>Crossref</i> Paper"],
                    "author": [{"given": "Alice", "family": "Smith"}],
                    "abstract": "<jats:p>Abstract text.</jats:p>",
                    "URL": "https://doi.org/10.1000/example",
                    "published": {"date-parts": [[2023, 5, 2]]},
                    "container-title": ["Journal"],
                    "subject": ["AI"],
                    "is-referenced-by-count": 7
                }]
            }
        });
        let parsed = parse_crossref_json(&value);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].title, "Crossref Paper");
        assert_eq!(parsed[0].authors, vec!["Alice Smith"]);
        assert_eq!(parsed[0].published_date.as_deref(), Some("2023-05-02"));
        assert_eq!(parsed[0].citation_count, Some(7));
    }

    #[test]
    fn parses_openalex_json_fixture() {
        let value = json!({
            "meta": {"count": 1},
            "results": [{
                "id": "https://openalex.org/W123",
                "display_name": "OpenAlex Paper",
                "doi": "https://doi.org/10.1000/openalex",
                "abstract_inverted_index": {"hello": [0], "world": [1]},
                "authorships": [{"author": {"display_name": "Jane Doe"}}],
                "publication_date": "2022-03-04",
                "primary_location": {
                    "landing_page_url": "https://example.org/paper",
                    "pdf_url": "https://example.org/paper.pdf",
                    "source": {"display_name": "Venue"}
                },
                "concepts": [{"display_name": "Biology"}],
                "cited_by_count": 11
            }]
        });
        let parsed = parse_openalex_json(&value);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].id, "W123");
        assert_eq!(parsed[0].abstract_text.as_deref(), Some("hello world"));
        assert_eq!(parsed[0].venue.as_deref(), Some("Venue"));
    }

    #[test]
    fn parses_preprint_json_fixture() {
        let value = json!({
            "collection": [{
                "doi": "10.1101/2024.01.01.123456",
                "title": "CRISPR screen",
                "authors": "Alice Smith; Bob Jones",
                "abstract": "A CRISPR abstract",
                "date": "2024-01-01",
                "version": "2",
                "category": "genomics",
                "server": "biorxiv"
            }]
        });
        let parsed = parse_preprint_json(PublicLiteratureSource::Biorxiv, &value, "crispr");
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].authors, vec!["Alice Smith", "Bob Jones"]);
        assert_eq!(
            parsed[0].pdf_url.as_deref(),
            Some("https://www.biorxiv.org/content/10.1101/2024.01.01.123456v2.full.pdf")
        );
    }

    #[test]
    fn literature_json_uses_serpapi_shape() {
        let response = LiteratureSearchResponse {
            query: "test".to_string(),
            source: PublicLiteratureSource::Arxiv,
            total: Some(1),
            notes: vec!["note".to_string()],
            results: vec![LiteraturePaper {
                id: "2401.01234".to_string(),
                source: PublicLiteratureSource::Arxiv,
                title: "Title".to_string(),
                authors: vec!["Alice".to_string()],
                abstract_text: Some("Abstract".to_string()),
                url: "https://arxiv.org/abs/2401.01234".to_string(),
                pdf_url: None,
                doi: None,
                published_date: Some("2024-01-01".to_string()),
                updated_date: None,
                venue: None,
                categories: vec!["cs.CL".to_string()],
                citation_count: None,
                extra: JsonMap::new(),
            }],
        };
        let json = search_response_to_json(&response);
        assert_eq!(json["category"], "literature");
        assert_eq!(json["source"], "arxiv");
        assert_eq!(json["results"][0]["metadata"]["authors"][0], "Alice");
        assert_eq!(
            json["results"][0]["favicon"],
            "https://arxiv.org/favicon.ico"
        );
    }

    #[test]
    fn literature_detail_json_preserves_fetch_fields() {
        let item = LiteraturePaper {
            id: "10.1000/example".to_string(),
            source: PublicLiteratureSource::Crossref,
            title: "Fetched Paper".to_string(),
            authors: vec!["Alice".to_string(), "Bob".to_string()],
            abstract_text: Some("Detailed abstract".to_string()),
            url: "https://doi.org/10.1000/example".to_string(),
            pdf_url: Some("https://example.org/paper.pdf".to_string()),
            doi: Some("10.1000/example".to_string()),
            published_date: Some("2024-01-02".to_string()),
            updated_date: None,
            venue: Some("Journal".to_string()),
            categories: vec!["AI".to_string()],
            citation_count: Some(12),
            extra: JsonMap::new(),
        };

        let json = paper_to_detail_json(&item);
        assert_eq!(json["category"], "literature");
        assert_eq!(json["source"], "crossref");
        assert_eq!(json["metadata"]["doi"], "10.1000/example");
        assert_eq!(json["authors"][0], "Alice");
        assert!(json["content"]
            .as_str()
            .unwrap()
            .contains("Detailed abstract"));
        assert_eq!(json["favicon"], "https://www.crossref.org/favicon.ico");
    }

    #[test]
    fn normalizes_fetch_identifiers() {
        assert_eq!(
            normalize_arxiv_identifier("https://arxiv.org/pdf/2401.01234v2.pdf").as_deref(),
            Some("2401.01234v2")
        );
        assert_eq!(
            normalize_arxiv_identifier("ARXIV:2401.01234").as_deref(),
            Some("2401.01234")
        );
        assert_eq!(
            normalize_openalex_identifier("https://openalex.org/W123").as_deref(),
            Some("W123")
        );
        assert_eq!(
            normalize_openalex_identifier("https://doi.org/10.1000/example").as_deref(),
            Some("doi:10.1000/example")
        );
        assert_eq!(normalize_doi("DOI:10.1000/example"), "10.1000/example");
    }
}
