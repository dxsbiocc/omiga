//! Structured database query tool.
//!
//! `query` is the structured companion to `search`/`fetch`: it executes
//! source-specific database operations while reusing the same built-in adapters.
//! The first migration target was `dataset`/`data` (GEO + ENA). Additional
//! databases are added one source at a time through this module.

use super::{ToolContext, ToolError, ToolSchema};
use crate::infrastructure::streaming::{StreamOutput, StreamOutputItem};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};
use std::pin::Pin;

pub const DESCRIPTION: &str = r#"Run a structured query against a typed database source and return formatted JSON.

Use `query` when the user wants database-native lookup/query semantics rather than broad discovery:
- `category="dataset"` (`data` alias) supports built-in dataset sources: `geo`, `ena`, `ena_run`, `ena_experiment`, `ena_sample`, `ena_analysis`, `ena_assembly`, `ena_sequence`, `cbioportal`.
- `category="knowledge", source="ncbi_gene"` supports the first migrated knowledge database: NCBI Gene via official NCBI E-utilities (`db=gene`).
- `operation="search"` searches records by keyword or database query string. `operation="fetch"`/`"get"` retrieves one record by accession, URL, or search result.
- `source="auto"` chooses a source from `subcategory` for search or from the identifier for fetch. Dataset subcategories: `expression` → GEO, `sequencing` → ENA run, `genomics` → ENA assembly, `sample_metadata` → ENA sample, `multi_omics` → cBioPortal.
- `params` may carry future database-specific filters; for NCBI Gene it can include `organism`, `taxon_id`, `ret_start`, and `sort`.
- `search`/`fetch` remain compatibility wrappers for discovery/detail flows; new structured dataset/database integrations should be added here one source at a time."#;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryArgs {
    pub category: String,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub operation: Option<String>,
    #[serde(default, alias = "subCategory", alias = "dataset_type", alias = "type")]
    pub subcategory: Option<String>,
    #[serde(default, alias = "q")]
    pub query: Option<String>,
    #[serde(default, alias = "accession")]
    pub id: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub result: Option<JsonValue>,
    #[serde(default)]
    pub params: Option<JsonValue>,
    #[serde(default, alias = "maxResults", alias = "limit", alias = "retmax")]
    pub max_results: Option<u32>,
}

pub struct QueryTool;

#[async_trait]
impl super::ToolImpl for QueryTool {
    type Args = QueryArgs;

    const DESCRIPTION: &'static str = DESCRIPTION;

    async fn execute(
        ctx: &ToolContext,
        args: Self::Args,
    ) -> Result<crate::infrastructure::streaming::StreamOutputBox, ToolError> {
        let category = normalized_category(&args.category);
        match category.as_str() {
            "data" => query_dataset(ctx, &args).await,
            "knowledge" => query_knowledge(ctx, &args).await,
            other => Err(ToolError::InvalidArguments {
                message: format!(
                    "Unsupported query category: {other}. Supported categories: dataset/data, knowledge."
                ),
            }),
        }
    }
}

