//! 执行环境抽象基类
//!
//! 对应 hermes-agent 中的 BaseEnvironment 类
//! 统一执行模型：每次命令生成新的 bash -c 进程
//! 会话快照在初始化时捕获一次，每个命令前重新加载
//! CWD 通过 stdout 标记或临时文件持久化

use super::types::{
    ExecOptions, ExecResult, ExecutionError, ExternalTerminalCommand, ProcessHandle,
};
use async_trait::async_trait;
use rand::distributions::Alphanumeric;
use rand::Rng;
use tokio::fs::read_to_string;
use tokio::time::{timeout, Duration};

/// CWD 同步间隔（秒）
const SYNC_INTERVAL_SECONDS: f64 = 5.0;

/// 最大 CWD 路径长度
const MAX_CWD_LEN: usize = 4096;

#[async_trait]
pub trait BaseEnvironment: Send + Sync {
    /// 获取当前工作目录
    fn cwd(&self) -> &str;

    /// 获取超时时间（毫秒）
    fn timeout_ms(&self) -> u64;

    /// 获取环境变量
    fn env(&self) -> &std::collections::HashMap<String, String>;

    /// 设置当前工作目录
    fn set_cwd(&mut self, cwd: String);

    /// 获取会话 ID
    fn session_id(&self) -> &str;

    /// 获取快照路径
    fn snapshot_path(&self) -> &str;

    /// 获取 CWD 文件路径
    fn cwd_file(&self) -> &str;

    /// 获取 CWD 标记
    fn cwd_marker(&self) -> &str;

    /// 检查快照是否就绪
    fn snapshot_ready(&self) -> bool;

    /// 设置快照就绪状态
    fn set_snapshot_ready(&mut self, ready: bool);

    /// 获取最后同步时间
    fn last_sync_time(&self) -> Option<f64>;

    /// 设置最后同步时间
    fn set_last_sync_time(&mut self, time: Option<f64>);

    /// 获取 stdin 模式
    fn stdin_mode(&self) -> &'static str;

    /// 获取快照超时时间（秒）
    fn snapshot_timeout_secs(&self) -> u64;

    /// 是否在本地文件系统执行（决定是否读取本地 cwd 文件）
    fn is_local_filesystem(&self) -> bool {
        false
    }

    /// Optional interactive command for opening this environment in the user's system terminal.
    ///
    /// Most environments execute commands through non-interactive APIs; those should return
    /// `None` until there is a real CLI/session that can attach to the same backend.
    fn external_terminal_command(&self) -> Option<ExternalTerminalCommand> {
        None
    }

    /// 执行 bash 命令 - 子类必须实现
    ///
    /// # Arguments
    /// * `cmd_string` - 要执行的命令字符串
    /// * `login` - 是否使用登录 shell
    /// * `timeout_secs` - 超时时间（秒）
    /// * `stdin_data` - 可选的标准输入数据
    async fn run_bash(
        &self,
        cmd_string: &str,
        login: bool,
        timeout_secs: u64,
        stdin_data: Option<&str>,
    ) -> Result<Box<dyn ProcessHandle>, ExecutionError>;

    /// 清理资源 - 子类必须实现
    async fn cleanup(&mut self) -> Result<(), ExecutionError>;

    /// 初始化会话：捕获登录 shell 环境到快照文件
    ///
    /// 对应 hermes-agent 中的 init_session()
    async fn init_session(&mut self) -> Result<(), ExecutionError> {
        let q_snap = format!("'{}'", self.snapshot_path().replace('\'', "'\\''"));
        let q_cwd = format!("'{}'", self.cwd_file().replace('\'', "'\\''"));
        let bootstrap = format!(
            "export -p > {snapshot}\n\
             declare -f | grep -vE '^_[^_]' >> {snapshot}\n\
             alias -p >> {snapshot}\n\
             echo 'shopt -s expand_aliases' >> {snapshot}\n\
             echo 'set +e' >> {snapshot}\n\
             echo 'set +u' >> {snapshot}\n\
             pwd -P > {cwd_file} 2>/dev/null || true\n\
             printf '\\n{marker}%s{marker}\\n' \"$(pwd -P)\"",
            snapshot = q_snap,
            cwd_file = q_cwd,
            marker = self.cwd_marker()
        );

        match self
            .run_bash(&bootstrap, true, self.snapshot_timeout_secs(), None)
            .await
        {
            Ok(handle) => {
                let result = self
                    .wait_for_process(handle, self.snapshot_timeout_secs() * 1000)
                    .await?;
                let mut result = result;
                self.update_cwd_from_output(&mut result).await;
                self.set_snapshot_ready(true);
                tracing::info!(
                    session_id = %self.session_id(),
                    cwd = %self.cwd(),
                    "Session snapshot created successfully"
                );
                Ok(())
            }
            Err(e) => {
                tracing::warn!(
                    session_id = %self.session_id(),
                    error = %e,
                    "init_session failed - falling back to bash -l per command"
                );
                self.set_snapshot_ready(false);
                Ok(())
            }
        }
    }

