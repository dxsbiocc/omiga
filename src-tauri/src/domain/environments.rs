//! Unit-level Environment profile discovery and resolution.
//!
//! Environment profiles are plugin contributions referenced by executable units
//! through `runtime.envRef`. The resolver itself stays side-effect free; the
//! Operator executor consumes the resolved profile to prepare isolated conda,
//! Docker, or Singularity runtimes when a run actually needs them.

use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::Instant;

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentManifestDiagnostic {
    pub source_plugin: String,
    pub manifest_path: String,
    pub severity: String,
    pub message: String,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentCheckResult {
    pub status: String,
    #[serde(default)]
    pub command: Vec<String>,
    #[serde(default, rename = "exitCode")]
    pub exit_code: Option<i32>,
    #[serde(default)]
    pub stdout: String,
    #[serde(default)]
    pub stderr: String,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default, rename = "durationMs")]
    pub duration_ms: u128,
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

pub fn list_environment_manifest_diagnostics() -> Vec<EnvironmentManifestDiagnostic> {
    let outcome = crate::domain::plugins::plugin_load_outcome();
    environment_manifest_diagnostics_from_plugins(outcome.plugins())
}

pub fn discover_environment_profiles_from_plugins<'a>(
    plugins: impl IntoIterator<Item = &'a crate::domain::plugins::LoadedPlugin>,
) -> Vec<EnvironmentSpecWithSource> {
    let mut out = Vec::new();
    for plugin in plugins.into_iter().filter(|plugin| plugin.is_active()) {
        for manifest_path in discover_environment_manifest_paths(&plugin.root) {
            match load_environment_manifest(&manifest_path, plugin.id.clone(), plugin.root.clone())
            {
                Ok(profile)
                    if crate::domain::plugins::environment_profile_enabled(
                        &plugin.id,
                        &profile.spec.metadata.id,
                    ) =>
                {
                    out.push(profile)
                }
                Ok(_) => {}
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

pub fn environment_manifest_diagnostics_from_plugins<'a>(
    plugins: impl IntoIterator<Item = &'a crate::domain::plugins::LoadedPlugin>,
) -> Vec<EnvironmentManifestDiagnostic> {
    let mut diagnostics = Vec::new();
    for plugin in plugins.into_iter().filter(|plugin| plugin.is_active()) {
        for manifest_path in discover_environment_manifest_paths(&plugin.root) {
            if let Err(error) =
                load_environment_manifest(&manifest_path, plugin.id.clone(), plugin.root.clone())
            {
                diagnostics.push(EnvironmentManifestDiagnostic {
                    source_plugin: plugin.id.clone(),
                    manifest_path: manifest_path.to_string_lossy().into_owned(),
                    severity: "error".to_string(),
                    message: error,
                });
            }
        }
    }
    diagnostics.sort_by(|left, right| {
        left.source_plugin
            .cmp(&right.source_plugin)
            .then_with(|| left.manifest_path.cmp(&right.manifest_path))
            .then_with(|| left.message.cmp(&right.message))
    });
    diagnostics
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
            if !crate::domain::plugins::environment_profile_enabled(
                &profile.source.source_plugin,
                &profile.spec.metadata.id,
            ) {
                continue;
            }
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

pub fn check_environment_profile(profile: &EnvironmentProfileSummary) -> EnvironmentCheckResult {
    let command = profile.diagnostics.check_command.clone();
    if command.is_empty() {
        return EnvironmentCheckResult {
            status: "notConfigured".to_string(),
            command,
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
            error: Some(
                "environment profile does not declare diagnostics.checkCommand".to_string(),
            ),
            duration_ms: 0,
        };
    }
    if !is_allowed_check_command(&command) {
        return EnvironmentCheckResult {
            status: "blocked".to_string(),
            command,
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
            error: Some(
                "diagnostics.checkCommand is not in the V4 safe environment-check allowlist"
                    .to_string(),
            ),
            duration_ms: 0,
        };
    }
    let started = Instant::now();
    match std::process::Command::new(&command[0])
        .args(&command[1..])
        .output()
    {
        Ok(output) => EnvironmentCheckResult {
            status: if output.status.success() {
                "available".to_string()
            } else {
                "unavailable".to_string()
            },
            command,
            exit_code: output.status.code(),
            stdout: String::from_utf8_lossy(&output.stdout)
                .chars()
                .take(4000)
                .collect(),
            stderr: String::from_utf8_lossy(&output.stderr)
                .chars()
                .take(4000)
                .collect(),
            error: None,
            duration_ms: started.elapsed().as_millis(),
        },
        Err(err) => EnvironmentCheckResult {
            status: "unavailable".to_string(),
            command,
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
            error: Some(err.to_string()),
            duration_ms: started.elapsed().as_millis(),
        },
    }
}

pub fn environment_summary(profile: EnvironmentSpecWithSource) -> EnvironmentProfileSummary {
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

fn is_allowed_check_command(command: &[String]) -> bool {
    let Some(executable) = command.first() else {
        return false;
    };
    let basename = Path::new(executable)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(executable)
        .trim()
        .to_ascii_lowercase();
    let args = command
        .iter()
        .skip(1)
        .map(|value| value.trim().to_ascii_lowercase())
        .collect::<Vec<_>>();
    match basename.as_str() {
        "true" => args.is_empty(),
        "rscript" | "python" | "python3" | "conda" | "singularity" => {
            matches!(args.as_slice(), [arg] if arg == "--version" || arg == "-v")
        }
        "docker" => matches!(args.as_slice(), [arg] if arg == "version" || arg == "--version"),
        _ => false,
    }
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

    #[test]
    fn environment_check_runs_only_safe_check_commands() {
        let safe = EnvironmentProfileSummary {
            id: "safe".to_string(),
            version: "0.1.0".to_string(),
            canonical_id: "p/environment/safe".to_string(),
            source_plugin: "p".to_string(),
            manifest_path: "environment.yaml".to_string(),
            name: None,
            description: None,
            tags: Vec::new(),
            runtime: EnvironmentRuntimeProfile::default(),
            requirements: EnvironmentRequirements::default(),
            diagnostics: EnvironmentDiagnostics {
                check_command: vec!["true".to_string()],
                ..EnvironmentDiagnostics::default()
            },
        };
        let checked = check_environment_profile(&safe);
        assert_eq!(checked.status, "available");
        assert_eq!(checked.exit_code, Some(0));

        let blocked = EnvironmentProfileSummary {
            diagnostics: EnvironmentDiagnostics {
                check_command: vec!["/bin/sh".to_string(), "-c".to_string(), "true".to_string()],
                ..EnvironmentDiagnostics::default()
            },
            ..safe
        };
        let checked = check_environment_profile(&blocked);
        assert_eq!(checked.status, "blocked");
    }

    #[test]
    fn reports_invalid_environment_manifest_diagnostics() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let plugin = tmp.path().join("plugin");
        write_plugin(&plugin, "plugin");
        let env_dir = plugin.join("environments").join("bad");
        fs::create_dir_all(&env_dir).expect("env dir");
        fs::write(
            env_dir.join("environment.yaml"),
            r#"apiVersion: wrong
kind: Environment
metadata:
  id: bad
  version: 0.1.0
"#,
        )
        .expect("bad env");
        let loaded = loaded_plugin("plugin@local", &plugin);

        let diagnostics = environment_manifest_diagnostics_from_plugins([&loaded]);

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].source_plugin, "plugin@local");
        assert!(diagnostics[0].message.contains("unsupported apiVersion"));
    }

    #[test]
    fn discovers_omiga_plugin_visualization_r_environment_profile() {
        let plugin_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root")
            .join(".omiga/plugins/visualization-r");
        let plugin = loaded_plugin("visualization-r@omiga-curated", &plugin_root);

        let profiles = discover_environment_profiles_from_plugins([&plugin]);

        let profile = profiles
            .iter()
            .find(|profile| profile.spec.metadata.id == "r-base")
            .expect("visualization-r r-base profile");
        assert_eq!(profile.spec.runtime.command.as_deref(), Some("Rscript"));
        assert_eq!(
            profile.spec.diagnostics.check_command,
            vec!["Rscript".to_string(), "--version".to_string()]
        );
    }

    #[test]
    fn discovers_bundled_ngs_alignment_conda_environment_profiles() {
        let plugin_root =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("bundled_plugins/plugins/ngs-alignment");
        let plugin = loaded_plugin("ngs-alignment@omiga-curated", &plugin_root);

        let profiles = discover_environment_profiles_from_plugins([&plugin]);
        let ids = profiles
            .iter()
            .map(|profile| profile.spec.metadata.id.as_str())
            .collect::<Vec<_>>();

        assert_eq!(
            ids,
            vec![
                "ngs-bowtie2",
                "ngs-bwa",
                "ngs-hisat2",
                "ngs-samtools",
                "ngs-star"
            ]
        );
        assert!(profiles
            .iter()
            .all(|profile| profile.spec.runtime.kind.as_deref() == Some("conda")));
        assert!(profiles.iter().all(|profile| profile
            .source
            .manifest_path
            .parent()
            .unwrap()
            .join("conda.yaml")
            .is_file()));
    }
}
