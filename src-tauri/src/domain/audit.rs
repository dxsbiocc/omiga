//! Append-only local audit log for sensitive tool executions.
//!
//! Writes one JSONL file per day to `~/.omiga/audit/YYYY-MM-DD.jsonl`.
//! All writes are fire-and-forget — failures are silently swallowed so the
//! audit path never crashes the app.

use chrono::Local;
use serde::Serialize;
use std::path::PathBuf;

#[derive(Serialize)]
pub struct AuditEntry {
    /// RFC 3339 UTC timestamp
    pub ts: String,
    pub session_id: String,
    pub tool: String,
    /// Brief, secret-free summary of the invocation (path, command prefix, etc.)
    pub args_summary: String,
    /// "ok" | "error" | "denied"
    pub status: &'static str,
    /// First 200 chars of the error message when status == "error"
    pub message: Option<String>,
}

/// Build a brief, secret-free summary of tool arguments.
pub fn summarize_args(tool_name: &str, arguments: &str) -> String {
    let val: serde_json::Value = match serde_json::from_str(arguments) {
        Ok(v) => v,
        Err(_) => return format!("{tool_name}(...)"),
    };
    match tool_name.to_ascii_lowercase().as_str() {
        "bash" => val
            .get("command")
            .and_then(|v| v.as_str())
            .map(|cmd| cmd.chars().take(80).collect::<String>())
            .unwrap_or_else(|| "bash(...)".to_string()),
        "file_write" | "file_edit" | "file_read" | "glob" => val
            .get("path")
            .or_else(|| val.get("file_path"))
            .or_else(|| val.get("pattern"))
            .and_then(|v| v.as_str())
            .map(str::to_owned)
            .unwrap_or_else(|| format!("{tool_name}(...)")),
        _ => format!("{tool_name}(...)"),
    }
}

pub fn audit_log_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".omiga")
        .join("audit")
}

pub fn audit_log_path() -> PathBuf {
    audit_log_dir().join(format!("{}.jsonl", Local::now().format("%Y-%m-%d")))
}

/// Write an audit entry as a JSONL record. Fire-and-forget — never blocks.
pub async fn log(entry: AuditEntry) {
    let path = audit_log_path();
    if let Err(e) = write_entry(&path, &entry).await {
        tracing::debug!("audit log write skipped: {}", e);
    }
}

async fn write_entry(path: &PathBuf, entry: &AuditEntry) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let mut line = serde_json::to_string(entry).map_err(std::io::Error::other)?;
    line.push('\n');
    use tokio::io::AsyncWriteExt;
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .await?;
    file.write_all(line.as_bytes()).await
}

/// Determine whether a tool name warrants an audit record.
pub fn should_audit(tool_name: &str) -> bool {
    matches!(
        tool_name.to_ascii_lowercase().as_str(),
        "bash" | "file_write" | "file_edit" | "file_delete" | "notebook_edit"
    )
}
