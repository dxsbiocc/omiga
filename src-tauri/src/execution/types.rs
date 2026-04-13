//! 执行环境共享类型定义
//!
//! 对应 hermes-agent 中的 environments/base.py 类型定义

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::pin::Pin;
use tokio::io::AsyncRead;

/// 执行结果
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExecResult {
    pub output: String,
    pub returncode: i32,
}

impl ExecResult {
    pub fn new(output: impl Into<String>, returncode: i32) -> Self {
        Self {
            output: output.into(),
            returncode,
        }
    }

    pub fn success(&self) -> bool {
        self.returncode == 0
    }
}

/// 执行选项
#[derive(Debug, Clone, Default)]
pub struct ExecOptions {
    pub timeout: Option<u64>, // 毫秒
    pub stdin_data: Option<String>,
    pub cwd: Option<String>,
}

impl ExecOptions {
    pub fn with_timeout(timeout_ms: u64) -> Self {
        Self {
            timeout: Some(timeout_ms),
            ..Default::default()
        }
    }
}

/// 环境类型
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum EnvironmentType {
    Local,
    Docker,
    Modal,
    Daytona,
    Ssh,
    Singularity,
}

impl std::fmt::Display for EnvironmentType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EnvironmentType::Local => write!(f, "local"),
            EnvironmentType::Docker => write!(f, "docker"),
            EnvironmentType::Modal => write!(f, "modal"),
            EnvironmentType::Daytona => write!(f, "daytona"),
            EnvironmentType::Ssh => write!(f, "ssh"),
            EnvironmentType::Singularity => write!(f, "singularity"),
        }
    }
}

/// 环境配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvironmentConfig {
    pub r#type: EnvironmentType,
    pub image: Option<String>,
    pub cwd: String,
    pub timeout: u64, // 毫秒

    // 容器资源限制
    #[serde(default)]
    pub cpu: f64,
    #[serde(default)]
    pub memory: u64, // MB
    #[serde(default)]
    pub disk: u64,   // MB

    // Docker 特定
    #[serde(default)]
    pub volumes: Vec<String>,
    #[serde(default)]
    pub forward_env: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default = "default_true")]
    pub network: bool,

    // SSH 特定
    pub ssh_host: Option<String>,
    pub ssh_user: Option<String>,
    #[serde(default = "default_ssh_port")]
    pub ssh_port: u16,
    pub ssh_key_path: Option<String>,

    // 云环境特定
    #[serde(default = "default_true")]
    pub persistent_filesystem: bool,

    // Modal 特定
    pub modal_sandbox_kwargs: Option<serde_json::Value>,

    // 任务标识
    #[serde(default = "default_task_id")]
    pub task_id: String,
}

fn default_true() -> bool {
    true
}
fn default_ssh_port() -> u16 {
    22
}
fn default_task_id() -> String {
    "default".to_string()
}

impl Default for EnvironmentConfig {
    fn default() -> Self {
        Self {
            r#type: EnvironmentType::Local,
            image: None,
            cwd: std::env::current_dir()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| ".".to_string()),
            timeout: 120_000, // 120秒
            cpu: 0.0,
            memory: 0,
            disk: 0,
            volumes: vec![],
            forward_env: vec![],
            env: HashMap::new(),
            network: true,
            ssh_host: None,
            ssh_user: None,
            ssh_port: 22,
            ssh_key_path: None,
            persistent_filesystem: true,
            modal_sandbox_kwargs: None,
            task_id: "default".to_string(),
        }
    }
}

impl EnvironmentConfig {
    /// 创建本地环境配置
    pub fn local(cwd: impl Into<String>) -> Self {
        Self {
            r#type: EnvironmentType::Local,
            cwd: cwd.into(),
            ..Default::default()
        }
    }

    /// 创建 Docker 环境配置
    pub fn docker(image: impl Into<String>, cwd: impl Into<String>) -> Self {
        Self {
            r#type: EnvironmentType::Docker,
            image: Some(image.into()),
            cwd: cwd.into(),
            ..Default::default()
        }
    }
}

/// 进程句柄 trait
///
/// 对应 hermes-agent 中的 ProcessHandle Protocol
#[async_trait::async_trait]
pub trait ProcessHandle: Send + Sync {
    /// 检查进程是否完成，返回退出码或 None
    fn poll(&self) -> Option<i32>;

    /// 终止进程
    fn kill(&self);

    /// 等待进程完成
    async fn wait(&self) -> i32;

    /// 获取 stdout 流
    fn stdout(&mut self) -> Option<Pin<Box<dyn AsyncRead + Send + '_>>>;

    /// 获取 stderr 流
    fn stderr(&mut self) -> Option<Pin<Box<dyn AsyncRead + Send + '_>>>;

    /// 获取当前退出码
    fn returncode(&self) -> Option<i32>;
}

/// 执行错误类型
#[derive(Debug, thiserror::Error)]
pub enum ExecutionError {
    #[error("Failed to spawn process: {0}")]
    SpawnError(String),

    #[error("Command timeout after {timeout_ms}ms")]
    Timeout { timeout_ms: u64 },

    #[error("Command cancelled")]
    Cancelled,

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Environment not available: {0}")]
    NotAvailable(String),

    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),

    #[error("Docker error: {0}")]
    DockerError(String),

    #[error("SSH error: {0}")]
    SshError(String),

    #[error("Modal error: {0}")]
    ModalError(String),

    #[error("Daytona error: {0}")]
    DaytonaError(String),

    #[error("Singularity error: {0}")]
    SingularityError(String),
}

/// 沙盒目录配置
pub fn get_sandbox_dir() -> std::path::PathBuf {
    std::env::var("TERMINAL_SANDBOX_DIR")
        .ok()
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
                .join(".hermes")
                .join("sandboxes")
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exec_result_success() {
        let result = ExecResult::new("output", 0);
        assert!(result.success());

        let result = ExecResult::new("error", 1);
        assert!(!result.success());
    }

    #[test]
    fn test_environment_type_display() {
        assert_eq!(EnvironmentType::Local.to_string(), "local");
        assert_eq!(EnvironmentType::Docker.to_string(), "docker");
    }

    #[test]
    fn test_env_config_default() {
        let config = EnvironmentConfig::default();
        assert_eq!(config.r#type, EnvironmentType::Local);
        assert_eq!(config.timeout, 120_000);
        assert!(config.network);
        assert!(config.persistent_filesystem);
    }

    #[test]
    fn test_env_config_local() {
        let config = EnvironmentConfig::local("/tmp");
        assert_eq!(config.r#type, EnvironmentType::Local);
        assert_eq!(config.cwd, "/tmp");
    }

    #[test]
    fn test_env_config_docker() {
        let config = EnvironmentConfig::docker("ubuntu:22.04", "/workspace");
        assert_eq!(config.r#type, EnvironmentType::Docker);
        assert_eq!(config.image, Some("ubuntu:22.04".to_string()));
        assert_eq!(config.cwd, "/workspace");
    }
}
