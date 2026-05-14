use crate::domain::retrieval::credentials::{is_allowed_credential_ref, normalize_credential_ref};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

pub const SUPPORTED_PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginRetrievalManifest {
    pub protocol_version: u32,
    pub runtime: PluginRetrievalRuntime,
    pub resources: Vec<PluginRetrievalResource>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginRetrievalRuntime {
    pub command: PathBuf,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
    pub cwd: PathBuf,
    pub idle_ttl_ms: Option<u64>,
    pub request_timeout_ms: Option<u64>,
    pub cancel_grace_ms: Option<u64>,
    pub concurrency: u16,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginRetrievalResource {
    pub id: String,
    pub category: String,
    pub label: String,
    pub description: String,
    pub aliases: Vec<String>,
    pub subcategories: Vec<String>,
    pub capabilities: Vec<String>,
    pub required_credential_refs: Vec<String>,
    pub optional_credential_refs: Vec<String>,
    pub risk_level: String,
    pub risk_notes: Vec<String>,
    pub default_enabled: bool,
    pub replaces_builtin: bool,
    pub parameters: Vec<JsonValue>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawPluginRetrievalManifest {
    #[serde(default, alias = "protocol_version")]
    protocol_version: Option<u32>,
    runtime: RawPluginRetrievalRuntime,
    #[serde(default, alias = "sources")]
    resources: Vec<RawPluginRetrievalResource>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawPluginRetrievalRuntime {
    command: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    env: HashMap<String, String>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default, alias = "idle_ttl_ms")]
    idle_ttl_ms: Option<u64>,
    #[serde(default, alias = "request_timeout_ms")]
    request_timeout_ms: Option<u64>,
    #[serde(default, alias = "cancel_grace_ms")]
    cancel_grace_ms: Option<u64>,
    #[serde(default)]
    concurrency: Option<u16>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawPluginRetrievalResource {
    id: String,
    category: String,
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    aliases: Vec<String>,
    #[serde(default)]
    subcategories: Vec<String>,
    #[serde(default)]
    capabilities: Vec<String>,
    #[serde(
        default,
        alias = "required_credentials",
        alias = "requiredCredentialRefs"
    )]
    required_credential_refs: Vec<String>,
    #[serde(
        default,
        alias = "optional_credentials",
        alias = "optionalCredentialRefs"
    )]
    optional_credential_refs: Vec<String>,
    #[serde(default)]
    risk_level: Option<String>,
    #[serde(default)]
    risk_notes: Vec<String>,
    #[serde(default)]
    default_enabled: bool,
    #[serde(default)]
    replaces_builtin: bool,
    #[serde(default)]
    parameters: Vec<JsonValue>,
}

pub fn load_plugin_retrieval_manifest(
    plugin_root: &Path,
    value: JsonValue,
) -> Result<PluginRetrievalManifest, String> {
    let raw: RawPluginRetrievalManifest =
        serde_json::from_value(value).map_err(|err| format!("parse retrieval manifest: {err}"))?;
    let protocol_version = raw.protocol_version.unwrap_or(SUPPORTED_PROTOCOL_VERSION);
    if protocol_version != SUPPORTED_PROTOCOL_VERSION {
        return Err(format!(
            "unsupported retrieval protocol version `{protocol_version}`; expected `{SUPPORTED_PROTOCOL_VERSION}`"
        ));
    }
    if raw.resources.is_empty() {
        return Err("retrieval.resources must contain at least one resource".to_string());
    }

    Ok(PluginRetrievalManifest {
        protocol_version,
        runtime: validate_runtime(plugin_root, raw.runtime)?,
        resources: raw
            .resources
            .into_iter()
            .map(validate_resource)
            .collect::<Result<Vec<_>, _>>()?,
    })
}

fn validate_runtime(
    plugin_root: &Path,
    raw: RawPluginRetrievalRuntime,
) -> Result<PluginRetrievalRuntime, String> {
    let concurrency = raw.concurrency.unwrap_or(1);
    if concurrency != 1 {
        return Err("retrieval.runtime.concurrency must be 1 in this version".to_string());
    }
    let command =
        resolve_safe_relative_path(plugin_root, &raw.command, "retrieval.runtime.command")?;
    if !command.is_file() {
        return Err(format!(
            "retrieval.runtime.command is not a file: {}",
            command.display()
        ));
    }
    let cwd = match raw.cwd.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        None | Some(".") => plugin_root.to_path_buf(),
        Some(value) => resolve_safe_relative_path(plugin_root, value, "retrieval.runtime.cwd")?,
    };
    if !cwd.is_dir() {
        return Err(format!(
            "retrieval.runtime.cwd is not a directory: {}",
            cwd.display()
        ));
    }

    Ok(PluginRetrievalRuntime {
        command,
        args: raw.args,
        env: raw.env,
        cwd,
        idle_ttl_ms: raw.idle_ttl_ms,
        request_timeout_ms: raw.request_timeout_ms,
        cancel_grace_ms: raw.cancel_grace_ms,
        concurrency,
    })
}

