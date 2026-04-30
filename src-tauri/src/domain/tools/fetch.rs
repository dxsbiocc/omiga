//! Unified fetch tool — retrieve details for web pages or source-specific records.
//!
//! The public model-visible function is `fetch`; first-version adapters are:
//! - `category="web"`: safe public HTTP(S) fetch and text extraction.
//! - `category="literature", source="pubmed"`: official NCBI EFetch by PMID.
//! - `category="literature", source="arxiv|crossref|openalex|biorxiv|medrxiv"`:
//!   public source metadata fetch by source id/DOI/arXiv/OpenAlex URL.

use super::{ToolContext, ToolError, ToolSchema};
use crate::infrastructure::streaming::{StreamOutput, StreamOutputItem};
use async_trait::async_trait;
use lazy_static::lazy_static;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};
use std::io::Cursor;
use std::pin::Pin;
use std::time::{Duration, Instant};

const MAX_BODY_BYTES: u64 = 10 * 1024 * 1024;
const MAX_TEXT_CHARS: usize = 100_000;
const BROWSER_FETCH_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";
const PUBMED_HOST: &str = "pubmed.ncbi.nlm.nih.gov";

pub const DESCRIPTION: &str = r#"Fetch one document/detail from a typed data source and return formatted JSON.

- `category` is required. Categories: `literature`, `dataset` (`data` alias), `knowledge` (use `recall` for search), `web`, `social`.
- `source` is optional and defaults to `auto`. Concrete sources: `web.auto`, `literature.pubmed|arxiv|crossref|openalex|biorxiv|medrxiv|semantic_scholar`, `dataset.geo|ena|ena_run|ena_experiment|ena_sample|ena_analysis|ena_assembly|ena_sequence`, optional `social.wechat`.
- `subcategory` is optional for dataset routing: `expression`, `sequencing`, `genomics`, `sample_metadata`, `multi_omics`.
- Locate the document with one of: `url`, `id` + `source`, or a full `result` object returned by `search`.
- `web` fetch sends a safe public HTTP(S) GET, follows public-safe redirects, blocks private/loopback targets, converts HTML to text, and pretty-prints JSON.
- `literature.pubmed` fetch expects a numeric PMID in `id` (or a PubMed URL / search result) and uses official NCBI EFetch.
- Public literature sources fetch source-specific metadata records: arXiv by arXiv id/URL, Crossref/bioRxiv/medRxiv by DOI, and OpenAlex by work id/URL/DOI.
- `literature.semantic_scholar` is opt-in and requires the user-enabled API key; it fetches by Semantic Scholar paper id or supported external id prefix.
- `data.geo` fetches GEO DataSets/Series/Samples/Platforms by UID or GEO accession via official NCBI E-utilities; `data.ena*` fetches ENA study/run/experiment/sample/analysis/assembly/sequence metadata by accession via ENA Portal/Browser APIs.
- `social.wechat` is disabled by default; when enabled it fetches the article URL with the safe web fetcher.
- Results are returned as formatted JSON with `title`, `link`, `url`, `favicon`, `content`, and `metadata`."#;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FetchArgs {
    pub category: String,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default, alias = "subCategory", alias = "dataset_type", alias = "type")]
    pub subcategory: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub result: Option<JsonValue>,
    #[serde(default)]
    pub prompt: Option<String>,
}

pub struct FetchTool;

fn browser_fetch_headers() -> reqwest::header::HeaderMap {
    let mut h = reqwest::header::HeaderMap::new();
    h.insert(
        reqwest::header::USER_AGENT,
        reqwest::header::HeaderValue::from_static(BROWSER_FETCH_USER_AGENT),
    );
    h.insert(
        reqwest::header::ACCEPT,
        reqwest::header::HeaderValue::from_static(
            "text/html,application/xhtml+xml,application/xml;q=0.9,application/json;q=0.8,*/*;q=0.7",
        ),
    );
    h.insert(
        reqwest::header::HeaderName::from_static("accept-language"),
        reqwest::header::HeaderValue::from_static("en-US,en;q=0.9"),
    );
    h
}

fn clean_inline_base64_images(text: &str) -> String {
    lazy_static! {
        static ref RE_BASE64_PARENS: Regex =
            Regex::new(r"\(data:image/[^;]+;base64,[A-Za-z0-9+/=]+\)").expect("regex");
        static ref RE_BASE64_PLAIN: Regex =
            Regex::new(r"data:image/[^;]+;base64,[A-Za-z0-9+/=]+").expect("regex");
    }
    let without_parens = RE_BASE64_PARENS.replace_all(text, "[image omitted]");
    RE_BASE64_PLAIN
        .replace_all(without_parens.as_ref(), "[image omitted]")
        .to_string()
}

#[async_trait]
impl super::ToolImpl for FetchTool {
    type Args = FetchArgs;

    const DESCRIPTION: &'static str = DESCRIPTION;

