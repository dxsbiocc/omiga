//! Multi-provider LLM API abstraction layer
//!
//! Supports:
//! - International: Anthropic (Claude), OpenAI, Azure, Google (Gemini)
//! - Domestic (Chinese): MiniMax, Alibaba (通义千问), DeepSeek, Zhipu (ChatGLM)
//! - Custom: Any OpenAI-compatible endpoint (Ollama, vLLM, etc.)
//!
//! Configuration sources (in order of priority):
//! 1. `omiga.yaml` config file in project root
//! 2. Environment variables (LLM_API_KEY, LLM_PROVIDER, etc.)
//! 3. Default values

use crate::domain::tools::ToolSchema;
use crate::errors::ApiError;
use async_trait::async_trait;
use futures::Stream;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::pin::Pin;

pub mod anthropic;
pub mod openai;
pub mod domestic;
pub mod config;
pub mod types;

pub use anthropic::AnthropicClient;
pub use config::{LlmConfigFile, load_config_file, save_config_file, ProviderConfig};
pub use domestic::{AlibabaClient, ZhipuClient};
pub use openai::OpenAiCompatibleClient;
pub use types::*;

/// Supported LLM providers
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum LlmProvider {
    /// Anthropic Claude API
    Anthropic,
    /// OpenAI API
    OpenAi,
    /// Azure OpenAI
    Azure,
    /// Google Gemini
    Google,
    /// MiniMax
    Minimax,
    /// Alibaba Tongyi Qianwen (通义千问)
    Alibaba,
    /// DeepSeek
    Deepseek,
    /// Zhipu AI ChatGLM (智谱)
    Zhipu,
    /// Moonshot AI (月之暗面)
    Moonshot,
    /// Custom OpenAI-compatible endpoint
    Custom,
}

impl Default for LlmProvider {
    fn default() -> Self {
        LlmProvider::Anthropic
    }
}

impl std::fmt::Display for LlmProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LlmProvider::Anthropic => write!(f, "anthropic"),
            LlmProvider::OpenAi => write!(f, "openai"),
            LlmProvider::Azure => write!(f, "azure"),
            LlmProvider::Google => write!(f, "google"),
            LlmProvider::Minimax => write!(f, "minimax"),
            LlmProvider::Alibaba => write!(f, "alibaba"),
            LlmProvider::Deepseek => write!(f, "deepseek"),
            LlmProvider::Zhipu => write!(f, "zhipu"),
            LlmProvider::Moonshot => write!(f, "moonshot"),
            LlmProvider::Custom => write!(f, "custom"),
        }
    }
}

impl std::str::FromStr for LlmProvider {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "anthropic" | "claude" => Ok(LlmProvider::Anthropic),
            "openai" | "open_ai" => Ok(LlmProvider::OpenAi),
            "azure" => Ok(LlmProvider::Azure),
            "google" | "gemini" => Ok(LlmProvider::Google),
            "minimax" => Ok(LlmProvider::Minimax),
            "alibaba" | "tongyi" | "qianwen" | "通义" | "阿里" => Ok(LlmProvider::Alibaba),
            "deepseek" | "深度求索" => Ok(LlmProvider::Deepseek),
            "zhipu" | "chatglm" | "智谱" => Ok(LlmProvider::Zhipu),
            "moonshot" | "月之暗面" => Ok(LlmProvider::Moonshot),
            "custom" => Ok(LlmProvider::Custom),
            _ => Err(format!("Unknown provider: {}", s)),
        }
    }
}

