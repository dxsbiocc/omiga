//! Persistent local bash sessions for the `bash` tool.
//!
//! Sessions are plain piped `bash -l` processes, not PTYs. That means shell
//! state such as exported variables persists, but interactive/TUI programs that
//! require terminal emulation, job control, raw mode, or prompt-aware behavior
//! are not supported by this module.

pub mod head_tail_buffer;
mod process;

pub use process::{ExecSessionError, ExecSessionOutput, ExecSessionResult, ExecSessionTimeout};

use process::ExecSessionProcess;
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::Instant;

const IDLE_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const REAPER_INTERVAL: Duration = Duration::from_secs(60);

/// Global manager for long-lived local bash sessions.
///
/// Each session is a plain-pipe `bash -l` child process with piped
/// stdin/stdout/stderr. No PTY is allocated, so interactive and TUI commands
/// may not behave like they do in a terminal.
#[derive(Clone)]
pub struct ExecSessionManager {
    sessions: Arc<Mutex<HashMap<String, Arc<ExecSession>>>>,
    reaper_started: Arc<AtomicBool>,
    idle_timeout: Duration,
    reaper_interval: Duration,
}

impl Default for ExecSessionManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ExecSessionManager {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
            reaper_started: Arc::new(AtomicBool::new(false)),
            idle_timeout: IDLE_TIMEOUT,
            reaper_interval: REAPER_INTERVAL,
        }
    }

    pub fn global() -> Self {
        static GLOBAL: OnceLock<ExecSessionManager> = OnceLock::new();
        GLOBAL.get_or_init(ExecSessionManager::new).clone()
    }

    pub async fn open_or_get(
        &self,
        session_id: &str,
    ) -> Result<Arc<ExecSession>, ExecSessionError> {
        self.open_or_get_with_cwd(session_id, None).await
    }

    pub async fn exec(
        &self,
        session_id: &str,
        command: &str,
        timeout: Duration,
    ) -> Result<ExecSessionResult, ExecSessionError> {
        self.exec_with_initial_cwd(session_id, command, timeout, None)
            .await
    }

    pub async fn close(&self, session_id: &str) -> Result<(), ExecSessionError> {
        let session_id = normalize_session_id(session_id)?;
        let session = {
            let mut guard = self.sessions.lock().await;
            guard.remove(&session_id)
        };
        if let Some(session) = session {
            session.shutdown().await;
        }
        Ok(())
    }

    pub(crate) async fn open_or_get_with_cwd(
        &self,
        session_id: &str,
        cwd: Option<&Path>,
    ) -> Result<Arc<ExecSession>, ExecSessionError> {
        self.ensure_reaper_started();
        self.reap_idle_and_exited().await;

        let session_id = normalize_session_id(session_id)?;
        if let Some(existing) = {
            let guard = self.sessions.lock().await;
            guard.get(&session_id).cloned()
        } {
            return Ok(existing);
        }

        let candidate = Arc::new(ExecSession::spawn(session_id.clone(), cwd).await?);
        let mut duplicate = None;
        let session = {
            let mut guard = self.sessions.lock().await;
            if let Some(existing) = guard.get(&session_id) {
                duplicate = Some(candidate);
                existing.clone()
            } else {
                guard.insert(session_id, candidate.clone());
                candidate
            }
        };
        if let Some(duplicate) = duplicate {
            duplicate.shutdown().await;
        }
        Ok(session)
    }

    pub(crate) async fn exec_with_initial_cwd(
        &self,
        session_id: &str,
        command: &str,
        timeout: Duration,
        cwd: Option<&Path>,
    ) -> Result<ExecSessionResult, ExecSessionError> {
        let session_id = normalize_session_id(session_id)?;
        let session = self.open_or_get_with_cwd(&session_id, cwd).await?;
        let result = session.exec(command, timeout).await;

        match &result {
            Ok(ExecSessionResult::Completed(_)) => {}
            Ok(ExecSessionResult::Timeout(_)) | Err(_) => {
                if let Some(removed) = self.remove_if_same(&session_id, &session).await {
                    removed.shutdown().await;
                }
            }
        }

        result
    }

    fn ensure_reaper_started(&self) {
        if self
            .reaper_started
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }

        let manager = self.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(manager.reaper_interval).await;
                manager.reap_idle_and_exited().await;
            }
        });
    }

    async fn reap_idle_and_exited(&self) {
        let now = Instant::now();
        let entries: Vec<_> = {
            let guard = self.sessions.lock().await;
            guard
                .iter()
                .map(|(id, session)| (id.clone(), session.clone()))
                .collect()
        };

        for (session_id, session) in entries {
            match session.reap_status(now, self.idle_timeout).await {
                ReapStatus::Active => {}
                ReapStatus::Exited => {
                    let _ = self.remove_if_same(&session_id, &session).await;
                }
                ReapStatus::Idle => {
                    if let Some(removed) = self.remove_if_same(&session_id, &session).await {
                        removed.shutdown().await;
                    }
                }
            }
        }
    }

    async fn remove_if_same(
        &self,
        session_id: &str,
        session: &Arc<ExecSession>,
    ) -> Option<Arc<ExecSession>> {
        let mut guard = self.sessions.lock().await;
        let current = guard.get(session_id)?;
        if Arc::ptr_eq(current, session) {
            guard.remove(session_id)
        } else {
            None
        }
    }
}

