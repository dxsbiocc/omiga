//! Omiga operator runtime.
//!
//! Operators are plugin-provided, declarative execution units exposed to the
//! model as dynamic `operator__{id}` tools.  They are intentionally separate
//! from MCP: Omiga owns the workspace, resource, validation, artifact, and
//! provenance lifecycle.
//!
//! The MVP keeps rich structured errors and explicit execution context in one
//! module so UI/model responses can include actionable field/run/log metadata.
//! Revisit these clippy allowances when the runtime is split into smaller
//! registry/validation/execution modules.

#![allow(clippy::result_large_err, clippy::too_many_arguments)]

use crate::domain::tools::ToolSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map as JsonMap, Value as JsonValue};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

pub const OPERATOR_API_VERSION_V1ALPHA1: &str = "omiga.ai/operator/v1alpha1";
pub const OPERATOR_KIND: &str = "Operator";
pub const OPERATOR_TOOL_PREFIX: &str = "operator__";
const REGISTRY_RELATIVE_PATH: &str = "operators/registry.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperatorMetadata {
    pub id: String,
    pub version: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperatorFieldKind {
    #[default]
    String,
    Integer,
    Number,
    Boolean,
    Enum,
    File,
    FileArray,
    Directory,
    DirectoryArray,
    Json,
}

impl OperatorFieldKind {
    fn parse(raw: Option<&str>) -> Self {
        match raw.unwrap_or("string").trim().to_ascii_lowercase().as_str() {
            "int" | "integer" => Self::Integer,
            "float" | "double" | "number" => Self::Number,
            "bool" | "boolean" => Self::Boolean,
            "enum" => Self::Enum,
            "file" => Self::File,
            "files" | "file_array" | "file-array" | "array[file]" => Self::FileArray,
            "directory" | "dir" => Self::Directory,
            "directories" | "directory_array" | "directory-array" | "dir_array" => {
                Self::DirectoryArray
            }
            "json" | "object" => Self::Json,
            _ => Self::String,
        }
    }

    fn is_array(&self) -> bool {
        matches!(self, Self::FileArray | Self::DirectoryArray)
    }

