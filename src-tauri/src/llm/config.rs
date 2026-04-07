//! Configuration file support for LLM providers
//!
//! Supports YAML, JSON, and TOML formats
//! Default config file: `omiga.yaml` (or `omiga.json`, `omiga.toml`)

use super::{LlmConfig, LlmProvider};
use crate::errors::ApiError;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Configuration file structure
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub struct LlmConfigFile {
    /// Version of the config file format
    #[serde(default = "default_version")]
    pub version: String,

    /// Active provider configuration
    #[serde(rename = "default")]
    pub default_provider: Option<String>,

    /// Provider configurations
    pub providers: Option<HashMap<String, ProviderConfig>>,

    /// Global settings
    pub settings: Option<GlobalSettings>,
}

fn default_version() -> String {
    "1.0".to_string()
}

/// Individual provider configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProviderConfig {
    /// Provider type (anthropic, openai, baidu, etc.)
    #[serde(rename = "type")]
    pub provider_type: String,

    /// API key
    pub api_key: Option<String>,

    /// Secret key (for some domestic providers)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secret_key: Option<String>,

    /// App ID (for Xunfei, etc.)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_id: Option<String>,

    /// Base URL override
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,

    /// Model name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,

    /// Maximum tokens
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,

    /// Temperature
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,

    /// System prompt
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,

    /// Request timeout in seconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout: Option<u64>,

    /// Extra headers
    #[serde(skip_serializing_if = "Option::is_none")]
    pub headers: Option<HashMap<String, String>>,

    /// Extra query parameters
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query_params: Option<HashMap<String, String>>,

    /// Whether this provider is enabled
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

/// Global settings
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GlobalSettings {
    /// Default max tokens for all providers
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,

    /// Default temperature
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,

    /// Default timeout in seconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout: Option<u64>,

    /// Whether to enable tools by default
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enable_tools: Option<bool>,
}

impl LlmConfigFile {
    /// Convert file config to runtime LlmConfig
    pub fn to_llm_config(&self) -> Option<LlmConfig> {
        let default_name = self.default_provider.clone()?;
        let providers = self.providers.clone()?;
        let provider_config = providers.get(&default_name)?;

        if !provider_config.enabled {
            return None;
        }

        let provider: LlmProvider = provider_config.provider_type.parse().ok()?;

        // Get API key - support env var reference like ${ANTHROPIC_API_KEY}
        let api_key = provider_config
            .api_key
            .as_ref()
            .map(|key| expand_env_vars(key))
            .unwrap_or_default();

        if api_key.is_empty() {
            return None;
        }

        let mut config = LlmConfig::new(provider, api_key);

        // Apply provider-specific settings
        if let Some(secret) = &provider_config.secret_key {
            config.secret_key = Some(expand_env_vars(secret));
        }
        if let Some(app_id) = &provider_config.app_id {
            config.app_id = Some(expand_env_vars(app_id));
        }
        if let Some(url) = &provider_config.base_url {
            config.base_url = Some(expand_env_vars(url));
        }
        if let Some(model) = &provider_config.model {
            config.model = expand_env_vars(model);
        }
        if let Some(tokens) = provider_config.max_tokens {
            config.max_tokens = tokens;
        }
        if let Some(temp) = provider_config.temperature {
            config.temperature = Some(temp);
        }
        if let Some(prompt) = &provider_config.system_prompt {
            config.system_prompt = Some(expand_env_vars(prompt));
        }
        if let Some(timeout) = provider_config.timeout {
            config.timeout_secs = timeout;
        }
        if let Some(headers) = &provider_config.headers {
            config.extra_headers = Some(
                headers
                    .iter()
                    .map(|(k, v)| (k.clone(), expand_env_vars(v)))
                    .collect(),
            );
        }
        if let Some(query) = &provider_config.query_params {
            config.extra_query = Some(
                query
                    .iter()
                    .map(|(k, v)| (k.clone(), expand_env_vars(v)))
                    .collect(),
            );
        }

        // Apply global settings (provider settings take precedence)
        if let Some(settings) = &self.settings {
            if config.max_tokens == 4096 {
                if let Some(tokens) = settings.max_tokens {
                    config.max_tokens = tokens;
                }
            }
            if config.temperature.is_none() {
                config.temperature = settings.temperature;
            }
            if config.timeout_secs == 120 {
                if let Some(timeout) = settings.timeout {
                    config.timeout_secs = timeout;
                }
            }
        }

        Some(config)
    }

