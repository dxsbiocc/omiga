//! LLM settings and API key management commands.

use super::provider::{get_config_file, invalidate_config_file_cache};
use super::subagent::persist_background_agent_task_snapshot;
use super::{get_llm_config, CommandResult};
use crate::app_state::OmigaAppState;
use crate::domain::agents::background::{BackgroundAgentStatus, BackgroundAgentTask};
use crate::domain::persistence::NewOrchestrationEventRecord;
use crate::domain::session::Message;
use crate::errors::{ApiError, ChatError, OmigaError};
use crate::llm::{create_client, load_config_from_env, LlmConfig, LlmProvider};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetLlmConfigRequest {
    provider: String,
    api_key: String,
    secret_key: Option<String>,
    app_id: Option<String>,
    model: Option<String>,
    base_url: Option<String>,
    context_window_tokens: Option<u32>,
    thinking: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveLlmSettingsRequest {
    provider: String,
    api_key: String,
    secret_key: Option<String>,
    app_id: Option<String>,
    model: Option<String>,
    base_url: Option<String>,
    context_window_tokens: Option<u32>,
    thinking: Option<bool>,
    timeout: Option<u64>,
}

#[tauri::command]
pub async fn set_llm_config(
    state: State<'_, OmigaAppState>,
    request: SetLlmConfigRequest,
) -> CommandResult<()> {
    let provider_enum = request
        .provider
        .parse::<LlmProvider>()
        .map_err(|e| OmigaError::Config(format!("Invalid provider: {}", e)))?;

    let mut config = LlmConfig::new(provider_enum, request.api_key);

    // Apply optional settings
    if let Some(secret) = request.secret_key {
        config.secret_key = Some(secret);
    }
    if let Some(id) = request.app_id {
        config.app_id = Some(id);
    }
    if let Some(m) = request.model {
        config.model = m;
    }
    if let Some(url) = request.base_url {
        config.base_url = Some(url);
    }
    config.context_window_tokens = request.context_window_tokens.filter(|n| *n >= 8_192);
    // Moonshot/Custom: always keep an explicit bool in memory (default false) so runtime matches API.
    // DeepSeek and others do not use `thinking`.
    match provider_enum {
        crate::llm::LlmProvider::Moonshot | crate::llm::LlmProvider::Custom => {
            config.thinking = Some(request.thinking.unwrap_or(false));
        }
        _ => {
            config.thinking = None;
        }
    }

    let mut config_guard = state.chat.llm_config.lock().await;
    *config_guard = Some(config);
    *state.chat.active_provider_entry_name.lock().await = None;
    Ok(())
}

/// Persist the Settings panel LLM choice to `omiga.yaml` as `default_provider`.
/// Does **not** overwrite the in-memory provider used for the current chat when the user has
/// switched to another named config via `quick_switch_provider` — only updates `omiga.yaml` and
/// refreshes runtime when no active entry is set or the active entry is the same one being saved.
#[tauri::command]
pub async fn save_llm_settings_to_config(
    state: State<'_, OmigaAppState>,
    request: SaveLlmSettingsRequest,
) -> CommandResult<()> {
    let provider_enum = request
        .provider
        .parse::<LlmProvider>()
        .map_err(|e| OmigaError::Config(format!("Invalid provider: {}", e)))?;

    let model_str = request
        .model
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| provider_enum.default_model().to_string());

    let sk_opt = request.secret_key.and_then(|s| {
        let t = s.trim();
        if t.is_empty() {
            None
        } else {
            Some(t.to_string())
        }
    });
    let aid_opt = request.app_id.and_then(|s| {
        let t = s.trim();
        if t.is_empty() {
            None
        } else {
            Some(t.to_string())
        }
    });
    let bu_opt = request.base_url.and_then(|s| {
        let t = s.trim();
        if t.is_empty() {
            None
        } else {
            Some(t.to_string())
        }
    });

    let mut config_file = crate::llm::config::load_config_file().unwrap_or_default();
    if config_file.providers.is_none() {
        config_file.providers = Some(HashMap::new());
    }
    let providers = config_file.providers.as_mut().unwrap();

    let entry_key = match config_file.default_provider.clone() {
        Some(ref k) if providers.contains_key(k) => k.clone(),
        _ => "default".to_string(),
    };

    let prev_thinking = providers.get(&entry_key).and_then(|p| p.thinking);
    let thinking_resolved = match provider_enum {
        crate::llm::LlmProvider::Moonshot | crate::llm::LlmProvider::Custom => {
            request.thinking.or(prev_thinking).or(Some(false))
        }
        _ => None,
    };

    let provider_cfg = crate::llm::config::ProviderConfig {
        provider_type: request.provider,
        api_key: Some(request.api_key.clone()),
        secret_key: sk_opt.clone(),
        app_id: aid_opt.clone(),
        base_url: bu_opt.clone(),
        model: Some(model_str.clone()),
        context_window_tokens: request.context_window_tokens.filter(|n| *n >= 8_192),
        thinking: thinking_resolved,
        enabled: true,
        ..Default::default()
    };
    providers.insert(entry_key.clone(), provider_cfg);
    config_file.default_provider = Some(entry_key.clone());

    let config_path = crate::llm::config::find_config_file()
        .or_else(|| dirs::config_dir().map(|d| d.join("omiga").join("omiga.yaml")))
        .ok_or_else(|| OmigaError::Config("Could not determine config file path".to_string()))?;

    if let Some(parent) = config_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    if let Some(t) = request.timeout {
        let mut global = config_file.settings.unwrap_or_default();
        global.timeout = Some(t);
        config_file.settings = Some(global);
    }

    crate::llm::config::save_config_file(&config_file, &config_path)
        .map_err(|e| OmigaError::Config(format!("Failed to save config: {}", e)))?;
    invalidate_config_file_cache(&state).await;

    let mut config = LlmConfig::new(provider_enum, request.api_key);
    config.model = model_str;
    config.secret_key = sk_opt;
    config.app_id = aid_opt;
    config.base_url = bu_opt;
    config.context_window_tokens = request.context_window_tokens.filter(|n| *n >= 8_192);
    config.thinking = thinking_resolved;
    if let Some(t) = request.timeout {
        config.timeout_secs = t;
    }

    let active_opt = state.chat.active_provider_entry_name.lock().await.clone();
    let should_apply_runtime = match active_opt.as_deref() {
        None => true,
        Some(active) => active == entry_key.as_str(),
    };

    if should_apply_runtime {
        let mut config_guard = state.chat.llm_config.lock().await;
        *config_guard = Some(config);
        drop(config_guard);
        let mut active_guard = state.chat.active_provider_entry_name.lock().await;
        if active_guard.is_none() {
            *active_guard = Some(entry_key.clone());
        }
    }
    Ok(())
}