    async fn execute(
        ctx: &ToolContext,
        args: Self::Args,
    ) -> Result<crate::infrastructure::streaming::StreamOutputBox, ToolError> {
        let category = normalized_category(&args.category);
        let source = normalized_source(args.source.as_deref());
        match category.as_str() {
            "web" => fetch_web(ctx, &args, &source).await,
            "literature" => match resolve_literature_source(&args, &source).as_str() {
                "pubmed" => fetch_pubmed(ctx, &args).await,
                "semantic_scholar" | "semanticscholar" => {
                    if !ctx.web_search_api_keys.semantic_scholar_enabled {
                        return Ok(json_stream(structured_error_json(
                            "source_disabled",
                            "literature",
                            &source,
                            "literature.semantic_scholar is disabled. Enable it and configure an API key in Settings → Search.",
                        )));
                    }
                    fetch_semantic_scholar(ctx, &args).await
                }
                public_source
                    if crate::domain::search::literature::PublicLiteratureSource::parse(
                        public_source,
                    )
                    .is_some() =>
                {
                    fetch_public_literature(ctx, &args, public_source).await
                }
                other => Err(ToolError::InvalidArguments {
                    message: format!("Unsupported literature fetch source: {other}"),
                }),
            },
            "data" => match resolve_data_source(&args, &source).as_str() {
                data_source
                    if crate::domain::search::data::PublicDataSource::parse(data_source)
                        .is_some() =>
                {
                    fetch_public_data(ctx, &args, data_source).await
                }
                other => Err(ToolError::InvalidArguments {
                    message: format!("Unsupported data fetch source: {other}"),
                }),
            },
            "knowledge" => Err(ToolError::InvalidArguments {
                message:
                    "fetch(category=knowledge) is not supported; use recall(query=...) or search(category=knowledge)."
                        .to_string(),
            }),
            "social" => match source.as_str() {
                "auto" | "wechat" => {
                    if !ctx.web_search_api_keys.wechat_search_enabled {
                        return Ok(json_stream(structured_error_json(
                            "source_disabled",
                            "social",
                            &source,
                            "social.wechat is disabled. Enable WeChat public-account search in Settings → Search.",
                        )));
                    }
                    fetch_web(ctx, &args, &source).await
                }
                other => Err(ToolError::InvalidArguments {
                    message: format!("Unsupported social fetch source: {other}"),
                }),
            },
            other => Err(ToolError::InvalidArguments {
                message: format!("Unsupported fetch category: {other}"),
            }),
        }
    }
}

async fn fetch_public_data(
    ctx: &ToolContext,
    args: &FetchArgs,
    source: &str,
) -> Result<crate::infrastructure::streaming::StreamOutputBox, ToolError> {
    let source = crate::domain::search::data::PublicDataSource::parse(source).ok_or_else(|| {
        ToolError::InvalidArguments {
            message: format!("Unsupported public data source: {source}"),
        }
    })?;
    let identifier = resolve_data_identifier(args).ok_or_else(|| ToolError::InvalidArguments {
        message: format!(
            "fetch(category=data, source={}) requires `id`, `url`, accession, or a search `result`",
            source.as_str()
        ),
    })?;
    let client = crate::domain::search::data::PublicDataClient::from_tool_context(ctx)
        .map_err(|message| ToolError::ExecutionFailed { message })?;
    let record = tokio::select! {
        _ = ctx.cancel.cancelled() => return Err(ToolError::Cancelled),
        r = client.fetch(source, &identifier) => r.map_err(|message| ToolError::ExecutionFailed { message })?,
    };
    Ok(json_stream(crate::domain::search::data::detail_to_json(
        &record,
    )))
}

async fn fetch_public_literature(
    ctx: &ToolContext,
    args: &FetchArgs,
    source: &str,
) -> Result<crate::infrastructure::streaming::StreamOutputBox, ToolError> {
    let source = crate::domain::search::literature::PublicLiteratureSource::parse(source)
        .ok_or_else(|| ToolError::InvalidArguments {
            message: format!("Unsupported public literature source: {source}"),
        })?;
    let identifier = resolve_literature_identifier(args, source.as_str()).ok_or_else(|| {
        ToolError::InvalidArguments {
            message: format!(
                "fetch(category=literature, source={}) requires `id`, `url`, DOI/arXiv/OpenAlex identifier, or a search `result`",
                source.as_str()
            ),
        }
    })?;
    let client = crate::domain::search::literature::PublicLiteratureClient::from_tool_context(ctx)
        .map_err(|message| ToolError::ExecutionFailed { message })?;
    let paper = tokio::select! {
        _ = ctx.cancel.cancelled() => return Err(ToolError::Cancelled),
        r = client.fetch(source, &identifier) => r.map_err(|message| ToolError::ExecutionFailed { message })?,
    };
    Ok(json_stream(
        crate::domain::search::literature::paper_to_detail_json(&paper),
    ))
}

