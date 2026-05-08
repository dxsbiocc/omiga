//! Unified search tool — web, literature, and extension-ready source routing
//!
//! Public fallback engines:
//! - DuckDuckGo: the public `api.duckduckgo.com` endpoint is an **instant-answer** API, not full
//!   web search — many queries return empty `AbstractURL` / `RelatedTopics`. HTML results at
//!   `html.duckduckgo.com` are used when the JSON API yields nothing.
//! - Bing / Google: lightweight HTML fallbacks used after configured APIs.
//!
//! Result links are often search-engine redirects (`duckduckgo.com/l/?uddg=…`,
//! `bing.com/ck/a?u=…`, `google.com/url?q=…`); we unwrap them to the real destination URL.

mod common;
mod dataset;
mod web;

use super::{ToolContext, ToolError, ToolSchema};
use async_trait::async_trait;
use common::*;
use std::time::Instant;

pub use common::SearchArgs;

pub(crate) enum BuiltinDataSearchResult {
    Response(crate::domain::search::data::DataSearchResponse),
    StructuredError(serde_json::Value),
}

impl BuiltinDataSearchResult {
    pub(crate) fn into_json(self) -> serde_json::Value {
        match self {
            Self::Response(response) => {
                crate::domain::search::data::search_response_to_json(&response)
            }
            Self::StructuredError(value) => value,
        }
    }
}

pub const DESCRIPTION: &str = r#"Search across typed data-source categories and return formatted results. Web/literature/dataset/social return SerpAPI-style JSON; knowledge returns recall excerpts.

- `category` is required. Categories: `literature`, `dataset` (`data` alias), `knowledge`, `web`, `social`.
- `source` is optional and defaults to `auto`. Web sources: `auto`, `tavily`, `exa`, `firecrawl`, `parallel`, `google`, `bing`, `ddg`. Literature sources: `auto`, `pubmed`, `arxiv`, `crossref`, `openalex`, `biorxiv`, `medrxiv`, `semantic_scholar` (opt-in, API key required). Dataset sources: `auto`, `geo`, `ena`, `ena_run`, `ena_experiment`, `ena_sample`, `ena_analysis`, `ena_assembly`, `ena_sequence`, `cbioportal`, `gtex`, `ncbi_datasets`, `arrayexpress`, `biosample`. Knowledge sources/scopes: `all`, `wiki`, `implicit`, `long_term`, `permanent`, `sources`. Social sources: `wechat` (opt-in).
- `subcategory` is optional. Prefer `query(category="dataset", operation="search", …)` for structured dataset/database lookup; this dataset path remains as a compatibility search wrapper.
- `source=auto` uses Settings → Search priority for web, PubMed for literature, and enabled dataset sources for dataset.
- Results are returned as formatted JSON with a top-level `results` array and SerpAPI-style fields (`position`, `title`, `name`, `link`, `url`, `displayed_link`, `favicon`, `snippet`, `metadata`).
- Optional `allowed_domains` or `blocked_domains` filter web result URLs (not both).
- `max_results` (default 5, max 10) limits how many hits are returned.
- Optional `search_url` overrides the DuckDuckGo HTML endpoint (for private search proxies or tests).
- Unsafe/private result URLs are filtered out; `search_url` must also be a public-safe HTTP(S) URL.
- After answering, cite sources with markdown links when you use this tool."#;

pub struct SearchTool;

#[async_trait]
impl super::ToolImpl for SearchTool {
    type Args = SearchArgs;

    const DESCRIPTION: &'static str = DESCRIPTION;

    async fn execute(
        ctx: &ToolContext,
        args: Self::Args,
    ) -> Result<crate::infrastructure::streaming::StreamOutputBox, ToolError> {
        crate::domain::retrieval::tool_bridge::execute_search(ctx, args).await
    }
}

pub(crate) fn validate_search_args(args: &SearchArgs) -> Result<(), ToolError> {
    validate(args)
}

