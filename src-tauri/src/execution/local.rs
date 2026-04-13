//! 本地执行环境
//!
//! 对应 hermes-agent 中的 environments/local.py
//! 在主机上直接执行命令，使用 subprocess spawn-per-call 模型

use super::base::{generate_session_id, BaseEnvironment};
use super::types::{ExecResult, ExecutionError, ProcessHandle};
use async_trait::async_trait;
use std::collections::HashMap;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

// Hermes 内部环境变量黑名单 — 防止这些变量泄漏到子进程中
lazy_static::lazy_static! {
    static ref HERMES_ENV_BLOCKLIST: HashMap<String, ()> = {
        let mut m = HashMap::new();
        // API Keys
        m.insert("OPENAI_API_KEY".to_string(), ());
        m.insert("OPENAI_BASE_URL".to_string(), ());
        m.insert("OPENAI_API_BASE".to_string(), ());
        m.insert("ANTHROPIC_API_KEY".to_string(), ());
        m.insert("ANTHROPIC_BASE_URL".to_string(), ());
        m.insert("CLAUDE_CODE_OAUTH_TOKEN".to_string(), ());
        m.insert("DEEPSEEK_API_KEY".to_string(), ());
        m.insert("GROQ_API_KEY".to_string(), ());
        m.insert("MISTRAL_API_KEY".to_string(), ());
        m.insert("GOOGLE_API_KEY".to_string(), ());
        m.insert("XAI_API_KEY".to_string(), ());
        // Provider configs
        m.insert("OPENROUTER_API_KEY".to_string(), ());
        m.insert("COHERE_API_KEY".to_string(), ());
        m.insert("FIREWORKS_API_KEY".to_string(), ());
        m.insert("TOGETHER_API_KEY".to_string(), ());
        m.insert("PERPLEXITY_API_KEY".to_string(), ());
        // Messaging
        m.insert("TELEGRAM_HOME_CHANNEL".to_string(), ());
        m.insert("DISCORD_HOME_CHANNEL".to_string(), ());
        m.insert("SLACK_HOME_CHANNEL".to_string(), ());
        m.insert("SIGNAL_HOME_CHANNEL".to_string(), ());
        // Cloud platforms
        m.insert("MODAL_TOKEN_ID".to_string(), ());
        m.insert("MODAL_TOKEN_SECRET".to_string(), ());
        m.insert("DAYTONA_API_KEY".to_string(), ());
        // VCS
        m.insert("GH_TOKEN".to_string(), ());
        m.insert("GITHUB_TOKEN".to_string(), ());
        m
    };
}

/// 标准 PATH 补充
const SANE_PATH: &str = "/opt/homebrew/bin:/opt/homebrew/sbin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";

/// 本地执行环境
pub struct LocalEnvironment {
    cwd: String,
    timeout_ms: u64,
    env: HashMap<String, String>,
    session_id: String,
    snapshot_path: String,
    cwd_file: String,
    cwd_marker: String,
    snapshot_ready: bool,
    last_sync_time: Option<f64>,
    shell_path: String,
}

impl LocalEnvironment {
    /// 创建新的本地执行环境
    pub async fn new(
        cwd: Option<String>,
        timeout_ms: Option<u64>,
        env: Option<HashMap<String, String>>,
    ) -> Result<Self, ExecutionError> {
        let cwd = cwd.unwrap_or_else(|| {
            std::env::current_dir()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| ".".to_string())
        });

        let shell_path = Self::find_bash().await?;

        let session_id = generate_session_id();
        let snapshot_path = std::env::temp_dir()
            .join(format!("hermes-snap-{}.sh", session_id))
            .to_string_lossy()
            .to_string();
        let cwd_file = std::env::temp_dir()
            .join(format!("hermes-cwd-{}.txt", session_id))
            .to_string_lossy()
            .to_string();
        let cwd_marker = format!("__HERMES_CWD_{}__", session_id);

        let mut me = Self {
            cwd,
            timeout_ms: timeout_ms.unwrap_or(60_000),
            env: env.unwrap_or_default(),
            session_id,
            snapshot_path,
            cwd_file,
            cwd_marker,
            snapshot_ready: false,
            last_sync_time: None,
            shell_path,
        };

        // 初始化会话快照
        me.init_session().await?;

