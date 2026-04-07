//! Connect to MCP servers via **rmcp** (stdio or streamable HTTP) and run `resources/*` calls.

use crate::domain::mcp_config::{McpServerConfig, merged_mcp_servers};
use rmcp::ServiceExt;
use rmcp::model::{
    CallToolRequestParams, CallToolResult, ReadResourceRequestParams, ReadResourceResult,
};
use rmcp::model::Tool as McpTool;
use rmcp::service::{Peer, RoleClient};
use rmcp::transport::{
    ConfigureCommandExt, TokioChildProcess,
    streamable_http_client::{StreamableHttpClientTransportConfig, StreamableHttpClientWorker},
};
use serde_json::json;
use std::path::Path;
use std::time::Duration;
use tokio::process::Command;

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
    match config {
        McpServerConfig::Stdio {
            command,
            args,
            env,
        } => {
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

            let mut running = ()
                .serve(transport)
                .await
                .map_err(|e| format!("MCP stdio handshake: {e}"))?;

            let peer = running.peer().clone();
            let out = tokio::time::timeout(timeout, work(peer))
                .await
                .map_err(|_| "MCP operation timed out".to_string())??;
            let _ = running.close().await;
            Ok(out)
        }
        McpServerConfig::Url(url) => {
            let http = reqwest::Client::builder()
                .timeout(timeout)
                .build()
                .map_err(|e| format!("reqwest: {e}"))?;

            let worker =
                StreamableHttpClientWorker::new(http, StreamableHttpClientTransportConfig::with_uri(url));

            let mut running = ()
                .serve(worker)
                .await
                .map_err(|e| format!("MCP HTTP handshake: {e}"))?;

            let peer = running.peer().clone();
            let out = tokio::time::timeout(timeout, work(peer))
                .await
                .map_err(|_| "MCP operation timed out".to_string())??;
            let _ = running.close().await;
            Ok(out)
        }
    }
}
