//! Remote file operations executed inside a sandbox environment
//! (Docker / Daytona / Modal / Singularity).
//!
//! The file tree and editor use these commands when `execution_environment == "sandbox"`.
//! Mirrors the hermes-agent pattern: each command resolves the session's cached
//! `BaseEnvironment` via `EnvStore`, then runs a bash snippet to list / read / write files.
//!
//! On first call the sandbox backend is spun up (container started, etc.);
//! subsequent calls in the same session reuse the cached environment.

use super::CommandResult;
use crate::app_state::OmigaAppState;
use crate::commands::fs::{
    DirectoryEntry, DirectoryListResponse, FileReadResponse, FileWriteResponse,
};
use crate::domain::tools::{env_store::EnvStore, ToolContext};
use crate::errors::{AppError, FsError};
use crate::execution::ExecOptions;
use crate::utils::shell::shell_single_quote;
use std::path::{Component, Path, PathBuf};

const SANDBOX_TIMEOUT_MS: u64 = 60_000;
const MAX_READ_BYTES: u64 = 2 * 1024 * 1024;

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn sh_quote(s: &str) -> String {
    shell_single_quote(s)
}

fn validate_sandbox_path(p: &str) -> Result<String, AppError> {
    let t = p.trim();
    if t.is_empty() {
        return Ok("/workspace".to_string());
    }
    if t.contains('\n') || t.contains('\0') {
        return Err(AppError::Fs(FsError::InvalidPath {
            path: p.to_string(),
        }));
    }
    if !t.starts_with('/') {
        return Err(AppError::Fs(FsError::InvalidPath {
            path: p.to_string(),
        }));
    }
    let pb = Path::new(t);
    let mut out = PathBuf::new();
    for c in pb.components() {
        match c {
            Component::RootDir => out.push("/"),
            Component::Normal(x) => {
                if x == ".." {
                    return Err(AppError::Fs(FsError::PathTraversal {
                        path: p.to_string(),
                    }));
                }
                out.push(x);
            }
            _ => {}
        }
    }
    Ok(out.to_string_lossy().replace('\\', "/"))
}

fn parse_find_mtime(s: &str) -> Option<String> {
    let t = s.trim();
    let sec: f64 = t.parse().ok()?;
    if !sec.is_finite() {
        return None;
    }
    let secs = sec.floor() as i64;
    let nanos = ((sec - sec.floor()) * 1e9).round() as u32;
    chrono::DateTime::from_timestamp(secs, nanos).map(|dt| dt.to_rfc3339())
}

async fn get_env_store(app_state: &OmigaAppState, session_id: &str) -> EnvStore {
    let sessions = app_state.chat.sessions.read().await;
    sessions
        .get(session_id)
        .map(|s| s.env_store.clone())
        .unwrap_or_else(EnvStore::new)
}

fn make_ctx(sandbox_backend: &str) -> ToolContext {
    ToolContext::new("/workspace")
        .with_execution_environment("sandbox")
        .with_sandbox_backend(sandbox_backend)
}

// ─── Commands ────────────────────────────────────────────────────────────────

/// List a directory inside the sandbox container.
/// Spins up the container on first call; reuses the cached env on subsequent calls.
#[tauri::command]
pub async fn sandbox_list_directory(
    app_state: tauri::State<'_, OmigaAppState>,
    session_id: String,
    sandbox_backend: String,
    path: String,
) -> CommandResult<DirectoryListResponse> {
    let env_store = get_env_store(&app_state, &session_id).await;
    let ctx = make_ctx(&sandbox_backend).with_env_store(Some(env_store.clone()));

    let dir = validate_sandbox_path(&path)?;
    let q = sh_quote(&dir);

    let script = format!(
        r#"if [ ! -d {q} ]; then echo "__OMIGA_ERR__not_a_dir"; exit 2; fi
printf '__OMIGA_DIR__%s\n' {q}
find {q} -mindepth 1 -maxdepth 1 -printf '%P\t%y\t%s\t%T@\n' 2>/dev/null | LC_ALL=C sort -f"#,
        q = q
    );

    let env_arc = env_store
        .get_or_create(&ctx, SANDBOX_TIMEOUT_MS)
        .await
        .map_err(|e| {
            AppError::Fs(FsError::IoError {
                message: e.to_string(),
            })
        })?;

    let result = {
        let mut guard = env_arc.lock().await;
        guard
            .execute(&script, ExecOptions::with_timeout(SANDBOX_TIMEOUT_MS))
            .await
    }
    .map_err(|e| {
        AppError::Fs(FsError::IoError {
            message: format!("sandbox execute: {}", e),
        })
    })?;

    if result.returncode != 0 {
        let msg = if result.output.contains("__OMIGA_ERR__not_a_dir") {
            format!("not a directory: {}", dir)
        } else {
            format!(
                "sandbox list_directory failed ({}): {}",
                result.returncode,
                result.output.trim()
            )
        };
        return Err(AppError::Fs(FsError::IoError { message: msg }));
    }

    let mut resolved_dir = dir.clone();
    let mut entries = Vec::new();

    for line in result.output.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if let Some(real_path) = line.strip_prefix("__OMIGA_DIR__") {
            resolved_dir = real_path.trim().to_string();
            continue;
        }
        let mut it = line.splitn(4, '\t');
        let name = it.next().unwrap_or("").to_string();
        let kind = it.next().unwrap_or("");
        let size_s = it.next().unwrap_or("0");
        let mtime_s = it.next().unwrap_or("");
        if name.is_empty() {
            continue;
        }
        let is_directory = kind == "d";
        let size = if is_directory {
            None
        } else {
            size_s.parse::<u64>().ok()
        };
        let modified = parse_find_mtime(mtime_s);
        let full_path = if resolved_dir.ends_with('/') {
            format!("{}{}", resolved_dir, name)
        } else {
            format!("{}/{}", resolved_dir, name)
        };
        entries.push(DirectoryEntry {
            name,
            path: full_path,
            is_directory,
            size,
            modified,
        });
    }

    entries.sort_by(|a, b| match (a.is_directory, b.is_directory) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
    });

    let total = entries.len();
    Ok(DirectoryListResponse {
        directory: resolved_dir,
        entries,
        total,
        has_more: false,
    })
}

