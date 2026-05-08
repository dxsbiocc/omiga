//! PubMed adapter backed by official NCBI Entrez E-utilities.
//!
//! Search uses ESearch + ESummary. Fetch uses EFetch XML and parses the first
//! PubMed article into a structured document. This module intentionally has no
//! MCP/stdout surface; it is the single in-process PubMed implementation used by
//! the built-in `search` and `fetch` tools.

use crate::domain::tools::{ToolContext, WebSearchApiKeys};
use lazy_static::lazy_static;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map as JsonMap, Value as Json};
use std::time::Duration;

const DEFAULT_EUTILS_BASE_URL: &str = "https://eutils.ncbi.nlm.nih.gov/entrez/eutils";
const DEFAULT_MAX_RESULTS: u32 = 10;
const MAX_RESULTS_CAP: u32 = 100;
const PUBMED_FAVICON: &str = "https://pubmed.ncbi.nlm.nih.gov/favicon.ico";
const DEFAULT_EMAIL: &str = "omiga@example.invalid";
const DEFAULT_TOOL: &str = "omiga";

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct PubmedSearchArgs {
    #[serde(alias = "term", alias = "q")]
    pub query: String,
    #[serde(default, alias = "maxResults", alias = "limit", alias = "retmax")]
    pub max_results: Option<u32>,
    #[serde(
        default,
        alias = "retStart",
        alias = "retstart",
        alias = "offset",
        alias = "start"
    )]
    pub ret_start: Option<u32>,
    #[serde(default)]
    pub sort: Option<String>,
    #[serde(default, alias = "dateType", alias = "datetype")]
    pub date_type: Option<String>,
    #[serde(default)]
    pub mindate: Option<String>,
    #[serde(default)]
    pub maxdate: Option<String>,
}

impl PubmedSearchArgs {
    pub fn normalized_max_results(&self) -> u32 {
        self.max_results
            .unwrap_or(DEFAULT_MAX_RESULTS)
            .clamp(1, MAX_RESULTS_CAP)
    }

