//! Settings UI: list MCP / skills and save enable-disable toggles (`.omiga/integrations.json`).

use crate::app_state::OmigaAppState;
use crate::commands::CommandResult;
use crate::domain::integrations_catalog::{
    IntegrationsCatalog, McpServerCatalogEntry, McpServerConfigCatalogEntry, McpToolCatalogEntry,
    SkillCatalogEntry,
};
use crate::domain::integrations_config::{self, IntegrationsConfig};
use crate::domain::mcp::client::list_tools_for_server;
use crate::domain::mcp::config::{merged_mcp_servers, McpServerConfig};
use crate::domain::mcp::names::{build_mcp_tool_name, normalize_name_for_mcp};
use crate::domain::mcp::oauth::{self, McpOAuthLoginPollStatus};
use crate::domain::skills::{self, SkillSource};
use serde::Serialize;
use std::path::{Path, PathBuf};
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

fn catalog_config_from_mcp_config(config: &McpServerConfig) -> McpServerConfigCatalogEntry {
    match config {
        McpServerConfig::Stdio {
            command,
            args,
            env,
            cwd,
        } => McpServerConfigCatalogEntry {
            kind: "stdio".to_string(),
            command: Some(command.clone()),
            args: args.clone(),
            env: env.clone(),
            headers: Default::default(),
            url: None,
            cwd: cwd.clone(),
        },
        McpServerConfig::Url { url, headers } => McpServerConfigCatalogEntry {
            kind: "http".to_string(),
            command: None,
            args: Vec::new(),
            env: Default::default(),
            headers: headers.clone(),
            url: Some(url.clone()),
            cwd: None,
        },
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AvailableSkillInfo {
    pub name: String,
    pub description: String,
    pub source: SkillSource,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerVerificationResult {
    pub config_key: String,
    pub normalized_key: String,
    pub ok: bool,
    pub tool_list_checked: bool,
    pub oauth_authenticated: bool,
    pub list_tools_error: Option<String>,
    pub tools: Vec<McpToolCatalogEntry>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerOAuthLoginStartResult {
    pub config_key: String,
    pub normalized_key: String,
    pub login_session_id: String,
    pub authorization_url: String,
    pub expires_in: u64,
    pub interval_secs: u64,
    pub expires_at: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerOAuthLoginPollResult {
    pub config_key: String,
    pub normalized_key: String,
    pub status: McpOAuthLoginPollStatus,
    pub message: String,
    pub interval_secs: u64,
    pub ok: bool,
    pub tool_list_checked: bool,
    pub oauth_authenticated: bool,
    pub list_tools_error: Option<String>,
    pub tools: Vec<McpToolCatalogEntry>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerOAuthLogoutResult {
    pub config_key: String,
    pub normalized_key: String,
}

fn mcp_tool_entries_for_server(
    server_key: &str,
    tools: Vec<rmcp::model::Tool>,
) -> Vec<McpToolCatalogEntry> {
    tools
        .into_iter()
        .map(|t| {
            let wire = build_mcp_tool_name(server_key, t.name.as_ref());
            let desc = t.description.as_deref().unwrap_or("MCP tool").to_string();
            McpToolCatalogEntry {
                wire_name: wire,
                description: desc,
            }
        })
        .collect()
}

fn config_has_explicit_authorization(config: &McpServerConfig) -> bool {
    matches!(
        config,
        McpServerConfig::Url { headers, .. }
            if headers
                .keys()
                .any(|key| key.eq_ignore_ascii_case("authorization"))
    )
}

fn oauth_authenticated_for_config(config: &McpServerConfig) -> bool {
    match config {
        McpServerConfig::Url { url, .. } if !config_has_explicit_authorization(config) => {
            oauth::has_stored_credentials_for_url(url)
        }
        _ => false,
    }
}

fn oauth_authenticated_for_server(root: &Path, server_key: &str) -> bool {
    merged_mcp_servers(root)
        .get(server_key)
        .map(oauth_authenticated_for_config)
        .unwrap_or(false)
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
    probe_tools: bool,
) -> CommandResult<IntegrationsCatalog> {
    let cfg = integrations_config::load_integrations_config(&root);

    let merged = merged_mcp_servers(&root);
    let mut server_keys: Vec<String> = merged.keys().cloned().collect();
    server_keys.sort();
    let merged_arc = std::sync::Arc::new(merged);

    // Query all servers in parallel with a short per-server timeout.
    let root_arc = std::sync::Arc::new(root.clone());
    let cfg_arc = std::sync::Arc::new(cfg.clone());
    let handles: Vec<_> = server_keys
        .into_iter()
        .map(|key| {
            let root_c = root_arc.clone();
            let cfg_c = cfg_arc.clone();
            let merged_c = merged_arc.clone();
            tokio::spawn(async move {
                let normalized_key = normalize_name_for_mcp(&key);
                let enabled = !integrations_config::is_mcp_config_server_disabled(&cfg_c, &key);
                let config = merged_c
                    .get(&key)
                    .map(catalog_config_from_mcp_config)
                    .unwrap_or_else(|| McpServerConfigCatalogEntry {
                        kind: "stdio".to_string(),
                        command: None,
                        args: Vec::new(),
                        env: Default::default(),
                        headers: Default::default(),
                        url: None,
                        cwd: None,
                    });
                let oauth_authenticated = merged_c
                    .get(&key)
                    .map(oauth_authenticated_for_config)
                    .unwrap_or(false);
                let (tool_list_checked, list_tools_error, tools) = if probe_tools && enabled {
                    let tools_res =
                        list_tools_for_server(&root_c, &key, CATALOG_TOOL_LIST_TIMEOUT).await;
                    match tools_res {
                        Ok(list) => (true, None, mcp_tool_entries_for_server(&key, list)),
                        Err(e) => {
                            tracing::warn!("catalog tools/list failed for \"{key}\": {e}");
                            (true, Some(e.to_string()), vec![])
                        }
                    }
                } else {
                    (false, None, Vec::new())
                };
                McpServerCatalogEntry {
                    config_key: key,
                    normalized_key,
                    enabled,
                    config,
                    tool_list_checked,
                    oauth_authenticated,
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
    // Re-sort by config_key after parallel collection. Keep the bundled Paperclip
    // literature MCP first so users see the default research source before
    // project/local additions.
    mcp_servers.sort_by(|a, b| {
        let a_rank = if a.config_key.eq_ignore_ascii_case("paperclip") {
            0
        } else {
            1
        };
        let b_rank = if b.config_key.eq_ignore_ascii_case("paperclip") {
            0
        } else {
            1
        };
        a_rank.cmp(&b_rank).then(a.config_key.cmp(&b.config_key))
    });

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
/// By default this does not probe remote `tools/list`; Settings can request an explicit probe when
/// the user clicks refresh/verify. This keeps opening the MCP tab instant and avoids surfacing stale
/// timeouts every time the page remounts.
///
/// When `ignore_cache` is true, always rebuilds and replaces the cache entry.
#[tauri::command]
pub async fn get_integrations_catalog(
    app_state: State<'_, OmigaAppState>,
    project_root: String,
    ignore_cache: Option<bool>,
    probe_tools: Option<bool>,
) -> CommandResult<IntegrationsCatalog> {
    let root = resolve_project_root(&project_root)?;
    let ignore_cache = ignore_cache.unwrap_or(false);
    let probe_tools = probe_tools.unwrap_or(false);

    if !ignore_cache {
        if let Ok(guard) = app_state.integrations_catalog_cache.lock() {
            if let Some(cached) = guard.get(&root) {
                return Ok(cached.clone());
            }
        }
    }

    let catalog = build_integrations_catalog(&app_state, root.clone(), probe_tools).await?;

    if let Ok(mut guard) = app_state.integrations_catalog_cache.lock() {
        guard.insert(root, catalog.clone());
    }

    Ok(catalog)
}

/// Verify one MCP server by running `tools/list` with the effective merged config.
///
/// This command returns connection/auth failures as data instead of a command error so the Settings
/// UI can update just the affected row and keep showing targeted remediation guidance.
#[tauri::command]
pub async fn verify_mcp_server(
    project_root: String,
    server_name: String,
) -> CommandResult<McpServerVerificationResult> {
    let root = resolve_project_root(&project_root)?;
    let config_key = server_name.trim().to_string();
    let normalized_key = normalize_name_for_mcp(&config_key);
    let oauth_authenticated = oauth_authenticated_for_server(&root, &config_key);
    match list_tools_for_server(&root, &config_key, CATALOG_TOOL_LIST_TIMEOUT).await {
        Ok(list) => Ok(McpServerVerificationResult {
            config_key: config_key.clone(),
            normalized_key,
            ok: true,
            tool_list_checked: true,
            oauth_authenticated,
            list_tools_error: None,
            tools: mcp_tool_entries_for_server(&config_key, list),
        }),
        Err(e) => Ok(McpServerVerificationResult {
            config_key,
            normalized_key,
            ok: false,
            tool_list_checked: true,
            oauth_authenticated,
            list_tools_error: Some(e.to_string()),
            tools: Vec::new(),
        }),
    }
}

/// Start the MCP OAuth browser flow for one remote HTTP MCP server.
#[tauri::command]
pub async fn start_mcp_oauth_login(
    project_root: String,
    server_name: String,
) -> CommandResult<McpServerOAuthLoginStartResult> {
    let root = resolve_project_root(&project_root)?;
    let config_key = server_name.trim().to_string();
    let normalized_key = normalize_name_for_mcp(&config_key);
    let started = crate::domain::mcp::oauth::start_mcp_oauth_login(&root, &config_key)
        .await
        .map_err(crate::errors::AppError::Config)?;
    Ok(McpServerOAuthLoginStartResult {
        config_key,
        normalized_key,
        login_session_id: started.login_session_id,
        authorization_url: started.authorization_url,
        expires_in: started.expires_in,
        interval_secs: started.interval_secs,
        expires_at: started.expires_at,
        message: started.message,
    })
}

/// Poll an active MCP OAuth browser flow.
///
/// Once OAuth completes, this command immediately reruns `tools/list` so the UI can transition from
/// “exchanging token” to “N tools enabled” without requiring a second manual click.
#[tauri::command]
pub async fn poll_mcp_oauth_login(
    app_state: State<'_, OmigaAppState>,
    project_root: String,
    login_session_id: String,
) -> CommandResult<McpServerOAuthLoginPollResult> {
    let root = resolve_project_root(&project_root)?;
    let poll = crate::domain::mcp::oauth::poll_mcp_oauth_login(&login_session_id)
        .await
        .map_err(crate::errors::AppError::Config)?;
    let config_key = poll.server_name.clone();
    let normalized_key = normalize_name_for_mcp(&config_key);

    let (ok, tool_list_checked, oauth_authenticated, list_tools_error, tools) =
        if matches!(poll.status, McpOAuthLoginPollStatus::Complete) {
            invalidate_integrations_catalog_cache(&app_state, &root);
            if let Ok(mut cache) = app_state.chat.mcp_tool_cache.try_lock() {
                cache.remove(&root);
            }
            app_state
                .chat
                .mcp_manager
                .close_project_connections(&root)
                .await;
            let oauth_authenticated = oauth_authenticated_for_server(&root, &config_key);
            match list_tools_for_server(&root, &config_key, CATALOG_TOOL_LIST_TIMEOUT).await {
                Ok(list) => (
                    true,
                    true,
                    oauth_authenticated,
                    None,
                    mcp_tool_entries_for_server(&config_key, list),
                ),
                Err(err) => (
                    false,
                    true,
                    oauth_authenticated,
                    Some(err.to_string()),
                    Vec::new(),
                ),
            }
        } else {
            (
                false,
                false,
                oauth_authenticated_for_server(&root, &config_key),
                None,
                Vec::new(),
            )
        };

    Ok(McpServerOAuthLoginPollResult {
        config_key,
        normalized_key,
        status: poll.status,
        message: poll.message,
        interval_secs: poll.interval_secs,
        ok,
        tool_list_checked,
        oauth_authenticated,
        list_tools_error,
        tools,
    })
}

/// Remove the stored OAuth credential for one remote HTTP MCP server.
#[tauri::command]
pub async fn logout_mcp_oauth_server(
    app_state: State<'_, OmigaAppState>,
    project_root: String,
    server_name: String,
) -> CommandResult<McpServerOAuthLogoutResult> {
    let root = resolve_project_root(&project_root)?;
    let config_key = server_name.trim().to_string();
    let normalized_key = normalize_name_for_mcp(&config_key);
    let cfg = merged_mcp_servers(&root)
        .remove(&config_key)
        .ok_or_else(|| {
            crate::errors::AppError::Config(format!("MCP server `{config_key}` was not found"))
        })?;
    let McpServerConfig::Url { url, .. } = cfg else {
        return Err(crate::errors::AppError::Config(format!(
            "MCP server `{config_key}` is not a remote HTTP server."
        )));
    };

    oauth::clear_stored_credentials_for_url(&url).map_err(crate::errors::AppError::Config)?;
    invalidate_integrations_catalog_cache(&app_state, &root);
    if let Ok(mut cache) = app_state.chat.mcp_tool_cache.try_lock() {
        cache.remove(&root);
    }
    app_state
        .chat
        .mcp_manager
        .close_project_connections(&root)
        .await;

    Ok(McpServerOAuthLogoutResult {
        config_key,
        normalized_key,
    })
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
    match build_integrations_catalog(app_state, root.clone(), false).await {
        Ok(catalog) => {
            if let Ok(mut g) = app_state.integrations_catalog_cache.lock() {
                g.insert(root.clone(), catalog);
            }
            tracing::info!("Integrations catalog warmed for {:?}", root);
        }
        Err(e) => tracing::warn!("Integrations catalog warm failed: {}", e),
    }
}
