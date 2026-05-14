use crate::domain::retrieval::types::{RetrievalItem, RetrievalOperation, RetrievalRequest};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeRequest {
    pub id: String,
    #[serde(rename = "type")]
    pub message_type: String,
    pub protocol_version: u32,
    pub plugin_id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShutdownRequest {
    pub id: String,
    #[serde(rename = "type")]
    pub message_type: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecuteRequestEnvelope {
    pub id: String,
    #[serde(rename = "type")]
    pub message_type: String,
    pub request: PluginExecuteRequest,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginExecuteRequest {
    pub operation: String,
    pub category: String,
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<JsonValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<JsonValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_results: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    pub credentials: HashMap<String, String>,
}

impl PluginExecuteRequest {
    pub fn from_retrieval_request(
        request: &RetrievalRequest,
        credentials: HashMap<String, String>,
    ) -> Self {
        Self {
            operation: request.operation.as_str().to_string(),
            category: request.category.clone(),
            source: request.source.clone(),
            query: request.query.clone(),
            id: request.id.clone(),
            url: request.url.clone(),
            result: request.result.clone(),
            params: request.params.clone(),
            max_results: request.max_results,
            prompt: request.prompt.clone(),
            credentials,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IpcResponseEnvelope {
    pub id: String,
    #[serde(rename = "type")]
    pub message_type: String,
    #[serde(default)]
    pub protocol_version: Option<u32>,
    #[serde(default, alias = "sources")]
    pub resources: Vec<IpcInitializedResource>,
    #[serde(default)]
    pub response: Option<PluginExecutionResponse>,
    #[serde(default)]
    pub error: Option<PluginIpcError>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct IpcInitializedResource {
    pub category: String,
    pub id: String,
    #[serde(default)]
    pub capabilities: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginExecutionResponse {
    #[serde(default)]
    pub ok: Option<bool>,
    pub operation: String,
    pub category: String,
    pub source: String,
    #[serde(default, alias = "effective_source")]
    pub effective_source: Option<String>,
    #[serde(default)]
    pub items: Vec<PluginResponseItem>,
    #[serde(default)]
    pub detail: Option<PluginResponseItem>,
    #[serde(default)]
    pub total: Option<u64>,
    #[serde(default)]
    pub notes: Vec<String>,
    #[serde(default)]
    pub raw: Option<JsonValue>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginResponseItem {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub accession: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub snippet: Option<String>,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub favicon: Option<String>,
    #[serde(default)]
    pub metadata: Option<JsonValue>,
    #[serde(default)]
    pub raw: Option<JsonValue>,
}

impl From<PluginResponseItem> for RetrievalItem {
    fn from(value: PluginResponseItem) -> Self {
        Self {
            id: value.id,
            accession: value.accession,
            title: value.title,
            url: value.url,
            snippet: value.snippet,
            content: value.content,
            favicon: value.favicon,
            metadata: value.metadata.unwrap_or_else(|| serde_json::json!({})),
            raw: value.raw,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginIpcError {
    pub code: String,
    pub message: String,
}

pub fn operation_from_plugin(value: &str) -> Option<RetrievalOperation> {
    match value
        .trim()
        .to_ascii_lowercase()
        .replace(['-', ' '], "_")
        .as_str()
    {
        "search" => Some(RetrievalOperation::Search),
        "fetch" | "get" | "detail" => Some(RetrievalOperation::Fetch),
        "query" => Some(RetrievalOperation::Query),
        "download_summary" => Some(RetrievalOperation::DownloadSummary),
        "resolve" => Some(RetrievalOperation::Resolve),
        _ => None,
    }
}
