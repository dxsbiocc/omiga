//! Dispatch `mcp__{server}__{tool}` invocations to MCP `tools/call` (Claude Code naming).

use crate::domain::integrations_config;
use crate::domain::mcp_client::{call_tool_on_server, list_tools_for_server};
use crate::domain::mcp_config::merged_mcp_servers;
use crate::domain::mcp_names::{normalize_name_for_mcp, parse_mcp_tool_name};
use std::path::Path;
use std::time::Duration;

/// Run an MCP dynamic tool; returns JSON text for the model and whether the server marked an error.
pub async fn execute_mcp_tool_call(
    project_root: &Path,
    full_tool_name: &str,
    arguments_json: &str,
    timeout: Duration,
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

    let tools = list_tools_for_server(project_root, &server_key, timeout).await?;
    let orig_name = tools
        .iter()
        .find(|t| normalize_name_for_mcp(t.name.as_ref()) == tool_norm)
        .map(|t| t.name.to_string())
        .ok_or_else(|| {
            format!("MCP tool \"{tool_norm}\" not found on server \"{server_key}\"")
        })?;

    let args_val: serde_json::Value =
        serde_json::from_str(arguments_json).unwrap_or_else(|_| serde_json::json!({}));
    let args_map = match args_val.as_object() {
        Some(o) => o.clone(),
        None => serde_json::Map::new(),
    };

    let result = call_tool_on_server(
        project_root,
        &server_key,
        &orig_name,
        Some(args_map),
        timeout,
    )
    .await?;

    let is_err = result.is_error.unwrap_or(false);
    let text = serde_json::to_string_pretty(&result).map_err(|e| e.to_string())?;
    Ok((text, is_err))
}
