use super::QueryArgs;
use crate::domain::retrieval_registry::{self, RetrievalCapability, RetrievalSourceStatus};
use crate::domain::tools::ToolError;
use crate::infrastructure::streaming::{StreamOutput, StreamOutputItem};
use serde_json::{json, Value as JsonValue};
use std::pin::Pin;

pub(super) fn requested_source(args: &QueryArgs) -> String {
    let param_source = param_string(args, &["source"]);
    let result_source = string_from_result(args, &["source", "effective_source"]);
    normalized_source(
        args.source
            .as_deref()
            .or(param_source.as_deref())
            .or(result_source.as_deref()),
    )
}

pub(super) fn annotate_query_json(value: &mut JsonValue, operation: &str, default_category: &str) {
    if let Some(obj) = value.as_object_mut() {
        obj.insert("tool".to_string(), json!("query"));
        obj.insert("operation".to_string(), json!(operation));
        obj.entry("category".to_string())
            .or_insert_with(|| json!(default_category));
    }
}

pub(super) fn ensure_registry_source_can_query(
    source: &retrieval_registry::RetrievalSourceDefinition,
) -> Result<(), ToolError> {
    match source.status {
        RetrievalSourceStatus::Planned => {
            return Err(ToolError::InvalidArguments {
                message: format!(
                    "{} source `{}` is planned but not implemented yet.",
                    source.category, source.id
                ),
            });
        }
        RetrievalSourceStatus::Extension => {
            return Err(ToolError::InvalidArguments {
                message: format!(
                    "{} source `{}` is provided by an extension and is not built in.",
                    source.category, source.id
                ),
            });
        }
        RetrievalSourceStatus::Available
        | RetrievalSourceStatus::RequiresApiKey
        | RetrievalSourceStatus::OptIn => {}
    }
    if !source.supports(RetrievalCapability::Query) {
        return Err(ToolError::InvalidArguments {
            message: format!(
                "{} source `{}` does not support query.",
                source.category, source.id
            ),
        });
    }
    Ok(())
}

pub(super) fn normalized_category(value: &str) -> String {
    match value.trim().to_ascii_lowercase().replace('-', "_").as_str() {
        "dataset" | "datasets" => "data".to_string(),
        other => other.to_string(),
    }
}

pub(super) fn normalized_source(value: Option<&str>) -> String {
    value
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("auto")
        .to_ascii_lowercase()
        .replace('-', "_")
}

pub(super) fn normalized_subcategory(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_ascii_lowercase().replace(['-', ' '], "_"))
}

pub(super) fn normalized_operation(args: &QueryArgs) -> String {
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

pub(super) fn query_text(args: &QueryArgs) -> Option<String> {
    args.query
        .as_deref()
        .and_then(clean_nonempty)
        .or_else(|| param_string(args, &["query", "q", "term"]))
}

pub(super) fn identifier_text(args: &QueryArgs) -> Option<String> {
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

pub(super) fn param_string(args: &QueryArgs, keys: &[&str]) -> Option<String> {
    let map = args.params.as_ref()?.as_object()?;
    keys.iter()
        .find_map(|key| map.get(*key).and_then(json_string_value))
        .and_then(clean_string)
}

pub(super) fn param_u32(args: &QueryArgs, keys: &[&str]) -> Option<u32> {
    let map = args.params.as_ref()?.as_object()?;
    keys.iter().find_map(|key| {
        let value = map.get(*key)?;
        value
            .as_u64()
            .and_then(|v| u32::try_from(v).ok())
            .or_else(|| value.as_str()?.trim().parse::<u32>().ok())
    })
}

pub(super) fn param_bool(args: &QueryArgs, keys: &[&str]) -> Option<bool> {
    let map = args.params.as_ref()?.as_object()?;
    keys.iter().find_map(|key| {
        let value = map.get(*key)?;
        value.as_bool().or_else(
            || match value.as_str()?.trim().to_ascii_lowercase().as_str() {
                "true" | "yes" | "1" | "reviewed" => Some(true),
                "false" | "no" | "0" | "unreviewed" => Some(false),
                _ => None,
            },
        )
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

pub(super) fn json_stream(value: JsonValue) -> crate::infrastructure::streaming::StreamOutputBox {
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
