//! Discover MCP `tools/list` entries and expose them as [`ToolSchema`] for the LLM (TS `assembleToolPool`).

use crate::domain::mcp::client::list_tools_for_server;
use crate::domain::mcp::config::merged_mcp_servers;
use crate::domain::mcp::names::build_mcp_tool_name;
use crate::domain::tools::ToolSchema;
use futures::future::join_all;
use serde_json::json;
use std::path::Path;
use std::time::Duration;

/// List tools from every configured MCP server (parallel). Failures are logged; successful servers still contribute.
pub async fn discover_mcp_tool_schemas(project_root: &Path, timeout: Duration) -> Vec<ToolSchema> {
    let merged = merged_mcp_servers(project_root);
    if merged.is_empty() {
        return vec![];
    }
    let mut names: Vec<String> = merged.keys().cloned().collect();
    names.sort();
    let handles = names.into_iter().map(|name| {
        let project_root = project_root.to_path_buf();
        async move {
            let r = list_tools_for_server(&project_root, &name, timeout).await;
            (name, r)
        }
    });
    let results = join_all(handles).await;
    let mut out = Vec::new();
    for (server_name, res) in results {
        match res {
            Ok(tools) => {
                for t in tools {
                    let fq = build_mcp_tool_name(&server_name, t.name.as_ref());
                    let desc = t
                        .description
                        .as_deref()
                        .unwrap_or("MCP tool")
                        .to_string();
                    let params = serde_json::to_value(&*t.input_schema)
                        .unwrap_or_else(|_| json!({"type": "object"}));
                    out.push(ToolSchema::new(fq, desc, params));
                }
            }
            Err(e) => {
                tracing::warn!("MCP tools/list failed for server \"{server_name}\": {e}");
            }
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}