    /// 构建完整命令字符串
    ///
    /// 对应 hermes-agent 中的 _wrap_command()
    fn wrap_command(&self, command: &str, cwd: &str) -> String {
        let escaped = command.replace('\'', "'\\''");
        let mut parts: Vec<String> = Vec::new();

        // Source 快照（如果可用）— 路径单引号转义，防止路径含空格或特殊字符时注入
        if self.snapshot_ready() {
            let quoted_snap = format!("'{}'", self.snapshot_path().replace('\'', "'\\''"));
            parts.push(format!("source {} 2>/dev/null || true", quoted_snap));
        }

        // cd 到工作目录
        // 安全引用路径：
        // - "~" 单独保持不变（shell 安全展开）
        // - "~/" 前缀路径：使用 "$HOME" 并对其余部分单引号转义，防止注入
        // - 其他路径：单引号包裹，并转义内部单引号
        let quoted_cwd = if cwd == "~" {
            "~".to_string()
        } else if let Some(rest) = cwd.strip_prefix("~/") {
            let quoted_rest = format!("'{}'", rest.replace('\'', "'\\''"));
            format!("\"$HOME\"/{}", quoted_rest)
        } else {
            format!("'{}'", cwd.replace('\'', "'\\''"))
        };
        parts.push(format!("cd {} || exit 126", quoted_cwd));

        // 执行实际命令
        parts.push(format!("eval '{}'", escaped));
        parts.push("__hermes_ec=$?".to_string());

        // 重新导出环境变量到快照
        if self.snapshot_ready() {
            let quoted_snap = format!("'{}'", self.snapshot_path().replace('\'', "'\\''"));
            parts.push(format!("export -p > {} 2>/dev/null || true", quoted_snap));
        }

        // 写入 CWD 到文件和标记
        let quoted_cwd_file = format!("'{}'", self.cwd_file().replace('\'', "'\\''"));
        parts.push(format!("pwd -P > {} 2>/dev/null || true", quoted_cwd_file));
        parts.push(format!(
            "printf '\\n{}%s{}\\n' \"$(pwd -P)\"",
            self.cwd_marker(),
            self.cwd_marker()
        ));
        parts.push("exit $__hermes_ec".to_string());

        parts.join("\n")
    }

    /// 将 stdin 嵌入为 heredoc
    ///
    /// 对应 hermes-agent 中的 _embed_stdin_heredoc()
    fn embed_stdin_heredoc(&self, command: &str, stdin_data: &str) -> String {
        // Regenerate delimiter if stdin_data contains it — prevents heredoc injection
        let delimiter = loop {
            let candidate: String = rand::thread_rng()
                .sample_iter(&Alphanumeric)
                .take(16)
                .map(char::from)
                .collect();
            // A heredoc closes when a line equals exactly the delimiter
            let safe = !stdin_data.lines().any(|l| l.trim() == candidate.as_str());
            if safe {
                break candidate;
            }
        };
        format!(
            "{} << '{}'\n{}\n{}",
            command, delimiter, stdin_data, delimiter
        )
    }

    /// 等待进程完成，处理中断和超时
    ///
    /// 对应 hermes-agent 中的 _wait_for_process()
    async fn wait_for_process(
        &self,
        mut handle: Box<dyn ProcessHandle>,
        timeout_ms: u64,
    ) -> Result<ExecResult, ExecutionError> {
        use tokio::io::AsyncBufReadExt;

        // 收集输出
        let mut stdout_lines: Vec<String> = Vec::new();

        // 使用 timeout 包装整个等待过程
        let wait_fut = async {
            // 读取 stdout（如果有）
            if let Some(stdout) = handle.stdout() {
                let reader = tokio::io::BufReader::new(stdout);
                let mut lines = reader.lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    stdout_lines.push(line);
                }
            }

            // 等待进程完成
            handle.wait().await
        };