    /// Create example config for a provider
    pub fn example(provider: LlmProvider) -> Self {
        let provider_name = provider.to_string();
        let mut providers = HashMap::new();

        let config = match provider {
            LlmProvider::Anthropic => ProviderConfig {
                provider_type: "anthropic".to_string(),
                api_key: Some("${ANTHROPIC_API_KEY}".to_string()),
                model: Some("claude-3-5-sonnet-20241022".to_string()),
                max_tokens: Some(4096),
                temperature: Some(0.7),
                ..Default::default()
            },
            LlmProvider::OpenAi => ProviderConfig {
                provider_type: "openai".to_string(),
                api_key: Some("${OPENAI_API_KEY}".to_string()),
                model: Some("gpt-4o".to_string()),
                max_tokens: Some(4096),
                temperature: Some(0.7),
                ..Default::default()
            },
            LlmProvider::Minimax => ProviderConfig {
                provider_type: "minimax".to_string(),
                api_key: Some("${MINIMAX_API_KEY}".to_string()),
                model: Some("abab6.5-chat".to_string()),
                max_tokens: Some(4096),
                temperature: Some(0.7),
                ..Default::default()
            },
            LlmProvider::Alibaba => ProviderConfig {
                provider_type: "alibaba".to_string(),
                api_key: Some("${DASHSCOPE_API_KEY}".to_string()),
                model: Some("qwen-max".to_string()),
                max_tokens: Some(4096),
                temperature: Some(0.7),
                ..Default::default()
            },
            LlmProvider::Deepseek => ProviderConfig {
                provider_type: "deepseek".to_string(),
                api_key: Some("${DEEPSEEK_API_KEY}".to_string()),
                model: Some("deepseek-chat".to_string()),
                max_tokens: Some(4096),
                temperature: Some(0.7),
                ..Default::default()
            },
            LlmProvider::Zhipu => ProviderConfig {
                provider_type: "zhipu".to_string(),
                api_key: Some("${ZHIPU_API_KEY}".to_string()),
                model: Some("glm-4".to_string()),
                max_tokens: Some(4096),
                temperature: Some(0.7),
                ..Default::default()
            },
            _ => ProviderConfig {
                provider_type: provider_name.clone(),
                api_key: Some("${API_KEY}".to_string()),
                ..Default::default()
            },
        };

        providers.insert(provider_name, config);

        Self {
            version: "1.0".to_string(),
            default_provider: Some(provider.to_string()),
            providers: Some(providers),
            settings: Some(GlobalSettings {
                max_tokens: Some(4096),
                temperature: Some(0.7),
                timeout: Some(120),
                enable_tools: Some(true),
            }),
        }
    }

    /// Create multi-provider example config
    pub fn multi_provider_example() -> Self {
        let mut providers = HashMap::new();

        providers.insert(
            "anthropic".to_string(),
            ProviderConfig {
                provider_type: "anthropic".to_string(),
                api_key: Some("${ANTHROPIC_API_KEY}".to_string()),
                model: Some("claude-3-5-sonnet-20241022".to_string()),
                enabled: true,
                ..Default::default()
            },
        );

        providers.insert(
            "openai".to_string(),
            ProviderConfig {
                provider_type: "openai".to_string(),
                api_key: Some("${OPENAI_API_KEY}".to_string()),
                model: Some("gpt-4o".to_string()),
                enabled: false,
                ..Default::default()
            },
        );

        providers.insert(
            "alibaba".to_string(),
            ProviderConfig {
                provider_type: "alibaba".to_string(),
                api_key: Some("${DASHSCOPE_API_KEY}".to_string()),
                model: Some("qwen-max".to_string()),
                enabled: true,
                ..Default::default()
            },
        );

        providers.insert(
            "zhipu".to_string(),
            ProviderConfig {
                provider_type: "zhipu".to_string(),
                api_key: Some("${ZHIPU_API_KEY}".to_string()),
                model: Some("glm-4".to_string()),
                enabled: true,
                ..Default::default()
            },
        );

        Self {
            version: "1.0".to_string(),
            default_provider: Some("anthropic".to_string()),
            providers: Some(providers),
            settings: Some(GlobalSettings {
                max_tokens: Some(4096),
                temperature: Some(0.7),
                timeout: Some(120),
                enable_tools: Some(true),
            }),
        }
    }

    /// Generate example YAML content
    pub fn to_yaml_example(&self) -> String {
        format!(
            r#"# Omiga LLM Configuration
# Place this file in your project root as `omiga.yaml`
# or in `~/.config/omiga/config.yaml`

version: "{}"
default: "{}"

providers:
{}

settings:
  max_tokens: {}
  temperature: {}
  timeout: {}
  enable_tools: {}
"#,
            self.version,
            self.default_provider.as_deref().unwrap_or("anthropic"),
            self.providers
                .as_ref()
                .map(|p| p
                    .iter()
                    .map(|(name, cfg)| format_provider_yaml(name, cfg))
                    .collect::<Vec<_>>()
                    .join("\n"))
                .unwrap_or_default(),
            self.settings
                .as_ref()
                .and_then(|s| s.max_tokens)
                .unwrap_or(4096),
            self.settings
                .as_ref()
                .and_then(|s| s.temperature)
                .unwrap_or(0.7),
            self.settings
                .as_ref()
                .and_then(|s| s.timeout)
                .unwrap_or(120),
            self.settings
                .as_ref()
                .and_then(|s| s.enable_tools)
                .unwrap_or(true)
        )
    }
}

