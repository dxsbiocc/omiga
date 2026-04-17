//! Parse merged MCP server configs from Omiga-only locations (parity with TS merge intent):
//! app-bundled defaults → user `~/.omiga/mcp.json` → project `<project>/.omiga/mcp.json`.
//! Stdio uses JSON `command`/`args`/`env`; remote uses `url` (streamable HTTP).

use serde_json::Value as Json;
use std::collections::HashMap;
use std::path::Path;

/// One MCP server entry from `mcpServers.<name>`.
#[derive(Debug, Clone)]
pub enum McpServerConfig {
    /// Local process (JSON-RPC over stdio).
    Stdio {
        command: String,
        args: Vec<String>,
        env: HashMap<String, String>,
    },
    /// Streamable HTTP / SSE endpoint (rmcp `StreamableHttpClientTransport`).
    Url(String),
}

fn read_if_exists(path: &Path) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

/// Shipped with the app; optional preset servers (later overridden by user / project files).
fn servers_from_bundled() -> HashMap<String, McpServerConfig> {
    const RAW: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/bundled_mcp.json"));
    servers_from_mcp_json(RAW)
}

fn user_omiga_mcp_path() -> Option<std::path::PathBuf> {
    dirs::home_dir().map(|h| h.join(".omiga").join("mcp.json"))
}

fn parse_env(obj: &serde_json::Map<String, Json>) -> HashMap<String, String> {
    let mut m = HashMap::new();
    for (k, v) in obj {
        if let Some(s) = v.as_str() {
            m.insert(k.clone(), s.to_string());
        } else {
            m.insert(k.clone(), v.to_string());
        }
    }
    m
}

fn parse_server_entry(v: &Json) -> Option<McpServerConfig> {
    if let Some(url) = v.get("url").and_then(|u| u.as_str()) {
        let u = url.trim();
        if !u.is_empty() {
            return Some(McpServerConfig::Url(u.to_string()));
        }
    }

    let command = v.get("command")?.as_str()?.to_string();
    let args: Vec<String> = v
        .get("args")
        .and_then(|a| a.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    let env = v
        .get("env")
        .and_then(|e| e.as_object())
        .map(parse_env)
        .unwrap_or_default();

    Some(McpServerConfig::Stdio { command, args, env })
}

fn servers_from_mcp_json(raw: &str) -> HashMap<String, McpServerConfig> {
    let Ok(v) = serde_json::from_str::<Json>(raw) else {
        return HashMap::new();
    };
    let Some(obj) = v.get("mcpServers").and_then(|x| x.as_object()) else {
        return HashMap::new();
    };
    let mut out = HashMap::new();
    for (name, cfg) in obj {
        if let Some(parsed) = parse_server_entry(cfg) {
            out.insert(name.clone(), parsed);
        }
    }
    out
}

/// Merge MCP configs (later sources win on same server name):
/// app-bundled `bundled_mcp.json` → `~/.omiga/mcp.json` → `<project>/.omiga/mcp.json`.
pub fn merged_mcp_servers(project_root: &Path) -> HashMap<String, McpServerConfig> {
    let mut merged = HashMap::new();

    merged.extend(servers_from_bundled());

    if let Some(p) = user_omiga_mcp_path() {
        if let Some(raw) = read_if_exists(&p) {
            merged.extend(servers_from_mcp_json(&raw));
        }
    }

    let proj = project_root.join(".omiga").join("mcp.json");
    if let Some(raw) = read_if_exists(&proj) {
        merged.extend(servers_from_mcp_json(&raw));
    }

    merged
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_stdio_and_url() {
        let raw = r#"{"mcpServers":{"a":{"command":"node","args":["x.js"]},"b":{"url":"http://localhost:8080/mcp"}}}"#;
        let m = servers_from_mcp_json(raw);
        assert!(matches!(
            m.get("a"),
            Some(McpServerConfig::Stdio { command, .. }) if command == "node"
        ));
        assert!(matches!(
            m.get("b"),
            Some(McpServerConfig::Url(u)) if u == "http://localhost:8080/mcp"
        ));
    }

    #[test]
    fn merged_loads_project_omiga_mcp_json() {
        let base = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("_mcp_cfg_tests");
        std::fs::create_dir_all(&base).expect("mkdir target/_mcp_cfg_tests");
        let tmp = tempfile::TempDir::new_in(&base).expect("tempdir in target/");
        let proj = tmp.path();
        let omiga = proj.join(".omiga");
        std::fs::create_dir_all(&omiga).expect("mkdir .omiga");
        std::fs::write(
            omiga.join("mcp.json"),
            r#"{"mcpServers":{"__omiga_test_from_dot__":{"command":"npx","args":["a"]}}}"#,
        )
        .expect("write");
        let merged = merged_mcp_servers(proj);
        match merged.get("__omiga_test_from_dot__") {
            Some(McpServerConfig::Stdio { command, .. }) => assert_eq!(command, "npx"),
            _ => panic!("expected .omiga/mcp.json server"),
        }
    }
}
