//! SSH 远程执行环境
//!
//! 对应 hermes-agent 中的 environments/ssh.py
//! 通过 SSH 在远程主机上执行命令
//!
//! 文件同步：对齐 hermes `ssh._sync_files` + `get_cache_directory_mounts` — rsync
//! `~/.omiga` 下凭证、skills、cache 子目录及用户上下文 Markdown 到远端 `~/.omiga`。

use super::base::{generate_session_id, BaseEnvironment};
use super::types::{ExecResult, ExecutionError, ProcessHandle};
use crate::utils::shell::shell_single_quote;
use async_trait::async_trait;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

fn sh_single_quote(s: &str) -> String {
    shell_single_quote(s)
}

/// Escape path for embedding in `rsync -e "ssh ... -i ..."` (POSIX single-quoted).
fn shell_escape_for_rsync_engine(path: &str) -> String {
    shell_single_quote(path)
}

fn load_terminal_credential_rel_paths() -> Vec<String> {
    let Some(cfg_path) = crate::llm::config::find_config_file() else {
        return Vec::new();
    };
    let Ok(cfg) = crate::llm::config::load_config_file_at(&cfg_path) else {
        return Vec::new();
    };
    cfg.terminal.map(|t| t.credential_files).unwrap_or_default()
}

/// Resolve `rel` under `~/.omiga`; reject traversal and non-files (parity with hermes credential_files).
/// 与 hermes `credential_files._CACHE_DIRS` 布局一致（Omiga 根为 `~/.omiga`）。
const OMIGA_CACHE_SUBDIRS: &[&str] = &[
    "cache/documents",
    "cache/images",
    "cache/audio",
    "cache/screenshots",
];

/// 用户级上下文文件（存在则同步单文件，便于远端 shell/工具引用）。
const OMIGA_USER_CONTEXT_FILES: &[&str] = &["SOUL.md", "MEMORY.md", "USER.md", "BOOTSTRAP.md"];

// rsync timeout constants (ms) — differentiated by expected data size.
const RSYNC_TIMEOUT_MKDIR_MS: u64 = 10_000; // mkdir -p: fast remote command
const RSYNC_TIMEOUT_SINGLE_FILE_MS: u64 = 30_000; // single credential / context file
const RSYNC_TIMEOUT_DIR_MS: u64 = 60_000; // skills directory (small text files)
const RSYNC_TIMEOUT_CACHE_DIR_MS: u64 = 90_000; // cache dirs (may contain images/audio)

fn resolve_safe_omiga_file(rel: &str, omiga_home: &Path) -> Option<PathBuf> {
    let t = rel.trim();
    if t.is_empty() || t.starts_with('/') || t.contains("..") {
        return None;
    }
    let p = omiga_home.join(t);
    let meta = std::fs::metadata(&p).ok()?;
    if !meta.is_file() {
        return None;
    }
    let home_canon = omiga_home.canonicalize().ok()?;
    let file_canon = p.canonicalize().ok()?;
    file_canon.strip_prefix(&home_canon).ok()?;
    Some(file_canon)
}

/// SSH 执行环境
pub struct SshEnvironment {
    cwd: String,
    timeout_ms: u64,
    env: HashMap<String, String>,
    session_id: String,
    snapshot_path: String,
    cwd_file: String,
    cwd_marker: String,
    snapshot_ready: bool,
    last_sync_time: Option<f64>,

    // SSH 特定配置
    host: String,
    user: String,
    port: u16,
    key_path: Option<String>,
    remote_home: String,
    control_socket: PathBuf,
    /// 本地项目根；用于 rsync `<root>/.omiga/skills` 覆盖远端同名技能。
    ssh_project_root: Option<PathBuf>,
}

