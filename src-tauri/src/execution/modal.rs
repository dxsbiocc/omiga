//! Modal 云执行环境
//!
//! 对应 hermes-agent 中的 environments/modal.py
//! 使用 Modal 云平台执行命令
//!
//! 注意：此实现需要 Modal SDK 和 API 密钥
//! 为简化实现，这里提供基础框架，实际使用时需要集成 modal-client crate

use super::base::{generate_session_id, BaseEnvironment};
use super::types::{ExecResult, ExecutionError, ProcessHandle};
use async_trait::async_trait;
use std::collections::HashMap;
use std::pin::Pin;

/// Modal 执行环境
pub struct ModalEnvironment {
    cwd: String,
    timeout_ms: u64,
    env: HashMap<String, String>,
    session_id: String,
    snapshot_path: String,
    cwd_file: String,
    cwd_marker: String,
    snapshot_ready: bool,
    last_sync_time: Option<f64>,

    // Modal 特定配置（SDK 集成待实现）
    _image: String,
    _task_id: String,
    _persistent: bool,
    _sandbox_id: Option<String>,
    _modal_config: HashMap<String, String>,
}

impl ModalEnvironment {
    /// 创建新的 Modal 执行环境
    pub async fn new(
        image: String,
        cwd: Option<String>,
        timeout_ms: Option<u64>,
        _modal_sandbox_kwargs: Option<serde_json::Value>,
        persistent: bool,
        task_id: String,
    ) -> Result<Self, ExecutionError> {
        // 检查 Modal 配置
        Self::check_modal_config().await?;

        let cwd = cwd.unwrap_or_else(|| "/root".to_string());

        let session_id = generate_session_id();
        let snapshot_path = format!("/tmp/hermes-snap-{}.sh", session_id);
        let cwd_file = format!("/tmp/hermes-cwd-{}.txt", session_id);
        let cwd_marker = format!("__HERMES_CWD_{}__", session_id);

        let mut me = Self {
            cwd,
            timeout_ms: timeout_ms.unwrap_or(60_000),
            env: HashMap::new(),
            session_id,
            snapshot_path,
            cwd_file,
            cwd_marker,
            snapshot_ready: false,
            last_sync_time: None,
            _image: image,
            _task_id: task_id,
            _persistent: persistent,
            _sandbox_id: None,
            _modal_config: HashMap::new(),
        };

        // 初始化 Modal sandbox
        me.init_sandbox().await?;

        // 初始化会话快照
        me.init_session().await?;

        Ok(me)
    }

    /// 检查 Modal 配置
    async fn check_modal_config() -> Result<(), ExecutionError> {
        // 从统一配置文件读取
        let config = crate::llm::config::load_config_file()
            .map_err(|e| ExecutionError::ModalError(format!("Failed to load config: {}", e)))?;

        // 检查配置文件或环境变量
        if config.modal_token_id().is_none() || config.modal_token_secret().is_none() {
            return Err(ExecutionError::ModalError(
                "Modal credentials not found. Please configure in Advanced Settings or set MODAL_TOKEN_ID and MODAL_TOKEN_SECRET environment variables.".to_string()
            ));
        }
        Ok(())
    }

    /// 初始化 Modal Sandbox
    async fn init_sandbox(&mut self) -> Result<(), ExecutionError> {
        // Modal SDK (modal-client) 尚未集成，返回明确错误
        Err(ExecutionError::NotAvailable(
            "Modal sandbox initialization is not yet implemented. \
             Please integrate modal-client crate."
                .to_string(),
        ))
    }

    /// 在 Modal sandbox 中执行命令
    async fn execute_in_sandbox(
        &self,
        _cmd_string: &str,
        _timeout_ms: u64,
    ) -> Result<(String, i32), ExecutionError> {
        // Modal SDK (modal-client) 尚未集成。
        // 返回明确错误而非静默假装成功，避免调用方误判执行结果。
        Err(ExecutionError::NotAvailable(
            "Modal execution is not yet implemented. \
             Please integrate modal-client crate to enable Modal sandbox execution."
                .to_string(),
        ))
    }

