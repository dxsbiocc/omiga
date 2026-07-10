use super::{
    apply_equal_bindings, apply_param_defaults, apply_resource_defaults_and_overrides,
    operator_spec_for_operation, reject_unknown_fields, validate_field_values, OperatorToolError,
    OPERATOR_PREFLIGHT_MAX_OPTIONS, OPERATOR_PREFLIGHT_MAX_QUESTIONS,
};
use crate::domain::operators::OperatorInvocation;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

pub const OPERATOR_API_VERSION_V1ALPHA1: &str = "omiga.ai/operator/v1alpha1";
pub const OPERATOR_API_VERSION_V1ALPHA2: &str = "omiga.ai/operator/v1alpha2";
pub const OPERATOR_KIND: &str = "Operator";

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

    pub(crate) fn is_array(&self) -> bool {
        matches!(self, Self::FileArray | Self::DirectoryArray)
    }

    pub(crate) fn is_path_like(&self) -> bool {
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperatorOperationSpec {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    /// Manifest-declared operation taxonomy for progressive disclosure.
    ///
    /// This is intentionally generic metadata: plugins decide the vocabulary
    /// (for example `ngs/sequence-processing`), while Omiga only parses and
    /// surfaces it for routing/catalog UIs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stage: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub interface: OperatorInterfaceSpec,
    #[serde(default)]
    pub smoke_tests: Vec<OperatorSmokeTestSpec>,
    pub execution: OperatorExecutionSpec,
    #[serde(default)]
    pub preflight: Option<OperatorPreflightSpec>,
    #[serde(default)]
    pub runtime: Option<JsonValue>,
    #[serde(default)]
    pub cache: Option<JsonValue>,
    #[serde(default)]
    pub resources: BTreeMap<String, OperatorResourceSpec>,
    #[serde(default)]
    pub bindings: Vec<OperatorBindingSpec>,
    #[serde(default)]
    pub permissions: Option<JsonValue>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperatorPreflightSpec {
    #[serde(default)]
    pub questions: Vec<OperatorPreflightQuestionSpec>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperatorPreflightQuestionSpec {
    #[serde(default)]
    pub id: Option<String>,
    pub param: String,
    pub question: String,
    pub header: String,
    #[serde(default)]
    pub multi_select: bool,
    #[serde(default)]
    pub ask_when: OperatorPreflightAskWhen,
    pub options: Vec<OperatorPreflightOptionSpec>,
    /// When set, the question is only shown if the referenced param currently equals the given value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub show_when: Option<OperatorPreflightShowWhen>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperatorPreflightShowWhen {
    pub param: String,
    pub value: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperatorPreflightAskWhen {
    #[serde(default)]
    pub always: bool,
    #[serde(default)]
    pub missing: bool,
    #[serde(default)]
    pub empty: bool,
    #[serde(default)]
    pub values: Vec<JsonValue>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperatorPreflightOptionSpec {
    pub label: String,
    pub description: String,
    pub value: JsonValue,
    #[serde(default)]
    pub preview: Option<String>,
    #[serde(default)]
    pub custom: bool,
    #[serde(default, rename = "customPlaceholder")]
    pub custom_placeholder: Option<String>,
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
    #[serde(default)]
    pub operations: BTreeMap<String, OperatorOperationSpec>,
    #[serde(default)]
    pub smoke_tests: Vec<OperatorSmokeTestSpec>,
    pub execution: OperatorExecutionSpec,
    #[serde(default)]
    pub preflight: Option<OperatorPreflightSpec>,
    #[serde(default)]
    pub runtime: Option<JsonValue>,
    #[serde(default)]
    pub cache: Option<JsonValue>,
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
    #[serde(default)]
    pub tags: Vec<String>,
    pub source_plugin: String,
    pub manifest_path: String,
    pub interface: OperatorInterfaceSpec,
    #[serde(default)]
    pub operations: Vec<OperatorOperationSummary>,
    pub execution: OperatorExecutionSpec,
    #[serde(default)]
    pub preflight: Option<OperatorPreflightSpec>,
    #[serde(default)]
    pub runtime: Option<JsonValue>,
    #[serde(default)]
    pub resources: BTreeMap<String, OperatorResourceSpec>,
    #[serde(default)]
    pub smoke_tests: Vec<OperatorSmokeTestSpec>,
    #[serde(default)]
    pub environment_ref: Option<String>,
    pub enabled_aliases: Vec<String>,
    pub exposed: bool,
    pub unavailable_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperatorOperationGroupSummary {
    pub key: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stage: Option<String>,
    #[serde(default)]
    pub operations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperatorOperationSummary {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stage: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub interface: OperatorInterfaceSpec,
    #[serde(default)]
    pub runtime: Option<JsonValue>,
    #[serde(default)]
    pub resources: BTreeMap<String, OperatorResourceSpec>,
    #[serde(default)]
    pub exposed: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperatorSmokeTestSpec {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub arguments: OperatorInvocation,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OperatorManifestDiagnostic {
    pub source_plugin: String,
    pub manifest_path: String,
    pub severity: String,
    pub message: String,
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
    pub(crate) fn enabled(&self) -> bool {
        match self {
            Self::Version(_) => true,
            Self::Full { enabled, .. } => enabled.unwrap_or(true),
        }
    }

    pub(crate) fn operator_id<'a>(&'a self, alias: &'a str) -> &'a str {
        match self {
            Self::Version(_) => alias,
            Self::Full { operator_id, .. } => operator_id.as_deref().unwrap_or(alias),
        }
    }

    pub(crate) fn version(&self) -> Option<&str> {
        match self {
            Self::Version(version) => Some(version.as_str()),
            Self::Full { version, .. } => version.as_deref(),
        }
    }

    pub(crate) fn source_plugin(&self) -> Option<&str> {
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
    #[serde(default, alias = "tests", alias = "smoke")]
    smoke_tests: Vec<RawOperatorSmokeTestSpec>,
    #[serde(default)]
    execution: Option<RawOperatorExecution>,
    #[serde(default)]
    operations: BTreeMap<String, RawOperatorOperationSpec>,
    #[serde(default)]
    preflight: Option<OperatorPreflightSpec>,
    #[serde(default)]
    runtime: Option<JsonValue>,
    #[serde(default)]
    cache: Option<JsonValue>,
    #[serde(default)]
    resources: BTreeMap<String, RawResourceSpec>,
    #[serde(default)]
    bindings: Vec<OperatorBindingSpec>,
    #[serde(default)]
    permissions: Option<JsonValue>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawOperatorOperationSpec {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    group: Option<String>,
    #[serde(default)]
    stage: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    interface: RawOperatorInterface,
    #[serde(default, alias = "tests", alias = "smoke")]
    smoke_tests: Vec<RawOperatorSmokeTestSpec>,
    #[serde(default)]
    execution: Option<RawOperatorExecution>,
    #[serde(default)]
    preflight: Option<OperatorPreflightSpec>,
    #[serde(default)]
    runtime: Option<JsonValue>,
    #[serde(default)]
    cache: Option<JsonValue>,
    #[serde(default)]
    resources: BTreeMap<String, RawResourceSpec>,
    #[serde(default)]
    bindings: Vec<OperatorBindingSpec>,
    #[serde(default)]
    permissions: Option<JsonValue>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawOperatorSmokeTestSpec {
    id: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    arguments: Option<OperatorInvocation>,
    #[serde(default)]
    inputs: BTreeMap<String, JsonValue>,
    #[serde(default)]
    params: BTreeMap<String, JsonValue>,
    #[serde(default)]
    resources: BTreeMap<String, JsonValue>,
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

impl TryFrom<RawOperatorSmokeTestSpec> for OperatorSmokeTestSpec {
    type Error = String;

    fn try_from(raw: RawOperatorSmokeTestSpec) -> Result<Self, Self::Error> {
        let id = raw.id.trim();
        if id.is_empty() {
            return Err("operator smokeTests.id must not be empty".to_string());
        }
        if !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
        {
            return Err(format!(
                "operator smoke test id `{id}` may contain only letters, numbers, `_`, or `-`"
            ));
        }
        let mut arguments = raw.arguments.unwrap_or_default();
        arguments.inputs.extend(raw.inputs);
        arguments.params.extend(raw.params);
        arguments.resources.extend(raw.resources);
        Ok(Self {
            id: id.to_string(),
            name: raw.name,
            description: raw.description,
            arguments,
        })
    }
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
pub fn load_operator_manifest(
    manifest_path: &Path,
    source_plugin: impl Into<String>,
    plugin_root: impl Into<PathBuf>,
) -> Result<OperatorSpec, String> {
    let raw = fs::read_to_string(manifest_path)
        .map_err(|err| format!("read operator manifest {}: {err}", manifest_path.display()))?;
    let parsed: RawOperatorManifest = serde_yaml::from_str(&raw)
        .map_err(|err| format!("parse operator manifest {}: {err}", manifest_path.display()))?;
    if parsed.api_version != OPERATOR_API_VERSION_V1ALPHA1
        && parsed.api_version != OPERATOR_API_VERSION_V1ALPHA2
    {
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
    let source = OperatorSource {
        source_plugin: source_plugin.into(),
        plugin_root: plugin_root.into(),
        manifest_path: manifest_path.to_path_buf(),
    };
    let metadata = OperatorMetadata {
        id: parsed.metadata.id,
        version: parsed.metadata.version,
        name: parsed.metadata.name,
        description: parsed.metadata.description,
        tags: parsed.metadata.tags,
    };
    let top_interface = operator_interface_from_raw(parsed.interface);
    let top_resources = resources_from_raw(parsed.resources);
    let top_execution = parsed.execution.and_then(raw_execution_argv);
    let top_smoke_tests = parsed
        .smoke_tests
        .into_iter()
        .map(OperatorSmokeTestSpec::try_from)
        .collect::<Result<Vec<_>, _>>()?;
    let top_bindings = parsed.bindings;
    let top_preflight = parsed.preflight;
    let top_runtime = parsed.runtime;
    let top_cache = parsed.cache;
    let top_permissions = parsed.permissions;
    let operations = normalize_operator_operations(
        parsed.operations,
        &metadata,
        &top_interface,
        &top_smoke_tests,
        top_execution.clone(),
        top_preflight.clone(),
        top_runtime.clone(),
        top_cache.clone(),
        &top_resources,
        &top_bindings,
        top_permissions.clone(),
    )?;
    let interface = aggregate_operation_interfaces(&operations);
    let resources = aggregate_operation_resources(&operations);
    let execution = representative_operation(&operations)
        .map(|operation| operation.execution.clone())
        .or_else(|| {
            top_execution
                .clone()
                .map(|argv| OperatorExecutionSpec { argv })
        })
        .ok_or_else(|| "operator execution.argv is required".to_string())?;
    if execution.argv.is_empty() {
        return Err("operator execution.argv must not be empty".to_string());
    }
    let spec = OperatorSpec {
        api_version: parsed.api_version,
        kind: parsed.kind,
        metadata,
        interface,
        operations,
        smoke_tests: top_smoke_tests,
        execution,
        preflight: top_preflight,
        runtime: top_runtime,
        cache: top_cache,
        resources,
        bindings: top_bindings,
        permissions: top_permissions,
        source,
    };
    validate_operator_preflight(&spec)?;
    validate_operator_smoke_tests(&spec)?;
    validate_operator_operations(&spec)?;
    Ok(spec)
}

fn raw_execution_argv(execution: RawOperatorExecution) -> Option<Vec<String>> {
    execution
        .argv
        .or_else(|| execution.command.and_then(|command| command.argv))
}

fn operator_interface_from_raw(raw: RawOperatorInterface) -> OperatorInterfaceSpec {
    OperatorInterfaceSpec {
        inputs: raw
            .inputs
            .into_iter()
            .map(|(key, value)| (key, value.into()))
            .collect(),
        params: raw
            .params
            .into_iter()
            .map(|(key, value)| (key, value.into()))
            .collect(),
        outputs: raw
            .outputs
            .into_iter()
            .map(|(key, value)| (key, value.into()))
            .collect(),
    }
}

fn resources_from_raw(
    raw: BTreeMap<String, RawResourceSpec>,
) -> BTreeMap<String, OperatorResourceSpec> {
    raw.into_iter()
        .map(|(key, value)| (key, value.into()))
        .collect()
}

fn merge_operator_interfaces(
    base: &OperatorInterfaceSpec,
    overlay: OperatorInterfaceSpec,
) -> OperatorInterfaceSpec {
    let mut merged = base.clone();
    merged.inputs.extend(overlay.inputs);
    merged.params.extend(overlay.params);
    merged.outputs.extend(overlay.outputs);
    merged
}

fn normalize_operator_operations(
    raw_operations: BTreeMap<String, RawOperatorOperationSpec>,
    metadata: &OperatorMetadata,
    top_interface: &OperatorInterfaceSpec,
    top_smoke_tests: &[OperatorSmokeTestSpec],
    top_execution: Option<Vec<String>>,
    top_preflight: Option<OperatorPreflightSpec>,
    top_runtime: Option<JsonValue>,
    top_cache: Option<JsonValue>,
    top_resources: &BTreeMap<String, OperatorResourceSpec>,
    top_bindings: &[OperatorBindingSpec],
    top_permissions: Option<JsonValue>,
) -> Result<BTreeMap<String, OperatorOperationSpec>, String> {
    if raw_operations.is_empty() {
        let argv = top_execution
            .clone()
            .ok_or_else(|| "operator execution.argv is required".to_string())?;
        if argv.is_empty() {
            return Err("operator execution.argv must not be empty".to_string());
        }
        return Ok(BTreeMap::from([(
            "run".to_string(),
            OperatorOperationSpec {
                name: metadata.name.clone(),
                description: metadata.description.clone(),
                category: None,
                group: None,
                stage: None,
                tags: metadata.tags.clone(),
                interface: top_interface.clone(),
                smoke_tests: top_smoke_tests.to_vec(),
                execution: OperatorExecutionSpec { argv },
                preflight: top_preflight,
                runtime: top_runtime,
                cache: top_cache,
                resources: top_resources.clone(),
                bindings: top_bindings.to_vec(),
                permissions: top_permissions,
            },
        )]));
    }

    let mut operations = BTreeMap::new();
    for (id, raw) in raw_operations {
        validate_operator_operation_id(&id)?;
        let argv = raw
            .execution
            .and_then(raw_execution_argv)
            .or_else(|| top_execution.clone())
            .ok_or_else(|| format!("operator operations.{id}.execution.argv is required"))?;
        if argv.is_empty() {
            return Err(format!(
                "operator operations.{id}.execution.argv must not be empty"
            ));
        }
        let operation_interface =
            merge_operator_interfaces(top_interface, operator_interface_from_raw(raw.interface));
        let mut operation_resources = top_resources.clone();
        operation_resources.extend(resources_from_raw(raw.resources));
        let smoke_tests = if raw.smoke_tests.is_empty() {
            top_smoke_tests.to_vec()
        } else {
            raw.smoke_tests
                .into_iter()
                .map(OperatorSmokeTestSpec::try_from)
                .collect::<Result<Vec<_>, _>>()?
        };
        operations.insert(
            id,
            OperatorOperationSpec {
                name: normalize_optional_string(raw.name),
                description: normalize_optional_string(raw.description),
                category: normalize_optional_string(raw.category),
                group: normalize_optional_string(raw.group),
                stage: normalize_optional_string(raw.stage),
                tags: raw.tags,
                interface: operation_interface,
                smoke_tests,
                execution: OperatorExecutionSpec { argv },
                preflight: raw.preflight.or_else(|| top_preflight.clone()),
                runtime: raw.runtime.or_else(|| top_runtime.clone()),
                cache: raw.cache.or_else(|| top_cache.clone()),
                resources: operation_resources,
                bindings: if raw.bindings.is_empty() {
                    top_bindings.to_vec()
                } else {
                    raw.bindings
                },
                permissions: raw.permissions.or_else(|| top_permissions.clone()),
            },
        );
    }
    Ok(operations)
}

pub(crate) fn validate_operator_operation_id(id: &str) -> Result<(), String> {
    let id = id.trim();
    if id.is_empty() {
        return Err("operator operation id must not be empty".to_string());
    }
    if !id
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
    {
        return Err(format!(
            "operator operation id `{id}` may contain only letters, numbers, `_`, or `-`"
        ));
    }
    Ok(())
}

pub(crate) fn aggregate_operation_interfaces(
    operations: &BTreeMap<String, OperatorOperationSpec>,
) -> OperatorInterfaceSpec {
    let mut aggregate = OperatorInterfaceSpec::default();
    for operation in operations.values() {
        aggregate.inputs.extend(operation.interface.inputs.clone());
        aggregate.params.extend(operation.interface.params.clone());
        aggregate
            .outputs
            .extend(operation.interface.outputs.clone());
    }
    aggregate
}

pub(crate) fn aggregate_operation_resources(
    operations: &BTreeMap<String, OperatorOperationSpec>,
) -> BTreeMap<String, OperatorResourceSpec> {
    let mut aggregate = BTreeMap::new();
    for operation in operations.values() {
        aggregate.extend(operation.resources.clone());
    }
    aggregate
}

pub(crate) fn representative_operation(
    operations: &BTreeMap<String, OperatorOperationSpec>,
) -> Option<&OperatorOperationSpec> {
    operations.get("run").or_else(|| operations.values().next())
}

pub(crate) fn validate_operator_preflight(spec: &OperatorSpec) -> Result<(), String> {
    let Some(preflight) = &spec.preflight else {
        return Ok(());
    };
    if preflight.questions.len() > OPERATOR_PREFLIGHT_MAX_QUESTIONS {
        return Err(format!(
            "operator preflight.questions supports at most {OPERATOR_PREFLIGHT_MAX_QUESTIONS} questions"
        ));
    }
    let mut seen_questions = HashSet::new();
    for question in &preflight.questions {
        if question.param.trim().is_empty() {
            return Err("operator preflight question param must not be empty".to_string());
        }
        if !spec.interface.params.contains_key(question.param.trim()) {
            return Err(format!(
                "operator preflight question references unknown param `{}`",
                question.param
            ));
        }
        if question.question.trim().is_empty() {
            return Err("operator preflight question text must not be empty".to_string());
        }
        if !seen_questions.insert(question.question.trim()) {
            return Err(format!(
                "operator preflight question `{}` is declared more than once",
                question.question
            ));
        }
        if question.header.trim().is_empty() {
            return Err("operator preflight question header must not be empty".to_string());
        }
        if question.options.len() < 2 || question.options.len() > OPERATOR_PREFLIGHT_MAX_OPTIONS {
            return Err(format!(
                "operator preflight question `{}` must declare 2-{OPERATOR_PREFLIGHT_MAX_OPTIONS} options",
                question.question
            ));
        }
        let mut labels = HashSet::new();
        for option in &question.options {
            if option.label.trim().is_empty() || option.description.trim().is_empty() {
                return Err(format!(
                    "operator preflight question `{}` has an option with empty label/description",
                    question.question
                ));
            }
            if !labels.insert(option.label.trim()) {
                return Err(format!(
                    "operator preflight question `{}` repeats option label `{}`",
                    question.question, option.label
                ));
            }
            if question.multi_select && option.custom {
                return Err(format!(
                    "operator preflight question `{}` cannot use custom options with multiSelect",
                    question.question
                ));
            }
            if !option.custom && option.custom_placeholder.is_some() {
                return Err(format!(
                    "operator preflight question `{}` has customPlaceholder on a non-custom option",
                    question.question
                ));
            }
        }
    }
    Ok(())
}

fn validate_operator_smoke_tests(spec: &OperatorSpec) -> Result<(), String> {
    let mut seen = HashSet::new();
    for smoke_test in &spec.smoke_tests {
        if !seen.insert(smoke_test.id.as_str()) {
            return Err(format!(
                "operator smoke test id `{}` is declared more than once",
                smoke_test.id
            ));
        }
        let args = &smoke_test.arguments;
        reject_unknown_fields(
            &format!("smokeTests.{}.inputs", smoke_test.id),
            args.inputs.keys(),
            &spec.interface.inputs,
        )
        .map_err(|error| smoke_validation_error(&smoke_test.id, error))?;
        reject_unknown_fields(
            &format!("smokeTests.{}.params", smoke_test.id),
            args.params.keys(),
            &spec.interface.params,
        )
        .map_err(|error| smoke_validation_error(&smoke_test.id, error))?;
        let mut effective_params = apply_param_defaults(spec, args.params.clone());
        validate_field_values(
            &format!("smokeTests.{}.inputs", smoke_test.id),
            &spec.interface.inputs,
            &args.inputs,
        )
        .map_err(|error| smoke_validation_error(&smoke_test.id, error))?;
        validate_field_values(
            &format!("smokeTests.{}.params", smoke_test.id),
            &spec.interface.params,
            &effective_params,
        )
        .map_err(|error| smoke_validation_error(&smoke_test.id, error))?;
        let effective_resources =
            apply_resource_defaults_and_overrides(spec, args.resources.clone())
                .map_err(|error| smoke_validation_error(&smoke_test.id, error))?;
        apply_equal_bindings(spec, &mut effective_params, &effective_resources)
            .map_err(|error| smoke_validation_error(&smoke_test.id, error))?;
    }
    Ok(())
}

pub(crate) fn validate_operator_operations(spec: &OperatorSpec) -> Result<(), String> {
    for operation_id in spec.operations.keys() {
        validate_operator_operation_id(operation_id)?;
        let operation_spec = operator_spec_for_operation(spec, operation_id)
            .map_err(|error| format!("operator operation `{operation_id}`: {}", error.message))?;
        validate_operator_preflight(&operation_spec)
            .map_err(|error| format!("operator operation `{operation_id}`: {error}"))?;
        validate_operator_smoke_tests(&operation_spec)
            .map_err(|error| format!("operator operation `{operation_id}`: {error}"))?;
    }
    Ok(())
}

fn smoke_validation_error(test_id: &str, error: OperatorToolError) -> String {
    match error.field {
        Some(field) => format!(
            "operator smoke test `{test_id}` is invalid at {field}: {}",
            error.message
        ),
        None => format!(
            "operator smoke test `{test_id}` is invalid: {}",
            error.message
        ),
    }
}

pub(crate) fn validate_operator_id(id: &str) -> Result<(), String> {
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

pub(crate) fn normalize_optional_string(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}
pub(crate) fn discover_manifest_paths(plugin_root: &Path) -> Vec<PathBuf> {
    let operators_root = crate::domain::plugins::load_plugin_manifest(plugin_root)
        .and_then(|manifest| manifest.operators)
        .unwrap_or_else(|| plugin_root.join("operators"));
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
