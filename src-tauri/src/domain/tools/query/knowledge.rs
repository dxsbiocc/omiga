use super::common::{
    annotate_query_json, ensure_registry_source_can_query, identifier_text, json_stream,
    normalized_operation, param_bool, param_string, param_u32, query_text, requested_source,
};
use super::QueryArgs;
use crate::domain::retrieval_registry::{self, RetrievalCapability};
use crate::domain::tools::{ToolContext, ToolError};

pub(super) async fn query_knowledge(
    ctx: &ToolContext,
    args: &QueryArgs,
) -> Result<crate::infrastructure::streaming::StreamOutputBox, ToolError> {
    let requested = requested_source(args);
    let source = canonical_knowledge_source(&requested)?;
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

    match source.as_str() {
        "ncbi_gene" => query_ncbi_gene(ctx, args).await,
        "uniprot" => query_uniprot(ctx, args).await,
        _ => Err(ToolError::InvalidArguments {
            message: format!("Unsupported knowledge query source: {source}"),
        }),
    }
}

fn canonical_knowledge_source(source: &str) -> Result<String, ToolError> {
    let source = if source == "auto" {
        "ncbi_gene"
    } else {
        source
    };
    let Some(def) = retrieval_registry::find_source("knowledge", source) else {
        return Err(ToolError::InvalidArguments {
            message: format!(
                "Unsupported knowledge query source: {source}. Supported available query sources: ncbi_gene, uniprot."
            ),
        });
    };
    ensure_registry_source_can_query(&def)?;
    if !def.supports(RetrievalCapability::Query) {
        return Err(ToolError::InvalidArguments {
            message: format!("Knowledge source `{}` does not support query.", def.id),
        });
    }
    Ok(def.id.to_string())
}

async fn query_ncbi_gene(
    ctx: &ToolContext,
    args: &QueryArgs,
) -> Result<crate::infrastructure::streaming::StreamOutputBox, ToolError> {
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

async fn query_uniprot(
    ctx: &ToolContext,
    args: &QueryArgs,
) -> Result<crate::infrastructure::streaming::StreamOutputBox, ToolError> {
    let operation = normalized_operation(args);
    let client = crate::domain::search::uniprot::UniProtClient::from_tool_context(ctx)
        .map_err(|message| ToolError::ExecutionFailed { message })?;

    match operation.as_str() {
        "search" | "query" => {
            let query = query_text(args).ok_or_else(|| ToolError::InvalidArguments {
                message:
                    "query(category=knowledge, source=uniprot, operation=search) requires `query` or params.query"
                        .to_string(),
            })?;
            let protein_args = crate::domain::search::uniprot::UniProtSearchArgs {
                query,
                organism: param_string(args, &["organism", "species"]),
                taxon_id: param_string(args, &["taxon_id", "taxid", "tax_id", "taxonomy_id"]),
                reviewed: param_bool(args, &["reviewed", "swiss_prot", "swissprot"]),
                max_results: args
                    .max_results
                    .or_else(|| param_u32(args, &["max_results", "maxResults", "limit", "size"])),
            };
            let response = tokio::select! {
                _ = ctx.cancel.cancelled() => return Err(ToolError::Cancelled),
                r = client.search(protein_args) => r.map_err(|message| ToolError::ExecutionFailed { message })?,
            };
            let mut json = crate::domain::search::uniprot::search_response_to_json(&response);
            annotate_query_json(&mut json, "search", "knowledge");
            Ok(json_stream(json))
        }
        "fetch" | "get" | "detail" => {
            let identifier = identifier_text(args).ok_or_else(|| ToolError::InvalidArguments {
                message:
                    "query(category=knowledge, source=uniprot, operation=fetch) requires a UniProt accession, URL, result, or params.id"
                        .to_string(),
            })?;
            let record = tokio::select! {
                _ = ctx.cancel.cancelled() => return Err(ToolError::Cancelled),
                r = client.fetch(&identifier) => r.map_err(|message| ToolError::ExecutionFailed { message })?,
            };
            let mut json = crate::domain::search::uniprot::detail_to_json(&record);
            annotate_query_json(&mut json, "fetch", "knowledge");
            Ok(json_stream(json))
        }
        other => Err(ToolError::InvalidArguments {
            message: format!("Unsupported UniProt query operation: {other}"),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonicalizes_uniprot_knowledge_aliases() {
        assert_eq!(canonical_knowledge_source("uniprotkb").unwrap(), "uniprot");
        assert_eq!(canonical_knowledge_source("protein").unwrap(), "uniprot");
    }

    #[test]
    fn rejects_planned_knowledge_sources_from_registry() {
        let err = canonical_knowledge_source("ensembl").unwrap_err();
        assert!(matches!(err, ToolError::InvalidArguments { .. }));
    }

    #[test]
    fn available_query_registry_sources_are_routable() {
        for source in retrieval_registry::registry().sources {
            if !source.can_execute() || !source.supports(RetrievalCapability::Query) {
                continue;
            }
            match source.category {
                "dataset" => {
                    assert!(
                        crate::domain::search::data::PublicDataSource::parse(source.id).is_some(),
                        "dataset source `{}` must route to a data adapter",
                        source.id
                    );
                }
                "knowledge" => {
                    assert!(
                        canonical_knowledge_source(source.id).is_ok(),
                        "knowledge source `{}` must route to a knowledge adapter",
                        source.id
                    );
                }
                other => panic!("query-capable source in unsupported category: {other}"),
            }
        }
    }
}
