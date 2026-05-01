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

- `category` is required. Categories: `literature`, `dataset` (`data` alias), `knowledge` (use `recall` for search), `web`, `social`.
- `source` is optional and defaults to `auto`. Concrete sources: `web.auto`, `literature.pubmed|arxiv|crossref|openalex|biorxiv|medrxiv|semantic_scholar`, `dataset.geo|ena|ena_run|ena_experiment|ena_sample|ena_analysis|ena_assembly|ena_sequence|cbioportal|gtex|ncbi_datasets`, optional `social.wechat`.
- `subcategory` is optional for dataset routing. Prefer `query(category="dataset", operation="fetch", …)` for structured dataset/database record lookup; this dataset path remains as a compatibility fetch wrapper.
- Locate the document with one of: `url`, `id` + `source`, or a full `result` object returned by `search`.
- `web` fetch sends a safe public HTTP(S) GET, follows public-safe redirects, blocks private/loopback targets, converts HTML to text, and pretty-prints JSON.
- `literature.pubmed` fetch expects a numeric PMID in `id` (or a PubMed URL / search result) and uses official NCBI EFetch.
- Public literature sources fetch source-specific metadata records: arXiv by arXiv id/URL, Crossref/bioRxiv/medRxiv by DOI, and OpenAlex by work id/URL/DOI.
- `literature.semantic_scholar` is opt-in and requires the user-enabled API key; it fetches by Semantic Scholar paper id or supported external id prefix.
- `data.geo` fetches GEO DataSets/Series/Samples/Platforms by UID or GEO accession via official NCBI E-utilities; `data.ena*` fetches ENA study/run/experiment/sample/analysis/assembly/sequence metadata by accession via ENA Portal/Browser APIs; `data.cbioportal` fetches cBioPortal study metadata by study id; `data.gtex` fetches GTEx gene metadata by symbol or GENCODE ID; `data.ncbi_datasets` fetches genome assembly reports by GCA_/GCF_ accession via NCBI Datasets v2.
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

#[async_trait]
impl super::ToolImpl for FetchTool {
    type Args = FetchArgs;

    const DESCRIPTION: &'static str = DESCRIPTION;

    async fn execute(
        ctx: &ToolContext,
        args: Self::Args,
    ) -> Result<crate::infrastructure::streaming::StreamOutputBox, ToolError> {
        let category = common::normalized_category(&args.category);
        let source = common::normalized_source(args.source.as_deref());
        match category.as_str() {
            "web" => web::fetch_web(ctx, &args, &source).await,
            "literature" => match literature::resolve_literature_source(&args, &source).as_str() {
                "pubmed" => literature::fetch_pubmed(ctx, &args).await,
                "semantic_scholar" | "semanticscholar" => {
                    if !ctx.web_search_api_keys.semantic_scholar_enabled {
                        return Ok(common::json_stream(common::structured_error_json(
                            "source_disabled",
                            "literature",
                            &source,
                            "literature.semantic_scholar is disabled. Enable it and configure an API key in Settings → Search.",
                        )));
                    }
                    literature::fetch_semantic_scholar(ctx, &args).await
                }
                public_source
                    if crate::domain::search::literature::PublicLiteratureSource::parse(
                        public_source,
                    )
                    .is_some() =>
                {
                    literature::fetch_public_literature(ctx, &args, public_source).await
                }
                other => Err(ToolError::InvalidArguments {
                    message: format!("Unsupported literature fetch source: {other}"),
                }),
            },
            "data" => {
                let data_source = data::resolve_data_source(&args, &source);
                match data_source.as_str() {
                    data_source
                        if crate::domain::search::data::PublicDataSource::parse(data_source)
                            .is_some() =>
                    {
                        if !ctx
                            .web_search_api_keys
                            .is_query_dataset_source_enabled(data_source)
                        {
                            return Ok(common::json_stream(common::structured_error_json(
                                "source_disabled",
                                "data",
                                data_source,
                                format!(
                                    "data.{data_source} is disabled. Enable it in Settings → Search."
                                ),
                            )));
                        }
                        data::fetch_public_data(ctx, &args, data_source).await
                    }
                    other => Err(ToolError::InvalidArguments {
                        message: format!("Unsupported data fetch source: {other}"),
                    }),
                }
            }
            "knowledge" => Err(ToolError::InvalidArguments {
                message:
                    "fetch(category=knowledge) is not supported; use recall(query=...) or search(category=knowledge)."
                        .to_string(),
            }),
            "social" => match source.as_str() {
                "auto" | "wechat" => {
                    if !ctx.web_search_api_keys.wechat_search_enabled {
                        return Ok(common::json_stream(common::structured_error_json(
                            "source_disabled",
                            "social",
                            &source,
                            "social.wechat is disabled. Enable WeChat public-account search in Settings → Search.",
                        )));
                    }
                    web::fetch_web(ctx, &args, &source).await
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
                    "description": "Source within the category. Defaults to auto. Literature supports pubmed, arxiv, crossref, openalex, biorxiv, medrxiv, semantic_scholar. Dataset supports geo, ena, ena_run, ena_experiment, ena_sample, ena_analysis, ena_assembly, ena_sequence, cbioportal, gtex, ncbi_datasets."
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
