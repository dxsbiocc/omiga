//! Network policy parsing shared by local sandbox backends.
//!
//! The current source of truth is process environment. That preserves the
//! existing `OMIGA_SANDBOX_NETWORK=deny` behavior while adding
//! `OMIGA_SANDBOX_NETWORK_ALLOW` and `OMIGA_SANDBOX_NETWORK_DENY` for finer
//! policy. Project config can be wired later without changing backend code.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkMode {
    AllowAll,
    DenyAll,
    AllowList,
    DenyList,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostRule {
    pub domain: String,
    pub port: Option<u16>,
}

impl HostRule {
    pub fn is_port_only(&self) -> bool {
        self.domain == "*"
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkPolicy {
    pub mode: NetworkMode,
    pub hosts: Vec<HostRule>,
}

impl Default for NetworkPolicy {
    fn default() -> Self {
        Self::from_env()
    }
}

impl NetworkPolicy {
    pub fn allow_all() -> Self {
        Self {
            mode: NetworkMode::AllowAll,
            hosts: Vec::new(),
        }
    }

    pub fn deny_all() -> Self {
        Self {
            mode: NetworkMode::DenyAll,
            hosts: Vec::new(),
        }
    }

    pub fn from_env() -> Self {
        // Thin env-reading shell around the pure `from_parts` core. Keeping the
        // process-global `getenv` isolated here lets tests exercise the policy
        // logic via `from_parts` without any `setenv`, which would otherwise
        // race against concurrent `getenv` at the libc level under parallel
        // `cargo test`.
        Self::from_parts(
            std::env::var("OMIGA_SANDBOX_NETWORK").ok(),
            std::env::var("OMIGA_SANDBOX_NETWORK_ALLOW").ok(),
            std::env::var("OMIGA_SANDBOX_NETWORK_DENY").ok(),
        )
    }

    /// Pure policy resolution from raw values, independent of process env.
    pub fn from_parts(legacy: Option<String>, allow: Option<String>, deny: Option<String>) -> Self {
        if legacy
            .as_deref()
            .map(str::trim)
            .is_some_and(|value| value.eq_ignore_ascii_case("deny"))
        {
            return Self::deny_all();
        }

        let allow_hosts = parse_host_rules(allow);
        if !allow_hosts.is_empty() {
            return Self {
                mode: NetworkMode::AllowList,
                hosts: allow_hosts,
            };
        }

        let deny_hosts = parse_host_rules(deny);
        if !deny_hosts.is_empty() {
            return Self {
                mode: NetworkMode::DenyList,
                hosts: deny_hosts,
            };
        }

        Self::allow_all()
    }
}

fn parse_host_rules(value: Option<String>) -> Vec<HostRule> {
    value
        .unwrap_or_default()
        .split(',')
        .filter_map(parse_host_rule)
        .collect()
}

fn parse_host_rule(raw: &str) -> Option<HostRule> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    let (domain, port) = match trimmed.rsplit_once(':') {
        Some((domain, port_text)) if !domain.is_empty() => match port_text.parse::<u16>() {
            Ok(port) if port > 0 => (domain.trim(), Some(port)),
            _ => (trimmed, None),
        },
        _ => (trimmed, None),
    };

    let domain = domain.trim().trim_matches('.').to_ascii_lowercase();
    if domain.is_empty() {
        return None;
    }

    Some(HostRule { domain, port })
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tests exercise the pure `from_parts` core rather than `from_env`, so they
    // never call `setenv`/`getenv`. Process-global env mutation is not
    // thread-safe at the libc level and would flake under parallel `cargo test`.

    #[test]
    fn default_policy_allows_all_network() {
        let policy = NetworkPolicy::from_parts(None, None, None);
        assert_eq!(policy.mode, NetworkMode::AllowAll);
        assert!(policy.hosts.is_empty());
    }

    #[test]
    fn legacy_network_deny_maps_to_deny_all() {
        let policy = NetworkPolicy::from_parts(Some("deny".to_string()), None, None);
        assert_eq!(policy, NetworkPolicy::deny_all());
    }

    #[test]
    fn allow_env_parses_domains_and_ports() {
        let policy = NetworkPolicy::from_parts(
            None,
            Some("Example.com, api.foo.com:443, *:8443".to_string()),
            None,
        );
        assert_eq!(policy.mode, NetworkMode::AllowList);
        assert_eq!(
            policy.hosts,
            vec![
                HostRule {
                    domain: "example.com".to_string(),
                    port: None,
                },
                HostRule {
                    domain: "api.foo.com".to_string(),
                    port: Some(443),
                },
                HostRule {
                    domain: "*".to_string(),
                    port: Some(8443),
                },
            ]
        );
    }

    #[test]
    fn deny_env_is_used_when_allow_env_is_absent() {
        let policy = NetworkPolicy::from_parts(None, None, Some("tracker.example:80".to_string()));
        assert_eq!(policy.mode, NetworkMode::DenyList);
        assert_eq!(policy.hosts[0].port, Some(80));
    }
}