impl LlmProvider {
    /// Get default API base URL for this provider
    pub fn default_base_url(&self) -> Option<String> {
        match self {
            LlmProvider::Anthropic => {
                Some("https://api.anthropic.com/v1/messages".to_string())
            }
            LlmProvider::OpenAi => {
                Some("https://api.openai.com/v1/chat/completions".to_string())
            }
            LlmProvider::Azure => None, // Must be provided
            LlmProvider::Google => {
                Some("https://generativelanguage.googleapis.com/v1beta".to_string())
            }
            LlmProvider::Minimax => {
                Some("https://api.minimax.chat/v1/text/chatcompletion_v2".to_string())
            }
            LlmProvider::Alibaba => {
                Some("https://dashscope.aliyuncs.com/api/v1/services/aigc/text-generation/generation".to_string())
            }
            LlmProvider::Deepseek => {
                Some("https://api.deepseek.com/v1/chat/completions".to_string())
            }
            LlmProvider::Zhipu => {
                Some("https://open.bigmodel.cn/api/paas/v4/chat/completions".to_string())
            }
            LlmProvider::Moonshot => {
                // International console (platform.moonshot.ai) uses api.moonshot.ai.
                // Mainland keys: set `base_url` to `https://api.moonshot.cn/v1/chat/completions` in Settings.
                Some("https://api.moonshot.ai/v1/chat/completions".to_string())
            }
            LlmProvider::Custom => Some("http://localhost:8080/v1/chat/completions".to_string()),
        }
    }

    /// Get default model for this provider
    pub fn default_model(&self) -> String {
        match self {
            LlmProvider::Anthropic => "claude-3-5-sonnet-20241022".to_string(),
            LlmProvider::OpenAi => "gpt-4o".to_string(),
            LlmProvider::Azure => "gpt-4".to_string(),
            LlmProvider::Google => "gemini-1.5-pro".to_string(),
            LlmProvider::Minimax => "abab6.5-chat".to_string(),
            LlmProvider::Alibaba => "qwen-max".to_string(),
            LlmProvider::Deepseek => "deepseek-chat".to_string(),
            LlmProvider::Zhipu => "glm-4".to_string(),
            // Kimi K2 — current OpenAI-compatible id (older moonshot-v1-* may 404 on newer keys)
            LlmProvider::Moonshot => "kimi-k2-0905-preview".to_string(),
            LlmProvider::Custom => "default".to_string(),
        }
    }

    /// Check if tools/function calling is supported
    pub fn supports_tools(&self) -> bool {
        match self {
            LlmProvider::Anthropic => true,
            LlmProvider::OpenAi => true,
            LlmProvider::Azure => true,
            LlmProvider::Google => true,
            LlmProvider::Minimax => true,
            LlmProvider::Alibaba => true,
            LlmProvider::Deepseek => true,
            LlmProvider::Zhipu => true,
            LlmProvider::Moonshot => true,
            LlmProvider::Custom => true,
        }
    }

    /// Get human-readable display name
    pub fn display_name(&self) -> &'static str {
        match self {
            LlmProvider::Anthropic => "Anthropic Claude",
            LlmProvider::OpenAi => "OpenAI",
            LlmProvider::Azure => "Azure OpenAI",
            LlmProvider::Google => "Google Gemini",
            LlmProvider::Minimax => "MiniMax",
            LlmProvider::Alibaba => "阿里通义千问 (Alibaba Qwen)",
            LlmProvider::Deepseek => "DeepSeek",
            LlmProvider::Zhipu => "智谱 ChatGLM (Zhipu)",
            LlmProvider::Moonshot => "Moonshot AI",
            LlmProvider::Custom => "自定义端点 (Custom)",
        }
    }

    /// Check if this is a domestic Chinese provider
    pub fn is_domestic(&self) -> bool {
        matches!(
            self,
            LlmProvider::Alibaba
                | LlmProvider::Zhipu
                | LlmProvider::Moonshot
                | LlmProvider::Minimax
                | LlmProvider::Deepseek
        )
    }

    /// Get environment variable name for API key
    pub fn api_key_env(&self) -> Vec<&'static str> {
        match self {
            LlmProvider::Anthropic => vec!["ANTHROPIC_API_KEY", "LLM_API_KEY"],
            LlmProvider::OpenAi => vec!["OPENAI_API_KEY", "LLM_API_KEY"],
            LlmProvider::Azure => vec!["AZURE_OPENAI_KEY", "LLM_API_KEY"],
            LlmProvider::Google => vec!["GOOGLE_API_KEY", "GEMINI_API_KEY", "LLM_API_KEY"],
            LlmProvider::Minimax => vec!["MINIMAX_API_KEY", "LLM_API_KEY"],
            LlmProvider::Alibaba => vec!["DASHSCOPE_API_KEY", "ALIBABA_API_KEY", "LLM_API_KEY"],
            LlmProvider::Deepseek => vec!["DEEPSEEK_API_KEY", "LLM_API_KEY"],
            LlmProvider::Zhipu => vec!["ZHIPU_API_KEY", "LLM_API_KEY"],
            LlmProvider::Moonshot => vec!["MOONSHOT_API_KEY", "LLM_API_KEY"],
            LlmProvider::Custom => vec!["LLM_API_KEY"],
        }
    }

    /// List all supported providers
    pub fn all_providers() -> Vec<LlmProvider> {
        vec![
            LlmProvider::Anthropic,
            LlmProvider::OpenAi,
            LlmProvider::Azure,
            LlmProvider::Google,
            LlmProvider::Minimax,
            LlmProvider::Alibaba,
            LlmProvider::Deepseek,
            LlmProvider::Zhipu,
            LlmProvider::Moonshot,
            LlmProvider::Custom,
        ]
    }
}

