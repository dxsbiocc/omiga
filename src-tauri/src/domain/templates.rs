//! Omiga TemplateSpec discovery and validation.
//!
//! Templates are read-only in the MVP: this module parses manifest-declared
//! template specs and exposes metadata to the Unit Index. Rendering and
//! execution are intentionally deferred to the executable Template milestone.

use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::fs;
use std::path::{Component, Path, PathBuf};

pub const TEMPLATE_API_VERSION_V1ALPHA1: &str = "omiga.ai/unit/v1alpha1";
pub const TEMPLATE_KIND: &str = "Template";
const TEMPLATE_MANIFEST_NAMES: &[&str] = &["template.yaml", "template.yml"];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TemplateMetadata {
    pub id: String,
    pub version: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TemplateClassification {
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default, rename = "stageInput")]
    pub stage_input: Vec<String>,
    #[serde(default, rename = "stageOutput")]
    pub stage_output: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TemplateExposure {
    #[serde(default, rename = "exposeToAgent")]
    pub expose_to_agent: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TemplateRuntime {
    #[serde(default, rename = "envRef")]
    pub env_ref: Option<String>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, JsonValue>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TemplateBody {
    pub engine: String,
    pub entry: PathBuf,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TemplateExecution {
    #[serde(default)]
    pub interpreter: Option<String>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, JsonValue>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TemplateSpec {
    #[serde(rename = "apiVersion")]
    pub api_version: String,
    pub kind: String,
    pub metadata: TemplateMetadata,
    #[serde(default)]
    pub classification: TemplateClassification,
    #[serde(default)]
    pub exposure: TemplateExposure,
    #[serde(default)]
    pub interface: JsonValue,
    #[serde(default)]
    pub runtime: TemplateRuntime,
    pub template: TemplateBody,
    #[serde(default)]
    pub execution: TemplateExecution,
    #[serde(default, rename = "migrationTarget")]
    pub migration_target: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TemplateSource {
    pub source_plugin: String,
    pub plugin_root: PathBuf,
    pub manifest_path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TemplateSpecWithSource {
    #[serde(flatten)]
    pub spec: TemplateSpec,
    pub source: TemplateSource,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TemplateManifestDiagnostic {
    pub source_plugin: String,
    pub manifest_path: String,
    pub severity: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TemplateCandidateSummary {
    pub id: String,
    pub version: String,
    pub name: Option<String>,
    pub description: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub source_plugin: String,
    pub manifest_path: String,
    pub classification: TemplateClassification,
    pub exposure: TemplateExposure,
    pub runtime: TemplateRuntime,
    pub template: TemplateBody,
    pub execution: TemplateExecution,
    pub migration_target: Option<String>,
}

pub fn load_template_manifest(
    manifest_path: &Path,
    source_plugin: impl Into<String>,
    plugin_root: impl Into<PathBuf>,
) -> Result<TemplateSpecWithSource, String> {
    let raw = fs::read_to_string(manifest_path).map_err(|err| {
        format!(
            "read template manifest `{}`: {err}",
            manifest_path.display()
        )
    })?;
    let spec: TemplateSpec = serde_yaml::from_str(&raw).map_err(|err| {
        format!(
            "parse template manifest `{}`: {err}",
            manifest_path.display()
        )
    })?;
    validate_template_spec(&spec, manifest_path)?;
    Ok(TemplateSpecWithSource {
        spec,
        source: TemplateSource {
            source_plugin: source_plugin.into(),
            plugin_root: plugin_root.into(),
            manifest_path: manifest_path.to_path_buf(),
        },
    })
}

pub fn validate_template_spec(spec: &TemplateSpec, manifest_path: &Path) -> Result<(), String> {
    if spec.api_version.trim() != TEMPLATE_API_VERSION_V1ALPHA1 {
        return Err(format!(
            "unsupported apiVersion `{}`; expected `{TEMPLATE_API_VERSION_V1ALPHA1}`",
            spec.api_version
        ));
    }
    if spec.kind.trim() != TEMPLATE_KIND {
        return Err(format!(
            "unsupported kind `{}`; expected `{TEMPLATE_KIND}`",
            spec.kind
        ));
    }
    validate_template_id(&spec.metadata.id)?;
    if spec.metadata.version.trim().is_empty() {
        return Err("metadata.version must not be empty".to_string());
    }
    if spec.template.engine.trim().is_empty() {
        return Err("template.engine must not be empty".to_string());
    }
    validate_template_entry(&spec.template.entry, manifest_path)?;
    Ok(())
}

pub fn discover_template_candidates() -> Vec<TemplateSpecWithSource> {
    let outcome = crate::domain::plugins::plugin_load_outcome();
    discover_template_candidates_from_plugins(outcome.plugins())
}

pub fn list_template_summaries() -> Vec<TemplateCandidateSummary> {
    discover_template_candidates()
        .into_iter()
        .map(template_summary)
        .collect()
}

pub fn list_template_manifest_diagnostics() -> Vec<TemplateManifestDiagnostic> {
    let outcome = crate::domain::plugins::plugin_load_outcome();
    template_manifest_diagnostics_from_plugins(outcome.plugins())
}

pub fn discover_template_candidates_from_plugins<'a>(
    plugins: impl IntoIterator<Item = &'a crate::domain::plugins::LoadedPlugin>,
) -> Vec<TemplateSpecWithSource> {
    let mut out = Vec::new();
    for plugin in plugins.into_iter().filter(|plugin| plugin.is_active()) {
        for manifest_path in discover_template_manifest_paths(&plugin.root) {
            match load_template_manifest(&manifest_path, plugin.id.clone(), plugin.root.clone()) {
                Ok(spec) => out.push(spec),
                Err(err) => tracing::warn!(
                    plugin_id = %plugin.id,
                    manifest = %manifest_path.display(),
                    "ignoring invalid template manifest: {err}"
                ),
            }
        }
    }
    out.sort_by(|left, right| {
        left.spec
            .metadata
            .id
            .cmp(&right.spec.metadata.id)
            .then_with(|| left.spec.metadata.version.cmp(&right.spec.metadata.version))
            .then_with(|| left.source.source_plugin.cmp(&right.source.source_plugin))
    });
    out
}

pub fn template_manifest_diagnostics_from_plugins<'a>(
    plugins: impl IntoIterator<Item = &'a crate::domain::plugins::LoadedPlugin>,
) -> Vec<TemplateManifestDiagnostic> {
    let mut diagnostics = Vec::new();
    for plugin in plugins.into_iter().filter(|plugin| plugin.is_active()) {
        for manifest_path in discover_template_manifest_paths(&plugin.root) {
            if let Err(error) =
                load_template_manifest(&manifest_path, plugin.id.clone(), plugin.root.clone())
            {
                diagnostics.push(TemplateManifestDiagnostic {
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

pub fn discover_template_manifest_paths(plugin_root: &Path) -> Vec<PathBuf> {
    let templates_root = crate::domain::plugins::load_plugin_manifest(plugin_root)
        .and_then(|manifest| manifest.templates)
        .unwrap_or_else(|| plugin_root.join("templates"));
    let mut out = Vec::new();
    if !templates_root.is_dir() {
        return out;
    }
    collect_template_manifest_paths(&templates_root, &mut out);
    out.sort();
    out.dedup();
    out
}

fn collect_template_manifest_paths(root: &Path, out: &mut Vec<PathBuf>) {
    for name in TEMPLATE_MANIFEST_NAMES {
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
                .is_some_and(is_template_manifest_name)
        {
            out.push(path);
            continue;
        }
        if !path.is_dir() {
            continue;
        }
        for name in TEMPLATE_MANIFEST_NAMES {
            let candidate = path.join(name);
            if candidate.is_file() {
                out.push(candidate);
            }
        }
        let Ok(children) = fs::read_dir(&path) else {
            continue;
        };
        for child in children.flatten() {
            let child_path = child.path();
            if !child_path.is_dir() {
                continue;
            }
            for name in TEMPLATE_MANIFEST_NAMES {
                let candidate = child_path.join(name);
                if candidate.is_file() {
                    out.push(candidate);
                }
            }
        }
    }
}

fn is_template_manifest_name(name: &str) -> bool {
    TEMPLATE_MANIFEST_NAMES.contains(&name)
}

fn template_summary(candidate: TemplateSpecWithSource) -> TemplateCandidateSummary {
    TemplateCandidateSummary {
        id: candidate.spec.metadata.id,
        version: candidate.spec.metadata.version,
        name: candidate.spec.metadata.name,
        description: candidate.spec.metadata.description,
        tags: candidate.spec.metadata.tags,
        source_plugin: candidate.source.source_plugin,
        manifest_path: candidate
            .source
            .manifest_path
            .to_string_lossy()
            .into_owned(),
        classification: candidate.spec.classification,
        exposure: candidate.spec.exposure,
        runtime: candidate.spec.runtime,
        template: candidate.spec.template,
        execution: candidate.spec.execution,
        migration_target: candidate.spec.migration_target,
    }
}

fn validate_template_id(id: &str) -> Result<(), String> {
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

fn validate_template_entry(entry: &Path, manifest_path: &Path) -> Result<(), String> {
    let raw = entry.to_string_lossy();
    let Some(rel) = raw.strip_prefix("./") else {
        return Err("template.entry must start with `./`".to_string());
    };
    if rel.trim().is_empty() {
        return Err("template.entry must not be empty".to_string());
    }
    for component in Path::new(rel).components() {
        match component {
            Component::Normal(_) | Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err("template.entry must stay within the template directory".to_string());
            }
        }
    }
    let base = manifest_path.parent().unwrap_or_else(|| Path::new(""));
    let resolved = base.join(rel);
    if resolved == base {
        return Err("template.entry must not resolve to the template directory".to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::plugins::LoadedPlugin;
    use std::collections::HashMap;

    fn write_valid_template(path: &Path, id: &str) {
        fs::write(
            path,
            format!(
                r#"apiVersion: omiga.ai/unit/v1alpha1
kind: Template
metadata:
  id: {id}
  version: 0.1.0
  name: Demo Template
  description: Demo read-only template.
classification:
  category: omics/demo
  tags: [demo, table]
  stageInput: [matrix]
  stageOutput: [report]
exposure:
  exposeToAgent: true
interface:
  inputs: {{}}
  params: {{}}
  outputs: {{}}
runtime:
  envRef: r-bioc
template:
  engine: static
  entry: ./template.yaml
execution:
  interpreter: existing-operator
migrationTarget: demo_operator
"#
            ),
        )
        .expect("write template");
    }

    #[test]
    fn parses_and_validates_template_spec() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let manifest = tmp.path().join("template.yaml");
        write_valid_template(&manifest, "demo_template");

        let loaded =
            load_template_manifest(&manifest, "demo@market", tmp.path()).expect("template spec");

        assert_eq!(loaded.spec.metadata.id, "demo_template");
        assert_eq!(
            loaded.spec.classification.category.as_deref(),
            Some("omics/demo")
        );
        assert!(loaded.spec.exposure.expose_to_agent);
        assert_eq!(loaded.spec.runtime.env_ref.as_deref(), Some("r-bioc"));
    }

    #[test]
    fn rejects_template_entry_escape() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let manifest = tmp.path().join("template.yaml");
        fs::write(
            &manifest,
            r#"apiVersion: omiga.ai/unit/v1alpha1
kind: Template
metadata:
  id: bad
  version: 0.1.0
template:
  engine: jinja2
  entry: ../bad.R
"#,
        )
        .expect("write bad template");

        let error = load_template_manifest(&manifest, "demo@market", tmp.path())
            .expect_err("escape should be rejected");
        assert!(
            error.contains("template.entry must start with `./`"),
            "{error}"
        );
    }

    #[test]
    fn discovers_templates_from_plugin_templates_path() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let plugin_root = tmp.path().join("plugin");
        let template_dir = plugin_root.join("custom-templates").join("demo");
        fs::create_dir_all(&template_dir).expect("mkdir");
        fs::write(
            plugin_root.join("plugin.json"),
            r#"{"name":"demo","version":"0.1.0","templates":"./custom-templates"}"#,
        )
        .expect("manifest");
        write_valid_template(&template_dir.join("template.yaml"), "demo_template");

        let plugin = LoadedPlugin {
            id: "demo@market".to_string(),
            manifest_name: Some("demo".to_string()),
            display_name: Some("Demo".to_string()),
            description: None,
            root: plugin_root,
            enabled: true,
            skill_roots: Vec::new(),
            mcp_servers: HashMap::new(),
            apps: Vec::new(),
            retrieval: None,
            error: None,
        };

        let candidates = discover_template_candidates_from_plugins([&plugin]);

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].spec.metadata.id, "demo_template");
        assert_eq!(candidates[0].source.source_plugin, "demo@market");
    }

    #[test]
    fn reports_invalid_template_diagnostics() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let plugin_root = tmp.path().join("plugin");
        let template_dir = plugin_root.join("templates").join("demo");
        fs::create_dir_all(&template_dir).expect("mkdir");
        fs::write(
            plugin_root.join("plugin.json"),
            r#"{"name":"demo","version":"0.1.0","templates":"./templates"}"#,
        )
        .expect("manifest");
        fs::write(
            template_dir.join("template.yaml"),
            r#"apiVersion: wrong
kind: Template
metadata:
  id: bad
  version: 0.1.0
template:
  engine: static
  entry: ./template.yaml
"#,
        )
        .expect("bad template");
        let plugin = LoadedPlugin {
            id: "demo@market".to_string(),
            manifest_name: Some("demo".to_string()),
            display_name: Some("Demo".to_string()),
            description: None,
            root: plugin_root,
            enabled: true,
            skill_roots: Vec::new(),
            mcp_servers: HashMap::new(),
            apps: Vec::new(),
            retrieval: None,
            error: None,
        };

        let diagnostics = template_manifest_diagnostics_from_plugins([&plugin]);

        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("unsupported apiVersion"));
    }

    #[test]
    fn discovers_bundled_differential_expression_template() {
        let plugin_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("bundled_plugins/plugins/operator-differential-expression-r");
        let plugin = LoadedPlugin {
            id: "operator-differential-expression-r@omiga-curated".to_string(),
            manifest_name: Some("operator-differential-expression-r".to_string()),
            display_name: Some("Differential Expression".to_string()),
            description: None,
            root: plugin_root,
            enabled: true,
            skill_roots: Vec::new(),
            mcp_servers: HashMap::new(),
            apps: Vec::new(),
            retrieval: None,
            error: None,
        };

        let candidates = discover_template_candidates_from_plugins([&plugin]);
        let template = candidates
            .iter()
            .find(|candidate| candidate.spec.metadata.id == "bulk_differential_expression_basic")
            .expect("bundled differential expression template should be discovered");

        assert_eq!(
            template.spec.migration_target.as_deref(),
            Some("omics_differential_expression_basic")
        );
        assert_eq!(
            template.spec.classification.category.as_deref(),
            Some("omics/transcriptomics/differential")
        );
        assert!(template.spec.exposure.expose_to_agent);
    }
}