/// Persist global settings from the Advanced Settings panel.
#[tauri::command(rename_all = "camelCase")]
pub async fn save_global_settings_to_config(
    state: State<'_, OmigaAppState>,
    timeout: Option<u64>,
    web_use_proxy: Option<bool>,
    web_search_engine: Option<String>,
    web_search_methods: Option<Vec<String>>,
    goal_second_opinion_provider_entry: Option<String>,
) -> CommandResult<()> {
    let mut config_file = get_config_file(&state)
        .await
        .as_deref()
        .cloned()
        .unwrap_or_default();
    let normalized_goal_second_opinion_provider_entry =
        goal_second_opinion_provider_entry.map(|entry| match entry.trim() {
            "" => None,
            value => Some(value.to_string()),
        });
    if let Some(Some(entry)) = normalized_goal_second_opinion_provider_entry.as_ref() {
        config_file.named_llm_config(entry).map_err(|reason| {
            OmigaError::Config(format!(
                "Global /goal second-opinion provider entry `{entry}` is invalid: {reason}"
            ))
        })?;
    }

    let mut global = config_file.settings.take().unwrap_or_default();
    if let Some(t) = timeout {
        global.timeout = Some(t);
    }
    if let Some(v) = web_use_proxy {
        global.web_use_proxy = Some(v);
    }
    if let Some(engine) = web_search_engine {
        global.web_search_engine = Some(normalize_web_search_engine_field(&engine)?);
    }
    if let Some(methods) = web_search_methods {
        global.web_search_methods =
            Some(crate::llm::config::normalize_web_search_methods(&methods));
    }
    if let Some(entry) = normalized_goal_second_opinion_provider_entry {
        global.goal_second_opinion_provider_entry = entry;
    }
    config_file.settings = Some(global);

    let config_path = crate::llm::config::find_config_file()
        .or_else(|| dirs::config_dir().map(|d| d.join("omiga").join("omiga.yaml")))
        .ok_or_else(|| OmigaError::Config("Could not determine config file path".to_string()))?;
    if let Some(parent) = config_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    crate::llm::config::save_config_file(&config_file, &config_path)
        .map_err(|e| OmigaError::Config(format!("Failed to save config: {}", e)))?;
    invalidate_config_file_cache(&state).await;

    if let Some(t) = timeout {
        if let Some(cfg) = state.chat.llm_config.lock().await.as_mut() {
            cfg.timeout_secs = t;
        }
    }
    Ok(())
}

fn normalize_web_search_engine_field(engine: &str) -> CommandResult<String> {
    match engine.trim().to_ascii_lowercase().as_str() {
        "google" => Ok("google".to_string()),
        "bing" => Ok("bing".to_string()),
        "duckduckgo" | "duck-duck-go" | "ddg" => Ok("ddg".to_string()),
        "" => Ok("ddg".to_string()),
        other => Err(OmigaError::Config(format!(
            "Unsupported web search engine `{other}`; expected google, bing, or ddg"
        ))),
    }
}

/// Get global settings from config file (for Settings UI)
#[tauri::command]
pub async fn get_global_settings(
    state: State<'_, OmigaAppState>,
) -> CommandResult<GlobalSettingsResponse> {
    let config_file = get_config_file(&state).await.unwrap_or_default();
    let settings = config_file.settings.clone().unwrap_or_default();
    Ok(GlobalSettingsResponse {
        timeout: settings.timeout,
        max_tokens: settings.max_tokens,
        temperature: settings.temperature,
        enable_tools: settings.enable_tools,
        web_use_proxy: settings.web_use_proxy,
        web_search_engine: settings.web_search_engine,
        web_search_methods: settings.web_search_methods,
        goal_second_opinion_provider_entry: settings.goal_second_opinion_provider_entry,
    })
}

/// Global settings response for frontend
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GlobalSettingsResponse {
    pub timeout: Option<u64>,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub enable_tools: Option<bool>,
    pub web_use_proxy: Option<bool>,
    pub web_search_engine: Option<String>,
    pub web_search_methods: Option<Vec<String>>,
    pub goal_second_opinion_provider_entry: Option<String>,
}

/// Get current LLM configuration
#[tauri::command]
pub async fn get_llm_config_state(
    state: State<'_, OmigaAppState>,
) -> CommandResult<Option<LlmConfigResponse>> {
    let config_guard = state.chat.llm_config.lock().await;
    Ok(config_guard.as_ref().map(|config| LlmConfigResponse {
        provider: format!("{}", config.provider),
        api_key_preview: if config.api_key.len() > 8 {
            format!("{}...", &config.api_key[..8])
        } else {
            config.api_key.clone()
        },
        model: Some(config.model.clone()),
        base_url: config.base_url.clone(),
        context_window_tokens: config.context_window_tokens,
        thinking: config.thinking,
    }))
}

/// LLM configuration response for frontend
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmConfigResponse {
    pub provider: String,
    pub api_key_preview: String,
    pub model: Option<String>,
    pub base_url: Option<String>,
    pub context_window_tokens: Option<u32>,
    pub thinking: Option<bool>,
}

/// Tavily API key status for Settings UI (never returns full secret).
#[derive(Debug, Serialize)]
pub struct TavilySearchKeyState {
    pub configured: bool,
    pub preview: String,
}

const DEFAULT_PUBMED_EMAIL: &str = "omiga@example.invalid";
const DEFAULT_PUBMED_TOOL_NAME: &str = "omiga";

fn normalize_web_search_key_field(s: &str) -> Option<String> {
    let t = s.trim();
    if t.is_empty() {
        None
    } else {
        Some(t.to_string())
    }
}

fn normalize_optional_search_key_field(s: Option<String>) -> Option<String> {
    s.and_then(|value| normalize_web_search_key_field(&value))
}

fn normalize_query_setting_list(
    values: Option<Vec<String>>,
    allowed: &[&str],
) -> Option<Vec<String>> {
    values.map(|values| {
        let mut out = Vec::new();
        for value in values {
            let normalized = value.trim().to_ascii_lowercase().replace(['-', ' '], "_");
            if allowed.contains(&normalized.as_str()) && !out.iter().any(|item| item == &normalized)
            {
                out.push(normalized);
            }
        }
        out
    })
}

