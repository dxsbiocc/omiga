//! Shared data-source types and serialization helpers.

use crate::domain::tools::WebSearchApiKeys;
use lazy_static::lazy_static;
use regex::Regex;
use serde::Deserialize;
use serde_json::{json, Map as JsonMap, Value as Json};

const DEFAULT_EUTILS_BASE_URL: &str = "https://eutils.ncbi.nlm.nih.gov/entrez/eutils";
const ENA_PORTAL_SEARCH_URL: &str = "https://www.ebi.ac.uk/ena/portal/api/search";
const ENA_BROWSER_XML_BASE_URL: &str = "https://www.ebi.ac.uk/ena/browser/api/xml";
const CBIOPORTAL_API_BASE_URL: &str = "https://www.cbioportal.org/api";
const GTEX_API_BASE_URL: &str = "https://gtexportal.org/api/v2";
const DEFAULT_MAX_RESULTS: u32 = 10;
pub(super) const MAX_RESULTS_CAP: u32 = 25;
const DEFAULT_EMAIL: &str = "omiga@example.invalid";
const DEFAULT_TOOL: &str = "omiga";
const GEO_FAVICON: &str = "https://www.ncbi.nlm.nih.gov/favicon.ico";
const ENA_FAVICON: &str = "https://www.ebi.ac.uk/favicon.ico";
const CBIOPORTAL_FAVICON: &str = "https://www.cbioportal.org/favicon.ico";
const GTEX_FAVICON: &str = "https://gtexportal.org/favicon.ico";

#[derive(Clone, Debug)]
pub struct DataApiBaseUrls {
    pub entrez: String,
    pub ena_portal_search: String,
    pub ena_browser_xml: String,
    pub cbioportal: String,
    pub gtex: String,
}

impl Default for DataApiBaseUrls {
    fn default() -> Self {
        Self {
            entrez: DEFAULT_EUTILS_BASE_URL.to_string(),
            ena_portal_search: ENA_PORTAL_SEARCH_URL.to_string(),
            ena_browser_xml: ENA_BROWSER_XML_BASE_URL.to_string(),
            cbioportal: CBIOPORTAL_API_BASE_URL.to_string(),
            gtex: GTEX_API_BASE_URL.to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublicDataSource {
    Geo,
    EnaStudy,
    EnaRun,
    EnaExperiment,
    EnaSample,
    EnaAnalysis,
    EnaAssembly,
    EnaSequence,
    CbioPortal,
    Gtex,
}

impl PublicDataSource {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().replace('-', "_").as_str() {
            "geo" | "gds" | "ncbi_geo" | "ncbi_gds" => Some(Self::Geo),
            "ena" | "ena_study" | "study" | "read_study" | "european_nucleotide_archive" => {
                Some(Self::EnaStudy)
            }
            "ena_run" | "run" | "read_run" => Some(Self::EnaRun),
            "ena_experiment" | "experiment" | "read_experiment" => Some(Self::EnaExperiment),
            "ena_sample" | "sample" | "read_sample" => Some(Self::EnaSample),
            "ena_analysis" | "analysis" => Some(Self::EnaAnalysis),
            "ena_assembly" | "assembly" => Some(Self::EnaAssembly),
            "ena_sequence" | "sequence" => Some(Self::EnaSequence),
            "cbioportal" | "cbio_portal" | "cbio" | "cancer_genomics" | "multi_omics"
            | "multiomics" | "projects" | "tcga" => Some(Self::CbioPortal),
            "gtex" | "genotype_tissue_expression" | "tissue_expression" => Some(Self::Gtex),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Geo => "geo",
            Self::EnaStudy => "ena",
            Self::EnaRun => "ena_run",
            Self::EnaExperiment => "ena_experiment",
            Self::EnaSample => "ena_sample",
            Self::EnaAnalysis => "ena_analysis",
            Self::EnaAssembly => "ena_assembly",
            Self::EnaSequence => "ena_sequence",
            Self::CbioPortal => "cbioportal",
            Self::Gtex => "gtex",
        }
    }

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Geo => "NCBI GEO DataSets",
            Self::EnaStudy => "ENA read studies",
            Self::EnaRun => "ENA raw read runs",
            Self::EnaExperiment => "ENA read experiments",
            Self::EnaSample => "ENA samples",
            Self::EnaAnalysis => "ENA analyses",
            Self::EnaAssembly => "ENA assemblies",
            Self::EnaSequence => "ENA nucleotide sequences",
            Self::CbioPortal => "cBioPortal cancer genomics studies",
            Self::Gtex => "GTEx tissue expression",
        }
    }

