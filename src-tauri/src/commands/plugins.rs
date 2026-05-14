//! Tauri commands for Omiga-native plugins.

use crate::app_state::OmigaAppState;
use crate::commands::CommandResult;
use crate::domain::plugin_runtime::retrieval::lifecycle::PluginLifecycleRouteStatus;
use crate::domain::plugin_runtime::retrieval::validation::{
    validate_retrieval_plugin_root, PluginRetrievalValidationReport,
};
use crate::domain::plugins::{
    self, PluginDetail, PluginEnvironmentCheckResult, PluginInstallResult, PluginMarketplaceEntry,
};
use crate::domain::retrieval::providers::plugin_provider::{
    clear_global_plugin_process_pool, global_plugin_process_pool_statuses,
    PluginProcessPoolRouteStatus,
};
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
    clear_global_plugin_process_pool().await;

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
pub async fn sync_omiga_plugin(
    app_state: State<'_, OmigaAppState>,
    plugin_id: String,
    marketplace_path: String,
    plugin_name: Option<String>,
    force: Option<bool>,
    project_root: Option<String>,
) -> CommandResult<plugins::PluginSyncResult> {
    let _root = resolve_optional_project_root(project_root);
    let result = plugins::sync_plugin(
        &plugin_id,
        Path::new(&marketplace_path),
        plugin_name.as_deref(),
        force.unwrap_or(false),
    )
    .map_err(plugin_error)?;
    invalidate_plugin_dependent_caches(&app_state).await;
    Ok(result)
}

#[tauri::command]
pub async fn check_omiga_remote_plugin_marketplaces(
    app: tauri::AppHandle,
    project_root: Option<String>,
) -> CommandResult<Vec<plugins::MarketplaceRemoteCheckResult>> {
    let root = resolve_optional_project_root(project_root);
    let resource_dir = app.path().resource_dir().ok();
    Ok(plugins::check_remote_plugin_marketplaces(root.as_deref(), resource_dir.as_deref()).await)
}

#[tauri::command]
pub async fn uninstall_omiga_plugin(
    app_state: State<'_, OmigaAppState>,
    plugin_id: String,
    project_root: Option<String>,
) -> CommandResult<()> {
    let _root = resolve_optional_project_root(project_root);
    clear_global_plugin_process_pool().await;
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
    clear_global_plugin_process_pool().await;
    plugins::set_plugin_enabled(&plugin_id, enabled).map_err(plugin_error)?;
    invalidate_plugin_dependent_caches(&app_state).await;
    Ok(())
}

#[tauri::command]
pub async fn set_omiga_template_enabled(
    app_state: State<'_, OmigaAppState>,
    plugin_id: String,
    template_id: String,
    enabled: bool,
    project_root: Option<String>,
) -> CommandResult<()> {
    let _root = resolve_optional_project_root(project_root);
    plugins::set_template_enabled(&plugin_id, &template_id, enabled).map_err(plugin_error)?;
    invalidate_plugin_dependent_caches(&app_state).await;
    Ok(())
}

#[tauri::command]
pub async fn set_omiga_retrieval_resource_enabled(
    app_state: State<'_, OmigaAppState>,
    plugin_id: String,
    category: String,
    resource_id: String,
    enabled: bool,
    project_root: Option<String>,
) -> CommandResult<()> {
    let _root = resolve_optional_project_root(project_root);
    clear_global_plugin_process_pool().await;
    plugins::set_retrieval_resource_enabled(&plugin_id, &category, &resource_id, enabled)
        .map_err(plugin_error)?;
    invalidate_plugin_dependent_caches(&app_state).await;
    Ok(())
}

#[tauri::command]
pub async fn set_omiga_environment_enabled(
    app_state: State<'_, OmigaAppState>,
    plugin_id: String,
    environment_id: String,
    enabled: bool,
    project_root: Option<String>,
) -> CommandResult<()> {
    let _root = resolve_optional_project_root(project_root);
    plugins::set_environment_enabled(&plugin_id, &environment_id, enabled).map_err(plugin_error)?;
    invalidate_plugin_dependent_caches(&app_state).await;
    Ok(())
}

#[tauri::command]
pub async fn check_omiga_plugin_environment(
    plugin_id: String,
    marketplace_path: Option<String>,
    plugin_name: Option<String>,
    env_ref: String,
    project_root: Option<String>,
) -> CommandResult<PluginEnvironmentCheckResult> {
    let root = resolve_optional_project_root(project_root);
    let marketplace_path = marketplace_path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    plugins::check_plugin_environment(
        &plugin_id,
        marketplace_path.as_deref(),
        plugin_name.as_deref(),
        &env_ref,
        root.as_deref(),
    )
    .map_err(plugin_error)
}

#[tauri::command]
pub async fn list_omiga_plugin_retrieval_statuses(
    project_root: Option<String>,
) -> CommandResult<Vec<PluginLifecycleRouteStatus>> {
    let _root = resolve_optional_project_root(project_root);
    Ok(plugins::enabled_plugin_retrieval_statuses())
}

#[tauri::command]
pub async fn list_omiga_plugin_process_pool_statuses(
    project_root: Option<String>,
) -> CommandResult<Vec<PluginProcessPoolRouteStatus>> {
    let _root = resolve_optional_project_root(project_root);
    Ok(global_plugin_process_pool_statuses().await)
}

#[tauri::command]
pub async fn clear_omiga_plugin_process_pool(project_root: Option<String>) -> CommandResult<usize> {
    let _root = resolve_optional_project_root(project_root);
    Ok(clear_global_plugin_process_pool().await)
}

#[tauri::command]
pub async fn validate_omiga_retrieval_plugin(
    plugin_root: String,
    smoke: Option<bool>,
    project_root: Option<String>,
) -> CommandResult<PluginRetrievalValidationReport> {
    let _root = resolve_optional_project_root(project_root);
    Ok(validate_retrieval_plugin_root(Path::new(plugin_root.trim()), smoke.unwrap_or(false)).await)
}
