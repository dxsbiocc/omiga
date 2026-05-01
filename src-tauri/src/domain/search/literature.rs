//! Public literature-source adapters used by the built-in `search` tool.
//!
//! This module follows the free-first connector shape from `paper-search-mcp`:
//! source-specific HTTP details stay behind one normalized `LiteraturePaper`
//! record and one SerpAPI-style JSON serializer. The first batch focuses on
//! public metadata APIs that do not need paid credentials.

use crate::domain::tools::ToolContext;
use chrono::{Duration as ChronoDuration, NaiveDate, Utc};
use lazy_static::lazy_static;
use regex::Regex;
use serde_json::{json, Map as JsonMap, Value as Json};
use std::time::Duration;

const DEFAULT_MAX_RESULTS: u32 = 10;
const MAX_RESULTS_CAP: u32 = 25;
const ARXIV_API_URL: &str = "https://export.arxiv.org/api/query";
const CROSSREF_WORKS_URL: &str = "https://api.crossref.org/works";
const OPENALEX_WORKS_URL: &str = "https://api.openalex.org/works";
const BIORXIV_API_URL: &str = "https://api.biorxiv.org/details/biorxiv";
const MEDRXIV_API_URL: &str = "https://api.medrxiv.org/details/medrxiv";
const PREPRINT_SEARCH_WINDOW_DAYS: i64 = 365;
const PREPRINT_MAX_SCAN_PAGES: u32 = 5;

mod output;

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

#[derive(Clone)]
pub struct PublicLiteratureClient {
    http: reqwest::Client,
    mailto: String,
}

impl PublicLiteratureClient {
    pub fn from_tool_context(ctx: &ToolContext) -> Result<Self, String> {
        let mut builder = reqwest::Client::builder()
            .timeout(Duration::from_secs(ctx.timeout_secs.clamp(5, 45)))
            .user_agent(format!(
                "Omiga-LiteratureSearch/{} (mailto:{})",
                env!("CARGO_PKG_VERSION"),
                clean_optional(ctx.web_search_api_keys.pubmed_email.as_deref())
                    .unwrap_or("omiga@example.invalid")
            ));
        if !ctx.web_use_proxy {
            builder = builder.no_proxy();
        }
        let http = builder
            .build()
            .map_err(|e| format!("build literature HTTP client: {e}"))?;
        Ok(Self {
            http,
            mailto: clean_optional(ctx.web_search_api_keys.pubmed_email.as_deref())
                .unwrap_or("omiga@example.invalid")
                .to_string(),
        })
    }

    #[cfg(test)]
    pub fn for_tests() -> Self {
        Self {
            http: reqwest::Client::new(),
            mailto: "omiga@example.invalid".to_string(),
        }
    }

    pub async fn search(
        &self,
        source: PublicLiteratureSource,
        args: LiteratureSearchArgs,
    ) -> Result<LiteratureSearchResponse, String> {
        if args.query.trim().len() < 2 {
            return Err("literature search query must contain at least 2 characters".to_string());
        }
        match source {
            PublicLiteratureSource::Arxiv => self.search_arxiv(args).await,
            PublicLiteratureSource::Crossref => self.search_crossref(args).await,
            PublicLiteratureSource::OpenAlex => self.search_openalex(args).await,
            PublicLiteratureSource::Biorxiv => self.search_preprint(source, args).await,
            PublicLiteratureSource::Medrxiv => self.search_preprint(source, args).await,
        }
    }

    pub async fn fetch(
        &self,
        source: PublicLiteratureSource,
        identifier: &str,
    ) -> Result<LiteraturePaper, String> {
        let identifier = identifier.trim();
        if identifier.is_empty() {
            return Err(format!("{} fetch requires a non-empty id", source.as_str()));
        }
        match source {
            PublicLiteratureSource::Arxiv => self.fetch_arxiv(identifier).await,
            PublicLiteratureSource::Crossref => self.fetch_crossref(identifier).await,
            PublicLiteratureSource::OpenAlex => self.fetch_openalex(identifier).await,
            PublicLiteratureSource::Biorxiv | PublicLiteratureSource::Medrxiv => {
                self.fetch_preprint(source, identifier).await
            }
        }
    }

