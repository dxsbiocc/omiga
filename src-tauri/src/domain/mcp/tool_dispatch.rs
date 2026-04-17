//! Dispatch `mcp__{server}__{tool}` invocations to MCP `tools/call` (Claude Code naming).
//!
//! Uses the session-aware [`McpConnectionManager`] stored in [`ChatState::mcp_manager`] to
//! avoid spawning a new stdio process (or HTTP session) on every tool call, while properly
//! managing session boundaries and stdio process lifecycle to avoid zombie processes.

use crate::domain::integrations_config;
use crate::domain::mcp::client::{
    call_tool_on_server, call_tool_via_peer, connect_mcp_server_legacy, McpLiveConnection,
};
use crate::domain::mcp::config::merged_mcp_servers;
use crate::domain::mcp::connection_manager::GlobalMcpManager;
use crate::domain::mcp::names::{normalize_name_for_mcp, parse_mcp_tool_name};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

/// Legacy connection pool type (for backwards compatibility)
type LegacyConnectionPool = Arc<Mutex<HashMap<String, McpLiveConnection>>>;

/// Run an MCP dynamic tool; returns JSON text for the model and whether the server marked an error.
///
/// This function uses the session-aware connection manager which:
/// - Reuses healthy connections across tool calls within the same session
/// - Reconnects stdio processes on session boundaries (avoiding zombies)
/// - Performs health checks on pooled connections
/// - Falls back to one-shot on connection failure
///
/// # Arguments
/// * `project_root` - Project root path
/// * `full_tool_name` - Full tool name in format `mcp__{server}__{tool}`
/// * `arguments_json` - JSON-encoded tool arguments
/// * `timeout` - Tool execution timeout
/// * `mcp_manager` - The global MCP connection manager (recommended)
/// * `connection_pool` - Legacy connection pool (deprecated, use mcp_manager)
/// * `session_id` - Current session ID for session boundary detection (required when using mcp_manager)
pub async fn execute_mcp_tool_call(
    project_root: &Path,
    full_tool_name: &str,
    arguments_json: &str,
    timeout: Duration,
    mcp_manager: Option<Arc<GlobalMcpManager>>,
    connection_pool: Option<LegacyConnectionPool>,
    session_id: Option<String>,
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

    // --- Session-aware managed path (recommended) ---
    if let (Some(manager), Some(session_id)) = (mcp_manager, session_id) {
        let project_root_owned = project_root.to_path_buf();

        // Get or create manager for this project
        let project_manager = manager.get_manager(project_root_owned, session_id).await;

        // Get connection (will auto-reconnect if needed based on session/health)
        match project_manager.get_connection(&server_key, timeout).await {
            Ok(conn) => {
                let orig = conn
                    .tool_name_map
                    .get(&tool_norm)
                    .cloned()
                    .unwrap_or_else(|| tool_norm.clone());

                let result =
                    call_tool_via_peer(&conn.peer, &orig, Some(args_map.clone()), timeout).await;

                match result {
                    Ok(r) => {
                        let is_err = r.is_error.unwrap_or(false);
                        let text = serde_json::to_string_pretty(&r).map_err(|e| e.to_string())?;
                        return Ok((text, is_err));
                    }
                    Err(e) => {
                        // Connection died mid-call, evict and retry with one-shot
                        tracing::warn!(
                            target: "omiga::mcp",
                            server = %server_key,
                            error = %e,
                            "managed connection failed — evicting and retrying"
                        );
                        let _ = project_manager.reconnect_server(&server_key).await;
                        return execute_one_shot(
                            project_root,
                            &server_key,
                            &tool_norm,
                            args_map,
                            timeout,
                        )
                        .await;
                    }
                }
            }
            Err(e) => {
                tracing::warn!(
                    target: "omiga::mcp",
                    server = %server_key,
                    error = %e,
                    "managed connection failed — falling back to one-shot"
                );
                return execute_one_shot(project_root, &server_key, &tool_norm, args_map, timeout)
                    .await;
            }
        }
    }

    // --- Legacy pooled path (deprecated) ---
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
            match connect_mcp_server_legacy(project_root, &server_key, timeout).await {
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
                        project_root,
                        &server_key,
                        &tool_norm,
                        args_map,
                        timeout,
                    )
                    .await;
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
                return execute_one_shot(project_root, &server_key, &tool_norm, args_map, timeout)
                    .await;
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
    use crate::domain::mcp::client::list_tools_for_server;

    let tools = list_tools_for_server(project_root, server_key, timeout).await?;
    let orig_name = tools
        .iter()
        .find(|t| normalize_name_for_mcp(t.name.as_ref()) == tool_norm)
        .map(|t| t.name.to_string())
        .ok_or_else(|| format!("MCP tool \"{tool_norm}\" not found on server \"{server_key}\""))?;

    let result = call_tool_on_server(
        project_root,
        server_key,
        &orig_name,
        Some(args_map),
        timeout,
    )
    .await?;
    let is_err = result.is_error.unwrap_or(false);
    let text = serde_json::to_string_pretty(&result).map_err(|e| e.to_string())?;
    Ok((text, is_err))
}
