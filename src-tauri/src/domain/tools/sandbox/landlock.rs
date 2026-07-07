//! Linux Landlock sandbox skeleton.
//!
//! This module intentionally does not claim enforcement yet. Without wiring the
//! `landlock` crate and Linux-specific spawn path, Linux local bash commands
//! continue through the existing direct execution path. The policy-layer pieces
//! are still present and tested so the future crate integration has a stable
//! shape.

use super::SandboxPolicy;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use tokio::process::Command;

#[allow(dead_code)]
pub fn is_supported() -> bool {
    false
}

#[allow(dead_code)]
pub fn unavailable_reason() -> &'static str {
    "Linux Landlock sandbox is not yet wired; running commands without a local sandbox"
}

#[allow(dead_code)]
pub fn default_writable_roots(cwd: &Path) -> Vec<PathBuf> {
    let mut roots = vec![cwd.to_path_buf()];

    if let Some(tmpdir) = std::env::var_os("TMPDIR").filter(|value| !value.is_empty()) {
        roots.push(PathBuf::from(tmpdir));
    }

    roots.extend([PathBuf::from("/tmp"), PathBuf::from("/var/tmp")]);
    dedupe_paths(roots)
}

#[allow(dead_code)]
pub fn wrap_local_command(
    _policy: &SandboxPolicy,
    _writable_roots: &[PathBuf],
    command: &str,
) -> Command {
    let mut cmd = Command::new("bash");
    cmd.arg("-l").arg("-c").arg(command);
    cmd
}

#[allow(dead_code)]
fn dedupe_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();

    for path in paths {
        if path.as_os_str().is_empty() {
            continue;
        }
        let candidates = match path.canonicalize() {
            Ok(canonical) if canonical != path => vec![path, canonical],
            _ => vec![path],
        };
        for candidate in candidates {
            let key = candidate.to_string_lossy().to_string();
            if seen.insert(key) {
                out.push(candidate);
            }
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard};

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn env_lock() -> MutexGuard<'static, ()> {
        ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    #[test]
    fn landlock_is_not_supported_on_current_macos_host() {
        if cfg!(target_os = "macos") {
            assert!(!is_supported());
        }
    }

    #[test]
    fn writable_roots_include_cwd_tmpdir_and_linux_tmp_defaults() {
        let _guard = env_lock();
        let previous = std::env::var_os("TMPDIR");
        std::env::set_var("TMPDIR", "/tmp/omiga-landlock-env-tmp");

        let roots = default_writable_roots(Path::new("/workspace/project"));

        assert!(roots.contains(&PathBuf::from("/workspace/project")));
        assert!(roots.contains(&PathBuf::from("/tmp/omiga-landlock-env-tmp")));
        assert!(roots.contains(&PathBuf::from("/tmp")));
        assert!(roots.contains(&PathBuf::from("/var/tmp")));

        match previous {
            Some(value) => std::env::set_var("TMPDIR", value),
            None => std::env::remove_var("TMPDIR"),
        }
    }

    #[test]
    fn writable_roots_are_deduped() {
        let path = PathBuf::from("/tmp/omiga-landlock-nonexistent-dedupe");
        let roots = dedupe_paths(vec![path.clone(), path.clone()]);
        assert_eq!(roots, vec![path]);
    }
}