async fn query_knowledge(
    ctx: &ToolContext,
    args: &QueryArgs,
) -> Result<crate::infrastructure::streaming::StreamOutputBox, ToolError> {
    let source = requested_source(args);
    let source = if source == "auto" {
        "ncbi_gene".to_string()
    } else {
        source
    };
    if !matches!(source.as_str(), "ncbi_gene" | "gene") {
        return Err(ToolError::InvalidArguments {
            message: format!(
                "Unsupported knowledge query source: {source}. First migrated source is ncbi_gene."
            ),
        });
    }
    if !ctx
        .web_search_api_keys
        .is_query_knowledge_source_enabled(&source)
    {
        return Err(ToolError::InvalidArguments {
            message: format!(
                "Knowledge source `{source}` is disabled in Settings → Search. Enable it before using query(category=knowledge)."
            ),
        });
    }

    let operation = normalized_operation(args);
    let client = crate::domain::search::ncbi_gene::NcbiGeneClient::from_tool_context(ctx)
        .map_err(|message| ToolError::ExecutionFailed { message })?;

    match operation.as_str() {
        "search" | "query" => {
            let query = query_text(args).ok_or_else(|| ToolError::InvalidArguments {
                message:
                    "query(category=knowledge, source=ncbi_gene, operation=search) requires `query` or params.query"
                        .to_string(),
            })?;
            let gene_args = crate::domain::search::ncbi_gene::GeneSearchArgs {
                query,
                organism: param_string(args, &["organism", "species"]),
                taxon_id: param_string(args, &["taxon_id", "taxid", "tax_id"]),
                max_results: args
                    .max_results
                    .or_else(|| param_u32(args, &["max_results", "maxResults", "limit", "retmax"])),
                ret_start: param_u32(args, &["ret_start", "retstart", "offset"]),
                sort: param_string(args, &["sort"]),
            };
            let response = tokio::select! {
                _ = ctx.cancel.cancelled() => return Err(ToolError::Cancelled),
                r = client.search(gene_args) => r.map_err(|message| ToolError::ExecutionFailed { message })?,
            };
            let mut json = crate::domain::search::ncbi_gene::search_response_to_json(&response);
            annotate_query_json(&mut json, "search", "knowledge");
            Ok(json_stream(json))
        }
        "fetch" | "get" | "detail" => {
            let identifier = identifier_text(args).ok_or_else(|| ToolError::InvalidArguments {
                message:
                    "query(category=knowledge, source=ncbi_gene, operation=fetch) requires numeric Gene ID in `id`, result, or params.id"
                        .to_string(),
            })?;
            let record = tokio::select! {
                _ = ctx.cancel.cancelled() => return Err(ToolError::Cancelled),
                r = client.fetch_by_gene_id(&identifier) => r.map_err(|message| ToolError::ExecutionFailed { message })?,
            };
            let mut json = crate::domain::search::ncbi_gene::detail_to_json(&record);
            annotate_query_json(&mut json, "fetch", "knowledge");
            Ok(json_stream(json))
        }
        other => Err(ToolError::InvalidArguments {
            message: format!("Unsupported NCBI Gene query operation: {other}"),
        }),
    }
}

