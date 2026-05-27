//! Omiga native plugin discovery, marketplace, installation, and runtime contribution loading.
//!
//! A plugin is an Omiga-native extension bundle: skills, MCP server configs, app connector
//! references, and UI metadata. It intentionally does not execute VS Code extension code.

use crate::domain::environments::{
    check_environment_profile, discover_environment_manifest_paths, environment_summary,
    load_environment_manifest, EnvironmentCheckResult, EnvironmentProfileSummary,
};
#[cfg(test)]
use crate::domain::environments::{
    EnvironmentDiagnostics, EnvironmentRequirements, EnvironmentRuntimeProfile,
};
use crate::domain::mcp::config::{servers_from_mcp_json, McpServerConfig};
use crate::domain::operators::OperatorCandidateSummary;
use crate::domain::plugin_runtime::retrieval::lifecycle::{
    PluginLifecycleKey, PluginLifecycleRouteStatus, PluginLifecycleState,
};
#[cfg(test)]
use crate::domain::plugin_runtime::retrieval::manifest::PluginRetrievalRuntime;
use crate::domain::plugin_runtime::retrieval::manifest::{
    load_plugin_retrieval_manifest, PluginRetrievalManifest, PluginRetrievalResource,
};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value as JsonValue;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::time::Duration;
use tokio::time::Instant;

pub const PLUGIN_MANIFEST_FILE: &str = "plugin.json";
pub const OMIGA_PLUGIN_MANIFEST_PATH: &str = ".omiga-plugin/plugin.json";
pub const CODEX_PLUGIN_MANIFEST_PATH: &str = ".codex-plugin/plugin.json";
const MARKETPLACE_FILE_NAME: &str = "marketplace.json";
const BUILTIN_GIT_URL: &str = "https://github.com/dxsbiocc/omiga-plugins.git";
const USER_PLUGINS_CONFIG_FILE: &str = "plugins/config.json";
const PLUGINS_CACHE_DIR: &str = "plugins/cache";
const PLUGINS_ROOT_DIR: &str = "plugins";
const RESOURCE_RUNNERS_DIR: &str = "resource_runners";
const LEGACY_SOURCE_RUNNERS_DIR: &str = "source_runners";
const RESOURCE_UTILS_DIR: &str = "utils";
const DEFAULT_PLUGIN_VERSION: &str = "local";
const PLUGIN_INSTALL_STATE_RELATIVE_PATH: &str = ".omiga-plugin/install-state.json";
const PLUGIN_SYNC_CONFLICTS_RELATIVE_DIR: &str = ".omiga-plugin/sync-conflicts";
const MAX_CHANGELOG_BYTES: usize = 128 * 1024;
const MAX_REMOTE_MARKETPLACE_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PluginKind {
    Operator,
    Resource,
    Workflow,
    Tool,
    Other,
}

impl PluginKind {
    const ALL: [Self; 5] = [
        Self::Operator,
        Self::Resource,
        Self::Workflow,
        Self::Tool,
        Self::Other,
    ];