/// Unified LLM configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    /// API provider type
    pub provider: LlmProvider,
    /// API key (or token)
    pub api_key: String,
    /// Optional secret key (for some domestic providers)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secret_key: Option<String>,
    /// Optional app ID (for some domestic providers like Xunfei)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_id: Option<String>,
    /// Base API URL (optional, uses provider default if not set)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    /// Model identifier
    pub model: String,
    /// Maximum tokens to generate
    pub max_tokens: u32,
    /// Temperature (0.0 - 2.0)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    /// System prompt
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,
    /// Request timeout in seconds
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
    /// Provider-specific extra headers
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extra_headers: Option<HashMap<String, String>>,
    /// Extra query parameters (for some providers)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extra_query: Option<HashMap<String, String>>,
    /// Moonshot/Custom only: request body always includes `thinking: true|false` (default false).
    /// DeepSeek and other providers leave this unset. When true, tool-call replays need `reasoning_content`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<bool>,
}

fn default_timeout() -> u64 {
    120
}

impl LlmConfig {
    /// Create new config with defaults for a provider
    pub fn new(provider: LlmProvider, api_key: impl Into<String>) -> Self {
        let api_key = api_key.into();
        let model = provider.default_model();
        Self {
            provider,
            api_key,
            secret_key: None,
            app_id: None,
            base_url: None,
            model,
            max_tokens: 4096,
            temperature: None,
            system_prompt: None,
            timeout_secs: 120,
            extra_headers: None,
            extra_query: None,
            thinking: None,
        }
    }

    /// Get API endpoint URL
    pub fn api_url(&self) -> String {
        let raw = self
            .base_url
            .clone()
            .or_else(|| self.provider.default_base_url())
            .unwrap_or_else(|| "http://localhost:8080".to_string());
        Self::normalize_moonshot_chat_url(&self.provider, raw)
    }

    /// If the user pastes only the API base (`.../v1`), Moonshot returns HTTP 404.
    fn normalize_moonshot_chat_url(provider: &LlmProvider, url: String) -> String {
        if !matches!(provider, LlmProvider::Moonshot) {
            return url.trim().to_string();
        }
        let t = url.trim().trim_end_matches('/');
        if t.contains("chat/completions") {
            return t.to_string();
        }
        if t.ends_with("/v1") {
            return format!("{}/chat/completions", t);
        }
        t.to_string()
    }

    /// Check if tools are supported
    pub fn supports_tools(&self) -> bool {
        self.provider.supports_tools()
    }

