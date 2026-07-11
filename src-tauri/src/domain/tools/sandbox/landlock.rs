//! Linux Landlock sandbox backend.

#[cfg(target_os = "linux")]
use super::NetworkMode;
use super::SandboxPolicy;
use std::collections::BTreeSet;
#[cfg(target_os = "linux")]
use std::io;
use std::path::{Path, PathBuf};
#[cfg(target_os = "linux")]
use std::sync::OnceLock;
use tokio::process::Command;

#[allow(dead_code)]
pub fn is_supported() -> bool {
    #[cfg(target_os = "linux")]
    {
        static SUPPORTED: OnceLock<bool> = OnceLock::new();
        return *SUPPORTED.get_or_init(omiga_landlock::probe_supported);
    }
    #[cfg(not(target_os = "linux"))]
    {
        false
    }
}

#[allow(dead_code)]
pub fn unavailable_reason() -> &'static str {
    "Linux Landlock sandbox is unavailable because the running kernel does not support an enabled Landlock ABI"
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
    policy: &SandboxPolicy,
    writable_roots: &[PathBuf],
    command: &str,
    _proxy_port: Option<u16>,
) -> Result<Command, String> {
    let mut cmd = Command::new("bash");
    cmd.arg("-l").arg("-c").arg(command);

    #[cfg(target_os = "linux")]
    {
        let deny_network = matches!(policy.network.mode, NetworkMode::DenyAll);
        if deny_network && !omiga_landlock::probe_network_supported() {
            tracing::warn!(
                "landlock: kernel lacks network ABI (>= V4); enforcing filesystem sandbox only, \
                network deny is NOT active for this command"
            );
        }
        let spec = omiga_landlock::RestrictionSpec {
            writable_roots: writable_roots.to_vec(),
            deny_network,
        };
        let prepared = match omiga_landlock::prepare_restrictions(&spec) {
            Ok(prepared) => prepared,
            Err(error) => {
                tracing::warn!("landlock: failed to prepare sandbox ruleset: {error}");
                return Err(error.to_string());
            }
        };
        let warnings = prepared.warnings();
        if warnings.fs_write_partially_enforced {
            tracing::warn!(
                kernel_abi = warnings.kernel_abi.unwrap_or_default(),
                missing_truncate = warnings.missing_truncate,
                "landlock: kernel only partially supports requested filesystem write \
                restrictions; sandbox strength is weaker than seatbelt"
            );
        }
        let mut prepared = Some(prepared);
        unsafe {
            cmd.pre_exec(move || {
                let Some(prepared) = prepared.take() else {
                    return Err(io::Error::from_raw_os_error(nix::libc::EINVAL));
                };
                unsafe { prepared.apply_in_child() }.map_err(io::Error::from_raw_os_error)
            });
        }
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = (policy, writable_roots);
    }

    Ok(cmd)
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
