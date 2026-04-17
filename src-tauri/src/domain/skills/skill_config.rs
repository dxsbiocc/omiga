//! Skill configuration management.
//!
//! Skills declare the configuration variables they need in their SKILL.md frontmatter:
//!
//! ```yaml
//! ---
//! name: my-skill
//! description: Does something useful
//! metadata:
//!   omiga:
//!     config:
//!       - key: api.endpoint
//!         description: Base URL for the external API
//!         default: https://api.example.com
//!         prompt: "API endpoint URL"
//!       - key: wiki.path
//!         description: Local wiki directory
//!         default: ~/notes
//! ---
//! ```
//!
//! Values are stored in YAML config files under `skills.config.<key>` (nested):
//!
//! ```yaml
//! # <project>/.omiga/config.yaml  OR  ~/.omiga/config.yaml
//! skills:
//!   config:
//!     api:
//!       endpoint: https://my-api.example.com
//!     wiki:
//!       path: /Users/alice/notes
//! ```
//!
//! Resolution order (highest priority first):
//! 1. `<project>/.omiga/config.yaml`
//! 2. `~/.omiga/config.yaml`
//! 3. `default` value declared in the skill's frontmatter
//!
//! # Config injection
//!
//! When a skill is invoked and it has declared config vars with non-default values set,
//! the resolved values are prepended to the skill body as:
//!
//! ```text
//! [Skill config (from .omiga/config.yaml):
//!   api.endpoint = https://my-api.example.com
//!   wiki.path = /Users/alice/notes
//! ]
//! ```

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Data structures
// ---------------------------------------------------------------------------

/// A single config variable declared in a skill's frontmatter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigVar {
    /// Dotted key, e.g. `api.endpoint` or `wiki.path`.
    pub key: String,
    /// Human-readable description shown to the model / user.
    pub description: String,
    /// Default value when not set in config files.
    #[serde(default)]
    pub default: Option<String>,
    /// Short prompt text for interactive setup (falls back to `description`).
    #[serde(default)]
    pub prompt: Option<String>,
}

/// A config variable with its resolved value.
#[derive(Debug, Clone, Serialize)]
pub struct ResolvedConfigVar {
    pub key: String,
    pub value: String,
    pub description: String,
    /// True when the value came from a config file; false when using the default.
    pub is_set: bool,
}

// ---------------------------------------------------------------------------
// Config file I/O
// ---------------------------------------------------------------------------

/// Path to the project-level Omiga config file.
pub fn project_config_path(project_root: &Path) -> PathBuf {
    project_root.join(".omiga").join("config.yaml")
}

/// Path to the user-level Omiga config file.
pub fn user_config_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".omiga").join("config.yaml"))
}

/// Read a YAML config file.  Returns an empty mapping on missing / unreadable file.
fn read_config_file(path: &Path) -> serde_yaml::Value {
    let Ok(text) = std::fs::read_to_string(path) else {
        return serde_yaml::Value::Mapping(Default::default());
    };
    serde_yaml::from_str(&text).unwrap_or(serde_yaml::Value::Mapping(Default::default()))
}

/// Walk `root` following a dot-separated key, e.g. `"skills.config.api.endpoint"`.
/// Returns `None` if any segment is missing or the value is `Null`.
fn get_nested<'v>(root: &'v serde_yaml::Value, dotted: &str) -> Option<&'v serde_yaml::Value> {
    let mut cur = root;
    for part in dotted.split('.') {
        cur = cur.get(part)?;
    }
    if cur.is_null() {
        None
    } else {
        Some(cur)
    }
}

/// Set a value at a dot-separated path inside a YAML mapping, creating intermediate
/// maps as needed.  Works in-place on `root`.
fn set_nested(root: &mut serde_yaml::Value, dotted: &str, value: serde_yaml::Value) {
    let parts: Vec<&str> = dotted.split('.').collect();
    let mut cur = root;
    for &part in &parts[..parts.len() - 1] {
        let key = serde_yaml::Value::String(part.to_string());
        if !cur.is_mapping() {
            *cur = serde_yaml::Value::Mapping(Default::default());
        }
        let map = cur.as_mapping_mut().unwrap();
        cur = map
            .entry(key)
            .or_insert(serde_yaml::Value::Mapping(Default::default()));
    }
    if !cur.is_mapping() {
        *cur = serde_yaml::Value::Mapping(Default::default());
    }
    let last_key = serde_yaml::Value::String(parts[parts.len() - 1].to_string());
    cur.as_mapping_mut().unwrap().insert(last_key, value);
}

const CONFIG_STORAGE_PREFIX: &str = "skills.config";

