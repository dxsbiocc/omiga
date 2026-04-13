//! Execution Environment Configuration Commands
//!
//! Tauri commands for managing execution environment API settings
//! All settings are stored in the unified config file (omiga.yaml)

use crate::llm::{
    DaytonaExecConfig, ExecutionEnvsConfig, ModalExecConfig, SshExecConfig,
    load_config_file, LlmConfigFile
};
use crate::llm::config::find_config_file;
use std::collections::HashMap;

/// Merge SSH configs from ~/.ssh/config with user-defined configs
fn get_merged_ssh_configs() -> Result<HashMap<String, SshExecConfig>, String> {
    // Parse ~/.ssh/config
    let ssh_configs = SshExecConfig::parse_ssh_config()
        .map_err(|e| format!("Failed to parse SSH config: {}", e))?;
    
    // Load user-defined configs from omiga.yaml
    let user_configs = match load_config_file() {
        Ok(config) => config
            .execution_envs
            .and_then(|e| e.ssh)
            .unwrap_or_default(),
        Err(_) => HashMap::new(),
    };
    
    // Merge configs: user-defined takes precedence
    let mut merged = ssh_configs;
    for (name, config) in user_configs {
        merged.insert(name, config);
    }
    
    Ok(merged)
}

/// Load the unified config file
fn load_config() -> Result<LlmConfigFile, String> {
    load_config_file().map_err(|e| format!("Failed to load config: {}", e))
}

/// Save the unified config file
fn save_config(config: &LlmConfigFile) -> Result<(), String> {
    let config_path = find_config_file()
        .or_else(|| dirs::config_dir().map(|d| d.join("omiga").join("omiga.yaml")))
        .ok_or("Could not determine config path")?;

    // Ensure parent directory exists
    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }

    let content = serde_yaml::to_string(config).map_err(|e| e.to_string())?;
    std::fs::write(&config_path, content).map_err(|e| e.to_string())?;

    Ok(())
}

/// Get execution environments configuration
#[tauri::command]
pub fn get_execution_envs_config() -> Result<ExecutionEnvsConfig, String> {
    let config = load_config()?;
    Ok(config.execution_envs.unwrap_or_default())
}

/// Save execution environments configuration
#[tauri::command]
pub fn save_execution_envs_config(env_config: ExecutionEnvsConfig) -> Result<(), String> {
    let mut config = load_config()?;
    config.execution_envs = Some(env_config);
    save_config(&config)
}

/// Get Modal configuration
#[tauri::command]
pub fn get_modal_config() -> Result<Option<ModalExecConfig>, String> {
    let config = load_config()?;
    Ok(config.execution_envs.and_then(|e| e.modal))
}

/// Save Modal configuration
#[tauri::command]
pub fn save_modal_config(modal_config: ModalExecConfig) -> Result<(), String> {
    let mut config = load_config()?;

    if config.execution_envs.is_none() {
        config.execution_envs = Some(ExecutionEnvsConfig::default());
    }

    if let Some(ref mut envs) = config.execution_envs {
        envs.modal = Some(modal_config);
    }

    save_config(&config)
}

/// Get Daytona configuration
#[tauri::command]
pub fn get_daytona_config() -> Result<Option<DaytonaExecConfig>, String> {
    let config = load_config()?;
    Ok(config.execution_envs.and_then(|e| e.daytona))
}

/// Save Daytona configuration
#[tauri::command]
pub fn save_daytona_config(daytona_config: DaytonaExecConfig) -> Result<(), String> {
    let mut config = load_config()?;

    if config.execution_envs.is_none() {
        config.execution_envs = Some(ExecutionEnvsConfig::default());
    }

    if let Some(ref mut envs) = config.execution_envs {
        envs.daytona = Some(daytona_config);
    }

    save_config(&config)
}

/// Get all SSH configurations (merged from ~/.ssh/config and omiga.yaml)
#[tauri::command]
pub fn get_ssh_configs() -> Result<HashMap<String, SshExecConfig>, String> {
    get_merged_ssh_configs()
}

/// Get a specific SSH configuration by name (searches ~/.ssh/config and omiga.yaml)
#[tauri::command]
pub fn get_ssh_config(name: String) -> Result<Option<SshExecConfig>, String> {
    let merged = get_merged_ssh_configs()?;
    Ok(merged.get(&name).cloned())
}

