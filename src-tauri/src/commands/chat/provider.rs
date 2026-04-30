//! LLM provider configuration CRUD commands.

use super::settings::{LlmConfigResponse, ProviderConfigEntry};
use super::{
    get_llm_config, stream_llm_response_with_cancel, AgentLlmRuntime, CommandResult,
    StreamLlmRequest,
};
use crate::app_state::OmigaAppState;
use crate::domain::persistence::{NewMessageRecord, NewOrchestrationEventRecord};
use crate::domain::skills;
use crate::errors::OmigaError;
use crate::infrastructure::streaming::StreamOutputItem;
use crate::llm::{LlmConfig, LlmMessage, LlmProvider};
use regex::Regex;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;
use tauri::{Emitter, State};
use tokio::sync::{Mutex, RwLock};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveProviderConfigRequest {
    name: String,
    provider_type: String,
    api_key: String,
    model: String,
    secret_key: Option<String>,
    app_id: Option<String>,
    base_url: Option<String>,
    set_as_default: Option<bool>,
    thinking: Option<bool>,
    /// DeepSeek only: "high" or "max" (only meaningful when thinking = true).
    reasoning_effort: Option<String>,
}

#[tauri::command]
pub async fn list_provider_configs(
    state: State<'_, OmigaAppState>,
) -> CommandResult<Vec<ProviderConfigEntry>> {
    // Runtime LLM config (quick-switch / last resolved load) — used to mark which row matches reality
    let current_config = state.chat.llm_config.lock().await;
    let runtime = current_config.clone();
    drop(current_config);

    let active_entry = state.chat.active_provider_entry_name.lock().await.clone();

    // Load from the in-memory config cache — avoids repeated disk reads when the
    // ProviderSwitcher fires list_provider_configs on every session switch.
    let config_file = match get_config_file(&state).await {
        Ok(cf) => cf,
        Err(_) => return Ok(vec![]),
    };

    let providers = config_file.providers.clone().unwrap_or_default();
    let default_provider = config_file.default_provider.clone().unwrap_or_default();

    let entries: Vec<ProviderConfigEntry> = providers
        .iter()
        .map(|(name, config)| {
            let api_key_preview = config
                .api_key
                .as_ref()
                .map(|k| {
                    if k.len() > 8 {
                        format!("{}...", &k[..8])
                    } else {
                        k.clone()
                    }
                })
                .unwrap_or_default();

            // Prefer the explicit saved config name so two entries with the same provider+model
            // cannot both show "In use".
            let matches_runtime = match active_entry.as_deref() {
                Some(active) => name == active,
                None => runtime.as_ref().is_some_and(|c| {
                    let Ok(pt) = config.provider_type.parse::<LlmProvider>() else {
                        return false;
                    };
                    let entry_model = config.model.as_deref().unwrap_or("").trim();
                    let run_model = c.model.trim();
                    pt == c.provider && entry_model == run_model
                }),
            };

            ProviderConfigEntry {
                name: name.clone(),
                provider_type: config.provider_type.clone(),
                model: config.model.clone().unwrap_or_default(),
                api_key_preview,
                base_url: config.base_url.clone(),
                thinking: config.thinking,
                reasoning_effort: config.reasoning_effort.clone(),
                enabled: config.enabled,
                is_session_active: matches_runtime,
                is_default: name == &default_provider,
            }
        })
        .collect();

    Ok(entries)
}

/// Switch to a different provider by name
#[tauri::command]
pub async fn switch_provider(
    state: State<'_, OmigaAppState>,
    provider_name: String,
) -> CommandResult<LlmConfigResponse> {
    // Load config file
    let config_file = crate::llm::config::load_config_file()
        .map_err(|e| OmigaError::Config(format!("Failed to load config file: {}", e)))?;

    // Find the provider in the config - extract all values by cloning
    let provider_config = config_file
        .providers
        .as_ref()
        .and_then(|p| p.get(&provider_name))
        .cloned()
        .ok_or_else(|| OmigaError::Config(format!("Provider '{}' not found", provider_name)))?;

    if !provider_config.enabled {
        return Err(OmigaError::Config(format!(
            "Provider '{}' is disabled",
            provider_name
        )));
    }

    // Parse provider type
    let provider_enum = provider_config
        .provider_type
        .parse::<LlmProvider>()
        .map_err(|e| OmigaError::Config(format!("Invalid provider type: {}", e)))?;

    // Get API key with env var expansion
    let api_key = provider_config
        .api_key
        .as_ref()
        .map(|k| expand_env_vars(k))
        .filter(|k| !k.is_empty())
        .ok_or_else(|| {
            OmigaError::Config(format!("API key not set for provider '{}'", provider_name))
        })?;

    // Build config
    let mut config = LlmConfig::new(provider_enum, api_key);

    if let Some(secret) = provider_config.secret_key {
        config.secret_key = Some(expand_env_vars(&secret));
    }
    if let Some(app_id) = provider_config.app_id {
        config.app_id = Some(expand_env_vars(&app_id));
    }
    if let Some(model) = provider_config.model {
        config.model = expand_env_vars(&model);
    }
    if let Some(url) = provider_config.base_url {
        config.base_url = Some(expand_env_vars(&url));
    }
    if let Some(tokens) = provider_config.max_tokens {
        config.max_tokens = tokens;
    }
    if let Some(temp) = provider_config.temperature {
        config.temperature = Some(temp);
    }
    if let Some(timeout) = provider_config.timeout {
        config.timeout_secs = timeout;
    }
    match provider_enum {
        LlmProvider::Moonshot | LlmProvider::Custom | LlmProvider::Deepseek => {
            config.thinking = Some(provider_config.thinking.unwrap_or(false));
        }
        _ => {
            config.thinking = None;
        }
    }
    if matches!(provider_enum, LlmProvider::Deepseek) {
        config.reasoning_effort = provider_config.reasoning_effort.clone();
    }

    // Update active config in state (session only — does not change `default` in omiga.yaml)
    let mut config_guard = state.chat.llm_config.lock().await;
    *config_guard = Some(config.clone());
    drop(config_guard);
    *state.chat.active_provider_entry_name.lock().await = Some(provider_name);

    Ok(LlmConfigResponse {
        provider: format!("{:?}", provider_enum),
        api_key_preview: if config.api_key.len() > 8 {
            format!("{}...", &config.api_key[..8])
        } else {
            config.api_key.clone()
        },
        model: Some(config.model),
        base_url: config.base_url,
        thinking: config.thinking,
    })
}

