//! Per-session configuration file I/O.
//!
//! Each session stores its composer settings (provider, permissions, execution env,
//! worktree, etc.) in `~/.omiga/sessions/<session_id>.yaml` so sessions are fully
//! independent and survive app restarts.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Per-session composer/runtime configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionConfig {
    /// `omiga.yaml` provider entry name for this session.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_provider_entry_name: Option<String>,
    /// `ask` | `auto` | `bypass`
    #[serde(default)]
    pub permission_mode: String,
    /// Specialist agent selected in the composer (e.g. `Explore`, `Plan`, `auto`).
    #[serde(default)]
    pub composer_agent_type: String,
    /// `local` | `ssh` | `sandbox`
    #[serde(default)]
    pub execution_environment: String,
    /// Selected SSH server name when `execution_environment == "ssh"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssh_server: Option<String>,
    /// `modal` | `daytona` | `docker` | `singularity`
    #[serde(default)]
    pub sandbox_backend: String,
    /// `"none"` | `"conda"` | `"venv"` | `"pyenv"`
    #[serde(default)]
    pub local_venv_type: String,
    /// Conda env name, venv directory path, or pyenv version string.
    #[serde(default)]
    pub local_venv_name: String,
    /// Whether to use a worktree for git operations in this session.
    #[serde(default)]
    pub use_worktree: bool,
    /// Runtime constraint overrides for this session.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_constraints: Option<crate::domain::runtime_constraints::RuntimeConstraintConfig>,
}

impl SessionConfig {
    /// Return sensible defaults for a brand-new session.
    pub fn default_for_new() -> Self {
        Self {
            active_provider_entry_name: None,
            permission_mode: "auto".to_string(),
            composer_agent_type: "auto".to_string(),
            execution_environment: "local".to_string(),
            ssh_server: None,
            sandbox_backend: "docker".to_string(),
            local_venv_type: "none".to_string(),
            local_venv_name: String::new(),
            use_worktree: false,
            runtime_constraints: None,
        }
    }
}

fn sessions_dir() -> PathBuf {
    dirs::home_dir()
        .map(|h| h.join(".omiga").join("sessions"))
        .unwrap_or_else(|| PathBuf::from(".omiga/sessions"))
}

fn config_path(session_id: &str) -> PathBuf {
    sessions_dir().join(format!("{}.yaml", session_id))
}

/// Load session config from `~/.omiga/sessions/<session_id>.yaml`.
/// Returns defaults if the file does not exist yet.
pub fn load_session_config(session_id: &str) -> SessionConfig {
    let path = config_path(session_id);
    if !path.exists() {
        return SessionConfig::default_for_new();
    }
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("Failed to read session config for {}: {}", session_id, e);
            return SessionConfig::default_for_new();
        }
    };
    match serde_yaml::from_str(&content) {
        Ok(cfg) => cfg,
        Err(e) => {
            tracing::warn!("Failed to parse session config for {}: {}", session_id, e);
            SessionConfig::default_for_new()
        }
    }
}

/// Save session config to `~/.omiga/sessions/<session_id>.yaml`.
pub fn save_session_config(session_id: &str, config: &SessionConfig) -> Result<(), String> {
    let dir = sessions_dir();
    if let Err(e) = std::fs::create_dir_all(&dir) {
        return Err(format!("Failed to create sessions dir: {}", e));
    }
    let path = config_path(session_id);
    let content = serde_yaml::to_string(config)
        .map_err(|e| format!("Failed to serialize session config: {}", e))?;
    std::fs::write(&path, content).map_err(|e| format!("Failed to write session config: {}", e))?;
    Ok(())
}

/// Delete the config file for a session (e.g. on session deletion).
pub fn delete_session_config(session_id: &str) {
    let path = config_path(session_id);
    if path.exists() {
        let _ = std::fs::remove_file(&path);
    }
}

// Rename is handled inside the SQLite `sessions` table; the config file is keyed by
// `session_id` which never changes, so no file rename is needed.
