use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value as JsonValue;
use tokio::time::Instant;

use super::*;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginSkillSummary {
    pub name: String,
    pub description: String,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginRetrievalRegistration {
    pub plugin_id: String,
    pub plugin_root: PathBuf,
    pub retrieval: PluginRetrievalManifest,
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

pub(super) fn materialize_retrieval_resource_config(
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

pub fn environment_profile_enabled(source_plugin: &str, environment_id: &str) -> bool {
    environment_exposed_from_config(&read_config(), source_plugin, environment_id)
}

pub fn enabled_plugin_skill_roots() -> Vec<PathBuf> {
    plugin_load_outcome().effective_skill_roots()
}

pub(super) fn plugin_skill_roots_for_manifest(
    plugin_root: &Path,
    manifest: &PluginManifest,
) -> Vec<PathBuf> {
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

pub(super) fn plugin_skill_summaries(
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

pub(super) fn collect_skill_summaries_from_root(root: &Path, out: &mut Vec<PluginSkillSummary>) {
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

pub(super) fn skill_summary_from_dir(skill_dir: &Path) -> PluginSkillSummary {
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

pub(super) fn parse_skill_frontmatter_name_description(
    raw: &str,
    fallback: &str,
) -> (String, String) {
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

pub(super) fn plugin_mcp_config_path(
    plugin_root: &Path,
    manifest: &PluginManifest,
) -> Option<PathBuf> {
    if let Some(path) = &manifest.mcp_servers {
        return path.is_file().then(|| path.clone());
    }
    let default = plugin_root.join(".mcp.json");
    default.is_file().then_some(default)
}

pub(super) fn plugin_mcp_server_names(
    plugin_root: &Path,
    manifest: &PluginManifest,
) -> Vec<String> {
    let mut names = plugin_mcp_servers(plugin_root, manifest)
        .into_keys()
        .collect::<Vec<_>>();
    names.sort();
    names
}

pub(super) fn plugin_mcp_servers(
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

pub(super) fn rebase_plugin_mcp_server(
    plugin_root: &Path,
    config: McpServerConfig,
) -> McpServerConfig {
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

pub(super) fn resolve_plugin_stdio_cwd(plugin_root: &Path, cwd: Option<&str>) -> String {
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

pub(super) fn plugin_retrieval_statuses_for_registrations(
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

pub(super) fn plugin_app_config_path(
    plugin_root: &Path,
    manifest: &PluginManifest,
) -> Option<PathBuf> {
    if let Some(path) = &manifest.apps {
        return path.is_file().then(|| path.clone());
    }
    let default = plugin_root.join(".app.json");
    default.is_file().then_some(default)
}

pub(super) fn plugin_app_ids(plugin_root: &Path, manifest: &PluginManifest) -> Vec<String> {
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