        match timeout(Duration::from_millis(timeout_ms), wait_fut).await {
            Ok(exit_code) => {
                let output = stdout_lines.join("\n");
                Ok(ExecResult {
                    output,
                    returncode: exit_code,
                })
            }
            Err(_) => {
                // 超时
                handle.kill();
                let output = stdout_lines.join("\n")
                    + &format!("\n[Command timed out after {}ms]", timeout_ms);
                Ok(ExecResult {
                    output,
                    returncode: 124,
                })
            }
        }
    }

    /// 执行前钩子
    ///
    /// 对应 hermes-agent 中的 _before_execute()
    async fn before_execute(&mut self) -> Result<(), ExecutionError> {
        if let Some(last_sync) = self.last_sync_time() {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs_f64();
            if now - last_sync >= SYNC_INTERVAL_SECONDS {
                self.sync_files().await?;
                self.set_last_sync_time(Some(now));
            }
        }
        Ok(())
    }

    /// 文件同步钩子 - 子类可覆盖
    ///
    /// 对应 hermes-agent 中的 _sync_files()
    async fn sync_files(&mut self) -> Result<(), ExecutionError> {
        Ok(())
    }

    /// 统一执行入口
    ///
    /// 对应 hermes-agent 中的 execute()
    async fn execute(
        &mut self,
        command: &str,
        options: ExecOptions,
    ) -> Result<ExecResult, ExecutionError> {
        self.before_execute().await?;

        let effective_timeout = options.timeout.unwrap_or(self.timeout_ms());
        let effective_cwd = options.cwd.unwrap_or_else(|| self.cwd().to_string());

        // 准备命令
        let (exec_command, effective_stdin): (String, Option<String>) = if let Some(stdin_data) =
            options
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
        let login = !self.snapshot_ready();

        let handle = self
            .run_bash(
                &wrapped,
                login,
                effective_timeout / 1000,
                effective_stdin.as_deref(),
            )
            .await?;

        let mut result = self.wait_for_process(handle, effective_timeout).await?;

        // 更新 CWD
        self.update_cwd_from_output(&mut result).await;

        Ok(result)
    }

    /// 从输出中提取 CWD 并清理标记
    ///
    /// 对应 hermes-agent 中的 _extract_cwd_from_output()
    async fn update_cwd_from_output(&mut self, result: &mut ExecResult) {
        // 仅在本地文件系统环境下从文件读取 CWD
        // 远程环境（Docker/SSH/Modal/Daytona）的 cwd_file 在远端，无法从本地读取
        if self.is_local_filesystem() {
            if let Ok(cwd) = read_to_string(self.cwd_file()).await {
                let cwd = cwd.trim();
                if !cwd.is_empty() {
                    self.set_cwd(cwd.to_string());
                }
            }
        }

        // 从 stdout 标记中提取
        // 使用 char 边界安全的方式查找，避免 UTF-8 多字节字符时 byte-index panic
        let output = result.output.clone();
        let marker = self.cwd_marker().to_string();

        let last = match output.rfind(&marker) {
            Some(i) => i,
            None => return,
        };

        // 确保 search_start 落在 char 边界上
        let raw_start = last.saturating_sub(MAX_CWD_LEN);
        let search_start = (raw_start..=last)
            .find(|&i| output.is_char_boundary(i))
            .unwrap_or(last);

        let first = match output[search_start..last].rfind(&marker) {
            Some(i) => search_start + i,
            None => return,
        };

        let cwd_path = output[first + marker.len()..last].trim();
        if !cwd_path.is_empty() {
            self.set_cwd(cwd_path.to_string());
        }

        // 清理标记行
        let line_start = output[..first].rfind('\n').map(|i| i + 1).unwrap_or(first);
        let line_end = output[last + marker.len()..]
            .find('\n')
            .map(|i| last + marker.len() + i + 1)
            .unwrap_or(output.len());

        result.output = output[..line_start].to_string() + &output[line_end..];
    }
}

/// 异步读取流的所有内容并发送到通道
///
/// 用于在 ProcessHandle 中读取 stdout/stderr
pub async fn read_to_end<R>(mut reader: R, tx: tokio::sync::mpsc::Sender<Vec<u8>>)
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    use tokio::io::AsyncReadExt;
    let mut buffer = Vec::new();
    if reader.read_to_end(&mut buffer).await.is_ok() {
        let _ = tx.send(buffer).await;
    }
}

/// 生成唯一会话 ID
pub fn generate_session_id() -> String {
    uuid::Uuid::new_v4().to_string().replace("-", "")[..12].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_session_id() {
        let id1 = generate_session_id();
        let id2 = generate_session_id();
        assert_eq!(id1.len(), 12);
        assert_eq!(id2.len(), 12);
        assert_ne!(id1, id2); // 几乎总是不同
    }

    #[test]
    fn test_embed_stdin_heredoc() {
        struct TestEnv;
        impl TestEnv {
            fn embed_stdin_heredoc(&self, command: &str, stdin_data: &str) -> String {
                let delimiter: String = rand::thread_rng()
                    .sample_iter(&Alphanumeric)
                    .take(12)
                    .map(char::from)
                    .collect();
                format!(
                    "{} << '{}'\n{}\n{}",
                    command, delimiter, stdin_data, delimiter
                )
            }
        }

        let env = TestEnv;
        let result = env.embed_stdin_heredoc("cat", "hello world");
        assert!(result.contains("cat << '"));
        assert!(result.contains("hello world"));
        assert!(result.lines().count() == 3);
    }
}
