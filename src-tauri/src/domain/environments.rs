//! Unit-level Environment profile discovery and resolution.
//!
//! Environment profiles are plugin contributions referenced by executable units
//! through `runtime.envRef`. V3 keeps this resolver diagnostic-only: it resolves
//! the profile, records requirements and hints, but does not install packages or
//! mutate the user's runtime.

use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};

pub const ENVIRONMENT_API_VERSION_V1ALPHA1: &str = "omiga.ai/environment/v1alpha1";
pub const ENVIRONMENT_KIND: &str = "Environment";
const ENVIRONMENT_MANIFEST_NAMES: &[&str] =
    &["environment.yaml", "environment.yml", "env.yaml", "env.yml"];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentMetadata {
    pub id: String,
    pub version: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentRuntimeProfile {
    #[serde(default, rename = "type")]
    pub kind: Option<String>,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub image: Option<String>,
    #[serde(default)]
    pub module: Option<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, JsonValue>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentRequirements {
    #[serde(default)]
    pub system: Vec<String>,
    #[serde(default, rename = "rPackages")]
    pub r_packages: Vec<String>,
    #[serde(default)]
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentDiagnostics {
    #[serde(default, rename = "installHint")]
    pub install_hint: Option<String>,
    #[serde(default, rename = "checkCommand")]
    pub check_command: Vec<String>,
    #[serde(default)]
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentSpec {
    #[serde(rename = "apiVersion")]
    pub api_version: String,
    pub kind: String,
    pub metadata: EnvironmentMetadata,
    #[serde(default)]
    pub runtime: EnvironmentRuntimeProfile,
    #[serde(default)]
    pub requirements: EnvironmentRequirements,
    #[serde(default)]
    pub diagnostics: EnvironmentDiagnostics,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentSource {
    pub source_plugin: String,
    pub plugin_root: PathBuf,
    pub manifest_path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentSpecWithSource {
    #[serde(flatten)]
    pub spec: EnvironmentSpec,
    pub source: EnvironmentSource,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentProfileSummary {
    pub id: String,
    pub version: String,
    pub canonical_id: String,
    pub source_plugin: String,
    pub manifest_path: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub runtime: EnvironmentRuntimeProfile,
    pub requirements: EnvironmentRequirements,
    pub diagnostics: EnvironmentDiagnostics,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentResolution {
    #[serde(default, rename = "envRef")]
    pub env_ref: Option<String>,
    pub status: String,
    #[serde(default)]
    pub canonical_id: Option<String>,
    #[serde(default)]
    pub profile: Option<EnvironmentProfileSummary>,
    #[serde(default)]
    pub candidates: Vec<String>,
    #[serde(default)]
    pub diagnostics: Vec<String>,
    #[serde(default, rename = "verificationStatus")]
    pub verification_status: String,
}

pub fn load_environment_manifest(
    manifest_path: &Path,
    source_plugin: impl Into<String>,
    plugin_root: impl Into<PathBuf>,
) -> Result<EnvironmentSpecWithSource, String> {
    let raw = fs::read_to_string(manifest_path).map_err(|err| {
        format!(
            "read environment manifest `{}`: {err}",
            manifest_path.display()
        )
    })?;
    let spec: EnvironmentSpec = serde_yaml::from_str(&raw).map_err(|err| {
        format!(
            "parse environment manifest `{}`: {err}",
            manifest_path.display()
        )
    })?;
    validate_environment_spec(&spec)?;
    Ok(EnvironmentSpecWithSource {
        spec,
        source: EnvironmentSource {
            source_plugin: source_plugin.into(),
            plugin_root: plugin_root.into(),
            manifest_path: manifest_path.to_path_buf(),
        },
    })
}

pub fn validate_environment_spec(spec: &EnvironmentSpec) -> Result<(), String> {
    if spec.api_version.trim() != ENVIRONMENT_API_VERSION_V1ALPHA1 {
        return Err(format!(
            "unsupported apiVersion `{}`; expected `{ENVIRONMENT_API_VERSION_V1ALPHA1}`",
            spec.api_version
        ));
    }
    if spec.kind.trim() != ENVIRONMENT_KIND {
        return Err(format!(
            "unsupported kind `{}`; expected `{ENVIRONMENT_KIND}`",
            spec.kind
        ));
    }
    validate_environment_id(&spec.metadata.id)?;
    if spec.metadata.version.trim().is_empty() {
        return Err("metadata.version must not be empty".to_string());
    }
    if let Some(command) = spec.runtime.command.as_deref() {
        if command.trim().is_empty() {
            return Err("runtime.command must not be empty when set".to_string());
        }
    }
    Ok(())
}

pub fn discover_environment_profiles() -> Vec<EnvironmentSpecWithSource> {
    let outcome = crate::domain::plugins::plugin_load_outcome();
    discover_environment_profiles_from_plugins(outcome.plugins())
}

pub fn discover_environment_profiles_from_plugins<'a>(
    plugins: impl IntoIterator<Item = &'a crate::domain::plugins::LoadedPlugin>,
) -> Vec<EnvironmentSpecWithSource> {
    let mut out = Vec::new();
    for plugin in plugins.into_iter().filter(|plugin| plugin.is_active()) {
        for manifest_path in discover_environment_manifest_paths(&plugin.root) {
            match load_environment_manifest(&manifest_path, plugin.id.clone(), plugin.root.clone())
            {
                Ok(profile) => out.push(profile),
                Err(err) => tracing::warn!(
                    plugin_id = %plugin.id,
                    manifest = %manifest_path.display(),
                    "ignoring invalid environment manifest: {err}"
                ),
            }
        }
    }
    out.sort_by(|left, right| {
        left.spec
            .metadata
            .id
            .cmp(&right.spec.metadata.id)
            .then_with(|| left.source.source_plugin.cmp(&right.source.source_plugin))
            .then_with(|| left.spec.metadata.version.cmp(&right.spec.metadata.version))
    });
    out
}

pub fn discover_environment_manifest_paths(plugin_root: &Path) -> Vec<PathBuf> {
    let environments_root = crate::domain::plugins::load_plugin_manifest(plugin_root)
        .and_then(|manifest| manifest.environments)
        .unwrap_or_else(|| plugin_root.join("environments"));
    let mut out = Vec::new();
    if !environments_root.is_dir() {
        return out;
    }
    collect_environment_manifest_paths(&environments_root, &mut out);
    out.sort();
    out.dedup();
    out
}

pub fn resolve_environment_ref(
    env_ref: Option<&str>,
    source_plugin: &str,
    plugin_root: &Path,
) -> EnvironmentResolution {
    let Some(raw_ref) = env_ref.map(str::trim).filter(|value| !value.is_empty()) else {
        return EnvironmentResolution {
            env_ref: None,
            status: "notRequested".to_string(),
            canonical_id: None,
            profile: None,
            candidates: Vec::new(),
            diagnostics: Vec::new(),
            verification_status: "notRun".to_string(),
        };
    };

    let mut profiles = discover_environment_profiles();
    for manifest_path in discover_environment_manifest_paths(plugin_root) {
        if let Ok(profile) =
            load_environment_manifest(&manifest_path, source_plugin.to_string(), plugin_root)
        {
            let canonical = canonical_environment_id(&profile);
            if !profiles
                .iter()
                .any(|existing| canonical_environment_id(existing) == canonical)
            {
                profiles.push(profile);
            }
        }
    }
    resolve_environment_ref_from_profiles(raw_ref, source_plugin, &profiles)
}

pub fn resolve_environment_ref_from_profiles(
    raw_ref: &str,
    source_plugin: &str,
    profiles: &[EnvironmentSpecWithSource],
) -> EnvironmentResolution {
    let needle = raw_ref.trim().to_ascii_lowercase();
    let matches = profiles
        .iter()
        .filter(|profile| environment_matches_ref(profile, &needle))
        .cloned()
        .collect::<Vec<_>>();
    let source_matches = matches
        .iter()
        .filter(|profile| profile.source.source_plugin == source_plugin)
        .cloned()
        .collect::<Vec<_>>();
    let selected = match source_matches.as_slice() {
        [only] => Some(Ok(only.clone())),
        many if many.len() > 1 => Some(Err(many.to_vec())),
        [] => match matches.as_slice() {
            [only] => Some(Ok(only.clone())),
            many if many.len() > 1 => Some(Err(many.to_vec())),
            [] => None,
            _ => None,
        },
        _ => None,
    };

    match selected {
        Some(Ok(profile)) => {
            let canonical_id = canonical_environment_id(&profile);
            let mut diagnostics = Vec::new();
            if profile.spec.diagnostics.check_command.is_empty() {
                diagnostics.push(
                    "environment profile resolved; availability check was not executed".to_string(),
                );
            } else {
                diagnostics.push(format!(
                    "environment profile resolved; suggested check command: {}",
                    profile.spec.diagnostics.check_command.join(" ")
                ));
            }
            EnvironmentResolution {
                env_ref: Some(raw_ref.to_string()),
                status: "resolved".to_string(),
                canonical_id: Some(canonical_id),
                profile: Some(environment_summary(profile)),
                candidates: Vec::new(),
                diagnostics,
                verification_status: "notRun".to_string(),
            }
        }
        Some(Err(ambiguous)) => EnvironmentResolution {
            env_ref: Some(raw_ref.to_string()),
            status: "ambiguous".to_string(),
            canonical_id: None,
            profile: None,
            candidates: ambiguous.iter().map(canonical_environment_id).collect(),
            diagnostics: vec![format!(
                "environment ref `{raw_ref}` matched multiple profiles; use a canonical provider-scoped id"
            )],
            verification_status: "notRun".to_string(),
        },
        None => EnvironmentResolution {
            env_ref: Some(raw_ref.to_string()),
            status: "missing".to_string(),
            canonical_id: None,
            profile: None,
            candidates: Vec::new(),
            diagnostics: vec![format!(
                "environment ref `{raw_ref}` did not resolve in plugin `{source_plugin}` or installed environment profiles"
            )],
            verification_status: "notRun".to_string(),
        },
    }
}

pub fn canonical_environment_id(profile: &EnvironmentSpecWithSource) -> String {
    format!(
        "{}/environment/{}",
        profile.source.source_plugin,
        profile.spec.metadata.id.trim()
    )
}

fn environment_summary(profile: EnvironmentSpecWithSource) -> EnvironmentProfileSummary {
    let canonical_id = canonical_environment_id(&profile);
    EnvironmentProfileSummary {
        id: profile.spec.metadata.id,
        version: profile.spec.metadata.version,
        canonical_id,
        source_plugin: profile.source.source_plugin,
        manifest_path: profile.source.manifest_path.to_string_lossy().into_owned(),
        name: profile.spec.metadata.name,
        description: profile.spec.metadata.description,
        tags: profile.spec.metadata.tags,
        runtime: profile.spec.runtime,
        requirements: profile.spec.requirements,
        diagnostics: profile.spec.diagnostics,
    }
}

fn environment_matches_ref(profile: &EnvironmentSpecWithSource, needle: &str) -> bool {
    canonical_environment_id(profile).to_ascii_lowercase() == needle
        || profile.spec.metadata.id.trim().to_ascii_lowercase() == needle
}

fn collect_environment_manifest_paths(root: &Path, out: &mut Vec<PathBuf>) {
    for name in ENVIRONMENT_MANIFEST_NAMES {
        let direct = root.join(name);
        if direct.is_file() {
            out.push(direct);
        }
    }
    let Ok(top) = fs::read_dir(root) else {
        return;
    };
    for entry in top.flatten() {
        let path = entry.path();
        if path.is_file()
            && path
                .file_name()
                .and_then(|s| s.to_str())
                .is_some_and(is_environment_manifest_name)
        {
            out.push(path);
            continue;
        }
        if !path.is_dir() {
            continue;
        }
        for name in ENVIRONMENT_MANIFEST_NAMES {
            let candidate = path.join(name);
            if candidate.is_file() {
                out.push(candidate);
            }
        }
    }
}

fn is_environment_manifest_name(name: &str) -> bool {
    ENVIRONMENT_MANIFEST_NAMES.contains(&name)
}

fn validate_environment_id(id: &str) -> Result<(), String> {
    let trimmed = id.trim();
    if trimmed.is_empty() {
        return Err("metadata.id must not be empty".to_string());
    }
    if !trimmed
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.'))
    {
        return Err(format!(
            "metadata.id `{id}` may only contain ASCII letters, digits, `_`, `-`, and `.`"
        ));
    }
    Ok(())
}

#[allow(dead_code)]
fn ensure_relative_path(value: &Path, field: &str) -> Result<(), String> {
    for component in value.components() {
        match component {
            Component::Normal(_) | Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(format!("{field} must stay within the environment profile"));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::plugins::LoadedPlugin;
    use std::collections::HashMap;

    fn loaded_plugin(id: &str, root: &Path) -> LoadedPlugin {
        LoadedPlugin {
            id: id.to_string(),
            manifest_name: Some(id.to_string()),
            display_name: Some(id.to_string()),
            description: None,
            root: root.to_path_buf(),
            enabled: true,
            skill_roots: Vec::new(),
            mcp_servers: HashMap::new(),
            apps: Vec::new(),
            retrieval: None,
            error: None,
        }
    }

    fn write_environment(root: &Path, id: &str, command: &str) {
        let env_dir = root.join("environments").join(id);
        fs::create_dir_all(&env_dir).expect("env dir");
        fs::write(
            env_dir.join("environment.yaml"),
            format!(
                r#"apiVersion: omiga.ai/environment/v1alpha1
kind: Environment
metadata:
  id: {id}
  version: 0.1.0
  name: Test Env
runtime:
  type: system
  command: {command}
requirements:
  rPackages: [limma]
diagnostics:
  checkCommand: [{command}, --version]
  installHint: Install {command} before running this unit.
"#
            ),
        )
        .expect("env manifest");
    }

    fn write_plugin(root: &Path, name: &str) {
        fs::create_dir_all(root).expect("plugin root");
        fs::write(
            root.join("plugin.json"),
            format!(
                r#"{{
  "name": "{name}",
  "version": "0.1.0",
  "environments": "./environments"
}}"#
            ),
        )
        .expect("plugin manifest");
    }

    #[test]
    fn parses_environment_profile_and_prefers_source_plugin_ref() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let left = tmp.path().join("left");
        let right = tmp.path().join("right");
        write_plugin(&left, "left");
        write_plugin(&right, "right");
        write_environment(&left, "r-bioc", "Rscript");
        write_environment(&right, "r-bioc", "Rscript");

        let plugins = [
            loaded_plugin("left@local", &left),
            loaded_plugin("right@local", &right),
        ];
        let profiles = discover_environment_profiles_from_plugins(&plugins);
        assert_eq!(profiles.len(), 2);

        let resolved = resolve_environment_ref_from_profiles("r-bioc", "right@local", &profiles);
        assert_eq!(resolved.status, "resolved");
        assert_eq!(
            resolved.canonical_id.as_deref(),
            Some("right@local/environment/r-bioc")
        );
        assert_eq!(
            resolved.profile.as_ref().unwrap().requirements.r_packages,
            vec!["limma".to_string()]
        );
    }

    #[test]
    fn missing_environment_ref_returns_diagnostic_resolution() {
        let resolved = resolve_environment_ref_from_profiles("r-bioc", "missing@local", &[]);
        assert_eq!(resolved.status, "missing");
        assert!(resolved.diagnostics[0].contains("did not resolve"));
    }

    #[test]
    fn resolver_includes_current_plugin_root_profiles() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let plugin = tmp.path().join("plugin");
        write_plugin(&plugin, "plugin");
        write_environment(&plugin, "r-bioc", "Rscript");

        let resolved = resolve_environment_ref(Some("r-bioc"), "plugin@local", &plugin);

        assert_eq!(resolved.status, "resolved");
        assert_eq!(
            resolved.canonical_id.as_deref(),
            Some("plugin@local/environment/r-bioc")
        );
    }
}