impl SshEnvironment {
    /// 创建新的 SSH 执行环境
    pub async fn new(
        host: String,
        user: String,
        cwd: Option<String>,
        timeout_ms: Option<u64>,
        port: u16,
        key_path: Option<String>,
        ssh_project_root: Option<PathBuf>,
    ) -> Result<Self, ExecutionError> {
        // 检查 SSH 是否可用
        Self::ensure_ssh_available().await?;

        let cwd = cwd.unwrap_or_else(|| "~".to_string());

        let session_id = generate_session_id();
        let snapshot_path = format!("/tmp/hermes-snap-{}.sh", session_id);
        let cwd_file = format!("/tmp/hermes-cwd-{}.txt", session_id);
        let cwd_marker = format!("__HERMES_CWD_{}__", session_id);

        // 创建控制 socket 目录
        // 对 user/host 中的非安全字符进行过滤，防止路径遍历攻击
        let safe_user = user
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
            .collect::<String>();
        let safe_host = host
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '.' || *c == '_')
            .collect::<String>();
        // macOS temp dirs under /var/folders/... are ~95 chars before SSH adds its own suffix,
        // blowing past the 104-byte Unix domain socket limit. Use /tmp on Unix.
        #[cfg(unix)]
        let control_dir = PathBuf::from(format!("/tmp/omiga-ssh-{}", safe_user));
        #[cfg(not(unix))]
        let control_dir = std::env::temp_dir().join("hermes-ssh");
        tokio::fs::create_dir_all(&control_dir).await.ok();
        // 仅当前用户可读写，防止其他用户观察 socket 路径或复用控制连接
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&control_dir, std::fs::Permissions::from_mode(0o700));
        }
        let control_socket = control_dir.join(format!("{}@{}:{}.sock", safe_user, safe_host, port));

        let mut me = Self {
            cwd,
            timeout_ms: timeout_ms.unwrap_or(60_000),
            env: HashMap::new(),
            session_id,
            snapshot_path: snapshot_path.clone(),
            cwd_file: cwd_file.clone(),
            cwd_marker: cwd_marker.clone(),
            snapshot_ready: false,
            last_sync_time: None,
            host: host.clone(),
            user: user.clone(),
            port,
            key_path: key_path.clone(),
            remote_home: format!("/home/{}", user),
            control_socket: control_socket.clone(),
            ssh_project_root,
        };

        // 建立 SSH 连接
        me.establish_connection().await?;

        // 检测远程 home 目录
        me.remote_home = me.detect_remote_home().await;

        // 同步文件（与 hermes ssh._sync_files 一致：rsync skills + credentials）
        me.sync_omiga_files_to_remote().await;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0);
        me.last_sync_time = Some(now);

        // 初始化会话快照
        me.init_session().await?;

        Ok(me)
    }

    /// 检查 SSH 是否可用
    async fn ensure_ssh_available() -> Result<(), ExecutionError> {
        match Command::new("ssh").arg("-V").output().await {
            Ok(_) => Ok(()),
            Err(e) => Err(ExecutionError::SshError(format!(
                "SSH not available: {}. Please install OpenSSH client.",
                e
            ))),
        }
    }

    /// 构建 SSH 命令基础参数
    fn build_ssh_base_args(&self) -> Vec<String> {
        let mut args = vec![
            "-o".to_string(),
            format!("ControlPath={}", self.control_socket.display()),
            "-o".to_string(),
            "ControlMaster=auto".to_string(),
            "-o".to_string(),
            "ControlPersist=300".to_string(),
            "-o".to_string(),
            "BatchMode=yes".to_string(),
            "-o".to_string(),
            // accept-new 首次连接自动信任主机密钥，存在 MITM 风险
            // 使用 yes 要求主机已在 known_hosts 中预注册
            "StrictHostKeyChecking=yes".to_string(),
            "-o".to_string(),
            "ConnectTimeout=10".to_string(),
        ];

        if self.port != 22 {
            args.push("-p".to_string());
            args.push(self.port.to_string());
        }

        if let Some(key) = &self.key_path {
            args.push("-i".to_string());
            args.push(key.clone());
        }

        args
    }

    /// 建立 SSH 连接
    async fn establish_connection(&self) -> Result<(), ExecutionError> {
        let mut args = self.build_ssh_base_args();
        args.push(format!("{}@{}", self.user, self.host));
        args.push("echo 'SSH connection established'".to_string());

        let output = Command::new("ssh")
            .args(&args)
            .output()
            .await
            .map_err(|e| {
                ExecutionError::SshError(format!("Failed to establish SSH connection: {}", e))
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ExecutionError::SshError(format!(
                "SSH connection failed: {}",
                stderr
            )));
        }

        tracing::info!("SSH connection established to {}@{}", self.user, self.host);
        Ok(())
    }

    /// 检测远程 home 目录
    async fn detect_remote_home(&self) -> String {
        let mut args = self.build_ssh_base_args();
        args.push(format!("{}@{}", self.user, self.host));
        args.push("echo $HOME".to_string());

        match Command::new("ssh").args(&args).output().await {
            Ok(output) if output.status.success() => {
                let home = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !home.is_empty() {
                    return home;
                }
            }
            _ => {}
        }

        // 默认回退
        if self.user == "root" {
            "/root".to_string()
        } else {
            format!("/home/{}", self.user)
        }
    }

    /// rsync 使用的 `-e` 参数字符串（经 ControlMaster 复用与 `ssh` 一致的连接）。
    fn rsync_ssh_engine_arg(&self) -> String {
        let mut s = format!(
            "ssh -o ControlPath={} -o ControlMaster=auto",
            shell_escape_for_rsync_engine(&self.control_socket.to_string_lossy())
        );
        if self.port != 22 {
            s.push_str(&format!(" -p {}", self.port));
        }
        if let Some(ref key) = self.key_path {
            s.push_str(&format!(" -i {}", shell_escape_for_rsync_engine(key)));
        }
        s
    }

    /// 在远端执行单行 shell（不经 `wrap_command`，用于 `mkdir`）。
    async fn ssh_remote_raw_cmd(&self, remote_cmd: &str, timeout_ms: u64) {
        let _ = self.execute_remote(remote_cmd, timeout_ms).await;
    }

    /// 将 `~/.omiga` 下凭证与 skills 目录同步到远端 `~/.omiga`（对齐 hermes-agent `ssh._sync_files`）。
    async fn sync_omiga_files_to_remote(&mut self) {
        if Self::ensure_rsync_available().await.is_err() {
            tracing::warn!("rsync not available; skipping SSH file sync. Install rsync for skills/credentials on remote.");
            return;
        }

        let Some(omiga_home) = dirs::home_dir().map(|h| h.join(".omiga")) else {
            tracing::debug!("SSH sync: no home dir");
            return;
        };

        let container_base = format!("{}/.omiga", self.remote_home.trim_end_matches('/'));
        let engine = self.rsync_ssh_engine_arg();
        let dest_prefix = format!("{}@{}", self.user, self.host);

        let cred_rel = load_terminal_credential_rel_paths();
        for rel in cred_rel {
            let Some(host_path) = resolve_safe_omiga_file(&rel, &omiga_home) else {
                tracing::debug!(path = %rel, "SSH sync: skip credential (missing or unsafe)");
                continue;
            };
            let remote_path = format!("{}/{}", container_base, rel.replace('\\', "/"));
            let parent = Path::new(&remote_path)
                .parent()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|| container_base.clone());
            self.ssh_remote_raw_cmd(
                &format!("mkdir -p {}", sh_single_quote(&parent)),
                RSYNC_TIMEOUT_MKDIR_MS,
            )
            .await;
            let dest = format!("{}:{}", dest_prefix, remote_path);
            let host_s = host_path.to_string_lossy();
            if self
                .run_rsync(
                    &engine,
                    host_s.as_ref(),
                    &dest,
                    RSYNC_TIMEOUT_SINGLE_FILE_MS,
                )
                .await
            {
                tracing::info!(host = %host_s, %remote_path, "SSH: synced credential");
            } else {
                tracing::debug!(host = %host_s, "SSH: rsync credential failed");
            }
        }

        let user_skills = omiga_home.join("skills");
        if user_skills.is_dir() {
            let remote_dir = format!("{}/skills", container_base);
            self.ssh_remote_raw_cmd(
                &format!("mkdir -p {}", sh_single_quote(&remote_dir)),
                RSYNC_TIMEOUT_MKDIR_MS,
            )
            .await;
            let src = format!("{}/", user_skills.to_string_lossy().trim_end_matches('/'));
            let dest = format!("{}:{}/", dest_prefix, remote_dir.trim_end_matches('/'));
            if self
                .run_rsync(&engine, &src, &dest, RSYNC_TIMEOUT_DIR_MS)
                .await
            {
                tracing::info!(path = ?user_skills, %remote_dir, "SSH: synced user skills dir");
            } else {
                tracing::debug!(path = ?user_skills, "SSH: rsync user skills failed");
            }
        }

        if let Some(ref root) = self.ssh_project_root {
            let proj_skills = root.join(".omiga").join("skills");
            if proj_skills.is_dir() {
                let remote_dir = format!("{}/skills", container_base);
                self.ssh_remote_raw_cmd(
                    &format!("mkdir -p {}", sh_single_quote(&remote_dir)),
                    RSYNC_TIMEOUT_MKDIR_MS,
                )
                .await;
                let src = format!("{}/", proj_skills.to_string_lossy().trim_end_matches('/'));
                let dest = format!("{}:{}/", dest_prefix, remote_dir.trim_end_matches('/'));
                if self
                    .run_rsync(&engine, &src, &dest, RSYNC_TIMEOUT_DIR_MS)
                    .await
                {
                    tracing::info!(path = ?proj_skills, %remote_dir, "SSH: synced project skills (overrides)");
                } else {
                    tracing::debug!(path = ?proj_skills, "SSH: rsync project skills failed");
                }
            }
        }

        for name in OMIGA_USER_CONTEXT_FILES {
            if let Some(host_path) = resolve_safe_omiga_file(name, &omiga_home) {
                let remote_path = format!("{}/{}", container_base, name);
                let parent = Path::new(&remote_path)
                    .parent()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|| container_base.clone());
                self.ssh_remote_raw_cmd(
                    &format!("mkdir -p {}", sh_single_quote(&parent)),
                    RSYNC_TIMEOUT_MKDIR_MS,
                )
                .await;
                let dest = format!("{}:{}", dest_prefix, remote_path);
                let host_s = host_path.to_string_lossy();
                if self
                    .run_rsync(
                        &engine,
                        host_s.as_ref(),
                        &dest,
                        RSYNC_TIMEOUT_SINGLE_FILE_MS,
                    )
                    .await
                {
                    tracing::info!(file = %name, "SSH: synced user context file");
                }
            }
        }

        for rel in OMIGA_CACHE_SUBDIRS {
            let host_dir = omiga_home.join(rel);
            if !host_dir.is_dir() {
                continue;
            }
            let remote_dir = format!("{}/{}", container_base, rel.replace('\\', "/"));
            self.ssh_remote_raw_cmd(
                &format!("mkdir -p {}", sh_single_quote(&remote_dir)),
                RSYNC_TIMEOUT_MKDIR_MS,
            )
            .await;
            let src = format!("{}/", host_dir.to_string_lossy().trim_end_matches('/'));
            let dest = format!("{}:{}/", dest_prefix, remote_dir.trim_end_matches('/'));
            if self
                .run_rsync(&engine, &src, &dest, RSYNC_TIMEOUT_CACHE_DIR_MS)
                .await
            {
                tracing::info!(dir = %rel, "SSH: synced cache directory");
            } else {
                tracing::debug!(dir = %rel, "SSH: rsync cache dir failed");
            }
        }
    }

    async fn ensure_rsync_available() -> Result<(), ()> {
        match Command::new("rsync").arg("--version").output().await {
            Ok(o) if o.status.success() => Ok(()),
            _ => Err(()),
        }
    }

    async fn run_rsync(&self, engine: &str, src: &str, dest: &str, timeout_ms: u64) -> bool {
        self.run_rsync_cmd(engine, &[src, dest], timeout_ms).await
    }

    async fn run_rsync_cmd(&self, engine: &str, args_tail: &[&str], timeout_ms: u64) -> bool {
        use tokio::time::{timeout, Duration};
        let mut cmd = Command::new("rsync");
        cmd.arg("-az")
            .arg("--timeout=30")
            .arg("--safe-links")
            .arg("-e")
            .arg(engine);
        for a in args_tail {
            cmd.arg(a);
        }
        match timeout(Duration::from_millis(timeout_ms), cmd.output()).await {
            Ok(Ok(o)) => o.status.success(),
            _ => false,
        }
    }

    /// 在远程主机上执行命令
    async fn execute_remote(
        &self,
        cmd_string: &str,
        timeout_ms: u64,
    ) -> Result<(String, i32), ExecutionError> {
        use tokio::time::{timeout, Duration};

        let mut args = self.build_ssh_base_args();
        args.push(format!("{}@{}", self.user, self.host));
        args.push(cmd_string.to_string());

        let mut child = Command::new("ssh")
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| ExecutionError::SshError(format!("Failed to spawn SSH: {}", e)))?;

        // 排空 stderr，防止 SSH 诊断输出填满管道缓冲区导致阻塞
        if let Some(stderr) = child.stderr.take() {
            tokio::spawn(async move {
                let reader = BufReader::new(stderr);
                let mut lines = reader.lines();
                while let Ok(Some(_)) = lines.next_line().await {}
            });
        }

        let result = timeout(Duration::from_millis(timeout_ms), async {
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
                // 超时：杀掉本地 ssh 进程，远端命令由 SSH 连接断开触发 SIGHUP 清理
                child.kill().await.ok();
                Ok((format!("\n[Command timed out after {}ms]", timeout_ms), 124))
            }
        }
    }
}

