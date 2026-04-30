//! Singularity/Apptainer 容器执行环境
//!
//! 对应 hermes-agent 中的 environments/singularity.py
//! 使用 Singularity/Apptainer 执行命令
//!
//! Singularity 与 Docker 不同：
//! - 更安全：用户权限执行，不需要 root
//! - 更适合 HPC 环境
//! - 镜像格式为 .sif 文件
//! - 默认挂载 $HOME 和 /tmp

use super::base::{generate_session_id, BaseEnvironment};
use super::types::{ExecResult, ExecutionError, ExternalTerminalCommand, ProcessHandle};
use async_trait::async_trait;
use std::collections::HashMap;
use std::pin::Pin;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::process::Command;

/// Singularity 安全参数
///
/// 对应 hermes-agent 中的 singularity_security_args
fn singularity_security_args() -> Vec<&'static str> {
    vec![
        "--cleanenv", // 清理环境变量
        "--no-home",  // 不挂载 $HOME
        "--contain",  // 限制容器环境
    ]
}

/// Singularity 进程句柄（占位符，execute() 已覆盖为直接执行，此 handle 不实际使用）
pub struct SingularityProcessHandle;

#[async_trait]
impl ProcessHandle for SingularityProcessHandle {
    fn poll(&self) -> Option<i32> {
        None
    }
    fn kill(&self) {}
    async fn wait(&self) -> i32 {
        -1
    }
    fn stdout(&mut self) -> Option<Pin<Box<dyn AsyncRead + Send + '_>>> {
        None
    }
    fn stderr(&mut self) -> Option<Pin<Box<dyn AsyncRead + Send + '_>>> {
        None
    }
    fn returncode(&self) -> Option<i32> {
        None
    }
}

/// Singularity 执行环境
pub struct SingularityEnvironment {
    cwd: String,
    timeout_ms: u64,
    env: HashMap<String, String>,
    session_id: String,
    snapshot_path: String,
    cwd_file: String,
    cwd_marker: String,
    snapshot_ready: bool,
    last_sync_time: Option<f64>,

    // Singularity 特定配置
    image: String,
    singularity_exe: String,
    volumes: Vec<String>,
    network: bool,
}

impl SingularityEnvironment {
    /// 创建新的 Singularity 执行环境
    pub async fn new(
        image: String,
        cwd: Option<String>,
        timeout_ms: Option<u64>,
        volumes: Vec<String>,
        network: bool,
        _task_id: String,
    ) -> Result<Self, ExecutionError> {
        // 查找 singularity/apptainer 可执行文件
        let singularity_exe = Self::find_singularity().await?;

        let cwd = cwd.unwrap_or_else(|| "/workspace".to_string());

        let session_id = generate_session_id();
        let snapshot_path = format!("/tmp/hermes-snap-{}.sh", session_id);
        let cwd_file = format!("/tmp/hermes-cwd-{}.txt", session_id);
        let cwd_marker = format!("__HERMES_CWD_{}__", session_id);

        let mut se = Self {
            cwd,
            timeout_ms: timeout_ms.unwrap_or(60_000),
            env: HashMap::new(),
            session_id,
            snapshot_path,
            cwd_file,
            cwd_marker,
            snapshot_ready: false,
            last_sync_time: None,
            image,
            singularity_exe,
            volumes,
            network,
        };

        // 初始化会话快照
        se.init_session().await?;

        Ok(se)
    }

    /// 查找 singularity/apptainer 可执行文件
    async fn find_singularity() -> Result<String, ExecutionError> {
        for cmd in &["singularity", "apptainer"] {
            match Command::new("which").arg(cmd).output().await {
                Ok(output) if output.status.success() => {
                    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    if !path.is_empty() {
                        return Ok(path);
                    }
                }
                _ => {}
            }
        }
        Err(ExecutionError::SingularityError(
            "Neither singularity nor apptainer found in PATH".to_string(),
        ))
    }

    /// 构建 singularity exec 命令参数
    fn build_exec_args(&self, cmd: &str) -> Vec<String> {
        let mut args = vec!["exec".to_string()];

        // 添加安全参数
        args.extend(singularity_security_args().into_iter().map(String::from));

        // 挂载 volumes
        for vol in &self.volumes {
            args.push("--bind".to_string());
            args.push(vol.clone());
        }

        // 网络选项
        if !self.network {
            args.push("--net".to_string());
            args.push("--network".to_string());
            args.push("none".to_string());
        }

        // 工作目录
        args.push("--pwd".to_string());
        args.push(self.cwd.clone());

        // 环境变量
        for (key, value) in &self.env {
            args.push("--env".to_string());
            args.push(format!("{}={}", key, value));
        }

        // 镜像和命令
        args.push(self.image.clone());
        args.push("bash".to_string());
        args.push("-c".to_string());
        args.push(cmd.to_string());

        args
    }

    /// 在 Singularity 容器中执行命令并直接收集输出
    ///
    /// 替代通过 run_bash + wait_for_process 的路径，直接读取 stdout/stderr
    async fn execute_in_container(
        &self,
        cmd_string: &str,
        timeout_ms: u64,
    ) -> Result<(String, i32), ExecutionError> {
        use tokio::time::{timeout, Duration};

        let args = self.build_exec_args(cmd_string);

        let result = timeout(Duration::from_millis(timeout_ms), async {
            let mut child = Command::new(&self.singularity_exe)
                .args(&args)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .map_err(|e| ExecutionError::SpawnError(e.to_string()))?;

            // 排空 stderr 防止管道阻塞
            if let Some(stderr) = child.stderr.take() {
                tokio::spawn(async move {
                    let reader = BufReader::new(stderr);
                    let mut lines = reader.lines();
                    while let Ok(Some(_)) = lines.next_line().await {}
                });
            }

            let mut stdout_lines: Vec<String> = Vec::new();
            if let Some(stdout) = child.stdout.take() {
                let reader = BufReader::new(stdout);
                let mut lines = reader.lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    stdout_lines.push(line);
                }
            }

            match child.wait().await {
                Ok(status) => Ok((stdout_lines.join("\n"), status.code().unwrap_or(-1))),
                Err(e) => Err(ExecutionError::IoError(e)),
            }
        })
        .await;

