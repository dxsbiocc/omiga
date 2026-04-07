//! List resources from MCP servers — aligned with `ListMcpResourcesTool` (TypeScript).
//!
//! Uses **rmcp** over **stdio** or **streamable HTTP** per merged Omiga MCP
//! (bundled + `~/.omiga/mcp.json` + `<project>/.omiga/mcp.json`).

use super::{ToolContext, ToolError, ToolSchema};
use crate::domain::integrations_config;
use crate::domain::mcp_client::list_resources_for_server;
use crate::domain::mcp_config::merged_mcp_servers;
use crate::domain::mcp_discovery;
use crate::infrastructure::streaming::{StreamOutput, StreamOutputItem};
use async_trait::async_trait;
use futures::future::join_all;
use std::pin::Pin;
use std::time::Duration;

pub const DESCRIPTION: &str = r#"List MCP resources from configured servers (stdio or HTTP in Omiga MCP config). Without `server`, queries every configured server in parallel. With `server`, only that server is queried (errors if the name is missing). Each resource includes a `server` field (same as Claude Code)."#;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ListMcpResourcesArgs {
    #[serde(default)]
    pub server: Option<String>,
}

pub struct ListMcpResourcesTool;

#[async_trait]
impl super::ToolImpl for ListMcpResourcesTool {
    type Args = ListMcpResourcesArgs;

    const DESCRIPTION: &'static str = DESCRIPTION;

    async fn execute(
        ctx: &ToolContext,
        args: Self::Args,
    ) -> Result<crate::infrastructure::streaming::StreamOutputBox, ToolError> {
        let merged = merged_mcp_servers(&ctx.project_root);
        let icfg = integrations_config::load_integrations_config(&ctx.project_root);
        let filter = args
            .server
            .as_ref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty());

        if let Some(name) = filter {
            if integrations_config::is_mcp_config_server_disabled(&icfg, name) {
                return Err(ToolError::ExecutionFailed {
                    message: format!(
                        "MCP server \"{name}\" is disabled in Omiga Settings → Integrations (MCP)."
                    ),
                });
            }
            if !merged.contains_key(name) {
                let available: Vec<String> = {
                    let mut v: Vec<String> = merged.keys().cloned().collect();
                    v.sort();
                    v
                };
                return Err(ToolError::ExecutionFailed {
                    message: format!(
                        "Server \"{name}\" not found in merged Omiga MCP config. Available: {}",
                        if available.is_empty() {
                            "(none)".to_string()
                        } else {
                            available.join(", ")
                        }
                    ),
                });
            }
        }

        let mut names_to_query: Vec<String> = if let Some(name) = filter {
            vec![name.to_string()]
        } else {
            let mut names: Vec<String> = merged.keys().cloned().collect();
            names.sort();
            names
        };
        names_to_query.retain(|n| !integrations_config::is_mcp_config_server_disabled(&icfg, n));

        let timeout = Duration::from_secs(ctx.timeout_secs.min(120));
        let project_root = ctx.project_root.clone();

        let handles = names_to_query.drain(..).map(|name| {
            let project_root = project_root.clone();
            async move {
                let r = list_resources_for_server(&project_root, &name, timeout).await;
                (name, r)
            }
        });
        let results = join_all(handles).await;

        let mut resources: Vec<serde_json::Value> = Vec::new();
        let mut errors: Vec<serde_json::Value> = Vec::new();
        for (name, r) in results {
            match r {
                Ok(mut v) => resources.append(&mut v),
                Err(e) => errors.push(serde_json::json!({
                    "server": name,
                    "message": e,
                })),
            }
        }

        let mut servers = mcp_discovery::collect_mcp_server_names(&ctx.project_root);
        if let Some(name) = filter {
            servers.retain(|s| s == name);
        }

        let mut payload = serde_json::json!({
            "resources": resources,
            "servers": servers,
            "query": args.server,
        });
        if !errors.is_empty() {
            payload
                .as_object_mut()
                .expect("payload is object")
                .insert("_errors".to_string(), serde_json::Value::Array(errors));
        }

        let text = serde_json::to_string_pretty(&payload).map_err(|e| ToolError::ExecutionFailed {
            message: e.to_string(),
        })?;
        Ok(JsonMcpOutput { text }.into_stream())
    }
}

struct JsonMcpOutput {
    text: String,
}

impl StreamOutput for JsonMcpOutput {
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
        "list_mcp_resources",
        DESCRIPTION,
        serde_json::json!({
            "type": "object",
            "properties": {
                "server": {
                    "type": "string",
                    "description": "Optional MCP server name; omit to list resources from all configured servers"
                }
            }
        }),
    )
}
