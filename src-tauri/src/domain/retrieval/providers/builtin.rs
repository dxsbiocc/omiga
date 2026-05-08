use crate::domain::retrieval::{
    RetrievalError, RetrievalItem, RetrievalOperation, RetrievalProvider, RetrievalProviderKind,
    RetrievalProviderOutput, RetrievalRequest, RetrievalResponse, RetrievalTool,
};
use crate::domain::tools::{fetch::FetchArgs, query::QueryArgs, search::SearchArgs, ToolContext};
use crate::errors::ToolError;
use async_trait::async_trait;
use serde_json::{json, Value as JsonValue};

#[derive(Debug, Clone, Default)]
pub struct BuiltinProvider;

#[async_trait]
impl RetrievalProvider for BuiltinProvider {
    async fn execute(
        &self,
        ctx: &ToolContext,
        request: RetrievalRequest,
    ) -> Result<RetrievalProviderOutput, RetrievalError> {
        match (request.tool, request.category.as_str()) {
            (RetrievalTool::Search, "dataset" | "data") => {
                execute_dataset_search(ctx, &request).await.map(Into::into)
            }
            (RetrievalTool::Search, "web") => {
                execute_web_search(ctx, &request).await.map(Into::into)
            }
            (RetrievalTool::Search, "literature") => execute_literature_search(ctx, &request)
                .await
                .map(Into::into),
            (RetrievalTool::Search, "social") => {
                execute_social_search(ctx, &request).await.map(Into::into)
            }
            (RetrievalTool::Search, "knowledge") => {
                execute_knowledge_recall_search(ctx, &request).await
            }
            (RetrievalTool::Fetch, "dataset" | "data") => {
                execute_dataset_fetch(ctx, &request).await.map(Into::into)
            }
            (RetrievalTool::Fetch, "web") => execute_web_fetch(ctx, &request).await.map(Into::into),
            (RetrievalTool::Fetch, "literature") => execute_literature_fetch(ctx, &request)
                .await
                .map(Into::into),
            (RetrievalTool::Fetch, "social") => {
                execute_social_fetch(ctx, &request).await.map(Into::into)
            }
            (RetrievalTool::Query, "dataset" | "data") => {
                execute_dataset_query(ctx, &request).await.map(Into::into)
            }
            (RetrievalTool::Query, "knowledge") => {
                execute_knowledge_query(ctx, &request).await.map(Into::into)
            }
            _ => Err(unsupported_builtin_route(&request)),
        }
    }
}

async fn execute_dataset_search(
    ctx: &ToolContext,
    request: &RetrievalRequest,
) -> Result<RetrievalResponse, RetrievalError> {
    let args = search_args_from_request(request)?;
    let result = crate::domain::tools::search::execute_builtin_data_search(ctx, &args)
        .await
        .map_err(tool_error_to_retrieval_error)?;
    Ok(match result {
        crate::domain::tools::search::BuiltinDataSearchResult::Response(response) => {
            data_search_response_to_retrieval(request, response)
        }
        crate::domain::tools::search::BuiltinDataSearchResult::StructuredError(value) => {
            structured_search_error_to_retrieval(request, value)
        }
    })
}

async fn execute_web_search(
    ctx: &ToolContext,
    request: &RetrievalRequest,
) -> Result<RetrievalResponse, RetrievalError> {
    let args = search_args_from_request(request)?;
    let raw = crate::domain::tools::search::execute_builtin_web_search_json(ctx, &args)
        .await
        .map_err(tool_error_to_retrieval_error)?;
    Ok(search_json_to_retrieval_response(request, raw))
}

async fn execute_literature_search(
    ctx: &ToolContext,
    request: &RetrievalRequest,
) -> Result<RetrievalResponse, RetrievalError> {
    let args = search_args_from_request(request)?;
    let raw = crate::domain::tools::search::execute_builtin_literature_search_json(ctx, &args)
        .await
        .map_err(tool_error_to_retrieval_error)?;
    Ok(search_json_to_retrieval_response(request, raw))
}

async fn execute_social_search(
    ctx: &ToolContext,
    request: &RetrievalRequest,
) -> Result<RetrievalResponse, RetrievalError> {
    let args = search_args_from_request(request)?;
    let raw = crate::domain::tools::search::execute_builtin_social_search_json(
        ctx,
        &args,
        &request.source,
    )
    .await
    .map_err(tool_error_to_retrieval_error)?;
    Ok(search_json_to_retrieval_response(request, raw))
}

async fn execute_knowledge_recall_search(
    ctx: &ToolContext,
    request: &RetrievalRequest,
) -> Result<RetrievalProviderOutput, RetrievalError> {
    let stream = crate::domain::tools::search::execute_builtin_search(
        ctx,
        search_args_from_request(request)?,
    )
    .await
    .map_err(tool_error_to_retrieval_error)?;
    Ok(RetrievalProviderOutput::Stream(stream))
}

async fn execute_dataset_fetch(
    ctx: &ToolContext,
    request: &RetrievalRequest,
) -> Result<RetrievalResponse, RetrievalError> {
    let raw = crate::domain::tools::fetch::execute_builtin_data_fetch_json(
        ctx,
        &fetch_args_from_request(request),
    )
    .await
    .map_err(tool_error_to_retrieval_error)?;
    Ok(detail_json_to_retrieval_response(request, raw))
}

async fn execute_web_fetch(
    ctx: &ToolContext,
    request: &RetrievalRequest,
) -> Result<RetrievalResponse, RetrievalError> {
    let raw = crate::domain::tools::fetch::execute_builtin_web_fetch_json(
        ctx,
        &fetch_args_from_request(request),
        &request.source,
    )
    .await
    .map_err(tool_error_to_retrieval_error)?;
    Ok(detail_json_to_retrieval_response(request, raw))
}

async fn execute_literature_fetch(
    ctx: &ToolContext,
    request: &RetrievalRequest,
) -> Result<RetrievalResponse, RetrievalError> {
    let raw = crate::domain::tools::fetch::execute_builtin_literature_fetch_json(
        ctx,
        &fetch_args_from_request(request),
        &request.source,
    )
    .await
    .map_err(tool_error_to_retrieval_error)?;
    Ok(detail_json_to_retrieval_response(request, raw))
}

