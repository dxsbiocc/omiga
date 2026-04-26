//! Background shell execution — mirrors [`src/tasks/LocalShellTask/LocalShellTask.tsx`] `spawnShellTask`:
//! fire-and-forget bash with output written to disk on completion, plus a Tauri event for the UI.

use serde::Serialize;
use std::path::PathBuf;
use tauri::{AppHandle, Emitter};

/// Resources passed from [`crate::commands::chat::execute_tool_calls`] so `bash` can spawn detached work.
#[derive(Clone)]
pub struct BackgroundShellHandle {
    pub app: AppHandle,
    /// Event name `chat-stream-{message_id}` for optional progress notifications.
    pub chat_stream_event: String,
    pub session_id: String,
    pub tool_use_id: String,
}

impl std::fmt::Debug for BackgroundShellHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BackgroundShellHandle")
            .field("session_id", &self.session_id)
            .field("tool_use_id", &self.tool_use_id)
            .field("chat_stream_event", &self.chat_stream_event)
            .finish_non_exhaustive()
    }
}
/// Tauri event payload when a background shell command finishes (same semantics as TS `enqueueShellNotification`).
#[derive(Debug, Clone, Serialize)]
pub struct BackgroundShellCompletePayload {
    pub session_id: String,
    pub tool_use_id: String,
    pub task_id: String,
    pub output_path: String,
    pub exit_code: i32,
    pub interrupted: bool,
    pub description: String,
}

pub const BACKGROUND_COMPLETE_EVENT: &str = "background-shell-complete";

/// Spawn a detached task that runs `bash -l -c` like `spawnShellTask` + `shellCommand.background()`.
/// Uses the same [`tokio_util::sync::CancellationToken`] as foreground `bash` so `cancel_stream` stops it.
pub fn spawn_background_bash_task(
    handle: BackgroundShellHandle,
    cwd: PathBuf,
    command: String,
    timeout_ms: u64,
    output_path: PathBuf,
    task_id: String,
    description: String,
    cancel: tokio_util::sync::CancellationToken,
) {
    let app = handle.app.clone();
    let stream_event = handle.chat_stream_event.clone();
    let session_id = handle.session_id.clone();
    let tool_use_id = handle.tool_use_id.clone();

    tokio::spawn(async move {
        let result =
            crate::domain::tools::bash::run_bash_command(&cwd, &command, cancel, timeout_ms, true)
                .await;

        let (exit_code, stdout, stderr, interrupted) = match result {
            Ok(raw) => (
                raw.exit_code,
                raw.stdout.join("\n"),
                raw.stderr.join("\n"),
                false,
            ),
            Err(e) => {
                let msg = e.to_string();
                let code = -1;
                (
                    code,
                    String::new(),
                    format!("{}\n", msg),
                    msg.contains("cancelled") || msg.contains("Cancelled"),
                )
            }
        };

        let combined = format!(
            "{}{}",
            stdout,
            if stderr.is_empty() {
                String::new()
            } else if !stdout.is_empty() && !stdout.ends_with('\n') {
                format!("\n{}", stderr)
            } else {
                stderr
            }
        );

        if let Some(parent) = output_path.parent() {
            if let Err(e) = tokio::fs::create_dir_all(parent).await {
                tracing::warn!("background shell: mkdir failed: {}", e);
            }
        }
        if let Err(e) = tokio::fs::write(&output_path, combined.as_bytes()).await {
            tracing::warn!("background shell: write output failed: {}", e);
        }

        let payload = BackgroundShellCompletePayload {
            session_id: session_id.clone(),
            tool_use_id: tool_use_id.clone(),
            task_id: task_id.clone(),
            output_path: output_path.to_string_lossy().to_string(),
            exit_code,
            interrupted,
            description: description.clone(),
        };

        let _ = app.emit(BACKGROUND_COMPLETE_EVENT, &payload);

        let summary = if interrupted {
            format!(
                "Background command \"{}\" was interrupted or stopped.\nOutput: {}",
                description,
                output_path.display()
            )
        } else if exit_code == 0 {
            format!(
                "Background command \"{}\" completed (exit code {exit_code}).\nOutput: {}",
                description,
                output_path.display()
            )
        } else {
            format!(
                "Background command \"{}\" finished with exit code {exit_code}.\nOutput: {}",
                description,
                output_path.display()
            )
        };

        let _ = app.emit(
            &stream_event,
            crate::infrastructure::streaming::StreamOutputItem::Text(summary),
        );
    });
}