    pub fn normalized_ret_start(&self) -> u32 {
        self.ret_start.unwrap_or(0)
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PubmedArticleId {
    pub id_type: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PubmedArticleSummary {
    pub pmid: String,
    pub title: String,
    pub authors: String,
    pub author_list: Vec<String>,
    pub source: String,
    pub full_journal_name: String,
    pub pub_date: String,
    pub epub_date: String,
    pub volume: String,
    pub issue: String,
    pub pages: String,
    pub doi: Option<String>,
    pub article_ids: Vec<PubmedArticleId>,
    pub pub_types: Vec<String>,
    pub pubmed_url: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PubmedSearchResponse {
    pub query: String,
    pub count: u64,
    pub ret_start: u32,
    pub ret_max: u32,
    pub query_translation: Option<String>,
    pub ids: Vec<String>,
    pub summaries: Vec<PubmedArticleSummary>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PubmedArticleDetail {
    pub pmid: String,
    pub title: String,
    pub article_title: String,
    pub abstract_text: String,
    pub authors: Vec<String>,
    pub journal: String,
    pub journal_abbreviation: String,
    pub publication_year: Option<u32>,
    pub pub_date: String,
    pub volume: String,
    pub issue: String,
    pub pages: String,
    pub doi: Option<String>,
    pub article_ids: Vec<PubmedArticleId>,
    pub pub_types: Vec<String>,
    pub link: String,
    pub content: String,
}

#[derive(Clone)]
pub struct EntrezClient {
    http: reqwest::Client,
    base_url: String,
    settings: PubmedEntrezSettings,
}

#[derive(Clone, Debug)]
struct PubmedEntrezSettings {
    api_key: Option<String>,
    email: String,
    tool: String,
}

impl PubmedEntrezSettings {
    fn from_keys(keys: &WebSearchApiKeys) -> Self {
        Self {
            api_key: clean_optional(&keys.pubmed_api_key),
            email: clean_optional(&keys.pubmed_email).unwrap_or_else(|| DEFAULT_EMAIL.to_string()),
            tool: clean_optional(&keys.pubmed_tool_name)
                .unwrap_or_else(|| DEFAULT_TOOL.to_string()),
        }
    }
}

impl EntrezClient {
    pub fn from_tool_context(ctx: &ToolContext) -> Result<Self, String> {
        let mut builder = reqwest::Client::builder()
            .timeout(Duration::from_secs(ctx.timeout_secs.clamp(5, 60)))
            .user_agent(format!("Omiga-PubMed/{}", env!("CARGO_PKG_VERSION")));
        if !ctx.web_use_proxy {
            builder = builder.no_proxy();
        }
        let http = builder
            .build()
            .map_err(|e| format!("build NCBI Entrez HTTP client: {e}"))?;
        Ok(Self {
            http,
            base_url: DEFAULT_EUTILS_BASE_URL.to_string(),
            settings: PubmedEntrezSettings::from_keys(&ctx.web_search_api_keys),
        })
    }

    #[cfg(test)]
    pub fn with_base_url(base_url: impl Into<String>) -> Result<Self, String> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent(format!("Omiga-PubMed/{}", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| format!("build NCBI Entrez HTTP client: {e}"))?;
        Ok(Self {
            http,
            base_url: base_url.into().trim_end_matches('/').to_string(),
            settings: PubmedEntrezSettings {
                api_key: None,
                email: DEFAULT_EMAIL.to_string(),
                tool: DEFAULT_TOOL.to_string(),
            },
        })
    }

    pub async fn search(&self, args: PubmedSearchArgs) -> Result<PubmedSearchResponse, String> {
        if args.query.trim().is_empty() {
            return Err("PubMed query must not be empty".to_string());
        }
        let ret_max = args.normalized_max_results();
        let ret_start = args.normalized_ret_start();

        let mut params = self.common_entrez_params("json");
        params.push(("term".to_string(), args.query.trim().to_string()));
        params.push(("retmax".to_string(), ret_max.to_string()));
        params.push(("retstart".to_string(), ret_start.to_string()));

        if let Some(sort) = clean_optional(&args.sort) {
            params.push(("sort".to_string(), sort));
        }
        if let Some(date_type) = clean_optional(&args.date_type) {
            params.push(("datetype".to_string(), date_type));
        }
        if let Some(mindate) = clean_optional(&args.mindate) {
            params.push(("mindate".to_string(), mindate));
        }
        if let Some(maxdate) = clean_optional(&args.maxdate) {
            params.push(("maxdate".to_string(), maxdate));
        }

        let search_json = self.get_json("esearch", &params).await?;
        let search = parse_esearch_result(&args.query, ret_start, ret_max, &search_json)?;

        if search.ids.is_empty() {
            return Ok(search);
        }

        let mut summary_params = self.common_entrez_params("json");
        summary_params.push(("id".to_string(), search.ids.join(",")));
        let summary_json = self.get_json("esummary", &summary_params).await?;
        let summaries = parse_esummary_result(&summary_json, &search.ids);

        Ok(PubmedSearchResponse {
            summaries,
            ..search
        })
    }

    pub async fn fetch_by_pmid(&self, pmid: &str) -> Result<PubmedArticleDetail, String> {
        let pmid = pmid.trim();
        if pmid.is_empty() || !pmid.chars().all(|c| c.is_ascii_digit()) {
            return Err("PubMed fetch currently expects a numeric PMID".to_string());
        }
        let mut params = self.common_entrez_params("xml");
        params.push(("id".to_string(), pmid.to_string()));
        let xml = self.get_text("efetch", &params).await?;
        parse_efetch_article(&xml, pmid)
    }

    fn common_entrez_params(&self, retmode: &str) -> Vec<(String, String)> {
        let mut params = vec![
            ("db".to_string(), "pubmed".to_string()),
            ("retmode".to_string(), retmode.to_string()),
            ("tool".to_string(), self.settings.tool.clone()),
            ("email".to_string(), self.settings.email.clone()),
        ];
        if let Some(api_key) = &self.settings.api_key {
            params.push(("api_key".to_string(), api_key.clone()));
        }
        params
    }

    async fn get_json(&self, utility: &str, params: &[(String, String)]) -> Result<Json, String> {
        let body = self.get_text(utility, params).await?;
        let json: Json = serde_json::from_str(&body).map_err(|e| {
            format!(
                "NCBI Entrez {utility} returned non-JSON response: {e}; body: {}",
                truncate_for_error(&body)
            )
        })?;
        if let Some(error) = json.get("error").and_then(Json::as_str) {
            return Err(format!("NCBI Entrez {utility} error: {error}"));
        }
        Ok(json)
    }

    async fn get_text(&self, utility: &str, params: &[(String, String)]) -> Result<String, String> {
        let url = format!("{}/{}.fcgi", self.base_url, utility);
        let response = self
            .http
            .get(&url)
            .query(params)
            .send()
            .await
            .map_err(|e| format!("NCBI Entrez {utility} request failed: {e}"))?;

        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| format!("NCBI Entrez {utility} response read failed: {e}"))?;

        if !status.is_success() {
            return Err(format!(
                "NCBI Entrez {utility} returned HTTP {status}: {}",
                truncate_for_error(&body)
            ));
        }
        Ok(body)
    }
}

pub fn search_response_to_json(response: &PubmedSearchResponse) -> Json {
    let results: Vec<Json> = response
        .summaries
        .iter()
        .enumerate()
        .map(|(idx, item)| summary_to_serp_result(item, idx + 1))
        .collect();
    json!({
        "query": response.query,
        "category": "literature",
        "source": "pubmed",
        "effective_source": "pubmed",
        "count": response.count,
        "ret_start": response.ret_start,
        "ret_max": response.ret_max,
        "query_translation": response.query_translation,
        "ids": response.ids,
        "results": results,
    })
}

pub fn detail_to_json(detail: &PubmedArticleDetail) -> Json {
    json!({
        "category": "literature",
        "source": "pubmed",
        "effective_source": "pubmed",
        "id": detail.pmid,
        "title": detail.title,
        "article_title": detail.article_title,
        "name": detail.title,
        "link": detail.link,
        "url": detail.link,
        "displayed_link": format!("pubmed.ncbi.nlm.nih.gov/{}", detail.pmid),
        "favicon": PUBMED_FAVICON,
        "abstract": detail.abstract_text,
        "authors": detail.authors,
        "content": detail.content,
        "metadata": {
            "pmid": detail.pmid,
            "doi": detail.doi,
            "authors": detail.authors,
            "journal": detail.journal,
            "journal_abbreviation": detail.journal_abbreviation,
            "publication_year": detail.publication_year,
            "pub_date": detail.pub_date,
            "volume": detail.volume,
            "issue": detail.issue,
            "pages": detail.pages,
            "article_ids": detail.article_ids,
            "pub_types": detail.pub_types,
        }
    })
}

fn summary_to_serp_result(item: &PubmedArticleSummary, position: usize) -> Json {
    let displayed_link = format!("pubmed.ncbi.nlm.nih.gov/{}", item.pmid);
    json!({
        "position": position,
        "category": "literature",
        "source": "pubmed",
        "title": item.title,
        "name": item.title,
        "link": item.pubmed_url,
        "url": item.pubmed_url,
        "displayed_link": displayed_link,
        "favicon": PUBMED_FAVICON,
        "snippet": pubmed_summary_snippet(item),
        "id": item.pmid,
        "metadata": {
            "pmid": item.pmid,
            "doi": item.doi,
            "authors": item.author_list,
            "author_text": item.authors,
            "journal": item.full_journal_name,
            "source": item.source,
            "publication_year": publication_year_from_date(&item.pub_date),
            "pub_date": item.pub_date,
            "epub_date": item.epub_date,
            "volume": item.volume,
            "issue": item.issue,
            "pages": item.pages,
            "article_ids": item.article_ids,
            "pub_types": item.pub_types,
            "article_title": item.title,
        }
    })
}

fn pubmed_summary_snippet(item: &PubmedArticleSummary) -> String {
    [
        item.authors.as_str(),
        item.full_journal_name.as_str(),
        item.source.as_str(),
        item.pub_date.as_str(),
    ]
    .into_iter()
    .map(str::trim)
    .filter(|s| !s.is_empty())
    .collect::<Vec<_>>()
    .join(" | ")
}

fn parse_esearch_result(
    query: &str,
    ret_start: u32,
    ret_max: u32,
    value: &Json,
) -> Result<PubmedSearchResponse, String> {
    let root = value
        .get("esearchresult")
        .and_then(Json::as_object)
        .ok_or_else(|| "NCBI ESearch response missing esearchresult".to_string())?;

    if let Some(error) = root.get("error").and_then(Json::as_str) {
        return Err(format!("NCBI ESearch error: {error}"));
    }

    let count = root
        .get("count")
        .and_then(json_u64_from_string_or_number)
        .unwrap_or(0);
    let ids = root
        .get("idlist")
        .and_then(Json::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Json::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let query_translation = root
        .get("querytranslation")
        .and_then(Json::as_str)
        .map(str::to_string);

    Ok(PubmedSearchResponse {
        query: query.to_string(),
        count,
        ret_start,
        ret_max,
        query_translation,
        ids,
        summaries: Vec::new(),
    })
}

fn parse_esummary_result(value: &Json, ordered_ids: &[String]) -> Vec<PubmedArticleSummary> {
    let Some(result) = value.get("result").and_then(Json::as_object) else {
        return Vec::new();
    };

    ordered_ids
        .iter()
        .filter_map(|pmid| {
            result
                .get(pmid)
                .and_then(|doc| parse_pubmed_summary_doc(pmid, doc))
        })
        .collect()
}

fn parse_pubmed_summary_doc(pmid: &str, doc: &Json) -> Option<PubmedArticleSummary> {
    let map = doc.as_object()?;
    let title = string_field(map, "title");
    let author_list = map
        .get("authors")
        .and_then(Json::as_array)
        .map(|authors| {
            authors
                .iter()
                .filter_map(|author| author.get("name").and_then(Json::as_str))
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let authors = author_list.join(", ");
    let article_ids = parse_article_ids_json(map.get("articleids"));
    let doi = article_ids
        .iter()
        .find(|id| id.id_type.eq_ignore_ascii_case("doi"))
        .map(|id| id.value.clone());
    let pub_types = map
        .get("pubtype")
        .and_then(Json::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Json::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();

    Some(PubmedArticleSummary {
        pmid: pmid.to_string(),
        title,
        authors,
        author_list,
        source: string_field(map, "source"),
        full_journal_name: string_field(map, "fulljournalname"),
        pub_date: string_field(map, "pubdate"),
        epub_date: string_field(map, "epubdate"),
        volume: string_field(map, "volume"),
        issue: string_field(map, "issue"),
        pages: string_field(map, "pages"),
        doi,
        article_ids,
        pub_types,
        pubmed_url: format!("https://pubmed.ncbi.nlm.nih.gov/{pmid}/"),
    })
}

fn parse_efetch_article(xml: &str, fallback_pmid: &str) -> Result<PubmedArticleDetail, String> {
    let article_xml = extract_first_block(xml, "PubmedArticle").unwrap_or_else(|| xml.to_string());
    let pmid = extract_tag_text(&article_xml, "PMID").unwrap_or_else(|| fallback_pmid.to_string());
    let article_title = extract_tag_text(&article_xml, "ArticleTitle").unwrap_or_default();
    let title = article_title.clone();
    let abstract_text = extract_abstract(&article_xml);
    let authors = extract_authors(&article_xml);
    let journal = extract_tag_text(&article_xml, "Title").unwrap_or_default();
    let journal_abbreviation =
        extract_tag_text(&article_xml, "ISOAbbreviation").unwrap_or_default();
    let pub_date = extract_pub_date(&article_xml);
    let publication_year = publication_year_from_date(&pub_date);
    let volume = extract_tag_text(&article_xml, "Volume").unwrap_or_default();
    let issue = extract_tag_text(&article_xml, "Issue").unwrap_or_default();
    let pages = extract_tag_text(&article_xml, "MedlinePgn").unwrap_or_default();
    let article_ids = extract_article_ids_xml(&article_xml);
    let doi = article_ids
        .iter()
        .find(|id| id.id_type.eq_ignore_ascii_case("doi"))
        .map(|id| id.value.clone());
    let pub_types = extract_publication_types(&article_xml);
    let link = format!("https://pubmed.ncbi.nlm.nih.gov/{pmid}/");
    let content = pubmed_detail_content(
        &article_title,
        &authors,
        &journal,
        publication_year,
        &doi,
        &abstract_text,
        &link,
    );

    if article_title.trim().is_empty() && abstract_text.trim().is_empty() {
        return Err(format!(
            "NCBI EFetch did not return a parseable PubMed article for PMID {fallback_pmid}"
        ));
    }

    Ok(PubmedArticleDetail {
        pmid,
        title,
        article_title,
        abstract_text,
        authors,
        journal,
        journal_abbreviation,
        publication_year,
        pub_date,
        volume,
        issue,
        pages,
        doi,
        article_ids,
        pub_types,
        link,
        content,
    })
}

fn pubmed_detail_content(
    title: &str,
    authors: &[String],
    journal: &str,
    year: Option<u32>,
    doi: &Option<String>,
    abstract_text: &str,
    link: &str,
) -> String {
    let mut out = String::new();
    if !title.trim().is_empty() {
        out.push_str(title.trim());
        out.push_str("\n\n");
    }
    if !authors.is_empty() {
        out.push_str("Authors: ");
        out.push_str(&authors.join(", "));
        out.push('\n');
    }
    if !journal.trim().is_empty() || year.is_some() {
        out.push_str("Journal: ");
        out.push_str(journal.trim());
        if let Some(year) = year {
            out.push_str(&format!(" ({year})"));
        }
        out.push('\n');
    }
    if let Some(doi) = doi.as_ref().filter(|d| !d.trim().is_empty()) {
        out.push_str("DOI: ");
        out.push_str(doi.trim());
        out.push('\n');
    }
    out.push_str("Link: ");
    out.push_str(link);
    out.push_str("\n\nAbstract:\n");
    out.push_str(abstract_text.trim());
    out
}

fn extract_abstract(xml: &str) -> String {
    lazy_static! {
        static ref RE_ABSTRACT_TEXT: Regex =
            Regex::new(r#"(?is)<AbstractText\b(?P<attrs>[^>]*)>(?P<body>.*?)</AbstractText>"#)
                .expect("regex");
        static ref RE_LABEL: Regex =
            Regex::new(r#"(?i)\bLabel\s*=\s*\"(?P<label>[^\"]+)\""#).expect("regex");
    }
    let mut parts = Vec::new();
    for cap in RE_ABSTRACT_TEXT.captures_iter(xml) {
        let attrs = cap.name("attrs").map(|m| m.as_str()).unwrap_or("");
        let label = RE_LABEL
            .captures(attrs)
            .and_then(|c| c.name("label"))
            .map(|m| decode_xml_text(m.as_str()));
        let body = cap
            .name("body")
            .map(|m| clean_xml_fragment(m.as_str()))
            .unwrap_or_default();
        if body.trim().is_empty() {
            continue;
        }
        if let Some(label) = label.filter(|l| !l.trim().is_empty()) {
            parts.push(format!("{}: {}", label.trim(), body.trim()));
        } else {
            parts.push(body);
        }
    }
    parts.join("\n")
}

fn extract_authors(xml: &str) -> Vec<String> {
    lazy_static! {
        static ref RE_AUTHOR: Regex =
            Regex::new(r#"(?is)<Author\b[^>]*>(?P<body>.*?)</Author>"#).expect("regex");
    }
    let mut out = Vec::new();
    for cap in RE_AUTHOR.captures_iter(xml) {
        let body = cap.name("body").map(|m| m.as_str()).unwrap_or("");
        if let Some(collective) = extract_tag_text(body, "CollectiveName") {
            out.push(collective);
            continue;
        }
        let fore =
            extract_tag_text(body, "ForeName").or_else(|| extract_tag_text(body, "Initials"));
        let last = extract_tag_text(body, "LastName");
        let name = match (fore, last) {
            (Some(fore), Some(last)) if !fore.is_empty() && !last.is_empty() => {
                format!("{fore} {last}")
            }
            (Some(fore), _) => fore,
            (_, Some(last)) => last,
            _ => String::new(),
        };
        if !name.trim().is_empty() {
            out.push(name);
        }
    }
    out
}

fn extract_article_ids_xml(xml: &str) -> Vec<PubmedArticleId> {
    lazy_static! {
        static ref RE_ARTICLE_ID: Regex = Regex::new(r#"(?is)<ArticleId\b[^>]*\bIdType\s*=\s*\"(?P<type>[^\"]+)\"[^>]*>(?P<value>.*?)</ArticleId>"#).expect("regex");
    }
    RE_ARTICLE_ID
        .captures_iter(xml)
        .filter_map(|cap| {
            let id_type = cap.name("type").map(|m| decode_xml_text(m.as_str()))?;
            let value = cap.name("value").map(|m| clean_xml_fragment(m.as_str()))?;
            (!id_type.trim().is_empty() && !value.trim().is_empty())
                .then_some(PubmedArticleId { id_type, value })
        })
        .collect()
}

fn extract_publication_types(xml: &str) -> Vec<String> {
    lazy_static! {
        static ref RE_PUB_TYPE: Regex =
            Regex::new(r#"(?is)<PublicationType\b[^>]*>(?P<body>.*?)</PublicationType>"#)
                .expect("regex");
    }
    RE_PUB_TYPE
        .captures_iter(xml)
        .filter_map(|cap| cap.name("body").map(|m| clean_xml_fragment(m.as_str())))
        .filter(|s| !s.trim().is_empty())
        .collect()
}

fn extract_pub_date(xml: &str) -> String {
    let Some(block) = extract_first_block(xml, "PubDate") else {
        return String::new();
    };
    if let Some(medline) = extract_tag_text(&block, "MedlineDate") {
        return medline;
    }
    ["Year", "Month", "Day"]
        .into_iter()
        .filter_map(|tag| extract_tag_text(&block, tag))
        .filter(|s| !s.trim().is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn extract_first_block(xml: &str, tag: &str) -> Option<String> {
    let re = Regex::new(&format!(r#"(?is)<{tag}\b[^>]*>.*?</{tag}>"#)).ok()?;
    re.find(xml).map(|m| m.as_str().to_string())
}

fn extract_tag_text(xml: &str, tag: &str) -> Option<String> {
    let re = Regex::new(&format!(r#"(?is)<{tag}\b[^>]*>(?P<body>.*?)</{tag}>"#)).ok()?;
    re.captures(xml)
        .and_then(|cap| cap.name("body"))
        .map(|m| clean_xml_fragment(m.as_str()))
        .filter(|s| !s.trim().is_empty())
}

fn clean_xml_fragment(fragment: &str) -> String {
    lazy_static! {
        static ref RE_TAGS: Regex = Regex::new(r#"(?is)<[^>]+>"#).expect("regex");
        static ref RE_WS: Regex = Regex::new(r#"\s+"#).expect("regex");
    }
    let without_tags = RE_TAGS.replace_all(fragment, " ");
    let decoded = decode_xml_text(without_tags.as_ref());
    RE_WS.replace_all(decoded.trim(), " ").to_string()
}

fn decode_xml_text(value: &str) -> String {
    value
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
}

fn parse_article_ids_json(value: Option<&Json>) -> Vec<PubmedArticleId> {
    value
        .and_then(Json::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let id_type = item.get("idtype").and_then(Json::as_str)?.to_string();
                    let value = item.get("value").and_then(Json::as_str)?.to_string();
                    Some(PubmedArticleId { id_type, value })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn clean_optional(value: &Option<String>) -> Option<String> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn string_field(map: &JsonMap<String, Json>, key: &str) -> String {
    map.get(key)
        .and_then(Json::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or_default()
        .to_string()
}

fn json_u64_from_string_or_number(value: &Json) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_str().and_then(|s| s.parse::<u64>().ok()))
}

fn publication_year_from_date(value: &str) -> Option<u32> {
    lazy_static! {
        static ref RE_YEAR: Regex = Regex::new(r#"\b(18|19|20|21)\d{2}\b"#).expect("regex");
    }
    RE_YEAR
        .find(value)
        .and_then(|m| m.as_str().parse::<u32>().ok())
}

fn truncate_for_error(body: &str) -> String {
    const LIMIT: usize = 512;
    if body.len() <= LIMIT {
        body.to_string()
    } else {
        format!("{}…", body.chars().take(LIMIT).collect::<String>())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_esearch_ids_and_count() {
        let value = json!({
            "esearchresult": {
                "count": "42",
                "idlist": ["123", "456"],
                "querytranslation": "\"cancer\"[All Fields]"
            }
        });

        let parsed = parse_esearch_result("cancer", 0, 2, &value).expect("parse esearch");
        assert_eq!(parsed.count, 42);
        assert_eq!(parsed.ids, vec!["123", "456"]);
        assert_eq!(
            parsed.query_translation.as_deref(),
            Some("\"cancer\"[All Fields]")
        );
    }

    #[test]
    fn parses_esummary_in_requested_id_order() {
        let value = json!({
            "result": {
                "uids": ["456", "123"],
                "123": {
                    "uid": "123",
                    "title": "First article",
                    "authors": [{"name": "Alice A"}, {"name": "Bob B"}],
                    "source": "Nature",
                    "fulljournalname": "Nature",
                    "pubdate": "2025 Jan",
                    "epubdate": "",
                    "volume": "1",
                    "issue": "2",
                    "pages": "3-4",
                    "articleids": [
                        {"idtype": "pubmed", "value": "123"},
                        {"idtype": "doi", "value": "10.1000/example"}
                    ],
                    "pubtype": ["Journal Article"]
                },
                "456": {
                    "uid": "456",
                    "title": "Second article",
                    "authors": [],
                    "source": "Cell",
                    "fulljournalname": "Cell",
                    "pubdate": "2024",
                    "articleids": [],
                    "pubtype": []
                }
            }
        });

        let ids = vec!["123".to_string(), "456".to_string()];
        let parsed = parse_esummary_result(&value, &ids);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].pmid, "123");
        assert_eq!(parsed[0].authors, "Alice A, Bob B");
        assert_eq!(parsed[0].doi.as_deref(), Some("10.1000/example"));
        assert_eq!(parsed[1].pmid, "456");
    }

    #[test]
    fn search_args_accept_aliases() {
        let args: PubmedSearchArgs =
            serde_json::from_value(json!({"query":"ido1","maxResults":5,"offset":3}))
                .expect("args");
        assert_eq!(args.query, "ido1");
        assert_eq!(args.normalized_max_results(), 5);
        assert_eq!(args.normalized_ret_start(), 3);
    }

    #[test]
    fn efetch_parser_extracts_structured_article() {
        let xml = r#"
        <PubmedArticleSet>
          <PubmedArticle>
            <MedlineCitation>
              <PMID>123</PMID>
              <Article>
                <Journal>
                  <JournalIssue><PubDate><Year>2024</Year><Month>Jan</Month></PubDate></JournalIssue>
                  <Title>Journal Full Name</Title>
                  <ISOAbbreviation>J Full</ISOAbbreviation>
                </Journal>
                <ArticleTitle>Example &amp; Trial</ArticleTitle>
                <Pagination><MedlinePgn>1-9</MedlinePgn></Pagination>
                <Abstract>
                  <AbstractText Label="BACKGROUND">Why it matters.</AbstractText>
                  <AbstractText>What was found.</AbstractText>
                </Abstract>
                <AuthorList>
                  <Author><ForeName>Alice</ForeName><LastName>Smith</LastName></Author>
                  <Author><CollectiveName>Consortium</CollectiveName></Author>
                </AuthorList>
                <PublicationTypeList><PublicationType>Journal Article</PublicationType></PublicationTypeList>
              </Article>
            </MedlineCitation>
            <PubmedData><ArticleIdList><ArticleId IdType="doi">10.1000/x</ArticleId></ArticleIdList></PubmedData>
          </PubmedArticle>
        </PubmedArticleSet>
        "#;
        let detail = parse_efetch_article(xml, "123").expect("parse efetch");
        assert_eq!(detail.pmid, "123");
        assert_eq!(detail.article_title, "Example & Trial");
        assert_eq!(detail.authors, vec!["Alice Smith", "Consortium"]);
        assert!(detail.abstract_text.contains("BACKGROUND: Why it matters."));
        assert_eq!(detail.doi.as_deref(), Some("10.1000/x"));
        assert_eq!(detail.publication_year, Some(2024));
    }

    #[test]
    fn pubmed_search_json_uses_serpapi_shape() {
        let response = PubmedSearchResponse {
            query: "ido1".into(),
            count: 1,
            ret_start: 0,
            ret_max: 1,
            query_translation: None,
            ids: vec!["123".into()],
            summaries: vec![PubmedArticleSummary {
                pmid: "123".into(),
                title: "Paper".into(),
                authors: "Alice".into(),
                author_list: vec!["Alice".into()],
                source: "Nature".into(),
                full_journal_name: "Nature".into(),
                pub_date: "2025".into(),
                epub_date: "".into(),
                volume: "".into(),
                issue: "".into(),
                pages: "".into(),
                doi: Some("10.1/x".into()),
                article_ids: vec![],
                pub_types: vec!["Journal Article".into()],
                pubmed_url: "https://pubmed.ncbi.nlm.nih.gov/123/".into(),
            }],
        };
        let json = search_response_to_json(&response);
        assert_eq!(json["results"][0]["position"], 1);
        assert_eq!(json["results"][0]["source"], "pubmed");
        assert_eq!(json["results"][0]["metadata"]["pmid"], "123");
    }
}