/// Save a provider configuration to the multi-provider config file.
/// `thinking`: when set, applies to Moonshot/Custom only; other provider types clear stored thinking.
#[tauri::command]
pub async fn save_provider_config(
    state: State<'_, OmigaAppState>,
    request: SaveProviderConfigRequest,
) -> CommandResult<()> {
    // Validate required fields
    if request.name.trim().is_empty() {
        return Err(OmigaError::Config(
            "Configuration name is required".to_string(),
        ));
    }
    if request.provider_type.trim().is_empty() {
        return Err(OmigaError::Config("Provider type is required".to_string()));
    }
    if request.model.trim().is_empty() {
        return Err(OmigaError::Config("Model name is required".to_string()));
    }

    let provider_enum = request
        .provider_type
        .parse::<LlmProvider>()
        .map_err(|e| OmigaError::Config(format!("Invalid provider type: {}", e)))?;

    // Load existing config or create new one
    let mut config_file = crate::llm::config::load_config_file().unwrap_or_default();

    // Ensure providers map exists
    if config_file.providers.is_none() {
        config_file.providers = Some(std::collections::HashMap::new());
    }

    let providers = config_file.providers.as_mut().unwrap();

    // Check if we're updating an existing provider and need to preserve the existing API key
    let final_api_key = if request.api_key == "${KEEP_EXISTING}" {
        // Preserve existing API key when editing
        let existing = providers
            .get(&request.name)
            .and_then(|p| p.api_key.clone())
            .filter(|k| !k.is_empty());
        if existing.is_none() {
            return Err(OmigaError::Config(
                "API key is required (existing key not found)".to_string(),
            ));
        }
        existing.unwrap()
    } else {
        if request.api_key.trim().is_empty() {
            return Err(OmigaError::Config("API key is required".to_string()));
        }
        request.api_key.clone()
    };

    let existing_thinking = providers.get(&request.name).and_then(|p| p.thinking);
    let existing_reasoning_effort = providers
        .get(&request.name)
        .and_then(|p| p.reasoning_effort.clone());

    let thinking_for_entry = match provider_enum {
        crate::llm::LlmProvider::Moonshot
        | crate::llm::LlmProvider::Custom
        | crate::llm::LlmProvider::Deepseek => match request.thinking {
            Some(t) => Some(t),
            None => existing_thinking.or(Some(false)),
        },
        _ => None,
    };

    let reasoning_effort_for_entry = match provider_enum {
        crate::llm::LlmProvider::Deepseek => request
            .reasoning_effort
            .clone()
            .or(existing_reasoning_effort),
        _ => None,
    };

    // Create or update provider config
    let provider_config = crate::llm::config::ProviderConfig {
        provider_type: request.provider_type.clone(),
        api_key: Some(final_api_key),
        secret_key: request.secret_key,
        app_id: request.app_id,
        base_url: request.base_url,
        model: Some(request.model),
        enabled: true,
        thinking: thinking_for_entry,
        reasoning_effort: reasoning_effort_for_entry,
        ..Default::default()
    };

    providers.insert(request.name.clone(), provider_config);

    // Set as default if requested
    if request.set_as_default.unwrap_or(false) {
        config_file.default_provider = Some(request.name.clone());

        // Also update the active config in state
        let saved = providers.get(&request.name).unwrap();
        let mut new_config =
            LlmConfig::new(provider_enum, saved.api_key.clone().unwrap_or_default())
                .with_model(saved.model.clone().unwrap_or_default());
        if let Some(url) = &saved.base_url {
            new_config.base_url = Some(expand_env_vars(url));
        }
        new_config.thinking = match provider_enum {
            LlmProvider::Moonshot | LlmProvider::Custom | LlmProvider::Deepseek => {
                Some(saved.thinking.unwrap_or(false))
            }
            _ => None,
        };
        new_config.reasoning_effort = match provider_enum {
            LlmProvider::Deepseek => saved.reasoning_effort.clone(),
            _ => None,
        };
        let mut config_guard = state.chat.llm_config.lock().await;
        *config_guard = Some(new_config);
        *state.chat.active_provider_entry_name.lock().await = Some(request.name.clone());
    }

    // Save to config file - use standard location if not found
    let config_path = crate::llm::config::find_config_file()
        .or_else(|| dirs::config_dir().map(|d| d.join("omiga").join("omiga.yaml")))
        .ok_or_else(|| OmigaError::Config("Could not determine config file path".to_string()))?;

    // Ensure directory exists
    if let Some(parent) = config_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    crate::llm::config::save_config_file(&config_file, &config_path)
        .map_err(|e| OmigaError::Config(format!("Failed to save config: {}", e)))?;
    invalidate_config_file_cache(&state).await;

    Ok(())
}

/// Delete a provider configuration
#[tauri::command]
pub async fn delete_provider_config(
    state: State<'_, OmigaAppState>,
    name: String,
) -> CommandResult<()> {
    // Load existing config
    let mut config_file = crate::llm::config::load_config_file()
        .map_err(|e| OmigaError::Config(format!("Failed to load config: {}", e)))?;

    let providers = config_file
        .providers
        .as_mut()
        .ok_or_else(|| OmigaError::Config("No providers configured".to_string()))?;

    if providers.remove(&name).is_none() {
        return Err(OmigaError::Config(format!("Provider '{}' not found", name)));
    }

    // If we removed the default, pick a new one
    if config_file.default_provider.as_ref() == Some(&name) {
        config_file.default_provider = providers.keys().next().cloned();
    }

    // Clear "in use" tracking if we deleted that row (or follow the new default key).
    {
        let mut active = state.chat.active_provider_entry_name.lock().await;
        if active.as_deref() == Some(name.as_str()) {
            *active = config_file.default_provider.clone();
        }
    }

    // Save back to file
    if let Some(path) = crate::llm::config::find_config_file() {
        crate::llm::config::save_config_file(&config_file, &path)
            .map_err(|e| OmigaError::Config(format!("Failed to save config: {}", e)))?;
        invalidate_config_file_cache(&state).await;
    }

    Ok(())
}

/// Apply a named `omiga.yaml` provider entry to in-memory chat state (no file write).
/// Return the cached `LlmConfigFile`, loading from disk only on first access.
/// The cache is invalidated by `invalidate_config_file_cache` whenever the file is written.
pub(crate) async fn get_config_file(
    state: &OmigaAppState,
) -> Result<std::sync::Arc<crate::llm::config::LlmConfigFile>, OmigaError> {
    let mut guard = state.chat.cached_config_file.lock().await;
    if let Some(cached) = guard.as_ref() {
        return Ok(std::sync::Arc::clone(cached));
    }
    // Cache miss — load from disk (blocking I/O, acceptable here because it is rare).
    let cf = crate::llm::config::load_config_file()
        .map_err(|e| OmigaError::Config(format!("Failed to load config file: {}", e)))?;
    let arc = std::sync::Arc::new(cf);
    *guard = Some(std::sync::Arc::clone(&arc));
    Ok(arc)
}

