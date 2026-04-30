//! Settings UI: list MCP / skills and save enable-disable toggles (`.omiga/integrations.json`).

use crate::app_state::OmigaAppState;
use crate::commands::CommandResult;
use crate::domain::integrations_catalog::{
    IntegrationsCatalog, McpServerCatalogEntry, McpToolCatalogEntry, SkillCatalogEntry,
};
use crate::domain::integrations_config::{self, IntegrationsConfig};
use crate::domain::mcp::client::list_tools_for_server;
use crate::domain::mcp::config::merged_mcp_servers;
use crate::domain::mcp::names::{build_mcp_tool_name, normalize_name_for_mcp};
use crate::domain::skills::{self, SkillSource};
use serde::Serialize;
use std::path::PathBuf;
use std::time::Duration;
use tauri::State;

/// Resolve a project root path to an absolute path.
/// Falls back gracefully: "." or empty → process cwd → "/"
/// When no project folder is set, the catalog still resolves MCP from bundled defaults and
/// `~/.omiga/mcp.json` so we never fail the entire command on path issues.
pub(crate) fn resolve_project_root(project_root: &str) -> CommandResult<PathBuf> {
    let t = project_root.trim();
    if t.is_empty() || t == "." {
        // No project path set — use cwd so global configs are still discoverable.
        return Ok(std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/")));
    }
    let p = PathBuf::from(t);
    // Try canonicalize; fall back to the raw path if it fails (avoids error on
    // partially-resolved or network paths).
    Ok(p.canonicalize().unwrap_or(p))
}

/// Short timeout used only for the settings catalog UI — avoids blocking on dead/auth-required servers.
/// Chat sessions use their own (longer) timeout from `mcp_tool_pool`.
const CATALOG_TOOL_LIST_TIMEOUT: Duration = Duration::from_secs(8);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AvailableSkillInfo {
    pub name: String,
    pub description: String,
    pub source: SkillSource,
    pub tags: Vec<String>,
}

/// Lightweight skills-only catalog for the chat composer `$` picker.
///
/// Unlike `get_integrations_catalog`, this does not query MCP servers, so opening the picker
/// cannot block on external tool discovery.
#[tauri::command]
pub async fn list_available_skills(
    app_state: State<'_, OmigaAppState>,
    project_root: String,
) -> CommandResult<Vec<AvailableSkillInfo>> {
    let root = resolve_project_root(&project_root)?;
    let cfg = integrations_config::load_integrations_config(&root);
    let mut skill_list = skills::load_skills_cached(&root, &app_state.skill_cache).await;
    skill_list = integrations_config::filter_skill_entries(skill_list, &cfg);
    let mut out: Vec<AvailableSkillInfo> = skill_list
        .into_iter()
        .map(|e| AvailableSkillInfo {
            name: e.name,
            description: e.description,
            source: e.source,
            tags: e.tags,
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

/// Build catalog (MCP parallel + skills from disk). Used by the command and startup warm task.
pub(crate) async fn build_integrations_catalog(
    _app_state: &OmigaAppState,
    root: PathBuf,
) -> CommandResult<IntegrationsCatalog> {
    let cfg = integrations_config::load_integrations_config(&root);

    let merged = merged_mcp_servers(&root);
    let mut server_keys: Vec<String> = merged.keys().cloned().collect();
    server_keys.sort();

    // Query all servers in parallel with a short per-server timeout.
    let root_arc = std::sync::Arc::new(root.clone());
    let cfg_arc = std::sync::Arc::new(cfg.clone());
    let handles: Vec<_> = server_keys
        .into_iter()
        .map(|key| {
            let root_c = root_arc.clone();
            let cfg_c = cfg_arc.clone();
            tokio::spawn(async move {
                let normalized_key = normalize_name_for_mcp(&key);
                let enabled = !integrations_config::is_mcp_config_server_disabled(&cfg_c, &key);
                let tools_res =
                    list_tools_for_server(&root_c, &key, CATALOG_TOOL_LIST_TIMEOUT).await;
                let (list_tools_error, tools) = match tools_res {
                    Ok(list) => (
                        None,
                        list.into_iter()
                            .map(|t| {
                                let wire = build_mcp_tool_name(&key, t.name.as_ref());
                                let desc =
                                    t.description.as_deref().unwrap_or("MCP tool").to_string();
                                McpToolCatalogEntry {
                                    wire_name: wire,
                                    description: desc,
                                }
                            })
                            .collect(),
                    ),
                    Err(e) => {
                        tracing::warn!("catalog tools/list failed for \"{key}\": {e}");
                        (Some(e.to_string()), vec![])
                    }
                };
                McpServerCatalogEntry {
                    config_key: key,
                    normalized_key,
                    enabled,
                    list_tools_error,
                    tools,
                }
            })
        })
        .collect();

    let mut mcp_servers: Vec<McpServerCatalogEntry> = Vec::with_capacity(handles.len());
    for h in handles {
        match h.await {
            Ok(entry) => mcp_servers.push(entry),
            Err(e) => tracing::warn!("catalog task panicked: {e}"),
        }
    }
    // Re-sort by config_key after parallel collection.
    mcp_servers.sort_by(|a, b| a.config_key.cmp(&b.config_key));

    let skill_list = skills::load_skills_for_project(&root).await;
    let skills_out: Vec<SkillCatalogEntry> = skill_list
        .into_iter()
        .map(|e| {
            let enabled = !integrations_config::is_skill_name_disabled(&cfg, &e.name);
            let directory_name = e
                .skill_dir
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let can_uninstall_omiga_copy =
                matches!(e.source, SkillSource::OmigaUser | SkillSource::OmigaProject);
            let skill_md_path = e.skill_dir.join("SKILL.md");
            SkillCatalogEntry {
                name: e.name,
                description: e.description,
                enabled,
                source: e.source,
                directory_name,
                skill_md_path: skill_md_path.to_string_lossy().into_owned(),
                tags: e.tags,
                can_uninstall_omiga_copy,
            }
        })
        .collect();

    Ok(IntegrationsCatalog {
        mcp_servers,
        skills: skills_out,
    })
}

/// Discover configured MCP servers and skills, merge with `.omiga/integrations.json` toggles.
/// MCP server tool-listing runs in parallel so a dead server does not block the rest.
///
/// When `ignore_cache` is true, always rebuilds and replaces the cache entry.
#[tauri::command]
pub async fn get_integrations_catalog(
    app_state: State<'_, OmigaAppState>,
    project_root: String,
    ignore_cache: Option<bool>,
) -> CommandResult<IntegrationsCatalog> {
    let root = resolve_project_root(&project_root)?;
    let ignore_cache = ignore_cache.unwrap_or(false);

    if !ignore_cache {
        if let Ok(guard) = app_state.integrations_catalog_cache.lock() {
            if let Some(cached) = guard.get(&root) {
                return Ok(cached.clone());
            }
        }
    }

    let catalog = build_integrations_catalog(&app_state, root.clone()).await?;

    if let Ok(mut guard) = app_state.integrations_catalog_cache.lock() {
        guard.insert(root, catalog.clone());
    }

    Ok(catalog)
}

/// Invalidate cached catalog for this project root (call after integrations file or imports change).
pub(crate) fn invalidate_integrations_catalog_cache(app_state: &OmigaAppState, root: &PathBuf) {
    if let Ok(mut guard) = app_state.integrations_catalog_cache.lock() {
        guard.remove(root);
    }
}

/// Persist disabled MCP servers (normalized keys) and disabled skill names.
#[tauri::command]
pub fn save_integrations_state(
    app_state: State<'_, OmigaAppState>,
    project_root: String,
    disabled_mcp_servers: Vec<String>,
    disabled_skills: Vec<String>,
) -> CommandResult<()> {
    let root = resolve_project_root(&project_root)?;
    let config = IntegrationsConfig {
        disabled_mcp_servers,
        disabled_skills,
    };
    integrations_config::save_integrations_config(&root, &config)
        .map_err(crate::errors::AppError::Config)?;
    invalidate_integrations_catalog_cache(&app_state, &root);
    // MCP enable/disable changes invalidate the tool schema cache and connection pool.
    if let Ok(mut cache) = app_state.chat.mcp_tool_cache.try_lock() {
        cache.remove(&root);
    }
    // Evict all pooled connections for this project so they are re-established with
    // the updated enabled/disabled config on the next tool call.
    // Using the new connection manager for proper lifecycle management.
    let rt = tokio::runtime::Handle::current();
    rt.block_on(async {
        app_state
            .chat
            .mcp_manager
            .close_project_connections(&root)
            .await;
    });
    Ok(())
}

/// Warm the integrations catalog cache in the background (e.g. at app startup for the default cwd).
pub async fn warm_integrations_catalog_cache(app_state: &OmigaAppState, project_root: &str) {
    let Ok(root) = resolve_project_root(project_root) else {
        return;
    };
    if app_state
        .integrations_catalog_cache
        .lock()
        .ok()
        .and_then(|g| g.get(&root).map(|_| ()))
        .is_some()
    {
        return;
    }
    match build_integrations_catalog(app_state, root.clone()).await {
        Ok(catalog) => {
            if let Ok(mut g) = app_state.integrations_catalog_cache.lock() {
                g.insert(root.clone(), catalog);
            }
            tracing::info!("Integrations catalog warmed for {:?}", root);
        }
        Err(e) => tracing::warn!("Integrations catalog warm failed: {}", e),
    }
}
