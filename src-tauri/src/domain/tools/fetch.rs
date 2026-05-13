//! Unified fetch tool — retrieve details for web pages or source-specific records.
//!
//! The public model-visible function is `fetch`; first-version adapters are:
//! - `category="web"`: safe public HTTP(S) fetch and text extraction.
//! - `category="literature", source="pubmed"`: official NCBI EFetch by PMID.
//! - `category="literature", source="arxiv|crossref|openalex|biorxiv|medrxiv"`:
//!   public source metadata fetch by source id/DOI/arXiv/OpenAlex URL.

use super::{ToolContext, ToolError, ToolSchema};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

mod common;
mod data;
mod literature;
mod web;

pub const DESCRIPTION: &str = r#"Fetch one document/detail from a typed data source and return formatted JSON.

- `category` is required. Categories: `literature`, `dataset` (`data` alias), `knowledge` (use `query` for structured knowledge records and `recall` for local knowledge), `web`, `social`; installed plugins can add more categories.
- `source` is optional and defaults to `auto`. Concrete sources: `web.auto`, `literature.pubmed|arxiv|crossref|openalex|biorxiv|medrxiv|semantic_scholar`, `dataset.geo|ena|ena_run|ena_experiment|ena_sample|ena_analysis|ena_assembly|ena_sequence|cbioportal|gtex|ncbi_datasets|arrayexpress|biosample`, optional `social.wechat`; installed plugins may expose additional sources.
- `subcategory` is optional for dataset routing. Prefer `query(category="dataset", operation="fetch", …)` for structured dataset/database record lookup; this dataset path remains as a compatibility fetch wrapper.
- Locate the document with one of: `url`, `id` + `source`, or a full `result` object returned by `search`.
- `web` fetch sends a safe public HTTP(S) GET, follows public-safe redirects, blocks private/loopback targets, converts HTML to text, and pretty-prints JSON.
- `literature.pubmed` fetch expects a numeric PMID in `id` (or a PubMed URL / search result) and uses official NCBI EFetch.
- Public literature sources fetch source-specific metadata records: arXiv by arXiv id/URL, Crossref/bioRxiv/medRxiv by DOI, and OpenAlex by work id/URL/DOI.
- `literature.semantic_scholar` is opt-in and requires the user-enabled API key; it fetches by Semantic Scholar paper id or supported external id prefix.
- `data.geo` fetches GEO DataSets/Series/Samples/Platforms by UID or GEO accession via official NCBI E-utilities; `data.ena*` fetches ENA study/run/experiment/sample/analysis/assembly/sequence metadata by accession via ENA Portal/Browser APIs; `data.cbioportal` fetches cBioPortal study metadata by study id; `data.gtex` fetches GTEx gene metadata by symbol or GENCODE ID; `data.ncbi_datasets` fetches genome assembly reports by GCA_/GCF_ accession via NCBI Datasets v2; `data.arrayexpress` fetches BioStudies ArrayExpress studies by E-* accession; `data.biosample` fetches BioSample reports by SAMN/SAMEA/SAMD accession.
- `social.wechat` is disabled by default; when enabled it fetches the article URL with the safe web fetcher.
- Results are returned as formatted JSON with `title`, `link`, `url`, `favicon`, `content`, and `metadata`."#;