    fn dir_name(self) -> &'static str {
        match self {
            Self::Operator => "operators",
            Self::Resource => "resources",
            Self::Workflow => "workflow",
            Self::Tool => "tools",
            Self::Other => "other",
        }
    }
}
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginInterface {
    pub display_name: Option<String>,
    pub short_description: Option<String>,
    pub long_description: Option<String>,
    pub developer_name: Option<String>,
    pub category: Option<String>,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default, alias = "websiteURL")]
    pub website_url: Option<String>,
    #[serde(default, alias = "privacyPolicyURL")]
    pub privacy_policy_url: Option<String>,
    #[serde(default, alias = "termsOfServiceURL")]
    pub terms_of_service_url: Option<String>,
    #[serde(default)]
    pub default_prompt: Vec<String>,
    pub brand_color: Option<String>,
    pub composer_icon: Option<PathBuf>,
    pub logo: Option<PathBuf>,
    #[serde(default)]
    pub screenshots: Vec<PathBuf>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginManifest {
    pub name: String,
    pub version: Option<String>,
    pub description: Option<String>,
    pub operators: Option<PathBuf>,
    pub templates: Option<PathBuf>,
    pub skills: Option<PathBuf>,
    pub agents: Option<PathBuf>,
    pub environments: Option<PathBuf>,
    pub mcp_servers: Option<PathBuf>,
    pub apps: Option<PathBuf>,
    pub hooks: Option<PathBuf>,
    pub changelog: Option<PathBuf>,
    pub retrieval: Option<PluginRetrievalManifest>,
    pub interface: Option<PluginInterface>,
    pub compatibility: PluginCompatibility,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginCompatibility {
    #[serde(default, alias = "supersededPlugins")]
    pub supersedes_plugins: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawPluginManifest {
    #[serde(default)]
    name: String,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    operators: Option<String>,
    #[serde(default)]
    templates: Option<String>,
    #[serde(default)]
    skills: Option<String>,
    #[serde(default)]
    agents: Option<String>,
    #[serde(default)]
    environments: Option<String>,
    #[serde(default)]
    mcp_servers: Option<String>,
    #[serde(default)]
    apps: Option<String>,
    #[serde(default)]
    hooks: Option<String>,
    #[serde(default)]
    changelog: Option<String>,
    #[serde(default)]
    retrieval: Option<JsonValue>,
    #[serde(default)]
    interface: Option<RawPluginInterface>,
    #[serde(default)]
    compatibility: PluginCompatibility,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawPluginInterface {
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    short_description: Option<String>,
    #[serde(default)]
    long_description: Option<String>,
    #[serde(default)]
    developer_name: Option<String>,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    capabilities: Vec<String>,
    #[serde(default, alias = "websiteURL")]
    website_url: Option<String>,
    #[serde(default, alias = "privacyPolicyURL")]
    privacy_policy_url: Option<String>,
    #[serde(default, alias = "termsOfServiceURL")]
    terms_of_service_url: Option<String>,
    #[serde(default)]
    default_prompt: Option<RawDefaultPrompt>,
    #[serde(default)]
    brand_color: Option<String>,
    #[serde(default)]
    composer_icon: Option<String>,
    #[serde(default)]
    logo: Option<String>,
    #[serde(default)]
    screenshots: Vec<String>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum RawDefaultPrompt {
    One(String),
    Many(Vec<String>),
    Other(JsonValue),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MarketplaceInterface {
    pub display_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MarketplaceRemote {
    pub url: String,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default, alias = "repositoryURL")]
    pub repository_url: Option<String>,
    #[serde(default, alias = "changelogURL")]
    pub changelog_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PluginInstallPolicy {
    NotAvailable,
    Available,
    InstalledByDefault,
}

#[allow(clippy::derivable_impls)]
impl Default for PluginInstallPolicy {
    fn default() -> Self {
        Self::Available
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PluginAuthPolicy {
    OnInstall,
    OnUse,
}

#[allow(clippy::derivable_impls)]
impl Default for PluginAuthPolicy {
    fn default() -> Self {
        Self::OnInstall
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawMarketplaceManifest {
    name: String,
    #[serde(default)]
    interface: Option<MarketplaceInterface>,
    #[serde(default)]
    remote: Option<MarketplaceRemote>,
    #[serde(default)]
    plugins: Vec<RawMarketplacePlugin>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawMarketplacePlugin {
    name: String,
    source: RawMarketplacePluginSource,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    policy: RawMarketplacePluginPolicy,
    #[serde(default)]
    category: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawMarketplacePluginSource {
    #[serde(default)]
    source: String,
    path: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawMarketplacePluginPolicy {
    #[serde(default)]
    installation: PluginInstallPolicy,
    #[serde(default)]
    authentication: PluginAuthPolicy,
}

impl Default for RawMarketplacePluginPolicy {
    fn default() -> Self {
        Self {
            installation: PluginInstallPolicy::Available,
            authentication: PluginAuthPolicy::OnInstall,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginMarketplaceEntry {
    pub name: String,
    pub path: String,
    pub interface: Option<MarketplaceInterface>,
    pub remote: Option<MarketplaceRemote>,
    pub plugins: Vec<PluginSummary>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginSummary {
    pub id: String,
    pub name: String,
    pub marketplace_name: String,
    pub marketplace_path: String,
    pub source_path: String,
    pub installed_path: Option<String>,
    pub installed: bool,
    pub enabled: bool,
    pub install_policy: PluginInstallPolicy,
    pub auth_policy: PluginAuthPolicy,
    pub interface: Option<PluginInterface>,
    pub retrieval: Option<PluginRetrievalSummary>,
    pub operators: Vec<OperatorCandidateSummary>,
    pub templates: Option<PluginTemplateSummary>,
    pub environments: Vec<PluginEnvironmentSummary>,
    pub sync: Option<PluginSyncSummary>,
    pub changelog: Option<PluginChangelogSummary>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginSyncSummary {
    pub state: String,
    pub label: String,
    pub message: String,
    pub source_digest: Option<String>,
    pub installed_digest: Option<String>,
    pub installed_from_digest: Option<String>,
    pub changed_count: usize,
    pub local_modified_count: usize,
    pub conflict_count: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginSyncResult {
    pub plugin_id: String,
    pub status: String,
    pub installed_path: String,
    pub updated: Vec<String>,
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub kept_local: Vec<String>,
    pub conflicts: Vec<String>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginChangelogSummary {
    pub path: String,
    pub latest_version: Option<String>,
    pub entries: Vec<PluginChangelogEntry>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginChangelogEntry {
    pub version: Option<String>,
    pub date: Option<String>,
    pub title: String,
    pub body: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MarketplaceRemoteCheckResult {
    pub name: String,
    pub path: String,
    pub remote: MarketplaceRemote,
    pub state: String,
    pub label: String,
    pub message: String,
    pub local_digest: Option<String>,
    pub remote_digest: Option<String>,
    pub remote_plugin_count: Option<usize>,
    pub changed_plugins: Vec<String>,
    pub checked_at: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginEnvironmentSummary {
    pub id: String,
    pub version: String,
    pub canonical_id: String,
    pub name: Option<String>,
    pub description: Option<String>,
    pub manifest_path: String,
    pub runtime_type: String,
    pub runtime_file: Option<String>,
    pub runtime_file_kind: Option<String>,
    pub install_hint: Option<String>,
    pub check_command: Vec<String>,
    pub availability_status: String,
    pub availability_manager: Option<String>,
    pub availability_message: String,
    pub exposed: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginEnvironmentCheckResult {
    pub plugin_id: String,
    pub environment_id: String,
    pub canonical_id: String,
    pub installed: bool,
    pub plugin_root: String,
    pub check: EnvironmentCheckResult,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginTemplateSummary {
    pub count: usize,
    pub groups: Vec<PluginTemplateGroupSummary>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginTemplateGroupSummary {
    pub id: String,
    pub title: String,
    pub count: usize,
    pub templates: Vec<PluginTemplateItemSummary>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginTemplateItemSummary {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub category: Option<String>,
    pub tags: Vec<String>,
    pub exposed: bool,
    pub execute: JsonValue,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginRetrievalSummary {
    pub protocol_version: u32,
    pub resources: Vec<PluginRetrievalResourceSummary>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginRetrievalResourceSummary {
    pub id: String,
    pub category: String,
    pub label: String,
    pub description: String,
    pub subcategories: Vec<String>,
    pub capabilities: Vec<String>,
    pub required_credential_refs: Vec<String>,
    pub optional_credential_refs: Vec<String>,
    pub default_enabled: bool,
    pub replaces_builtin: bool,
    pub exposed: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginSkillSummary {
    pub name: String,
    pub description: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginDetail {
    pub summary: PluginSummary,
    pub description: Option<String>,
    pub changelog: Option<PluginChangelogSummary>,
    pub skills: Vec<PluginSkillSummary>,
    pub mcp_servers: Vec<String>,
    pub apps: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginInstallResult {
    pub plugin_id: String,
    pub installed_path: String,
    pub auth_policy: PluginAuthPolicy,
}

#[derive(Debug, Clone)]
pub struct PluginCapabilitySummary {
    pub id: String,
    pub display_name: String,
    pub description: Option<String>,
    pub has_skills: bool,
    pub mcp_servers: Vec<String>,
    pub apps: Vec<String>,
    pub retrieval_routes: Vec<String>,
    pub operator_count: usize,
    pub operation_count: usize,
    pub template_count: usize,
    pub template_groups: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct LoadedPlugin {
    pub id: String,
    pub manifest_name: Option<String>,
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub root: PathBuf,
    pub enabled: bool,
    pub skill_roots: Vec<PathBuf>,
    pub mcp_servers: HashMap<String, McpServerConfig>,
    pub apps: Vec<String>,
    pub retrieval: Option<PluginRetrievalManifest>,
    pub error: Option<String>,
}

impl LoadedPlugin {
    pub fn is_active(&self) -> bool {
        self.enabled && self.error.is_none()
    }
}

#[derive(Debug, Clone, Default)]
pub struct PluginLoadOutcome {
    plugins: Vec<LoadedPlugin>,
    capability_summaries: Vec<PluginCapabilitySummary>,
}

impl PluginLoadOutcome {
    fn from_plugins(plugins: Vec<LoadedPlugin>) -> Self {
        let capability_summaries = plugins
            .iter()
            .filter_map(plugin_capability_summary_from_loaded)
            .collect();
        Self {
            plugins,
            capability_summaries,
        }
    }

    pub fn plugins(&self) -> &[LoadedPlugin] {
        &self.plugins
    }

    pub fn capability_summaries(&self) -> &[PluginCapabilitySummary] {
        &self.capability_summaries
    }

    pub fn effective_skill_roots(&self) -> Vec<PathBuf> {
        let mut roots = self
            .plugins
            .iter()
            .filter(|plugin| plugin.is_active())
            .flat_map(|plugin| plugin.skill_roots.iter().cloned())
            .collect::<Vec<_>>();
        roots.sort();
        roots.dedup();
        roots
    }

    pub fn effective_mcp_servers(&self) -> HashMap<String, McpServerConfig> {
        let mut out = HashMap::new();
        for plugin in self.plugins.iter().filter(|plugin| plugin.is_active()) {
            // Keep duplicate MCP server-key precedence deterministic by applying
            // loaded plugins in sorted plugin-id order; later sorted IDs override earlier ones.
            out.extend(plugin.mcp_servers.clone());
        }
        out
    }

    pub fn effective_apps(&self) -> Vec<String> {
        let mut apps = Vec::new();
        let mut seen = HashSet::new();
        for plugin in self.plugins.iter().filter(|plugin| plugin.is_active()) {
            for app in &plugin.apps {
                if seen.insert(app.clone()) {
                    apps.push(app.clone());
                }
            }
        }
        apps
    }

    pub fn effective_retrieval_plugins(&self) -> Vec<PluginRetrievalRegistration> {
        self.plugins
            .iter()
            .filter(|plugin| plugin.is_active())
            .filter_map(|plugin| {
                Some(PluginRetrievalRegistration {
                    plugin_id: plugin.id.clone(),
                    plugin_root: plugin.root.clone(),
                    retrieval: plugin.retrieval.clone()?,
                })
            })
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginRetrievalRegistration {
    pub plugin_id: String,
    pub plugin_root: PathBuf,
    pub retrieval: PluginRetrievalManifest,
}

fn prompt_inline_text(raw: &str, max_chars: usize) -> String {
    let normalized = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    normalized.chars().take(max_chars).collect()
}

fn backtick_list(values: &[String]) -> String {
    values
        .iter()
        .map(|value| format!("`{}`", prompt_inline_text(value, 96)))
        .collect::<Vec<_>>()
        .join(", ")
}

fn plugin_capability_parts(plugin: &PluginCapabilitySummary) -> Vec<String> {
    let mut capabilities = Vec::new();
    if plugin.has_skills {
        capabilities.push("skills".to_string());
    }
    if !plugin.mcp_servers.is_empty() {
        capabilities.push(format!(
            "MCP servers: {}",
            backtick_list(&plugin.mcp_servers)
        ));
    }
    if !plugin.apps.is_empty() {
        capabilities.push(format!(
            "app connector refs: {} (metadata only unless matching app tools are explicitly available)",
            backtick_list(&plugin.apps)
        ));
    }
    if !plugin.retrieval_routes.is_empty() {
        capabilities.push(format!(
            "retrieval routes: {}",
            backtick_list(&plugin.retrieval_routes)
        ));
    }
    if plugin.operator_count > 0 {
        capabilities.push(format!(
            "operators: {} programs / {} operations via `unit_search` / `operator_describe` / `operator_execute`",
            plugin.operator_count, plugin.operation_count
        ));
    }
    if plugin.template_count > 0 {
        let groups = if plugin.template_groups.is_empty() {
            String::new()
        } else {
            format!("; groups: {}", backtick_list(&plugin.template_groups))
        };
        capabilities.push(format!(
            "templates: {} via `unit_search` / `unit_describe` / `template_execute`{}",
            plugin.template_count, groups
        ));
    }
    capabilities
}

fn format_plugin_capability_line(plugin: &PluginCapabilitySummary) -> String {
    let name = prompt_inline_text(&plugin.display_name, 96);
    let description = plugin
        .description
        .as_deref()
        .map(|description| prompt_inline_text(description, 180))
        .filter(|description| !description.is_empty());
    let description = description
        .map(|description| format!(": {description}"))
        .unwrap_or_default();
    let capabilities = plugin_capability_parts(plugin);
    let capability_suffix = if capabilities.is_empty() {
        String::new()
    } else {
        format!(" ({})", capabilities.join("; "))
    };
    format!("- `{name}`{description}{capability_suffix}")
}

pub fn format_plugins_system_section(outcome: &PluginLoadOutcome) -> Option<String> {
    let plugins = outcome.capability_summaries();
    if plugins.is_empty() {
        return None;
    }

    let mut lines = vec![
        "## Plugins (available)".to_string(),
        "Omiga plugins are native capability bundles: skills, MCP server configs, app connector references, Operator/Template units, retrieval routes, environments, and UI metadata. They do not run VS Code extension code or require a VS Code Extension Host.".to_string(),
        String::new(),
        "### Available plugins".to_string(),
    ];

    for plugin in plugins {
        lines.push(format_plugin_capability_line(plugin));
    }

    lines.push(String::new());
    lines.push("### How to use plugins".to_string());
    lines.push(
        "- Plugins are not invoked directly; use their underlying skills, MCP tools, operator tools, or explicitly available app tools.\n\
         - Template plugins expose Template units. Discover them with `unit_search` / `unit_describe`, then run exposed templates with `template_execute`; do not rebuild template logic with ad-hoc shell/file writes unless the user explicitly asks for custom code.\n\
         - Operator plugins expose Operator programs and operations through the Unit Index. Discover them with `unit_search` / `unit_describe` or `operator_describe`, then run them with `operator_execute`; subcommands are operation parameters, not separate tools.\n\
         - Retrieval plugin routes are local Search / Query / Fetch routes, not MCP tool names. If a plugin lists `retrieval routes: category.source`, call `search`, `query`, or `fetch` with that category/source.\n\
         - If the user explicitly names a plugin, prefer capabilities associated with that plugin for that turn.\n\
         - If a plugin contributes skills, those skills also appear in the Skills list and should be loaded with `skill_view` / `skill` before use.\n\
         - Do not assume VS Code extension UI/runtime behavior from an Omiga plugin."
            .to_string(),
    );

    Some(lines.join("\n"))
}

pub fn format_selected_plugins_system_section(
    outcome: &PluginLoadOutcome,
    selected_plugin_ids: &[String],
) -> Option<String> {
    let mut selected = Vec::new();
    let mut seen = HashSet::new();
    for id in selected_plugin_ids {
        let id = id.trim();
        if id.is_empty() || !seen.insert(id.to_string()) {
            continue;
        }
        selected.push(id.to_string());
    }
    if selected.is_empty() {
        return None;
    }

    let summaries = outcome
        .capability_summaries()
        .iter()
        .map(|plugin| (plugin.id.as_str(), plugin))
        .collect::<HashMap<_, _>>();

    let mut lines = vec![
        "## Explicitly selected plugins for this turn".to_string(),
        "The user selected the following Omiga plugins with the composer # picker. Prefer their capabilities for this turn when relevant; if a selected plugin is unavailable, explain that briefly and continue with the best fallback.".to_string(),
        String::new(),
    ];

    for id in selected {
        if let Some(plugin) = summaries.get(id.as_str()) {
            lines.push(format_plugin_capability_line(plugin));
        } else {
            lines.push(format!(
                "- `{}` is not currently active or installed; do not invent capabilities for it.",
                prompt_inline_text(&id, 96)
            ));
        }
    }

    Some(lines.join("\n"))
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct PluginConfigFile {
    #[serde(default)]
    plugins: HashMap<String, PluginConfigEntry>,
    // Preserve the rest of config.json when a single user marketplace entry is malformed.
    #[serde(
        default,
        deserialize_with = "deserialize_marketplace_sources_leniently"
    )]
    marketplaces: Vec<UserMarketplaceSource>,
}

fn deserialize_marketplace_sources_leniently<'de, D>(
    deserializer: D,
) -> Result<Vec<UserMarketplaceSource>, D::Error>
where
    D: Deserializer<'de>,
{
    let entries = Vec::<JsonValue>::deserialize(deserializer)?;
    Ok(entries
        .into_iter()
        .filter_map(
            |entry| match serde_json::from_value::<UserMarketplaceSource>(entry) {
                Ok(source) => Some(source),
                Err(err) => {
                    tracing::warn!(
                        "skipping invalid plugin marketplace source config entry: {err}"
                    );
                    None
                }
            },
        )
        .collect())
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum MarketplaceSourceKind {
    Local,
    Remote,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UserMarketplaceSource {
    pub id: String,
    pub kind: MarketplaceSourceKind,
    pub location: String,
    pub label: Option<String>,
    pub enabled: bool,
    pub added_at: String,
}

#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub enum MarketplaceSourceViewKind {
    Builtin,
    Local,
    Remote,
}

#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MarketplaceSourceView {
    pub id: String,
    pub kind: MarketplaceSourceViewKind,
    pub location: String,
    pub label: Option<String>,
    pub enabled: bool,
    pub removable: bool,
    pub added_at: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
pub struct BuiltinMarketplaceStatus {
    pub ok: bool,
    pub source: String,
    pub path: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RefreshResult {
    pub id: String,
    pub ok: bool,
    pub message: String,
    pub marketplace_name: Option<String>,
    pub plugin_count: Option<usize>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct PluginConfigEntry {
    #[serde(default)]
    enabled: bool,
    #[serde(default)]
    disabled_templates: HashSet<String>,
    #[serde(default)]
    retrieval_resources_configured: bool,
    #[serde(default, alias = "enabledRetrievalSources")]
    enabled_retrieval_resources: HashSet<String>,
    #[serde(default, alias = "disabledRetrievalSources")]
    disabled_retrieval_resources: HashSet<String>,
    #[serde(default)]
    disabled_environments: HashSet<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct PluginInstallState {
    #[serde(default = "plugin_install_state_schema_version")]
    schema_version: u32,
    plugin_id: String,
    installed_from_version: Option<String>,
    installed_from_digest: String,
    installed_at: String,
    last_synced_at: String,
    #[serde(default)]
    files: BTreeMap<String, String>,
}

fn plugin_install_state_schema_version() -> u32 {
    1
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginId {
    pub name: String,
    pub marketplace: String,
}

impl PluginId {
    pub fn new(name: &str, marketplace: &str) -> Result<Self, String> {
        let name = name.trim();
        let marketplace = marketplace.trim();
        validate_segment(name, "plugin name")?;
        validate_segment(marketplace, "marketplace name")?;
        Ok(Self {
            name: name.to_string(),
            marketplace: marketplace.to_string(),
        })
    }

    pub fn parse(id: &str) -> Result<Self, String> {
        let Some((name, marketplace)) = id.rsplit_once('@') else {
            return Err(format!(
                "invalid plugin id `{id}`; expected <plugin>@<marketplace>"
            ));
        };
        Self::new(name, marketplace)
    }

    pub fn key(&self) -> String {
        format!("{}@{}", self.name, self.marketplace)
    }
}

fn validate_segment(segment: &str, kind: &str) -> Result<(), String> {
    let trimmed = segment.trim();
    if trimmed.is_empty() || trimmed == "." {
        return Err(format!("{kind} must not be empty"));
    }
    if trimmed.contains("..") || trimmed.contains('/') || trimmed.contains('\\') {
        return Err(format!("{kind} contains unsafe path characters: {segment}"));
    }
    if !trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
    {
        return Err(format!(
            "{kind} must contain only letters, numbers, '.', '-' or '_'"
        ));
    }
    Ok(())
}

fn omiga_home() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".omiga")
}

fn config_path() -> PathBuf {
    omiga_home().join(USER_PLUGINS_CONFIG_FILE)
}

fn plugin_cache_root() -> PathBuf {
    omiga_home().join(PLUGINS_CACHE_DIR)
}

fn plugin_store_root() -> PathBuf {
    omiga_home().join(PLUGINS_ROOT_DIR)
}

fn user_marketplace_cache_dir(source_id: &str) -> Result<PathBuf, String> {
    validate_segment(source_id, "marketplace source id")?;
    Ok(omiga_home().join("marketplaces").join(source_id))
}

fn user_marketplace_cache_manifest_path(source_id: &str) -> Result<PathBuf, String> {
    Ok(user_marketplace_cache_dir(source_id)?.join(MARKETPLACE_FILE_NAME))
}

fn default_url_validation_project_root() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

fn plugin_store_root_from_cache_root(cache_root: &Path) -> PathBuf {
    cache_root
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(plugin_store_root)
}

pub fn dev_builtin_marketplace_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("repo root")
        .parent()
        .expect("workspace root")
        .join("omiga-plugins")
        .join(MARKETPLACE_FILE_NAME)
}

fn builtin_marketplace_cache_dir() -> PathBuf {
    omiga_home().join("marketplaces").join("builtin")
}

fn builtin_marketplace_cache_manifest_path() -> PathBuf {
    builtin_marketplace_cache_dir().join(MARKETPLACE_FILE_NAME)
}

fn builtin_env_override_path() -> Option<PathBuf> {
    std::env::var_os("OMIGA_PLUGINS_DIR")
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
}

fn resolve_marketplace_json_override(path: PathBuf) -> Option<PathBuf> {
    let candidate = if path.is_dir() {
        path.join(MARKETPLACE_FILE_NAME)
    } else {
        path
    };
    if candidate.file_name().and_then(|name| name.to_str()) == Some(MARKETPLACE_FILE_NAME)
        && candidate.is_file()
    {
        Some(candidate)
    } else {
        None
    }
}

fn resolve_builtin_env_marketplace_path() -> Option<PathBuf> {
    builtin_env_override_path().and_then(resolve_marketplace_json_override)
}

fn path_to_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

pub fn resolve_builtin_marketplace_path(
    env_override: Option<PathBuf>,
    dev_sibling: Option<PathBuf>,
    cache_marketplace: PathBuf,
) -> Option<PathBuf> {
    if let Some(path) = env_override.and_then(resolve_marketplace_json_override) {
        return Some(path);
    }
    if let Some(path) = dev_sibling.filter(|path| path.is_file()) {
        return Some(path);
    }
    cache_marketplace.is_file().then_some(cache_marketplace)
}

pub fn builtin_marketplace_path() -> Option<PathBuf> {
    resolve_builtin_marketplace_path(
        builtin_env_override_path(),
        Some(dev_builtin_marketplace_path()),
        builtin_marketplace_cache_manifest_path(),
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum BuiltinMarketplaceSource {
    Env(PathBuf),
    Dev(PathBuf),
    GithubCache(PathBuf),
    GithubRemote,
}

fn builtin_marketplace_source() -> BuiltinMarketplaceSource {
    if let Some(path) = resolve_builtin_env_marketplace_path() {
        return BuiltinMarketplaceSource::Env(path);
    }

    let dev_path = dev_builtin_marketplace_path();
    if dev_path.is_file() {
        return BuiltinMarketplaceSource::Dev(dev_path);
    }

    let cache_path = builtin_marketplace_cache_manifest_path();
    if cache_path.is_file() {
        BuiltinMarketplaceSource::GithubCache(cache_path)
    } else {
        BuiltinMarketplaceSource::GithubRemote
    }
}

pub fn marketplace_paths(
    _project_root: Option<&Path>,
    _resource_dir: Option<&Path>,
) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let mut seen = HashSet::new();
    let mut push_path = |path: PathBuf| {
        let key = fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
        if path.is_file() && seen.insert(key) {
            paths.push(path);
        }
    };
    if let Some(path) = builtin_marketplace_path() {
        push_path(path);
    }
    for source in read_config()
        .marketplaces
        .into_iter()
        .filter(|source| source.enabled)
    {
        match source.kind {
            MarketplaceSourceKind::Local => {
                match resolve_user_local_marketplace_path(&source.location) {
                    Ok(path) => push_path(path),
                    Err(err) => {
                        tracing::warn!(
                            source_id = %source.id,
                            location = %source.location,
                            "skipping configured plugin marketplace source: {err}"
                        );
                    }
                }
            }
            MarketplaceSourceKind::Remote => {
                let path = match user_marketplace_cache_manifest_path(&source.id) {
                    Ok(path) => path,
                    Err(err) => {
                        tracing::warn!(
                            source_id = %source.id,
                            location = %source.location,
                            "skipping configured remote plugin marketplace source: {err}"
                        );
                        continue;
                    }
                };
                if !path.is_file() {
                    tracing::warn!(
                        source_id = %source.id,
                        location = %source.location,
                        path = %path.display(),
                        "skipping configured remote plugin marketplace source: cached marketplace does not exist"
                    );
                    continue;
                }
                match read_marketplace(&path) {
                    Ok(_) => push_path(path),
                    Err(err) => {
                        tracing::warn!(
                            source_id = %source.id,
                            location = %source.location,
                            path = %path.display(),
                            "skipping configured remote plugin marketplace source: {err}"
                        );
                    }
                }
            }
        }
    }
    paths
}

fn resolve_user_local_marketplace_path(location: &str) -> Result<PathBuf, String> {
    let trimmed = location.trim();
    if trimmed.is_empty() {
        return Err("local marketplace source path must not be empty".to_string());
    }
    let input = PathBuf::from(trimmed);
    let metadata = fs::metadata(&input).map_err(|err| {
        format!(
            "local marketplace source `{}` does not exist: {err}",
            input.display()
        )
    })?;

    let candidate = if metadata.is_dir() {
        let dir = fs::canonicalize(&input).map_err(|err| {
            format!(
                "canonicalize local marketplace directory `{}`: {err}",
                input.display()
            )
        })?;
        dir.join(MARKETPLACE_FILE_NAME)
    } else if metadata.is_file() {
        let path = fs::canonicalize(&input).map_err(|err| {
            format!(
                "canonicalize local marketplace file `{}`: {err}",
                input.display()
            )
        })?;
        if path.file_name().and_then(|name| name.to_str()) != Some(MARKETPLACE_FILE_NAME) {
            return Err(format!(
                "local marketplace source file must be named `{MARKETPLACE_FILE_NAME}`"
            ));
        }
        path
    } else {
        return Err(format!(
            "local marketplace source `{}` must be a file or directory",
            input.display()
        ));
    };

    if !candidate.is_file() {
        return Err(format!(
            "local marketplace source `{}` does not contain `{MARKETPLACE_FILE_NAME}`",
            input.display()
        ));
    }
    let path = fs::canonicalize(&candidate).map_err(|err| {
        format!(
            "canonicalize local marketplace file `{}`: {err}",
            candidate.display()
        )
    })?;
    read_marketplace(&path)
        .map_err(|err| format!("invalid local marketplace `{}`: {err}", path.display()))?;
    Ok(path)
}

fn normalized_user_local_marketplace_location(location: &str) -> Result<(String, PathBuf), String> {
    let input = PathBuf::from(location.trim());
    let marketplace_path = resolve_user_local_marketplace_path(location)?;
    let metadata = fs::metadata(&input).map_err(|err| {
        format!(
            "local marketplace source `{}` does not exist: {err}",
            input.display()
        )
    })?;
    let location_path = if metadata.is_dir() {
        fs::canonicalize(&input).map_err(|err| {
            format!(
                "canonicalize local marketplace directory `{}`: {err}",
                input.display()
            )
        })?
    } else {
        marketplace_path.clone()
    };
    let location = location_path
        .to_str()
        .ok_or_else(|| "local marketplace source path must be valid UTF-8".to_string())?
        .to_string();
    Ok((location, marketplace_path))
}

pub fn marketplace_plugin_source_root(
    plugin_id: &str,
    project_root: Option<&Path>,
    resource_dir: Option<&Path>,
) -> Option<PathBuf> {
    let plugin_id = PluginId::parse(plugin_id).ok()?;
    for path in marketplace_paths(project_root, resource_dir) {
        let Ok(marketplace) = read_marketplace(&path) else {
            continue;
        };
        if marketplace.name != plugin_id.marketplace {
            continue;
        }
        for entry in &marketplace.plugins {
            if entry.name == plugin_id.name {
                return resolve_marketplace_source_path(&path, &entry.source).ok();
            }
        }
    }
    None
}

fn marketplace_root_dir(marketplace_path: &Path) -> PathBuf {
    let parent = marketplace_path.parent().unwrap_or_else(|| Path::new("."));
    if parent.file_name().and_then(|s| s.to_str()) == Some("plugins")
        && parent
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|s| s.to_str())
            == Some(".omiga")
    {
        return parent.parent().unwrap_or(parent).to_path_buf();
    }
    parent.to_path_buf()
}

fn resolve_safe_relative_path(root: &Path, value: &str, field: &str) -> Result<PathBuf, String> {
    let Some(rel) = value.strip_prefix("./") else {
        return Err(format!("{field} must start with `./`"));
    };
    if rel.trim().is_empty() {
        return Err(format!("{field} must not be empty"));
    }
    let mut normalized = PathBuf::new();
    for component in Path::new(rel).components() {
        match component {
            Component::Normal(part) => normalized.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(format!("{field} must stay within plugin root"));
            }
        }
    }
    if normalized.as_os_str().is_empty() {
        return Err(format!("{field} must not resolve to plugin root"));
    }
    Ok(root.join(normalized))
}

fn resolve_optional_path(root: &Path, value: Option<&str>, field: &str) -> Option<PathBuf> {
    match value {
        Some(value) => match resolve_safe_relative_path(root, value, field) {
            Ok(path) => Some(path),
            Err(err) => {
                tracing::warn!(plugin = %root.display(), "ignoring {field}: {err}");
                None
            }
        },
        None => None,
    }
}

fn resolve_interface_path(root: &Path, value: Option<String>, field: &str) -> Option<PathBuf> {
    resolve_optional_path(root, value.as_deref(), field)
}

fn default_prompts(value: Option<RawDefaultPrompt>) -> Vec<String> {
    let values = match value {
        Some(RawDefaultPrompt::One(prompt)) => vec![prompt],
        Some(RawDefaultPrompt::Many(prompts)) => prompts,
        Some(RawDefaultPrompt::Other(_)) | None => Vec::new(),
    };
    values
        .into_iter()
        .map(|prompt| prompt.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|prompt| !prompt.is_empty())
        .take(3)
        .map(|prompt| prompt.chars().take(128).collect())
        .collect()
}

pub fn load_plugin_manifest(plugin_root: &Path) -> Option<PluginManifest> {
    let manifest_path = plugin_manifest_path(plugin_root)?;
    let raw = fs::read_to_string(&manifest_path).ok()?;
    let parsed: RawPluginManifest = serde_json::from_str(&raw)
        .map_err(|err| {
            tracing::warn!(path = %manifest_path.display(), "invalid plugin manifest: {err}");
            err
        })
        .ok()?;
    let fallback_name = plugin_root
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("plugin");
    let name = if parsed.name.trim().is_empty() {
        fallback_name.to_string()
    } else {
        parsed.name.trim().to_string()
    };

    let interface = parsed.interface.map(|interface| PluginInterface {
        display_name: interface.display_name,
        short_description: interface.short_description,
        long_description: interface.long_description,
        developer_name: interface.developer_name,
        category: interface.category,
        capabilities: interface.capabilities,
        website_url: interface.website_url,
        privacy_policy_url: interface.privacy_policy_url,
        terms_of_service_url: interface.terms_of_service_url,
        default_prompt: default_prompts(interface.default_prompt),
        brand_color: interface.brand_color,
        composer_icon: resolve_interface_path(
            plugin_root,
            interface.composer_icon,
            "interface.composerIcon",
        ),
        logo: resolve_interface_path(plugin_root, interface.logo, "interface.logo"),
        screenshots: interface
            .screenshots
            .into_iter()
            .filter_map(|path| {
                resolve_interface_path(plugin_root, Some(path), "interface.screenshots")
            })
            .collect(),
    });

    let retrieval = parsed.retrieval.and_then(|value| {
        match load_plugin_retrieval_manifest(plugin_root, value) {
            Ok(manifest) => Some(manifest),
            Err(err) => {
                tracing::warn!(
                    plugin = %plugin_root.display(),
                    "ignoring invalid retrieval plugin manifest: {err}"
                );
                None
            }
        }
    });

    Some(PluginManifest {
        name,
        version: parsed.version,
        description: parsed.description,
        operators: resolve_optional_path(plugin_root, parsed.operators.as_deref(), "operators"),
        templates: resolve_optional_path(plugin_root, parsed.templates.as_deref(), "templates"),
        skills: resolve_optional_path(plugin_root, parsed.skills.as_deref(), "skills"),
        agents: resolve_optional_path(plugin_root, parsed.agents.as_deref(), "agents"),
        environments: resolve_optional_path(
            plugin_root,
            parsed.environments.as_deref(),
            "environments",
        ),
        mcp_servers: resolve_optional_path(
            plugin_root,
            parsed.mcp_servers.as_deref(),
            "mcpServers",
        ),
        apps: resolve_optional_path(plugin_root, parsed.apps.as_deref(), "apps"),
        hooks: resolve_optional_path(plugin_root, parsed.hooks.as_deref(), "hooks"),
        changelog: resolve_optional_path(plugin_root, parsed.changelog.as_deref(), "changelog"),
        retrieval,
        interface,
        compatibility: parsed.compatibility,
    })
}

pub fn plugin_manifest_path(plugin_root: &Path) -> Option<PathBuf> {
    [
        PLUGIN_MANIFEST_FILE,
        OMIGA_PLUGIN_MANIFEST_PATH,
        CODEX_PLUGIN_MANIFEST_PATH,
    ]
    .into_iter()
    .map(|relative| plugin_root.join(relative))
    .find(|path| path.is_file())
}

fn default_changelog_path(plugin_root: &Path) -> Option<PathBuf> {
    ["CHANGELOG.md", "changelog.md", "CHANGELOG"]
        .into_iter()
        .map(|relative| plugin_root.join(relative))
        .find(|path| path.is_file())
}

fn truncate_text(value: &str, max_chars: usize) -> String {
    let mut out = value.chars().take(max_chars).collect::<String>();
    if value.chars().count() > max_chars {
        out.push('…');
    }
    out
}

fn changelog_heading_parts(raw: &str) -> (Option<String>, Option<String>, String) {
    let title = raw.trim().trim_matches('#').trim().to_string();
    let mut date = None;
    let mut version_source = title.as_str();
    if let Some((left, right)) = title.split_once(" - ") {
        let candidate = right.trim();
        if candidate.len() >= 10
            && candidate
                .chars()
                .take(10)
                .all(|ch| ch.is_ascii_digit() || ch == '-')
        {
            date = Some(candidate.chars().take(10).collect::<String>());
            version_source = left.trim();
        }
    }
    let version = version_source
        .trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .trim_start_matches('v')
        .trim()
        .to_string();
    let version =
        (version.chars().any(|ch| ch.is_ascii_digit()) && version.len() <= 40).then_some(version);
    (version, date, title)
}

fn parse_changelog_entries(raw: &str, fallback_version: Option<&str>) -> Vec<PluginChangelogEntry> {
    let mut entries = Vec::new();
    let mut current_title: Option<String> = None;
    let mut current_body = Vec::<String>::new();

    fn flush_entry(
        entries: &mut Vec<PluginChangelogEntry>,
        title: Option<String>,
        body: &mut Vec<String>,
    ) {
        let Some(title) = title else {
            body.clear();
            return;
        };
        if title.eq_ignore_ascii_case("changelog") {
            body.clear();
            return;
        }
        let (version, date, title) = changelog_heading_parts(&title);
        let body_text = body.join("\n").trim().to_string();
        entries.push(PluginChangelogEntry {
            version,
            date,
            title,
            body: truncate_text(&body_text, 900),
        });
        body.clear();
    }

    for line in raw.lines() {
        let trimmed = line.trim_start();
        let is_entry_heading = (trimmed.starts_with("## ") && !trimmed.starts_with("### "))
            || trimmed.starts_with("### ");
        if is_entry_heading {
            flush_entry(&mut entries, current_title.take(), &mut current_body);
            current_title = Some(trimmed.trim_start_matches('#').trim().to_string());
        } else if current_title.is_some() {
            current_body.push(line.to_string());
        }
    }
    flush_entry(&mut entries, current_title.take(), &mut current_body);

    if entries.is_empty() {
        let body = truncate_text(raw.trim(), 1200);
        if !body.is_empty() {
            entries.push(PluginChangelogEntry {
                version: fallback_version.map(str::to_string),
                date: None,
                title: fallback_version
                    .map(|version| format!("Version {version}"))
                    .unwrap_or_else(|| "Changelog".to_string()),
                body,
            });
        }
    }

    entries.truncate(8);
    entries
}

fn plugin_changelog_summary(
    plugin_root: &Path,
    manifest: Option<&PluginManifest>,
) -> Option<PluginChangelogSummary> {
    let path = manifest
        .and_then(|manifest| manifest.changelog.clone())
        .or_else(|| default_changelog_path(plugin_root))?;
    let metadata = fs::metadata(&path).ok()?;
    if metadata.len() as usize > MAX_CHANGELOG_BYTES {
        tracing::warn!(path = %path.display(), "plugin changelog is too large; skipping preview");
        return None;
    }
    let raw = fs::read_to_string(&path).ok()?;
    let entries = parse_changelog_entries(
        &raw,
        manifest.and_then(|manifest| manifest.version.as_deref()),
    );
    if entries.is_empty() {
        return None;
    }
    let latest_version = entries
        .iter()
        .find_map(|entry| entry.version.clone())
        .or_else(|| manifest.and_then(|manifest| manifest.version.clone()));
    Some(PluginChangelogSummary {
        path: path.to_string_lossy().into_owned(),
        latest_version,
        entries,
    })
}

fn read_config() -> PluginConfigFile {
    let mut config = fs::read_to_string(config_path())
        .ok()
        .and_then(|raw| serde_json::from_str::<PluginConfigFile>(&raw).ok())
        .unwrap_or_default();
    if migrate_superseded_builtin_plugin_config(&mut config) {
        if let Err(err) = write_config(&config) {
            tracing::warn!("failed to persist superseded curated plugin migration: {err}");
        }
    }
    config
}

#[derive(Debug, Clone, Default)]
struct SupersededPluginMigrationIndex {
    plugin_replacements: HashMap<String, Vec<String>>,
    retrieval_resource_replacements: HashMap<String, (String, String)>,
}

fn migration_plugin_name(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

fn migration_plugin_key(plugin_name: &str, marketplace_name: &str) -> String {
    format!(
        "{}@{}",
        migration_plugin_name(plugin_name),
        migration_plugin_name(marketplace_name)
    )
}

fn migration_legacy_plugin_key(legacy_plugin_name: &str, marketplace_name: &str) -> String {
    let legacy = migration_plugin_name(legacy_plugin_name);
    if legacy.contains('@') {
        legacy
    } else {
        migration_plugin_key(&legacy, marketplace_name)
    }
}

fn migration_kebab_id(value: &str) -> String {
    normalize_id(value).replace('_', "-")
}

fn migration_resource_legacy_plugin_names(resource: &PluginRetrievalResource) -> Vec<String> {
    let category = migration_kebab_id(&resource.category);
    let resource_id = migration_kebab_id(&resource.id);
    let mut names = vec![
        format!("retrieval-{category}-{resource_id}"),
        format!("operator-{resource_id}-search"),
    ];
    for alias in &resource.aliases {
        let alias = migration_kebab_id(alias);
        if alias.is_empty() {
            continue;
        }
        names.push(format!("retrieval-{category}-{alias}"));
        names.push(format!("operator-{alias}-search"));
    }
    names.sort();
    names.dedup();
    names
}

fn build_superseded_plugin_migration_index(
    marketplace_paths: &[PathBuf],
) -> SupersededPluginMigrationIndex {
    let mut index = SupersededPluginMigrationIndex::default();
    for marketplace_path in marketplace_paths {
        let Ok(marketplace) = read_marketplace(marketplace_path) else {
            continue;
        };
        for entry in &marketplace.plugins {
            let Ok(source_path) = resolve_marketplace_source_path(marketplace_path, &entry.source)
            else {
                continue;
            };
            let Some(manifest) = load_plugin_manifest(&source_path) else {
                continue;
            };
            let replacement_key = format!("{}@{}", manifest.name, marketplace.name);
            for legacy_name in manifest
                .compatibility
                .supersedes_plugins
                .iter()
                .map(|name| migration_legacy_plugin_key(name, &marketplace.name))
                .filter(|name| !name.is_empty())
            {
                index
                    .plugin_replacements
                    .entry(legacy_name)
                    .or_default()
                    .push(replacement_key.clone());
            }
            if let Some(retrieval) = &manifest.retrieval {
                for resource in &retrieval.resources {
                    if !resource.replaces_builtin {
                        continue;
                    }
                    let resource_key =
                        retrieval_resource_config_key(&resource.category, &resource.id);
                    for legacy_name in migration_resource_legacy_plugin_names(resource) {
                        let legacy_key =
                            migration_legacy_plugin_key(&legacy_name, &marketplace.name);
                        index
                            .retrieval_resource_replacements
                            .insert(legacy_key, (replacement_key.clone(), resource_key.clone()));
                    }
                }
            }
        }
    }
    for replacements in index.plugin_replacements.values_mut() {
        replacements.sort();
        replacements.dedup();
    }
    index
}

fn removed_builtin_plugin(plugin_name: &str) -> bool {
    matches!(plugin_name, "operator-smoke" | "notebook-helper")
}

fn migrate_superseded_builtin_plugin_config(config: &mut PluginConfigFile) -> bool {
    let marketplace_paths = builtin_marketplace_path().into_iter().collect::<Vec<_>>();
    migrate_superseded_builtin_plugin_config_with_marketplaces(config, &marketplace_paths)
}

fn migrate_superseded_builtin_plugin_config_with_marketplaces(
    config: &mut PluginConfigFile,
    marketplace_paths: &[PathBuf],
) -> bool {
    let migration_index = build_superseded_plugin_migration_index(marketplace_paths);
    let mut changed = false;
    let mut replacements = HashMap::<String, PluginConfigEntry>::new();
    let keys = config.plugins.keys().cloned().collect::<Vec<_>>();
    for key in keys {
        let Ok(plugin_id) = PluginId::parse(&key) else {
            continue;
        };
        let migration_key = migration_plugin_key(&plugin_id.name, &plugin_id.marketplace);
        if plugin_id.marketplace == "omiga-curated" && removed_builtin_plugin(&plugin_id.name) {
            config.plugins.remove(&key);
            changed = true;
            continue;
        }
        if let Some(replacement_names) = migration_index.plugin_replacements.get(&migration_key) {
            let removed_entry = config.plugins.remove(&key).unwrap_or_default();
            for replacement_key in replacement_names {
                let replacement = replacements.entry(replacement_key.clone()).or_default();
                replacement.enabled = replacement.enabled || removed_entry.enabled;
            }
            changed = true;
            continue;
        }
        let Some((replacement_key, resource_key)) = migration_index
            .retrieval_resource_replacements
            .get(&migration_key)
        else {
            continue;
        };
        let removed_entry = config.plugins.remove(&key).unwrap_or_default();
        let replacement = replacements.entry(replacement_key.clone()).or_default();
        replacement.enabled = replacement.enabled || removed_entry.enabled;
        if removed_entry.enabled {
            replacement
                .disabled_retrieval_resources
                .remove(resource_key);
        } else {
            replacement
                .disabled_retrieval_resources
                .insert(resource_key.to_string());
        }
        changed = true;
    }

    for (key, replacement) in replacements {
        let entry = config.plugins.entry(key).or_default();
        entry.enabled = entry.enabled || replacement.enabled;
        if !replacement.enabled_retrieval_resources.is_empty() {
            entry
                .enabled_retrieval_resources
                .extend(replacement.enabled_retrieval_resources);
        }
        if !replacement.disabled_retrieval_resources.is_empty() {
            entry
                .disabled_retrieval_resources
                .extend(replacement.disabled_retrieval_resources);
        }
    }
    changed
}

fn write_config(config: &PluginConfigFile) -> Result<(), String> {
    let path = config_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| format!("create plugin config dir: {err}"))?;
    }
    let raw = serde_json::to_string_pretty(config).map_err(|err| err.to_string())?;
    fs::write(&path, format!("{raw}\n")).map_err(|err| format!("write plugin config: {err}"))
}

pub fn list_user_marketplace_sources() -> Vec<UserMarketplaceSource> {
    read_config().marketplaces
}

fn builtin_marketplace_label(path: &Path) -> Option<String> {
    match read_marketplace(path) {
        Ok(marketplace) => marketplace
            .interface
            .and_then(|interface| interface.display_name)
            .filter(|display_name| !display_name.trim().is_empty())
            .or(Some(marketplace.name)),
        Err(err) => {
            tracing::warn!(
                path = %path.display(),
                "failed to read built-in plugin marketplace source label: {err}"
            );
            None
        }
    }
}

pub fn list_marketplace_source_views() -> Vec<MarketplaceSourceView> {
    let mut views = Vec::new();
    let (location, label) = match builtin_marketplace_source() {
        BuiltinMarketplaceSource::Env(path) | BuiltinMarketplaceSource::Dev(path) => {
            (path_to_string(&path), builtin_marketplace_label(&path))
        }
        BuiltinMarketplaceSource::GithubCache(path) => (
            BUILTIN_GIT_URL.to_string(),
            builtin_marketplace_label(&path),
        ),
        BuiltinMarketplaceSource::GithubRemote => (BUILTIN_GIT_URL.to_string(), None),
    };
    views.push(MarketplaceSourceView {
        id: "builtin".to_string(),
        kind: MarketplaceSourceViewKind::Builtin,
        location,
        label,
        enabled: true,
        removable: false,
        added_at: None,
    });

    views.extend(
        list_user_marketplace_sources()
            .into_iter()
            .map(|source| MarketplaceSourceView {
                id: source.id,
                kind: match source.kind {
                    MarketplaceSourceKind::Local => MarketplaceSourceViewKind::Local,
                    MarketplaceSourceKind::Remote => MarketplaceSourceViewKind::Remote,
                },
                location: source.location,
                label: source.label,
                enabled: source.enabled,
                removable: true,
                added_at: Some(source.added_at),
            }),
    );

    views
}

fn normalized_marketplace_label(label: Option<String>) -> Option<String> {
    label
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn new_marketplace_source_id(prefix: &str, config: &PluginConfigFile) -> String {
    let existing_ids = config
        .marketplaces
        .iter()
        .map(|source| source.id.clone())
        .collect::<HashSet<_>>();
    let mut id = format!("{prefix}-{}", uuid::Uuid::new_v4());
    while existing_ids.contains(&id) {
        id = format!("{prefix}-{}", uuid::Uuid::new_v4());
    }
    id
}

fn normalized_remote_marketplace_location_for_compare(location: &str) -> String {
    reqwest::Url::parse(location.trim())
        .map(|url| url.to_string())
        .unwrap_or_else(|_| location.trim().to_string())
}

fn validate_remote_marketplace_url(
    location: &str,
    project_root: &Path,
    resolve_dns: bool,
) -> Result<String, String> {
    let trimmed = location.trim();
    if trimmed.is_empty() {
        return Err("remote marketplace source URL must not be empty".to_string());
    }
    let parsed = reqwest::Url::parse(trimmed).map_err(|err| format!("Invalid URL: {err}"))?;
    if parsed.scheme() != "https" {
        return Err("remote marketplace source URL must use https".to_string());
    }
    crate::domain::tools::web_safety::validate_public_http_url(project_root, trimmed, resolve_dns)
        .map_err(|err| format!("remote marketplace source URL is not allowed: {err}"))?;
    Ok(parsed.to_string())
}

fn add_local_user_marketplace_source(
    location: String,
    label: Option<String>,
) -> Result<UserMarketplaceSource, String> {
    let (location, marketplace_path) = normalized_user_local_marketplace_location(&location)?;
    let mut config = read_config();
    for source in &config.marketplaces {
        if source.kind != MarketplaceSourceKind::Local {
            continue;
        }
        match resolve_user_local_marketplace_path(&source.location) {
            Ok(existing_path) if existing_path == marketplace_path => {
                return Err(format!(
                    "local marketplace source `{}` is already configured",
                    marketplace_path.display()
                ));
            }
            Ok(_) => {}
            Err(err) => {
                tracing::warn!(
                    source_id = %source.id,
                    location = %source.location,
                    "ignoring invalid configured plugin marketplace source while checking duplicates: {err}"
                );
            }
        }
    }

    let source = UserMarketplaceSource {
        id: new_marketplace_source_id("local", &config),
        kind: MarketplaceSourceKind::Local,
        location,
        label: normalized_marketplace_label(label),
        enabled: true,
        added_at: chrono::Utc::now().to_rfc3339(),
    };
    config.marketplaces.push(source.clone());
    write_config(&config)?;
    Ok(source)
}

fn add_remote_user_marketplace_source(
    location: String,
    label: Option<String>,
) -> Result<UserMarketplaceSource, String> {
    let project_root = default_url_validation_project_root();
    let location = validate_remote_marketplace_url(&location, &project_root, false)?;
    let duplicate_key = normalized_remote_marketplace_location_for_compare(&location);
    let mut config = read_config();
    for source in &config.marketplaces {
        if source.kind == MarketplaceSourceKind::Remote
            && normalized_remote_marketplace_location_for_compare(&source.location) == duplicate_key
        {
            return Err(format!(
                "remote marketplace source URL `{location}` is already configured"
            ));
        }
    }

    let source = UserMarketplaceSource {
        id: new_marketplace_source_id("remote", &config),
        kind: MarketplaceSourceKind::Remote,
        location,
        label: normalized_marketplace_label(label),
        enabled: true,
        added_at: chrono::Utc::now().to_rfc3339(),
    };
    config.marketplaces.push(source.clone());
    write_config(&config)?;
    Ok(source)
}

pub fn add_user_marketplace_source(
    kind: MarketplaceSourceKind,
    location: String,
    label: Option<String>,
) -> Result<UserMarketplaceSource, String> {
    match kind {
        MarketplaceSourceKind::Local => add_local_user_marketplace_source(location, label),
        MarketplaceSourceKind::Remote => add_remote_user_marketplace_source(location, label),
    }
}

const GIT_REQUIRED_MESSAGE: &str =
    "git is required to add remote marketplace sources; install git or use a local path";

fn git_failure_message(output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if !stderr.is_empty() {
        return stderr;
    }
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if !stdout.is_empty() {
        return stdout;
    }
    format!("git exited with status {}", output.status)
}

fn run_git_command(command: &mut Command, action: &str) -> Result<(), String> {
    let output = match command.output() {
        Ok(output) => output,
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            return Err(GIT_REQUIRED_MESSAGE.to_string());
        }
        Err(err) => return Err(format!("run git {action}: {err}")),
    };
    if output.status.success() {
        return Ok(());
    }
    Err(format!(
        "git {action} failed: {}",
        git_failure_message(&output)
    ))
}

fn clone_or_update_marketplace_repo(remote_url: &str, dest: &Path) -> Result<(), String> {
    if dest.exists() && !valid_git_work_tree(dest)? {
        remove_marketplace_cache_dest(dest)?;
    }

    if !dest.exists() {
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)
                .map_err(|err| format!("create marketplace cache dir: {err}"))?;
        }
        let mut command = Command::new("git");
        command
            .arg("clone")
            .arg("--depth")
            .arg("1")
            .arg("--quiet")
            .arg(remote_url)
            .arg(dest);
        return run_git_command(&mut command, "clone");
    }

    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(dest)
        .arg("pull")
        .arg("--ff-only")
        .arg("--quiet");
    run_git_command(&mut command, "pull")
}

fn valid_git_work_tree(dest: &Path) -> Result<bool, String> {
    if !dest.is_dir() || !dest.join(".git").exists() {
        return Ok(false);
    }

    let output = match Command::new("git")
        .arg("-C")
        .arg(dest)
        .arg("rev-parse")
        .arg("--is-inside-work-tree")
        .output()
    {
        Ok(output) => output,
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            return Err(GIT_REQUIRED_MESSAGE.to_string());
        }
        Err(err) => return Err(format!("run git rev-parse: {err}")),
    };
    if !output.status.success() {
        return Ok(false);
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim() == "true")
}

fn remove_marketplace_cache_dest(dest: &Path) -> Result<(), String> {
    if dest.is_dir() {
        fs::remove_dir_all(dest)
    } else {
        fs::remove_file(dest)
    }
    .map_err(|err| {
        format!(
            "remove invalid marketplace cache `{}`: {err}",
            dest.display()
        )
    })
}

fn builtin_marketplace_status(
    ok: bool,
    source: &str,
    path: Option<&Path>,
    message: impl Into<String>,
) -> BuiltinMarketplaceStatus {
    BuiltinMarketplaceStatus {
        ok,
        source: source.to_string(),
        path: path.map(path_to_string),
        message: message.into(),
    }
}

pub fn ensure_builtin_marketplace() -> Result<BuiltinMarketplaceStatus, String> {
    if let Some(raw_env_path) = builtin_env_override_path() {
        if let Some(path) = resolve_marketplace_json_override(raw_env_path.clone()) {
            if let Err(err) = read_marketplace(&path) {
                return Ok(builtin_marketplace_status(
                    false,
                    "env",
                    Some(&path),
                    format!(
                        "OMIGA_PLUGINS_DIR built-in marketplace override `{}` is invalid: {err}",
                        path.display()
                    ),
                ));
            }
            return Ok(builtin_marketplace_status(
                true,
                "env",
                Some(&path),
                "Using OMIGA_PLUGINS_DIR built-in marketplace override",
            ));
        }
        tracing::warn!(
            location = %raw_env_path.display(),
            "OMIGA_PLUGINS_DIR does not point to an existing marketplace.json; falling back"
        );
    }

    let dev_path = dev_builtin_marketplace_path();
    if dev_path.is_file() {
        if let Err(err) = read_marketplace(&dev_path) {
            return Ok(builtin_marketplace_status(
                false,
                "dev",
                Some(&dev_path),
                format!(
                    "Development built-in marketplace `{}` is invalid: {err}",
                    dev_path.display()
                ),
            ));
        }
        return Ok(builtin_marketplace_status(
            true,
            "dev",
            Some(&dev_path),
            "Using development built-in marketplace",
        ));
    }

    let cache_dir = builtin_marketplace_cache_dir();
    if let Err(err) = clone_or_update_marketplace_repo(BUILTIN_GIT_URL, &cache_dir) {
        return Ok(builtin_marketplace_status(
            false,
            "github",
            None,
            format!(
                "Unable to clone or update the built-in marketplace from {BUILTIN_GIT_URL}: {err}. Check git and network access, then retry."
            ),
        ));
    }

    let marketplace_path = cache_dir.join(MARKETPLACE_FILE_NAME);
    if !marketplace_path.is_file() {
        return Ok(builtin_marketplace_status(
            false,
            "github",
            None,
            format!(
                "The built-in marketplace clone from {BUILTIN_GIT_URL} did not contain `{MARKETPLACE_FILE_NAME}` at `{}`. Remove `{}` and retry.",
                marketplace_path.display(),
                cache_dir.display()
            ),
        ));
    }

    if let Err(err) = read_marketplace(&marketplace_path) {
        return Ok(builtin_marketplace_status(
            false,
            "github",
            None,
            format!(
                "The built-in marketplace cache at `{}` is invalid: {err}. Remove `{}` and retry.",
                marketplace_path.display(),
                cache_dir.display()
            ),
        ));
    }

    Ok(builtin_marketplace_status(
        true,
        "github",
        Some(&marketplace_path),
        "Built-in marketplace is available from GitHub cache",
    ))
}

#[tauri::command]
pub fn ensure_builtin_marketplace_source(
    _project_root: Option<String>,
) -> Result<BuiltinMarketplaceStatus, String> {
    ensure_builtin_marketplace()
}

fn refresh_success_result(
    id: &str,
    message: impl Into<String>,
    marketplace: RawMarketplaceManifest,
) -> RefreshResult {
    RefreshResult {
        id: id.to_string(),
        ok: true,
        message: message.into(),
        marketplace_name: Some(marketplace.name),
        plugin_count: Some(marketplace.plugins.len()),
    }
}

fn refresh_error_result(id: &str, message: impl Into<String>) -> RefreshResult {
    RefreshResult {
        id: id.to_string(),
        ok: false,
        message: message.into(),
        marketplace_name: None,
        plugin_count: None,
    }
}

fn refresh_builtin_local_result(id: &str, source: &str, path: &Path) -> RefreshResult {
    match read_marketplace(path) {
        Ok(marketplace) => refresh_success_result(
            id,
            format!("Built-in marketplace source `{source}` does not require refresh"),
            marketplace,
        ),
        Err(err) => {
            tracing::warn!(
                source = source,
                path = %path.display(),
                "built-in marketplace source does not require refresh, but metadata could not be read: {err}"
            );
            refresh_error_result(
                id,
                format!(
                    "Built-in marketplace source `{source}` at `{}` is invalid: {err}",
                    path.display()
                ),
            )
        }
    }
}

fn refresh_builtin_github_result(id: &str) -> RefreshResult {
    let cache_dir = builtin_marketplace_cache_dir();
    if let Err(err) = clone_or_update_marketplace_repo(BUILTIN_GIT_URL, &cache_dir) {
        return refresh_error_result(
            id,
            format!("Unable to refresh the built-in marketplace from {BUILTIN_GIT_URL}: {err}"),
        );
    }

    let marketplace_path = cache_dir.join(MARKETPLACE_FILE_NAME);
    match read_marketplace(&marketplace_path) {
        Ok(marketplace) => refresh_success_result(
            id,
            "Built-in GitHub marketplace source refreshed",
            marketplace,
        ),
        Err(err) => refresh_error_result(
            id,
            format!(
                "invalid built-in marketplace cache `{}`: {err}",
                marketplace_path.display()
            ),
        ),
    }
}

fn refresh_builtin_marketplace_source(id: &str) -> RefreshResult {
    match builtin_marketplace_source() {
        BuiltinMarketplaceSource::Env(path) => refresh_builtin_local_result(id, "env", &path),
        BuiltinMarketplaceSource::Dev(path) => refresh_builtin_local_result(id, "dev", &path),
        BuiltinMarketplaceSource::GithubCache(_) | BuiltinMarketplaceSource::GithubRemote => {
            refresh_builtin_github_result(id)
        }
    }
}

pub fn refresh_user_marketplace_source(id: &str) -> Result<RefreshResult, String> {
    let project_root = default_url_validation_project_root();
    refresh_user_marketplace_source_with_project_root(id, &project_root)
}

pub fn refresh_user_marketplace_source_with_project_root(
    id: &str,
    project_root: &Path,
) -> Result<RefreshResult, String> {
    let id = id.trim();
    if id.is_empty() {
        return Err("marketplace source id must not be empty".to_string());
    }
    if id == "builtin" {
        return Ok(refresh_builtin_marketplace_source(id));
    }

    let source = read_config()
        .marketplaces
        .into_iter()
        .find(|source| source.id == id)
        .ok_or_else(|| format!("marketplace source `{id}` was not found"))?;

    match source.kind {
        MarketplaceSourceKind::Local => match resolve_user_local_marketplace_path(&source.location)
        {
            Ok(path) => {
                let marketplace = read_marketplace(&path)?;
                Ok(refresh_success_result(
                    id,
                    "Local marketplace source is available",
                    marketplace,
                ))
            }
            Err(err) => Ok(refresh_error_result(id, err)),
        },
        MarketplaceSourceKind::Remote => {
            let remote_url =
                match validate_remote_marketplace_url(&source.location, project_root, true) {
                    Ok(url) => url,
                    Err(err) => return Ok(refresh_error_result(id, err)),
                };
            let cache_dir = user_marketplace_cache_dir(&source.id)?;
            if let Err(err) = clone_or_update_marketplace_repo(&remote_url, &cache_dir) {
                return Ok(refresh_error_result(id, err));
            }
            let marketplace_path = cache_dir.join(MARKETPLACE_FILE_NAME);
            match read_marketplace(&marketplace_path) {
                Ok(marketplace) => Ok(refresh_success_result(
                    id,
                    "Remote marketplace source refreshed",
                    marketplace,
                )),
                Err(err) => Ok(refresh_error_result(
                    id,
                    format!(
                        "invalid remote marketplace `{}`: {err}",
                        marketplace_path.display()
                    ),
                )),
            }
        }
    }
}

pub fn remove_user_marketplace_source(id: &str) -> Result<(), String> {
    let id = id.trim();
    if id.is_empty() {
        return Err("marketplace source id must not be empty".to_string());
    }
    let mut config = read_config();
    let removed = config
        .marketplaces
        .iter()
        .find(|source| source.id == id)
        .cloned();
    let Some(removed_source) = removed.as_ref() else {
        return Err(format!("marketplace source `{id}` was not found"));
    };
    let remote_cache_dir = if removed_source.kind == MarketplaceSourceKind::Remote {
        Some(user_marketplace_cache_dir(&removed_source.id)?)
    } else {
        None
    };
    config.marketplaces.retain(|source| source.id != id);
    write_config(&config)?;
    if let Some(cache_dir) = remote_cache_dir {
        if cache_dir.exists() {
            if let Err(err) = fs::remove_dir_all(&cache_dir) {
                tracing::warn!(
                    source_id = %removed_source.id,
                    path = %cache_dir.display(),
                    "failed to remove remote marketplace cache dir: {err}"
                );
            }
        }
    }
    Ok(())
}

pub fn set_user_marketplace_source_enabled(id: &str, enabled: bool) -> Result<(), String> {
    let id = id.trim();
    if id.is_empty() {
        return Err("marketplace source id must not be empty".to_string());
    }
    let mut config = read_config();
    let Some(source) = config
        .marketplaces
        .iter_mut()
        .find(|source| source.id == id)
    else {
        return Err(format!("marketplace source `{id}` was not found"));
    };
    source.enabled = enabled;
    write_config(&config)
}

pub fn set_plugin_enabled(plugin_id: &str, enabled: bool) -> Result<(), String> {
    let plugin_id = PluginId::parse(plugin_id)?;
    let key = plugin_id.key();
    let mut config = read_config();
    if enabled {
        config.plugins.entry(key).or_default().enabled = true;
    } else if let Some(entry) = config.plugins.get_mut(&key) {
        entry.enabled = false;
    } else {
        config.plugins.insert(
            key,
            PluginConfigEntry {
                enabled: false,
                ..Default::default()
            },
        );
    }
    write_config(&config)
}

pub fn set_template_enabled(
    plugin_id: &str,
    template_id: &str,
    enabled: bool,
) -> Result<(), String> {
    let plugin_id = PluginId::parse(plugin_id)?;
    let template_id = template_id.trim();
    if template_id.is_empty() {
        return Err("template id must not be empty".to_string());
    }
    if template_id.contains('/') || template_id.contains('\\') {
        return Err("template id must not contain path separators".to_string());
    }
    let plugin_root = active_plugin_root(&plugin_id)
        .ok_or_else(|| format!("plugin `{}` is not installed", plugin_id.key()))?;
    let template_exists = crate::domain::templates::discover_template_manifest_paths(&plugin_root)
        .into_iter()
        .filter_map(|manifest_path| {
            crate::domain::templates::load_template_manifest(
                &manifest_path,
                plugin_id.key(),
                plugin_root.clone(),
            )
            .ok()
        })
        .any(|template| template.spec.metadata.id == template_id);
    if !template_exists {
        return Err(format!(
            "template `{template_id}` was not found in plugin `{}`",
            plugin_id.key()
        ));
    }

    let mut config = read_config();
    let entry = config.plugins.entry(plugin_id.key()).or_default();
    if enabled {
        entry.disabled_templates.remove(template_id);
    } else {
        entry.disabled_templates.insert(template_id.to_string());
    }
    write_config(&config)
}

pub fn set_retrieval_resource_enabled(
    plugin_id: &str,
    category: &str,
    source_id: &str,
    enabled: bool,
) -> Result<(), String> {
    let plugin_id = PluginId::parse(plugin_id)?;
    let category = normalize_id(category);
    let source_id = normalize_id(source_id);
    if category.is_empty() || source_id.is_empty() {
        return Err("retrieval resource category and id must not be empty".to_string());
    }
    let plugin_root = active_plugin_root(&plugin_id)
        .ok_or_else(|| format!("plugin `{}` is not installed", plugin_id.key()))?;
    let Some(manifest) = load_plugin_manifest(&plugin_root) else {
        return Err(format!(
            "plugin `{}` has no valid manifest",
            plugin_id.key()
        ));
    };
    let resource = manifest.retrieval.as_ref().and_then(|retrieval| {
        retrieval
            .resources
            .iter()
            .find(|source| source.category == category && source.id == source_id)
    });
    let Some(resource) = resource else {
        return Err(format!(
            "retrieval resource `{category}.{source_id}` was not found in plugin `{}`",
            plugin_id.key()
        ));
    };

    let mut config = read_config();
    let entry = config.plugins.entry(plugin_id.key()).or_default();
    materialize_retrieval_resource_config(entry, manifest.retrieval.as_ref().unwrap());
    let key = retrieval_resource_config_key(&category, &source_id);
    if enabled {
        entry.disabled_retrieval_resources.remove(&key);
        if !resource.default_enabled {
            entry.enabled_retrieval_resources.insert(key);
        }
    } else {
        entry.enabled_retrieval_resources.remove(&key);
        if resource.default_enabled {
            entry.disabled_retrieval_resources.insert(key);
        } else {
            entry.disabled_retrieval_resources.remove(&key);
        }
    }
    write_config(&config)
}

fn materialize_retrieval_resource_config(
    entry: &mut PluginConfigEntry,
    retrieval: &PluginRetrievalManifest,
) {
    if entry.retrieval_resources_configured {
        return;
    }
    if entry.enabled {
        for source in &retrieval.resources {
            if source.default_enabled {
                continue;
            }
            let key = retrieval_resource_config_key(&source.category, &source.id);
            if !entry.disabled_retrieval_resources.contains(&key) {
                entry.enabled_retrieval_resources.insert(key);
            }
        }
    }
    entry.retrieval_resources_configured = true;
}

pub fn set_environment_enabled(
    plugin_id: &str,
    environment_id: &str,
    enabled: bool,
) -> Result<(), String> {
    let plugin_id = PluginId::parse(plugin_id)?;
    let environment_id = environment_id.trim();
    if environment_id.is_empty() {
        return Err("environment id must not be empty".to_string());
    }
    if environment_id.contains('/') || environment_id.contains('\\') {
        return Err("environment id must not contain path separators".to_string());
    }
    let plugin_root = active_plugin_root(&plugin_id)
        .ok_or_else(|| format!("plugin `{}` is not installed", plugin_id.key()))?;
    let environment_exists = discover_environment_manifest_paths(&plugin_root)
        .into_iter()
        .filter_map(|manifest_path| {
            load_environment_manifest(&manifest_path, plugin_id.key(), plugin_root.clone()).ok()
        })
        .any(|environment| environment.spec.metadata.id == environment_id);
    if !environment_exists {
        return Err(format!(
            "environment `{environment_id}` was not found in plugin `{}`",
            plugin_id.key()
        ));
    }

    let mut config = read_config();
    let entry = config.plugins.entry(plugin_id.key()).or_default();
    let key = environment_config_key(environment_id);
    if enabled {
        entry.disabled_environments.remove(&key);
    } else {
        entry.disabled_environments.insert(key);
    }
    write_config(&config)
}

fn is_plugin_enabled(config: &PluginConfigFile, key: &str) -> bool {
    config
        .plugins
        .get(key)
        .map(|entry| entry.enabled)
        .unwrap_or(false)
}

fn configured_plugin_ids(config: &PluginConfigFile) -> Vec<PluginId> {
    let mut ids = config
        .plugins
        .keys()
        .filter_map(|key| match PluginId::parse(key) {
            Ok(plugin_id) => Some(plugin_id),
            Err(err) => {
                tracing::warn!(plugin = key, "ignoring invalid plugin config entry: {err}");
                None
            }
        })
        .collect::<Vec<_>>();
    ids.sort_by_key(PluginId::key);
    ids.dedup_by(|a, b| a.key() == b.key());
    ids
}

fn normalize_id(value: &str) -> String {
    value.trim().to_ascii_lowercase().replace('-', "_")
}

fn retrieval_resource_config_key(category: &str, source_id: &str) -> String {
    format!("{}.{}", normalize_id(category), normalize_id(source_id))
}

fn retrieval_resource_exposed_from_config(
    config: &PluginConfigFile,
    source_plugin: &str,
    category: &str,
    source_id: &str,
    default_enabled: bool,
) -> bool {
    let key = retrieval_resource_config_key(category, source_id);
    let Some(entry) = config.plugins.get(source_plugin) else {
        return default_enabled;
    };
    if default_enabled {
        !entry.disabled_retrieval_resources.contains(&key)
    } else if entry.enabled_retrieval_resources.contains(&key) {
        true
    } else if entry.retrieval_resources_configured {
        false
    } else if entry.enabled {
        !entry.disabled_retrieval_resources.contains(&key)
    } else {
        false
    }
}

pub(crate) fn template_expose_to_agent(
    source_plugin: &str,
    template_id: &str,
    manifest_exposed: bool,
) -> bool {
    if !manifest_exposed {
        return false;
    }
    let config = read_config();
    template_expose_to_agent_from_config(&config, source_plugin, template_id, manifest_exposed)
}

fn template_expose_to_agent_from_config(
    config: &PluginConfigFile,
    source_plugin: &str,
    template_id: &str,
    manifest_exposed: bool,
) -> bool {
    manifest_exposed
        && !config
            .plugins
            .get(source_plugin)
            .map(|entry| entry.disabled_templates.contains(template_id))
            .unwrap_or(false)
}

fn environment_config_key(environment_id: &str) -> String {
    normalize_id(environment_id)
}

fn environment_exposed_from_config(
    config: &PluginConfigFile,
    source_plugin: &str,
    environment_id: &str,
) -> bool {
    let key = environment_config_key(environment_id);
    !config
        .plugins
        .get(source_plugin)
        .map(|entry| entry.disabled_environments.contains(&key))
        .unwrap_or(false)
}

pub fn environment_profile_enabled(source_plugin: &str, environment_id: &str) -> bool {
    environment_exposed_from_config(&read_config(), source_plugin, environment_id)
}

#[cfg(test)]
fn enabled_plugin_ids(config: &PluginConfigFile) -> Vec<PluginId> {
    configured_plugin_ids(config)
        .into_iter()
        .filter(|plugin_id| is_plugin_enabled(config, &plugin_id.key()))
        .collect()
}

fn plugin_base_root_in_cache(cache_root: &Path, plugin_id: &PluginId) -> PathBuf {
    cache_root
        .join(&plugin_id.marketplace)
        .join(&plugin_id.name)
}

fn plugin_base_root_for_kind(store_root: &Path, kind: PluginKind, plugin_id: &PluginId) -> PathBuf {
    store_root.join(kind.dir_name()).join(&plugin_id.name)
}

fn typed_plugin_base_roots(store_root: &Path, plugin_id: &PluginId) -> Vec<PathBuf> {
    PluginKind::ALL
        .into_iter()
        .map(|kind| plugin_base_root_for_kind(store_root, kind, plugin_id))
        .collect()
}

fn plugin_text_matches<I, S>(values: I, needles: &[&str]) -> bool
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    values.into_iter().any(|value| {
        let value = value.as_ref().trim().to_ascii_lowercase();
        needles.iter().any(|needle| value == *needle)
    })
}

fn plugin_interface_matches(interface: Option<&PluginInterface>, needles: &[&str]) -> bool {
    let Some(interface) = interface else {
        return false;
    };
    plugin_text_matches(interface.category.as_deref(), needles)
        || plugin_text_matches(interface.capabilities.iter().map(String::as_str), needles)
}

fn plugin_kind_for_manifest(
    source_path: &Path,
    marketplace_category: Option<&str>,
    manifest: &PluginManifest,
) -> PluginKind {
    let category_matches = |needles: &[&str]| {
        plugin_text_matches(marketplace_category, needles)
            || plugin_interface_matches(manifest.interface.as_ref(), needles)
    };

    if manifest.operators.is_some()
        || source_path.join("operators").is_dir()
        || category_matches(&["operator"])
    {
        return PluginKind::Operator;
    }
    if manifest.retrieval.is_some()
        || category_matches(&[
            "resource",
            "resources",
            "source",
            "retrieval",
            "dataset",
            "literature",
            "knowledge",
            "search",
            "query",
            "fetch",
        ])
    {
        return PluginKind::Resource;
    }
    if manifest.templates.is_some() || category_matches(&["workflow", "notebook", "template"]) {
        return PluginKind::Workflow;
    }
    if manifest.skills.is_some()
        || source_path.join("skills").is_dir()
        || manifest.mcp_servers.is_some()
        || source_path.join(".mcp.json").is_file()
        || manifest.apps.is_some()
        || source_path.join(".app.json").is_file()
        || category_matches(&["tool", "tools", "skill", "mcp", "app"])
    {
        return PluginKind::Tool;
    }
    PluginKind::Other
}

fn active_plugin_root_in_base(base: &Path) -> Option<PathBuf> {
    if plugin_manifest_path(base).is_some() {
        return Some(base.to_path_buf());
    }
    let mut versions = fs::read_dir(base)
        .ok()?
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_type().ok()?.is_dir().then(|| entry.file_name()))
        .filter_map(|name| name.into_string().ok())
        .filter(|name| validate_segment(name, "plugin version").is_ok())
        .collect::<Vec<_>>();
    versions.sort();
    let version = if versions
        .iter()
        .any(|version| version == DEFAULT_PLUGIN_VERSION)
    {
        DEFAULT_PLUGIN_VERSION.to_string()
    } else {
        versions.pop()?
    };
    Some(base.join(version))
}

fn active_plugin_root_in_cache(cache_root: &Path, plugin_id: &PluginId) -> Option<PathBuf> {
    active_plugin_root_in_base(&plugin_base_root_in_cache(cache_root, plugin_id))
}

fn active_plugin_root_from_roots(cache_root: &Path, plugin_id: &PluginId) -> Option<PathBuf> {
    let store_root = plugin_store_root_from_cache_root(cache_root);
    typed_plugin_base_roots(&store_root, plugin_id)
        .into_iter()
        .find_map(|base| active_plugin_root_in_base(&base))
}

pub fn active_plugin_root(plugin_id: &PluginId) -> Option<PathBuf> {
    active_plugin_root_from_roots(&plugin_cache_root(), plugin_id)
}

pub fn active_plugin_root_by_name(plugin_name: &str) -> Option<PathBuf> {
    let plugin_name = plugin_name.trim();
    let config = read_config();
    let cache_root = plugin_cache_root();
    migrate_legacy_plugin_cache_best_effort(&cache_root);
    let mut candidates = configured_plugin_ids(&config)
        .into_iter()
        .filter(|plugin_id| plugin_id.name.as_str() == plugin_name)
        .filter(|plugin_id| active_plugin_root_from_roots(&cache_root, plugin_id).is_some())
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| {
        let left_enabled = is_plugin_enabled(&config, &left.key());
        let right_enabled = is_plugin_enabled(&config, &right.key());
        right_enabled
            .cmp(&left_enabled)
            .then_with(|| left.marketplace.cmp(&right.marketplace))
    });
    candidates
        .into_iter()
        .find_map(|plugin_id| active_plugin_root(&plugin_id))
}

fn prompt_safe_description(description: Option<&str>) -> Option<String> {
    let description = description?
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if description.is_empty() {
        None
    } else {
        Some(description.chars().take(1024).collect())
    }
}

fn plugin_retrieval_route_names(retrieval: Option<&PluginRetrievalManifest>) -> Vec<String> {
    let mut routes = retrieval
        .into_iter()
        .flat_map(|retrieval| {
            retrieval
                .resources
                .iter()
                .map(|source| format!("{}.{}", source.category, source.id))
        })
        .collect::<Vec<_>>();
    routes.sort();
    routes
}

fn plugin_retrieval_summary(
    retrieval: Option<&PluginRetrievalManifest>,
    source_plugin: &str,
    config: &PluginConfigFile,
) -> Option<PluginRetrievalSummary> {
    let retrieval = retrieval?;
    let mut resources = retrieval
        .resources
        .iter()
        .map(|source| PluginRetrievalResourceSummary {
            id: source.id.clone(),
            category: source.category.clone(),
            label: source.label.clone(),
            description: source.description.clone(),
            subcategories: source.subcategories.clone(),
            capabilities: source.capabilities.clone(),
            required_credential_refs: source.required_credential_refs.clone(),
            optional_credential_refs: source.optional_credential_refs.clone(),
            default_enabled: source.default_enabled,
            replaces_builtin: source.replaces_builtin,
            exposed: retrieval_resource_exposed_from_config(
                config,
                source_plugin,
                &source.category,
                &source.id,
                source.default_enabled,
            ),
        })
        .collect::<Vec<_>>();
    resources.sort_by(|left, right| {
        left.category
            .cmp(&right.category)
            .then_with(|| left.id.cmp(&right.id))
    });
    Some(PluginRetrievalSummary {
        protocol_version: retrieval.protocol_version,
        resources,
    })
}

fn plugin_environment_summaries(
    plugin_root: &Path,
    source_plugin: &str,
    config: &PluginConfigFile,
) -> Vec<PluginEnvironmentSummary> {
    let mut out = discover_environment_manifest_paths(plugin_root)
        .into_iter()
        .filter_map(|manifest_path| {
            load_environment_manifest(&manifest_path, source_plugin.to_string(), plugin_root)
                .ok()
                .map(environment_summary)
        })
        .map(|profile| {
            let runtime_type = profile
                .runtime
                .kind
                .as_deref()
                .unwrap_or("system")
                .trim()
                .to_ascii_lowercase();
            let (runtime_file, runtime_file_kind) = plugin_environment_runtime_file(
                &profile.manifest_path,
                &profile.runtime,
                &runtime_type,
            );
            let (availability_status, availability_manager, availability_message) =
                plugin_environment_availability(&profile.runtime, &runtime_type);
            let exposed = environment_exposed_from_config(config, source_plugin, &profile.id);
            PluginEnvironmentSummary {
                id: profile.id,
                version: profile.version,
                canonical_id: profile.canonical_id,
                name: profile.name,
                description: profile.description,
                manifest_path: profile.manifest_path,
                runtime_type,
                runtime_file,
                runtime_file_kind,
                install_hint: profile.diagnostics.install_hint,
                check_command: profile.diagnostics.check_command,
                availability_status,
                availability_manager,
                availability_message,
                exposed,
            }
        })
        .collect::<Vec<_>>();
    out.sort_by(|left, right| {
        left.runtime_type
            .cmp(&right.runtime_type)
            .then_with(|| left.id.cmp(&right.id))
    });
    out
}

fn plugin_environment_runtime_file(
    manifest_path: &str,
    runtime: &crate::domain::environments::EnvironmentRuntimeProfile,
    runtime_type: &str,
) -> (Option<String>, Option<String>) {
    let manifest = PathBuf::from(manifest_path);
    let manifest_dir = manifest.parent().map(Path::to_path_buf);
    let extra_path = |keys: &[&str]| -> Option<PathBuf> {
        keys.iter()
            .find_map(|key| runtime.extra.get(*key).and_then(JsonValue::as_str))
            .map(|raw| {
                let path = PathBuf::from(raw.trim());
                if path.is_absolute() {
                    path
                } else {
                    manifest_dir
                        .clone()
                        .unwrap_or_else(|| PathBuf::from("."))
                        .join(path)
                }
            })
    };
    let candidate = match runtime_type {
        "conda" | "mamba" | "micromamba" => extra_path(&[
            "condaEnvFile",
            "conda_env_file",
            "condaFile",
            "conda_file",
            "environmentFile",
            "environment_file",
        ])
        .or_else(|| {
            let dir = manifest_dir?;
            ["conda.yaml", "conda.yml"]
                .into_iter()
                .map(|name| dir.join(name))
                .find(|path| path.is_file())
        })
        .map(|path| (path, "conda.yaml|conda.yml".to_string())),
        "docker" => extra_path(&["dockerfile", "dockerFile"])
            .or_else(|| {
                let path = manifest_dir?.join("Dockerfile");
                path.is_file().then_some(path)
            })
            .map(|path| (path, "Dockerfile".to_string())),
        "singularity" => extra_path(&[
            "definitionFile",
            "definition_file",
            "singularityDef",
            "singularity_def",
        ])
        .or_else(|| {
            let path = manifest_dir?.join("singularity.def");
            path.is_file().then_some(path)
        })
        .map(|path| (path, "singularity.def".to_string())),
        _ => None,
    };
    match candidate {
        Some((path, kind)) => (Some(path.to_string_lossy().into_owned()), Some(kind)),
        None => (None, None),
    }
}

fn plugin_environment_availability(
    runtime: &crate::domain::environments::EnvironmentRuntimeProfile,
    runtime_type: &str,
) -> (String, Option<String>, String) {
    let result = match runtime_type {
        "conda" | "mamba" | "micromamba" => find_conda_manager(),
        "docker" => find_executable_on_path("docker").map(|path| ("docker".to_string(), path)),
        "singularity" => find_executable_on_path("singularity")
            .map(|path| ("singularity".to_string(), path))
            .or_else(|| {
                find_executable_on_path("apptainer").map(|path| ("apptainer".to_string(), path))
            }),
        "system" | "local" | "host" => runtime
            .command
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .and_then(|command| {
                find_executable_on_path(command).map(|path| (command.to_string(), path))
            }),
        _ => None,
    };
    if let Some((manager, path)) = result {
        (
            "available".to_string(),
            Some(manager.clone()),
            format!(
                "Found `{manager}` at {} in the Omiga app process PATH.",
                path.display()
            ),
        )
    } else {
        (
            "missing".to_string(),
            None,
            match runtime_type {
                "conda" | "mamba" | "micromamba" => "No micromamba, mamba, or conda executable was found in the Omiga app process PATH. Operator execution checks the selected base/virtual environment again.".to_string(),
                "docker" => "Docker CLI was not found in the Omiga app process PATH. Install Docker Desktop/Engine and ensure `docker` is available.".to_string(),
                "singularity" => "Neither singularity nor apptainer was found in the Omiga app process PATH.".to_string(),
                "system" | "local" | "host" => "Profile runtime.command was not found or not configured in PATH.".to_string(),
                other => format!("Runtime type `{other}` is not supported by plugin-level availability probing."),
            },
        )
    }
}

pub fn check_plugin_environment(
    plugin_id: &str,
    marketplace_path: Option<&Path>,
    plugin_name: Option<&str>,
    env_ref: &str,
    project_root: Option<&Path>,
) -> Result<PluginEnvironmentCheckResult, String> {
    let plugin_id = PluginId::parse(plugin_id)?;
    let installed_root = active_plugin_root(&plugin_id);
    let (plugin_root, installed) = if let Some(root) = installed_root {
        (root, true)
    } else {
        let marketplace_path = marketplace_path
            .ok_or_else(|| format!("plugin `{}` is not installed", plugin_id.key()))?;
        let plugin_name = plugin_name.unwrap_or(&plugin_id.name);
        let marketplace = read_marketplace(marketplace_path)?;
        let entry = marketplace
            .plugins
            .iter()
            .find(|entry| entry.name == plugin_name)
            .ok_or_else(|| {
                format!(
                    "plugin `{plugin_name}` not found in marketplace `{}`",
                    marketplace.name
                )
            })?;
        (
            resolve_marketplace_source_path(marketplace_path, &entry.source)?,
            false,
        )
    };

    let needle = env_ref.trim();
    if needle.is_empty() {
        return Err("environment id must not be empty".to_string());
    }
    let Some(profile) = discover_environment_manifest_paths(&plugin_root)
        .into_iter()
        .filter_map(|manifest_path| {
            load_environment_manifest(&manifest_path, plugin_id.key(), &plugin_root).ok()
        })
        .map(environment_summary)
        .find(|profile| {
            profile.id == needle
                || profile.canonical_id == needle
                || profile
                    .canonical_id
                    .rsplit('/')
                    .next()
                    .is_some_and(|tail| tail == needle)
        })
    else {
        return Err(format!(
            "environment `{needle}` was not found in plugin `{}`",
            plugin_id.key()
        ));
    };

    let runtime_type = profile
        .runtime
        .kind
        .as_deref()
        .unwrap_or("system")
        .trim()
        .to_ascii_lowercase();
    let check = if matches!(runtime_type.as_str(), "conda" | "mamba" | "micromamba") {
        check_conda_plugin_environment(&profile, project_root)?
    } else {
        check_environment_profile(&profile)
    };

    Ok(PluginEnvironmentCheckResult {
        plugin_id: plugin_id.key(),
        environment_id: profile.id,
        canonical_id: profile.canonical_id,
        installed,
        plugin_root: plugin_root.to_string_lossy().into_owned(),
        check,
    })
}

fn check_conda_plugin_environment(
    profile: &EnvironmentProfileSummary,
    project_root: Option<&Path>,
) -> Result<EnvironmentCheckResult, String> {
    let command = profile.diagnostics.check_command.clone();
    if command.is_empty() {
        return Ok(EnvironmentCheckResult {
            status: "notConfigured".to_string(),
            command,
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
            error: Some(
                "environment profile does not declare diagnostics.checkCommand".to_string(),
            ),
            duration_ms: 0,
        });
    }
    let conda_file = plugin_conda_environment_file(profile)?;
    if !is_allowed_plugin_environment_check_command(profile, &command, &conda_file) {
        return Ok(EnvironmentCheckResult {
            status: "blocked".to_string(),
            command,
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
            error: Some(
                "diagnostics.checkCommand is not in the safe plugin environment-check allowlist"
                    .to_string(),
            ),
            duration_ms: 0,
        });
    }

    let bytes = fs::read(&conda_file).map_err(|err| {
        format!(
            "Read conda environment file `{}`: {err}",
            conda_file.display()
        )
    })?;
    let env_hash = sha256_hex(&bytes);
    let env_key = format!(
        "{}-{}",
        safe_environment_component(&profile.canonical_id),
        &env_hash[..12]
    );
    let project_root = project_root
        .map(Path::to_path_buf)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| plugin_store_root()));
    let env_prefix = project_root
        .join(".omiga/operator-envs/conda")
        .join(env_key);
    let started = Instant::now();
    let script = conda_environment_check_shell_script(
        &env_prefix,
        &conda_file,
        &env_hash,
        &profile.runtime.env,
        &shell_join(&command),
    );
    match std::process::Command::new("/bin/sh")
        .arg("-lc")
        .arg(script)
        .output()
    {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout)
                .chars()
                .take(4000)
                .collect::<String>();
            let stderr = String::from_utf8_lossy(&output.stderr)
                .chars()
                .take(4000)
                .collect::<String>();
            let success = output.status.success()
                || plugin_environment_check_accepts_nonzero_version_output(
                    &command, &stdout, &stderr,
                );
            Ok(EnvironmentCheckResult {
                status: if success {
                    "available".to_string()
                } else {
                    "unavailable".to_string()
                },
                command,
                exit_code: output.status.code(),
                stdout,
                stderr,
                error: None,
                duration_ms: started.elapsed().as_millis(),
            })
        }
        Err(err) => Ok(EnvironmentCheckResult {
            status: "unavailable".to_string(),
            command,
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
            error: Some(err.to_string()),
            duration_ms: started.elapsed().as_millis(),
        }),
    }
}

fn plugin_environment_check_accepts_nonzero_version_output(
    command: &[String],
    stdout: &str,
    stderr: &str,
) -> bool {
    if !plugin_environment_check_uses_version_arg(command) {
        return false;
    }
    let combined = format!("{stdout}\n{stderr}").to_ascii_lowercase();
    combined.contains("version")
}

fn plugin_environment_check_uses_version_arg(command: &[String]) -> bool {
    let args = command
        .iter()
        .skip(1)
        .map(|value| value.trim().to_ascii_lowercase())
        .collect::<Vec<_>>();
    matches!(args.as_slice(), [arg] if matches!(arg.as_str(), "--version" | "-v" | "version"))
}

fn conda_dependency_name(raw: &str) -> Option<String> {
    let package = raw
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .rsplit("::")
        .next()
        .unwrap_or(raw)
        .split(|ch: char| ch.is_ascii_whitespace() || matches!(ch, '=' | '<' | '>' | '!' | '~'))
        .next()?
        .trim();
    (!package.is_empty()).then(|| package.to_ascii_lowercase())
}

fn conda_environment_declares_executable(env_yaml: &Path, executable: &str) -> bool {
    let wanted = executable.trim().to_ascii_lowercase();
    if wanted.is_empty() {
        return false;
    }
    let Ok(raw) = fs::read_to_string(env_yaml) else {
        return false;
    };
    let Ok(value) = serde_yaml::from_str::<serde_yaml::Value>(&raw) else {
        return false;
    };
    let Some(dependencies) = value
        .get("dependencies")
        .and_then(serde_yaml::Value::as_sequence)
    else {
        return false;
    };
    dependencies.iter().any(|dependency| {
        dependency
            .as_str()
            .and_then(conda_dependency_name)
            .is_some_and(|package| package == wanted)
    })
}

fn plugin_conda_environment_file(profile: &EnvironmentProfileSummary) -> Result<PathBuf, String> {
    let manifest = PathBuf::from(&profile.manifest_path);
    let manifest_dir = manifest.parent().ok_or_else(|| {
        format!(
            "Environment profile `{}` has no manifest parent directory.",
            profile.canonical_id
        )
    })?;
    for key in [
        "condaEnvFile",
        "conda_env_file",
        "condaFile",
        "conda_file",
        "environmentFile",
        "environment_file",
    ] {
        if let Some(raw) = profile.runtime.extra.get(key).and_then(JsonValue::as_str) {
            let path = if Path::new(raw.trim()).is_absolute() {
                PathBuf::from(raw.trim())
            } else {
                manifest_dir.join(raw.trim())
            };
            validate_plugin_conda_yaml_path(profile, &path)?;
            if !path.is_file() {
                return Err(format!(
                    "Environment profile `{}` declares conda YAML file `{}` but it does not exist.",
                    profile.canonical_id,
                    path.display()
                ));
            }
            return Ok(path);
        }
    }
    for name in ["conda.yaml", "conda.yml"] {
        let candidate = manifest_dir.join(name);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    Err(format!(
        "Environment profile `{}` does not declare or contain a standard conda YAML file. Use `runtime.condaEnvFile: ./conda.yaml` or `./conda.yml`.",
        profile.canonical_id
    ))
}

fn validate_plugin_conda_yaml_path(
    profile: &EnvironmentProfileSummary,
    path: &Path,
) -> Result<(), String> {
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase());
    if !matches!(extension.as_deref(), Some("yaml" | "yml")) {
        return Err(format!(
            "Conda/mamba environment profile `{}` must use a `.yaml` or `.yml` file; got `{}`.",
            profile.canonical_id,
            path.display()
        ));
    }
    Ok(())
}

fn conda_environment_check_shell_script(
    env_prefix: &Path,
    env_yaml: &Path,
    env_hash: &str,
    env_vars: &BTreeMap<String, String>,
    inner_command: &str,
) -> String {
    let exports = shell_export_lines(env_vars);
    format!(
        r#"set -e
OMIGA_CONDA_PREFIX={env_prefix}
OMIGA_CONDA_YAML={env_yaml}
OMIGA_CONDA_HASH={env_hash}
OMIGA_MICROMAMBA="${{OMIGA_MICROMAMBA:-$HOME/.omiga/bin/micromamba}}"
mkdir -p "$(dirname "$OMIGA_CONDA_PREFIX")"
omiga_find_conda_manager() {{
  OMIGA_CONDA_MANAGER_KIND=
  OMIGA_CONDA_BIN=
  if [ -n "${{OMIGA_MICROMAMBA:-}}" ] && [ -x "$OMIGA_MICROMAMBA" ]; then
    OMIGA_CONDA_MANAGER_KIND=micromamba
    OMIGA_CONDA_BIN=$OMIGA_MICROMAMBA
    return 0
  fi
  if command -v micromamba >/dev/null 2>&1; then
    OMIGA_CONDA_MANAGER_KIND=micromamba
    OMIGA_CONDA_BIN=$(command -v micromamba)
    return 0
  fi
  if command -v mamba >/dev/null 2>&1; then
    OMIGA_CONDA_MANAGER_KIND=mamba
    OMIGA_CONDA_BIN=$(command -v mamba)
    return 0
  fi
  if command -v conda >/dev/null 2>&1; then
    OMIGA_CONDA_MANAGER_KIND=conda
    OMIGA_CONDA_BIN=$(command -v conda)
    return 0
  fi
  return 1
}}
omiga_find_conda_manager || {{
  cat >&2 <<'OMIGA_CONDA_HINT'
No micromamba, mamba, or conda executable was found in the active PATH/base environment/virtual environment.
Recommended: install the official micromamba binary at $HOME/.omiga/bin/micromamba, or set OMIGA_MICROMAMBA=/absolute/path/to/micromamba.
OMIGA_CONDA_HINT
  exit 127
}}
if [ ! -f "$OMIGA_CONDA_PREFIX/.omiga-env-hash" ] || [ "$(cat "$OMIGA_CONDA_PREFIX/.omiga-env-hash" 2>/dev/null || true)" != "$OMIGA_CONDA_HASH" ]; then
  rm -rf "$OMIGA_CONDA_PREFIX"
  case "$OMIGA_CONDA_MANAGER_KIND" in
    micromamba)
      "$OMIGA_CONDA_BIN" create -y -p "$OMIGA_CONDA_PREFIX" -f "$OMIGA_CONDA_YAML"
      ;;
    mamba)
      "$OMIGA_CONDA_BIN" env create -y -p "$OMIGA_CONDA_PREFIX" -f "$OMIGA_CONDA_YAML" || "$OMIGA_CONDA_BIN" create -y -p "$OMIGA_CONDA_PREFIX" -f "$OMIGA_CONDA_YAML"
      ;;
    conda)
      "$OMIGA_CONDA_BIN" env create -y -p "$OMIGA_CONDA_PREFIX" -f "$OMIGA_CONDA_YAML" || "$OMIGA_CONDA_BIN" create -y -p "$OMIGA_CONDA_PREFIX" -f "$OMIGA_CONDA_YAML"
      ;;
  esac
  printf '%s' "$OMIGA_CONDA_HASH" > "$OMIGA_CONDA_PREFIX/.omiga-env-hash"
fi
{exports}
"$OMIGA_CONDA_BIN" run -p "$OMIGA_CONDA_PREFIX" /bin/sh -lc {inner}
"#,
        env_prefix = sh_quote(&env_prefix.to_string_lossy()),
        env_yaml = sh_quote(&env_yaml.to_string_lossy()),
        env_hash = sh_quote(env_hash),
        exports = exports,
        inner = sh_quote(inner_command),
    )
}

fn is_allowed_plugin_environment_check_command(
    profile: &EnvironmentProfileSummary,
    command: &[String],
    conda_file: &Path,
) -> bool {
    let Some(executable) = command.first() else {
        return false;
    };
    let basename = Path::new(executable)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(executable)
        .trim()
        .to_ascii_lowercase();
    let args = command
        .iter()
        .skip(1)
        .map(|value| value.trim().to_ascii_lowercase())
        .collect::<Vec<_>>();
    let version_arg = match args.as_slice() {
        [] => true,
        [arg] => matches!(arg.as_str(), "--version" | "-v" | "version"),
        _ => false,
    };
    let is_bare_executable = executable == &basename
        && !basename.is_empty()
        && basename
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | '+'));
    if !is_bare_executable || !version_arg {
        return false;
    }
    let runtime_command_matches = profile
        .runtime
        .command
        .as_deref()
        .map(|command| {
            Path::new(command)
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or(command)
                .trim()
                .eq_ignore_ascii_case(&basename)
        })
        .unwrap_or(false);
    runtime_command_matches || conda_environment_declares_executable(conda_file, &basename)
}

fn shell_export_lines(env: &BTreeMap<String, String>) -> String {
    env.iter()
        .filter(|(key, _)| is_safe_shell_identifier(key))
        .map(|(key, value)| format!("export {key}={}", sh_quote(value)))
        .collect::<Vec<_>>()
        .join("\n")
}

fn is_safe_shell_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first == '_' || first.is_ascii_alphabetic())
        && chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

fn shell_join(tokens: &[String]) -> String {
    tokens
        .iter()
        .map(|token| sh_quote(token))
        .collect::<Vec<_>>()
        .join(" ")
}

fn sh_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn safe_environment_component(value: &str) -> String {
    let mut out = String::new();
    for ch in value.trim().chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.') {
            out.push(ch);
        } else {
            out.push('-');
        }
    }
    let out = out.trim_matches('-').trim_matches('.');
    if out.is_empty() {
        "environment".to_string()
    } else {
        out.to_string()
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(bytes);
    format!("{digest:x}")
}

fn find_conda_manager() -> Option<(String, PathBuf)> {
    if let Ok(raw) = std::env::var("OMIGA_MICROMAMBA") {
        let path = PathBuf::from(raw.trim());
        if path.is_file() {
            return Some(("micromamba".to_string(), path));
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        let path = PathBuf::from(home).join(".omiga/bin/micromamba");
        if path.is_file() {
            return Some(("micromamba".to_string(), path));
        }
    }
    ["micromamba", "mamba", "conda"]
        .into_iter()
        .find_map(|name| find_executable_on_path(name).map(|path| (name.to_string(), path)))
}

fn find_executable_on_path(name: &str) -> Option<PathBuf> {
    let candidate = PathBuf::from(name);
    if candidate.is_absolute() && candidate.is_file() {
        return Some(candidate);
    }
    let path_var = std::env::var_os("PATH")?;
    std::env::split_paths(&path_var).find_map(|dir| {
        let path = dir.join(name);
        path.is_file().then_some(path)
    })
}

fn plugin_template_summary(
    plugin_root: &Path,
    source_plugin: &str,
    config: &PluginConfigFile,
) -> Option<PluginTemplateSummary> {
    let mut groups: BTreeMap<String, Vec<PluginTemplateItemSummary>> = BTreeMap::new();
    for manifest_path in crate::domain::templates::discover_template_manifest_paths(plugin_root) {
        let Ok(template) = crate::domain::templates::load_template_manifest(
            &manifest_path,
            source_plugin.to_string(),
            plugin_root.to_path_buf(),
        ) else {
            continue;
        };
        let category = template.spec.classification.category.clone();
        let group_id = category
            .as_deref()
            .and_then(|category| category.strip_prefix("visualization/"))
            .unwrap_or_else(|| category.as_deref().unwrap_or("templates"))
            .to_string();
        let mut tags = template.spec.metadata.tags.clone();
        tags.extend(template.spec.classification.tags.clone());
        tags.sort();
        tags.dedup();
        let canonical_id = crate::domain::templates::canonical_template_unit_id(&template);
        let execute = crate::domain::templates::template_execute_example(&template, &canonical_id);
        groups
            .entry(group_id)
            .or_default()
            .push(PluginTemplateItemSummary {
                id: template.spec.metadata.id.clone(),
                name: template
                    .spec
                    .metadata
                    .name
                    .clone()
                    .unwrap_or_else(|| template.spec.metadata.id.clone()),
                description: template.spec.metadata.description.clone(),
                category,
                tags,
                exposed: template_expose_to_agent_from_config(
                    config,
                    source_plugin,
                    &template.spec.metadata.id,
                    template.spec.exposure.expose_to_agent,
                ),
                execute,
            });
    }
    if groups.is_empty() {
        return None;
    }

    let mut group_summaries = groups
        .into_iter()
        .map(|(id, mut templates)| {
            templates.sort_by(|left, right| {
                left.name
                    .cmp(&right.name)
                    .then_with(|| left.id.cmp(&right.id))
            });
            let title = plugin_template_group_title(&id, &templates);
            PluginTemplateGroupSummary {
                id,
                title,
                count: templates.len(),
                templates,
            }
        })
        .collect::<Vec<_>>();
    group_summaries.sort_by(|left, right| {
        template_group_order(&left.id)
            .cmp(&template_group_order(&right.id))
            .then_with(|| left.title.cmp(&right.title))
    });
    let count = group_summaries.iter().map(|group| group.count).sum();
    Some(PluginTemplateSummary {
        count,
        groups: group_summaries,
    })
}

fn plugin_template_group_title(id: &str, templates: &[PluginTemplateItemSummary]) -> String {
    match id {
        "scatter"
            if templates
                .iter()
                .any(|template| template.tags.iter().any(|tag| tag == "omics-preset")) =>
        {
            "Scatter & omics presets".to_string()
        }
        "bar" => "Bar".to_string(),
        "distribution" => "Distribution".to_string(),
        "heatmap" => "Heatmap".to_string(),
        "line" => "Line".to_string(),
        "templates" => "Templates".to_string(),
        other => other
            .replace(['-', '_'], " ")
            .split_whitespace()
            .map(|word| {
                let mut chars = word.chars();
                match chars.next() {
                    Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                    None => String::new(),
                }
            })
            .collect::<Vec<_>>()
            .join(" "),
    }
}

fn template_group_order(id: &str) -> u8 {
    match id {
        "scatter" => 0,
        "distribution" => 1,
        "bar" => 2,
        "heatmap" => 3,
        "line" => 4,
        _ => 100,
    }
}

fn plugin_capability_summary_from_loaded(plugin: &LoadedPlugin) -> Option<PluginCapabilitySummary> {
    if !plugin.is_active() {
        return None;
    }

    let mut mcp_servers = plugin.mcp_servers.keys().cloned().collect::<Vec<_>>();
    mcp_servers.sort();
    let retrieval_routes = plugin_retrieval_route_names(plugin.retrieval.as_ref());
    let operators =
        crate::domain::operators::list_operator_summaries_for_plugin_root(&plugin.id, &plugin.root);
    let operator_count = operators.len();
    let operation_count = operators
        .iter()
        .map(|operator| operator.operations.len())
        .sum();
    let (template_count, template_groups) = plugin_template_capability_summary(plugin);
    let summary = PluginCapabilitySummary {
        id: plugin.id.clone(),
        display_name: plugin
            .display_name
            .clone()
            .or_else(|| plugin.manifest_name.clone())
            .unwrap_or_else(|| plugin.id.clone()),
        description: prompt_safe_description(plugin.description.as_deref()),
        has_skills: !plugin.skill_roots.is_empty(),
        mcp_servers,
        apps: plugin.apps.clone(),
        retrieval_routes,
        operator_count,
        operation_count,
        template_count,
        template_groups,
    };

    (summary.has_skills
        || !summary.mcp_servers.is_empty()
        || !summary.apps.is_empty()
        || !summary.retrieval_routes.is_empty()
        || summary.operator_count > 0
        || summary.template_count > 0)
        .then_some(summary)
}

fn plugin_template_capability_summary(plugin: &LoadedPlugin) -> (usize, Vec<String>) {
    let mut count = 0usize;
    let mut groups = BTreeSet::new();
    for manifest_path in crate::domain::templates::discover_template_manifest_paths(&plugin.root) {
        let Ok(template) = crate::domain::templates::load_template_manifest(
            &manifest_path,
            plugin.id.clone(),
            plugin.root.clone(),
        ) else {
            continue;
        };
        count += 1;
        let Some(category) = template.spec.classification.category.as_deref() else {
            continue;
        };
        let group = category
            .strip_prefix("visualization/")
            .or_else(|| category.strip_prefix("omics/"))
            .unwrap_or(category)
            .trim();
        if !group.is_empty() {
            groups.insert(group.to_string());
        }
    }
    (count, groups.into_iter().take(8).collect())
}

fn load_configured_plugin(
    configured_id: String,
    entry: &PluginConfigEntry,
    cache_root: &Path,
) -> LoadedPlugin {
    let parsed_id = PluginId::parse(&configured_id);
    let store_root = plugin_store_root_from_cache_root(cache_root);
    let root = parsed_id
        .as_ref()
        .ok()
        .and_then(|plugin_id| active_plugin_root_from_roots(cache_root, plugin_id))
        .unwrap_or_else(|| match &parsed_id {
            Ok(plugin_id) => plugin_base_root_for_kind(&store_root, PluginKind::Other, plugin_id),
            Err(_) => cache_root.to_path_buf(),
        });
    let mut loaded = LoadedPlugin {
        id: configured_id,
        manifest_name: None,
        display_name: None,
        description: None,
        root,
        enabled: entry.enabled,
        skill_roots: Vec::new(),
        mcp_servers: HashMap::new(),
        apps: Vec::new(),
        retrieval: None,
        error: None,
    };

    if !entry.enabled {
        return loaded;
    }

    let plugin_id = match parsed_id {
        Ok(plugin_id) => plugin_id,
        Err(err) => {
            loaded.error = Some(err);
            return loaded;
        }
    };
    let Some(plugin_root) = active_plugin_root_from_roots(cache_root, &plugin_id) else {
        loaded.error = Some("plugin is not installed".to_string());
        return loaded;
    };
    loaded.root = plugin_root;

    let Some(manifest) = load_plugin_manifest(&loaded.root) else {
        loaded.error = Some("missing or invalid plugin manifest".to_string());
        return loaded;
    };

    loaded.manifest_name = Some(manifest.name.clone());
    loaded.display_name = manifest
        .interface
        .as_ref()
        .and_then(|interface| interface.display_name.as_deref())
        .map(str::trim)
        .filter(|display_name| !display_name.is_empty())
        .map(str::to_string);
    loaded.description = manifest.description.clone();
    loaded.skill_roots = plugin_skill_roots_for_manifest(&loaded.root, &manifest);
    loaded.mcp_servers = plugin_mcp_servers(&loaded.root, &manifest);
    loaded.apps = plugin_app_ids(&loaded.root, &manifest);
    loaded.retrieval = manifest
        .retrieval
        .clone()
        .and_then(|retrieval| filter_retrieval_manifest_for_config(retrieval, &loaded.id, entry));
    loaded
}

fn filter_retrieval_manifest_for_config(
    mut retrieval: PluginRetrievalManifest,
    source_plugin: &str,
    entry: &PluginConfigEntry,
) -> Option<PluginRetrievalManifest> {
    retrieval.resources.retain_mut(|source| {
        let key = retrieval_resource_config_key(&source.category, &source.id);
        let exposed = if source.default_enabled {
            !entry.disabled_retrieval_resources.contains(&key)
        } else if entry.enabled_retrieval_resources.contains(&key) {
            true
        } else if entry.retrieval_resources_configured {
            false
        } else if entry.enabled {
            !entry.disabled_retrieval_resources.contains(&key)
        } else {
            false
        };
        if exposed {
            // The runtime registry receives only plugin-config-exposed resources.
            // Mark them enabled for this routing view so plugin categories do not
            // need to be hardcoded into the built-in search settings registry.
            source.default_enabled = true;
        }
        exposed
    });
    if retrieval.resources.is_empty() {
        tracing::debug!(
            plugin_id = source_plugin,
            "all plugin retrieval resources are disabled"
        );
        None
    } else {
        Some(retrieval)
    }
}

fn load_plugins_from_config(config: &PluginConfigFile, cache_root: &Path) -> PluginLoadOutcome {
    migrate_legacy_plugin_cache_best_effort(cache_root);
    let mut configured = config.plugins.iter().collect::<Vec<_>>();
    configured.sort_by(|(left, _), (right, _)| left.cmp(right));
    let plugins = configured
        .into_iter()
        .map(|(configured_id, entry)| {
            load_configured_plugin(configured_id.clone(), entry, cache_root)
        })
        .collect();
    PluginLoadOutcome::from_plugins(plugins)
}

pub fn plugin_load_outcome() -> PluginLoadOutcome {
    let config = read_config();
    load_plugins_from_config(&config, &plugin_cache_root())
}

fn read_marketplace(path: &Path) -> Result<RawMarketplaceManifest, String> {
    let raw = fs::read_to_string(path).map_err(|err| format!("read marketplace: {err}"))?;
    serde_json::from_str(&raw).map_err(|err| format!("parse marketplace: {err}"))
}

fn resolve_marketplace_source_path(
    marketplace_path: &Path,
    source: &RawMarketplacePluginSource,
) -> Result<PathBuf, String> {
    if source.source.trim().is_empty() || source.source == "local" {
        let root = marketplace_root_dir(marketplace_path);
        return resolve_safe_relative_path(&root, &source.path, "plugin source path");
    }
    Err(format!("unsupported plugin source `{}`", source.source))
}

fn plugin_summary_from_marketplace_entry(
    marketplace_path: &Path,
    marketplace_name: &str,
    entry: &RawMarketplacePlugin,
    config: &PluginConfigFile,
) -> Result<PluginSummary, String> {
    let source_path = resolve_marketplace_source_path(marketplace_path, &entry.source)?;
    let plugin_id = PluginId::new(&entry.name, marketplace_name)?;
    let key = plugin_id.key();
    let installed_path = active_plugin_root(&plugin_id);
    let contribution_root = installed_path.as_deref().unwrap_or(&source_path);
    let source_manifest = load_plugin_manifest(&source_path);
    let installed_manifest = installed_path.as_deref().and_then(load_plugin_manifest);
    let manifest = installed_manifest.as_ref().or(source_manifest.as_ref());
    let retrieval = manifest
        .and_then(|manifest| plugin_retrieval_summary(manifest.retrieval.as_ref(), &key, config));
    let interface = manifest
        .and_then(|manifest| manifest.interface.clone())
        .map(|mut interface| {
            if interface.category.is_none() {
                interface.category = entry.category.clone();
            }
            interface
        });
    let templates = plugin_template_summary(contribution_root, &key, config);
    let operators =
        crate::domain::operators::list_operator_summaries_for_plugin_root(&key, contribution_root);
    let environments = plugin_environment_summaries(contribution_root, &key, config);
    let sync = plugin_sync_summary(&source_path, installed_path.as_deref());
    let changelog = plugin_changelog_summary(&source_path, source_manifest.as_ref());
    Ok(PluginSummary {
        id: key.clone(),
        name: entry.name.clone(),
        marketplace_name: marketplace_name.to_string(),
        marketplace_path: marketplace_path.to_string_lossy().into_owned(),
        source_path: source_path.to_string_lossy().into_owned(),
        installed: installed_path.is_some(),
        installed_path: installed_path.map(|path| path.to_string_lossy().into_owned()),
        enabled: is_plugin_enabled(config, &key),
        install_policy: entry.policy.installation.clone(),
        auth_policy: entry.policy.authentication.clone(),
        interface,
        retrieval,
        operators,
        templates,
        environments,
        sync,
        changelog,
    })
}

fn plugin_summary_from_installed_root(
    plugin_id: &PluginId,
    plugin_root: &Path,
    config: &PluginConfigFile,
) -> PluginSummary {
    let manifest = load_plugin_manifest(plugin_root);
    let key = plugin_id.key();
    let retrieval = manifest
        .as_ref()
        .and_then(|manifest| plugin_retrieval_summary(manifest.retrieval.as_ref(), &key, config));
    let templates = plugin_template_summary(plugin_root, &key, config);
    let operators =
        crate::domain::operators::list_operator_summaries_for_plugin_root(&key, plugin_root);
    let environments = plugin_environment_summaries(plugin_root, &key, config);
    let changelog = plugin_changelog_summary(plugin_root, manifest.as_ref());
    let marketplace_path = plugin_root
        .parent()
        .unwrap_or(plugin_root)
        .to_string_lossy()
        .into_owned();
    PluginSummary {
        id: key.clone(),
        name: plugin_id.name.clone(),
        marketplace_name: plugin_id.marketplace.clone(),
        marketplace_path,
        source_path: plugin_root.to_string_lossy().into_owned(),
        installed_path: Some(plugin_root.to_string_lossy().into_owned()),
        installed: true,
        enabled: is_plugin_enabled(config, &key),
        install_policy: PluginInstallPolicy::Available,
        auth_policy: PluginAuthPolicy::OnUse,
        interface: manifest.and_then(|manifest| manifest.interface),
        retrieval,
        operators,
        templates,
        environments,
        sync: None,
        changelog,
    }
}

fn cached_plugin_ids_in_cache(cache_root: &Path) -> Vec<PluginId> {
    let mut ids = Vec::new();
    let Ok(marketplaces) = fs::read_dir(cache_root) else {
        return ids;
    };
    for marketplace in marketplaces.flatten() {
        let Ok(file_type) = marketplace.file_type() else {
            continue;
        };
        if !file_type.is_dir() {
            continue;
        }
        let Ok(marketplace_name) = marketplace.file_name().into_string() else {
            continue;
        };
        let Ok(plugins) = fs::read_dir(marketplace.path()) else {
            continue;
        };
        for plugin in plugins.flatten() {
            let Ok(file_type) = plugin.file_type() else {
                continue;
            };
            if !file_type.is_dir() {
                continue;
            }
            let Ok(plugin_name) = plugin.file_name().into_string() else {
                continue;
            };
            if let Ok(plugin_id) = PluginId::new(&plugin_name, &marketplace_name) {
                ids.push(plugin_id);
            }
        }
    }
    ids.sort_by_key(PluginId::key);
    ids.dedup_by(|a, b| a.key() == b.key());
    ids
}

fn migrate_legacy_plugin_cache(cache_root: &Path) -> Result<usize, String> {
    let store_root = plugin_store_root_from_cache_root(cache_root);
    let mut migrated = 0;
    for plugin_id in cached_plugin_ids_in_cache(cache_root) {
        let legacy_base = plugin_base_root_in_cache(cache_root, &plugin_id);
        let Some(legacy_root) = active_plugin_root_in_cache(cache_root, &plugin_id) else {
            continue;
        };
        let Some(manifest) = load_plugin_manifest(&legacy_root) else {
            continue;
        };
        let kind = plugin_kind_for_manifest(&legacy_root, None, &manifest);
        let target = plugin_base_root_for_kind(&store_root, kind, &plugin_id);
        if target.exists() {
            remove_path_if_exists(&legacy_base)?;
            continue;
        }
        let parent = target
            .parent()
            .ok_or_else(|| format!("plugin install path has no parent: {}", target.display()))?;
        fs::create_dir_all(parent).map_err(|err| format!("create plugin install dir: {err}"))?;
        fs::rename(&legacy_root, &target).map_err(|err| {
            format!(
                "move legacy plugin `{}` from {} to {}: {err}",
                plugin_id.key(),
                legacy_root.display(),
                target.display()
            )
        })?;
        remove_path_if_exists(&legacy_base)?;
        migrated += 1;
    }
    Ok(migrated)
}

fn migrate_legacy_plugin_cache_best_effort(cache_root: &Path) {
    match migrate_legacy_plugin_cache(cache_root) {
        Ok(migrated) if migrated > 0 => {
            tracing::info!(count = migrated, "migrated legacy plugin cache entries")
        }
        Ok(_) => {}
        Err(err) => tracing::warn!("failed to migrate legacy plugin cache entries: {err}"),
    }
}

fn refresh_configured_builtin_plugins(
    config: &PluginConfigFile,
    cache_root: &Path,
) -> Result<usize, String> {
    let Some(marketplace_path) = builtin_marketplace_path() else {
        return Ok(0);
    };
    let marketplace = read_marketplace(&marketplace_path)?;
    let store_root = plugin_store_root_from_cache_root(cache_root);
    let mut refreshed = 0;

    for entry in &marketplace.plugins {
        let plugin_id = PluginId::new(&entry.name, &marketplace.name)?;
        if !config.plugins.contains_key(&plugin_id.key()) {
            continue;
        }
        if entry.policy.installation == PluginInstallPolicy::NotAvailable {
            continue;
        }
        let source_path = resolve_marketplace_source_path(&marketplace_path, &entry.source)?;
        let Some(manifest) = load_plugin_manifest(&source_path) else {
            continue;
        };
        if manifest.name != entry.name {
            continue;
        }
        let kind = plugin_kind_for_manifest(&source_path, entry.category.as_deref(), &manifest);
        let target_base = plugin_base_root_for_kind(&store_root, kind, &plugin_id);
        if migrate_stale_typed_plugin_root(&store_root, &plugin_id, &target_base)? {
            refreshed += 1;
        }
    }

    Ok(refreshed)
}

fn migrate_stale_typed_plugin_root(
    store_root: &Path,
    plugin_id: &PluginId,
    target_base: &Path,
) -> Result<bool, String> {
    // This is a typed-root migration only. It intentionally moves the user's
    // existing installed plugin tree instead of copying marketplace source
    // content, so read/list paths never bypass explicit sync conflict checks.
    if target_base.exists() {
        return Ok(false);
    }
    let Some(stale_base) = typed_plugin_base_roots(store_root, plugin_id)
        .into_iter()
        .filter(|candidate| candidate != target_base && candidate.exists())
        .find(|candidate| active_plugin_root_in_base(candidate).is_some())
    else {
        return Ok(false);
    };
    let parent = target_base.parent().ok_or_else(|| {
        format!(
            "plugin install path has no parent: {}",
            target_base.display()
        )
    })?;
    fs::create_dir_all(parent).map_err(|err| format!("create plugin install dir: {err}"))?;
    fs::rename(&stale_base, target_base).map_err(|err| {
        format!(
            "move configured plugin `{}` from {} to {}: {err}",
            plugin_id.key(),
            stale_base.display(),
            target_base.display()
        )
    })?;
    Ok(true)
}

fn unlisted_installed_plugin_summaries(
    config: &PluginConfigFile,
    listed_ids: &HashSet<String>,
    cache_root: &Path,
) -> Vec<PluginSummary> {
    migrate_legacy_plugin_cache_best_effort(cache_root);
    configured_plugin_ids(config)
        .into_iter()
        .filter_map(|plugin_id| {
            let key = plugin_id.key();
            if listed_ids.contains(&key) {
                return None;
            }
            let plugin_root = active_plugin_root_from_roots(cache_root, &plugin_id)?;
            Some(plugin_summary_from_installed_root(
                &plugin_id,
                &plugin_root,
                config,
            ))
        })
        .collect()
}

pub fn list_plugin_marketplaces(
    project_root: Option<&Path>,
    resource_dir: Option<&Path>,
) -> Vec<PluginMarketplaceEntry> {
    let cache_root = plugin_cache_root();
    migrate_legacy_plugin_cache_best_effort(&cache_root);
    let config = read_config();
    let mut out = Vec::new();
    let mut listed_ids = HashSet::new();
    for path in marketplace_paths(project_root, resource_dir) {
        let marketplace = match read_marketplace(&path) {
            Ok(marketplace) => marketplace,
            Err(err) => {
                tracing::warn!(path = %path.display(), "skipping plugin marketplace: {err}");
                continue;
            }
        };
        let mut plugins = Vec::new();
        for entry in &marketplace.plugins {
            match plugin_summary_from_marketplace_entry(&path, &marketplace.name, entry, &config) {
                Ok(summary) => {
                    if !listed_ids.insert(summary.id.clone()) {
                        continue;
                    }
                    plugins.push(summary);
                }
                Err(err) => {
                    tracing::warn!(path = %path.display(), plugin = entry.name, "skipping plugin: {err}")
                }
            }
        }
        if plugins.is_empty() && marketplace.remote.is_none() {
            continue;
        }
        out.push(PluginMarketplaceEntry {
            name: marketplace.name,
            path: path.to_string_lossy().into_owned(),
            interface: marketplace.interface,
            remote: marketplace.remote,
            plugins,
        });
    }
    let installed_plugins = unlisted_installed_plugin_summaries(&config, &listed_ids, &cache_root);
    if !installed_plugins.is_empty() {
        out.push(PluginMarketplaceEntry {
            name: "installed-plugins".to_string(),
            path: plugin_store_root_from_cache_root(&cache_root)
                .to_string_lossy()
                .into_owned(),
            interface: Some(MarketplaceInterface {
                display_name: Some("Installed plugins".to_string()),
            }),
            remote: None,
            plugins: installed_plugins,
        });
    }
    out
}

fn marketplace_raw_digest(path: &Path) -> Option<String> {
    fs::read(path)
        .ok()
        .map(|bytes| format!("sha256:{}", sha256_hex(&bytes)))
}

fn marketplace_entry_versions(
    marketplace_path: &Path,
    marketplace: &RawMarketplaceManifest,
) -> BTreeMap<String, Option<String>> {
    let mut versions = BTreeMap::new();
    for entry in &marketplace.plugins {
        let version = entry.version.clone().or_else(|| {
            resolve_marketplace_source_path(marketplace_path, &entry.source)
                .ok()
                .and_then(|source_path| load_plugin_manifest(&source_path))
                .and_then(|manifest| manifest.version)
        });
        versions.insert(entry.name.clone(), version);
    }
    versions
}

fn remote_marketplace_versions(
    marketplace: &RawMarketplaceManifest,
) -> BTreeMap<String, Option<String>> {
    marketplace
        .plugins
        .iter()
        .map(|entry| (entry.name.clone(), entry.version.clone()))
        .collect()
}

fn changed_marketplace_plugins(
    local_versions: &BTreeMap<String, Option<String>>,
    remote_versions: &BTreeMap<String, Option<String>>,
) -> Vec<String> {
    let mut names = BTreeSet::new();
    names.extend(local_versions.keys().cloned());
    names.extend(remote_versions.keys().cloned());
    names
        .into_iter()
        .filter(
            |name| match (local_versions.get(name), remote_versions.get(name)) {
                (None, Some(_)) | (Some(_), None) => true,
                (Some(local), Some(remote)) => match (local, remote) {
                    (Some(local), Some(remote)) => local != remote,
                    (None, Some(_)) => true,
                    // A remote marketplace without per-plugin versions can still
                    // signal manifest-level changes via digest; do not mark every
                    // existing plugin as changed just because the remote omitted
                    // optional version metadata.
                    _ => false,
                },
                (None, None) => false,
            },
        )
        .collect()
}

fn marketplace_check_error(
    path: &Path,
    marketplace: RawMarketplaceManifest,
    remote: MarketplaceRemote,
    local_digest: Option<String>,
    message: String,
) -> MarketplaceRemoteCheckResult {
    MarketplaceRemoteCheckResult {
        name: marketplace.name,
        path: path.to_string_lossy().into_owned(),
        remote,
        state: "error".to_string(),
        label: "Remote check failed".to_string(),
        message,
        local_digest,
        remote_digest: None,
        remote_plugin_count: None,
        changed_plugins: Vec::new(),
        checked_at: chrono::Utc::now().to_rfc3339(),
    }
}

async fn check_one_remote_marketplace(
    path: &Path,
    marketplace: RawMarketplaceManifest,
    project_root: &Path,
    client: &reqwest::Client,
) -> MarketplaceRemoteCheckResult {
    let Some(remote) = marketplace.remote.clone() else {
        unreachable!("caller filters marketplaces without remote metadata");
    };
    let local_digest = marketplace_raw_digest(path);
    if let Err(err) =
        crate::domain::tools::web_safety::validate_public_http_url(project_root, &remote.url, true)
    {
        return marketplace_check_error(
            path,
            marketplace,
            remote,
            local_digest,
            format!("Remote marketplace URL is not allowed: {err}"),
        );
    }

    let response = match client
        .get(&remote.url)
        .header(reqwest::header::USER_AGENT, "Omiga")
        .header(
            reqwest::header::ACCEPT,
            "application/json,text/plain;q=0.9,*/*;q=0.1",
        )
        .send()
        .await
    {
        Ok(response) => response,
        Err(err) => {
            return marketplace_check_error(
                path,
                marketplace,
                remote,
                local_digest,
                format!("Remote marketplace request failed: {err}"),
            );
        }
    };
    let status = response.status();
    if !status.is_success() {
        return marketplace_check_error(
            path,
            marketplace,
            remote,
            local_digest,
            format!("Remote marketplace returned HTTP {status}."),
        );
    }
    if let Some(length) = response.content_length() {
        if length as usize > MAX_REMOTE_MARKETPLACE_BYTES {
            return marketplace_check_error(
                path,
                marketplace,
                remote,
                local_digest,
                format!("Remote marketplace is too large: {length} bytes."),
            );
        }
    }
    let bytes = match response.bytes().await {
        Ok(bytes) => bytes,
        Err(err) => {
            return marketplace_check_error(
                path,
                marketplace,
                remote,
                local_digest,
                format!("Read remote marketplace body failed: {err}"),
            );
        }
    };
    if bytes.len() > MAX_REMOTE_MARKETPLACE_BYTES {
        return marketplace_check_error(
            path,
            marketplace,
            remote,
            local_digest,
            format!("Remote marketplace is too large: {} bytes.", bytes.len()),
        );
    }
    let remote_digest = format!("sha256:{}", sha256_hex(&bytes));
    let remote_marketplace = match serde_json::from_slice::<RawMarketplaceManifest>(&bytes) {
        Ok(remote_marketplace) => remote_marketplace,
        Err(err) => {
            return marketplace_check_error(
                path,
                marketplace,
                remote,
                local_digest,
                format!("Parse remote marketplace failed: {err}"),
            );
        }
    };
    if remote_marketplace.name != marketplace.name {
        let local_name = marketplace.name.clone();
        return MarketplaceRemoteCheckResult {
            name: local_name.clone(),
            path: path.to_string_lossy().into_owned(),
            remote,
            state: "error".to_string(),
            label: "Remote mismatch".to_string(),
            message: format!(
                "Remote marketplace name `{}` does not match local `{}`.",
                remote_marketplace.name, local_name
            ),
            local_digest,
            remote_digest: Some(remote_digest),
            remote_plugin_count: Some(remote_marketplace.plugins.len()),
            changed_plugins: Vec::new(),
            checked_at: chrono::Utc::now().to_rfc3339(),
        };
    }

    let changed_plugins = changed_marketplace_plugins(
        &marketplace_entry_versions(path, &marketplace),
        &remote_marketplace_versions(&remote_marketplace),
    );
    let digest_changed = local_digest
        .as_ref()
        .map(|digest| digest != &remote_digest)
        .unwrap_or(true);
    let update_available = digest_changed || !changed_plugins.is_empty();
    let (state, label, message) = if update_available {
        (
            "updateAvailable",
            "Remote update available",
            if changed_plugins.is_empty() {
                "Remote marketplace manifest differs from the local copy.".to_string()
            } else {
                format!(
                    "Remote marketplace differs for {} plugin{}.",
                    changed_plugins.len(),
                    if changed_plugins.len() == 1 { "" } else { "s" }
                )
            },
        )
    } else {
        (
            "upToDate",
            "Remote up to date",
            "Remote marketplace manifest matches the local copy.".to_string(),
        )
    };

    MarketplaceRemoteCheckResult {
        name: marketplace.name,
        path: path.to_string_lossy().into_owned(),
        remote,
        state: state.to_string(),
        label: label.to_string(),
        message,
        local_digest,
        remote_digest: Some(remote_digest),
        remote_plugin_count: Some(remote_marketplace.plugins.len()),
        changed_plugins,
        checked_at: chrono::Utc::now().to_rfc3339(),
    }
}

pub async fn check_remote_plugin_marketplaces(
    project_root: Option<&Path>,
    resource_dir: Option<&Path>,
) -> Vec<MarketplaceRemoteCheckResult> {
    let policy_root = project_root
        .map(Path::to_path_buf)
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
    {
        Ok(client) => client,
        Err(err) => {
            tracing::warn!("failed to create remote marketplace client: {err}");
            return Vec::new();
        }
    };
    let mut out = Vec::new();
    for path in marketplace_paths(project_root, resource_dir) {
        let marketplace = match read_marketplace(&path) {
            Ok(marketplace) => marketplace,
            Err(err) => {
                tracing::warn!(path = %path.display(), "skipping remote marketplace check: {err}");
                continue;
            }
        };
        if marketplace.remote.is_none() {
            continue;
        }
        out.push(check_one_remote_marketplace(&path, marketplace, &policy_root, &client).await);
    }
    out
}

pub fn read_plugin(marketplace_path: &Path, plugin_name: &str) -> Result<PluginDetail, String> {
    migrate_legacy_plugin_cache_best_effort(&plugin_cache_root());
    let marketplace = read_marketplace(marketplace_path)?;
    let config = read_config();
    let entry = marketplace
        .plugins
        .iter()
        .find(|entry| entry.name == plugin_name)
        .ok_or_else(|| format!("plugin `{plugin_name}` not found in `{}`", marketplace.name))?;
    let summary =
        plugin_summary_from_marketplace_entry(marketplace_path, &marketplace.name, entry, &config)?;
    let source_path = PathBuf::from(&summary.source_path);
    let manifest = load_plugin_manifest(&source_path)
        .ok_or_else(|| "missing or invalid plugin manifest".to_string())?;
    Ok(PluginDetail {
        summary,
        description: manifest.description.clone(),
        changelog: plugin_changelog_summary(&source_path, Some(&manifest)),
        skills: plugin_skill_summaries(&source_path, &manifest),
        mcp_servers: plugin_mcp_server_names(&source_path, &manifest),
        apps: plugin_app_ids(&source_path, &manifest),
    })
}

fn plugin_skill_roots_for_manifest(plugin_root: &Path, manifest: &PluginManifest) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    let default = plugin_root.join("skills");
    if default.is_dir() {
        roots.push(default);
    }
    if let Some(path) = &manifest.skills {
        if path.is_dir() && !roots.contains(path) {
            roots.push(path.clone());
        }
    }
    roots
}

pub fn enabled_plugin_skill_roots() -> Vec<PathBuf> {
    plugin_load_outcome().effective_skill_roots()
}

fn plugin_skill_summaries(
    plugin_root: &Path,
    manifest: &PluginManifest,
) -> Vec<PluginSkillSummary> {
    let mut out = Vec::new();
    for root in plugin_skill_roots_for_manifest(plugin_root, manifest) {
        collect_skill_summaries_from_root(&root, &mut out);
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out.dedup_by(|a, b| a.name == b.name && a.path == b.path);
    out
}

fn collect_skill_summaries_from_root(root: &Path, out: &mut Vec<PluginSkillSummary>) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let skill_md = path.join("SKILL.md");
        if skill_md.is_file() {
            out.push(skill_summary_from_dir(&path));
            continue;
        }
        let Ok(children) = fs::read_dir(&path) else {
            continue;
        };
        for child in children.flatten() {
            let child_path = child.path();
            if child_path.is_dir() && child_path.join("SKILL.md").is_file() {
                out.push(skill_summary_from_dir(&child_path));
            }
        }
    }
}

fn skill_summary_from_dir(skill_dir: &Path) -> PluginSkillSummary {
    let fallback = skill_dir
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("skill")
        .to_string();
    let raw = fs::read_to_string(skill_dir.join("SKILL.md")).unwrap_or_default();
    let (name, description) = parse_skill_frontmatter_name_description(&raw, &fallback);
    PluginSkillSummary {
        name,
        description,
        path: skill_dir.join("SKILL.md").to_string_lossy().into_owned(),
    }
}

fn parse_skill_frontmatter_name_description(raw: &str, fallback: &str) -> (String, String) {
    if !raw.starts_with("---") {
        return (fallback.to_string(), String::new());
    }
    let Some(rest) = raw.strip_prefix("---") else {
        return (fallback.to_string(), String::new());
    };
    let Some((frontmatter, _)) = rest.split_once("---") else {
        return (fallback.to_string(), String::new());
    };
    let parsed = serde_yaml::from_str::<serde_yaml::Value>(frontmatter).ok();
    let name = parsed
        .as_ref()
        .and_then(|v| v.get("name"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(fallback)
        .to_string();
    let description = parsed
        .as_ref()
        .and_then(|v| v.get("description"))
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    (name, description)
}

fn plugin_mcp_config_path(plugin_root: &Path, manifest: &PluginManifest) -> Option<PathBuf> {
    if let Some(path) = &manifest.mcp_servers {
        return path.is_file().then(|| path.clone());
    }
    let default = plugin_root.join(".mcp.json");
    default.is_file().then_some(default)
}

fn plugin_mcp_server_names(plugin_root: &Path, manifest: &PluginManifest) -> Vec<String> {
    let mut names = plugin_mcp_servers(plugin_root, manifest)
        .into_keys()
        .collect::<Vec<_>>();
    names.sort();
    names
}

fn plugin_mcp_servers(
    plugin_root: &Path,
    manifest: &PluginManifest,
) -> HashMap<String, McpServerConfig> {
    let Some(path) = plugin_mcp_config_path(plugin_root, manifest) else {
        return HashMap::new();
    };
    let Ok(raw) = fs::read_to_string(path) else {
        return HashMap::new();
    };
    servers_from_mcp_json(&raw)
        .into_iter()
        .map(|(name, config)| (name, rebase_plugin_mcp_server(plugin_root, config)))
        .collect()
}

fn rebase_plugin_mcp_server(plugin_root: &Path, config: McpServerConfig) -> McpServerConfig {
    match config {
        McpServerConfig::Stdio {
            command,
            args,
            env,
            cwd,
        } => {
            let cwd = Some(resolve_plugin_stdio_cwd(plugin_root, cwd.as_deref()));
            McpServerConfig::Stdio {
                command,
                args,
                env,
                cwd,
            }
        }
        other => other,
    }
}

fn resolve_plugin_stdio_cwd(plugin_root: &Path, cwd: Option<&str>) -> String {
    let Some(raw) = cwd.map(str::trim).filter(|value| !value.is_empty()) else {
        return plugin_root.to_string_lossy().into_owned();
    };

    if let Some(rest) = raw.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest).to_string_lossy().into_owned();
        }
    }

    let path = PathBuf::from(raw);
    if path.is_absolute() {
        path.to_string_lossy().into_owned()
    } else if raw == "." {
        plugin_root.to_string_lossy().into_owned()
    } else {
        let relative = raw.strip_prefix("./").unwrap_or(raw);
        plugin_root.join(relative).to_string_lossy().into_owned()
    }
}

pub fn enabled_plugin_mcp_servers() -> HashMap<String, McpServerConfig> {
    plugin_load_outcome().effective_mcp_servers()
}

pub fn enabled_plugin_retrieval_plugins() -> Vec<PluginRetrievalRegistration> {
    plugin_load_outcome().effective_retrieval_plugins()
}

pub fn enabled_plugin_retrieval_statuses() -> Vec<PluginLifecycleRouteStatus> {
    plugin_retrieval_statuses_for_registrations(
        &enabled_plugin_retrieval_plugins(),
        &PluginLifecycleState::global(),
        Instant::now(),
    )
}

fn plugin_retrieval_statuses_for_registrations(
    registrations: &[PluginRetrievalRegistration],
    lifecycle: &PluginLifecycleState,
    now: Instant,
) -> Vec<PluginLifecycleRouteStatus> {
    lifecycle.route_statuses(
        registrations.iter().flat_map(|registration| {
            registration.retrieval.resources.iter().map(|source| {
                PluginLifecycleKey::new(
                    registration.plugin_id.clone(),
                    source.category.clone(),
                    source.id.clone(),
                )
            })
        }),
        now,
    )
}

pub fn enabled_plugin_apps() -> Vec<String> {
    plugin_load_outcome().effective_apps()
}

fn plugin_app_config_path(plugin_root: &Path, manifest: &PluginManifest) -> Option<PathBuf> {
    if let Some(path) = &manifest.apps {
        return path.is_file().then(|| path.clone());
    }
    let default = plugin_root.join(".app.json");
    default.is_file().then_some(default)
}

fn plugin_app_ids(plugin_root: &Path, manifest: &PluginManifest) -> Vec<String> {
    let Some(path) = plugin_app_config_path(plugin_root, manifest) else {
        return Vec::new();
    };
    let Ok(raw) = fs::read_to_string(path) else {
        return Vec::new();
    };
    let Ok(value) = serde_json::from_str::<JsonValue>(&raw) else {
        return Vec::new();
    };
    let mut out = value
        .get("apps")
        .and_then(JsonValue::as_object)
        .map(|apps| {
            apps.values()
                .filter_map(|app| app.get("id").and_then(JsonValue::as_str))
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    out.sort();
    out.dedup();
    out
}

fn copy_dir_recursive(source: &Path, target: &Path) -> Result<(), String> {
    fs::create_dir_all(target).map_err(|err| format!("create plugin target dir: {err}"))?;
    for entry in fs::read_dir(source).map_err(|err| format!("read plugin source dir: {err}"))? {
        let entry = entry.map_err(|err| format!("enumerate plugin source: {err}"))?;
        let source_path = entry.path();
        let target_path = target.join(entry.file_name());
        let file_type = entry
            .file_type()
            .map_err(|err| format!("inspect plugin source entry: {err}"))?;
        if file_type.is_dir() {
            copy_dir_recursive(&source_path, &target_path)?;
        } else if file_type.is_file() {
            fs::copy(&source_path, &target_path)
                .map_err(|err| format!("copy plugin file: {err}"))?;
        }
    }
    Ok(())
}

fn plugin_install_state_path(plugin_root: &Path) -> PathBuf {
    plugin_root.join(PLUGIN_INSTALL_STATE_RELATIVE_PATH)
}

fn plugin_relative_path(root: &Path, path: &Path) -> Result<String, String> {
    let rel = path
        .strip_prefix(root)
        .map_err(|err| format!("derive plugin relative path: {err}"))?;
    Ok(rel
        .components()
        .filter_map(|component| match component {
            Component::Normal(part) => Some(part.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/"))
}

fn plugin_sync_internal_path(relative: &str) -> bool {
    relative == PLUGIN_INSTALL_STATE_RELATIVE_PATH
        || relative.starts_with(&format!("{PLUGIN_SYNC_CONFLICTS_RELATIVE_DIR}/"))
}

fn plugin_file_hashes(plugin_root: &Path) -> Result<BTreeMap<String, String>, String> {
    let mut files = BTreeMap::new();
    if !plugin_root.is_dir() {
        return Ok(files);
    }
    for entry in walkdir::WalkDir::new(plugin_root)
        .follow_links(false)
        .into_iter()
    {
        let entry = entry.map_err(|err| format!("walk plugin files: {err}"))?;
        if !entry.file_type().is_file() {
            continue;
        }
        let relative = plugin_relative_path(plugin_root, entry.path())?;
        if relative.is_empty() || plugin_sync_internal_path(&relative) {
            continue;
        }
        let bytes = fs::read(entry.path()).map_err(|err| {
            format!(
                "read plugin file `{}` for digest: {err}",
                entry.path().display()
            )
        })?;
        files.insert(relative, format!("sha256:{}", sha256_hex(&bytes)));
    }
    Ok(files)
}

fn plugin_tree_digest(files: &BTreeMap<String, String>) -> String {
    let mut hasher = Sha256::new();
    for (relative, hash) in files {
        hasher.update(relative.as_bytes());
        hasher.update([0]);
        hasher.update(hash.as_bytes());
        hasher.update([0]);
    }
    format!("sha256:{:x}", hasher.finalize())
}

fn read_plugin_install_state(plugin_root: &Path) -> Option<PluginInstallState> {
    fs::read_to_string(plugin_install_state_path(plugin_root))
        .ok()
        .and_then(|raw| serde_json::from_str::<PluginInstallState>(&raw).ok())
}

fn write_plugin_install_state(
    plugin_root: &Path,
    state: &PluginInstallState,
) -> Result<(), String> {
    let path = plugin_install_state_path(plugin_root);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| format!("create plugin state dir: {err}"))?;
    }
    let raw = serde_json::to_string_pretty(state)
        .map_err(|err| format!("serialize plugin install state: {err}"))?;
    fs::write(&path, format!("{raw}\n")).map_err(|err| format!("write plugin state: {err}"))
}

fn record_plugin_install_state(
    plugin_root: &Path,
    plugin_id: &PluginId,
    version: Option<String>,
    installed_at: Option<String>,
) -> Result<PluginInstallState, String> {
    let files = plugin_file_hashes(plugin_root)?;
    let now = chrono::Utc::now().to_rfc3339();
    let installed_at = installed_at.unwrap_or_else(|| now.clone());
    let state = PluginInstallState {
        schema_version: plugin_install_state_schema_version(),
        plugin_id: plugin_id.key(),
        installed_from_version: version,
        installed_from_digest: plugin_tree_digest(&files),
        installed_at,
        last_synced_at: now,
        files,
    };
    write_plugin_install_state(plugin_root, &state)?;
    Ok(state)
}

#[derive(Debug, Clone, Default)]
struct PluginSyncPlan {
    updated: Vec<String>,
    added: Vec<String>,
    removed: Vec<String>,
    kept_local: Vec<String>,
    conflicts: Vec<String>,
}

impl PluginSyncPlan {
    fn changed_count(&self) -> usize {
        self.updated.len() + self.added.len() + self.removed.len()
    }

    fn local_modified_count(&self) -> usize {
        self.kept_local.len() + self.conflicts.len()
    }
}

fn plugin_sync_plan(
    base_files: Option<&BTreeMap<String, String>>,
    current_files: &BTreeMap<String, String>,
    source_files: &BTreeMap<String, String>,
) -> PluginSyncPlan {
    let mut plan = PluginSyncPlan::default();
    let mut paths = BTreeSet::new();
    paths.extend(current_files.keys().cloned());
    paths.extend(source_files.keys().cloned());
    if let Some(base_files) = base_files {
        paths.extend(base_files.keys().cloned());
    }

    for path in paths {
        let base = base_files.and_then(|files| files.get(&path));
        let current = current_files.get(&path);
        let source = source_files.get(&path);

        if current == source {
            continue;
        }

        match base {
            Some(base_hash) => {
                if current == Some(base_hash) {
                    match source {
                        Some(_) => plan.updated.push(path),
                        None => plan.removed.push(path),
                    }
                } else if source == Some(base_hash) {
                    plan.kept_local.push(path);
                } else {
                    plan.conflicts.push(path);
                }
            }
            None => match (current, source) {
                (None, Some(_)) => plan.added.push(path),
                (Some(_), None) => plan.kept_local.push(path),
                (Some(_), Some(_)) => plan.conflicts.push(path),
                (None, None) => {}
            },
        }
    }

    plan
}

fn plugin_force_sync_plan(
    current_files: &BTreeMap<String, String>,
    source_files: &BTreeMap<String, String>,
) -> PluginSyncPlan {
    let mut plan = PluginSyncPlan::default();
    let mut paths = BTreeSet::new();
    paths.extend(current_files.keys().cloned());
    paths.extend(source_files.keys().cloned());
    for path in paths {
        match (current_files.get(&path), source_files.get(&path)) {
            (Some(current), Some(source)) if current != source => plan.updated.push(path),
            (None, Some(_)) => plan.added.push(path),
            (Some(_), None) => plan.removed.push(path),
            _ => {}
        }
    }
    plan
}

fn plugin_sync_summary(
    source_path: &Path,
    installed_path: Option<&Path>,
) -> Option<PluginSyncSummary> {
    let installed_path = installed_path?;
    let source_files = plugin_file_hashes(source_path).ok()?;
    let current_files = plugin_file_hashes(installed_path).ok()?;
    let source_digest = plugin_tree_digest(&source_files);
    let installed_digest = plugin_tree_digest(&current_files);
    let state = read_plugin_install_state(installed_path);
    let base_files = state.as_ref().map(|state| &state.files);
    let plan = plugin_sync_plan(base_files, &current_files, &source_files);
    let installed_from_digest = state
        .as_ref()
        .map(|state| state.installed_from_digest.clone());
    let upstream_changed = installed_from_digest
        .as_ref()
        .map(|digest| digest != &source_digest)
        .unwrap_or(installed_digest != source_digest);
    let local_modified = state
        .as_ref()
        .map(|state| state.files != current_files)
        .unwrap_or(installed_digest != source_digest);
    let (state_name, label, message) = if plan.conflicts.is_empty()
        && plan.changed_count() == 0
        && !local_modified
    {
        (
            "upToDate",
            "Up to date",
            "Installed plugin files match the marketplace source.",
        )
    } else if state.is_none() && !upstream_changed && !local_modified {
        (
            "unknown",
            "Track sync",
            "Installed plugin matches the marketplace source but has no install-state snapshot yet.",
        )
    } else if plan.conflicts.is_empty() && upstream_changed && !local_modified {
        (
            "updateAvailable",
            "Update available",
            "Marketplace source changed; safe sync can update the user copy.",
        )
    } else if plan.conflicts.is_empty() && !upstream_changed && local_modified {
        (
            "localModified",
            "Local edits",
            "User plugin files differ from the last installed snapshot.",
        )
    } else if plan.conflicts.is_empty() {
        (
            "updateAvailable",
            "Sync available",
            "Safe sync can apply non-conflicting marketplace changes.",
        )
    } else {
        (
            "conflictRisk",
            "Review sync",
            "Marketplace and user plugin files changed in overlapping paths; safe sync will keep local files.",
        )
    };

    Some(PluginSyncSummary {
        state: state_name.to_string(),
        label: label.to_string(),
        message: message.to_string(),
        source_digest: Some(source_digest),
        installed_digest: Some(installed_digest),
        installed_from_digest,
        changed_count: plan.changed_count(),
        local_modified_count: plan.local_modified_count(),
        conflict_count: plan.conflicts.len(),
    })
}

fn copy_plugin_relative_file(
    source_root: &Path,
    target_root: &Path,
    relative: &str,
) -> Result<(), String> {
    let source = source_root.join(relative);
    let target = target_root.join(relative);
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|err| format!("create plugin sync dir: {err}"))?;
    }
    fs::copy(&source, &target)
        .map(|_| ())
        .map_err(|err| format!("copy synced plugin file `{relative}`: {err}"))
}

fn remove_plugin_relative_file(target_root: &Path, relative: &str) -> Result<(), String> {
    let target = target_root.join(relative);
    if !target.exists() {
        return Ok(());
    }
    fs::remove_file(&target).map_err(|err| format!("remove synced plugin file `{relative}`: {err}"))
}

fn copy_marketplace_resource_runner_assets(
    marketplace_path: &Path,
    _marketplace_name: &str,
    cache_root: &Path,
) -> Result<bool, String> {
    let marketplace_root = marketplace_root_dir(marketplace_path);
    let canonical_source = marketplace_root.join(RESOURCE_RUNNERS_DIR);
    let legacy_source = marketplace_root.join(LEGACY_SOURCE_RUNNERS_DIR);
    let source = if canonical_source.is_dir() {
        canonical_source
    } else {
        legacy_source
    };
    if !source.is_dir() {
        return Ok(false);
    }
    let target = plugin_store_root_from_cache_root(cache_root)
        .join(PluginKind::Resource.dir_name())
        .join(RESOURCE_RUNNERS_DIR);
    copy_dir_recursive(&source, &target)?;
    let legacy_target = plugin_store_root_from_cache_root(cache_root)
        .join(PluginKind::Resource.dir_name())
        .join(LEGACY_SOURCE_RUNNERS_DIR);
    copy_dir_recursive(&source, &legacy_target)?;
    Ok(true)
}

fn copy_marketplace_nested_resource_utils(
    marketplace_path: &Path,
    cache_root: &Path,
) -> Result<bool, String> {
    let marketplace_root = marketplace_root_dir(marketplace_path);
    let source = marketplace_root
        .join("plugins")
        .join(PluginKind::Resource.dir_name())
        .join(RESOURCE_UTILS_DIR);
    if !source.is_dir() {
        return Ok(false);
    }
    let target = plugin_store_root_from_cache_root(cache_root)
        .join(PluginKind::Resource.dir_name())
        .join(RESOURCE_UTILS_DIR);
    copy_dir_recursive(&source, &target)?;
    Ok(true)
}

fn copy_marketplace_shared_resource_assets(
    marketplace_path: &Path,
    marketplace_name: &str,
    cache_root: &Path,
) -> Result<bool, String> {
    let copied_legacy =
        copy_marketplace_resource_runner_assets(marketplace_path, marketplace_name, cache_root)?;
    let copied_utils = copy_marketplace_nested_resource_utils(marketplace_path, cache_root)?;
    Ok(copied_legacy || copied_utils)
}

fn remove_path_if_exists(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(format!("remove {}: {err}", path.display())),
    }
}

fn prepare_plugin_root_install(
    source: &Path,
    target_base: &Path,
    plugin_id: &PluginId,
    version: Option<String>,
    installed_at: Option<String>,
) -> Result<PathBuf, String> {
    let parent = target_base.parent().ok_or_else(|| {
        format!(
            "plugin install path has no parent: {}",
            target_base.display()
        )
    })?;
    fs::create_dir_all(parent).map_err(|err| format!("create plugin install dir: {err}"))?;
    let staged_base = parent.join(format!(".install-{}", uuid::Uuid::new_v4()));
    copy_dir_recursive(source, &staged_base)?;
    record_plugin_install_state(&staged_base, plugin_id, version, installed_at).inspect_err(
        |_| {
            let _ = remove_path_if_exists(&staged_base);
        },
    )?;
    Ok(staged_base)
}

fn activate_staged_plugin_root(
    staged_base: &Path,
    target_base: &Path,
) -> Result<Option<PathBuf>, String> {
    let parent = target_base.parent().ok_or_else(|| {
        format!(
            "plugin install path has no parent: {}",
            target_base.display()
        )
    })?;
    let backup_base = parent.join(format!(".install-backup-{}", uuid::Uuid::new_v4()));
    if target_base.exists() {
        fs::rename(target_base, &backup_base).map_err(|err| {
            let _ = remove_path_if_exists(&staged_base);
            format!(
                "stage existing plugin install {} for replacement: {err}",
                target_base.display()
            )
        })?;
    }
    if let Err(err) = fs::rename(&staged_base, target_base) {
        let rollback = if backup_base.exists() {
            match fs::rename(&backup_base, target_base) {
                Ok(()) => "existing install restored".to_string(),
                Err(rollback_err) => format!("restore failed: {rollback_err}"),
            }
        } else {
            "no existing install to restore".to_string()
        };
        let _ = remove_path_if_exists(&staged_base);
        return Err(format!("activate plugin install entry: {err}; {rollback}"));
    }
    Ok(backup_base.exists().then_some(backup_base))
}

fn rollback_activated_plugin_root(
    target_base: &Path,
    backup_base: Option<&Path>,
) -> Result<(), String> {
    remove_path_if_exists(target_base)?;
    if let Some(backup_base) = backup_base {
        if backup_base.exists() {
            fs::rename(backup_base, target_base)
                .map_err(|err| format!("restore plugin install backup: {err}"))?;
        }
    }
    Ok(())
}

fn cleanup_plugin_root_backup_best_effort(backup_base: Option<PathBuf>) {
    let Some(backup_base) = backup_base else {
        return;
    };
    if let Err(err) = remove_path_if_exists(&backup_base) {
        tracing::warn!(
            path = %backup_base.display(),
            "failed to remove plugin install backup after successful activation: {err}"
        );
    }
}

fn remove_other_typed_plugin_roots_best_effort(
    store_root: &Path,
    plugin_id: &PluginId,
    keep_base: &Path,
) {
    if let Err(err) = remove_other_typed_plugin_roots(store_root, plugin_id, keep_base) {
        tracing::warn!(
            plugin_id = %plugin_id.key(),
            keep = %keep_base.display(),
            "failed to remove stale typed plugin roots after activation: {err}"
        );
    }
}

fn remove_other_typed_plugin_roots(
    store_root: &Path,
    plugin_id: &PluginId,
    keep_base: &Path,
) -> Result<(), String> {
    for candidate in typed_plugin_base_roots(store_root, plugin_id) {
        if candidate == keep_base {
            continue;
        }
        remove_path_if_exists(&candidate)?;
    }
    Ok(())
}

pub fn install_plugin(
    marketplace_path: &Path,
    plugin_name: &str,
) -> Result<PluginInstallResult, String> {
    let marketplace = read_marketplace(marketplace_path)?;
    let entry = marketplace
        .plugins
        .iter()
        .find(|entry| entry.name == plugin_name)
        .ok_or_else(|| format!("plugin `{plugin_name}` not found in `{}`", marketplace.name))?;
    if entry.policy.installation == PluginInstallPolicy::NotAvailable {
        return Err(format!(
            "plugin `{plugin_name}` is not available for install"
        ));
    }
    let source_path = resolve_marketplace_source_path(marketplace_path, &entry.source)?;
    if !source_path.is_dir() {
        return Err(format!(
            "plugin source path is not a directory: {}",
            source_path.display()
        ));
    }
    let manifest = load_plugin_manifest(&source_path)
        .ok_or_else(|| "missing or invalid plugin manifest".to_string())?;
    if manifest.name != entry.name {
        return Err(format!(
            "plugin manifest name `{}` does not match marketplace plugin name `{}`",
            manifest.name, entry.name
        ));
    }
    let plugin_id = PluginId::new(&entry.name, &marketplace.name)?;
    let kind = plugin_kind_for_manifest(&source_path, entry.category.as_deref(), &manifest);
    let store_root = plugin_store_root();
    let target_base = plugin_base_root_for_kind(&store_root, kind, &plugin_id);
    copy_marketplace_shared_resource_assets(
        marketplace_path,
        &marketplace.name,
        &plugin_cache_root(),
    )?;
    let staged_path = prepare_plugin_root_install(
        &source_path,
        &target_base,
        &plugin_id,
        manifest.version.clone(),
        None,
    )?;
    let backup_path = activate_staged_plugin_root(&staged_path, &target_base)?;
    if let Err(err) = set_plugin_enabled(&plugin_id.key(), true) {
        let _ = rollback_activated_plugin_root(&target_base, backup_path.as_deref());
        return Err(err);
    }
    cleanup_plugin_root_backup_best_effort(backup_path);
    remove_other_typed_plugin_roots_best_effort(&store_root, &plugin_id, &target_base);
    Ok(PluginInstallResult {
        plugin_id: plugin_id.key(),
        installed_path: target_base.to_string_lossy().into_owned(),
        auth_policy: entry.policy.authentication.clone(),
    })
}

pub fn sync_plugin(
    plugin_id: &str,
    marketplace_path: &Path,
    plugin_name: Option<&str>,
    force: bool,
) -> Result<PluginSyncResult, String> {
    let plugin_id = PluginId::parse(plugin_id)?;
    let marketplace = read_marketplace(marketplace_path)?;
    if marketplace.name != plugin_id.marketplace {
        return Err(format!(
            "plugin `{}` belongs to marketplace `{}`, not `{}`",
            plugin_id.key(),
            plugin_id.marketplace,
            marketplace.name
        ));
    }
    let plugin_name = plugin_name
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or(&plugin_id.name);
    let entry = marketplace
        .plugins
        .iter()
        .find(|entry| entry.name == plugin_name)
        .ok_or_else(|| format!("plugin `{plugin_name}` not found in `{}`", marketplace.name))?;
    if entry.name != plugin_id.name {
        return Err(format!(
            "plugin id `{}` does not match marketplace entry `{}`",
            plugin_id.name, entry.name
        ));
    }
    let source_path = resolve_marketplace_source_path(marketplace_path, &entry.source)?;
    if !source_path.is_dir() {
        return Err(format!(
            "plugin source path is not a directory: {}",
            source_path.display()
        ));
    }
    let manifest = load_plugin_manifest(&source_path)
        .ok_or_else(|| "missing or invalid plugin manifest".to_string())?;
    if manifest.name != entry.name {
        return Err(format!(
            "plugin manifest name `{}` does not match marketplace plugin name `{}`",
            manifest.name, entry.name
        ));
    }
    let installed_path = active_plugin_root(&plugin_id)
        .ok_or_else(|| format!("plugin `{}` is not installed", plugin_id.key()))?;

    let source_files = plugin_file_hashes(&source_path)?;
    let current_files = plugin_file_hashes(&installed_path)?;
    let install_state = read_plugin_install_state(&installed_path);
    let base_files = install_state.as_ref().map(|state| &state.files);
    let plan = plugin_sync_plan(base_files, &current_files, &source_files);

    if force {
        let force_plan = plugin_force_sync_plan(&current_files, &source_files);
        let kind = plugin_kind_for_manifest(&source_path, entry.category.as_deref(), &manifest);
        let store_root = plugin_store_root();
        let target_base = plugin_base_root_for_kind(&store_root, kind, &plugin_id);
        copy_marketplace_shared_resource_assets(
            marketplace_path,
            &marketplace.name,
            &plugin_cache_root(),
        )?;
        let installed_at = install_state
            .as_ref()
            .map(|state| state.installed_at.clone());
        let staged_path = prepare_plugin_root_install(
            &source_path,
            &target_base,
            &plugin_id,
            manifest.version.clone(),
            installed_at,
        )?;
        let backup_path = activate_staged_plugin_root(&staged_path, &target_base)?;
        cleanup_plugin_root_backup_best_effort(backup_path);
        remove_other_typed_plugin_roots_best_effort(&store_root, &plugin_id, &target_base);
        return Ok(PluginSyncResult {
            plugin_id: plugin_id.key(),
            status: "forceSynced".to_string(),
            installed_path: target_base.to_string_lossy().into_owned(),
            updated: force_plan.updated,
            added: force_plan.added,
            removed: force_plan.removed,
            kept_local: Vec::new(),
            conflicts: Vec::new(),
            message: "Force synced plugin from marketplace source; local edits were overwritten."
                .to_string(),
        });
    }

    for relative in plan.updated.iter().chain(plan.added.iter()) {
        copy_plugin_relative_file(&source_path, &installed_path, relative)?;
    }
    for relative in &plan.removed {
        remove_plugin_relative_file(&installed_path, relative)?;
    }
    copy_marketplace_shared_resource_assets(
        marketplace_path,
        &marketplace.name,
        &plugin_cache_root(),
    )?;

    let conflicts = plan.conflicts.clone();
    let kept_local = plan.kept_local.clone();
    let updated = plan.updated.clone();
    let added = plan.added.clone();
    let removed = plan.removed.clone();
    let status = if conflicts.is_empty() {
        let installed_at = install_state
            .as_ref()
            .map(|state| state.installed_at.clone());
        record_plugin_install_state(
            &installed_path,
            &plugin_id,
            manifest.version.clone(),
            installed_at,
        )?;
        if updated.is_empty() && added.is_empty() && removed.is_empty() {
            "upToDate"
        } else {
            "synced"
        }
    } else if updated.is_empty() && added.is_empty() && removed.is_empty() {
        "conflicts"
    } else {
        "partial"
    }
    .to_string();
    let message = if status == "upToDate" {
        "Plugin is already up to date.".to_string()
    } else if status == "synced" {
        format!(
            "Synced plugin: {} updated, {} added, {} removed.",
            updated.len(),
            added.len(),
            removed.len()
        )
    } else if status == "partial" {
        format!(
            "Partially synced plugin; {} conflict{} kept local.",
            conflicts.len(),
            if conflicts.len() == 1 { "" } else { "s" }
        )
    } else {
        format!(
            "No files were changed because {} conflict{} need review.",
            conflicts.len(),
            if conflicts.len() == 1 { "" } else { "s" }
        )
    };

    Ok(PluginSyncResult {
        plugin_id: plugin_id.key(),
        status,
        installed_path: installed_path.to_string_lossy().into_owned(),
        updated,
        added,
        removed,
        kept_local,
        conflicts,
        message,
    })
}

pub fn uninstall_plugin(plugin_id: &str) -> Result<(), String> {
    let plugin_id = PluginId::parse(plugin_id)?;
    for target in typed_plugin_base_roots(&plugin_store_root(), &plugin_id) {
        remove_path_if_exists(&target)?;
    }
    remove_path_if_exists(&plugin_base_root_in_cache(&plugin_cache_root(), &plugin_id))?;
    let mut config = read_config();
    config.plugins.remove(&plugin_id.key());
    write_config(&config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::{OsStr, OsString};

    static PLUGIN_HOME_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    struct ScopedEnv {
        key: &'static str,
        old: Option<OsString>,
    }

    impl ScopedEnv {
        fn set(key: &'static str, value: impl AsRef<OsStr>) -> Self {
            let old = std::env::var_os(key);
            std::env::set_var(key, value);
            Self { key, old }
        }

        fn remove(key: &'static str) -> Self {
            let old = std::env::var_os(key);
            std::env::remove_var(key);
            Self { key, old }
        }
    }

    impl Drop for ScopedEnv {
        fn drop(&mut self) {
            match self.old.take() {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }

    fn curated_marketplace_path() -> PathBuf {
        dev_builtin_marketplace_path()
    }

    #[test]
    fn dev_curated_marketplace_uses_external_omiga_plugins_repo() {
        let marketplace_path = dev_builtin_marketplace_path();
        let expected_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root")
            .parent()
            .expect("workspace root")
            .join("omiga-plugins");

        assert_eq!(
            marketplace_path,
            expected_root.join(MARKETPLACE_FILE_NAME),
            "dev curated marketplace must come from the independent omiga-plugins repository"
        );
        assert!(marketplace_path.is_file());
    }

    #[test]
    fn default_marketplace_paths_use_only_external_omiga_plugins_repo() {
        let _guard = PLUGIN_HOME_ENV_LOCK.lock().expect("plugin home env lock");
        let tmp = tempfile::tempdir().expect("tempdir");
        let _home = ScopedEnv::set("HOME", tmp.path());
        let _user_profile = ScopedEnv::set("USERPROFILE", tmp.path());
        let _plugins_dir = ScopedEnv::remove("OMIGA_PLUGINS_DIR");

        let paths = marketplace_paths(None, None);
        assert_eq!(
            paths,
            vec![dev_builtin_marketplace_path()],
            "Omiga must not add app-local .omiga, src-tauri fixtures, packaged resources, project plugin marketplaces, or absent user sources"
        );
    }

    #[test]
    fn packaged_resource_paths_do_not_add_embedded_marketplaces() {
        let _guard = PLUGIN_HOME_ENV_LOCK.lock().expect("plugin home env lock");
        let tmp = tempfile::tempdir().expect("tempdir");
        let _home = ScopedEnv::set("HOME", tmp.path().join("home"));
        let _user_profile = ScopedEnv::set("USERPROFILE", tmp.path().join("home"));
        let _plugins_dir = ScopedEnv::remove("OMIGA_PLUGINS_DIR");
        let resource_dir = tmp.path();
        let curated = resource_dir
            .join("omiga-plugins")
            .join(MARKETPLACE_FILE_NAME);
        let internal = resource_dir
            .join("embedded_plugins")
            .join(MARKETPLACE_FILE_NAME);
        fs::create_dir_all(curated.parent().unwrap()).unwrap();
        fs::create_dir_all(internal.parent().unwrap()).unwrap();
        fs::write(
            &curated,
            r#"{"name":"omiga-curated","plugins":[],"remote":{"url":"https://example.com/marketplace.json"}}"#,
        )
        .unwrap();
        fs::write(&internal, r#"{"name":"omiga-internal","plugins":[]}"#).unwrap();

        let paths = marketplace_paths(None, Some(resource_dir));
        assert_eq!(
            paths,
            vec![dev_builtin_marketplace_path()],
            "resource-dir plugin copies must not become marketplace sources"
        );
    }

    fn write_empty_marketplace(root: &Path, name: &str) -> PathBuf {
        fs::create_dir_all(root).expect("marketplace root");
        let path = root.join(MARKETPLACE_FILE_NAME);
        fs::write(&path, format!(r#"{{"name":"{name}","plugins":[]}}"#))
            .expect("marketplace manifest");
        path
    }

    #[test]
    fn resolve_builtin_marketplace_path_env_override_wins() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let env_path = write_empty_marketplace(&tmp.path().join("env"), "env-marketplace");
        let sibling_path =
            write_empty_marketplace(&tmp.path().join("sibling"), "sibling-marketplace");
        let cache_path = write_empty_marketplace(&tmp.path().join("cache"), "cache-marketplace");

        assert_eq!(
            resolve_builtin_marketplace_path(
                Some(env_path.parent().expect("env parent").to_path_buf()),
                Some(sibling_path),
                cache_path
            ),
            Some(env_path)
        );
    }

    #[test]
    fn resolve_builtin_marketplace_path_sibling_over_cache() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let sibling_path =
            write_empty_marketplace(&tmp.path().join("sibling"), "sibling-marketplace");
        let cache_path = write_empty_marketplace(&tmp.path().join("cache"), "cache-marketplace");

        assert_eq!(
            resolve_builtin_marketplace_path(None, Some(sibling_path.clone()), cache_path),
            Some(sibling_path)
        );
    }

    #[test]
    fn resolve_builtin_marketplace_path_cache_fallback() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let cache_path = write_empty_marketplace(&tmp.path().join("cache"), "cache-marketplace");

        assert_eq!(
            resolve_builtin_marketplace_path(None, None, cache_path.clone()),
            Some(cache_path)
        );
    }

    #[test]
    fn resolve_builtin_marketplace_path_none_when_all_absent() {
        let tmp = tempfile::tempdir().expect("tempdir");

        assert_eq!(
            resolve_builtin_marketplace_path(
                Some(tmp.path().join("missing-env")),
                Some(
                    tmp.path()
                        .join("missing-sibling")
                        .join(MARKETPLACE_FILE_NAME)
                ),
                tmp.path().join("missing-cache").join(MARKETPLACE_FILE_NAME)
            ),
            None
        );
    }

    #[test]
    fn ensure_builtin_marketplace_reports_malformed_env_override() {
        let _guard = PLUGIN_HOME_ENV_LOCK.lock().expect("plugin home env lock");
        let tmp = tempfile::tempdir().expect("tempdir");
        let home = tmp.path().join("home");
        let marketplace_root = tmp.path().join("env-marketplace");
        fs::create_dir_all(&marketplace_root).expect("marketplace root");
        fs::write(
            marketplace_root.join(MARKETPLACE_FILE_NAME),
            "{not valid json",
        )
        .expect("malformed marketplace");
        let _home = ScopedEnv::set("HOME", &home);
        let _user_profile = ScopedEnv::set("USERPROFILE", &home);
        let _plugins_dir = ScopedEnv::set("OMIGA_PLUGINS_DIR", &marketplace_root);

        let status = ensure_builtin_marketplace().expect("built-in status");

        assert!(!status.ok, "malformed env marketplace must be unhealthy");
        assert_eq!(status.source, "env");
        assert!(
            status.message.contains("parse marketplace"),
            "unexpected status message: {}",
            status.message
        );
    }

    #[test]
    fn refresh_builtin_local_result_reports_malformed_dev_source() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let marketplace_path = tmp.path().join(MARKETPLACE_FILE_NAME);
        fs::write(&marketplace_path, "{not valid json").expect("malformed marketplace");

        let result = refresh_builtin_local_result("builtin", "dev", &marketplace_path);

        assert!(!result.ok, "malformed dev marketplace refresh must fail");
        assert!(
            result.message.contains("parse marketplace"),
            "unexpected refresh message: {}",
            result.message
        );
        assert_eq!(result.marketplace_name, None);
        assert_eq!(result.plugin_count, None);
    }

    fn git_available() -> bool {
        Command::new("git").arg("--version").output().is_ok()
    }

    fn run_git(repo: &Path, args: &[&str]) {
        let status = Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(args)
            .status()
            .expect("run git command");
        assert!(status.success(), "git command failed: {args:?}");
    }

    fn commit_marketplace(repo: &Path, name: &str, message: &str) {
        fs::write(
            repo.join(MARKETPLACE_FILE_NAME),
            format!(r#"{{"name":"{name}","plugins":[]}}"#),
        )
        .expect("marketplace manifest");
        run_git(repo, &["add", MARKETPLACE_FILE_NAME]);
        run_git(
            repo,
            &[
                "-c",
                "user.name=Omiga Tests",
                "-c",
                "user.email=omiga-tests@example.invalid",
                "commit",
                "-m",
                message,
            ],
        );
    }

    fn write_superseding_marketplace(
        root: &Path,
        marketplace_name: &str,
        plugin_name: &str,
        superseded_plugin_name: &str,
        category: &str,
    ) -> PathBuf {
        let plugin_root = root.join("plugins").join(plugin_name);
        fs::create_dir_all(&plugin_root).expect("plugin root");
        fs::write(
            plugin_root.join(PLUGIN_MANIFEST_FILE),
            format!(
                r#"{{
                  "name":"{plugin_name}",
                  "version":"0.1.0",
                  "compatibility": {{ "supersedesPlugins": ["{superseded_plugin_name}"] }},
                  "interface": {{ "category":"{category}" }}
                }}"#
            ),
        )
        .expect("plugin manifest");
        fs::write(
            root.join(MARKETPLACE_FILE_NAME),
            format!(
                r#"{{
                  "name":"{marketplace_name}",
                  "plugins":[
                    {{
                      "name":"{plugin_name}",
                      "source":{{"source":"local","path":"./plugins/{plugin_name}"}},
                      "policy":{{"installation":"AVAILABLE","authentication":"ON_USE"}},
                      "category":"{category}"
                    }}
                  ]
                }}"#
            ),
        )
        .expect("marketplace manifest");
        root.join(MARKETPLACE_FILE_NAME)
    }

    #[test]
    fn clone_or_update_marketplace_repo_clones_and_pulls_local_git_fixture() {
        if !git_available() {
            eprintln!("skipping clone_or_update_marketplace_repo test: git is not available");
            return;
        }

        let tmp = tempfile::tempdir().expect("tempdir");
        let source = tmp.path().join("source");
        let cache = tmp.path().join("cache");
        fs::create_dir_all(&source).expect("source repo");
        run_git(&source, &["init"]);
        commit_marketplace(&source, "git-marketplace", "initial marketplace");

        let source_url = reqwest::Url::from_directory_path(
            fs::canonicalize(&source).expect("canonical source repo"),
        )
        .expect("file url")
        .to_string();
        clone_or_update_marketplace_repo(&source_url, &cache).expect("clone marketplace");
        assert_eq!(
            read_marketplace(&cache.join(MARKETPLACE_FILE_NAME))
                .expect("cloned marketplace")
                .name,
            "git-marketplace"
        );

        commit_marketplace(&source, "git-marketplace-updated", "update marketplace");
        clone_or_update_marketplace_repo(&source_url, &cache).expect("pull marketplace");
        assert_eq!(
            read_marketplace(&cache.join(MARKETPLACE_FILE_NAME))
                .expect("updated marketplace")
                .name,
            "git-marketplace-updated"
        );
    }

    #[test]
    fn clone_or_update_marketplace_repo_reclones_non_git_cache_dir() {
        if !git_available() {
            eprintln!(
                "skipping clone_or_update_marketplace_repo_reclones_non_git_cache_dir test: git is not available"
            );
            return;
        }

        let tmp = tempfile::tempdir().expect("tempdir");
        let source = tmp.path().join("source");
        let cache = tmp.path().join("cache");
        fs::create_dir_all(&source).expect("source repo");
        run_git(&source, &["init"]);
        commit_marketplace(&source, "recovered-marketplace", "initial marketplace");
        fs::create_dir_all(&cache).expect("partial cache dir");
        fs::write(cache.join("partial.txt"), "left by failed clone").expect("partial cache file");

        let source_url = reqwest::Url::from_directory_path(
            fs::canonicalize(&source).expect("canonical source repo"),
        )
        .expect("file url")
        .to_string();
        clone_or_update_marketplace_repo(&source_url, &cache)
            .expect("reclone marketplace from non-git cache");

        assert!(cache.join(".git").exists());
        assert!(!cache.join("partial.txt").exists());
        assert_eq!(
            read_marketplace(&cache.join(MARKETPLACE_FILE_NAME))
                .expect("recloned marketplace")
                .name,
            "recovered-marketplace"
        );
    }

    #[test]
    fn ensure_builtin_marketplace_uses_github_cache() {
        let _guard = PLUGIN_HOME_ENV_LOCK.lock().expect("plugin home env lock");
        if !git_available() {
            eprintln!(
                "skipping ensure_builtin_marketplace_uses_github_cache test: git is not available"
            );
            return;
        }

        let tmp = tempfile::tempdir().expect("tempdir");
        let home = tmp.path().join("home");
        let _home = ScopedEnv::set("HOME", &home);
        let _user_profile = ScopedEnv::set("USERPROFILE", &home);
        let _plugins_dir = ScopedEnv::remove("OMIGA_PLUGINS_DIR");
        let source = tmp.path().join("source");
        fs::create_dir_all(&source).expect("source repo");
        run_git(&source, &["init"]);
        commit_marketplace(&source, "github-cache-marketplace", "initial marketplace");

        let source_url = reqwest::Url::from_directory_path(
            fs::canonicalize(&source).expect("canonical source repo"),
        )
        .expect("file url")
        .to_string();
        let cache_dir = builtin_marketplace_cache_dir();
        clone_or_update_marketplace_repo(&source_url, &cache_dir).expect("clone marketplace");

        let marketplace_path = cache_dir.join(MARKETPLACE_FILE_NAME);
        assert!(marketplace_path.is_file());
        assert_eq!(
            read_marketplace(&marketplace_path)
                .expect("cached marketplace")
                .name,
            "github-cache-marketplace"
        );
        assert_eq!(
            resolve_builtin_marketplace_path(None, None, marketplace_path.clone()),
            Some(marketplace_path.clone())
        );

        if dev_builtin_marketplace_path().is_file() {
            assert_eq!(
                builtin_marketplace_path(),
                Some(dev_builtin_marketplace_path()),
                "the dev sibling must continue to outrank the GitHub cache in development"
            );
        } else {
            assert_eq!(builtin_marketplace_path(), Some(marketplace_path.clone()));
            let status = ensure_builtin_marketplace().expect("ensure built-in marketplace");
            assert!(status.ok, "unexpected ensure status: {status:?}");
            assert_eq!(status.source, "github");
            let expected_path = path_to_string(&marketplace_path);
            assert_eq!(status.path.as_deref(), Some(expected_path.as_str()));
        }
    }

    #[test]
    fn enabled_local_user_marketplace_source_is_added_after_dev_path() {
        let _guard = PLUGIN_HOME_ENV_LOCK.lock().expect("plugin home env lock");
        let tmp = tempfile::tempdir().expect("tempdir");
        let home = tmp.path().join("home");
        let _home = ScopedEnv::set("HOME", &home);
        let _user_profile = ScopedEnv::set("USERPROFILE", &home);
        let marketplace_path =
            write_empty_marketplace(&tmp.path().join("local-marketplace"), "local-marketplace");

        let source = add_user_marketplace_source(
            MarketplaceSourceKind::Local,
            marketplace_path
                .parent()
                .expect("marketplace parent")
                .to_string_lossy()
                .into_owned(),
            Some("Local Marketplace".to_string()),
        )
        .expect("add source");

        assert_eq!(source.kind, MarketplaceSourceKind::Local);
        assert!(source.enabled);
        assert_eq!(source.label.as_deref(), Some("Local Marketplace"));
        assert_eq!(
            marketplace_paths(None, None),
            vec![
                dev_builtin_marketplace_path(),
                fs::canonicalize(&marketplace_path).expect("canonical marketplace path")
            ]
        );
    }

    #[test]
    fn test_list_marketplace_source_views_builtin_first() {
        let _guard = PLUGIN_HOME_ENV_LOCK.lock().expect("plugin home env lock");
        let tmp = tempfile::tempdir().expect("tempdir");
        let home = tmp.path().join("home");
        let _home = ScopedEnv::set("HOME", &home);
        let _user_profile = ScopedEnv::set("USERPROFILE", &home);
        let marketplace_path =
            write_empty_marketplace(&tmp.path().join("local-marketplace"), "local-marketplace");

        let source = add_user_marketplace_source(
            MarketplaceSourceKind::Local,
            marketplace_path.to_string_lossy().into_owned(),
            Some("Local Marketplace".to_string()),
        )
        .expect("add source");

        let views = list_marketplace_source_views();
        assert!(
            views.len() >= 2,
            "expected built-in source plus configured user source"
        );
        assert_eq!(views[0].id, "builtin");
        assert!(!views[0].removable);
        assert!(views[0].enabled);
        assert_eq!(views[1].id, source.id);
        assert!(views[1].removable);
    }

    #[test]
    fn disabled_local_user_marketplace_sources_are_excluded_from_paths() {
        let _guard = PLUGIN_HOME_ENV_LOCK.lock().expect("plugin home env lock");
        let tmp = tempfile::tempdir().expect("tempdir");
        let home = tmp.path().join("home");
        let _home = ScopedEnv::set("HOME", &home);
        let _user_profile = ScopedEnv::set("USERPROFILE", &home);
        let marketplace_path =
            write_empty_marketplace(&tmp.path().join("local-marketplace"), "local-marketplace");
        let source = add_user_marketplace_source(
            MarketplaceSourceKind::Local,
            marketplace_path.to_string_lossy().into_owned(),
            None,
        )
        .expect("add source");

        set_user_marketplace_source_enabled(&source.id, false).expect("disable source");

        assert_eq!(
            marketplace_paths(None, None),
            vec![dev_builtin_marketplace_path()]
        );
        assert_eq!(
            list_user_marketplace_sources()
                .into_iter()
                .find(|candidate| candidate.id == source.id)
                .expect("configured source")
                .enabled,
            false
        );
    }

    #[test]
    fn removing_local_user_marketplace_source_removes_it_from_config_and_paths() {
        let _guard = PLUGIN_HOME_ENV_LOCK.lock().expect("plugin home env lock");
        let tmp = tempfile::tempdir().expect("tempdir");
        let home = tmp.path().join("home");
        let _home = ScopedEnv::set("HOME", &home);
        let _user_profile = ScopedEnv::set("USERPROFILE", &home);
        let marketplace_path =
            write_empty_marketplace(&tmp.path().join("local-marketplace"), "local-marketplace");
        let source = add_user_marketplace_source(
            MarketplaceSourceKind::Local,
            marketplace_path.to_string_lossy().into_owned(),
            None,
        )
        .expect("add source");

        remove_user_marketplace_source(&source.id).expect("remove source");

        assert!(list_user_marketplace_sources().is_empty());
        assert_eq!(
            marketplace_paths(None, None),
            vec![dev_builtin_marketplace_path()]
        );
    }

    #[test]
    fn adding_remote_user_marketplace_source_rejects_http_urls() {
        let _guard = PLUGIN_HOME_ENV_LOCK.lock().expect("plugin home env lock");
        let tmp = tempfile::tempdir().expect("tempdir");
        let _home = ScopedEnv::set("HOME", tmp.path());
        let _user_profile = ScopedEnv::set("USERPROFILE", tmp.path());

        let err = add_user_marketplace_source(
            MarketplaceSourceKind::Remote,
            "http://example.com/marketplace.git".to_string(),
            None,
        )
        .expect_err("http remote source should be rejected");

        assert_eq!(err, "remote marketplace source URL must use https");
    }

    #[test]
    fn adding_remote_user_marketplace_source_rejects_ssh_urls() {
        let _guard = PLUGIN_HOME_ENV_LOCK.lock().expect("plugin home env lock");
        let tmp = tempfile::tempdir().expect("tempdir");
        let _home = ScopedEnv::set("HOME", tmp.path());
        let _user_profile = ScopedEnv::set("USERPROFILE", tmp.path());

        let err = add_user_marketplace_source(
            MarketplaceSourceKind::Remote,
            "ssh://git@example.com/omiga/marketplace.git".to_string(),
            None,
        )
        .expect_err("ssh remote source should be rejected");

        assert_eq!(err, "remote marketplace source URL must use https");
    }

    #[test]
    fn adding_remote_user_marketplace_source_rejects_file_urls() {
        let _guard = PLUGIN_HOME_ENV_LOCK.lock().expect("plugin home env lock");
        let tmp = tempfile::tempdir().expect("tempdir");
        let _home = ScopedEnv::set("HOME", tmp.path());
        let _user_profile = ScopedEnv::set("USERPROFILE", tmp.path());

        let err = add_user_marketplace_source(
            MarketplaceSourceKind::Remote,
            "file:///tmp/marketplace.git".to_string(),
            None,
        )
        .expect_err("file remote source should be rejected");

        assert_eq!(err, "remote marketplace source URL must use https");
    }

    #[test]
    fn adding_remote_user_marketplace_source_accepts_https_without_cloning() {
        let _guard = PLUGIN_HOME_ENV_LOCK.lock().expect("plugin home env lock");
        let tmp = tempfile::tempdir().expect("tempdir");
        let _home = ScopedEnv::set("HOME", tmp.path());
        let _user_profile = ScopedEnv::set("USERPROFILE", tmp.path());

        let source = crate::commands::plugins::add_omiga_plugin_marketplace_source(
            MarketplaceSourceKind::Remote,
            "https://example.com/marketplace.git".to_string(),
            Some("Remote Marketplace".to_string()),
        )
        .expect("https remote source should be accepted");

        assert!(source.id.starts_with("remote-"));
        assert_eq!(source.kind, MarketplaceSourceKind::Remote);
        assert_eq!(source.location, "https://example.com/marketplace.git");
        assert_eq!(source.label.as_deref(), Some("Remote Marketplace"));
        assert!(source.enabled);
        assert!(!user_marketplace_cache_dir(&source.id)
            .expect("valid marketplace source id")
            .exists());
    }

    #[test]
    fn marketplace_paths_include_enabled_cached_remote_marketplace() {
        let _guard = PLUGIN_HOME_ENV_LOCK.lock().expect("plugin home env lock");
        let tmp = tempfile::tempdir().expect("tempdir");
        let _home = ScopedEnv::set("HOME", tmp.path());
        let _user_profile = ScopedEnv::set("USERPROFILE", tmp.path());
        let source = add_user_marketplace_source(
            MarketplaceSourceKind::Remote,
            "https://example.com/marketplace.git".to_string(),
            None,
        )
        .expect("add remote source");
        let cache_dir =
            user_marketplace_cache_dir(&source.id).expect("valid marketplace source id");
        let marketplace_path = write_empty_marketplace(&cache_dir, "cached-remote");

        assert_eq!(
            marketplace_paths(None, None),
            vec![dev_builtin_marketplace_path(), marketplace_path]
        );
    }

    #[test]
    fn marketplace_paths_exclude_disabled_cached_remote_marketplace() {
        let _guard = PLUGIN_HOME_ENV_LOCK.lock().expect("plugin home env lock");
        let tmp = tempfile::tempdir().expect("tempdir");
        let _home = ScopedEnv::set("HOME", tmp.path());
        let _user_profile = ScopedEnv::set("USERPROFILE", tmp.path());
        let source = add_user_marketplace_source(
            MarketplaceSourceKind::Remote,
            "https://example.com/marketplace.git".to_string(),
            None,
        )
        .expect("add remote source");
        let cache_dir =
            user_marketplace_cache_dir(&source.id).expect("valid marketplace source id");
        let marketplace_path = write_empty_marketplace(&cache_dir, "cached-remote");
        set_user_marketplace_source_enabled(&source.id, false).expect("disable source");

        let paths = marketplace_paths(None, None);
        assert_eq!(paths, vec![dev_builtin_marketplace_path()]);
        assert!(!paths.contains(&marketplace_path));
    }

    #[test]
    fn marketplace_paths_exclude_remote_marketplace_when_cache_absent() {
        let _guard = PLUGIN_HOME_ENV_LOCK.lock().expect("plugin home env lock");
        let tmp = tempfile::tempdir().expect("tempdir");
        let _home = ScopedEnv::set("HOME", tmp.path());
        let _user_profile = ScopedEnv::set("USERPROFILE", tmp.path());
        let source = add_user_marketplace_source(
            MarketplaceSourceKind::Remote,
            "https://example.com/marketplace.git".to_string(),
            None,
        )
        .expect("add remote source");
        let marketplace_path =
            user_marketplace_cache_manifest_path(&source.id).expect("valid marketplace source id");

        let paths = marketplace_paths(None, None);
        assert_eq!(paths, vec![dev_builtin_marketplace_path()]);
        assert!(!paths.contains(&marketplace_path));
    }

    #[test]
    fn path_traversal_id_is_rejected() {
        let _guard = PLUGIN_HOME_ENV_LOCK.lock().expect("plugin home env lock");
        let tmp = tempfile::tempdir().expect("tempdir");
        let home = tmp.path().join("home");
        let _home = ScopedEnv::set("HOME", &home);
        let _user_profile = ScopedEnv::set("USERPROFILE", &home);
        let cache_root = home.join(".omiga").join("marketplaces");
        let outside_cache_dir = home.join(".omiga").join("escape");
        let outside_marker = outside_cache_dir.join("marker.txt");
        fs::create_dir_all(&outside_cache_dir).expect("outside cache dir");
        fs::write(&outside_marker, "must remain").expect("outside marker");
        let config_file = config_path();
        fs::create_dir_all(config_file.parent().expect("config parent")).expect("config dir");
        fs::write(
            &config_file,
            r#"{
  "plugins": {},
  "marketplaces": [
    {
      "id": "../escape",
      "kind": "remote",
      "location": "https://example.com/marketplace.git",
      "label": null,
      "enabled": true,
      "addedAt": "2026-05-27T00:00:00Z"
    }
  ]
}
"#,
        )
        .expect("plugin config");

        let paths = marketplace_paths(None, None);
        assert!(
            paths.iter().all(
                |path| *path == dev_builtin_marketplace_path() || path.starts_with(&cache_root)
            ),
            "malicious remote source must not contribute paths outside the cache root: {paths:?}"
        );
        assert!(
            !paths
                .iter()
                .any(|path| path.starts_with(&outside_cache_dir)),
            "malicious remote source must be skipped"
        );

        let err = remove_user_marketplace_source("../escape")
            .expect_err("path traversal id should be rejected");
        assert!(
            err.contains("unsafe path characters"),
            "unexpected rejection error: {err}"
        );
        assert!(
            outside_marker.is_file(),
            "outside cache marker must not be removed"
        );
        assert!(
            !cache_root.exists(),
            "invalid source id must not create the marketplace cache root"
        );
    }

    #[test]
    fn read_config_drops_only_malformed_marketplace_entries() {
        let _guard = PLUGIN_HOME_ENV_LOCK.lock().expect("plugin home env lock");
        let tmp = tempfile::tempdir().expect("tempdir");
        let home = tmp.path().join("home");
        let _home = ScopedEnv::set("HOME", &home);
        let _user_profile = ScopedEnv::set("USERPROFILE", &home);
        let _plugins_dir = ScopedEnv::remove("OMIGA_PLUGINS_DIR");
        let config_file = config_path();
        fs::create_dir_all(config_file.parent().expect("config parent")).expect("config dir");
        fs::write(
            &config_file,
            r#"{
  "plugins": {
    "analysis-tool@custom-market": { "enabled": true }
  },
  "marketplaces": [
    {
      "id": "source-valid",
      "kind": "local",
      "location": "/tmp/omiga-plugins",
      "label": "Valid",
      "enabled": true,
      "addedAt": "2026-05-27T00:00:00Z"
    },
    {
      "id": "source-bad",
      "kind": "remote"
    }
  ]
}
"#,
        )
        .expect("plugin config");

        let config = read_config();

        assert!(config
            .plugins
            .get("analysis-tool@custom-market")
            .is_some_and(|entry| entry.enabled));
        assert_eq!(config.marketplaces.len(), 1);
        assert_eq!(config.marketplaces[0].id, "source-valid");
    }

    #[test]
    fn removing_remote_user_marketplace_source_deletes_cache_dir() {
        let _guard = PLUGIN_HOME_ENV_LOCK.lock().expect("plugin home env lock");
        let tmp = tempfile::tempdir().expect("tempdir");
        let _home = ScopedEnv::set("HOME", tmp.path());
        let _user_profile = ScopedEnv::set("USERPROFILE", tmp.path());
        let source = add_user_marketplace_source(
            MarketplaceSourceKind::Remote,
            "https://example.com/marketplace.git".to_string(),
            None,
        )
        .expect("add remote source");
        let cache_dir =
            user_marketplace_cache_dir(&source.id).expect("valid marketplace source id");
        write_empty_marketplace(&cache_dir, "cached-remote");
        assert!(cache_dir.exists());

        remove_user_marketplace_source(&source.id).expect("remove remote source");

        assert!(list_user_marketplace_sources().is_empty());
        assert!(!cache_dir.exists());
    }

    #[test]
    fn marketplace_listing_keeps_local_plugins_and_remote_metadata() {
        let entries = list_plugin_marketplaces(None, None);
        let builtin = dev_builtin_marketplace_path();
        let entry = entries
            .iter()
            .find(|entry| entry.path == builtin.to_string_lossy())
            .expect("curated marketplace entry");
        assert!(
            entry
                .remote
                .as_ref()
                .and_then(|remote| remote.url.strip_prefix("https://"))
                .is_some(),
            "remote metadata should be preserved so the UI can enable update checks"
        );
        assert!(
            !entry.plugins.is_empty(),
            "packaged marketplace must keep local plugin entries available for production installs"
        );
    }

    fn write_cached_plugin(
        cache_root: &Path,
        marketplace: &str,
        name: &str,
        manifest_interface: &str,
        mcp_url: Option<&str>,
        app_id: Option<&str>,
        with_skill: bool,
    ) -> PathBuf {
        let plugin_root = cache_root.join(marketplace).join(name).join("local");
        fs::create_dir_all(plugin_root.join(".omiga-plugin")).unwrap();
        fs::write(
            plugin_root.join(".omiga-plugin/plugin.json"),
            format!(
                r#"{{
                  "name": "{name}",
                  "version": "local",
                  "description": "{name} plugin",
                  "interface": {manifest_interface}
                }}"#
            ),
        )
        .unwrap();
        if with_skill {
            let skill_dir = plugin_root.join("skills").join("sample-skill");
            fs::create_dir_all(&skill_dir).unwrap();
            fs::write(
                skill_dir.join("SKILL.md"),
                "---\nname: sample-skill\ndescription: sample skill\n---\n",
            )
            .unwrap();
        }
        if let Some(url) = mcp_url {
            fs::write(
                plugin_root.join(".mcp.json"),
                format!(
                    r#"{{
                      "mcpServers": {{
                        "sample": {{ "url": "{url}" }}
                      }}
                    }}"#
                ),
            )
            .unwrap();
        }
        if let Some(app_id) = app_id {
            fs::write(
                plugin_root.join(".app.json"),
                format!(
                    r#"{{
                      "apps": {{
                        "calendar": {{ "id": "{app_id}" }}
                      }}
                    }}"#
                ),
            )
            .unwrap();
        }
        plugin_root
    }

    fn write_typed_plugin(
        store_root: &Path,
        kind: PluginKind,
        name: &str,
        manifest_interface: &str,
        with_skill: bool,
    ) -> PathBuf {
        let plugin_root = store_root.join(kind.dir_name()).join(name);
        fs::create_dir_all(&plugin_root).unwrap();
        fs::write(
            plugin_root.join(PLUGIN_MANIFEST_FILE),
            format!(
                r#"{{
                  "name": "{name}",
                  "version": "0.1.0",
                  "description": "{name} plugin",
                  "interface": {manifest_interface}
                }}"#
            ),
        )
        .unwrap();
        if with_skill {
            let skill_dir = plugin_root.join("skills").join("sample-skill");
            fs::create_dir_all(&skill_dir).unwrap();
            fs::write(
                skill_dir.join("SKILL.md"),
                "---\nname: sample-skill\ndescription: sample skill\n---\n",
            )
            .unwrap();
        }
        plugin_root
    }

    #[test]
    fn test_active_plugin_root_by_name_non_curated_computer_use_marketplace() {
        let _guard = PLUGIN_HOME_ENV_LOCK.lock().expect("plugin home env lock");
        let tmp = tempfile::tempdir().expect("tempdir");
        let _home = ScopedEnv::set("HOME", tmp.path());
        let _user_profile = ScopedEnv::set("USERPROFILE", tmp.path());

        let marketplace_root = tmp.path().join("marketplace");
        let plugin_source = marketplace_root.join("plugins").join("computer-use");
        fs::create_dir_all(&plugin_source).expect("plugin source");
        fs::write(
            plugin_source.join(PLUGIN_MANIFEST_FILE),
            r#"{
              "name": "computer-use",
              "version": "0.1.0",
              "interface": { "category": "Tool" }
            }"#,
        )
        .expect("plugin manifest");
        let marketplace_path = marketplace_root.join(MARKETPLACE_FILE_NAME);
        fs::write(
            &marketplace_path,
            r#"{
              "name": "my-marketplace",
              "plugins": [
                {
                  "name": "computer-use",
                  "source": { "source": "local", "path": "./plugins/computer-use" },
                  "policy": { "installation": "AVAILABLE", "authentication": "ON_USE" },
                  "category": "Tool"
                }
              ]
            }"#,
        )
        .expect("marketplace manifest");

        let installed = install_plugin(&marketplace_path, "computer-use").expect("install plugin");
        let expected_path = tmp
            .path()
            .join(".omiga")
            .join("plugins")
            .join("tools")
            .join("computer-use");

        assert_eq!(installed.plugin_id, "computer-use@my-marketplace");
        assert_eq!(PathBuf::from(installed.installed_path), expected_path);
        assert_eq!(
            active_plugin_root_by_name("computer-use").as_deref(),
            Some(expected_path.as_path())
        );
    }

    #[test]
    fn resolves_manifest_paths_safely() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let plugin = tmp.path().join("sample");
        fs::create_dir_all(&plugin).unwrap();
        fs::write(
            plugin.join(PLUGIN_MANIFEST_FILE),
            r#"{
              "name":"sample",
              "operators":"./ops",
              "templates":"./templates",
              "skills":"./skills",
              "agents":"./agents",
              "environments":"./envs",
              "mcpServers":"../bad",
              "hooks":"./hooks/hooks.json"
            }"#,
        )
        .unwrap();
        let manifest = load_plugin_manifest(&plugin).expect("manifest");
        assert_eq!(manifest.name, "sample");
        assert_eq!(manifest.operators, Some(plugin.join("ops")));
        assert_eq!(manifest.templates, Some(plugin.join("templates")));
        assert_eq!(manifest.skills, Some(plugin.join("skills")));
        assert_eq!(manifest.agents, Some(plugin.join("agents")));
        assert_eq!(manifest.environments, Some(plugin.join("envs")));
        assert_eq!(manifest.mcp_servers, None);
        assert_eq!(manifest.hooks, Some(plugin.join("hooks/hooks.json")));
    }

    #[test]
    fn plugin_id_rejects_path_segments() {
        assert!(PluginId::parse("demo@market").is_ok());
        assert!(PluginId::parse(".@market").is_err());
        assert!(PluginId::parse("demo@.").is_err());
        assert!(PluginId::parse("../demo@market").is_err());
        assert!(PluginId::parse("demo").is_err());
    }

    #[test]
    fn enabled_plugin_ids_are_stably_sorted() {
        let mut config = PluginConfigFile::default();
        config.plugins.insert(
            "zeta@market".to_string(),
            PluginConfigEntry {
                enabled: true,
                ..Default::default()
            },
        );
        config.plugins.insert(
            "alpha@market".to_string(),
            PluginConfigEntry {
                enabled: true,
                ..Default::default()
            },
        );
        config.plugins.insert(
            "disabled@market".to_string(),
            PluginConfigEntry {
                enabled: false,
                ..Default::default()
            },
        );

        let ids = enabled_plugin_ids(&config)
            .into_iter()
            .map(|id| id.key())
            .collect::<Vec<_>>();
        assert_eq!(ids, vec!["alpha@market", "zeta@market"]);
    }

    fn fixture_retrieval_manifest() -> PluginRetrievalManifest {
        PluginRetrievalManifest {
            protocol_version: 1,
            runtime: PluginRetrievalRuntime {
                command: PathBuf::from("./router.py"),
                args: Vec::new(),
                env: HashMap::new(),
                cwd: PathBuf::from("."),
                idle_ttl_ms: None,
                request_timeout_ms: None,
                cancel_grace_ms: None,
                concurrency: 1,
            },
            resources: vec![
                PluginRetrievalResource {
                    id: "alpha".to_string(),
                    category: "dataset".to_string(),
                    label: "Alpha".to_string(),
                    description: "Alpha dataset source.".to_string(),
                    aliases: Vec::new(),
                    subcategories: Vec::new(),
                    capabilities: vec!["search".to_string(), "query".to_string()],
                    required_credential_refs: Vec::new(),
                    optional_credential_refs: Vec::new(),
                    risk_level: "low".to_string(),
                    risk_notes: Vec::new(),
                    default_enabled: false,
                    replaces_builtin: true,
                    parameters: Vec::new(),
                },
                PluginRetrievalResource {
                    id: "beta".to_string(),
                    category: "literature".to_string(),
                    label: "Beta".to_string(),
                    description: "Beta literature source.".to_string(),
                    aliases: Vec::new(),
                    subcategories: Vec::new(),
                    capabilities: vec!["search".to_string(), "fetch".to_string()],
                    required_credential_refs: Vec::new(),
                    optional_credential_refs: Vec::new(),
                    risk_level: "low".to_string(),
                    risk_notes: Vec::new(),
                    default_enabled: false,
                    replaces_builtin: true,
                    parameters: Vec::new(),
                },
                PluginRetrievalResource {
                    id: "gamma".to_string(),
                    category: "knowledge".to_string(),
                    label: "Gamma".to_string(),
                    description: "Gamma knowledge source.".to_string(),
                    aliases: Vec::new(),
                    subcategories: Vec::new(),
                    capabilities: vec!["fetch".to_string()],
                    required_credential_refs: Vec::new(),
                    optional_credential_refs: Vec::new(),
                    risk_level: "low".to_string(),
                    risk_notes: Vec::new(),
                    default_enabled: false,
                    replaces_builtin: false,
                    parameters: Vec::new(),
                },
            ],
        }
    }

    fn fixture_environment_profile(check_command: Vec<String>) -> EnvironmentProfileSummary {
        EnvironmentProfileSummary {
            id: "fixture-env".to_string(),
            version: "0.1.0".to_string(),
            canonical_id: "plugin/environment/fixture-env".to_string(),
            source_plugin: "plugin@test".to_string(),
            manifest_path: "/tmp/environment.yaml".to_string(),
            name: None,
            description: None,
            tags: Vec::new(),
            runtime: EnvironmentRuntimeProfile {
                kind: Some("conda".to_string()),
                ..Default::default()
            },
            requirements: EnvironmentRequirements::default(),
            diagnostics: EnvironmentDiagnostics {
                check_command,
                ..Default::default()
            },
        }
    }

    #[test]
    fn plugin_conda_environment_check_commands_must_match_declared_dependencies() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let conda = tmp.path().join("conda.yaml");
        fs::write(
            &conda,
            "channels:\n  - conda-forge\ndependencies:\n  - demoalign =1.0\n",
        )
        .unwrap();
        let profile =
            fixture_environment_profile(vec!["demoalign".to_string(), "--version".to_string()]);

        assert!(is_allowed_plugin_environment_check_command(
            &profile,
            &profile.diagnostics.check_command,
            &conda
        ));
        assert!(!is_allowed_plugin_environment_check_command(
            &fixture_environment_profile(vec!["rm".to_string(), "--version".to_string()]),
            &["rm".to_string(), "--version".to_string()],
            &conda
        ));
        assert!(!is_allowed_plugin_environment_check_command(
            &fixture_environment_profile(vec!["demoalign".to_string(), "run".to_string()]),
            &["demoalign".to_string(), "run".to_string()],
            &conda
        ));
    }

    #[test]
    fn read_config_migrates_superseded_plugins_using_resolved_builtin_marketplace_path() {
        let _guard = PLUGIN_HOME_ENV_LOCK.lock().expect("plugin home env lock");
        let tmp = tempfile::tempdir().expect("tempdir");
        let home = tmp.path().join("home");
        let marketplace_root = tmp.path().join("resolved-marketplace");
        write_superseding_marketplace(
            &marketplace_root,
            "resolved-market",
            "analysis-bundle",
            "legacy-analysis-tool",
            "Workflow",
        );
        let _home = ScopedEnv::set("HOME", &home);
        let _user_profile = ScopedEnv::set("USERPROFILE", &home);
        let _plugins_dir = ScopedEnv::set("OMIGA_PLUGINS_DIR", &marketplace_root);
        let config_file = config_path();
        fs::create_dir_all(config_file.parent().expect("config parent")).expect("config dir");
        fs::write(
            &config_file,
            r#"{
  "plugins": {
    "legacy-analysis-tool@resolved-market": { "enabled": true }
  },
  "marketplaces": []
}
"#,
        )
        .expect("plugin config");

        let config = read_config();

        assert!(config
            .plugins
            .get("analysis-bundle@resolved-market")
            .is_some_and(|entry| entry.enabled));
        assert!(!config
            .plugins
            .contains_key("legacy-analysis-tool@resolved-market"));
    }

    #[test]
    fn superseded_plugin_config_migrates_from_marketplace_manifest_metadata() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let marketplace_root = tmp.path().join("marketplace");
        let plugins_root = marketplace_root.join("plugins");
        let analysis_root = plugins_root.join("analysis-bundle");
        let resource_root = plugins_root.join("resource-bundle");
        fs::create_dir_all(&analysis_root).unwrap();
        fs::create_dir_all(&resource_root).unwrap();
        fs::write(resource_root.join("router.py"), "#!/usr/bin/env python3\n").unwrap();
        fs::write(
            analysis_root.join(PLUGIN_MANIFEST_FILE),
            r#"{
              "name":"analysis-bundle",
              "version":"0.1.0",
              "compatibility": { "supersedesPlugins": ["legacy-analysis-tool"] },
              "interface": { "category":"Analysis" }
            }"#,
        )
        .unwrap();
        fs::write(
            resource_root.join(PLUGIN_MANIFEST_FILE),
            r#"{
              "name":"resource-bundle",
              "version":"0.1.0",
              "retrieval": {
                "protocolVersion": 1,
                "runtime": { "command":"./router.py" },
                "resources": [
                  {
                    "id":"example_source",
                    "category":"dataset",
                    "label":"Example Source",
                    "description":"Example public dataset source.",
                    "aliases":["example"],
                    "capabilities":["search", "fetch"],
                    "riskLevel":"low",
                    "defaultEnabled": false,
                    "replacesBuiltin": true
                  }
                ]
              },
              "interface": { "category":"Retrieval" }
            }"#,
        )
        .unwrap();
        fs::write(
            marketplace_root.join(MARKETPLACE_FILE_NAME),
            r#"{
              "name":"test-market",
              "plugins":[
                {
                  "name":"analysis-bundle",
                  "source":{"source":"local","path":"./plugins/analysis-bundle"},
                  "policy":{"installation":"AVAILABLE","authentication":"ON_USE"},
                  "category":"Analysis"
                },
                {
                  "name":"resource-bundle",
                  "source":{"source":"local","path":"./plugins/resource-bundle"},
                  "policy":{"installation":"AVAILABLE","authentication":"ON_USE"},
                  "category":"Retrieval"
                }
              ]
            }"#,
        )
        .unwrap();

        let mut config = PluginConfigFile::default();
        for key in [
            "legacy-analysis-tool@test-market",
            "operator-smoke@omiga-curated",
        ] {
            config.plugins.insert(
                key.to_string(),
                PluginConfigEntry {
                    enabled: true,
                    ..Default::default()
                },
            );
        }
        config.plugins.insert(
            "retrieval-dataset-example@test-market".to_string(),
            PluginConfigEntry {
                enabled: false,
                ..Default::default()
            },
        );
        config.plugins.insert(
            "third-party-old@custom-market".to_string(),
            PluginConfigEntry {
                enabled: true,
                ..Default::default()
            },
        );

        assert!(migrate_superseded_builtin_plugin_config_with_marketplaces(
            &mut config,
            &[marketplace_root.join(MARKETPLACE_FILE_NAME)]
        ));

        assert!(config
            .plugins
            .get("analysis-bundle@test-market")
            .is_some_and(|entry| entry.enabled));
        let resource_entry = config
            .plugins
            .get("resource-bundle@test-market")
            .expect("resource replacement");
        assert!(!resource_entry.enabled);
        assert!(resource_entry
            .disabled_retrieval_resources
            .contains("dataset.example_source"));
        assert!(config.plugins.contains_key("third-party-old@custom-market"));
        assert!(!config
            .plugins
            .contains_key("legacy-analysis-tool@test-market"));
        assert!(!config
            .plugins
            .contains_key("retrieval-dataset-example-source@test-market"));
        assert!(!config
            .plugins
            .contains_key("operator-example-search@test-market"));
        assert!(!config.plugins.contains_key("operator-smoke@omiga-curated"));
    }

    #[test]
    fn configured_plugins_load_from_typed_operator_root() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store_root = tmp.path().join("plugins");
        let cache_root = store_root.join("cache");
        let plugin_root = write_typed_plugin(
            &store_root,
            PluginKind::Operator,
            "sample-operator",
            r#"{"displayName":"Sample Operator","category":"Operator"}"#,
            true,
        );
        let mut config = PluginConfigFile::default();
        config.plugins.insert(
            "sample-operator@market".to_string(),
            PluginConfigEntry {
                enabled: true,
                ..Default::default()
            },
        );

        let outcome = load_plugins_from_config(&config, &cache_root);

        assert_eq!(outcome.plugins().len(), 1);
        assert!(outcome.plugins()[0].is_active());
        assert_eq!(outcome.plugins()[0].root, plugin_root);
        assert_eq!(
            outcome.effective_skill_roots(),
            vec![plugin_root.join("skills")]
        );
    }

    #[test]
    fn active_plugin_root_does_not_read_legacy_cache() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store_root = tmp.path().join("plugins");
        let cache_root = store_root.join("cache");
        write_cached_plugin(
            &cache_root,
            "market",
            "sample",
            r#"{"displayName":"Legacy Sample"}"#,
            None,
            None,
            false,
        );
        let plugin_id = PluginId::new("sample", "market").unwrap();

        assert_eq!(active_plugin_root_from_roots(&cache_root, &plugin_id), None);
    }

    #[test]
    fn legacy_cache_plugins_are_migrated_to_typed_root_before_loading() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store_root = tmp.path().join("plugins");
        let cache_root = store_root.join("cache");
        let legacy_root = write_cached_plugin(
            &cache_root,
            "market",
            "sample",
            r#"{"displayName":"Sample Operator","category":"Operator"}"#,
            None,
            None,
            true,
        );
        let mut config = PluginConfigFile::default();
        config.plugins.insert(
            "sample@market".to_string(),
            PluginConfigEntry {
                enabled: true,
                ..Default::default()
            },
        );
        let typed_root = store_root.join("operators").join("sample");

        let outcome = load_plugins_from_config(&config, &cache_root);

        assert_eq!(outcome.plugins()[0].root, typed_root);
        assert_eq!(
            outcome.effective_skill_roots(),
            vec![typed_root.join("skills")]
        );
        assert!(!legacy_root.exists());
        assert!(!cache_root.join("market").join("sample").exists());
    }

    #[test]
    fn plugin_kind_classification_matches_install_sections() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let operator_root = write_typed_plugin(
            tmp.path(),
            PluginKind::Operator,
            "operator-fixture",
            r#"{"displayName":"Operator Fixture","category":"Operator"}"#,
            false,
        );
        let analysis_root = write_typed_plugin(
            tmp.path(),
            PluginKind::Workflow,
            "analysis-fixture",
            r#"{"displayName":"Analysis Fixture","category":"Analysis"}"#,
            false,
        );
        let resource_root = write_typed_plugin(
            tmp.path(),
            PluginKind::Resource,
            "resource-fixture",
            r#"{"displayName":"Resource Fixture","category":"Retrieval"}"#,
            false,
        );
        let workflow_root = write_typed_plugin(
            tmp.path(),
            PluginKind::Workflow,
            "notebook-fixture",
            r#"{"displayName":"Notebook Fixture","category":"Notebook"}"#,
            false,
        );
        let visualization_root = write_typed_plugin(
            tmp.path(),
            PluginKind::Workflow,
            "visualization-fixture",
            r#"{"displayName":"Visualization Fixture","category":"Visualization"}"#,
            false,
        );
        for (root, name, category) in [
            (&analysis_root, "analysis-fixture", "Analysis"),
            (
                &visualization_root,
                "visualization-fixture",
                "Visualization",
            ),
        ] {
            fs::create_dir_all(root.join("templates")).unwrap();
            fs::write(
                root.join(PLUGIN_MANIFEST_FILE),
                format!(
                    r#"{{
                      "name": "{name}",
                      "version": "0.1.0",
                      "description": "{name} plugin",
                      "templates": "./templates",
                      "interface": {{"displayName":"{name}","category":"{category}"}}
                    }}"#
                ),
            )
            .unwrap();
        }

        let operator_manifest = load_plugin_manifest(&operator_root).expect("operator manifest");
        let analysis_manifest = load_plugin_manifest(&analysis_root).expect("analysis manifest");
        let resource_manifest = load_plugin_manifest(&resource_root).expect("source manifest");
        let workflow_manifest = load_plugin_manifest(&workflow_root).expect("workflow manifest");
        let visualization_manifest =
            load_plugin_manifest(&visualization_root).expect("visualization manifest");

        assert_eq!(
            plugin_kind_for_manifest(&operator_root, Some("Operator"), &operator_manifest),
            PluginKind::Operator
        );
        assert_eq!(
            plugin_kind_for_manifest(&analysis_root, Some("Analysis"), &analysis_manifest),
            PluginKind::Workflow
        );
        assert_eq!(
            plugin_kind_for_manifest(&resource_root, Some("Retrieval"), &resource_manifest),
            PluginKind::Resource
        );
        assert_eq!(
            plugin_kind_for_manifest(&workflow_root, Some("Notebook"), &workflow_manifest),
            PluginKind::Workflow
        );
        assert_eq!(
            plugin_kind_for_manifest(
                &visualization_root,
                Some("Visualization"),
                &visualization_manifest
            ),
            PluginKind::Workflow
        );
    }

    #[test]
    fn remove_other_typed_plugin_roots_keeps_only_current_install_kind() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store_root = tmp.path().join("plugins");
        let plugin_id = PluginId::new("sample-plugin", "test-market").unwrap();
        let old_operator_root = write_typed_plugin(
            &store_root,
            PluginKind::Operator,
            "sample-plugin",
            r#"{"displayName":"Old Operator","category":"Operator"}"#,
            false,
        );
        let workflow_root = write_typed_plugin(
            &store_root,
            PluginKind::Workflow,
            "sample-plugin",
            r#"{"displayName":"Sample Workflow","category":"Visualization"}"#,
            true,
        );
        let unrelated_root = write_typed_plugin(
            &store_root,
            PluginKind::Operator,
            "other-plugin",
            r#"{"displayName":"Other","category":"Operator"}"#,
            false,
        );

        remove_other_typed_plugin_roots(&store_root, &plugin_id, &workflow_root)
            .expect("cleanup stale typed roots");

        assert!(!old_operator_root.exists());
        assert!(workflow_root.exists());
        assert!(unrelated_root.exists());
    }

    #[test]
    fn configured_builtin_plugins_refresh_moves_stale_typed_roots_without_overwriting_local_edits()
    {
        let _guard = PLUGIN_HOME_ENV_LOCK.lock().expect("plugin home env lock");
        let tmp = tempfile::tempdir().expect("tempdir");
        let home = tmp.path().join("home");
        let marketplace_root = tmp.path().join("resolved-marketplace");
        write_superseding_marketplace(
            &marketplace_root,
            "resolved-market",
            "analysis-bundle",
            "legacy-analysis-tool",
            "Workflow",
        );
        let _home = ScopedEnv::set("HOME", &home);
        let _user_profile = ScopedEnv::set("USERPROFILE", &home);
        let _plugins_dir = ScopedEnv::set("OMIGA_PLUGINS_DIR", &marketplace_root);
        let store_root = tmp.path().join("plugins");
        let cache_root = store_root.join("cache");
        let old_operator_root = write_typed_plugin(
            &store_root,
            PluginKind::Operator,
            "analysis-bundle",
            r#"{"displayName":"Stale Install","category":"Operator"}"#,
            false,
        );
        let local_marker = old_operator_root.join("local-edit.txt");
        fs::write(&local_marker, "keep me").expect("local marker");
        let mut config = PluginConfigFile::default();
        config.plugins.insert(
            "analysis-bundle@resolved-market".to_string(),
            PluginConfigEntry {
                enabled: true,
                ..Default::default()
            },
        );

        let refreshed = refresh_configured_builtin_plugins(&config, &cache_root)
            .expect("refresh curated plugin");

        let expected_root = store_root
            .join(PluginKind::Workflow.dir_name())
            .join("analysis-bundle");
        assert_eq!(refreshed, 1);
        assert!(!old_operator_root.exists());
        assert!(expected_root.exists());
        assert_eq!(
            fs::read_to_string(expected_root.join("local-edit.txt")).expect("local marker"),
            "keep me"
        );
    }

    #[test]
    fn configured_builtin_plugins_refresh_uses_resolved_builtin_marketplace_path() {
        let _guard = PLUGIN_HOME_ENV_LOCK.lock().expect("plugin home env lock");
        let tmp = tempfile::tempdir().expect("tempdir");
        let home = tmp.path().join("home");
        let marketplace_root = tmp.path().join("resolved-marketplace");
        write_superseding_marketplace(
            &marketplace_root,
            "resolved-market",
            "analysis-bundle",
            "legacy-analysis-tool",
            "Workflow",
        );
        let _home = ScopedEnv::set("HOME", &home);
        let _user_profile = ScopedEnv::set("USERPROFILE", &home);
        let _plugins_dir = ScopedEnv::set("OMIGA_PLUGINS_DIR", &marketplace_root);
        let store_root = tmp.path().join("plugins");
        let cache_root = store_root.join("cache");
        let old_operator_root = write_typed_plugin(
            &store_root,
            PluginKind::Operator,
            "analysis-bundle",
            r#"{"displayName":"Stale Install","category":"Operator"}"#,
            false,
        );
        let mut config = PluginConfigFile::default();
        config.plugins.insert(
            "analysis-bundle@resolved-market".to_string(),
            PluginConfigEntry {
                enabled: true,
                ..Default::default()
            },
        );

        let refreshed = refresh_configured_builtin_plugins(&config, &cache_root)
            .expect("refresh resolved built-in plugin");

        let expected_root = store_root
            .join(PluginKind::Workflow.dir_name())
            .join("analysis-bundle");
        assert_eq!(refreshed, 1);
        assert!(!old_operator_root.exists());
        assert!(expected_root.exists());
    }

    #[test]
    fn listing_marketplaces_does_not_migrate_or_sync_installed_plugin_roots() {
        let _guard = PLUGIN_HOME_ENV_LOCK.lock().expect("plugin home env lock");
        let tmp = tempfile::tempdir().expect("tempdir");
        let home = tmp.path().join("home");
        let marketplace_root = tmp.path().join("resolved-marketplace");
        write_superseding_marketplace(
            &marketplace_root,
            "resolved-market",
            "analysis-bundle",
            "legacy-analysis-tool",
            "Workflow",
        );
        let _home = ScopedEnv::set("HOME", &home);
        let _user_profile = ScopedEnv::set("USERPROFILE", &home);
        let _plugins_dir = ScopedEnv::set("OMIGA_PLUGINS_DIR", &marketplace_root);
        let config_file = config_path();
        fs::create_dir_all(config_file.parent().expect("config parent")).expect("config dir");
        fs::write(
            &config_file,
            r#"{
              "plugins": {
                "analysis-bundle@resolved-market": { "enabled": true }
              }
            }"#,
        )
        .expect("plugin config");
        let old_operator_root = write_typed_plugin(
            &plugin_store_root(),
            PluginKind::Operator,
            "analysis-bundle",
            r#"{"displayName":"Locally Edited Install","category":"Operator"}"#,
            false,
        );
        fs::write(old_operator_root.join("local-edit.txt"), "must remain").expect("local marker");
        let workflow_root = plugin_store_root()
            .join(PluginKind::Workflow.dir_name())
            .join("analysis-bundle");

        let marketplaces = list_plugin_marketplaces(None, None);

        assert!(marketplaces
            .iter()
            .flat_map(|marketplace| marketplace.plugins.iter())
            .any(|plugin| plugin.id == "analysis-bundle@resolved-market"));
        assert!(old_operator_root.exists());
        assert_eq!(
            fs::read_to_string(old_operator_root.join("local-edit.txt")).expect("local marker"),
            "must remain"
        );
        assert!(!workflow_root.exists());
    }

    #[test]
    fn plugin_sync_plan_updates_safe_paths_and_keeps_conflicts() {
        let base = BTreeMap::from([
            ("same.txt".to_string(), "sha256:base".to_string()),
            ("update.txt".to_string(), "sha256:old".to_string()),
            ("local.txt".to_string(), "sha256:old".to_string()),
            ("conflict.txt".to_string(), "sha256:old".to_string()),
            ("remove.txt".to_string(), "sha256:old".to_string()),
        ]);
        let current = BTreeMap::from([
            ("same.txt".to_string(), "sha256:base".to_string()),
            ("update.txt".to_string(), "sha256:old".to_string()),
            ("local.txt".to_string(), "sha256:user".to_string()),
            ("conflict.txt".to_string(), "sha256:user".to_string()),
            ("remove.txt".to_string(), "sha256:old".to_string()),
        ]);
        let source = BTreeMap::from([
            ("same.txt".to_string(), "sha256:base".to_string()),
            ("update.txt".to_string(), "sha256:new".to_string()),
            ("local.txt".to_string(), "sha256:old".to_string()),
            ("conflict.txt".to_string(), "sha256:new".to_string()),
            ("added.txt".to_string(), "sha256:new".to_string()),
        ]);

        let plan = plugin_sync_plan(Some(&base), &current, &source);

        assert_eq!(plan.updated, vec!["update.txt"]);
        assert_eq!(plan.added, vec!["added.txt"]);
        assert_eq!(plan.removed, vec!["remove.txt"]);
        assert_eq!(plan.kept_local, vec!["local.txt"]);
        assert_eq!(plan.conflicts, vec!["conflict.txt"]);
    }

    #[test]
    fn plugin_sync_summary_detects_marketplace_update_without_local_edits() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let source = tmp.path().join("source");
        let installed = tmp.path().join("installed");
        fs::create_dir_all(&source).unwrap();
        fs::write(
            source.join(PLUGIN_MANIFEST_FILE),
            r#"{"name":"demo","version":"0.1.0"}"#,
        )
        .unwrap();
        fs::write(source.join("tool.txt"), "v1").unwrap();
        copy_dir_recursive(&source, &installed).unwrap();
        let plugin_id = PluginId::new("demo", "local").unwrap();
        record_plugin_install_state(&installed, &plugin_id, Some("0.1.0".to_string()), None)
            .unwrap();

        fs::write(source.join("tool.txt"), "v2").unwrap();
        fs::write(source.join("new.txt"), "added").unwrap();

        let summary = plugin_sync_summary(&source, Some(&installed)).expect("sync summary");

        assert_eq!(summary.state, "updateAvailable");
        assert_eq!(summary.changed_count, 2);
        assert_eq!(summary.local_modified_count, 0);
        assert_eq!(summary.conflict_count, 0);
    }

    #[test]
    fn plugin_force_sync_plan_marks_overwritten_and_removed_paths() {
        let current = BTreeMap::from([
            ("same.txt".to_string(), "sha256:base".to_string()),
            ("local-only.txt".to_string(), "sha256:user".to_string()),
            ("changed.txt".to_string(), "sha256:user".to_string()),
        ]);
        let source = BTreeMap::from([
            ("same.txt".to_string(), "sha256:base".to_string()),
            ("changed.txt".to_string(), "sha256:source".to_string()),
            ("source-only.txt".to_string(), "sha256:source".to_string()),
        ]);

        let plan = plugin_force_sync_plan(&current, &source);

        assert_eq!(plan.updated, vec!["changed.txt"]);
        assert_eq!(plan.added, vec!["source-only.txt"]);
        assert_eq!(plan.removed, vec!["local-only.txt"]);
        assert!(plan.kept_local.is_empty());
        assert!(plan.conflicts.is_empty());
    }

    #[test]
    fn plugin_changelog_summary_reads_manifest_declared_path() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let plugin = tmp.path().join("plugin");
        fs::create_dir_all(&plugin).unwrap();
        fs::write(
            plugin.join(PLUGIN_MANIFEST_FILE),
            r#"{"name":"demo","version":"0.2.0","changelog":"./docs/CHANGES.md"}"#,
        )
        .unwrap();
        fs::create_dir_all(plugin.join("docs")).unwrap();
        fs::write(
            plugin.join("docs/CHANGES.md"),
            "# Changelog\n\n## 0.2.0 - 2026-05-12\n\n- Added remote sync.\n\n## 0.1.0\n\n- Initial release.\n",
        )
        .unwrap();
        let manifest = load_plugin_manifest(&plugin).expect("manifest");

        let summary = plugin_changelog_summary(&plugin, Some(&manifest)).expect("changelog");

        assert_eq!(summary.latest_version, Some("0.2.0".to_string()));
        assert_eq!(summary.entries.len(), 2);
        assert_eq!(summary.entries[0].date, Some("2026-05-12".to_string()));
        assert!(summary.entries[0].body.contains("remote sync"));
    }

    #[test]
    fn remote_marketplace_diff_ignores_missing_remote_versions_for_existing_plugins() {
        let local = BTreeMap::from([
            ("alignment".to_string(), Some("0.1.0".to_string())),
            ("analysis-bundle".to_string(), Some("0.1.0".to_string())),
        ]);
        let remote_without_versions = BTreeMap::from([
            ("alignment".to_string(), None),
            ("analysis-bundle".to_string(), None),
        ]);
        assert!(changed_marketplace_plugins(&local, &remote_without_versions).is_empty());

        let remote_with_change = BTreeMap::from([
            ("alignment".to_string(), Some("0.2.0".to_string())),
            ("analysis-bundle".to_string(), None),
            ("new-plugin".to_string(), Some("0.1.0".to_string())),
        ]);
        assert_eq!(
            changed_marketplace_plugins(&local, &remote_with_change),
            vec!["alignment".to_string(), "new-plugin".to_string()]
        );
    }

    #[test]
    fn curated_marketplace_uses_nested_resource_utils_not_global_resource_runners() {
        let marketplace_path = curated_marketplace_path();
        let marketplace_root = marketplace_path.parent().expect("marketplace root");
        assert!(
            !marketplace_root.join(RESOURCE_RUNNERS_DIR).exists(),
            "latest omiga-plugins marketplace should not use top-level resource_runners"
        );
        assert!(
            marketplace_root
                .join("plugins")
                .join("resources")
                .join("utils")
                .join("retrieval_http.py")
                .is_file(),
            "latest omiga-plugins marketplace should share retrieval utilities under plugins/resources/utils"
        );
    }

    #[test]
    fn legacy_marketplace_resource_runners_are_still_copied_to_resource_root() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let marketplace_root = tmp.path().join("marketplace");
        let resource_runners = marketplace_root.join(RESOURCE_RUNNERS_DIR);
        fs::create_dir_all(&resource_runners).unwrap();
        fs::write(
            resource_runners.join("public_knowledge_sources.py"),
            "print('runner')\n",
        )
        .unwrap();
        fs::write(
            marketplace_root.join(MARKETPLACE_FILE_NAME),
            r#"{"name":"omiga-curated","plugins":[]}"#,
        )
        .unwrap();
        let cache_root = tmp.path().join("plugins").join("cache");

        let copied = copy_marketplace_resource_runner_assets(
            &marketplace_root.join(MARKETPLACE_FILE_NAME),
            "omiga-curated",
            &cache_root,
        )
        .unwrap();

        assert!(copied);
        assert_eq!(
            fs::read_to_string(
                tmp.path()
                    .join("plugins")
                    .join("resources")
                    .join(RESOURCE_RUNNERS_DIR)
                    .join("public_knowledge_sources.py")
            )
            .unwrap(),
            "print('runner')\n"
        );
        assert_eq!(
            fs::read_to_string(
                tmp.path()
                    .join("plugins")
                    .join("resources")
                    .join(LEGACY_SOURCE_RUNNERS_DIR)
                    .join("public_knowledge_sources.py")
            )
            .unwrap(),
            "print('runner')\n"
        );
    }

    #[test]
    fn marketplace_nested_resource_utils_are_copied_to_installed_resource_root() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let marketplace_root = tmp.path().join("marketplace");
        let resource_utils = marketplace_root
            .join("plugins")
            .join("resources")
            .join(RESOURCE_UTILS_DIR);
        fs::create_dir_all(&resource_utils).unwrap();
        fs::write(
            resource_utils.join("retrieval_http.py"),
            "# shared helper\n",
        )
        .unwrap();
        fs::write(
            marketplace_root.join(MARKETPLACE_FILE_NAME),
            r#"{"name":"omiga-curated","plugins":[]}"#,
        )
        .unwrap();
        let cache_root = tmp.path().join("plugins").join("cache");

        let copied = copy_marketplace_shared_resource_assets(
            &marketplace_root.join(MARKETPLACE_FILE_NAME),
            "omiga-curated",
            &cache_root,
        )
        .unwrap();

        assert!(copied);
        assert_eq!(
            fs::read_to_string(
                tmp.path()
                    .join("plugins")
                    .join("resources")
                    .join(RESOURCE_UTILS_DIR)
                    .join("retrieval_http.py")
            )
            .unwrap(),
            "# shared helper\n"
        );
    }

    #[test]
    fn plugin_load_outcome_collects_effective_capabilities() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let cache_root = tmp.path().join("cache");
        let legacy_root = write_cached_plugin(
            &cache_root,
            "market",
            "sample",
            r#"{"displayName":"Sample Plugin"}"#,
            Some("https://sample.example/mcp"),
            Some("calendar"),
            true,
        );
        let mut config = PluginConfigFile::default();
        config.plugins.insert(
            "sample@market".to_string(),
            PluginConfigEntry {
                enabled: true,
                ..Default::default()
            },
        );

        let outcome = load_plugins_from_config(&config, &cache_root);

        assert_eq!(outcome.plugins().len(), 1);
        assert!(outcome.plugins()[0].is_active());
        let plugin_root = tmp.path().join("tools").join("sample");
        assert_eq!(outcome.plugins()[0].root, plugin_root);
        assert!(!legacy_root.exists());
        assert_eq!(
            outcome.effective_skill_roots(),
            vec![plugin_root.join("skills")]
        );
        match outcome.effective_mcp_servers().get("sample") {
            Some(McpServerConfig::Url { url, .. }) => assert_eq!(url, "https://sample.example/mcp"),
            other => panic!("expected sample URL MCP server, got {other:?}"),
        }
        assert_eq!(outcome.effective_apps(), vec!["calendar".to_string()]);
        assert_eq!(outcome.capability_summaries().len(), 1);
        let summary = &outcome.capability_summaries()[0];
        assert_eq!(summary.id, "sample@market");
        assert_eq!(summary.display_name, "Sample Plugin");
        assert_eq!(summary.description.as_deref(), Some("sample plugin"));
        assert!(summary.has_skills);
        assert_eq!(summary.mcp_servers, vec!["sample".to_string()]);
        assert_eq!(summary.apps, vec!["calendar".to_string()]);
    }

    #[test]
    fn plugin_stdio_mcp_servers_resolve_relative_cwd_from_plugin_root() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let plugin_root = tmp.path().join("computer-use");
        fs::create_dir_all(&plugin_root).unwrap();
        fs::write(
            plugin_root.join(PLUGIN_MANIFEST_FILE),
            r#"{
              "name": "computer-use",
              "version": "0.1.0",
              "mcpServers": "./.mcp.json",
              "interface": {
                "displayName": "Computer Use",
                "category": "Automation"
              }
            }"#,
        )
        .unwrap();
        fs::write(
            plugin_root.join(".mcp.json"),
            r#"{
              "mcpServers": {
                "computer": {
                  "command": "./bin/darwin-arm64/computer-use",
                  "args": ["--stdio"]
                },
                "computer-subdir": {
                  "command": "../bin/darwin-arm64/computer-use",
                  "cwd": "./mcp"
                }
              }
            }"#,
        )
        .unwrap();

        let manifest = load_plugin_manifest(&plugin_root).expect("manifest");
        let servers = plugin_mcp_servers(&plugin_root, &manifest);

        match servers.get("computer") {
            Some(McpServerConfig::Stdio { command, cwd, .. }) => {
                assert_eq!(command, "./bin/darwin-arm64/computer-use");
                let expected = plugin_root.to_string_lossy().into_owned();
                assert_eq!(cwd.as_deref(), Some(expected.as_str()));
            }
            other => panic!("expected computer stdio server, got {other:?}"),
        }
        match servers.get("computer-subdir") {
            Some(McpServerConfig::Stdio { command, cwd, .. }) => {
                assert_eq!(command, "../bin/darwin-arm64/computer-use");
                let expected = plugin_root.join("mcp").to_string_lossy().into_owned();
                assert_eq!(cwd.as_deref(), Some(expected.as_str()));
            }
            other => panic!("expected computer-subdir stdio server, got {other:?}"),
        }
    }

    #[test]
    fn plugins_system_section_renders_available_capabilities() {
        let mut mcp_servers = HashMap::new();
        mcp_servers.insert(
            "sample".to_string(),
            McpServerConfig::Url {
                url: "https://sample.example/mcp".to_string(),
                headers: HashMap::new(),
            },
        );
        let outcome = PluginLoadOutcome::from_plugins(vec![LoadedPlugin {
            id: "sample@market".to_string(),
            manifest_name: Some("sample".to_string()),
            display_name: Some("Sample Plugin".to_string()),
            description: Some("  sample\n   capability plugin  ".to_string()),
            root: PathBuf::from("/tmp/sample"),
            enabled: true,
            skill_roots: vec![PathBuf::from("/tmp/sample/skills")],
            mcp_servers,
            apps: vec!["calendar".to_string()],
            retrieval: None,
            error: None,
        }]);

        let section = format_plugins_system_section(&outcome).expect("plugins section");

        assert!(section.contains("## Plugins (available)"));
        assert!(section.contains("- `Sample Plugin`: sample capability plugin"));
        assert!(section.contains("skills"));
        assert!(section.contains("MCP servers: `sample`"));
        assert!(section.contains("app connector refs: `calendar`"));
        assert!(section.contains("operator_execute"));
        assert!(section.contains("subcommands are operation parameters"));
        assert!(section.contains("Do not assume VS Code extension UI/runtime behavior"));
    }

    #[test]
    fn template_plugins_are_visible_as_template_execute_capabilities() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let plugin_root = tmp.path().join("template-plugin");
        let template_dir = plugin_root.join("templates").join("scatter");
        fs::create_dir_all(&template_dir).expect("template dir");
        fs::write(
            plugin_root.join("plugin.json"),
            r#"{
              "name": "template-plugin",
              "version": "0.1.0",
              "description": "Template-only visualization plugin",
              "templates": "./templates"
            }"#,
        )
        .expect("plugin manifest");
        fs::write(template_dir.join("run.sh"), "#!/bin/sh\n").expect("script");
        fs::write(
            template_dir.join("template.yaml"),
            r#"apiVersion: omiga.ai/unit/v1alpha1
kind: Template
metadata:
  id: viz_demo
  version: 0.1.0
  name: Demo Plot
classification:
  category: visualization/scatter
template:
  engine: static
  entry: ./run.sh
"#,
        )
        .expect("template manifest");
        let manifest = load_plugin_manifest(&plugin_root).expect("plugin manifest");
        let outcome = PluginLoadOutcome::from_plugins(vec![LoadedPlugin {
            id: "template-plugin@market".to_string(),
            manifest_name: Some(manifest.name.clone()),
            display_name: Some("Template Plugin".to_string()),
            description: manifest.description.clone(),
            root: plugin_root,
            enabled: true,
            skill_roots: vec![],
            mcp_servers: HashMap::new(),
            apps: vec![],
            retrieval: None,
            error: None,
        }]);

        let section = format_plugins_system_section(&outcome).expect("plugins section");

        assert!(section.contains("- `Template Plugin`: Template-only visualization plugin"));
        assert!(section
            .contains("templates: 1 via `unit_search` / `unit_describe` / `template_execute`"));
        assert!(section.contains("groups: `scatter`"));
        assert!(section.contains("Template plugins expose Template units"));
    }

    #[test]
    fn operator_plugins_are_visible_as_operator_execute_capabilities() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let plugin_root = tmp.path().join("operator-plugin");
        let operator_dir = plugin_root.join("operators").join("demo-program");
        fs::create_dir_all(&operator_dir).expect("operator dir");
        fs::write(
            plugin_root.join("plugin.json"),
            r#"{
              "name": "operator-plugin",
              "version": "0.1.0",
              "description": "Operator-only analysis plugin",
              "operators": "./operators"
            }"#,
        )
        .expect("plugin manifest");
        fs::write(
            operator_dir.join("operator.yaml"),
            r#"apiVersion: omiga.ai/operator/v1alpha1
kind: Operator
metadata:
  id: demo_program
  version: "1"
  name: Demo Program
operations:
  sample:
    description: Sample reads
  summarize:
    description: Summarize results
interface:
  params:
    message:
      kind: string
      default: hello
execution:
  argv: ["demo-program", "${params.operation}", "${params.message}"]
"#,
        )
        .expect("operator manifest");
        let manifest = load_plugin_manifest(&plugin_root).expect("plugin manifest");
        let outcome = PluginLoadOutcome::from_plugins(vec![LoadedPlugin {
            id: "operator-plugin@market".to_string(),
            manifest_name: Some(manifest.name.clone()),
            display_name: Some("Operator Plugin".to_string()),
            description: manifest.description.clone(),
            root: plugin_root,
            enabled: true,
            skill_roots: vec![],
            mcp_servers: HashMap::new(),
            apps: vec![],
            retrieval: None,
            error: None,
        }]);

        let section = format_plugins_system_section(&outcome).expect("plugins section");

        assert!(section.contains("- `Operator Plugin`: Operator-only analysis plugin"));
        assert!(section.contains(
            "operators: 1 programs / 2 operations via `unit_search` / `operator_describe` / `operator_execute`"
        ));
        assert!(section.contains("Operator plugins expose Operator programs"));
    }

    #[test]
    fn retrieval_only_plugins_are_visible_in_system_section() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let plugin_root = tmp.path().join("resource-fixture");
        fs::create_dir_all(&plugin_root).unwrap();
        let retrieval = fixture_retrieval_manifest();
        let outcome = PluginLoadOutcome::from_plugins(vec![LoadedPlugin {
            id: "resource-fixture@test-market".to_string(),
            manifest_name: Some("resource-fixture".to_string()),
            display_name: Some("Resource Fixture".to_string()),
            description: Some("Fixture retrieval-only plugin".to_string()),
            root: plugin_root,
            enabled: true,
            skill_roots: vec![],
            mcp_servers: HashMap::new(),
            apps: vec![],
            retrieval: Some(retrieval),
            error: None,
        }]);

        let section = format_plugins_system_section(&outcome).expect("plugins section");

        assert!(section.contains("Resource Fixture"));
        assert!(section.contains("retrieval routes"));
        assert!(section.contains("`dataset.alpha`"));
        assert!(section.contains("`literature.beta`"));
    }

    #[test]
    fn selected_plugins_system_section_prioritizes_explicit_plugin_mentions() {
        let mut mcp_servers = HashMap::new();
        mcp_servers.insert(
            "sample".to_string(),
            McpServerConfig::Url {
                url: "https://sample.example/mcp".to_string(),
                headers: HashMap::new(),
            },
        );
        let outcome = PluginLoadOutcome::from_plugins(vec![LoadedPlugin {
            id: "sample@market".to_string(),
            manifest_name: Some("sample".to_string()),
            display_name: Some("Sample Plugin".to_string()),
            description: Some("sample capability plugin".to_string()),
            root: PathBuf::from("/tmp/sample"),
            enabled: true,
            skill_roots: vec![PathBuf::from("/tmp/sample/skills")],
            mcp_servers,
            apps: vec![],
            retrieval: None,
            error: None,
        }]);

        let section = format_selected_plugins_system_section(
            &outcome,
            &[
                "sample@market".to_string(),
                "sample@market".to_string(),
                "missing@market".to_string(),
            ],
        )
        .expect("selected plugin section");

        assert!(section.contains("## Explicitly selected plugins for this turn"));
        assert_eq!(section.matches("Sample Plugin").count(), 1);
        assert!(section.contains("composer # picker"));
        assert!(section.contains("Prefer their capabilities"));
        assert!(section.contains("missing@market"));
        assert!(section.contains("do not invent capabilities"));
    }

    #[test]
    fn plugin_load_outcome_keeps_mcp_precedence_deterministic() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let cache_root = tmp.path().join("cache");
        write_cached_plugin(
            &cache_root,
            "market",
            "zeta",
            r#"{"displayName":"Zeta"}"#,
            Some("https://zeta.example/mcp"),
            None,
            false,
        );
        write_cached_plugin(
            &cache_root,
            "market",
            "alpha",
            r#"{"displayName":"Alpha"}"#,
            Some("https://alpha.example/mcp"),
            None,
            false,
        );
        let mut config = PluginConfigFile::default();
        config.plugins.insert(
            "zeta@market".to_string(),
            PluginConfigEntry {
                enabled: true,
                ..Default::default()
            },
        );
        config.plugins.insert(
            "alpha@market".to_string(),
            PluginConfigEntry {
                enabled: true,
                ..Default::default()
            },
        );

        let servers = load_plugins_from_config(&config, &cache_root).effective_mcp_servers();

        match servers.get("sample") {
            Some(McpServerConfig::Url { url, .. }) => {
                assert_eq!(url, "https://zeta.example/mcp")
            }
            other => panic!("expected zeta to win duplicate MCP key, got {other:?}"),
        }
    }

    #[test]
    fn disabled_plugin_is_loaded_but_not_effective() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let cache_root = tmp.path().join("cache");
        write_cached_plugin(
            &cache_root,
            "market",
            "sample",
            r#"{"displayName":"Sample Plugin"}"#,
            Some("https://sample.example/mcp"),
            Some("calendar"),
            true,
        );
        let mut config = PluginConfigFile::default();
        config.plugins.insert(
            "sample@market".to_string(),
            PluginConfigEntry {
                enabled: false,
                ..Default::default()
            },
        );

        let outcome = load_plugins_from_config(&config, &cache_root);

        assert_eq!(outcome.plugins().len(), 1);
        assert!(!outcome.plugins()[0].is_active());
        assert!(outcome.effective_skill_roots().is_empty());
        assert!(outcome.effective_mcp_servers().is_empty());
        assert!(outcome.effective_apps().is_empty());
        assert!(outcome.capability_summaries().is_empty());
    }

    #[test]
    fn unlisted_installed_plugins_are_migrated_then_summarized() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let cache_root = tmp.path().join("cache");
        let legacy_base = cache_root.join("removed-market").join("orphan");
        let plugin_root = cache_root
            .join("removed-market")
            .join("orphan")
            .join("1.0.0");
        fs::create_dir_all(plugin_root.join(".omiga-plugin")).unwrap();
        fs::write(
            plugin_root.join(".omiga-plugin/plugin.json"),
            r#"{
              "name": "orphan",
              "version": "1.0.0",
              "interface": { "displayName": "Orphan Plugin" }
            }"#,
        )
        .unwrap();
        let mut config = PluginConfigFile::default();
        config.plugins.insert(
            "orphan@removed-market".to_string(),
            PluginConfigEntry {
                enabled: true,
                ..Default::default()
            },
        );

        let summaries = unlisted_installed_plugin_summaries(&config, &HashSet::new(), &cache_root);

        assert_eq!(summaries.len(), 1);
        let summary = &summaries[0];
        let typed_root = tmp.path().join("other").join("orphan");
        assert_eq!(summary.id, "orphan@removed-market");
        assert!(summary.installed);
        assert!(summary.enabled);
        assert_eq!(
            summary.installed_path.as_deref(),
            Some(typed_root.to_str().unwrap())
        );
        assert!(!legacy_base.exists());
        assert_eq!(
            summary
                .interface
                .as_ref()
                .and_then(|i| i.display_name.as_deref()),
            Some("Orphan Plugin")
        );
    }

    #[test]
    fn listed_legacy_plugins_are_not_duplicated_after_migration() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let cache_root = tmp.path().join("cache");
        let legacy_base = cache_root.join("market").join("known");
        let plugin_root = cache_root.join("market").join("known").join("local");
        fs::create_dir_all(plugin_root.join(".omiga-plugin")).unwrap();
        fs::write(
            plugin_root.join(".omiga-plugin/plugin.json"),
            r#"{"name":"known","version":"local"}"#,
        )
        .unwrap();
        let config = PluginConfigFile::default();
        let listed_ids = HashSet::from(["known@market".to_string()]);

        let summaries = unlisted_installed_plugin_summaries(&config, &listed_ids, &cache_root);

        assert!(summaries.is_empty());
        assert!(!legacy_base.exists());
    }

    #[test]
    fn external_marketplace_hides_internal_smoke_and_notebook_helper_plugins() {
        let marketplace = read_marketplace(&dev_builtin_marketplace_path()).unwrap();
        for removed in ["operator-smoke", "notebook-helper"] {
            assert!(
                !marketplace
                    .plugins
                    .iter()
                    .any(|entry| entry.name == removed),
                "internal helper plugin `{removed}` should not be marketplace-visible"
            );
        }
    }

    #[test]
    fn external_marketplace_skill_plugins_load_from_manifest_structure() {
        let marketplace_path = curated_marketplace_path();
        let marketplace = read_marketplace(&marketplace_path).unwrap();
        let mut skill_plugin_count = 0;
        let mut skill_count = 0;

        for entry in &marketplace.plugins {
            let source_path = resolve_marketplace_source_path(&marketplace_path, &entry.source)
                .unwrap_or_else(|_| panic!("{} source path", entry.name));
            let manifest = load_plugin_manifest(&source_path)
                .unwrap_or_else(|| panic!("{} plugin manifest", entry.name));
            let skills = plugin_skill_summaries(&source_path, &manifest);
            if manifest.skills.is_some() {
                skill_plugin_count += 1;
                assert!(
                    !skills.is_empty(),
                    "{} declares skills but no skill summaries loaded",
                    entry.name
                );
                skill_count += skills.len();
            }
        }

        assert!(
            skill_plugin_count > 0,
            "marketplace should contain at least one skill plugin"
        );
        assert!(
            skill_count > 0,
            "skill plugins should expose at least one skill"
        );
    }

    #[test]
    fn external_marketplace_operator_plugins_load_from_manifest_structure() {
        let marketplace_path = curated_marketplace_path();
        let marketplace = read_marketplace(&marketplace_path).unwrap();
        let mut operator_plugin_count = 0;
        let mut operator_count = 0;
        let mut operation_count = 0;

        for entry in &marketplace.plugins {
            let source_path = resolve_marketplace_source_path(&marketplace_path, &entry.source)
                .unwrap_or_else(|_| panic!("{} source path", entry.name));
            let manifest = load_plugin_manifest(&source_path)
                .unwrap_or_else(|| panic!("{} plugin manifest", entry.name));
            let Some(operators_root) = manifest.operators.as_ref() else {
                continue;
            };
            operator_plugin_count += 1;
            assert_eq!(
                plugin_kind_for_manifest(&source_path, entry.category.as_deref(), &manifest),
                PluginKind::Operator
            );

            let mut ids = HashSet::new();
            for dir_entry in fs::read_dir(operators_root).unwrap().flatten() {
                let manifest_path = dir_entry.path().join("operator.yaml");
                if !manifest_path.is_file() {
                    continue;
                }
                let operator = crate::domain::operators::load_operator_manifest(
                    &manifest_path,
                    &format!("{}@{}", entry.name, marketplace.name),
                    &source_path,
                )
                .unwrap_or_else(|err| panic!("{}: {err}", manifest_path.display()));
                assert!(
                    ids.insert(operator.metadata.id.clone()),
                    "duplicate operator id `{}` in {}",
                    operator.metadata.id,
                    entry.name
                );
                assert!(
                    !operator.operations.is_empty(),
                    "operator `{}` should expose at least one operation",
                    operator.metadata.id
                );
                operator_count += 1;
                operation_count += operator.operations.len();
            }
        }

        assert!(
            operator_plugin_count > 0,
            "marketplace should contain at least one operator plugin"
        );
        assert!(
            operator_count > 0,
            "operator plugins should expose operators"
        );
        assert!(
            operation_count >= operator_count,
            "each operator should expose operations"
        );
    }

    #[test]
    fn external_marketplace_entries_resolve_without_core_plugin_name_allowlist() {
        let marketplace_path = curated_marketplace_path();
        let marketplace = read_marketplace(&marketplace_path).unwrap();
        assert!(
            !marketplace.plugins.is_empty(),
            "marketplace should not be empty"
        );

        let mut by_category = BTreeMap::<String, usize>::new();
        let mut retrieval_plugins = 0usize;
        let mut template_plugins = 0usize;
        let mut operator_plugins = 0usize;

        for entry in &marketplace.plugins {
            assert_eq!(entry.policy.installation, PluginInstallPolicy::Available);
            assert_eq!(entry.policy.authentication, PluginAuthPolicy::OnUse);
            let category = entry
                .category
                .as_deref()
                .unwrap_or("Uncategorized")
                .to_string();
            *by_category.entry(category).or_default() += 1;

            let source_path = resolve_marketplace_source_path(&marketplace_path, &entry.source)
                .unwrap_or_else(|_| panic!("{} source path", entry.name));
            let manifest = load_plugin_manifest(&source_path)
                .unwrap_or_else(|| panic!("{} plugin manifest", entry.name));
            assert_eq!(
                manifest.name, entry.name,
                "marketplace entry name should match plugin manifest name"
            );

            if manifest.retrieval.is_some() {
                retrieval_plugins += 1;
            }
            if manifest.templates.is_some() {
                template_plugins += 1;
            }
            if manifest.operators.is_some() {
                operator_plugins += 1;
            }
        }

        assert!(
            by_category.len() > 1,
            "marketplace categories should come from entries"
        );
        assert!(
            retrieval_plugins > 0,
            "marketplace should expose retrieval plugins"
        );
        assert!(
            template_plugins > 0,
            "marketplace should expose template plugins"
        );
        assert!(
            operator_plugins > 0,
            "marketplace should expose operator plugins"
        );
    }

    #[test]
    fn external_marketplace_retrieval_plugins_load_declared_routes() {
        let marketplace_path = curated_marketplace_path();
        let marketplace = read_marketplace(&marketplace_path).unwrap();
        let mut retrieval_plugin_count = 0usize;
        let mut route_count = 0usize;

        for entry in &marketplace.plugins {
            let source_path = resolve_marketplace_source_path(&marketplace_path, &entry.source)
                .unwrap_or_else(|_| panic!("{} source path", entry.name));
            let manifest = load_plugin_manifest(&source_path)
                .unwrap_or_else(|| panic!("{} plugin manifest", entry.name));
            let Some(retrieval) = manifest.retrieval.as_ref() else {
                continue;
            };
            retrieval_plugin_count += 1;
            route_count += retrieval.resources.len();
            let summary = plugin_summary_from_marketplace_entry(
                &marketplace_path,
                &marketplace.name,
                entry,
                &PluginConfigFile::default(),
            )
            .unwrap();
            let summary_routes = summary
                .retrieval
                .as_ref()
                .map(|retrieval| {
                    retrieval
                        .resources
                        .iter()
                        .map(|source| format!("{}.{}", source.category, source.id))
                        .collect::<HashSet<_>>()
                })
                .unwrap_or_default();
            let manifest_routes = retrieval
                .resources
                .iter()
                .map(|source| format!("{}.{}", source.category, source.id))
                .collect::<HashSet<_>>();
            assert_eq!(summary_routes, manifest_routes);
            assert!(
                retrieval
                    .resources
                    .iter()
                    .all(|source| !source.id.trim().is_empty()
                        && !source.category.trim().is_empty()
                        && !source.capabilities.is_empty()),
                "{} has an incomplete retrieval resource declaration",
                entry.name
            );
        }

        assert!(
            retrieval_plugin_count > 0,
            "marketplace should contain at least one retrieval plugin"
        );
        assert!(route_count > 0, "retrieval plugins should expose routes");
    }

    #[test]
    fn retrieval_resource_config_exposes_only_explicit_provider_routes() {
        let retrieval = fixture_retrieval_manifest();
        let mut config = PluginConfigFile::default();
        config.plugins.insert(
            "fixture-resource@test-market".to_string(),
            PluginConfigEntry {
                enabled: true,
                enabled_retrieval_resources: HashSet::from(["literature.beta".to_string()]),
                disabled_retrieval_resources: HashSet::from(["dataset.alpha".to_string()]),
                ..Default::default()
            },
        );

        let summary =
            plugin_retrieval_summary(Some(&retrieval), "fixture-resource@test-market", &config)
                .expect("retrieval summary");
        let alpha = summary
            .resources
            .iter()
            .find(|source| source.category == "dataset" && source.id == "alpha")
            .expect("alpha route summary");
        let beta = summary
            .resources
            .iter()
            .find(|source| source.category == "literature" && source.id == "beta")
            .expect("beta route summary");
        assert!(!alpha.exposed);
        assert!(beta.exposed);

        let entry = config
            .plugins
            .get("fixture-resource@test-market")
            .expect("plugin config");
        let filtered =
            filter_retrieval_manifest_for_config(retrieval, "fixture-resource@test-market", entry)
                .expect("filtered retrieval");
        assert!(!filtered
            .resources
            .iter()
            .any(|source| source.category == "dataset" && source.id == "alpha"));
        assert!(filtered
            .resources
            .iter()
            .any(|source| source.category == "literature" && source.id == "beta"));
        assert!(filtered
            .resources
            .iter()
            .all(|source| source.default_enabled));
    }

    #[test]
    fn legacy_enabled_retrieval_plugin_exposes_all_non_disabled_routes() {
        let retrieval = fixture_retrieval_manifest();
        assert!(retrieval
            .resources
            .iter()
            .all(|source| !source.default_enabled));

        let mut config = PluginConfigFile::default();
        config.plugins.insert(
            "fixture-resource@test-market".to_string(),
            PluginConfigEntry {
                enabled: true,
                disabled_retrieval_resources: HashSet::from(["dataset.alpha".to_string()]),
                ..Default::default()
            },
        );

        let summary =
            plugin_retrieval_summary(Some(&retrieval), "fixture-resource@test-market", &config)
                .expect("retrieval summary");
        let enabled_routes = summary
            .resources
            .iter()
            .filter(|source| source.exposed)
            .map(|source| format!("{}.{}", source.category, source.id))
            .collect::<HashSet<_>>();
        assert!(!enabled_routes.contains("dataset.alpha"));
        assert!(enabled_routes.contains("literature.beta"));
        assert!(enabled_routes.contains("knowledge.gamma"));

        let entry = config
            .plugins
            .get("fixture-resource@test-market")
            .expect("plugin config");
        let filtered =
            filter_retrieval_manifest_for_config(retrieval, "fixture-resource@test-market", entry)
                .expect("filtered retrieval");
        assert!(!filtered
            .resources
            .iter()
            .any(|source| source.category == "dataset" && source.id == "alpha"));
        assert!(filtered
            .resources
            .iter()
            .any(|source| source.category == "literature" && source.id == "beta"));
        assert!(filtered
            .resources
            .iter()
            .all(|source| source.default_enabled));
    }

    #[test]
    fn legacy_retrieval_config_materializes_explicit_resources_before_toggle() {
        let retrieval = fixture_retrieval_manifest();
        let mut entry = PluginConfigEntry {
            enabled: true,
            disabled_retrieval_resources: HashSet::from(["dataset.alpha".to_string()]),
            ..Default::default()
        };

        materialize_retrieval_resource_config(&mut entry, &retrieval);

        assert!(entry.retrieval_resources_configured);
        assert!(!entry.enabled_retrieval_resources.contains("dataset.alpha"));
        assert!(entry
            .enabled_retrieval_resources
            .contains("literature.beta"));
        assert!(entry
            .enabled_retrieval_resources
            .contains("knowledge.gamma"));
    }

    #[test]
    fn explicitly_configured_empty_retrieval_resource_set_stays_disabled() {
        let retrieval = fixture_retrieval_manifest();
        let entry = PluginConfigEntry {
            enabled: true,
            retrieval_resources_configured: true,
            ..Default::default()
        };

        assert!(filter_retrieval_manifest_for_config(
            retrieval,
            "fixture-resource@test-market",
            &entry
        )
        .is_none());
    }

    #[test]
    fn external_marketplace_template_summaries_are_manifest_driven() {
        let marketplace_path = curated_marketplace_path();
        let marketplace = read_marketplace(&marketplace_path).unwrap();
        let mut template_plugin_count = 0usize;
        let mut template_count = 0usize;

        for entry in &marketplace.plugins {
            let source_path = resolve_marketplace_source_path(&marketplace_path, &entry.source)
                .unwrap_or_else(|_| panic!("{} source path", entry.name));
            let manifest = load_plugin_manifest(&source_path)
                .unwrap_or_else(|| panic!("{} plugin manifest", entry.name));
            if manifest.templates.is_none() {
                continue;
            }
            let detail = read_plugin(&marketplace_path, &entry.name)
                .unwrap_or_else(|_| panic!("{} plugin detail", entry.name));
            let Some(template_summary) = detail.summary.templates.as_ref() else {
                panic!(
                    "{} declares templates but has no template summary",
                    entry.name
                );
            };
            template_plugin_count += 1;
            template_count += template_summary.count;
            assert_eq!(
                template_summary.count,
                template_summary
                    .groups
                    .iter()
                    .map(|group| group.count)
                    .sum::<usize>()
            );
            for template in template_summary
                .groups
                .iter()
                .flat_map(|group| group.templates.iter())
            {
                assert_eq!(template.execute["tool"], "template_execute");
                assert!(
                    template.execute["arguments"]["id"].as_str().is_some_and(
                        |id| id.starts_with(&format!("{}@{}", entry.name, marketplace.name))
                    ),
                    "template execute id should be scoped to the source plugin: {}",
                    template.execute
                );
            }
        }

        assert!(
            template_plugin_count > 0,
            "marketplace should contain at least one template plugin"
        );
        assert!(
            template_count > 0,
            "template plugins should expose templates"
        );
    }
}