    pub(super) fn favicon(self) -> &'static str {
        match self {
            Self::Geo => GEO_FAVICON,
            Self::EnaStudy
            | Self::EnaRun
            | Self::EnaExperiment
            | Self::EnaSample
            | Self::EnaAnalysis
            | Self::EnaAssembly
            | Self::EnaSequence => ENA_FAVICON,
            Self::CbioPortal => CBIOPORTAL_FAVICON,
            Self::Gtex => GTEX_FAVICON,
        }
    }

    pub(super) fn ena_result(self) -> Option<&'static str> {
        match self {
            Self::Geo => None,
            Self::EnaStudy => Some("read_study"),
            Self::EnaRun => Some("read_run"),
            Self::EnaExperiment => Some("read_experiment"),
            Self::EnaSample => Some("sample"),
            Self::EnaAnalysis => Some("analysis"),
            Self::EnaAssembly => Some("assembly"),
            Self::EnaSequence => Some("sequence"),
            Self::CbioPortal | Self::Gtex => None,
        }
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct DataSearchArgs {
    #[serde(alias = "term", alias = "q")]
    pub query: String,
    #[serde(default, alias = "maxResults", alias = "limit", alias = "retmax")]
    pub max_results: Option<u32>,
    #[serde(default)]
    pub params: Option<Json>,
}

impl DataSearchArgs {
    pub fn normalized_max_results(&self) -> u32 {
        self.max_results
            .unwrap_or(DEFAULT_MAX_RESULTS)
            .clamp(1, MAX_RESULTS_CAP)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataRecord {
    pub id: String,
    pub accession: String,
    pub source: PublicDataSource,
    pub title: String,
    pub summary: String,
    pub url: String,
    pub record_type: Option<String>,
    pub organism: Option<String>,
    pub published_date: Option<String>,
    pub updated_date: Option<String>,
    pub sample_count: Option<u64>,
    pub platform: Option<String>,
    pub files: Vec<String>,
    pub extra: JsonMap<String, Json>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataSearchResponse {
    pub query: String,
    pub source: String,
    pub total: Option<u64>,
    pub results: Vec<DataRecord>,
    pub notes: Vec<String>,
}

#[derive(Clone, Debug)]
pub(super) struct EntrezSettings {
    pub(super) api_key: Option<String>,
    pub(super) email: String,
    pub(super) tool: String,
}

impl EntrezSettings {
    pub(super) fn from_keys(keys: &WebSearchApiKeys) -> Self {
        Self {
            api_key: clean_optional(&keys.pubmed_api_key),
            email: clean_optional(&keys.pubmed_email).unwrap_or_else(|| DEFAULT_EMAIL.to_string()),
            tool: clean_optional(&keys.pubmed_tool_name)
                .unwrap_or_else(|| DEFAULT_TOOL.to_string()),
        }
    }
}

pub fn search_response_to_json(response: &DataSearchResponse) -> Json {
    let results: Vec<Json> = response
        .results
        .iter()
        .enumerate()
        .map(|(idx, item)| record_to_serp_result(item, idx + 1))
        .collect();
    json!({
        "query": response.query,
        "category": "data",
        "source": response.source,
        "effective_source": response.source,
        "total": response.total,
        "route_notes": response.notes,
        "results": results,
    })
}

pub fn detail_to_json(record: &DataRecord) -> Json {
    json!({
        "category": "data",
        "source": record.source.as_str(),
        "effective_source": record.source.as_str(),
        "id": record.id,
        "accession": record.accession,
        "title": record.title,
        "name": record.title,
        "link": record.url,
        "url": record.url,
        "displayed_link": displayed_link_for_url(&record.url),
        "favicon": record.source.favicon(),
        "snippet": data_record_snippet(record),
        "content": data_record_content(record),
        "metadata": record_metadata(record),
    })
}

fn record_to_serp_result(record: &DataRecord, position: usize) -> Json {
    json!({
        "position": position,
        "category": "data",
        "source": record.source.as_str(),
        "title": record.title,
        "name": record.title,
        "link": record.url,
        "url": record.url,
        "displayed_link": displayed_link_for_url(&record.url),
        "favicon": record.source.favicon(),
        "snippet": data_record_snippet(record),
        "id": record.id,
        "accession": record.accession,
        "metadata": record_metadata(record),
    })
}

fn record_metadata(record: &DataRecord) -> Json {
    json!({
        "accession": record.accession,
        "source_label": record.source.label(),
        "record_type": record.record_type,
        "organism": record.organism,
        "published_date": record.published_date,
        "updated_date": record.updated_date,
        "sample_count": record.sample_count,
        "platform": record.platform,
        "files": record.files,
        "source_specific": record.extra,
    })
}

fn data_record_snippet(record: &DataRecord) -> String {
    let mut pieces = Vec::new();
    if let Some(record_type) = record
        .record_type
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        pieces.push(record_type.to_string());
    }
    if let Some(organism) = record.organism.as_deref().filter(|s| !s.trim().is_empty()) {
        pieces.push(organism.to_string());
    }
    if let Some(samples) = record.sample_count {
        pieces.push(format!("{samples} samples"));
    }
    if !record.summary.trim().is_empty() {
        pieces.push(truncate_chars(&record.summary, 280));
    }
    pieces.join(" | ")
}

fn data_record_content(record: &DataRecord) -> String {
    let mut out = String::new();
    out.push_str(&record.title);
    out.push_str("\n\n");
    out.push_str("Source: ");
    out.push_str(record.source.label());
    out.push('\n');
    if !record.accession.trim().is_empty() {
        out.push_str("Accession: ");
        out.push_str(&record.accession);
        out.push('\n');
    }
    if let Some(record_type) = record
        .record_type
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        out.push_str("Record type: ");
        out.push_str(record_type);
        out.push('\n');
    }
    if let Some(organism) = record.organism.as_deref().filter(|s| !s.trim().is_empty()) {
        out.push_str("Organism: ");
        out.push_str(organism);
        out.push('\n');
    }
    if let Some(samples) = record.sample_count {
        out.push_str("Samples: ");
        out.push_str(&samples.to_string());
        out.push('\n');
    }
    if let Some(platform) = record.platform.as_deref().filter(|s| !s.trim().is_empty()) {
        out.push_str("Platform: ");
        out.push_str(platform);
        out.push('\n');
    }
    if let Some(date) = record
        .published_date
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        out.push_str("Published: ");
        out.push_str(date);
        out.push('\n');
    }
    out.push_str("Link: ");
    out.push_str(&record.url);
    if !record.files.is_empty() {
        out.push_str("\nFiles:\n");
        for file in &record.files {
            out.push_str("- ");
            out.push_str(file);
            out.push('\n');
        }
    }
    if !record.summary.trim().is_empty() {
        out.push_str("\nSummary\n");
        out.push_str(record.summary.trim());
    }
    out.trim().to_string()
}

