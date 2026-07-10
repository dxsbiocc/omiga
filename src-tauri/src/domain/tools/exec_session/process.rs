use super::head_tail_buffer::HeadTailBuffer;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;
use thiserror::Error;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command};
use tokio::time::Instant;
use uuid::Uuid;

pub const DEFAULT_OUTPUT_MAX_BYTES: usize = 128 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecSessionOutput {
    pub exit_code: i32,
    pub stdout: Vec<String>,
    pub stderr: Vec<String>,
    pub stdout_truncated_bytes: usize,
    pub stderr_truncated_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecSessionTimeout {
    pub seconds: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecSessionResult {
    Completed(ExecSessionOutput),
    Timeout(ExecSessionTimeout),
}

#[derive(Debug, Error)]
pub enum ExecSessionError {
    #[error("exec session id must not be empty")]
    InvalidSessionId,
    #[error("failed to spawn persistent bash process: {message}")]
    Spawn { message: String },
    #[error("persistent bash process is missing {stream}")]
    MissingPipe { stream: &'static str },
    #[error("bash session io error: {message}")]
    Io { message: String },
    #[error("bash session exited before command sentinel was observed")]
    ProcessExited,
    #[error("bash session sentinel protocol error: {message}")]
    Protocol { message: String },
}

impl ExecSessionError {
    fn io(err: std::io::Error) -> Self {
        Self::Io {
            message: err.to_string(),
        }
    }
}

pub(super) struct ExecSessionProcess {
    child: Child,
    stdin: ChildStdin,
    stdout: Option<ChildStdout>,
    stderr: Option<ChildStderr>,
    pid: Option<u32>,
    last_used: Instant,
}

impl ExecSessionProcess {
    pub(super) async fn spawn(cwd: Option<&Path>) -> Result<Self, ExecSessionError> {
        let mut command = Command::new("bash");
        command.arg("-l");
        command.stdin(Stdio::piped());
        command.stdout(Stdio::piped());
        command.stderr(Stdio::piped());
        if let Some(cwd) = cwd {
            command.current_dir(cwd);
        }

        #[cfg(unix)]
        command.process_group(0);

        let mut child = command.spawn().map_err(|e| ExecSessionError::Spawn {
            message: e.to_string(),
        })?;
        let stdin = child
            .stdin
            .take()
            .ok_or(ExecSessionError::MissingPipe { stream: "stdin" })?;
        let stdout = child
            .stdout
            .take()
            .ok_or(ExecSessionError::MissingPipe { stream: "stdout" })?;
        let stderr = child
            .stderr
            .take()
            .ok_or(ExecSessionError::MissingPipe { stream: "stderr" })?;
        let pid = child.id();

        Ok(Self {
            child,
            stdin,
            stdout: Some(stdout),
            stderr: Some(stderr),
            pid,
            last_used: Instant::now(),
        })
    }

    pub(super) fn last_used(&self) -> Instant {
        self.last_used
    }

    pub(super) fn has_exited(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(Some(_)))
    }

    pub(super) async fn exec(
        &mut self,
        command: &str,
        timeout_duration: Duration,
    ) -> Result<ExecSessionResult, ExecSessionError> {
        let sentinel = format!("__OMIGA_EXEC_SESSION_DONE_{}__", Uuid::new_v4().simple());
        let script = sentinel_wrapped_command(command, &sentinel);

        self.stdin
            .write_all(script.as_bytes())
            .await
            .map_err(ExecSessionError::io)?;
        self.stdin.flush().await.map_err(ExecSessionError::io)?;

        let stdout = self
            .stdout
            .take()
            .ok_or(ExecSessionError::MissingPipe { stream: "stdout" })?;
        let stderr = self
            .stderr
            .take()
            .ok_or(ExecSessionError::MissingPipe { stream: "stderr" })?;

        let stdout_sentinel = sentinel.clone();
        let read_both = async move {
            tokio::try_join!(
                read_stream_until_sentinel(stdout, &stdout_sentinel),
                read_stream_until_sentinel(stderr, &sentinel)
            )
        };

        let timeout_result = tokio::time::timeout(timeout_duration, read_both).await;
        match timeout_result {
            Ok(Ok((stdout_capture, stderr_capture))) => {
                self.stdout = Some(stdout_capture.reader);
                self.stderr = Some(stderr_capture.reader);
                self.last_used = Instant::now();
                Ok(ExecSessionResult::Completed(ExecSessionOutput {
                    exit_code: stdout_capture.exit_code,
                    stdout: bytes_to_lines(stdout_capture.output),
                    stderr: bytes_to_lines(stderr_capture.output),
                    stdout_truncated_bytes: stdout_capture.truncated_bytes,
                    stderr_truncated_bytes: stderr_capture.truncated_bytes,
                }))
            }
            Ok(Err(err)) => {
                self.shutdown().await;
                Err(err)
            }
            Err(_) => {
                self.shutdown().await;
                Ok(ExecSessionResult::Timeout(ExecSessionTimeout {
                    seconds: timeout_seconds(timeout_duration),
                }))
            }
        }
    }

    pub(super) async fn shutdown(&mut self) {
        if self.has_exited() {
            return;
        }

        if let Some(pid) = self.pid {
            kill_process_tree(pid).await;
        }

        let _ = self.child.start_kill();
        let _ = tokio::time::timeout(Duration::from_secs(1), self.child.wait()).await;
    }
}

fn sentinel_wrapped_command(command: &str, sentinel: &str) -> String {
    format!(
        "{{\n{command}\n}}\n\
         __omiga_exec_status=$?\n\
         printf '%s:%s\\n' '{sentinel}' \"$__omiga_exec_status\"\n\
         printf '%s:%s\\n' '{sentinel}' \"$__omiga_exec_status\" >&2\n"
    )
}

struct StreamCapture<R> {
    reader: R,
    output: Vec<u8>,
    exit_code: i32,
    truncated_bytes: usize,
}

async fn read_stream_until_sentinel<R>(
    mut reader: R,
    sentinel: &str,
) -> Result<StreamCapture<R>, ExecSessionError>
where
    R: AsyncRead + Unpin,
{
    let marker = format!("{sentinel}:").into_bytes();
    let retain_for_marker = marker.len().saturating_sub(1);
    let mut pending = Vec::with_capacity(marker.len().saturating_mul(2));
    let mut output = HeadTailBuffer::new(DEFAULT_OUTPUT_MAX_BYTES);
    let mut chunk = [0_u8; 8192];

    loop {
        let n = reader
            .read(&mut chunk)
            .await
            .map_err(ExecSessionError::io)?;
        if n == 0 {
            return Err(ExecSessionError::ProcessExited);
        }

        pending.extend_from_slice(&chunk[..n]);
        if let Some(pos) = find_subsequence(&pending, &marker) {
            output.push(&pending[..pos]);
            let status_start = pos.saturating_add(marker.len());
            let exit_code = read_exit_code(&mut reader, pending[status_start..].to_vec()).await?;
            return Ok(StreamCapture {
                reader,
                output: output.to_bytes(),
                exit_code,
                truncated_bytes: output.omitted_bytes(),
            });
        }

        if pending.len() > retain_for_marker {
            let flush_len = pending.len().saturating_sub(retain_for_marker);
            output.push(&pending[..flush_len]);
            pending.drain(..flush_len);
        }
    }
}

async fn read_exit_code<R>(
    reader: &mut R,
    mut status_bytes: Vec<u8>,
) -> Result<i32, ExecSessionError>
where
    R: AsyncRead + Unpin,
{
    let mut chunk = [0_u8; 128];
    loop {
        if let Some(newline) = status_bytes.iter().position(|b| *b == b'\n') {
            let raw = String::from_utf8_lossy(&status_bytes[..newline]);
            return raw
                .trim()
                .parse::<i32>()
                .map_err(|e| ExecSessionError::Protocol {
                    message: format!("invalid exit code `{}`: {}", raw.trim(), e),
                });
        }

        if status_bytes.len() > 32 {
            return Err(ExecSessionError::Protocol {
                message: "sentinel exit code was too long".to_string(),
            });
        }

        let n = reader
            .read(&mut chunk)
            .await
            .map_err(ExecSessionError::io)?;
        if n == 0 {
            return Err(ExecSessionError::ProcessExited);
        }
        status_bytes.extend_from_slice(&chunk[..n]);
    }
}

fn bytes_to_lines(bytes: Vec<u8>) -> Vec<String> {
    if bytes.is_empty() {
        return Vec::new();
    }
    String::from_utf8_lossy(&bytes)
        .lines()
        .map(String::from)
        .collect()
}

fn timeout_seconds(duration: Duration) -> u64 {
    let millis = duration.as_millis().max(1);
    millis.div_ceil(1000).min(u128::from(u64::MAX)) as u64
}

fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

async fn kill_process_tree(pid: u32) {
    #[cfg(unix)]
    {
        use nix::sys::signal::{killpg, Signal};
        use nix::unistd::Pid;
        let _ = killpg(Pid::from_raw(pid as i32), Signal::SIGTERM);
        tokio::time::sleep(Duration::from_millis(200)).await;
        let _ = killpg(Pid::from_raw(pid as i32), Signal::SIGKILL);
    }

    #[cfg(windows)]
    {
        let _ = Command::new("taskkill")
            .args(["/F", "/T", "/PID", &pid.to_string()])
            .output()
            .await;
    }
}