        Ok(me)
    }

    /// 查找可用的 bash shell
    ///
    /// 对应 hermes-agent 中的 _find_bash()
    async fn find_bash() -> Result<String, ExecutionError> {
        // 首先检查 SHELL 环境变量
        if let Ok(shell) = std::env::var("SHELL") {
            if shell.contains("bash") || shell.contains("zsh") {
                if Self::check_shell_executable(&shell).await {
                    return Ok(shell);
                }
            }
        }

        // 尝试通过 which 查找
        if let Some(bash_path) = Self::which("bash").await {
            return Ok(bash_path);
        }

        if let Some(zsh_path) = Self::which("zsh").await {
            return Ok(zsh_path);
        }

        // 尝试常见路径
        let candidates = [
            "/opt/homebrew/bin/bash",
            "/opt/homebrew/bin/zsh",
            "/usr/local/bin/bash",
            "/usr/local/bin/zsh",
            "/bin/bash",
            "/bin/zsh",
            "/usr/bin/bash",
            "/usr/bin/zsh",
        ];

        for candidate in &candidates {
            if tokio::fs::metadata(candidate).await.is_ok() {
                return Ok(candidate.to_string());
            }
        }

        Err(ExecutionError::NotAvailable(
            "No suitable shell found. Please install bash or zsh.".to_string(),
        ))
    }

    /// 检查 shell 是否可执行
    async fn check_shell_executable(shell_path: &str) -> bool {
        // 尝试执行 shell --version
        match Command::new(shell_path).arg("--version").output().await {
            Ok(output) => output.status.success(),
            Err(_) => false,
        }
    }

    /// 类似 which 命令的实现（使用 tokio::fs 避免在 async 上下文中阻塞 executor）
    async fn which(command: &str) -> Option<String> {
        let path_env = std::env::var("PATH").unwrap_or_default();
        let paths: Vec<String> = path_env.split(':').map(String::from).collect();

        for path in paths {
            let full_path = std::path::Path::new(&path).join(command);
            if tokio::fs::metadata(&full_path).await.is_ok() {
                return Some(full_path.to_string_lossy().to_string());
            }
        }

        None
    }

    /// 构建运行环境变量
    ///
    /// 对应 hermes-agent 中的 _sanitize_subprocess_env() 和 _make_run_env()
    fn build_run_env(&self) -> HashMap<String, String> {
        let mut run_env: HashMap<String, String> = HashMap::new();

        // 从当前进程环境变量复制，过滤黑名单
        for (key, value) in std::env::vars() {
            // 跳过 Hermes 内部强制变量前缀
            if key.starts_with("_HERMES_FORCE_") {
                continue;
            }
            // 检查是否在黑名单中
            if HERMES_ENV_BLOCKLIST.contains_key(&key) {
                continue;
            }
            run_env.insert(key, value);
        }

        // 添加自定义环境变量
        for (key, value) in &self.env {
            run_env.insert(key.clone(), value.clone());
        }

        // 确保 PATH 包含标准目录
        if let Some(existing_path) = run_env.get("PATH") {
            if !existing_path.contains("/usr/bin") {
                run_env.insert(
                    "PATH".to_string(),
                    format!("{}:{}", existing_path, SANE_PATH),
                );
            }
        } else {
            run_env.insert("PATH".to_string(), SANE_PATH.to_string());
        }

        run_env
    }

    /// 直接执行命令并返回结果（覆盖基类方法以正确处理输出）
    async fn execute_direct(
        &mut self,
        command: &str,
        options: super::types::ExecOptions,
    ) -> Result<ExecResult, ExecutionError> {
        use tokio::time::{timeout, Duration};
        
        self.before_execute().await?;

        let effective_timeout = options.timeout.unwrap_or(self.timeout_ms);
        let effective_cwd = options.cwd.unwrap_or_else(|| self.cwd.clone());

        // 准备命令
        let (exec_command, _effective_stdin): (String, Option<String>) =
            if options.stdin_data.is_some() && self.stdin_mode() == "heredoc" {
                let cmd = self.embed_stdin_heredoc(command, options.stdin_data.as_ref().unwrap());
                (cmd, None)
            } else {
                (command.to_string(), options.stdin_data)
            };

        let wrapped = self.wrap_command(&exec_command, &effective_cwd);
        let login = !self.snapshot_ready;

        // 构建命令参数
        let args: Vec<&str> = if login {
            vec!["-l", "-c", &wrapped]
        } else {
            vec!["-c", &wrapped]
        };

        let run_env = self.build_run_env();

        let mut cmd = Command::new(&self.shell_path);
        cmd.args(&args)
            .env_clear()
            .envs(run_env)
            .current_dir(&effective_cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        #[cfg(unix)]
        cmd.process_group(0);

        let mut child = cmd
            .spawn()
            .map_err(|e| ExecutionError::SpawnError(e.to_string()))?;

        // 启动独立任务排空 stderr，防止管道缓冲区满导致进程阻塞
        if let Some(stderr) = child.stderr.take() {
            tokio::spawn(async move {
                let reader = BufReader::new(stderr);
                let mut lines = reader.lines();
                while let Ok(Some(_)) = lines.next_line().await {}
            });
        }

        // 读取 stdout 并等待进程完成（带超时）
        let timeout_result = timeout(
            Duration::from_millis(effective_timeout),
            async {
                let mut stdout_lines: Vec<String> = Vec::new();

                if let Some(stdout) = child.stdout.take() {
                    let reader = BufReader::new(stdout);
                    let mut lines = reader.lines();
                    while let Ok(Some(line)) = lines.next_line().await {
                        stdout_lines.push(line);
                    }
                }

                match child.wait().await {
                    Ok(status) => (stdout_lines, status.code().unwrap_or(-1)),
                    Err(_) => (stdout_lines, -1),
                }
            }
        ).await;

        let (output, returncode) = match timeout_result {
            Ok((lines, code)) => (lines.join("\n"), code),
            Err(_) => {
                // 超时：杀掉整个进程组，防止孤儿进程
                #[cfg(unix)]
                {
                    use nix::sys::signal::{killpg, Signal};
                    use nix::unistd::Pid;
                    if let Some(pid) = child.id() {
                        let _ = killpg(Pid::from_raw(pid as i32), Signal::SIGKILL);
                    }
                }
                #[cfg(not(unix))]
                child.kill().await.ok();

                (format!("\n[Command timed out after {}ms]", effective_timeout), 124)
            }
        };

        let mut result = ExecResult { output, returncode };

        // 更新 CWD
        self.update_cwd_from_output(&mut result).await;

        Ok(result)
    }
}