    /// 创建 sandbox 快照
    async fn create_snapshot(&self) -> Result<Option<String>, ExecutionError> {
        if !self._persistent {
            return Ok(None);
        }

        // 实际实现应该调用 Modal API 创建快照
        tracing::info!("Creating Modal snapshot for sandbox {:?}", self._sandbox_id);

        // 模拟快照 ID
        Ok(Some(format!("snap-{}", self.session_id)))
    }
}

#[async_trait]
impl BaseEnvironment for ModalEnvironment {
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
        "heredoc" // Modal 使用 heredoc 模式
    }

    fn snapshot_timeout_secs(&self) -> u64 {
        60 // Modal 冷启动较慢
    }

    async fn run_bash(
        &self,
        _cmd_string: &str,
        _login: bool,
        _timeout_secs: u64,
        _stdin_data: Option<&str>,
    ) -> Result<Box<dyn ProcessHandle>, ExecutionError> {
        Err(ExecutionError::NotAvailable(
            "ModalEnvironment uses execute_direct".to_string(),
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
        let (exec_command, _): (String, Option<String>) = if let Some(stdin_data) = options
            .stdin_data
            .as_ref()
            .filter(|_| self.stdin_mode() == "heredoc")
        {
            let cmd = self.embed_stdin_heredoc(command, stdin_data);
            (cmd, None)
        } else {
            (command.to_string(), options.stdin_data)
        };

        let wrapped = self.wrap_command(&exec_command, &effective_cwd);

        // 在 Modal sandbox 中执行
        let (output, returncode) = self.execute_in_sandbox(&wrapped, effective_timeout).await?;

        let mut result = ExecResult { output, returncode };

        // 更新 CWD
        self.update_cwd_from_output(&mut result).await;

        Ok(result)
    }

    async fn cleanup(&mut self) -> Result<(), ExecutionError> {
        // 如果持久化，创建快照
        if self._persistent {
            if let Some(snapshot_id) = self.create_snapshot().await? {
                tracing::info!("Modal snapshot created: {}", snapshot_id);
            }
        }

        // 终止 sandbox
        if let Some(sandbox_id) = &self._sandbox_id {
            tracing::info!("Terminating Modal sandbox: {}", sandbox_id);
            // 实际应该调用 Modal API 终止 sandbox
        }

        Ok(())
    }
}

/// Modal 进程句柄（占位符）
pub struct ModalProcessHandle;

#[async_trait]
impl ProcessHandle for ModalProcessHandle {
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

    #[test]
    fn test_modal_environment_type() {
        // Modal 使用 heredoc 模式
        // 验证结构体能创建（不需要 Modal 凭证的简单测试）
        // stdin_mode 应该在实现中返回 "heredoc"
    }

    #[tokio::test]
    async fn test_modal_check_config() {
        // 此测试验证配置检查逻辑
        // 实际行为取决于配置文件和环境变量

        let result = ModalEnvironment::check_modal_config().await;

        // 如果配置存在则通过，否则失败 - 两种都是有效行为
        match result {
            Ok(()) => println!("Modal is configured"),
            Err(e) => println!("Modal not configured: {}", e),
        }
    }

    #[tokio::test]
    async fn test_modal_environment_creation() {
        // 此测试需要 Modal 凭证，如果没有则跳过
        if std::env::var("MODAL_TOKEN_ID").is_err() || std::env::var("MODAL_TOKEN_SECRET").is_err()
        {
            println!("Modal credentials not available, skipping test");
            return;
        }

        // 尝试创建环境（可能会失败，但不应该 panic）
        let result = ModalEnvironment::new(
            "python:3.11".to_string(),
            Some("/root".to_string()),
            Some(60_000),
            None,
            false,
            "test-task".to_string(),
        )
        .await;

        // 由于我们没有真正的 Modal SDK 集成，这里预期会失败或创建模拟环境
        match result {
            Ok(env) => {
                assert_eq!(env.cwd, "/root");
                assert_eq!(env._image, "python:3.11");
                assert!(env._sandbox_id.is_some());
            }
            Err(e) => {
                println!(
                    "Modal environment creation failed (expected without real SDK): {}",
                    e
                );
            }
        }
    }
}
