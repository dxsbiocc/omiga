//! Omiga native plugin discovery, marketplace, installation, and runtime contribution loading.
//!
//! A plugin is an Omiga-native extension bundle: skills, MCP server configs, app connector
//! references, and UI metadata. It intentionally does not execute VS Code extension code.

use crate::domain::environments::{
    check_environment_profile, discover_environment_manifest_paths, environment_summary,
    load_environment_manifest, EnvironmentCheckResult, EnvironmentProfileSummary,
};
use crate::domain::mcp::config::{servers_from_mcp_json, McpServerConfig};
use crate::domain::plugin_runtime::retrieval::lifecycle::{
    PluginLifecycleKey, PluginLifecycleRouteStatus, PluginLifecycleState,
};
use crate::domain::plugin_runtime::retrieval::manifest::{
    load_plugin_retrieval_manifest, PluginRetrievalManifest,
};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;
use tokio::time::Instant;

pub const PLUGIN_MANIFEST_FILE: &str = "plugin.json";
pub const OMIGA_PLUGIN_MANIFEST_PATH: &str = ".omiga-plugin/plugin.json";
pub const CODEX_PLUGIN_MANIFEST_PATH: &str = ".codex-plugin/plugin.json";
const MARKETPLACE_FILE_NAME: &str = "marketplace.json";
const USER_PLUGINS_CONFIG_FILE: &str = "plugins/config.json";
const PLUGINS_CACHE_DIR: &str = "plugins/cache";
const PLUGINS_ROOT_DIR: &str = "plugins";
const RESOURCE_RUNNERS_DIR: &str = "resource_runners";
const LEGACY_SOURCE_RUNNERS_DIR: &str = "source_runners";
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

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginMarketplaceEntry {
    pub name: String,
    pub path: String,
    pub interface: Option<MarketplaceInterface>,
    pub remote: Option<MarketplaceRemote>,
    pub plugins: Vec<PluginSummary>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
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

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
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
        "Omiga plugins are native capability bundles: skills, MCP server configs, app connector references, and UI metadata. They do not run VS Code extension code or require a VS Code Extension Host.".to_string(),
        String::new(),
        "### Available plugins".to_string(),
    ];

    for plugin in plugins {
        lines.push(format_plugin_capability_line(plugin));
    }

    lines.push(String::new());
    lines.push("### How to use plugins".to_string());
    lines.push(
        "- Plugins are not invoked directly; use their underlying skills, MCP tools, or explicitly available app tools.\n\
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
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct PluginConfigEntry {
    #[serde(default)]
    enabled: bool,
    #[serde(default)]
    disabled_templates: HashSet<String>,
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

fn plugin_store_root_from_cache_root(cache_root: &Path) -> PathBuf {
    cache_root
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(plugin_store_root)
}

pub fn dev_builtin_marketplace_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("bundled_plugins")
        .join(MARKETPLACE_FILE_NAME)
}

fn resource_builtin_marketplace_path(resource_dir: &Path) -> PathBuf {
    resource_dir
        .join("bundled_plugins")
        .join(MARKETPLACE_FILE_NAME)
}

fn user_marketplace_path() -> PathBuf {
    omiga_home().join("plugins").join(MARKETPLACE_FILE_NAME)
}

fn project_marketplace_path(project_root: &Path) -> PathBuf {
    project_root
        .join(".omiga")
        .join("plugins")
        .join(MARKETPLACE_FILE_NAME)
}

pub fn marketplace_paths(project_root: Option<&Path>, resource_dir: Option<&Path>) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let builtin = dev_builtin_marketplace_path();
    if builtin.is_file() {
        paths.push(builtin);
    }
    if let Some(resource_dir) = resource_dir {
        let builtin = resource_builtin_marketplace_path(resource_dir);
        if builtin.is_file() && !paths.contains(&builtin) {
            paths.push(builtin);
        }
    }
    let user = user_marketplace_path();
    if user.is_file() && !paths.contains(&user) {
        paths.push(user);
    }
    if let Some(root) = project_root {
        let project = project_marketplace_path(root);
        if project.is_file() && !paths.contains(&project) {
            paths.push(project);
        }
    }
    paths
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
            tracing::warn!("failed to persist superseded bundled plugin migration: {err}");
        }
    }
    config
}

