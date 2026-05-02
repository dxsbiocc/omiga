use super::types::{
    RetrievalError, RetrievalOperation, RetrievalRequest, RetrievalTool, RetrievalWebOptions,
};
use crate::domain::tools::{fetch::FetchArgs, query::QueryArgs, search::SearchArgs};
use serde_json::Value as JsonValue;

pub fn search_request(args: &SearchArgs) -> Result<RetrievalRequest, RetrievalError> {
    Ok(RetrievalRequest {
        request_id: new_request_id(),
        tool: RetrievalTool::Search,
        operation: RetrievalOperation::Search,
        category: normalized_category(&args.category),
        source: normalized_source(args.source.as_deref()),
        subcategory: normalized_optional(args.subcategory.as_deref()),
        query: Some(args.query.trim().to_string()),
        id: None,
        url: None,
        result: None,
        params: None,
        max_results: args.max_results,
        prompt: None,
        web: Some(RetrievalWebOptions {
            allowed_domains: args.allowed_domains.clone(),
            blocked_domains: args.blocked_domains.clone(),
            search_url: args.search_url.clone(),
        }),
    })
}

pub fn fetch_request(args: &FetchArgs) -> Result<RetrievalRequest, RetrievalError> {
    Ok(RetrievalRequest {
        request_id: new_request_id(),
        tool: RetrievalTool::Fetch,
        operation: RetrievalOperation::Fetch,
        category: normalized_category(&args.category),
        source: normalized_source(args.source.as_deref()),
        subcategory: normalized_optional(args.subcategory.as_deref()),
        query: None,
        id: clean_optional(args.id.as_deref()),
        url: clean_optional(args.url.as_deref()),
        result: args.result.clone(),
        params: None,
        max_results: None,
        prompt: clean_optional(args.prompt.as_deref()),
        web: None,
    })
}

pub fn query_request(args: &QueryArgs) -> Result<RetrievalRequest, RetrievalError> {
    Ok(RetrievalRequest {
        request_id: new_request_id(),
        tool: RetrievalTool::Query,
        operation: query_operation(args)?,
        category: normalized_category(&args.category),
        source: requested_query_source(args),
        subcategory: normalized_optional(args.subcategory.as_deref()),
        query: clean_optional(args.query.as_deref())
            .or_else(|| param_string(args.params.as_ref(), &["query", "q", "term"])),
        id: clean_optional(args.id.as_deref()),
        url: clean_optional(args.url.as_deref()),
        result: args.result.clone(),
        params: args.params.clone(),
        max_results: args.max_results.or_else(|| {
            param_u32(
                args.params.as_ref(),
                &["max_results", "maxResults", "limit", "retmax"],
            )
        }),
        prompt: None,
        web: None,
    })
}

pub fn normalized_category(value: &str) -> String {
    match normalize_id(value).as_str() {
        "data" | "dataset" | "datasets" => "dataset".to_string(),
        "knowledge_base" | "kb" | "memory" => "knowledge".to_string(),
        other => other.to_string(),
    }
}

pub fn normalized_source(value: Option<&str>) -> String {
    value
        .and_then(clean_nonempty)
        .map(|s| normalize_id(&s))
        .unwrap_or_else(|| "auto".to_string())
}

pub fn normalized_optional(value: Option<&str>) -> Option<String> {
    value.and_then(clean_nonempty).map(|s| normalize_id(&s))
}

pub fn normalize_id(value: &str) -> String {
    value.trim().to_ascii_lowercase().replace(['-', ' '], "_")
}

fn query_operation(args: &QueryArgs) -> Result<RetrievalOperation, RetrievalError> {
    let explicit = args
        .operation
        .as_deref()
        .and_then(clean_nonempty)
        .or_else(|| param_string(args.params.as_ref(), &["operation", "op"]));
    if let Some(op) = explicit {
        return operation_from_str(&op);
    }
    if args.id.as_deref().and_then(clean_nonempty).is_some()
        || args.url.as_deref().and_then(clean_nonempty).is_some()
        || args.result.is_some()
        || param_string(args.params.as_ref(), &["id", "accession", "gene_id", "url"]).is_some()
    {
        Ok(RetrievalOperation::Fetch)
    } else {
        Ok(RetrievalOperation::Search)
    }
}

fn operation_from_str(value: &str) -> Result<RetrievalOperation, RetrievalError> {
    match normalize_id(value).as_str() {
        "search" => Ok(RetrievalOperation::Search),
        "query" => Ok(RetrievalOperation::Query),
        "fetch" | "get" | "detail" => Ok(RetrievalOperation::Fetch),
        "download_summary" | "download_summary_preview" | "download_preview" => {
            Ok(RetrievalOperation::DownloadSummary)
        }
        "resolve" => Ok(RetrievalOperation::Resolve),
        other => Err(RetrievalError::InvalidRequest {
            message: format!("Unsupported retrieval operation: {other}"),
        }),
    }
}