fn validate_resource(raw: RawPluginRetrievalResource) -> Result<PluginRetrievalResource, String> {
    let id = normalize_id(&raw.id);
    let category = normalize_category(&raw.category);
    if id.is_empty() {
        return Err("retrieval.resources[].id must not be empty".to_string());
    }
    if category.is_empty() {
        return Err(format!(
            "retrieval resource `{id}` category must not be empty"
        ));
    }
    let capabilities = raw
        .capabilities
        .into_iter()
        .map(|capability| normalize_id(&capability))
        .filter(|capability| !capability.is_empty())
        .collect::<Vec<_>>();
    if capabilities.is_empty() {
        return Err(format!(
            "retrieval resource `{category}.{id}` must declare at least one capability"
        ));
    }
    for capability in &capabilities {
        match capability.as_str() {
            "search" | "fetch" | "query" => {}
            other => {
                return Err(format!(
                    "retrieval resource `{category}.{id}` has unsupported capability `{other}`"
                ))
            }
        }
    }
    if raw.default_enabled {
        return Err(format!(
            "retrieval resource `{category}.{id}` must not be default-enabled in this version"
        ));
    }

    let required_credential_refs = normalize_and_validate_credential_refs(
        &format!("retrieval resource `{category}.{id}` requiredCredentialRefs"),
        raw.required_credential_refs,
    )?;
    let optional_credential_refs = normalize_and_validate_credential_refs(
        &format!("retrieval resource `{category}.{id}` optionalCredentialRefs"),
        raw.optional_credential_refs,
    )?;

    Ok(PluginRetrievalResource {
        label: raw.label.unwrap_or_else(|| id.clone()),
        description: raw.description.unwrap_or_default(),
        aliases: raw.aliases.into_iter().map(|v| normalize_id(&v)).collect(),
        subcategories: raw
            .subcategories
            .into_iter()
            .map(|v| normalize_id(&v))
            .collect(),
        risk_level: raw
            .risk_level
            .map(|v| normalize_id(&v))
            .unwrap_or_else(|| "medium".to_string()),
        risk_notes: raw.risk_notes,
        default_enabled: raw.default_enabled,
        replaces_builtin: raw.replaces_builtin,
        parameters: raw.parameters,
        id,
        category,
        capabilities,
        required_credential_refs,
        optional_credential_refs,
    })
}

fn normalize_and_validate_credential_refs(
    label: &str,
    values: Vec<String>,
) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    for value in values {
        let normalized = normalize_credential_ref(&value);
        if normalized.is_empty() {
            continue;
        }
        if !is_allowed_credential_ref(&normalized) {
            return Err(format!(
                "{label} contains unsupported credential ref `{value}`"
            ));
        }
        if !out.iter().any(|item| item == &normalized) {
            out.push(normalized);
        }
    }
    Ok(out)
}

fn normalize_category(value: &str) -> String {
    match normalize_id(value).as_str() {
        "data" | "dataset" | "datasets" => "dataset".to_string(),
        "knowledge_base" | "kb" | "memory" => "knowledge".to_string(),
        other => other.to_string(),
    }
}

fn normalize_id(value: &str) -> String {
    value.trim().to_ascii_lowercase().replace(['-', ' '], "_")
}

