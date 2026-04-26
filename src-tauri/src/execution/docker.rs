//! Docker 执行环境
//!
//! 对应 hermes-agent 中的 environments/docker.py
//! 在 Docker 容器中执行命令，提供隔离的执行环境

use super::base::{generate_session_id, BaseEnvironment};
use super::types::{ExecResult, ExecutionError, ProcessHandle};
use async_trait::async_trait;
use std::collections::HashMap;
use std::path::PathBuf;
use std::pin::Pin;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

/// Docker 安全参数
/// 对应 hermes-agent 中的 _SECURITY_ARGS
const SECURITY_ARGS: &[&str] = &[
    "--cap-drop",
    "ALL",
    "--cap-add",
    "DAC_OVERRIDE",
    "--cap-add",
    "CHOWN",
    "--cap-add",
    "FOWNER",
    "--security-opt",
    "no-new-privileges",
    "--pids-limit",
    "256",
    "--tmpfs",
    "/tmp:rw,nosuid,size=512m",
    "--tmpfs",
    "/var/tmp:rw,noexec,nosuid,size=256m",
];

/// Docker 可执行文件搜索路径
const DOCKER_SEARCH_PATHS: &[&str] = &[
    "/usr/local/bin/docker",
    "/opt/homebrew/bin/docker",
    "/Applications/Docker.app/Contents/Resources/bin/docker",
    "/usr/bin/docker",
    "/bin/docker",
];

/// Docker 执行环境
pub struct DockerEnvironment {
    cwd: String,
    timeout_ms: u64,
    env: HashMap<String, String>,
    session_id: String,
    snapshot_path: String,
    cwd_file: String,
    cwd_marker: String,
    snapshot_ready: bool,
    last_sync_time: Option<f64>,

    // Docker 特定配置
    image: String,
    container_id: Option<String>,
    docker_exe: String,
    workspace_dir: Option<PathBuf>,
    home_dir: Option<PathBuf>,
    init_env_args: Vec<String>,
    persistent: bool,
    task_id: String,
}

impl DockerEnvironment {
    /// 创建新的 Docker 执行环境
    pub async fn new(
        image: String,
        cwd: Option<String>,
        timeout_ms: Option<u64>,
        cpu: Option<f64>,
        memory: Option<u64>,
        disk: Option<u64>,
        persistent: bool,
        task_id: String,
        volumes: Vec<String>,
        forward_env: Vec<String>,
        env_vars: HashMap<String, String>,
        network: bool,
    ) -> Result<Self, ExecutionError> {
        let docker_exe = Self::find_docker().await?;
        let cwd = cwd.unwrap_or_else(|| "/root".to_string());

        // 验证 Docker 可用
        Self::ensure_docker_available(&docker_exe).await?;

        let session_id = generate_session_id();
        let snapshot_path = format!("/tmp/hermes-snap-{}.sh", session_id);
        let cwd_file = format!("/tmp/hermes-cwd-{}.txt", session_id);
        let cwd_marker = format!("__HERMES_CWD_{}__", session_id);

        let mut me = Self {
            cwd,
            timeout_ms: timeout_ms.unwrap_or(60_000),
            env: env_vars.clone(),
            session_id,
            snapshot_path,
            cwd_file,
            cwd_marker,
            snapshot_ready: false,
            last_sync_time: None,
            image,
            container_id: None,
            docker_exe,
            workspace_dir: None,
            home_dir: None,
            init_env_args: Vec::new(),
            persistent,
            task_id,
        };

        // 初始化容器
        me.init_container(cpu, memory, disk, &volumes, &forward_env, network)
            .await?;

        // 初始化会话快照
        me.init_session().await?;

        Ok(me)
    }

