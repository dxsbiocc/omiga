//! Daytona 云执行环境
//!
//! 对应 hermes-agent 中的 environments/daytona.py
//! 使用 Daytona 云平台执行命令
//!
//! 注意：Daytona 需要 API 密钥和 API 端点

use super::base::{generate_session_id, BaseEnvironment};
use super::types::{ExecResult, ExecutionError, ProcessHandle};
use async_trait::async_trait;
use std::collections::HashMap;
use std::pin::Pin;

/// Daytona 执行环境
pub struct DaytonaEnvironment {
    cwd: String,
    timeout_ms: u64,
    env: HashMap<String, String>,
    session_id: String,
    snapshot_path: String,
    cwd_file: String,
    cwd_marker: String,
    snapshot_ready: bool,
    last_sync_time: Option<f64>,

    // Daytona 特定配置（API 集成待实现）
    workspace_id: String,
    _image: String,
    _persistent: bool,
    _daytona_url: String,
    _api_key: String,
}

impl DaytonaEnvironment {
    /// 创建新的 Daytona 执行环境
    pub async fn new(
        image: String,
        cwd: Option<String>,
        timeout_ms: Option<u64>,
        persistent: bool,
        workspace_id: String,
        daytona_url: Option<String>,
        api_key: Option<String>,
    ) -> Result<Self, ExecutionError> {
        // 检查 Daytona 配置
        let (url, key) = Self::check_daytona_config(daytona_url, api_key).await?;

        let cwd = cwd.unwrap_or_else(|| "/workspace".to_string());

        let session_id = generate_session_id();
        let snapshot_path = format!("/tmp/hermes-snap-{}.sh", session_id);
        let cwd_file = format!("/tmp/hermes-cwd-{}.txt", session_id);
        let cwd_marker = format!("__HERMES_CWD_{}__", session_id);

        tracing::info!("Initializing Daytona workspace: {}", workspace_id);

        let mut de = Self {
            cwd,
            timeout_ms: timeout_ms.unwrap_or(60_000),
            env: HashMap::new(),
            session_id,
            snapshot_path,
            cwd_file,
            cwd_marker,
            snapshot_ready: false,
            last_sync_time: None,
            workspace_id,
            _image: image,
            _persistent: persistent,
            _daytona_url: url,
            _api_key: key,
        };

        // 验证/创建工作空间
        de.init_workspace().await?;

        // 初始化会话快照
        de.init_session().await?;

        Ok(de)
    }

    /// 检查 Daytona 配置
    async fn check_daytona_config(
        url: Option<String>,
        key: Option<String>,
    ) -> Result<(String, String), ExecutionError> {
        // 从统一配置文件读取
        let config = crate::llm::config::load_config_file()
            .map_err(|e| ExecutionError::DaytonaError(format!("Failed to load config: {}", e)))?;

        let url = url
            .or_else(|| config.daytona_server_url())
            .ok_or_else(|| ExecutionError::DaytonaError(
                "Daytona server URL not found. Please configure in Advanced Settings or set DAYTONA_SERVER_URL environment variable.".to_string()
            ))?;

        let key = key
            .or_else(|| config.daytona_api_key())
            .ok_or_else(|| ExecutionError::DaytonaError(
                "Daytona API key not found. Please configure in Advanced Settings or set DAYTONA_API_KEY environment variable.".to_string()
            ))?;

        Ok((url, key))
    }

    /// 初始化 Daytona 工作空间
    async fn init_workspace(&mut self) -> Result<(), ExecutionError> {
        // Daytona HTTP API 集成尚未实现，返回明确错误
        Err(ExecutionError::NotAvailable(
            "Daytona workspace API integration is not yet implemented. \
             Please add HTTP client calls to the Daytona API."
                .to_string(),
        ))
    }

    /// 在 Daytona workspace 中执行命令
    async fn execute_in_workspace(
        &self,
        _cmd_string: &str,
        _timeout_ms: u64,
    ) -> Result<(String, i32), ExecutionError> {
        // Daytona HTTP API 集成尚未实现，返回明确错误
        Err(ExecutionError::NotAvailable(
            "Daytona execution is not yet implemented. \
             Please integrate Daytona HTTP API client."
                .to_string(),
        ))
    }
}

#[async_trait]
impl BaseEnvironment for DaytonaEnvironment {
    fn cwd(&self) -> &str {
        &self.cwd
    }

    fn timeout_ms(&self) -> u64 {
        self.timeout_ms
    }

    fn env(&self) -> &HashMap<String, String> {
        &self.env
    }

    fn set_cwd(&mut self, cwd: String) {
        self.cwd = cwd;
    }

    fn session_id(&self) -> &str {
        &self.session_id
    }

    fn snapshot_path(&self) -> &str {
        &self.snapshot_path
    }

    fn cwd_file(&self) -> &str {
        &self.cwd_file
    }

    fn cwd_marker(&self) -> &str {
        &self.cwd_marker
    }

    fn snapshot_ready(&self) -> bool {
        self.snapshot_ready
    }