pub struct ExecSession {
    session_id: String,
    process: Mutex<ExecSessionProcess>,
}

impl ExecSession {
    async fn spawn(session_id: String, cwd: Option<&Path>) -> Result<Self, ExecSessionError> {
        Ok(Self {
            session_id,
            process: Mutex::new(ExecSessionProcess::spawn(cwd).await?),
        })
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    async fn exec(
        &self,
        command: &str,
        timeout: Duration,
    ) -> Result<ExecSessionResult, ExecSessionError> {
        let mut process = self.process.lock().await;
        process.exec(command, timeout).await
    }

    async fn shutdown(&self) {
        let mut process = self.process.lock().await;
        process.shutdown().await;
    }

    async fn reap_status(&self, now: Instant, idle_timeout: Duration) -> ReapStatus {
        let Ok(mut process) = self.process.try_lock() else {
            return ReapStatus::Active;
        };
        if process.has_exited() {
            return ReapStatus::Exited;
        }
        if now.duration_since(process.last_used()) >= idle_timeout {
            ReapStatus::Idle
        } else {
            ReapStatus::Active
        }
    }
}

enum ReapStatus {
    Active,
    Exited,
    Idle,
}

fn normalize_session_id(session_id: &str) -> Result<String, ExecSessionError> {
    let trimmed = session_id.trim();
    if trimmed.is_empty()
        || trimmed.len() > 128
        || trimmed.contains('\0')
        || trimmed.contains('\n')
        || trimmed.contains('\r')
    {
        return Err(ExecSessionError::InvalidSessionId);
    }
    Ok(trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::{ExecSessionManager, ExecSessionResult};
    use std::time::Duration;

    #[tokio::test]
    async fn same_session_state_persists() {
        let dir = tempfile::tempdir().unwrap();
        let manager = ExecSessionManager::new();
        let session_id = format!("test-{}", uuid::Uuid::new_v4());

        let first = manager
            .exec_with_initial_cwd(
                &session_id,
                "export FOO=bar",
                Duration::from_secs(5),
                Some(dir.path()),
            )
            .await
            .unwrap();
        assert!(matches!(first, ExecSessionResult::Completed(_)));

        let second = manager
            .exec(&session_id, "echo $FOO", Duration::from_secs(5))
            .await
            .unwrap();
        let output = match second {
            ExecSessionResult::Completed(output) => output,
            other => panic!("expected completed command, got {other:?}"),
        };

        assert_eq!(output.exit_code, 0);
        assert!(
            output.stdout.join("\n").contains("bar"),
            "stdout was {:?}",
            output.stdout
        );

        manager.close(&session_id).await.unwrap();
    }

    #[tokio::test]
    async fn timeout_reaps_session_and_next_exec_is_fresh() {
        let dir = tempfile::tempdir().unwrap();
        let manager = ExecSessionManager::new();
        let session_id = format!("test-{}", uuid::Uuid::new_v4());

        manager
            .exec_with_initial_cwd(
                &session_id,
                "export AFTER_TIMEOUT=stale",
                Duration::from_secs(5),
                Some(dir.path()),
            )
            .await
            .unwrap();

        let timed_out = manager
            .exec(&session_id, "sleep 5", Duration::from_millis(100))
            .await
            .unwrap();
        assert!(
            matches!(timed_out, ExecSessionResult::Timeout(_)),
            "expected timeout result, got {timed_out:?}"
        );

        let after = manager
            .exec_with_initial_cwd(
                &session_id,
                "echo ${AFTER_TIMEOUT:-missing}",
                Duration::from_secs(5),
                Some(dir.path()),
            )
            .await
            .unwrap();
        let output = match after {
            ExecSessionResult::Completed(output) => output,
            other => panic!("expected fresh completed session, got {other:?}"),
        };

        assert_eq!(output.exit_code, 0);
        assert!(
            output.stdout.join("\n").contains("missing"),
            "stdout was {:?}",
            output.stdout
        );

        manager.close(&session_id).await.unwrap();
    }
}