async fn query_dataset(
    ctx: &ToolContext,
    args: &QueryArgs,
) -> Result<crate::infrastructure::streaming::StreamOutputBox, ToolError> {
    let source = requested_source(args);
    let operation = normalized_operation(args);
    let param_subcategory = param_string(
        args,
        &["subcategory", "subCategory", "dataset_type", "type"],
    );
    let subcategory =
        normalized_subcategory(args.subcategory.as_deref().or(param_subcategory.as_deref()));
    if let Some(type_id) = dataset_subcategory_id(subcategory.as_deref())? {
        if !ctx
            .web_search_api_keys
            .is_query_dataset_type_enabled(type_id)
        {
            return Err(ToolError::InvalidArguments {
                message: format!(
                    "Dataset type `{type_id}` is disabled in Settings → Search. Enable it before querying that subcategory."
                ),
            });
        }
    }
    let client = crate::domain::search::data::PublicDataClient::from_tool_context(ctx)
        .map_err(|message| ToolError::ExecutionFailed { message })?;

    match operation.as_str() {
        "search" | "query" => {
            let query = query_text(args).ok_or_else(|| ToolError::InvalidArguments {
                message:
                    "query(category=dataset, operation=search) requires `query` or params.query"
                        .to_string(),
            })?;
            let max_results = args
                .max_results
                .or_else(|| param_u32(args, &["max_results", "maxResults", "limit", "retmax"]));
            let data_args = crate::domain::search::data::DataSearchArgs { query, max_results };
            let response = if source == "auto" {
                if let Some(source_kind) = dataset_source_for_subcategory(subcategory.as_deref())? {
                    ensure_dataset_source_enabled(ctx, source_kind)?;
                    tokio::select! {
                        _ = ctx.cancel.cancelled() => return Err(ToolError::Cancelled),
                        r = client.search(source_kind, data_args) => r.map_err(|message| ToolError::ExecutionFailed { message })?,
                    }
                } else {
                    dataset_auto_search(ctx, &client, data_args).await?
                }
            } else {
                let source_kind = crate::domain::search::data::PublicDataSource::parse(&source)
                    .ok_or_else(|| ToolError::InvalidArguments {
                        message: format!("Unsupported dataset query source: {source}"),
                    })?;
                ensure_dataset_source_enabled(ctx, source_kind)?;
                tokio::select! {
                    _ = ctx.cancel.cancelled() => return Err(ToolError::Cancelled),
                    r = client.search(source_kind, data_args) => r.map_err(|message| ToolError::ExecutionFailed { message })?,
                }
            };
            let mut json = crate::domain::search::data::search_response_to_json(&response);
            annotate_query_json(&mut json, "search", "dataset");
            Ok(json_stream(json))
        }
        "fetch" | "get" | "detail" => {
            let identifier = identifier_text(args).ok_or_else(|| ToolError::InvalidArguments {
                message:
                    "query(category=dataset, operation=fetch) requires `id`, `url`, result, or params.id"
                        .to_string(),
            })?;
            let source_kind = if source == "auto" {
                infer_dataset_source(&identifier, subcategory.as_deref())?
            } else {
                crate::domain::search::data::PublicDataSource::parse(&source).ok_or_else(|| {
                    ToolError::InvalidArguments {
                        message: format!("Unsupported dataset query source: {source}"),
                    }
                })?
            };
            ensure_dataset_source_enabled(ctx, source_kind)?;
            let record = tokio::select! {
                _ = ctx.cancel.cancelled() => return Err(ToolError::Cancelled),
                r = client.fetch(source_kind, &identifier) => r.map_err(|message| ToolError::ExecutionFailed { message })?,
            };
            let mut json = crate::domain::search::data::detail_to_json(&record);
            annotate_query_json(&mut json, "fetch", "dataset");
            Ok(json_stream(json))
        }
        other => Err(ToolError::InvalidArguments {
            message: format!("Unsupported dataset query operation: {other}"),
        }),
    }
}

fn requested_source(args: &QueryArgs) -> String {
    let param_source = param_string(args, &["source"]);
    let result_source = string_from_result(args, &["source", "effective_source"]);
    normalized_source(
        args.source
            .as_deref()
            .or(param_source.as_deref())
            .or(result_source.as_deref()),
    )
}

fn annotate_query_json(value: &mut JsonValue, operation: &str, default_category: &str) {
    if let Some(obj) = value.as_object_mut() {
        obj.insert("tool".to_string(), json!("query"));
        obj.insert("operation".to_string(), json!(operation));
        obj.entry("category".to_string())
            .or_insert_with(|| json!(default_category));
    }
}

