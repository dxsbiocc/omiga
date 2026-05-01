use super::common::{
    annotate_query_json, identifier_text, json_stream, normalized_operation,
    normalized_subcategory, param_string, param_u32, query_text, requested_source,
};
use super::QueryArgs;
use crate::domain::tools::{ToolContext, ToolError};

mod auto_search;
mod routing;

use routing::{
    dataset_source_for_subcategory, dataset_subcategory_id, ensure_dataset_source_enabled,
    infer_dataset_source, resolve_dataset_source,
};

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
                    auto_search::dataset_auto_search(ctx, &client, data_args).await?
                }
            } else {
                let source_kind = resolve_dataset_source(&source)?;
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
                resolve_dataset_source(&source)?
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
        "download_summary" | "download_summary_preview" | "download_preview" => {
            let identifier = identifier_text(args).ok_or_else(|| ToolError::InvalidArguments {
                message:
                    "query(category=dataset, source=ncbi_datasets, operation=download_summary) requires `id`, `url`, result, or params.id"
                        .to_string(),
            })?;
            let source_kind = if source == "auto" {
                infer_dataset_source(&identifier, subcategory.as_deref())?
            } else {
                resolve_dataset_source(&source)?
            };
            ensure_dataset_source_enabled(ctx, source_kind)?;
            if source_kind != crate::domain::search::data::PublicDataSource::NcbiDatasets {
                return Err(ToolError::InvalidArguments {
                    message: format!(
                        "download_summary is only supported for dataset source `ncbi_datasets`, not `{}`.",
                        source_kind.as_str()
                    ),
                });
            }
            let mut json = tokio::select! {
                _ = ctx.cancel.cancelled() => return Err(ToolError::Cancelled),
                r = client.ncbi_datasets_download_summary(&identifier, args.params.as_ref()) => r.map_err(|message| ToolError::ExecutionFailed { message })?,
            };
            annotate_query_json(&mut json, "download_summary", "dataset");
            Ok(json_stream(json))
        }
        other => Err(ToolError::InvalidArguments {
            message: format!("Unsupported dataset query operation: {other}"),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::routing::{dataset_source_for_subcategory, infer_dataset_source};
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
        assert_eq!(
            infer_dataset_source("SAMN15960293", None).unwrap(),
            crate::domain::search::data::PublicDataSource::BioSample
        );
    }
}
