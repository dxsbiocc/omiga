//! Linux Landlock restrictions for local one-shot command execution.
//!
//! The public API is intentionally small so the Tauri crate only needs a thin
//! `cfg(target_os = "linux")` glue layer.

use std::fmt;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestrictionSpec {
    pub writable_roots: Vec<PathBuf>,
    pub deny_network: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RestrictionWarnings {
    pub fs_write_partially_enforced: bool,
    pub missing_truncate: bool,
    pub kernel_abi: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LandlockError {
    Unsupported(String),
    NoNewPrivs(String),
    Ruleset(String),
    NotEnforced(String),
}

impl fmt::Display for LandlockError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported(message)
            | Self::NoNewPrivs(message)
            | Self::Ruleset(message)
            | Self::NotEnforced(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for LandlockError {}

#[cfg(target_os = "linux")]
mod platform {
    use super::{LandlockError, RestrictionSpec, RestrictionWarnings};
    use landlock::{
        path_beneath_rules, Access, AccessFs, AccessNet, CompatLevel, Compatible, Ruleset,
        RulesetAttr, RulesetCreatedAttr, ABI,
    };
    use std::fmt;
    use std::os::fd::{IntoRawFd, OwnedFd};
    use std::ptr;

    const REQUESTED_FS_ABI: ABI = ABI::V7;
    const REQUESTED_NET_ABI: ABI = ABI::V4;
    const LANDLOCK_CREATE_RULESET_VERSION: libc::c_uint = 1;

    #[derive(Debug)]
    pub struct PreparedRestrictions {
        ruleset_fd: OwnedFd,
        warnings: RestrictionWarnings,
    }

    impl PreparedRestrictions {
        pub fn warnings(&self) -> RestrictionWarnings {
            self.warnings
        }

        /// Applies prepared restrictions after fork and before exec.
        ///
        /// # Safety
        ///
        /// This must only be called in the child-side pre-exec path or another
        /// context where applying Landlock to the current thread is intended.
        /// The implementation only issues raw libc syscalls and returns errno
        /// integers so callers can avoid allocation in pre-exec.
        pub unsafe fn apply_in_child(self) -> Result<(), i32> {
            let fd = self.ruleset_fd.into_raw_fd();
            if libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0 {
                let errno = last_errno();
                let _ = libc::close(fd);
                return Err(errno);
            }

            if libc::syscall(libc::SYS_landlock_restrict_self, fd, 0) != 0 {
                let errno = last_errno();
                let _ = libc::close(fd);
                return Err(errno);
            }

            let _ = libc::close(fd);
            Ok(())
        }
    }

    pub fn probe_supported() -> bool {
        Ruleset::default()
            .set_compatibility(CompatLevel::HardRequirement)
            .handle_access(AccessFs::from_all(ABI::V1))
            .and_then(|ruleset| ruleset.create())
            .is_ok()
    }

    /// Whether the running kernel can enforce Landlock TCP bind/connect
    /// restrictions (ABI >= V4). When false, `apply_restrictions` still
    /// enforces the filesystem policy but silently skips the network rules
    /// (BestEffort), so callers should surface the degradation themselves.
    pub fn probe_network_supported() -> bool {
        Ruleset::default()
            .set_compatibility(CompatLevel::HardRequirement)
            .handle_access(AccessNet::from_all(REQUESTED_NET_ABI))
            .and_then(|ruleset| ruleset.create())
            .is_ok()
    }

    pub fn probe_fs_abi() -> Option<i32> {
        let version = unsafe {
            libc::syscall(
                libc::SYS_landlock_create_ruleset,
                ptr::null::<libc::c_void>(),
                0usize,
                LANDLOCK_CREATE_RULESET_VERSION,
            )
        };

        (version > 0).then_some(version as i32)
    }

    pub fn prepare_restrictions(
        spec: &RestrictionSpec,
    ) -> Result<PreparedRestrictions, LandlockError> {
        let fs_all = AccessFs::from_all(REQUESTED_FS_ABI);
        let fs_read = AccessFs::from_read(REQUESTED_FS_ABI);
        let warnings = filesystem_warnings();
        let mut ruleset = Ruleset::default()
            .set_compatibility(CompatLevel::BestEffort)
            .handle_access(fs_all)
            .map_err(ruleset_error)?;

        if spec.deny_network {
            ruleset = ruleset
                .handle_access(AccessNet::from_all(REQUESTED_NET_ABI))
                .map_err(ruleset_error)?;
        }

        // Rule construction and path opening happen in the parent. The child
        // only applies no_new_privs and the prepared ruleset fd.
        let mut ruleset = ruleset
            .create()
            .map_err(ruleset_error)?
            .add_rules(path_beneath_rules(["/"], fs_read))
            .map_err(ruleset_error)?;

        if !spec.writable_roots.is_empty() {
            ruleset = ruleset
                .add_rules(path_beneath_rules(&spec.writable_roots, fs_all))
                .map_err(ruleset_error)?;
        }

        let ruleset_fd: Option<OwnedFd> = ruleset.into();
        let ruleset_fd = ruleset_fd.ok_or_else(|| {
            LandlockError::NotEnforced(
                "Landlock ruleset was created but not enforced by the kernel".to_string(),
            )
        })?;

        Ok(PreparedRestrictions {
            ruleset_fd,
            warnings,
        })
    }

    pub fn apply_restrictions(spec: &RestrictionSpec) -> Result<(), LandlockError> {
        let prepared = prepare_restrictions(spec)?;
        unsafe {
            prepared.apply_in_child().map_err(|errno| {
                LandlockError::Ruleset(format!(
                    "failed to apply Landlock restrictions: {}",
                    std::io::Error::from_raw_os_error(errno)
                ))
            })
        }
    }

    fn filesystem_warnings() -> RestrictionWarnings {
        let Some(kernel_abi) = probe_fs_abi() else {
            return RestrictionWarnings::default();
        };
        let kernel_write = AccessFs::from_write(ABI::from(kernel_abi));
        let requested_write = AccessFs::from_write(REQUESTED_FS_ABI);

        RestrictionWarnings {
            fs_write_partially_enforced: kernel_write != requested_write,
            missing_truncate: kernel_abi < 3,
            kernel_abi: Some(kernel_abi),
        }
    }

    fn last_errno() -> i32 {
        unsafe { *libc::__errno_location() }
    }

    fn ruleset_error(error: impl fmt::Display) -> LandlockError {
        LandlockError::Ruleset(format!("failed to configure Landlock ruleset: {error}"))
    }
}

#[cfg(not(target_os = "linux"))]
mod platform {
    use super::{LandlockError, RestrictionSpec, RestrictionWarnings};

    #[derive(Debug)]
    pub struct PreparedRestrictions;

    impl PreparedRestrictions {
        pub fn warnings(&self) -> RestrictionWarnings {
            RestrictionWarnings::default()
        }

        pub unsafe fn apply_in_child(self) -> Result<(), i32> {
            Err(38)
        }
    }

    pub fn probe_supported() -> bool {
        false
    }

    pub fn probe_network_supported() -> bool {
        false
    }

    pub fn probe_fs_abi() -> Option<i32> {
        None
    }

    pub fn prepare_restrictions(
        _spec: &RestrictionSpec,
    ) -> Result<PreparedRestrictions, LandlockError> {
        Err(LandlockError::Unsupported(
            "Linux Landlock sandbox is not available on this platform".to_string(),
        ))
    }

    pub fn apply_restrictions(_spec: &RestrictionSpec) -> Result<(), LandlockError> {
        Err(LandlockError::Unsupported(
            "Linux Landlock sandbox is not available on this platform".to_string(),
        ))
    }
}

pub use platform::{
    apply_restrictions, prepare_restrictions, probe_fs_abi, probe_network_supported,
    probe_supported, PreparedRestrictions,
};

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn spec_keeps_writable_roots_and_network_flag() {
        let spec = RestrictionSpec {
            writable_roots: vec![PathBuf::from("/tmp"), PathBuf::from("/workspace")],
            deny_network: true,
        };

        assert_eq!(spec.writable_roots.len(), 2);
        assert!(spec.deny_network);
    }

    #[test]
    fn error_messages_are_human_readable() {
        let error = LandlockError::Unsupported("not supported here".to_string());
        assert_eq!(error.to_string(), "not supported here");
    }

    #[test]
    fn prepared_restrictions_is_send_sync() {
        assert_send_sync::<PreparedRestrictions>();
    }

    #[cfg(not(target_os = "linux"))]
    #[test]
    fn non_linux_stub_reports_unsupported() {
        let spec = RestrictionSpec {
            writable_roots: Vec::new(),
            deny_network: false,
        };

        assert!(!probe_supported());
        match apply_restrictions(&spec) {
            Err(LandlockError::Unsupported(message)) => {
                assert!(message.contains("not available"));
            }
            other => panic!("expected Unsupported, got {other:?}"),
        }
        assert!(probe_fs_abi().is_none());
        assert!(prepare_restrictions(&spec).is_err());
    }
}