pub(crate) async fn execute_builtin_search(
    ctx: &ToolContext,
    args: SearchArgs,
) -> Result<crate::infrastructure::streaming::StreamOutputBox, ToolError> {
    validate(&args)?;
    let start = Instant::now();
    let category = normalized_category(&args.category);
    let source = normalized_source(args.source.as_deref());
    let max_n = effective_max_results(&args);

    match category.as_str() {
        "web" => Ok(json_stream(
            execute_builtin_web_search_json_with_route(ctx, &args, &source, max_n, start).await?,
        )),
        "literature" => Ok(json_stream(
            execute_builtin_literature_search_json(ctx, &args).await?,
        )),
        "knowledge" => {
            <super::recall::RecallTool as super::ToolImpl>::execute(
                ctx,
                super::recall::RecallArgs {
                    query: args.query.trim().to_string(),
                    limit: max_n.clamp(1, 20),
                    scope: recall_scope_for_source(&source),
                },
            )
            .await
        }
        "data" => Ok(json_stream(
            execute_builtin_data_search(ctx, &args).await?.into_json(),
        )),
        "social" => Ok(json_stream(
            execute_builtin_social_search_json(ctx, &args, &source).await?,
        )),
        other => Err(ToolError::InvalidArguments {
            message: format!("Unsupported search category: {other}"),
        }),
    }
}

pub(crate) async fn execute_builtin_web_search_json(
    ctx: &ToolContext,
    args: &SearchArgs,
) -> Result<serde_json::Value, ToolError> {
    let source = normalized_source(args.source.as_deref());
    let max_n = effective_max_results(args);
    execute_builtin_web_search_json_with_route(ctx, args, &source, max_n, Instant::now()).await
}

async fn execute_builtin_web_search_json_with_route(
    ctx: &ToolContext,
    args: &SearchArgs,
    source: &str,
    max_n: usize,
    start: Instant,
) -> Result<serde_json::Value, ToolError> {
    web::search_web_json(ctx, args, source, max_n, start).await
}

pub(crate) async fn execute_builtin_social_search_json(
    ctx: &ToolContext,
    args: &SearchArgs,
    source: &str,
) -> Result<serde_json::Value, ToolError> {
    match source {
        "auto" | "wechat" => {
            if !ctx.web_search_api_keys.wechat_search_enabled {
                return Ok(structured_error_json(
                    "source_disabled",
                    "social",
                    source,
                    "social.wechat is disabled. Enable WeChat public-account search in Settings → Search.",
                ));
            }
            let client = crate::domain::search::wechat::WechatClient::from_tool_context(ctx)
                .map_err(|message| ToolError::ExecutionFailed { message })?;
            let response = tokio::select! {
                _ = ctx.cancel.cancelled() => return Err(ToolError::Cancelled),
                r = client.search(crate::domain::search::wechat::WechatSearchArgs {
                    query: args.query.trim().to_string(),
                    max_results: args.max_results,
                    page: None,
                }) => r.map_err(|message| ToolError::ExecutionFailed { message })?,
            };
            Ok(crate::domain::search::wechat::search_response_to_json(
                &response,
            ))
        }
        other => Err(ToolError::InvalidArguments {
            message: format!("Unsupported social search source: {other}"),
        }),
    }
}

