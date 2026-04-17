//! Remote file operations over SSH (POSIX paths on the server).
//! Used when the chat composer uses execution environment **SSH** and the session
//! workspace path is a remote absolute path (e.g. `/home/ubuntu/project`).

use super::CommandResult;
use crate::commands::execution_envs::get_merged_ssh_configs;
use crate::commands::fs::{
    DirectoryEntry, DirectoryListResponse, FileReadResponse, FileWriteResponse,
};
use crate::errors::{AppError, FsError};
use crate::llm::config::SshExecConfig;
use std::path::{Component, Path, PathBuf};
use tokio::process::Command;
use tokio::time::{timeout, Duration};

const SSH_TIMEOUT: Duration = Duration::from_secs(60);
const MAX_READ_BYTES: u64 = 2 * 1024 * 1024;

fn expand_tilde_identity(p: &str) -> String {
    if let Some(rest) = p.strip_prefix("~/") {
        dirs::home_dir()
            .map(|h| h.join(rest).to_string_lossy().into_owned())
            .unwrap_or_else(|| p.to_string())
    } else {
        p.to_string()
    }
}

fn validate_remote_path(p: &str) -> Result<String, AppError> {
    let t = p.trim();
    if t.is_empty() {
        return Err(AppError::Fs(FsError::InvalidPath {
            path: p.to_string(),
        }));
    }
    if t.contains('\n') || t.contains('\0') {
        return Err(AppError::Fs(FsError::InvalidPath {
            path: p.to_string(),
        }));
    }
    // Allow home-relative paths: "~" or "~/subpath" — expanded to $HOME on the remote side.
    if t == "~" {
        return Ok("~".to_string());
    }
    if let Some(rest) = t.strip_prefix("~/") {
        // Validate the suffix (no traversal)
        if rest.split('/').any(|seg| seg == "..") {
            return Err(AppError::Fs(FsError::PathTraversal {
                path: p.to_string(),
            }));
        }
        return Ok(format!("~/{}", rest));
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

fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\"'\"'"))
}

fn resolve_profile(name: &str) -> Result<SshExecConfig, AppError> {
    let merged =
        get_merged_ssh_configs().map_err(|e| AppError::Fs(FsError::IoError { message: e }))?;
    let cfg = merged.get(name).cloned().ok_or_else(|| {
        AppError::Fs(FsError::IoError {
            message: format!("Unknown SSH profile: {}", name),
        })
    })?;
    let _ = cfg.effective_hostname().ok_or_else(|| {
        AppError::Fs(FsError::IoError {
            message: format!("SSH profile `{}` has no HostName", name),
        })
    })?;
    let _ = cfg
        .user
        .as_ref()
        .filter(|u| !u.trim().is_empty())
        .ok_or_else(|| {
            AppError::Fs(FsError::IoError {
                message: format!("SSH profile `{}` has no User", name),
            })
        })?;
    Ok(cfg)
}

fn ssh_base_args(cfg: &SshExecConfig) -> Vec<String> {
    let mut args = vec![
        "-o".to_string(),
        "BatchMode=yes".to_string(),
        "-o".to_string(),
        "StrictHostKeyChecking=yes".to_string(),
        "-o".to_string(),
        "ConnectTimeout=15".to_string(),
    ];
    if cfg.port != 22 {
        args.push("-p".to_string());
        args.push(cfg.port.to_string());
    }
    if let Some(ref id) = cfg.identity_file {
        let exp = expand_tilde_identity(id);
        args.push("-i".to_string());
        args.push(exp);
    }
    args
}

async fn ssh_run_remote(
    cfg: &SshExecConfig,
    remote_bash_script: &str,
) -> Result<(i32, String, String), AppError> {
    let host = cfg.effective_hostname().ok_or_else(|| {
        AppError::Fs(FsError::IoError {
            message: "SSH: missing HostName".to_string(),
        })
    })?;
    let user = cfg.user.as_ref().unwrap();
    let target = format!("{}@{}", user, host);
    let mut args = ssh_base_args(cfg);
    args.push(target);
    args.push(format!("bash -lc {}", sh_quote(remote_bash_script)));

    let fut = async {
        let out = Command::new("ssh")
            .args(&args)
            .output()
            .await
            .map_err(|e| {
                AppError::Fs(FsError::IoError {
                    message: format!("ssh: {}", e),
                })
            })?;
        let code = out.status.code().unwrap_or(-1);
        let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
        Ok::<_, AppError>((code, stdout, stderr))
    };
    timeout(SSH_TIMEOUT, fut).await.map_err(|_| {
        AppError::Fs(FsError::IoError {
            message: "ssh: timed out".to_string(),
        })
    })?
}

