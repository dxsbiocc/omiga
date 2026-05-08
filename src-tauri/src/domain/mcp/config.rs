//! Parse merged MCP server configs from Omiga-only locations (parity with TS merge intent):
//! app-bundled defaults → user `~/.omiga/mcp.json` → project `<project>/.omiga/mcp.json`.
//! Stdio uses JSON `command`/`args`/`env`; remote uses `url` (streamable HTTP).

use serde_json::Value as Json;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::Path;

/// One MCP server entry from `mcpServers.<name>`.
#[derive(Debug, Clone)]
pub enum McpServerConfig {
    /// Local process (JSON-RPC over stdio).
    Stdio {
        command: String,
        args: Vec<String>,
        env: HashMap<String, String>,
        cwd: Option<String>,
    },
    /// Streamable HTTP / SSE endpoint (rmcp `StreamableHttpClientTransport`).
    Url {
        url: String,
        headers: HashMap<String, String>,
    },
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

fn parse_headers(v: &Json) -> HashMap<String, String> {
    let mut headers = v
        .get("headers")
        .and_then(|e| e.as_object())
        .map(parse_env)
        .unwrap_or_default();

    if let Some(auth_headers) = v
        .get("auth")
        .and_then(|auth| auth.get("headers"))
        .and_then(|e| e.as_object())
        .map(parse_env)
    {
        headers.extend(auth_headers);
    }

    if !headers
        .keys()
        .any(|key| key.eq_ignore_ascii_case("authorization"))
    {
        if let Some(token) = v
            .get("bearerToken")
            .or_else(|| v.get("bearer_token"))
            .and_then(|t| t.as_str())
            .map(str::trim)
            .filter(|t| !t.is_empty())
        {
            headers.insert("Authorization".to_string(), format!("Bearer {token}"));
        }
    }

    headers
}

fn parse_server_entry(v: &Json) -> Option<McpServerConfig> {
    if is_disabled_server_entry(v) {
        return None;
    }

    if let Some(url) = v.get("url").and_then(|u| u.as_str()) {
        let u = url.trim();
        if !u.is_empty() {
            return Some(McpServerConfig::Url {
                url: u.to_string(),
                headers: parse_headers(v),
            });
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

    let cwd = v
        .get("cwd")
        .or_else(|| v.get("workingDirectory"))
        .and_then(|c| c.as_str())
        .map(str::trim)
        .filter(|c| !c.is_empty())
        .map(str::to_string);

    Some(McpServerConfig::Stdio {
        command,
        args,
        env,
        cwd,
    })
}

fn is_disabled_server_entry(v: &Json) -> bool {
    v.is_null()
        || v.get("disabled").and_then(|x| x.as_bool()) == Some(true)
        || v.get("enabled").and_then(|x| x.as_bool()) == Some(false)
}

pub(crate) fn servers_from_mcp_json(raw: &str) -> HashMap<String, McpServerConfig> {
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

fn apply_mcp_json_to_merged(merged: &mut HashMap<String, McpServerConfig>, raw: &str) {
    let Ok(v) = serde_json::from_str::<Json>(raw) else {
        return;
    };
    let Some(obj) = v.get("mcpServers").and_then(|x| x.as_object()) else {
        return;
    };
    for (name, cfg) in obj {
        if is_disabled_server_entry(cfg) {
            merged.remove(name);
        } else if let Some(parsed) = parse_server_entry(cfg) {
            merged.insert(name.clone(), parsed);
        }
    }
}

/// Merge MCP configs (later sources win on same server name):
/// app-bundled `bundled_mcp.json` → `~/.omiga/mcp.json` → `<project>/.omiga/mcp.json`.
pub fn merged_mcp_servers(project_root: &Path) -> HashMap<String, McpServerConfig> {
    let mut merged = HashMap::new();

    merged.extend(servers_from_bundled());

    if let Some(p) = user_omiga_mcp_path() {
        if let Some(raw) = read_if_exists(&p) {
            apply_mcp_json_to_merged(&mut merged, &raw);
        }
    }

    // Enabled Omiga plugins contribute MCP servers after user config and before
    // project config, so a project can still override a plugin-provided server.
    merged.extend(crate::domain::plugins::enabled_plugin_mcp_servers());

    let proj = project_root.join(".omiga").join("mcp.json");
    if let Some(raw) = read_if_exists(&proj) {
        apply_mcp_json_to_merged(&mut merged, &raw);
    }

    merged
}

/// Stable fingerprint for the currently effective MCP configuration.
///
/// This is used to invalidate cached `tools/list` schemas immediately when a
/// server is removed from any Omiga MCP source. Without this, a deleted server
/// can remain visible to the model until the tool-cache TTL expires.
pub fn merged_mcp_servers_signature(project_root: &Path) -> String {
    servers_signature(&merged_mcp_servers(project_root))
}

pub(crate) fn servers_signature(servers: &HashMap<String, McpServerConfig>) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    let mut names = servers.keys().collect::<Vec<_>>();
    names.sort();
    for name in names {
        name.hash(&mut hasher);
        match &servers[name] {
            McpServerConfig::Stdio {
                command,
                args,
                env,
                cwd,
            } => {
                "stdio".hash(&mut hasher);
                command.hash(&mut hasher);
                args.hash(&mut hasher);
                cwd.hash(&mut hasher);
                let mut env_keys = env.keys().collect::<Vec<_>>();
                env_keys.sort();
                for key in env_keys {
                    key.hash(&mut hasher);
                    env[key].hash(&mut hasher);
                }
            }
            McpServerConfig::Url { url, headers } => {
                "url".hash(&mut hasher);
                url.hash(&mut hasher);
                let mut header_keys = headers.keys().collect::<Vec<_>>();
                header_keys.sort();
                for key in header_keys {
                    key.hash(&mut hasher);
                    headers[key].hash(&mut hasher);
                }
            }
        }
    }
    format!("{:016x}", hasher.finish())
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
            Some(McpServerConfig::Url { url, headers }) if url == "http://localhost:8080/mcp" && headers.is_empty()
        ));
    }