/// Save an SSH configuration
#[tauri::command]
pub fn save_ssh_config(name: String, ssh_config: SshExecConfig) -> Result<(), String> {
    let mut config = load_config()?;

    if config.execution_envs.is_none() {
        config.execution_envs = Some(ExecutionEnvsConfig::default());
    }

    if let Some(ref mut envs) = config.execution_envs {
        if envs.ssh.is_none() {
            envs.ssh = Some(HashMap::new());
        }
        if let Some(ref mut ssh) = envs.ssh {
            ssh.insert(name, ssh_config);
        }
    }

    save_config(&config)
}

/// Delete an SSH configuration
#[tauri::command]
pub fn delete_ssh_config(name: String) -> Result<(), String> {
    let mut config = load_config()?;

    if let Some(ref mut envs) = config.execution_envs {
        if let Some(ref mut ssh) = envs.ssh {
            ssh.remove(&name);
        }
    }

    save_config(&config)
}

/// Check if Modal is configured
#[tauri::command]
pub fn is_modal_configured() -> Result<bool, String> {
    let config = load_config()?;
    Ok(config.is_modal_configured())
}

/// Check if Daytona is configured
#[tauri::command]
pub fn is_daytona_configured() -> Result<bool, String> {
    let config = load_config()?;
    Ok(config.is_daytona_configured())
}

/// Get the path to the config file
#[tauri::command]
pub fn get_execution_envs_config_path() -> Result<String, String> {
    let path = find_config_file()
        .or_else(|| dirs::config_dir().map(|d| d.join("omiga").join("omiga.yaml")))
        .ok_or("Could not determine config path")?;
    Ok(path.to_string_lossy().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_modal_config_roundtrip() {
        let modal = ModalExecConfig {
            token_id: Some("test-id".to_string()),
            token_secret: Some("test-secret".to_string()),
            default_image: Some("python:3.11".to_string()),
            enabled: true,
        };

        // Serialize to JSON (simulating what Tauri does)
        let json = serde_json::to_string(&modal).unwrap();
        let deserialized: ModalExecConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.token_id, Some("test-id".to_string()));
        assert_eq!(deserialized.token_secret, Some("test-secret".to_string()));
    }

    #[test]
    fn test_daytona_config_roundtrip() {
        let daytona = DaytonaExecConfig {
            server_url: Some("https://api.daytona.io".to_string()),
            api_key: Some("test-key".to_string()),
            default_image: Some("ubuntu:22.04".to_string()),
            enabled: true,
        };

        let json = serde_json::to_string(&daytona).unwrap();
        let deserialized: DaytonaExecConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.server_url, Some("https://api.daytona.io".to_string()));
        assert_eq!(deserialized.api_key, Some("test-key".to_string()));
    }

    #[test]
    fn test_ssh_config_roundtrip() {
        let ssh = SshExecConfig {
            host: Some("my-server".to_string()),
            host_name: Some("192.168.1.100".to_string()),
            user: Some("ubuntu".to_string()),
            port: 22,
            identity_file: Some("~/.ssh/id_rsa".to_string()),
            enabled: true,
        };

        let json = serde_json::to_string(&ssh).unwrap();
        let deserialized: SshExecConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.host, Some("my-server".to_string()));
        assert_eq!(deserialized.host_name, Some("192.168.1.100".to_string()));
        assert_eq!(deserialized.user, Some("ubuntu".to_string()));
        assert_eq!(deserialized.port, 22);
    }

    #[test]
    fn test_llm_config_file_exec_envs_methods() {
        let mut exec_envs = ExecutionEnvsConfig::default();
        exec_envs.modal = Some(ModalExecConfig {
            token_id: Some("modal-id".to_string()),
            token_secret: Some("modal-secret".to_string()),
            default_image: None,
            enabled: true,
        });
        exec_envs.daytona = Some(DaytonaExecConfig {
            server_url: Some("https://daytona.io".to_string()),
            api_key: Some("daytona-key".to_string()),
            default_image: None,
            enabled: true,
        });

        let config = LlmConfigFile {
            execution_envs: Some(exec_envs),
            ..Default::default()
        };

        assert!(config.is_modal_configured());
        assert!(config.is_daytona_configured());
        assert_eq!(config.modal_token_id(), Some("modal-id".to_string()));
        assert_eq!(config.daytona_server_url(), Some("https://daytona.io".to_string()));
    }
}