async fn fetch_semantic_scholar(
    ctx: &ToolContext,
    args: &FetchArgs,
) -> Result<crate::infrastructure::streaming::StreamOutputBox, ToolError> {
    let paper_id = resolve_semantic_scholar_id(args).ok_or_else(|| {
        ToolError::InvalidArguments {
            message: "Semantic Scholar fetch requires a paper id, DOI/arXiv/PubMed external id, URL, or search result".to_string(),
        }
    })?;
    let client =
        crate::domain::search::semantic_scholar::SemanticScholarClient::from_tool_context(ctx)
            .map_err(|message| ToolError::ExecutionFailed { message })?;
    let paper = tokio::select! {
        _ = ctx.cancel.cancelled() => return Err(ToolError::Cancelled),
        r = client.fetch(&paper_id) => r.map_err(|message| ToolError::ExecutionFailed { message })?,
    };
    Ok(json_stream(
        crate::domain::search::semantic_scholar::detail_to_json(&paper),
    ))
}

async fn fetch_pubmed(
    ctx: &ToolContext,
    args: &FetchArgs,
) -> Result<crate::infrastructure::streaming::StreamOutputBox, ToolError> {
    let pmid = resolve_pubmed_pmid(args).ok_or_else(|| ToolError::InvalidArguments {
        message: "PubMed fetch expects a numeric PMID via `id`, a PubMed `url`, or a PubMed search `result`. DOI-to-PMID resolution is planned for a later version.".to_string(),
    })?;
    let client = crate::domain::search::pubmed::EntrezClient::from_tool_context(ctx)
        .map_err(|message| ToolError::ExecutionFailed { message })?;
    let detail = tokio::select! {
        _ = ctx.cancel.cancelled() => return Err(ToolError::Cancelled),
        r = client.fetch_by_pmid(&pmid) => r.map_err(|message| ToolError::ExecutionFailed { message })?,
    };
    Ok(json_stream(crate::domain::search::pubmed::detail_to_json(
        &detail,
    )))
}

async fn fetch_web(
    ctx: &ToolContext,
    args: &FetchArgs,
    requested_source: &str,
) -> Result<crate::infrastructure::streaming::StreamOutputBox, ToolError> {
    let start = Instant::now();
    let url = resolve_url(args).ok_or_else(|| ToolError::InvalidArguments {
        message: "fetch(category=web) requires `url` or a search `result` with `url`/`link`"
            .to_string(),
    })?;

    let parsed = reqwest::Url::parse(&url).map_err(|e| ToolError::InvalidArguments {
        message: format!("Invalid URL: {}", e),
    })?;

    match parsed.scheme() {
        "http" | "https" => {}
        other => {
            return Err(ToolError::InvalidArguments {
                message: format!("Unsupported URL scheme: {}", other),
            })
        }
    }

    super::web_safety::validate_public_http_url(&ctx.project_root, url.trim(), true)
        .map_err(|message| ToolError::InvalidArguments { message })?;

    let timeout = Duration::from_secs(ctx.timeout_secs.clamp(5, 120));

    let project_root_for_redirect = ctx.project_root.clone();
    let mut client_builder = reqwest::Client::builder()
        .timeout(timeout)
        .redirect(reqwest::redirect::Policy::custom(move |attempt| {
            if attempt.previous().len() >= 10 {
                return attempt.error("Too many redirects");
            }
            match super::web_safety::validate_public_http_url(
                &project_root_for_redirect,
                attempt.url().as_str(),
                false,
            ) {
                Ok(_) => attempt.follow(),
                Err(message) => attempt.error(message),
            }
        }))
        .default_headers(browser_fetch_headers());
    if !ctx.web_use_proxy {
        client_builder = client_builder.no_proxy();
    }
    let client = client_builder
        .build()
        .map_err(|e| ToolError::ExecutionFailed {
            message: format!("HTTP client: {}", e),
        })?;

    let send_fut = client.get(url.clone()).send();

    let response = tokio::select! {
        _ = ctx.cancel.cancelled() => return Err(ToolError::Cancelled),
        res = send_fut => res.map_err(|e| ToolError::ExecutionFailed {
            message: format!("HTTP error: {}", e),
        })?,
    };

    let status = response.status();
    let code = status.as_u16();
    let code_text = status.canonical_reason().unwrap_or("Unknown").to_string();

    let final_url = response.url().to_string();
    super::web_safety::validate_public_http_url(&ctx.project_root, &final_url, true)
        .map_err(|message| ToolError::ExecutionFailed { message })?;
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_string();

    let content_length = response.content_length();

    let body_bytes = tokio::select! {
        _ = ctx.cancel.cancelled() => return Err(ToolError::Cancelled),
        b = response.bytes() => b.map_err(|e| ToolError::ExecutionFailed {
            message: format!("Read body: {}", e),
        })?,
    };

    if body_bytes.len() as u64 > MAX_BODY_BYTES {
        return Err(ToolError::ExecutionFailed {
            message: format!(
                "Response body too large ({} bytes, max {} bytes)",
                body_bytes.len(),
                MAX_BODY_BYTES
            ),
        });
    }

    let ct_lower = content_type.to_ascii_lowercase();
    let sniff_html = sniff_likely_html(&body_bytes);
    let text = if ct_lower.contains("html") || sniff_html {
        html2text::from_read(Cursor::new(body_bytes.as_ref()), 120).map_err(|e| {
            ToolError::ExecutionFailed {
                message: format!("HTML conversion: {}", e),
            }
        })?
    } else if ct_lower.contains("json") {
        let s = String::from_utf8(body_bytes.to_vec()).map_err(|_| ToolError::ExecutionFailed {
            message: "Response body is not valid UTF-8".to_string(),
        })?;
        serde_json::from_str::<serde_json::Value>(&s)
            .ok()
            .and_then(|v| serde_json::to_string_pretty(&v).ok())
            .unwrap_or(s)
    } else if ct_lower.starts_with("text/")
        || ct_lower.contains("javascript")
        || ct_lower.contains("xml")
    {
        String::from_utf8(body_bytes.to_vec()).map_err(|_| ToolError::ExecutionFailed {
            message: "Text response is not valid UTF-8".to_string(),
        })?
    } else {
        return Err(ToolError::ExecutionFailed {
            message: format!(
                "Unsupported content type for text extraction: {}. Try a URL that returns HTML, JSON, or plain text.",
                content_type
            ),
        });
    };

    let cleaned = clean_inline_base64_images(&text);
    let (content, truncated_note) = truncate_chars(&cleaned, MAX_TEXT_CHARS);
    let title = title_from_result(args).unwrap_or_else(|| final_url.clone());
    let value = json!({
        "category": "web",
        "source": requested_source,
        "effective_source": "http",
        "title": title,
        "name": title,
        "link": final_url,
        "url": final_url,
        "requested_url": url,
        "displayed_link": displayed_link_for_url(&final_url),
        "favicon": favicon_for_url(&final_url),
        "status": code,
        "status_text": code_text,
        "content_type": content_type,
        "content_length": content_length,
        "duration_ms": start.elapsed().as_millis(),
        "prompt": args.prompt.as_deref().unwrap_or(""),
        "content": content,
        "truncated_note": truncated_note,
        "metadata": {
            "bytes_decoded_text": text.len(),
        }
    });
    Ok(json_stream(value))
}