fn format_provider_yaml(name: &str, cfg: &ProviderConfig) -> String {
    let mut lines = vec![format!("  {}:", name)];
    lines.push(format!("    type: {}", cfg.provider_type));

    if let Some(key) = &cfg.api_key {
        lines.push(format!("    api_key: {}", key));
    }
    if let Some(secret) = &cfg.secret_key {
        lines.push(format!("    secret_key: {}", secret));
    }
    if let Some(app_id) = &cfg.app_id {
        lines.push(format!("    app_id: {}", app_id));
    }
    if let Some(model) = &cfg.model {
        lines.push(format!("    model: {}", model));
    }
    if let Some(url) = &cfg.base_url {
        lines.push(format!("    base_url: {}", url));
    }
    if let Some(tokens) = cfg.max_tokens {
        lines.push(format!("    max_tokens: {}", tokens));
    }
    if let Some(temp) = cfg.temperature {
        lines.push(format!("    temperature: {}", temp));
    }
    if !cfg.enabled {
        lines.push("    enabled: false".to_string());
    }

    lines.join("\n")
}

/// Expand environment variables in string
/// Supports ${VAR} and $VAR syntax
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

    result
}

/// Find config file in standard locations
pub fn find_config_file() -> Option<PathBuf> {
    let possible_names = ["omiga.yaml", "omiga.yml", "omiga.json", "omiga.toml"];

    // Check current directory
    for name in &possible_names {
        let path = Path::new(name);
        if path.exists() {
            return Some(path.to_path_buf());
        }
    }

    // Check project root (if running from src-tauri)
    let project_root = Path::new("../..");
    for name in &possible_names {
        let path = project_root.join(name);
        if path.exists() {
            return Some(path);
        }
    }

    // Check config directory
    if let Some(config_dir) = dirs::config_dir() {
        let omiga_config = config_dir.join("omiga");
        for name in &possible_names {
            let path = omiga_config.join(name);
            if path.exists() {
                return Some(path);
            }
        }
    }

    None
}

/// Load config file from standard locations
pub fn load_config_file() -> Result<LlmConfigFile, ApiError> {
    let path = find_config_file().ok_or_else(|| ApiError::Config {
        message: "No config file found".to_string(),
    })?;

    load_config_file_at(&path)
}

/// Load config file from specific path
pub fn load_config_file_at(path: &Path) -> Result<LlmConfigFile, ApiError> {
    let content = std::fs::read_to_string(path).map_err(|e| ApiError::Config {
        message: format!("Failed to read config file: {}", e),
    })?;

    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");

    let config: LlmConfigFile = match ext {
        "yaml" | "yml" => serde_yaml::from_str(&content).map_err(|e| ApiError::Config {
            message: format!("Failed to parse YAML config: {}", e),
        })?,
        "json" => serde_json::from_str(&content).map_err(|e| ApiError::Config {
            message: format!("Failed to parse JSON config: {}", e),
        })?,
        "toml" => toml::from_str(&content).map_err(|e| ApiError::Config {
            message: format!("Failed to parse TOML config: {}", e),
        })?,
        _ => {
            // Try YAML first, then JSON
            serde_yaml::from_str(&content)
                .or_else(|_| serde_json::from_str(&content))
                .map_err(|e| ApiError::Config {
                    message: format!("Failed to parse config: {}", e),
                })?
        }
    };

    Ok(config)
}

/// Save config file
pub fn save_config_file(config: &LlmConfigFile, path: &Path) -> Result<(), ApiError> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("yaml");

    let content = match ext {
        "yaml" | "yml" => serde_yaml::to_string(config).map_err(|e| ApiError::Config {
            message: format!("Failed to serialize config: {}", e),
        })?,
        "json" => serde_json::to_string_pretty(config).map_err(|e| ApiError::Config {
            message: format!("Failed to serialize config: {}", e),
        })?,
        "toml" => toml::to_string(config).map_err(|e| ApiError::Config {
            message: format!("Failed to serialize config: {}", e),
        })?,
        _ => serde_yaml::to_string(config).map_err(|e| ApiError::Config {
            message: format!("Failed to serialize config: {}", e),
        })?,
    };

    std::fs::write(path, content).map_err(|e| ApiError::Config {
        message: format!("Failed to write config file: {}", e),
    })?;

    Ok(())
}

/// Initialize a new config file with example content
pub fn init_config_file(provider: LlmProvider, path: &Path) -> Result<(), ApiError> {
    let config = LlmConfigFile::example(provider);
    save_config_file(&config, path)?;
    Ok(())
}

/// Create multi-provider config file
pub fn init_multi_provider_config(path: &Path) -> Result<(), ApiError> {
    let config = LlmConfigFile::multi_provider_example();
    save_config_file(&config, path)?;
    Ok(())
}
