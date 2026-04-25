use std::collections::HashSet;
use std::fs;
use std::net::{IpAddr, Ipv4Addr, ToSocketAddrs};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebsiteBlock {
    pub host: String,
    pub rule: String,
    pub source: String,
    pub message: String,
}

#[derive(Debug, Default)]
struct WebsitePolicy {
    enabled: bool,
    rules: Vec<(String, String)>,
}

const BLOCKED_HOSTNAMES: &[&str] = &["metadata.google.internal", "metadata.goog"];
const SECRETISH_QUERY_KEYS: &[&str] = &[
    "access_token",
    "api_key",
    "apikey",
    "auth",
    "authorization",
    "key",
    "secret",
    "signature",
    "sig",
    "token",
];
const CGNAT_NETWORK: (Ipv4Addr, Ipv4Addr) = (
    Ipv4Addr::new(100, 64, 0, 0),
    Ipv4Addr::new(100, 127, 255, 255),
);

fn normalize_host(host: &str) -> String {
    host.trim().trim_end_matches('.').to_ascii_lowercase()
}

fn is_cgnat(ip: Ipv4Addr) -> bool {
    ip >= CGNAT_NETWORK.0 && ip <= CGNAT_NETWORK.1
}

fn is_blocked_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || v4.is_multicast()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || is_cgnat(v4)
                || v4.octets()[0] == 0
        }
        IpAddr::V6(v6) => {
            v6.is_loopback()
                || v6.is_multicast()
                || v6.is_unspecified()
                || v6.is_unique_local()
                || ((v6.segments()[0] & 0xffc0) == 0xfe80)
        }
    }
}

fn blocked_host_reason(host: &str) -> Option<String> {
    let host = normalize_host(host);
    if host.is_empty() {
        return Some("URL must include a hostname".to_string());
    }
    if BLOCKED_HOSTNAMES.iter().any(|blocked| host == *blocked) {
        return Some(format!(
            "Blocked: URL targets an internal hostname ({host})"
        ));
    }
    if host == "localhost"
        || host.ends_with(".localhost")
        || host == "local"
        || host.ends_with(".local")
    {
        return Some(format!(
            "Blocked: URL targets a loopback or local-only hostname ({host})"
        ));
    }
    if let Ok(ip) = host.parse::<IpAddr>() {
        if is_blocked_ip(ip) {
            return Some(format!(
                "Blocked: URL targets a private or internal network address ({ip})"
            ));
        }
    }
    None
}

fn validate_resolved_ips(host: &str, port: u16) -> Result<(), String> {
    let lookup = format!("{host}:{port}");
    let addrs = lookup
        .to_socket_addrs()
        .map_err(|_| format!("Blocked: DNS resolution failed for host {host}"))?;
    let mut saw_any = false;
    for addr in addrs {
        saw_any = true;
        if is_blocked_ip(addr.ip()) {
            return Err(format!(
                "Blocked: URL resolves to a private or internal network address ({})",
                addr.ip()
            ));
        }
    }
    if !saw_any {
        return Err(format!(
            "Blocked: DNS resolution returned no addresses for {host}"
        ));
    }
    Ok(())
}

fn contains_embedded_secret(parsed: &reqwest::Url) -> bool {
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return true;
    }
    parsed.query_pairs().any(|(key, value)| {
        let key = key.to_ascii_lowercase();
        let value = value.trim();
        SECRETISH_QUERY_KEYS
            .iter()
            .any(|needle| key == *needle || key.contains(needle))
            && value.len() >= 12
    })
}

fn config_paths(project_root: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(home) = dirs::home_dir() {
        paths.push(home.join(".omiga").join("config.yaml"));
    }
    paths.push(project_root.join(".omiga").join("config.yaml"));
    paths
}

fn load_yaml_mapping(path: &Path) -> Option<serde_yaml::Mapping> {
    let raw = fs::read_to_string(path).ok()?;
    let value = serde_yaml::from_str::<serde_yaml::Value>(&raw).ok()?;
    value.as_mapping().cloned()
}

fn mapping_get<'a>(map: &'a serde_yaml::Mapping, key: &str) -> Option<&'a serde_yaml::Value> {
    map.get(serde_yaml::Value::String(key.to_string()))
}

fn normalize_rule(value: &str) -> Option<String> {
    let mut rule = value.trim().to_ascii_lowercase();
    if rule.is_empty() || rule.starts_with('#') {
        return None;
    }
    if rule.contains("://") {
        if let Ok(parsed) = reqwest::Url::parse(&rule) {
            rule = parsed.host_str().unwrap_or("").to_string();
        }
    }
    let rule = rule
        .trim_start_matches("www.")
        .split('/')
        .next()
        .unwrap_or("")
        .trim_end_matches('.')
        .to_string();
    (!rule.is_empty()).then_some(rule)
}

fn load_rules_from_file(path: &Path) -> Vec<(String, String)> {
    let Ok(raw) = fs::read_to_string(path) else {
        return Vec::new();
    };
    raw.lines()
        .filter_map(normalize_rule)
        .map(|rule| (rule, path.display().to_string()))
        .collect()
}

