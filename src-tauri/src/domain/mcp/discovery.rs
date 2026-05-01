//! Discover MCP server names from Omiga-only `mcp.json` files (bundled + `~/.omiga` + project `.omiga`).
//! Does not connect to MCP transports — used so `list_mcp_resources` can report configured servers.

use serde_json::Value as Json;
use std::path::Path;

/// Parse `mcp.json` and return server keys under `mcpServers`.
pub fn server_names_from_mcp_json(raw: &str) -> Vec<String> {
    let Ok(v) = serde_json::from_str::<Json>(raw) else {
        return vec![];
    };
    let Some(obj) = v.get("mcpServers").and_then(|x| x.as_object()) else {
        return vec![];
    };
    let mut names: Vec<String> = obj.keys().cloned().collect();
    names.sort();
    names
}

fn read_if_exists(path: &Path) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

fn bundled_mcp_raw() -> &'static str {
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/bundled_mcp.json"))
}

fn user_omiga_mcp_path() -> Option<std::path::PathBuf> {
    dirs::home_dir().map(|h| h.join(".omiga").join("mcp.json"))
}

/// All configured server names from bundled defaults, `~/.omiga/mcp.json`, and `<project>/.omiga/mcp.json`
/// (same merge order as [`crate::domain::mcp::config::merged_mcp_servers`]).
pub fn collect_mcp_server_names(project_root: &Path) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();

    out.extend(server_names_from_mcp_json(bundled_mcp_raw()));

    if let Some(p) = user_omiga_mcp_path() {
        if let Some(raw) = read_if_exists(&p) {
            out.extend(server_names_from_mcp_json(&raw));
        }
    }

    out.extend(crate::domain::plugins::enabled_plugin_mcp_servers().into_keys());

    let proj = project_root.join(".omiga").join("mcp.json");
    if let Some(raw) = read_if_exists(&proj) {
        out.extend(server_names_from_mcp_json(&raw));
    }

    out.sort();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_mcp_servers_keys() {
        let raw = r#"{"mcpServers":{"a":{"command":"x"},"b":{"url":"http://localhost"}}}"#;
        let mut n = server_names_from_mcp_json(raw);
        n.sort();
        assert_eq!(n, vec!["a", "b"]);
    }
}