fn normalized_category(value: &str) -> String {
    match value.trim().to_ascii_lowercase().replace('-', "_").as_str() {
        "dataset" | "datasets" => "data".to_string(),
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

fn normalized_operation(args: &QueryArgs) -> String {
    let param_operation = param_string(args, &["operation", "op"]);
    let explicit = args.operation.as_deref().or(param_operation.as_deref());
    if let Some(op) = explicit.map(str::trim).filter(|s| !s.is_empty()) {
        return op.to_ascii_lowercase().replace('-', "_");
    }
    if identifier_text(args).is_some() {
        "fetch".to_string()
    } else {
        "search".to_string()
    }
}

async fn dataset_auto_search(
    ctx: &ToolContext,
    client: &crate::domain::search::data::PublicDataClient,
    data_args: crate::domain::search::data::DataSearchArgs,
) -> Result<crate::domain::search::data::DataSearchResponse, ToolError> {
    let enabled = ctx.web_search_api_keys.enabled_query_dataset_sources();
    let mut sources = Vec::new();
    if enabled.iter().any(|source| source == "geo") {
        sources.push(crate::domain::search::data::PublicDataSource::Geo);
    }
    if enabled.iter().any(|source| source == "ena") {
        sources.push(crate::domain::search::data::PublicDataSource::EnaStudy);
    }
    if enabled.iter().any(|source| source == "cbioportal") {
        sources.push(crate::domain::search::data::PublicDataSource::CbioPortal);
    }
    if sources.is_empty() {
        return Err(ToolError::InvalidArguments {
            message: "All dataset sources are disabled in Settings → Search. Enable at least one dataset source before using query(category=dataset).".to_string(),
        });
    }
    let mut results = Vec::new();
    let mut total = 0u64;
    let mut saw_total = false;
    let mut notes = vec!["Combined enabled dataset-source search".to_string()];
    for source in sources {
        let response = tokio::select! {
            _ = ctx.cancel.cancelled() => return Err(ToolError::Cancelled),
            r = client.search(source, data_args.clone()) => r,
        };
        match response {
            Ok(response) => {
                if let Some(count) = response.total {
                    total = total.saturating_add(count);
                    saw_total = true;
                }
                notes.extend(response.notes);
                results.extend(response.results);
            }
            Err(err) => notes.push(format!("{} source failed: {err}", source.as_str())),
        }
    }
    results.truncate(data_args.normalized_max_results() as usize);
    Ok(crate::domain::search::data::DataSearchResponse {
        query: data_args.query.trim().to_string(),
        source: "auto".to_string(),
        total: saw_total.then_some(total),
        results,
        notes,
    })
}

fn ensure_dataset_source_enabled(
    ctx: &ToolContext,
    source: crate::domain::search::data::PublicDataSource,
) -> Result<(), ToolError> {
    if ctx
        .web_search_api_keys
        .is_query_dataset_source_enabled(source.as_str())
    {
        return Ok(());
    }
    Err(ToolError::InvalidArguments {
        message: format!(
            "Dataset source `{}` is disabled in Settings → Search. Enable it before querying/fetching that source.",
            source.as_str()
        ),
    })
}

fn dataset_subcategory_id(subcategory: Option<&str>) -> Result<Option<&'static str>, ToolError> {
    let Some(subcategory) = subcategory else {
        return Ok(None);
    };
    match subcategory {
        "expression" | "gene_expression" | "transcriptomics" | "transcriptome" => {
            Ok(Some("expression"))
        }
        "sequencing" | "sequence_reads" | "raw_reads" | "reads" | "sra" => Ok(Some("sequencing")),
        "genomics" | "genome" | "genomes" | "assembly" | "assemblies" => Ok(Some("genomics")),
        "sample_metadata" | "sample" | "samples" | "metadata" => Ok(Some("sample_metadata")),
        "multi_omics" | "multiomics" | "projects" | "project" => Ok(Some("multi_omics")),
        other => Err(ToolError::InvalidArguments {
            message: format!("Unsupported dataset subcategory: {other}"),
        }),
    }
}

fn dataset_source_for_subcategory(
    subcategory: Option<&str>,
) -> Result<Option<crate::domain::search::data::PublicDataSource>, ToolError> {
    let Some(subcategory) = subcategory else {
        return Ok(None);
    };
    match subcategory {
        "expression" | "gene_expression" | "transcriptomics" | "transcriptome" => {
            Ok(Some(crate::domain::search::data::PublicDataSource::Geo))
        }
        "sequencing" | "sequence_reads" | "raw_reads" | "reads" | "sra" => {
            Ok(Some(crate::domain::search::data::PublicDataSource::EnaRun))
        }
        "genomics" | "genome" | "genomes" | "assembly" | "assemblies" => Ok(Some(
            crate::domain::search::data::PublicDataSource::EnaAssembly,
        )),
        "sample_metadata" | "sample" | "samples" | "metadata" => Ok(Some(
            crate::domain::search::data::PublicDataSource::EnaSample,
        )),
        "multi_omics" | "multiomics" | "projects" | "project" => Ok(Some(
            crate::domain::search::data::PublicDataSource::CbioPortal,
        )),
        other => Err(ToolError::InvalidArguments {
            message: format!("Unsupported dataset subcategory: {other}"),
        }),
    }
}

fn infer_dataset_source(
    identifier: &str,
    subcategory: Option<&str>,
) -> Result<crate::domain::search::data::PublicDataSource, ToolError> {
    if crate::domain::search::data::looks_like_geo_accession(identifier)
        || identifier
            .to_ascii_lowercase()
            .contains("ncbi.nlm.nih.gov/geo")
        || identifier
            .to_ascii_lowercase()
            .contains("ncbi.nlm.nih.gov/gds")
    {
        return Ok(crate::domain::search::data::PublicDataSource::Geo);
    }
    if let Some(source) = crate::domain::search::data::inferred_ena_source_key(identifier) {
        return crate::domain::search::data::PublicDataSource::parse(source).ok_or_else(|| {
            ToolError::InvalidArguments {
                message: format!("Unsupported inferred ENA source: {source}"),
            }
        });
    }
    if identifier.to_ascii_lowercase().contains("ebi.ac.uk/ena") {
        return Ok(crate::domain::search::data::PublicDataSource::EnaStudy);
    }
    if let Some(source) = dataset_source_for_subcategory(subcategory)? {
        return Ok(source);
    }
    Ok(crate::domain::search::data::PublicDataSource::Geo)
}

fn query_text(args: &QueryArgs) -> Option<String> {
    args.query
        .as_deref()
        .and_then(clean_nonempty)
        .or_else(|| param_string(args, &["query", "q", "term"]))
}

fn identifier_text(args: &QueryArgs) -> Option<String> {
    args.id
        .as_deref()
        .and_then(clean_nonempty)
        .or_else(|| args.url.as_deref().and_then(clean_nonempty))
        .or_else(|| string_from_result(args, &["accession", "gene_id", "id", "url", "link"]))
        .or_else(|| {
            metadata_string_from_result(
                args,
                &[
                    "accession",
                    "geo_accession",
                    "ena_accession",
                    "gene_id",
                    "ncbi_gene_id",
                    "uid",
                ],
            )
        })
        .or_else(|| param_string(args, &["id", "accession", "gene_id", "url"]))
}

fn param_string(args: &QueryArgs, keys: &[&str]) -> Option<String> {
    let map = args.params.as_ref()?.as_object()?;
    keys.iter()
        .find_map(|key| map.get(*key).and_then(json_string_value))
        .and_then(clean_string)
}

fn param_u32(args: &QueryArgs, keys: &[&str]) -> Option<u32> {
    let map = args.params.as_ref()?.as_object()?;
    keys.iter().find_map(|key| {
        let value = map.get(*key)?;
        value
            .as_u64()
            .and_then(|v| u32::try_from(v).ok())
            .or_else(|| value.as_str()?.trim().parse::<u32>().ok())
    })
}

fn string_from_result(args: &QueryArgs, keys: &[&str]) -> Option<String> {
    let result = args.result.as_ref()?.as_object()?;
    keys.iter()
        .find_map(|key| result.get(*key).and_then(json_string_value))
        .and_then(clean_string)
}

fn metadata_string_from_result(args: &QueryArgs, keys: &[&str]) -> Option<String> {
    let metadata = args.result.as_ref()?.get("metadata")?.as_object()?;
    keys.iter()
        .find_map(|key| metadata.get(*key).and_then(json_string_value))
        .and_then(clean_string)
}

fn json_string_value(value: &JsonValue) -> Option<String> {
    value
        .as_str()
        .map(str::to_string)
        .or_else(|| value.as_u64().map(|v| v.to_string()))
        .or_else(|| value.as_i64().map(|v| v.to_string()))
}

fn clean_nonempty(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn clean_string(value: String) -> Option<String> {
    clean_nonempty(&value)
}

fn json_stream(value: JsonValue) -> crate::infrastructure::streaming::StreamOutputBox {
    let text = serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string());
    QueryOutput { text }.into_stream()
}

#[derive(Debug, Clone)]
struct QueryOutput {
    text: String,
}

impl StreamOutput for QueryOutput {
    fn into_stream(self) -> Pin<Box<dyn futures::Stream<Item = StreamOutputItem> + Send>> {
        use futures::stream;
        Box::pin(stream::iter(vec![
            StreamOutputItem::Start,
            StreamOutputItem::Content(self.text),
            StreamOutputItem::Complete,
        ]))
    }
}

pub fn schema() -> ToolSchema {
    ToolSchema::new(
        "query",
        DESCRIPTION,
        serde_json::json!({
            "type": "object",
            "properties": {
                "category": {
                    "type": "string",
                    "description": "Database category. Supports dataset (alias: data) and knowledge."
                },
                "source": {
                    "type": "string",
                    "description": "Database source. Dataset supports auto, geo, ena, ena_run, ena_experiment, ena_sample, ena_analysis, ena_assembly, ena_sequence. Knowledge supports ncbi_gene."
                },
                "operation": {
                    "type": "string",
                    "description": "Operation to run: search/query for record search; fetch/get/detail for one accession or URL. Defaults from supplied fields."
                },
                "subcategory": {
                    "type": "string",
                    "description": "Dataset routing hint: expression, sequencing, genomics, sample_metadata, multi_omics."
                },
                "query": {
                    "type": "string",
                    "description": "Keyword or database query string for operation=search/query."
                },
                "id": {
                    "type": "string",
                    "description": "Source-specific identifier/accession for operation=fetch/get/detail."
                },
                "url": {
                    "type": "string",
                    "description": "Source-specific record URL for operation=fetch/get/detail."
                },
                "result": {
                    "type": "object",
                    "description": "A result object returned by search/query; query will read source, id, accession, link/url, and metadata."
                },
                "params": {
                    "type": "object",
                    "description": "Database-specific structured parameters. Dataset sources accept query/q, id/accession/url, source, operation, subcategory, and max_results/limit. NCBI Gene accepts organism, taxon_id/taxid, ret_start/retstart, and sort."
                },
                "max_results": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 25,
                    "description": "Maximum records for search/query operations."
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
    fn normalizes_dataset_categories_and_operations() {
        let args = QueryArgs {
            category: "dataset".to_string(),
            source: None,
            operation: None,
            subcategory: Some("sample metadata".to_string()),
            query: Some("lung".to_string()),
            id: None,
            url: None,
            result: None,
            params: None,
            max_results: None,
        };
        assert_eq!(normalized_category(&args.category), "data");
        assert_eq!(normalized_operation(&args), "search");
        assert_eq!(
            normalized_subcategory(args.subcategory.as_deref()).as_deref(),
            Some("sample_metadata")
        );
        assert_eq!(
            dataset_source_for_subcategory(Some("sample_metadata")).unwrap(),
            Some(crate::domain::search::data::PublicDataSource::EnaSample)
        );
    }

    #[test]
    fn infers_fetch_operation_and_source_from_identifier() {
        let args = QueryArgs {
            category: "dataset".to_string(),
            source: None,
            operation: None,
            subcategory: None,
            query: None,
            id: Some("ERR123".to_string()),
            url: None,
            result: None,
            params: None,
            max_results: None,
        };
        assert_eq!(normalized_operation(&args), "fetch");
        assert_eq!(
            infer_dataset_source("ERR123", None).unwrap(),
            crate::domain::search::data::PublicDataSource::EnaRun
        );
    }

    #[test]
    fn reads_gene_identifier_from_search_result_metadata() {
        let args = QueryArgs {
            category: "knowledge".to_string(),
            source: Some("ncbi_gene".to_string()),
            operation: None,
            subcategory: None,
            query: None,
            id: None,
            url: None,
            result: Some(json!({
                "source": "ncbi_gene",
                "metadata": {"gene_id": 7157}
            })),
            params: None,
            max_results: None,
        };

        assert_eq!(normalized_operation(&args), "fetch");
        assert_eq!(requested_source(&args), "ncbi_gene");
        assert_eq!(identifier_text(&args).as_deref(), Some("7157"));
    }
}
