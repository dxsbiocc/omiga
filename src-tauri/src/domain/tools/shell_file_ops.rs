//! Shell-based file operations for remote/sandbox execution environments.
//!
//! Mirrors hermes-agent `ShellFileOperations` and `file_tools.py`:
//! > "all file operations can be expressed as shell commands"
//!
//! Instead of calling `tokio::fs` (which only works locally), every operation
//! is expressed as a shell command executed through the active `BaseEnvironment`.
//! This means the same `file_read` / `file_write` / `file_edit` / `grep` / `glob`
//! tool implementations work on SSH, Docker, Modal, Daytona, and Singularity
//! without per-backend code paths.

use base64::{engine::general_purpose::STANDARD, Engine as _};
use crate::execution::{BaseEnvironment, ExecOptions};
use crate::errors::ToolError;

/// Timeout for a single file-operation shell command (ms).
const FILE_OP_TIMEOUT_MS: u64 = 30_000;

// ─── Main struct ──────────────────────────────────────────────────────────────

/// Wraps a `BaseEnvironment` and exposes file-system operations as shell commands.
///
/// ```rust,ignore
/// let env_arc = env_store.get_or_create(&ctx, 30_000).await?;
/// let mut guard = env_arc.lock().await;
/// let mut ops = ShellFileOps::new(&mut *guard);
/// let result = ops.read_file("src/main.rs", 0, 100).await?;
/// ```
pub struct ShellFileOps<'a> {
    env: &'a mut (dyn BaseEnvironment + 'a),
}

impl<'a> ShellFileOps<'a> {
    pub fn new(env: &'a mut dyn BaseEnvironment) -> Self {
        Self { env }
    }

    // ── Read ──────────────────────────────────────────────────────────────────

    /// Read file with 0-indexed line pagination.
    /// Returns lines formatted as `"<lineno>\t<text>"` (same as `awk` output).
    pub async fn read_file(
        &mut self,
        path: &str,
        offset: usize,
        limit: usize,
    ) -> Result<ShellReadResult, ToolError> {
        let q = shell_quote(path);

        // Total lines
        let total_res = self
            .run(&format!("wc -l < {q} 2>/dev/null || echo 0"))
            .await?;
        let total_lines: usize = total_res
            .trim()
            .split_whitespace()
            .next()
            .unwrap_or("0")
            .parse()
            .unwrap_or(0);

        // Paginated content: awk is 1-indexed; offset is 0-indexed
        let start = offset + 1;
        let end = offset + limit;
        let content = self
            .run(&format!(
                r#"awk 'NR>={start} && NR<={end} {{printf "%d\t%s\n", NR, $0}}' {q}"#
            ))
            .await?;

        Ok(ShellReadResult {
            content,
            total_lines,
            has_more: offset + limit < total_lines,
        })
    }

    // ── Write ─────────────────────────────────────────────────────────────────

    /// Write content to a file, creating parent directories automatically.
    /// Uses base64 encoding to avoid shell-escaping issues with arbitrary content.
    pub async fn write_file(&mut self, path: &str, content: &str) -> Result<usize, ToolError> {
        let b64 = STANDARD.encode(content.as_bytes());
        let q_path = shell_quote(path);
        let q_b64 = shell_quote(&b64);

        // Ensure parent directories exist
        let _ = self.run(&format!("mkdir -p $(dirname {q_path})")).await;

        // Decode base64 and write atomically via a temp file
        let tmp = format!("{}.omiga_tmp", path);
        let q_tmp = shell_quote(&tmp);
        let write_cmd = format!(
            "printf '%s' {q_b64} | base64 -d > {q_tmp} && mv -f {q_tmp} {q_path}"
        );
        let res = self.run(&write_cmd).await?;
        if !res.trim().is_empty() && res.contains("error") {
            return Err(ToolError::ExecutionFailed {
                message: format!("远程写文件失败: {}", res.trim()),
            });
        }
        Ok(content.len())
    }

    // ── Edit ──────────────────────────────────────────────────────────────────