fn normalized_category(value: &str) -> String {
    match value.trim().to_ascii_lowercase().replace('-', "_").as_str() {
        "dataset" | "datasets" => "data".to_string(),
        "knowledge_base" | "kb" | "memory" => "knowledge".to_string(),
        other => other.to_string(),
    }
}

fn normalized_source(value: Option<&str>) -> String {
    value
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("auto")
        .to_ascii_lowercase()
        .replace('-', "_")
}

fn normalized_subcategory(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_ascii_lowercase().replace(['-', ' '], "_"))
}

fn data_source_for_subcategory(subcategory: Option<&str>) -> Option<&'static str> {
    match subcategory? {
        "expression" | "gene_expression" | "transcriptomics" | "transcriptome" => Some("geo"),
        "sequencing" | "sequence_reads" | "raw_reads" | "reads" | "sra" => Some("ena_run"),
        "genomics" | "genome" | "genomes" | "assembly" | "assemblies" => Some("ena_assembly"),
        "sample_metadata" | "sample" | "samples" | "metadata" => Some("ena_sample"),
        _ => None,
    }
}

fn resolve_url(args: &FetchArgs) -> Option<String> {
    args.url
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| string_from_result(args, &["url", "link", "href"]))
}

fn title_from_result(args: &FetchArgs) -> Option<String> {
    string_from_result(args, &["title", "name"])
}

fn string_from_result(args: &FetchArgs, keys: &[&str]) -> Option<String> {
    let object = args.result.as_ref()?.as_object()?;
    for key in keys {
        let value = object
            .get(*key)
            .and_then(JsonValue::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty());
        if let Some(value) = value {
            return Some(value.to_string());
        }
    }
    None
}

fn resolve_pubmed_pmid(args: &FetchArgs) -> Option<String> {
    let raw = args
        .id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| string_from_result(args, &["id", "pmid"]))
        .or_else(|| {
            args.result
                .as_ref()
                .and_then(|v| v.get("metadata"))
                .and_then(|m| m.get("pmid"))
                .and_then(JsonValue::as_str)
                .map(str::to_string)
        })
        .or_else(|| resolve_url(args).and_then(|url| pmid_from_pubmed_url(&url)))?;
    let trimmed = raw.trim().trim_start_matches("PMID:").trim();
    trimmed
        .chars()
        .all(|c| c.is_ascii_digit())
        .then(|| trimmed.to_string())
}

