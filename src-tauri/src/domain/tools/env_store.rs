//! Session-scoped execution environment store.
//!
//! Mirrors hermes-agent's `_active_environments[task_id]` pattern:
//! - One `BaseEnvironment` per `(env_type, server/backend)` per session
//! - Lazily created on first use; cached for the full session lifetime
//! - `init_session()` (snapshot capture) runs **once** per backend — not once per tool call
//! - All tools (bash, file_read, file_write, file_edit, grep, glob) share the same environment
//!
//! Before this change every `bash` call against SSH/sandbox:
//!   1. `create_environment()` — opens a new connection / spins up a new container
//!   2. executes the command
//!   3. `cleanup()` — tears down the connection / container
//!
//! Now only step 1 happens once per session; steps 2 repeat; step 3 runs at session teardown.

use crate::execution::{create_environment, BaseEnvironment, EnvironmentConfig, EnvironmentType};
use crate::llm::config::{load_config_file, merged_ssh_configs};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use super::{ToolContext, ToolError};

// ─── Public API ──────────────────────────────────────────────────────────────

/// Cheap-to-clone handle to the session-scoped environment cache.
#[derive(Clone)]
pub struct EnvStore(Arc<StoreInner>);

impl std::fmt::Debug for EnvStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EnvStore").finish_non_exhaustive()
    }
}

struct StoreInner {
    envs: Mutex<HashMap<String, Arc<Mutex<dyn BaseEnvironment>>>>,
}

impl EnvStore {
    pub fn new() -> Self {
        Self(Arc::new(StoreInner {
            envs: Mutex::new(HashMap::new()),
        }))
    }

    /// Canonical cache key: `"local"` | `"ssh:<name>"` | `"sandbox:docker"` | …
    pub fn key(ctx: &ToolContext) -> String {
        match ctx.execution_environment.as_str() {
            "ssh" => format!("ssh:{}", ctx.ssh_server.as_deref().unwrap_or("_")),
            "sandbox" | "remote" => format!("sandbox:{}", ctx.sandbox_backend.trim()),
            _ => "local".to_string(),
        }
    }

    /// Get an existing or lazily-create a new environment for this context.
    ///
    /// Thread-safe: concurrent callers for the same key will both call
    /// `create_environment`, but only the first result is stored (second is dropped).
    pub async fn get_or_create(
        &self,
        ctx: &ToolContext,
        timeout_ms: u64,
    ) -> Result<Arc<Mutex<dyn BaseEnvironment>>, ToolError> {
        let key = Self::key(ctx);

        // Fast path — already cached
        {
            let guard = self.0.envs.lock().await;
            if let Some(env) = guard.get(&key) {
                return Ok(env.clone());
            }
        }

        // Slow path — create and cache
        let config = build_env_config(ctx, timeout_ms)?;
        let env = create_environment(config).await.map_err(|e| ToolError::ExecutionFailed {
            message: format!("执行环境初始化失败 ({}): {}", key, e),
        })?;

        {
            let mut guard = self.0.envs.lock().await;
            // A racing caller may have inserted already; use whichever got in first
            Ok(guard.entry(key).or_insert(env).clone())
        }
    }

    /// Call on session teardown to release remote connections / sandbox containers.
    pub async fn shutdown(&self) {
        let envs: Vec<_> = {
            let mut guard = self.0.envs.lock().await;
            guard.drain().map(|(_, v)| v).collect()
        };
        for arc in envs {
            if let Ok(mut env) = arc.try_lock() {
                let _ = env.cleanup().await;
            }
        }
    }
}

// ─── Path mapping ─────────────────────────────────────────────────────────────

/// Translate a tool-supplied path to the equivalent path on the remote side.
///
/// Rules (mirrors `bash.rs::ssh_remote_cwd`):
/// - Absolute paths (`/…` or `~/…`) are used verbatim.
/// - Relative paths are prefixed with:
///   - `OMIGA_SSH_REMOTE_ROOT` (default `~`) for SSH
///   - `/workspace` for sandbox environments
pub fn remote_path(ctx: &ToolContext, path: &str) -> String {
    if path.starts_with('/') || path.starts_with("~/") {
        return path.to_string();
    }
    let root = match ctx.execution_environment.as_str() {
        "ssh" => std::env::var("OMIGA_SSH_REMOTE_ROOT").unwrap_or_else(|_| "~".to_string()),
        _ => "/workspace".to_string(),
    };
    format!("{}/{}", root.trim_end_matches('/'), path)
}

// ─── Config builder ───────────────────────────────────────────────────────────

fn build_env_config(ctx: &ToolContext, timeout_ms: u64) -> Result<EnvironmentConfig, ToolError> {
    match ctx.execution_environment.as_str() {
        "ssh" => build_ssh_config(ctx, timeout_ms),
        "sandbox" | "remote" => build_sandbox_config(ctx, timeout_ms),
        _ => Ok(EnvironmentConfig {
            r#type: EnvironmentType::Local,
            cwd: ctx.cwd.to_string_lossy().to_string(),
            timeout: timeout_ms.max(5_000),
            task_id: "omiga-local-session".to_string(),
            ..Default::default()
        }),
    }
}