pub(super) fn normalize_accession(value: &str) -> Option<String> {
    let value = value.trim().trim_end_matches('/');
    if value.is_empty() {
        return None;
    }
    if let Ok(parsed) = reqwest::Url::parse(value) {
        let host = parsed.host_str().unwrap_or_default().to_ascii_lowercase();
        if host.contains("ebi.ac.uk") && parsed.path().contains("/ena/browser/view/") {
            return parsed
                .path_segments()
                .and_then(|mut segments| segments.next_back())
                .filter(|s| !s.is_empty())
                .map(str::to_string);
        }
        if host.contains("ncbi.nlm.nih.gov") {
            for (key, val) in parsed.query_pairs() {
                if key.eq_ignore_ascii_case("acc") && !val.trim().is_empty() {
                    return Some(val.into_owned());
                }
            }
        }
    }
    Some(value.to_string())
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
    if let Some(query) = parsed.query().filter(|q| !q.is_empty()) {
        out.push('?');
        out.push_str(query);
    }
    out
}

pub(super) fn string_field_any(map: &JsonMap<String, Json>, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(value) = map.get(*key).and_then(json_string) {
            return Some(value);
        }
    }
    None
}

pub(super) fn nested_string_field(
    map: &JsonMap<String, Json>,
    object_key: &str,
    keys: &[&str],
) -> Option<String> {
    let nested = map.get(object_key)?.as_object()?;
    string_field_any(nested, keys)
}

pub(super) fn string_vec_field_any(map: &JsonMap<String, Json>, keys: &[&str]) -> Vec<String> {
    for key in keys {
        let Some(value) = map.get(*key) else {
            continue;
        };
        if let Some(items) = value.as_array() {
            let out = items.iter().filter_map(json_string).collect::<Vec<_>>();
            if !out.is_empty() {
                return out;
            }
        }
        if let Some(value) = json_string(value) {
            let out = value
                .split(';')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>();
            if !out.is_empty() {
                return out;
            }
        }
    }
    Vec::new()
}

