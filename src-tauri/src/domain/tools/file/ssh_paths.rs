//! When `execution_environment == "ssh"`, workspace paths are remote POSIX paths
//! (stored as `PathBuf` on the client). Resolve tool paths without local `canonicalize`.

use crate::errors::FsError;
use std::path::Path;

/// Single-root resolution: absolute paths must stay under `project_root`.
pub fn resolve_ssh_path(project_root: &Path, path_arg: &str) -> Result<String, FsError> {
    let p = path_arg.trim();
    if p.is_empty() {
        return Err(FsError::InvalidPath {
            path: path_arg.to_string(),
        });
    }
    let root_s = project_root.to_string_lossy().replace('\\', "/");
    let root_s = root_s.trim_end_matches('/').to_string();
    if p.starts_with('/') {
        let norm = p.replace('\\', "/");
        if norm == root_s || norm.starts_with(&(root_s.clone() + "/")) {
            return Ok(norm);
        }
        return Err(FsError::PathTraversal {
            path: path_arg.to_string(),
        });
    }
    Ok(format!("{}/{}", root_s, p.trim_start_matches('/')))
}

pub fn resolve_bash_cwd_ssh(
    project_root: &Path,
    default_cwd: &Path,
    cwd_arg: Option<&str>,
) -> Result<String, FsError> {
    match cwd_arg {
        None => Ok(default_cwd.to_string_lossy().replace('\\', "/")),
        Some(p) if p.trim().is_empty() => Ok(default_cwd.to_string_lossy().replace('\\', "/")),
        Some(p) => resolve_ssh_path(project_root, p),
    }
}
