//! Settings UI: list MCP / skills and save enable-disable toggles (`.omiga/integrations.json`).

use crate::app_state::OmigaAppState;
use crate::commands::CommandResult;
use crate::domain::integrations_config::{
    self, IntegrationsConfig,
};
use crate::domain::mcp_client::list_tools_for_server;
use crate::domain::mcp_config::merged_mcp_servers;
use crate::domain::mcp_names::{build_mcp_tool_name, normalize_name_for_mcp};
use crate::domain::skills::{self, SkillSource};
use serde::Serialize;
use std::path::PathBuf;
use std::time::Duration;
use tauri::State;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpToolCatalogEntry {
    pub wire_name: String,
    pub description: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerCatalogEntry {
    pub config_key: String,
    pub normalized_key: String,
    pub enabled: bool,
    pub tools: Vec<McpToolCatalogEntry>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillCatalogEntry {
    pub name: String,
    pub description: String,
    pub enabled: bool,
    /// Where the skill was loaded from (for UI labeling).
    pub source: crate::domain::skills::SkillSource,
    /// Folder basename under the skills root (matches `remove_omiga_imported_skill`).
    pub directory_name: String,
    /// Skill lives under `~/.omiga/skills` or `<project>/.omiga/skills` — safe to delete that folder.
    pub can_uninstall_omiga_copy: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IntegrationsCatalog {
    pub mcp_servers: Vec<McpServerCatalogEntry>,
    pub skills: Vec<SkillCatalogEntry>,
}

/// Resolve a project root path to an absolute path.
/// Falls back gracefully: "." or empty → process cwd → "/"
/// When no project folder is set, the catalog still resolves MCP from bundled defaults and
/// `~/.omiga/mcp.json` so we never fail the entire command on path issues.
fn resolve_project_root(project_root: &str) -> CommandResult<PathBuf> {
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

/// Discover configured MCP servers and skills, merge with `.omiga/integrations.json` toggles.
/// MCP server tool-listing runs in parallel so a dead server does not block the rest.
#[tauri::command]
pub async fn get_integrations_catalog(
    app_state: State<'_, OmigaAppState>,
    project_root: String,
) -> CommandResult<IntegrationsCatalog> {
    let root = resolve_project_root(&project_root)?;
    let include_claude_user_skills = {
        let repo = app_state.repo.lock().await;
        match repo
            .get_setting(skills::SETTING_KEY_LOAD_CLAUDE_USER_SKILLS)
            .await
        {
            Ok(v) => skills::parse_load_claude_user_skills_setting(v.as_deref()),
            Err(_) => false,
        }
    };
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
                let tools = match tools_res {
                    Ok(list) => list
                        .into_iter()
                        .map(|t| {
                            let wire = build_mcp_tool_name(&key, t.name.as_ref());
                            let desc = t
                                .description
                                .as_deref()
                                .unwrap_or("MCP tool")
                                .to_string();
                            McpToolCatalogEntry {
                                wire_name: wire,
                                description: desc,
                            }
                        })
                        .collect(),
                    Err(e) => {
                        tracing::warn!("catalog tools/list failed for \"{key}\": {e}");
                        vec![]
                    }
                };
                McpServerCatalogEntry {
                    config_key: key,
                    normalized_key,
                    enabled,
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

    let skill_list = skills::load_skills_for_project(&root, include_claude_user_skills).await;
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
            SkillCatalogEntry {
                name: e.name,
                description: e.description,
                enabled,
                source: e.source,
                directory_name,
                can_uninstall_omiga_copy,
            }
        })
        .collect();

    Ok(IntegrationsCatalog {
        mcp_servers,
        skills: skills_out,
    })
}

/// Persist disabled MCP servers (normalized keys) and disabled skill names.
#[tauri::command]
pub fn save_integrations_state(
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
        .map_err(|e| crate::errors::AppError::Config(e))
}