    async fn search_arxiv(
        &self,
        args: LiteratureSearchArgs,
    ) -> Result<LiteratureSearchResponse, String> {
        let max_results = args.normalized_max_results();
        let response = self
            .http
            .get(ARXIV_API_URL)
            .query(&[
                ("search_query", format!("all:{}", args.query.trim())),
                ("start", "0".to_string()),
                ("max_results", max_results.to_string()),
                ("sortBy", "relevance".to_string()),
                ("sortOrder", "descending".to_string()),
            ])
            .send()
            .await
            .map_err(|e| format!("arXiv search request failed: {e}"))?;
        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| format!("read arXiv response: {e}"))?;
        if !status.is_success() {
            return Err(format!(
                "arXiv search returned HTTP {}: {}",
                status.as_u16(),
                truncate_chars(&body, 240)
            ));
        }
        Ok(LiteratureSearchResponse {
            query: args.query.trim().to_string(),
            source: PublicLiteratureSource::Arxiv,
            total: None,
            results: parse_arxiv_atom(&body)
                .into_iter()
                .take(max_results as usize)
                .collect(),
            notes: vec!["arXiv official Atom API".to_string()],
        })
    }

    async fn fetch_arxiv(&self, identifier: &str) -> Result<LiteraturePaper, String> {
        let arxiv_id = normalize_arxiv_identifier(identifier)
            .ok_or_else(|| "arXiv fetch requires an arXiv id or arxiv.org URL".to_string())?;
        let response = self
            .http
            .get(ARXIV_API_URL)
            .query(&[("id_list", arxiv_id.clone())])
            .send()
            .await
            .map_err(|e| format!("arXiv fetch request failed: {e}"))?;
        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| format!("read arXiv fetch response: {e}"))?;
        if !status.is_success() {
            return Err(format!(
                "arXiv fetch returned HTTP {}: {}",
                status.as_u16(),
                truncate_chars(&body, 240)
            ));
        }
        parse_arxiv_atom(&body)
            .into_iter()
            .next()
            .ok_or_else(|| format!("arXiv did not return a record for `{arxiv_id}`"))
    }

    async fn search_crossref(
        &self,
        args: LiteratureSearchArgs,
    ) -> Result<LiteratureSearchResponse, String> {
        let max_results = args.normalized_max_results();
        let response = self
            .http
            .get(CROSSREF_WORKS_URL)
            .query(&[
                ("query", args.query.trim().to_string()),
                ("rows", max_results.to_string()),
                ("sort", "relevance".to_string()),
                ("order", "desc".to_string()),
                ("mailto", self.mailto.clone()),
            ])
            .send()
            .await
            .map_err(|e| format!("Crossref search request failed: {e}"))?;
        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| format!("read Crossref response: {e}"))?;
        if !status.is_success() {
            return Err(format!(
                "Crossref search returned HTTP {}: {}",
                status.as_u16(),
                truncate_chars(&body, 240)
            ));
        }
        let json: Json =
            serde_json::from_str(&body).map_err(|e| format!("parse Crossref JSON: {e}"))?;
        let total = json
            .pointer("/message/total-results")
            .and_then(Json::as_u64);
        Ok(LiteratureSearchResponse {
            query: args.query.trim().to_string(),
            source: PublicLiteratureSource::Crossref,
            total,
            results: parse_crossref_json(&json)
                .into_iter()
                .take(max_results as usize)
                .collect(),
            notes: vec!["Crossref REST API metadata search".to_string()],
        })
    }

    async fn fetch_crossref(&self, identifier: &str) -> Result<LiteraturePaper, String> {
        let doi = normalize_doi(identifier);
        if doi.is_empty() {
            return Err("Crossref fetch requires a DOI or DOI URL".to_string());
        }
        let url = format!("{CROSSREF_WORKS_URL}/{}", encode_path_segment(&doi));
        let response = self
            .http
            .get(url)
            .query(&[("mailto", self.mailto.clone())])
            .send()
            .await
            .map_err(|e| format!("Crossref fetch request failed: {e}"))?;
        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| format!("read Crossref fetch response: {e}"))?;
        if !status.is_success() {
            return Err(format!(
                "Crossref fetch returned HTTP {}: {}",
                status.as_u16(),
                truncate_chars(&body, 240)
            ));
        }
        let json: Json =
            serde_json::from_str(&body).map_err(|e| format!("parse Crossref JSON: {e}"))?;
        parse_crossref_item(json.get("message").unwrap_or(&json))
            .ok_or_else(|| format!("Crossref did not return a parseable work for `{doi}`"))
    }

    async fn search_openalex(
        &self,
        args: LiteratureSearchArgs,
    ) -> Result<LiteratureSearchResponse, String> {
        let max_results = args.normalized_max_results();
        let response = self
            .http
            .get(OPENALEX_WORKS_URL)
            .query(&[
                ("search", args.query.trim().to_string()),
                ("per-page", max_results.to_string()),
                ("mailto", self.mailto.clone()),
            ])
            .send()
            .await
            .map_err(|e| format!("OpenAlex search request failed: {e}"))?;
        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| format!("read OpenAlex response: {e}"))?;
        if !status.is_success() {
            return Err(format!(
                "OpenAlex search returned HTTP {}: {}",
                status.as_u16(),
                truncate_chars(&body, 240)
            ));
        }
        let json: Json =
            serde_json::from_str(&body).map_err(|e| format!("parse OpenAlex JSON: {e}"))?;
        let total = json.pointer("/meta/count").and_then(Json::as_u64);
        Ok(LiteratureSearchResponse {
            query: args.query.trim().to_string(),
            source: PublicLiteratureSource::OpenAlex,
            total,
            results: parse_openalex_json(&json)
                .into_iter()
                .take(max_results as usize)
                .collect(),
            notes: vec!["OpenAlex Works API metadata search".to_string()],
        })
    }

    async fn fetch_openalex(&self, identifier: &str) -> Result<LiteraturePaper, String> {
        let work_id = normalize_openalex_identifier(identifier)
            .ok_or_else(|| "OpenAlex fetch requires an OpenAlex work id/URL or DOI".to_string())?;
        let url = format!("{OPENALEX_WORKS_URL}/{}", encode_path_segment(&work_id));
        let response = self
            .http
            .get(url)
            .query(&[("mailto", self.mailto.clone())])
            .send()
            .await
            .map_err(|e| format!("OpenAlex fetch request failed: {e}"))?;
        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| format!("read OpenAlex fetch response: {e}"))?;
        if !status.is_success() {
            return Err(format!(
                "OpenAlex fetch returned HTTP {}: {}",
                status.as_u16(),
                truncate_chars(&body, 240)
            ));
        }
        let json: Json =
            serde_json::from_str(&body).map_err(|e| format!("parse OpenAlex JSON: {e}"))?;
        parse_openalex_item(&json)
            .ok_or_else(|| format!("OpenAlex did not return a parseable work for `{work_id}`"))
    }

    async fn search_preprint(
        &self,
        source: PublicLiteratureSource,
        args: LiteratureSearchArgs,
    ) -> Result<LiteratureSearchResponse, String> {
        let max_results = args.normalized_max_results();
        let base_url = match source {
            PublicLiteratureSource::Biorxiv => BIORXIV_API_URL,
            PublicLiteratureSource::Medrxiv => MEDRXIV_API_URL,
            _ => return Err(format!("unsupported preprint source {}", source.as_str())),
        };
        let mut results = Vec::new();
        let mut cursor = 0_u32;
        let query = args.query.trim().to_string();
        for _ in 0..PREPRINT_MAX_SCAN_PAGES {
            let url = format!("{base_url}/{PREPRINT_SEARCH_WINDOW_DAYS}d/{cursor}/json");
            let response = self
                .http
                .get(url)
                .send()
                .await
                .map_err(|e| format!("{} search request failed: {e}", source.as_str()))?;
            let status = response.status();
            let body = response
                .text()
                .await
                .map_err(|e| format!("read {} response: {e}", source.as_str()))?;
            if !status.is_success() {
                return Err(format!(
                    "{} search returned HTTP {}: {}",
                    source.as_str(),
                    status.as_u16(),
                    truncate_chars(&body, 240)
                ));
            }
            let json: Json = serde_json::from_str(&body)
                .map_err(|e| format!("parse {} JSON: {e}", source.as_str()))?;
            let mut page = parse_preprint_json(source, &json, &query);
            results.append(&mut page);
            if results.len() >= max_results as usize {
                break;
            }
            let page_len = json
                .get("collection")
                .and_then(Json::as_array)
                .map(Vec::len)
                .unwrap_or(0);
            if page_len < 100 {
                break;
            }
            cursor += 100;
        }
        results.truncate(max_results as usize);
        Ok(LiteratureSearchResponse {
            query,
            source,
            total: None,
            results,
            notes: vec![format!(
                "{} scans recent {} days and filters title/abstract/category locally",
                source.as_str(),
                PREPRINT_SEARCH_WINDOW_DAYS
            )],
        })
    }

    async fn fetch_preprint(
        &self,
        source: PublicLiteratureSource,
        identifier: &str,
    ) -> Result<LiteraturePaper, String> {
        let doi = normalize_doi(identifier);
        if doi.is_empty() {
            return Err(format!(
                "{} fetch requires a DOI or DOI URL",
                source.as_str()
            ));
        }
        let base_url = match source {
            PublicLiteratureSource::Biorxiv => BIORXIV_API_URL,
            PublicLiteratureSource::Medrxiv => MEDRXIV_API_URL,
            _ => return Err(format!("unsupported preprint source {}", source.as_str())),
        };
        let url = format!("{base_url}/{doi}/na/json");
        let response = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|e| format!("{} fetch request failed: {e}", source.as_str()))?;
        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| format!("read {} fetch response: {e}", source.as_str()))?;
        if !status.is_success() {
            return Err(format!(
                "{} fetch returned HTTP {}: {}",
                source.as_str(),
                status.as_u16(),
                truncate_chars(&body, 240)
            ));
        }
        let json: Json = serde_json::from_str(&body)
            .map_err(|e| format!("parse {} JSON: {e}", source.as_str()))?;
        parse_preprint_json(source, &json, "")
            .into_iter()
            .next()
            .ok_or_else(|| {
                format!(
                    "{} did not return a parseable work for `{doi}`",
                    source.as_str()
                )
            })
    }
}