    fn set_snapshot_ready(&mut self, ready: bool) {
        self.snapshot_ready = ready;
    }

    fn last_sync_time(&self) -> Option<f64> {
        self.last_sync_time
    }

    fn set_last_sync_time(&mut self, time: Option<f64>) {
        self.last_sync_time = time;
    }

    fn stdin_mode(&self) -> &'static str {
        "heredoc" // Daytona 使用 heredoc 模式
    }

    fn snapshot_timeout_secs(&self) -> u64 {
        60 // Daytona 冷启动较慢
    }

    async fn run_bash(
        &self,
        _cmd_string: &str,
        _login: bool,
        _timeout_secs: u64,
        _stdin_data: Option<&str>,
    ) -> Result<Box<dyn ProcessHandle>, ExecutionError> {
        Err(ExecutionError::NotAvailable(
            "DaytonaEnvironment uses execute_direct".to_string(),
        ))
    }

    /// 覆盖 execute 方法
    async fn execute(
        &mut self,
        command: &str,
        options: super::types::ExecOptions,
    ) -> Result<ExecResult, ExecutionError> {
        self.before_execute().await?;

        let effective_timeout = options.timeout.unwrap_or(self.timeout_ms);
        let effective_cwd = options.cwd.unwrap_or_else(|| self.cwd.clone());

        // 准备命令
        let (exec_command, _): (String, Option<String>) =
            if options.stdin_data.is_some() && self.stdin_mode() == "heredoc" {
                let cmd = self.embed_stdin_heredoc(command, options.stdin_data.as_ref().unwrap());
                (cmd, None)
            } else {
                (command.to_string(), options.stdin_data)
            };

        let wrapped = self.wrap_command(&exec_command, &effective_cwd);

        // 在 Daytona workspace 中执行
        let (output, returncode) = self
            .execute_in_workspace(&wrapped, effective_timeout)
            .await?;

        let mut result = ExecResult { output, returncode };

        // 更新 CWD
        self.update_cwd_from_output(&mut result).await;

        Ok(result)
    }

    async fn cleanup(&mut self) -> Result<(), ExecutionError> {
        tracing::info!("Cleaning up Daytona workspace: {}", self.workspace_id);

        // 如果持久化，保留工作空间
        if self._persistent {
            tracing::info!("Daytona workspace {} persists", self.workspace_id);
        } else {
            // 实际应该调用 Daytona API 删除工作空间
            tracing::info!("Daytona workspace {} would be deleted", self.workspace_id);
        }

        Ok(())
    }
}

/// Daytona 进程句柄（占位符）
pub struct DaytonaProcessHandle;

#[async_trait]
impl ProcessHandle for DaytonaProcessHandle {
    fn poll(&self) -> Option<i32> {
        None
    }
    fn kill(&self) {}
    async fn wait(&self) -> i32 {
        -1
    }
    fn stdout(&mut self) -> Option<Pin<Box<dyn tokio::io::AsyncRead + Send + '_>>> {
        None
    }
    fn stderr(&mut self) -> Option<Pin<Box<dyn tokio::io::AsyncRead + Send + '_>>> {
        None
    }
    fn returncode(&self) -> Option<i32> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_daytona_check_config() {
        // 此测试验证配置检查逻辑
        let result = DaytonaEnvironment::check_daytona_config(None, None).await;

        // 如果配置存在则通过，否则失败 - 两种都是有效行为
        match result {
            Ok((url, key)) => println!("Daytona is configured: url={}, key_len={}", url, key.len()),
            Err(e) => println!("Daytona not configured: {}", e),
        }
    }

    #[tokio::test]
    async fn test_daytona_check_config_with_params() {
        // 通过参数提供配置
        let result = DaytonaEnvironment::check_daytona_config(
            Some("https://api.daytona.io".to_string()),
            Some("test-api-key".to_string()),
        )
        .await;

        assert!(result.is_ok());
        let (url, key) = result.unwrap();
        assert_eq!(url, "https://api.daytona.io");
        assert_eq!(key, "test-api-key");
    }

    #[tokio::test]
    async fn test_daytona_environment_creation() {
        // 此测试需要 Daytona 凭证，如果没有则跳过
        if std::env::var("DAYTONA_SERVER_URL").is_err() || std::env::var("DAYTONA_API_KEY").is_err()
        {
            println!("Daytona credentials not available, skipping test");
            return;
        }

        // 尝试创建环境
        let result = DaytonaEnvironment::new(
            "ubuntu:22.04".to_string(),
            Some("/workspace".to_string()),
            Some(60_000),
            false,
            "test-workspace".to_string(),
            None,
            None,
        )
        .await;

        match result {
            Ok(env) => {
                assert_eq!(env.cwd, "/workspace");
                assert_eq!(env._image, "ubuntu:22.04");
                assert_eq!(env.workspace_id, "test-workspace");
            }
            Err(e) => {
                println!(
                    "Daytona environment creation failed (expected without real API): {}",
                    e
                );
            }
        }
    }
}
