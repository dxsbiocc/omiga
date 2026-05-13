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
    #[error("retrieval resource disabled: {category}.{source_id}: {message}")]
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
        let category = public_category(category);
        let route = format!("{category}.{source}");
        match self {
            Self::SourceDisabled { message, .. } => json!({
                "error": "source_disabled",
                "category": category,
                "source": source,
                "route": route,
                "message": format!("{route} is available as a local retrieval plugin route, but it is disabled."),
                "details": message,
                "next_action": "Enable this plugin in Settings → Plugins, then retry the same search/query/fetch call.",
                "diagnostics_hint": "Open Settings → Plugins → Details to inspect the route and copy diagnostics if it still fails.",
                "recoverable": true,
                "results": [],
            }),
            Self::MissingCredentials { refs, .. } => json!({
                "error": "missing_credentials",
                "category": category,
                "source": source,
                "route": route,
                "message": format!("Missing credentials for {route}: {}", refs.join(", ")),
                "next_action": "Configure the required credential references for this plugin route, then retry.",
                "diagnostics_hint": "Use Settings → Plugins → Details to see the route and copy diagnostics without exposing credential values.",
                "recoverable": true,
                "missing_credentials": refs,
                "results": [],
            }),
            Self::Timeout { seconds } => json!({
                "error": "retrieval_plugin_timeout",
                "category": category,
                "source": source,
                "route": route,
                "message": format!("Local retrieval plugin route {route} timed out after {seconds}s."),
                "next_action": "Retry once; if it repeats, reduce the request size or inspect the plugin process in Settings → Plugins.",
                "diagnostics_hint": "Timed-out plugin child processes are discarded instead of returned to the pool. Copy route diagnostics from the plugin Details dialog.",
                "recoverable": true,
                "results": [],
            }),
            Self::ProviderUnavailable { message } => {
                let lower = message.to_ascii_lowercase();
                let (error, next_action, recoverable) = if lower.contains("quarantined") {
                    (
                        "retrieval_plugin_quarantined",
                        "Wait for the quarantine window to expire, then retry. If it repeats, inspect the plugin Details diagnostics.",
                        true,
                    )
                } else {
                    (
                        "retrieval_plugin_unavailable",
                        "Confirm the plugin is installed and enabled in Settings → Plugins, then refresh plugins.",
                        true,
                    )
                };
                json!({
                    "error": error,
                    "category": category,
                    "source": source,
                    "route": route,
                    "message": message,
                    "next_action": next_action,
                    "diagnostics_hint": "Open Settings → Plugins → Details and copy route diagnostics for troubleshooting.",
                    "recoverable": recoverable,
                    "results": [],
                })
            }
            Self::Protocol { plugin, message } => {
                let (error, next_action) = classify_plugin_protocol_error(message);
                json!({
                    "error": error,
                    "category": category,
                    "source": source,
                    "route": route,
                    "plugin": plugin,
                    "message": message,
                    "next_action": next_action,
                    "diagnostics_hint": "Validate the plugin against docs/retrieval-plugin-protocol.md and copy route diagnostics from Settings → Plugins → Details.",
                    "recoverable": false,
                    "results": [],
                })
            }
            Self::ExecutionFailed { message } => json!({
                "error": "retrieval_plugin_failed",
                "category": category,
                "source": source,
                "route": route,
                "message": message,
                "next_action": "Retry once. If the error repeats, inspect the plugin Details diagnostics or disable the plugin route.",
                "diagnostics_hint": "The child process failed the request and was discarded; copy route diagnostics from Settings → Plugins → Details.",
                "recoverable": true,
                "results": [],
            }),
            Self::InvalidRequest { message } => json!({
                "error": "invalid_retrieval_request",
                "category": category,
                "source": source,
                "route": route,
                "message": message,
                "next_action": "Check the search/query/fetch arguments: category, source, operation, id/url/query, and max_results.",
                "recoverable": true,
                "results": [],
            }),
            Self::Cancelled => json!({
                "error": "retrieval_cancelled",
                "category": category,
                "source": source,
                "route": route,
                "message": "Retrieval was cancelled.",
                "next_action": "Retry if you still need this result.",
                "recoverable": true,
                "results": [],
            }),
        }
    }
}