/// Expand a validated remote path — replace leading `~` with `$HOME` so it works
/// inside double quotes in bash scripts.
fn expand_tilde_to_home_var(p: &str) -> String {
    if p == "~" {
        "$HOME".to_string()
    } else if let Some(rest) = p.strip_prefix("~/") {
        format!("$HOME/{}", rest)
    } else {
        p.to_string()
    }
}

/// Quote a path for use in a double-quoted bash string (safe for `$HOME` expansion).
fn sh_double_quote(s: &str) -> String {
    // Inside double quotes: escape \, ", $, ` — but we intentionally allow $HOME to expand.
    // Since our validated paths never contain $, `, or \ except via $HOME, we only
    // escape " and literal backslashes.
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Get the remote user's home directory.
#[tauri::command]
pub async fn ssh_get_home_directory(ssh_profile_name: String) -> CommandResult<String> {
    let cfg = resolve_profile(&ssh_profile_name)?;
    let script = "echo $HOME";
    let (code, stdout, stderr) = ssh_run_remote(&cfg, script).await?;
    if code != 0 {
        return Err(AppError::Fs(FsError::IoError {
            message: format!(
                "ssh get home directory failed ({}): {}",
                code,
                stderr.trim()
            ),
        }));
    }
    let home = stdout.trim().to_string();
    Ok(home)
}

/// List a directory on the remote host (GNU `find` with `-printf`, common on Linux).
#[tauri::command]
pub async fn ssh_list_directory(
    ssh_profile_name: String,
    path: String,
) -> CommandResult<DirectoryListResponse> {
    let cfg = resolve_profile(&ssh_profile_name)?;
    let dir = validate_remote_path(&path)?;
    // Use $HOME-expanded path so tilde works inside double-quoted bash strings.
    let expanded = expand_tilde_to_home_var(&dir);
    let q = sh_double_quote(&expanded);
    let script = format!(
        // First line: echo the real absolute path (resolves $HOME) prefixed with marker.
        // Remaining lines: find output.
        r#"if [ ! -d {q} ]; then echo "__OMIGA_ERR__not_a_dir"; exit 2; fi
printf '__OMIGA_DIR__%s\n' {q}
find {q} -mindepth 1 -maxdepth 1 -printf '%P\t%y\t%s\t%T@\n' 2>/dev/null | LC_ALL=C sort -f"#,
        q = q
    );
    let (code, stdout, stderr) = ssh_run_remote(&cfg, &script).await?;
    if code != 0 {
        let msg = if stdout.contains("__OMIGA_ERR__not_a_dir") {
            format!("not a directory: {}", dir)
        } else {
            format!("ssh list_directory failed ({}): {}", code, stderr.trim())
        };
        return Err(AppError::Fs(FsError::IoError { message: msg }));
    }

    // Extract the real resolved directory path from the first marker line.
    let mut resolved_dir = dir.clone();
    let mut entries = Vec::new();
    for line in stdout.lines() {
        if line.trim().is_empty() {
            continue;
        }
        // Marker line: "__OMIGA_DIR__%s" — real expanded absolute path.
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

/// Read a text file from the remote host (up to 2 MiB).
#[tauri::command]
pub async fn ssh_read_file(
    ssh_profile_name: String,
    path: String,
    offset: Option<usize>,
    limit: Option<usize>,
) -> CommandResult<FileReadResponse> {
    let cfg = resolve_profile(&ssh_profile_name)?;
    let file = validate_remote_path(&path)?;
    let q = sh_double_quote(&expand_tilde_to_home_var(&file));
    let script = format!(
        r#"if [ ! -f {q} ]; then echo "__OMIGA_ERR__not_file"; exit 2; fi
wc -c < {q}"#,
        q = q
    );
    let (code, stdout, stderr) = ssh_run_remote(&cfg, &script).await?;
    if code != 0 || stdout.contains("__OMIGA_ERR__") {
        return Err(AppError::Fs(FsError::InvalidPath { path: path.clone() }));
    }
    let size_line = stdout.lines().next().unwrap_or("").trim();
    let nbytes: u64 = size_line.parse().map_err(|_| {
        AppError::Fs(FsError::IoError {
            message: format!("ssh wc: {}", stderr),
        })
    })?;
    if nbytes > MAX_READ_BYTES {
        return Err(AppError::Fs(FsError::FileTooLarge {
            path: path.clone(),
            size: nbytes,
            max: MAX_READ_BYTES,
        }));
    }

    let cat_script = format!(
        r#"cat {}"#,
        sh_double_quote(&expand_tilde_to_home_var(&file))
    );
    let (_c2, full, e2) = ssh_run_remote(&cfg, &cat_script).await?;
    if !e2.trim().is_empty() && full.is_empty() {
        return Err(AppError::Fs(FsError::IoError { message: e2 }));
    }

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

/// Write a file on the remote host (creates or overwrites; parents created with `mkdir -p`).
#[tauri::command]
pub async fn ssh_write_file(
    ssh_profile_name: String,
    path: String,
    content: String,
    _expected_hash: Option<String>,
) -> CommandResult<FileWriteResponse> {
    let cfg = resolve_profile(&ssh_profile_name)?;
    let file = validate_remote_path(&path)?;
    let file_q = sh_double_quote(&expand_tilde_to_home_var(&file));
    let parent_expanded = {
        let exp = expand_tilde_to_home_var(&file);
        Path::new(&exp)
            .parent()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| "$HOME".to_string())
    };
    let mkdir = format!("mkdir -p {}", sh_double_quote(&parent_expanded));
    let (_a, _, em) = ssh_run_remote(&cfg, &mkdir).await?;
    if !em.trim().is_empty() {
        tracing::warn!(target: "ssh_fs", "mkdir stderr: {}", em);
    }

    // Write via stdin: ssh ... "cat > file"
    let host = cfg.effective_hostname().unwrap();
    let user = cfg.user.as_ref().unwrap();
    let target = format!("{}@{}", user, host);
    let mut args = ssh_base_args(&cfg);
    args.push(target);
    args.push(format!(
        "bash -lc {}",
        sh_quote(&format!("cat > {}", file_q))
    ));

    let bytes = content.as_bytes();
    let fut = async {
        let mut child = Command::new("ssh")
            .args(&args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| {
                AppError::Fs(FsError::IoError {
                    message: format!("ssh: {}", e),
                })
            })?;
        if let Some(mut stdin) = child.stdin.take() {
            use tokio::io::AsyncWriteExt;
            stdin.write_all(bytes).await.map_err(|e| {
                AppError::Fs(FsError::IoError {
                    message: format!("ssh stdin: {}", e),
                })
            })?;
        }
        let out = child.wait_with_output().await.map_err(|e| {
            AppError::Fs(FsError::IoError {
                message: format!("ssh: {}", e),
            })
        })?;
        Ok::<_, AppError>(out)
    };
    let out = timeout(SSH_TIMEOUT, fut).await.map_err(|_| {
        AppError::Fs(FsError::IoError {
            message: "ssh write timed out".to_string(),
        })
    })??;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(AppError::Fs(FsError::IoError {
            message: format!("ssh write failed: {}", err.trim()),
        }));
    }
    Ok(FileWriteResponse {
        bytes_written: bytes.len(),
        new_hash: String::new(),
    })
}

/// Create a directory on the remote host (parents created with `mkdir -p`).
#[tauri::command]
pub async fn ssh_create_directory(ssh_profile_name: String, path: String) -> CommandResult<String> {
    let cfg = resolve_profile(&ssh_profile_name)?;
    let dir = validate_remote_path(&path)?;
    let dir_q = sh_double_quote(&expand_tilde_to_home_var(&dir));
    let script = format!("mkdir -p {}", dir_q);
    let (code, _stdout, stderr) = ssh_run_remote(&cfg, &script).await?;
    if code != 0 {
        return Err(AppError::Fs(FsError::IoError {
            message: format!("ssh mkdir failed ({}): {}", code, stderr.trim()),
        }));
    }
    Ok(dir)
}
