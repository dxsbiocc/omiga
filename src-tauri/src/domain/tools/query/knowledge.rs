use super::common::{
    annotate_query_json, ensure_registry_source_can_query, identifier_text, normalized_operation,
    param_bool, param_string, param_u32, query_text, requested_source,
};
use super::QueryArgs;
use crate::domain::retrieval_registry::{self, RetrievalCapability};
use crate::domain::tools::{ToolContext, ToolError};
use serde_json::Value as JsonValue;

const BUILTIN_KNOWLEDGE_QUERY_SOURCES: &[&str] = &["ncbi_gene", "ensembl", "uniprot"];

pub(super) async fn query_knowledge_json(
    ctx: &ToolContext,
    args: &QueryArgs,
) -> Result<JsonValue, ToolError> {
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
        "ncbi_gene" => query_ncbi_gene_json(ctx, args).await,
        "ensembl" => query_ensembl_json(ctx, args).await,
        "uniprot" => query_uniprot_json(ctx, args).await,
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
                "Unsupported knowledge query source: {source}. Supported available query sources: {}.",
                BUILTIN_KNOWLEDGE_QUERY_SOURCES.join(", ")
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

async fn query_ensembl_json(ctx: &ToolContext, args: &QueryArgs) -> Result<JsonValue, ToolError> {
    let operation = normalized_operation(args);
    let client = crate::domain::search::ensembl::EnsemblClient::from_tool_context(ctx)
        .map_err(|message| ToolError::ExecutionFailed { message })?;

    match operation.as_str() {
        "search" | "query" => {
            let query = query_text(args).ok_or_else(|| ToolError::InvalidArguments {
                message:
                    "query(category=knowledge, source=ensembl, operation=search) requires `query` or params.query"
                        .to_string(),
            })?;
            let ensembl_args = crate::domain::search::ensembl::EnsemblSearchArgs {
                query,
                species: param_string(args, &["species", "organism"]),
                object_type: param_string(args, &["object_type", "type"]),
                max_results: args
                    .max_results
                    .or_else(|| param_u32(args, &["max_results", "maxResults", "limit", "size"])),
            };
            let response = tokio::select! {
                _ = ctx.cancel.cancelled() => return Err(ToolError::Cancelled),
                r = client.search(ensembl_args) => r.map_err(|message| ToolError::ExecutionFailed { message })?,
            };
            let mut json = crate::domain::search::ensembl::search_response_to_json(&response);
            annotate_query_json(&mut json, "search", "knowledge");
            Ok(json)
        }
        "fetch" | "get" | "detail" => {
            let identifier = identifier_text(args).ok_or_else(|| ToolError::InvalidArguments {
                message:
                    "query(category=knowledge, source=ensembl, operation=fetch) requires an Ensembl stable ID, rsID, symbol, URL, result, or params.id"
                        .to_string(),
            })?;
            let species = param_string(args, &["species", "organism"]);
            let record = tokio::select! {
                _ = ctx.cancel.cancelled() => return Err(ToolError::Cancelled),
                r = client.fetch(&identifier, species.as_deref()) => r.map_err(|message| ToolError::ExecutionFailed { message })?,
            };
            let mut json = crate::domain::search::ensembl::detail_to_json(&record);
            annotate_query_json(&mut json, "fetch", "knowledge");
            Ok(json)
        }
        other => Err(ToolError::InvalidArguments {
            message: format!("Unsupported Ensembl query operation: {other}"),
        }),
    }
}

async fn query_ncbi_gene_json(ctx: &ToolContext, args: &QueryArgs) -> Result<JsonValue, ToolError> {
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
            Ok(json)
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
            Ok(json)
        }
        other => Err(ToolError::InvalidArguments {
            message: format!("Unsupported NCBI Gene query operation: {other}"),
        }),
    }
}

async fn query_uniprot_json(ctx: &ToolContext, args: &QueryArgs) -> Result<JsonValue, ToolError> {
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
            Ok(json)
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
            Ok(json)
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
    fn canonicalizes_ensembl_knowledge_aliases() {
        assert_eq!(
            canonical_knowledge_source("ensembl_gene").unwrap(),
            "ensembl"
        );
        assert_eq!(
            canonical_knowledge_source("transcripts").unwrap(),
            "ensembl"
        );
    }

    #[test]
    fn rejects_planned_knowledge_sources_from_registry() {
        let err = canonical_knowledge_source("reactome").unwrap_err();
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

    #[test]
    fn builtin_knowledge_query_sources_match_registry() {
        let expected: std::collections::HashSet<_> =
            BUILTIN_KNOWLEDGE_QUERY_SOURCES.iter().copied().collect();
        let registered: std::collections::HashSet<_> = retrieval_registry::registry()
            .sources
            .into_iter()
            .filter(|source| {
                source.category == "knowledge"
                    && source.can_execute()
                    && source.supports(RetrievalCapability::Query)
            })
            .map(|source| source.id)
            .collect();

        assert_eq!(
            registered, expected,
            "update query/knowledge.rs routing when changing available built-in knowledge query sources"
        );
    }
}
