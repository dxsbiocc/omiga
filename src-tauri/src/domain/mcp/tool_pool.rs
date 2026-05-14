//! Discover MCP `tools/list` entries and expose them as [`ToolSchema`] for the LLM (TS `assembleToolPool`).

use crate::domain::mcp::client::list_tools_for_server;
use crate::domain::mcp::config::merged_mcp_servers;
use crate::domain::mcp::names::{
    build_mcp_tool_name, is_reserved_computer_mcp_tool, normalize_name_for_mcp, parse_mcp_tool_name,
};
use crate::domain::tools::ToolSchema;
use futures::future::join_all;
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::Duration;

pub fn filter_mcp_tool_schemas_by_configured_servers(
    schemas: Vec<ToolSchema>,
    configured_server_names: &HashSet<String>,
) -> Vec<ToolSchema> {
    let configured: HashSet<String> = configured_server_names
        .iter()
        .map(|name| normalize_name_for_mcp(name))
        .collect();

    schemas
        .into_iter()
        .filter(|schema| {
            !is_reserved_computer_mcp_tool(&schema.name)
                && parse_mcp_tool_name(&schema.name)
                    .map(|(server, _)| configured.contains(&server))
                    .unwrap_or(false)
        })
        .collect()
}

pub fn filter_mcp_tool_schemas_for_current_config(
    project_root: &Path,
    schemas: Vec<ToolSchema>,
) -> Vec<ToolSchema> {
    let configured_server_names = merged_mcp_servers(project_root)
        .keys()
        .cloned()
        .collect::<HashSet<_>>();
    filter_mcp_tool_schemas_by_configured_servers(schemas, &configured_server_names)
}

/// List tools from every configured MCP server (parallel). Failures are logged; successful servers still contribute.
pub async fn discover_mcp_tool_schemas(project_root: &Path, timeout: Duration) -> Vec<ToolSchema> {
    let merged = merged_mcp_servers(project_root);
    if merged.is_empty() {
        return vec![];
    }
    let integrations_cfg =
        crate::domain::integrations_config::load_integrations_config(project_root);
    let mut names = enabled_mcp_server_names(&merged, &integrations_cfg);
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
                    if is_reserved_computer_mcp_tool(&fq) {
                        tracing::debug!(
                            tool = %fq,
                            "reserved Computer Use MCP backend tool hidden behind computer_* facade"
                        );
                        continue;
                    }
                    let desc = t.description.as_deref().unwrap_or("MCP tool").to_string();
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

fn enabled_mcp_server_names(
    merged: &HashMap<String, crate::domain::mcp::config::McpServerConfig>,
    integrations_cfg: &crate::domain::integrations_config::IntegrationsConfig,
) -> Vec<String> {
    merged
        .keys()
        .filter(|name| {
            !crate::domain::integrations_config::is_mcp_config_server_disabled(
                integrations_cfg,
                name,
            )
        })
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_cached_schemas_to_current_mcp_servers() {
        let schemas = vec![
            ToolSchema::new(
                "mcp__pubmed__pubmed_search_articles",
                "legacy PubMed",
                json!({"type":"object"}),
            ),
            ToolSchema::new(
                "mcp__playwright__browser_navigate",
                "Playwright",
                json!({"type":"object"}),
            ),
            ToolSchema::new("mcp__computer__click", "Computer", json!({"type":"object"})),
        ];
        let configured_server_names =
            HashSet::from(["playwright".to_string(), "computer".to_string()]);

        let filtered =
            filter_mcp_tool_schemas_by_configured_servers(schemas, &configured_server_names);
        let names = filtered
            .into_iter()
            .map(|schema| schema.name)
            .collect::<Vec<_>>();

        assert_eq!(names, vec!["mcp__playwright__browser_navigate"]);
    }

    #[test]
    fn discovery_skips_disabled_mcp_servers_before_connecting() {
        use crate::domain::integrations_config::IntegrationsConfig;
        use crate::domain::mcp::config::McpServerConfig;

        let merged = HashMap::from([
            (
                "paperclip".to_string(),
                McpServerConfig::Url {
                    url: "https://paperclip.gxl.ai/mcp".to_string(),
                    headers: Default::default(),
                },
            ),
            (
                "local".to_string(),
                McpServerConfig::Stdio {
                    command: "node".to_string(),
                    args: vec!["server.js".to_string()],
                    env: Default::default(),
                    cwd: None,
                },
            ),
        ]);
        let cfg = IntegrationsConfig {
            disabled_mcp_servers: vec!["paperclip".to_string()],
            disabled_skills: vec![],
        };

        let mut names = enabled_mcp_server_names(&merged, &cfg);
        names.sort();

        assert_eq!(names, vec!["local"]);
    }
}