#[async_trait]
impl BaseEnvironment for LocalEnvironment {
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
        // Use heredoc so that stdin_data is embedded into the command via <<EOF rather than
        // being piped — execute_direct always sets Stdio::null(), so pipe mode silently drops it.
        "heredoc"
    }

    fn snapshot_timeout_secs(&self) -> u64 {
        30
    }

    fn is_local_filesystem(&self) -> bool {
        true
    }

    async fn run_bash(
        &self,
        _cmd_string: &str,
        _login: bool,
        _timeout_secs: u64,
        _stdin_data: Option<&str>,
    ) -> Result<Box<dyn ProcessHandle>, ExecutionError> {
        // 这个方法在 execute_direct 中不直接使用
        // 为了保持 trait 兼容性，返回一个空的 handle
        Err(ExecutionError::NotAvailable(
            "LocalEnvironment uses execute_direct instead".to_string()
        ))
    }

    /// 覆盖 execute 方法以正确处理输出
    async fn execute(
        &mut self,
        command: &str,
        options: super::types::ExecOptions,
    ) -> Result<ExecResult, ExecutionError> {
        self.execute_direct(command, options).await
    }

    async fn cleanup(&mut self) -> Result<(), ExecutionError> {
        // 清理临时文件
        let _ = tokio::fs::remove_file(&self.snapshot_path).await;
        let _ = tokio::fs::remove_file(&self.cwd_file).await;
        Ok(())
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution::{create_environment, ExecOptions};

    #[tokio::test]
    async fn test_local_env_basic() {
        let config = super::super::EnvironmentConfig::local(std::env::current_dir().unwrap().to_string_lossy());
        let env = create_environment(config).await;
        assert!(env.is_ok());

        let env = env.unwrap();
        let result = {
            let mut guard = env.lock().await;
            guard.execute("echo hello", ExecOptions::default()).await
        };
        assert!(result.is_ok(), "Execute failed: {:?}", result.err());

        let result = result.unwrap();
        assert!(result.success(), "Command failed with exit code: {}", result.returncode);
        assert!(result.output.contains("hello"), "Output doesn't contain 'hello': {}", result.output);

        {
            let mut guard = env.lock().await;
            guard.cleanup().await.ok();
        }
    }

    #[tokio::test]
    async fn test_local_env_cd() {
        let config = super::super::EnvironmentConfig {
            cwd: "/tmp".to_string(),
            ..Default::default()
        };
        let env = create_environment(config).await.unwrap();

        // 执行 pwd 检查当前目录
        let result = {
            let mut guard = env.lock().await;
            guard.execute("pwd", ExecOptions::default()).await
        };
        let result = result.unwrap();
        assert!(result.success(), "Command failed: {}", result.output);
        assert!(result.output.contains("/tmp"), "Output doesn't contain /tmp: {}", result.output);

        {
            let mut guard = env.lock().await;
            guard.cleanup().await.ok();
        }
    }

    #[tokio::test]
    async fn test_local_env_env_var() {
        let mut env_vars = HashMap::new();
        env_vars.insert("TEST_VAR".to_string(), "test_value".to_string());

        let config = super::super::EnvironmentConfig {
            env: env_vars,
            ..Default::default()
        };
        let env = create_environment(config).await.unwrap();

        let result = {
            let mut guard = env.lock().await;
            guard.execute("echo $TEST_VAR", ExecOptions::default()).await
        };
        let result = result.unwrap();
        assert!(result.success(), "Command failed: {}", result.output);
        assert!(result.output.contains("test_value"), "Output doesn't contain test_value: {}", result.output);

        {
            let mut guard = env.lock().await;
            guard.cleanup().await.ok();
        }
    }

    #[tokio::test]
    async fn test_local_env_timeout() {
        let config = super::super::EnvironmentConfig::default();
        let env = create_environment(config).await.unwrap();

        // 执行一个会超时的命令
        let result = {
            let mut guard = env.lock().await;
            guard.execute("sleep 10", ExecOptions::with_timeout(500)).await
        };
        let result = result.unwrap();

        // 检查是否超时
        assert!(result.returncode == 124 || result.output.contains("timed out"), 
            "Expected timeout but got exit code {} with output: {}", result.returncode, result.output);

        {
            let mut guard = env.lock().await;
            guard.cleanup().await.ok();
        }
    }

    #[tokio::test]
    async fn test_local_env_cwd_tracking() {
        let temp_dir = std::env::temp_dir().join(format!("test_{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir(&temp_dir).await.ok();
        let temp_dir = temp_dir.canonicalize().unwrap_or(temp_dir);

        let config = super::super::EnvironmentConfig::default();
        let env = create_environment(config).await.unwrap();

        // 先切换到临时目录
        let result = {
            let mut guard = env.lock().await;
            guard.execute(&format!("cd {}", temp_dir.to_string_lossy()), ExecOptions::default()).await
        };
        let result = result.unwrap();
        assert!(result.success(), "Command failed: {}", result.output);

        // 检查 cwd 是否被跟踪（使用规范化后的路径）
        {
            let guard = env.lock().await;
            let actual_cwd = std::path::Path::new(guard.cwd()).canonicalize().unwrap_or_else(|_| std::path::PathBuf::from(guard.cwd()));
            assert_eq!(actual_cwd, temp_dir);
        }

        {
            let mut guard = env.lock().await;
            guard.cleanup().await.ok();
        }
        tokio::fs::remove_dir(&temp_dir).await.ok();
    }

    #[test]
    fn test_build_run_env_filters_secrets() {
        // 注意：这个测试需要在单线程运行时中运行，因为使用了 std::env
        let env = LocalEnvironment {
            cwd: "/tmp".to_string(),
            timeout_ms: 60_000,
            env: HashMap::new(),
            session_id: "test".to_string(),
            snapshot_path: "/tmp/snap".to_string(),
            cwd_file: "/tmp/cwd".to_string(),
            cwd_marker: "MARKER".to_string(),
            snapshot_ready: false,
            last_sync_time: None,
            shell_path: "/bin/bash".to_string(),
        };

        let run_env = env.build_run_env();

        // 确保黑名单中的变量被过滤
        assert!(!run_env.contains_key("OPENAI_API_KEY"));
        assert!(!run_env.contains_key("ANTHROPIC_API_KEY"));
    }
}