        match result {
            Ok(Ok((output, code))) => Ok((output, code)),
            Ok(Err(e)) => Err(e),
            Err(_) => Ok((format!("\n[Command timed out after {}ms]", timeout_ms), 124)),
        }
    }
}

#[async_trait]
impl BaseEnvironment for SingularityEnvironment {
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
        "pipe" // Singularity 使用管道模式
    }

    fn snapshot_timeout_secs(&self) -> u64 {
        30 // Singularity 冷启动比 Docker 快
    }

    fn external_terminal_command(&self) -> Option<ExternalTerminalCommand> {
        let mut args = vec!["shell".to_string()];
        args.extend(singularity_security_args().into_iter().map(String::from));
        for vol in &self.volumes {
            args.push("--bind".to_string());
            args.push(vol.clone());
        }
        if !self.network {
            args.push("--net".to_string());
            args.push("--network".to_string());
            args.push("none".to_string());
        }
        args.push("--pwd".to_string());
        args.push(self.cwd.clone());
        for (key, value) in &self.env {
            args.push("--env".to_string());
            args.push(format!("{}={}", key, value));
        }
        args.push(self.image.clone());

        Some(ExternalTerminalCommand::new(
            self.singularity_exe.clone(),
            args,
            "Singularity 容器",
        ))
    }

    /// Singularity 使用无状态容器（--contain），快照无法在运行间持久化，跳过 init_session
    async fn init_session(&mut self) -> Result<(), ExecutionError> {
        tracing::debug!(
            session_id = %self.session_id,
            "Singularity: skipping session snapshot (stateless --contain mode)"
        );
        // snapshot_ready 保持 false，每条命令以 login shell 方式独立运行
        Ok(())
    }

    async fn run_bash(
        &self,
        _cmd_string: &str,
        _login: bool,
        _timeout_secs: u64,
        _stdin_data: Option<&str>,
    ) -> Result<Box<dyn ProcessHandle>, ExecutionError> {
        // execute() 已覆盖，此路径不应被调用
        Err(ExecutionError::NotAvailable(
            "SingularityEnvironment uses execute_in_container directly".to_string(),
        ))
    }

    /// 覆盖 execute 方法，使用 execute_in_container 直接收集输出，避免 stdout() 返回 None 的问题
    async fn execute(
        &mut self,
        command: &str,
        options: super::types::ExecOptions,
    ) -> Result<ExecResult, ExecutionError> {
        self.before_execute().await?;

        let effective_timeout = options.timeout.unwrap_or(self.timeout_ms);
        let effective_cwd = options.cwd.unwrap_or_else(|| self.cwd.clone());

        let exec_command = if let Some(stdin) = options.stdin_data.as_ref() {
            self.embed_stdin_heredoc(command, stdin)
        } else {
            command.to_string()
        };

        let wrapped = self.wrap_command(&exec_command, &effective_cwd);

        let (output, returncode) = self
            .execute_in_container(&wrapped, effective_timeout)
            .await?;

        let mut result = ExecResult { output, returncode };
        self.update_cwd_from_output(&mut result).await;

        Ok(result)
    }

    async fn cleanup(&mut self) -> Result<(), ExecutionError> {
        tracing::info!("Cleaning up Singularity environment");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_singularity_find_executable() {
        let result = SingularityEnvironment::find_singularity().await;
        // 可能在 CI 环境没有 singularity，所以允许失败
        match result {
            Ok(exe) => {
                assert!(exe.contains("singularity") || exe.contains("apptainer"));
            }
            Err(_) => {
                println!("Singularity/Apptainer not installed, skipping");
            }
        }
    }

    #[tokio::test]
    async fn test_singularity_security_args() {
        let args = singularity_security_args();
        assert!(args.contains(&"--cleanenv"));
        assert!(args.contains(&"--no-home"));
        assert!(args.contains(&"--contain"));
    }

    #[tokio::test]
    async fn test_singularity_build_args() {
        // 检查是否能构建（即使 singularity 未安装）
        let result = SingularityEnvironment::new(
            "docker://ubuntu:22.04".to_string(),
            Some("/workspace".to_string()),
            Some(60_000),
            vec!["/host/data:/data".to_string()],
            false, // no network
            "test-task".to_string(),
        )
        .await;

        // 如果 singularity 未安装会失败
        match result {
            Ok(env) => {
                let args = env.build_exec_args("echo hello");

                // 检查安全参数
                assert!(args.contains(&"--cleanenv".to_string()));
                assert!(args.contains(&"--no-home".to_string()));
                assert!(args.contains(&"--contain".to_string()));

                // 检查 volume
                assert!(args.contains(&"--bind".to_string()));
                assert!(args.contains(&"/host/data:/data".to_string()));

                // 检查网络禁用
                assert!(args.contains(&"--net".to_string()));
                assert!(args.contains(&"none".to_string()));

                // 检查镜像
                assert!(args.contains(&"docker://ubuntu:22.04".to_string()));

                // 检查命令
                assert!(args.contains(&"bash".to_string()));
                assert!(args.contains(&"-c".to_string()));
                assert!(args.contains(&"echo hello".to_string()));
            }
            Err(e) => {
                println!("Singularity not available: {}", e);
            }
        }
    }
}