    /// Builder method: set base URL
    pub fn with_base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = Some(url.into());
        self
    }

    /// Builder method: set model
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = model.into();
        self
    }

    /// Builder method: set temperature
    pub fn with_temperature(mut self, temp: f32) -> Self {
        self.temperature = Some(temp);
        self
    }

    /// Builder method: set system prompt
    pub fn with_system_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.system_prompt = Some(prompt.into());
        self
    }

    /// Builder method: set max tokens
    pub fn with_max_tokens(mut self, tokens: u32) -> Self {
        self.max_tokens = tokens;
        self
    }

    /// Builder method: set secret key (for domestic providers)
    pub fn with_secret_key(mut self, key: impl Into<String>) -> Self {
        self.secret_key = Some(key.into());
        self
    }

    /// Builder method: set app ID (for Xunfei, etc.)
    pub fn with_app_id(mut self, id: impl Into<String>) -> Self {
        self.app_id = Some(id.into());
        self
    }

    /// Validate the configuration
    pub fn validate(&self) -> Result<(), String> {
        if self.api_key.is_empty() {
            return Err(format!(
                "API key is required for provider {}",
                self.provider.display_name()
            ));
        }

        Ok(())
    }
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            provider: LlmProvider::Anthropic,
            api_key: String::new(),
            secret_key: None,
            app_id: None,
            base_url: None,
            model: LlmProvider::Anthropic.default_model(),
            max_tokens: 4096,
            temperature: None,
            system_prompt: None,
            timeout_secs: 120,
            extra_headers: None,
            extra_query: None,
            thinking: None,
        }
    }
}

/// Trait for LLM API clients
#[async_trait]
pub trait LlmClient: Send + Sync {
    /// Send a message and get streaming response
    async fn send_message_streaming(
        &self,
        messages: Vec<LlmMessage>,
        tools: Vec<ToolSchema>,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<LlmStreamChunk, ApiError>> + Send>>, ApiError>;

    /// Check if the API is accessible
    async fn health_check(&self) -> Result<bool, ApiError>;

    /// Get the configuration
    fn config(&self) -> &LlmConfig;

    /// Get provider display name
    fn provider_name(&self) -> &'static str {
        self.config().provider.display_name()
    }
}

/// Create appropriate client based on config
pub fn create_client(config: LlmConfig) -> Result<Box<dyn LlmClient>, ApiError> {
    config.validate().map_err(|e| ApiError::Config { message: e })?;

    match config.provider {
        LlmProvider::Anthropic => Ok(Box::new(AnthropicClient::new(config))),
        LlmProvider::OpenAi
        | LlmProvider::Azure
        | LlmProvider::Moonshot
        | LlmProvider::Minimax
        | LlmProvider::Deepseek
        | LlmProvider::Custom => Ok(Box::new(OpenAiCompatibleClient::new(config))),
        LlmProvider::Alibaba => Ok(Box::new(AlibabaClient::new(config))),
        LlmProvider::Zhipu => Ok(Box::new(ZhipuClient::new(config))),
        LlmProvider::Google => Err(ApiError::Config {
            message: "Google Gemini provider not yet implemented. Use OpenAI-compatible providers.".to_string(),
        }),
    }
}

/// Load configuration from all sources (config file + env vars)
///
/// Priority for **provider / model**: **user config file** (`omiga.yaml` `default_provider` and its
/// model) wins over `LLM_PROVIDER` / `LLM_MODEL` / inferred provider from env. Settings and the
/// in-app provider switcher persist to that file (or memory); env must not override the user's
/// explicit model choice.
///
/// Env may still supply `LLM_API_KEY` when merging (non-empty key) and fill optional fields only
/// when the file leaves them unset (e.g. `base_url`, `temperature`).
pub fn load_config() -> Result<LlmConfig, ApiError> {
    // First, try to load from config file
    let file_config = if let Ok(config_file) = load_config_file() {
        config_file.to_llm_config()
    } else {
        None
    };

    // Load from environment variables
    let env_config = load_config_from_env();

    // Merge: file (user settings) is authoritative for provider/model; env supplements keys / gaps
    match (file_config, env_config) {
        (Some(file), Ok(env)) => {
            let mut merged = file;
            if !env.api_key.is_empty() {
                merged.api_key = env.api_key;
            }
            if merged.base_url.is_none() {
                merged.base_url = env.base_url.clone();
            }
            if merged.temperature.is_none() {
                merged.temperature = env.temperature;
            }
            // Do not override merged.provider / merged.model — user chose them in Settings or yaml.
            Ok(merged)
        }
        (Some(file), Err(_)) => Ok(file),
        (None, Ok(env)) => Ok(env),
        (None, Err(e)) => Err(e),
    }
}

