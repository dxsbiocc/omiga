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

use super::{ToolContext, ToolError};
use crate::execution::{create_environment, BaseEnvironment, EnvironmentConfig, EnvironmentType};
use crate::llm::config::merged_ssh_configs;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;

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
    /// Cached, ready-to-use environments keyed by canonical env key.
    envs: Mutex<HashMap<String, Arc<Mutex<dyn BaseEnvironment>>>>,
    /// In-flight creation signals: concurrent callers wait on the Notify
    /// instead of each launching their own `create_environment` call.
    /// Ensures at most one SSH handshake per key, even under concurrent tool calls.
    creating: Mutex<HashMap<String, Arc<tokio::sync::Notify>>>,
}

impl Default for EnvStore {
    fn default() -> Self {
        Self::new()
    }
}

impl EnvStore {
    pub fn new() -> Self {
        Self(Arc::new(StoreInner {
            envs: Mutex::new(HashMap::new()),
            creating: Mutex::new(HashMap::new()),
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
    /// **Concurrent-safe with deduplication**: if two callers race on the same key,
    /// only ONE `create_environment` call is made. The second caller waits on a
    /// `Notify` and retries the fast path when creation completes.
    pub async fn get_or_create(
        &self,
        ctx: &ToolContext,
        timeout_ms: u64,
    ) -> Result<Arc<Mutex<dyn BaseEnvironment>>, ToolError> {
        let key = Self::key(ctx);

        loop {
            // ── Fast path: already cached ────────────────────────────────────
            {
                let guard = self.0.envs.lock().await;
                if let Some(env) = guard.get(&key) {
                    return Ok(env.clone());
                }
            }

            // ── In-flight check: another caller is already creating this key ─
            let notify = {
                let mut creating = self.0.creating.lock().await;
                if let Some(existing_notify) = creating.get(&key) {
                    // Another caller is creating — wait for it, then retry fast path
                    existing_notify.clone()
                } else {
                    // We are the creator — register our Notify and break to slow path
                    let notify = Arc::new(tokio::sync::Notify::new());
                    creating.insert(key.clone(), notify.clone());
                    // Return a sentinel that signals "we are the creator"
                    // (break out of loop by dropping into creation code below)
                    drop(creating);

                    // ── Slow path: create the environment ────────────────────
                    let config = build_env_config(ctx, timeout_ms)?;
                    let create_result =
                        create_environment(config)
                            .await
                            .map_err(|e| ToolError::ExecutionFailed {
                                message: format!("执行环境初始化失败 ({}): {}", key, e),
                            });

                    // Remove from creating map and notify waiters regardless of outcome
                    {
                        let mut creating = self.0.creating.lock().await;
                        if let Some(n) = creating.remove(&key) {
                            n.notify_waiters();
                        }
                    }

                    let env = create_result?;

                    // Cache and return
                    let mut guard = self.0.envs.lock().await;
                    return Ok(guard.entry(key).or_insert(env).clone());
                }
            };

            // Wait for in-flight creation to finish, then retry the fast path
            notify.notified().await;
        }
    }

    /// Eagerly pre-warm the connection for non-local execution environments.
    ///
    /// Call this once at session start (before the first LLM turn) so that the SSH
    /// handshake / sandbox spin-up is already complete when the AI's first tool call
    /// arrives.  The first tool call in a session is the most latency-sensitive one —
    /// lazy init adds ~10–30 s of SSH negotiation on top of the already-running 45 s
    /// outer tool timeout.
    ///
    /// A connection failure here is logged but **not** propagated — the session should
    /// still start, and a proper error will be surfaced when the first tool actually runs.
    pub async fn warmup(&self, ctx: &ToolContext) {
        if ctx.execution_environment == "local" {
            return; // Nothing to warm up for local sessions
        }
        let store = self.clone();
        let ctx_key = Self::key(ctx);
        // Clone ctx fields needed for build_env_config (ToolContext is not Clone)
        let env_type = ctx.execution_environment.clone();
        let ssh_server = ctx.ssh_server.clone();
        let sandbox_backend = ctx.sandbox_backend.clone();
        let cwd = ctx.cwd.clone();
        let project_root = ctx.project_root.clone();

        tokio::spawn(async move {
            // Build a minimal ToolContext for config construction — only env fields matter
            let mut mini_ctx = ToolContext::new(project_root);
            mini_ctx.cwd = cwd;
            mini_ctx.execution_environment = env_type;
            mini_ctx.ssh_server = ssh_server;
            mini_ctx.sandbox_backend = sandbox_backend;

            match store.get_or_create(&mini_ctx, 60_000).await {
                Ok(_) => tracing::debug!("env_store: pre-warmed {}", ctx_key),
                Err(e) => tracing::warn!("env_store: warmup failed for {}: {}", ctx_key, e),
            }
        });
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
///   - the active session project root for SSH
///   - `/workspace` for sandbox environments
pub fn remote_path(ctx: &ToolContext, path: &str) -> String {
    let path = path.trim();
    if path.starts_with('/') || path.starts_with("~/") {
        return path.to_string();
    }
    let root = match ctx.execution_environment.as_str() {
        "ssh" => ssh_remote_root_for_project(&ctx.project_root),
        _ => "/workspace".to_string(),
    };
    let mut rel = path;
    while let Some(rest) = rel.strip_prefix("./") {
        rel = rest;
    }
    if rel.is_empty() || rel == "." {
        root
    } else {
        format!("{}/{}", root.trim_end_matches('/'), rel)
    }
}

/// Remote project root for SSH sessions.
///
/// SSH session project paths are remote POSIX paths selected by the user in the
/// workspace picker. Falling back to `~` makes large operator results accumulate
/// in the user's home directory, so the session path is authoritative whenever
/// it is set. `OMIGA_SSH_REMOTE_ROOT` remains only as a compatibility fallback
/// for legacy callers that have no usable project path.
pub fn ssh_remote_root_for_project(project_root: &Path) -> String {
    let root = project_root.to_string_lossy().replace('\\', "/");
    let root = root.trim().trim_end_matches('/');
    if root.is_empty() || root == "." {
        std::env::var("OMIGA_SSH_REMOTE_ROOT").unwrap_or_else(|_| "~".to_string())
    } else {
        root.to_string()
    }
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
    let host = cfg
        .effective_hostname()
        .ok_or_else(|| ToolError::ExecutionFailed {
            message: "SSH: 配置缺少 HostName".to_string(),
        })?;
    let user = cfg
        .user
        .as_ref()
        .ok_or_else(|| ToolError::ExecutionFailed {
            message: "SSH: 配置缺少 User".to_string(),
        })?;

    let remote_root = ssh_remote_root_for_project(&ctx.project_root);
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

fn build_sandbox_config(
    ctx: &ToolContext,
    timeout_ms: u64,
) -> Result<EnvironmentConfig, ToolError> {
    let cwd = "/workspace".to_string();
    let backend = ctx.sandbox_backend.trim();

    match backend {
        "docker" => {
            let image =
                std::env::var("OMIGA_DOCKER_IMAGE").unwrap_or_else(|_| "ubuntu:22.04".to_string());
            Ok(EnvironmentConfig {
                r#type: EnvironmentType::Docker,
                image: Some(image),
                cwd,
                timeout: timeout_ms.max(5_000),
                task_id: "omiga-docker-session".to_string(),
                ..Default::default()
            })
        }
        "modal" | "daytona" => Err(ToolError::ExecutionFailed {
            message: format!(
                "沙箱后端 '{}' 暂未开放；请选择 docker 或 singularity。",
                backend
            ),
        }),
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
            message: format!("未知沙箱后端: '{}' (支持: docker, singularity)", backend),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ssh_remote_path_uses_session_project_root() {
        let ctx = ToolContext::new("/remote/work/data/query")
            .with_execution_environment("ssh")
            .with_ssh_server(Some("gpu".to_string()));

        assert_eq!(remote_path(&ctx, "."), "/remote/work/data/query");
        assert_eq!(
            remote_path(&ctx, ".omiga/runs/oprun_123"),
            "/remote/work/data/query/.omiga/runs/oprun_123"
        );
        assert_eq!(
            remote_path(&ctx, "./nested/file.txt"),
            "/remote/work/data/query/nested/file.txt"
        );
        assert_eq!(remote_path(&ctx, "/tmp/file.txt"), "/tmp/file.txt");
    }

    #[test]
    fn sandbox_remote_path_keeps_workspace_root() {
        let ctx = ToolContext::new("/local/project")
            .with_execution_environment("sandbox")
            .with_sandbox_backend("docker");

        assert_eq!(remote_path(&ctx, "."), "/workspace");
        assert_eq!(
            remote_path(&ctx, ".omiga/runs/oprun_123"),
            "/workspace/.omiga/runs/oprun_123"
        );
    }
}
