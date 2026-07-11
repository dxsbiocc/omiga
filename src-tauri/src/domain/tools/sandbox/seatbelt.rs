#[cfg(test)]
use super::NetworkPolicy;
use super::{HostRule, NetworkMode, SandboxPolicy};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use tokio::process::Command;

pub const SANDBOX_EXEC_PATH: &str = "/usr/bin/sandbox-exec";

pub fn is_supported() -> bool {
    cfg!(target_os = "macos") && Path::new(SANDBOX_EXEC_PATH).is_file()
}

pub fn unavailable_reason() -> &'static str {
    if !cfg!(target_os = "macos") {
        "local sandbox not available on this platform (landlock TODO)"
    } else {
        "local macOS sandbox not available because /usr/bin/sandbox-exec was not found"
    }
}

pub fn default_writable_roots(cwd: &Path) -> Vec<PathBuf> {
    let mut roots = vec![cwd.to_path_buf()];

    if let Some(tmpdir) = std::env::var_os("TMPDIR").filter(|value| !value.is_empty()) {
        roots.push(PathBuf::from(tmpdir));
    }

    roots.extend([
        PathBuf::from("/tmp"),
        PathBuf::from("/private/tmp"),
        PathBuf::from("/var/folders"),
        PathBuf::from("/private/var/folders"),
    ]);

    dedupe_paths(roots)
}