fn resolve_safe_relative_path(root: &Path, value: &str, field: &str) -> Result<PathBuf, String> {
    let Some(rel) = value.strip_prefix("./") else {
        return Err(format!("{field} must start with `./`"));
    };
    if rel.trim().is_empty() {
        return Err(format!("{field} must not be empty"));
    }
    let mut normalized = PathBuf::new();
    for component in Path::new(rel).components() {
        match component {
            Component::Normal(part) => normalized.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(format!("{field} must stay within plugin root"));
            }
        }
    }
    if normalized.as_os_str().is_empty() {
        return Err(format!("{field} must not resolve to plugin root"));
    }
    Ok(root.join(normalized))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;

    fn plugin_root() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        fs::write(
            dir.path().join("mock_plugin.py"),
            "#!/usr/bin/env python3\n",
        )
        .unwrap();
        fs::create_dir_all(dir.path().join("bin")).unwrap();
        fs::write(dir.path().join("bin/worker"), "#!/bin/sh\n").unwrap();
        dir
    }

    fn documented_basic_fixture_root() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/retrieval-plugins/basic")
    }

    #[test]
    fn parses_valid_retrieval_manifest() {
        let dir = plugin_root();
        let manifest = load_plugin_retrieval_manifest(
            dir.path(),
            json!({
                "protocolVersion": 1,
                "runtime": {
                    "command": "./mock_plugin.py",
                    "args": ["--stdio"],
                    "cwd": ".",
                    "requestTimeoutMs": 5000,
                    "concurrency": 1
                },
                "resources": [{
                    "id": "Mock-Source",
                    "category": "data",
                    "label": "Mock Source",
                    "description": "Test source",
                    "aliases": ["mock source"],
                    "subcategories": ["sample metadata"],
                    "capabilities": ["search", "fetch", "query"],
                    "requiredCredentialRefs": ["PubMed API Key"],
                    "optionalCredentialRefs": ["pubmed_email"],
                    "riskLevel": "low",
                    "replacesBuiltin": false
                }]
            }),
        )
        .expect("valid manifest");

        assert_eq!(manifest.protocol_version, 1);
        assert_eq!(manifest.runtime.command, dir.path().join("mock_plugin.py"));
        assert_eq!(manifest.runtime.args, vec!["--stdio".to_string()]);
        assert_eq!(manifest.resources[0].id, "mock_source");
        assert_eq!(manifest.resources[0].category, "dataset");
        assert_eq!(
            manifest.resources[0].aliases,
            vec!["mock_source".to_string()]
        );
        assert_eq!(
            manifest.resources[0].subcategories,
            vec!["sample_metadata".to_string()]
        );
        assert_eq!(
            manifest.resources[0].required_credential_refs,
            vec!["pubmed_api_key".to_string()]
        );
    }

    #[test]
    fn parses_documented_basic_retrieval_fixture_manifest() {
        let root = documented_basic_fixture_root();
        let plugin_json: JsonValue =
            serde_json::from_str(&fs::read_to_string(root.join("plugin.json")).unwrap()).unwrap();
        let retrieval = plugin_json
            .get("retrieval")
            .cloned()
            .expect("fixture has retrieval manifest");
        let manifest = load_plugin_retrieval_manifest(&root, retrieval).unwrap();

        assert_eq!(manifest.protocol_version, SUPPORTED_PROTOCOL_VERSION);
        assert_eq!(
            manifest.runtime.command,
            root.join("scripts/basic_retrieval_plugin.py")
        );
        assert_eq!(manifest.runtime.concurrency, 1);
        assert_eq!(manifest.runtime.idle_ttl_ms, Some(30_000));
        assert_eq!(manifest.runtime.request_timeout_ms, Some(5_000));
        assert_eq!(manifest.runtime.cancel_grace_ms, Some(500));
        assert_eq!(manifest.resources.len(), 1);
        assert_eq!(manifest.resources[0].category, "dataset");
        assert_eq!(manifest.resources[0].id, "example_dataset");
        assert_eq!(
            manifest.resources[0].capabilities,
            vec![
                "search".to_string(),
                "query".to_string(),
                "fetch".to_string()
            ]
        );
        assert!(!manifest.resources[0].default_enabled);
        assert_eq!(
            manifest.resources[0].optional_credential_refs,
            vec!["pubmed_email".to_string()]
        );
    }

    #[test]
    fn rejects_unsafe_command_path() {
        let dir = plugin_root();
        let err = load_plugin_retrieval_manifest(
            dir.path(),
            json!({
                "runtime": {"command": "../bad", "concurrency": 1},
                "resources": [{"id":"mock", "category":"dataset", "capabilities":["search"]}]
            }),
        )
        .unwrap_err();

        assert!(err.contains("must start with `./`"));
    }

    #[test]
    fn rejects_unknown_credential_ref() {
        let dir = plugin_root();
        let err = load_plugin_retrieval_manifest(
            dir.path(),
            json!({
                "runtime": {"command": "./mock_plugin.py", "concurrency": 1},
                "resources": [{
                    "id":"mock",
                    "category":"dataset",
                    "capabilities":["search"],
                    "requiredCredentialRefs":["aws_secret_access_key"]
                }]
            }),
        )
        .unwrap_err();

        assert!(err.contains("unsupported credential ref"));
    }

    #[test]
    fn rejects_concurrency_above_one() {
        let dir = plugin_root();
        let err = load_plugin_retrieval_manifest(
            dir.path(),
            json!({
                "runtime": {"command": "./mock_plugin.py", "concurrency": 2},
                "resources": [{"id":"mock", "category":"dataset", "capabilities":["search"]}]
            }),
        )
        .unwrap_err();

        assert!(err.contains("concurrency must be 1"));
    }
}