pub(crate) async fn execute_builtin_literature_search_json(
    ctx: &ToolContext,
    args: &SearchArgs,
) -> Result<serde_json::Value, ToolError> {
    let source = normalized_source(args.source.as_deref());
    match source.as_str() {
        "auto" | "pubmed" => {
            let client = crate::domain::search::pubmed::EntrezClient::from_tool_context(ctx)
                .map_err(|message| ToolError::ExecutionFailed { message })?;
            let response = tokio::select! {
                _ = ctx.cancel.cancelled() => return Err(ToolError::Cancelled),
                r = client.search(crate::domain::search::pubmed::PubmedSearchArgs {
                    query: args.query.trim().to_string(),
                    max_results: args.max_results,
                    ret_start: None,
                    sort: None,
                    date_type: None,
                    mindate: None,
                    maxdate: None,
                }) => r.map_err(|message| ToolError::ExecutionFailed { message })?,
            };
            Ok(crate::domain::search::pubmed::search_response_to_json(
                &response,
            ))
        }
        "semantic_scholar" | "semanticscholar" | "s2" => {
            if !ctx.web_search_api_keys.semantic_scholar_enabled {
                return Ok(structured_error_json(
                    "source_disabled",
                    "literature",
                    &source,
                    "literature.semantic_scholar is disabled. Enable it and configure an API key in Settings → Search.",
                ));
            }
            let client =
                crate::domain::search::semantic_scholar::SemanticScholarClient::from_tool_context(
                    ctx,
                )
                .map_err(|message| ToolError::ExecutionFailed { message })?;
            let response = tokio::select! {
                _ = ctx.cancel.cancelled() => return Err(ToolError::Cancelled),
                r = client.search(crate::domain::search::semantic_scholar::SemanticScholarSearchArgs {
                    query: args.query.trim().to_string(),
                    max_results: args.max_results,
                    token: None,
                }) => r.map_err(|message| ToolError::ExecutionFailed { message })?,
            };
            Ok(crate::domain::search::semantic_scholar::search_response_to_json(&response))
        }
        "arxiv" | "crossref" | "openalex" | "biorxiv" | "bio_rxiv" | "medrxiv" | "med_rxiv" => {
            let source_kind =
                crate::domain::search::literature::PublicLiteratureSource::parse(&source)
                    .ok_or_else(|| ToolError::InvalidArguments {
                        message: format!("Unsupported literature search source: {source}"),
                    })?;
            let client =
                crate::domain::search::literature::PublicLiteratureClient::from_tool_context(ctx)
                    .map_err(|message| ToolError::ExecutionFailed { message })?;
            let response = tokio::select! {
                _ = ctx.cancel.cancelled() => return Err(ToolError::Cancelled),
                r = client.search(source_kind, crate::domain::search::literature::LiteratureSearchArgs {
                    query: args.query.trim().to_string(),
                    max_results: args.max_results,
                }) => r.map_err(|message| ToolError::ExecutionFailed { message })?,
            };
            Ok(crate::domain::search::literature::search_response_to_json(
                &response,
            ))
        }
        other => Err(ToolError::InvalidArguments {
            message: format!("Unsupported literature search source: {other}"),
        }),
    }
}

