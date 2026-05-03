//! Omiga native plugin discovery, marketplace, installation, and runtime capability loading.
//!
//! A plugin is an Omiga-native capability bundle: skills, MCP server configs, app connector
//! references, and UI metadata. It intentionally does not execute VS Code extension code.

use crate::domain::mcp::config::{servers_from_mcp_json, McpServerConfig};
use crate::domain::retrieval::plugin::lifecycle::{
    PluginLifecycleKey, PluginLifecycleRouteStatus, PluginLifecycleState,
};
use crate::domain::retrieval::plugin::manifest::{
    load_plugin_retrieval_manifest, PluginRetrievalManifest,
};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use tokio::time::Instant;

pub const OMIGA_PLUGIN_MANIFEST_PATH: &str = ".omiga-plugin/plugin.json";
pub const CODEX_PLUGIN_MANIFEST_PATH: &str = ".codex-plugin/plugin.json";
const MARKETPLACE_FILE_NAME: &str = "marketplace.json";
const USER_PLUGINS_CONFIG_FILE: &str = "plugins/config.json";
const PLUGINS_CACHE_DIR: &str = "plugins/cache";
const DEFAULT_PLUGIN_VERSION: &str = "local";
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
    pub skills: Option<PathBuf>,
    pub mcp_servers: Option<PathBuf>,
    pub apps: Option<PathBuf>,
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
    skills: Option<String>,
    #[serde(default)]
    mcp_servers: Option<String>,
    #[serde(default)]
    apps: Option<String>,
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
    plugins: Vec<RawMarketplacePlugin>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawMarketplacePlugin {
    name: String,
    source: RawMarketplacePluginSource,
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
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginRetrievalSummary {
    pub protocol_version: u32,
    pub sources: Vec<PluginRetrievalSourceSummary>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginRetrievalSourceSummary {
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
        "The user selected the following Omiga plugins with the composer @ picker. Prefer their capabilities for this turn when relevant; if a selected plugin is unavailable, explain that briefly and continue with the best fallback.".to_string(),
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
    let manifest_path = if plugin_root.join(OMIGA_PLUGIN_MANIFEST_PATH).is_file() {
        plugin_root.join(OMIGA_PLUGIN_MANIFEST_PATH)
    } else {
        plugin_root.join(CODEX_PLUGIN_MANIFEST_PATH)
    };
    if !manifest_path.is_file() {
        return None;
    }
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
        skills: resolve_optional_path(plugin_root, parsed.skills.as_deref(), "skills"),
        mcp_servers: resolve_optional_path(
            plugin_root,
            parsed.mcp_servers.as_deref(),
            "mcpServers",
        ),
        apps: resolve_optional_path(plugin_root, parsed.apps.as_deref(), "apps"),
        retrieval,
        interface,
    })
}

fn read_config() -> PluginConfigFile {
    fs::read_to_string(config_path())
        .ok()
        .and_then(|raw| serde_json::from_str::<PluginConfigFile>(&raw).ok())
        .unwrap_or_default()
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
        config
            .plugins
            .insert(key, PluginConfigEntry { enabled: false });
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

fn plugin_base_root(plugin_id: &PluginId) -> PathBuf {
    plugin_base_root_in_cache(&plugin_cache_root(), plugin_id)
}

fn sanitize_version(version: Option<&str>) -> String {
    let version = version.unwrap_or(DEFAULT_PLUGIN_VERSION).trim();
    let clean: String = version
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '.' || *c == '-' || *c == '_')
        .collect();
    if clean.is_empty() || clean.contains("..") {
        DEFAULT_PLUGIN_VERSION.to_string()
    } else {
        clean
    }
}

fn active_plugin_root_in_cache(cache_root: &Path, plugin_id: &PluginId) -> Option<PathBuf> {
    let base = plugin_base_root_in_cache(cache_root, plugin_id);
    let mut versions = fs::read_dir(&base)
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

pub fn active_plugin_root(plugin_id: &PluginId) -> Option<PathBuf> {
    active_plugin_root_in_cache(&plugin_cache_root(), plugin_id)
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
                .sources
                .iter()
                .map(|source| format!("{}.{}", source.category, source.id))
        })
        .collect::<Vec<_>>();
    routes.sort();
    routes
}