fn load_website_policy(project_root: &Path) -> WebsitePolicy {
    let mut enabled = false;
    let mut rules = Vec::new();
    let mut seen = HashSet::new();

    for config_path in config_paths(project_root) {
        let Some(root) = load_yaml_mapping(&config_path) else {
            continue;
        };
        let Some(security) = mapping_get(&root, "security").and_then(serde_yaml::Value::as_mapping)
        else {
            continue;
        };
        let Some(blocklist) =
            mapping_get(security, "website_blocklist").and_then(serde_yaml::Value::as_mapping)
        else {
            continue;
        };

        if let Some(flag) = mapping_get(blocklist, "enabled").and_then(serde_yaml::Value::as_bool) {
            enabled = flag;
        }

        if let Some(items) =
            mapping_get(blocklist, "domains").and_then(serde_yaml::Value::as_sequence)
        {
            for item in items {
                let Some(rule) = item.as_str().and_then(normalize_rule) else {
                    continue;
                };
                if seen.insert((rule.clone(), "config".to_string())) {
                    rules.push((rule, "config".to_string()));
                }
            }
        }

        if let Some(items) =
            mapping_get(blocklist, "shared_files").and_then(serde_yaml::Value::as_sequence)
        {
            for item in items {
                let Some(path_str) = item.as_str() else {
                    continue;
                };
                let file_path = if Path::new(path_str).is_absolute() {
                    PathBuf::from(path_str)
                } else {
                    config_path.parent().unwrap_or(project_root).join(path_str)
                };
                for (rule, source) in load_rules_from_file(&file_path) {
                    if seen.insert((rule.clone(), source.clone())) {
                        rules.push((rule, source));
                    }
                }
            }
        }
    }

    WebsitePolicy { enabled, rules }
}

fn host_matches_rule(host: &str, rule: &str) -> bool {
    let host = normalize_host(host);
    let rule = normalize_host(rule);
    if host.is_empty() || rule.is_empty() {
        return false;
    }
    if let Some(suffix) = rule.strip_prefix("*.") {
        return host == suffix || host.ends_with(&format!(".{suffix}"));
    }
    host == rule || host.ends_with(&format!(".{rule}"))
}

pub fn check_website_access(project_root: &Path, url: &str) -> Option<WebsiteBlock> {
    let parsed = reqwest::Url::parse(url).ok()?;
    let host = normalize_host(parsed.host_str()?);
    let policy = load_website_policy(project_root);
    if !policy.enabled {
        return None;
    }
    for (rule, source) in policy.rules {
        if host_matches_rule(&host, &rule) {
            return Some(WebsiteBlock {
                host: host.clone(),
                rule: rule.clone(),
                source: source.clone(),
                message: format!("Blocked: access to {host} is disallowed by website policy"),
            });
        }
    }
    None
}

pub fn validate_public_http_url(
    project_root: &Path,
    url: &str,
    resolve_dns: bool,
) -> Result<(), String> {
    let parsed = reqwest::Url::parse(url).map_err(|e| format!("Invalid URL: {e}"))?;
    match parsed.scheme() {
        "http" | "https" => {}
        other => return Err(format!("Unsupported URL scheme: {other}")),
    }
    if contains_embedded_secret(&parsed) {
        return Err(
            "Blocked: URL appears to embed credentials or secret-bearing query parameters"
                .to_string(),
        );
    }
    let host = parsed
        .host_str()
        .ok_or_else(|| "URL must include a hostname".to_string())?;
    if let Some(reason) = blocked_host_reason(host) {
        return Err(reason);
    }
    if resolve_dns {
        validate_resolved_ips(host, parsed.port_or_known_default().unwrap_or(80))?;
    }
    if let Some(blocked) = check_website_access(project_root, url) {
        return Err(blocked.message);
    }
    Ok(())
}

pub fn is_safe_result_url(project_root: &Path, url: &str) -> bool {
    let Ok(parsed) = reqwest::Url::parse(url) else {
        return false;
    };
    if !matches!(parsed.scheme(), "http" | "https") {
        return false;
    }
    let Some(host) = parsed.host_str() else {
        return false;
    };
    blocked_host_reason(host).is_none() && check_website_access(project_root, url).is_none()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn validate_public_http_url_blocks_private_ips() {
        let root = TempDir::new().expect("tempdir");
        let err = validate_public_http_url(root.path(), "http://127.0.0.1/x", false)
            .expect_err("should block loopback");
        assert!(err.contains("private") || err.contains("loopback"));
    }

    #[test]
    fn validate_public_http_url_blocks_embedded_credentials() {
        let root = TempDir::new().expect("tempdir");
        let err = validate_public_http_url(root.path(), "https://user:pass@example.com/x", false)
            .expect_err("should block embedded creds");
        assert!(err.contains("credentials") || err.contains("secret"));
    }

    #[test]
    fn project_blocklist_blocks_matching_host() {
        let root = TempDir::new().expect("tempdir");
        let omiga_dir = root.path().join(".omiga");
        fs::create_dir_all(&omiga_dir).expect("mkdir .omiga");
        fs::write(
            omiga_dir.join("config.yaml"),
            "security:\n  website_blocklist:\n    enabled: true\n    domains:\n      - blocked.example.com\n",
        )
        .expect("write config");

        let blocked =
            check_website_access(root.path(), "https://blocked.example.com/page").expect("blocked");
        assert_eq!(blocked.host, "blocked.example.com");
        assert_eq!(blocked.rule, "blocked.example.com");
    }

    #[test]
    fn safe_result_url_rejects_localhost() {
        let root = TempDir::new().expect("tempdir");
        assert!(!is_safe_result_url(root.path(), "http://localhost:3000"));
        assert!(is_safe_result_url(root.path(), "https://example.com/docs"));
    }
}