#[async_trait]
impl BaseEnvironment for SshEnvironment {
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
        30
    }

    /// 周期同步（由 [`BaseEnvironment::before_execute`] 限频调用），与 hermes `ssh._sync_files` 一致。
    async fn sync_files(&mut self) -> Result<(), ExecutionError> {
        self.sync_omiga_files_to_remote().await;
        Ok(())
    }

    async fn run_bash(
        &self,
        _cmd_string: &str,
        _login: bool,
        _timeout_secs: u64,
        _stdin_data: Option<&str>,
    ) -> Result<Box<dyn ProcessHandle>, ExecutionError> {
        Err(ExecutionError::NotAvailable(
            "SshEnvironment uses execute_direct".to_string(),
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

        // 准备命令：SSH 通过控制复用连接传递 stdin 不可靠，统一使用 heredoc 嵌入
        let exec_command = if let Some(stdin) = options.stdin_data.as_ref() {
            self.embed_stdin_heredoc(command, stdin)
        } else {
            command.to_string()
        };

        // When snapshot_ready is false (SSH never successfully runs init_session via run_bash),
        // no snapshot is sourced by wrap_command.  Prepend a profile source so that login-set
        // env vars (PATH additions from nvm, pyenv, etc.) are available on the remote side.
        let exec_command = if !self.snapshot_ready {
            format!(
                "source ~/.bash_profile 2>/dev/null || source ~/.profile 2>/dev/null || true\n{}",
                exec_command
            )
        } else {
            exec_command
        };

        let wrapped = self.wrap_command(&exec_command, &effective_cwd);

        // 在远程执行
        let (output, returncode) = self.execute_remote(&wrapped, effective_timeout).await?;

        let mut result = ExecResult { output, returncode };

        // 更新 CWD
        self.update_cwd_from_output(&mut result).await;

        Ok(result)
    }

    async fn cleanup(&mut self) -> Result<(), ExecutionError> {
        // 关闭 SSH 控制连接
        if self.control_socket.exists() {
            let _ = Command::new("ssh")
                .args(&[
                    "-o",
                    &format!("ControlPath={}", self.control_socket.display()),
                    "-O",
                    "exit",
                    &format!("{}@{}", self.user, self.host),
                ])
                .output()
                .await;

            // 删除 socket 文件
            let _ = tokio::fs::remove_file(&self.control_socket).await;
        }

        // 清理远程临时文件
        let _ = self
            .execute_remote(
                &format!("rm -f {} {}", self.snapshot_path, self.cwd_file),
                10_000,
            )
            .await;

        Ok(())
    }
}

/// SSH 进程句柄（占位符）
pub struct SshProcessHandle;

#[async_trait]
impl ProcessHandle for SshProcessHandle {
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
    async fn test_ssh_ensure_available() {
        // 测试 SSH 可用性检查
        let result = SshEnvironment::ensure_ssh_available().await;
        // 在 CI 中可能没有 SSH，接受两种情况
        match result {
            Ok(()) => {
                println!("SSH is available");
            }
            Err(e) => {
                println!("SSH not available: {}", e);
            }
        }
    }

    #[test]
    fn test_ssh_build_args() {
        let env = SshEnvironment {
            cwd: "~".to_string(),
            timeout_ms: 60_000,
            env: HashMap::new(),
            session_id: "test".to_string(),
            snapshot_path: "/tmp/snap".to_string(),
            cwd_file: "/tmp/cwd".to_string(),
            cwd_marker: "MARKER".to_string(),
            snapshot_ready: false,
            last_sync_time: None,
            host: "example.com".to_string(),
            user: "testuser".to_string(),
            port: 22,
            key_path: Some("/path/to/key".to_string()),
            remote_home: "/home/testuser".to_string(),
            control_socket: PathBuf::from("/tmp/test.sock"),
            ssh_project_root: None,
        };

        let args = env.build_ssh_base_args();

        // 验证控制 socket 参数
        assert!(args.iter().any(|a| a.contains("ControlPath")));
        assert!(args.iter().any(|a| a == "ControlMaster=auto"));
        assert!(args.iter().any(|a| a == "BatchMode=yes"));

        // 验证密钥参数
        assert!(args.iter().any(|a| a == "-i"));
        assert!(args.iter().any(|a| a == "/path/to/key"));
    }

    #[test]
    fn test_ssh_build_args_with_port() {
        let env = SshEnvironment {
            cwd: "~".to_string(),
            timeout_ms: 60_000,
            env: HashMap::new(),
            session_id: "test".to_string(),
            snapshot_path: "/tmp/snap".to_string(),
            cwd_file: "/tmp/cwd".to_string(),
            cwd_marker: "MARKER".to_string(),
            snapshot_ready: false,
            last_sync_time: None,
            host: "example.com".to_string(),
            user: "testuser".to_string(),
            port: 2222,
            key_path: None,
            remote_home: "/home/testuser".to_string(),
            control_socket: PathBuf::from("/tmp/test.sock"),
            ssh_project_root: None,
        };

        let args = env.build_ssh_base_args();

        // 验证端口参数
        assert!(args.iter().any(|a| a == "-p"));
        assert!(args.iter().any(|a| a == "2222"));
    }
}