fn build_ssh_config(ctx: &ToolContext, timeout_ms: u64) -> Result<EnvironmentConfig, ToolError> {
    let name = ctx
        .ssh_server
        .as_ref()
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| ToolError::ExecutionFailed {
            message: "SSH: 未选择服务器，请在执行环境菜单中选择 SSH 主机。".to_string(),
        })?;
    let merged = merged_ssh_configs().map_err(|e| ToolError::ExecutionFailed { message: e })?;
    let cfg = merged.get(name).ok_or_else(|| ToolError::ExecutionFailed {
        message: format!("SSH: 找不到配置 '{}'", name),
    })?;
    let host = cfg.effective_hostname().ok_or_else(|| ToolError::ExecutionFailed {
        message: "SSH: 配置缺少 HostName".to_string(),
    })?;
    let user = cfg.user.as_ref().ok_or_else(|| ToolError::ExecutionFailed {
        message: "SSH: 配置缺少 User".to_string(),
    })?;

    let remote_root =
        std::env::var("OMIGA_SSH_REMOTE_ROOT").unwrap_or_else(|_| "~".to_string());
    let remote_cwd = if let Ok(rel) = ctx.cwd.strip_prefix(&ctx.project_root) {
        let r = rel.to_string_lossy().replace('\\', "/");
        let r = r.trim_start_matches('/');
        if r.is_empty() {
            remote_root.clone()
        } else {
            format!("{}/{}", remote_root.trim_end_matches('/'), r)
        }
    } else {
        remote_root
    };

    Ok(EnvironmentConfig {
        r#type: EnvironmentType::Ssh,
        cwd: remote_cwd,
        timeout: timeout_ms.max(5_000),
        ssh_host: Some(host.to_string()),
        ssh_user: Some(user.clone()),
        ssh_port: cfg.port,
        ssh_key_path: cfg.identity_file.clone(),
        ssh_project_root: Some(ctx.project_root.clone()),
        task_id: format!("omiga-ssh-{}", name),
        ..Default::default()
    })
}

fn build_sandbox_config(ctx: &ToolContext, timeout_ms: u64) -> Result<EnvironmentConfig, ToolError> {
    let cfg_file = load_config_file().map_err(|e| ToolError::ExecutionFailed {
        message: format!("配置读取失败: {}", e),
    })?;
    let cwd = "/workspace".to_string();
    let backend = ctx.sandbox_backend.trim();

    match backend {
        "docker" => {
            let image = std::env::var("OMIGA_DOCKER_IMAGE")
                .unwrap_or_else(|_| "ubuntu:22.04".to_string());
            Ok(EnvironmentConfig {
                r#type: EnvironmentType::Docker,
                image: Some(image),
                cwd,
                timeout: timeout_ms.max(5_000),
                task_id: "omiga-docker-session".to_string(),
                ..Default::default()
            })
        }
        "modal" => {
            let image = cfg_file
                .execution_envs.as_ref()
                .and_then(|e| e.modal.as_ref())
                .and_then(|m| m.default_image.clone())
                .unwrap_or_else(|| "ubuntu:22.04".to_string());
            Ok(EnvironmentConfig {
                r#type: EnvironmentType::Modal,
                image: Some(image),
                cwd,
                timeout: timeout_ms.max(5_000),
                task_id: "omiga-modal-session".to_string(),
                ..Default::default()
            })
        }
        "daytona" => {
            let image = cfg_file
                .execution_envs.as_ref()
                .and_then(|e| e.daytona.as_ref())
                .and_then(|d| d.default_image.clone())
                .unwrap_or_else(|| "ubuntu:22.04".to_string());
            Ok(EnvironmentConfig {
                r#type: EnvironmentType::Daytona,
                image: Some(image),
                cwd,
                timeout: timeout_ms.max(5_000),
                task_id: "omiga-daytona-session".to_string(),
                ..Default::default()
            })
        }
        "singularity" => {
            let image = std::env::var("OMIGA_SINGULARITY_IMAGE")
                .unwrap_or_else(|_| "docker://ubuntu:22.04".to_string());
            Ok(EnvironmentConfig {
                r#type: EnvironmentType::Singularity,
                image: Some(image),
                cwd,
                timeout: timeout_ms.max(5_000),
                task_id: "omiga-singularity-session".to_string(),
                network: true,
                ..Default::default()
            })
        }
        _ => Err(ToolError::ExecutionFailed {
            message: format!(
                "未知沙箱后端: '{}' (支持: docker, modal, daytona, singularity)",
                backend
            ),
        }),
    }
}