async fn execute_social_fetch(
    ctx: &ToolContext,
    request: &RetrievalRequest,
) -> Result<RetrievalResponse, RetrievalError> {
    let raw = crate::domain::tools::fetch::execute_builtin_social_fetch_json(
        ctx,
        &fetch_args_from_request(request),
        &request.source,
    )
    .await
    .map_err(tool_error_to_retrieval_error)?;
    Ok(detail_json_to_retrieval_response(request, raw))
}

async fn execute_dataset_query(
    ctx: &ToolContext,
    request: &RetrievalRequest,
) -> Result<RetrievalResponse, RetrievalError> {
    let raw = crate::domain::tools::query::execute_builtin_dataset_query_json(
        ctx,
        &query_args_from_request(request),
    )
    .await
    .map_err(tool_error_to_retrieval_error)?;
    Ok(builtin_query_json_to_retrieval_response(request, raw))
}

async fn execute_knowledge_query(
    ctx: &ToolContext,
    request: &RetrievalRequest,
) -> Result<RetrievalResponse, RetrievalError> {
    let raw = crate::domain::tools::query::execute_builtin_knowledge_query_json(
        ctx,
        &query_args_from_request(request),
    )
    .await
    .map_err(tool_error_to_retrieval_error)?;
    Ok(builtin_query_json_to_retrieval_response(request, raw))
}

fn unsupported_builtin_route(request: &RetrievalRequest) -> RetrievalError {
    let category = request.public_category();
    let message = match request.tool {
        RetrievalTool::Search => format!("Unsupported search category: {category}"),
        RetrievalTool::Fetch if category == "knowledge" => {
            "fetch(category=knowledge) is not supported; use query(category=knowledge, operation=fetch) for structured knowledge records or recall(query=...) for local knowledge."
                .to_string()
        }
        RetrievalTool::Fetch => format!("Unsupported fetch category: {category}"),
        RetrievalTool::Query => format!(
            "Unsupported query category: {category}. Supported categories: dataset/data, knowledge."
        ),
    };
    RetrievalError::InvalidRequest { message }
}

fn data_search_response_to_retrieval(
    request: &RetrievalRequest,
    response: crate::domain::search::data::DataSearchResponse,
) -> RetrievalResponse {
    let raw = crate::domain::search::data::search_response_to_json(&response);
    search_json_to_retrieval_response(request, raw)
}