/// Invalidate the in-memory config file cache.  Must be called after every `save_config_file`
/// so the next session switch re-reads the updated file from disk.
pub(crate) async fn invalidate_config_file_cache(state: &OmigaAppState) {
    *state.chat.cached_config_file.lock().await = None;
}

pub(crate) async fn apply_named_provider_runtime(
    state: &OmigaAppState,
    provider_name: &str,
) -> Result<LlmConfigResponse, OmigaError> {
    let name = provider_name.trim();
    if name.is_empty() {
        return Err(OmigaError::Config("Provider name is required".to_string()));
    }

    let config_file = get_config_file(state).await?;

    let providers = config_file
        .providers
        .as_ref()
        .ok_or_else(|| OmigaError::Config("No providers configured".to_string()))?;

    let provider_config = providers
        .get(name)
        .ok_or_else(|| OmigaError::Config(format!("Provider '{}' not found", name)))?;

    if !provider_config.enabled {
        return Err(OmigaError::Config(format!(
            "Provider '{}' is disabled",
            name
        )));
    }

    let provider_enum = provider_config
        .provider_type
        .parse::<LlmProvider>()
        .map_err(|e| OmigaError::Config(format!("Invalid provider type: {}", e)))?;

    let api_key = provider_config
        .api_key
        .as_ref()
        .map(|k| expand_env_vars(k))
        .filter(|k| !k.is_empty())
        .ok_or_else(|| OmigaError::Config(format!("API key not set for provider '{}'", name)))?;

    let mut config = LlmConfig::new(provider_enum, api_key);

    if let Some(model) = &provider_config.model {
        config.model = expand_env_vars(model);
    }
    if let Some(url) = &provider_config.base_url {
        config.base_url = Some(expand_env_vars(url));
    }
    match provider_enum {
        LlmProvider::Moonshot | LlmProvider::Custom | LlmProvider::Deepseek => {
            config.thinking = Some(provider_config.thinking.unwrap_or(false));
        }
        _ => {
            config.thinking = None;
        }
    }
    if matches!(provider_enum, LlmProvider::Deepseek) {
        config.reasoning_effort = provider_config.reasoning_effort.clone();
    }

    let mut config_guard = state.chat.llm_config.lock().await;
    *config_guard = Some(config.clone());
    drop(config_guard);
    *state.chat.active_provider_entry_name.lock().await = Some(name.to_string());

    Ok(LlmConfigResponse {
        provider: format!("{:?}", provider_enum),
        api_key_preview: if config.api_key.len() > 8 {
            format!("{}...", &config.api_key[..8])
        } else {
            config.api_key.clone()
        },
        model: Some(config.model),
        base_url: config.base_url,
        thinking: config.thinking,
    })
}

/// Quick switch provider - set active without saving to file (for UI quick-switch).
/// Persists the choice on the given `session_id` when provided (per-session model).
#[tauri::command]
pub async fn quick_switch_provider(
    state: State<'_, OmigaAppState>,
    provider_name: String,
    session_id: Option<String>,
) -> CommandResult<LlmConfigResponse> {
    let name = provider_name.trim();
    if name.is_empty() {
        return Err(OmigaError::Config("Provider name is required".to_string()));
    }

    let resp = apply_named_provider_runtime(&state, name).await?;

    if let Some(sid) = session_id {
        let sid = sid.trim();
        if !sid.is_empty() {
            let repo = &*state.repo;
            repo.set_session_active_provider(sid, Some(name))
                .await
                .map_err(|e| {
                    OmigaError::Persistence(format!("Failed to save session provider: {}", e))
                })?;
            // Also update the per-session config file so the provider choice survives restarts
            // and is returned by `get_session_config`.
            let mut cfg = crate::domain::session::load_session_config(sid);
            cfg.active_provider_entry_name = Some(name.to_string());
            let _ = crate::domain::session::save_session_config(sid, &cfg);
        }
    }

    Ok(resp)
}

/// Set `default_provider` in `omiga.yaml` only — which model starts as default on next launch.
/// Does **not** change [`OmigaAppState::llm_config`] or [`active_provider_entry_name`]
/// (use `quick_switch_provider` for the current session).
#[tauri::command]
pub async fn set_default_provider_config(
    state: State<'_, OmigaAppState>,
    provider_name: String,
) -> CommandResult<()> {
    let name = provider_name.trim();
    if name.is_empty() {
        return Err(OmigaError::Config("Provider name is required".to_string()));
    }

    let mut config_file = crate::llm::config::load_config_file()
        .map_err(|e| OmigaError::Config(format!("Failed to load config file: {}", e)))?;

    let providers = config_file
        .providers
        .as_mut()
        .ok_or_else(|| OmigaError::Config("No providers configured".to_string()))?;

    let entry = providers
        .get(name)
        .ok_or_else(|| OmigaError::Config(format!("Provider '{}' not found", name)))?;

    if !entry.enabled {
        return Err(OmigaError::Config(format!(
            "Provider '{}' is disabled",
            name
        )));
    }

    config_file.default_provider = Some(name.to_string());

    let config_path = crate::llm::config::find_config_file()
        .or_else(|| dirs::config_dir().map(|d| d.join("omiga").join("omiga.yaml")))
        .ok_or_else(|| OmigaError::Config("Could not determine config file path".to_string()))?;

    if let Some(parent) = config_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    crate::llm::config::save_config_file(&config_file, &config_path)
        .map_err(|e| OmigaError::Config(format!("Failed to save config: {}", e)))?;
    invalidate_config_file_cache(&state).await;

    Ok(())
}

// ─── Agent Scheduler ────────────────────────────────────────────────────────