    /// 查找 Docker 可执行文件
    async fn find_docker() -> Result<String, ExecutionError> {
        // 首先检查 PATH
        if let Ok(output) = Command::new("which").arg("docker").output().await {
            if output.status.success() {
                let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !path.is_empty() {
                    return Ok(path);
                }
            }
        }

        // 检查常见路径
        for path in DOCKER_SEARCH_PATHS {
            if tokio::fs::metadata(path).await.is_ok() {
                return Ok(path.to_string());
            }
        }

        Err(ExecutionError::DockerError(
            "Docker executable not found in PATH or common locations".to_string(),
        ))
    }

    /// 验证 Docker 可用
    async fn ensure_docker_available(docker_exe: &str) -> Result<(), ExecutionError> {
        match Command::new(docker_exe).arg("version").output().await {
            Ok(output) if output.status.success() => Ok(()),
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                Err(ExecutionError::DockerError(format!(
                    "Docker daemon not available: {}",
                    stderr
                )))
            }
            Err(e) => Err(ExecutionError::DockerError(format!(
                "Failed to run docker: {}",
                e
            ))),
        }
    }

    /// 初始化 Docker 容器
    async fn init_container(
        &mut self,
        cpu: Option<f64>,
        memory: Option<u64>,
        _disk: Option<u64>,
        volumes: &[String],
        forward_env: &[String],
        network: bool,
    ) -> Result<(), ExecutionError> {
        // 构建资源限制参数
        let mut resource_args: Vec<String> = Vec::new();
        if let Some(cpu) = cpu {
            resource_args.push("--cpus".to_string());
            resource_args.push(cpu.to_string());
        }
        if let Some(memory) = memory {
            resource_args.push("--memory".to_string());
            resource_args.push(format!("{}m", memory));
        }
        if !network {
            resource_args.push("--network=none".to_string());
        }

        // 持久化存储
        let mut writable_args: Vec<String> = Vec::new();
        if self.persistent {
            let sandbox_dir = super::types::get_sandbox_dir()
                .join("docker")
                .join(&self.task_id);

            self.home_dir = Some(sandbox_dir.join("home"));
            self.workspace_dir = Some(sandbox_dir.join("workspace"));

            tokio::fs::create_dir_all(self.home_dir.as_ref().unwrap())
                .await
                .ok();
            tokio::fs::create_dir_all(self.workspace_dir.as_ref().unwrap())
                .await
                .ok();

            writable_args.push("-v".to_string());
            writable_args.push(format!(
                "{}:/root",
                self.home_dir.as_ref().unwrap().display()
            ));
            writable_args.push("-v".to_string());
            writable_args.push(format!(
                "{}:/workspace",
                self.workspace_dir.as_ref().unwrap().display()
            ));
        } else {
            writable_args.push("--tmpfs".to_string());
            writable_args.push("/workspace:rw,exec,size=10g".to_string());
            writable_args.push("--tmpfs".to_string());
            writable_args.push("/home:rw,exec,size=1g".to_string());
            writable_args.push("--tmpfs".to_string());
            writable_args.push("/root:rw,exec,size=1g".to_string());
        }

        // 卷挂载
        let mut volume_args: Vec<String> = Vec::new();
        for vol in volumes {
            volume_args.push("-v".to_string());
            volume_args.push(vol.clone());
        }

        // 环境变量
        let mut env_args: Vec<String> = Vec::new();
        for (key, value) in &self.env {
            env_args.push("-e".to_string());
            env_args.push(format!("{}={}", key, value));
        }

        // 生成容器名称
        let container_name = format!("hermes-{}", &self.session_id[..8]);
        let container_name_for_log = container_name.clone();

        // 构建运行命令
        let mut run_cmd: Vec<String> = vec![
            self.docker_exe.clone(),
            "run".to_string(),
            "-d".to_string(),
            "--name".to_string(),
            container_name,
            "-w".to_string(),
            self.cwd.clone(),
        ];

        // 添加安全参数
        for arg in SECURITY_ARGS {
            run_cmd.push(arg.to_string());
        }

        // 添加可写参数
        for arg in &writable_args {
            run_cmd.push(arg.clone());
        }

        // 添加资源参数
        for arg in &resource_args {
            run_cmd.push(arg.clone());
        }

        // 添加卷参数
        for arg in &volume_args {
            run_cmd.push(arg.clone());
        }

        // 添加环境变量
        for arg in &env_args {
            run_cmd.push(arg.clone());
        }

        // 添加镜像和命令
        run_cmd.push(self.image.clone());
        run_cmd.push("sleep".to_string());
        run_cmd.push("2h".to_string());

        tracing::info!("Starting Docker container: {}", run_cmd.join(" "));

        let output = Command::new(&run_cmd[0])
            .args(&run_cmd[1..])
            .output()
            .await
            .map_err(|e| {
                ExecutionError::DockerError(format!("Failed to start container: {}", e))
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ExecutionError::DockerError(format!(
                "Failed to start container: {}",
                stderr
            )));
        }

        self.container_id = Some(String::from_utf8_lossy(&output.stdout).trim().to_string());
        self.init_env_args = self.build_init_env_args(forward_env);

        tracing::info!(
            "Started container {} (id: {})",
            container_name_for_log,
            self.container_id.as_ref().unwrap()
        );

        Ok(())
    }

    /// 构建初始化环境变量参数
    fn build_init_env_args(&self, forward_env: &[String]) -> Vec<String> {
        let mut args: Vec<String> = Vec::new();
        for key in forward_env {
            if let Ok(value) = std::env::var(key) {
                args.push("-e".to_string());
                args.push(format!("{}={}", key, value));
            }
        }
        args
    }

    /// 在容器中执行命令（直接方式）
    async fn execute_in_container(
        &self,
        cmd_string: &str,
        timeout_ms: u64,
    ) -> Result<(String, i32), ExecutionError> {
        let container_id = self
            .container_id
            .as_ref()
            .ok_or_else(|| ExecutionError::DockerError("Container not started".to_string()))?;

        // 构建 docker exec 命令
        let mut exec_cmd: Vec<String> = vec![self.docker_exe.clone(), "exec".to_string()];

        // 添加环境变量参数
        for arg in &self.init_env_args {
            exec_cmd.push(arg.clone());
        }

        exec_cmd.push(container_id.clone());
        exec_cmd.push("bash".to_string());
        exec_cmd.push("-c".to_string());
        exec_cmd.push(cmd_string.to_string());

        use tokio::time::{timeout, Duration};

        let result = timeout(Duration::from_millis(timeout_ms), async {
            let mut child = Command::new(&exec_cmd[0])
                .args(&exec_cmd[1..])
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .map_err(|e| ExecutionError::DockerError(format!("Failed to exec: {}", e)))?;

            // 排空 stderr，防止管道缓冲区满导致容器进程阻塞
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
            Err(_) => {
                // 超时
                Ok((format!("\n[Command timed out after {}ms]", timeout_ms), 124))
            }
        }
    }
}

