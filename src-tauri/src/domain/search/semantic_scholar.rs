//! Semantic Scholar adapter backed by the official Academic Graph API.
//!
//! First-version support is search-only. It uses the paper bulk search endpoint
//! recommended by the Semantic Scholar tutorial and mirrored by the `allenai/s2-folks`
//! examples: `GET /graph/v1/paper/search/bulk` with `query`, `fields`, and `limit`.

use crate::domain::tools::{ToolContext, WebSearchApiKeys};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{json, Value as Json};
use std::time::Duration;

const DEFAULT_GRAPH_BASE_URL: &str = "https://api.semanticscholar.org/graph/v1";
const DEFAULT_MAX_RESULTS: u32 = 10;
const MAX_RESULTS_CAP: u32 = 100;
const SEMANTIC_SCHOLAR_FAVICON: &str = "https://www.semanticscholar.org/favicon.ico";
const DEFAULT_FIELDS: &str = "paperId,url,title,abstract,authors,year,venue,publicationTypes,publicationDate,externalIds,citationCount,influentialCitationCount,isOpenAccess,openAccessPdf,fieldsOfStudy";

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct SemanticScholarSearchArgs {
    #[serde(alias = "q")]
    pub query: String,
    #[serde(default, alias = "maxResults", alias = "limit")]
    pub max_results: Option<u32>,
    #[serde(default)]
    pub token: Option<String>,
}