    /// Exact string replace inside a file (once or all occurrences).
    /// Uses a Python one-liner for reliable Unicode handling.
    pub async fn edit_file(
        &mut self,
        path: &str,
        old_string: &str,
        new_string: &str,
        replace_all: bool,
    ) -> Result<String, ToolError> {
        let old_b64 = STANDARD.encode(old_string.as_bytes());
        let new_b64 = STANDARD.encode(new_string.as_bytes());
        let count = if replace_all { 0usize } else { 1usize };
        let p_path = python_str(path);

        let script = format!(
            "python3 - << 'OMIGA_EDIT_EOF'\n\
import sys, base64\n\
old = base64.b64decode('{old_b64}').decode('utf-8', errors='surrogateescape')\n\
new = base64.b64decode('{new_b64}').decode('utf-8', errors='surrogateescape')\n\
try:\n\
    with open({p_path}, 'r', encoding='utf-8', errors='surrogateescape') as f: c = f.read()\n\
except OSError as e:\n\
    print('ERR:IO:' + str(e)); sys.exit(1)\n\
if old not in c:\n\
    print('ERR:NOT_FOUND'); sys.exit(1)\n\
n = {count}\n\
c = c.replace(old, new) if n == 0 else c.replace(old, new, n)\n\
with open({p_path}, 'w', encoding='utf-8', errors='surrogateescape') as f: f.write(c)\n\
print('OK')\n\
OMIGA_EDIT_EOF"
        );

        let res = self.run(&script).await?;
        let trimmed = res.trim();
        if trimmed == "OK" {
            Ok(trimmed.to_string())
        } else if trimmed.starts_with("ERR:NOT_FOUND") {
            Err(ToolError::ExecutionFailed {
                message: format!("old_string not found in {}", path),
            })
        } else {
            Err(ToolError::ExecutionFailed {
                message: format!("远程 file_edit 失败: {}", trimmed),
            })
        }
    }

    // ── Grep ──────────────────────────────────────────────────────────────────

    /// Search files with regex. Returns raw lines in `filename:lineno:text` format.
    /// Tries `rg` first; falls back to `grep -rn`.
    pub async fn grep_raw(
        &mut self,
        pattern: &str,
        search_path: &str,
        glob: Option<&str>,
        case_insensitive: bool,
        max_results: usize,
    ) -> Result<String, ToolError> {
        let ci = if case_insensitive { "-i " } else { "" };
        let glob_arg = glob
            .map(|g| format!("--glob {} ", shell_quote(g)))
            .unwrap_or_default();
        let cap = max_results.min(5000);
        let q_pat = shell_quote(pattern);
        let q_path = shell_quote(search_path);

        // Prefer rg (same engine as local); fall back to grep
        let cmd = format!(
            "( rg --no-heading --with-filename --line-number {ci}{glob_arg}{q_pat} {q_path} \
             2>/dev/null | head -{cap} ) \
             || ( grep -rn {ci}{q_pat} {q_path} 2>/dev/null | head -{cap} )"
        );
        self.run(&cmd).await
    }

    // ── Glob ──────────────────────────────────────────────────────────────────

    /// Find files matching a glob pattern. Returns one absolute path per line.
    pub async fn glob_find(
        &mut self,
        pattern: &str,
        base_path: &str,
        max_results: usize,
        include_hidden: bool,
    ) -> Result<Vec<String>, ToolError> {
        // Build the appropriate `find` predicate from the glob pattern
        let find_pred = if pattern.contains('/') {
            // Pattern with path separator → use -path
            let p = pattern.trim_start_matches("**/");
            format!("-path {}", shell_quote(&format!("*/{}", p)))
        } else {
            format!("-name {}", shell_quote(pattern))
        };

        let hidden_prune = if !include_hidden {
            r#"! -name '.*' "#
        } else {
            ""
        };

        let cmd = format!(
            "find {} {}{} 2>/dev/null | sort | head -{}",
            shell_quote(base_path),
            hidden_prune,
            find_pred,
            max_results,
        );

        let out = self.run(&cmd).await?;
        Ok(out
            .lines()
            .map(str::to_string)
            .filter(|s| !s.is_empty())
            .collect())
    }

    // ── Internal helpers ──────────────────────────────────────────────────────

    async fn run(&mut self, cmd: &str) -> Result<String, ToolError> {
        let opts = ExecOptions::with_timeout(FILE_OP_TIMEOUT_MS);
        let result = self
            .env
            .execute(cmd, opts)
            .await
            .map_err(|e| ToolError::ExecutionFailed { message: e.to_string() })?;
        Ok(result.output)
    }
}

// ─── Result types ─────────────────────────────────────────────────────────────

pub struct ShellReadResult {
    pub content: String,
    pub total_lines: usize,
    pub has_more: bool,
}

// ─── Shell quoting ────────────────────────────────────────────────────────────

/// POSIX single-quote escaping: wrap in `'…'` and escape any `'` inside.
pub fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Python string literal (single-quoted) — safe for embedding in a heredoc script.
fn python_str(s: &str) -> String {
    // Escape backslash and single quote
    let escaped = s.replace('\\', "\\\\").replace('\'', "\\'");
    format!("'{}'", escaped)
}