fn classify_plugin_protocol_error(message: &str) -> (&'static str, &'static str) {
    let lower = message.to_ascii_lowercase();
    if lower.contains("exited before response") {
        (
            "retrieval_plugin_process_exited",
            "Restart or reinstall the plugin; if it repeats, inspect the plugin logs/diagnostics and validate its executable.",
        )
    } else if lower.contains("spawn plugin process") {
        (
            "retrieval_plugin_process_start_failed",
            "Check that the plugin executable exists, is executable, and that its manifest runtime command is valid.",
        )
    } else if lower.contains("parse plugin response json")
        || lower.contains("expected result/error response")
        || lower.contains("unsupported plugin response")
        || lower.contains("did not match request")
        || lower.contains("initialized with unsupported protocol version")
    {
        (
            "retrieval_plugin_protocol_error",
            "Validate that the plugin speaks the local JSONL retrieval protocol documented in docs/retrieval-plugin-protocol.md.",
        )
    } else {
        (
            "retrieval_plugin_protocol_error",
            "Inspect the plugin manifest/runtime and validate it against docs/retrieval-plugin-protocol.md.",
        )
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn structured_source_disabled_error_points_to_plugins_settings() {
        let value = RetrievalError::SourceDisabled {
            category: "dataset".to_string(),
            source_id: "geo".to_string(),
            message: "dataset.geo is disabled. Enable it in Settings → Search.".to_string(),
        }
        .structured_json("dataset", "geo");

        assert_eq!(value["error"], json!("source_disabled"));
        assert_eq!(value["category"], json!("data"));
        assert_eq!(value["route"], json!("data.geo"));
        assert_eq!(value["recoverable"], json!(true));
        assert!(value["next_action"]
            .as_str()
            .unwrap()
            .contains("Settings → Plugins"));
        assert_eq!(value["results"], json!([]));
    }

    #[test]
    fn structured_timeout_error_is_actionable() {
        let value = RetrievalError::Timeout { seconds: 7 }.structured_json("dataset", "geo");

        assert_eq!(value["error"], json!("retrieval_plugin_timeout"));
        assert_eq!(value["route"], json!("data.geo"));
        assert!(value["message"].as_str().unwrap().contains("7s"));
        assert!(value["diagnostics_hint"]
            .as_str()
            .unwrap()
            .contains("Timed-out plugin child processes are discarded"));
    }

    #[test]
    fn structured_protocol_error_distinguishes_process_exit() {
        let value = RetrievalError::Protocol {
            plugin: Some("mock-plugin".to_string()),
            message: "plugin exited before response".to_string(),
        }
        .structured_json("dataset", "geo");

        assert_eq!(value["error"], json!("retrieval_plugin_process_exited"));
        assert_eq!(value["plugin"], json!("mock-plugin"));
        assert_eq!(value["recoverable"], json!(false));
        assert!(value["next_action"]
            .as_str()
            .unwrap()
            .contains("Restart or reinstall"));
    }

    #[test]
    fn structured_quarantine_error_has_retry_guidance() {
        let value = RetrievalError::ProviderUnavailable {
            message: "retrieval plugin route dataset.geo via mock is quarantined for 30s after 3 consecutive failures. Last error: upstream failed".to_string(),
        }
        .structured_json("dataset", "geo");

        assert_eq!(value["error"], json!("retrieval_plugin_quarantined"));
        assert_eq!(value["recoverable"], json!(true));
        assert!(value["next_action"]
            .as_str()
            .unwrap()
            .contains("quarantine window"));
    }
}