fn search_json_to_retrieval_response(
    request: &RetrievalRequest,
    raw: JsonValue,
) -> RetrievalResponse {
    if raw.get("error").is_some() {
        return structured_search_error_to_retrieval(request, raw);
    }

    let items = raw
        .get("results")
        .and_then(JsonValue::as_array)
        .map(|results| {
            results
                .iter()
                .map(search_result_json_to_retrieval_item)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let (source, effective_source) = source_fields_from_raw(request, &raw);
    let total = raw
        .get("total")
        .or_else(|| raw.get("count"))
        .and_then(JsonValue::as_u64);
    let notes = raw
        .get("route_notes")
        .and_then(JsonValue::as_array)
        .map(|notes| {
            notes
                .iter()
                .filter_map(JsonValue::as_str)
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    RetrievalResponse {
        operation: RetrievalOperation::Search,
        category: request.category.clone(),
        source,
        effective_source,
        provider: RetrievalProviderKind::Builtin,
        plugin: None,
        items,
        detail: None,
        total,
        notes,
        raw: Some(raw),
    }
}

fn structured_search_error_to_retrieval(
    request: &RetrievalRequest,
    value: JsonValue,
) -> RetrievalResponse {
    let (source, effective_source) = source_fields_from_raw(request, &value);
    RetrievalResponse {
        operation: RetrievalOperation::Search,
        category: request.category.clone(),
        source,
        effective_source,
        provider: RetrievalProviderKind::Builtin,
        plugin: None,
        items: Vec::new(),
        detail: None,
        total: Some(0),
        notes: message_notes_from_raw(&value),
        raw: Some(value),
    }
}

fn builtin_query_json_to_retrieval_response(
    request: &RetrievalRequest,
    raw: JsonValue,
) -> RetrievalResponse {
    let operation = raw
        .get("operation")
        .and_then(JsonValue::as_str)
        .unwrap_or(request.operation.as_str())
        .to_string();
    match operation.as_str() {
        "search" | "query" => search_json_to_retrieval_response(request, raw),
        "download_summary" | "download_summary_preview" | "download_preview" => {
            raw_json_to_retrieval_response(request, raw)
        }
        _ => detail_json_to_retrieval_response(request, raw),
    }
}

fn detail_json_to_retrieval_response(
    request: &RetrievalRequest,
    raw: JsonValue,
) -> RetrievalResponse {
    if raw.get("error").is_some() {
        return raw_json_to_retrieval_response(request, raw);
    }

    let detail = detail_json_to_retrieval_item(&raw);
    let (source, effective_source) = source_fields_from_raw(request, &raw);
    RetrievalResponse {
        operation: request.operation,
        category: request.category.clone(),
        source,
        effective_source,
        provider: RetrievalProviderKind::Builtin,
        plugin: None,
        items: vec![detail.clone()],
        detail: Some(detail),
        total: Some(1),
        notes: Vec::new(),
        raw: Some(raw),
    }
}

fn raw_json_to_retrieval_response(request: &RetrievalRequest, raw: JsonValue) -> RetrievalResponse {
    let (source, effective_source) = source_fields_from_raw(request, &raw);
    RetrievalResponse {
        operation: request.operation,
        category: request.category.clone(),
        source,
        effective_source,
        provider: RetrievalProviderKind::Builtin,
        plugin: None,
        items: Vec::new(),
        detail: None,
        total: None,
        notes: message_notes_from_raw(&raw),
        raw: Some(raw),
    }
}

fn source_fields_from_raw(request: &RetrievalRequest, raw: &JsonValue) -> (String, String) {
    let source = raw
        .get("source")
        .and_then(JsonValue::as_str)
        .unwrap_or(&request.source)
        .to_string();
    let effective_source = raw
        .get("effective_source")
        .and_then(JsonValue::as_str)
        .unwrap_or(&source)
        .to_string();
    (source, effective_source)
}

fn message_notes_from_raw(raw: &JsonValue) -> Vec<String> {
    raw.get("message")
        .and_then(JsonValue::as_str)
        .map(|message| vec![message.to_string()])
        .unwrap_or_default()
}

fn search_result_json_to_retrieval_item(value: &JsonValue) -> RetrievalItem {
    RetrievalItem {
        id: json_string(value, "id"),
        accession: json_string(value, "accession"),
        title: json_string(value, "title").or_else(|| json_string(value, "name")),
        url: json_string(value, "url").or_else(|| json_string(value, "link")),
        snippet: json_string(value, "snippet"),
        content: json_string(value, "content"),
        favicon: json_string(value, "favicon"),
        metadata: value.get("metadata").cloned().unwrap_or_else(|| json!({})),
        raw: Some(value.clone()),
    }
}

fn detail_json_to_retrieval_item(value: &JsonValue) -> RetrievalItem {
    RetrievalItem {
        id: json_string(value, "id"),
        accession: json_string(value, "accession"),
        title: json_string(value, "title").or_else(|| json_string(value, "name")),
        url: json_string(value, "url").or_else(|| json_string(value, "link")),
        snippet: json_string(value, "snippet").or_else(|| json_string(value, "abstract")),
        content: json_string(value, "content"),
        favicon: json_string(value, "favicon"),
        metadata: value.get("metadata").cloned().unwrap_or_else(|| json!({})),
        raw: Some(value.clone()),
    }
}

fn json_string(value: &JsonValue, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(JsonValue::as_str)
        .map(ToString::to_string)
        .filter(|value| !value.trim().is_empty())
}

fn search_args_from_request(request: &RetrievalRequest) -> Result<SearchArgs, RetrievalError> {
    let Some(query) = request.query.clone() else {
        return Err(RetrievalError::InvalidRequest {
            message: "search retrieval request requires query".to_string(),
        });
    };
    Ok(SearchArgs {
        category: request.public_category().to_string(),
        source: Some(request.source.clone()),
        subcategory: request.subcategory.clone(),
        query,
        allowed_domains: request
            .web
            .as_ref()
            .and_then(|web| web.allowed_domains.clone()),
        blocked_domains: request
            .web
            .as_ref()
            .and_then(|web| web.blocked_domains.clone()),
        max_results: request.max_results,
        search_url: request.web.as_ref().and_then(|web| web.search_url.clone()),
    })
}

fn fetch_args_from_request(request: &RetrievalRequest) -> FetchArgs {
    FetchArgs {
        category: request.public_category().to_string(),
        source: Some(request.source.clone()),
        subcategory: request.subcategory.clone(),
        url: request.url.clone(),
        id: request.id.clone(),
        result: request.result.clone(),
        prompt: request.prompt.clone(),
    }
}

fn query_args_from_request(request: &RetrievalRequest) -> QueryArgs {
    QueryArgs {
        category: request.public_category().to_string(),
        source: Some(request.source.clone()),
        operation: Some(request.operation.as_str().to_string()),
        subcategory: request.subcategory.clone(),
        query: request.query.clone(),
        id: request.id.clone(),
        url: request.url.clone(),
        result: request.result.clone(),
        params: request.params.clone(),
        max_results: request.max_results,
    }
}

fn tool_error_to_retrieval_error(error: ToolError) -> RetrievalError {
    match error {
        ToolError::InvalidArguments { message } => RetrievalError::InvalidRequest { message },
        ToolError::Cancelled => RetrievalError::Cancelled,
        ToolError::Timeout { seconds } => RetrievalError::Timeout { seconds },
        ToolError::ExecutionFailed { message }
        | ToolError::PermissionDenied { action: message }
        | ToolError::UnknownTool { name: message } => RetrievalError::ExecutionFailed { message },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::retrieval::output;
    use crate::domain::retrieval::types::{RetrievalOperation, RetrievalWebOptions};
    use crate::domain::search::data::{DataRecord, DataSearchResponse, PublicDataSource};
    use serde_json::json;

    fn base_request(tool: RetrievalTool) -> RetrievalRequest {
        RetrievalRequest {
            request_id: "req".to_string(),
            tool,
            operation: match tool {
                RetrievalTool::Search => RetrievalOperation::Search,
                RetrievalTool::Fetch => RetrievalOperation::Fetch,
                RetrievalTool::Query => RetrievalOperation::Search,
            },
            category: "dataset".to_string(),
            source: "geo".to_string(),
            subcategory: Some("sample_metadata".to_string()),
            query: Some("brca1".to_string()),
            id: Some("GSE1".to_string()),
            url: None,
            result: Some(json!({"source":"geo"})),
            params: Some(json!({"limit": 2})),
            max_results: Some(2),
            prompt: Some("summarize".to_string()),
            web: Some(RetrievalWebOptions {
                allowed_domains: Some(vec!["example.org".to_string()]),
                blocked_domains: None,
                search_url: Some("https://html.duckduckgo.com/html/".to_string()),
            }),
        }
    }

    fn data_record(source: PublicDataSource) -> DataRecord {
        DataRecord {
            id: "1".to_string(),
            accession: "GSE1".to_string(),
            source,
            title: "BRCA1 expression".to_string(),
            summary: "Breast cancer expression data".to_string(),
            url: "https://www.ncbi.nlm.nih.gov/geo/query/acc.cgi?acc=GSE1".to_string(),
            record_type: Some("Series".to_string()),
            organism: Some("Homo sapiens".to_string()),
            published_date: Some("2020-01-01".to_string()),
            updated_date: None,
            sample_count: Some(12),
            platform: Some("GPL570".to_string()),
            files: vec!["GSE1_family.soft.gz".to_string()],
            extra: serde_json::Map::new(),
        }
    }

    fn literature_request() -> RetrievalRequest {
        RetrievalRequest {
            request_id: "req-lit".to_string(),
            tool: RetrievalTool::Search,
            operation: RetrievalOperation::Search,
            category: "literature".to_string(),
            source: "pubmed".to_string(),
            subcategory: None,
            query: Some("brca1".to_string()),
            id: None,
            url: None,
            result: None,
            params: None,
            max_results: Some(2),
            prompt: None,
            web: Some(RetrievalWebOptions::default()),
        }
    }

    fn web_request() -> RetrievalRequest {
        RetrievalRequest {
            request_id: "req-web".to_string(),
            tool: RetrievalTool::Search,
            operation: RetrievalOperation::Search,
            category: "web".to_string(),
            source: "auto".to_string(),
            subcategory: None,
            query: Some("rust async".to_string()),
            id: None,
            url: None,
            result: None,
            params: None,
            max_results: Some(2),
            prompt: None,
            web: Some(RetrievalWebOptions {
                allowed_domains: None,
                blocked_domains: Some(vec!["blocked.example".to_string()]),
                search_url: None,
            }),
        }
    }

    fn social_request() -> RetrievalRequest {
        RetrievalRequest {
            request_id: "req-social".to_string(),
            tool: RetrievalTool::Search,
            operation: RetrievalOperation::Search,
            category: "social".to_string(),
            source: "wechat".to_string(),
            subcategory: None,
            query: Some("AI agent".to_string()),
            id: None,
            url: None,
            result: None,
            params: None,
            max_results: Some(2),
            prompt: None,
            web: Some(RetrievalWebOptions::default()),
        }
    }

    fn web_fetch_request() -> RetrievalRequest {
        RetrievalRequest {
            request_id: "req-web-fetch".to_string(),
            tool: RetrievalTool::Fetch,
            operation: RetrievalOperation::Fetch,
            category: "web".to_string(),
            source: "auto".to_string(),
            subcategory: None,
            query: None,
            id: None,
            url: Some("https://example.org/original".to_string()),
            result: None,
            params: None,
            max_results: None,
            prompt: Some("extract key points".to_string()),
            web: Some(RetrievalWebOptions::default()),
        }
    }

    fn social_fetch_request() -> RetrievalRequest {
        RetrievalRequest {
            request_id: "req-social-fetch".to_string(),
            tool: RetrievalTool::Fetch,
            operation: RetrievalOperation::Fetch,
            category: "social".to_string(),
            source: "wechat".to_string(),
            subcategory: None,
            query: None,
            id: None,
            url: Some("https://mp.weixin.qq.com/s/example".to_string()),
            result: None,
            params: None,
            max_results: None,
            prompt: Some("summarize article".to_string()),
            web: Some(RetrievalWebOptions::default()),
        }
    }

    fn literature_fetch_request() -> RetrievalRequest {
        RetrievalRequest {
            request_id: "req-lit-fetch".to_string(),
            tool: RetrievalTool::Fetch,
            operation: RetrievalOperation::Fetch,
            category: "literature".to_string(),
            source: "pubmed".to_string(),
            subcategory: None,
            query: None,
            id: Some("123".to_string()),
            url: None,
            result: None,
            params: None,
            max_results: None,
            prompt: Some("summarize findings".to_string()),
            web: Some(RetrievalWebOptions::default()),
        }
    }

    fn dataset_query_fetch_request() -> RetrievalRequest {
        RetrievalRequest {
            request_id: "req-query-fetch".to_string(),
            tool: RetrievalTool::Query,
            operation: RetrievalOperation::Fetch,
            category: "dataset".to_string(),
            source: "geo".to_string(),
            subcategory: None,
            query: None,
            id: Some("GSE1".to_string()),
            url: None,
            result: None,
            params: None,
            max_results: None,
            prompt: None,
            web: Some(RetrievalWebOptions::default()),
        }
    }

    fn knowledge_query_search_request(source: &str) -> RetrievalRequest {
        RetrievalRequest {
            request_id: "req-knowledge-search".to_string(),
            tool: RetrievalTool::Query,
            operation: RetrievalOperation::Search,
            category: "knowledge".to_string(),
            source: source.to_string(),
            subcategory: None,
            query: Some("TP53".to_string()),
            id: None,
            url: None,
            result: None,
            params: None,
            max_results: Some(2),
            prompt: None,
            web: Some(RetrievalWebOptions::default()),
        }
    }

    fn knowledge_query_fetch_request(source: &str, id: &str) -> RetrievalRequest {
        RetrievalRequest {
            request_id: "req-knowledge-fetch".to_string(),
            tool: RetrievalTool::Query,
            operation: RetrievalOperation::Fetch,
            category: "knowledge".to_string(),
            source: source.to_string(),
            subcategory: None,
            query: None,
            id: Some(id.to_string()),
            url: None,
            result: None,
            params: None,
            max_results: None,
            prompt: None,
            web: Some(RetrievalWebOptions::default()),
        }
    }

    #[test]
    fn maps_search_request_back_to_public_args() {
        let args = search_args_from_request(&base_request(RetrievalTool::Search)).unwrap();

        assert_eq!(args.category, "data");
        assert_eq!(args.source.as_deref(), Some("geo"));
        assert_eq!(args.subcategory.as_deref(), Some("sample_metadata"));
        assert_eq!(args.query, "brca1");
        assert_eq!(args.allowed_domains, Some(vec!["example.org".to_string()]));
        assert_eq!(args.max_results, Some(2));
    }

    #[test]
    fn maps_fetch_request_back_to_public_args() {
        let args = fetch_args_from_request(&base_request(RetrievalTool::Fetch));

        assert_eq!(args.category, "data");
        assert_eq!(args.source.as_deref(), Some("geo"));
        assert_eq!(args.id.as_deref(), Some("GSE1"));
        assert_eq!(args.prompt.as_deref(), Some("summarize"));
    }

    #[test]
    fn maps_query_request_back_to_public_args() {
        let args = query_args_from_request(&base_request(RetrievalTool::Query));

        assert_eq!(args.category, "data");
        assert_eq!(args.source.as_deref(), Some("geo"));
        assert_eq!(args.operation.as_deref(), Some("search"));
        assert_eq!(args.params, Some(json!({"limit": 2})));
    }

    #[test]
    fn converts_data_search_response_to_normalized_retrieval_items() {
        let request = base_request(RetrievalTool::Search);
        let response = data_search_response_to_retrieval(
            &request,
            DataSearchResponse {
                query: "brca1".to_string(),
                source: "auto".to_string(),
                total: Some(42),
                results: vec![data_record(PublicDataSource::Geo)],
                notes: vec!["combined search".to_string()],
            },
        );

        assert_eq!(response.provider, RetrievalProviderKind::Builtin);
        assert_eq!(response.source, "auto");
        assert_eq!(response.items.len(), 1);
        assert_eq!(response.items[0].accession.as_deref(), Some("GSE1"));
        assert_eq!(
            response.items[0].metadata["source_label"],
            json!("NCBI GEO DataSets")
        );

        let rendered = output::search_json(&request, &response);
        assert_eq!(rendered["provider"], json!("builtin"));
        assert_eq!(rendered["source"], json!("auto"));
        assert_eq!(rendered["results"][0]["source"], json!("geo"));
        assert_eq!(
            rendered["results"][0]["favicon"],
            json!("https://www.ncbi.nlm.nih.gov/favicon.ico")
        );
    }

    #[test]
    fn preserves_structured_data_search_error_json() {
        let request = base_request(RetrievalTool::Search);
        let response = structured_search_error_to_retrieval(
            &request,
            json!({
                "error": "source_disabled",
                "category": "data",
                "source": "geo",
                "message": "data.geo is disabled.",
                "results": []
            }),
        );

        let rendered = output::search_json(&request, &response);
        assert_eq!(rendered["error"], json!("source_disabled"));
        assert_eq!(rendered["provider"], json!("builtin"));
        assert_eq!(rendered["results"], json!([]));
    }

    #[test]
    fn converts_literature_search_json_to_normalized_response_preserving_extras() {
        let request = literature_request();
        let response = search_json_to_retrieval_response(
            &request,
            json!({
                "query": "brca1",
                "category": "literature",
                "source": "pubmed",
                "effective_source": "pubmed",
                "count": 42,
                "ret_start": 0,
                "ret_max": 2,
                "query_translation": "BRCA1[All Fields]",
                "ids": ["123"],
                "results": [{
                    "position": 1,
                    "category": "literature",
                    "source": "pubmed",
                    "title": "BRCA1 paper",
                    "name": "BRCA1 paper",
                    "link": "https://pubmed.ncbi.nlm.nih.gov/123/",
                    "url": "https://pubmed.ncbi.nlm.nih.gov/123/",
                    "displayed_link": "pubmed.ncbi.nlm.nih.gov/123",
                    "favicon": "https://pubmed.ncbi.nlm.nih.gov/favicon.ico",
                    "snippet": "Example Journal",
                    "id": "123",
                    "metadata": {
                        "pmid": "123",
                        "journal": "Example Journal"
                    }
                }]
            }),
        );

        assert_eq!(response.provider, RetrievalProviderKind::Builtin);
        assert_eq!(response.category, "literature");
        assert_eq!(response.source, "pubmed");
        assert_eq!(response.total, Some(42));
        assert_eq!(response.items[0].id.as_deref(), Some("123"));

        let rendered = output::search_json(&request, &response);
        assert_eq!(rendered["provider"], json!("builtin"));
        assert_eq!(rendered["count"], json!(42));
        assert_eq!(rendered["ret_start"], json!(0));
        assert_eq!(rendered["query_translation"], json!("BRCA1[All Fields]"));
        assert_eq!(rendered["ids"], json!(["123"]));
        assert_eq!(rendered["results"][0]["source"], json!("pubmed"));
        assert_eq!(rendered["results"][0]["provider"], json!("builtin"));
    }

    #[test]
    fn converts_web_search_json_to_normalized_response_preserving_route_metadata() {
        let request = web_request();
        let response = search_json_to_retrieval_response(
            &request,
            json!({
                "query": "rust async",
                "category": "web",
                "source": "auto",
                "effective_source": "ddg",
                "source_label": "DuckDuckGo public search",
                "duration_seconds": 0.42,
                "route_notes": ["Search order: DuckDuckGo → Google → Bing"],
                "results": [{
                    "position": 1,
                    "category": "web",
                    "source": "ddg",
                    "title": "Rust async book",
                    "name": "Rust async book",
                    "link": "https://rust-lang.github.io/async-book/",
                    "url": "https://rust-lang.github.io/async-book/",
                    "displayed_link": "rust-lang.github.io/async-book",
                    "favicon": "https://www.google.com/s2/favicons?domain=rust-lang.github.io&sz=64",
                    "snippet": "Async programming in Rust.",
                    "id": null,
                    "metadata": {}
                }]
            }),
        );

        assert_eq!(response.provider, RetrievalProviderKind::Builtin);
        assert_eq!(response.category, "web");
        assert_eq!(response.source, "auto");
        assert_eq!(response.effective_source, "ddg");
        assert_eq!(
            response.notes,
            vec!["Search order: DuckDuckGo → Google → Bing"]
        );

        let rendered = output::search_json(&request, &response);
        assert_eq!(rendered["provider"], json!("builtin"));
        assert_eq!(rendered["source"], json!("auto"));
        assert_eq!(rendered["effective_source"], json!("ddg"));
        assert_eq!(rendered["source_label"], json!("DuckDuckGo public search"));
        assert_eq!(rendered["duration_seconds"], json!(0.42));
        assert_eq!(
            rendered["route_notes"],
            json!(["Search order: DuckDuckGo → Google → Bing"])
        );
        assert_eq!(rendered["results"][0]["source"], json!("ddg"));
        assert_eq!(
            rendered["results"][0]["displayed_link"],
            json!("rust-lang.github.io/async-book")
        );
    }

    #[test]
    fn converts_social_search_json_to_normalized_response() {
        let request = social_request();
        let response = search_json_to_retrieval_response(
            &request,
            json!({
                "query": "AI agent",
                "category": "social",
                "source": "wechat",
                "effective_source": "wechat",
                "page": 1,
                "count": 1,
                "results": [{
                    "position": 1,
                    "category": "social",
                    "source": "wechat",
                    "title": "Agent article",
                    "name": "Agent article",
                    "link": "https://mp.weixin.qq.com/s/example",
                    "url": "https://mp.weixin.qq.com/s/example",
                    "displayed_link": "mp.weixin.qq.com/s/example",
                    "favicon": "https://res.wx.qq.com/a/wx_fed/assets/res/NTI4MWU5.ico",
                    "snippet": "WeChat article snippet",
                    "id": null,
                    "metadata": {
                        "platform": "wechat",
                        "account_name": "公众号",
                        "published_at": "2026-05-02",
                        "page": 1
                    }
                }]
            }),
        );

        assert_eq!(response.provider, RetrievalProviderKind::Builtin);
        assert_eq!(response.category, "social");
        assert_eq!(response.source, "wechat");
        assert_eq!(response.total, Some(1));

        let rendered = output::search_json(&request, &response);
        assert_eq!(rendered["provider"], json!("builtin"));
        assert_eq!(rendered["page"], json!(1));
        assert_eq!(rendered["count"], json!(1));
        assert_eq!(rendered["results"][0]["source"], json!("wechat"));
        assert_eq!(
            rendered["results"][0]["metadata"]["account_name"],
            json!("公众号")
        );
    }

    #[test]
    fn preserves_social_search_structured_error_json() {
        let request = social_request();
        let response = search_json_to_retrieval_response(
            &request,
            json!({
                "error": "source_disabled",
                "category": "social",
                "source": "wechat",
                "message": "social.wechat is disabled.",
                "results": []
            }),
        );

        let rendered = output::search_json(&request, &response);
        assert_eq!(rendered["error"], json!("source_disabled"));
        assert_eq!(rendered["provider"], json!("builtin"));
        assert_eq!(rendered["source"], json!("wechat"));
        assert_eq!(rendered["results"], json!([]));
    }

    #[test]
    fn converts_web_fetch_json_to_normalized_response_preserving_fetch_metadata() {
        let request = web_fetch_request();
        let response = detail_json_to_retrieval_response(
            &request,
            json!({
                "category": "web",
                "source": "auto",
                "effective_source": "http",
                "title": "Example document",
                "name": "Example document",
                "link": "https://example.org/final",
                "url": "https://example.org/final",
                "requested_url": "https://example.org/original",
                "displayed_link": "example.org/final",
                "favicon": "https://www.google.com/s2/favicons?domain=example.org&sz=64",
                "status": 200,
                "status_text": "OK",
                "content_type": "text/html; charset=utf-8",
                "content_length": 1234,
                "duration_ms": 42,
                "prompt": "extract key points",
                "content": "Example body text",
                "truncated_note": null,
                "metadata": {
                    "bytes_decoded_text": 17
                }
            }),
        );

        assert_eq!(response.provider, RetrievalProviderKind::Builtin);
        assert_eq!(response.category, "web");
        assert_eq!(response.source, "auto");
        assert_eq!(response.effective_source, "http");
        assert_eq!(
            response.detail.as_ref().unwrap().url.as_deref(),
            Some("https://example.org/final")
        );
        assert_eq!(
            response.detail.as_ref().unwrap().content.as_deref(),
            Some("Example body text")
        );

        let rendered = output::fetch_json(&request, &response);
        assert_eq!(rendered["provider"], json!("builtin"));
        assert_eq!(rendered["source"], json!("auto"));
        assert_eq!(rendered["effective_source"], json!("http"));
        assert_eq!(
            rendered["requested_url"],
            json!("https://example.org/original")
        );
        assert_eq!(rendered["status"], json!(200));
        assert_eq!(rendered["status_text"], json!("OK"));
        assert_eq!(rendered["content_type"], json!("text/html; charset=utf-8"));
        assert_eq!(rendered["content_length"], json!(1234));
        assert_eq!(rendered["duration_ms"], json!(42));
        assert_eq!(rendered["prompt"], json!("extract key points"));
        assert_eq!(rendered["metadata"]["bytes_decoded_text"], json!(17));
    }

    #[test]
    fn converts_social_fetch_web_json_to_normalized_response_with_compat_category() {
        let request = social_fetch_request();
        let response = detail_json_to_retrieval_response(
            &request,
            json!({
                "category": "web",
                "source": "wechat",
                "effective_source": "http",
                "title": "WeChat article",
                "name": "WeChat article",
                "link": "https://mp.weixin.qq.com/s/example",
                "url": "https://mp.weixin.qq.com/s/example",
                "requested_url": "https://mp.weixin.qq.com/s/example",
                "displayed_link": "mp.weixin.qq.com/s/example",
                "favicon": "https://www.google.com/s2/favicons?domain=mp.weixin.qq.com&sz=64",
                "status": 200,
                "status_text": "OK",
                "content_type": "text/html",
                "content_length": null,
                "duration_ms": 64,
                "prompt": "summarize article",
                "content": "WeChat article body",
                "truncated_note": null,
                "metadata": {
                    "bytes_decoded_text": 19
                }
            }),
        );

        assert_eq!(response.provider, RetrievalProviderKind::Builtin);
        assert_eq!(response.category, "social");
        assert_eq!(response.source, "wechat");
        assert_eq!(response.effective_source, "http");

        let rendered = output::fetch_json(&request, &response);
        assert_eq!(
            rendered["category"],
            json!("web"),
            "public output keeps the historical web fetch payload shape for social fetch"
        );
        assert_eq!(rendered["provider"], json!("builtin"));
        assert_eq!(rendered["source"], json!("wechat"));
        assert_eq!(rendered["status"], json!(200));
        assert_eq!(rendered["content"], json!("WeChat article body"));
    }

    #[test]
    fn preserves_social_fetch_structured_error_json() {
        let request = social_fetch_request();
        let response = detail_json_to_retrieval_response(
            &request,
            json!({
                "error": "source_disabled",
                "category": "social",
                "source": "wechat",
                "message": "social.wechat is disabled."
            }),
        );

        let rendered = output::fetch_json(&request, &response);
        assert_eq!(rendered["error"], json!("source_disabled"));
        assert_eq!(rendered["provider"], json!("builtin"));
        assert_eq!(rendered["source"], json!("wechat"));
        assert_eq!(rendered["message"], json!("social.wechat is disabled."));
    }

    #[test]
    fn converts_literature_detail_json_to_normalized_fetch_response() {
        let request = literature_fetch_request();
        let response = detail_json_to_retrieval_response(
            &request,
            json!({
                "category": "literature",
                "source": "pubmed",
                "effective_source": "pubmed",
                "id": "123",
                "title": "BRCA1 paper",
                "article_title": "BRCA1 paper",
                "name": "BRCA1 paper",
                "link": "https://pubmed.ncbi.nlm.nih.gov/123/",
                "url": "https://pubmed.ncbi.nlm.nih.gov/123/",
                "displayed_link": "pubmed.ncbi.nlm.nih.gov/123",
                "favicon": "https://pubmed.ncbi.nlm.nih.gov/favicon.ico",
                "abstract": "Abstract text",
                "authors": ["Alice", "Bob"],
                "content": "BRCA1 paper\n\nAbstract text",
                "metadata": {
                    "pmid": "123",
                    "doi": "10.1000/example",
                    "authors": ["Alice", "Bob"]
                }
            }),
        );

        assert_eq!(response.provider, RetrievalProviderKind::Builtin);
        assert_eq!(response.category, "literature");
        assert_eq!(response.detail.as_ref().unwrap().id.as_deref(), Some("123"));
        assert_eq!(
            response.detail.as_ref().unwrap().snippet.as_deref(),
            Some("Abstract text")
        );
        assert_eq!(
            response.detail.as_ref().unwrap().metadata["doi"],
            json!("10.1000/example")
        );

        let rendered = output::fetch_json(&request, &response);
        assert_eq!(rendered["provider"], json!("builtin"));
        assert_eq!(rendered["source"], json!("pubmed"));
        assert_eq!(rendered["article_title"], json!("BRCA1 paper"));
        assert_eq!(rendered["authors"], json!(["Alice", "Bob"]));
        assert_eq!(rendered["abstract"], json!("Abstract text"));
        assert_eq!(rendered["prompt"], json!("summarize findings"));
    }

    #[test]
    fn preserves_literature_fetch_structured_error_json() {
        let mut request = literature_fetch_request();
        request.source = "semantic_scholar".to_string();
        let response = detail_json_to_retrieval_response(
            &request,
            json!({
                "error": "source_disabled",
                "category": "literature",
                "source": "semantic_scholar",
                "message": "literature.semantic_scholar is disabled."
            }),
        );

        let rendered = output::fetch_json(&request, &response);
        assert_eq!(rendered["error"], json!("source_disabled"));
        assert_eq!(rendered["provider"], json!("builtin"));
        assert_eq!(rendered["source"], json!("semantic_scholar"));
        assert_eq!(
            rendered["message"],
            json!("literature.semantic_scholar is disabled.")
        );
    }

    #[test]
    fn converts_dataset_detail_json_to_normalized_fetch_response() {
        let request = base_request(RetrievalTool::Fetch);
        let response = detail_json_to_retrieval_response(
            &request,
            json!({
                "category": "data",
                "source": "geo",
                "effective_source": "geo",
                "id": "1",
                "accession": "GSE1",
                "title": "BRCA1 expression",
                "name": "BRCA1 expression",
                "link": "https://www.ncbi.nlm.nih.gov/geo/query/acc.cgi?acc=GSE1",
                "url": "https://www.ncbi.nlm.nih.gov/geo/query/acc.cgi?acc=GSE1",
                "displayed_link": "ncbi.nlm.nih.gov/geo/query/acc.cgi",
                "favicon": "https://www.ncbi.nlm.nih.gov/favicon.ico",
                "snippet": "Series | Homo sapiens",
                "content": "BRCA1 expression\n\nSource: NCBI GEO DataSets",
                "metadata": {
                    "accession": "GSE1",
                    "source_label": "NCBI GEO DataSets"
                }
            }),
        );

        assert_eq!(response.provider, RetrievalProviderKind::Builtin);
        assert_eq!(
            response.detail.as_ref().unwrap().accession.as_deref(),
            Some("GSE1")
        );
        assert_eq!(
            response.detail.as_ref().unwrap().content.as_deref(),
            Some("BRCA1 expression\n\nSource: NCBI GEO DataSets")
        );

        let rendered = output::fetch_json(&request, &response);
        assert_eq!(rendered["provider"], json!("builtin"));
        assert_eq!(rendered["source"], json!("geo"));
        assert_eq!(rendered["accession"], json!("GSE1"));
        assert_eq!(
            rendered["metadata"]["source_label"],
            json!("NCBI GEO DataSets")
        );
    }

    #[test]
    fn converts_dataset_query_fetch_json_to_normalized_query_response() {
        let request = dataset_query_fetch_request();
        let response = builtin_query_json_to_retrieval_response(
            &request,
            json!({
                "tool": "query",
                "operation": "fetch",
                "category": "data",
                "source": "geo",
                "effective_source": "geo",
                "id": "1",
                "accession": "GSE1",
                "title": "BRCA1 expression",
                "name": "BRCA1 expression",
                "link": "https://www.ncbi.nlm.nih.gov/geo/query/acc.cgi?acc=GSE1",
                "url": "https://www.ncbi.nlm.nih.gov/geo/query/acc.cgi?acc=GSE1",
                "displayed_link": "ncbi.nlm.nih.gov/geo/query/acc.cgi",
                "favicon": "https://www.ncbi.nlm.nih.gov/favicon.ico",
                "content": "BRCA1 expression detail",
                "metadata": {"accession": "GSE1"}
            }),
        );

        let rendered = output::query_json(&request, &response);
        assert_eq!(rendered["provider"], json!("builtin"));
        assert_eq!(rendered["tool"], json!("query"));
        assert_eq!(rendered["operation"], json!("fetch"));
        assert_eq!(rendered["accession"], json!("GSE1"));
        assert_eq!(rendered["content"], json!("BRCA1 expression detail"));
    }

    #[test]
    fn preserves_dataset_download_summary_raw_query_json() {
        let mut request = dataset_query_fetch_request();
        request.operation = RetrievalOperation::DownloadSummary;
        let response = builtin_query_json_to_retrieval_response(
            &request,
            json!({
                "tool": "query",
                "operation": "download_summary",
                "category": "data",
                "source": "ncbi_datasets",
                "effective_source": "ncbi_datasets",
                "accession": "GCF_000001405.40",
                "estimated_size": 12345,
                "files": ["genomic.fna"]
            }),
        );

        let rendered = output::query_json(&request, &response);
        assert_eq!(rendered["provider"], json!("builtin"));
        assert_eq!(rendered["operation"], json!("download_summary"));
        assert_eq!(rendered["estimated_size"], json!(12345));
        assert_eq!(rendered["files"], json!(["genomic.fna"]));
    }

    #[test]
    fn converts_ncbi_gene_query_search_json_to_normalized_response() {
        let request = knowledge_query_search_request("ncbi_gene");
        let response = builtin_query_json_to_retrieval_response(
            &request,
            json!({
                "tool": "query",
                "operation": "search",
                "query": "TP53",
                "effective_query": "TP53[All Fields] AND Homo sapiens[Organism]",
                "category": "knowledge",
                "source": "ncbi_gene",
                "effective_source": "ncbi_gene",
                "count": 1,
                "ret_start": 0,
                "ret_max": 2,
                "query_translation": "TP53[All Fields]",
                "ids": ["7157"],
                "results": [{
                    "position": 1,
                    "category": "knowledge",
                    "source": "ncbi_gene",
                    "title": "TP53 tumor protein p53",
                    "name": "TP53",
                    "link": "https://www.ncbi.nlm.nih.gov/gene/7157",
                    "url": "https://www.ncbi.nlm.nih.gov/gene/7157",
                    "displayed_link": "ncbi.nlm.nih.gov/gene/7157",
                    "favicon": "https://www.ncbi.nlm.nih.gov/favicon.ico",
                    "snippet": "Homo sapiens",
                    "id": "7157",
                    "gene_id": "7157",
                    "metadata": {
                        "gene_id": "7157",
                        "symbol": "TP53",
                        "organism": "Homo sapiens"
                    }
                }]
            }),
        );

        assert_eq!(response.provider, RetrievalProviderKind::Builtin);
        assert_eq!(response.operation, RetrievalOperation::Search);
        assert_eq!(response.category, "knowledge");
        assert_eq!(response.source, "ncbi_gene");
        assert_eq!(response.total, Some(1));
        assert_eq!(response.items[0].id.as_deref(), Some("7157"));

        let rendered = output::query_json(&request, &response);
        assert_eq!(rendered["provider"], json!("builtin"));
        assert_eq!(rendered["tool"], json!("query"));
        assert_eq!(rendered["operation"], json!("search"));
        assert_eq!(
            rendered["effective_query"],
            json!("TP53[All Fields] AND Homo sapiens[Organism]")
        );
        assert_eq!(rendered["query_translation"], json!("TP53[All Fields]"));
        assert_eq!(rendered["ids"], json!(["7157"]));
        assert_eq!(rendered["results"][0]["source"], json!("ncbi_gene"));
        assert_eq!(rendered["results"][0]["metadata"]["gene_id"], json!("7157"));
    }

    #[test]
    fn converts_ensembl_query_fetch_json_to_normalized_response() {
        let request = knowledge_query_fetch_request("ensembl", "ENSG00000141510");
        let response = builtin_query_json_to_retrieval_response(
            &request,
            json!({
                "tool": "query",
                "operation": "fetch",
                "category": "knowledge",
                "source": "ensembl",
                "effective_source": "ensembl",
                "id": "ENSG00000141510",
                "accession": "ENSG00000141510",
                "title": "TP53 (Gene)",
                "name": "TP53",
                "link": "https://www.ensembl.org/Homo_sapiens/Gene/Summary?g=ENSG00000141510",
                "url": "https://www.ensembl.org/Homo_sapiens/Gene/Summary?g=ENSG00000141510",
                "displayed_link": "ensembl.org/Homo_sapiens/Gene/Summary",
                "favicon": "https://www.ensembl.org/favicon.ico",
                "snippet": "Homo sapiens gene on chromosome 17",
                "content": "TP53\n\nSource: Ensembl",
                "metadata": {
                    "id": "ENSG00000141510",
                    "record_type": "Gene",
                    "display_name": "TP53",
                    "species": "homo_sapiens"
                }
            }),
        );

        assert_eq!(response.provider, RetrievalProviderKind::Builtin);
        assert_eq!(response.operation, RetrievalOperation::Fetch);
        assert_eq!(
            response.detail.as_ref().unwrap().accession.as_deref(),
            Some("ENSG00000141510")
        );

        let rendered = output::query_json(&request, &response);
        assert_eq!(rendered["provider"], json!("builtin"));
        assert_eq!(rendered["tool"], json!("query"));
        assert_eq!(rendered["operation"], json!("fetch"));
        assert_eq!(rendered["source"], json!("ensembl"));
        assert_eq!(rendered["accession"], json!("ENSG00000141510"));
        assert_eq!(rendered["metadata"]["record_type"], json!("Gene"));
    }

    #[test]
    fn converts_uniprot_query_fetch_json_to_normalized_response() {
        let request = knowledge_query_fetch_request("uniprot", "P04637");
        let response = builtin_query_json_to_retrieval_response(
            &request,
            json!({
                "tool": "query",
                "operation": "fetch",
                "category": "knowledge",
                "source": "uniprot",
                "effective_source": "uniprot",
                "id": "P04637",
                "accession": "P04637",
                "title": "Cellular tumor antigen p53",
                "name": "P53_HUMAN",
                "link": "https://www.uniprot.org/uniprotkb/P04637",
                "url": "https://www.uniprot.org/uniprotkb/P04637",
                "displayed_link": "uniprot.org/uniprotkb/P04637",
                "favicon": "https://www.uniprot.org/favicon.ico",
                "snippet": "Reviewed human protein",
                "content": "Cellular tumor antigen p53\n\nFunction: acts as tumor suppressor",
                "metadata": {
                    "accession": "P04637",
                    "entry_name": "P53_HUMAN",
                    "reviewed": true,
                    "gene_names": ["TP53"],
                    "organism": "Homo sapiens"
                }
            }),
        );

        assert_eq!(response.provider, RetrievalProviderKind::Builtin);
        assert_eq!(
            response.detail.as_ref().unwrap().accession.as_deref(),
            Some("P04637")
        );
        assert_eq!(
            response.detail.as_ref().unwrap().metadata["reviewed"],
            json!(true)
        );

        let rendered = output::query_json(&request, &response);
        assert_eq!(rendered["provider"], json!("builtin"));
        assert_eq!(rendered["tool"], json!("query"));
        assert_eq!(rendered["operation"], json!("fetch"));
        assert_eq!(rendered["source"], json!("uniprot"));
        assert_eq!(rendered["accession"], json!("P04637"));
        assert_eq!(rendered["metadata"]["gene_names"], json!(["TP53"]));
    }
}
