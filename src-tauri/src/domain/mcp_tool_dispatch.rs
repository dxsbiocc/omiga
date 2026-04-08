//! Dispatch `mcp__{server}__{tool}` invocations to MCP `tools/call` (Claude Code naming).
//!
//! Uses the process-wide [`McpLiveConnection`] pool stored in [`ChatState::mcp_connections`] to
//! avoid spawning a new stdio process (or HTTP session) on every tool call.  On cache miss or
//! closed connection the pool entry is transparently rebuilt.

use crate::domain::integrations_config;
use crate::domain::mcp_client::{
    call_tool_on_server, call_tool_via_peer, connect_mcp_server, McpLiveConnection,
};
use crate::domain::mcp_config::merged_mcp_servers;
use crate::domain::mcp_names::{normalize_name_for_mcp, parse_mcp_tool_name};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

/// Run an MCP dynamic tool; returns JSON text for the model and whether the server marked an error.
///
/// Pass the `connection_pool` from [`ChatState::mcp_connections`] to reuse live connections
/// across calls (eliminates per-call spawn + handshake overhead).
/// If `connection_pool` is `None` the function falls back to the legacy one-shot path.
pub async fn execute_mcp_tool_call(
    project_root: &Path,
    full_tool_name: &str,
    arguments_json: &str,
    timeout: Duration,
    connection_pool: Option<Arc<Mutex<HashMap<String, McpLiveConnection>>>>,
) -> Result<(String, bool), String> {
    let (server_norm, tool_norm) = parse_mcp_tool_name(full_tool_name)
        .ok_or_else(|| format!("invalid MCP tool name: {full_tool_name}"))?;

    let icfg = integrations_config::load_integrations_config(project_root);
    if icfg.is_mcp_normalized_disabled(&server_norm) {
        return Err(format!(
            "MCP server \"{server_norm}\" is disabled in Omiga Settings → Integrations (MCP)."
        ));
    }

    let merged = merged_mcp_servers(project_root);
    let server_key = merged
        .keys()
        .find(|k| normalize_name_for_mcp(k) == server_norm)
        .cloned()
        .ok_or_else(|| {
            format!(
                "no MCP server in merged Omiga MCP config matches normalized name \"{server_norm}\""
            )
        })?;

    let args_val: serde_json::Value =
        serde_json::from_str(arguments_json).unwrap_or_else(|_| serde_json::json!({}));
    let args_map = match args_val.as_object() {
        Some(o) => o.clone(),
        None => serde_json::Map::new(),
    };

    // --- Pooled path ---
    if let Some(pool) = connection_pool {
        let pool_key = format!("{}::{}", project_root.display(), server_key);

        // Try to reuse existing connection.
        let orig_name_and_peer = {
            let pool_guard = pool.lock().await;
            if let Some(conn) = pool_guard.get(&pool_key) {
                if !conn.is_closed() {
                    let orig = conn
                        .tool_name_map
                        .get(&tool_norm)
                        .cloned()
                        .unwrap_or_else(|| tool_norm.clone());
                    Some((orig, conn.peer.clone()))
                } else {
                    None
                }
            } else {
                None
            }
        };

        let (orig_name, peer) = if let Some(cached) = orig_name_and_peer {
            cached
        } else {
            // Connection missing or dead — (re)connect and update pool.
            tracing::debug!(
                target: "omiga::mcp",
                server = %server_key,
                "MCP connection pool miss — connecting"
            );
            match connect_mcp_server(project_root, &server_key, timeout).await {
                Ok(conn) => {
                    let orig = conn
                        .tool_name_map
                        .get(&tool_norm)
                        .cloned()
                        .unwrap_or_else(|| tool_norm.clone());
                    let peer = conn.peer.clone();
                    let mut pool_guard = pool.lock().await;
                    pool_guard.insert(pool_key, conn);
                    (orig, peer)
                }
                Err(e) => {
                    tracing::warn!(
                        target: "omiga::mcp",
                        server = %server_key,
                        error = %e,
                        "pool connect failed — falling back to one-shot"
                    );
                    // Fall back to legacy one-shot path below.
                    return execute_one_shot(
                        project_root, &server_key, &tool_norm, args_map, timeout,
                    ).await;
                }
            }
        };

        let result = call_tool_via_peer(&peer, &orig_name, Some(args_map.clone()), timeout).await;

        match result {
            Ok(r) => {
                let is_err = r.is_error.unwrap_or(false);
                let text = serde_json::to_string_pretty(&r).map_err(|e| e.to_string())?;
                return Ok((text, is_err));
            }
            Err(e) => {
                // Connection may have died mid-session; evict and retry once with one-shot.
                tracing::warn!(
                    target: "omiga::mcp",
                    server = %server_key,
                    error = %e,
                    "pooled call failed — evicting connection and retrying"
                );
                let pool_key2 = format!("{}::{}", project_root.display(), server_key);
                let _ = pool.lock().await.remove(&pool_key2);
                return execute_one_shot(
                    project_root, &server_key, &tool_norm, args_map, timeout,
                ).await;
            }
        }
    }

    // --- Legacy one-shot path (no pool provided) ---
    execute_one_shot(project_root, &server_key, &tool_norm, args_map, timeout).await
}

/// Legacy one-shot: spawn → handshake → list → call → close.
async fn execute_one_shot(
    project_root: &Path,
    server_key: &str,
    tool_norm: &str,
    args_map: serde_json::Map<String, serde_json::Value>,
    timeout: Duration,
) -> Result<(String, bool), String> {
    use crate::domain::mcp_client::list_tools_for_server;

    let tools = list_tools_for_server(project_root, server_key, timeout).await?;
    let orig_name = tools
        .iter()
        .find(|t| normalize_name_for_mcp(t.name.as_ref()) == tool_norm)
        .map(|t| t.name.to_string())
        .ok_or_else(|| format!("MCP tool \"{tool_norm}\" not found on server \"{server_key}\""))?;

    let result = call_tool_on_server(project_root, server_key, &orig_name, Some(args_map), timeout)
        .await?;
    let is_err = result.is_error.unwrap_or(false);
    let text = serde_json::to_string_pretty(&result).map_err(|e| e.to_string())?;
    Ok((text, is_err))
}
