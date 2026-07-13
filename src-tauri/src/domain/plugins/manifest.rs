use crate::domain::plugin_runtime::retrieval::manifest::{
    load_plugin_retrieval_manifest, PluginRetrievalManifest,
};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::fs;
use std::path::{Component, Path, PathBuf};

pub const PLUGIN_MANIFEST_FILE: &str = "plugin.json";
pub const OMIGA_PLUGIN_MANIFEST_PATH: &str = ".omiga-plugin/plugin.json";
pub const CODEX_PLUGIN_MANIFEST_PATH: &str = ".codex-plugin/plugin.json";
const MAX_CHANGELOG_BYTES: usize = 128 * 1024;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PluginKind {
    Operator,
    Resource,
    Workflow,
    Tool,
    Other,
}

impl PluginKind {
    pub(crate) const ALL: [Self; 5] = [
        Self::Operator,
        Self::Resource,
        Self::Workflow,
        Self::Tool,
        Self::Other,
    ];

    pub(crate) fn dir_name(self) -> &'static str {
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
pub(crate) fn resolve_safe_relative_path(
    root: &Path,
    value: &str,
    field: &str,
) -> Result<PathBuf, String> {
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

pub(crate) fn plugin_changelog_summary(
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