#[async_trait]
impl BaseEnvironment for DockerEnvironment {
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
        "pipe"
    }

    fn snapshot_timeout_secs(&self) -> u64 {
        60 // Docker 冷启动可能较慢
    }

    async fn run_bash(
        &self,
        _cmd_string: &str,
        _login: bool,
        _timeout_secs: u64,
        _stdin_data: Option<&str>,
    ) -> Result<Box<dyn ProcessHandle>, ExecutionError> {
        // 使用 execute_direct 方式
        Err(ExecutionError::NotAvailable(
            "DockerEnvironment uses execute_direct".to_string(),
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

        // 准备命令：heredoc 模式将 stdin 嵌入命令，pipe 模式暂不支持（exec 不接受 stdin）
        let exec_command = if let Some(stdin_data) = options.stdin_data.as_ref() {
            self.embed_stdin_heredoc(command, stdin_data)
        } else {
            command.to_string()
        };

        let wrapped = self.wrap_command(&exec_command, &effective_cwd);

        // 在容器中执行
        let (output, returncode) = self
            .execute_in_container(&wrapped, effective_timeout)
            .await?;

        let mut result = ExecResult { output, returncode };

        // 更新 CWD
        self.update_cwd_from_output(&mut result).await;

        Ok(result)
    }

    async fn cleanup(&mut self) -> Result<(), ExecutionError> {
        if let Some(container_id) = &self.container_id {
            tracing::info!("Stopping container {}", container_id);

            // 停止容器
            let _ = Command::new(&self.docker_exe)
                .args(["stop", "-t", "60", container_id])
                .output()
                .await;

            if !self.persistent {
                // 删除容器
                let _ = Command::new(&self.docker_exe)
                    .args(["rm", "-f", container_id])
                    .output()
                    .await;

                // 清理目录
                if let Some(dir) = &self.workspace_dir {
                    let _ = tokio::fs::remove_dir_all(dir).await;
                }
                if let Some(dir) = &self.home_dir {
                    let _ = tokio::fs::remove_dir_all(dir).await;
                }
            }
        }
        Ok(())
    }
}

impl Drop for DockerEnvironment {
    fn drop(&mut self) {
        if let Some(container_id) = self.container_id.take() {
            let docker_exe = self.docker_exe.clone();
            // 在 Drop 中使用 std::process::Command（非异步），确保进程退出时容器被清理
            let _ = std::process::Command::new(&docker_exe)
                .args(["rm", "-f", &container_id])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn();
        }
    }
}

/// Docker 进程句柄（占位符）
pub struct DockerProcessHandle;

#[async_trait]
impl ProcessHandle for DockerProcessHandle {
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
    use crate::execution::{create_environment, EnvironmentConfig, ExecOptions};

    /// 检查 Docker 是否可用
    async fn docker_available() -> bool {
        match Command::new("docker").arg("version").output().await {
            Ok(output) => output.status.success(),
            Err(_) => false,
        }
    }

    #[tokio::test]
    async fn test_docker_env_basic() {
        if !docker_available().await {
            println!("Docker not available, skipping test");
            return;
        }

        let config = EnvironmentConfig {
            r#type: super::super::EnvironmentType::Docker,
            image: Some("alpine:latest".to_string()),
            cwd: "/root".to_string(),
            timeout: 60_000,
            ..Default::default()
        };

        let env = create_environment(config).await;
        if env.is_err() {
            println!("Failed to create Docker environment: {:?}", env.err());
            return; // 如果 Docker 不可用则跳过
        }

        let env = env.unwrap();
        let result = {
            let mut guard = env.lock().await;
            guard.execute("echo hello", ExecOptions::default()).await
        };

        assert!(result.is_ok(), "Execute failed: {:?}", result.err());
        let result = result.unwrap();
        assert!(
            result.success() || result.output.contains("hello"),
            "Command failed: exit_code={}, output={}",
            result.returncode,
            result.output
        );

        {
            let mut guard = env.lock().await;
            guard.cleanup().await.ok();
        }
    }

    #[tokio::test]
    async fn test_docker_find_executable() {
        // 测试查找 Docker 可执行文件
        let result = DockerEnvironment::find_docker().await;
        // 在 CI 环境中可能没有 Docker，所以接受失败
        match result {
            Ok(path) => {
                assert!(path.contains("docker"));
            }
            Err(_) => {
                println!("Docker not found, skipping assertions");
            }
        }
    }

    #[test]
    fn test_docker_security_args() {
        // 验证安全参数包含关键选项
        assert!(SECURITY_ARGS.contains(&"--cap-drop"));
        assert!(SECURITY_ARGS.contains(&"ALL"));
        assert!(SECURITY_ARGS.contains(&"--security-opt"));
        assert!(SECURITY_ARGS.contains(&"no-new-privileges"));
    }
}
