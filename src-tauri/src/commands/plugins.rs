//! Tauri commands for Omiga-native plugins.

use crate::app_state::OmigaAppState;
use crate::commands::CommandResult;
use crate::domain::plugins::{self, PluginDetail, PluginInstallResult, PluginMarketplaceEntry};
use crate::errors::AppError;
use std::path::{Path, PathBuf};
use tauri::{Manager, State};

fn resolve_optional_project_root(project_root: Option<String>) -> Option<PathBuf> {
    let raw = project_root.unwrap_or_default();
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed == "." {
        return None;
    }
    let path = PathBuf::from(trimmed);
    Some(path.canonicalize().unwrap_or(path))
}

async fn invalidate_plugin_dependent_caches(app_state: &OmigaAppState) {
    // Plugin install/enabled state is global (`~/.omiga/plugins`), so every
    // project-scoped view/cache can be affected by any plugin mutation.
    if let Ok(mut guard) = app_state.integrations_catalog_cache.lock() {
        guard.clear();
    }
    if let Ok(mut guard) = app_state.skill_cache.lock() {
        guard.clear();
    }
    app_state.chat.mcp_tool_cache.lock().await.clear();

    let project_roots: Vec<PathBuf> = app_state
        .chat
        .mcp_manager
        .all_stats()
        .await
        .into_iter()
        .map(|(root, _)| root)
        .collect();
    for root in project_roots {
        app_state
            .chat
            .mcp_manager
            .close_project_connections(&root)
            .await;
    }
}

fn plugin_error(error: String) -> AppError {
    AppError::Config(error)
}

#[tauri::command]
pub async fn list_omiga_plugin_marketplaces(
    app: tauri::AppHandle,
    project_root: Option<String>,
) -> CommandResult<Vec<PluginMarketplaceEntry>> {
    let root = resolve_optional_project_root(project_root);
    let resource_dir = app.path().resource_dir().ok();
    Ok(plugins::list_plugin_marketplaces(
        root.as_deref(),
        resource_dir.as_deref(),
    ))
}

#[tauri::command]
pub async fn read_omiga_plugin(
    marketplace_path: String,
    plugin_name: String,
) -> CommandResult<PluginDetail> {
    plugins::read_plugin(Path::new(&marketplace_path), &plugin_name).map_err(plugin_error)
}

#[tauri::command]
pub async fn install_omiga_plugin(
    app_state: State<'_, OmigaAppState>,
    marketplace_path: String,
    plugin_name: String,
    project_root: Option<String>,
) -> CommandResult<PluginInstallResult> {
    let _root = resolve_optional_project_root(project_root);
    let result = plugins::install_plugin(Path::new(&marketplace_path), &plugin_name)
        .map_err(plugin_error)?;
    invalidate_plugin_dependent_caches(&app_state).await;
    Ok(result)
}

#[tauri::command]
pub async fn uninstall_omiga_plugin(
    app_state: State<'_, OmigaAppState>,
    plugin_id: String,
    project_root: Option<String>,
) -> CommandResult<()> {
    let _root = resolve_optional_project_root(project_root);
    plugins::uninstall_plugin(&plugin_id).map_err(plugin_error)?;
    invalidate_plugin_dependent_caches(&app_state).await;
    Ok(())
}

#[tauri::command]
pub async fn set_omiga_plugin_enabled(
    app_state: State<'_, OmigaAppState>,
    plugin_id: String,
    enabled: bool,
    project_root: Option<String>,
) -> CommandResult<()> {
    let _root = resolve_optional_project_root(project_root);
    plugins::set_plugin_enabled(&plugin_id, enabled).map_err(plugin_error)?;
    invalidate_plugin_dependent_caches(&app_state).await;
    Ok(())
}
