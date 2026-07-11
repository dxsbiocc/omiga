//! Local command sandboxing support.
//!
//! One-shot local bash execution is routed through a platform backend:
//! macOS uses Seatbelt (`sandbox-exec`), Linux uses Landlock when the running
//! kernel supports it, and unsupported platforms deliberately fall through to
//! the existing local execution path.
//!
//! Sandbox denials are surfaced by the bash layer with the machine-readable
//! `SANDBOX_DENIED:` prefix. Approval UI for a single unsandboxed retry is a
//! follow-up outside this module.

mod landlock;
pub mod network;
pub mod proxy;
mod seatbelt;

pub use network::{HostRule, NetworkMode, NetworkPolicy};
pub use proxy::ensure_proxy_for_policy;
pub use proxy::{connect_allowed, NetworkPolicyProxy};

use std::path::{Path, PathBuf};
use tokio::process::Command;

/// Process-wide serialization lock for tests that mutate `OMIGA_SANDBOX_*`
/// environment variables. Because env vars are global to the process, every
/// sandbox test module (bash, network, seatbelt) must share a single lock —
/// `local_bash_command` reads the network env vars, so bash tests race with
/// network tests unless they serialize through the same mutex.
#[cfg(test)]
pub(crate) fn sandbox_env_test_lock() -> std::sync::MutexGuard<'static, ()> {
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    ENV_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxPolicy {
    pub network: NetworkPolicy,
}

impl Default for SandboxPolicy {
    fn default() -> Self {
        Self::from_env()
    }
}

impl SandboxPolicy {
    /// Builds the local sandbox policy from process environment.
    ///
    /// Network policy currently comes from environment variables to match the
    /// existing `OMIGA_SANDBOX_NETWORK=deny` convention:
    /// `OMIGA_SANDBOX_NETWORK_ALLOW=a.com,api.foo.com:443` or
    /// `OMIGA_SANDBOX_NETWORK_DENY=a.com,api.foo.com:443`.
    pub fn from_env() -> Self {
        Self {
            network: NetworkPolicy::from_env(),
        }
    }
}

pub fn is_supported() -> bool {
    #[cfg(target_os = "macos")]
    {
        return seatbelt::is_supported();
    }
    #[cfg(target_os = "linux")]
    {
        return landlock::is_supported();
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        false
    }
}

pub fn unavailable_reason() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        return seatbelt::unavailable_reason();
    }
    #[cfg(target_os = "linux")]
    {
        return landlock::unavailable_reason();
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        "local sandbox not available on this platform; running commands without a local sandbox"
    }
}

pub fn backend_name() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        return "seatbelt";
    }
    #[cfg(target_os = "linux")]
    {
        return "landlock";
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        "none"
    }
}

pub fn default_writable_roots(cwd: &Path) -> Vec<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        return seatbelt::default_writable_roots(cwd);
    }
    #[cfg(target_os = "linux")]
    {
        return landlock::default_writable_roots(cwd);
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        vec![cwd.to_path_buf()]
    }
}

#[cfg(target_os = "macos")]
pub use seatbelt::policy_text;

pub fn wrap_local_command(
    policy: &SandboxPolicy,
    writable_roots: &[PathBuf],
    command: &str,
    proxy_port: Option<u16>,
) -> Result<Command, String> {
    #[cfg(target_os = "macos")]
    {
        return Ok(seatbelt::wrap_local_command(
            policy,
            writable_roots,
            command,
            proxy_port,
        ));
    }
    #[cfg(target_os = "linux")]
    {
        return landlock::wrap_local_command(policy, writable_roots, command, proxy_port);
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = (policy, writable_roots, proxy_port);
        let mut cmd = Command::new("bash");
        cmd.arg("-l").arg("-c").arg(command);
        Ok(cmd)
    }
}

pub fn network_proxy_enforcement_enabled() -> bool {
    parse_network_proxy_enforcement_flag(
        std::env::var("OMIGA_SANDBOX_NETWORK_PROXY").ok().as_deref(),
    )
}

fn parse_network_proxy_enforcement_flag(raw: Option<&str>) -> bool {
    raw.map(str::trim).is_some_and(|value| {
        value.eq_ignore_ascii_case("1")
            || value.eq_ignore_ascii_case("true")
            || value.eq_ignore_ascii_case("yes")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn network_proxy_enforcement_flag_parsing() {
        assert!(!parse_network_proxy_enforcement_flag(None));
        assert!(!parse_network_proxy_enforcement_flag(Some("")));
        assert!(parse_network_proxy_enforcement_flag(Some("1")));
        assert!(parse_network_proxy_enforcement_flag(Some("true")));
        assert!(parse_network_proxy_enforcement_flag(Some("TRUE")));
        assert!(parse_network_proxy_enforcement_flag(Some("yes")));
        assert!(parse_network_proxy_enforcement_flag(Some("YeS")));
    }
}