/// Request body for `run_agent_schedule`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunAgentScheduleRequest {
    /// Free-form user request used for task decomposition.
    pub user_request: String,
    /// Working directory (project root) for all subtasks.
    pub project_root: String,
    /// Session id for tool-results dir and background-task attribution.
    pub session_id: String,
    /// Max agents to spawn in parallel (default 5).
    #[serde(default)]
    pub max_agents: Option<usize>,
    /// Whether to let the planner decompose the request (default true).
    #[serde(default = "default_auto_decompose")]
    pub auto_decompose: bool,
    /// Scheduling strategy; `None` means Auto.
    #[serde(default)]
    pub strategy: Option<crate::domain::agents::scheduler::SchedulingStrategy>,
    /// Optional orchestration mode hint (e.g. `autopilot`, `team`, `ralph`) so
    /// the scheduler can attach mode-specific reviewer subtasks.
    #[serde(default)]
    pub mode_hint: Option<String>,
    /// When `true`, skip the confirmation gate even if the scheduler deems it required.
    /// Set by the frontend after the user approves the plan in the confirmation dialog.
    #[serde(default)]
    pub skip_confirmation: bool,
}

/// Request body for executing an already-generated plan without asking the planner to rebuild it.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunExistingAgentPlanRequest {
    /// Existing scheduler plan payload returned by `/plan`.
    pub plan: crate::domain::agents::scheduler::TaskPlan,
    /// Working directory (project root) for all subtasks.
    pub project_root: String,
    /// Parent chat session id for attribution and final synthesis.
    pub session_id: String,
    /// Optional orchestration mode hint (`schedule`, `team`, `autopilot`).
    #[serde(default)]
    pub mode_hint: Option<String>,
    /// Execution strategy to use when running the existing plan.
    #[serde(default)]
    pub strategy: Option<crate::domain::agents::scheduler::SchedulingStrategy>,
}

fn default_auto_decompose() -> bool {
    true
}

async fn register_active_orchestration(
    app_state: &OmigaAppState,
    session_id: &str,
    token: tokio_util::sync::CancellationToken,
) -> String {
    let orch_id = uuid::Uuid::new_v4().to_string();
    let mut orch_map = app_state.chat.active_orchestrations.lock().await;
    orch_map
        .entry(session_id.to_string())
        .or_default()
        .insert(orch_id.clone(), token);
    orch_id
}

async fn deregister_active_orchestration(
    app_state: &OmigaAppState,
    session_id: &str,
    orch_id: &str,
) {
    let mut orch_map = app_state.chat.active_orchestrations.lock().await;
    if let Some(inner) = orch_map.get_mut(session_id) {
        inner.remove(orch_id);
        if inner.is_empty() {
            orch_map.remove(session_id);
        }
    }
}

/// Run the agent scheduler: decompose `user_request`, then execute subtasks in
/// parallel using real LLM sub-agents backed by `spawn_background_agent`.
///
/// The caller must have an active LLM session (API key configured). Returns the
/// full `OrchestrationResult` when all subtasks finish.
pub(crate) async fn run_agent_schedule_inner(
    app: tauri::AppHandle,
    app_state: &OmigaAppState,
    request: RunAgentScheduleRequest,
) -> Result<crate::domain::agents::scheduler::OrchestrationResult, OmigaError> {
    use crate::domain::agents::scheduler::{AgentScheduler, SchedulingRequest};
    use crate::errors::ChatError;

    // Build runtime inheriting execution environment and constraints from the caller's session.
    let project_root_path = std::path::Path::new(&request.project_root);
    let runtime = AgentLlmRuntime::from_app(&app, Some(request.session_id.as_str()))
        .await
        .map_err(|e| OmigaError::Chat(ChatError::StreamError(e)))?
        .with_runtime_context(project_root_path, &request.session_id);

    let max_agents = request.max_agents.unwrap_or(5);
    let sched_req = SchedulingRequest::new(request.user_request.clone())
        .with_project_root(request.project_root.clone())
        .with_mode_hint(request.mode_hint.clone().unwrap_or_default())
        .with_parallel(true)
        .with_max_agents(max_agents)
        .with_auto_decompose(request.auto_decompose)
        .with_strategy(
            request
                .strategy
                .unwrap_or(crate::domain::agents::scheduler::SchedulingStrategy::Auto),
        );

    let scheduler = AgentScheduler::new();
    let sched_llm_cfg = get_llm_config(&app_state.chat).await.ok();

    // Step 1: build the execution plan (LLM planner → heuristic fallback).
    let sched_result = scheduler
        .schedule(sched_req, sched_llm_cfg.as_ref())
        .await
        .map_err(|e| OmigaError::Chat(ChatError::StreamError(e)))?;

    tracing::info!(
        target: "omiga::scheduler",
        plan_id = %sched_result.plan.plan_id,
        subtasks = sched_result.plan.subtasks.len(),
        agents = ?sched_result.selected_agents,
        "Agent schedule plan built"
    );

    // Step 1b: confirmation gate — emit event and return early when the plan requires approval.
    if sched_result.requires_confirmation && !request.skip_confirmation {
        let pending_plan = sched_result.plan.clone();
        let pending_plan_id = pending_plan.plan_id.clone();
        let _ = app.emit(
            "agent-schedule-confirmation-required",
            serde_json::json!({
                "sessionId": request.session_id.clone(),
                "planId": pending_plan_id,
                "summary": sched_result.confirmation_message
                    .as_deref()
                    .unwrap_or("此计划需要用户确认后才能执行"),
                "estimatedMinutes": sched_result.estimated_duration_secs.div_ceil(60),
                "agents": sched_result.selected_agents.clone(),
                // Send the exact plan that was reviewed so approval never triggers a re-plan.
                "plan": pending_plan,
                "projectRoot": request.project_root.clone(),
                "strategy": sched_result.recommended_strategy,
                "modeHint": request.mode_hint.clone(),
                // Legacy echo for older UI surfaces; confirmation approval should prefer `plan`.
                "originalRequest": {
                    "userRequest": request.user_request.clone(),
                    "projectRoot": request.project_root.clone(),
                    "sessionId": request.session_id.clone(),
                    "maxAgents": request.max_agents,
                    "autoDecompose": request.auto_decompose,
                    "strategy": request.strategy,
                    "modeHint": request.mode_hint.clone(),
                    "skipConfirmation": true,
                }
            }),
        );
        use crate::domain::agents::scheduler::orchestrator::{
            ExecutionStatus, OrchestrationResult,
        };
        return Ok(OrchestrationResult {
            plan_id: sched_result.plan.plan_id,
            status: ExecutionStatus::Pending,
            subtask_results: Default::default(),
            execution_log: vec![],
            started_at: None,
            completed_at: None,
            final_summary: "等待用户确认后执行".to_string(),
        });
    }

    // Step 2: execute with real agents (preserve strategy so execute_with_runtime can branch on it).
    let exec_req = SchedulingRequest::new(request.user_request.clone())
        .with_project_root(request.project_root.clone())
        .with_mode_hint(request.mode_hint.clone().unwrap_or_default())
        .with_strategy(
            request
                .strategy
                .unwrap_or(crate::domain::agents::scheduler::SchedulingStrategy::Auto),
        );
    // Register only once execution actually starts. Pending confirmation plans are not
    // cancellable work yet and must not leave stale active-orchestration tokens behind.
    let orch_cancel = tokio_util::sync::CancellationToken::new();
    let orch_id =
        register_active_orchestration(app_state, &request.session_id, orch_cancel.clone()).await;
    let orch_result = scheduler
        .execute_plan_with_runtime(
            &sched_result.plan,
            &exec_req,
            &app,
            &runtime,
            &request.session_id,
            orch_cancel.clone(),
        )
        .await
        .map_err(|e| OmigaError::Chat(ChatError::StreamError(e)));

    // Deregister this orchestration's cancel token when execution finishes.
    deregister_active_orchestration(app_state, &request.session_id, &orch_id).await;

    let orch_result = orch_result?;

    // Step 3: inject a synthesized assistant message into the parent session so the
    // conversation history reflects the orchestration outcome.
    inject_schedule_summary_message(
        &app,
        &request.session_id,
        &request.user_request,
        &orch_result,
        &runtime,
    )
    .await;

    Ok(orch_result)
}

