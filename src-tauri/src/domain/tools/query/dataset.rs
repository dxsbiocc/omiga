use super::common::{
    annotate_query_json, ensure_registry_source_can_query, identifier_text, json_stream,
    normalized_operation, normalized_subcategory, param_string, param_u32, query_text,
    requested_source,
};
use super::QueryArgs;
use crate::domain::retrieval_registry;
use crate::domain::tools::{ToolContext, ToolError};

pub(super) async fn query_dataset(
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
            let data_args = crate::domain::search::data::DataSearchArgs {
                query,
                max_results,
                params: args.params.clone(),
            };
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
                let registry_source = retrieval_registry::find_source("dataset", &source)
                    .ok_or_else(|| ToolError::InvalidArguments {
                        message: format!("Unsupported dataset query source: {source}"),
                    })?;
                ensure_registry_source_can_query(&registry_source)?;
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
                let registry_source = retrieval_registry::find_source("dataset", &source)
                    .ok_or_else(|| ToolError::InvalidArguments {
                        message: format!("Unsupported dataset query source: {source}"),
                    })?;
                ensure_registry_source_can_query(&registry_source)?;
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
    if enabled.iter().any(|source| source == "gtex") {
        sources.push(crate::domain::search::data::PublicDataSource::Gtex);
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
    if crate::domain::search::data::looks_like_gtex_identifier(identifier)
        || identifier.to_ascii_lowercase().contains("gtexportal.org")
    {
        return Ok(crate::domain::search::data::PublicDataSource::Gtex);
    }
    if let Some(source) = dataset_source_for_subcategory(subcategory)? {
        return Ok(source);
    }
    Ok(crate::domain::search::data::PublicDataSource::Geo)
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
        assert_eq!(
            super::super::common::normalized_category(&args.category),
            "data"
        );
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
}
