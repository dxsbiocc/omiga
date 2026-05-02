use crate::errors::ToolError;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetrievalTool {
    Search,
    Fetch,
    Query,
}

impl RetrievalTool {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Search => "search",
            Self::Fetch => "fetch",
            Self::Query => "query",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetrievalOperation {
    Search,
    Fetch,
    Query,
    DownloadSummary,
    Resolve,
}

impl RetrievalOperation {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Search => "search",
            Self::Fetch => "fetch",
            Self::Query => "query",
            Self::DownloadSummary => "download_summary",
            Self::Resolve => "resolve",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetrievalProviderKind {
    Builtin,
    Plugin,
}

impl RetrievalProviderKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Builtin => "builtin",
            Self::Plugin => "plugin",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RetrievalWebOptions {
    #[serde(default)]
    pub allowed_domains: Option<Vec<String>>,
    #[serde(default)]
    pub blocked_domains: Option<Vec<String>>,
    #[serde(default)]
    pub search_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RetrievalRequest {
    pub request_id: String,
    pub tool: RetrievalTool,
    pub operation: RetrievalOperation,
    pub category: String,
    pub source: String,
    #[serde(default)]
    pub subcategory: Option<String>,
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub result: Option<JsonValue>,
    #[serde(default)]
    pub params: Option<JsonValue>,
    #[serde(default)]
    pub max_results: Option<u32>,
    #[serde(default)]
    pub prompt: Option<String>,
    #[serde(default)]
    pub web: Option<RetrievalWebOptions>,
}

impl RetrievalRequest {
    pub fn public_category(&self) -> &str {
        public_category(&self.category)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RetrievalItem {
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
    #[serde(default = "empty_object")]
    pub metadata: JsonValue,
    #[serde(default)]
    pub raw: Option<JsonValue>,
}

impl Default for RetrievalItem {
    fn default() -> Self {
        Self {
            id: None,
            accession: None,
            title: None,
            url: None,
            snippet: None,
            content: None,
            favicon: None,
            metadata: empty_object(),
            raw: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RetrievalResponse {
    pub operation: RetrievalOperation,
    pub category: String,
    pub source: String,
    pub effective_source: String,
    pub provider: RetrievalProviderKind,
    #[serde(default)]
    pub plugin: Option<String>,
    #[serde(default)]
    pub items: Vec<RetrievalItem>,
    #[serde(default)]
    pub detail: Option<RetrievalItem>,
    #[serde(default)]
    pub total: Option<u64>,
    #[serde(default)]
    pub notes: Vec<String>,
    #[serde(default)]
    pub raw: Option<JsonValue>,
}

pub enum RetrievalProviderOutput {
    Response(Box<RetrievalResponse>),
    Stream(crate::infrastructure::streaming::StreamOutputBox),
}

impl From<RetrievalResponse> for RetrievalProviderOutput {
    fn from(value: RetrievalResponse) -> Self {
        Self::Response(Box::new(value))
    }
}

impl RetrievalResponse {
    pub fn builtin(
        operation: RetrievalOperation,
        category: impl Into<String>,
        source: impl Into<String>,
    ) -> Self {
        let source = source.into();
        Self {
            operation,
            category: category.into(),
            effective_source: source.clone(),
            source,
            provider: RetrievalProviderKind::Builtin,
            plugin: None,
            items: Vec::new(),
            detail: None,
            total: None,
            notes: Vec::new(),
            raw: None,
        }
    }

    pub fn public_category(&self) -> &str {
        public_category(&self.category)
    }
}

#[derive(Debug, Clone, Error, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum RetrievalError {
    #[error("invalid retrieval request: {message}")]
    InvalidRequest { message: String },
    #[error("retrieval source disabled: {category}.{source_id}: {message}")]
    SourceDisabled {
        category: String,
        source_id: String,
        message: String,
    },
    #[error("missing retrieval credentials for {category}.{source_id}: {refs:?}")]
    MissingCredentials {
        category: String,
        source_id: String,
        refs: Vec<String>,
    },
    #[error("retrieval provider unavailable: {message}")]
    ProviderUnavailable { message: String },
    #[error("retrieval plugin protocol error: {message}")]
    Protocol {
        plugin: Option<String>,
        message: String,
    },
    #[error("retrieval execution failed: {message}")]
    ExecutionFailed { message: String },
    #[error("retrieval timed out after {seconds}s")]
    Timeout { seconds: u64 },
    #[error("retrieval cancelled")]
    Cancelled,
}

impl RetrievalError {
    pub fn structured_json(&self, category: &str, source: &str) -> JsonValue {
        match self {
            Self::SourceDisabled { message, .. } => json!({
                "error": "source_disabled",
                "category": public_category(category),
                "source": source,
                "message": message,
                "results": [],
            }),
            Self::MissingCredentials { refs, .. } => json!({
                "error": "missing_credentials",
                "category": public_category(category),
                "source": source,
                "message": format!("Missing credentials: {}", refs.join(", ")),
                "missing_credentials": refs,
                "results": [],
            }),
            Self::Timeout { seconds } => json!({
                "error": "retrieval_timeout",
                "category": public_category(category),
                "source": source,
                "message": format!("Retrieval timed out after {seconds}s"),
                "results": [],
            }),
            other => json!({
                "error": "retrieval_error",
                "category": public_category(category),
                "source": source,
                "message": other.to_string(),
                "results": [],
            }),
        }
    }
}

impl From<RetrievalError> for ToolError {
    fn from(value: RetrievalError) -> Self {
        match value {
            RetrievalError::InvalidRequest { message } => ToolError::InvalidArguments { message },
            RetrievalError::SourceDisabled { message, .. } => {
                ToolError::InvalidArguments { message }
            }
            RetrievalError::MissingCredentials {
                category,
                source_id,
                refs,
            } => ToolError::InvalidArguments {
                message: format!(
                    "Missing credentials for {category}.{source_id}: {}",
                    refs.join(", ")
                ),
            },
            RetrievalError::ProviderUnavailable { message }
            | RetrievalError::Protocol { message, .. }
            | RetrievalError::ExecutionFailed { message } => ToolError::ExecutionFailed { message },
            RetrievalError::Timeout { seconds } => ToolError::Timeout { seconds },
            RetrievalError::Cancelled => ToolError::Cancelled,
        }
    }
}

pub fn public_category(category: &str) -> &str {
    match category {
        "dataset" => "data",
        other => other,
    }
}

fn empty_object() -> JsonValue {
    json!({})
}
