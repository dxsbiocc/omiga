//! Serializable MCP + skills catalog for the Settings UI and warm-cache storage.

use crate::domain::connectors::{ConnectorCatalog, ConnectorInfo};
use crate::domain::mcp::names::normalize_name_for_mcp;
use crate::domain::skills::SkillSource;
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct McpToolCatalogEntry {
    pub wire_name: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connector_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connector_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connector_description: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerConfigCatalogEntry {
    pub kind: String,
    pub command: Option<String>,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
    pub headers: HashMap<String, String>,
    pub url: Option<String>,
    pub cwd: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerCatalogEntry {
    pub config_key: String,
    pub normalized_key: String,
    pub enabled: bool,
    pub config: McpServerConfigCatalogEntry,
    /// Whether Settings has actively probed `tools/list` for this server in this UI session.
    pub tool_list_checked: bool,
    /// True when an OAuth credential for this HTTP endpoint exists in the secure secret store.
    /// The token value itself is never serialized to the UI or written to MCP JSON.
    pub oauth_authenticated: bool,
    /// When tool discovery failed (timeout, handshake error, etc.); UI can show error state.
    pub list_tools_error: Option<String>,
    pub tools: Vec<McpToolCatalogEntry>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillCatalogEntry {
    pub name: String,
    pub description: String,
    pub enabled: bool,
    /// Where the skill was loaded from (for UI labeling).
    pub source: SkillSource,
    /// Folder basename under the skills root (matches `remove_omiga_imported_skill`).
    pub directory_name: String,
    /// Absolute path to `SKILL.md` (for Settings preview).
    pub skill_md_path: String,
    /// From YAML frontmatter `tags` (search / display).
    pub tags: Vec<String>,
    /// Skill lives under `~/.omiga/skills` or `<project>/.omiga/skills` — safe to delete that folder.
    pub can_uninstall_omiga_copy: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IntegrationsCatalog {
    pub mcp_servers: Vec<McpServerCatalogEntry>,
    pub skills: Vec<SkillCatalogEntry>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExternalIntegrationKind {
    Connector,
    McpServer,
    McpBackedConnector,
    Skill,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExternalIntegrationMcpToolBinding {
    pub wire_name: String,
    pub connector_id: Option<String>,
    pub connector_name: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalIntegrationMcpServerBinding {
    pub server: McpServerCatalogEntry,
    #[serde(default)]
    pub tool_bindings: Vec<ExternalIntegrationMcpToolBinding>,
}

impl From<McpServerCatalogEntry> for ExternalIntegrationMcpServerBinding {
    fn from(server: McpServerCatalogEntry) -> Self {
        Self {
            server,
            tool_bindings: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalIntegrationsCatalog {
    pub items: Vec<ExternalIntegrationCatalogEntry>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalIntegrationCatalogEntry {
    pub id: String,
    pub kind: ExternalIntegrationKind,
    pub display_name: String,
    pub category: Option<String>,
    pub enabled: bool,
    pub connected: bool,
    pub authenticated: bool,
    pub accessible: bool,
    pub connector_id: Option<String>,
    pub normalized_mcp_key: Option<String>,
    pub mcp_server_keys: Vec<String>,
    pub auth_source: Option<String>,
    pub account_label: Option<String>,
    pub last_error: Option<String>,
    pub connector: Option<ConnectorInfo>,
    pub mcp_servers: Vec<McpServerCatalogEntry>,
    pub skill: Option<SkillCatalogEntry>,
}

pub fn external_integrations_from_connector_catalog(
    connector_catalog: &ConnectorCatalog,
) -> ExternalIntegrationsCatalog {
    ExternalIntegrationsCatalog {
        items: connector_catalog
            .connectors
            .iter()
            .map(external_integration_from_connector)
            .collect(),
    }
}

pub fn external_integrations_from_integrations_catalog(
    integrations_catalog: &IntegrationsCatalog,
) -> ExternalIntegrationsCatalog {
    let mut items: Vec<ExternalIntegrationCatalogEntry> = integrations_catalog
        .mcp_servers
        .iter()
        .map(external_integration_from_mcp_server)
        .collect();
    items.extend(
        integrations_catalog
            .skills
            .iter()
            .map(external_integration_from_skill),
    );
    ExternalIntegrationsCatalog { items }
}

pub fn merge_external_integrations(
    connector_catalog: &ConnectorCatalog,
    mcp_servers: &[ExternalIntegrationMcpServerBinding],
    skills: &[SkillCatalogEntry],
) -> ExternalIntegrationsCatalog {
    let mut items: Vec<ExternalIntegrationCatalogEntry> = connector_catalog
        .connectors
        .iter()
        .map(external_integration_from_connector)
        .collect();
    let connector_positions: HashMap<String, usize> = connector_catalog
        .connectors
        .iter()
        .enumerate()
        .map(|(index, connector)| (connector.definition.id.clone(), index))
        .collect();

    for binding in mcp_servers {
        merge_mcp_server_binding(
            &mut items,
            &connector_catalog.connectors,
            &connector_positions,
            binding,
        );
    }

    items.extend(skills.iter().map(external_integration_from_skill));
    ExternalIntegrationsCatalog { items }
}

pub fn merge_external_integrations_from_catalogs(
    connector_catalog: &ConnectorCatalog,
    integrations_catalog: &IntegrationsCatalog,
) -> ExternalIntegrationsCatalog {
    let mcp_servers: Vec<ExternalIntegrationMcpServerBinding> = integrations_catalog
        .mcp_servers
        .iter()
        .cloned()
        .map(ExternalIntegrationMcpServerBinding::from)
        .collect();
    merge_external_integrations(
        connector_catalog,
        &mcp_servers,
        &integrations_catalog.skills,
    )
}

fn external_integration_from_connector(
    connector: &ConnectorInfo,
) -> ExternalIntegrationCatalogEntry {
    ExternalIntegrationCatalogEntry {
        id: connector_item_id(&connector.definition.id),
        kind: ExternalIntegrationKind::Connector,
        display_name: connector.definition.name.clone(),
        category: Some(connector.definition.category.clone()),
        enabled: connector.enabled,
        connected: connector.connected,
        authenticated: connector.connected,
        accessible: connector.accessible,
        connector_id: Some(connector.definition.id.clone()),
        normalized_mcp_key: None,
        mcp_server_keys: Vec::new(),
        auth_source: connector.auth_source.clone(),
        account_label: connector.account_label.clone(),
        last_error: connector
            .last_connection_test
            .as_ref()
            .filter(|result| !result.ok)
            .map(|result| result.message.clone()),
        connector: Some(connector.clone()),
        mcp_servers: Vec::new(),
        skill: None,
    }
}

fn external_integration_from_mcp_server(
    server: &McpServerCatalogEntry,
) -> ExternalIntegrationCatalogEntry {
    external_integration_from_mcp_server_with_tools(server, server.tools.clone())
}

fn external_integration_from_mcp_server_with_tools(
    server: &McpServerCatalogEntry,
    tools: Vec<McpToolCatalogEntry>,
) -> ExternalIntegrationCatalogEntry {
    ExternalIntegrationCatalogEntry {
        id: mcp_server_item_id(&server.normalized_key),
        kind: ExternalIntegrationKind::McpServer,
        display_name: server.config_key.clone(),
        category: Some("mcp_server".to_string()),
        enabled: server.enabled,
        connected: server.tool_list_checked && server.list_tools_error.is_none(),
        authenticated: server.oauth_authenticated,
        accessible: server.tool_list_checked && server.list_tools_error.is_none(),
        connector_id: None,
        normalized_mcp_key: Some(server.normalized_key.clone()),
        mcp_server_keys: vec![server.config_key.clone()],
        auth_source: None,
        account_label: None,
        last_error: server.list_tools_error.clone(),
        connector: None,
        mcp_servers: vec![clone_mcp_server_with_tools(server, tools)],
        skill: None,
    }
}

fn external_integration_from_skill(skill: &SkillCatalogEntry) -> ExternalIntegrationCatalogEntry {
    ExternalIntegrationCatalogEntry {
        id: skill_item_id(&skill.directory_name),
        kind: ExternalIntegrationKind::Skill,
        display_name: skill.name.clone(),
        category: Some("skill".to_string()),
        enabled: skill.enabled,
        connected: false,
        authenticated: false,
        accessible: skill.enabled,
        connector_id: None,
        normalized_mcp_key: None,
        mcp_server_keys: Vec::new(),
        auth_source: None,
        account_label: None,
        last_error: None,
        connector: None,
        mcp_servers: Vec::new(),
        skill: Some(skill.clone()),
    }
}

fn merge_mcp_server_binding(
    items: &mut Vec<ExternalIntegrationCatalogEntry>,
    connectors: &[ConnectorInfo],
    connector_positions: &HashMap<String, usize>,
    binding: &ExternalIntegrationMcpServerBinding,
) {
    let binding_lookup: HashMap<&str, &ExternalIntegrationMcpToolBinding> = binding
        .tool_bindings
        .iter()
        .map(|tool_binding| (tool_binding.wire_name.as_str(), tool_binding))
        .collect();
    let mut matched_tools_by_connector: BTreeMap<String, Vec<McpToolCatalogEntry>> =
        BTreeMap::new();
    let mut unmatched_tools = Vec::new();

    for tool in &binding.server.tools {
        let Some(tool_binding) = binding_lookup.get(tool.wire_name.as_str()) else {
            unmatched_tools.push(tool.clone());
            continue;
        };
        let Some(connector) = resolve_connector_binding(connectors, tool_binding) else {
            unmatched_tools.push(tool.clone());
            continue;
        };
        matched_tools_by_connector
            .entry(connector.definition.id.clone())
            .or_default()
            .push(tool.clone());
    }

    let matched_tool_count: usize = matched_tools_by_connector.values().map(Vec::len).sum();
    for (connector_id, tools) in matched_tools_by_connector {
        let Some(index) = connector_positions.get(&connector_id).copied() else {
            continue;
        };
        attach_mcp_server_to_connector_entry(&mut items[index], &binding.server, tools);
    }

    if should_keep_standalone_mcp_server(&binding.server, matched_tool_count, unmatched_tools.len())
    {
        items.push(external_integration_from_mcp_server_with_tools(
            &binding.server,
            unmatched_tools,
        ));
    }
}

fn attach_mcp_server_to_connector_entry(
    entry: &mut ExternalIntegrationCatalogEntry,
    server: &McpServerCatalogEntry,
    tools: Vec<McpToolCatalogEntry>,
) {
    entry.kind = ExternalIntegrationKind::McpBackedConnector;
    if !entry
        .mcp_server_keys
        .iter()
        .any(|key| key == &server.config_key)
    {
        entry.mcp_server_keys.push(server.config_key.clone());
    }
    match entry.normalized_mcp_key.as_deref() {
        None => entry.normalized_mcp_key = Some(server.normalized_key.clone()),
        Some(current) if current != server.normalized_key => entry.normalized_mcp_key = None,
        Some(_) => {}
    }
    entry.connected |= server.tool_list_checked && server.list_tools_error.is_none();
    entry.authenticated |= server.oauth_authenticated;
    entry.accessible |= server.list_tools_error.is_none() && !tools.is_empty();
    if entry.last_error.is_none() {
        entry.last_error = server.list_tools_error.clone();
    }
    entry
        .mcp_servers
        .push(clone_mcp_server_with_tools(server, tools));
}

fn should_keep_standalone_mcp_server(
    server: &McpServerCatalogEntry,
    matched_tool_count: usize,
    unmatched_tool_count: usize,
) -> bool {
    matched_tool_count == 0 || unmatched_tool_count > 0 || server.list_tools_error.is_some()
}

fn resolve_connector_binding<'a>(
    connectors: &'a [ConnectorInfo],
    tool_binding: &ExternalIntegrationMcpToolBinding,
) -> Option<&'a ConnectorInfo> {
    tool_binding
        .connector_id
        .as_deref()
        .and_then(|value| find_matching_connector(connectors, value))
        .or_else(|| {
            tool_binding
                .connector_name
                .as_deref()
                .and_then(|value| find_matching_connector(connectors, value))
        })
}

fn find_matching_connector<'a>(
    connectors: &'a [ConnectorInfo],
    value: &str,
) -> Option<&'a ConnectorInfo> {
    connectors
        .iter()
        .find(|connector| connector_identity_matches(connector, value))
}

fn connector_identity_matches(connector: &ConnectorInfo, value: &str) -> bool {
    let candidate = connector_match_key(value);
    if candidate.is_empty() {
        return false;
    }
    let id_key = connector_match_key(&connector.definition.id);
    let name_key = connector_match_key(&connector.definition.name);
    candidate == id_key || candidate == name_key
}

fn connector_match_key(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn clone_mcp_server_with_tools(
    server: &McpServerCatalogEntry,
    tools: Vec<McpToolCatalogEntry>,
) -> McpServerCatalogEntry {
    let mut server = server.clone();
    server.tools = tools;
    server
}

fn connector_item_id(connector_id: &str) -> String {
    format!("connector:{connector_id}")
}

fn mcp_server_item_id(normalized_key: &str) -> String {
    format!("mcp:{}", normalize_name_for_mcp(normalized_key))
}

fn skill_item_id(directory_name: &str) -> String {
    format!("skill:{directory_name}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::connectors::{
        ConnectorAuthType, ConnectorConnectionStatus, ConnectorDefinition,
        ConnectorDefinitionSource, ConnectorHealthSummary,
    };

    fn sample_connector(id: &str, name: &str) -> ConnectorInfo {
        ConnectorInfo {
            definition: ConnectorDefinition {
                id: id.to_string(),
                name: name.to_string(),
                description: format!("{name} connector"),
                category: "communication".to_string(),
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
            auth_source: None,
            connected_at: None,
            env_configured: false,
            referenced_by_plugins: Vec::new(),
            source: ConnectorDefinitionSource::BuiltIn,
            last_connection_test: None,
            connection_test_history: Vec::new(),
            connection_health: ConnectorHealthSummary::default(),
        }
    }

    fn sample_mcp_server(key: &str, tools: Vec<McpToolCatalogEntry>) -> McpServerCatalogEntry {
        McpServerCatalogEntry {
            config_key: key.to_string(),
            normalized_key: normalize_name_for_mcp(key),
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
            oauth_authenticated: false,
            list_tools_error: None,
            tools,
        }
    }

    fn sample_tool(name: &str) -> McpToolCatalogEntry {
        McpToolCatalogEntry {
            wire_name: name.to_string(),
            description: format!("{name} tool"),
            connector_id: None,
            connector_name: None,
            connector_description: None,
        }
    }

    #[test]
    fn merge_external_integrations_deduplicates_mcp_backed_connector() {
        let connector = sample_connector("slack", "Slack");
        let server = sample_mcp_server(
            "codex_apps",
            vec![sample_tool("mcp__codex_apps__read_thread")],
        );
        let merged = merge_external_integrations(
            &ConnectorCatalog {
                connectors: vec![connector],
                scope: "user".to_string(),
                config_path: "/tmp/connectors.json".to_string(),
                notes: Vec::new(),
            },
            &[ExternalIntegrationMcpServerBinding {
                server: server.clone(),
                tool_bindings: vec![ExternalIntegrationMcpToolBinding {
                    wire_name: "mcp__codex_apps__read_thread".to_string(),
                    connector_id: Some("slack".to_string()),
                    connector_name: None,
                }],
            }],
            &[],
        );

        assert_eq!(merged.items.len(), 1);
        assert_eq!(
            merged.items[0].kind,
            ExternalIntegrationKind::McpBackedConnector
        );
        assert_eq!(merged.items[0].connector_id.as_deref(), Some("slack"));
        assert_eq!(merged.items[0].mcp_server_keys, vec!["codex_apps"]);
        assert_eq!(merged.items[0].mcp_servers.len(), 1);
        assert_eq!(
            merged.items[0].mcp_servers[0].tools,
            vec![sample_tool("mcp__codex_apps__read_thread")]
        );
    }

    #[test]
    fn merge_external_integrations_keeps_standalone_mcp_server_for_unmatched_tools() {
        let connector = sample_connector("slack", "Slack");
        let server = sample_mcp_server(
            "codex_apps",
            vec![
                sample_tool("mcp__codex_apps__read_thread"),
                sample_tool("mcp__codex_apps__search_workspace"),
            ],
        );
        let merged = merge_external_integrations(
            &ConnectorCatalog {
                connectors: vec![connector],
                scope: "user".to_string(),
                config_path: "/tmp/connectors.json".to_string(),
                notes: Vec::new(),
            },
            &[ExternalIntegrationMcpServerBinding {
                server,
                tool_bindings: vec![ExternalIntegrationMcpToolBinding {
                    wire_name: "mcp__codex_apps__read_thread".to_string(),
                    connector_id: None,
                    connector_name: Some("Slack".to_string()),
                }],
            }],
            &[],
        );

        assert_eq!(merged.items.len(), 2);
        assert_eq!(
            merged.items[0].kind,
            ExternalIntegrationKind::McpBackedConnector
        );
        assert_eq!(merged.items[0].mcp_servers.len(), 1);
        assert_eq!(
            merged.items[0].mcp_servers[0].tools,
            vec![sample_tool("mcp__codex_apps__read_thread")]
        );
        assert_eq!(merged.items[1].kind, ExternalIntegrationKind::McpServer);
        assert_eq!(
            merged.items[1].mcp_servers[0].tools,
            vec![sample_tool("mcp__codex_apps__search_workspace")]
        );
    }

    #[test]
    fn merge_external_integrations_matches_connector_binding_exactly() {
        let github = sample_connector("github", "GitHub");
        let github_enterprise = sample_connector("github-enterprise", "GitHub Enterprise");
        let server = sample_mcp_server(
            "codex_apps",
            vec![sample_tool("mcp__codex_apps__read_enterprise_issue")],
        );

        let merged = merge_external_integrations(
            &ConnectorCatalog {
                connectors: vec![github, github_enterprise],
                scope: "user".to_string(),
                config_path: "/tmp/connectors.json".to_string(),
                notes: Vec::new(),
            },
            &[ExternalIntegrationMcpServerBinding {
                server,
                tool_bindings: vec![ExternalIntegrationMcpToolBinding {
                    wire_name: "mcp__codex_apps__read_enterprise_issue".to_string(),
                    connector_id: Some("github-enterprise".to_string()),
                    connector_name: None,
                }],
            }],
            &[],
        );

        assert_eq!(merged.items.len(), 2);
        assert_eq!(merged.items[0].connector_id.as_deref(), Some("github"));
        assert_eq!(merged.items[0].kind, ExternalIntegrationKind::Connector);
        assert_eq!(
            merged.items[1].connector_id.as_deref(),
            Some("github-enterprise")
        );
        assert_eq!(
            merged.items[1].kind,
            ExternalIntegrationKind::McpBackedConnector
        );
        assert_eq!(merged.items[1].mcp_server_keys, vec!["codex_apps"]);
    }

    #[test]
    fn merge_external_integrations_clears_normalized_mcp_key_when_multiple_servers_attach() {
        let connector = sample_connector("slack", "Slack");
        let codex_apps = sample_mcp_server(
            "codex_apps",
            vec![sample_tool("mcp__codex_apps__read_thread")],
        );
        let mut slack_remote = sample_mcp_server(
            "slack_remote",
            vec![sample_tool("mcp__slack_remote__post_message")],
        );
        slack_remote.oauth_authenticated = true;

        let merged = merge_external_integrations(
            &ConnectorCatalog {
                connectors: vec![connector],
                scope: "user".to_string(),
                config_path: "/tmp/connectors.json".to_string(),
                notes: Vec::new(),
            },
            &[
                ExternalIntegrationMcpServerBinding {
                    server: codex_apps,
                    tool_bindings: vec![ExternalIntegrationMcpToolBinding {
                        wire_name: "mcp__codex_apps__read_thread".to_string(),
                        connector_id: Some("slack".to_string()),
                        connector_name: None,
                    }],
                },
                ExternalIntegrationMcpServerBinding {
                    server: slack_remote,
                    tool_bindings: vec![ExternalIntegrationMcpToolBinding {
                        wire_name: "mcp__slack_remote__post_message".to_string(),
                        connector_id: Some("slack".to_string()),
                        connector_name: None,
                    }],
                },
            ],
            &[],
        );

        assert_eq!(merged.items.len(), 1);
        let item = &merged.items[0];
        assert_eq!(item.kind, ExternalIntegrationKind::McpBackedConnector);
        assert_eq!(item.mcp_server_keys, vec!["codex_apps", "slack_remote"]);
        assert_eq!(item.normalized_mcp_key, None);
        assert_eq!(item.mcp_servers.len(), 2);
        assert!(item.connected);
        assert!(item.authenticated);
        assert!(item.accessible);
    }

    #[test]
    fn merge_external_integrations_from_catalogs_keeps_plain_mcp_and_skill_entries() {
        let connector_catalog = ConnectorCatalog {
            connectors: vec![sample_connector("github", "GitHub")],
            scope: "user".to_string(),
            config_path: "/tmp/connectors.json".to_string(),
            notes: Vec::new(),
        };
        let integrations_catalog = IntegrationsCatalog {
            mcp_servers: vec![sample_mcp_server(
                "filesystem",
                vec![sample_tool("mcp__filesystem__read_file")],
            )],
            skills: vec![SkillCatalogEntry {
                name: "browser".to_string(),
                description: "Browser skill".to_string(),
                enabled: true,
                source: SkillSource::OmigaPlugin,
                directory_name: "browser".to_string(),
                skill_md_path: "/tmp/browser/SKILL.md".to_string(),
                tags: vec!["web".to_string()],
                can_uninstall_omiga_copy: false,
            }],
        };

        let merged =
            merge_external_integrations_from_catalogs(&connector_catalog, &integrations_catalog);

        assert_eq!(merged.items.len(), 3);
        assert_eq!(merged.items[0].kind, ExternalIntegrationKind::Connector);
        assert_eq!(merged.items[1].kind, ExternalIntegrationKind::McpServer);
        assert_eq!(merged.items[2].kind, ExternalIntegrationKind::Skill);
    }
}

#[cfg(test)]
mod serialization_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn serializes_integrations_catalog_with_camel_case_keys() {
        let catalog = IntegrationsCatalog {
            mcp_servers: vec![McpServerCatalogEntry {
                config_key: "paperclip".to_string(),
                normalized_key: "paperclip".to_string(),
                enabled: true,
                config: McpServerConfigCatalogEntry {
                    kind: "http".to_string(),
                    command: None,
                    args: Vec::new(),
                    env: HashMap::new(),
                    headers: HashMap::from([(
                        "Authorization".to_string(),
                        "Bearer ${PAPERCLIP_TOKEN}".to_string(),
                    )]),
                    url: Some("https://paperclip.example/mcp".to_string()),
                    cwd: None,
                },
                tool_list_checked: true,
                oauth_authenticated: true,
                list_tools_error: None,
                tools: vec![McpToolCatalogEntry {
                    wire_name: "mcp__paperclip__search".to_string(),
                    description: "Search Paperclip".to_string(),
                    connector_id: None,
                    connector_name: None,
                    connector_description: None,
                }],
            }],
            skills: vec![SkillCatalogEntry {
                name: "researcher".to_string(),
                description: "Research workflow".to_string(),
                enabled: true,
                source: SkillSource::OmigaProject,
                directory_name: "researcher".to_string(),
                skill_md_path: "/tmp/researcher/SKILL.md".to_string(),
                tags: vec!["research".to_string()],
                can_uninstall_omiga_copy: true,
            }],
        };

        let value = serde_json::to_value(&catalog).expect("catalog should serialize");

        assert_eq!(
            value,
            json!({
                "mcpServers": [
                    {
                        "configKey": "paperclip",
                        "normalizedKey": "paperclip",
                        "enabled": true,
                        "config": {
                            "kind": "http",
                            "command": null,
                            "args": [],
                            "env": {},
                            "headers": {
                                "Authorization": "Bearer ${PAPERCLIP_TOKEN}"
                            },
                            "url": "https://paperclip.example/mcp",
                            "cwd": null
                        },
                        "toolListChecked": true,
                        "oauthAuthenticated": true,
                        "listToolsError": null,
                        "tools": [
                            {
                                "wireName": "mcp__paperclip__search",
                                "description": "Search Paperclip"
                            }
                        ]
                    }
                ],
                "skills": [
                    {
                        "name": "researcher",
                        "description": "Research workflow",
                        "enabled": true,
                        "source": "omigaProject",
                        "directoryName": "researcher",
                        "skillMdPath": "/tmp/researcher/SKILL.md",
                        "tags": ["research"],
                        "canUninstallOmigaCopy": true
                    }
                ]
            })
        );
    }

    #[test]
    fn serializes_external_integrations_catalog_with_ui_field_names() {
        let server = McpServerCatalogEntry {
            config_key: "codex_apps".to_string(),
            normalized_key: "codex_apps".to_string(),
            enabled: true,
            config: McpServerConfigCatalogEntry {
                kind: "http".to_string(),
                command: None,
                args: Vec::new(),
                env: HashMap::new(),
                headers: HashMap::new(),
                url: Some("https://apps.example/mcp".to_string()),
                cwd: None,
            },
            tool_list_checked: true,
            oauth_authenticated: true,
            list_tools_error: None,
            tools: vec![McpToolCatalogEntry {
                wire_name: "mcp__codex_apps__read_thread".to_string(),
                description: "Read thread".to_string(),
                connector_id: Some("slack".to_string()),
                connector_name: Some("Slack".to_string()),
                connector_description: None,
            }],
        };
        let catalog = ExternalIntegrationsCatalog {
            items: vec![ExternalIntegrationCatalogEntry {
                id: "connector:slack".to_string(),
                kind: ExternalIntegrationKind::McpBackedConnector,
                display_name: "Slack".to_string(),
                category: Some("communication".to_string()),
                enabled: true,
                connected: true,
                authenticated: true,
                accessible: true,
                connector_id: Some("slack".to_string()),
                normalized_mcp_key: Some("codex_apps".to_string()),
                mcp_server_keys: vec!["codex_apps".to_string()],
                auth_source: Some("codex_apps".to_string()),
                account_label: Some("Slack workspace".to_string()),
                last_error: None,
                connector: None,
                mcp_servers: vec![server],
                skill: None,
            }],
        };

        let value = serde_json::to_value(&catalog).expect("catalog should serialize");
        let item = &value["items"][0];

        assert_eq!(item["kind"], json!("mcp_backed_connector"));
        assert!(item.get("displayName").is_some());
        assert!(item.get("connectorId").is_some());
        assert!(item.get("normalizedMcpKey").is_some());
        assert!(item.get("mcpServerKeys").is_some());
        assert!(item.get("authSource").is_some());
        assert!(item.get("accountLabel").is_some());
        assert!(item.get("lastError").is_some());
        assert!(item.get("mcpServers").is_some());
        let nested_server = &item["mcpServers"][0];
        assert!(nested_server.get("configKey").is_some());
        assert!(nested_server.get("normalizedKey").is_some());
        assert!(nested_server.get("toolListChecked").is_some());
        assert!(nested_server.get("oauthAuthenticated").is_some());
        assert!(nested_server.get("listToolsError").is_some());
    }

    #[test]
    fn serializes_skill_source_variants_with_ui_facing_names() {
        assert_eq!(
            serde_json::to_value(SkillSource::ClaudeUser).unwrap(),
            json!("claudeUser")
        );
        assert_eq!(
            serde_json::to_value(SkillSource::OmigaUser).unwrap(),
            json!("omigaUser")
        );
        assert_eq!(
            serde_json::to_value(SkillSource::OmigaProject).unwrap(),
            json!("omigaProject")
        );
        assert_eq!(
            serde_json::to_value(SkillSource::OmigaPlugin).unwrap(),
            json!("omigaPlugin")
        );
    }
}
