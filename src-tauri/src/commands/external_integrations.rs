//! Tauri commands for the unified external integrations catalog.

use crate::app_state::OmigaAppState;
use crate::commands::integrations_settings::{build_integrations_catalog, resolve_project_root};
use crate::commands::CommandResult;
use crate::domain::connectors::{ConnectorCatalog, ConnectorInfo};
use crate::domain::integrations_catalog::{
    merge_external_integrations, ExternalIntegrationMcpServerBinding,
    ExternalIntegrationMcpToolBinding, ExternalIntegrationsCatalog, IntegrationsCatalog,
    McpToolCatalogEntry,
};
use tauri::State;

fn connector_is_mcp_app_backed(connector: &ConnectorInfo) -> bool {
    connector
        .auth_source
        .as_deref()
        .is_some_and(|source| matches!(source, "codex_apps" | "mcp_app"))
}

fn tool_binding_from_catalog_metadata(
    tool: &McpToolCatalogEntry,
) -> Option<ExternalIntegrationMcpToolBinding> {
    if tool.connector_id.is_none() && tool.connector_name.is_none() {
        return None;
    }

    Some(ExternalIntegrationMcpToolBinding {
        wire_name: tool.wire_name.clone(),
        connector_id: tool.connector_id.clone(),
        connector_name: tool.connector_name.clone(),
    })
}

fn mcp_server_bindings_for_external_catalog(
    connector_catalog: &ConnectorCatalog,
    integrations_catalog: &IntegrationsCatalog,
) -> Vec<ExternalIntegrationMcpServerBinding> {
    let mcp_backed_connectors = connector_catalog
        .connectors
        .iter()
        .filter(|connector| connector_is_mcp_app_backed(connector))
        .collect::<Vec<_>>();

    integrations_catalog
        .mcp_servers
        .iter()
        .cloned()
        .map(|server| {
            let is_codex_apps = server.normalized_key == "codex_apps";
            let metadata_tool_bindings = server
                .tools
                .iter()
                .filter_map(tool_binding_from_catalog_metadata)
                .collect::<Vec<_>>();
            if !metadata_tool_bindings.is_empty() {
                ExternalIntegrationMcpServerBinding {
                    server,
                    tool_bindings: metadata_tool_bindings,
                }
            } else if is_codex_apps && mcp_backed_connectors.len() == 1 {
                let connector_id = mcp_backed_connectors[0].definition.id.clone();
                let tool_bindings = server
                    .tools
                    .iter()
                    .map(|tool| ExternalIntegrationMcpToolBinding {
                        wire_name: tool.wire_name.clone(),
                        connector_id: Some(connector_id.clone()),
                        connector_name: None,
                    })
                    .collect();
                ExternalIntegrationMcpServerBinding {
                    server,
                    tool_bindings,
                }
            } else {
                ExternalIntegrationMcpServerBinding::from(server)
            }
        })
        .collect()
}