fn resolve_literature_source(args: &FetchArgs, requested_source: &str) -> String {
    if requested_source != "auto" {
        return requested_source.to_string();
    }
    if let Some(source) = string_from_result(args, &["source", "effective_source"])
        .map(|s| normalized_source(Some(&s)))
        .filter(|s| s != "auto")
    {
        return source;
    }
    if resolve_pubmed_pmid(args).is_some() {
        return "pubmed".to_string();
    }
    if let Some(id) = args.id.as_deref().and_then(clean_nonempty) {
        if looks_like_arxiv_identifier(&id) {
            return "arxiv".to_string();
        }
        if looks_like_openalex_identifier(&id) {
            return "openalex".to_string();
        }
        if looks_like_doi_identifier(&id) {
            return "crossref".to_string();
        }
    }
    if let Some(url) = resolve_url(args) {
        let lower = url.to_ascii_lowercase();
        if lower.contains("arxiv.org/") {
            return "arxiv".to_string();
        }
        if lower.contains("openalex.org/") {
            return "openalex".to_string();
        }
        if lower.contains("biorxiv.org/") {
            return "biorxiv".to_string();
        }
        if lower.contains("medrxiv.org/") {
            return "medrxiv".to_string();
        }
        if lower.contains("doi.org/") {
            return "crossref".to_string();
        }
    }
    if let Some(arxiv_id) = metadata_string_from_result(args, &["arxiv_id", "arxiv"]) {
        if !arxiv_id.is_empty() {
            return "arxiv".to_string();
        }
    }
    if let Some(doi) = metadata_string_from_result(args, &["doi"]) {
        if !doi.is_empty() {
            return "crossref".to_string();
        }
    }
    "pubmed".to_string()
}

fn resolve_literature_identifier(args: &FetchArgs, source: &str) -> Option<String> {
    let source = normalized_source(Some(source));
    match source.as_str() {
        "pubmed" => resolve_pubmed_pmid(args),
        "arxiv" => args
            .id
            .as_deref()
            .and_then(clean_nonempty)
            .or_else(|| metadata_string_from_result(args, &["arxiv_id", "arxiv"]))
            .or_else(|| string_from_result(args, &["id"]))
            .or_else(|| resolve_url(args)),
        "openalex" => metadata_string_from_result(args, &["openalex_id", "openalex"])
            .or_else(|| args.id.as_deref().and_then(clean_nonempty))
            .or_else(|| string_from_result(args, &["id"]))
            .or_else(|| metadata_string_from_result(args, &["doi"]))
            .or_else(|| resolve_url(args)),
        "crossref" | "biorxiv" | "medrxiv" => metadata_string_from_result(args, &["doi"])
            .or_else(|| args.id.as_deref().and_then(clean_nonempty))
            .or_else(|| string_from_result(args, &["id"]))
            .or_else(|| resolve_url(args)),
        _ => args
            .id
            .as_deref()
            .and_then(clean_nonempty)
            .or_else(|| string_from_result(args, &["id"]))
            .or_else(|| resolve_url(args)),
    }
}

fn resolve_data_source(args: &FetchArgs, requested_source: &str) -> String {
    if requested_source != "auto" {
        return requested_source.to_string();
    }
    if let Some(source) = string_from_result(args, &["source", "effective_source"])
        .map(|s| normalized_source(Some(&s)))
        .filter(|s| s != "auto")
    {
        if crate::domain::search::data::PublicDataSource::parse(&source).is_some() {
            return source;
        }
    }
    if let Some(value) = resolve_data_identifier(args) {
        if crate::domain::search::data::looks_like_geo_accession(&value) {
            return "geo".to_string();
        }
        if let Some(source) = crate::domain::search::data::inferred_ena_source_key(&value) {
            return source.to_string();
        }
    }
    if let Some(url) = resolve_url(args) {
        let lower = url.to_ascii_lowercase();
        if lower.contains("ncbi.nlm.nih.gov/geo") || lower.contains("ncbi.nlm.nih.gov/gds") {
            return "geo".to_string();
        }
        if lower.contains("ebi.ac.uk/ena") {
            return "ena".to_string();
        }
    }
    if let Some(source) =
        data_source_for_subcategory(normalized_subcategory(args.subcategory.as_deref()).as_deref())
    {
        return source.to_string();
    }
    "geo".to_string()
}

fn resolve_data_identifier(args: &FetchArgs) -> Option<String> {
    args.id
        .as_deref()
        .and_then(clean_nonempty)
        .or_else(|| {
            metadata_string_from_result(args, &["accession", "geo_accession", "ena_accession"])
        })
        .or_else(|| string_from_result(args, &["accession", "id"]))
        .or_else(|| resolve_url(args))
}

fn resolve_semantic_scholar_id(args: &FetchArgs) -> Option<String> {
    if let Some(id) = args.id.as_deref().and_then(clean_nonempty) {
        return Some(normalize_semantic_scholar_id(&id));
    }
    if let Some(paper_id) = metadata_string_from_result(args, &["paper_id", "paperId"]) {
        return Some(paper_id);
    }
    if let Some(id) = string_from_result(args, &["id"]) {
        if !id.trim().is_empty() {
            return Some(normalize_semantic_scholar_id(&id));
        }
    }
    if let Some(url) = resolve_url(args) {
        if let Some(id) = semantic_scholar_id_from_url(&url) {
            return Some(id);
        }
    }
    if let Some(doi) = metadata_string_from_result(args, &["doi"]) {
        return Some(format!("DOI:{}", strip_doi_prefix(&doi)));
    }
    if let Some(arxiv) = metadata_string_from_result(args, &["arxiv_id", "arxiv"]) {
        return Some(format!("ARXIV:{arxiv}"));
    }
    if let Some(pmid) = metadata_string_from_result(args, &["pubmed_id", "pmid"]) {
        return Some(format!("PMID:{pmid}"));
    }
    None
}