pub(super) fn json_u64_from_keys(map: &JsonMap<String, Json>, keys: &[&str]) -> Option<u64> {
    keys.iter()
        .find_map(|key| map.get(*key).and_then(json_u64_from_string_or_number))
}

pub(super) fn json_string(value: &Json) -> Option<String> {
    match value {
        Json::String(s) => {
            let s = s.trim();
            (!s.is_empty()).then(|| s.to_string())
        }
        Json::Number(n) => Some(n.to_string()),
        Json::Array(items) => {
            let joined = items
                .iter()
                .filter_map(json_string)
                .collect::<Vec<_>>()
                .join(", ");
            (!joined.is_empty()).then_some(joined)
        }
        _ => None,
    }
}

pub(super) fn json_number_string(value: &Json) -> Option<String> {
    value
        .as_f64()
        .map(|v| {
            if v.fract() == 0.0 {
                format!("{v:.0}")
            } else {
                v.to_string()
            }
        })
        .or_else(|| value.as_str().map(|s| s.trim().to_string()))
        .filter(|s| !s.is_empty())
}

pub(super) fn json_u64_from_string_or_number(value: &Json) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_str()?.trim().parse::<u64>().ok())
}

pub(super) fn clean_html_text(value: &str) -> String {
    lazy_static! {
        static ref RE_TAG: Regex = Regex::new(r#"(?is)<[^>]+>"#).unwrap();
        static ref RE_WS: Regex = Regex::new(r#"\s+"#).unwrap();
    }
    let without_tags = RE_TAG.replace_all(value, " ");
    let decoded = decode_xml_text(without_tags.as_ref());
    RE_WS.replace_all(decoded.trim(), " ").to_string()
}

pub(super) fn clean_xml_fragment(value: &str) -> String {
    clean_html_text(value)
}

fn decode_xml_text(value: &str) -> String {
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

pub(super) fn extract_xml_attr(xml: &str, tag: &str, attr: &str) -> Option<String> {
    let re = Regex::new(&format!(
        r#"(?is)<{}\b(?P<attrs>[^>]*)>"#,
        regex::escape(tag)
    ))
    .ok()?;
    let attr_re = Regex::new(&format!(
        r#"(?i)\b{}\s*=\s*["'](?P<value>[^"']+)["']"#,
        regex::escape(attr)
    ))
    .ok()?;
    for cap in re.captures_iter(xml) {
        let Some(attrs) = cap.name("attrs").map(|m| m.as_str()) else {
            continue;
        };
        if let Some(value) = attr_re
            .captures(attrs)
            .and_then(|c| c.name("value"))
            .map(|m| decode_xml_text(m.as_str()))
        {
            return Some(value);
        }
    }
    None
}

pub(super) fn extract_first_xml_tag(xml: &str, tags: &[&str]) -> Option<String> {
    for tag in tags {
        let re = Regex::new(&format!(
            r#"(?is)<{}\b[^>]*>(?P<body>.*?)</{}>"#,
            regex::escape(tag),
            regex::escape(tag)
        ))
        .ok()?;
        if let Some(value) = re
            .captures(xml)
            .and_then(|cap| cap.name("body"))
            .map(|m| clean_xml_fragment(m.as_str()))
            .filter(|s| !s.trim().is_empty())
        {
            return Some(value);
        }
    }
    None
}

pub(super) fn extract_ena_file_links(xml: &str) -> Vec<String> {
    lazy_static! {
        static ref RE_XREF: Regex =
            Regex::new(r#"(?is)<XREF_LINK\b[^>]*>(?P<body>.*?)</XREF_LINK>"#).unwrap();
    }
    let mut files = Vec::new();
    for cap in RE_XREF.captures_iter(xml) {
        let body = cap.name("body").map(|m| m.as_str()).unwrap_or("");
        if let Some(url) = extract_first_xml_tag(body, &["URL"]).filter(|u| {
            let lower = u.to_ascii_lowercase();
            lower.starts_with("ftp://")
                || lower.starts_with("http://")
                || lower.starts_with("https://")
        }) {
            files.push(url);
        }
    }
    files
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

pub(super) fn truncate_for_error(value: &str) -> String {
    truncate_chars(value, 500)
}

pub(super) fn encode_path_segment(value: &str) -> String {
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

pub(super) fn clean_optional(value: &Option<String>) -> Option<String> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}
