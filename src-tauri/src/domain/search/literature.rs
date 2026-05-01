//! Public literature-source adapters used by the built-in `search` tool.
//!
//! This module follows the free-first connector shape from `paper-search-mcp`:
//! source-specific HTTP details stay behind one normalized `LiteraturePaper`
//! record and one SerpAPI-style JSON serializer. The first batch focuses on
//! public metadata APIs that do not need paid credentials.

use serde_json::{Map as JsonMap, Value as Json};

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
mod common;
mod crossref;
mod openalex;
mod operations;
mod output;
mod preprint;
mod routing;

pub use arxiv::parse_arxiv_atom;
pub use client::PublicLiteratureClient;
pub(in crate::domain::search::literature) use common::{
    arxiv_id_from_url, attr_value, clean_html_text, clean_optional, clean_xml_text,
    encode_path_segment, first_xml_tag, normalize_arxiv_identifier, normalize_date_string,
    normalize_doi, normalize_openalex_identifier, truncate_chars,
};
pub(in crate::domain::search::literature) use crossref::parse_crossref_item;
pub use crossref::parse_crossref_json;
pub(in crate::domain::search::literature) use openalex::parse_openalex_item;
pub use openalex::parse_openalex_json;
pub use output::{paper_to_detail_json, search_response_to_json};
pub use preprint::parse_preprint_json;

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

#[cfg(test)]
mod tests;