fn plugin_retrieval_summary(
    retrieval: Option<&PluginRetrievalManifest>,
) -> Option<PluginRetrievalSummary> {
    let retrieval = retrieval?;
    let mut sources = retrieval
        .sources
        .iter()
        .map(|source| PluginRetrievalSourceSummary {
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
        })
        .collect::<Vec<_>>();
    sources.sort_by(|left, right| {
        left.category
            .cmp(&right.category)
            .then_with(|| left.id.cmp(&right.id))
    });
    Some(PluginRetrievalSummary {
        protocol_version: retrieval.protocol_version,
        sources,
    })
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
    let root = parsed_id
        .as_ref()
        .ok()
        .and_then(|plugin_id| active_plugin_root_in_cache(cache_root, plugin_id))
        .unwrap_or_else(|| match &parsed_id {
            Ok(plugin_id) => plugin_base_root_in_cache(cache_root, plugin_id),
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
    let Some(plugin_root) = active_plugin_root_in_cache(cache_root, &plugin_id) else {
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
    loaded.retrieval = manifest.retrieval.clone();
    loaded
}

fn load_plugins_from_config(config: &PluginConfigFile, cache_root: &Path) -> PluginLoadOutcome {
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
    let manifest = load_plugin_manifest(&source_path);
    let retrieval = manifest
        .as_ref()
        .and_then(|manifest| plugin_retrieval_summary(manifest.retrieval.as_ref()));
    let interface = manifest
        .as_ref()
        .and_then(|manifest| manifest.interface.clone())
        .map(|mut interface| {
            if interface.category.is_none() {
                interface.category = entry.category.clone();
            }
            interface
        });
    let plugin_id = PluginId::new(&entry.name, marketplace_name)?;
    let installed_path = active_plugin_root(&plugin_id);
    let key = plugin_id.key();
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
    })
}

fn plugin_summary_from_installed_root(
    plugin_id: &PluginId,
    plugin_root: &Path,
    config: &PluginConfigFile,
    cache_root: &Path,
) -> PluginSummary {
    let manifest = load_plugin_manifest(plugin_root);
    let retrieval = manifest
        .as_ref()
        .and_then(|manifest| plugin_retrieval_summary(manifest.retrieval.as_ref()));
    let key = plugin_id.key();
    PluginSummary {
        id: key.clone(),
        name: plugin_id.name.clone(),
        marketplace_name: plugin_id.marketplace.clone(),
        marketplace_path: cache_root
            .join(&plugin_id.marketplace)
            .to_string_lossy()
            .into_owned(),
        source_path: plugin_root.to_string_lossy().into_owned(),
        installed_path: Some(plugin_root.to_string_lossy().into_owned()),
        installed: true,
        enabled: is_plugin_enabled(config, &key),
        install_policy: PluginInstallPolicy::Available,
        auth_policy: PluginAuthPolicy::OnUse,
        interface: manifest.and_then(|manifest| manifest.interface),
        retrieval,
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

fn unlisted_installed_plugin_summaries(
    config: &PluginConfigFile,
    listed_ids: &HashSet<String>,
    cache_root: &Path,
) -> Vec<PluginSummary> {
    cached_plugin_ids_in_cache(cache_root)
        .into_iter()
        .filter_map(|plugin_id| {
            let key = plugin_id.key();
            if listed_ids.contains(&key) {
                return None;
            }
            let plugin_root = active_plugin_root_in_cache(cache_root, &plugin_id)?;
            Some(plugin_summary_from_installed_root(
                &plugin_id,
                &plugin_root,
                config,
                cache_root,
            ))
        })
        .collect()
}

pub fn list_plugin_marketplaces(
    project_root: Option<&Path>,
    resource_dir: Option<&Path>,
) -> Vec<PluginMarketplaceEntry> {
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
        if plugins.is_empty() {
            continue;
        }
        out.push(PluginMarketplaceEntry {
            name: marketplace.name,
            path: path.to_string_lossy().into_owned(),
            interface: marketplace.interface,
            plugins,
        });
    }
    let cache_root = plugin_cache_root();
    let installed_plugins = unlisted_installed_plugin_summaries(&config, &listed_ids, &cache_root);
    if !installed_plugins.is_empty() {
        out.push(PluginMarketplaceEntry {
            name: "omiga-installed-cache".to_string(),
            path: cache_root.to_string_lossy().into_owned(),
            interface: Some(MarketplaceInterface {
                display_name: Some("Installed plugins".to_string()),
            }),
            plugins: installed_plugins,
        });
    }
    out
}

pub fn read_plugin(marketplace_path: &Path, plugin_name: &str) -> Result<PluginDetail, String> {
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
            registration.retrieval.sources.iter().map(|source| {
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

fn replace_plugin_root_atomically(
    source: &Path,
    target_base: &Path,
    version: &str,
) -> Result<PathBuf, String> {
    let parent = target_base
        .parent()
        .ok_or_else(|| format!("plugin cache path has no parent: {}", target_base.display()))?;
    fs::create_dir_all(parent).map_err(|err| format!("create plugin cache dir: {err}"))?;
    let staged_base = parent.join(format!(".install-{}", uuid::Uuid::new_v4()));
    let staged_version = staged_base.join(version);
    copy_dir_recursive(source, &staged_version)?;

    if target_base.exists() {
        remove_path_if_exists(target_base)?;
    }
    fs::rename(&staged_base, target_base)
        .map_err(|err| format!("activate plugin cache entry: {err}"))?;
    Ok(target_base.join(version))
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
    let version = sanitize_version(manifest.version.as_deref());
    let installed_path =
        replace_plugin_root_atomically(&source_path, &plugin_base_root(&plugin_id), &version)?;
    set_plugin_enabled(&plugin_id.key(), true)?;
    Ok(PluginInstallResult {
        plugin_id: plugin_id.key(),
        installed_path: installed_path.to_string_lossy().into_owned(),
        auth_policy: entry.policy.authentication.clone(),
    })
}

pub fn uninstall_plugin(plugin_id: &str) -> Result<(), String> {
    let plugin_id = PluginId::parse(plugin_id)?;
    remove_path_if_exists(&plugin_base_root(&plugin_id))?;
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

    #[test]
    fn resolves_manifest_paths_safely() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let plugin = tmp.path().join("sample");
        fs::create_dir_all(plugin.join(".omiga-plugin")).unwrap();
        fs::write(
            plugin.join(".omiga-plugin/plugin.json"),
            r#"{"name":"sample","skills":"./skills","mcpServers":"../bad"}"#,
        )
        .unwrap();
        let manifest = load_plugin_manifest(&plugin).expect("manifest");
        assert_eq!(manifest.name, "sample");
        assert_eq!(manifest.skills, Some(plugin.join("skills")));
        assert_eq!(manifest.mcp_servers, None);
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
            PluginConfigEntry { enabled: true },
        );
        config.plugins.insert(
            "alpha@market".to_string(),
            PluginConfigEntry { enabled: true },
        );
        config.plugins.insert(
            "disabled@market".to_string(),
            PluginConfigEntry { enabled: false },
        );

        let ids = enabled_plugin_ids(&config)
            .into_iter()
            .map(|id| id.key())
            .collect::<Vec<_>>();
        assert_eq!(ids, vec!["alpha@market", "zeta@market"]);
    }

    #[test]
    fn plugin_load_outcome_collects_effective_capabilities() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let cache_root = tmp.path().join("cache");
        let plugin_root = write_cached_plugin(
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
            PluginConfigEntry { enabled: true },
        );

        let outcome = load_plugins_from_config(&config, &cache_root);

        assert_eq!(outcome.plugins().len(), 1);
        assert!(outcome.plugins()[0].is_active());
        assert_eq!(
            outcome.effective_skill_roots(),
            vec![plugin_root.join("skills")]
        );
        match outcome.effective_mcp_servers().get("sample") {
            Some(McpServerConfig::Url(url)) => assert_eq!(url, "https://sample.example/mcp"),
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
    fn plugins_system_section_renders_available_capabilities() {
        let mut mcp_servers = HashMap::new();
        mcp_servers.insert(
            "sample".to_string(),
            McpServerConfig::Url("https://sample.example/mcp".to_string()),
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

        assert!(section.contains("BioSample Retrieval Source"));
        assert!(section.contains("retrieval routes"));
        assert!(section.contains("`dataset.biosample`"));
        assert!(!section.contains("`dataset.arrayexpress`"));
    }

    #[test]
    fn selected_plugins_system_section_prioritizes_explicit_plugin_mentions() {
        let mut mcp_servers = HashMap::new();
        mcp_servers.insert(
            "sample".to_string(),
            McpServerConfig::Url("https://sample.example/mcp".to_string()),
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
            PluginConfigEntry { enabled: true },
        );
        config.plugins.insert(
            "alpha@market".to_string(),
            PluginConfigEntry { enabled: true },
        );

        let servers = load_plugins_from_config(&config, &cache_root).effective_mcp_servers();

        match servers.get("sample") {
            Some(McpServerConfig::Url(url)) => {
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
            PluginConfigEntry { enabled: false },
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
    fn unlisted_installed_plugins_are_summarized_from_cache() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let cache_root = tmp.path().join("cache");
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
            PluginConfigEntry { enabled: true },
        );

        let summaries = unlisted_installed_plugin_summaries(&config, &HashSet::new(), &cache_root);

        assert_eq!(summaries.len(), 1);
        let summary = &summaries[0];
        assert_eq!(summary.id, "orphan@removed-market");
        assert!(summary.installed);
        assert!(summary.enabled);
        assert_eq!(
            summary
                .interface
                .as_ref()
                .and_then(|i| i.display_name.as_deref()),
            Some("Orphan Plugin")
        );
    }

    #[test]
    fn listed_installed_plugins_are_not_duplicated_from_cache() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let cache_root = tmp.path().join("cache");
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
    }

    #[test]
    fn bundled_marketplace_exposes_individual_retrieval_source_plugins() {
        let marketplace = read_marketplace(&dev_builtin_marketplace_path()).unwrap();
        for removed in [
            "public-dataset-sources",
            "public-literature-sources",
            "public-knowledge-sources",
        ] {
            assert!(
                !marketplace
                    .plugins
                    .iter()
                    .any(|entry| entry.name == removed),
                "grouped retrieval plugin `{removed}` should not be marketplace-visible"
            );
        }

        let cases = [
            (
                "retrieval-dataset-geo",
                "GEO Retrieval Source",
                vec!["dataset.geo"],
            ),
            (
                "retrieval-dataset-ena",
                "ENA Retrieval Source",
                vec![
                    "dataset.ena",
                    "dataset.ena_analysis",
                    "dataset.ena_assembly",
                    "dataset.ena_experiment",
                    "dataset.ena_run",
                    "dataset.ena_sample",
                    "dataset.ena_sequence",
                ],
            ),
            (
                "retrieval-dataset-biosample",
                "BioSample Retrieval Source",
                vec!["dataset.biosample"],
            ),
            (
                "retrieval-dataset-arrayexpress",
                "ArrayExpress Retrieval Source",
                vec!["dataset.arrayexpress"],
            ),
            (
                "retrieval-dataset-ncbi-datasets",
                "NCBI Datasets Retrieval Source",
                vec!["dataset.ncbi_datasets"],
            ),
            (
                "retrieval-dataset-gtex",
                "GTEx Retrieval Source",
                vec!["dataset.gtex"],
            ),
            (
                "retrieval-dataset-cbioportal",
                "cBioPortal Retrieval Source",
                vec!["dataset.cbioportal"],
            ),
            (
                "retrieval-literature-pubmed",
                "PubMed Retrieval Source",
                vec!["literature.pubmed"],
            ),
            (
                "retrieval-literature-semantic-scholar",
                "Semantic Scholar Retrieval Source",
                vec!["literature.semantic_scholar"],
            ),
            (
                "retrieval-knowledge-ncbi-gene",
                "NCBI Gene Retrieval Source",
                vec!["knowledge.ncbi_gene"],
            ),
            (
                "retrieval-knowledge-ensembl",
                "Ensembl Retrieval Source",
                vec!["knowledge.ensembl"],
            ),
            (
                "retrieval-knowledge-uniprot",
                "UniProt Retrieval Source",
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
                            .sources
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
                    .sources
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
}