#[tauri::command]
pub async fn run_agent_schedule(
    app: tauri::AppHandle,
    app_state: State<'_, OmigaAppState>,
    request: RunAgentScheduleRequest,
) -> CommandResult<crate::domain::agents::scheduler::OrchestrationResult> {
    run_agent_schedule_inner(app, &app_state, request).await
}

/// Execute a plan that was already generated and shown to the user.
///
/// This is intentionally different from `run_agent_schedule`: it does **not** call the planner
/// again, so `/plan` → "执行此计划" preserves the visible steps instead of silently replanning.
#[tauri::command]
pub async fn run_existing_agent_plan(
    app: tauri::AppHandle,
    app_state: State<'_, OmigaAppState>,
    request: RunExistingAgentPlanRequest,
) -> CommandResult<crate::domain::agents::scheduler::OrchestrationResult> {
    use crate::domain::agents::scheduler::{AgentScheduler, SchedulingRequest, TaskPlanner};
    use crate::errors::ChatError;

    let project_root_path = std::path::Path::new(&request.project_root);
    let runtime = AgentLlmRuntime::from_app(&app, Some(request.session_id.as_str()))
        .await
        .map_err(|e| OmigaError::Chat(ChatError::StreamError(e)))?
        .with_runtime_context(project_root_path, &request.session_id);

    let strategy = request
        .strategy
        .unwrap_or(crate::domain::agents::scheduler::SchedulingStrategy::Phased);
    let sched_req = SchedulingRequest::new(request.plan.original_request.clone())
        .with_project_root(request.project_root.clone())
        .with_mode_hint(
            request
                .mode_hint
                .clone()
                .unwrap_or_else(|| "schedule".to_string()),
        )
        .with_strategy(strategy)
        .with_parallel(request.plan.allow_parallel);

    let plan = if strategy == crate::domain::agents::scheduler::SchedulingStrategy::Team {
        TaskPlanner::new().ensure_team_verify(request.plan, &sched_req)
    } else {
        request.plan
    }
    .with_execution_defaults();

    let _ = app_state
        .repo
        .append_orchestration_event(NewOrchestrationEventRecord {
            session_id: &request.session_id,
            round_id: None,
            message_id: None,
            mode: request.mode_hint.as_deref().or(Some("schedule")),
            event_type: "approved_plan_execution_started",
            phase: Some("executing"),
            task_id: None,
            payload_json: &serde_json::json!({
                "planId": plan.plan_id,
                "taskCount": plan.subtasks.len(),
                "strategy": format!("{:?}", strategy),
            })
            .to_string(),
        })
        .await;

    let scheduler = AgentScheduler::new();
    let orch_cancel = tokio_util::sync::CancellationToken::new();
    let orch_id =
        register_active_orchestration(&app_state, &request.session_id, orch_cancel.clone()).await;
    let orch_result = scheduler
        .execute_plan_with_runtime(
            &plan,
            &sched_req,
            &app,
            &runtime,
            &request.session_id,
            orch_cancel.clone(),
        )
        .await
        .map_err(|e| OmigaError::Chat(ChatError::StreamError(e)));

    deregister_active_orchestration(&app_state, &request.session_id, &orch_id).await;

    let orch_result = orch_result?;
    inject_schedule_summary_message(
        &app,
        &request.session_id,
        &plan.original_request,
        &orch_result,
        &runtime,
    )
    .await;

    Ok(orch_result)
}

/// Cancel all running agent orchestrations for the given session.
#[tauri::command]
pub async fn cancel_agent_schedule(
    app_state: State<'_, OmigaAppState>,
    session_id: String,
) -> CommandResult<bool> {
    let _ = app_state
        .repo
        .append_orchestration_event(NewOrchestrationEventRecord {
            session_id: &session_id,
            round_id: None,
            message_id: None,
            mode: None,
            event_type: "cancel_requested",
            phase: None,
            task_id: None,
            payload_json: &serde_json::json!({}).to_string(),
        })
        .await;
    let tokens: Vec<tokio_util::sync::CancellationToken> = {
        let orch_map = app_state.chat.active_orchestrations.lock().await;
        orch_map
            .get(&session_id)
            .map(|inner| inner.values().cloned().collect())
            .unwrap_or_default()
    };
    if tokens.is_empty() {
        return Ok(false);
    }
    for token in &tokens {
        token.cancel();
    }
    // Also cancel all background agent tasks associated with this session.
    use crate::domain::agents::background::get_background_agent_manager;
    let mgr = get_background_agent_manager();
    let tasks = mgr.get_session_tasks(&session_id).await;
    for t in tasks {
        if t.status == crate::domain::agents::background::BackgroundAgentStatus::Running
            || t.status == crate::domain::agents::background::BackgroundAgentStatus::Pending
        {
            mgr.cancel_task(&t.task_id).await;
        }
    }
    {
        let mut orch_map = app_state.chat.active_orchestrations.lock().await;
        orch_map.remove(&session_id);
    }
    let _ = app_state
        .repo
        .append_orchestration_event(NewOrchestrationEventRecord {
            session_id: &session_id,
            round_id: None,
            message_id: None,
            mode: None,
            event_type: "cancel_completed",
            phase: None,
            task_id: None,
            payload_json: &serde_json::json!({ "cancelled": true }).to_string(),
        })
        .await;
    Ok(true)
}