fn superseded_builtin_plugin_replacement(plugin_name: &str) -> Option<&'static str> {
    match plugin_name {
        "operator-pca-r" | "operator-differential-expression-r" | "operator-enrichment-r" => {
            Some("transcriptomics")
        }
        "operator-pubmed-search"
        | "operator-geo-search"
        | "retrieval-dataset-geo"
        | "retrieval-dataset-biosample"
        | "retrieval-dataset-ncbi-datasets"
        | "retrieval-literature-pubmed"
        | "retrieval-knowledge-ncbi-gene" => Some("resource-ncbi"),
        "retrieval-dataset-ena"
        | "retrieval-dataset-arrayexpress"
        | "retrieval-knowledge-ensembl" => Some("resource-embl-ebi"),
        "operator-uniprot-search" => Some("retrieval-knowledge-uniprot"),
        _ => None,
    }
}

fn removed_builtin_plugin(plugin_name: &str) -> bool {
    matches!(plugin_name, "operator-smoke" | "notebook-helper")
}

fn migrate_superseded_builtin_plugin_config(config: &mut PluginConfigFile) -> bool {
    let mut changed = false;
    let mut replacements = HashMap::<String, bool>::new();
    let keys = config.plugins.keys().cloned().collect::<Vec<_>>();
    for key in keys {
        let Ok(plugin_id) = PluginId::parse(&key) else {
            continue;
        };
        if plugin_id.marketplace != "omiga-curated" {
            continue;
        }
        if removed_builtin_plugin(&plugin_id.name) {
            config.plugins.remove(&key);
            changed = true;
            continue;
        }
        let Some(replacement_name) = superseded_builtin_plugin_replacement(&plugin_id.name) else {
            continue;
        };
        let enabled = config
            .plugins
            .remove(&key)
            .map(|entry| entry.enabled)
            .unwrap_or(false);
        replacements
            .entry(format!("{replacement_name}@omiga-curated"))
            .and_modify(|value| *value = *value || enabled)
            .or_insert(enabled);
        changed = true;
    }

    for (key, enabled) in replacements {
        if enabled {
            config.plugins.entry(key).or_default().enabled = true;
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
    let resource_exists = manifest
        .retrieval
        .as_ref()
        .map(|retrieval| {
            retrieval
                .resources
                .iter()
                .any(|source| source.category == category && source.id == source_id)
        })
        .unwrap_or(false);
    if !resource_exists {
        return Err(format!(
            "retrieval resource `{category}.{source_id}` was not found in plugin `{}`",
            plugin_id.key()
        ));
    }

    let mut config = read_config();
    let entry = config.plugins.entry(plugin_id.key()).or_default();
    let key = retrieval_resource_config_key(&category, &source_id);
    if enabled {
        entry.disabled_retrieval_resources.remove(&key);
    } else {
        entry.disabled_retrieval_resources.insert(key);
    }
    write_config(&config)
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
) -> bool {
    let key = retrieval_resource_config_key(category, source_id);
    !config
        .plugins
        .get(source_plugin)
        .map(|entry| entry.disabled_retrieval_resources.contains(&key))
        .unwrap_or(false)
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
    let mut ids = config
        .plugins
        .iter()
        .filter(|(_, entry)| entry.enabled)
        .filter_map(|(key, _)| PluginId::parse(key).ok())
        .collect::<Vec<_>>();
    ids.sort_by_key(PluginId::key);
    ids
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
    if !is_allowed_plugin_environment_check_command(&command) {
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

    let conda_file = plugin_conda_environment_file(profile)?;
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
    let Some(executable) = command.first() else {
        return false;
    };
    let basename = Path::new(executable)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(executable)
        .trim()
        .to_ascii_lowercase();
    let combined = format!("{stdout}\n{stderr}").to_ascii_lowercase();
    basename == "bwa" && combined.contains("version")
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

fn is_allowed_plugin_environment_check_command(command: &[String]) -> bool {
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
    matches!(
        basename.as_str(),
        "true"
            | "rscript"
            | "python"
            | "python3"
            | "conda"
            | "mamba"
            | "micromamba"
            | "docker"
            | "singularity"
            | "apptainer"
            | "bwa"
            | "bowtie2"
            | "bowtie2-build"
            | "star"
            | "hisat2"
            | "hisat2-build"
            | "samtools"
    ) && version_arg
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
    };

    (summary.has_skills
        || !summary.mcp_servers.is_empty()
        || !summary.apps.is_empty()
        || !summary.retrieval_routes.is_empty())
    .then_some(summary)
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
    retrieval.resources.retain(|source| {
        !entry
            .disabled_retrieval_resources
            .contains(&retrieval_resource_config_key(&source.category, &source.id))
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
    refresh_configured_builtin_plugins_best_effort(config, cache_root);
    repair_configured_builtin_resource_runner_assets(config, cache_root);
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
    let marketplace_path = dev_builtin_marketplace_path();
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
        let has_stale_typed_root = typed_plugin_base_roots(&store_root, &plugin_id)
            .into_iter()
            .any(|candidate| candidate != target_base && candidate.exists());
        if !has_stale_typed_root {
            continue;
        }
        remove_other_typed_plugin_roots(&store_root, &plugin_id, &target_base)?;
        replace_plugin_root_atomically(&source_path, &target_base)?;
        refreshed += 1;
    }

    Ok(refreshed)
}

fn refresh_configured_builtin_plugins_best_effort(config: &PluginConfigFile, cache_root: &Path) {
    match refresh_configured_builtin_plugins(config, cache_root) {
        Ok(refreshed) if refreshed > 0 => tracing::info!(
            count = refreshed,
            "refreshed configured bundled plugin installs"
        ),
        Ok(_) => {}
        Err(err) => tracing::warn!("failed to refresh configured bundled plugin installs: {err}"),
    }
}

fn unlisted_installed_plugin_summaries(
    config: &PluginConfigFile,
    listed_ids: &HashSet<String>,
    cache_root: &Path,
) -> Vec<PluginSummary> {
    migrate_legacy_plugin_cache_best_effort(cache_root);
    let mut ids = config
        .plugins
        .keys()
        .filter_map(|key| PluginId::parse(key).ok())
        .collect::<Vec<_>>();
    ids.sort_by_key(PluginId::key);
    ids.dedup_by(|a, b| a.key() == b.key());

    ids.into_iter()
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
    refresh_configured_builtin_plugins_best_effort(&config, &cache_root);
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
        if let Err(err) =
            copy_marketplace_resource_runner_assets(&path, &marketplace.name, &cache_root)
        {
            tracing::warn!(
                path = %path.display(),
                marketplace = %marketplace.name,
                "failed to copy plugin resource runner assets: {err}"
            );
        }
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
        if plugins.is_empty() {
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

fn repair_configured_builtin_resource_runner_assets(config: &PluginConfigFile, cache_root: &Path) {
    let marketplace_path = dev_builtin_marketplace_path();
    let Ok(marketplace) = read_marketplace(&marketplace_path) else {
        return;
    };
    let has_configured_plugin = config.plugins.keys().any(|key| {
        PluginId::parse(key)
            .map(|plugin_id| plugin_id.marketplace == marketplace.name)
            .unwrap_or(false)
    });
    if !has_configured_plugin {
        return;
    }
    if let Err(err) =
        copy_marketplace_resource_runner_assets(&marketplace_path, &marketplace.name, cache_root)
    {
        tracing::warn!(
            marketplace = %marketplace.name,
            path = %marketplace_path.display(),
            "failed to repair plugin resource runner assets: {err}"
        );
    }
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

fn replace_plugin_root_atomically(source: &Path, target_base: &Path) -> Result<PathBuf, String> {
    let parent = target_base.parent().ok_or_else(|| {
        format!(
            "plugin install path has no parent: {}",
            target_base.display()
        )
    })?;
    fs::create_dir_all(parent).map_err(|err| format!("create plugin install dir: {err}"))?;
    let staged_base = parent.join(format!(".install-{}", uuid::Uuid::new_v4()));
    copy_dir_recursive(source, &staged_base)?;

    if target_base.exists() {
        remove_path_if_exists(target_base)?;
    }
    fs::rename(&staged_base, target_base)
        .map_err(|err| format!("activate plugin install entry: {err}"))?;
    Ok(target_base.to_path_buf())
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
    remove_other_typed_plugin_roots(&store_root, &plugin_id, &target_base)?;
    copy_marketplace_resource_runner_assets(
        marketplace_path,
        &marketplace.name,
        &plugin_cache_root(),
    )?;
    let installed_path = replace_plugin_root_atomically(&source_path, &target_base)?;
    record_plugin_install_state(&installed_path, &plugin_id, manifest.version.clone(), None)?;
    set_plugin_enabled(&plugin_id.key(), true)?;
    Ok(PluginInstallResult {
        plugin_id: plugin_id.key(),
        installed_path: installed_path.to_string_lossy().into_owned(),
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
        remove_other_typed_plugin_roots(&store_root, &plugin_id, &target_base)?;
        copy_marketplace_resource_runner_assets(
            marketplace_path,
            &marketplace.name,
            &plugin_cache_root(),
        )?;
        let installed_path = replace_plugin_root_atomically(&source_path, &target_base)?;
        let installed_at = install_state
            .as_ref()
            .map(|state| state.installed_at.clone());
        record_plugin_install_state(
            &installed_path,
            &plugin_id,
            manifest.version.clone(),
            installed_at,
        )?;
        return Ok(PluginSyncResult {
            plugin_id: plugin_id.key(),
            status: "forceSynced".to_string(),
            installed_path: installed_path.to_string_lossy().into_owned(),
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

    #[test]
    fn superseded_bundled_plugin_config_migrates_to_aggregate_plugins() {
        let mut config = PluginConfigFile::default();
        for key in [
            "operator-pca-r@omiga-curated",
            "operator-differential-expression-r@omiga-curated",
            "retrieval-dataset-geo@omiga-curated",
            "retrieval-literature-pubmed@omiga-curated",
            "retrieval-dataset-ena@omiga-curated",
            "retrieval-knowledge-ensembl@omiga-curated",
            "operator-uniprot-search@omiga-curated",
            "operator-smoke@omiga-curated",
            "notebook-helper@omiga-curated",
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
            "third-party-old@custom-market".to_string(),
            PluginConfigEntry {
                enabled: true,
                ..Default::default()
            },
        );

        assert!(migrate_superseded_builtin_plugin_config(&mut config));

        assert_eq!(
            config
                .plugins
                .get("transcriptomics@omiga-curated")
                .unwrap()
                .enabled,
            true
        );
        assert_eq!(
            config
                .plugins
                .get("resource-ncbi@omiga-curated")
                .unwrap()
                .enabled,
            true
        );
        assert_eq!(
            config
                .plugins
                .get("resource-embl-ebi@omiga-curated")
                .unwrap()
                .enabled,
            true
        );
        assert_eq!(
            config
                .plugins
                .get("retrieval-knowledge-uniprot@omiga-curated")
                .unwrap()
                .enabled,
            true
        );
        assert!(config.plugins.contains_key("third-party-old@custom-market"));
        assert!(!config.plugins.contains_key("operator-pca-r@omiga-curated"));
        assert!(!config
            .plugins
            .contains_key("retrieval-literature-pubmed@omiga-curated"));
        assert!(!config.plugins.contains_key("operator-smoke@omiga-curated"));
        assert!(!config.plugins.contains_key("notebook-helper@omiga-curated"));
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
        let builtin = Path::new(env!("CARGO_MANIFEST_DIR")).join("bundled_plugins/plugins");
        let operator_root = builtin.join("operator-pca-r");
        let analysis_root = builtin.join("transcriptomics");
        let source_root = builtin.join("retrieval-dataset-geo");
        let workflow_root = builtin.join("notebook-helper");
        let visualization_root = builtin.join("visualization-r");

        let operator_manifest = load_plugin_manifest(&operator_root).expect("operator manifest");
        let analysis_manifest = load_plugin_manifest(&analysis_root).expect("analysis manifest");
        let source_manifest = load_plugin_manifest(&source_root).expect("source manifest");
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
            plugin_kind_for_manifest(&source_root, Some("Retrieval"), &source_manifest),
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
        let plugin_id = PluginId::new("visualization-r", "omiga-curated").unwrap();
        let old_operator_root = write_typed_plugin(
            &store_root,
            PluginKind::Operator,
            "visualization-r",
            r#"{"displayName":"Old Operator","category":"Operator"}"#,
            false,
        );
        let workflow_root = write_typed_plugin(
            &store_root,
            PluginKind::Workflow,
            "visualization-r",
            r#"{"displayName":"R Visualization","category":"Visualization"}"#,
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
    fn configured_bundled_plugins_refresh_from_current_source_metadata() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store_root = tmp.path().join("plugins");
        let cache_root = store_root.join("cache");
        let old_operator_root = write_typed_plugin(
            &store_root,
            PluginKind::Operator,
            "visualization-r",
            r#"{"displayName":"R Visualization Templates","category":"Operator"}"#,
            false,
        );
        let mut config = PluginConfigFile::default();
        config.plugins.insert(
            "visualization-r@omiga-curated".to_string(),
            PluginConfigEntry {
                enabled: true,
                ..Default::default()
            },
        );

        let refreshed = refresh_configured_builtin_plugins(&config, &cache_root)
            .expect("refresh bundled plugin");

        let workflow_root = store_root.join("workflow").join("visualization-r");
        let manifest = load_plugin_manifest(&workflow_root).expect("refreshed manifest");
        assert_eq!(refreshed, 1);
        assert!(!old_operator_root.exists());
        assert_eq!(
            manifest
                .interface
                .and_then(|interface| interface.display_name),
            Some("R Visualization".to_string())
        );
        assert!(workflow_root
            .join("skills")
            .join("visualize-r")
            .join("SKILL.md")
            .is_file());
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
            ("transcriptomics".to_string(), Some("0.1.0".to_string())),
        ]);
        let remote_without_versions = BTreeMap::from([
            ("alignment".to_string(), None),
            ("transcriptomics".to_string(), None),
        ]);
        assert!(changed_marketplace_plugins(&local, &remote_without_versions).is_empty());

        let remote_with_change = BTreeMap::from([
            ("alignment".to_string(), Some("0.2.0".to_string())),
            ("transcriptomics".to_string(), None),
            ("new-plugin".to_string(), Some("0.1.0".to_string())),
        ]);
        assert_eq!(
            changed_marketplace_plugins(&local, &remote_with_change),
            vec!["alignment".to_string(), "new-plugin".to_string()]
        );
    }

    #[test]
    fn marketplace_resource_runners_are_copied_to_resource_root() {
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
        assert!(section.contains("Do not assume VS Code extension UI/runtime behavior"));
    }

    #[test]
    fn retrieval_only_plugins_are_visible_in_system_section() {
        let plugin_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("bundled_plugins/plugins/retrieval-dataset-biosample");
        let manifest = load_plugin_manifest(&plugin_root).expect("biosample plugin manifest");
        let outcome = PluginLoadOutcome::from_plugins(vec![LoadedPlugin {
            id: "retrieval-dataset-biosample@omiga-curated".to_string(),
            manifest_name: Some(manifest.name.clone()),
            display_name: manifest
                .interface
                .as_ref()
                .and_then(|interface| interface.display_name.clone()),
            description: manifest.description.clone(),
            root: plugin_root,
            enabled: true,
            skill_roots: vec![],
            mcp_servers: HashMap::new(),
            apps: vec![],
            retrieval: manifest.retrieval.clone(),
            error: None,
        }]);

        let section = format_plugins_system_section(&outcome).expect("plugins section");

        assert!(section.contains("BioSample Retrieval Resource"));
        assert!(section.contains("retrieval routes"));
        assert!(section.contains("`dataset.biosample`"));
        assert!(!section.contains("`dataset.arrayexpress`"));
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
    fn bundled_marketplace_hides_internal_smoke_and_notebook_helper_plugins() {
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
    fn bundled_marketplace_exposes_omiga_plugin_creator_skill() {
        let marketplace = read_marketplace(&dev_builtin_marketplace_path()).unwrap();
        let entry = marketplace
            .plugins
            .iter()
            .find(|entry| entry.name == "omiga-developer-tools")
            .expect("omiga developer tools marketplace entry");
        assert_eq!(entry.category.as_deref(), Some("Tools"));
        assert_eq!(entry.policy.authentication, PluginAuthPolicy::OnUse);

        let source_path =
            resolve_marketplace_source_path(&dev_builtin_marketplace_path(), &entry.source)
                .expect("developer tools source path");
        let manifest = load_plugin_manifest(&source_path).expect("developer tools manifest");
        assert_eq!(
            plugin_kind_for_manifest(&source_path, entry.category.as_deref(), &manifest),
            PluginKind::Tool
        );

        let skills = plugin_skill_summaries(&source_path, &manifest);
        assert!(
            skills.iter().any(|skill| skill.name == "plugin-creator"),
            "developer tools should contribute plugin-creator"
        );
    }

    #[test]
    fn bundled_marketplace_exposes_ngs_alignment_operator_bundle() {
        let marketplace = read_marketplace(&dev_builtin_marketplace_path()).unwrap();
        let entry = marketplace
            .plugins
            .iter()
            .find(|entry| entry.name == "ngs-alignment")
            .expect("ngs alignment marketplace entry");
        assert_eq!(entry.category.as_deref(), Some("Bioinformatics"));

        let source_path =
            resolve_marketplace_source_path(&dev_builtin_marketplace_path(), &entry.source)
                .expect("ngs alignment source path");
        let manifest = load_plugin_manifest(&source_path).expect("ngs alignment manifest");
        assert_eq!(
            manifest
                .interface
                .as_ref()
                .and_then(|interface| interface.display_name.as_deref()),
            Some("Alignment")
        );
        assert_eq!(
            plugin_kind_for_manifest(&source_path, entry.category.as_deref(), &manifest),
            PluginKind::Operator
        );

        let operators_root = manifest.operators.as_ref().expect("operators root");
        let mut ids = fs::read_dir(operators_root)
            .unwrap()
            .flatten()
            .filter_map(|entry| {
                let manifest_path = entry.path().join("operator.yaml");
                manifest_path.is_file().then(|| {
                    crate::domain::operators::load_operator_manifest(
                        &manifest_path,
                        "ngs-alignment@omiga-curated",
                        &source_path,
                    )
                })
            })
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
            .into_iter()
            .map(|operator| operator.metadata.id)
            .collect::<Vec<_>>();
        ids.sort();
        assert_eq!(
            ids,
            vec![
                "bowtie2_align_reads",
                "bowtie2_build_reference",
                "bwa_index_reference",
                "bwa_mem_align_reads",
                "hisat2_align_reads",
                "hisat2_build_reference",
                "samtools_alignment_utility",
                "star_align_reads",
                "star_generate_genome_index",
            ]
        );

        let envs = plugin_environment_summaries(
            &source_path,
            "ngs-alignment@omiga-curated",
            &PluginConfigFile::default(),
        );
        assert_eq!(envs.len(), 5);
        assert_eq!(
            envs.iter().map(|env| env.id.as_str()).collect::<Vec<_>>(),
            vec![
                "ngs-bowtie2",
                "ngs-bwa",
                "ngs-hisat2",
                "ngs-samtools",
                "ngs-star"
            ]
        );
        assert!(envs.iter().all(|env| env.runtime_type == "conda"));
        assert!(envs
            .iter()
            .all(|env| env.runtime_file_kind.as_deref() == Some("conda.yaml|conda.yml")));
        assert!(envs.iter().all(|env| !env.availability_message.is_empty()));
    }

    #[test]
    fn bundled_marketplace_exposes_provider_level_retrieval_resource_plugins() {
        let marketplace = read_marketplace(&dev_builtin_marketplace_path()).unwrap();
        for removed in [
            "public-dataset-sources",
            "public-literature-sources",
            "public-knowledge-sources",
            "operator-pubmed-search",
            "operator-geo-search",
            "operator-uniprot-search",
            "retrieval-dataset-geo",
            "retrieval-dataset-ena",
            "retrieval-dataset-biosample",
            "retrieval-dataset-arrayexpress",
            "retrieval-dataset-ncbi-datasets",
            "retrieval-literature-pubmed",
            "retrieval-knowledge-ncbi-gene",
            "retrieval-knowledge-ensembl",
        ] {
            assert!(
                !marketplace
                    .plugins
                    .iter()
                    .any(|entry| entry.name == removed),
                "database-level retrieval plugin `{removed}` should not be marketplace-visible"
            );
        }

        let cases = [
            (
                "resource-ncbi",
                "NCBI",
                vec![
                    "dataset.biosample",
                    "dataset.geo",
                    "dataset.ncbi_datasets",
                    "knowledge.ncbi_gene",
                    "literature.pubmed",
                ],
            ),
            (
                "resource-embl-ebi",
                "EMBL-EBI",
                vec![
                    "dataset.arrayexpress",
                    "dataset.ena",
                    "dataset.ena_analysis",
                    "dataset.ena_assembly",
                    "dataset.ena_experiment",
                    "dataset.ena_run",
                    "dataset.ena_sample",
                    "dataset.ena_sequence",
                    "knowledge.ensembl",
                ],
            ),
            (
                "retrieval-dataset-gtex",
                "GTEx Retrieval Resource",
                vec!["dataset.gtex"],
            ),
            (
                "retrieval-dataset-cbioportal",
                "cBioPortal Retrieval Resource",
                vec!["dataset.cbioportal"],
            ),
            (
                "retrieval-literature-semantic-scholar",
                "Semantic Scholar Retrieval Resource",
                vec!["literature.semantic_scholar"],
            ),
            (
                "retrieval-knowledge-uniprot",
                "UniProt Retrieval Resource",
                vec!["knowledge.uniprot"],
            ),
        ];

        for (plugin_name, display_name, expected_routes) in cases {
            let entry = marketplace
                .plugins
                .iter()
                .find(|entry| entry.name == plugin_name)
                .unwrap_or_else(|| panic!("{plugin_name} marketplace entry"));
            assert_eq!(entry.category.as_deref(), Some("Retrieval"));
            assert_eq!(entry.policy.authentication, PluginAuthPolicy::OnUse);

            let source_path =
                resolve_marketplace_source_path(&dev_builtin_marketplace_path(), &entry.source)
                    .unwrap();
            let summary = plugin_summary_from_marketplace_entry(
                &dev_builtin_marketplace_path(),
                &marketplace.name,
                entry,
                &PluginConfigFile::default(),
            )
            .unwrap();
            assert_eq!(
                summary
                    .interface
                    .as_ref()
                    .and_then(|interface| interface.display_name.as_deref()),
                Some(display_name)
            );
            assert_eq!(
                summary
                    .retrieval
                    .as_ref()
                    .map(|retrieval| {
                        retrieval
                            .resources
                            .iter()
                            .map(|source| format!("{}.{}", source.category, source.id))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default(),
                expected_routes
                    .iter()
                    .map(|route| (*route).to_string())
                    .collect::<Vec<_>>()
            );

            let manifest = load_plugin_manifest(&source_path).unwrap();
            let retrieval = manifest.retrieval.expect("retrieval manifest");
            assert!(
                retrieval
                    .resources
                    .iter()
                    .all(|source| source.replaces_builtin
                        && source.capabilities
                            == vec![
                                "search".to_string(),
                                "query".to_string(),
                                "fetch".to_string(),
                            ]),
                "{plugin_name} should replace builtins and support search/query/fetch"
            );
        }
    }

    #[test]
    fn retrieval_resource_config_disables_individual_provider_routes() {
        let source_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("bundled_plugins")
            .join("plugins")
            .join("resource-ncbi");
        let manifest = load_plugin_manifest(&source_path).expect("resource-ncbi manifest");
        let retrieval = manifest.retrieval.expect("retrieval manifest");
        let mut config = PluginConfigFile::default();
        config.plugins.insert(
            "resource-ncbi@omiga-curated".to_string(),
            PluginConfigEntry {
                enabled: true,
                disabled_retrieval_resources: HashSet::from(["dataset.geo".to_string()]),
                ..Default::default()
            },
        );

        let summary =
            plugin_retrieval_summary(Some(&retrieval), "resource-ncbi@omiga-curated", &config)
                .expect("retrieval summary");
        let geo = summary
            .resources
            .iter()
            .find(|source| source.category == "dataset" && source.id == "geo")
            .expect("geo route summary");
        let pubmed = summary
            .resources
            .iter()
            .find(|source| source.category == "literature" && source.id == "pubmed")
            .expect("pubmed route summary");
        assert!(!geo.exposed);
        assert!(pubmed.exposed);

        let entry = config
            .plugins
            .get("resource-ncbi@omiga-curated")
            .expect("plugin config");
        let filtered =
            filter_retrieval_manifest_for_config(retrieval, "resource-ncbi@omiga-curated", entry)
                .expect("filtered retrieval");
        assert!(!filtered
            .resources
            .iter()
            .any(|source| source.category == "dataset" && source.id == "geo"));
        assert!(filtered
            .resources
            .iter()
            .any(|source| source.category == "literature" && source.id == "pubmed"));
    }

    #[test]
    fn bundled_marketplace_exposes_visualization_r_plugin_and_skill() {
        let marketplace_path = dev_builtin_marketplace_path();
        let marketplace = read_marketplace(&marketplace_path).unwrap();
        let entry = marketplace
            .plugins
            .iter()
            .find(|entry| entry.name == "visualization-r")
            .expect("visualization-r marketplace entry");
        assert_eq!(entry.category.as_deref(), Some("Visualization"));
        assert_eq!(entry.policy.installation, PluginInstallPolicy::Available);
        assert_eq!(entry.policy.authentication, PluginAuthPolicy::OnUse);

        let detail = read_plugin(&marketplace_path, "visualization-r").expect("plugin detail");
        assert_eq!(detail.summary.id, "visualization-r@omiga-curated");
        assert_eq!(
            detail
                .summary
                .interface
                .as_ref()
                .and_then(|interface| interface.display_name.as_deref()),
            Some("R Visualization")
        );
        let template_summary = detail
            .summary
            .templates
            .as_ref()
            .expect("visualization-r template summary");
        assert_eq!(
            template_summary.count,
            template_summary
                .groups
                .iter()
                .map(|group| group.count)
                .sum::<usize>()
        );
        for expected_group in ["scatter", "distribution", "bar", "heatmap", "line"] {
            assert!(
                template_summary
                    .groups
                    .iter()
                    .any(|group| group.id == expected_group && group.count > 0),
                "visualization-r should expose non-empty `{expected_group}` templates"
            );
        }
        let scatter_basic = template_summary
            .groups
            .iter()
            .flat_map(|group| group.templates.iter())
            .find(|template| template.id == "viz_scatter_basic")
            .expect("basic scatter template");
        assert_eq!(scatter_basic.execute["tool"], "template_execute");
        assert_eq!(
            scatter_basic.execute["arguments"]["id"],
            "visualization-r@omiga-curated/template/viz_scatter_basic"
        );
        assert!(
            scatter_basic.execute["arguments"]["inputs"]["table"]
                .as_str()
                .is_some_and(|path| path.ends_with("templates/scatter/basic/example.tsv")),
            "{}",
            scatter_basic.execute
        );
        assert_eq!(
            scatter_basic.execute["arguments"]["params"]["x_column"],
            "x_value"
        );
        assert!(
            detail
                .skills
                .iter()
                .any(|skill| skill.name == "visualize-r"),
            "visualization-r should expose $visualize-r"
        );
    }
}