#[tauri::command(rename_all = "camelCase")]
#[allow(clippy::too_many_arguments)]
pub async fn set_web_search_api_keys(
    state: State<'_, OmigaAppState>,
    tavily: String,
    exa: String,
    parallel: String,
    firecrawl: String,
    firecrawl_url: String,
    semantic_scholar_enabled: Option<bool>,
    semantic_scholar_api_key: Option<String>,
    wechat_search_enabled: Option<bool>,
    pubmed_api_key: Option<String>,
    pubmed_email: Option<String>,
    pubmed_tool_name: Option<String>,
    query_dataset_types: Option<Vec<String>>,
    query_dataset_sources: Option<Vec<String>>,
    query_knowledge_sources: Option<Vec<String>>,
    enabled_sources_by_category: Option<HashMap<String, Vec<String>>>,
    enabled_subcategories_by_category: Option<HashMap<String, Vec<String>>>,
) -> CommandResult<()> {
    let mut g = state.chat.web_search_api_keys.lock().await;
    g.tavily = normalize_web_search_key_field(&tavily);
    g.exa = normalize_web_search_key_field(&exa);
    g.parallel = normalize_web_search_key_field(&parallel);
    g.firecrawl = normalize_web_search_key_field(&firecrawl);
    g.firecrawl_url = normalize_web_search_key_field(&firecrawl_url);
    g.semantic_scholar_enabled = semantic_scholar_enabled.unwrap_or(false);
    g.semantic_scholar_api_key = normalize_optional_search_key_field(semantic_scholar_api_key);
    g.wechat_search_enabled = wechat_search_enabled.unwrap_or(false);
    g.pubmed_api_key = normalize_optional_search_key_field(pubmed_api_key);
    g.pubmed_email = normalize_optional_search_key_field(pubmed_email)
        .or_else(|| Some(DEFAULT_PUBMED_EMAIL.to_string()));
    g.pubmed_tool_name = normalize_optional_search_key_field(pubmed_tool_name)
        .or_else(|| Some(DEFAULT_PUBMED_TOOL_NAME.to_string()));
    if let Some(values) = normalize_query_setting_list(
        query_dataset_types,
        crate::domain::tools::QUERY_DATASET_TYPE_IDS,
    ) {
        g.query_dataset_types = Some(values);
    }
    if let Some(values) = normalize_query_setting_list(
        query_dataset_sources,
        crate::domain::tools::QUERY_DATASET_SOURCE_IDS,
    ) {
        g.query_dataset_sources = Some(values);
    }
    if let Some(values) = normalize_query_setting_list(
        query_knowledge_sources,
        crate::domain::tools::QUERY_KNOWLEDGE_SOURCE_IDS,
    ) {
        g.query_knowledge_sources = Some(values);
    }
    if let Some(values) = enabled_sources_by_category {
        g.enabled_sources_by_category =
            Some(crate::domain::retrieval_registry::normalize_enabled_map(
                values,
                crate::domain::retrieval_registry::RegistryEntryKind::Source,
            ));
    }
    if let Some(values) = enabled_subcategories_by_category {
        g.enabled_subcategories_by_category =
            Some(crate::domain::retrieval_registry::normalize_enabled_map(
                values,
                crate::domain::retrieval_registry::RegistryEntryKind::Subcategory,
            ));
    }
    Ok(())
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebSearchKeyFieldState {
    pub configured: bool,
    pub preview: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebSearchApiKeysState {
    pub tavily: WebSearchKeyFieldState,
    pub exa: WebSearchKeyFieldState,
    pub parallel: WebSearchKeyFieldState,
    pub firecrawl: WebSearchKeyFieldState,
    pub firecrawl_url: WebSearchKeyFieldState,
    pub semantic_scholar_enabled: bool,
    pub semantic_scholar_api_key: WebSearchKeyFieldState,
    pub wechat_search_enabled: bool,
    pub pubmed_api_key: WebSearchKeyFieldState,
    pub pubmed_email: WebSearchKeyFieldState,
    pub pubmed_tool_name: WebSearchKeyFieldState,
    pub query_dataset_types: Vec<String>,
    pub query_dataset_sources: Vec<String>,
    pub query_knowledge_sources: Vec<String>,
    pub enabled_sources_by_category: HashMap<String, Vec<String>>,
    pub enabled_subcategories_by_category: HashMap<String, Vec<String>>,
}

fn web_search_key_field_state(key: &Option<String>) -> WebSearchKeyFieldState {
    match key {
        None => WebSearchKeyFieldState {
            configured: false,
            preview: String::new(),
        },
        Some(k) if k.trim().is_empty() => WebSearchKeyFieldState {
            configured: false,
            preview: String::new(),
        },
        Some(key) => WebSearchKeyFieldState {
            configured: true,
            preview: if key.len() > 8 {
                format!("{}...", &key[..8])
            } else {
                key.clone()
            },
        },
    }
}

#[tauri::command]
pub async fn get_web_search_api_keys_state(
    state: State<'_, OmigaAppState>,
) -> CommandResult<WebSearchApiKeysState> {
    let g = state.chat.web_search_api_keys.lock().await;
    Ok(WebSearchApiKeysState {
        tavily: web_search_key_field_state(&g.tavily),
        exa: web_search_key_field_state(&g.exa),
        parallel: web_search_key_field_state(&g.parallel),
        firecrawl: web_search_key_field_state(&g.firecrawl),
        firecrawl_url: web_search_key_field_state(&g.firecrawl_url),
        semantic_scholar_enabled: g.semantic_scholar_enabled,
        semantic_scholar_api_key: web_search_key_field_state(&g.semantic_scholar_api_key),
        wechat_search_enabled: g.wechat_search_enabled,
        pubmed_api_key: web_search_key_field_state(&g.pubmed_api_key),
        pubmed_email: web_search_key_field_state(
            &g.pubmed_email
                .clone()
                .or_else(|| Some(DEFAULT_PUBMED_EMAIL.to_string())),
        ),
        pubmed_tool_name: web_search_key_field_state(
            &g.pubmed_tool_name
                .clone()
                .or_else(|| Some(DEFAULT_PUBMED_TOOL_NAME.to_string())),
        ),
        query_dataset_types: g.enabled_query_dataset_types(),
        query_dataset_sources: g.enabled_query_dataset_sources(),
        query_knowledge_sources: g.enabled_query_knowledge_sources(),
        enabled_sources_by_category: g.enabled_sources_by_category(),
        enabled_subcategories_by_category: g.enabled_subcategories_by_category(),
    })
}

#[tauri::command]
pub fn get_retrieval_source_registry() -> crate::domain::retrieval_registry::RetrievalSourceRegistry
{
    crate::domain::retrieval_registry::registry()
}

/// Store Tavily API key for built-in search (empty clears user override; env still works).
#[tauri::command]
pub async fn set_tavily_search_api_key(
    state: State<'_, OmigaAppState>,
    api_key: String,
) -> CommandResult<()> {
    let t = api_key.trim();
    let mut g = state.chat.web_search_api_keys.lock().await;
    if t.is_empty() {
        g.tavily = None;
    } else {
        g.tavily = Some(t.to_string());
    }
    Ok(())
}

#[tauri::command]
pub async fn get_tavily_search_api_key_state(
    state: State<'_, OmigaAppState>,
) -> CommandResult<TavilySearchKeyState> {
    let g = state.chat.web_search_api_keys.lock().await;
    let Some(ref key) = g.tavily else {
        return Ok(TavilySearchKeyState {
            configured: false,
            preview: String::new(),
        });
    };
    let preview = if key.len() > 8 {
        format!("{}...", &key[..8])
    } else {
        key.clone()
    };
    Ok(TavilySearchKeyState {
        configured: true,
        preview,
    })
}

/// Legacy: Set API key (deprecated, use set_llm_config)
#[tauri::command]
pub async fn set_api_key(state: State<'_, OmigaAppState>, api_key: String) -> CommandResult<()> {
    let mut config_guard = state.chat.llm_config.lock().await;
    let mut config = config_guard.clone().unwrap_or_default();
    config.api_key = api_key;
    *config_guard = Some(config);
    Ok(())
}

/// Get API key status - checks if API key is configured via environment or state
#[tauri::command]
pub async fn get_api_key_status(state: State<'_, OmigaAppState>) -> CommandResult<ApiKeyStatus> {
    // First check if we have a stored config with API key
    let stored = state.chat.llm_config.lock().await;
    if let Some(config) = stored.as_ref() {
        if !config.api_key.is_empty() {
            return Ok(ApiKeyStatus {
                configured: true,
                source: Some("state".to_string()),
                provider: Some(format!("{:?}", config.provider)),
                message: None,
            });
        }
    }
    drop(stored);

    // Try to load from environment
    match load_config_from_env() {
        Ok(config) => {
            // Store for future use
            let mut stored = state.chat.llm_config.lock().await;
            *stored = Some(config.clone());
            drop(stored);
            *state.chat.active_provider_entry_name.lock().await = None;
            Ok(ApiKeyStatus {
                configured: true,
                source: Some("environment".to_string()),
                provider: Some(format!("{:?}", config.provider)),
                message: None,
            })
        }
        Err(_e) => Ok(ApiKeyStatus {
            configured: false,
            source: None,
            provider: None,
            message: Some(
                "未配置 API key。请设置环境变量: ANTHROPIC_API_KEY, OPENAI_API_KEY, 或 LLM_API_KEY"
                    .to_string(),
            ),
        }),
    }
}

/// API key status response
#[derive(Debug, Serialize)]
pub struct ApiKeyStatus {
    pub configured: bool,
    pub source: Option<String>,
    pub provider: Option<String>,
    pub message: Option<String>,
}

/// 初始 Todo 项（用于 Plan mode）
#[derive(Debug, Clone, Serialize)]
pub struct InitialTodoItem {
    pub id: String,
    pub content: String,
    pub status: String, // "pending" | "in_progress" | "completed"
}

/// Response from send_message
#[derive(Debug, Serialize)]
pub struct MessageResponse {
    pub message_id: String,
    pub session_id: String,
    pub round_id: String,
    /// Persisted SQLite row id for the user message for this turn (for client-side retry anchoring).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_message_id: Option<String>,
    /// `leader` (default) | `background_followup_queued` when text was routed to a bg task queue.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_kind: Option<String>,
    /// 调度系统生成的任务执行计划（如果有）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scheduler_plan: Option<crate::domain::agents::scheduler::SchedulingResult>,
    /// Plan mode 下的初始 todo 列表
    #[serde(skip_serializing_if = "Option::is_none")]
    pub initial_todos: Option<Vec<InitialTodoItem>>,
}

/// Test if the LLM model is available and responding
#[tauri::command]
pub async fn test_model(state: State<'_, OmigaAppState>) -> CommandResult<ModelTestResult> {
    let config_guard = state.chat.llm_config.lock().await;

    let config = match config_guard.as_ref() {
        Some(c) if !c.api_key.is_empty() => c.clone(),
        _ => {
            return Ok(ModelTestResult {
                available: false,
                provider: None,
                model: None,
                latency_ms: None,
                error: Some("No API key configured".to_string()),
            });
        }
    };
    drop(config_guard);

    let provider = config.provider;
    let model = config.model.clone();

    match create_client(config) {
        Ok(client) => {
            let start = std::time::Instant::now();
            match client.health_check().await {
                Ok(true) => {
                    let latency_ms = start.elapsed().as_millis() as u64;
                    Ok(ModelTestResult {
                        available: true,
                        provider: Some(format!("{:?}", provider)),
                        model: Some(model),
                        latency_ms: Some(latency_ms),
                        error: None,
                    })
                }
                Ok(false) => Ok(ModelTestResult {
                    available: false,
                    provider: Some(format!("{:?}", provider)),
                    model: Some(model),
                    latency_ms: None,
                    error: Some("Health check returned false".to_string()),
                }),
                Err(e) => Ok(ModelTestResult {
                    available: false,
                    provider: Some(format!("{:?}", provider)),
                    model: Some(model),
                    latency_ms: None,
                    error: Some(match e {
                        ApiError::Http { message, .. } => message.clone(),
                        ApiError::Network { message } => message.clone(),
                        ApiError::Authentication => "Authentication failed".to_string(),
                        ApiError::Timeout => "Request timeout".to_string(),
                        ApiError::RateLimited { retry_after } => {
                            format!("Rate limited (retry after {}s)", retry_after)
                        }
                        ApiError::Server { message } => message.clone(),
                        ApiError::SseParse { message } => message.clone(),
                        ApiError::Config { message } => message.clone(),
                    }),
                }),
            }
        }
        Err(e) => Ok(ModelTestResult {
            available: false,
            provider: Some(format!("{:?}", provider)),
            model: Some(model),
            latency_ms: None,
            error: Some(e.to_string()),
        }),
    }
}

/// Generate a short sidebar title from the first user message (one cheap LLM call; falls back to heuristic truncation).
#[tauri::command]
pub async fn suggest_session_title(
    app_state: State<'_, OmigaAppState>,
    user_message: String,
) -> CommandResult<String> {
    let fallback = crate::domain::chat_session_title::fallback_title_from_message(&user_message);
    if std::env::var("OMIGA_DISABLE_SESSION_TITLE_LLM")
        .ok()
        .as_deref()
        == Some("1")
    {
        return Ok(fallback);
    }
    let config = match get_llm_config(&app_state.chat).await {
        Ok(c) => c,
        Err(e) => {
            tracing::debug!(target: "omiga::session_title", "no llm config: {}", e);
            return Ok(fallback);
        }
    };
    let client = match create_client(config) {
        Ok(c) => c,
        Err(e) => {
            tracing::debug!(target: "omiga::session_title", "create_client: {}", e);
            return Ok(fallback);
        }
    };
    match crate::domain::chat_session_title::suggest_session_title_llm(
        client.as_ref(),
        &user_message,
    )
    .await
    {
        Ok(t) if !t.trim().is_empty() => Ok(t),
        Ok(_) => Ok(fallback),
        Err(e) => {
            tracing::warn!(target: "omiga::session_title", "llm title failed: {}", e);
            Ok(fallback)
        }
    }
}

/// Event payload emitted to the frontend when a background title task finishes.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionTitleUpdatedPayload {
    pub session_id: String,
    pub title: String,
}

/// Spawn an independent title-generation task and return immediately.
///
/// The spawned task waits briefly so the main `send_message` agent gets ahead in the
/// request queue, then makes a cheap single-turn LLM call.  On success it:
/// 1. Persists the new name via `SessionRepository::rename_session`.
/// 2. Emits `session-title-updated` so the frontend can refresh the sidebar without
///    making a second `rename_session` round-trip.
#[tauri::command]
pub async fn spawn_session_title_async(
    app: AppHandle,
    app_state: State<'_, OmigaAppState>,
    session_id: String,
    user_message: String,
) -> CommandResult<()> {
    // Snapshot the LLM config now (cheap clone) so the spawned task owns it.
    let config_snapshot = app_state.chat.llm_config.lock().await.clone();
    let repo = Arc::clone(&app_state.repo);

    tokio::spawn(async move {
        // Let send_message acquire its resources before we compete for the LLM endpoint.
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;

        let config = match config_snapshot {
            Some(c) if !c.api_key.is_empty() => c,
            _ => match crate::llm::load_config() {
                Ok(c) => c,
                Err(e) => {
                    tracing::debug!(target: "omiga::session_title", "spawn: no llm config: {e}");
                    return;
                }
            },
        };

        let client = match create_client(config) {
            Ok(c) => c,
            Err(e) => {
                tracing::debug!(target: "omiga::session_title", "spawn: create_client: {e}");
                return;
            }
        };

        let title = match crate::domain::chat_session_title::suggest_session_title_llm(
            client.as_ref(),
            &user_message,
        )
        .await
        {
            Ok(t) if !t.trim().is_empty() => t,
            Ok(_) => return,
            Err(e) => {
                tracing::warn!(target: "omiga::session_title", "spawn: llm failed: {e}");
                return;
            }
        };

        if let Err(e) = repo.rename_session(&session_id, &title).await {
            tracing::warn!(target: "omiga::session_title", "spawn: rename_session: {e}");
            return;
        }

        let _ = app.emit(
            "session-title-updated",
            SessionTitleUpdatedPayload { session_id, title },
        );
    });

    Ok(())
}

/// Result of model test
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelTestResult {
    pub available: bool,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub latency_ms: Option<u64>,
    pub error: Option<String>,
}

/// Request to send a message
#[derive(Debug, Deserialize)]
pub struct SendMessageRequest {
    pub content: String,
    /// Optional routing-only content used for skill detection / scheduler forcing while preserving
    /// the original user-visible transcript content.
    #[serde(default, rename = "routingContent")]
    pub routing_content: Option<String>,
    pub session_id: Option<String>,
    /// Explicit project path (required for new sessions)
    pub project_path: Option<String>,
    /// Optional session name (defaults to a one-line title from the first user message, same as the UI)
    pub session_name: Option<String>,
    #[serde(default)]
    pub use_tools: bool,
    /// Optional routing: omit / `leader` = main session; `bg:<task_id>` = queue follow-up for that background Agent task.
    #[serde(default, rename = "inputTarget")]
    pub input_target: Option<String>,
    /// Specialist agent id from [`list_available_agents`] (e.g. Explore, Plan). Omit or `general-purpose` for default.
    #[serde(default, rename = "composerAgentType")]
    pub composer_agent_type: Option<String>,
    /// Explicit Omiga plugin IDs selected for this turn by the composer `#` picker.
    #[serde(default, rename = "selectedPluginIds")]
    pub selected_plugin_ids: Vec<String>,
    /// `ask` | `auto` | `bypass` — user-facing permission stance for this turn.
    #[serde(default, rename = "permissionMode")]
    pub permission_mode: Option<String>,
    /// `off` | `task` | `session` — explicit user gate for Computer Use facade tools.
    #[serde(default, rename = "computerUseMode")]
    pub computer_use_mode: Option<String>,
    /// `local` | `ssh` | `sandbox` — chat composer execution surface (tools / terminal).
    #[serde(default, rename = "executionEnvironment")]
    pub execution_environment: Option<String>,
    /// Selected SSH server name; used when `execution_environment == "ssh"`.
    #[serde(default, rename = "sshServer")]
    pub ssh_server: Option<String>,
    /// `modal` | `daytona` | `docker` | `singularity` — composer sandbox backend; used when `execution_environment == "sandbox"`.
    #[serde(default, rename = "sandboxBackend")]
    pub sandbox_backend: Option<String>,
    /// `"none"` | `"conda"` | `"venv"` | `"pyenv"` — local virtual env type.
    #[serde(default, rename = "localVenvType")]
    pub local_venv_type: Option<String>,
    /// Conda env name, venv directory path, or pyenv version string.
    #[serde(default, rename = "localVenvName")]
    pub local_venv_name: Option<String>,
    /// When set, truncate SQLite transcript after this user row and reuse it instead of inserting a new user message.
    #[serde(default, rename = "retryFromUserMessageId")]
    pub retry_from_user_message_id: Option<String>,
    /// Explicit workflow slash command from the composer (`plan` | `schedule` | `team` | `autopilot`).
    #[serde(default, rename = "workflowCommand")]
    pub workflow_command: Option<String>,
    /// Session's stored provider entry name (from load_session → active_provider_entry_name).
    /// Used for lazy LLM config restoration: ensures the correct provider is active for this
    /// session without blocking the session-switch path.
    #[serde(default, rename = "activeProviderEntryName")]
    pub active_provider_entry_name: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct OrchestrationEventDto {
    pub id: String,
    pub session_id: String,
    pub round_id: Option<String>,
    pub message_id: Option<String>,
    pub mode: Option<String>,
    pub event_type: String,
    pub phase: Option<String>,
    pub task_id: Option<String>,
    pub payload: Value,
    pub created_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MockOrchestrationScenarioRequest {
    pub session_id: String,
    pub project_root: String,
    pub scenario: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MockOrchestrationScenarioResult {
    pub session_id: String,
    pub scenario: String,
    pub background_task_count: usize,
    pub event_count: usize,
}

#[tauri::command]
pub async fn list_orchestration_events(
    app_state: State<'_, OmigaAppState>,
    session_id: String,
    limit: Option<i64>,
) -> CommandResult<Vec<OrchestrationEventDto>> {
    let rows = app_state
        .repo
        .list_orchestration_events_for_session(&session_id, limit.unwrap_or(50).clamp(1, 200))
        .await
        .map_err(|e| OmigaError::Persistence(e.to_string()))?;

    Ok(rows
        .into_iter()
        .map(|row| OrchestrationEventDto {
            id: row.id,
            session_id: row.session_id,
            round_id: row.round_id,
            message_id: row.message_id,
            mode: row.mode,
            event_type: row.event_type,
            phase: row.phase,
            task_id: row.task_id,
            payload: serde_json::from_str(&row.payload_json).unwrap_or(Value::Null),
            created_at: row.created_at,
        })
        .collect())
}

async fn ensure_session_exists(
    repo: &crate::domain::persistence::SessionRepository,
    session_id: &str,
    project_root: &str,
    name: &str,
) -> Result<(), OmigaError> {
    let existing = repo
        .get_session(session_id)
        .await
        .map_err(|e| OmigaError::Persistence(e.to_string()))?;
    if existing.is_some() {
        return Ok(());
    }
    repo.create_session(session_id, name, project_root)
        .await
        .map_err(|e| OmigaError::Persistence(e.to_string()))
}

async fn append_mock_bg_task(
    repo: &crate::domain::persistence::SessionRepository,
    task: &BackgroundAgentTask,
    transcript: &[Message],
) -> Result<(), OmigaError> {
    repo.upsert_background_agent_task(task)
        .await
        .map_err(|e| OmigaError::Persistence(e.to_string()))?;
    if !transcript.is_empty() {
        repo.append_background_agent_messages_batch(&task.task_id, &task.session_id, transcript)
            .await
            .map_err(|e| OmigaError::Persistence(e.to_string()))?;
    }
    Ok(())
}

async fn append_mock_event(
    repo: &crate::domain::persistence::SessionRepository,
    session_id: &str,
    mode: Option<&str>,
    event_type: &str,
    phase: Option<&str>,
    task_id: Option<&str>,
    payload: Value,
) -> Result<(), OmigaError> {
    let payload_json = serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_string());
    repo.append_orchestration_event(NewOrchestrationEventRecord {
        session_id,
        round_id: Some("mock-round"),
        message_id: None,
        mode,
        event_type,
        phase,
        task_id,
        payload_json: &payload_json,
    })
    .await
    .map_err(|e| OmigaError::Persistence(e.to_string()))
}

pub async fn seed_mock_orchestration_scenario(
    repo: &crate::domain::persistence::SessionRepository,
    project_root: &Path,
    session_id: &str,
    scenario: &str,
) -> Result<MockOrchestrationScenarioResult, OmigaError> {
    ensure_session_exists(
        repo,
        session_id,
        &project_root.to_string_lossy(),
        &format!("Mock {scenario}"),
    )
    .await?;

    let now = chrono::Utc::now();
    let base_ts = now.timestamp() as u64;
    let mut event_count = 0usize;
    let mut task_count = 0usize;

    match scenario {
        "schedule" => {
            append_mock_event(
                repo,
                session_id,
                Some("schedule"),
                "schedule_plan_created",
                None,
                None,
                serde_json::json!({ "planId": "mock-plan-a", "taskCount": 3 }),
            )
            .await?;
            event_count += 1;

            let exec_task = BackgroundAgentTask {
                task_id: "mock-schedule-exec".to_string(),
                agent_type: "executor".to_string(),
                description: "实现登录流程重构".to_string(),
                status: BackgroundAgentStatus::Completed,
                created_at: base_ts.saturating_sub(90),
                started_at: Some(base_ts.saturating_sub(88)),
                completed_at: Some(base_ts.saturating_sub(50)),
                result_summary: Some("已完成实现".to_string()),
                error_message: None,
                output_path: None,
                session_id: session_id.to_string(),
                message_id: "mock-msg".to_string(),
                round_id: Some("mock-round".to_string()),
                plan_id: Some("mock-plan-a".to_string()),
            };
            append_mock_bg_task(
                repo,
                &exec_task,
                &[Message::User {
                    content: "实现登录流程重构".to_string(),
                }],
            )
            .await?;
            task_count += 1;
            append_mock_event(
                repo,
                session_id,
                Some("schedule"),
                "worker_completed",
                Some("executing"),
                Some(&exec_task.task_id),
                serde_json::json!({
                    "agentType": exec_task.agent_type,
                    "description": exec_task.description,
                }),
            )
            .await?;
            event_count += 1;

            let reviewer_task = BackgroundAgentTask {
                task_id: "mock-schedule-review".to_string(),
                agent_type: "verification".to_string(),
                description: "验证登录流程重构".to_string(),
                status: BackgroundAgentStatus::Completed,
                created_at: base_ts.saturating_sub(45),
                started_at: Some(base_ts.saturating_sub(44)),
                completed_at: Some(base_ts.saturating_sub(20)),
                result_summary: Some("VERDICT: PASS\n验证通过，无阻断问题".to_string()),
                error_message: None,
                output_path: None,
                session_id: session_id.to_string(),
                message_id: "mock-msg".to_string(),
                round_id: Some("mock-round".to_string()),
                plan_id: Some("mock-plan-a".to_string()),
            };
            append_mock_bg_task(
                repo,
                &reviewer_task,
                &[Message::Assistant {
                    content: "VERDICT: PASS\n验证通过，无阻断问题".to_string(),
                    tool_calls: None,
                    token_usage: None,
                    reasoning_content: None,
                    follow_up_suggestions: None,
                    turn_summary: None,
                }],
            )
            .await?;
            task_count += 1;
            append_mock_event(
                repo,
                session_id,
                Some("schedule"),
                "reviewer_verdict",
                Some("verifying"),
                Some(&reviewer_task.task_id),
                serde_json::json!({
                    "agentType": reviewer_task.agent_type,
                    "verdict": "pass",
                    "summary": "验证通过，无阻断问题",
                }),
            )
            .await?;
            event_count += 1;
        }
        "team" => {
            let mock_goal = "修复导出流程并完成验证";

            for (event_type, phase) in [
                ("mode_requested", Some("planning")),
                ("phase_changed", Some("executing")),
                ("verification_started", Some("verifying")),
                ("fix_started", Some("fixing")),
                ("verification_started", Some("verifying")),
                ("synthesizing_started", Some("synthesizing")),
            ] {
                append_mock_event(
                    repo,
                    session_id,
                    Some("team"),
                    event_type,
                    phase,
                    None,
                    serde_json::json!({ "goal": mock_goal }),
                )
                .await?;
                event_count += 1;
            }

            let worker_task = BackgroundAgentTask {
                task_id: "mock-team-worker".to_string(),
                agent_type: "executor".to_string(),
                description: "修复导出并发问题".to_string(),
                status: BackgroundAgentStatus::Completed,
                created_at: base_ts.saturating_sub(120),
                started_at: Some(base_ts.saturating_sub(118)),
                completed_at: Some(base_ts.saturating_sub(80)),
                result_summary: Some("已修复导出并发问题".to_string()),
                error_message: None,
                output_path: None,
                session_id: session_id.to_string(),
                message_id: "mock-msg".to_string(),
                round_id: Some("mock-round".to_string()),
                plan_id: Some("mock-team-plan".to_string()),
            };
            append_mock_bg_task(repo, &worker_task, &[]).await?;
            task_count += 1;
            let reviewer_task = BackgroundAgentTask {
                task_id: "mock-team-review".to_string(),
                agent_type: "verification".to_string(),
                description: "验证修复结果".to_string(),
                status: BackgroundAgentStatus::Completed,
                created_at: base_ts.saturating_sub(70),
                started_at: Some(base_ts.saturating_sub(69)),
                completed_at: Some(base_ts.saturating_sub(40)),
                result_summary: Some("VERDICT: PARTIAL\n需要补充导出回归测试".to_string()),
                error_message: None,
                output_path: None,
                session_id: session_id.to_string(),
                message_id: "mock-msg".to_string(),
                round_id: Some("mock-round".to_string()),
                plan_id: Some("mock-team-plan".to_string()),
            };
            append_mock_bg_task(repo, &reviewer_task, &[]).await?;
            task_count += 1;
        }
        "autopilot" => {
            let mock_goal = "实现设置同步并完成验收";
            let mock_qa_cycles = 2;

            for (event_type, phase) in [
                ("mode_requested", Some("intake")),
                ("phase_changed", Some("design")),
                ("phase_changed", Some("plan")),
                ("phase_changed", Some("implementation")),
                ("phase_changed", Some("qa")),
                ("phase_changed", Some("validation")),
            ] {
                append_mock_event(
                    repo,
                    session_id,
                    Some("autopilot"),
                    event_type,
                    phase,
                    None,
                    serde_json::json!({ "goal": mock_goal, "qaCycles": mock_qa_cycles }),
                )
                .await?;
                event_count += 1;
            }

            let review_task = BackgroundAgentTask {
                task_id: "mock-auto-review".to_string(),
                agent_type: "verification".to_string(),
                description: "验证设置同步实现".to_string(),
                status: BackgroundAgentStatus::Completed,
                created_at: base_ts.saturating_sub(55),
                started_at: Some(base_ts.saturating_sub(54)),
                completed_at: Some(base_ts.saturating_sub(30)),
                result_summary: Some("VERDICT: PASS\n设置同步通过 QA 与 validation".to_string()),
                error_message: None,
                output_path: None,
                session_id: session_id.to_string(),
                message_id: "mock-msg".to_string(),
                round_id: Some("mock-round".to_string()),
                plan_id: Some("mock-auto-plan".to_string()),
            };
            append_mock_bg_task(repo, &review_task, &[]).await?;
            task_count += 1;
            append_mock_event(
                repo,
                session_id,
                Some("autopilot"),
                "reviewer_verdict",
                Some("validation"),
                Some(&review_task.task_id),
                serde_json::json!({
                    "agentType": review_task.agent_type,
                    "verdict": "pass",
                    "summary": "设置同步通过 QA 与 validation",
                }),
            )
            .await?;
            event_count += 1;
        }
        other => {
            return Err(OmigaError::Chat(ChatError::StreamError(format!(
                "Unknown mock orchestration scenario: {other}"
            ))));
        }
    }

    Ok(MockOrchestrationScenarioResult {
        session_id: session_id.to_string(),
        scenario: scenario.to_string(),
        background_task_count: task_count,
        event_count,
    })
}

#[tauri::command]
pub async fn run_mock_orchestration_scenario(
    app_state: State<'_, OmigaAppState>,
    app: AppHandle,
    request: MockOrchestrationScenarioRequest,
) -> CommandResult<MockOrchestrationScenarioResult> {
    let result = seed_mock_orchestration_scenario(
        &app_state.repo,
        Path::new(&request.project_root),
        &request.session_id,
        &request.scenario,
    )
    .await?;
    let _ = app.emit(
        "mock-orchestration-scenario-loaded",
        serde_json::json!({
            "sessionId": result.session_id,
            "scenario": result.scenario,
        }),
    );
    Ok(result)
}

/// Background Agent tasks for one session (Claude Code–style teammate follow-ups).
/// Merges SQLite rows with in-memory manager so live tasks overlay DB after restart.
#[tauri::command]
pub async fn list_session_background_tasks(
    app_state: State<'_, OmigaAppState>,
    session_id: String,
) -> CommandResult<Vec<crate::domain::agents::background::BackgroundAgentTask>> {
    let mgr = crate::domain::agents::background::get_background_agent_manager();
    let from_mem = mgr.get_session_tasks(&session_id).await;

    let mut from_db = {
        let repo = &*app_state.repo;
        repo.list_background_agent_tasks_for_session(&session_id)
            .await
            .map_err(|e| OmigaError::Persistence(e.to_string()))?
    };

    for t in from_mem {
        if let Some(existing) = from_db.iter_mut().find(|x| x.task_id == t.task_id) {
            *existing = t;
        } else {
            from_db.push(t);
        }
    }
    from_db.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(from_db)
}

/// Load persisted sidechain transcript for one background Agent task (teammate view).
#[tauri::command]
pub async fn load_background_agent_transcript(
    app_state: State<'_, OmigaAppState>,
    session_id: String,
    task_id: String,
) -> CommandResult<Vec<Message>> {
    let repo = &*app_state.repo;
    let task = repo
        .get_background_agent_task_by_id(&task_id)
        .await
        .map_err(|e| OmigaError::Persistence(e.to_string()))?;
    let Some(task) = task else {
        return Err(OmigaError::NotFound {
            resource: format!("background task {}", task_id),
        });
    };
    if task.session_id != session_id {
        return Err(OmigaError::Chat(ChatError::StreamError(
            "background task does not belong to this session".to_string(),
        )));
    }
    repo.list_background_agent_messages_for_task(&task_id)
        .await
        .map_err(|e| OmigaError::Persistence(e.to_string()))
}

/// Cancel a background Agent task: cancellation token + memory state, write-through to SQLite,
/// and DB-only reconcile when the worker is gone (e.g. after app restart) but the row is still pending/running.
#[tauri::command]
pub async fn cancel_background_agent_task(
    app: tauri::AppHandle,
    app_state: State<'_, OmigaAppState>,
    session_id: String,
    task_id: String,
) -> CommandResult<crate::domain::agents::background::BackgroundAgentTask> {
    use crate::domain::agents::background::{
        emit_background_agent_complete, emit_background_agent_update, get_background_agent_manager,
        BackgroundAgentStatus,
    };

    let mgr = get_background_agent_manager();

    if let Some(existing) = mgr.get_task(&task_id).await {
        if existing.session_id != session_id {
            return Err(OmigaError::Chat(ChatError::StreamError(
                "background task does not belong to this session".to_string(),
            )));
        }
    }

    if let Some(task) = mgr.cancel_task(&task_id).await {
        persist_background_agent_task_snapshot(&app_state.repo, &task).await;
        if let Err(e) = emit_background_agent_update(&app, &task) {
            tracing::warn!(target: "omiga::bg_agent", "emit background-agent-update failed: {}", e);
        }
        if let Err(e) = emit_background_agent_complete(&app, &task) {
            tracing::warn!(target: "omiga::bg_agent", "emit background-agent-complete failed: {}", e);
        }
        return Ok(task);
    }

    let repo = &*app_state.repo;
    let row = repo
        .get_background_agent_task_by_id(&task_id)
        .await
        .map_err(|e| OmigaError::Persistence(e.to_string()))?;

    let Some(mut task) = row else {
        return Err(OmigaError::NotFound {
            resource: format!("background task {}", task_id),
        });
    };

    if task.session_id != session_id {
        return Err(OmigaError::Chat(ChatError::StreamError(
            "background task does not belong to this session".to_string(),
        )));
    }

    match task.status {
        BackgroundAgentStatus::Completed
        | BackgroundAgentStatus::Failed
        | BackgroundAgentStatus::Cancelled => Ok(task),
        BackgroundAgentStatus::Pending | BackgroundAgentStatus::Running => {
            task.status = BackgroundAgentStatus::Cancelled;
            task.completed_at = Some(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
            );
            repo.upsert_background_agent_task(&task)
                .await
                .map_err(|e| OmigaError::Persistence(e.to_string()))?;
            if let Err(e) = emit_background_agent_update(&app, &task) {
                tracing::warn!(target: "omiga::bg_agent", "emit background-agent-update failed: {}", e);
            }
            if let Err(e) = emit_background_agent_complete(&app, &task) {
                tracing::warn!(target: "omiga::bg_agent", "emit background-agent-complete failed: {}", e);
            }
            Ok(task)
        }
    }
}

/// Provider configuration entry for multi-provider management
#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ProviderConfigEntry {
    pub name: String,
    pub provider_type: String,
    pub model: String,
    pub api_key_preview: String,
    pub base_url: Option<String>,
    /// Model context window capacity, in tokens.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_window_tokens: Option<u32>,
    /// Moonshot / Custom / DeepSeek: request `thinking` object and stream `reasoning_content`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<bool>,
    /// DeepSeek only: "high" or "max", used when thinking is enabled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
    pub enabled: bool,
    /// Matches in-memory runtime config (current chat session / quick switch).
    pub is_session_active: bool,
    /// `default` in `omiga.yaml` — the default used on startup and after Settings save.
    pub is_default: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::persistence::{init_db, SessionRepository};

    #[tokio::test]
    async fn seeds_mock_schedule_scenario() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("mock-schedule.sqlite");
        let pool = init_db(&db_path).await.expect("init db");
        let repo = SessionRepository::new(pool);

        let result =
            seed_mock_orchestration_scenario(&repo, dir.path(), "sess-mock-schedule", "schedule")
                .await
                .expect("seed schedule scenario");

        assert_eq!(result.scenario, "schedule");
        assert!(result.event_count >= 3);
        assert!(result.background_task_count >= 2);

        let events = repo
            .list_orchestration_events_for_session("sess-mock-schedule", 50)
            .await
            .expect("list events");
        assert!(events
            .iter()
            .any(|e| e.event_type == "schedule_plan_created"));
        assert!(events.iter().any(|e| e.event_type == "worker_completed"));
        assert!(events.iter().any(|e| e.event_type == "reviewer_verdict"));
    }

    #[tokio::test]
    async fn seeds_mock_team_scenario() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("mock-scenario.sqlite");
        let pool = init_db(&db_path).await.expect("init db");
        let repo = SessionRepository::new(pool);

        let result = seed_mock_orchestration_scenario(&repo, dir.path(), "sess-mock-team", "team")
            .await
            .expect("seed team scenario");

        assert_eq!(result.scenario, "team");
        assert!(result.event_count >= 5);
        assert!(result.background_task_count >= 2);

        let events = repo
            .list_orchestration_events_for_session("sess-mock-team", 50)
            .await
            .expect("list events");
        assert!(events
            .iter()
            .any(|e| e.event_type == "verification_started"));
        assert!(events.iter().any(|e| e.event_type == "fix_started"));
        assert!(events
            .iter()
            .any(|e| e.event_type == "synthesizing_started"));

        let tasks = repo
            .list_background_agent_tasks_for_session("sess-mock-team")
            .await
            .expect("list tasks");
        assert!(tasks.iter().any(|t| t.agent_type == "executor"));
        assert!(tasks.iter().any(|t| t.agent_type == "verification"));
        assert!(
            crate::domain::team_state::read_state(dir.path(), "sess-mock-team")
                .await
                .is_none(),
            "mock team scenario should not write live team state"
        );
    }

    #[tokio::test]
    async fn seeds_mock_autopilot_scenario() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("mock-autopilot.sqlite");
        let pool = init_db(&db_path).await.expect("init db");
        let repo = SessionRepository::new(pool);

        let result =
            seed_mock_orchestration_scenario(&repo, dir.path(), "sess-mock-autopilot", "autopilot")
                .await
                .expect("seed autopilot scenario");

        assert_eq!(result.scenario, "autopilot");
        assert!(result.event_count >= 6);
        assert!(result.background_task_count >= 1);

        let events = repo
            .list_orchestration_events_for_session("sess-mock-autopilot", 50)
            .await
            .expect("list events");
        assert!(events.iter().any(|e| e.event_type == "mode_requested"));
        assert!(events
            .iter()
            .any(|e| e.event_type == "phase_changed" && e.phase.as_deref() == Some("validation")));
        assert!(events.iter().any(|e| e.event_type == "reviewer_verdict"));
        assert!(
            crate::domain::autopilot_state::read_state(dir.path(), "sess-mock-autopilot")
                .await
                .is_none(),
            "mock autopilot scenario should not write live autopilot state"
        );
    }
}