pub fn parse_arxiv_atom(xml: &str) -> Vec<LiteraturePaper> {
    lazy_static! {
        static ref RE_ENTRY: Regex = Regex::new(r#"(?is)<entry\b[^>]*>(.*?)</entry>"#).unwrap();
        static ref RE_AUTHOR: Regex = Regex::new(r#"(?is)<author\b[^>]*>(.*?)</author>"#).unwrap();
        static ref RE_LINK: Regex = Regex::new(r#"(?is)<link\b([^>]*)/?>"#).unwrap();
        static ref RE_CATEGORY: Regex = Regex::new(r#"(?is)<category\b([^>]*)/?>"#).unwrap();
    }

    let mut out = Vec::new();
    for entry in RE_ENTRY.captures_iter(xml) {
        let block = entry.get(1).map(|m| m.as_str()).unwrap_or_default();
        let title = first_xml_tag(block, "title").unwrap_or_default();
        let title = clean_xml_text(&title);
        if title.is_empty() {
            continue;
        }
        let url = clean_xml_text(&first_xml_tag(block, "id").unwrap_or_default());
        let id = arxiv_id_from_url(&url).unwrap_or_else(|| url.clone());
        let abstract_text = clean_xml_text(&first_xml_tag(block, "summary").unwrap_or_default());
        let published_date = normalize_date_string(&clean_xml_text(
            &first_xml_tag(block, "published").unwrap_or_default(),
        ));
        let updated_date = normalize_date_string(&clean_xml_text(
            &first_xml_tag(block, "updated").unwrap_or_default(),
        ));
        let doi = first_xml_tag(block, "doi")
            .map(|s| normalize_doi(&clean_xml_text(&s)))
            .filter(|s| !s.is_empty());
        let authors = RE_AUTHOR
            .captures_iter(block)
            .filter_map(|cap| first_xml_tag(cap.get(1)?.as_str(), "name"))
            .map(|s| clean_xml_text(&s))
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>();
        let mut pdf_url = None;
        for link in RE_LINK.captures_iter(block) {
            let attrs = link.get(1).map(|m| m.as_str()).unwrap_or_default();
            let href = attr_value(attrs, "href").unwrap_or_default();
            let title_attr = attr_value(attrs, "title").unwrap_or_default();
            let type_attr = attr_value(attrs, "type").unwrap_or_default();
            if title_attr.eq_ignore_ascii_case("pdf")
                || type_attr.to_ascii_lowercase().contains("pdf")
                || href.contains("/pdf/")
            {
                pdf_url = Some(href);
                break;
            }
        }
        let categories = RE_CATEGORY
            .captures_iter(block)
            .filter_map(|cap| attr_value(cap.get(1)?.as_str(), "term"))
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>();
        let mut extra = JsonMap::new();
        if let Some(comment) = first_xml_tag(block, "comment").map(|s| clean_xml_text(&s)) {
            if !comment.is_empty() {
                extra.insert("comment".to_string(), json!(comment));
            }
        }
        if let Some(journal_ref) = first_xml_tag(block, "journal_ref").map(|s| clean_xml_text(&s)) {
            if !journal_ref.is_empty() {
                extra.insert("journal_ref".to_string(), json!(journal_ref));
            }
        }
        out.push(LiteraturePaper {
            id,
            source: PublicLiteratureSource::Arxiv,
            title,
            authors,
            abstract_text: if abstract_text.is_empty() {
                None
            } else {
                Some(abstract_text)
            },
            url,
            pdf_url,
            doi,
            published_date,
            updated_date,
            venue: None,
            categories,
            citation_count: None,
            extra,
        });
    }
    out
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

fn first_xml_tag(block: &str, tag: &str) -> Option<String> {
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

fn attr_value(attrs: &str, name: &str) -> Option<String> {
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

fn clean_xml_text(value: &str) -> String {
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

fn normalize_doi(value: &str) -> String {
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

fn normalize_date_string(value: &str) -> Option<String> {
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

fn arxiv_id_from_url(url: &str) -> Option<String> {
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