#[tauri::command]
pub async fn get_external_integrations_catalog(
    app_state: State<'_, OmigaAppState>,
    project_root: String,
    _ignore_cache: Option<bool>,
    probe_tools: Option<bool>,
) -> CommandResult<ExternalIntegrationsCatalog> {
    let root = resolve_project_root(&project_root)?;
    let connector_catalog = crate::domain::connectors::list_connector_catalog();
    let integrations_catalog =
        build_integrations_catalog(&app_state, root, probe_tools.unwrap_or(false)).await?;
    let mcp_servers =
        mcp_server_bindings_for_external_catalog(&connector_catalog, &integrations_catalog);
    Ok(merge_external_integrations(
        &connector_catalog,
        &mcp_servers,
        &integrations_catalog.skills,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::connectors::{
        ConnectorAuthType, ConnectorConnectionStatus, ConnectorDefinition,
        ConnectorDefinitionSource, ConnectorHealthSummary,
    };
    use crate::domain::integrations_catalog::{
        McpServerCatalogEntry, McpServerConfigCatalogEntry, McpToolCatalogEntry,
    };
    use std::collections::HashMap;

    fn sample_connector(id: &str, auth_source: Option<&str>) -> ConnectorInfo {
        let name = match id {
            "github" => "GitHub",
            "slack" => "Slack",
            other => other,
        };
        ConnectorInfo {
            definition: ConnectorDefinition {
                id: id.to_string(),
                name: name.to_string(),
                description: format!("{name} connector"),
                category: "code".to_string(),
                auth_type: ConnectorAuthType::ExternalMcp,
                env_vars: Vec::new(),
                install_url: None,
                docs_url: None,
                default_enabled: true,
                tools: Vec::new(),
            },
            enabled: true,
            connected: false,
            accessible: false,
            status: ConnectorConnectionStatus::MetadataOnly,
            account_label: None,
            auth_source: auth_source.map(str::to_string),
            connected_at: None,
            env_configured: false,
            referenced_by_plugins: Vec::new(),
            source: ConnectorDefinitionSource::BuiltIn,
            last_connection_test: None,
            connection_test_history: Vec::new(),
            connection_health: ConnectorHealthSummary::default(),
        }
    }

    fn sample_tool(
        wire_name: &str,
        connector_id: Option<&str>,
        connector_name: Option<&str>,
    ) -> McpToolCatalogEntry {
        McpToolCatalogEntry {
            wire_name: wire_name.to_string(),
            description: "Read thread".to_string(),
            connector_id: connector_id.map(str::to_string),
            connector_name: connector_name.map(str::to_string),
            connector_description: None,
        }
    }

    fn sample_integrations_catalog(tools: Vec<McpToolCatalogEntry>) -> IntegrationsCatalog {
        IntegrationsCatalog {
            mcp_servers: vec![McpServerCatalogEntry {
                config_key: "codex_apps".to_string(),
                normalized_key: "codex_apps".to_string(),
                enabled: true,
                config: McpServerConfigCatalogEntry {
                    kind: "http".to_string(),
                    command: None,
                    args: Vec::new(),
                    env: HashMap::new(),
                    headers: HashMap::new(),
                    url: Some("https://example.test/mcp".to_string()),
                    cwd: None,
                },
                tool_list_checked: true,
                oauth_authenticated: true,
                list_tools_error: None,
                tools,
            }],
            skills: Vec::new(),
        }
    }

    #[test]
    fn codex_apps_fallback_binds_only_one_mcp_app_backed_connector() {
        let connector_catalog = ConnectorCatalog {
            connectors: vec![sample_connector("github", Some("codex_apps"))],
            scope: "user".to_string(),
            config_path: "/tmp/connectors.json".to_string(),
            notes: Vec::new(),
        };

        let catalog = sample_integrations_catalog(vec![sample_tool(
            "mcp__codex_apps__read_thread",
            None,
            None,
        )]);
        let bindings = mcp_server_bindings_for_external_catalog(&connector_catalog, &catalog);

        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0].server.config_key, "codex_apps");
        assert_eq!(bindings[0].tool_bindings.len(), 1);
        assert_eq!(
            bindings[0].tool_bindings[0].connector_id.as_deref(),
            Some("github")
        );

        let connector_catalog = ConnectorCatalog {
            connectors: vec![sample_connector("github", None)],
            scope: "user".to_string(),
            config_path: "/tmp/connectors.json".to_string(),
            notes: Vec::new(),
        };
        let bindings = mcp_server_bindings_for_external_catalog(&connector_catalog, &catalog);
        assert!(bindings[0].tool_bindings.is_empty());
    }

    #[test]
    fn codex_apps_fallback_does_not_guess_when_multiple_mcp_app_connectors_exist() {
        let connector_catalog = ConnectorCatalog {
            connectors: vec![
                sample_connector("github", Some("codex_apps")),
                sample_connector("slack", Some("codex_apps")),
            ],
            scope: "user".to_string(),
            config_path: "/tmp/connectors.json".to_string(),
            notes: Vec::new(),
        };

        let catalog = sample_integrations_catalog(vec![sample_tool(
            "mcp__codex_apps__read_thread",
            None,
            None,
        )]);
        let bindings = mcp_server_bindings_for_external_catalog(&connector_catalog, &catalog);

        assert_eq!(bindings.len(), 1);
        assert!(bindings[0].tool_bindings.is_empty());
    }

    #[test]
    fn mcp_metadata_binds_codex_apps_tools_to_multiple_connectors() {
        let connector_catalog = ConnectorCatalog {
            connectors: vec![
                sample_connector("github", Some("codex_apps")),
                sample_connector("slack", Some("codex_apps")),
            ],
            scope: "user".to_string(),
            config_path: "/tmp/connectors.json".to_string(),
            notes: Vec::new(),
        };
        let catalog = sample_integrations_catalog(vec![
            sample_tool("mcp__codex_apps__read_pull_request", Some("github"), None),
            sample_tool("mcp__codex_apps__read_thread", None, Some("Slack")),
        ]);

        let bindings = mcp_server_bindings_for_external_catalog(&connector_catalog, &catalog);

        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0].tool_bindings.len(), 2);
        assert_eq!(
            bindings[0].tool_bindings[0].connector_id.as_deref(),
            Some("github")
        );
        assert_eq!(
            bindings[0].tool_bindings[1].connector_name.as_deref(),
            Some("Slack")
        );
    }
}