/// Load config from environment variables only
pub fn load_config_from_env() -> Result<LlmConfig, ApiError> {
    use std::env;

    // Detect provider
    let provider = env::var("LLM_PROVIDER")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| {
            // Try to infer from API key env var
            if env::var("ANTHROPIC_API_KEY").is_ok() {
                LlmProvider::Anthropic
            } else if env::var("OPENAI_API_KEY").is_ok() {
                LlmProvider::OpenAi
            } else if env::var("AZURE_OPENAI_KEY").is_ok() {
                LlmProvider::Azure
            } else if env::var("MINIMAX_API_KEY").is_ok() {
                LlmProvider::Minimax
            } else if env::var("DASHSCOPE_API_KEY").is_ok() || env::var("ALIBABA_API_KEY").is_ok() {
                LlmProvider::Alibaba
            } else if env::var("DEEPSEEK_API_KEY").is_ok() {
                LlmProvider::Deepseek
            } else if env::var("ZHIPU_API_KEY").is_ok() {
                LlmProvider::Zhipu
            } else if env::var("MOONSHOT_API_KEY").is_ok() {
                LlmProvider::Moonshot
            } else {
                LlmProvider::Anthropic
            }
        });

    // Get API key based on provider
    let api_key = provider
        .api_key_env()
        .iter()
        .filter_map(|&env_var| env::var(env_var).ok())
        .next()
        .or_else(|| env::var("LLM_API_KEY").ok())
        .ok_or_else(|| ApiError::Config {
            message: format!(
                "No API key found for provider {:?}. Set one of: {} or LLM_API_KEY",
                provider,
                provider.api_key_env().join(", ")
            ),
        })?;

    // Optional secret key
    let secret_key = env::var(format!("{:?}_SECRET_KEY", provider).to_uppercase())
        .or_else(|_| env::var("LLM_SECRET_KEY"))
        .ok();

    // Optional app ID
    let app_id = env::var(format!("{:?}_APP_ID", provider).to_uppercase())
        .or_else(|_| env::var("LLM_APP_ID"))
        .ok();

    // Get model
    let model = env::var("LLM_MODEL").unwrap_or_else(|_| provider.default_model());

    // Optional settings
    let base_url = env::var("LLM_BASE_URL").ok();
    let temperature = env::var("LLM_TEMPERATURE").ok().and_then(|s| s.parse().ok());
    let max_tokens = env::var("LLM_MAX_TOKENS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(4096u32);
    let timeout_secs = env::var("LLM_TIMEOUT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(120u64);
    let system_prompt = env::var("LLM_SYSTEM_PROMPT").ok();

    Ok(LlmConfig {
        provider,
        api_key,
        secret_key,
        app_id,
        base_url,
        model,
        max_tokens,
        temperature,
        system_prompt,
        timeout_secs,
        extra_headers: None,
        extra_query: None,
        thinking: None,
    })
}

/// Parse provider from string
pub fn parse_provider(s: &str) -> Result<LlmProvider, String> {
    s.parse()
}

/// List all supported providers with display names
pub fn list_providers() -> Vec<(LlmProvider, &'static str)> {
    LlmProvider::all_providers()
        .into_iter()
        .map(|p| (p, p.display_name()))
        .collect()
}

/// Check if configuration is available
pub fn is_configured() -> bool {
    load_config().is_ok()
}

/// Get current configuration info (for UI display)
pub fn get_config_info() -> HashMap<String, String> {
    let mut info = HashMap::new();

    match load_config() {
        Ok(config) => {
            info.insert("provider".to_string(), config.provider.display_name().to_string());
            info.insert("model".to_string(), config.model.clone());
            info.insert("status".to_string(), "configured".to_string());
            if config.provider.is_domestic() {
                info.insert("region".to_string(), "China".to_string());
            }
        }
        Err(_) => {
            info.insert("status".to_string(), "not configured".to_string());
        }
    }

    info
}