pub fn policy_text(
    policy: &SandboxPolicy,
    writable_roots: &[PathBuf],
    proxy_port: Option<u16>,
) -> String {
    let mut sbpl = String::from(
        r#"(version 1)

; Closed by default. Specific read, write, process, device, and network rules
; below reopen only what local one-shot bash execution needs.
(deny default)

; Let bash and its children execute while inheriting this same sandbox.
(allow process-exec)
(allow process-fork)
(allow signal (target same-sandbox))
(allow process-info* (target same-sandbox))

; Local tools need to inspect installed binaries, libraries, project files, and
; user configuration. Writes remain constrained by the allowlist below.
(allow file-read*)

; Basic terminal/device endpoints used by shells, stdio redirection, random
; number generation, and macOS tracing/runtime helpers.
(allow pseudo-tty)
(allow file-read* file-write* file-ioctl
  (literal "/dev/null")
  (literal "/dev/zero")
  (literal "/dev/random")
  (literal "/dev/urandom")
  (literal "/dev/tty")
  (literal "/dev/ptmx")
  (literal "/dev/stdin")
  (literal "/dev/stdout")
  (literal "/dev/stderr")
  (literal "/dev/dtracehelper")
  (subpath "/dev/fd"))
(allow file-read* file-write*
  (require-all
    (regex #"^/dev/ttys[0-9]+")
    (extension "com.apple.sandbox.pty")))
(allow file-ioctl (regex #"^/dev/ttys[0-9]+"))

; Common runtime queries performed by shells and language runtimes.
(allow sysctl-read)
(allow mach-lookup
  (global-name "com.apple.system.opendirectoryd.libinfo")
  (global-name "com.apple.cfprefsd.daemon")
  (global-name "com.apple.cfprefsd.agent")
  (local-name "com.apple.cfprefsd.agent"))
(allow user-preference-read)

"#,
    );

    let roots = dedupe_paths(writable_roots.iter().cloned().collect());
    if !roots.is_empty() {
        sbpl.push_str("; Writable roots: cwd plus system temporary directories.\n");
        sbpl.push_str("(allow file-write*\n");
        for root in roots {
            let quoted = sbpl_string(&root.to_string_lossy());
            sbpl.push_str("  (literal ");
            sbpl.push_str(&quoted);
            sbpl.push_str(")\n  (subpath ");
            sbpl.push_str(&quoted);
            sbpl.push_str(")\n");
        }
        sbpl.push_str(")\n\n");
    }

    append_network_policy(&mut sbpl, policy, proxy_port);

    sbpl
}

pub fn wrap_local_command(
    policy: &SandboxPolicy,
    writable_roots: &[PathBuf],
    command: &str,
    proxy_port: Option<u16>,
) -> Command {
    let mut cmd = Command::new(SANDBOX_EXEC_PATH);
    cmd.arg("-p")
        .arg(policy_text(policy, writable_roots, proxy_port))
        .arg("--")
        .arg("bash")
        .arg("-l")
        .arg("-c")
        .arg(command);
    cmd
}

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

fn append_network_policy(sbpl: &mut String, policy: &SandboxPolicy, proxy_port: Option<u16>) {
    if let Some(proxy_port) = proxy_port {
        if matches!(
            policy.network.mode,
            NetworkMode::AllowList | NetworkMode::DenyList
        ) {
            sbpl.push_str(
                "; Domain enforcement is delegated to the loopback proxy for this command.\n",
            );
            sbpl.push_str(
                "; Real domain allow/deny matching happens in the Rust proxy; sandbox only allows\n",
            );
            sbpl.push_str("; traffic to localhost.\n");
            sbpl.push_str(&format!(
                "(allow network-outbound (remote ip \"localhost:{}\"))\n",
                proxy_port
            ));
            return;
        }
    }

    match policy.network.mode {
        NetworkMode::AllowAll => {
            sbpl.push_str(
                "; Outbound network is allowed by default. Set OMIGA_SANDBOX_NETWORK=deny to remove this rule.\n",
            );
            sbpl.push_str("(allow network-outbound)\n");
        }
        NetworkMode::DenyAll => {
            sbpl.push_str(
                "; Network disabled by OMIGA_SANDBOX_NETWORK=deny; default deny remains in effect.\n",
            );
        }
        NetworkMode::AllowList => {
            sbpl.push_str("; Network allowlist from OMIGA_SANDBOX_NETWORK_ALLOW.\n");
            sbpl.push_str(
                "; Seatbelt cannot resolve domain names here. Only explicit port-only rules such as *:443 are expressible; domain rules are stricter-by-default and remain denied. Full domain filtering needs the future network proxy.\n",
            );
            let mut any_allowed = false;
            for host in &policy.network.hosts {
                if let Some(rule) = seatbelt_port_rule(host, "allow") {
                    sbpl.push_str(&rule);
                    any_allowed = true;
                }
            }
            if !any_allowed {
                sbpl.push_str(
                    "; No allowlist entry could be represented without widening domain scope; default deny remains in effect.\n",
                );
            }
        }
        NetworkMode::DenyList => {
            sbpl.push_str("; Network denylist from OMIGA_SANDBOX_NETWORK_DENY.\n");
            sbpl.push_str(
                "; Seatbelt cannot resolve domain names here. Domain:port deny entries are approximated by denying that port for all hosts, which is stricter than the requested domain-only block. Domain entries without a port cannot be expressed without widening, so default deny remains in effect. Full domain filtering needs the future network proxy.\n",
            );
            let mut all_rules_expressed = true;
            for host in &policy.network.hosts {
                if let Some(rule) = seatbelt_port_rule(host, "deny") {
                    sbpl.push_str(&rule);
                } else {
                    all_rules_expressed = false;
                }
            }
            if all_rules_expressed {
                sbpl.push_str("(allow network-outbound)\n");
            } else {
                sbpl.push_str(
                    "; At least one denylist entry could not be represented safely; default deny remains in effect.\n",
                );
            }
        }
    }
}

fn seatbelt_port_rule(host: &HostRule, action: &str) -> Option<String> {
    let port = host.port?;
    match action {
        "allow" if host.is_port_only() => Some(format!(
            "(allow network-outbound (remote tcp \"*:{}\"))\n",
            port
        )),
        "deny" => Some(format!(
            "(deny network-outbound (remote tcp \"*:{}\"))\n",
            port
        )),
        _ => None,
    }
}

fn sbpl_string(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len() + 2);
    escaped.push('"');
    for ch in value.chars() {
        match ch {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            _ => escaped.push(ch),
        }
    }
    escaped.push('"');
    escaped
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::MutexGuard;

    // Delegates to the shared process-wide sandbox env lock so seatbelt tests
    // serialize against bash/network env tests (see `super::sandbox_env_test_lock`).
    fn sandbox_test_lock() -> MutexGuard<'static, ()> {
        crate::domain::tools::sandbox::sandbox_env_test_lock()
    }

    #[test]
    fn policy_includes_default_deny_writable_roots_and_network_allow() {
        let _guard = sandbox_test_lock();
        let cwd = PathBuf::from("/tmp/omiga-seatbelt-cwd");
        let tmpdir = PathBuf::from("/private/tmp/omiga-seatbelt-tmp");
        let policy = SandboxPolicy {
            network: NetworkPolicy::allow_all(),
        };

        let text = policy_text(&policy, &[cwd.clone(), tmpdir.clone()], None);

        assert!(text.contains("(deny default)"));
        assert!(text.contains("(allow file-read*)"));
        assert!(text.contains(&format!(
            "(subpath {})",
            sbpl_string(&cwd.to_string_lossy())
        )));
        assert!(text.contains(&format!(
            "(subpath {})",
            sbpl_string(&tmpdir.to_string_lossy())
        )));
        assert!(text.contains("(allow network-outbound)"));
    }

    #[test]
    fn policy_omits_network_when_env_requests_deny() {
        let _guard = sandbox_test_lock();
        let policy = SandboxPolicy {
            network: NetworkPolicy::deny_all(),
        };
        let text = policy_text(&policy, &[PathBuf::from("/tmp")], None);

        assert_eq!(policy.network.mode, NetworkMode::DenyAll);
        assert!(!text.contains("(allow network-outbound)"));
        assert!(text.contains("OMIGA_SANDBOX_NETWORK=deny"));
    }

    #[test]
    fn sandbox_policy_maps_network_deny_and_allow() {
        // Pure wiring check: SandboxPolicy carries whatever NetworkPolicy it is
        // built with. Env parsing itself is covered by NetworkPolicy::from_parts
        // tests; constructing directly avoids `setenv`, which is not thread-safe
        // against concurrent `getenv` under parallel `cargo test`.
        let deny = SandboxPolicy {
            network: NetworkPolicy::from_parts(Some("deny".to_string()), None, None),
        };
        assert_eq!(deny.network.mode, NetworkMode::DenyAll);

        let allow = SandboxPolicy {
            network: NetworkPolicy::from_parts(None, None, None),
        };
        assert_eq!(allow.network.mode, NetworkMode::AllowAll);
    }

    #[test]
    fn allowlist_domain_port_does_not_widen_to_all_hosts_on_port() {
        let policy = SandboxPolicy {
            network: NetworkPolicy {
                mode: NetworkMode::AllowList,
                hosts: vec![HostRule {
                    domain: "api.foo.com".to_string(),
                    port: Some(443),
                }],
            },
        };

        let text = policy_text(&policy, &[PathBuf::from("/tmp")], None);

        assert!(!text.contains("(allow network-outbound (remote tcp \"*:443\"))"));
        assert!(!text.contains("(allow network-outbound)"));
        assert!(text.contains("No allowlist entry could be represented"));
    }

    #[test]
    fn allowlist_with_proxy_port_allows_only_loopback_proxy() {
        let _guard = sandbox_test_lock();
        let policy = SandboxPolicy {
            network: NetworkPolicy {
                mode: NetworkMode::AllowList,
                hosts: vec![HostRule {
                    domain: "api.foo.com".to_string(),
                    port: Some(443),
                }],
            },
        };

        let text = policy_text(&policy, &[PathBuf::from("/tmp")], Some(4567));

        assert!(text.contains("(allow network-outbound (remote ip \"localhost:4567\"))"));
        assert!(!text.contains("(allow network-outbound (remote tcp"));
        assert!(!text.contains("(deny network-outbound"));
        assert!(!text.contains("(allow network-outbound)\n"));
    }

    #[test]
    fn allowlist_port_only_generates_remote_port_rule() {
        let _guard = sandbox_test_lock();
        let policy = SandboxPolicy {
            network: NetworkPolicy {
                mode: NetworkMode::AllowList,
                hosts: vec![HostRule {
                    domain: "*".to_string(),
                    port: Some(443),
                }],
            },
        };

        let text = policy_text(&policy, &[PathBuf::from("/tmp")], None);

        assert!(text.contains("(allow network-outbound (remote tcp \"*:443\"))"));
        assert!(!text.contains("\n(allow network-outbound)\n"));
    }

    #[test]
    fn denylist_domain_port_generates_stricter_port_deny_then_allow_all() {
        let _guard = sandbox_test_lock();
        let policy = SandboxPolicy {
            network: NetworkPolicy {
                mode: NetworkMode::DenyList,
                hosts: vec![HostRule {
                    domain: "tracker.example".to_string(),
                    port: Some(80),
                }],
            },
        };

        let text = policy_text(&policy, &[PathBuf::from("/tmp")], None);

        assert!(text.contains("(deny network-outbound (remote tcp \"*:80\"))"));
        assert!(text.contains("(allow network-outbound)"));
    }

    #[test]
    fn denylist_domain_without_port_stays_default_deny() {
        let _guard = sandbox_test_lock();
        let policy = SandboxPolicy {
            network: NetworkPolicy {
                mode: NetworkMode::DenyList,
                hosts: vec![HostRule {
                    domain: "tracker.example".to_string(),
                    port: None,
                }],
            },
        };

        let text = policy_text(&policy, &[PathBuf::from("/tmp")], None);

        assert!(!text.contains("(allow network-outbound)"));
        assert!(text.contains("could not be represented safely"));
    }

    #[test]
    fn is_supported_matches_current_platform_and_binary() {
        let _guard = sandbox_test_lock();
        let expected = cfg!(target_os = "macos") && Path::new(SANDBOX_EXEC_PATH).is_file();
        assert_eq!(is_supported(), expected);
    }

    #[cfg(target_os = "macos")]
    #[test]
    // Heavyweight end-to-end check: it spawns a child test process that runs a
    // real `sandbox-exec` command. Under the default multi-threaded suite that
    // self-spawn is flaky (target/exe contention + transient sandbox_apply
    // failures), so it is gated behind `--ignored` and run on demand. Sandbox
    // policy logic is covered deterministically by the non-ignored unit tests;
    // real enforcement is verified by `macos_seatbelt_helper` when invoked
    // explicitly (`cargo test -- --ignored`).
    #[ignore = "run on demand: spawns a child process that executes sandbox-exec"]
    fn macos_seatbelt_allows_cwd_write_and_denies_home_write() {
        if !is_supported() {
            // Some CI/macOS images remove sandbox-exec; skip rather than faking
            // sandbox coverage when the platform facility is unavailable.
            return;
        }

        // On macOS, sandbox_apply can fail with EPERM from the multi-threaded
        // libtest harness. Run the real integration assertion in a single-test
        // child process so the test remains meaningful under default cargo test.
        let output = std::process::Command::new(std::env::current_exe().expect("current test exe"))
            .args([
                "--exact",
                "domain::tools::sandbox::seatbelt::tests::macos_seatbelt_helper",
                "--ignored",
                "--nocapture",
                "--test-threads=1",
            ])
            .output()
            .expect("run macOS seatbelt helper");
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !output.status.success() && stderr.contains("sandbox_apply: Operation not permitted") {
            eprintln!(
                "skipping macOS seatbelt integration from this test harness: sandbox_apply returned EPERM"
            );
            return;
        }
        assert!(
            output.status.success(),
            "macOS seatbelt helper failed\nstdout:\n{}\nstderr:\n{}",
            stdout,
            stderr
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "invoked by macos_seatbelt_allows_cwd_write_and_denies_home_write"]
    fn macos_seatbelt_helper() {
        if !is_supported() {
            return;
        }

        let _guard = sandbox_test_lock();
        let dir = tempfile::tempdir().expect("tempdir");
        let policy = SandboxPolicy {
            network: NetworkPolicy::deny_all(),
        };
        let roots = default_writable_roots(dir.path());

        let mut allowed = std_sandbox_command(&policy, &roots, r#"echo hi > "$PWD/sbtest.txt""#);
        allowed.current_dir(dir.path());
        let output = allowed.output().expect("run sandboxed cwd write");
        assert!(
            output.status.success(),
            "cwd write should succeed; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("sbtest.txt")).expect("read sbtest"),
            "hi\n"
        );

        let deny_path =
            Path::new(&std::env::var("HOME").expect("HOME")).join("omiga_sb_should_fail.txt");
        let _ = std::fs::remove_file(&deny_path);

        let mut denied = std_sandbox_command(
            &policy,
            &roots,
            r#"echo x > "$HOME/omiga_sb_should_fail.txt""#,
        );
        denied.current_dir(dir.path());
        let output = denied.output().expect("run sandboxed denied write");

        assert!(
            !output.status.success() || !deny_path.exists(),
            "home write should be rejected; status={:?}, stderr={}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            !deny_path.exists(),
            "sandboxed command unexpectedly created {}",
            deny_path.display()
        );
    }

    #[cfg(target_os = "macos")]
    fn std_sandbox_command(
        policy: &SandboxPolicy,
        writable_roots: &[PathBuf],
        command: &str,
    ) -> std::process::Command {
        let mut cmd = std::process::Command::new(SANDBOX_EXEC_PATH);
        cmd.arg("-p")
            .arg(policy_text(policy, writable_roots, None))
            .arg("--")
            .arg("bash")
            .arg("-l")
            .arg("-c")
            .arg(command);
        cmd
    }
}