fn requested_query_source(args: &QueryArgs) -> String {
    if let Some(source) = args.source.as_deref() {
        return normalized_source(Some(source));
    }
    if let Some(source) = param_string(args.params.as_ref(), &["source"]) {
        return normalized_source(Some(&source));
    }
    if let Some(source) = string_from_result(args.result.as_ref(), &["source", "effective_source"])
    {
        return normalized_source(Some(&source));
    }
    normalized_source(None)
}

fn clean_optional(value: Option<&str>) -> Option<String> {
    value.and_then(clean_nonempty)
}

fn clean_nonempty(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn param_string(params: Option<&JsonValue>, keys: &[&str]) -> Option<String> {
    let map = params?.as_object()?;
    keys.iter()
        .find_map(|key| map.get(*key).and_then(json_string_value))
        .and_then(|value| clean_nonempty(&value))
}

fn param_u32(params: Option<&JsonValue>, keys: &[&str]) -> Option<u32> {
    let map = params?.as_object()?;
    keys.iter().find_map(|key| {
        let value = map.get(*key)?;
        value
            .as_u64()
            .and_then(|v| u32::try_from(v).ok())
            .or_else(|| value.as_str()?.trim().parse::<u32>().ok())
    })
}

fn string_from_result(result: Option<&JsonValue>, keys: &[&str]) -> Option<String> {
    let object = result?.as_object()?;
    keys.iter()
        .find_map(|key| object.get(*key).and_then(json_string_value))
        .and_then(|value| clean_nonempty(&value))
}

fn json_string_value(value: &JsonValue) -> Option<String> {
    value
        .as_str()
        .map(str::to_string)
        .or_else(|| value.as_u64().map(|v| v.to_string()))
        .or_else(|| value.as_i64().map(|v| v.to_string()))
}

fn new_request_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn search_args_map_to_retrieval_request() {
        let args = SearchArgs {
            category: "dataset".to_string(),
            source: Some("GEO".to_string()),
            subcategory: Some("sample metadata".to_string()),
            query: " lung cancer ".to_string(),
            allowed_domains: None,
            blocked_domains: None,
            max_results: Some(7),
            search_url: None,
        };

        let request = search_request(&args).unwrap();

        assert_eq!(request.tool, RetrievalTool::Search);
        assert_eq!(request.operation, RetrievalOperation::Search);
        assert_eq!(request.category, "dataset");
        assert_eq!(request.source, "geo");
        assert_eq!(request.subcategory.as_deref(), Some("sample_metadata"));
        assert_eq!(request.query.as_deref(), Some("lung cancer"));
        assert_eq!(request.max_results, Some(7));
    }

    #[test]
    fn fetch_args_map_to_retrieval_request() {
        let args = FetchArgs {
            category: "data".to_string(),
            source: Some("ena-run".to_string()),
            subcategory: None,
            url: None,
            id: Some(" ERR123 ".to_string()),
            result: None,
            prompt: Some(" summarize ".to_string()),
        };

        let request = fetch_request(&args).unwrap();

        assert_eq!(request.tool, RetrievalTool::Fetch);
        assert_eq!(request.operation, RetrievalOperation::Fetch);
        assert_eq!(request.category, "dataset");
        assert_eq!(request.source, "ena_run");
        assert_eq!(request.id.as_deref(), Some("ERR123"));
        assert_eq!(request.prompt.as_deref(), Some("summarize"));
    }

    #[test]
    fn query_args_infer_operation_and_source_from_params() {
        let args = QueryArgs {
            category: "dataset".to_string(),
            source: None,
            operation: None,
            subcategory: None,
            query: None,
            id: None,
            url: None,
            result: Some(json!({"source": "geo"})),
            params: Some(json!({"limit": "3"})),
            max_results: None,
        };

        let request = query_request(&args).unwrap();

        assert_eq!(request.tool, RetrievalTool::Query);
        assert_eq!(request.operation, RetrievalOperation::Fetch);
        assert_eq!(request.source, "geo");
        assert_eq!(request.max_results, Some(3));
    }

    #[test]
    fn query_args_normalize_download_summary_alias() {
        let args = QueryArgs {
            category: "dataset".to_string(),
            source: Some("ncbi-datasets".to_string()),
            operation: Some("download-preview".to_string()),
            subcategory: None,
            query: None,
            id: Some("GCF_000001405.40".to_string()),
            url: None,
            result: None,
            params: None,
            max_results: None,
        };

        let request = query_request(&args).unwrap();

        assert_eq!(request.operation, RetrievalOperation::DownloadSummary);
        assert_eq!(request.source, "ncbi_datasets");
    }
}
