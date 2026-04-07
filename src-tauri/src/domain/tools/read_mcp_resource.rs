//! Read a resource — aligned with `ReadMcpResourceTool` (TypeScript).
//!
//! - **http(s)://** URIs: fetched with `reqwest`; response is wrapped as `{ contents: [...] }` (TS
//!   only defines MCP shape; Omiga uses the same envelope for direct HTTP reads).
//! - **Other URIs**: MCP `resources/read`, then blob → disk + `{ contents }` like TS (`mcp_resource_output`).

use super::{ToolContext, ToolError, ToolSchema};
use crate::constants::tool_limits::DEFAULT_MAX_RESULT_SIZE_CHARS;
use crate::domain::integrations_config;
use crate::domain::mcp_client::read_resource_for_server;
use crate::domain::mcp_config::merged_mcp_servers;
use crate::domain::mcp_discovery;
use crate::domain::mcp_resource_output::read_resource_result_to_ts_json;
use crate::infrastructure::streaming::{StreamOutput, StreamOutputItem};
use async_trait::async_trait;
use serde_json::json;
use std::path::PathBuf;
use std::pin::Pin;
use std::time::Duration;

pub const DESCRIPTION: &str = r#"Read a resource. For `http://` or `https://` URIs, Omiga fetches the URL and returns `{ "contents": [...] }`. For MCP resource URIs, Omiga calls `resources/read` on the named server (merged Omiga MCP: bundled + ~/.omiga/mcp.json + project .omiga/mcp.json); binary blobs are saved under the session tool-results directory with `blobSavedTo`, matching Claude Code behavior."#;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ReadMcpResourceArgs {
    pub server: String,
    pub uri: String,
}

fn tool_results_dir_for_mcp(ctx: &ToolContext) -> PathBuf {
    ctx.tool_results_dir
        .clone()
        .or_else(|| ctx.background_output_dir.clone())
        .unwrap_or_else(|| std::env::temp_dir().join("omiga-tool-results"))
}

pub struct ReadMcpResourceTool;

#[async_trait]
impl super::ToolImpl for ReadMcpResourceTool {
    type Args = ReadMcpResourceArgs;

    const DESCRIPTION: &'static str = DESCRIPTION;

    async fn execute(
        ctx: &ToolContext,
        args: Self::Args,
    ) -> Result<crate::infrastructure::streaming::StreamOutputBox, ToolError> {
        let uri = args.uri.trim();
        if uri.is_empty() {
            return Err(ToolError::InvalidArguments {
                message: "`uri` must not be empty.".to_string(),
            });
        }

        let server = args.server.trim();
        if server.is_empty() {
            return Err(ToolError::InvalidArguments {
                message: "`server` must not be empty.".to_string(),
            });
        }

        if uri.starts_with("http://") || uri.starts_with("https://") {
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(ctx.timeout_secs.min(120)))
                .build()
                .map_err(|e| ToolError::ExecutionFailed {
                    message: format!("HTTP client: {e}"),
                })?;
            let resp = client
                .get(uri)
                .send()
                .await
                .map_err(|e| ToolError::ExecutionFailed {
                    message: format!("HTTP fetch failed: {e}"),
                })?;
            let status = resp.status();
            let mut text = resp
                .text()
                .await
                .map_err(|e| ToolError::ExecutionFailed {
                    message: format!("HTTP body: {e}"),
                })?;
            let truncated = text.len() > DEFAULT_MAX_RESULT_SIZE_CHARS;
            if truncated {
                text.truncate(DEFAULT_MAX_RESULT_SIZE_CHARS);
                text.push_str("\n\n[Truncated to DEFAULT_MAX_RESULT_SIZE_CHARS]");
            }
            let payload = json!({
                "contents": [{
                    "uri": uri,
                    "mimeType": "text/plain",
                    "text": text,
                }],
                "httpStatus": status.as_u16(),
                "_omigaNote": if truncated { "Response truncated for size." } else { "Fetched via HTTP (not MCP protocol)." }
            });
            let s = serde_json::to_string_pretty(&payload).map_err(|e| ToolError::ExecutionFailed {
                message: e.to_string(),
            })?;
            return Ok(JsonOut { text: s }.into_stream());
        }

        let icfg = integrations_config::load_integrations_config(&ctx.project_root);
        if integrations_config::is_mcp_config_server_disabled(&icfg, server) {
            return Err(ToolError::ExecutionFailed {
                message: format!(
                    "MCP server \"{server}\" is disabled in Omiga Settings → Integrations (MCP)."
                ),
            });
        }

        let merged = merged_mcp_servers(&ctx.project_root);
        if !merged.contains_key(server) {
            let hint = if mcp_discovery::collect_mcp_server_names(&ctx.project_root).is_empty() {
                "No MCP servers listed in Omiga MCP config (~/.omiga/mcp.json or project .omiga/mcp.json).".to_string()
            } else {
                let mut names: Vec<String> = merged.keys().cloned().collect();
                names.sort();
                format!("Configured MCP server names (merged Omiga MCP): {}", names.join(", "))
            };
            return Err(ToolError::ExecutionFailed {
                message: format!(
                    "Server \"{server}\" not found in merged Omiga MCP config. {hint}",
                ),
            });
        }

        let timeout = Duration::from_secs(ctx.timeout_secs.min(120));
        let result = read_resource_for_server(&ctx.project_root, server, uri, timeout)
            .await
            .map_err(|e| ToolError::ExecutionFailed { message: e })?;

        let dir = tool_results_dir_for_mcp(ctx);
        let payload = read_resource_result_to_ts_json(result, server, &dir).await?;

        let s = serde_json::to_string_pretty(&payload).map_err(|e| ToolError::ExecutionFailed {
            message: e.to_string(),
        })?;
        Ok(JsonOut { text: s }.into_stream())
    }
}

struct JsonOut {
    text: String,
}

impl StreamOutput for JsonOut {
    fn into_stream(self) -> Pin<Box<dyn futures::Stream<Item = StreamOutputItem> + Send>> {
        use futures::stream;
        Box::pin(stream::iter(vec![
            StreamOutputItem::Start,
            StreamOutputItem::Content(self.text),
            StreamOutputItem::Complete,
        ]))
    }
}

pub fn schema() -> ToolSchema {
    ToolSchema::new(
        "read_mcp_resource",
        DESCRIPTION,
        serde_json::json!({
            "type": "object",
            "properties": {
                "server": {
                    "type": "string",
                    "description": "MCP server name from Omiga MCP config (required for MCP resource URIs)"
                },
                "uri": {
                    "type": "string",
                    "description": "Resource URI from list_mcp_resources, or http(s) URL"
                }
            },
            "required": ["server", "uri"]
        }),
    )
}