pub(crate) async fn execute_builtin_data_search(
    ctx: &ToolContext,
    args: &SearchArgs,
) -> Result<BuiltinDataSearchResult, ToolError> {
    let source = normalized_source(args.source.as_deref());
    let subcategory = normalized_subcategory(args.subcategory.as_deref());

    if let Some(type_id) = dataset::dataset_subcategory_id(subcategory.as_deref())? {
        if !ctx
            .web_search_api_keys
            .is_query_dataset_type_enabled(type_id)
        {
            return Ok(BuiltinDataSearchResult::StructuredError(
                structured_error_json(
                    "source_disabled",
                    "data",
                    &source,
                    format!("data.{type_id} is disabled. Enable it in Settings → Search.",),
                ),
            ));
        }
    }
    match source.as_str() {
        "auto" => {
            let client = crate::domain::search::data::PublicDataClient::from_tool_context(ctx)
                .map_err(|message| ToolError::ExecutionFailed { message })?;
            let data_args = crate::domain::search::data::DataSearchArgs {
                query: args.query.trim().to_string(),
                max_results: args.max_results,
                params: None,
            };
            let response = if let Some(source_kind) =
                dataset::dataset_source_for_subcategory(subcategory.as_deref())?
            {
                if !ctx
                    .web_search_api_keys
                    .is_query_dataset_source_enabled(source_kind.as_str())
                {
                    return Ok(BuiltinDataSearchResult::StructuredError(
                        structured_error_json(
                            "source_disabled",
                            "data",
                            source_kind.as_str(),
                            format!(
                                "data.{} is disabled. Enable it in Settings → Search.",
                                source_kind.as_str()
                            ),
                        ),
                    ));
                }
                tokio::select! {
                    _ = ctx.cancel.cancelled() => return Err(ToolError::Cancelled),
                    r = client.search(source_kind, data_args) => r.map_err(|message| ToolError::ExecutionFailed { message })?,
                }
            } else {
                dataset::dataset_auto_search(ctx, &client, data_args).await?
            };
            Ok(BuiltinDataSearchResult::Response(response))
        }
        source if crate::domain::search::data::PublicDataSource::parse(source).is_some() => {
            let source_kind = crate::domain::search::data::PublicDataSource::parse(source)
                .ok_or_else(|| ToolError::InvalidArguments {
                    message: format!("Unsupported data search source: {source}"),
                })?;
            if !ctx
                .web_search_api_keys
                .is_query_dataset_source_enabled(source_kind.as_str())
            {
                return Ok(BuiltinDataSearchResult::StructuredError(
                    structured_error_json(
                        "source_disabled",
                        "data",
                        source_kind.as_str(),
                        format!(
                            "data.{} is disabled. Enable it in Settings → Search.",
                            source_kind.as_str()
                        ),
                    ),
                ));
            }
            let client = crate::domain::search::data::PublicDataClient::from_tool_context(ctx)
                .map_err(|message| ToolError::ExecutionFailed { message })?;
            let response = tokio::select! {
                _ = ctx.cancel.cancelled() => return Err(ToolError::Cancelled),
                r = client.search(source_kind, crate::domain::search::data::DataSearchArgs {
                    query: args.query.trim().to_string(),
                    max_results: args.max_results,
                    params: None,
                }) => r.map_err(|message| ToolError::ExecutionFailed { message })?,
            };
            Ok(BuiltinDataSearchResult::Response(response))
        }
        other => Err(ToolError::InvalidArguments {
            message: format!("Unsupported data search source: {other}"),
        }),
    }
}

pub fn schema() -> ToolSchema {
    ToolSchema::new(
        "search",
        DESCRIPTION,
        serde_json::json!({
            "type": "object",
            "properties": {
                "category": {
                    "type": "string",
                    "description": "Source category. Supports literature, dataset (alias: data), knowledge, web, social."
                },
                "source": {
                    "type": "string",
                    "description": "Source within the category. Defaults to auto. Examples: google, ddg, bing, tavily, pubmed, arxiv, crossref, openalex, biorxiv, medrxiv, semantic_scholar (opt-in), geo, ena, ena_run, ena_experiment, ena_sample, ena_analysis, ena_assembly, ena_sequence, cbioportal, gtex, ncbi_datasets, arrayexpress, biosample, wiki, implicit, long_term, sources, wechat (opt-in)."
                },
                "subcategory": {
                    "type": "string",
                    "description": "Optional subcategory. For dataset: expression, sequencing, genomics, sample_metadata, multi_omics. With source=auto, supported dataset subcategories choose the best built-in source automatically."
                },
                "query": {
                    "type": "string",
                    "description": "Search query (at least 2 characters)"
                },
                "allowed_domains": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Only include results whose URL host matches one of these domains"
                },
                "blocked_domains": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Exclude results from these domains"
                },
                "max_results": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 10,
                    "description": "Maximum number of results (default 5)"
                },
                "search_url": {
                    "type": "string",
                    "description": "Optional override for the DuckDuckGo HTML search endpoint (http(s) URL)"
                }
            },
            "required": ["category", "query"]
        }),
    )
}