impl SemanticScholarSearchArgs {
    pub fn normalized_max_results(&self) -> u32 {
        self.max_results
            .unwrap_or(DEFAULT_MAX_RESULTS)
            .clamp(1, MAX_RESULTS_CAP)
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SemanticScholarAuthor {
    pub author_id: Option<String>,
    pub name: String,
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SemanticScholarOpenAccessPdf {
    pub url: Option<String>,
    pub status: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SemanticScholarPaper {
    pub paper_id: String,
    pub title: String,
    pub url: String,
    pub abstract_text: Option<String>,
    pub authors: Vec<SemanticScholarAuthor>,
    pub year: Option<u32>,
    pub venue: Option<String>,
    pub publication_types: Vec<String>,
    pub publication_date: Option<String>,
    pub external_ids: Json,
    pub citation_count: Option<u64>,
    pub influential_citation_count: Option<u64>,
    pub is_open_access: Option<bool>,
    pub open_access_pdf: Option<SemanticScholarOpenAccessPdf>,
    pub fields_of_study: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SemanticScholarSearchResponse {
    pub query: String,
    pub total: Option<u64>,
    pub token: Option<String>,
    pub results: Vec<SemanticScholarPaper>,
}

#[derive(Clone)]
pub struct SemanticScholarClient {
    http: reqwest::Client,
    base_url: String,
    api_key: Option<String>,
}

impl SemanticScholarClient {
    pub fn from_tool_context(ctx: &ToolContext) -> Result<Self, String> {
        if !ctx.web_search_api_keys.semantic_scholar_enabled {
            return Err(
                "Semantic Scholar search is disabled. Enable it in Settings → Search and provide an API key."
                    .to_string(),
            );
        }
        let api_key = resolve_api_key(&ctx.web_search_api_keys).ok_or_else(|| {
            "Semantic Scholar search requires an API key. Configure one in Settings → Search."
                .to_string()
        })?;
        let mut builder = reqwest::Client::builder()
            .timeout(Duration::from_secs(ctx.timeout_secs.clamp(5, 60)))
            .user_agent(format!(
                "Omiga-SemanticScholar/{}",
                env!("CARGO_PKG_VERSION")
            ));
        if !ctx.web_use_proxy {
            builder = builder.no_proxy();
        }
        let http = builder
            .build()
            .map_err(|e| format!("build Semantic Scholar HTTP client: {e}"))?;
        Ok(Self {
            http,
            base_url: DEFAULT_GRAPH_BASE_URL.to_string(),
            api_key: Some(api_key),
        })
    }

    #[cfg(test)]
    pub fn with_base_url(base_url: impl Into<String>) -> Result<Self, String> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent(format!(
                "Omiga-SemanticScholar/{}",
                env!("CARGO_PKG_VERSION")
            ))
            .build()
            .map_err(|e| format!("build Semantic Scholar HTTP client: {e}"))?;
        Ok(Self {
            http,
            base_url: base_url.into().trim_end_matches('/').to_string(),
            api_key: None,
        })
    }

    pub async fn search(
        &self,
        args: SemanticScholarSearchArgs,
    ) -> Result<SemanticScholarSearchResponse, String> {
        if args.query.trim().is_empty() {
            return Err("Semantic Scholar query must not be empty".to_string());
        }

        let limit = args.normalized_max_results();
        let url = format!("{}/paper/search/bulk", self.base_url);
        let mut request = self.http.get(&url).query(&[
            ("query", args.query.trim().to_string()),
            ("fields", DEFAULT_FIELDS.to_string()),
            ("limit", limit.to_string()),
        ]);
        if let Some(token) = clean_optional(&args.token) {
            request = request.query(&[("token", token)]);
        }
        if let Some(api_key) = &self.api_key {
            request = request.header("x-api-key", api_key);
        }

        let response = request
            .send()
            .await
            .map_err(|e| format!("Semantic Scholar search request failed: {e}"))?;
        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| format!("read Semantic Scholar search response: {e}"))?;
        if !status.is_success() {
            return Err(format!(
                "Semantic Scholar search returned HTTP {}: {}",
                status.as_u16(),
                truncate_for_error(&body)
            ));
        }
        let raw: RawSemanticScholarSearchResponse = serde_json::from_str(&body).map_err(|e| {
            format!(
                "Semantic Scholar search returned invalid JSON: {e}; body: {}",
                truncate_for_error(&body)
            )
        })?;
        let mut parsed = raw.into_response(args.query.trim());
        parsed.results.truncate(limit as usize);
        Ok(parsed)
    }
}

pub fn search_response_to_json(response: &SemanticScholarSearchResponse) -> Json {
    let results: Vec<Json> = response
        .results
        .iter()
        .enumerate()
        .map(|(idx, item)| paper_to_serp_result(item, idx + 1))
        .collect();
    json!({
        "query": response.query,
        "category": "literature",
        "source": "semantic_scholar",
        "effective_source": "semantic_scholar",
        "count": response.total,
        "next_token": response.token,
        "results": results,
    })
}

fn paper_to_serp_result(item: &SemanticScholarPaper, position: usize) -> Json {
    let authors: Vec<String> = item
        .authors
        .iter()
        .map(|a| a.name.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let doi = external_id(&item.external_ids, "DOI");
    let arxiv_id = external_id(&item.external_ids, "ArXiv");
    let pubmed_id = external_id(&item.external_ids, "PubMed");
    json!({
        "position": position,
        "category": "literature",
        "source": "semantic_scholar",
        "title": item.title,
        "name": item.title,
        "link": item.url,
        "url": item.url,
        "displayed_link": displayed_link_for_url(&item.url),
        "favicon": SEMANTIC_SCHOLAR_FAVICON,
        "snippet": semantic_scholar_snippet(item, &authors),
        "id": item.paper_id,
        "authors": authors,
        "metadata": {
            "paper_id": item.paper_id,
            "doi": doi,
            "arxiv_id": arxiv_id,
            "pubmed_id": pubmed_id,
            "authors": item.authors,
            "year": item.year,
            "venue": item.venue,
            "publication_types": item.publication_types,
            "publication_date": item.publication_date,
            "external_ids": item.external_ids,
            "citation_count": item.citation_count,
            "influential_citation_count": item.influential_citation_count,
            "is_open_access": item.is_open_access,
            "open_access_pdf": item.open_access_pdf,
            "fields_of_study": item.fields_of_study,
            "abstract": item.abstract_text,
        }
    })
}

fn semantic_scholar_snippet(item: &SemanticScholarPaper, authors: &[String]) -> String {
    if let Some(abstract_text) = item
        .abstract_text
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return truncate_chars(abstract_text, 512);
    }
    [
        (!authors.is_empty()).then(|| authors.join(", ")),
        item.venue.clone(),
        item.year.map(|y| y.to_string()),
        (!item.fields_of_study.is_empty()).then(|| item.fields_of_study.join(", ")),
    ]
    .into_iter()
    .flatten()
    .map(|s| s.trim().to_string())
    .filter(|s| !s.is_empty())
    .collect::<Vec<_>>()
    .join(" | ")
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

fn external_id(external_ids: &Json, key: &str) -> Option<String> {
    external_ids
        .get(key)
        .and_then(Json::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn resolve_api_key(keys: &WebSearchApiKeys) -> Option<String> {
    clean_optional(&keys.semantic_scholar_api_key)
        .or_else(|| clean_optional(&std::env::var("OMIGA_SEMANTIC_SCHOLAR_API_KEY").ok()))
        .or_else(|| clean_optional(&std::env::var("SEMANTIC_SCHOLAR_API_KEY").ok()))
        .or_else(|| clean_optional(&std::env::var("S2_API_KEY").ok()))
}

fn clean_optional(value: &Option<String>) -> Option<String> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn clean_string(value: String) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn truncate_for_error(body: &str) -> String {
    truncate_chars(body, 300)
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for (count, ch) in value.chars().enumerate() {
        if count >= max_chars {
            out.push('…');
            return out;
        }
        out.push(ch);
    }
    out
}

fn null_to_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawSemanticScholarSearchResponse {
    total: Option<u64>,
    token: Option<String>,
    #[serde(default, deserialize_with = "null_to_default")]
    data: Vec<RawSemanticScholarPaper>,
}

impl RawSemanticScholarSearchResponse {
    fn into_response(self, query: &str) -> SemanticScholarSearchResponse {
        let results = self.data.into_iter().map(Into::into).collect();
        SemanticScholarSearchResponse {
            query: query.to_string(),
            total: self.total,
            token: clean_optional(&self.token),
            results,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawSemanticScholarPaper {
    paper_id: Option<String>,
    url: Option<String>,
    title: Option<String>,
    #[serde(rename = "abstract")]
    abstract_text: Option<String>,
    #[serde(default, deserialize_with = "null_to_default")]
    authors: Vec<RawSemanticScholarAuthor>,
    year: Option<u32>,
    venue: Option<String>,
    #[serde(default, deserialize_with = "null_to_default")]
    publication_types: Vec<String>,
    publication_date: Option<String>,
    #[serde(default)]
    external_ids: Json,
    citation_count: Option<u64>,
    influential_citation_count: Option<u64>,
    is_open_access: Option<bool>,
    open_access_pdf: Option<RawSemanticScholarOpenAccessPdf>,
    #[serde(default, deserialize_with = "null_to_default")]
    fields_of_study: Vec<String>,
}

impl From<RawSemanticScholarPaper> for SemanticScholarPaper {
    fn from(raw: RawSemanticScholarPaper) -> Self {
        let paper_id = raw.paper_id.unwrap_or_default();
        let url = raw
            .url
            .and_then(clean_string)
            .unwrap_or_else(|| format!("https://www.semanticscholar.org/paper/{paper_id}"));
        Self {
            paper_id,
            title: raw.title.unwrap_or_default(),
            url,
            abstract_text: raw.abstract_text.and_then(clean_string),
            authors: raw
                .authors
                .into_iter()
                .filter_map(RawSemanticScholarAuthor::into_author)
                .collect(),
            year: raw.year,
            venue: raw.venue.and_then(clean_string),
            publication_types: raw.publication_types,
            publication_date: raw.publication_date.and_then(clean_string),
            external_ids: raw.external_ids,
            citation_count: raw.citation_count,
            influential_citation_count: raw.influential_citation_count,
            is_open_access: raw.is_open_access,
            open_access_pdf: raw.open_access_pdf.map(Into::into),
            fields_of_study: raw.fields_of_study,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawSemanticScholarAuthor {
    author_id: Option<String>,
    name: Option<String>,
    url: Option<String>,
}

impl RawSemanticScholarAuthor {
    fn into_author(self) -> Option<SemanticScholarAuthor> {
        let name = self.name.and_then(clean_string)?;
        Some(SemanticScholarAuthor {
            author_id: self.author_id.and_then(clean_string),
            name,
            url: self.url.and_then(clean_string),
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawSemanticScholarOpenAccessPdf {
    url: Option<String>,
    status: Option<String>,
}

impl From<RawSemanticScholarOpenAccessPdf> for SemanticScholarOpenAccessPdf {
    fn from(raw: RawSemanticScholarOpenAccessPdf) -> Self {
        Self {
            url: raw.url.and_then(clean_string),
            status: raw.status.and_then(clean_string),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_response() -> &'static str {
        r#"{
            "total": 345,
            "token": "next-token",
            "data": [
                {
                    "paperId": "001720a782840652b573bb4794774aee826510ca",
                    "url": "https://www.semanticscholar.org/paper/001720a782840652b573bb4794774aee826510ca",
                    "title": "Developing Design Features to Facilitate AI-Assisted User Interactions",
                    "abstract": "This paper studies AI-assisted user interactions.",
                    "authors": [{"authorId":"1", "name":"Alice Example"}, {"name":"Bob Example"}],
                    "year": 2024,
                    "venue": "CHI",
                    "publicationTypes": ["Conference"],
                    "publicationDate": "2024-05-03",
                    "externalIds": {"DOI":"10.1234/example", "ArXiv":"2401.00001", "PubMed":"123456"},
                    "citationCount": 7,
                    "influentialCitationCount": 1,
                    "isOpenAccess": true,
                    "openAccessPdf": {"url":"https://example.org/paper.pdf", "status":"GOLD"},
                    "fieldsOfStudy": ["Computer Science"]
                },
                {
                    "paperId": "no-abstract",
                    "url": null,
                    "title": "Null fields are tolerated",
                    "abstract": null,
                    "authors": null,
                    "publicationTypes": null,
                    "fieldsOfStudy": null,
                    "externalIds": null
                }
            ]
        }"#
    }

    #[test]
    fn parses_bulk_search_response_and_null_arrays() {
        let raw: RawSemanticScholarSearchResponse =
            serde_json::from_str(sample_response()).unwrap();
        let response = raw.into_response("AI interaction");
        assert_eq!(response.total, Some(345));
        assert_eq!(response.token.as_deref(), Some("next-token"));
        assert_eq!(response.results.len(), 2);
        assert_eq!(response.results[0].authors.len(), 2);
        assert_eq!(response.results[0].year, Some(2024));
        assert_eq!(response.results[1].authors.len(), 0);
        assert!(response.results[1].url.ends_with("/paper/no-abstract"));
    }

    #[test]
    fn semantic_scholar_json_uses_serpapi_shape() {
        let raw: RawSemanticScholarSearchResponse =
            serde_json::from_str(sample_response()).unwrap();
        let response = raw.into_response("AI interaction");
        let json = search_response_to_json(&response);
        assert_eq!(json["category"], "literature");
        assert_eq!(json["source"], "semantic_scholar");
        assert_eq!(json["results"][0]["position"], 1);
        assert_eq!(
            json["results"][0]["id"],
            "001720a782840652b573bb4794774aee826510ca"
        );
        assert_eq!(json["results"][0]["metadata"]["doi"], "10.1234/example");
        assert_eq!(json["results"][0]["authors"][0], "Alice Example");
        assert!(json["results"][0]["favicon"]
            .as_str()
            .unwrap()
            .contains("semanticscholar"));
    }

    #[test]
    fn search_args_accept_aliases() {
        let args: SemanticScholarSearchArgs =
            serde_json::from_str(r#"{"q":"gene therapy","maxResults":3,"token":"abc"}"#).unwrap();
        assert_eq!(args.query, "gene therapy");
        assert_eq!(args.normalized_max_results(), 3);
        assert_eq!(args.token.as_deref(), Some("abc"));
    }

    #[test]
    fn client_requires_explicit_enablement() {
        let ctx = ToolContext::new(std::env::temp_dir());
        let err = match SemanticScholarClient::from_tool_context(&ctx) {
            Ok(_) => panic!("client should require explicit enablement"),
            Err(err) => err,
        };
        assert!(err.contains("disabled"));
    }
}