/// Read a text file inside the sandbox container (up to 2 MiB).
#[tauri::command]
pub async fn sandbox_read_file(
    app_state: tauri::State<'_, OmigaAppState>,
    session_id: String,
    sandbox_backend: String,
    path: String,
    offset: Option<usize>,
    limit: Option<usize>,
) -> CommandResult<FileReadResponse> {
    let env_store = get_env_store(&app_state, &session_id).await;
    let ctx = make_ctx(&sandbox_backend).with_env_store(Some(env_store.clone()));

    let file = validate_sandbox_path(&path)?;
    let q = sh_quote(&file);

    let size_script = format!(
        r#"if [ ! -f {q} ]; then echo "__OMIGA_ERR__not_file"; exit 2; fi
wc -c < {q}"#,
        q = q
    );

    let env_arc = env_store
        .get_or_create(&ctx, SANDBOX_TIMEOUT_MS)
        .await
        .map_err(|e| {
            AppError::Fs(FsError::IoError {
                message: e.to_string(),
            })
        })?;

    let size_result = {
        let mut guard = env_arc.lock().await;
        guard
            .execute(&size_script, ExecOptions::with_timeout(30_000))
            .await
    }
    .map_err(|e| {
        AppError::Fs(FsError::IoError {
            message: format!("sandbox execute: {}", e),
        })
    })?;

    if size_result.returncode != 0 || size_result.output.contains("__OMIGA_ERR__") {
        return Err(AppError::Fs(FsError::InvalidPath { path: path.clone() }));
    }

    let nbytes: u64 = size_result
        .output
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .parse()
        .map_err(|_| {
            AppError::Fs(FsError::IoError {
                message: "sandbox wc failed".to_string(),
            })
        })?;

    if nbytes > MAX_READ_BYTES {
        return Err(AppError::Fs(FsError::FileTooLarge {
            path: path.clone(),
            size: nbytes,
            max: MAX_READ_BYTES,
        }));
    }

    let cat_result = {
        let mut guard = env_arc.lock().await;
        guard
            .execute(&format!("cat {}", q), ExecOptions::with_timeout(30_000))
            .await
    }
    .map_err(|e| {
        AppError::Fs(FsError::IoError {
            message: format!("sandbox execute: {}", e),
        })
    })?;

    let full = cat_result.output;
    let lines: Vec<&str> = full.lines().collect();
    let total_lines = lines.len();
    let off = offset.unwrap_or(0);
    let lim = limit.unwrap_or(usize::MAX);
    let slice: Vec<&str> = lines.iter().skip(off).take(lim).copied().collect();
    let content = slice.join("\n");
    let returned = slice.len();
    let has_more = off + returned < total_lines;

    Ok(FileReadResponse {
        content,
        total_lines,
        has_more,
    })
}

/// Write a file inside the sandbox container.
#[tauri::command]
pub async fn sandbox_write_file(
    app_state: tauri::State<'_, OmigaAppState>,
    session_id: String,
    sandbox_backend: String,
    path: String,
    content: String,
    _expected_hash: Option<String>,
) -> CommandResult<FileWriteResponse> {
    let env_store = get_env_store(&app_state, &session_id).await;
    let ctx = make_ctx(&sandbox_backend).with_env_store(Some(env_store.clone()));

    let file = validate_sandbox_path(&path)?;
    let q = sh_quote(&file);

    let parent = Path::new(&file)
        .parent()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| "/workspace".to_string());

    let env_arc = env_store
        .get_or_create(&ctx, SANDBOX_TIMEOUT_MS)
        .await
        .map_err(|e| {
            AppError::Fs(FsError::IoError {
                message: e.to_string(),
            })
        })?;

    {
        let mut guard = env_arc.lock().await;
        guard
            .execute(
                &format!("mkdir -p {}", sh_quote(&parent)),
                ExecOptions::with_timeout(10_000),
            )
            .await
            .map_err(|e| {
                AppError::Fs(FsError::IoError {
                    message: format!("mkdir: {}", e),
                })
            })?;
    }

    let bytes = content.len();
    let write_result = {
        let mut guard = env_arc.lock().await;
        guard
            .execute(
                &format!("cat > {}", q),
                ExecOptions {
                    timeout: Some(SANDBOX_TIMEOUT_MS),
                    stdin_data: Some(content),
                    cwd: None,
                },
            )
            .await
    }
    .map_err(|e| {
        AppError::Fs(FsError::IoError {
            message: format!("sandbox write: {}", e),
        })
    })?;

    if write_result.returncode != 0 {
        return Err(AppError::Fs(FsError::IoError {
            message: format!("sandbox write failed: {}", write_result.output.trim()),
        }));
    }

    Ok(FileWriteResponse {
        bytes_written: bytes,
        new_hash: String::new(),
    })
}