fn default_category() -> String {
    "web".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FetchArgs {
    #[serde(default = "default_category")]
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

#[async_trait]
impl super::ToolImpl for FetchTool {
    type Args = FetchArgs;

    const DESCRIPTION: &'static str = DESCRIPTION;

    async fn execute(
        ctx: &ToolContext,
        args: Self::Args,
    ) -> Result<crate::infrastructure::streaming::StreamOutputBox, ToolError> {
        crate::domain::retrieval::tool_bridge::execute_fetch(ctx, args).await
    }
}

pub(crate) async fn execute_builtin_web_fetch_json(
    ctx: &ToolContext,
    args: &FetchArgs,
    requested_source: &str,
) -> Result<JsonValue, ToolError> {
    web::fetch_web_json(ctx, args, requested_source).await
}

pub(crate) async fn execute_builtin_social_fetch_json(
    ctx: &ToolContext,
    args: &FetchArgs,
    requested_source: &str,
) -> Result<JsonValue, ToolError> {
    match requested_source {
        "auto" | "wechat" => {
            if !ctx.web_search_api_keys.wechat_search_enabled {
                return Ok(common::structured_error_json(
                    "source_disabled",
                    "social",
                    requested_source,
                    "social.wechat is disabled. Enable WeChat public-account search in Settings → Search.",
                ));
            }
            execute_builtin_web_fetch_json(ctx, args, requested_source).await
        }
        other => Err(ToolError::InvalidArguments {
            message: format!("Unsupported social fetch source: {other}"),
        }),
    }
}

pub(crate) async fn execute_builtin_data_fetch_json(
    ctx: &ToolContext,
    args: &FetchArgs,
) -> Result<JsonValue, ToolError> {
    let source = common::normalized_source(args.source.as_deref());
    let data_source = data::resolve_data_source(args, &source);
    match data_source.as_str() {
        data_source
            if crate::domain::search::data::PublicDataSource::parse(data_source).is_some() =>
        {
            if !ctx
                .web_search_api_keys
                .is_query_dataset_source_enabled(data_source)
            {
                return Ok(common::structured_error_json(
                    "source_disabled",
                    "data",
                    data_source,
                    format!("data.{data_source} is disabled. Enable it in Settings → Search."),
                ));
            }
            data::fetch_public_data_json(ctx, args, data_source).await
        }
        other => Err(ToolError::InvalidArguments {
            message: format!("Unsupported data fetch source: {other}"),
        }),
    }
}

pub(crate) async fn execute_builtin_literature_fetch_json(
    ctx: &ToolContext,
    args: &FetchArgs,
    requested_source: &str,
) -> Result<JsonValue, ToolError> {
    match literature::resolve_literature_source(args, requested_source).as_str() {
        "pubmed" => literature::fetch_pubmed_json(ctx, args).await,
        "semantic_scholar" | "semanticscholar" => {
            if !ctx.web_search_api_keys.semantic_scholar_enabled {
                return Ok(common::structured_error_json(
                    "source_disabled",
                    "literature",
                    requested_source,
                    "literature.semantic_scholar is disabled. Enable it and configure an API key in Settings → Search.",
                ));
            }
            literature::fetch_semantic_scholar_json(ctx, args).await
        }
        public_source
            if crate::domain::search::literature::PublicLiteratureSource::parse(public_source)
                .is_some() =>
        {
            literature::fetch_public_literature_json(ctx, args, public_source).await
        }
        other => Err(ToolError::InvalidArguments {
            message: format!("Unsupported literature fetch source: {other}"),
        }),
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
                    "description": "Source category. Supports literature, dataset (alias: data), web, social, and categories exposed by installed plugins. Knowledge records use query(category=knowledge, operation=fetch); local knowledge uses recall."
                },
                "source": {
                    "type": "string",
                    "description": "Source within the category. Defaults to auto. Literature supports pubmed, arxiv, crossref, openalex, biorxiv, medrxiv, semantic_scholar. Dataset supports geo, ena, ena_run, ena_experiment, ena_sample, ena_analysis, ena_assembly, ena_sequence, cbioportal, gtex, ncbi_datasets, arrayexpress, biosample. Installed plugins may expose additional sources."
                },
                "subcategory": {
                    "type": "string",
                    "description": "Optional dataset subcategory hint: expression, sequencing, genomics, sample_metadata, multi_omics."
                },
                "url": {
                    "type": "string",
                    "description": "Fully qualified URL to fetch, or a source-specific literature/data URL (PubMed/arXiv/OpenAlex/DOI/preprint/Semantic Scholar/GEO/ENA/ArrayExpress/BioSample)"
                },
                "id": {
                    "type": "string",
                    "description": "Source-specific identifier such as PMID, arXiv id, DOI, OpenAlex work id, preprint DOI, Semantic Scholar paper id, GEO accession/UID, ENA accession, ArrayExpress accession, or BioSample accession."
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