/// Persist a synthesized assistant message after orchestration and notify the frontend.
///
/// Attempts to call the LLM once to synthesize all sub-agent outputs into a coherent
/// user-facing reply with next-step suggestions. Falls back to a mechanical summary if
/// the synthesis call fails.
pub(crate) async fn inject_schedule_summary_message(
    app: &tauri::AppHandle,
    session_id: &str,
    user_request: &str,
    result: &crate::domain::agents::scheduler::OrchestrationResult,
    runtime: &AgentLlmRuntime,
) {
    use crate::app_state::OmigaAppState;
    use tauri::Manager;

    let status_label = match result.status {
        crate::domain::agents::scheduler::orchestrator::ExecutionStatus::Completed => "全部成功",
        crate::domain::agents::scheduler::orchestrator::ExecutionStatus::PartiallyCompleted => {
            "部分完成"
        }
        crate::domain::agents::scheduler::orchestrator::ExecutionStatus::Failed => "执行失败",
        crate::domain::agents::scheduler::orchestrator::ExecutionStatus::Cancelled => "已取消",
        _ => "异常终止",
    };

    if let Err(e) = runtime
        .repo()
        .append_orchestration_event(NewOrchestrationEventRecord {
            session_id,
            round_id: Some(runtime.round_id()),
            message_id: None,
            mode: Some("schedule"),
            event_type: "leader_summary_started",
            phase: Some("synthesizing"),
            task_id: None,
            payload_json: &serde_json::json!({
                "planId": result.plan_id,
                "status": status_label,
                "entryAgentType": "general-purpose",
            })
            .to_string(),
        })
        .await
    {
        tracing::warn!(target: "omiga::scheduler", "Failed to append leader_summary_started event: {}", e);
    }

    let is_reviewer = |agent_type: &str| {
        matches!(
            agent_type,
            "verification"
                | "code-reviewer"
                | "security-reviewer"
                | "performance-reviewer"
                | "quality-reviewer"
                | "api-reviewer"
                | "critic"
                | "test-engineer"
        )
    };

    let mut reviewer_outputs: Vec<(String, String)> = Vec::new();
    let mut worker_outputs: Vec<(String, String)> = Vec::new();
    let mut reviewer_verdicts: Vec<crate::domain::agents::reviewer_verdict::ReviewerVerdict> =
        Vec::new();
    for subtask in result.subtask_results.values() {
        let Some(output) = subtask.output.as_deref().filter(|s| !s.is_empty()) else {
            continue;
        };
        let agent = subtask
            .agent_type
            .clone()
            .unwrap_or_else(|| "unknown".to_string());
        if is_reviewer(agent.as_str()) {
            reviewer_outputs.push((agent.clone(), output.to_string()));
            reviewer_verdicts.push(
                crate::domain::agents::reviewer_verdict::parse_reviewer_verdict(&agent, output),
            );
        } else {
            worker_outputs.push((agent, output.to_string()));
        }
    }

    // Collect sub-agent outputs that have content.
    let outputs: Vec<String> = result
        .subtask_results
        .values()
        .filter_map(|r| r.output.as_deref().filter(|s| !s.is_empty()))
        .map(|s| s.to_string())
        .collect();

    let synthesis = if !outputs.is_empty()
        && result.status
            != crate::domain::agents::scheduler::orchestrator::ExecutionStatus::Cancelled
    {
        // Build a single-turn synthesis prompt.
        let worker_outputs_block = worker_outputs
            .iter()
            .enumerate()
            .map(|(i, (agent, o))| {
                let truncated = o.chars().take(8_000).collect::<String>();
                format!(
                    "### 任务 {}（{}）输出\n\n{}",
                    i + 1,
                    agent,
                    truncated.trim()
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n---\n\n");
        let reviewer_outputs_block = reviewer_outputs
            .iter()
            .enumerate()
            .map(|(i, (agent, o))| {
                let truncated = o.chars().take(8_000).collect::<String>();
                format!(
                    "### Reviewer {}（{}）意见\n\n{}",
                    i + 1,
                    agent,
                    truncated.trim()
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n---\n\n");
        let reviewer_structured_block = reviewer_verdicts
            .iter()
            .map(|v| {
                format!(
                    "- `{}` | severity=`{:?}` | verdict=`{:?}` | summary={}{}",
                    v.agent_type,
                    v.severity,
                    v.verdict,
                    v.summary,
                    v.recommendation
                        .as_deref()
                        .map(|r| format!(" | recommendation={}", r))
                        .unwrap_or_default()
                )
            })
            .collect::<Vec<_>>()
            .join("\n");

        let citation_instruction =
            "\n\n**引用格式规则（严格执行）：**\n\
             - 所有文献引用必须格式化为可点击/可 hover 的链接；优先 Markdown 超链接，也可使用安全 HTML 锚点 `<a href=\"https://...\">标签</a>`\n\
             - 禁止裸文本形式（如 `[PMID: 12345678]` 或 `[1]`）；禁止把 URL 只裸露为纯文本\n\
             - PubMed：`[PMID: 12345678](https://pubmed.ncbi.nlm.nih.gov/12345678/)`\n\
             - DOI：`[Smith et al., 2023](https://doi.org/10.XXXX/YYYY)`\n\
             - arXiv：`[Smith et al., 2023](https://arxiv.org/abs/XXXX.XXXXX)`\n\
             - 链接文本应使用期刊/来源、作者年份、PMID/DOI 或论文标题，不要只用裸 URL\n\
             - 引用紧跟对应陈述就近嵌入；如需文末参考文献列表，仍要保留正文内联链接引用\n\
             - 禁止丢弃已有链接，禁止编造引用";

        let synthesis_prompt = format!(
            "你是一个任务协调助手（Leader）。多个专职 Worker Agent 已经并行完成了以下任务，\
            请将它们的输出综合成一个完整、连贯的最终回复。\n\n\
            **用户的原始请求：**\n{}\n\n\
            **当前整体状态：** {}\n\n\
            **各 Worker 输出：**\n\n{}\n\n\
            **Reviewer 结构化结论：**\n{}\n\n\
            **Reviewer 结论：**\n\n{}\n\n\
            综合要求：\n\
            1. 用清晰的 Markdown 结构直接回答用户问题，不得提及「子任务」「Agent」「Worker」等内部概念\n\
            2. 保留所有实质性内容，不要因为「精简」而丢弃数据、结论或引用\n\
            2.5. 明确吸收 reviewer 结论：如果 reviewer 提出阻断性问题，不要把结果表述为完全完成；需要在主文中如实反映风险和限制\n\
            3. 【Markdown 表格严格规则】如需使用表格：\n\
               - 每个单元格内容必须写在同一行，禁止在单元格内换行\n\
               - 需要列出多个条目时，用「①②③」或「；」分隔，写在一行内\n\
               - 如果单元格内容确实过长无法单行表达，放弃使用表格，改用「**标题**」+缩进列表的形式\n\
            4. 加入 `### 下一步建议`，用编号列表列出 2-3 条具体建议（格式：`1. 建议内容`）。如果回复包含 `## References` / `## 参考文献`，下一步建议必须放在参考文献之前，让参考文献保持为最后一个章节，便于 UI 折叠展示{}\n",
            user_request,
            status_label,
            worker_outputs_block,
            reviewer_structured_block,
            reviewer_outputs_block,
            citation_instruction
        );

        match crate::llm::create_client(runtime.llm_config.clone()) {
            Ok(client) => {
                let messages = vec![LlmMessage::user(synthesis_prompt)];
                let stream_msg_id = uuid::Uuid::new_v4().to_string();
                let stream_round_id = uuid::Uuid::new_v4().to_string();
                let cancel_flag = Arc::new(RwLock::new(false));
                let pending_tools = Arc::new(Mutex::new(HashMap::new()));

                // Tell frontend a new streaming message is starting so it can subscribe
                // to the chat-stream-{id} events BEFORE the first chunk arrives.
                use tauri::Emitter as _;
                let _ = app.emit(
                    "chat-synthesis-start",
                    serde_json::json!({
                        "sessionId": session_id,
                        "messageId": stream_msg_id,
                    }),
                );

                let stream_result = stream_llm_response_with_cancel(StreamLlmRequest {
                    client: client.as_ref(),
                    app,
                    message_id: &stream_msg_id,
                    round_id: &stream_round_id,
                    messages: &messages,
                    tools: &[],
                    emit_text_chunks: true,
                    pending_tools: &pending_tools,
                    cancel_flag: &cancel_flag,
                    repo: runtime.repo.clone(),
                })
                .await;

                // Always emit Complete for the synthesis stream so the frontend exits
                // streaming state and can display suggestions / accept new input.
                let _ = app.emit(
                    &format!("chat-stream-{}", stream_msg_id),
                    &StreamOutputItem::Complete,
                );

                match stream_result {
                    Ok((_, text, _, cancelled, _)) if !cancelled && !text.is_empty() => {
                        Some((text, stream_msg_id))
                    }
                    Ok(_) | Err(_) => None,
                }
            }
            Err(_) => None,
        }
    } else {
        None
    };

    // synthesis is now Option<(text, stream_msg_id)>
    let (summary, msg_id_to_use) = match synthesis {
        Some((synthesized_text, streamed_msg_id)) => (synthesized_text, streamed_msg_id),
        None => {
            // Fallback: mechanical summary when synthesis is unavailable or skipped.
            let reviewer_block = if reviewer_outputs.is_empty() {
                String::new()
            } else {
                let verdict_lines = reviewer_verdicts
                    .iter()
                    .map(|v| {
                        format!(
                            "- `{}` [{:?}/{:?}] {}",
                            v.agent_type, v.severity, v.verdict, v.summary
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                format!("\n\n**Reviewer 结论:**\n{}", verdict_lines)
            };
            let failed_block = {
                let failed: Vec<_> = result
                    .subtask_results
                    .values()
                    .filter(|r| {
                        r.status == crate::domain::agents::scheduler::orchestrator::ExecutionStatus::Failed
                    })
                    .collect();
                if !failed.is_empty() {
                    format!(
                        "\n\n**失败子任务:**\n{}",
                        failed
                            .iter()
                            .map(|r| format!(
                                "- `{}`: {}",
                                r.subtask_id,
                                r.error.as_deref().unwrap_or("unknown error")
                            ))
                            .collect::<Vec<_>>()
                            .join("\n")
                    )
                } else {
                    String::new()
                }
            };
            let fallback_text = format!(
                "**Agent 编排完成（{}）**\n\n{}{}{}",
                status_label, result.final_summary, reviewer_block, failed_block,
            );
            let fallback_id = uuid::Uuid::new_v4().to_string();
            (fallback_text, fallback_id)
        }
    };

    let state = match app.try_state::<OmigaAppState>() {
        Some(s) => s,
        None => return,
    };

    if let Err(e) = state
        .repo
        .save_message(NewMessageRecord {
            id: &msg_id_to_use,
            session_id,
            role: "assistant",
            content: &summary,
            tool_calls: None,
            tool_call_id: None,
            token_usage_json: None,
            reasoning_content: None,
            follow_up_suggestions_json: None,
            turn_summary: None,
        })
        .await
    {
        tracing::warn!(target: "omiga::scheduler", "Failed to save schedule summary message: {}", e);
        return;
    }

    let (summary_enabled, follow_enabled) =
        crate::domain::post_turn_settings::load_post_turn_meta_flags(&state.repo)
            .await
            .unwrap_or((true, true));

    if summary_enabled || follow_enabled {
        match crate::llm::create_client(runtime.llm_config.clone()) {
            Ok(client) => {
                let turn_summary =
                    match crate::domain::agents::output_formatter::run_turn_summary_pass(
                        client.as_ref(),
                        &summary,
                        summary_enabled,
                    )
                    .await
                    {
                        Ok(value) => value,
                        Err(e) => {
                            tracing::warn!(
                                target: "omiga::scheduler",
                                "Failed to generate orchestration turn summary: {}",
                                e
                            );
                            None
                        }
                    };

                let _ = app.emit(
                    &format!("chat-stream-{}", msg_id_to_use),
                    &StreamOutputItem::TurnSummary {
                        text: turn_summary.clone(),
                    },
                );

                if let Some(turn_summary_text) = turn_summary.as_deref() {
                    if let Err(e) = state
                        .repo
                        .update_message_turn_summary(&msg_id_to_use, Some(turn_summary_text))
                        .await
                    {
                        tracing::warn!(
                            target: "omiga::scheduler",
                            "Failed to persist orchestration turn summary: {}",
                            e
                        );
                    }
                }

                if follow_enabled {
                    let _ = app.emit(
                        &format!("chat-stream-{}", msg_id_to_use),
                        &StreamOutputItem::SuggestionsGenerating,
                    );

                    match crate::domain::suggestions::generate_follow_up_suggestions(
                        client.as_ref(),
                        &summary,
                        follow_enabled,
                    )
                    .await
                    {
                        Ok(items) if !items.is_empty() => {
                            let _ = app.emit(
                                &format!("chat-stream-{}", msg_id_to_use),
                                &StreamOutputItem::FollowUpSuggestions(items.clone()),
                            );
                            if let Ok(json) = serde_json::to_string(&items) {
                                if let Err(e) = state
                                    .repo
                                    .update_message_follow_up_suggestions(
                                        &msg_id_to_use,
                                        Some(&json),
                                    )
                                    .await
                                {
                                    tracing::warn!(
                                        target: "omiga::scheduler",
                                        "Failed to persist orchestration follow-up suggestions: {}",
                                        e
                                    );
                                }
                            }
                            let _ = app.emit(
                                &format!("chat-stream-{}", msg_id_to_use),
                                &StreamOutputItem::SuggestionsComplete {
                                    generated: true,
                                    error: None,
                                },
                            );
                        }
                        Ok(_) => {
                            let _ = app.emit(
                                &format!("chat-stream-{}", msg_id_to_use),
                                &StreamOutputItem::SuggestionsComplete {
                                    generated: false,
                                    error: None,
                                },
                            );
                        }
                        Err(e) => {
                            tracing::warn!(
                                target: "omiga::scheduler",
                                "Failed to generate orchestration follow-up suggestions: {}",
                                e
                            );
                            let _ = app.emit(
                                &format!("chat-stream-{}", msg_id_to_use),
                                &StreamOutputItem::SuggestionsComplete {
                                    generated: false,
                                    error: Some(e.to_string()),
                                },
                            );
                        }
                    }
                }
            }
            Err(e) => {
                tracing::warn!(
                    target: "omiga::scheduler",
                    "Failed to create post-turn client for orchestration summary: {}",
                    e
                );
            }
        }
    }

    // Notify the frontend: the streamed message is now persisted; scroll to it.
    use tauri::Emitter as _;
    let _ = app.emit(
        "agent-schedule-complete",
        serde_json::json!({ "sessionId": session_id, "messageId": msg_id_to_use }),
    );
}

/// Handle the `skill_config` tool: get / set / list skill configuration variables.
pub(super) async fn handle_skill_config(
    project_root: &std::path::Path,
    arguments: &str,
    skill_cache: &std::sync::Arc<std::sync::Mutex<skills::SkillCacheMap>>,
) -> Result<serde_json::Value, String> {
    use crate::domain::tools::skill_config::{ConfigAction, SkillConfigArgs};

    let args: SkillConfigArgs =
        serde_json::from_str(arguments).map_err(|e| format!("skill_config: invalid JSON: {e}"))?;

    match args.action {
        ConfigAction::List => {
            let all_skills = skills::load_skills_cached(project_root, skill_cache).await;
            let mut entries = Vec::new();
            for skill in &all_skills {
                if skill.config_vars.is_empty() {
                    continue;
                }
                let resolved =
                    skills::skill_config::resolve_config_vars(&skill.config_vars, project_root);
                entries.push(serde_json::json!({
                    "skill": skill.name,
                    "config_vars": resolved,
                }));
            }
            Ok(serde_json::json!({ "success": true, "skills": entries, "count": entries.len() }))
        }
        ConfigAction::Get => {
            let skill_name = args
                .skill
                .as_deref()
                .filter(|s| !s.trim().is_empty())
                .ok_or_else(|| "skill_config: `skill` is required for action `get`".to_string())?;
            let all_skills = skills::load_skills_cached(project_root, skill_cache).await;
            let entry = skills::find_skill_entry(&all_skills, skill_name)
                .ok_or_else(|| format!("skill_config: unknown skill `{skill_name}`"))?;
            if entry.config_vars.is_empty() {
                return Ok(serde_json::json!({
                    "success": true,
                    "skill": entry.name,
                    "config_vars": [],
                    "message": "This skill declares no config variables."
                }));
            }
            let resolved =
                skills::skill_config::resolve_config_vars(&entry.config_vars, project_root);
            Ok(serde_json::json!({
                "success": true,
                "skill": entry.name,
                "config_vars": resolved,
                "config_file": skills::project_config_path(project_root),
            }))
        }
        ConfigAction::Set => {
            let skill_name = args
                .skill
                .as_deref()
                .filter(|s| !s.trim().is_empty())
                .ok_or_else(|| "skill_config: `skill` is required for action `set`".to_string())?;
            let key = args
                .key
                .as_deref()
                .filter(|s| !s.trim().is_empty())
                .ok_or_else(|| "skill_config: `key` is required for action `set`".to_string())?;
            let value = args
                .value
                .as_deref()
                .ok_or_else(|| "skill_config: `value` is required for action `set`".to_string())?;

            // Validate the key is declared by the skill.
            let all_skills = skills::load_skills_cached(project_root, skill_cache).await;
            let entry = skills::find_skill_entry(&all_skills, skill_name)
                .ok_or_else(|| format!("skill_config: unknown skill `{skill_name}`"))?;
            if !entry.config_vars.is_empty() && !entry.config_vars.iter().any(|v| v.key == key) {
                return Err(format!(
                    "skill_config: key `{key}` is not declared by skill `{skill_name}`. \
                     Declared keys: {}",
                    entry
                        .config_vars
                        .iter()
                        .map(|v| v.key.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }

            skills::set_config_var(project_root, key, value).await?;
            Ok(serde_json::json!({
                "success": true,
                "skill": skill_name,
                "key": key,
                "value": value,
                "config_file": skills::project_config_path(project_root),
            }))
        }
    }
}

/// Helper to expand environment variables
fn expand_env_vars(s: &str) -> String {
    let mut result = s.to_string();

    // Expand ${VAR}
    while let Some(start) = result.find("${") {
        if let Some(end) = result[start..].find("}") {
            let var_name = &result[start + 2..start + end];
            let var_value = std::env::var(var_name).unwrap_or_default();
            result.replace_range(start..start + end + 1, &var_value);
        } else {
            break;
        }
    }

    // Expand $VAR
    let re = Regex::new(r"\$([A-Za-z_][A-Za-z0-9_]*)").unwrap();
    result = re
        .replace_all(&result, |caps: &regex::Captures| {
            std::env::var(&caps[1]).unwrap_or_else(|_| caps[0].to_string())
        })
        .to_string();

    result
}