    #[test]
    fn parses_http_headers_and_bearer_token() {
        let raw = r#"{"mcpServers":{"paperclip":{"url":"https://paperclip.gxl.ai/mcp","headers":{"X-Api-Key":"${PAPERCLIP_API_KEY}"},"bearerToken":"abc"}}}"#;
        let m = servers_from_mcp_json(raw);
        match m.get("paperclip") {
            Some(McpServerConfig::Url { headers, .. }) => {
                assert_eq!(
                    headers.get("X-Api-Key").map(String::as_str),
                    Some("${PAPERCLIP_API_KEY}")
                );
                assert_eq!(
                    headers.get("Authorization").map(String::as_str),
                    Some("Bearer abc")
                );
            }
            other => panic!("expected paperclip URL MCP server, got {other:?}"),
        }
    }

    #[test]
    fn parses_stdio_working_directory() {
        let raw = r#"{"mcpServers":{"a":{"command":"node","cwd":"./tools"}}}"#;
        let m = servers_from_mcp_json(raw);
        assert!(matches!(
            m.get("a"),
            Some(McpServerConfig::Stdio { cwd: Some(cwd), .. }) if cwd == "./tools"
        ));
    }

    #[test]
    fn disabled_project_entry_removes_earlier_server() {
        let mut merged = servers_from_mcp_json(
            r#"{"mcpServers":{"paperclip":{"url":"https://paperclip.gxl.ai/mcp"}}}"#,
        );
        apply_mcp_json_to_merged(
            &mut merged,
            r#"{"mcpServers":{"paperclip":{"disabled":true}}}"#,
        );
        assert!(!merged.contains_key("paperclip"));
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

    #[test]
    fn server_signature_changes_when_server_removed() {
        let one = servers_from_mcp_json(
            r#"{"mcpServers":{"pubmed":{"command":"node","args":["pubmed.js"]},"playwright":{"command":"npx","args":["-y","@playwright/mcp@latest"]}}}"#,
        );
        let two = servers_from_mcp_json(
            r#"{"mcpServers":{"playwright":{"command":"npx","args":["-y","@playwright/mcp@latest"]}}}"#,
        );

        assert_ne!(servers_signature(&one), servers_signature(&two));
    }
}