    fn is_path_like(&self) -> bool {
        matches!(
            self,
            Self::File | Self::FileArray | Self::Directory | Self::DirectoryArray
        )
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperatorFieldSpec {
    pub kind: OperatorFieldKind,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub default: Option<JsonValue>,
    #[serde(default, rename = "enum")]
    pub enum_values: Vec<JsonValue>,
    #[serde(default)]
    pub formats: Vec<String>,
    #[serde(default)]
    pub minimum: Option<f64>,
    #[serde(default)]
    pub maximum: Option<f64>,
    #[serde(default)]
    pub min_size: Option<u64>,
    #[serde(default)]
    pub glob: Option<String>,
    #[serde(default)]
    pub non_empty: Option<bool>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperatorInterfaceSpec {
    #[serde(default)]
    pub inputs: BTreeMap<String, OperatorFieldSpec>,
    #[serde(default)]
    pub params: BTreeMap<String, OperatorFieldSpec>,
    #[serde(default)]
    pub outputs: BTreeMap<String, OperatorFieldSpec>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperatorResourceSpec {
    #[serde(default)]
    pub default: Option<JsonValue>,
    #[serde(default)]
    pub min: Option<JsonValue>,
    #[serde(default)]
    pub max: Option<JsonValue>,
    #[serde(default)]
    pub exposed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperatorBindingSpec {
    pub param: String,
    pub resource: String,
    #[serde(default = "default_equal_mode")]
    pub mode: String,
}

fn default_equal_mode() -> String {
    "equal".to_string()
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperatorExecutionSpec {
    pub argv: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperatorSource {
    pub source_plugin: String,
    pub plugin_root: PathBuf,
    pub manifest_path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperatorSpec {
    pub api_version: String,
    pub kind: String,
    pub metadata: OperatorMetadata,
    pub interface: OperatorInterfaceSpec,
    pub execution: OperatorExecutionSpec,
    #[serde(default)]
    pub runtime: Option<JsonValue>,
    #[serde(default)]
    pub resources: BTreeMap<String, OperatorResourceSpec>,
    #[serde(default)]
    pub bindings: Vec<OperatorBindingSpec>,
    #[serde(default)]
    pub permissions: Option<JsonValue>,
    pub source: OperatorSource,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperatorCandidateSummary {
    pub id: String,
    pub version: String,
    pub name: Option<String>,
    pub description: Option<String>,
    pub source_plugin: String,
    pub manifest_path: String,
    pub enabled_aliases: Vec<String>,
    pub exposed: bool,
    pub unavailable_reason: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ResolvedOperator {
    pub alias: String,
    pub spec: OperatorSpec,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperatorRegistryFile {
    #[serde(default)]
    pub enabled: BTreeMap<String, OperatorRegistryEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperatorRegistryUpdate {
    pub alias: String,
    #[serde(default)]
    pub operator_id: Option<String>,
    #[serde(default)]
    pub source_plugin: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum OperatorRegistryEntry {
    Version(String),
    Full {
        #[serde(default, alias = "operator_id")]
        operator_id: Option<String>,
        #[serde(default, alias = "source_plugin")]
        source_plugin: Option<String>,
        #[serde(default)]
        version: Option<String>,
        #[serde(default)]
        enabled: Option<bool>,
    },
}

impl OperatorRegistryEntry {
    fn enabled(&self) -> bool {
        match self {
            Self::Version(_) => true,
            Self::Full { enabled, .. } => enabled.unwrap_or(true),
        }
    }

    fn operator_id<'a>(&'a self, alias: &'a str) -> &'a str {
        match self {
            Self::Version(_) => alias,
            Self::Full { operator_id, .. } => operator_id.as_deref().unwrap_or(alias),
        }
    }

    fn version(&self) -> Option<&str> {
        match self {
            Self::Version(version) => Some(version.as_str()),
            Self::Full { version, .. } => version.as_deref(),
        }
    }

    fn source_plugin(&self) -> Option<&str> {
        match self {
            Self::Version(_) => None,
            Self::Full { source_plugin, .. } => source_plugin.as_deref(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawOperatorManifest {
    #[serde(rename = "apiVersion")]
    api_version: String,
    kind: String,
    metadata: RawOperatorMetadata,
    #[serde(default)]
    interface: RawOperatorInterface,
    execution: RawOperatorExecution,
    #[serde(default)]
    runtime: Option<JsonValue>,
    #[serde(default)]
    resources: BTreeMap<String, RawResourceSpec>,
    #[serde(default)]
    bindings: Vec<OperatorBindingSpec>,
    #[serde(default)]
    permissions: Option<JsonValue>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawOperatorMetadata {
    id: String,
    version: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawOperatorInterface {
    #[serde(default)]
    inputs: BTreeMap<String, RawFieldSpec>,
    #[serde(default)]
    params: BTreeMap<String, RawFieldSpec>,
    #[serde(default)]
    outputs: BTreeMap<String, RawFieldSpec>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawFieldSpec {
    #[serde(default, alias = "type")]
    kind: Option<String>,
    #[serde(default)]
    required: Option<bool>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    default: Option<JsonValue>,
    #[serde(default, rename = "enum")]
    enum_values: Vec<JsonValue>,
    #[serde(default)]
    formats: Vec<String>,
    #[serde(default)]
    minimum: Option<f64>,
    #[serde(default)]
    maximum: Option<f64>,
    #[serde(default)]
    min_size: Option<u64>,
    #[serde(default)]
    glob: Option<String>,
    #[serde(default)]
    non_empty: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawOperatorExecution {
    #[serde(default)]
    argv: Option<Vec<String>>,
    #[serde(default)]
    command: Option<RawOperatorCommand>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawOperatorCommand {
    #[serde(default)]
    argv: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum RawResourceSpec {
    Rich(OperatorResourceSpec),
    Scalar(JsonValue),
}

impl From<RawFieldSpec> for OperatorFieldSpec {
    fn from(raw: RawFieldSpec) -> Self {
        Self {
            kind: OperatorFieldKind::parse(raw.kind.as_deref()),
            required: raw.required.unwrap_or(false),
            description: raw.description,
            default: raw.default,
            enum_values: raw.enum_values,
            formats: raw.formats,
            minimum: raw.minimum,
            maximum: raw.maximum,
            min_size: raw.min_size,
            glob: raw.glob,
            non_empty: raw.non_empty,
        }
    }
}

impl From<RawResourceSpec> for OperatorResourceSpec {
    fn from(raw: RawResourceSpec) -> Self {
        match raw {
            RawResourceSpec::Rich(spec) => spec,
            RawResourceSpec::Scalar(value) => Self {
                default: Some(value),
                ..Self::default()
            },
        }
    }
}

pub fn registry_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".omiga")
        .join(REGISTRY_RELATIVE_PATH)
}

pub fn load_registry_file() -> OperatorRegistryFile {
    fs::read_to_string(registry_path())
        .ok()
        .and_then(|raw| serde_json::from_str::<OperatorRegistryFile>(&raw).ok())
        .unwrap_or_default()
}

fn write_registry_file(registry: &OperatorRegistryFile) -> Result<(), String> {
    let path = registry_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| format!("create operator registry dir: {err}"))?;
    }
    let raw = serde_json::to_string_pretty(registry).map_err(|err| err.to_string())?;
    fs::write(&path, format!("{raw}\n")).map_err(|err| format!("write operator registry: {err}"))
}

pub fn load_operator_manifest(
    manifest_path: &Path,
    source_plugin: impl Into<String>,
    plugin_root: impl Into<PathBuf>,
) -> Result<OperatorSpec, String> {
    let raw = fs::read_to_string(manifest_path)
        .map_err(|err| format!("read operator manifest {}: {err}", manifest_path.display()))?;
    let parsed: RawOperatorManifest = serde_yaml::from_str(&raw)
        .map_err(|err| format!("parse operator manifest {}: {err}", manifest_path.display()))?;
    if parsed.api_version != OPERATOR_API_VERSION_V1ALPHA1 {
        return Err(format!(
            "unsupported operator apiVersion `{}` in {}",
            parsed.api_version,
            manifest_path.display()
        ));
    }
    if parsed.kind != OPERATOR_KIND {
        return Err(format!(
            "unsupported operator kind `{}` in {}",
            parsed.kind,
            manifest_path.display()
        ));
    }
    validate_operator_id(&parsed.metadata.id)?;
    if parsed.metadata.version.trim().is_empty() {
        return Err("operator metadata.version must not be empty".to_string());
    }
    let argv = parsed
        .execution
        .argv
        .or_else(|| parsed.execution.command.and_then(|command| command.argv))
        .ok_or_else(|| "operator execution.argv is required".to_string())?;
    if argv.is_empty() {
        return Err("operator execution.argv must not be empty".to_string());
    }
    Ok(OperatorSpec {
        api_version: parsed.api_version,
        kind: parsed.kind,
        metadata: OperatorMetadata {
            id: parsed.metadata.id,
            version: parsed.metadata.version,
            name: parsed.metadata.name,
            description: parsed.metadata.description,
            tags: parsed.metadata.tags,
        },
        interface: OperatorInterfaceSpec {
            inputs: parsed
                .interface
                .inputs
                .into_iter()
                .map(|(key, value)| (key, value.into()))
                .collect(),
            params: parsed
                .interface
                .params
                .into_iter()
                .map(|(key, value)| (key, value.into()))
                .collect(),
            outputs: parsed
                .interface
                .outputs
                .into_iter()
                .map(|(key, value)| (key, value.into()))
                .collect(),
        },
        execution: OperatorExecutionSpec { argv },
        runtime: parsed.runtime,
        resources: parsed
            .resources
            .into_iter()
            .map(|(key, value)| (key, value.into()))
            .collect(),
        bindings: parsed.bindings,
        permissions: parsed.permissions,
        source: OperatorSource {
            source_plugin: source_plugin.into(),
            plugin_root: plugin_root.into(),
            manifest_path: manifest_path.to_path_buf(),
        },
    })
}

fn validate_operator_id(id: &str) -> Result<(), String> {
    let trimmed = id.trim();
    if trimmed.is_empty() {
        return Err("operator metadata.id must not be empty".to_string());
    }
    if !trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
    {
        return Err(format!(
            "operator metadata.id `{id}` must contain only letters, numbers, '.', '-' or '_'"
        ));
    }
    Ok(())
}

pub fn discover_operator_candidates() -> Vec<OperatorSpec> {
    let mut out = Vec::new();
    let outcome = crate::domain::plugins::plugin_load_outcome();
    for plugin in outcome.plugins().iter().filter(|plugin| plugin.is_active()) {
        for manifest_path in discover_manifest_paths(&plugin.root) {
            match load_operator_manifest(&manifest_path, plugin.id.clone(), plugin.root.clone()) {
                Ok(spec) => out.push(spec),
                Err(err) => tracing::warn!(
                    plugin_id = %plugin.id,
                    manifest = %manifest_path.display(),
                    "ignoring invalid operator manifest: {err}"
                ),
            }
        }
    }
    out.sort_by(|left, right| {
        left.metadata
            .id
            .cmp(&right.metadata.id)
            .then_with(|| left.metadata.version.cmp(&right.metadata.version))
            .then_with(|| left.source.source_plugin.cmp(&right.source.source_plugin))
    });
    out
}

fn discover_manifest_paths(plugin_root: &Path) -> Vec<PathBuf> {
    let operators_root = plugin_root.join("operators");
    let mut out = Vec::new();
    if !operators_root.is_dir() {
        return out;
    }
    let Ok(top) = fs::read_dir(&operators_root) else {
        return out;
    };
    for entry in top.flatten() {
        let path = entry.path();
        if path.is_file() && path.file_name().and_then(|s| s.to_str()) == Some("operator.yaml") {
            out.push(path);
            continue;
        }
        if !path.is_dir() {
            continue;
        }
        let direct = path.join("operator.yaml");
        if direct.is_file() {
            out.push(direct);
        }
        let Ok(children) = fs::read_dir(&path) else {
            continue;
        };
        for child in children.flatten() {
            let candidate = child.path().join("operator.yaml");
            if candidate.is_file() {
                out.push(candidate);
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

pub fn resolve_enabled_operators() -> Vec<ResolvedOperator> {
    resolve_enabled_operators_from(discover_operator_candidates(), load_registry_file())
}

pub fn set_operator_enabled(update: OperatorRegistryUpdate) -> Result<(), String> {
    let mut registry = load_registry_file();
    apply_operator_registry_update(&mut registry, discover_operator_candidates(), update)?;
    write_registry_file(&registry)
}

fn apply_operator_registry_update(
    registry: &mut OperatorRegistryFile,
    candidates: Vec<OperatorSpec>,
    update: OperatorRegistryUpdate,
) -> Result<(), String> {
    validate_operator_id(&update.alias)?;
    let operator_id = update
        .operator_id
        .as_deref()
        .unwrap_or(update.alias.as_str())
        .trim()
        .to_string();
    validate_operator_id(&operator_id)?;

    if !update.enabled {
        registry.enabled.insert(
            update.alias,
            OperatorRegistryEntry::Full {
                operator_id: Some(operator_id),
                source_plugin: update.source_plugin,
                version: update.version,
                enabled: Some(false),
            },
        );
        return Ok(());
    }

    let matches = candidates
        .into_iter()
        .filter(|candidate| candidate.metadata.id == operator_id)
        .filter(|candidate| {
            update
                .version
                .as_deref()
                .map(|version| candidate.metadata.version == version)
                .unwrap_or(true)
        })
        .filter(|candidate| {
            update
                .source_plugin
                .as_deref()
                .map(|plugin| candidate.source.source_plugin == plugin)
                .unwrap_or(true)
        })
        .collect::<Vec<_>>();

    let selected = match matches.as_slice() {
        [only] => only,
        [] => {
            return Err(format!(
                "operator `{operator_id}` could not be resolved from installed enabled plugins"
            ))
        }
        many => {
            return Err(format!(
                "operator `{operator_id}` is ambiguous across {} candidates; specify sourcePlugin and version",
                many.len()
            ))
        }
    };

    registry.enabled.insert(
        update.alias,
        OperatorRegistryEntry::Full {
            operator_id: Some(selected.metadata.id.clone()),
            source_plugin: Some(selected.source.source_plugin.clone()),
            version: Some(selected.metadata.version.clone()),
            enabled: Some(true),
        },
    );
    Ok(())
}

fn resolve_enabled_operators_from(
    candidates: Vec<OperatorSpec>,
    registry: OperatorRegistryFile,
) -> Vec<ResolvedOperator> {
    let mut resolved = Vec::new();
    for (alias, entry) in registry.enabled {
        if !entry.enabled() {
            continue;
        }
        if validate_operator_id(&alias).is_err() {
            tracing::warn!(alias = %alias, "ignoring invalid operator registry alias");
            continue;
        }
        let wanted_id = entry.operator_id(&alias);
        let matches = candidates
            .iter()
            .filter(|candidate| candidate.metadata.id == wanted_id)
            .filter(|candidate| {
                entry
                    .version()
                    .map(|version| candidate.metadata.version == version)
                    .unwrap_or(true)
            })
            .filter(|candidate| {
                entry
                    .source_plugin()
                    .map(|plugin| candidate.source.source_plugin == plugin)
                    .unwrap_or(true)
            })
            .cloned()
            .collect::<Vec<_>>();
        match matches.as_slice() {
            [only] => resolved.push(ResolvedOperator {
                alias,
                spec: only.clone(),
            }),
            [] => tracing::warn!(
                alias = %alias,
                operator_id = %wanted_id,
                "enabled operator could not be resolved"
            ),
            many => tracing::warn!(
                alias = %alias,
                operator_id = %wanted_id,
                candidates = many.len(),
                "enabled operator is ambiguous; set sourcePlugin and version"
            ),
        }
    }
    resolved.sort_by(|left, right| left.alias.cmp(&right.alias));
    resolved
}

pub fn resolve_operator_alias(alias: &str) -> Result<ResolvedOperator, OperatorToolError> {
    let alias = alias
        .strip_prefix(OPERATOR_TOOL_PREFIX)
        .unwrap_or(alias)
        .trim();
    for resolved in resolve_enabled_operators() {
        if resolved.alias == alias {
            return Ok(resolved);
        }
    }
    Err(OperatorToolError::new(
        "unknown_operator",
        false,
        format!("Operator `{alias}` is not enabled or could not be resolved."),
    )
    .with_suggested_action("Run operator_list to inspect installed/enabled operators."))
}

pub fn describe_operator(
    id_or_alias: &str,
) -> Result<(Option<String>, OperatorSpec), OperatorToolError> {
    if let Ok(resolved) = resolve_operator_alias(id_or_alias) {
        return Ok((Some(resolved.alias), resolved.spec));
    }
    let id = id_or_alias
        .strip_prefix(OPERATOR_TOOL_PREFIX)
        .unwrap_or(id_or_alias)
        .trim();
    let matches = discover_operator_candidates()
        .into_iter()
        .filter(|candidate| candidate.metadata.id == id)
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [only] => Ok((None, only.clone())),
        [] => Err(OperatorToolError::new(
            "unknown_operator",
            false,
            format!("Operator `{id}` is not installed or enabled."),
        )
        .with_suggested_action("Run operator_list to inspect installed operators.")),
        many => Err(OperatorToolError::new(
            "operator_version_unresolved",
            false,
            format!(
                "Operator `{id}` has {} installed candidates; enable one alias in the operator registry first.",
                many.len()
            ),
        )
        .with_suggested_action("Resolve the operator source/version conflict in settings or registry.json.")),
    }
}

pub fn list_operator_summaries() -> Vec<OperatorCandidateSummary> {
    let candidates = discover_operator_candidates();
    let enabled = resolve_enabled_operators();
    let mut enabled_by_key: HashMap<(String, String, String), Vec<String>> = HashMap::new();
    for item in enabled {
        enabled_by_key
            .entry((
                item.spec.metadata.id.clone(),
                item.spec.metadata.version.clone(),
                item.spec.source.source_plugin.clone(),
            ))
            .or_default()
            .push(item.alias);
    }
    candidates
        .into_iter()
        .map(|candidate| {
            let aliases = enabled_by_key
                .remove(&(
                    candidate.metadata.id.clone(),
                    candidate.metadata.version.clone(),
                    candidate.source.source_plugin.clone(),
                ))
                .unwrap_or_default();
            OperatorCandidateSummary {
                id: candidate.metadata.id,
                version: candidate.metadata.version,
                name: candidate.metadata.name,
                description: candidate.metadata.description,
                source_plugin: candidate.source.source_plugin,
                manifest_path: candidate
                    .source
                    .manifest_path
                    .to_string_lossy()
                    .into_owned(),
                exposed: !aliases.is_empty(),
                enabled_aliases: aliases,
                unavailable_reason: None,
            }
        })
        .collect()
}

pub fn enabled_operator_tool_schemas() -> Vec<ToolSchema> {
    resolve_enabled_operators()
        .into_iter()
        .map(operator_tool_schema)
        .collect()
}

pub fn operator_tool_schema(operator: ResolvedOperator) -> ToolSchema {
    let name = format!("{OPERATOR_TOOL_PREFIX}{}", operator.alias);
    let description = operator
        .spec
        .metadata
        .description
        .clone()
        .or_else(|| operator.spec.metadata.name.clone())
        .unwrap_or_else(|| {
            format!(
                "Run operator {}@{}",
                operator.spec.metadata.id, operator.spec.metadata.version
            )
        });
    ToolSchema::new(
        name,
        description,
        operator_parameters_schema(&operator.spec),
    )
}

pub fn operator_parameters_schema(spec: &OperatorSpec) -> JsonValue {
    let mut properties = JsonMap::new();
    properties.insert(
        "inputs".to_string(),
        fields_object_schema(&spec.interface.inputs, true),
    );
    properties.insert(
        "params".to_string(),
        fields_object_schema(&spec.interface.params, false),
    );
    properties.insert(
        "resources".to_string(),
        resources_object_schema(&spec.resources),
    );
    json!({
        "type": "object",
        "properties": properties,
        "required": ["inputs"],
        "additionalProperties": false
    })
}

fn fields_object_schema(
    fields: &BTreeMap<String, OperatorFieldSpec>,
    include_required: bool,
) -> JsonValue {
    let mut properties = JsonMap::new();
    let mut required = Vec::new();
    for (name, field) in fields {
        if include_required && field.required {
            required.push(JsonValue::String(name.clone()));
        }
        properties.insert(name.clone(), field_schema(field));
    }
    let mut schema = JsonMap::new();
    schema.insert("type".to_string(), JsonValue::String("object".to_string()));
    schema.insert("properties".to_string(), JsonValue::Object(properties));
    schema.insert("additionalProperties".to_string(), JsonValue::Bool(false));
    if !required.is_empty() {
        schema.insert("required".to_string(), JsonValue::Array(required));
    }
    JsonValue::Object(schema)
}

fn field_schema(field: &OperatorFieldSpec) -> JsonValue {
    let mut schema = JsonMap::new();
    match field.kind {
        OperatorFieldKind::Integer => {
            schema.insert("type".to_string(), JsonValue::String("integer".to_string()));
        }
        OperatorFieldKind::Number => {
            schema.insert("type".to_string(), JsonValue::String("number".to_string()));
        }
        OperatorFieldKind::Boolean => {
            schema.insert("type".to_string(), JsonValue::String("boolean".to_string()));
        }
        OperatorFieldKind::Json => {
            schema.insert("type".to_string(), JsonValue::String("object".to_string()));
        }
        kind if kind.is_array() => {
            schema.insert("type".to_string(), JsonValue::String("array".to_string()));
            schema.insert("items".to_string(), json!({"type": "string"}));
        }
        _ => {
            schema.insert("type".to_string(), JsonValue::String("string".to_string()));
        }
    }
    if let Some(description) = field_description(field) {
        schema.insert("description".to_string(), JsonValue::String(description));
    }
    if let Some(default) = &field.default {
        schema.insert("default".to_string(), default.clone());
    }
    if !field.enum_values.is_empty() {
        schema.insert(
            "enum".to_string(),
            JsonValue::Array(field.enum_values.clone()),
        );
    }
    if let Some(minimum) = field.minimum {
        schema.insert("minimum".to_string(), json!(minimum));
    }
    if let Some(maximum) = field.maximum {
        schema.insert("maximum".to_string(), json!(maximum));
    }
    JsonValue::Object(schema)
}

fn field_description(field: &OperatorFieldSpec) -> Option<String> {
    let mut parts = Vec::new();
    if let Some(description) = &field.description {
        parts.push(description.clone());
    }
    if field.kind.is_path_like() {
        parts.push(
            "Path string accepted; Omiga canonicalizes it to a FileRef/ArtifactRef.".to_string(),
        );
    }
    if !field.formats.is_empty() {
        parts.push(format!("Expected formats: {}.", field.formats.join(", ")));
    }
    (!parts.is_empty()).then(|| parts.join(" "))
}

fn resources_object_schema(resources: &BTreeMap<String, OperatorResourceSpec>) -> JsonValue {
    let mut properties = JsonMap::new();
    for (name, resource) in resources.iter().filter(|(_, resource)| resource.exposed) {
        let mut schema = JsonMap::new();
        match name.as_str() {
            "cpu" | "gpu" => {
                schema.insert("type".to_string(), JsonValue::String("integer".to_string()));
                schema.insert("minimum".to_string(), json!(0));
            }
            "memory" | "disk" | "walltime" => {
                schema.insert("type".to_string(), JsonValue::String("string".to_string()));
            }
            _ => {
                schema.insert(
                    "description".to_string(),
                    JsonValue::String("Resource override.".to_string()),
                );
            }
        }
        if let Some(default) = &resource.default {
            schema.insert("default".to_string(), default.clone());
        }
        properties.insert(name.clone(), JsonValue::Object(schema));
    }
    json!({
        "type": "object",
        "properties": properties,
        "additionalProperties": false
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperatorInvocation {
    #[serde(default)]
    pub inputs: BTreeMap<String, JsonValue>,
    #[serde(default)]
    pub params: BTreeMap<String, JsonValue>,
    #[serde(default)]
    pub resources: BTreeMap<String, JsonValue>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperatorToolError {
    pub kind: String,
    pub retryable: bool,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_dir: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stdout_tail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stderr_tail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggested_action: Option<String>,
}

impl OperatorToolError {
    pub fn new(kind: impl Into<String>, retryable: bool, message: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            retryable,
            message: message.into(),
            field: None,
            run_dir: None,
            stdout_tail: None,
            stderr_tail: None,
            suggested_action: None,
        }
    }

    pub fn with_field(mut self, field: impl Into<String>) -> Self {
        self.field = Some(field.into());
        self
    }

    pub fn with_run_dir(mut self, run_dir: impl Into<String>) -> Self {
        self.run_dir = Some(run_dir.into());
        self
    }

    pub fn with_logs(mut self, stdout_tail: Option<String>, stderr_tail: Option<String>) -> Self {
        self.stdout_tail = stdout_tail;
        self.stderr_tail = stderr_tail;
        self
    }

    pub fn with_suggested_action(mut self, action: impl Into<String>) -> Self {
        self.suggested_action = Some(action.into());
        self
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct OperatorRunResult {
    status: String,
    run_id: String,
    operator: OperatorRunIdentity,
    run_dir: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    provenance_path: Option<String>,
    outputs: BTreeMap<String, Vec<ArtifactRef>>,
    effective_params: BTreeMap<String, JsonValue>,
    effective_resources: BTreeMap<String, JsonValue>,
    enforcement: JsonValue,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<OperatorToolError>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct OperatorRunIdentity {
    alias: String,
    id: String,
    version: String,
    source_plugin: String,
    manifest_path: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactRef {
    pub location: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server: Option<String>,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<JsonValue>,
}

pub async fn execute_operator_tool_call(
    ctx: &crate::domain::tools::ToolContext,
    tool_name: &str,
    arguments: &str,
) -> (String, bool) {
    let alias = tool_name
        .strip_prefix(OPERATOR_TOOL_PREFIX)
        .unwrap_or(tool_name);
    let resolved = match resolve_operator_alias(alias) {
        Ok(resolved) => resolved,
        Err(error) => return (failure_json(alias, None, None, error), true),
    };
    let invocation = match serde_json::from_str::<OperatorInvocation>(arguments) {
        Ok(invocation) => invocation,
        Err(err) => {
            let error = OperatorToolError::new(
                "invalid_arguments",
                false,
                format!(
                    "Operator arguments must be JSON object {{inputs, params, resources}}: {err}"
                ),
            )
            .with_suggested_action(
                "Retry with the operator schema's inputs/params/resources shape.",
            );
            return (failure_json(alias, Some(&resolved), None, error), true);
        }
    };

    match execute_resolved_operator(ctx, resolved.clone(), invocation).await {
        Ok(result) => (
            serde_json::to_string_pretty(&result).unwrap_or_else(|err| {
                failure_json(
                    alias,
                    Some(&resolved),
                    None,
                    OperatorToolError::new("serialization_failed", false, err.to_string()),
                )
            }),
            false,
        ),
        Err(error) => {
            let run_dir = error.run_dir.clone();
            (
                failure_json(alias, Some(&resolved), run_dir.as_deref(), error),
                true,
            )
        }
    }
}

fn failure_json(
    alias: &str,
    resolved: Option<&ResolvedOperator>,
    run_dir: Option<&str>,
    error: OperatorToolError,
) -> String {
    let identity = resolved.map(|resolved| OperatorRunIdentity {
        alias: alias.to_string(),
        id: resolved.spec.metadata.id.clone(),
        version: resolved.spec.metadata.version.clone(),
        source_plugin: resolved.spec.source.source_plugin.clone(),
        manifest_path: resolved
            .spec
            .source
            .manifest_path
            .to_string_lossy()
            .into_owned(),
    });
    serde_json::to_string_pretty(&json!({
        "status": "failed",
        "operator": identity,
        "runDir": run_dir,
        "error": error,
    }))
    .unwrap_or_else(|_| "{\"status\":\"failed\"}".to_string())
}

async fn execute_resolved_operator(
    ctx: &crate::domain::tools::ToolContext,
    resolved: ResolvedOperator,
    invocation: OperatorInvocation,
) -> Result<OperatorRunResult, OperatorToolError> {
    if !runtime_supported(ctx, &resolved.spec) {
        return Err(OperatorToolError::new(
            "runtime_unsupported",
            false,
            format!(
                "Operator `{}` does not support current execution surface `{}`/`{}`.",
                resolved.alias, ctx.execution_environment, ctx.sandbox_backend
            ),
        )
        .with_suggested_action(
            "Switch the session execution environment or choose a different operator.",
        ));
    }

    let run_id = format!(
        "oprun_{}_{}",
        chrono::Utc::now().format("%Y%m%d%H%M%S"),
        uuid::Uuid::new_v4().simple()
    );
    let is_ssh = ctx.execution_environment == "ssh";
    let run_dir = if is_ssh {
        crate::domain::tools::env_store::remote_path(ctx, &format!(".omiga/runs/{run_id}"))
    } else {
        ctx.project_root
            .join(".omiga")
            .join("runs")
            .join(&run_id)
            .to_string_lossy()
            .into_owned()
    };

    let mut effective_params = apply_param_defaults(&resolved.spec, invocation.params);
    let effective_resources =
        apply_resource_defaults_and_overrides(&resolved.spec, invocation.resources)?;
    apply_equal_bindings(&resolved.spec, &mut effective_params, &effective_resources)?;

    let canonical_inputs = canonicalize_inputs(ctx, &resolved.spec, invocation.inputs, is_ssh)?;
    let argv = expand_argv(
        &resolved.spec,
        &canonical_inputs,
        &effective_params,
        &effective_resources,
        &run_dir,
    )?;
    let walltime_secs =
        resource_walltime_secs(&effective_resources).unwrap_or(ctx.timeout_secs.max(60));

    if is_ssh {
        execute_in_environment(
            ctx,
            &resolved,
            &run_id,
            &run_dir,
            &argv,
            walltime_secs,
            effective_params,
            effective_resources,
        )
        .await
    } else {
        execute_local(
            ctx,
            &resolved,
            &run_id,
            &run_dir,
            &argv,
            walltime_secs,
            effective_params,
            effective_resources,
        )
        .await
    }
}

fn runtime_supported(ctx: &crate::domain::tools::ToolContext, spec: &OperatorSpec) -> bool {
    let Some(runtime) = &spec.runtime else {
        return ctx.execution_environment == "local";
    };
    let placement = match ctx.execution_environment.as_str() {
        "ssh" => "ssh",
        "sandbox" | "remote" => "local",
        _ => "local",
    };
    let container = match ctx.execution_environment.as_str() {
        "sandbox" | "remote" => ctx.sandbox_backend.as_str(),
        _ => "none",
    };
    let placements = runtime_axis_values(runtime, "placement");
    let containers = runtime_axis_values(runtime, "container");
    let flat = runtime
        .get("supported")
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect::<HashSet<_>>()
        })
        .unwrap_or_default();
    (placements.is_empty() || placements.contains(placement) || flat.contains(placement))
        && (containers.is_empty() || containers.contains(container) || flat.contains(container))
}

fn runtime_axis_values(runtime: &JsonValue, axis: &str) -> HashSet<String> {
    runtime
        .get(axis)
        .and_then(|value| {
            value
                .get("supported")
                .or_else(|| value.get("supportedValues"))
        })
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(|s| s.trim().to_ascii_lowercase()))
                .collect()
        })
        .unwrap_or_default()
}

fn apply_param_defaults(
    spec: &OperatorSpec,
    mut params: BTreeMap<String, JsonValue>,
) -> BTreeMap<String, JsonValue> {
    for (name, field) in &spec.interface.params {
        if !params.contains_key(name) {
            if let Some(default) = &field.default {
                params.insert(name.clone(), default.clone());
            }
        }
    }
    params
}

fn apply_resource_defaults_and_overrides(
    spec: &OperatorSpec,
    overrides: BTreeMap<String, JsonValue>,
) -> Result<BTreeMap<String, JsonValue>, OperatorToolError> {
    let mut out = BTreeMap::new();
    for (name, resource) in &spec.resources {
        if let Some(default) = &resource.default {
            out.insert(name.clone(), default.clone());
        }
    }
    for (name, value) in overrides {
        let resource = spec.resources.get(&name).ok_or_else(|| {
            OperatorToolError::new(
                "invalid_arguments",
                false,
                format!("Resource `{name}` is not declared by this operator."),
            )
            .with_field(format!("resources.{name}"))
        })?;
        if !resource.exposed {
            return Err(OperatorToolError::new(
                "invalid_arguments",
                false,
                format!("Resource `{name}` is not exposed for Agent override."),
            )
            .with_field(format!("resources.{name}")));
        }
        out.insert(name, value);
    }
    Ok(out)
}

fn apply_equal_bindings(
    spec: &OperatorSpec,
    params: &mut BTreeMap<String, JsonValue>,
    resources: &BTreeMap<String, JsonValue>,
) -> Result<(), OperatorToolError> {
    for binding in &spec.bindings {
        if binding.mode != "equal" {
            continue;
        }
        let param = params.get(&binding.param).cloned();
        let resource = resources.get(&binding.resource).cloned();
        match (param, resource) {
            (Some(param), Some(resource)) if param != resource => {
                return Err(OperatorToolError::new(
                    "invalid_arguments",
                    false,
                    format!(
                        "Binding requires params.{} == resources.{}.",
                        binding.param, binding.resource
                    ),
                )
                .with_field(format!("params.{}", binding.param)));
            }
            (None, Some(resource)) => {
                params.insert(binding.param.clone(), resource);
            }
            _ => {}
        }
    }
    Ok(())
}

fn canonicalize_inputs(
    ctx: &crate::domain::tools::ToolContext,
    spec: &OperatorSpec,
    inputs: BTreeMap<String, JsonValue>,
    is_ssh: bool,
) -> Result<BTreeMap<String, JsonValue>, OperatorToolError> {
    let mut out = BTreeMap::new();
    for (name, field) in &spec.interface.inputs {
        let value = inputs.get(name).cloned().or_else(|| field.default.clone());
        let Some(value) = value else {
            if field.required {
                return Err(OperatorToolError::new(
                    "input_validation_failed",
                    false,
                    format!("Required input `{name}` is missing."),
                )
                .with_field(format!("inputs.{name}")));
            }
            continue;
        };
        let canonical = if field.kind.is_path_like() {
            canonicalize_path_value(ctx, &field.kind, name, value, is_ssh)?
        } else {
            value
        };
        out.insert(name.clone(), canonical);
    }
    Ok(out)
}

fn canonicalize_path_value(
    ctx: &crate::domain::tools::ToolContext,
    kind: &OperatorFieldKind,
    name: &str,
    value: JsonValue,
    is_ssh: bool,
) -> Result<JsonValue, OperatorToolError> {
    if kind.is_array() {
        let array = value.as_array().ok_or_else(|| {
            OperatorToolError::new(
                "input_validation_failed",
                false,
                format!("Input `{name}` must be an array of paths."),
            )
            .with_field(format!("inputs.{name}"))
        })?;
        let values = array
            .iter()
            .enumerate()
            .map(|(idx, item)| {
                let path = item.as_str().ok_or_else(|| {
                    OperatorToolError::new(
                        "input_validation_failed",
                        false,
                        format!("Input `{name}[{idx}]` must be a path string."),
                    )
                    .with_field(format!("inputs.{name}[{idx}]"))
                })?;
                canonicalize_one_path(ctx, path, is_ssh).map(JsonValue::String)
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(JsonValue::Array(values))
    } else {
        let path = value.as_str().ok_or_else(|| {
            OperatorToolError::new(
                "input_validation_failed",
                false,
                format!("Input `{name}` must be a path string."),
            )
            .with_field(format!("inputs.{name}"))
        })?;
        canonicalize_one_path(ctx, path, is_ssh).map(JsonValue::String)
    }
}

fn canonicalize_one_path(
    ctx: &crate::domain::tools::ToolContext,
    raw: &str,
    is_ssh: bool,
) -> Result<String, OperatorToolError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(OperatorToolError::new(
            "input_validation_failed",
            false,
            "Input path must not be empty.",
        ));
    }
    if is_ssh {
        return Ok(crate::domain::tools::env_store::remote_path(ctx, trimmed));
    }
    let path = PathBuf::from(trimmed);
    let full = if path.is_absolute() {
        path
    } else {
        ctx.project_root.join(path)
    };
    let canonical = full.canonicalize().map_err(|err| {
        OperatorToolError::new(
            "input_validation_failed",
            false,
            format!("Input path `{trimmed}` is not accessible: {err}"),
        )
    })?;
    let project = ctx
        .project_root
        .canonicalize()
        .unwrap_or_else(|_| ctx.project_root.clone());
    if !canonical.starts_with(&project) {
        return Err(OperatorToolError::new(
            "input_validation_failed",
            false,
            format!(
                "Input path `{}` is outside the project root.",
                canonical.display()
            ),
        )
        .with_suggested_action("Move or reference files under the current project root."));
    }
    Ok(canonical.to_string_lossy().into_owned())
}

fn expand_argv(
    spec: &OperatorSpec,
    inputs: &BTreeMap<String, JsonValue>,
    params: &BTreeMap<String, JsonValue>,
    resources: &BTreeMap<String, JsonValue>,
    run_dir: &str,
) -> Result<Vec<String>, OperatorToolError> {
    let mut argv = Vec::new();
    for (index, token) in spec.execution.argv.iter().enumerate() {
        if let Some(expanded) = expand_exact_array_token(token, inputs) {
            argv.extend(expanded);
            continue;
        }
        let mut replaced = replace_token_vars(token, spec, inputs, params, resources, run_dir)?;
        if index == 0 && replaced.contains('/') && !Path::new(&replaced).is_absolute() {
            replaced = spec
                .source
                .plugin_root
                .join(replaced)
                .to_string_lossy()
                .into_owned();
        }
        argv.push(replaced);
    }
    Ok(argv)
}

fn expand_exact_array_token(
    token: &str,
    inputs: &BTreeMap<String, JsonValue>,
) -> Option<Vec<String>> {
    let key = exact_var_key(token)?;
    let name = key.strip_prefix("inputs.")?;
    inputs.get(name)?.as_array().map(|items| {
        items
            .iter()
            .filter_map(|item| item.as_str().map(str::to_string))
            .collect()
    })
}

fn exact_var_key(token: &str) -> Option<String> {
    let trimmed = token.trim();
    if trimmed.starts_with("${") && trimmed.ends_with('}') {
        return Some(trimmed[2..trimmed.len() - 1].trim().to_string());
    }
    if trimmed.starts_with("{{") && trimmed.ends_with("}}") {
        return Some(trimmed[2..trimmed.len() - 2].trim().to_string());
    }
    None
}

fn replace_token_vars(
    token: &str,
    spec: &OperatorSpec,
    inputs: &BTreeMap<String, JsonValue>,
    params: &BTreeMap<String, JsonValue>,
    resources: &BTreeMap<String, JsonValue>,
    run_dir: &str,
) -> Result<String, OperatorToolError> {
    let mut out = token.to_string();
    let outdir = format!("{run_dir}/out.tmp");
    let workdir = format!("{run_dir}/work");
    let replacements = [
        ("workdir".to_string(), workdir),
        ("outdir".to_string(), outdir),
        (
            "plugin_dir".to_string(),
            spec.source.plugin_root.to_string_lossy().into_owned(),
        ),
    ];
    for (key, value) in replacements {
        out = out.replace(&format!("${{{key}}}"), &value);
        out = out.replace(&format!("{{{{ {key} }}}}"), &value);
    }
    for (prefix, map) in [
        ("inputs", inputs),
        ("params", params),
        ("resources", resources),
    ] {
        for (name, value) in map {
            let rendered = value_to_arg_string(value);
            out = out.replace(&format!("${{{prefix}.{name}}}"), &rendered);
            out = out.replace(&format!("{{{{ {prefix}.{name} }}}}"), &rendered);
        }
    }
    Ok(out)
}

fn value_to_arg_string(value: &JsonValue) -> String {
    match value {
        JsonValue::String(s) => s.clone(),
        JsonValue::Number(n) => n.to_string(),
        JsonValue::Bool(b) => b.to_string(),
        JsonValue::Array(items) => items
            .iter()
            .map(value_to_arg_string)
            .collect::<Vec<_>>()
            .join(" "),
        JsonValue::Null => String::new(),
        other => other.to_string(),
    }
}

async fn execute_local(
    ctx: &crate::domain::tools::ToolContext,
    resolved: &ResolvedOperator,
    run_id: &str,
    run_dir: &str,
    argv: &[String],
    walltime_secs: u64,
    effective_params: BTreeMap<String, JsonValue>,
    effective_resources: BTreeMap<String, JsonValue>,
) -> Result<OperatorRunResult, OperatorToolError> {
    let run_path = PathBuf::from(run_dir);
    update_local_status(&run_path, "created", None)?;
    fs::create_dir_all(run_path.join("work")).map_err(|err| {
        OperatorToolError::new(
            "execution_infra_error",
            true,
            format!("create work dir: {err}"),
        )
        .with_run_dir(run_dir)
    })?;
    fs::create_dir_all(run_path.join("out.tmp")).map_err(|err| {
        OperatorToolError::new(
            "execution_infra_error",
            true,
            format!("create out.tmp: {err}"),
        )
        .with_run_dir(run_dir)
    })?;
    fs::create_dir_all(run_path.join("logs")).map_err(|err| {
        OperatorToolError::new(
            "execution_infra_error",
            true,
            format!("create logs dir: {err}"),
        )
        .with_run_dir(run_dir)
    })?;
    update_local_status(&run_path, "running", None)?;

    let command = command_with_log_capture(argv);
    let result = execute_env_command(ctx, run_dir, &command, walltime_secs).await?;
    let stdout_tail = read_tail(run_path.join("logs/stdout.txt"));
    let stderr_tail = read_tail(run_path.join("logs/stderr.txt"));
    if result.returncode != 0 {
        let error = OperatorToolError::new(
            "tool_exit_nonzero",
            false,
            format!("Operator process exited with code {}.", result.returncode),
        )
        .with_run_dir(run_dir)
        .with_logs(stdout_tail, stderr_tail)
        .with_suggested_action("Inspect stdout/stderr, then adjust inputs or params and retry.");
        update_local_status(&run_path, "failed", Some(&error))?;
        return Err(error);
    }

    update_local_status(&run_path, "collecting_outputs", None)?;
    let out_tmp = run_path.join("out.tmp");
    let out = run_path.join("out");
    if out.exists() {
        fs::remove_dir_all(&out).map_err(|err| {
            OperatorToolError::new("artifact_collection_failed", false, err.to_string())
                .with_run_dir(run_dir)
        })?;
    }
    fs::rename(&out_tmp, &out).map_err(|err| {
        OperatorToolError::new(
            "artifact_collection_failed",
            false,
            format!("publish out.tmp to out: {err}"),
        )
        .with_run_dir(run_dir)
    })?;

    let outputs = collect_local_outputs(&resolved.spec, &out)?;
    let provenance_path = run_path.join("provenance.json");
    let result = OperatorRunResult {
        status: "succeeded".to_string(),
        run_id: run_id.to_string(),
        operator: run_identity(resolved),
        run_dir: run_dir.to_string(),
        provenance_path: Some(provenance_path.to_string_lossy().into_owned()),
        outputs,
        effective_params,
        effective_resources,
        enforcement: enforcement_json(ctx),
        error: None,
    };
    write_json_file(&provenance_path, &result).map_err(|err| {
        OperatorToolError::new("provenance_write_failed", false, err).with_run_dir(run_dir)
    })?;
    update_local_status(&run_path, "succeeded", None)?;
    Ok(result)
}

async fn execute_in_environment(
    ctx: &crate::domain::tools::ToolContext,
    resolved: &ResolvedOperator,
    run_id: &str,
    run_dir: &str,
    argv: &[String],
    walltime_secs: u64,
    effective_params: BTreeMap<String, JsonValue>,
    effective_resources: BTreeMap<String, JsonValue>,
) -> Result<OperatorRunResult, OperatorToolError> {
    let mkdir = format!(
        "mkdir -p {}/work {}/out.tmp {}/logs",
        sh_quote(run_dir),
        sh_quote(run_dir),
        sh_quote(run_dir)
    );
    execute_env_command(ctx, "~", &mkdir, 30).await?;
    let staged_argv = stage_remote_plugin_files(ctx, &resolved.spec, run_dir, argv).await?;
    let command = command_with_log_capture(&staged_argv);
    let result = execute_env_command(ctx, run_dir, &command, walltime_secs).await?;
    let stdout_tail = remote_tail(ctx, run_dir, "logs/stdout.txt").await;
    let stderr_tail = remote_tail(ctx, run_dir, "logs/stderr.txt").await;
    if result.returncode != 0 {
        return Err(OperatorToolError::new(
            "tool_exit_nonzero",
            false,
            format!("Operator process exited with code {}.", result.returncode),
        )
        .with_run_dir(run_dir)
        .with_logs(stdout_tail, stderr_tail)
        .with_suggested_action(
            "Inspect the remote run logs, then adjust inputs or params and retry.",
        ));
    }
    let publish = "rm -rf out && mv out.tmp out";
    execute_env_command(ctx, run_dir, publish, 30).await?;
    let outputs = collect_remote_outputs(ctx, &resolved.spec, run_dir).await?;
    Ok(OperatorRunResult {
        status: "succeeded".to_string(),
        run_id: run_id.to_string(),
        operator: run_identity(resolved),
        run_dir: run_dir.to_string(),
        provenance_path: Some(format!("{run_dir}/provenance.json")),
        outputs,
        effective_params,
        effective_resources,
        enforcement: enforcement_json(ctx),
        error: None,
    })
}

async fn stage_remote_plugin_files(
    ctx: &crate::domain::tools::ToolContext,
    spec: &OperatorSpec,
    run_dir: &str,
    argv: &[String],
) -> Result<Vec<String>, OperatorToolError> {
    let plugin_root = spec
        .source
        .plugin_root
        .canonicalize()
        .unwrap_or_else(|_| spec.source.plugin_root.clone());
    let mut staged = Vec::with_capacity(argv.len());
    for arg in argv {
        let path = PathBuf::from(arg);
        let path = if path.is_absolute() {
            path.canonicalize().unwrap_or(path)
        } else {
            path
        };
        if path.is_absolute() && path.starts_with(&plugin_root) && path.is_file() {
            let rel = path.strip_prefix(&plugin_root).map_err(|err| {
                OperatorToolError::new(
                    "execution_infra_error",
                    true,
                    format!("stage plugin file {}: {err}", path.display()),
                )
                .with_run_dir(run_dir)
            })?;
            let rel = safe_relative_string(rel)?;
            let remote_path = format!("{run_dir}/plugin/{rel}");
            let remote_parent = remote_path
                .rsplit_once('/')
                .map(|(parent, _)| parent)
                .unwrap_or(run_dir);
            let bytes = fs::read(&path).map_err(|err| {
                OperatorToolError::new(
                    "execution_infra_error",
                    true,
                    format!("read plugin file {}: {err}", path.display()),
                )
                .with_run_dir(run_dir)
            })?;
            use base64::{engine::general_purpose, Engine as _};
            let encoded = general_purpose::STANDARD.encode(bytes);
            let command = format!(
                "mkdir -p {} && printf %s {} | base64 -d > {} && chmod +x {}",
                sh_quote(remote_parent),
                sh_quote(&encoded),
                sh_quote(&remote_path),
                sh_quote(&remote_path)
            );
            execute_env_command(ctx, "~", &command, 30).await?;
            staged.push(remote_path);
        } else {
            staged.push(arg.clone());
        }
    }
    Ok(staged)
}

fn safe_relative_string(path: &Path) -> Result<String, OperatorToolError> {
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            std::path::Component::Normal(part) => {
                parts.push(part.to_string_lossy().into_owned());
            }
            std::path::Component::CurDir => {}
            _ => {
                return Err(OperatorToolError::new(
                    "execution_infra_error",
                    true,
                    format!("unsafe plugin relative path {}", path.display()),
                ))
            }
        }
    }
    Ok(parts.join("/"))
}

async fn execute_env_command(
    ctx: &crate::domain::tools::ToolContext,
    cwd: &str,
    command: &str,
    timeout_secs: u64,
) -> Result<crate::execution::ExecResult, OperatorToolError> {
    let store = ctx.env_store.clone().unwrap_or_default();
    let env = store
        .get_or_create(ctx, timeout_secs * 1000)
        .await
        .map_err(|err| {
            OperatorToolError::new("environment_unavailable", true, err.to_string())
                .with_suggested_action("Check the selected execution environment and retry.")
        })?;
    let exec_opts = crate::execution::ExecOptions {
        timeout: Some(timeout_secs * 1000),
        cwd: Some(cwd.to_string()),
        stdin_data: None,
    };
    let mut guard = env.lock().await;
    guard.execute(command, exec_opts).await.map_err(|err| {
        OperatorToolError::new("execution_infra_error", true, err.to_string())
            .with_suggested_action("Retry if the execution backend was temporarily unavailable.")
    })
}

fn command_with_log_capture(argv: &[String]) -> String {
    let rendered = argv
        .iter()
        .map(|arg| sh_quote(arg))
        .collect::<Vec<_>>()
        .join(" ");
    format!(
        "set +e\n{rendered} > logs/stdout.txt 2> logs/stderr.txt\ncode=$?\nprintf '\\n__OMIGA_OPERATOR_EXIT_CODE=%s\\n' \"$code\"\nexit \"$code\""
    )
}

fn sh_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn collect_local_outputs(
    spec: &OperatorSpec,
    out_dir: &Path,
) -> Result<BTreeMap<String, Vec<ArtifactRef>>, OperatorToolError> {
    let mut outputs = BTreeMap::new();
    for (name, field) in &spec.interface.outputs {
        let Some(pattern) = field.glob.as_deref() else {
            outputs.insert(name.clone(), Vec::new());
            continue;
        };
        let search = out_dir.join(pattern).to_string_lossy().into_owned();
        let mut artifacts = Vec::new();
        for entry in glob::glob(&search).map_err(|err| {
            OperatorToolError::new("artifact_collection_failed", false, err.to_string())
        })? {
            let path = entry.map_err(|err| {
                OperatorToolError::new("artifact_collection_failed", false, err.to_string())
            })?;
            if path.is_file() {
                let size = path.metadata().ok().map(|m| m.len());
                artifacts.push(ArtifactRef {
                    location: "local".to_string(),
                    server: None,
                    path: path.to_string_lossy().into_owned(),
                    size,
                    fingerprint: size.map(|s| json!({"mode": "stat", "size": s})),
                });
            }
        }
        if field.required && artifacts.is_empty() {
            return Err(OperatorToolError::new(
                "output_validation_failed",
                false,
                format!("Required output `{name}` matched no files with glob `{pattern}`."),
            )
            .with_field(format!("outputs.{name}")));
        }
        outputs.insert(name.clone(), artifacts);
    }
    Ok(outputs)
}

async fn collect_remote_outputs(
    ctx: &crate::domain::tools::ToolContext,
    spec: &OperatorSpec,
    run_dir: &str,
) -> Result<BTreeMap<String, Vec<ArtifactRef>>, OperatorToolError> {
    let mut outputs = BTreeMap::new();
    for (name, field) in &spec.interface.outputs {
        let pattern = field.glob.as_deref().unwrap_or("*");
        let command = format!(
            "find out -type f -name {} -print",
            sh_quote(pattern.rsplit('/').next().unwrap_or(pattern))
        );
        let result = execute_env_command(ctx, run_dir, &command, 30).await?;
        let mut artifacts = Vec::new();
        for line in result
            .output
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
        {
            let path = if line.starts_with('/') {
                line.to_string()
            } else {
                format!("{run_dir}/{line}")
            };
            artifacts.push(ArtifactRef {
                location: "ssh".to_string(),
                server: ctx.ssh_server.clone(),
                path,
                size: None,
                fingerprint: None,
            });
        }
        if field.required && artifacts.is_empty() {
            return Err(OperatorToolError::new(
                "output_validation_failed",
                false,
                format!("Required output `{name}` matched no remote files with glob `{pattern}`."),
            )
            .with_field(format!("outputs.{name}"))
            .with_run_dir(run_dir));
        }
        outputs.insert(name.clone(), artifacts);
    }
    Ok(outputs)
}

async fn remote_tail(
    ctx: &crate::domain::tools::ToolContext,
    run_dir: &str,
    rel: &str,
) -> Option<String> {
    let command = format!("tail -c 4000 {}", sh_quote(rel));
    execute_env_command(ctx, run_dir, &command, 15)
        .await
        .ok()
        .map(|result| result.output)
}

fn update_local_status(
    run_path: &Path,
    status: &str,
    error: Option<&OperatorToolError>,
) -> Result<(), OperatorToolError> {
    fs::create_dir_all(run_path).map_err(|err| {
        OperatorToolError::new("execution_infra_error", true, err.to_string())
            .with_run_dir(run_path.to_string_lossy())
    })?;
    let value = json!({
        "status": status,
        "updatedAt": chrono::Utc::now().to_rfc3339(),
        "error": error,
    });
    write_json_file(&run_path.join("status.json"), &value).map_err(|err| {
        OperatorToolError::new("provenance_write_failed", false, err)
            .with_run_dir(run_path.to_string_lossy())
    })
}

fn write_json_file(path: &Path, value: &impl Serialize) -> Result<(), String> {
    let raw = serde_json::to_string_pretty(value).map_err(|err| err.to_string())?;
    fs::write(path, format!("{raw}\n")).map_err(|err| err.to_string())
}

fn read_tail(path: impl AsRef<Path>) -> Option<String> {
    let raw = fs::read_to_string(path).ok()?;
    let chars = raw.chars().collect::<Vec<_>>();
    let start = chars.len().saturating_sub(4000);
    Some(chars[start..].iter().collect())
}

fn run_identity(resolved: &ResolvedOperator) -> OperatorRunIdentity {
    OperatorRunIdentity {
        alias: resolved.alias.clone(),
        id: resolved.spec.metadata.id.clone(),
        version: resolved.spec.metadata.version.clone(),
        source_plugin: resolved.spec.source.source_plugin.clone(),
        manifest_path: resolved
            .spec
            .source
            .manifest_path
            .to_string_lossy()
            .into_owned(),
    }
}

fn enforcement_json(ctx: &crate::domain::tools::ToolContext) -> JsonValue {
    match ctx.execution_environment.as_str() {
        "sandbox" | "remote" => json!({
            "filesystem": "container_best_effort",
            "network": "not_yet_enforced_by_operator_mvp"
        }),
        "ssh" => json!({
            "filesystem": "trusted_remote_best_effort",
            "network": "remote_user_environment"
        }),
        _ => json!({
            "filesystem": "local_best_effort",
            "network": "local_user_environment"
        }),
    }
}

fn resource_walltime_secs(resources: &BTreeMap<String, JsonValue>) -> Option<u64> {
    let value = resources.get("walltime")?;
    match value {
        JsonValue::Number(n) => n.as_u64(),
        JsonValue::String(s) => parse_duration_secs(s),
        _ => None,
    }
}

fn parse_duration_secs(raw: &str) -> Option<u64> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let split_at = trimmed
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(trimmed.len());
    let (num, unit) = trimmed.split_at(split_at);
    let value = num.parse::<u64>().ok()?;
    match unit.trim().to_ascii_lowercase().as_str() {
        "" | "s" | "sec" | "secs" | "second" | "seconds" => Some(value),
        "m" | "min" | "mins" | "minute" | "minutes" => Some(value * 60),
        "h" | "hr" | "hour" | "hours" => Some(value * 3600),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn parses_manifest_and_generates_tool_schema() {
        let tmp = TempDir::new().unwrap();
        let manifest = tmp.path().join("operator.yaml");
        fs::write(
            &manifest,
            r#"
apiVersion: omiga.ai/operator/v1alpha1
kind: Operator
metadata:
  id: fastqc
  version: 0.12.1
  description: FASTQ quality control
interface:
  inputs:
    reads:
      kind: file_array
      required: true
      formats: [fastq.gz]
  params:
    threads:
      kind: integer
      default: 4
  outputs:
    reports:
      kind: file_array
      glob: "*.html"
      required: true
execution:
  argv: ["fastqc", "--threads", "${params.threads}", "${inputs.reads}"]
resources:
  cpu:
    default: 4
    exposed: true
bindings:
  - param: threads
    resource: cpu
"#,
        )
        .unwrap();
        let spec = load_operator_manifest(&manifest, "p@m", tmp.path()).unwrap();
        assert_eq!(spec.metadata.id, "fastqc");
        let schema = operator_parameters_schema(&spec);
        assert_eq!(schema["required"][0], "inputs");
        assert!(schema["properties"]["inputs"]["properties"]["reads"]["items"].is_object());
        assert_eq!(
            schema["properties"]["resources"]["properties"]["cpu"]["type"],
            "integer"
        );
    }

    #[test]
    fn registry_requires_disambiguation_for_conflicts() {
        let tmp = TempDir::new().unwrap();
        let source = |plugin: &str| OperatorSpec {
            api_version: OPERATOR_API_VERSION_V1ALPHA1.to_string(),
            kind: OPERATOR_KIND.to_string(),
            metadata: OperatorMetadata {
                id: "fastqc".to_string(),
                version: "1".to_string(),
                name: None,
                description: None,
                tags: Vec::new(),
            },
            interface: OperatorInterfaceSpec::default(),
            execution: OperatorExecutionSpec {
                argv: vec!["true".to_string()],
            },
            runtime: None,
            resources: BTreeMap::new(),
            bindings: Vec::new(),
            permissions: None,
            source: OperatorSource {
                source_plugin: plugin.to_string(),
                plugin_root: tmp.path().to_path_buf(),
                manifest_path: tmp.path().join("operator.yaml"),
            },
        };
        let registry = OperatorRegistryFile {
            enabled: BTreeMap::from([(
                "fastqc".to_string(),
                OperatorRegistryEntry::Version("1".to_string()),
            )]),
        };
        assert!(
            resolve_enabled_operators_from(vec![source("a"), source("b")], registry).is_empty()
        );
    }

    #[test]
    fn registry_update_pins_resolved_source_and_version() {
        let tmp = TempDir::new().unwrap();
        let spec = OperatorSpec {
            api_version: OPERATOR_API_VERSION_V1ALPHA1.to_string(),
            kind: OPERATOR_KIND.to_string(),
            metadata: OperatorMetadata {
                id: "fastqc".to_string(),
                version: "0.12.1".to_string(),
                name: None,
                description: None,
                tags: Vec::new(),
            },
            interface: OperatorInterfaceSpec::default(),
            execution: OperatorExecutionSpec {
                argv: vec!["true".to_string()],
            },
            runtime: None,
            resources: BTreeMap::new(),
            bindings: Vec::new(),
            permissions: None,
            source: OperatorSource {
                source_plugin: "bio@builtin".to_string(),
                plugin_root: tmp.path().to_path_buf(),
                manifest_path: tmp.path().join("operator.yaml"),
            },
        };
        let mut registry = OperatorRegistryFile::default();
        apply_operator_registry_update(
            &mut registry,
            vec![spec],
            OperatorRegistryUpdate {
                alias: "fastqc".to_string(),
                operator_id: None,
                source_plugin: None,
                version: None,
                enabled: true,
            },
        )
        .unwrap();
        match registry.enabled.get("fastqc").unwrap() {
            OperatorRegistryEntry::Full {
                operator_id,
                source_plugin,
                version,
                enabled,
            } => {
                assert_eq!(operator_id.as_deref(), Some("fastqc"));
                assert_eq!(source_plugin.as_deref(), Some("bio@builtin"));
                assert_eq!(version.as_deref(), Some("0.12.1"));
                assert_eq!(*enabled, Some(true));
            }
            other => panic!("expected full registry entry, got {other:?}"),
        }
    }

    #[test]
    fn expands_array_inputs_as_multiple_argv_tokens() {
        let tmp = TempDir::new().unwrap();
        let spec = OperatorSpec {
            api_version: OPERATOR_API_VERSION_V1ALPHA1.to_string(),
            kind: OPERATOR_KIND.to_string(),
            metadata: OperatorMetadata {
                id: "x".to_string(),
                version: "1".to_string(),
                name: None,
                description: None,
                tags: Vec::new(),
            },
            interface: OperatorInterfaceSpec::default(),
            execution: OperatorExecutionSpec {
                argv: vec!["cat".to_string(), "${inputs.files}".to_string()],
            },
            runtime: None,
            resources: BTreeMap::new(),
            bindings: Vec::new(),
            permissions: None,
            source: OperatorSource {
                source_plugin: "p".to_string(),
                plugin_root: tmp.path().to_path_buf(),
                manifest_path: tmp.path().join("operator.yaml"),
            },
        };
        let argv = expand_argv(
            &spec,
            &BTreeMap::from([("files".to_string(), json!(["a.txt", "b.txt"]))]),
            &BTreeMap::new(),
            &BTreeMap::new(),
            "/run",
        )
        .unwrap();
        assert_eq!(argv, vec!["cat", "a.txt", "b.txt"]);
    }

    #[tokio::test]
    async fn executes_local_operator_and_collects_outputs() {
        let tmp = TempDir::new().unwrap();
        let input = tmp.path().join("input.txt");
        fs::write(&input, "hello operator\n").unwrap();
        let spec = OperatorSpec {
            api_version: OPERATOR_API_VERSION_V1ALPHA1.to_string(),
            kind: OPERATOR_KIND.to_string(),
            metadata: OperatorMetadata {
                id: "copy_report".to_string(),
                version: "1".to_string(),
                name: None,
                description: Some("copy input to report".to_string()),
                tags: Vec::new(),
            },
            interface: OperatorInterfaceSpec {
                inputs: BTreeMap::from([(
                    "input".to_string(),
                    OperatorFieldSpec {
                        kind: OperatorFieldKind::File,
                        required: true,
                        ..OperatorFieldSpec::default()
                    },
                )]),
                outputs: BTreeMap::from([(
                    "report".to_string(),
                    OperatorFieldSpec {
                        kind: OperatorFieldKind::FileArray,
                        required: true,
                        glob: Some("copy.txt".to_string()),
                        ..OperatorFieldSpec::default()
                    },
                )]),
                ..OperatorInterfaceSpec::default()
            },
            execution: OperatorExecutionSpec {
                argv: vec![
                    "/bin/sh".to_string(),
                    "-c".to_string(),
                    "cp ${inputs.input} ${outdir}/copy.txt".to_string(),
                ],
            },
            runtime: None,
            resources: BTreeMap::new(),
            bindings: Vec::new(),
            permissions: None,
            source: OperatorSource {
                source_plugin: "test@local".to_string(),
                plugin_root: tmp.path().to_path_buf(),
                manifest_path: tmp.path().join("operator.yaml"),
            },
        };
        let ctx = crate::domain::tools::ToolContext::new(tmp.path());
        let result = execute_resolved_operator(
            &ctx,
            ResolvedOperator {
                alias: "copy_report".to_string(),
                spec,
            },
            OperatorInvocation {
                inputs: BTreeMap::from([(
                    "input".to_string(),
                    JsonValue::String("input.txt".to_string()),
                )]),
                params: BTreeMap::new(),
                resources: BTreeMap::new(),
            },
        )
        .await
        .unwrap();
        assert_eq!(result.status, "succeeded");
        assert_eq!(result.outputs["report"].len(), 1);
        assert!(Path::new(&result.outputs["report"][0].path).is_file());
    }
}