/// Resolve the current value of a config var from the config files + defaults.
fn resolve_one(key: &str, default: Option<&str>, project_root: &Path) -> (String, bool) {
    let storage_key = format!("{CONFIG_STORAGE_PREFIX}.{key}");

    // Project-level config has highest priority.
    let project_cfg = read_config_file(&project_config_path(project_root));
    if let Some(v) = get_nested(&project_cfg, &storage_key) {
        if let Some(s) = yaml_value_to_string(v) {
            return (expand_path(s), true);
        }
    }

    // User-level config is fallback.
    if let Some(user_path) = user_config_path() {
        let user_cfg = read_config_file(&user_path);
        if let Some(v) = get_nested(&user_cfg, &storage_key) {
            if let Some(s) = yaml_value_to_string(v) {
                return (expand_path(s), true);
            }
        }
    }

    // Fall through to default.
    let default_val = default.unwrap_or("").to_string();
    (expand_path(default_val), false)
}

fn yaml_value_to_string(v: &serde_yaml::Value) -> Option<String> {
    match v {
        serde_yaml::Value::String(s) if !s.trim().is_empty() => Some(s.clone()),
        serde_yaml::Value::Number(n) => Some(n.to_string()),
        serde_yaml::Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

/// Allowlist of safe environment variables for expansion.
/// Only these variables will be expanded to prevent potential data exfiltration
/// if config files are shared between users.
const ALLOWED_ENV_VARS: &[&str] = &[
    "HOME",
    "USER",
    "USERNAME",
    "SHELL",
    "EDITOR",
    "LANG",
    "LC_ALL",
    "PWD",
    "OLDPWD",
    "XDG_CONFIG_HOME",
    "XDG_DATA_HOME",
    "XDG_CACHE_HOME",
    "PATH", // Safe to expose, commonly needed for tool paths
];

/// Check if an environment variable name is in the allowlist.
fn is_env_var_allowed(name: &str) -> bool {
    ALLOWED_ENV_VARS
        .iter()
        .any(|&allowed| allowed.eq_ignore_ascii_case(name))
}

/// Expand `~` and `${VAR}` in path-like values.
/// Only environment variables in the ALLOWED_ENV_VARS list are expanded
/// to prevent potential exfiltration of secrets if config files are shared.
fn expand_path(s: String) -> String {
    if !s.contains('~') && !s.contains("${") {
        return s;
    }
    // Expand ~ to home dir.
    let expanded = if let Some(home) = dirs::home_dir() {
        s.replacen('~', &home.to_string_lossy(), 1)
    } else {
        s
    };
    // Expand ${VAR} environment variables (allowlist only).
    let mut result = expanded.clone();
    let mut remaining = expanded.as_str();
    let mut out = String::new();
    while let Some(start) = remaining.find("${") {
        out.push_str(&remaining[..start]);
        remaining = &remaining[start + 2..];
        if let Some(end) = remaining.find('}') {
            let var_name = &remaining[..end];
            if is_env_var_allowed(var_name) {
                if let Ok(val) = std::env::var(var_name) {
                    out.push_str(&val);
                } else {
                    out.push_str("${");
                    out.push_str(var_name);
                    out.push('}');
                }
            } else {
                // Variable not in allowlist - leave unexpanded with a warning comment
                out.push_str("${");
                out.push_str(var_name);
                out.push('}');
            }
            remaining = &remaining[end + 1..];
        } else {
            out.push_str("${");
        }
    }
    out.push_str(remaining);
    if out != result {
        result = out;
    }
    result
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Resolve all declared config vars for a skill.
pub fn resolve_config_vars(vars: &[ConfigVar], project_root: &Path) -> Vec<ResolvedConfigVar> {
    vars.iter()
        .map(|v| {
            let (value, is_set) = resolve_one(&v.key, v.default.as_deref(), project_root);
            ResolvedConfigVar {
                key: v.key.clone(),
                value,
                description: v.description.clone(),
                is_set,
            }
        })
        .collect()
}

/// Format a config injection block for the skill body.
///
/// Only includes vars that have a value (either from config or a non-empty default).
/// Returns `None` when there's nothing to inject.
pub fn format_config_injection(resolved: &[ResolvedConfigVar]) -> Option<String> {
    let items: Vec<String> = resolved
        .iter()
        .filter(|r| !r.value.is_empty())
        .map(|r| format!("  {} = {}", r.key, r.value))
        .collect();

    if items.is_empty() {
        return None;
    }
    Some(format!(
        "[Skill config (from .omiga/config.yaml):\n{}\n]",
        items.join("\n")
    ))
}

/// Set a config var value in the project-level config file.
/// Creates the file (and parent directories) if it doesn't exist.
pub async fn set_config_var(project_root: &Path, key: &str, value: &str) -> Result<(), String> {
    let config_path = project_config_path(project_root);

    if let Some(parent) = config_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| format!("skill_config: mkdir: {e}"))?;
    }

    let mut root = if config_path.exists() {
        let text = tokio::fs::read_to_string(&config_path)
            .await
            .map_err(|e| format!("skill_config: read config: {e}"))?;
        serde_yaml::from_str::<serde_yaml::Value>(&text)
            .unwrap_or(serde_yaml::Value::Mapping(Default::default()))
    } else {
        serde_yaml::Value::Mapping(Default::default())
    };

    let storage_key = format!("{CONFIG_STORAGE_PREFIX}.{key}");
    set_nested(
        &mut root,
        &storage_key,
        serde_yaml::Value::String(value.to_string()),
    );

    let yaml_text =
        serde_yaml::to_string(&root).map_err(|e| format!("skill_config: serialize: {e}"))?;

    // Write atomically via temp + rename.
    let dir = config_path.parent().unwrap();
    let tmp = dir.join(format!(".tmp.{}.write", uuid::Uuid::new_v4().simple()));
    tokio::fs::write(&tmp, yaml_text.as_bytes())
        .await
        .map_err(|e| format!("skill_config: write temp: {e}"))?;
    if let Err(e) = tokio::fs::rename(&tmp, &config_path).await {
        let _ = tokio::fs::remove_file(&tmp).await;
        return Err(format!("skill_config: rename: {e}"));
    }
    Ok(())
}

/// List all config vars from a skill's declaration.
/// Returns the vars with their resolved values from the config files.
pub fn list_skill_config_vars(vars: &[ConfigVar], project_root: &Path) -> serde_json::Value {
    let resolved = resolve_config_vars(vars, project_root);
    serde_json::to_value(&resolved).unwrap_or(serde_json::Value::Array(vec![]))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn get_nested_works() {
        let yaml: serde_yaml::Value =
            serde_yaml::from_str("skills:\n  config:\n    wiki:\n      path: ~/notes\n").unwrap();
        let v = get_nested(&yaml, "skills.config.wiki.path").unwrap();
        assert_eq!(v.as_str().unwrap(), "~/notes");
    }

    #[test]
    fn set_nested_creates_intermediate_maps() {
        let mut root = serde_yaml::Value::Mapping(Default::default());
        set_nested(
            &mut root,
            "skills.config.wiki.path",
            serde_yaml::Value::String("~/wiki".to_string()),
        );
        let v = get_nested(&root, "skills.config.wiki.path").unwrap();
        assert_eq!(v.as_str().unwrap(), "~/wiki");
    }

    #[tokio::test]
    async fn set_and_resolve_config_var() {
        let tmp = tempdir().unwrap();
        let root = tmp.path();

        set_config_var(root, "api.endpoint", "https://test.example.com")
            .await
            .unwrap();

        let vars = vec![ConfigVar {
            key: "api.endpoint".to_string(),
            description: "API endpoint".to_string(),
            default: Some("https://default.example.com".to_string()),
            prompt: None,
        }];
        let resolved = resolve_config_vars(&vars, root);
        assert_eq!(resolved[0].value, "https://test.example.com");
        assert!(resolved[0].is_set);
    }

    #[test]
    fn resolve_falls_back_to_default() {
        let tmp = tempdir().unwrap();
        let vars = vec![ConfigVar {
            key: "some.missing.key".to_string(),
            description: "test".to_string(),
            default: Some("default_val".to_string()),
            prompt: None,
        }];
        let resolved = resolve_config_vars(&vars, tmp.path());
        assert_eq!(resolved[0].value, "default_val");
        assert!(!resolved[0].is_set);
    }

    #[test]
    fn format_injection_skips_empty_values() {
        let resolved = vec![
            ResolvedConfigVar {
                key: "a.key".to_string(),
                value: "hello".to_string(),
                description: "d".to_string(),
                is_set: true,
            },
            ResolvedConfigVar {
                key: "b.key".to_string(),
                value: "".to_string(),
                description: "d".to_string(),
                is_set: false,
            },
        ];
        let injection = format_config_injection(&resolved).unwrap();
        assert!(injection.contains("a.key = hello"));
        assert!(!injection.contains("b.key"));
    }

    #[test]
    fn format_injection_returns_none_when_all_empty() {
        let resolved = vec![ResolvedConfigVar {
            key: "x".to_string(),
            value: "".to_string(),
            description: "d".to_string(),
            is_set: false,
        }];
        assert!(format_config_injection(&resolved).is_none());
    }
}