fn normalize_semantic_scholar_id(value: &str) -> String {
    let value = value.trim();
    if let Some(id) = semantic_scholar_id_from_url(value) {
        return id;
    }
    let lower = value.to_ascii_lowercase();
    if lower.starts_with("https://doi.org/") || lower.starts_with("http://doi.org/") {
        return format!("DOI:{}", strip_doi_prefix(value));
    }
    if lower.starts_with("doi:") {
        return format!("DOI:{}", strip_doi_prefix(value));
    }
    if lower.starts_with("arxiv:") {
        return format!("ARXIV:{}", value["arxiv:".len()..].trim());
    }
    if lower.starts_with("pmid:") {
        return format!("PMID:{}", value["pmid:".len()..].trim());
    }
    if !value.contains(':') && value.contains('/') && value.starts_with("10.") {
        return format!("DOI:{}", strip_doi_prefix(value));
    }
    value.to_string()
}

fn semantic_scholar_id_from_url(url: &str) -> Option<String> {
    let parsed = reqwest::Url::parse(url).ok()?;
    let host = parsed.host_str()?.to_ascii_lowercase();
    if !host.ends_with("semanticscholar.org") {
        return None;
    }
    let segments = parsed
        .path_segments()?
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    segments
        .iter()
        .position(|segment| *segment == "paper")
        .and_then(|idx| segments.get(idx + 1..))
        .and_then(|remaining| remaining.last().or_else(|| remaining.first()))
        .map(|s| s.to_string())
}

fn strip_doi_prefix(value: &str) -> String {
    let trimmed = value.trim();
    let lower = trimmed.to_ascii_lowercase();
    for prefix in ["https://doi.org/", "http://doi.org/", "doi:"] {
        if lower.starts_with(prefix) {
            return trimmed[prefix.len()..].trim().to_string();
        }
    }
    trimmed.to_string()
}

fn looks_like_doi_identifier(value: &str) -> bool {
    let value = value.trim();
    let lower = value.to_ascii_lowercase();
    lower.starts_with("doi:")
        || lower.starts_with("https://doi.org/")
        || lower.starts_with("http://doi.org/")
        || (value.starts_with("10.") && value.contains('/'))
}

fn looks_like_arxiv_identifier(value: &str) -> bool {
    let value = value.trim().trim_end_matches(".pdf");
    let lower = value.to_ascii_lowercase();
    if lower.starts_with("arxiv:") || lower.contains("arxiv.org/") {
        return true;
    }
    let id = lower.trim_start_matches("arxiv:");
    let id = id
        .rsplit_once('v')
        .filter(|(_, version)| version.chars().all(|c| c.is_ascii_digit()))
        .map(|(base, _)| base)
        .unwrap_or(id);
    let mut parts = id.split('.');
    matches!(
        (parts.next(), parts.next(), parts.next()),
        (Some(ym), Some(seq), None)
            if ym.len() == 4
                && ym.chars().all(|c| c.is_ascii_digit())
                && seq.len() >= 4
                && seq.chars().all(|c| c.is_ascii_digit())
    )
}

fn looks_like_openalex_identifier(value: &str) -> bool {
    let value = value.trim();
    let lower = value.to_ascii_lowercase();
    lower.contains("openalex.org/")
        || value
            .strip_prefix('W')
            .or_else(|| value.strip_prefix('w'))
            .is_some_and(|rest| !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()))
}

fn metadata_string_from_result(args: &FetchArgs, keys: &[&str]) -> Option<String> {
    let metadata = args.result.as_ref()?.get("metadata")?.as_object()?;
    for key in keys {
        let value = metadata
            .get(*key)
            .and_then(JsonValue::as_str)
            .and_then(clean_nonempty);
        if value.is_some() {
            return value;
        }
    }
    None
}

