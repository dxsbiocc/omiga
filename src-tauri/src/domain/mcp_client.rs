//! Connect to MCP servers via **rmcp** (stdio or streamable HTTP) and run `resources/*` calls.

use crate::domain::mcp_config::{McpServerConfig, merged_mcp_servers};
use rmcp::ServiceExt;
use rmcp::model::{
    CallToolRequestParams, CallToolResult, ReadResourceRequestParams, ReadResourceResult,
};
use rmcp::model::Tool as McpTool;
use rmcp::service::{Peer, RoleClient, RunningService};
use rmcp::transport::{
    ConfigureCommandExt, TokioChildProcess,
    streamable_http_client::{StreamableHttpClientTransportConfig, StreamableHttpClientWorker},
};
use serde_json::json;
use std::path::Path;
use std::time::Duration;
use tokio::process::Command;

/// A live, reusable MCP connection.
/// Keep this value alive (e.g. in a HashMap) to hold the background I/O pump open.
/// Drop it to cancel the pump and (for stdio servers) kill the child process.
pub struct McpLiveConnection {
    /// Cloneable handle used to send requests; cheap to clone.
    pub peer: Peer<RoleClient>,
    /// Background service that drives the transport; must stay alive.
    _running: RunningService<RoleClient, ()>,
    /// Normalized-tool-name → original-server-tool-name map, built once on connect.
    /// Avoids calling `tools/list` on every tool invocation.
    pub tool_name_map: std::collections::HashMap<String, String>,
}

impl McpLiveConnection {
    /// Returns true if the underlying transport has been cancelled or closed.
    pub fn is_closed(&self) -> bool {
        self._running.is_closed()
    }
}

/// Establish a persistent connection to a named MCP server and eagerly fetch the tool list.
/// Returns an [`McpLiveConnection`] whose lifetime controls the underlying process / HTTP session.
/// The embedded `tool_name_map` lets callers resolve normalized → original tool names without
/// a second round-trip.
pub async fn connect_mcp_server(
    project_root: &Path,
    server_name: &str,
    timeout: Duration,
) -> Result<McpLiveConnection, String> {
    use crate::domain::mcp_names::normalize_name_for_mcp;

    let cfg = merged_mcp_servers(project_root)
        .remove(server_name)
        .ok_or_else(|| {
            format!(
                "MCP server \"{server_name}\" not found in merged Omiga MCP config (bundled defaults, ~/.omiga/mcp.json, <project>/.omiga/mcp.json)"
            )
        })?;

    let running = create_running_service(project_root, cfg, timeout).await?;
    let peer = running.peer().clone();

    // Eagerly fetch tool list so every subsequent call can resolve names without a round-trip.
    let tools = list_tools_via_peer(&peer, timeout).await.unwrap_or_default();
    let tool_name_map: std::collections::HashMap<String, String> = tools
        .into_iter()
        .map(|t| (normalize_name_for_mcp(t.name.as_ref()), t.name.to_string()))
        .collect();

    Ok(McpLiveConnection { peer, _running: running, tool_name_map })
}

/// Call a tool using a pre-established peer (no new connection overhead).
pub async fn call_tool_via_peer(
    peer: &Peer<RoleClient>,
    tool_name: &str,
    arguments: Option<serde_json::Map<String, serde_json::Value>>,
    timeout: Duration,
) -> Result<CallToolResult, String> {
    let peer = peer.clone();
    let tool_name = tool_name.to_string();
    tokio::time::timeout(timeout, async move {
        let mut params = CallToolRequestParams::new(tool_name);
        if let Some(args) = arguments {
            params = params.with_arguments(args);
        }
        peer.call_tool(params)
            .await
            .map_err(|e| format!("tools/call: {e}"))
    })
    .await
    .map_err(|_| "MCP operation timed out".to_string())?
}

/// List tools using a pre-established peer.
pub async fn list_tools_via_peer(
    peer: &Peer<RoleClient>,
    timeout: Duration,
) -> Result<Vec<McpTool>, String> {
    let peer = peer.clone();
    tokio::time::timeout(timeout, async move {
        peer.list_all_tools()
            .await
            .map_err(|e| format!("tools/list: {e}"))
    })
    .await
    .map_err(|_| "MCP operation timed out".to_string())?
}

/// List resources from one named server (MCP `resources/list` with pagination via rmcp).
pub async fn list_resources_for_server(
    project_root: &Path,
    server_name: &str,
    timeout: Duration,
) -> Result<Vec<serde_json::Value>, String> {
    let cfg = merged_mcp_servers(project_root)
        .remove(server_name)
        .ok_or_else(|| {
            format!(
                "MCP server \"{server_name}\" not found in merged Omiga MCP config (bundled defaults, ~/.omiga/mcp.json, <project>/.omiga/mcp.json)"
            )
        })?;

    with_mcp_peer(project_root, cfg, timeout, |peer| async move {
        let resources = peer
            .list_all_resources()
            .await
            .map_err(|e| format!("resources/list: {e}"))?;
        let mut out = Vec::with_capacity(resources.len());
        for r in resources {
            let mut v = serde_json::to_value(&r).unwrap_or_else(|_| json!({ "error": "serialize Resource" }));
            if let Some(obj) = v.as_object_mut() {
                obj.insert("server".to_string(), json!(server_name));
            }
            out.push(v);
        }
        Ok(out)
    })
    .await
}