fn clean_nonempty(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn pmid_from_pubmed_url(url: &str) -> Option<String> {
    let parsed = reqwest::Url::parse(url).ok()?;
    let host = parsed.host_str()?.to_ascii_lowercase();
    if !host.ends_with(PUBMED_HOST) {
        return None;
    }
    parsed
        .path_segments()?
        .find(|segment| !segment.is_empty() && segment.chars().all(|c| c.is_ascii_digit()))
        .map(str::to_string)
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

fn favicon_for_url(url: &str) -> Option<String> {
    let parsed = reqwest::Url::parse(url).ok()?;
    let host = parsed.host_str()?.to_ascii_lowercase();
    Some(format!(
        "https://www.google.com/s2/favicons?domain={}&sz=64",
        host.trim_start_matches("www.")
    ))
}

fn structured_error_json(
    code: &str,
    category: &str,
    source: &str,
    message: impl Into<String>,
) -> JsonValue {
    json!({
        "error": code,
        "category": category,
        "source": source,
        "message": message.into(),
    })
}

fn json_stream(value: JsonValue) -> crate::infrastructure::streaming::StreamOutputBox {
    let text = serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string());
    FetchOutput { text }.into_stream()
}

fn sniff_likely_html(bytes: &[u8]) -> bool {
    let take = bytes.len().min(512);
    let slice = &bytes[..take];
    let Ok(s) = std::str::from_utf8(slice) else {
        return false;
    };
    let t = s.trim_start();
    let l = t.to_ascii_lowercase();
    l.starts_with("<!doctype html") || l.starts_with("<html")
}

fn truncate_chars(s: &str, max: usize) -> (String, Option<String>) {
    if s.len() <= max {
        return (s.to_string(), None);
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    let note = format!(
        "Truncated to {} characters; full decoded text was {} characters",
        end,
        s.len()
    );
    (s[..end].to_string(), Some(note))
}

#[derive(Debug, Clone)]
struct FetchOutput {
    text: String,
}

impl StreamOutput for FetchOutput {
    fn into_stream(self) -> Pin<Box<dyn futures::Stream<Item = StreamOutputItem> + Send>> {
        use futures::stream;
        let items = vec![
            StreamOutputItem::Start,
            StreamOutputItem::Content(self.text),
            StreamOutputItem::Complete,
        ];
        Box::pin(stream::iter(items))
    }
}

pub fn schema() -> ToolSchema {
    ToolSchema::new(
        "fetch",
        DESCRIPTION,
        serde_json::json!({
            "type": "object",
            "properties": {
                "category": {
                    "type": "string",
                    "description": "Source category. Supports literature, dataset (alias: data), web, social. Knowledge retrieval uses recall/search."
                },
                "source": {
                    "type": "string",
                    "description": "Source within the category. Defaults to auto. Literature supports pubmed, arxiv, crossref, openalex, biorxiv, medrxiv, semantic_scholar. Dataset supports geo, ena, ena_run, ena_experiment, ena_sample, ena_analysis, ena_assembly, ena_sequence."
                },
                "subcategory": {
                    "type": "string",
                    "description": "Optional dataset subcategory hint: expression, sequencing, genomics, sample_metadata, multi_omics."
                },
                "url": {
                    "type": "string",
                    "description": "Fully qualified URL to fetch, or a source-specific literature/data URL (PubMed/arXiv/OpenAlex/DOI/preprint/Semantic Scholar/GEO/ENA)"
                },
                "id": {
                    "type": "string",
                    "description": "Source-specific identifier such as PMID, arXiv id, DOI, OpenAlex work id, preprint DOI, Semantic Scholar paper id, GEO accession/UID, or ENA accession."
                },
                "result": {
                    "type": "object",
                    "description": "A single result object returned by search; fetch will read source, url/link, id, and metadata identifiers from it."
                },
                "prompt": {
                    "type": "string",
                    "description": "What to extract or how to use the fetched document (included in JSON output)"
                }
            },
            "required": ["category"]
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_inline_base64_images_replaces_payloads() {
        let raw = "before data:image/png;base64,abcd1234+/= after";
        let cleaned = clean_inline_base64_images(raw);
        assert!(cleaned.contains("[image omitted]"));
        assert!(!cleaned.contains("base64"));
    }

    #[test]
    fn truncate_chars_preserves_boundaries() {
        let s = "é".repeat(10);
        let (t, note) = truncate_chars(&s, 7);
        assert!(t.is_char_boundary(t.len()));
        assert!(note.is_some());
    }

    #[test]
    fn resolves_url_from_search_result_link() {
        let args = FetchArgs {
            category: "web".into(),
            source: None,
            subcategory: None,
            url: None,
            id: None,
            result: Some(json!({"title":"A","link":"https://example.org/a"})),
            prompt: None,
        };
        assert_eq!(resolve_url(&args).as_deref(), Some("https://example.org/a"));
        assert_eq!(title_from_result(&args).as_deref(), Some("A"));
    }

    #[test]
    fn resolves_pubmed_pmid_from_url_or_metadata() {
        let from_url = FetchArgs {
            category: "literature".into(),
            source: Some("pubmed".into()),
            subcategory: None,
            url: Some("https://pubmed.ncbi.nlm.nih.gov/12345678/".into()),
            id: None,
            result: None,
            prompt: None,
        };
        assert_eq!(resolve_pubmed_pmid(&from_url).as_deref(), Some("12345678"));

        let from_result = FetchArgs {
            category: "literature".into(),
            source: Some("pubmed".into()),
            subcategory: None,
            url: None,
            id: None,
            result: Some(json!({"metadata":{"pmid":"42"}})),
            prompt: None,
        };
        assert_eq!(resolve_pubmed_pmid(&from_result).as_deref(), Some("42"));
    }

    #[test]
    fn resolves_literature_source_and_identifier_for_public_sources() {
        let from_arxiv_result = FetchArgs {
            category: "literature".into(),
            source: None,
            subcategory: None,
            url: None,
            id: None,
            result: Some(json!({
                "source": "arxiv",
                "link": "https://arxiv.org/abs/2401.01234",
                "metadata": {"arxiv_id": "2401.01234"}
            })),
            prompt: None,
        };
        assert_eq!(
            resolve_literature_source(&from_arxiv_result, "auto"),
            "arxiv"
        );
        assert_eq!(
            resolve_literature_identifier(&from_arxiv_result, "arxiv").as_deref(),
            Some("2401.01234")
        );

        let from_doi_url = FetchArgs {
            category: "literature".into(),
            source: None,
            subcategory: None,
            url: Some("https://doi.org/10.1000/example".into()),
            id: None,
            result: None,
            prompt: None,
        };
        assert_eq!(resolve_literature_source(&from_doi_url, "auto"), "crossref");
        assert_eq!(
            resolve_literature_identifier(&from_doi_url, "crossref").as_deref(),
            Some("https://doi.org/10.1000/example")
        );

        let from_openalex_metadata = FetchArgs {
            category: "literature".into(),
            source: Some("openalex".into()),
            subcategory: None,
            url: None,
            id: None,
            result: Some(json!({
                "id": "ignored",
                "metadata": {"openalex_id": "W123"}
            })),
            prompt: None,
        };
        assert_eq!(
            resolve_literature_identifier(&from_openalex_metadata, "openalex").as_deref(),
            Some("W123")
        );

        let from_doi_id = FetchArgs {
            category: "literature".into(),
            source: None,
            subcategory: None,
            url: None,
            id: Some("10.1000/example".into()),
            result: None,
            prompt: None,
        };
        assert_eq!(resolve_literature_source(&from_doi_id, "auto"), "crossref");

        let from_arxiv_id = FetchArgs {
            category: "literature".into(),
            source: None,
            subcategory: None,
            url: None,
            id: Some("2401.01234v2".into()),
            result: None,
            prompt: None,
        };
        assert_eq!(resolve_literature_source(&from_arxiv_id, "auto"), "arxiv");
    }

    #[test]
    fn resolves_data_source_and_identifier() {
        let from_geo_result = FetchArgs {
            category: "data".into(),
            source: None,
            subcategory: None,
            url: None,
            id: None,
            result: Some(json!({
                "source": "geo",
                "accession": "GSE123",
                "metadata": {"accession": "GSE123"}
            })),
            prompt: None,
        };
        assert_eq!(resolve_data_source(&from_geo_result, "auto"), "geo");
        assert_eq!(
            resolve_data_identifier(&from_geo_result).as_deref(),
            Some("GSE123")
        );

        let from_ena_url = FetchArgs {
            category: "data".into(),
            source: None,
            subcategory: None,
            url: Some("https://www.ebi.ac.uk/ena/browser/view/PRJEB123".into()),
            id: None,
            result: None,
            prompt: None,
        };
        assert_eq!(resolve_data_source(&from_ena_url, "auto"), "ena");

        let from_geo_url = FetchArgs {
            category: "data".into(),
            source: None,
            subcategory: None,
            url: Some("https://www.ncbi.nlm.nih.gov/geo/query/acc.cgi?acc=GSM575".into()),
            id: None,
            result: None,
            prompt: None,
        };
        assert_eq!(resolve_data_source(&from_geo_url, "auto"), "geo");
    }

    #[test]
    fn resolves_semantic_scholar_ids_from_common_inputs() {
        let from_doi = FetchArgs {
            category: "literature".into(),
            source: Some("semantic_scholar".into()),
            subcategory: None,
            url: None,
            id: Some("10.1000/example".into()),
            result: None,
            prompt: None,
        };
        assert_eq!(
            resolve_semantic_scholar_id(&from_doi).as_deref(),
            Some("DOI:10.1000/example")
        );

        let from_slug_url = FetchArgs {
            category: "literature".into(),
            source: Some("semantic_scholar".into()),
            subcategory: None,
            url: Some("https://www.semanticscholar.org/paper/A-title/abcdef123456".into()),
            id: None,
            result: None,
            prompt: None,
        };
        assert_eq!(
            resolve_semantic_scholar_id(&from_slug_url).as_deref(),
            Some("abcdef123456")
        );

        let from_metadata = FetchArgs {
            category: "literature".into(),
            source: Some("semantic_scholar".into()),
            subcategory: None,
            url: None,
            id: None,
            result: Some(json!({
                "metadata": {"paper_id": "paper-1", "doi": "10.1000/fallback"}
            })),
            prompt: None,
        };
        assert_eq!(
            resolve_semantic_scholar_id(&from_metadata).as_deref(),
            Some("paper-1")
        );

        assert_eq!(
            normalize_semantic_scholar_id("doi:10.1000/example"),
            "DOI:10.1000/example"
        );
    }
}