/// Read one resource (MCP `resources/read`).
pub async fn read_resource_for_server(
    project_root: &Path,
    server_name: &str,
    uri: &str,
    timeout: Duration,
) -> Result<ReadResourceResult, String> {
    let cfg = merged_mcp_servers(project_root)
        .remove(server_name)
        .ok_or_else(|| {
            format!(
                "MCP server \"{server_name}\" not found in merged Omiga MCP config (bundled defaults, ~/.omiga/mcp.json, <project>/.omiga/mcp.json)"
            )
        })?;

    with_mcp_peer(project_root, cfg, timeout, |peer| async move {
        peer.read_resource(ReadResourceRequestParams::new(uri))
            .await
            .map_err(|e| format!("resources/read: {e}"))
    })
    .await
}

/// List tools from one named server (MCP `tools/list`).
pub async fn list_tools_for_server(
    project_root: &Path,
    server_name: &str,
    timeout: Duration,
) -> Result<Vec<McpTool>, String> {
    let cfg = merged_mcp_servers(project_root)
        .remove(server_name)
        .ok_or_else(|| {
            format!(
                "MCP server \"{server_name}\" not found in merged Omiga MCP config (bundled defaults, ~/.omiga/mcp.json, <project>/.omiga/mcp.json)"
            )
        })?;

    with_mcp_peer(project_root, cfg, timeout, |peer| async move {
        peer.list_all_tools()
            .await
            .map_err(|e| format!("tools/list: {e}"))
    })
    .await
}

/// Call a tool on a named server (MCP `tools/call`).
/// Prefer [`call_tool_via_peer`] with a pooled connection to avoid per-call spawn overhead.
pub async fn call_tool_on_server(
    project_root: &Path,
    server_name: &str,
    tool_name: &str,
    arguments: Option<serde_json::Map<String, serde_json::Value>>,
    timeout: Duration,
) -> Result<CallToolResult, String> {
    let cfg = merged_mcp_servers(project_root)
        .remove(server_name)
        .ok_or_else(|| {
            format!(
                "MCP server \"{server_name}\" not found in merged Omiga MCP config (bundled defaults, ~/.omiga/mcp.json, <project>/.omiga/mcp.json)"
            )
        })?;

    with_mcp_peer(project_root, cfg, timeout, |peer| async move {
        let mut params = CallToolRequestParams::new(tool_name.to_string());
        if let Some(args) = arguments {
            params = params.with_arguments(args);
        }
        peer.call_tool(params)
            .await
            .map_err(|e| format!("tools/call: {e}"))
    })
    .await
}

/// Create a `RunningService` from a config (used by both the pool and one-shot helpers).
async fn create_running_service(
    project_root: &Path,
    config: McpServerConfig,
    timeout: Duration,
) -> Result<RunningService<RoleClient, ()>, String> {
    match config {
        McpServerConfig::Stdio { command, args, env } => {
            let transport = TokioChildProcess::new(
                Command::new(&command).configure(|cmd| {
                    cmd.args(&args);
                    cmd.envs(env.iter().map(|(k, v)| (k.as_str(), v.as_str())));
                    cmd.current_dir(project_root);
                    cmd.stdin(std::process::Stdio::piped());
                    cmd.stdout(std::process::Stdio::piped());
                    cmd.stderr(std::process::Stdio::piped());
                }),
            )
            .map_err(|e| format!("MCP stdio spawn: {e}"))?;

            tokio::time::timeout(timeout, ().serve(transport))
                .await
                .map_err(|_| "MCP stdio handshake timed out".to_string())?
                .map_err(|e| format!("MCP stdio handshake: {e}"))
        }
        McpServerConfig::Url(url) => {
            let http = reqwest::Client::builder()
                .timeout(timeout)
                .build()
                .map_err(|e| format!("reqwest: {e}"))?;

            let worker = StreamableHttpClientWorker::new(
                http,
                StreamableHttpClientTransportConfig::with_uri(url),
            );

            tokio::time::timeout(timeout, ().serve(worker))
                .await
                .map_err(|_| "MCP HTTP handshake timed out".to_string())?
                .map_err(|e| format!("MCP HTTP handshake: {e}"))
        }
    }
}

async fn with_mcp_peer<F, Fut, T>(
    project_root: &Path,
    config: McpServerConfig,
    timeout: Duration,
    work: F,
) -> Result<T, String>
where
    F: FnOnce(Peer<RoleClient>) -> Fut,
    Fut: std::future::Future<Output = Result<T, String>>,
{
    let mut running = create_running_service(project_root, config, timeout).await?;
    let peer = running.peer().clone();
    let out = tokio::time::timeout(timeout, work(peer))
        .await
        .map_err(|_| "MCP operation timed out".to_string())??;
    let _ = running.close().await;
    Ok(out)
}
