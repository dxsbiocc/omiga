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
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

pub const OPERATOR_API_VERSION_V1ALPHA1: &str = "omiga.ai/operator/v1alpha1";
pub const OPERATOR_KIND: &str = "Operator";
pub const OPERATOR_TOOL_PREFIX: &str = "operator__";
const OPERATOR_STATE_DIR_NAME: &str = ".omiga";
const REGISTRY_RELATIVE_PATH: &str = "operators/registry.json";
const RUNS_RELATIVE_PATH: &str = "runs";
const OPERATOR_DEFAULT_MAX_ATTEMPTS: u32 = 2;
const OPERATOR_MAX_MAX_ATTEMPTS: u32 = 5;
const OPERATOR_CACHE_SCAN_LIMIT: usize = 200;
const OPERATOR_STRUCTURED_OUTPUTS_FILE: &str = "outputs.json";
const OPERATOR_STRUCTURED_OUTPUTS_MAX_BYTES: u64 = 1024 * 1024;
const OPERATOR_PREFLIGHT_MAX_QUESTIONS: usize = 4;
const OPERATOR_PREFLIGHT_MAX_OPTIONS: usize = 5;
const OPERATOR_PREFLIGHT_ASK_STATE: &str = "ask";
const OPERATOR_PREFLIGHT_METADATA_KEY: &str = "preflight";
const OPERATOR_PARAM_SOURCE_USER_PREFLIGHT: &str = "user_preflight";
const OPERATOR_PARAM_SOURCE_CALLER: &str = "caller";
const OPERATOR_PARAM_SOURCE_DEFAULT: &str = "default";
const OPERATOR_PARAM_SOURCE_SYSTEM: &str = "system";

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
    pub execution: OperatorExecutionSpec,
    #[serde(default)]
    pub preflight: Option<OperatorPreflightSpec>,
    #[serde(default)]
    pub runtime: Option<JsonValue>,
    #[serde(default)]
    pub resources: BTreeMap<String, OperatorResourceSpec>,
    #[serde(default)]
    pub smoke_tests: Vec<OperatorSmokeTestSpec>,
    pub enabled_aliases: Vec<String>,
    pub exposed: bool,
    pub unavailable_reason: Option<String>,
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
    #[serde(default, alias = "tests", alias = "smoke")]
    smoke_tests: Vec<RawOperatorSmokeTestSpec>,
    execution: RawOperatorExecution,
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

pub fn registry_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(OPERATOR_STATE_DIR_NAME)
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
    let spec = OperatorSpec {
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
        smoke_tests: parsed
            .smoke_tests
            .into_iter()
            .map(OperatorSmokeTestSpec::try_from)
            .collect::<Result<Vec<_>, _>>()?,
        execution: OperatorExecutionSpec { argv },
        preflight: parsed.preflight,
        runtime: parsed.runtime,
        cache: parsed.cache,
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
    };
    validate_operator_preflight(&spec)?;
    validate_operator_smoke_tests(&spec)?;
    Ok(spec)
}

fn validate_operator_preflight(spec: &OperatorSpec) -> Result<(), String> {
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

fn normalize_optional_string(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn discover_operator_candidates_from_plugins<'a>(
    plugins: impl IntoIterator<Item = &'a crate::domain::plugins::LoadedPlugin>,
) -> Vec<OperatorSpec> {
    let mut out = Vec::new();
    for plugin in plugins.into_iter().filter(|plugin| plugin.is_active()) {
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

pub fn discover_operator_candidates() -> Vec<OperatorSpec> {
    let outcome = crate::domain::plugins::plugin_load_outcome();
    discover_operator_candidates_from_plugins(outcome.plugins())
}

fn operator_manifest_diagnostics_from_plugins<'a>(
    plugins: impl IntoIterator<Item = &'a crate::domain::plugins::LoadedPlugin>,
) -> Vec<OperatorManifestDiagnostic> {
    let mut diagnostics = Vec::new();
    for plugin in plugins.into_iter().filter(|plugin| plugin.is_active()) {
        for manifest_path in discover_manifest_paths(&plugin.root) {
            if let Err(error) =
                load_operator_manifest(&manifest_path, plugin.id.clone(), plugin.root.clone())
            {
                diagnostics.push(OperatorManifestDiagnostic {
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

pub fn list_operator_manifest_diagnostics() -> Vec<OperatorManifestDiagnostic> {
    let outcome = crate::domain::plugins::plugin_load_outcome();
    operator_manifest_diagnostics_from_plugins(outcome.plugins())
}

pub fn list_operator_authoring_diagnostics() -> Vec<OperatorManifestDiagnostic> {
    let mut diagnostics = discover_operator_candidates()
        .iter()
        .flat_map(|spec| {
            operator_preflight_authoring_diagnostics(spec)
                .into_iter()
                .chain(operator_external_network_authoring_diagnostics(spec))
        })
        .collect::<Vec<_>>();
    diagnostics.sort_by(|left, right| {
        left.source_plugin
            .cmp(&right.source_plugin)
            .then_with(|| left.manifest_path.cmp(&right.manifest_path))
            .then_with(|| left.message.cmp(&right.message))
    });
    diagnostics
}

fn operator_preflight_authoring_diagnostics(
    spec: &OperatorSpec,
) -> Vec<OperatorManifestDiagnostic> {
    let Some(preflight) = &spec.preflight else {
        return Vec::new();
    };
    if preflight.questions.is_empty() {
        return Vec::new();
    }
    let asks_method = preflight
        .questions
        .iter()
        .any(preflight_question_mentions_method_choice);
    let asks_threshold_or_filter = preflight
        .questions
        .iter()
        .any(preflight_question_mentions_threshold_or_filter);
    let only_data_or_grouping = preflight
        .questions
        .iter()
        .all(preflight_question_mentions_data_or_grouping);

    if only_data_or_grouping && !asks_method && !asks_threshold_or_filter {
        return vec![OperatorManifestDiagnostic {
            source_plugin: spec.source.source_plugin.clone(),
            manifest_path: spec.source.manifest_path.to_string_lossy().into_owned(),
            severity: "warning".to_string(),
            message: format!(
                "operator `{}` preflight only asks data/grouping questions; add method, threshold, or filtering choices when those decisions affect analysis semantics",
                spec.metadata.id
            ),
        }];
    }
    Vec::new()
}

fn operator_external_network_authoring_diagnostics(
    spec: &OperatorSpec,
) -> Vec<OperatorManifestDiagnostic> {
    if !operator_declares_external_network(spec) {
        return Vec::new();
    }

    let mut diagnostics = Vec::new();
    if !cache_config_enabled(spec.cache.as_ref()) {
        diagnostics.push(OperatorManifestDiagnostic {
            source_plugin: spec.source.source_plugin.clone(),
            manifest_path: spec.source.manifest_path.to_string_lossy().into_owned(),
            severity: "warning".to_string(),
            message: format!(
                "operator `{}` declares external_network permissions but has no enabled cache policy; add cache.enabled plus policy metadata for repeatable network runs",
                spec.metadata.id
            ),
        });
    }

    let mode_supports_offline_fixture = spec
        .interface
        .params
        .get("mode")
        .filter(|field| matches!(field.kind, OperatorFieldKind::Enum))
        .map(|field| {
            field.enum_values.iter().any(|value| {
                value
                    .as_str()
                    .map(|value| value == "offline_fixture")
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false);
    let fixture_param_exists = spec.interface.params.contains_key("fixture_json");
    if !mode_supports_offline_fixture || !fixture_param_exists {
        diagnostics.push(OperatorManifestDiagnostic {
            source_plugin: spec.source.source_plugin.clone(),
            manifest_path: spec.source.manifest_path.to_string_lossy().into_owned(),
            severity: "warning".to_string(),
            message: format!(
                "operator `{}` declares external_network permissions but does not expose both mode=offline_fixture and fixture_json params for deterministic offline validation",
                spec.metadata.id
            ),
        });
    }

    diagnostics
}

fn operator_declares_external_network(spec: &OperatorSpec) -> bool {
    spec.metadata
        .tags
        .iter()
        .any(|tag| tag.eq_ignore_ascii_case("external-network"))
        || spec
            .permissions
            .as_ref()
            .and_then(|permissions| permissions.get("sideEffects"))
            .and_then(JsonValue::as_array)
            .map(|side_effects| {
                side_effects.iter().any(|value| {
                    value
                        .as_str()
                        .map(|value| value.eq_ignore_ascii_case("external_network"))
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false)
}

fn preflight_question_mentions_method_choice(question: &OperatorPreflightQuestionSpec) -> bool {
    preflight_question_text(question).contains_any(&[
        "method",
        "stat",
        "test",
        "model",
        "algorithm",
        "de_method",
        "方法",
        "统计",
        "检验",
        "模型",
    ])
}

fn preflight_question_mentions_threshold_or_filter(
    question: &OperatorPreflightQuestionSpec,
) -> bool {
    preflight_question_text(question).contains_any(&[
        "threshold",
        "cutoff",
        "filter",
        "pvalue",
        "p-value",
        "fdr",
        "padj",
        "log2fc",
        "fc",
        "min",
        "max",
        "top",
        "size",
        "阈值",
        "过滤",
        "筛选",
        "显著",
        "p值",
    ])
}

fn preflight_question_mentions_data_or_grouping(question: &OperatorPreflightQuestionSpec) -> bool {
    preflight_question_text(question).contains_any(&[
        "input",
        "data",
        "type",
        "sample",
        "group",
        "control",
        "case",
        "metadata",
        "column",
        "comparison",
        "delimiter",
        "row",
        "输入",
        "数据",
        "样本",
        "分组",
        "对照",
        "列",
        "比较",
    ])
}

fn preflight_question_text(question: &OperatorPreflightQuestionSpec) -> String {
    format!(
        "{} {} {} {}",
        question.id.as_deref().unwrap_or_default(),
        question.param,
        question.question,
        question.header
    )
    .to_ascii_lowercase()
}

trait ContainsAny {
    fn contains_any(&self, needles: &[&str]) -> bool;
}

impl ContainsAny for String {
    fn contains_any(&self, needles: &[&str]) -> bool {
        needles.iter().any(|needle| self.contains(needle))
    }
}

fn discover_manifest_paths(plugin_root: &Path) -> Vec<PathBuf> {
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
                tags: candidate.metadata.tags,
                source_plugin: candidate.source.source_plugin,
                manifest_path: candidate
                    .source
                    .manifest_path
                    .to_string_lossy()
                    .into_owned(),
                interface: candidate.interface,
                execution: candidate.execution,
                preflight: candidate.preflight,
                runtime: candidate.runtime,
                resources: candidate.resources,
                smoke_tests: candidate.smoke_tests,
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
    let mut description = operator
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
    if let Some(resource_note) = operator_resource_profile_description(&operator.spec) {
        description.push_str("\n\nResource note: ");
        description.push_str(&resource_note);
    }
    ToolSchema::new(
        name,
        description,
        operator_parameters_schema(&operator.spec),
    )
}

fn operator_resource_profile_description(spec: &OperatorSpec) -> Option<String> {
    let profile = spec.runtime.as_ref()?.get("resourceProfile")?.as_object()?;
    let tier = profile
        .get("tier")
        .and_then(JsonValue::as_str)
        .map(|value| value.trim().to_ascii_lowercase().replace('_', "-"))
        .filter(|value| !value.is_empty())?;
    if tier == "local-ok" {
        return None;
    }
    let label = match tier.as_str() {
        "hpc-required" => "HPC required",
        "hpc-recommended" | "server-recommended" => "HPC/server recommended",
        "heavy" => "resource-heavy",
        "local-warn" => "local warning",
        _ => tier.as_str(),
    };
    let mut parts = vec![label.to_string()];
    if let Some(cpu) = profile.get("recommendedCpu").and_then(JsonValue::as_u64) {
        parts.push(format!("{cpu} CPU recommended"));
    }
    if let Some(memory) = profile
        .get("recommendedMemoryGb")
        .and_then(JsonValue::as_u64)
    {
        parts.push(format!("{memory} GB RAM recommended"));
    }
    if let Some(disk) = profile.get("diskGb").and_then(JsonValue::as_u64) {
        parts.push(format!("{disk} GB disk"));
    }
    let mut out = parts.join("; ");
    if let Some(note) = profile
        .get("notes")
        .and_then(JsonValue::as_array)
        .and_then(|notes| notes.iter().find_map(JsonValue::as_str))
        .map(str::trim)
        .filter(|note| !note.is_empty())
    {
        out.push_str(". ");
        out.push_str(note);
    }
    out.push_str(" Prefer SSH/server/HPC execution for production-size inputs; local smoke fixtures are acceptable.");
    Some(out)
}

pub fn operator_parameters_schema(spec: &OperatorSpec) -> JsonValue {
    let mut properties = JsonMap::new();
    let preflight_questions = preflight_question_text_by_param(spec);
    properties.insert(
        "inputs".to_string(),
        fields_object_schema(&spec.interface.inputs, true, None),
    );
    properties.insert(
        "params".to_string(),
        fields_object_schema(&spec.interface.params, true, Some(&preflight_questions)),
    );
    properties.insert(
        "resources".to_string(),
        resources_object_schema(&spec.resources),
    );
    let mut required = vec![JsonValue::String("inputs".to_string())];
    if has_caller_required_fields(&spec.interface.params, Some(&preflight_questions)) {
        required.push(JsonValue::String("params".to_string()));
    }
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false
    })
}

fn preflight_question_text_by_param(spec: &OperatorSpec) -> BTreeMap<String, String> {
    spec.preflight
        .as_ref()
        .map(|preflight| {
            preflight
                .questions
                .iter()
                .map(|question| (question.param.clone(), question.question.clone()))
                .collect()
        })
        .unwrap_or_default()
}

fn has_caller_required_fields(
    fields: &BTreeMap<String, OperatorFieldSpec>,
    preflight_questions: Option<&BTreeMap<String, String>>,
) -> bool {
    fields.iter().any(|(name, field)| {
        field.required
            && field.default.is_none()
            && !preflight_questions
                .map(|questions| questions.contains_key(name))
                .unwrap_or(false)
    })
}

fn fields_object_schema(
    fields: &BTreeMap<String, OperatorFieldSpec>,
    include_required: bool,
    preflight_questions: Option<&BTreeMap<String, String>>,
) -> JsonValue {
    let mut properties = JsonMap::new();
    let mut required = Vec::new();
    for (name, field) in fields {
        let is_preflight_answered = preflight_questions
            .map(|questions| questions.contains_key(name))
            .unwrap_or(false);
        if include_required && field.required && field.default.is_none() && !is_preflight_answered {
            required.push(JsonValue::String(name.clone()));
        }
        properties.insert(
            name.clone(),
            field_schema(
                field,
                preflight_questions.and_then(|questions| questions.get(name)),
            ),
        );
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

fn field_schema(field: &OperatorFieldSpec, preflight_question: Option<&String>) -> JsonValue {
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
    if let Some(description) = field_description(field, preflight_question.map(String::as_str)) {
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
    let value_schema = JsonValue::Object(schema);
    if preflight_question.is_some() {
        return preflight_ask_state_schema(value_schema);
    }
    value_schema
}

fn preflight_ask_state_schema(value_schema: JsonValue) -> JsonValue {
    let mut wrapped = JsonMap::new();
    if let Some(description) = value_schema.get("description").cloned() {
        wrapped.insert("description".to_string(), description);
    }
    wrapped.insert(
        "oneOf".to_string(),
        json!([
            value_schema,
            {
                "type": "string",
                "enum": [OPERATOR_PREFLIGHT_ASK_STATE],
                "description": "Explicit ask state: set this parameter to `ask` to make Omiga ask the user through the operator preflight UI."
            }
        ]),
    );
    JsonValue::Object(wrapped)
}

fn field_description(
    field: &OperatorFieldSpec,
    preflight_question: Option<&str>,
) -> Option<String> {
    let mut parts = Vec::new();
    if let Some(description) = &field.description {
        parts.push(description.clone());
    }
    if let Some(question) = preflight_question {
        parts.push(format!(
            "Ask state: omit this value or set it to `{OPERATOR_PREFLIGHT_ASK_STATE}` to make Omiga collect it before execution (`{question}`); do not guess a value unless the user already specified it."
        ));
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

pub fn operator_preflight_question(
    tool_name: &str,
    arguments: &str,
) -> Option<crate::domain::tools::ask_user_question::AskUserQuestionArgs> {
    let resolved = resolve_operator_alias(tool_name).ok()?;
    let value = serde_json::from_str::<JsonValue>(arguments).ok()?;
    let params = value.get("params").and_then(JsonValue::as_object);
    operator_preflight_question_for_spec(&resolved.spec, Some(resolved.alias.as_str()), params)
}

pub fn operator_preflight_question_with_project_preferences(
    project_root: &Path,
    tool_name: &str,
    arguments: &str,
) -> Option<crate::domain::tools::ask_user_question::AskUserQuestionArgs> {
    let resolved = resolve_operator_alias(tool_name).ok()?;
    let value = serde_json::from_str::<JsonValue>(arguments).ok()?;
    let params = value.get("params").and_then(JsonValue::as_object);
    let recommended_params = operator_project_preference_params(project_root, &resolved.spec);
    operator_preflight_question_for_spec_with_recommended_params(
        &resolved.spec,
        Some(resolved.alias.as_str()),
        params,
        recommended_params.as_ref(),
    )
}

pub fn operator_preflight_question_for_spec(
    spec: &OperatorSpec,
    alias: Option<&str>,
    params: Option<&JsonMap<String, JsonValue>>,
) -> Option<crate::domain::tools::ask_user_question::AskUserQuestionArgs> {
    operator_preflight_question_for_spec_with_recommended_params(spec, alias, params, None)
}

pub fn operator_preflight_question_for_spec_with_recommended_params(
    spec: &OperatorSpec,
    alias: Option<&str>,
    params: Option<&JsonMap<String, JsonValue>>,
    recommended_params: Option<&BTreeMap<String, JsonValue>>,
) -> Option<crate::domain::tools::ask_user_question::AskUserQuestionArgs> {
    let preflight = spec.preflight.as_ref()?;
    let recommended_keys = recommended_params
        .map(|params| params.keys().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    let questions = preflight
        .questions
        .iter()
        .filter(|question| preflight_question_should_ask(question, params))
        .map(
            |question| crate::domain::tools::ask_user_question::QuestionItem {
                question: question.question.clone(),
                header: question.header.clone(),
                multi_select: question.multi_select,
                options: ask_options_from_specs(
                    question,
                    recommended_params.and_then(|params| params.get(&question.param)),
                ),
            },
        )
        .collect::<Vec<_>>();
    if questions.is_empty() {
        return None;
    }

    Some(
        crate::domain::tools::ask_user_question::AskUserQuestionArgs {
            questions,
            answers: None,
            annotations: None,
            metadata: Some(json!({
                "source": "operator_preflight",
                "operator_id": spec.metadata.id,
                "operator_alias": alias,
                "recommended_params": recommended_keys,
            })),
        },
    )
}

pub fn apply_operator_preflight_answers(
    tool_name: &str,
    arguments: &str,
    ask_user_output: &JsonValue,
) -> Result<String, String> {
    let resolved = match resolve_operator_alias(tool_name) {
        Ok(resolved) => resolved,
        Err(_) => return Ok(arguments.to_string()),
    };
    let Some(preflight) = resolved.spec.preflight.as_ref() else {
        return Ok(arguments.to_string());
    };
    apply_operator_preflight_answers_for_spec(&resolved.spec, preflight, arguments, ask_user_output)
}

pub fn apply_operator_preflight_answers_for_spec(
    spec: &OperatorSpec,
    preflight: &OperatorPreflightSpec,
    arguments: &str,
    ask_user_output: &JsonValue,
) -> Result<String, String> {
    let answers = ask_user_output
        .get("answers")
        .and_then(JsonValue::as_object)
        .ok_or_else(|| "ask_user_question output did not contain an answers object".to_string())?;
    let mut invocation = serde_json::from_str::<JsonValue>(arguments)
        .map_err(|err| format!("Invalid operator arguments JSON: {err}"))?;
    let root = invocation
        .as_object_mut()
        .ok_or_else(|| "Operator arguments must be a JSON object".to_string())?;
    let mut answered_params = Vec::new();
    {
        let params_value = root
            .entry("params".to_string())
            .or_insert_with(|| JsonValue::Object(JsonMap::new()));
        if !params_value.is_object() {
            *params_value = JsonValue::Object(JsonMap::new());
        }
        let params = params_value
            .as_object_mut()
            .ok_or_else(|| "Operator params must be a JSON object".to_string())?;

        for question in &preflight.questions {
            let Some(answer) = answers.get(question.question.trim()) else {
                continue;
            };
            let labels = preflight_answer_labels(answer, question.multi_select);
            if labels.is_empty() {
                continue;
            }
            let field = spec
                .interface
                .params
                .get(question.param.trim())
                .ok_or_else(|| {
                    format!(
                        "Preflight question references unknown operator parameter `{}`",
                        question.param
                    )
                })?;
            let values = labels
                .iter()
                .map(|label| preflight_value_for_answer(question, field, label))
                .collect::<Result<Vec<_>, _>>()?;
            let value = if question.multi_select {
                JsonValue::Array(values)
            } else {
                values
                    .into_iter()
                    .next()
                    .ok_or_else(|| format!("Missing preflight choice for `{}`", question.param))?
            };
            params.insert(question.param.clone(), value);
            answered_params.push(json!({
                "param": question.param.clone(),
                "questionId": question.id.clone(),
                "question": question.question.clone(),
                "labels": labels,
            }));
        }
    }
    if !answered_params.is_empty() {
        attach_operator_preflight_metadata(root, spec, answered_params);
    }

    serde_json::to_string(&invocation).map_err(|err| err.to_string())
}

fn attach_operator_preflight_metadata(
    root: &mut JsonMap<String, JsonValue>,
    spec: &OperatorSpec,
    answered_params: Vec<JsonValue>,
) {
    let mut params_by_source = JsonMap::new();
    for param in answered_params
        .iter()
        .filter_map(|entry| entry.get("param").and_then(JsonValue::as_str))
    {
        params_by_source.insert(
            param.to_string(),
            JsonValue::String(OPERATOR_PARAM_SOURCE_USER_PREFLIGHT.to_string()),
        );
    }
    let metadata_value = root
        .entry("metadata".to_string())
        .or_insert_with(|| JsonValue::Object(JsonMap::new()));
    if !metadata_value.is_object() {
        *metadata_value = JsonValue::Object(JsonMap::new());
    }
    if let Some(metadata) = metadata_value.as_object_mut() {
        metadata.insert(
            OPERATOR_PREFLIGHT_METADATA_KEY.to_string(),
            json!({
                "source": "operator_preflight",
                "operatorId": spec.metadata.id,
                "answeredParams": answered_params,
                "paramsBySource": params_by_source,
            }),
        );
    }
}

fn preflight_question_should_ask(
    question: &OperatorPreflightQuestionSpec,
    params: Option<&JsonMap<String, JsonValue>>,
) -> bool {
    if question.ask_when.always {
        return true;
    }
    let value = params.and_then(|params| params.get(&question.param));
    if value
        .map(json_value_is_preflight_ask_state)
        .unwrap_or(false)
    {
        return true;
    }
    let missing = value.is_none() || matches!(value, Some(JsonValue::Null));
    if question.ask_when.missing && missing {
        return true;
    }
    if question.ask_when.empty && value.map(json_value_is_empty).unwrap_or(false) {
        return true;
    }
    if let Some(actual) = value {
        return question
            .ask_when
            .values
            .iter()
            .any(|expected| preflight_value_matches(actual, expected));
    }
    false
}

fn json_value_is_preflight_ask_state(value: &JsonValue) -> bool {
    match value {
        JsonValue::String(value) => value
            .trim()
            .eq_ignore_ascii_case(OPERATOR_PREFLIGHT_ASK_STATE),
        JsonValue::Object(values) => {
            values
                .get("state")
                .or_else(|| values.get("status"))
                .and_then(JsonValue::as_str)
                .map(|value| {
                    value
                        .trim()
                        .eq_ignore_ascii_case(OPERATOR_PREFLIGHT_ASK_STATE)
                })
                .unwrap_or(false)
                || values
                    .get(OPERATOR_PREFLIGHT_ASK_STATE)
                    .and_then(JsonValue::as_bool)
                    .unwrap_or(false)
        }
        _ => false,
    }
}

fn json_value_is_empty(value: &JsonValue) -> bool {
    match value {
        JsonValue::Null => true,
        JsonValue::String(value) => value.trim().is_empty(),
        JsonValue::Array(values) => values.is_empty(),
        JsonValue::Object(values) => values.is_empty(),
        _ => false,
    }
}

fn preflight_value_matches(actual: &JsonValue, expected: &JsonValue) -> bool {
    match (actual, expected) {
        (JsonValue::String(left), JsonValue::String(right)) => {
            left.trim().eq_ignore_ascii_case(right.trim())
        }
        _ => actual == expected,
    }
}

fn operator_project_preference_params(
    project_root: &Path,
    spec: &OperatorSpec,
) -> Option<BTreeMap<String, JsonValue>> {
    let canonical_id = canonical_operator_unit_id_for_spec(spec);
    let hints = crate::domain::learning_proposals::matching_learning_project_preference_hints(
        project_root,
        Some(spec.metadata.id.as_str()),
        Some(canonical_id.as_str()),
        Some(spec.source.source_plugin.as_str()),
    )
    .ok()?;
    let mut params = BTreeMap::new();
    for hint in hints.hints {
        for (key, value) in hint.params {
            params.entry(key).or_insert(value);
        }
    }
    (!params.is_empty()).then_some(params)
}

fn ask_options_from_specs(
    question: &OperatorPreflightQuestionSpec,
    recommended_value: Option<&JsonValue>,
) -> Vec<crate::domain::tools::ask_user_question::QuestionOption> {
    let recommended_index = recommended_value.and_then(|value| {
        question
            .options
            .iter()
            .position(|option| preflight_option_matches_recommended_value(option, value))
    });
    let mut options = question
        .options
        .iter()
        .enumerate()
        .map(|(index, option)| ask_option_from_spec(option, Some(index) == recommended_index))
        .collect::<Vec<_>>();
    if let Some(index) = recommended_index.filter(|index| *index < options.len()) {
        let recommended = options.remove(index);
        options.insert(0, recommended);
    }
    options
}

fn preflight_option_matches_recommended_value(
    option: &OperatorPreflightOptionSpec,
    recommended_value: &JsonValue,
) -> bool {
    preflight_value_matches(&option.value, recommended_value)
        || recommended_value
            .as_str()
            .map(|value| option.label.trim().eq_ignore_ascii_case(value.trim()))
            .unwrap_or(false)
}

fn ask_option_from_spec(
    option: &OperatorPreflightOptionSpec,
    recommended: bool,
) -> crate::domain::tools::ask_user_question::QuestionOption {
    let description = if recommended {
        format!(
            "推荐：可直接确认，或改选其他选项。{}",
            option.description.trim()
        )
    } else {
        option.description.clone()
    };
    crate::domain::tools::ask_user_question::QuestionOption {
        label: option.label.clone(),
        description,
        preview: option.preview.clone(),
        recommended,
        custom: option.custom,
        custom_placeholder: option.custom_placeholder.clone(),
    }
}

fn preflight_value_for_answer(
    question: &OperatorPreflightQuestionSpec,
    field: &OperatorFieldSpec,
    answer_label: &str,
) -> Result<JsonValue, String> {
    if let Some(option) = question
        .options
        .iter()
        .find(|option| option.label.trim() == answer_label)
    {
        if option.custom {
            return Err(format!(
                "Custom preflight choice `{}` for operator parameter `{}` needs a value",
                option.label, question.param
            ));
        }
        return Ok(option.value.clone());
    }

    for option in question.options.iter().filter(|option| option.custom) {
        if let Some(raw_value) = strip_custom_answer_value(answer_label, option.label.trim()) {
            return parse_custom_preflight_value(field, raw_value).map_err(|err| {
                format!(
                    "Invalid custom value for operator parameter `{}`: {err}",
                    question.param
                )
            });
        }
    }

    Err(format!(
        "Unsupported preflight choice `{}` for operator parameter `{}`",
        answer_label, question.param
    ))
}

fn strip_custom_answer_value<'a>(answer: &'a str, label: &str) -> Option<&'a str> {
    let answer = answer.trim();
    let rest = answer.strip_prefix(label)?.trim_start();
    let rest = rest
        .strip_prefix(':')
        .or_else(|| rest.strip_prefix('：'))?
        .trim();
    (!rest.is_empty()).then_some(rest)
}

fn parse_custom_preflight_value(field: &OperatorFieldSpec, raw: &str) -> Result<JsonValue, String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err("value is empty".to_string());
    }
    let value = match field.kind {
        OperatorFieldKind::Integer => {
            json!(raw
                .parse::<i64>()
                .map_err(|_| "expected an integer".to_string())?)
        }
        OperatorFieldKind::Number => {
            let parsed = raw
                .parse::<f64>()
                .map_err(|_| "expected a number".to_string())?;
            if !parsed.is_finite() {
                return Err("expected a finite number".to_string());
            }
            json!(parsed)
        }
        OperatorFieldKind::Boolean => match raw.to_ascii_lowercase().as_str() {
            "true" | "yes" | "y" | "1" | "是" => JsonValue::Bool(true),
            "false" | "no" | "n" | "0" | "否" => JsonValue::Bool(false),
            _ => return Err("expected true/false".to_string()),
        },
        OperatorFieldKind::Json => {
            serde_json::from_str(raw).map_err(|err| format!("expected valid JSON: {err}"))?
        }
        _ => match field.enum_values.iter().find(|value| match value {
            JsonValue::String(candidate) => candidate.trim().eq_ignore_ascii_case(raw),
            _ => *value == &json!(raw),
        }) {
            Some(value) => value.clone(),
            None if !field.enum_values.is_empty() => {
                let allowed = field
                    .enum_values
                    .iter()
                    .map(|value| value.to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                return Err(format!("expected one of {allowed}"));
            }
            None => JsonValue::String(raw.to_string()),
        },
    };

    validate_custom_preflight_bounds(field, &value)?;
    Ok(value)
}

fn validate_custom_preflight_bounds(
    field: &OperatorFieldSpec,
    value: &JsonValue,
) -> Result<(), String> {
    let Some(number) = value.as_f64() else {
        return Ok(());
    };
    if let Some(minimum) = field.minimum {
        if number < minimum {
            return Err(format!("must be >= {minimum}"));
        }
    }
    if let Some(maximum) = field.maximum {
        if number > maximum {
            return Err(format!("must be <= {maximum}"));
        }
    }
    Ok(())
}

fn preflight_answer_labels(answer: &JsonValue, multi_select: bool) -> Vec<String> {
    match answer {
        JsonValue::String(value) if multi_select => value
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .collect(),
        JsonValue::String(value) => {
            let value = value.trim();
            if value.is_empty() {
                Vec::new()
            } else {
                vec![value.to_string()]
            }
        }
        JsonValue::Array(values) => values
            .iter()
            .filter_map(JsonValue::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .collect(),
        _ => Vec::new(),
    }
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperatorInvocation {
    #[serde(default)]
    pub inputs: BTreeMap<String, JsonValue>,
    #[serde(default)]
    pub params: BTreeMap<String, JsonValue>,
    #[serde(default)]
    pub resources: BTreeMap<String, JsonValue>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, JsonValue>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperatorToolError {
    pub kind: String,
    pub retryable: bool,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attempt: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_attempts: Option<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub previous_errors: Vec<OperatorRetryAttemptSummary>,
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
            attempt: None,
            max_attempts: None,
            previous_errors: Vec::new(),
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

    fn with_retry_state(mut self, retry: &OperatorRetryState) -> Self {
        self.attempt = Some(retry.attempt);
        self.max_attempts = Some(retry.max_attempts);
        self.previous_errors = retry.previous_errors.clone();
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OperatorRetryAttemptSummary {
    pub attempt: u32,
    pub kind: String,
    pub retryable: bool,
    pub message: String,
}

impl OperatorRetryAttemptSummary {
    fn from_error(attempt: u32, error: &OperatorToolError) -> Self {
        Self {
            attempt,
            kind: error.kind.clone(),
            retryable: error.retryable,
            message: error.message.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct OperatorRetryState {
    attempt: u32,
    max_attempts: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    previous_errors: Vec<OperatorRetryAttemptSummary>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct OperatorRetryPolicy {
    max_attempts: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OperatorRunCacheMetadata {
    key: String,
    #[serde(default)]
    hit: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_run_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_run_dir: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OperatorRunResult {
    status: String,
    run_id: String,
    location: String,
    operator: OperatorRunIdentity,
    run_dir: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    run_context: Option<OperatorRunContext>,
    #[serde(skip_serializing_if = "Option::is_none")]
    provenance_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    export_dir: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    markdown_report: Option<String>,
    outputs: BTreeMap<String, Vec<ArtifactRef>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    structured_outputs: Option<JsonValue>,
    effective_inputs: BTreeMap<String, JsonValue>,
    input_fingerprints: BTreeMap<String, JsonValue>,
    effective_params: BTreeMap<String, JsonValue>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    param_sources: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    preflight: Option<JsonValue>,
    effective_resources: BTreeMap<String, JsonValue>,
    attempt: u32,
    max_attempts: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    previous_errors: Vec<OperatorRetryAttemptSummary>,
    enforcement: JsonValue,
    #[serde(skip_serializing_if = "Option::is_none")]
    cache: Option<OperatorRunCacheMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<OperatorToolError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OperatorRunIdentity {
    alias: String,
    id: String,
    version: String,
    source_plugin: String,
    manifest_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OperatorRunStatusMetadata {
    run_id: String,
    location: String,
    operator: OperatorRunIdentity,
    run_dir: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    run_context: Option<OperatorRunContext>,
    #[serde(skip_serializing_if = "Option::is_none")]
    retry: Option<OperatorRetryState>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperatorRunContext {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub smoke_test_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub smoke_test_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_execution_id: Option<String>,
}

impl OperatorRunContext {
    fn is_empty(&self) -> bool {
        self.kind.as_deref().unwrap_or_default().trim().is_empty()
            && self
                .smoke_test_id
                .as_deref()
                .unwrap_or_default()
                .trim()
                .is_empty()
            && self
                .smoke_test_name
                .as_deref()
                .unwrap_or_default()
                .trim()
                .is_empty()
            && self
                .parent_execution_id
                .as_deref()
                .unwrap_or_default()
                .trim()
                .is_empty()
    }

    fn normalized(self) -> Option<Self> {
        let normalized = Self {
            kind: normalize_optional_string(self.kind),
            smoke_test_id: normalize_optional_string(self.smoke_test_id),
            smoke_test_name: normalize_optional_string(self.smoke_test_name),
            parent_execution_id: normalize_optional_string(self.parent_execution_id),
        };
        (!normalized.is_empty()).then_some(normalized)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperatorRunSummary {
    pub run_id: String,
    pub status: String,
    pub location: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operator_alias: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operator_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operator_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_plugin: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub smoke_test_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub smoke_test_name: Option<String>,
    pub run_dir: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provenance_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub export_dir: Option<String>,
    pub output_count: usize,
    pub structured_output_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retryable: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggested_action: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stdout_tail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stderr_tail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_hit: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_source_run_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_source_run_dir: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperatorRunDetail {
    pub run_id: String,
    pub location: String,
    pub run_dir: String,
    pub source_path: String,
    pub document: JsonValue,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperatorRunLog {
    pub run_id: String,
    pub location: String,
    pub log_name: String,
    pub path: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperatorRunVerification {
    pub run_id: String,
    pub location: String,
    pub run_dir: String,
    pub ok: bool,
    pub checks: Vec<OperatorRunCheck>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperatorRunCleanupRequest {
    #[serde(default)]
    pub dry_run: bool,
    #[serde(default)]
    pub keep_latest: Option<usize>,
    #[serde(default)]
    pub max_age_days: Option<u64>,
    #[serde(default)]
    pub include_cache_hits: bool,
    #[serde(default)]
    pub include_failed: bool,
    #[serde(default)]
    pub include_succeeded: bool,
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default)]
    pub operator_alias: Option<String>,
    #[serde(default)]
    pub operator_id: Option<String>,
    #[serde(default)]
    pub operator_version: Option<String>,
    #[serde(default)]
    pub source_plugin: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperatorRunCleanupCandidate {
    pub run_id: String,
    pub status: String,
    pub location: String,
    pub run_dir: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_hit: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_source_run_id: Option<String>,
    pub output_count: usize,
    pub reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub estimated_bytes: Option<u64>,
    #[serde(default)]
    pub deleted: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperatorRunCleanupResult {
    pub dry_run: bool,
    pub location: String,
    pub runs_root: String,
    pub scanned_count: usize,
    pub matched_count: usize,
    pub deleted_count: usize,
    pub skipped_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub estimated_bytes: Option<u64>,
    pub candidates: Vec<OperatorRunCleanupCandidate>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperatorRunCheck {
    pub name: String,
    pub ok: bool,
    pub severity: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OperatorExecutionSurfaceKind {
    Local,
    Ssh,
    Sandbox,
}

#[derive(Debug, Clone)]
struct OperatorExecutionSurface {
    kind: OperatorExecutionSurfaceKind,
    run_dir: String,
}

impl OperatorExecutionSurface {
    fn for_context(ctx: &crate::domain::tools::ToolContext, run_id: &str) -> Self {
        match ctx.execution_environment.as_str() {
            "ssh" => Self {
                kind: OperatorExecutionSurfaceKind::Ssh,
                run_dir: crate::domain::tools::env_store::remote_path(
                    ctx,
                    &operator_run_relative_path(run_id),
                ),
            },
            "sandbox" | "remote" => Self {
                kind: OperatorExecutionSurfaceKind::Sandbox,
                run_dir: crate::domain::tools::env_store::remote_path(
                    ctx,
                    &operator_run_relative_path(run_id),
                ),
            },
            _ => Self {
                kind: OperatorExecutionSurfaceKind::Local,
                run_dir: operator_run_dir(&ctx.project_root, run_id)
                    .to_string_lossy()
                    .into_owned(),
            },
        }
    }

    fn for_runs_root(ctx: &crate::domain::tools::ToolContext) -> Self {
        match ctx.execution_environment.as_str() {
            "ssh" => Self {
                kind: OperatorExecutionSurfaceKind::Ssh,
                run_dir: crate::domain::tools::env_store::remote_path(
                    ctx,
                    &operator_runs_relative_path(),
                ),
            },
            "sandbox" | "remote" => Self {
                kind: OperatorExecutionSurfaceKind::Sandbox,
                run_dir: crate::domain::tools::env_store::remote_path(
                    ctx,
                    &operator_runs_relative_path(),
                ),
            },
            _ => Self {
                kind: OperatorExecutionSurfaceKind::Local,
                run_dir: operator_runs_root(&ctx.project_root)
                    .to_string_lossy()
                    .into_owned(),
            },
        }
    }

    fn is_environment(&self) -> bool {
        self.kind != OperatorExecutionSurfaceKind::Local
    }

    fn artifact_location(&self) -> &'static str {
        match self.kind {
            OperatorExecutionSurfaceKind::Local => "local",
            OperatorExecutionSurfaceKind::Ssh => "ssh",
            OperatorExecutionSurfaceKind::Sandbox => "sandbox",
        }
    }
}

pub async fn execute_operator_tool_call(
    ctx: &crate::domain::tools::ToolContext,
    tool_name: &str,
    arguments: &str,
) -> (String, bool) {
    execute_operator_tool_call_with_context(ctx, tool_name, arguments, None).await
}

pub async fn execute_operator_tool_call_with_context(
    ctx: &crate::domain::tools::ToolContext,
    tool_name: &str,
    arguments: &str,
    run_context: Option<OperatorRunContext>,
) -> (String, bool) {
    let run_context = run_context.and_then(OperatorRunContext::normalized);
    let alias = tool_name
        .strip_prefix(OPERATOR_TOOL_PREFIX)
        .unwrap_or(tool_name);
    let resolved = match resolve_operator_alias(alias) {
        Ok(resolved) => resolved,
        Err(error) => return (failure_json(alias, None, None, run_context, error), true),
    };
    execute_resolved_operator_tool_call_with_context(ctx, alias, resolved, arguments, run_context)
        .await
}

pub async fn execute_resolved_operator_tool_call_with_context(
    ctx: &crate::domain::tools::ToolContext,
    alias: &str,
    resolved: ResolvedOperator,
    arguments: &str,
    run_context: Option<OperatorRunContext>,
) -> (String, bool) {
    let run_context = run_context.and_then(OperatorRunContext::normalized);
    let started_at = chrono::Utc::now().to_rfc3339();
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
            record_operator_failure_best_effort(
                ctx,
                alias,
                Some(&resolved),
                None,
                None,
                run_context.clone(),
                &started_at,
                &error,
            )
            .await;
            return (
                failure_json(alias, Some(&resolved), None, run_context, error),
                true,
            );
        }
    };

    match execute_resolved_operator(
        ctx,
        resolved.clone(),
        invocation.clone(),
        run_context.clone(),
    )
    .await
    {
        Ok(result) => {
            record_operator_success_best_effort(ctx, &result, &started_at).await;
            (
                serde_json::to_string_pretty(&result).unwrap_or_else(|err| {
                    failure_json(
                        alias,
                        Some(&resolved),
                        None,
                        run_context.clone(),
                        OperatorToolError::new("serialization_failed", false, err.to_string()),
                    )
                }),
                false,
            )
        }
        Err(error) => {
            let run_dir = error.run_dir.clone();
            record_operator_failure_best_effort(
                ctx,
                alias,
                Some(&resolved),
                run_dir.as_deref(),
                Some(&invocation),
                run_context.clone(),
                &started_at,
                &error,
            )
            .await;
            (
                failure_json(
                    alias,
                    Some(&resolved),
                    run_dir.as_deref(),
                    run_context,
                    error,
                ),
                true,
            )
        }
    }
}

fn failure_json(
    alias: &str,
    resolved: Option<&ResolvedOperator>,
    run_dir: Option<&str>,
    run_context: Option<OperatorRunContext>,
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
        "runContext": run_context,
        "error": error,
    }))
    .unwrap_or_else(|_| "{\"status\":\"failed\"}".to_string())
}

async fn record_operator_success_best_effort(
    ctx: &crate::domain::tools::ToolContext,
    result: &OperatorRunResult,
    started_at: &str,
) {
    let output_summary = operator_output_summary(result);
    let mut metadata = json!({
        "runId": result.run_id,
        "runDir": result.run_dir,
        "provenancePath": result.provenance_path,
        "exportDir": result.export_dir,
        "markdownReport": result.markdown_report,
        "operatorAlias": result.operator.alias,
        "runContext": result.run_context,
        "paramSources": result.param_sources,
        "cache": result.cache,
    });
    if let Some(preflight) = &result.preflight {
        metadata["preflight"] = preflight.clone();
    }
    let selected_params = selected_params_for_source(
        &result.effective_params,
        &result.param_sources,
        "user_preflight",
    );
    if !selected_params.is_empty() {
        metadata["selectedParams"] = json!(selected_params);
    }
    let record = crate::domain::execution_records::ExecutionRecordInput {
        kind: "operator".to_string(),
        unit_id: Some(result.operator.id.clone()),
        canonical_id: Some(canonical_operator_unit_id(&result.operator)),
        provider_plugin: Some(result.operator.source_plugin.clone()),
        status: result.status.clone(),
        session_id: ctx.session_id.clone(),
        parent_execution_id: result
            .run_context
            .as_ref()
            .and_then(|context| context.parent_execution_id.clone()),
        started_at: Some(started_at.to_string()),
        ended_at: Some(chrono::Utc::now().to_rfc3339()),
        input_hash: crate::domain::execution_records::hash_execution_map(&result.effective_inputs),
        param_hash: crate::domain::execution_records::hash_execution_map(&result.effective_params),
        output_summary_json: Some(output_summary),
        runtime_json: Some(result.enforcement.clone()),
        metadata_json: Some(metadata),
    };
    crate::domain::execution_records::record_execution_best_effort(&ctx.project_root, record).await;
}

fn selected_params_for_source(
    effective_params: &BTreeMap<String, JsonValue>,
    param_sources: &BTreeMap<String, String>,
    wanted_source: &str,
) -> BTreeMap<String, JsonValue> {
    effective_params
        .iter()
        .filter_map(|(param, value)| {
            param_sources
                .get(param)
                .filter(|source| source.as_str() == wanted_source)
                .map(|_| (param.clone(), value.clone()))
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
async fn record_operator_failure_best_effort(
    ctx: &crate::domain::tools::ToolContext,
    alias: &str,
    resolved: Option<&ResolvedOperator>,
    run_dir: Option<&str>,
    invocation: Option<&OperatorInvocation>,
    run_context: Option<OperatorRunContext>,
    started_at: &str,
    error: &OperatorToolError,
) {
    let operator = resolved.map(|resolved| OperatorRunIdentity {
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
    let mut metadata = json!({
        "runDir": run_dir,
        "operatorAlias": alias,
        "operator": operator,
        "runContext": run_context,
        "error": error,
    });
    if let (Some(resolved), Some(invocation)) = (resolved, invocation) {
        let preflight_param_names = operator_invocation_preflight_answered_params(invocation);
        let supplied_param_names = invocation.params.keys().cloned().collect::<BTreeSet<_>>();
        metadata["paramSources"] = json!(operator_param_sources(
            &resolved.spec,
            &supplied_param_names,
            &preflight_param_names,
            &invocation.params,
        ));
        if let Some(preflight) = operator_invocation_preflight_metadata(invocation) {
            metadata["preflight"] = preflight;
        }
    }
    let (input_hash, param_hash) = invocation
        .map(|invocation| {
            (
                crate::domain::execution_records::hash_execution_map(&invocation.inputs),
                crate::domain::execution_records::hash_execution_map(&invocation.params),
            )
        })
        .unwrap_or((None, None));
    let output_summary = json!({
        "errorKind": error.kind,
        "retryable": error.retryable,
        "runDir": run_dir,
    });
    let record = crate::domain::execution_records::ExecutionRecordInput {
        kind: "operator".to_string(),
        unit_id: operator.as_ref().map(|operator| operator.id.clone()),
        canonical_id: operator.as_ref().map(canonical_operator_unit_id),
        provider_plugin: operator
            .as_ref()
            .map(|operator| operator.source_plugin.clone()),
        status: "failed".to_string(),
        session_id: ctx.session_id.clone(),
        parent_execution_id: run_context
            .as_ref()
            .and_then(|context| context.parent_execution_id.clone()),
        started_at: Some(started_at.to_string()),
        ended_at: Some(chrono::Utc::now().to_rfc3339()),
        input_hash,
        param_hash,
        output_summary_json: Some(output_summary),
        runtime_json: Some(enforcement_json_for_context(ctx)),
        metadata_json: Some(metadata),
    };
    crate::domain::execution_records::record_execution_best_effort(&ctx.project_root, record).await;
}

fn canonical_operator_unit_id(operator: &OperatorRunIdentity) -> String {
    format!("{}/operator/{}", operator.source_plugin, operator.id.trim())
}

fn canonical_operator_unit_id_for_spec(spec: &OperatorSpec) -> String {
    format!(
        "{}/operator/{}",
        spec.source.source_plugin,
        spec.metadata.id.trim()
    )
}

fn operator_output_summary(result: &OperatorRunResult) -> JsonValue {
    let output_artifact_count = result
        .outputs
        .values()
        .map(|artifacts| artifacts.len())
        .sum::<usize>();
    json!({
        "runId": result.run_id,
        "outputKeys": result.outputs.keys().cloned().collect::<Vec<_>>(),
        "outputArtifactCount": output_artifact_count,
        "structuredOutputCount": result.structured_outputs.as_ref().map(output_artifact_count_json).unwrap_or(0),
        "status": result.status,
    })
}

fn output_artifact_count_json(value: &JsonValue) -> usize {
    value.as_object().map(|object| object.len()).unwrap_or(0)
}

fn enforcement_json_for_context(ctx: &crate::domain::tools::ToolContext) -> JsonValue {
    json!({
        "executionEnvironment": ctx.execution_environment,
        "sandboxBackend": ctx.sandbox_backend,
        "localVenvType": ctx.local_venv_type,
        "localVenvName": ctx.local_venv_name,
    })
}

pub fn operator_invocation_preflight_metadata(
    invocation: &OperatorInvocation,
) -> Option<JsonValue> {
    invocation
        .metadata
        .get(OPERATOR_PREFLIGHT_METADATA_KEY)
        .cloned()
}

pub fn operator_invocation_preflight_param_sources(
    invocation: &OperatorInvocation,
) -> BTreeMap<String, String> {
    operator_invocation_preflight_answered_params(invocation)
        .into_iter()
        .map(|param| (param, OPERATOR_PARAM_SOURCE_USER_PREFLIGHT.to_string()))
        .collect()
}

fn operator_invocation_preflight_answered_params(
    invocation: &OperatorInvocation,
) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let Some(preflight) = operator_invocation_preflight_metadata(invocation) else {
        return out;
    };
    if let Some(params_by_source) = preflight
        .get("paramsBySource")
        .and_then(JsonValue::as_object)
    {
        out.extend(params_by_source.keys().cloned());
    }
    if let Some(answered) = preflight
        .get("answeredParams")
        .and_then(JsonValue::as_array)
    {
        out.extend(
            answered
                .iter()
                .filter_map(|entry| entry.get("param").and_then(JsonValue::as_str))
                .map(str::to_string),
        );
    }
    out
}

fn operator_param_sources(
    spec: &OperatorSpec,
    supplied_param_names: &BTreeSet<String>,
    preflight_param_names: &BTreeSet<String>,
    effective_params: &BTreeMap<String, JsonValue>,
) -> BTreeMap<String, String> {
    effective_params
        .keys()
        .map(|param| {
            let source = if preflight_param_names.contains(param) {
                OPERATOR_PARAM_SOURCE_USER_PREFLIGHT
            } else if supplied_param_names.contains(param) {
                OPERATOR_PARAM_SOURCE_CALLER
            } else if spec
                .interface
                .params
                .get(param)
                .and_then(|field| field.default.as_ref())
                .is_some()
            {
                OPERATOR_PARAM_SOURCE_DEFAULT
            } else {
                OPERATOR_PARAM_SOURCE_SYSTEM
            };
            (param.clone(), source.to_string())
        })
        .collect()
}

async fn execute_resolved_operator(
    ctx: &crate::domain::tools::ToolContext,
    resolved: ResolvedOperator,
    invocation: OperatorInvocation,
    run_context: Option<OperatorRunContext>,
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
    let surface = OperatorExecutionSurface::for_context(ctx, &run_id);

    reject_unknown_fields(
        "inputs",
        invocation.inputs.keys(),
        &resolved.spec.interface.inputs,
    )?;
    reject_unknown_fields(
        "params",
        invocation.params.keys(),
        &resolved.spec.interface.params,
    )?;
    let preflight = operator_invocation_preflight_metadata(&invocation);
    let preflight_param_names = operator_invocation_preflight_answered_params(&invocation);
    let supplied_param_names = invocation.params.keys().cloned().collect::<BTreeSet<_>>();
    let mut effective_params = apply_param_defaults(&resolved.spec, invocation.params);
    validate_field_values("params", &resolved.spec.interface.params, &effective_params)?;
    let effective_resources =
        apply_resource_defaults_and_overrides(&resolved.spec, invocation.resources)?;
    apply_equal_bindings(&resolved.spec, &mut effective_params, &effective_resources)?;
    let param_sources = operator_param_sources(
        &resolved.spec,
        &supplied_param_names,
        &preflight_param_names,
        &effective_params,
    );

    let canonical_inputs = canonicalize_inputs(
        ctx,
        &resolved.spec,
        invocation.inputs,
        surface.is_environment(),
    )?;
    let input_fingerprints = fingerprint_inputs(
        ctx,
        &resolved.spec,
        &canonical_inputs,
        surface.artifact_location(),
    )
    .await;
    let argv = expand_argv(
        &resolved.spec,
        &canonical_inputs,
        &effective_params,
        &effective_resources,
        &surface.run_dir,
    )?;
    let enforcement = enforcement_json(ctx, &resolved.spec);
    let cache_key = operator_cache_key(
        ctx,
        &resolved,
        &surface,
        &canonical_inputs,
        &input_fingerprints,
        &effective_params,
        &effective_resources,
        &resolved.spec.execution.argv,
        &enforcement,
    );
    let cache_metadata =
        operator_cache_metadata(&resolved.spec, run_context.as_ref(), cache_key.clone());
    if let Some(cache) = cache_metadata.as_ref() {
        if let Some(hit) = find_operator_cache_hit(ctx, &resolved, &cache.key).await {
            return record_operator_cache_hit(
                ctx,
                &resolved,
                &run_id,
                &surface,
                run_context,
                cache.key.clone(),
                hit,
                canonical_inputs,
                input_fingerprints,
                effective_params,
                param_sources,
                preflight,
                effective_resources,
                enforcement,
            )
            .await;
        }
    }
    let walltime_secs = effective_walltime_secs(&effective_resources, ctx.timeout_secs);

    let retry_policy = operator_retry_policy(&resolved.spec);
    let mut previous_errors = Vec::new();
    for attempt in 1..=retry_policy.max_attempts {
        let retry_state = OperatorRetryState {
            attempt,
            max_attempts: retry_policy.max_attempts,
            previous_errors: previous_errors.clone(),
        };
        let result = if surface.kind == OperatorExecutionSurfaceKind::Local {
            execute_local(
                ctx,
                &resolved,
                &run_id,
                &surface.run_dir,
                &argv,
                walltime_secs,
                canonical_inputs.clone(),
                input_fingerprints.clone(),
                effective_params.clone(),
                param_sources.clone(),
                preflight.clone(),
                effective_resources.clone(),
                enforcement.clone(),
                cache_metadata.clone(),
                run_context.clone(),
                retry_state.clone(),
            )
            .await
        } else {
            execute_in_environment(
                ctx,
                &resolved,
                &run_id,
                &surface,
                &argv,
                walltime_secs,
                canonical_inputs.clone(),
                input_fingerprints.clone(),
                effective_params.clone(),
                param_sources.clone(),
                preflight.clone(),
                effective_resources.clone(),
                enforcement.clone(),
                cache_metadata.clone(),
                run_context.clone(),
                retry_state.clone(),
            )
            .await
        };
        match result {
            Ok(result) => return Ok(result),
            Err(error) => {
                let error = error.with_retry_state(&retry_state);
                if should_retry_operator_error(&error, &retry_policy, attempt) {
                    let summary = OperatorRetryAttemptSummary::from_error(attempt, &error);
                    let mut retrying_state = retry_state.clone();
                    retrying_state.previous_errors.push(summary.clone());
                    let _ = record_operator_retry_status(
                        ctx,
                        &resolved,
                        &run_id,
                        &surface,
                        run_context.clone(),
                        &retrying_state,
                        &error,
                    )
                    .await;
                    previous_errors.push(summary);
                    continue;
                }
                return Err(error);
            }
        }
    }
    unreachable!("operator retry loop must return on success or final failure")
}

struct OperatorCacheHit {
    run_id: String,
    run_dir: String,
    result: OperatorRunResult,
}

fn operator_cache_metadata(
    spec: &OperatorSpec,
    run_context: Option<&OperatorRunContext>,
    key: String,
) -> Option<OperatorRunCacheMetadata> {
    operator_cache_enabled(spec, run_context).then_some(OperatorRunCacheMetadata {
        key,
        hit: false,
        source_run_id: None,
        source_run_dir: None,
    })
}

fn operator_cache_enabled(spec: &OperatorSpec, run_context: Option<&OperatorRunContext>) -> bool {
    if run_context
        .and_then(|context| context.kind.as_deref())
        .map(|kind| kind.eq_ignore_ascii_case("smoke"))
        .unwrap_or(false)
    {
        return false;
    }
    cache_config_enabled(spec.cache.as_ref())
        || spec
            .runtime
            .as_ref()
            .and_then(|runtime| runtime.get("cache"))
            .map(cache_config_enabled_value)
            .unwrap_or(false)
}

fn cache_config_enabled(value: Option<&JsonValue>) -> bool {
    value.map(cache_config_enabled_value).unwrap_or(false)
}

fn cache_config_enabled_value(value: &JsonValue) -> bool {
    match value {
        JsonValue::Bool(enabled) => *enabled,
        JsonValue::Object(object) => object
            .get("enabled")
            .or_else(|| object.get("enable"))
            .and_then(JsonValue::as_bool)
            .unwrap_or(false),
        _ => false,
    }
}

fn operator_cache_key(
    ctx: &crate::domain::tools::ToolContext,
    resolved: &ResolvedOperator,
    surface: &OperatorExecutionSurface,
    canonical_inputs: &BTreeMap<String, JsonValue>,
    input_fingerprints: &BTreeMap<String, JsonValue>,
    effective_params: &BTreeMap<String, JsonValue>,
    effective_resources: &BTreeMap<String, JsonValue>,
    argv_template: &[String],
    enforcement: &JsonValue,
) -> String {
    let payload = json!({
        "schema": "operator-cache/v1",
        "operator": {
            "alias": resolved.alias.as_str(),
            "id": resolved.spec.metadata.id.as_str(),
            "version": resolved.spec.metadata.version.as_str(),
            "sourcePlugin": resolved.spec.source.source_plugin.as_str(),
            "manifestPath": resolved.spec.source.manifest_path.to_string_lossy(),
        },
        "surface": {
            "location": surface.artifact_location(),
            "executionEnvironment": ctx.execution_environment.as_str(),
            "sshServer": ctx.ssh_server.as_deref(),
            "sandboxBackend": ctx.sandbox_backend.as_str(),
        },
        "inputs": canonical_inputs,
        "inputFingerprints": input_fingerprints,
        "params": effective_params,
        "resources": effective_resources,
        "argvTemplate": argv_template,
        "enforcement": enforcement,
    });
    stable_sha256_json(&payload)
}

fn stable_sha256_json(value: &JsonValue) -> String {
    use sha2::{Digest, Sha256};

    let raw = serde_json::to_vec(value).unwrap_or_else(|_| value.to_string().into_bytes());
    let digest = Sha256::digest(&raw);
    format!("sha256:{digest:x}")
}

async fn find_operator_cache_hit(
    ctx: &crate::domain::tools::ToolContext,
    resolved: &ResolvedOperator,
    cache_key: &str,
) -> Option<OperatorCacheHit> {
    let summaries = list_operator_runs_for_context(ctx, OPERATOR_CACHE_SCAN_LIMIT)
        .await
        .ok()?;
    for summary in summaries {
        if summary.status != "succeeded"
            || summary.operator_alias.as_deref() != Some(resolved.alias.as_str())
            || summary.operator_id.as_deref() != Some(resolved.spec.metadata.id.as_str())
            || summary.operator_version.as_deref() != Some(resolved.spec.metadata.version.as_str())
            || summary.source_plugin.as_deref() != Some(resolved.spec.source.source_plugin.as_str())
        {
            continue;
        }
        let detail = match read_operator_run_for_context(ctx, &summary.run_id).await {
            Ok(detail) => detail,
            Err(_) => continue,
        };
        let result = match serde_json::from_value::<OperatorRunResult>(detail.document.clone()) {
            Ok(result) => result,
            Err(_) => continue,
        };
        let Some(cache) = result.cache.as_ref() else {
            continue;
        };
        if result.status != "succeeded" || cache.hit || cache.key.as_str() != cache_key {
            continue;
        }
        if !operator_cached_outputs_exist(ctx, &result.location, &result.outputs).await {
            continue;
        }
        return Some(OperatorCacheHit {
            run_id: detail.run_id,
            run_dir: detail.run_dir,
            result,
        });
    }
    None
}

async fn operator_cached_outputs_exist(
    ctx: &crate::domain::tools::ToolContext,
    location: &str,
    outputs: &BTreeMap<String, Vec<ArtifactRef>>,
) -> bool {
    let paths = outputs
        .values()
        .flat_map(|artifacts| artifacts.iter())
        .map(|artifact| artifact.path.as_str())
        .filter(|path| !path.trim().is_empty())
        .collect::<Vec<_>>();
    if paths.is_empty() {
        return true;
    }
    if location == "local" {
        return paths.iter().all(|path| {
            fs::metadata(path)
                .map(|metadata| metadata.is_file())
                .unwrap_or(false)
        });
    }
    let tests = paths
        .iter()
        .map(|path| format!("[ -f {} ]", sh_quote(path)))
        .collect::<Vec<_>>()
        .join(" && ");
    execute_env_command(ctx, &operator_environment_cwd(ctx), &tests, 30)
        .await
        .map(|result| result.returncode == 0)
        .unwrap_or(false)
}

#[allow(clippy::too_many_arguments)]
async fn record_operator_cache_hit(
    ctx: &crate::domain::tools::ToolContext,
    resolved: &ResolvedOperator,
    run_id: &str,
    surface: &OperatorExecutionSurface,
    run_context: Option<OperatorRunContext>,
    cache_key: String,
    hit: OperatorCacheHit,
    effective_inputs: BTreeMap<String, JsonValue>,
    input_fingerprints: BTreeMap<String, JsonValue>,
    effective_params: BTreeMap<String, JsonValue>,
    param_sources: BTreeMap<String, String>,
    preflight: Option<JsonValue>,
    effective_resources: BTreeMap<String, JsonValue>,
    enforcement: JsonValue,
) -> Result<OperatorRunResult, OperatorToolError> {
    let run_dir = surface.run_dir.as_str();
    let retry_state = OperatorRetryState {
        attempt: 0,
        max_attempts: 0,
        previous_errors: Vec::new(),
    };
    let status_metadata = operator_run_status_metadata(
        resolved,
        run_id,
        surface.artifact_location(),
        run_dir,
        run_context.clone(),
        retry_state.clone(),
    );
    let cache = OperatorRunCacheMetadata {
        key: cache_key,
        hit: true,
        source_run_id: Some(hit.run_id.clone()),
        source_run_dir: Some(hit.run_dir.clone()),
    };
    let export_dir = if surface.kind == OperatorExecutionSurfaceKind::Local {
        let source_out = PathBuf::from(&hit.run_dir).join("out");
        Some(
            export_local_operator_results(ctx, resolved, run_id, &source_out)
                .map_err(|error| error.with_run_dir(run_dir))?,
        )
    } else {
        Some(
            export_environment_operator_results(
                ctx,
                resolved,
                run_id,
                &format!("{}/out", hit.run_dir),
            )
            .await
            .map_err(|error| error.with_run_dir(run_dir))?,
        )
    };
    let markdown_report =
        operator_result_markdown_report(resolved, export_dir.as_deref(), &hit.result.outputs);
    if let (Some(export_dir), Some(report)) = (export_dir.as_deref(), markdown_report.as_deref()) {
        if surface.kind == OperatorExecutionSurfaceKind::Local {
            write_local_operator_result_readme(export_dir, report)
                .map_err(|error| error.with_run_dir(run_dir))?;
        } else {
            write_environment_operator_result_readme(ctx, export_dir, report)
                .await
                .map_err(|error| error.with_run_dir(run_dir))?;
        }
    }
    let result = OperatorRunResult {
        status: "succeeded".to_string(),
        run_id: run_id.to_string(),
        location: surface.artifact_location().to_string(),
        operator: status_metadata.operator.clone(),
        run_dir: run_dir.to_string(),
        run_context,
        provenance_path: Some(if surface.kind == OperatorExecutionSurfaceKind::Local {
            PathBuf::from(run_dir)
                .join("provenance.json")
                .to_string_lossy()
                .into_owned()
        } else {
            format!("{run_dir}/provenance.json")
        }),
        export_dir,
        markdown_report,
        outputs: hit.result.outputs.clone(),
        structured_outputs: hit.result.structured_outputs.clone(),
        effective_inputs,
        input_fingerprints,
        effective_params,
        param_sources,
        preflight,
        effective_resources,
        attempt: retry_state.attempt,
        max_attempts: retry_state.max_attempts,
        previous_errors: Vec::new(),
        enforcement,
        cache: Some(cache),
        error: None,
    };
    if surface.kind == OperatorExecutionSurfaceKind::Local {
        record_local_operator_cache_hit(run_dir, &hit, &status_metadata, &result)?;
    } else {
        record_environment_operator_cache_hit(ctx, run_dir, &hit, &status_metadata, &result)
            .await?;
    }
    Ok(result)
}

fn record_local_operator_cache_hit(
    run_dir: &str,
    hit: &OperatorCacheHit,
    status_metadata: &OperatorRunStatusMetadata,
    result: &OperatorRunResult,
) -> Result<(), OperatorToolError> {
    let run_path = PathBuf::from(run_dir);
    fs::create_dir_all(run_path.join("logs")).map_err(|err| {
        OperatorToolError::new("execution_infra_error", true, err.to_string()).with_run_dir(run_dir)
    })?;
    fs::create_dir_all(run_path.join("out")).map_err(|err| {
        OperatorToolError::new("execution_infra_error", true, err.to_string()).with_run_dir(run_dir)
    })?;
    fs::create_dir_all(run_path.join("work")).map_err(|err| {
        OperatorToolError::new("execution_infra_error", true, err.to_string()).with_run_dir(run_dir)
    })?;
    fs::write(
        run_path.join("logs/stdout.txt"),
        format!("Operator cache hit: reused run {}.\n", hit.run_id),
    )
    .map_err(|err| {
        OperatorToolError::new("provenance_write_failed", false, err.to_string())
            .with_run_dir(run_dir)
    })?;
    fs::write(run_path.join("logs/stderr.txt"), "").map_err(|err| {
        OperatorToolError::new("provenance_write_failed", false, err.to_string())
            .with_run_dir(run_dir)
    })?;
    write_json_file(&run_path.join("provenance.json"), result).map_err(|err| {
        OperatorToolError::new("provenance_write_failed", false, err).with_run_dir(run_dir)
    })?;
    update_local_status(&run_path, "succeeded", None, Some(status_metadata))
}

async fn record_environment_operator_cache_hit(
    ctx: &crate::domain::tools::ToolContext,
    run_dir: &str,
    hit: &OperatorCacheHit,
    status_metadata: &OperatorRunStatusMetadata,
    result: &OperatorRunResult,
) -> Result<(), OperatorToolError> {
    let stdout = format!("Operator cache hit: reused run {}.\n", hit.run_id);
    let command = format!(
        "mkdir -p {}/logs {}/out {}/work && printf %s {} > {}/logs/stdout.txt && : > {}/logs/stderr.txt",
        sh_quote(run_dir),
        sh_quote(run_dir),
        sh_quote(run_dir),
        sh_quote(&stdout),
        sh_quote(run_dir),
        sh_quote(run_dir)
    );
    execute_env_command(ctx, &operator_environment_cwd(ctx), &command, 30).await?;
    write_environment_json(ctx, run_dir, "provenance.json", result).await?;
    update_environment_status(ctx, run_dir, "succeeded", None, Some(status_metadata)).await
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
    let container = effective_runtime_container(ctx, runtime);
    let placements = runtime_axis_values(runtime, "placement");
    let containers = runtime_axis_values(runtime, "container");
    let schedulers = runtime_axis_values(runtime, "scheduler");
    let flat = runtime
        .get("supported")
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(|s| s.trim().to_ascii_lowercase()))
                .collect::<HashSet<_>>()
        })
        .unwrap_or_default();
    (placements.is_empty() || placements.contains(placement) || flat.contains(placement))
        && (containers.is_empty() || containers.contains(&container) || flat.contains(&container))
        && (schedulers.is_empty() || schedulers.contains("none") || flat.contains("scheduler=none"))
}

fn effective_runtime_container(
    ctx: &crate::domain::tools::ToolContext,
    runtime: &JsonValue,
) -> String {
    match ctx.execution_environment.as_str() {
        "sandbox" | "remote" => {
            let backend = ctx.sandbox_backend.trim().to_ascii_lowercase();
            if backend.is_empty() {
                "none".to_string()
            } else {
                backend
            }
        }
        "ssh" | "local" => selected_direct_container(ctx, runtime)
            .map(|selection| selection.kind.as_str().to_string())
            .unwrap_or_else(|| "none".to_string()),
        _ => "none".to_string(),
    }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OperatorContainerKind {
    Docker,
    Singularity,
}

impl OperatorContainerKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Docker => "docker",
            Self::Singularity => "singularity",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct OperatorContainerSelection {
    kind: OperatorContainerKind,
    image: String,
    prepare: Option<OperatorContainerImagePrepare>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum OperatorContainerImagePrepare {
    Dockerfile {
        dockerfile: String,
        context: String,
        tag: String,
    },
    SingularityDefinition {
        definition: String,
        sif: String,
        hash: String,
    },
}

fn selected_direct_container(
    ctx: &crate::domain::tools::ToolContext,
    runtime: &JsonValue,
) -> Option<OperatorContainerSelection> {
    let declared = declared_runtime_containers(runtime);
    if declared.contains("none") {
        let explicit = explicit_runtime_container_kind(runtime);
        return explicit
            .filter(|kind| declared.contains(kind.as_str()))
            .map(|kind| OperatorContainerSelection {
                kind,
                image: runtime_container_image(runtime, kind),
                prepare: None,
            });
    }

    let backend = ctx.sandbox_backend.trim().to_ascii_lowercase();
    let preferred =
        explicit_runtime_container_kind(runtime).or_else(|| container_kind_from_name(&backend));
    preferred
        .filter(|kind| declared.contains(kind.as_str()))
        .map(|kind| OperatorContainerSelection {
            kind,
            image: runtime_container_image(runtime, kind),
            prepare: None,
        })
}

fn declared_runtime_containers(runtime: &JsonValue) -> HashSet<String> {
    let mut out = runtime_axis_values(runtime, "container")
        .into_iter()
        .filter(|value| matches!(value.as_str(), "none" | "docker" | "singularity"))
        .collect::<HashSet<_>>();
    if let Some(items) = runtime.get("supported").and_then(JsonValue::as_array) {
        for item in items {
            if let Some(value) = item.as_str().map(|value| value.trim().to_ascii_lowercase()) {
                if matches!(value.as_str(), "none" | "docker" | "singularity") {
                    out.insert(value);
                }
            }
        }
    }
    if out.is_empty() {
        out.insert("none".to_string());
    }
    out
}

fn explicit_runtime_container_kind(runtime: &JsonValue) -> Option<OperatorContainerKind> {
    let container = runtime.get("container")?;
    ["default", "preferred", "type", "backend"]
        .into_iter()
        .filter_map(|key| container.get(key).and_then(JsonValue::as_str))
        .find_map(|value| container_kind_from_name(value.trim()))
}

fn container_kind_from_name(value: &str) -> Option<OperatorContainerKind> {
    match value.trim().to_ascii_lowercase().as_str() {
        "docker" => Some(OperatorContainerKind::Docker),
        "singularity" => Some(OperatorContainerKind::Singularity),
        _ => None,
    }
}

fn runtime_container_image(runtime: &JsonValue, kind: OperatorContainerKind) -> String {
    let container = runtime.get("container").unwrap_or(&JsonValue::Null);
    let by_kind = match kind {
        OperatorContainerKind::Docker => container
            .get("dockerImage")
            .or_else(|| container.get("docker_image")),
        OperatorContainerKind::Singularity => container
            .get("singularityImage")
            .or_else(|| container.get("singularity_image")),
    };
    by_kind
        .and_then(JsonValue::as_str)
        .or_else(|| {
            container
                .get("images")
                .and_then(|images| images.get(kind.as_str()))
                .and_then(JsonValue::as_str)
        })
        .or_else(|| container.get("image").and_then(JsonValue::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| match kind {
            OperatorContainerKind::Docker => {
                std::env::var("OMIGA_DOCKER_IMAGE").unwrap_or_else(|_| "ubuntu:22.04".to_string())
            }
            OperatorContainerKind::Singularity => std::env::var("OMIGA_SINGULARITY_IMAGE")
                .unwrap_or_else(|_| "docker://ubuntu:22.04".to_string()),
        })
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

fn reject_unknown_fields<'a>(
    scope: &str,
    names: impl Iterator<Item = &'a String>,
    declared: &BTreeMap<String, OperatorFieldSpec>,
) -> Result<(), OperatorToolError> {
    for name in names {
        if !declared.contains_key(name) {
            return Err(OperatorToolError::new(
                "invalid_arguments",
                false,
                format!("Unknown {scope} field `{name}`."),
            )
            .with_field(format!("{scope}.{name}"))
            .with_suggested_action("Retry with only fields declared in the operator schema."));
        }
    }
    Ok(())
}

fn validate_field_values(
    scope: &str,
    fields: &BTreeMap<String, OperatorFieldSpec>,
    values: &BTreeMap<String, JsonValue>,
) -> Result<(), OperatorToolError> {
    let error_kind = field_validation_error_kind(scope);
    for (name, field) in fields {
        match values.get(name) {
            Some(value) => validate_field_value(scope, name, field, value)?,
            None if field.required => {
                return Err(OperatorToolError::new(
                    error_kind,
                    false,
                    format!("Required {scope} field `{name}` is missing."),
                )
                .with_field(format!("{scope}.{name}")))
            }
            None => {}
        }
    }
    Ok(())
}

fn field_validation_error_kind(scope: &str) -> &'static str {
    if scope == "structuredOutputs" {
        "output_validation_failed"
    } else {
        "input_validation_failed"
    }
}

fn validate_field_value(
    scope: &str,
    name: &str,
    field: &OperatorFieldSpec,
    value: &JsonValue,
) -> Result<(), OperatorToolError> {
    let error_kind = field_validation_error_kind(scope);
    let field_path = format!("{scope}.{name}");
    if value.is_null() {
        if field.required {
            return Err(OperatorToolError::new(
                error_kind,
                false,
                format!("Required {scope} field `{name}` must not be null."),
            )
            .with_field(field_path));
        }
        return Ok(());
    }

    match field.kind {
        OperatorFieldKind::String | OperatorFieldKind::File | OperatorFieldKind::Directory => {
            let text = value.as_str().ok_or_else(|| {
                OperatorToolError::new(
                    error_kind,
                    false,
                    format!("{scope} field `{name}` must be a string."),
                )
                .with_field(field_path.clone())
            })?;
            if field.non_empty.unwrap_or(false) && text.trim().is_empty() {
                return Err(OperatorToolError::new(
                    error_kind,
                    false,
                    format!("{scope} field `{name}` must not be empty."),
                )
                .with_field(field_path));
            }
        }
        OperatorFieldKind::Integer => {
            let number = value.as_i64().or_else(|| value.as_u64().map(|n| n as i64));
            let Some(number) = number else {
                return Err(OperatorToolError::new(
                    error_kind,
                    false,
                    format!("{scope} field `{name}` must be an integer."),
                )
                .with_field(field_path));
            };
            validate_numeric_bounds(scope, name, &field_path, number as f64, field)?;
        }
        OperatorFieldKind::Number => {
            let Some(number) = value.as_f64() else {
                return Err(OperatorToolError::new(
                    error_kind,
                    false,
                    format!("{scope} field `{name}` must be a number."),
                )
                .with_field(field_path));
            };
            validate_numeric_bounds(scope, name, &field_path, number, field)?;
        }
        OperatorFieldKind::Boolean => {
            if !value.is_boolean() {
                return Err(OperatorToolError::new(
                    error_kind,
                    false,
                    format!("{scope} field `{name}` must be a boolean."),
                )
                .with_field(field_path));
            }
        }
        OperatorFieldKind::Json => {
            if !value.is_object() {
                return Err(OperatorToolError::new(
                    error_kind,
                    false,
                    format!("{scope} field `{name}` must be a JSON object."),
                )
                .with_field(field_path));
            }
        }
        OperatorFieldKind::FileArray | OperatorFieldKind::DirectoryArray => {
            let array = value.as_array().ok_or_else(|| {
                OperatorToolError::new(
                    error_kind,
                    false,
                    format!("{scope} field `{name}` must be an array of strings."),
                )
                .with_field(field_path.clone())
            })?;
            if field.non_empty.unwrap_or(false) && array.is_empty() {
                return Err(OperatorToolError::new(
                    error_kind,
                    false,
                    format!("{scope} field `{name}` must not be empty."),
                )
                .with_field(field_path.clone()));
            }
            if let Some(min_size) = field.min_size {
                if (array.len() as u64) < min_size {
                    return Err(OperatorToolError::new(
                        error_kind,
                        false,
                        format!("{scope} field `{name}` requires at least {min_size} item(s)."),
                    )
                    .with_field(field_path.clone()));
                }
            }
            for (index, item) in array.iter().enumerate() {
                if !item.is_string() {
                    return Err(OperatorToolError::new(
                        error_kind,
                        false,
                        format!("{scope} field `{name}[{index}]` must be a string."),
                    )
                    .with_field(format!("{field_path}[{index}]")));
                }
            }
        }
        OperatorFieldKind::Enum => {}
    }

    if !field.enum_values.is_empty() && !field.enum_values.iter().any(|item| item == value) {
        return Err(OperatorToolError::new(
            error_kind,
            false,
            format!("{scope} field `{name}` is not one of the allowed enum values."),
        )
        .with_field(field_path));
    }
    Ok(())
}

fn validate_numeric_bounds(
    scope: &str,
    name: &str,
    field_path: &str,
    number: f64,
    field: &OperatorFieldSpec,
) -> Result<(), OperatorToolError> {
    let error_kind = field_validation_error_kind(scope);
    if field
        .minimum
        .map(|minimum| number < minimum)
        .unwrap_or(false)
    {
        return Err(OperatorToolError::new(
            error_kind,
            false,
            format!(
                "{scope} field `{name}` must be >= {}.",
                field.minimum.unwrap_or_default()
            ),
        )
        .with_field(field_path.to_string()));
    }
    if field
        .maximum
        .map(|maximum| number > maximum)
        .unwrap_or(false)
    {
        return Err(OperatorToolError::new(
            error_kind,
            false,
            format!(
                "{scope} field `{name}` must be <= {}.",
                field.maximum.unwrap_or_default()
            ),
        )
        .with_field(field_path.to_string()));
    }
    Ok(())
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
        validate_resource_value(&name, &value, resource)?;
        out.insert(name, value);
    }
    for (name, value) in &out {
        if let Some(resource) = spec.resources.get(name) {
            validate_resource_value(name, value, resource)?;
        }
    }
    Ok(out)
}

fn validate_resource_value(
    name: &str,
    value: &JsonValue,
    spec: &OperatorResourceSpec,
) -> Result<(), OperatorToolError> {
    if let (Some(value), Some(minimum)) = (
        value.as_f64(),
        spec.min.as_ref().and_then(JsonValue::as_f64),
    ) {
        if value < minimum {
            return Err(OperatorToolError::new(
                "invalid_arguments",
                false,
                format!("Resource `{name}` must be >= {minimum}."),
            )
            .with_field(format!("resources.{name}")));
        }
    }
    if let (Some(value), Some(maximum)) = (
        value.as_f64(),
        spec.max.as_ref().and_then(JsonValue::as_f64),
    ) {
        if value > maximum {
            return Err(OperatorToolError::new(
                "invalid_arguments",
                false,
                format!("Resource `{name}` must be <= {maximum}."),
            )
            .with_field(format!("resources.{name}")));
        }
    }
    Ok(())
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
        if !field.required
            && field.kind.is_path_like()
            && value.as_str().map(str::trim).is_some_and(str::is_empty)
        {
            continue;
        }
        validate_field_value("inputs", name, field, &value)?;
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

async fn fingerprint_inputs(
    ctx: &crate::domain::tools::ToolContext,
    spec: &OperatorSpec,
    inputs: &BTreeMap<String, JsonValue>,
    location: &str,
) -> BTreeMap<String, JsonValue> {
    let mut out = BTreeMap::new();
    for (name, value) in inputs {
        let Some(field) = spec.interface.inputs.get(name) else {
            continue;
        };
        if !field.kind.is_path_like() {
            out.insert(
                name.clone(),
                json!({"mode": "value", "fingerprint": stable_json_fingerprint(value)}),
            );
            continue;
        }
        if field.kind.is_array() {
            let mut items = Vec::new();
            for path in value
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(JsonValue::as_str)
            {
                items.push(path_fingerprint(ctx, location, path, &field.kind).await);
            }
            out.insert(name.clone(), JsonValue::Array(items));
        } else if let Some(path) = value.as_str() {
            out.insert(
                name.clone(),
                path_fingerprint(ctx, location, path, &field.kind).await,
            );
        }
    }
    out
}

fn stable_json_fingerprint(value: &JsonValue) -> String {
    // FNV-1a over serde_json's canonical-ish rendering is sufficient for the
    // MVP lightweight fingerprint. It is intentionally not a strong cache key.
    let raw = serde_json::to_string(value).unwrap_or_else(|_| value.to_string());
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in raw.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("fnv1a64:{hash:016x}")
}

async fn path_fingerprint(
    ctx: &crate::domain::tools::ToolContext,
    location: &str,
    path: &str,
    kind: &OperatorFieldKind,
) -> JsonValue {
    if location == "local" {
        return local_path_fingerprint(location, path, kind);
    }
    remote_path_fingerprint(ctx, location, path).await
}

fn local_path_fingerprint(location: &str, path: &str, kind: &OperatorFieldKind) -> JsonValue {
    let metadata = fs::metadata(path).ok();
    let size = metadata.as_ref().map(|metadata| metadata.len());
    let modified = metadata
        .as_ref()
        .and_then(|metadata| metadata.modified().ok())
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs());
    let mut value = json!({
        "mode": "stat",
        "location": location,
        "path": path,
        "size": size,
        "modifiedUnixSecs": modified
    });
    if matches!(kind, OperatorFieldKind::File | OperatorFieldKind::FileArray)
        && metadata
            .as_ref()
            .map(|metadata| metadata.is_file())
            .unwrap_or(false)
    {
        if let Some(sha256) = sha256_file(path) {
            value["mode"] = json!("sha256");
            value["algorithm"] = json!("sha256");
            value["fingerprint"] = json!(format!("sha256:{sha256}"));
            value["sha256"] = json!(sha256);
        }
    }
    value
}

fn sha256_file(path: &str) -> Option<String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;

    let mut file = fs::File::open(path).ok()?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).ok()?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Some(format!("{:x}", hasher.finalize()))
}

async fn remote_path_fingerprint(
    ctx: &crate::domain::tools::ToolContext,
    location: &str,
    path: &str,
) -> JsonValue {
    let command = format!(
        "p={}; if [ -f \"$p\" ]; then size=$(wc -c < \"$p\" 2>/dev/null | tr -d ' '); modified=$( (stat -c %Y \"$p\" 2>/dev/null || stat -f %m \"$p\" 2>/dev/null) | head -n 1 ); checksum=$(sha256sum \"$p\" 2>/dev/null | awk '{{print $1}}'); if [ -z \"$checksum\" ]; then checksum=$(shasum -a 256 \"$p\" 2>/dev/null | awk '{{print $1}}'); fi; printf '__OMIGA_FILE__\\n%s\\n%s\\n%s\\n' \"$size\" \"$modified\" \"$checksum\"; elif [ -e \"$p\" ]; then modified=$( (stat -c %Y \"$p\" 2>/dev/null || stat -f %m \"$p\" 2>/dev/null) | head -n 1 ); printf '__OMIGA_PATH__\\n%s\\n' \"$modified\"; else printf '__OMIGA_MISSING__\\n'; fi",
        sh_quote(path)
    );
    match execute_env_command(ctx, &operator_environment_cwd(ctx), &command, 30).await {
        Ok(result) => parse_remote_path_fingerprint(location, path, &result.output),
        Err(_) => json!({
            "mode": "reference",
            "location": location,
            "path": path
        }),
    }
}

fn parse_remote_path_fingerprint(location: &str, path: &str, output: &str) -> JsonValue {
    let lines = output.lines().collect::<Vec<_>>();
    match lines.first().copied().unwrap_or_default() {
        "__OMIGA_FILE__" => {
            let size = lines
                .get(1)
                .and_then(|value| value.trim().parse::<u64>().ok());
            let modified = lines
                .get(2)
                .and_then(|value| value.trim().parse::<u64>().ok());
            let checksum = lines.get(3).map(|value| value.trim()).filter(|value| {
                value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
            });
            let mut value = json!({
                "mode": "stat",
                "location": location,
                "path": path,
                "size": size,
                "modifiedUnixSecs": modified
            });
            if let Some(checksum) = checksum {
                value["mode"] = json!("sha256");
                value["algorithm"] = json!("sha256");
                value["fingerprint"] = json!(format!("sha256:{checksum}"));
                value["sha256"] = json!(checksum);
            }
            value
        }
        "__OMIGA_PATH__" => {
            let modified = lines
                .get(1)
                .and_then(|value| value.trim().parse::<u64>().ok());
            json!({
                "mode": "stat",
                "location": location,
                "path": path,
                "modifiedUnixSecs": modified
            })
        }
        "__OMIGA_MISSING__" => json!({
            "mode": "reference",
            "location": location,
            "path": path,
            "available": false
        }),
        _ => json!({
            "mode": "reference",
            "location": location,
            "path": path
        }),
    }
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
        if replaced.contains('/') && !Path::new(&replaced).is_absolute() {
            let plugin_file = spec.source.plugin_root.join(&replaced);
            if plugin_file.is_file() || index == 0 {
                replaced = plugin_file.to_string_lossy().into_owned();
            }
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
    clear_missing_optional_vars(
        &mut out,
        "inputs",
        &spec.interface.inputs,
        inputs,
        |field| !field.required,
    );
    clear_missing_optional_vars(
        &mut out,
        "params",
        &spec.interface.params,
        params,
        |field| !field.required,
    );
    clear_missing_optional_vars(&mut out, "resources", &spec.resources, resources, |field| {
        !field.exposed
    });
    Ok(out)
}

fn clear_missing_optional_vars<T>(
    out: &mut String,
    prefix: &str,
    specs: &BTreeMap<String, T>,
    provided: &BTreeMap<String, JsonValue>,
    optional: impl Fn(&T) -> bool,
) {
    for (name, spec) in specs {
        if optional(spec) && !provided.contains_key(name) {
            *out = out.replace(&format!("${{{prefix}.{name}}}"), "");
            *out = out.replace(&format!("{{{{ {prefix}.{name} }}}}"), "");
        }
    }
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

fn operator_retry_policy(spec: &OperatorSpec) -> OperatorRetryPolicy {
    let configured = spec
        .runtime
        .as_ref()
        .and_then(|runtime| runtime.get("retry"))
        .and_then(|retry| {
            retry
                .get("maxAttempts")
                .or_else(|| retry.get("max_attempts"))
        })
        .and_then(JsonValue::as_u64)
        .and_then(|value| u32::try_from(value).ok());
    let max_attempts = configured
        .unwrap_or(OPERATOR_DEFAULT_MAX_ATTEMPTS)
        .clamp(1, OPERATOR_MAX_MAX_ATTEMPTS);
    OperatorRetryPolicy { max_attempts }
}

fn should_retry_operator_error(
    error: &OperatorToolError,
    policy: &OperatorRetryPolicy,
    attempt: u32,
) -> bool {
    attempt < policy.max_attempts && error.retryable && retryable_operator_error_kind(&error.kind)
}

fn retryable_operator_error_kind(kind: &str) -> bool {
    matches!(
        kind,
        "environment_unavailable" | "execution_infra_error" | "provenance_write_failed"
    )
}

fn operator_run_status_metadata(
    resolved: &ResolvedOperator,
    run_id: &str,
    location: &str,
    run_dir: &str,
    run_context: Option<OperatorRunContext>,
    retry: OperatorRetryState,
) -> OperatorRunStatusMetadata {
    OperatorRunStatusMetadata {
        run_id: run_id.to_string(),
        location: location.to_string(),
        operator: run_identity(resolved),
        run_dir: run_dir.to_string(),
        run_context,
        retry: Some(retry),
    }
}

async fn record_operator_retry_status(
    ctx: &crate::domain::tools::ToolContext,
    resolved: &ResolvedOperator,
    run_id: &str,
    surface: &OperatorExecutionSurface,
    run_context: Option<OperatorRunContext>,
    retry: &OperatorRetryState,
    error: &OperatorToolError,
) -> Result<(), OperatorToolError> {
    let run_dir = surface.run_dir.as_str();
    let metadata = operator_run_status_metadata(
        resolved,
        run_id,
        surface.artifact_location(),
        run_dir,
        run_context,
        retry.clone(),
    );
    let error = error.clone().with_retry_state(retry);
    if surface.kind == OperatorExecutionSurfaceKind::Local {
        update_local_status(
            &PathBuf::from(run_dir),
            "retrying",
            Some(&error),
            Some(&metadata),
        )
    } else {
        update_environment_status(ctx, run_dir, "retrying", Some(&error), Some(&metadata)).await
    }
}

async fn execute_local(
    ctx: &crate::domain::tools::ToolContext,
    resolved: &ResolvedOperator,
    run_id: &str,
    run_dir: &str,
    argv: &[String],
    walltime_secs: u64,
    effective_inputs: BTreeMap<String, JsonValue>,
    input_fingerprints: BTreeMap<String, JsonValue>,
    effective_params: BTreeMap<String, JsonValue>,
    param_sources: BTreeMap<String, String>,
    preflight: Option<JsonValue>,
    effective_resources: BTreeMap<String, JsonValue>,
    enforcement: JsonValue,
    cache: Option<OperatorRunCacheMetadata>,
    run_context: Option<OperatorRunContext>,
    retry_state: OperatorRetryState,
) -> Result<OperatorRunResult, OperatorToolError> {
    let run_path = PathBuf::from(run_dir);
    let status_metadata = operator_run_status_metadata(
        resolved,
        run_id,
        "local",
        run_dir,
        run_context.clone(),
        retry_state.clone(),
    );
    update_local_status(&run_path, "created", None, Some(&status_metadata))?;
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
    update_local_status(&run_path, "running", None, Some(&status_metadata))?;

    let command = operator_execution_command(
        ctx,
        &resolved.spec,
        OperatorExecutionSurfaceKind::Local,
        run_dir,
        argv,
        &effective_inputs,
    );
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
        update_local_status(&run_path, "failed", Some(&error), Some(&status_metadata))?;
        return Err(error);
    }

    update_local_status(
        &run_path,
        "collecting_outputs",
        None,
        Some(&status_metadata),
    )?;
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

    let outputs = match collect_local_outputs(&resolved.spec, run_dir, &out) {
        Ok(outputs) => outputs,
        Err(error) => {
            let error = if error.run_dir.is_none() {
                error.with_run_dir(run_dir)
            } else {
                error
            };
            update_local_status(&run_path, "failed", Some(&error), Some(&status_metadata))?;
            return Err(error);
        }
    };
    let structured_outputs =
        match read_local_structured_outputs(&out, run_dir).and_then(|outputs| {
            validate_structured_outputs_against_manifest(outputs, &resolved.spec, run_dir)
        }) {
            Ok(outputs) => outputs,
            Err(error) => {
                let error = if error.run_dir.is_none() {
                    error.with_run_dir(run_dir)
                } else {
                    error
                };
                update_local_status(&run_path, "failed", Some(&error), Some(&status_metadata))?;
                return Err(error);
            }
        };
    update_local_status(&run_path, "exporting_results", None, Some(&status_metadata))?;
    let export_dir = match export_local_operator_results(ctx, resolved, run_id, &out) {
        Ok(path) => Some(path),
        Err(error) => {
            let error = if error.run_dir.is_none() {
                error.with_run_dir(run_dir)
            } else {
                error
            };
            update_local_status(&run_path, "failed", Some(&error), Some(&status_metadata))?;
            return Err(error);
        }
    };
    let markdown_report =
        operator_result_markdown_report(resolved, export_dir.as_deref(), &outputs);
    if let (Some(export_dir), Some(report)) = (export_dir.as_deref(), markdown_report.as_deref()) {
        if let Err(error) = write_local_operator_result_readme(export_dir, report) {
            let error = error.with_run_dir(run_dir);
            update_local_status(&run_path, "failed", Some(&error), Some(&status_metadata))?;
            return Err(error);
        }
    }
    let provenance_path = run_path.join("provenance.json");
    let result = OperatorRunResult {
        status: "succeeded".to_string(),
        run_id: run_id.to_string(),
        location: "local".to_string(),
        operator: status_metadata.operator.clone(),
        run_dir: run_dir.to_string(),
        run_context,
        provenance_path: Some(provenance_path.to_string_lossy().into_owned()),
        export_dir,
        markdown_report,
        outputs,
        structured_outputs,
        effective_inputs,
        input_fingerprints,
        effective_params,
        param_sources,
        preflight,
        effective_resources,
        attempt: retry_state.attempt,
        max_attempts: retry_state.max_attempts,
        previous_errors: retry_state.previous_errors,
        enforcement,
        cache,
        error: None,
    };
    write_json_file(&provenance_path, &result).map_err(|err| {
        OperatorToolError::new("provenance_write_failed", false, err).with_run_dir(run_dir)
    })?;
    update_local_status(&run_path, "succeeded", None, Some(&status_metadata))?;
    Ok(result)
}

async fn execute_in_environment(
    ctx: &crate::domain::tools::ToolContext,
    resolved: &ResolvedOperator,
    run_id: &str,
    surface: &OperatorExecutionSurface,
    argv: &[String],
    walltime_secs: u64,
    effective_inputs: BTreeMap<String, JsonValue>,
    input_fingerprints: BTreeMap<String, JsonValue>,
    effective_params: BTreeMap<String, JsonValue>,
    param_sources: BTreeMap<String, String>,
    preflight: Option<JsonValue>,
    effective_resources: BTreeMap<String, JsonValue>,
    enforcement: JsonValue,
    cache: Option<OperatorRunCacheMetadata>,
    run_context: Option<OperatorRunContext>,
    retry_state: OperatorRetryState,
) -> Result<OperatorRunResult, OperatorToolError> {
    let run_dir = surface.run_dir.as_str();
    let status_metadata = operator_run_status_metadata(
        resolved,
        run_id,
        surface.artifact_location(),
        run_dir,
        run_context.clone(),
        retry_state.clone(),
    );
    let mkdir = format!(
        "mkdir -p {}/work {}/out.tmp {}/logs",
        sh_quote(run_dir),
        sh_quote(run_dir),
        sh_quote(run_dir)
    );
    execute_env_command(ctx, &operator_environment_cwd(ctx), &mkdir, 30).await?;
    update_environment_status(ctx, run_dir, "created", None, Some(&status_metadata)).await?;
    update_environment_status(ctx, run_dir, "running", None, Some(&status_metadata)).await?;
    let staged_argv = stage_remote_plugin_files(ctx, &resolved.spec, run_dir, argv).await?;
    let command = operator_execution_command(
        ctx,
        &resolved.spec,
        surface.kind,
        run_dir,
        &staged_argv,
        &effective_inputs,
    );
    let result = execute_env_command(ctx, run_dir, &command, walltime_secs).await?;
    let stdout_tail = remote_tail(ctx, run_dir, "logs/stdout.txt").await;
    let stderr_tail = remote_tail(ctx, run_dir, "logs/stderr.txt").await;
    if result.returncode != 0 {
        let error = OperatorToolError::new(
            "tool_exit_nonzero",
            false,
            format!("Operator process exited with code {}.", result.returncode),
        )
        .with_run_dir(run_dir)
        .with_logs(stdout_tail, stderr_tail)
        .with_suggested_action(
            "Inspect the remote run logs, then adjust inputs or params and retry.",
        );
        update_environment_status(ctx, run_dir, "failed", Some(&error), Some(&status_metadata))
            .await?;
        return Err(error);
    }
    let publish = "rm -rf out && mv out.tmp out";
    execute_env_command(ctx, run_dir, publish, 30).await?;
    let outputs = match collect_environment_outputs(ctx, &resolved.spec, surface).await {
        Ok(outputs) => outputs,
        Err(error) => {
            let error = if error.run_dir.is_none() {
                error.with_run_dir(run_dir)
            } else {
                error
            };
            update_environment_status(ctx, run_dir, "failed", Some(&error), Some(&status_metadata))
                .await?;
            return Err(error);
        }
    };
    let structured_outputs = match read_environment_structured_outputs(ctx, run_dir)
        .await
        .and_then(|outputs| {
            validate_structured_outputs_against_manifest(outputs, &resolved.spec, run_dir)
        }) {
        Ok(outputs) => outputs,
        Err(error) => {
            let error = if error.run_dir.is_none() {
                error.with_run_dir(run_dir)
            } else {
                error
            };
            update_environment_status(ctx, run_dir, "failed", Some(&error), Some(&status_metadata))
                .await?;
            return Err(error);
        }
    };
    update_environment_status(
        ctx,
        run_dir,
        "exporting_results",
        None,
        Some(&status_metadata),
    )
    .await?;
    let export_dir =
        match export_environment_operator_results(ctx, resolved, run_id, &format!("{run_dir}/out"))
            .await
        {
            Ok(path) => Some(path),
            Err(error) => {
                let error = if error.run_dir.is_none() {
                    error.with_run_dir(run_dir)
                } else {
                    error
                };
                update_environment_status(
                    ctx,
                    run_dir,
                    "failed",
                    Some(&error),
                    Some(&status_metadata),
                )
                .await?;
                return Err(error);
            }
        };
    let markdown_report =
        operator_result_markdown_report(resolved, export_dir.as_deref(), &outputs);
    if let (Some(export_dir), Some(report)) = (export_dir.as_deref(), markdown_report.as_deref()) {
        if let Err(error) = write_environment_operator_result_readme(ctx, export_dir, report).await
        {
            let error = error.with_run_dir(run_dir);
            update_environment_status(ctx, run_dir, "failed", Some(&error), Some(&status_metadata))
                .await?;
            return Err(error);
        }
    }
    let result = OperatorRunResult {
        status: "succeeded".to_string(),
        run_id: run_id.to_string(),
        location: surface.artifact_location().to_string(),
        operator: status_metadata.operator.clone(),
        run_dir: run_dir.to_string(),
        run_context,
        provenance_path: Some(format!("{run_dir}/provenance.json")),
        export_dir,
        markdown_report,
        outputs,
        structured_outputs,
        effective_inputs,
        input_fingerprints,
        effective_params,
        param_sources,
        preflight,
        effective_resources,
        attempt: retry_state.attempt,
        max_attempts: retry_state.max_attempts,
        previous_errors: retry_state.previous_errors,
        enforcement,
        cache,
        error: None,
    };
    write_environment_json(ctx, run_dir, "provenance.json", &result).await?;
    update_environment_status(ctx, run_dir, "succeeded", None, Some(&status_metadata)).await?;
    Ok(result)
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
            execute_env_command(ctx, &operator_environment_cwd(ctx), &command, 30).await?;
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
    let command = if ctx.execution_environment == "local" {
        crate::domain::tools::bash::prepend_venv_activation(
            &ctx.local_venv_type,
            &ctx.local_venv_name,
            command,
        )
    } else {
        command.to_string()
    };
    let mut guard = env.lock().await;
    guard.execute(&command, exec_opts).await.map_err(|err| {
        OperatorToolError::new("execution_infra_error", true, err.to_string())
            .with_suggested_action("Retry if the execution backend was temporarily unavailable.")
    })
}

fn operator_environment_cwd(ctx: &crate::domain::tools::ToolContext) -> String {
    crate::domain::tools::env_store::remote_path(ctx, ".")
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

fn operator_execution_command(
    ctx: &crate::domain::tools::ToolContext,
    spec: &OperatorSpec,
    surface_kind: OperatorExecutionSurfaceKind,
    run_dir: &str,
    argv: &[String],
    inputs: &BTreeMap<String, JsonValue>,
) -> String {
    let Some(selection) = operator_container_for_command(ctx, spec, surface_kind) else {
        if let Some(command) =
            operator_conda_environment_command(ctx, spec, surface_kind, run_dir, argv)
        {
            return command;
        }
        if let Some(command) = operator_environment_ref_error_command(ctx, spec, surface_kind) {
            return command;
        }
        return command_with_log_capture(argv);
    };
    containerized_operator_command(ctx, spec, selection, surface_kind, run_dir, argv, inputs)
}

fn operator_container_for_command(
    ctx: &crate::domain::tools::ToolContext,
    spec: &OperatorSpec,
    surface_kind: OperatorExecutionSurfaceKind,
) -> Option<OperatorContainerSelection> {
    if surface_kind == OperatorExecutionSurfaceKind::Sandbox {
        return None;
    }
    spec.runtime
        .as_ref()
        .and_then(|runtime| selected_direct_container(ctx, runtime))
        .or_else(|| operator_container_from_environment_profile(ctx, spec, surface_kind))
}

fn operator_container_from_environment_profile(
    ctx: &crate::domain::tools::ToolContext,
    spec: &OperatorSpec,
    surface_kind: OperatorExecutionSurfaceKind,
) -> Option<OperatorContainerSelection> {
    operator_environment_container_selection(ctx, spec, surface_kind)
        .ok()
        .flatten()
}

fn operator_environment_container_selection(
    ctx: &crate::domain::tools::ToolContext,
    spec: &OperatorSpec,
    surface_kind: OperatorExecutionSurfaceKind,
) -> Result<Option<OperatorContainerSelection>, String> {
    let Some(profile) = operator_environment_profile(spec) else {
        return Ok(None);
    };
    let kind = profile
        .runtime
        .kind
        .as_deref()
        .unwrap_or("system")
        .trim()
        .to_ascii_lowercase();
    let Some(container_kind) = container_kind_from_name(&kind) else {
        return Ok(None);
    };
    if let Some(image) = operator_environment_profile_image(&profile) {
        return Ok(Some(OperatorContainerSelection {
            kind: container_kind,
            image,
            prepare: None,
        }));
    }
    if surface_kind != OperatorExecutionSurfaceKind::Local {
        return Err(format!(
            "Environment profile `{}` uses `{kind}` without runtime.image. File-based `{kind}` builds are only supported for local Operator runs; build the image on the target system and set runtime.image.",
            profile.canonical_id
        ));
    }
    match container_kind {
        OperatorContainerKind::Docker => {
            let dockerfile = operator_dockerfile_from_environment_profile(&profile)?;
            let context =
                operator_docker_build_context_from_environment_profile(&profile, &dockerfile);
            let dockerfile_bytes = fs::read(&dockerfile).map_err(|err| {
                format!(
                    "Read Dockerfile for environment profile `{}` at `{}`: {err}",
                    profile.canonical_id,
                    dockerfile.display()
                )
            })?;
            let env_hash = sha256_hex(&dockerfile_bytes);
            let tag = format!(
                "omiga-env-{}:{}",
                safe_operator_env_component(&profile.canonical_id).to_ascii_lowercase(),
                &env_hash[..12]
            );
            Ok(Some(OperatorContainerSelection {
                kind: OperatorContainerKind::Docker,
                image: tag.clone(),
                prepare: Some(OperatorContainerImagePrepare::Dockerfile {
                    dockerfile: dockerfile.to_string_lossy().into_owned(),
                    context: context.to_string_lossy().into_owned(),
                    tag,
                }),
            }))
        }
        OperatorContainerKind::Singularity => {
            let definition = operator_singularity_definition_from_environment_profile(&profile)?;
            let definition_bytes = fs::read(&definition).map_err(|err| {
                format!(
                    "Read Singularity definition for environment profile `{}` at `{}`: {err}",
                    profile.canonical_id,
                    definition.display()
                )
            })?;
            let env_hash = sha256_hex(&definition_bytes);
            let env_key = format!(
                "{}-{}",
                safe_operator_env_component(&profile.canonical_id),
                &env_hash[..12]
            );
            let sif = ctx
                .project_root
                .join(".omiga/operator-envs/singularity")
                .join(format!("{env_key}.sif"))
                .to_string_lossy()
                .into_owned();
            Ok(Some(OperatorContainerSelection {
                kind: OperatorContainerKind::Singularity,
                image: sif.clone(),
                prepare: Some(OperatorContainerImagePrepare::SingularityDefinition {
                    definition: definition.to_string_lossy().into_owned(),
                    sif,
                    hash: env_hash,
                }),
            }))
        }
    }
}

fn operator_environment_ref_error_command(
    ctx: &crate::domain::tools::ToolContext,
    spec: &OperatorSpec,
    surface_kind: OperatorExecutionSurfaceKind,
) -> Option<String> {
    if surface_kind == OperatorExecutionSurfaceKind::Sandbox {
        return None;
    }
    let env_ref = operator_runtime_env_ref(spec)?;
    let Some(profile) = operator_environment_profile(spec) else {
        return None;
    };
    let kind = profile
        .runtime
        .kind
        .as_deref()
        .unwrap_or("system")
        .trim()
        .to_ascii_lowercase();
    let message = if matches!(kind.as_str(), "docker" | "singularity") {
        match operator_environment_container_selection(ctx, spec, surface_kind) {
            Ok(Some(_)) | Ok(None) => return None,
            Err(message) => message,
        }
    } else if matches!(
        kind.as_str(),
        "system" | "local" | "host" | "conda" | "mamba" | "micromamba"
    ) {
        return None;
    } else {
        format!(
            "Operator environment envRef `{env_ref}` resolved to unsupported runtime.type `{kind}`. Supported environment runtimes are system/local/host, conda/mamba/micromamba, docker, and singularity."
        )
    };
    Some(command_with_log_capture(&[
        "/bin/sh".to_string(),
        "-lc".to_string(),
        format!("printf '%s\\n' {} >&2; exit 127", sh_quote(&message)),
    ]))
}

fn operator_environment_profile_image(
    profile: &crate::domain::environments::EnvironmentProfileSummary,
) -> Option<String> {
    profile
        .runtime
        .image
        .clone()
        .or_else(|| {
            profile
                .runtime
                .extra
                .get("image")
                .and_then(JsonValue::as_str)
                .map(str::to_string)
        })
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn operator_dockerfile_from_environment_profile(
    profile: &crate::domain::environments::EnvironmentProfileSummary,
) -> Result<PathBuf, String> {
    if let Some(raw) = profile_runtime_extra_str(profile, &["dockerfile", "dockerFile"]) {
        let path = operator_profile_relative_path(profile, raw)?;
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        if file_name != "Dockerfile" {
            return Err(format!(
                "Docker environment profile `{}` must use a standard `Dockerfile`; got `{}`.",
                profile.canonical_id,
                path.display()
            ));
        }
        if !path.is_file() {
            return Err(format!(
                "Docker environment profile `{}` declares Dockerfile `{}` but it does not exist.",
                profile.canonical_id,
                path.display()
            ));
        }
        return Ok(path);
    }
    let manifest_dir = operator_environment_manifest_dir(profile)?;
    let candidate = manifest_dir.join("Dockerfile");
    if candidate.is_file() {
        return Ok(candidate);
    }
    Err(format!(
        "Docker environment profile `{}` requires runtime.image or a standard `Dockerfile` next to environment.yaml.",
        profile.canonical_id
    ))
}

fn operator_docker_build_context_from_environment_profile(
    profile: &crate::domain::environments::EnvironmentProfileSummary,
    dockerfile: &Path,
) -> PathBuf {
    profile_runtime_extra_str(profile, &["context", "buildContext", "build_context"])
        .and_then(|raw| operator_profile_relative_path(profile, raw).ok())
        .unwrap_or_else(|| {
            dockerfile
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| PathBuf::from("."))
        })
}

fn operator_singularity_definition_from_environment_profile(
    profile: &crate::domain::environments::EnvironmentProfileSummary,
) -> Result<PathBuf, String> {
    if let Some(raw) = profile_runtime_extra_str(
        profile,
        &[
            "definitionFile",
            "definition_file",
            "singularityDef",
            "singularity_def",
        ],
    ) {
        let path = operator_profile_relative_path(profile, raw)?;
        if path.extension().and_then(|ext| ext.to_str()) != Some("def") {
            return Err(format!(
                "Singularity environment profile `{}` must use a `.def` definition file; got `{}`.",
                profile.canonical_id,
                path.display()
            ));
        }
        if !path.is_file() {
            return Err(format!(
                "Singularity environment profile `{}` declares definition file `{}` but it does not exist.",
                profile.canonical_id,
                path.display()
            ));
        }
        return Ok(path);
    }
    let manifest_dir = operator_environment_manifest_dir(profile)?;
    let candidate = manifest_dir.join("singularity.def");
    if candidate.is_file() {
        return Ok(candidate);
    }
    Err(format!(
        "Singularity environment profile `{}` requires runtime.image or a standard `singularity.def` next to environment.yaml.",
        profile.canonical_id
    ))
}

fn profile_runtime_extra_str<'a>(
    profile: &'a crate::domain::environments::EnvironmentProfileSummary,
    keys: &[&str],
) -> Option<&'a str> {
    keys.iter().find_map(|key| {
        profile
            .runtime
            .extra
            .get(*key)
            .and_then(JsonValue::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
    })
}

fn operator_profile_relative_path(
    profile: &crate::domain::environments::EnvironmentProfileSummary,
    raw: &str,
) -> Result<PathBuf, String> {
    let path = PathBuf::from(raw);
    if path.is_absolute() {
        return Ok(path);
    }
    Ok(operator_environment_manifest_dir(profile)?.join(path))
}

fn operator_environment_manifest_dir(
    profile: &crate::domain::environments::EnvironmentProfileSummary,
) -> Result<PathBuf, String> {
    let manifest = PathBuf::from(&profile.manifest_path);
    manifest.parent().map(Path::to_path_buf).ok_or_else(|| {
        format!(
            "Environment profile `{}` has no manifest parent directory.",
            profile.canonical_id
        )
    })
}

fn operator_conda_environment_command(
    ctx: &crate::domain::tools::ToolContext,
    spec: &OperatorSpec,
    surface_kind: OperatorExecutionSurfaceKind,
    run_dir: &str,
    argv: &[String],
) -> Option<String> {
    if surface_kind == OperatorExecutionSurfaceKind::Sandbox {
        return None;
    }
    let Some(env_ref) = operator_runtime_env_ref(spec) else {
        return None;
    };
    let Some(profile) = operator_environment_profile(spec) else {
        return Some(command_with_log_capture(&[
            "/bin/sh".to_string(),
            "-lc".to_string(),
            format!(
                "printf '%s\\n' {} >&2; exit 127",
                sh_quote(&format!(
                    "Operator environment envRef `{env_ref}` did not resolve for plugin `{}`.",
                    spec.source.source_plugin
                ))
            ),
        ]));
    };
    let kind = profile
        .runtime
        .kind
        .as_deref()
        .unwrap_or("system")
        .trim()
        .to_ascii_lowercase();
    if !matches!(kind.as_str(), "conda" | "mamba" | "micromamba") {
        return None;
    }
    match operator_conda_environment_selection(ctx, &profile, surface_kind) {
        Ok(selection) => {
            let shell_script =
                conda_environment_shell_script(&selection, run_dir, &shell_join(argv));
            Some(command_with_log_capture(&[
                "/bin/sh".to_string(),
                "-lc".to_string(),
                shell_script,
            ]))
        }
        Err(message) => Some(command_with_log_capture(&[
            "/bin/sh".to_string(),
            "-lc".to_string(),
            format!("printf '%s\\n' {} >&2; exit 127", sh_quote(&message)),
        ])),
    }
}

#[derive(Debug, Clone)]
struct OperatorCondaEnvironmentSelection {
    env_prefix: String,
    env_yaml_b64: String,
    env_hash: String,
    env_vars: BTreeMap<String, String>,
}

fn operator_conda_environment_selection(
    ctx: &crate::domain::tools::ToolContext,
    profile: &crate::domain::environments::EnvironmentProfileSummary,
    surface_kind: OperatorExecutionSurfaceKind,
) -> Result<OperatorCondaEnvironmentSelection, String> {
    let conda_file = operator_conda_environment_file(profile)?;
    let bytes = fs::read(&conda_file).map_err(|err| {
        format!(
            "Read conda environment file `{}`: {err}",
            conda_file.display()
        )
    })?;
    let env_hash = sha256_hex(&bytes);
    let env_key = format!(
        "{}-{}",
        safe_operator_env_component(&profile.canonical_id),
        &env_hash[..12]
    );
    let env_prefix = operator_conda_env_prefix(ctx, surface_kind, &env_key);
    use base64::{engine::general_purpose, Engine as _};
    Ok(OperatorCondaEnvironmentSelection {
        env_prefix,
        env_yaml_b64: general_purpose::STANDARD.encode(bytes),
        env_hash,
        env_vars: profile.runtime.env.clone(),
    })
}

fn operator_conda_environment_file(
    profile: &crate::domain::environments::EnvironmentProfileSummary,
) -> Result<PathBuf, String> {
    for key in [
        "condaEnvFile",
        "conda_env_file",
        "condaFile",
        "conda_file",
        "environmentFile",
        "environment_file",
    ] {
        if let Some(raw) = profile_runtime_extra_str(profile, &[key]) {
            let path = operator_profile_relative_path(profile, raw)?;
            validate_conda_environment_yaml_path(profile, &path)?;
            if !path.is_file() {
                return Err(format!(
                    "Environment profile `{}` declares conda YAML file `{}` but it does not exist.",
                    profile.canonical_id,
                    path.display()
                ));
            }
            return Ok(path);
        }
    }
    let manifest_dir = operator_environment_manifest_dir(profile)?;
    for name in ["conda.yaml", "conda.yml"] {
        let candidate = manifest_dir.join(name);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    Err(format!(
        "Environment profile `{}` does not declare or contain a standard conda YAML file. Use `runtime.condaEnvFile: ./conda.yaml` or `./conda.yml`.",
        profile.canonical_id
    ))
}

fn validate_conda_environment_yaml_path(
    profile: &crate::domain::environments::EnvironmentProfileSummary,
    path: &Path,
) -> Result<(), String> {
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase());
    if !matches!(extension.as_deref(), Some("yaml" | "yml")) {
        return Err(format!(
            "Conda/mamba environment profile `{}` must use a `.yaml` or `.yml` file; got `{}`.",
            profile.canonical_id,
            path.display()
        ));
    }
    Ok(())
}

fn operator_conda_env_prefix(
    ctx: &crate::domain::tools::ToolContext,
    surface_kind: OperatorExecutionSurfaceKind,
    env_key: &str,
) -> String {
    let rel = format!(".omiga/operator-envs/conda/{env_key}");
    if surface_kind == OperatorExecutionSurfaceKind::Local {
        return ctx.project_root.join(rel).to_string_lossy().into_owned();
    }
    crate::domain::tools::env_store::remote_path(ctx, &rel)
}

fn conda_environment_shell_script(
    selection: &OperatorCondaEnvironmentSelection,
    run_dir: &str,
    inner_command: &str,
) -> String {
    let env_yaml = format!("{run_dir}/env/conda-environment.yaml");
    let exports = shell_export_lines(&selection.env_vars);
    format!(
        r#"set -e
OMIGA_CONDA_PREFIX={env_prefix}
OMIGA_CONDA_YAML={env_yaml}
OMIGA_CONDA_HASH={env_hash}
OMIGA_MICROMAMBA="${{OMIGA_MICROMAMBA:-$HOME/.omiga/bin/micromamba}}"
mkdir -p "$(dirname "$OMIGA_CONDA_YAML")" "$(dirname "$OMIGA_CONDA_PREFIX")"
printf %s {env_yaml_b64} | python3 -c 'import base64,sys; sys.stdout.buffer.write(base64.b64decode(sys.stdin.read()))' > "$OMIGA_CONDA_YAML"
omiga_find_conda_manager() {{
  OMIGA_CONDA_MANAGER_KIND=
  OMIGA_CONDA_BIN=
  if [ -n "${{OMIGA_MICROMAMBA:-}}" ] && [ -x "$OMIGA_MICROMAMBA" ]; then
    OMIGA_CONDA_MANAGER_KIND=micromamba
    OMIGA_CONDA_BIN=$OMIGA_MICROMAMBA
    return 0
  fi
  if command -v micromamba >/dev/null 2>&1; then
    OMIGA_CONDA_MANAGER_KIND=micromamba
    OMIGA_CONDA_BIN=$(command -v micromamba)
    return 0
  fi
  if command -v mamba >/dev/null 2>&1; then
    OMIGA_CONDA_MANAGER_KIND=mamba
    OMIGA_CONDA_BIN=$(command -v mamba)
    return 0
  fi
  if command -v conda >/dev/null 2>&1; then
    OMIGA_CONDA_MANAGER_KIND=conda
    OMIGA_CONDA_BIN=$(command -v conda)
    return 0
  fi
  return 1
}}
omiga_missing_conda_manager() {{
  cat >&2 <<'OMIGA_CONDA_HINT'
No micromamba, mamba, or conda executable was found in the active PATH/base environment/virtual environment.
Recommended: install the official micromamba binary at $HOME/.omiga/bin/micromamba, or set OMIGA_MICROMAMBA=/absolute/path/to/micromamba.
Then rerun the Operator; Omiga will create and reuse the isolated env from conda.yaml/conda.yml under .omiga/operator-envs/conda.
OMIGA_CONDA_HINT
  exit 127
}}
omiga_find_conda_manager || true
if [ ! -f "$OMIGA_CONDA_PREFIX/.omiga-env-hash" ] || [ "$(cat "$OMIGA_CONDA_PREFIX/.omiga-env-hash" 2>/dev/null || true)" != "$OMIGA_CONDA_HASH" ]; then
  if [ -z "$OMIGA_CONDA_BIN" ]; then
    omiga_missing_conda_manager
  fi
  rm -rf "$OMIGA_CONDA_PREFIX"
  case "$OMIGA_CONDA_MANAGER_KIND" in
    micromamba)
      "$OMIGA_CONDA_BIN" create -y -p "$OMIGA_CONDA_PREFIX" -f "$OMIGA_CONDA_YAML"
      ;;
    mamba)
      "$OMIGA_CONDA_BIN" env create -y -p "$OMIGA_CONDA_PREFIX" -f "$OMIGA_CONDA_YAML" || "$OMIGA_CONDA_BIN" create -y -p "$OMIGA_CONDA_PREFIX" -f "$OMIGA_CONDA_YAML"
      ;;
    conda)
      "$OMIGA_CONDA_BIN" env create -y -p "$OMIGA_CONDA_PREFIX" -f "$OMIGA_CONDA_YAML" || "$OMIGA_CONDA_BIN" create -y -p "$OMIGA_CONDA_PREFIX" -f "$OMIGA_CONDA_YAML"
      ;;
  esac
  printf '%s' "$OMIGA_CONDA_HASH" > "$OMIGA_CONDA_PREFIX/.omiga-env-hash"
fi
{exports}
case "${{OMIGA_CONDA_MANAGER_KIND:-}}" in
  micromamba)
    "$OMIGA_CONDA_BIN" run -p "$OMIGA_CONDA_PREFIX" /bin/sh -lc {inner}
    ;;
  mamba)
    "$OMIGA_CONDA_BIN" run -p "$OMIGA_CONDA_PREFIX" /bin/sh -lc {inner}
    ;;
  conda)
    "$OMIGA_CONDA_BIN" run -p "$OMIGA_CONDA_PREFIX" /bin/sh -lc {inner}
    ;;
  *)
    PATH="$OMIGA_CONDA_PREFIX/bin:$PATH" /bin/sh -lc {inner}
    ;;
esac"#,
        env_prefix = sh_quote(&selection.env_prefix),
        env_yaml = sh_quote(&env_yaml),
        env_hash = sh_quote(&selection.env_hash),
        env_yaml_b64 = sh_quote(&selection.env_yaml_b64),
        exports = exports,
        inner = sh_quote(inner_command),
    )
}

fn shell_export_lines(env: &BTreeMap<String, String>) -> String {
    env.iter()
        .filter(|(key, _)| is_safe_shell_identifier(key))
        .map(|(key, value)| format!("export {key}={}", sh_quote(value)))
        .collect::<Vec<_>>()
        .join("\n")
}

fn is_safe_shell_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first == '_' || first.is_ascii_alphabetic())
        && chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

fn operator_runtime_env_ref(spec: &OperatorSpec) -> Option<&str> {
    let runtime = spec.runtime.as_ref()?;
    runtime
        .get("envRef")
        .or_else(|| runtime.get("env_ref"))
        .and_then(JsonValue::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn operator_environment_profile(
    spec: &OperatorSpec,
) -> Option<crate::domain::environments::EnvironmentProfileSummary> {
    let env_ref = operator_runtime_env_ref(spec)?;
    let resolved = crate::domain::environments::resolve_environment_ref(
        Some(env_ref),
        &spec.source.source_plugin,
        &spec.source.plugin_root,
    );
    resolved.profile
}

fn safe_operator_env_component(value: &str) -> String {
    let mut out = String::new();
    for ch in value.trim().chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.') {
            out.push(ch);
        } else {
            out.push('-');
        }
    }
    let out = out.trim_matches('-').trim_matches('.');
    if out.is_empty() {
        "environment".to_string()
    } else {
        out.to_string()
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(bytes);
    format!("{digest:x}")
}

fn containerized_operator_command(
    ctx: &crate::domain::tools::ToolContext,
    spec: &OperatorSpec,
    selection: OperatorContainerSelection,
    surface_kind: OperatorExecutionSurfaceKind,
    run_dir: &str,
    argv: &[String],
    inputs: &BTreeMap<String, JsonValue>,
) -> String {
    let inner = command_with_log_capture(argv);
    let mounts = operator_container_mounts(ctx, spec, surface_kind, run_dir, inputs);
    let kind = selection.kind;
    let container_command = match selection.kind {
        OperatorContainerKind::Docker => {
            let mut tokens = vec!["docker".to_string(), "run".to_string(), "--rm".to_string()];
            for mount in mounts {
                tokens.push("-v".to_string());
                tokens.push(container_bind_spec(&mount.path, mount.writable));
            }
            tokens.extend([
                "-w".to_string(),
                run_dir.to_string(),
                selection.image.clone(),
                "/bin/sh".to_string(),
                "-lc".to_string(),
                inner,
            ]);
            shell_join(&tokens)
        }
        OperatorContainerKind::Singularity => {
            let mut tokens = vec![
                "singularity".to_string(),
                "exec".to_string(),
                "--cleanenv".to_string(),
                "--pwd".to_string(),
                run_dir.to_string(),
            ];
            for mount in mounts {
                tokens.push("--bind".to_string());
                tokens.push(container_bind_spec(&mount.path, mount.writable));
            }
            tokens.extend([
                selection.image.clone(),
                "/bin/sh".to_string(),
                "-lc".to_string(),
                inner,
            ]);
            shell_join(&tokens)
        }
    };
    container_runtime_shell_script(kind, selection.prepare.as_ref(), &container_command)
}

fn container_runtime_shell_script(
    kind: OperatorContainerKind,
    prepare: Option<&OperatorContainerImagePrepare>,
    container_command: &str,
) -> String {
    let preflight = container_runtime_preflight_script(kind);
    let prepare = prepare
        .map(container_runtime_prepare_script)
        .unwrap_or_default();
    format!(
        r#"set -e
mkdir -p logs
omiga_container_runtime_missing() {{
  message=$1
  printf '%s\n' "$message" >&2
  printf '%s\n' "$message" >> logs/stderr.txt
  printf '\n__OMIGA_OPERATOR_EXIT_CODE=127\n'
  exit 127
}}
{preflight}
{prepare}
set +e
{container_command} >> logs/stdout.txt 2>> logs/stderr.txt
code=$?
printf '\n__OMIGA_OPERATOR_EXIT_CODE=%s\n' "$code"
exit "$code""#
    )
}

fn container_runtime_preflight_script(kind: OperatorContainerKind) -> &'static str {
    match kind {
        OperatorContainerKind::Docker => {
            r#"if ! command -v docker >/dev/null 2>&1; then
  omiga_container_runtime_missing 'Docker runtime is required for this Operator environment but `docker` was not found in the active PATH/base environment/virtual environment. Install Docker Desktop/Engine, make the `docker` CLI available, and retry.'
fi
if ! docker version >/dev/null 2>&1; then
  omiga_container_runtime_missing 'Docker CLI was found, but `docker version` failed. Start Docker Desktop/daemon or fix Docker permissions, then retry.'
fi"#
        }
        OperatorContainerKind::Singularity => {
            r#"if command -v singularity >/dev/null 2>&1; then
  :
elif command -v apptainer >/dev/null 2>&1; then
  singularity() { apptainer "$@"; }
else
  omiga_container_runtime_missing 'Singularity/Apptainer runtime is required for this Operator environment but neither `singularity` nor `apptainer` was found in the active PATH/base environment/virtual environment. Install SingularityCE or Apptainer and retry.'
fi"#
        }
    }
}

fn container_runtime_prepare_script(prepare: &OperatorContainerImagePrepare) -> String {
    match prepare {
        OperatorContainerImagePrepare::Dockerfile {
            dockerfile,
            context,
            tag,
        } => format!(
            r#"OMIGA_DOCKERFILE={dockerfile}
OMIGA_DOCKER_CONTEXT={context}
OMIGA_DOCKER_IMAGE={tag}
if ! docker image inspect "$OMIGA_DOCKER_IMAGE" >/dev/null 2>&1; then
  docker build -t "$OMIGA_DOCKER_IMAGE" -f "$OMIGA_DOCKERFILE" "$OMIGA_DOCKER_CONTEXT" >> logs/stdout.txt 2>> logs/stderr.txt
fi"#,
            dockerfile = sh_quote(dockerfile),
            context = sh_quote(context),
            tag = sh_quote(tag),
        ),
        OperatorContainerImagePrepare::SingularityDefinition {
            definition,
            sif,
            hash,
        } => format!(
            r#"OMIGA_SINGULARITY_DEF={definition}
OMIGA_SINGULARITY_SIF={sif}
OMIGA_SINGULARITY_HASH={hash}
mkdir -p "$(dirname "$OMIGA_SINGULARITY_SIF")"
if [ ! -f "$OMIGA_SINGULARITY_SIF" ] || [ "$(cat "$OMIGA_SINGULARITY_SIF.omiga-env-hash" 2>/dev/null || true)" != "$OMIGA_SINGULARITY_HASH" ]; then
  rm -f "$OMIGA_SINGULARITY_SIF.tmp"
  singularity build "$OMIGA_SINGULARITY_SIF.tmp" "$OMIGA_SINGULARITY_DEF" >> logs/stdout.txt 2>> logs/stderr.txt
  mv "$OMIGA_SINGULARITY_SIF.tmp" "$OMIGA_SINGULARITY_SIF"
  printf '%s' "$OMIGA_SINGULARITY_HASH" > "$OMIGA_SINGULARITY_SIF.omiga-env-hash"
fi"#,
            definition = sh_quote(definition),
            sif = sh_quote(sif),
            hash = sh_quote(hash),
        ),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct OperatorContainerMount {
    path: String,
    writable: bool,
}

fn operator_container_mounts(
    ctx: &crate::domain::tools::ToolContext,
    spec: &OperatorSpec,
    surface_kind: OperatorExecutionSurfaceKind,
    run_dir: &str,
    inputs: &BTreeMap<String, JsonValue>,
) -> Vec<OperatorContainerMount> {
    let mut mounts = BTreeMap::<String, bool>::new();
    insert_container_mount(&mut mounts, run_dir, true);

    if surface_kind == OperatorExecutionSurfaceKind::Local {
        insert_container_mount(&mut mounts, &ctx.project_root.to_string_lossy(), false);
        insert_container_mount(
            &mut mounts,
            &spec.source.plugin_root.to_string_lossy(),
            false,
        );
    }

    for path in path_like_input_values(spec, inputs) {
        insert_container_mount(&mut mounts, &path, false);
    }

    mounts
        .into_iter()
        .map(|(path, writable)| OperatorContainerMount { path, writable })
        .collect()
}

fn insert_container_mount(mounts: &mut BTreeMap<String, bool>, path: &str, writable: bool) {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return;
    }
    let entry = mounts.entry(trimmed.to_string()).or_insert(false);
    *entry = *entry || writable;
}

fn path_like_input_values(
    spec: &OperatorSpec,
    inputs: &BTreeMap<String, JsonValue>,
) -> Vec<String> {
    let mut paths = BTreeSet::new();
    for (name, field) in &spec.interface.inputs {
        if !field.kind.is_path_like() {
            continue;
        }
        let Some(value) = inputs.get(name) else {
            continue;
        };
        if field.kind.is_array() {
            if let Some(items) = value.as_array() {
                for item in items {
                    if let Some(path) = item.as_str() {
                        paths.insert(path.to_string());
                    }
                }
            }
        } else if let Some(path) = value.as_str() {
            paths.insert(path.to_string());
        }
    }
    paths.into_iter().collect()
}

fn container_bind_spec(path: &str, writable: bool) -> String {
    if writable {
        format!("{path}:{path}")
    } else {
        format!("{path}:{path}:ro")
    }
}

fn shell_join(tokens: &[String]) -> String {
    tokens
        .iter()
        .map(|token| sh_quote(token))
        .collect::<Vec<_>>()
        .join(" ")
}

fn sh_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn validate_output_glob_pattern<'a>(
    name: &str,
    pattern: &'a str,
) -> Result<std::borrow::Cow<'a, str>, OperatorToolError> {
    let trimmed = pattern.trim();
    if trimmed.is_empty() {
        return Err(OperatorToolError::new(
            "output_validation_failed",
            false,
            format!("Output `{name}` glob must not be empty."),
        )
        .with_field(format!("outputs.{name}"))
        .with_suggested_action("Declare a relative glob under the operator `${outdir}`."));
    }
    let path = Path::new(trimmed);
    let escapes_outdir = path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        });
    if escapes_outdir {
        return Err(OperatorToolError::new(
            "output_validation_failed",
            false,
            format!(
                "Output `{name}` glob `{trimmed}` must stay relative to the operator `${{outdir}}`."
            ),
        )
        .with_field(format!("outputs.{name}"))
        .with_suggested_action("Remove absolute paths and `..` components from the output glob."));
    }

    let mut normalized = trimmed;
    while let Some(rest) = normalized.strip_prefix("./") {
        normalized = rest;
    }
    while let Some(rest) = normalized.strip_prefix('/') {
        normalized = rest;
    }
    if normalized.is_empty() {
        return Err(OperatorToolError::new(
            "output_validation_failed",
            false,
            format!("Output `{name}` glob must name files under `${{outdir}}`."),
        )
        .with_field(format!("outputs.{name}"))
        .with_suggested_action("Declare a file glob such as `*.txt` or `reports/*.html`."));
    }
    if normalized.len() == trimmed.len() {
        Ok(std::borrow::Cow::Borrowed(trimmed))
    } else {
        Ok(std::borrow::Cow::Owned(normalized.to_string()))
    }
}

fn collect_local_outputs(
    spec: &OperatorSpec,
    run_dir: &str,
    out_dir: &Path,
) -> Result<BTreeMap<String, Vec<ArtifactRef>>, OperatorToolError> {
    let canonical_run_dir = Path::new(run_dir).canonicalize().map_err(|err| {
        OperatorToolError::new(
            "output_validation_failed",
            false,
            format!("resolve operator run dir `{run_dir}`: {err}"),
        )
        .with_run_dir(run_dir)
    })?;
    let canonical_out_dir = out_dir.canonicalize().map_err(|err| {
        OperatorToolError::new(
            "output_validation_failed",
            false,
            format!("resolve operator output dir {}: {err}", out_dir.display()),
        )
        .with_run_dir(run_dir)
    })?;
    if !canonical_out_dir.starts_with(&canonical_run_dir) {
        return Err(OperatorToolError::new(
            "output_validation_failed",
            false,
            "Operator output directory escaped the active session run workspace.",
        )
        .with_run_dir(run_dir)
        .with_suggested_action("Write results only under `${outdir}` for this operator run."));
    }
    let mut outputs = BTreeMap::new();
    for (name, field) in &spec.interface.outputs {
        let Some(pattern) = field.glob.as_deref() else {
            outputs.insert(name.clone(), Vec::new());
            continue;
        };
        let pattern = validate_output_glob_pattern(name, pattern)?.into_owned();
        let search = out_dir.join(&pattern).to_string_lossy().into_owned();
        let mut artifacts = Vec::new();
        for entry in glob::glob(&search).map_err(|err| {
            OperatorToolError::new("artifact_collection_failed", false, err.to_string())
        })? {
            let path = entry.map_err(|err| {
                OperatorToolError::new("artifact_collection_failed", false, err.to_string())
            })?;
            if path.is_file() {
                let canonical_path = path.canonicalize().map_err(|err| {
                    OperatorToolError::new(
                        "artifact_collection_failed",
                        false,
                        format!("resolve output artifact {}: {err}", path.display()),
                    )
                    .with_run_dir(run_dir)
                })?;
                if !canonical_path.starts_with(&canonical_out_dir) {
                    return Err(OperatorToolError::new(
                        "output_validation_failed",
                        false,
                        format!(
                            "Output `{name}` matched artifact outside the active session outdir: {}",
                            path.display()
                        ),
                    )
                    .with_field(format!("outputs.{name}"))
                    .with_run_dir(run_dir)
                    .with_suggested_action(
                        "Write result artifacts only under `${outdir}` for this operator run.",
                    ));
                }
                let size = canonical_path.metadata().ok().map(|m| m.len());
                artifacts.push(ArtifactRef {
                    location: "local".to_string(),
                    server: None,
                    path: canonical_path.to_string_lossy().into_owned(),
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

async fn collect_environment_outputs(
    ctx: &crate::domain::tools::ToolContext,
    spec: &OperatorSpec,
    surface: &OperatorExecutionSurface,
) -> Result<BTreeMap<String, Vec<ArtifactRef>>, OperatorToolError> {
    let run_dir = surface.run_dir.as_str();
    let mut outputs = BTreeMap::new();
    for (name, field) in &spec.interface.outputs {
        let pattern =
            validate_output_glob_pattern(name, field.glob.as_deref().unwrap_or("*"))?.into_owned();
        let command = format!(
            "find out -type f -path {} -print",
            sh_quote(&format!("out/{pattern}"))
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
                location: surface.artifact_location().to_string(),
                server: (surface.kind == OperatorExecutionSurfaceKind::Ssh)
                    .then(|| ctx.ssh_server.clone())
                    .flatten(),
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

fn safe_operator_export_component(value: &str, fallback: &str) -> String {
    let mut out = value
        .trim()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    out = out
        .trim_matches(|ch| matches!(ch, '.' | '_' | '-' | ' '))
        .to_string();
    if out.is_empty() {
        fallback.to_string()
    } else {
        out
    }
}

fn operator_export_relative_path(resolved: &ResolvedOperator, run_id: &str) -> String {
    let alias = safe_operator_export_component(&resolved.alias, "operator");
    let run = safe_operator_export_component(run_id, "run");
    format!("operator-results/{alias}/{run}")
}

fn local_operator_export_dir(
    ctx: &crate::domain::tools::ToolContext,
    resolved: &ResolvedOperator,
    run_id: &str,
) -> PathBuf {
    ctx.project_root
        .join(operator_export_relative_path(resolved, run_id))
}

fn environment_operator_export_dir(
    ctx: &crate::domain::tools::ToolContext,
    resolved: &ResolvedOperator,
    run_id: &str,
) -> String {
    crate::domain::tools::env_store::remote_path(
        ctx,
        &operator_export_relative_path(resolved, run_id),
    )
}

fn copy_dir_contents(source_dir: &Path, target_dir: &Path) -> Result<(), String> {
    if !source_dir.is_dir() {
        return Err(format!(
            "operator output directory {} does not exist",
            source_dir.display()
        ));
    }
    if target_dir.exists() {
        fs::remove_dir_all(target_dir).map_err(|err| {
            format!(
                "remove previous exported results {}: {err}",
                target_dir.display()
            )
        })?;
    }
    fs::create_dir_all(target_dir).map_err(|err| {
        format!(
            "create exported results dir {}: {err}",
            target_dir.display()
        )
    })?;
    for entry in fs::read_dir(source_dir).map_err(|err| {
        format!(
            "read operator output directory {}: {err}",
            source_dir.display()
        )
    })? {
        let entry = entry.map_err(|err| format!("read operator output entry: {err}"))?;
        let source = entry.path();
        let target = target_dir.join(entry.file_name());
        copy_path_recursively(&source, &target)?;
    }
    Ok(())
}

fn copy_path_recursively(source: &Path, target: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(source)
        .map_err(|err| format!("read metadata for {}: {err}", source.display()))?;
    if metadata.is_dir() {
        fs::create_dir_all(target)
            .map_err(|err| format!("create exported subdir {}: {err}", target.display()))?;
        for entry in fs::read_dir(source)
            .map_err(|err| format!("read output subdir {}: {err}", source.display()))?
        {
            let entry = entry.map_err(|err| format!("read output subdir entry: {err}"))?;
            copy_path_recursively(&entry.path(), &target.join(entry.file_name()))?;
        }
        return Ok(());
    }
    if metadata.is_file() || metadata.file_type().is_symlink() {
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)
                .map_err(|err| format!("create exported parent {}: {err}", parent.display()))?;
        }
        fs::copy(source, target).map_err(|err| {
            format!(
                "copy operator result {} to {}: {err}",
                source.display(),
                target.display()
            )
        })?;
    }
    Ok(())
}

fn export_local_operator_results(
    ctx: &crate::domain::tools::ToolContext,
    resolved: &ResolvedOperator,
    run_id: &str,
    source_out_dir: &Path,
) -> Result<String, OperatorToolError> {
    let export_dir = local_operator_export_dir(ctx, resolved, run_id);
    copy_dir_contents(source_out_dir, &export_dir).map_err(|err| {
        OperatorToolError::new("result_export_failed", false, err)
            .with_suggested_action("Check session workspace write permissions and retry.")
    })?;
    Ok(export_dir.to_string_lossy().into_owned())
}

async fn export_environment_operator_results(
    ctx: &crate::domain::tools::ToolContext,
    resolved: &ResolvedOperator,
    run_id: &str,
    source_out_dir: &str,
) -> Result<String, OperatorToolError> {
    let export_dir = environment_operator_export_dir(ctx, resolved, run_id);
    let command = format!(
        "if [ ! -d {} ]; then echo 'operator output directory missing' >&2; exit 2; fi\nrm -rf {}\nmkdir -p {}\ncp -R {}/. {}/",
        sh_quote(source_out_dir),
        sh_quote(&export_dir),
        sh_quote(&export_dir),
        sh_quote(source_out_dir),
        sh_quote(&export_dir),
    );
    let result = execute_env_command(ctx, &operator_environment_cwd(ctx), &command, 60).await?;
    if result.returncode != 0 {
        return Err(OperatorToolError::new(
            "result_export_failed",
            false,
            format!(
                "copy operator results to session workspace failed with exit code {}.",
                result.returncode
            ),
        )
        .with_suggested_action("Check session workspace write permissions and retry."));
    }
    Ok(export_dir)
}

fn operator_result_markdown_report(
    resolved: &ResolvedOperator,
    export_dir: Option<&str>,
    outputs: &BTreeMap<String, Vec<ArtifactRef>>,
) -> Option<String> {
    let export_dir = export_dir?.trim();
    if export_dir.is_empty() {
        return None;
    }
    let title = resolved
        .spec
        .metadata
        .name
        .as_deref()
        .filter(|name| !name.trim().is_empty())
        .unwrap_or(resolved.alias.as_str());
    let mut lines = vec![
        format!("# {title}"),
        String::new(),
        "Generated artifacts are exported together in this folder so the final reply can reference static files directly instead of embedding JSON, HTML, or base64 payloads.".to_string(),
        "Use the PNG Markdown image below in the final reply; Omiga renders it through the chat image component. Keep the full path inside `<...>` and do not shorten it to `figure.png`. PNG exports are generated at 300 DPI minimum.".to_string(),
        String::new(),
    ];

    if let Some(path) =
        first_exported_artifact_path(export_dir, outputs, &["figure_png", "plot_png", "png"])
    {
        lines.push(format!("![{title}](<{path}>)"));
        lines.push(String::new());
    }

    let mut primary_links = Vec::new();
    if let Some(path) =
        first_exported_artifact_path(export_dir, outputs, &["figure_pdf", "plot_pdf", "pdf"])
    {
        primary_links.push(format!("[PDF](<{path}>)"));
    }
    if let Some(path) =
        first_exported_artifact_path(export_dir, outputs, &["plot_script", "script", "r_script"])
    {
        primary_links.push(format!("[plot-script.R](<{path}>)"));
    }
    if let Some(path) =
        first_exported_artifact_path(export_dir, outputs, &["rerun_script", "rerun"])
    {
        primary_links.push(format!("[rerun.sh](<{path}>)"));
    }
    primary_links.push(format!("[Result folder](<{export_dir}>)"));
    lines.push(format!("Primary files: {}", primary_links.join(" · ")));
    lines.push(String::new());
    lines.push("## Files".to_string());

    let mut seen = std::collections::BTreeSet::new();
    for (name, artifacts) in outputs {
        for artifact in artifacts {
            let path = exported_artifact_path(export_dir, &artifact.path);
            if !seen.insert(path.clone()) {
                continue;
            }
            let size = artifact
                .size
                .map(|value| format!(" — {} bytes", value))
                .unwrap_or_default();
            lines.push(format!(
                "- `{name}`: [{file}](<{path}>){size}",
                file = exported_artifact_label(&path)
            ));
        }
    }
    if seen.is_empty() {
        lines.push("- No declared output artifacts were exported.".to_string());
    }
    Some(lines.join("\n"))
}

fn first_exported_artifact_path(
    export_dir: &str,
    outputs: &BTreeMap<String, Vec<ArtifactRef>>,
    preferred_output_names: &[&str],
) -> Option<String> {
    for preferred in preferred_output_names {
        if let Some(path) = outputs
            .get(*preferred)
            .and_then(|artifacts| artifacts.first())
            .map(|artifact| exported_artifact_path(export_dir, &artifact.path))
        {
            return Some(path);
        }
    }
    for (name, artifacts) in outputs {
        let lower_name = name.to_ascii_lowercase();
        if preferred_output_names
            .iter()
            .any(|preferred| lower_name.contains(preferred.trim_matches('_')))
        {
            if let Some(path) = artifacts
                .first()
                .map(|artifact| exported_artifact_path(export_dir, &artifact.path))
            {
                return Some(path);
            }
        }
    }
    None
}

fn exported_artifact_path(export_dir: &str, source_path: &str) -> String {
    let file = exported_artifact_label(source_path);
    if export_dir.ends_with('/') || export_dir.ends_with('\\') {
        format!("{export_dir}{file}")
    } else if export_dir.contains('\\') && !export_dir.contains('/') {
        format!("{export_dir}\\{file}")
    } else {
        format!("{export_dir}/{file}")
    }
}

fn exported_artifact_label(path: &str) -> String {
    Path::new(path)
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(path)
        .to_string()
}

fn write_local_operator_result_readme(
    export_dir: &str,
    markdown: &str,
) -> Result<(), OperatorToolError> {
    fs::write(Path::new(export_dir).join("README.md"), markdown).map_err(|err| {
        OperatorToolError::new(
            "result_export_failed",
            false,
            format!("write result README.md: {err}"),
        )
        .with_suggested_action("Check session workspace write permissions and retry.")
    })
}

async fn write_environment_operator_result_readme(
    ctx: &crate::domain::tools::ToolContext,
    export_dir: &str,
    markdown: &str,
) -> Result<(), OperatorToolError> {
    use base64::{engine::general_purpose, Engine as _};
    let encoded = general_purpose::STANDARD.encode(markdown.as_bytes());
    let target = format!("{}/README.md", export_dir.trim_end_matches('/'));
    let command = format!(
        "mkdir -p {} && printf %s {} | base64 -d > {}",
        sh_quote(export_dir),
        sh_quote(&encoded),
        sh_quote(&target),
    );
    execute_env_command(ctx, &operator_environment_cwd(ctx), &command, 30)
        .await
        .map(|_| ())
        .map_err(|err| {
            OperatorToolError::new("result_export_failed", true, err.message)
                .with_suggested_action("Check session workspace write permissions and retry.")
        })
}

fn read_local_structured_outputs(
    out_dir: &Path,
    run_dir: &str,
) -> Result<Option<JsonValue>, OperatorToolError> {
    let target = out_dir.join(OPERATOR_STRUCTURED_OUTPUTS_FILE);
    if !target.exists() {
        return Ok(None);
    }
    let canonical_run_dir = Path::new(run_dir).canonicalize().map_err(|err| {
        OperatorToolError::new(
            "output_validation_failed",
            false,
            format!("resolve operator run dir `{run_dir}`: {err}"),
        )
        .with_run_dir(run_dir)
    })?;
    let canonical_out_dir = out_dir.canonicalize().map_err(|err| {
        OperatorToolError::new(
            "output_validation_failed",
            false,
            format!("resolve operator output dir {}: {err}", out_dir.display()),
        )
        .with_run_dir(run_dir)
    })?;
    let canonical_target = target.canonicalize().map_err(|err| {
        OperatorToolError::new(
            "output_validation_failed",
            false,
            format!(
                "resolve structured output manifest {}: {err}",
                target.display()
            ),
        )
        .with_field("structuredOutputs")
        .with_run_dir(run_dir)
    })?;
    if !canonical_out_dir.starts_with(&canonical_run_dir)
        || !canonical_target.starts_with(&canonical_out_dir)
    {
        return Err(OperatorToolError::new(
            "output_validation_failed",
            false,
            "Structured output manifest must stay under the active session outdir.",
        )
        .with_field("structuredOutputs")
        .with_run_dir(run_dir)
        .with_suggested_action("Write structured metadata only to `${outdir}/outputs.json`."));
    }
    let metadata = canonical_target.metadata().map_err(|err| {
        OperatorToolError::new(
            "output_validation_failed",
            false,
            format!(
                "read structured output manifest metadata {}: {err}",
                canonical_target.display()
            ),
        )
        .with_field("structuredOutputs")
        .with_run_dir(run_dir)
    })?;
    if !metadata.is_file() {
        return Err(OperatorToolError::new(
            "output_validation_failed",
            false,
            "Structured output manifest must be a regular JSON file.",
        )
        .with_field("structuredOutputs")
        .with_run_dir(run_dir)
        .with_suggested_action("Write a JSON object to `${outdir}/outputs.json`."));
    }
    if metadata.len() > OPERATOR_STRUCTURED_OUTPUTS_MAX_BYTES {
        return Err(OperatorToolError::new(
            "output_validation_failed",
            false,
            format!(
                "Structured output manifest exceeds {} bytes.",
                OPERATOR_STRUCTURED_OUTPUTS_MAX_BYTES
            ),
        )
        .with_field("structuredOutputs")
        .with_run_dir(run_dir)
        .with_suggested_action(
            "Keep `${outdir}/outputs.json` small and put large payloads in declared output artifacts.",
        ));
    }
    let raw = fs::read_to_string(&canonical_target).map_err(|err| {
        OperatorToolError::new(
            "output_validation_failed",
            false,
            format!(
                "read structured output manifest {}: {err}",
                target.display()
            ),
        )
        .with_field("structuredOutputs")
        .with_run_dir(run_dir)
    })?;
    let value = serde_json::from_str::<JsonValue>(&raw).map_err(|err| {
        OperatorToolError::new(
            "output_validation_failed",
            false,
            format!(
                "parse structured output manifest {}: {err}",
                target.display()
            ),
        )
        .with_field("structuredOutputs")
        .with_run_dir(run_dir)
        .with_suggested_action("Write valid JSON object metadata to `${outdir}/outputs.json`.")
    })?;
    validate_structured_outputs_shape(value, run_dir).map(Some)
}

async fn read_environment_structured_outputs(
    ctx: &crate::domain::tools::ToolContext,
    run_dir: &str,
) -> Result<Option<JsonValue>, OperatorToolError> {
    let missing = "__OMIGA_OPERATOR_STRUCTURED_OUTPUTS_MISSING__";
    let prefix = "__OMIGA_OPERATOR_STRUCTURED_OUTPUTS_JSON__";
    let escaped = "__OMIGA_OPERATOR_STRUCTURED_OUTPUTS_ESCAPED__";
    let not_file = "__OMIGA_OPERATOR_STRUCTURED_OUTPUTS_NOT_FILE__";
    let too_large = "__OMIGA_OPERATOR_STRUCTURED_OUTPUTS_TOO_LARGE__";
    let bad_size = "__OMIGA_OPERATOR_STRUCTURED_OUTPUTS_BAD_SIZE__";
    let command = format!(
        r#"target={target}
if [ ! -e "$target" ]; then printf %s {missing}; exit 0; fi
if [ ! -f "$target" ]; then printf %s {not_file}; exit 0; fi
out_root=$(cd out 2>/dev/null && pwd -P) || exit 65
resolved=$(readlink -f "$target" 2>/dev/null || realpath "$target" 2>/dev/null || printf '')
case "$resolved" in "$out_root"/*) ;; *) printf %s {escaped}; exit 0 ;; esac
size=$(wc -c < "$target" | tr -d '[:space:]')
case "$size" in ''|*[!0-9]*) printf %s {bad_size}; exit 0 ;; esac
if [ "$size" -gt {max_bytes} ]; then printf %s {too_large}; exit 0; fi
printf '%s\n' {prefix}
cat "$target""#,
        target = sh_quote(&format!("out/{OPERATOR_STRUCTURED_OUTPUTS_FILE}")),
        missing = sh_quote(missing),
        not_file = sh_quote(not_file),
        escaped = sh_quote(escaped),
        bad_size = sh_quote(bad_size),
        too_large = sh_quote(too_large),
        max_bytes = OPERATOR_STRUCTURED_OUTPUTS_MAX_BYTES,
        prefix = sh_quote(prefix),
    );
    let result = execute_env_command(ctx, run_dir, &command, 30).await?;
    if result.returncode != 0 {
        return Err(OperatorToolError::new(
            "output_validation_failed",
            false,
            format!(
                "Structured output manifest validation exited with code {}.",
                result.returncode
            ),
        )
        .with_field("structuredOutputs")
        .with_run_dir(run_dir)
        .with_suggested_action("Check `${outdir}/outputs.json` and retry."));
    }
    let trimmed = result.output.trim();
    match trimmed {
        value if value == missing => return Ok(None),
        value if value == not_file => {
            return Err(OperatorToolError::new(
                "output_validation_failed",
                false,
                "Structured output manifest must be a regular JSON file.",
            )
            .with_field("structuredOutputs")
            .with_run_dir(run_dir)
            .with_suggested_action("Write a JSON object to `${outdir}/outputs.json`."))
        }
        value if value == escaped => {
            return Err(OperatorToolError::new(
                "output_validation_failed",
                false,
                "Structured output manifest must stay under the active session outdir.",
            )
            .with_field("structuredOutputs")
            .with_run_dir(run_dir)
            .with_suggested_action(
                "Write structured metadata only to `${outdir}/outputs.json`.",
            ))
        }
        value if value == too_large => {
            return Err(OperatorToolError::new(
                "output_validation_failed",
                false,
                format!(
                    "Structured output manifest exceeds {} bytes.",
                    OPERATOR_STRUCTURED_OUTPUTS_MAX_BYTES
                ),
            )
            .with_field("structuredOutputs")
            .with_run_dir(run_dir)
            .with_suggested_action(
                "Keep `${outdir}/outputs.json` small and put large payloads in declared output artifacts.",
            ))
        }
        value if value == bad_size => {
            return Err(OperatorToolError::new(
                "output_validation_failed",
                false,
                "Structured output manifest size could not be validated.",
            )
            .with_field("structuredOutputs")
            .with_run_dir(run_dir)
            .with_suggested_action("Check `${outdir}/outputs.json` and retry."))
        }
        _ => {}
    }
    let Some(raw) = result.output.strip_prefix(prefix) else {
        return Err(OperatorToolError::new(
            "output_validation_failed",
            false,
            "Structured output manifest reader returned an unexpected payload.",
        )
        .with_field("structuredOutputs")
        .with_run_dir(run_dir)
        .with_suggested_action("Check `${outdir}/outputs.json` and retry."));
    };
    let raw = raw
        .strip_prefix("\r\n")
        .or_else(|| raw.strip_prefix('\n'))
        .unwrap_or(raw);
    let value = serde_json::from_str::<JsonValue>(raw).map_err(|err| {
        OperatorToolError::new(
            "output_validation_failed",
            false,
            format!(
                "parse structured output manifest out/{OPERATOR_STRUCTURED_OUTPUTS_FILE}: {err}"
            ),
        )
        .with_field("structuredOutputs")
        .with_run_dir(run_dir)
        .with_suggested_action("Write valid JSON object metadata to `${outdir}/outputs.json`.")
    })?;
    validate_structured_outputs_shape(value, run_dir).map(Some)
}

fn validate_structured_outputs_shape(
    value: JsonValue,
    run_dir: &str,
) -> Result<JsonValue, OperatorToolError> {
    if value.is_object() {
        return Ok(value);
    }
    Err(OperatorToolError::new(
        "output_validation_failed",
        false,
        "Structured output manifest must contain a JSON object.",
    )
    .with_field("structuredOutputs")
    .with_run_dir(run_dir)
    .with_suggested_action("Write object-shaped metadata to `${outdir}/outputs.json`."))
}

fn validate_structured_outputs_against_manifest(
    value: Option<JsonValue>,
    spec: &OperatorSpec,
    run_dir: &str,
) -> Result<Option<JsonValue>, OperatorToolError> {
    let Some(value) = value else {
        if let Some((name, _field)) = spec
            .interface
            .outputs
            .iter()
            .find(|(_name, field)| is_structured_output_field(field) && field.required)
        {
            return Err(OperatorToolError::new(
                "output_validation_failed",
                false,
                format!(
                    "Required structured output `{name}` is missing because `${{outdir}}/{OPERATOR_STRUCTURED_OUTPUTS_FILE}` was not written."
                ),
            )
            .with_field(format!("structuredOutputs.{name}"))
            .with_run_dir(run_dir)
            .with_suggested_action(
                "Write a JSON object to `${outdir}/outputs.json` with all required structured output fields.",
            ));
        }
        return Ok(None);
    };
    let Some(object) = value.as_object() else {
        return validate_structured_outputs_shape(value, run_dir).map(Some);
    };
    for (name, field) in &spec.interface.outputs {
        if !is_structured_output_field(field) {
            continue;
        }
        match object.get(name) {
            Some(field_value) => {
                validate_field_value("structuredOutputs", name, field, field_value).map_err(
                    |error| {
                        if error.run_dir.is_none() {
                            error.with_run_dir(run_dir)
                        } else {
                            error
                        }
                    },
                )?
            }
            None if field.required => {
                return Err(OperatorToolError::new(
                    "output_validation_failed",
                    false,
                    format!("Required structured output `{name}` is missing."),
                )
                .with_field(format!("structuredOutputs.{name}"))
                .with_run_dir(run_dir)
                .with_suggested_action(
                    "Write all required structured output fields in `${outdir}/outputs.json`.",
                ))
            }
            None => {}
        }
    }
    Ok(Some(value))
}

fn is_structured_output_field(field: &OperatorFieldSpec) -> bool {
    field.glob.is_none() && !field.kind.is_path_like()
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

async fn read_environment_json(
    ctx: &crate::domain::tools::ToolContext,
    run_dir: &str,
    rel: &str,
) -> Result<Option<JsonValue>, OperatorToolError> {
    let target = format!("{}/{}", run_dir.trim_end_matches('/'), rel);
    let command = format!(
        "if [ -f {} ]; then cat {}; else printf %s __OMIGA_OPERATOR_MISSING__; fi",
        sh_quote(&target),
        sh_quote(&target),
    );
    let result = execute_env_command(ctx, &operator_environment_cwd(ctx), &command, 30).await?;
    if result.output.trim() == "__OMIGA_OPERATOR_MISSING__" {
        return Ok(None);
    }
    serde_json::from_str::<JsonValue>(&result.output)
        .map(Some)
        .map_err(|err| {
            OperatorToolError::new(
                "run_state_read_failed",
                true,
                format!("parse remote run JSON {target}: {err}"),
            )
            .with_run_dir(run_dir)
        })
}

async fn read_environment_text_tail(
    ctx: &crate::domain::tools::ToolContext,
    run_dir: &str,
    rel: &str,
    limit_bytes: u64,
) -> Result<Option<String>, OperatorToolError> {
    let target = format!("{}/{}", run_dir.trim_end_matches('/'), rel);
    let command = format!(
        "if [ -f {} ]; then tail -c {} {}; else printf %s __OMIGA_OPERATOR_MISSING__; fi",
        sh_quote(&target),
        limit_bytes,
        sh_quote(&target),
    );
    let result = execute_env_command(ctx, &operator_environment_cwd(ctx), &command, 30).await?;
    if result.output.trim() == "__OMIGA_OPERATOR_MISSING__" {
        Ok(None)
    } else {
        Ok(Some(result.output))
    }
}

async fn update_environment_status(
    ctx: &crate::domain::tools::ToolContext,
    run_dir: &str,
    status: &str,
    error: Option<&OperatorToolError>,
    metadata: Option<&OperatorRunStatusMetadata>,
) -> Result<(), OperatorToolError> {
    let mut value = json!({
        "status": status,
        "updatedAt": chrono::Utc::now().to_rfc3339(),
        "error": error,
    });
    apply_status_metadata(&mut value, metadata);
    write_environment_json(ctx, run_dir, "status.json", &value).await
}

async fn write_environment_json(
    ctx: &crate::domain::tools::ToolContext,
    run_dir: &str,
    rel: &str,
    value: &impl Serialize,
) -> Result<(), OperatorToolError> {
    let raw = serde_json::to_vec_pretty(value).map_err(|err| {
        OperatorToolError::new("provenance_write_failed", false, err.to_string())
            .with_run_dir(run_dir)
    })?;
    use base64::{engine::general_purpose, Engine as _};
    let encoded = general_purpose::STANDARD.encode(raw);
    let target = format!("{}/{}", run_dir.trim_end_matches('/'), rel);
    let command = format!(
        "mkdir -p {} && printf %s {} | base64 -d > {}",
        sh_quote(run_dir),
        sh_quote(&encoded),
        sh_quote(&target),
    );
    execute_env_command(ctx, &operator_environment_cwd(ctx), &command, 30)
        .await
        .map(|_| ())
        .map_err(|err| {
            OperatorToolError::new("provenance_write_failed", true, err.message)
                .with_run_dir(run_dir)
        })
}

fn update_local_status(
    run_path: &Path,
    status: &str,
    error: Option<&OperatorToolError>,
    metadata: Option<&OperatorRunStatusMetadata>,
) -> Result<(), OperatorToolError> {
    fs::create_dir_all(run_path).map_err(|err| {
        OperatorToolError::new("execution_infra_error", true, err.to_string())
            .with_run_dir(run_path.to_string_lossy())
    })?;
    let mut value = json!({
        "status": status,
        "updatedAt": chrono::Utc::now().to_rfc3339(),
        "error": error,
    });
    apply_status_metadata(&mut value, metadata);
    write_json_file(&run_path.join("status.json"), &value).map_err(|err| {
        OperatorToolError::new("provenance_write_failed", false, err)
            .with_run_dir(run_path.to_string_lossy())
    })
}

fn apply_status_metadata(value: &mut JsonValue, metadata: Option<&OperatorRunStatusMetadata>) {
    let Some(metadata) = metadata else {
        return;
    };
    if let Some(object) = value.as_object_mut() {
        object.insert(
            "runId".to_string(),
            JsonValue::String(metadata.run_id.clone()),
        );
        object.insert(
            "location".to_string(),
            JsonValue::String(metadata.location.clone()),
        );
        object.insert(
            "runDir".to_string(),
            JsonValue::String(metadata.run_dir.clone()),
        );
        object.insert(
            "operator".to_string(),
            serde_json::to_value(&metadata.operator).unwrap_or(JsonValue::Null),
        );
        if let Some(run_context) = &metadata.run_context {
            object.insert(
                "runContext".to_string(),
                serde_json::to_value(run_context).unwrap_or(JsonValue::Null),
            );
        }
        if let Some(retry) = &metadata.retry {
            object.insert("attempt".to_string(), json!(retry.attempt));
            object.insert("maxAttempts".to_string(), json!(retry.max_attempts));
            if !retry.previous_errors.is_empty() {
                object.insert(
                    "previousErrors".to_string(),
                    serde_json::to_value(&retry.previous_errors).unwrap_or(JsonValue::Null),
                );
            }
        }
    }
}

fn write_json_file(path: &Path, value: &impl Serialize) -> Result<(), String> {
    let raw = serde_json::to_string_pretty(value).map_err(|err| err.to_string())?;
    fs::write(path, format!("{raw}\n")).map_err(|err| err.to_string())
}

pub fn list_local_operator_runs(project_root: &Path, limit: usize) -> Vec<OperatorRunSummary> {
    let runs_root = operator_runs_root(project_root);
    let Ok(entries) = fs::read_dir(&runs_root) else {
        return Vec::new();
    };
    let mut summaries = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false))
        .filter_map(|entry| {
            let run_id = entry.file_name().to_string_lossy().into_owned();
            if !is_safe_operator_run_id(&run_id) {
                return None;
            }
            summarize_local_operator_run_dir(&entry.path(), &run_id)
        })
        .collect::<Vec<_>>();
    summaries.sort_by(|left, right| {
        right
            .summary
            .updated_at
            .cmp(&left.summary.updated_at)
            .then_with(|| right.sort_key.cmp(&left.sort_key))
            .then_with(|| right.summary.run_id.cmp(&left.summary.run_id))
    });
    summaries
        .into_iter()
        .take(limit)
        .map(|item| item.summary)
        .collect()
}

pub async fn list_operator_runs_for_context(
    ctx: &crate::domain::tools::ToolContext,
    limit: usize,
) -> Result<Vec<OperatorRunSummary>, String> {
    let surface = OperatorExecutionSurface::for_runs_root(ctx);
    if surface.kind == OperatorExecutionSurfaceKind::Local {
        return Ok(list_local_operator_runs(&ctx.project_root, limit));
    }

    let command = format!(
        "if [ -d {} ]; then find {} -mindepth 1 -maxdepth 1 -type d -name 'oprun_*' -print; fi",
        sh_quote(&surface.run_dir),
        sh_quote(&surface.run_dir)
    );
    let result = execute_env_command(ctx, &operator_environment_cwd(ctx), &command, 30)
        .await
        .map_err(|err| err.message)?;
    let mut summaries = Vec::new();
    for run_dir in result
        .output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        let run_id = run_dir.rsplit('/').next().unwrap_or(run_dir);
        if !is_safe_operator_run_id(run_id) {
            continue;
        }
        let provenance = read_environment_json(ctx, run_dir, "provenance.json")
            .await
            .map_err(|err| err.message)?;
        let status_doc = read_environment_json(ctx, run_dir, "status.json")
            .await
            .map_err(|err| err.message)?;
        let updated_at = status_doc
            .as_ref()
            .and_then(|value| json_string_at(value, &["updatedAt"]))
            .or_else(|| {
                provenance
                    .as_ref()
                    .and_then(|value| json_string_at(value, &["updatedAt"]))
            });
        let sort_key = rfc3339_sort_key(updated_at.as_deref());
        if let Some(summary) = summarize_operator_run_documents(
            run_id,
            surface.artifact_location(),
            run_dir.to_string(),
            Some(format!("{}/provenance.json", run_dir.trim_end_matches('/'))),
            provenance,
            status_doc,
            updated_at,
            sort_key,
        ) {
            summaries.push(summary);
        }
    }
    summaries.sort_by(|left, right| {
        right
            .summary
            .updated_at
            .cmp(&left.summary.updated_at)
            .then_with(|| right.sort_key.cmp(&left.sort_key))
            .then_with(|| right.summary.run_id.cmp(&left.summary.run_id))
    });
    Ok(summaries
        .into_iter()
        .take(limit)
        .map(|item| item.summary)
        .collect())
}

pub fn read_local_operator_run(project_root: &Path, run_id: &str) -> Result<JsonValue, String> {
    let run_id = run_id.trim();
    if !is_safe_operator_run_id(run_id) {
        return Err(
            "operator run id must start with `oprun_` and contain only letters, numbers, `_`, or `-`"
                .to_string(),
        );
    }
    let runs_root = operator_runs_root(project_root);
    let run_dir = runs_root.join(run_id);
    let canonical_root = runs_root.canonicalize().unwrap_or(runs_root);
    let canonical_run_dir = run_dir
        .canonicalize()
        .map_err(|err| format!("operator run `{run_id}` not found: {err}"))?;
    if !canonical_run_dir.starts_with(&canonical_root) {
        return Err(format!(
            "operator run `{run_id}` is outside the run registry"
        ));
    }
    let provenance = canonical_run_dir.join("provenance.json");
    if provenance.is_file() {
        return read_json_value(&provenance);
    }
    let status = canonical_run_dir.join("status.json");
    if status.is_file() {
        return read_json_value(&status);
    }
    Err(format!(
        "operator run `{run_id}` has no provenance.json or status.json"
    ))
}

pub async fn read_operator_run_for_context(
    ctx: &crate::domain::tools::ToolContext,
    run_id: &str,
) -> Result<OperatorRunDetail, String> {
    let run_id = run_id.trim();
    if !is_safe_operator_run_id(run_id) {
        return Err(
            "operator run id must start with `oprun_` and contain only letters, numbers, `_`, or `-`"
                .to_string(),
        );
    }
    let surface = OperatorExecutionSurface::for_context(ctx, run_id);
    if surface.kind == OperatorExecutionSurfaceKind::Local {
        let document = read_local_operator_run(&ctx.project_root, run_id)?;
        let source_path = document
            .get("provenancePath")
            .and_then(JsonValue::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| {
                operator_run_dir(&ctx.project_root, run_id)
                    .join("status.json")
                    .to_string_lossy()
                    .into_owned()
            });
        return Ok(OperatorRunDetail {
            run_id: run_id.to_string(),
            location: "local".to_string(),
            run_dir: surface.run_dir,
            source_path,
            document,
        });
    }

    let run_dir = surface.run_dir.clone();
    for rel in ["provenance.json", "status.json"] {
        if let Some(document) = read_environment_json(ctx, &run_dir, rel)
            .await
            .map_err(|err| err.message)?
        {
            return Ok(OperatorRunDetail {
                run_id: run_id.to_string(),
                location: surface.artifact_location().to_string(),
                run_dir,
                source_path: format!("{}/{}", surface.run_dir.trim_end_matches('/'), rel),
                document,
            });
        }
    }
    Err(format!(
        "operator run `{run_id}` has no remote provenance.json or status.json at {}",
        surface.run_dir
    ))
}

pub async fn read_operator_run_log_for_context(
    ctx: &crate::domain::tools::ToolContext,
    run_id: &str,
    log_name: &str,
    limit_bytes: u64,
) -> Result<OperatorRunLog, String> {
    let run_id = run_id.trim();
    if !is_safe_operator_run_id(run_id) {
        return Err(
            "operator run id must start with `oprun_` and contain only letters, numbers, `_`, or `-`"
                .to_string(),
        );
    }
    let normalized_log = match log_name.trim() {
        "stdout" | "stdout.txt" => "stdout",
        "stderr" | "stderr.txt" => "stderr",
        other => return Err(format!("unsupported operator log `{other}`")),
    };
    let rel = format!("logs/{normalized_log}.txt");
    let surface = OperatorExecutionSurface::for_context(ctx, run_id);
    let limit = limit_bytes.clamp(1, 64 * 1024);
    let path = format!("{}/{}", surface.run_dir.trim_end_matches('/'), rel);
    let content = if surface.kind == OperatorExecutionSurfaceKind::Local {
        read_tail_limited(&path, limit as usize).ok_or_else(|| {
            format!("operator run `{run_id}` has no local `{normalized_log}` log at {path}")
        })?
    } else {
        read_environment_text_tail(ctx, &surface.run_dir, &rel, limit)
            .await
            .map_err(|err| err.message)?
            .ok_or_else(|| {
                format!("operator run `{run_id}` has no remote `{normalized_log}` log at {path}")
            })?
    };
    Ok(OperatorRunLog {
        run_id: run_id.to_string(),
        location: surface.artifact_location().to_string(),
        log_name: normalized_log.to_string(),
        path,
        content,
    })
}

pub async fn verify_operator_run_for_context(
    ctx: &crate::domain::tools::ToolContext,
    run_id: &str,
) -> Result<OperatorRunVerification, String> {
    let detail = read_operator_run_for_context(ctx, run_id).await?;
    let mut checks = Vec::new();
    checks.push(OperatorRunCheck {
        name: "run_state_readable".to_string(),
        ok: true,
        severity: "info".to_string(),
        message: "Run status/provenance is readable.".to_string(),
        path: Some(detail.source_path.clone()),
    });

    let status =
        json_string_at(&detail.document, &["status"]).unwrap_or_else(|| "unknown".to_string());
    let status_ok = status == "succeeded";
    checks.push(OperatorRunCheck {
        name: "run_status".to_string(),
        ok: status_ok,
        severity: if status_ok { "info" } else { "error" }.to_string(),
        message: if status_ok {
            "Run status is succeeded.".to_string()
        } else {
            format!("Run status is `{status}`.")
        },
        path: Some(detail.source_path.clone()),
    });

    for log_name in ["stdout", "stderr"] {
        match read_operator_run_log_for_context(ctx, run_id, log_name, 256).await {
            Ok(log) => checks.push(OperatorRunCheck {
                name: format!("{log_name}_log_readable"),
                ok: true,
                severity: "info".to_string(),
                message: format!("{} log is readable.", log_name),
                path: Some(log.path),
            }),
            Err(error) => checks.push(OperatorRunCheck {
                name: format!("{log_name}_log_readable"),
                ok: false,
                severity: "warning".to_string(),
                message: error,
                path: Some(format!(
                    "{}/logs/{log_name}.txt",
                    detail.run_dir.trim_end_matches('/')
                )),
            }),
        }
    }

    let artifacts = output_artifact_paths(&detail.document);
    if artifacts.is_empty() {
        checks.push(OperatorRunCheck {
            name: "output_artifacts_declared".to_string(),
            ok: true,
            severity: "info".to_string(),
            message: "No output artifact refs were declared in this run.".to_string(),
            path: None,
        });
    } else {
        for (output_name, path) in artifacts {
            let check =
                verify_artifact_path_for_context(ctx, &detail.location, &output_name, &path).await;
            checks.push(check);
        }
    }

    let ok = checks
        .iter()
        .filter(|check| check.severity == "error")
        .all(|check| check.ok);
    Ok(OperatorRunVerification {
        run_id: detail.run_id,
        location: detail.location,
        run_dir: detail.run_dir,
        ok,
        checks,
    })
}

pub async fn cleanup_operator_runs_for_context(
    ctx: &crate::domain::tools::ToolContext,
    request: OperatorRunCleanupRequest,
) -> Result<OperatorRunCleanupResult, String> {
    let limit = request.limit.unwrap_or(500).clamp(1, 2_000);
    let surface = OperatorExecutionSurface::for_runs_root(ctx);
    let summaries = list_operator_runs_for_context(ctx, limit).await?;
    let selected = select_operator_cleanup_candidates(&summaries, &request);
    let mut candidates = Vec::new();
    let mut estimated_total = 0_u64;

    for (summary, reason) in selected {
        let estimated_bytes = if surface.kind == OperatorExecutionSurfaceKind::Local {
            Some(local_operator_run_dir_size(&ctx.project_root, &summary.run_id).unwrap_or(0))
        } else {
            estimate_environment_run_dir_size(ctx, &summary.run_dir).await
        };
        if let Some(bytes) = estimated_bytes {
            estimated_total = estimated_total.saturating_add(bytes);
        }
        let mut candidate = OperatorRunCleanupCandidate {
            run_id: summary.run_id.clone(),
            status: summary.status.clone(),
            location: summary.location.clone(),
            run_dir: summary.run_dir.clone(),
            updated_at: summary.updated_at.clone(),
            cache_hit: summary.cache_hit,
            cache_source_run_id: summary.cache_source_run_id.clone(),
            output_count: summary.output_count,
            reason,
            estimated_bytes,
            deleted: false,
            error: None,
        };
        if !request.dry_run {
            let deletion = if surface.kind == OperatorExecutionSurfaceKind::Local {
                delete_local_operator_run_dir(&ctx.project_root, &summary.run_id)
            } else {
                delete_environment_operator_run_dir(ctx, &surface.run_dir, &summary.run_dir).await
            };
            match deletion {
                Ok(()) => candidate.deleted = true,
                Err(error) => candidate.error = Some(error),
            }
        }
        candidates.push(candidate);
    }

    let deleted_count = candidates
        .iter()
        .filter(|candidate| candidate.deleted)
        .count();
    let skipped_count = candidates
        .iter()
        .filter(|candidate| candidate.error.is_some())
        .count();
    Ok(OperatorRunCleanupResult {
        dry_run: request.dry_run,
        location: surface.artifact_location().to_string(),
        runs_root: surface.run_dir,
        scanned_count: summaries.len(),
        matched_count: candidates.len(),
        deleted_count,
        skipped_count,
        estimated_bytes: Some(estimated_total),
        candidates,
    })
}

fn select_operator_cleanup_candidates(
    summaries: &[OperatorRunSummary],
    request: &OperatorRunCleanupRequest,
) -> Vec<(OperatorRunSummary, String)> {
    let keep_latest = request.keep_latest.unwrap_or(25);
    let scoped = summaries
        .iter()
        .filter(|summary| cleanup_request_matches_summary(summary, request))
        .collect::<Vec<_>>();
    let protected = scoped
        .iter()
        .take(keep_latest)
        .map(|summary| summary.run_id.as_str())
        .collect::<HashSet<_>>();
    let mut selected = Vec::new();
    let mut selected_ids = HashSet::new();
    for summary in &scoped {
        if protected.contains(summary.run_id.as_str())
            || !is_terminal_operator_status(&summary.status)
        {
            continue;
        }
        let reason = if request.include_cache_hits && summary.cache_hit == Some(true) {
            Some("cache_hit_record".to_string())
        } else if request.include_failed
            && is_failed_operator_status(&summary.status)
            && run_matches_cleanup_age(summary, request.max_age_days)
        {
            Some("old_failed_run".to_string())
        } else if request.include_succeeded
            && is_succeeded_operator_status(&summary.status)
            && run_matches_cleanup_age(summary, request.max_age_days)
        {
            Some("old_succeeded_run".to_string())
        } else {
            None
        };
        if let Some(reason) = reason {
            selected_ids.insert(summary.run_id.clone());
            selected.push(((*summary).clone(), reason));
        }
    }

    if request.include_cache_hits {
        let selected_sources = selected
            .iter()
            .filter(|(summary, _)| summary.cache_hit != Some(true))
            .map(|(summary, _)| summary.run_id.clone())
            .collect::<HashSet<_>>();
        for summary in &scoped {
            if protected.contains(summary.run_id.as_str())
                || selected_ids.contains(&summary.run_id)
                || summary.cache_hit != Some(true)
            {
                continue;
            }
            if summary
                .cache_source_run_id
                .as_ref()
                .map(|source| selected_sources.contains(source))
                .unwrap_or(false)
            {
                selected_ids.insert(summary.run_id.clone());
                selected.push(((*summary).clone(), "cache_source_cleanup".to_string()));
            }
        }
    }

    let retained_cache_sources = scoped
        .iter()
        .filter(|summary| {
            summary.cache_hit == Some(true) && !selected_ids.contains(&summary.run_id)
        })
        .filter_map(|summary| summary.cache_source_run_id.clone())
        .collect::<HashSet<_>>();
    selected
        .into_iter()
        .filter(|(summary, _)| {
            summary.cache_hit == Some(true) || !retained_cache_sources.contains(&summary.run_id)
        })
        .collect()
}

fn cleanup_request_matches_summary(
    summary: &OperatorRunSummary,
    request: &OperatorRunCleanupRequest,
) -> bool {
    cleanup_text_filter_matches(
        request.operator_id.as_deref(),
        summary.operator_id.as_deref(),
    ) && cleanup_operator_alias_matches(
        request.operator_alias.as_deref(),
        summary.operator_alias.as_deref(),
        summary.operator_id.as_deref(),
    ) && cleanup_text_filter_matches(
        request.operator_version.as_deref(),
        summary.operator_version.as_deref(),
    ) && cleanup_text_filter_matches(
        request.source_plugin.as_deref(),
        summary.source_plugin.as_deref(),
    )
}

fn cleanup_text_filter_matches(filter: Option<&str>, value: Option<&str>) -> bool {
    let Some(filter) = filter.map(str::trim).filter(|value| !value.is_empty()) else {
        return true;
    };
    value.map(str::trim) == Some(filter)
}

fn cleanup_operator_alias_matches(
    filter: Option<&str>,
    alias: Option<&str>,
    operator_id: Option<&str>,
) -> bool {
    let Some(filter) = filter.map(str::trim).filter(|value| !value.is_empty()) else {
        return true;
    };
    alias.map(str::trim) == Some(filter) || operator_id.map(str::trim) == Some(filter)
}

fn is_terminal_operator_status(status: &str) -> bool {
    let status = status.trim().to_ascii_lowercase();
    matches!(
        status.as_str(),
        "succeeded" | "success" | "failed" | "error" | "cancelled" | "timeout" | "timed_out"
    )
}

fn is_failed_operator_status(status: &str) -> bool {
    let status = status.trim().to_ascii_lowercase();
    matches!(
        status.as_str(),
        "failed" | "error" | "cancelled" | "timeout" | "timed_out"
    )
}

fn is_succeeded_operator_status(status: &str) -> bool {
    let status = status.trim().to_ascii_lowercase();
    matches!(status.as_str(), "succeeded" | "success")
}

fn run_matches_cleanup_age(summary: &OperatorRunSummary, max_age_days: Option<u64>) -> bool {
    let Some(max_age_days) = max_age_days else {
        return true;
    };
    let Some(updated_at) = summary.updated_at.as_deref() else {
        return true;
    };
    let Ok(updated_at) = chrono::DateTime::parse_from_rfc3339(updated_at) else {
        return true;
    };
    let age = chrono::Utc::now().signed_duration_since(updated_at.with_timezone(&chrono::Utc));
    age.num_seconds() >= (max_age_days as i64).saturating_mul(24 * 60 * 60)
}

fn local_operator_run_dir_size(project_root: &Path, run_id: &str) -> Result<u64, String> {
    let run_dir = safe_local_operator_run_dir(project_root, run_id)?;
    Ok(path_tree_size(&run_dir))
}

fn safe_local_operator_run_dir(project_root: &Path, run_id: &str) -> Result<PathBuf, String> {
    if !is_safe_operator_run_id(run_id) {
        return Err(
            "operator run id must start with `oprun_` and contain only letters, numbers, `_`, or `-`"
                .to_string(),
        );
    }
    let runs_root = operator_runs_root(project_root);
    let run_dir = runs_root.join(run_id);
    let canonical_root = runs_root.canonicalize().unwrap_or(runs_root);
    let canonical_run_dir = run_dir
        .canonicalize()
        .map_err(|err| format!("operator run `{run_id}` not found: {err}"))?;
    if !canonical_run_dir.starts_with(&canonical_root) {
        return Err(format!(
            "operator run `{run_id}` is outside the run registry"
        ));
    }
    Ok(canonical_run_dir)
}

fn path_tree_size(path: &Path) -> u64 {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return 0;
    };
    if metadata.is_file() {
        return metadata.len();
    }
    if !metadata.is_dir() {
        return 0;
    }
    fs::read_dir(path)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|entry| path_tree_size(&entry.path()))
        .fold(0_u64, u64::saturating_add)
}

fn delete_local_operator_run_dir(project_root: &Path, run_id: &str) -> Result<(), String> {
    let run_dir = safe_local_operator_run_dir(project_root, run_id)?;
    fs::remove_dir_all(&run_dir).map_err(|err| {
        format!(
            "delete operator run `{run_id}` at {}: {err}",
            run_dir.display()
        )
    })
}

async fn estimate_environment_run_dir_size(
    ctx: &crate::domain::tools::ToolContext,
    run_dir: &str,
) -> Option<u64> {
    let command = format!(
        "du -sk {} 2>/dev/null | awk '{{print $1}}'",
        sh_quote(run_dir)
    );
    let result = execute_env_command(ctx, &operator_environment_cwd(ctx), &command, 30)
        .await
        .ok()?;
    result.output.trim().parse::<u64>().ok().map(|kb| kb * 1024)
}

async fn delete_environment_operator_run_dir(
    ctx: &crate::domain::tools::ToolContext,
    runs_root: &str,
    run_dir: &str,
) -> Result<(), String> {
    let normalized_root = runs_root.trim_end_matches('/');
    let normalized_run_dir = run_dir.trim_end_matches('/');
    let run_id = normalized_run_dir.rsplit('/').next().unwrap_or_default();
    if !is_safe_operator_run_id(run_id)
        || !normalized_run_dir.starts_with(&format!("{normalized_root}/oprun_"))
    {
        return Err(format!(
            "refusing to delete operator run outside active run registry: {run_dir}"
        ));
    }
    let command = format!(
        "target={}; root={}; case \"$target\" in \"$root\"/oprun_*) rm -rf -- \"$target\" ;; *) exit 64 ;; esac",
        sh_quote(normalized_run_dir),
        sh_quote(normalized_root),
    );
    let result = execute_env_command(ctx, &operator_environment_cwd(ctx), &command, 30)
        .await
        .map_err(|err| err.message)?;
    if result.returncode == 0 {
        Ok(())
    } else {
        Err(format!(
            "remote cleanup command exited with code {}",
            result.returncode
        ))
    }
}

fn output_artifact_paths(document: &JsonValue) -> Vec<(String, String)> {
    let Some(outputs) = json_value_at(document, &["outputs"]).and_then(JsonValue::as_object) else {
        return Vec::new();
    };
    let mut paths = Vec::new();
    for (name, artifacts) in outputs {
        for artifact in artifacts.as_array().into_iter().flatten() {
            if let Some(path) = json_string_at(artifact, &["path"]) {
                paths.push((name.clone(), path));
            }
        }
    }
    paths
}

async fn verify_artifact_path_for_context(
    ctx: &crate::domain::tools::ToolContext,
    location: &str,
    output_name: &str,
    path: &str,
) -> OperatorRunCheck {
    if location == "local" {
        let metadata = fs::metadata(path).ok();
        let ok = metadata
            .as_ref()
            .map(|metadata| metadata.is_file())
            .unwrap_or(false);
        return OperatorRunCheck {
            name: format!("output_artifact:{output_name}"),
            ok,
            severity: if ok { "info" } else { "error" }.to_string(),
            message: if ok {
                format!(
                    "Output artifact `{output_name}` exists ({} bytes).",
                    metadata.map(|metadata| metadata.len()).unwrap_or(0)
                )
            } else {
                format!("Output artifact `{output_name}` is missing.")
            },
            path: Some(path.to_string()),
        };
    }

    let command = format!(
        "if [ -f {} ]; then wc -c < {}; else exit 2; fi",
        sh_quote(path),
        sh_quote(path)
    );
    match execute_env_command(ctx, &operator_environment_cwd(ctx), &command, 30).await {
        Ok(result) if result.returncode == 0 => OperatorRunCheck {
            name: format!("output_artifact:{output_name}"),
            ok: true,
            severity: "info".to_string(),
            message: format!(
                "Output artifact `{output_name}` exists remotely ({} bytes).",
                result.output.trim()
            ),
            path: Some(path.to_string()),
        },
        Ok(result) => OperatorRunCheck {
            name: format!("output_artifact:{output_name}"),
            ok: false,
            severity: "error".to_string(),
            message: format!(
                "Output artifact `{output_name}` is missing or unreadable remotely (exit {}).",
                result.returncode
            ),
            path: Some(path.to_string()),
        },
        Err(error) => OperatorRunCheck {
            name: format!("output_artifact:{output_name}"),
            ok: false,
            severity: "error".to_string(),
            message: error.message,
            path: Some(path.to_string()),
        },
    }
}

fn operator_runs_root(project_root: &Path) -> PathBuf {
    project_root
        .join(OPERATOR_STATE_DIR_NAME)
        .join(RUNS_RELATIVE_PATH)
}

fn operator_run_dir(project_root: &Path, run_id: &str) -> PathBuf {
    operator_runs_root(project_root).join(run_id)
}

fn operator_run_relative_path(run_id: &str) -> String {
    format!("{OPERATOR_STATE_DIR_NAME}/{RUNS_RELATIVE_PATH}/{run_id}")
}

fn operator_runs_relative_path() -> String {
    format!("{OPERATOR_STATE_DIR_NAME}/{RUNS_RELATIVE_PATH}")
}

#[derive(Debug)]
struct OperatorRunSummaryWithSortKey {
    summary: OperatorRunSummary,
    sort_key: u64,
}

fn summarize_local_operator_run_dir(
    run_dir: &Path,
    run_id: &str,
) -> Option<OperatorRunSummaryWithSortKey> {
    let provenance_path = run_dir.join("provenance.json");
    let status_path = run_dir.join("status.json");
    let provenance = read_json_value(&provenance_path).ok();
    let status_doc = read_json_value(&status_path).ok();
    if provenance.is_none() && status_doc.is_none() {
        return None;
    }
    let modified_path = if provenance_path.is_file() {
        provenance_path.as_path()
    } else if status_path.is_file() {
        status_path.as_path()
    } else {
        run_dir
    };
    let updated_at = status_doc
        .as_ref()
        .and_then(|value| json_string_at(value, &["updatedAt"]))
        .or_else(|| {
            provenance
                .as_ref()
                .and_then(|value| json_string_at(value, &["updatedAt"]))
        })
        .or_else(|| file_modified_rfc3339(modified_path));
    let default_provenance_path = if provenance_path.is_file() {
        Some(provenance_path.to_string_lossy().into_owned())
    } else {
        None
    };
    summarize_operator_run_documents(
        run_id,
        "local",
        run_dir.to_string_lossy().into_owned(),
        default_provenance_path,
        provenance,
        status_doc,
        updated_at,
        file_modified_sort_key(modified_path),
    )
}

fn summarize_operator_run_documents(
    run_id: &str,
    default_location: &str,
    default_run_dir: String,
    default_provenance_path: Option<String>,
    provenance: Option<JsonValue>,
    status_doc: Option<JsonValue>,
    updated_at: Option<String>,
    sort_key: u64,
) -> Option<OperatorRunSummaryWithSortKey> {
    if provenance.is_none() && status_doc.is_none() {
        return None;
    }
    let status = status_doc
        .as_ref()
        .and_then(|value| json_string_at(value, &["status"]))
        .or_else(|| {
            provenance
                .as_ref()
                .and_then(|value| json_string_at(value, &["status"]))
        })
        .unwrap_or_else(|| "unknown".to_string());
    let location = provenance
        .as_ref()
        .and_then(|value| json_string_at(value, &["location"]))
        .unwrap_or_else(|| default_location.to_string());
    let operator_alias = provenance
        .as_ref()
        .and_then(|value| json_string_at(value, &["operator", "alias"]));
    let operator_id = provenance
        .as_ref()
        .and_then(|value| json_string_at(value, &["operator", "id"]))
        .or_else(|| {
            status_doc
                .as_ref()
                .and_then(|value| json_string_at(value, &["operator", "id"]))
        });
    let operator_alias = operator_alias.or_else(|| {
        status_doc
            .as_ref()
            .and_then(|value| json_string_at(value, &["operator", "alias"]))
    });
    let operator_version = provenance
        .as_ref()
        .and_then(|value| json_string_at(value, &["operator", "version"]))
        .or_else(|| {
            status_doc
                .as_ref()
                .and_then(|value| json_string_at(value, &["operator", "version"]))
        });
    let source_plugin = provenance
        .as_ref()
        .and_then(|value| json_string_at(value, &["operator", "sourcePlugin"]))
        .or_else(|| {
            status_doc
                .as_ref()
                .and_then(|value| json_string_at(value, &["operator", "sourcePlugin"]))
        });
    let run_kind = provenance
        .as_ref()
        .and_then(|value| json_string_at(value, &["runContext", "kind"]))
        .or_else(|| {
            status_doc
                .as_ref()
                .and_then(|value| json_string_at(value, &["runContext", "kind"]))
        });
    let smoke_test_id = provenance
        .as_ref()
        .and_then(|value| json_string_at(value, &["runContext", "smokeTestId"]))
        .or_else(|| {
            status_doc
                .as_ref()
                .and_then(|value| json_string_at(value, &["runContext", "smokeTestId"]))
        });
    let smoke_test_name = provenance
        .as_ref()
        .and_then(|value| json_string_at(value, &["runContext", "smokeTestName"]))
        .or_else(|| {
            status_doc
                .as_ref()
                .and_then(|value| json_string_at(value, &["runContext", "smokeTestName"]))
        });
    let run_dir_value = provenance
        .as_ref()
        .and_then(|value| json_string_at(value, &["runDir"]))
        .or_else(|| {
            status_doc
                .as_ref()
                .and_then(|value| json_string_at(value, &["runDir"]))
        })
        .unwrap_or(default_run_dir);
    let provenance_path_value = provenance.as_ref().and_then(|value| {
        json_string_at(value, &["provenancePath"]).or(default_provenance_path.clone())
    });
    let export_dir = provenance
        .as_ref()
        .and_then(|value| json_string_at(value, &["exportDir"]))
        .or_else(|| {
            status_doc
                .as_ref()
                .and_then(|value| json_string_at(value, &["exportDir"]))
        });
    let output_count = provenance.as_ref().map(output_artifact_count).unwrap_or(0);
    let structured_output_count = provenance
        .as_ref()
        .map(structured_output_count)
        .unwrap_or(0);
    let error_message = status_doc
        .as_ref()
        .and_then(operator_error_message)
        .or_else(|| provenance.as_ref().and_then(operator_error_message));
    let error_kind = status_doc
        .as_ref()
        .and_then(|value| json_string_at(value, &["error", "kind"]))
        .or_else(|| {
            provenance
                .as_ref()
                .and_then(|value| json_string_at(value, &["error", "kind"]))
        });
    let retryable = status_doc
        .as_ref()
        .and_then(|value| json_bool_at(value, &["error", "retryable"]))
        .or_else(|| {
            provenance
                .as_ref()
                .and_then(|value| json_bool_at(value, &["error", "retryable"]))
        });
    let suggested_action = status_doc
        .as_ref()
        .and_then(|value| json_string_at(value, &["error", "suggestedAction"]))
        .or_else(|| {
            provenance
                .as_ref()
                .and_then(|value| json_string_at(value, &["error", "suggestedAction"]))
        });
    let stdout_tail = status_doc
        .as_ref()
        .and_then(|value| json_string_at(value, &["error", "stdoutTail"]))
        .or_else(|| {
            provenance
                .as_ref()
                .and_then(|value| json_string_at(value, &["error", "stdoutTail"]))
        });
    let stderr_tail = status_doc
        .as_ref()
        .and_then(|value| json_string_at(value, &["error", "stderrTail"]))
        .or_else(|| {
            provenance
                .as_ref()
                .and_then(|value| json_string_at(value, &["error", "stderrTail"]))
        });
    let cache_key = provenance
        .as_ref()
        .and_then(|value| json_string_at(value, &["cache", "key"]))
        .or_else(|| {
            status_doc
                .as_ref()
                .and_then(|value| json_string_at(value, &["cache", "key"]))
        });
    let cache_hit = provenance
        .as_ref()
        .and_then(|value| json_bool_at(value, &["cache", "hit"]))
        .or_else(|| {
            status_doc
                .as_ref()
                .and_then(|value| json_bool_at(value, &["cache", "hit"]))
        });
    let cache_source_run_id = provenance
        .as_ref()
        .and_then(|value| json_string_at(value, &["cache", "sourceRunId"]))
        .or_else(|| {
            status_doc
                .as_ref()
                .and_then(|value| json_string_at(value, &["cache", "sourceRunId"]))
        });
    let cache_source_run_dir = provenance
        .as_ref()
        .and_then(|value| json_string_at(value, &["cache", "sourceRunDir"]))
        .or_else(|| {
            status_doc
                .as_ref()
                .and_then(|value| json_string_at(value, &["cache", "sourceRunDir"]))
        });
    Some(OperatorRunSummaryWithSortKey {
        summary: OperatorRunSummary {
            run_id: run_id.to_string(),
            status,
            location,
            operator_alias,
            operator_id,
            operator_version,
            source_plugin,
            run_kind,
            smoke_test_id,
            smoke_test_name,
            run_dir: run_dir_value,
            updated_at,
            provenance_path: provenance_path_value,
            export_dir,
            output_count,
            structured_output_count,
            error_message,
            error_kind,
            retryable,
            suggested_action,
            stdout_tail,
            stderr_tail,
            cache_key,
            cache_hit,
            cache_source_run_id,
            cache_source_run_dir,
        },
        sort_key,
    })
}

fn is_safe_operator_run_id(run_id: &str) -> bool {
    run_id.starts_with("oprun_")
        && run_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

fn read_json_value(path: &Path) -> Result<JsonValue, String> {
    let raw = fs::read_to_string(path).map_err(|err| err.to_string())?;
    serde_json::from_str(&raw).map_err(|err| err.to_string())
}

fn json_value_at<'a>(value: &'a JsonValue, path: &[&str]) -> Option<&'a JsonValue> {
    let mut current = value;
    for key in path {
        current = current.get(*key)?;
    }
    Some(current)
}

fn json_string_at(value: &JsonValue, path: &[&str]) -> Option<String> {
    json_value_at(value, path).and_then(|value| match value {
        JsonValue::String(value) if !value.trim().is_empty() => Some(value.clone()),
        _ => None,
    })
}

fn json_bool_at(value: &JsonValue, path: &[&str]) -> Option<bool> {
    json_value_at(value, path).and_then(JsonValue::as_bool)
}

fn operator_error_message(value: &JsonValue) -> Option<String> {
    json_string_at(value, &["error", "message"])
}

fn output_artifact_count(value: &JsonValue) -> usize {
    json_value_at(value, &["outputs"])
        .and_then(JsonValue::as_object)
        .map(|outputs| {
            outputs
                .values()
                .filter_map(JsonValue::as_array)
                .map(Vec::len)
                .sum()
        })
        .unwrap_or(0)
}

fn structured_output_count(value: &JsonValue) -> usize {
    json_value_at(value, &["structuredOutputs"])
        .and_then(JsonValue::as_object)
        .map(JsonMap::len)
        .unwrap_or(0)
}

fn file_modified_sort_key(path: &Path) -> u64 {
    fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn rfc3339_sort_key(value: Option<&str>) -> u64 {
    value
        .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
        .and_then(|value| u64::try_from(value.timestamp()).ok())
        .unwrap_or(0)
}

fn file_modified_rfc3339(path: &Path) -> Option<String> {
    let modified = fs::metadata(path).ok()?.modified().ok()?;
    let datetime: chrono::DateTime<chrono::Utc> = modified.into();
    Some(datetime.to_rfc3339())
}

fn read_tail(path: impl AsRef<Path>) -> Option<String> {
    read_tail_limited(path, 4000)
}

fn read_tail_limited(path: impl AsRef<Path>, limit_chars: usize) -> Option<String> {
    let raw = fs::read_to_string(path).ok()?;
    let chars = raw.chars().collect::<Vec<_>>();
    let start = chars.len().saturating_sub(limit_chars);
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

fn enforcement_json(ctx: &crate::domain::tools::ToolContext, spec: &OperatorSpec) -> JsonValue {
    match ctx.execution_environment.as_str() {
        "sandbox" | "remote" => json!({
            "placement": "local",
            "container": if ctx.sandbox_backend.trim().is_empty() { "sandbox" } else { ctx.sandbox_backend.trim() },
            "filesystem": "container_best_effort",
            "network": "backend_policy_or_manifest_permissions_best_effort"
        }),
        "ssh" => {
            let container = spec
                .runtime
                .as_ref()
                .and_then(|runtime| selected_direct_container(ctx, runtime))
                .map(|selection| selection.kind.as_str().to_string());
            json!({
                "placement": "ssh",
                "container": container.as_deref().unwrap_or("none"),
                "filesystem": if container.is_some() { "remote_container_bind_mount_best_effort" } else { "trusted_remote_best_effort" },
                "network": if container.is_some() { "container_runtime_policy" } else { "remote_user_environment" }
            })
        }
        _ => {
            let container = spec
                .runtime
                .as_ref()
                .and_then(|runtime| selected_direct_container(ctx, runtime))
                .map(|selection| selection.kind.as_str().to_string());
            json!({
                "placement": "local",
                "container": container.as_deref().unwrap_or("none"),
                "filesystem": if container.is_some() { "local_container_bind_mount_best_effort" } else { "local_best_effort" },
                "network": if container.is_some() { "container_runtime_policy" } else { "local_user_environment" }
            })
        }
    }
}

fn effective_walltime_secs(resources: &BTreeMap<String, JsonValue>, session_hard_secs: u64) -> u64 {
    let hard_limit = session_hard_secs.max(1);
    resource_walltime_secs(resources)
        .unwrap_or(hard_limit)
        .min(hard_limit)
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

    fn bundled_smoke_operator_paths() -> (PathBuf, PathBuf) {
        bundled_operator_manifest_path("write-text-report")
    }

    fn bundled_container_operator_paths() -> (PathBuf, PathBuf) {
        bundled_operator_manifest_path("container-text-report")
    }

    fn bundled_operator_manifest_path(operator_dir: &str) -> (PathBuf, PathBuf) {
        bundled_plugin_operator_manifest_path("operator-smoke", operator_dir)
    }

    fn bundled_plugin_operator_manifest_path(
        plugin_name: &str,
        operator_dir: &str,
    ) -> (PathBuf, PathBuf) {
        let plugin_root = crate::domain::plugins::dev_builtin_marketplace_path()
            .parent()
            .unwrap()
            .join("plugins")
            .join(plugin_name);
        let manifest = plugin_root
            .join("operators")
            .join(operator_dir)
            .join("operator.yaml");
        (plugin_root, manifest)
    }

    fn cached_report_operator_spec(
        tmp: &TempDir,
        marker_path: &Path,
        cache: Option<JsonValue>,
    ) -> OperatorSpec {
        OperatorSpec {
            api_version: OPERATOR_API_VERSION_V1ALPHA1.to_string(),
            kind: OPERATOR_KIND.to_string(),
            metadata: OperatorMetadata {
                id: "cached_report".to_string(),
                version: "1".to_string(),
                name: None,
                description: Some("cacheable local report".to_string()),
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
                        kind: OperatorFieldKind::File,
                        required: true,
                        glob: Some("report.txt".to_string()),
                        ..OperatorFieldSpec::default()
                    },
                )]),
                ..OperatorInterfaceSpec::default()
            },
            smoke_tests: Vec::new(),
            execution: OperatorExecutionSpec {
                argv: vec![
                    "/bin/sh".to_string(),
                    "-c".to_string(),
                    "printf 'run\\n' >> \"$1\"; cat \"$2\" > \"$3/report.txt\"".to_string(),
                    "cached_report".to_string(),
                    marker_path.to_string_lossy().into_owned(),
                    "${inputs.input}".to_string(),
                    "${outdir}".to_string(),
                ],
            },
            preflight: None,
            runtime: None,
            cache,
            resources: BTreeMap::new(),
            bindings: Vec::new(),
            permissions: None,
            source: OperatorSource {
                source_plugin: "test@local".to_string(),
                plugin_root: tmp.path().to_path_buf(),
                manifest_path: tmp.path().join("operator.yaml"),
            },
        }
    }

    fn argv_operator_spec(tmp: &TempDir, argv: &[&str]) -> OperatorSpec {
        OperatorSpec {
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
            smoke_tests: Vec::new(),
            execution: OperatorExecutionSpec {
                argv: argv.iter().map(|value| value.to_string()).collect(),
            },
            preflight: None,
            runtime: None,
            cache: None,
            resources: BTreeMap::new(),
            bindings: Vec::new(),
            permissions: None,
            source: OperatorSource {
                source_plugin: "p".to_string(),
                plugin_root: tmp.path().to_path_buf(),
                manifest_path: tmp.path().join("operator.yaml"),
            },
        }
    }

    fn input_file_invocation(input: &str) -> OperatorInvocation {
        OperatorInvocation {
            inputs: BTreeMap::from([("input".to_string(), JsonValue::String(input.to_string()))]),
            params: BTreeMap::new(),
            resources: BTreeMap::new(),
            metadata: BTreeMap::new(),
        }
    }

    #[test]
    fn operator_tool_schema_surfaces_resource_profile_warning() {
        let tmp = TempDir::new().unwrap();
        let mut spec = argv_operator_spec(&tmp, &["/bin/echo", "ok"]);
        spec.metadata.description = Some("Align reads".to_string());
        spec.runtime = Some(json!({
            "resourceProfile": {
                "tier": "hpc-recommended",
                "localPolicy": "warn",
                "recommendedCpu": 32,
                "recommendedMemoryGb": 128,
                "diskGb": 200,
                "notes": ["Use SSH/server/HPC for production RNA-seq runs."]
            }
        }));

        let schema = operator_tool_schema(ResolvedOperator {
            alias: "align".to_string(),
            spec,
        });

        assert!(schema.description.contains("Resource note"));
        assert!(schema.description.contains("HPC/server recommended"));
        assert!(schema.description.contains("32 CPU recommended"));
        assert!(schema.description.contains("128 GB RAM recommended"));
    }

    #[tokio::test]
    async fn successful_operator_tool_call_writes_execution_record() {
        let tmp = TempDir::new().unwrap();
        let spec = argv_operator_spec(
            &tmp,
            &[
                "/bin/sh",
                "-c",
                "printf 'execution-record-success\\n' >/dev/null",
            ],
        );
        let ctx = crate::domain::tools::ToolContext::new(tmp.path())
            .with_session_id(Some("session-op".to_string()));
        let arguments =
            serde_json::to_string(&OperatorInvocation::default()).expect("serialize invocation");

        let (raw, is_error) = execute_resolved_operator_tool_call_with_context(
            &ctx,
            "x",
            ResolvedOperator {
                alias: "x".to_string(),
                spec,
            },
            &arguments,
            None,
        )
        .await;

        assert!(!is_error, "{raw}");
        let rows = crate::domain::execution_records::list_recent_execution_records(tmp.path(), 10)
            .await
            .expect("records");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].kind, "operator");
        assert_eq!(rows[0].unit_id.as_deref(), Some("x"));
        assert_eq!(rows[0].canonical_id.as_deref(), Some("p/operator/x"));
        assert_eq!(rows[0].provider_plugin.as_deref(), Some("p"));
        assert_eq!(rows[0].status, "succeeded");
        assert_eq!(rows[0].session_id.as_deref(), Some("session-op"));
    }

    #[tokio::test]
    async fn operator_result_and_record_include_param_sources() {
        let tmp = TempDir::new().unwrap();
        let mut spec = argv_operator_spec(&tmp, &["/bin/sh", "-c", "true"]);
        spec.interface.params.insert(
            "method".to_string(),
            OperatorFieldSpec {
                kind: OperatorFieldKind::Enum,
                enum_values: vec![json!("auto"), json!("manual")],
                ..OperatorFieldSpec::default()
            },
        );
        spec.interface.params.insert(
            "alpha".to_string(),
            OperatorFieldSpec {
                kind: OperatorFieldKind::Number,
                default: Some(json!(0.05)),
                ..OperatorFieldSpec::default()
            },
        );
        let invocation = OperatorInvocation {
            params: BTreeMap::from([("method".to_string(), json!("manual"))]),
            metadata: BTreeMap::from([(
                OPERATOR_PREFLIGHT_METADATA_KEY.to_string(),
                json!({
                    "source": "operator_preflight",
                    "operatorId": "x",
                    "answeredParams": [{"param": "method"}],
                    "paramsBySource": {"method": OPERATOR_PARAM_SOURCE_USER_PREFLIGHT},
                }),
            )]),
            ..OperatorInvocation::default()
        };
        let ctx = crate::domain::tools::ToolContext::new(tmp.path());
        let arguments = serde_json::to_string(&invocation).expect("serialize invocation");

        let (raw, is_error) = execute_resolved_operator_tool_call_with_context(
            &ctx,
            "x",
            ResolvedOperator {
                alias: "x".to_string(),
                spec,
            },
            &arguments,
            None,
        )
        .await;

        assert!(!is_error, "{raw}");
        let parsed = serde_json::from_str::<JsonValue>(&raw).unwrap();
        assert_eq!(
            parsed["paramSources"]["method"],
            OPERATOR_PARAM_SOURCE_USER_PREFLIGHT
        );
        assert_eq!(
            parsed["paramSources"]["alpha"],
            OPERATOR_PARAM_SOURCE_DEFAULT
        );
        assert_eq!(
            parsed["preflight"]["paramsBySource"]["method"],
            OPERATOR_PARAM_SOURCE_USER_PREFLIGHT
        );

        let rows = crate::domain::execution_records::list_recent_execution_records(tmp.path(), 10)
            .await
            .expect("records");
        let metadata = rows[0].metadata_json.as_deref().unwrap_or_default();
        assert!(metadata.contains("\"paramSources\""));
        assert!(metadata.contains(OPERATOR_PARAM_SOURCE_USER_PREFLIGHT));
    }

    #[tokio::test]
    async fn failed_operator_tool_call_writes_execution_record() {
        let tmp = TempDir::new().unwrap();
        let spec = argv_operator_spec(
            &tmp,
            &["/bin/sh", "-c", "echo execution-record-failure >&2; exit 7"],
        );
        let ctx = crate::domain::tools::ToolContext::new(tmp.path());
        let arguments =
            serde_json::to_string(&OperatorInvocation::default()).expect("serialize invocation");

        let (raw, is_error) = execute_resolved_operator_tool_call_with_context(
            &ctx,
            "x",
            ResolvedOperator {
                alias: "x".to_string(),
                spec,
            },
            &arguments,
            None,
        )
        .await;

        assert!(is_error, "{raw}");
        let rows = crate::domain::execution_records::list_recent_execution_records(tmp.path(), 10)
            .await
            .expect("records");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].kind, "operator");
        assert_eq!(rows[0].unit_id.as_deref(), Some("x"));
        assert_eq!(rows[0].status, "failed");
        assert!(rows[0]
            .metadata_json
            .as_deref()
            .unwrap_or_default()
            .contains("tool_exit_nonzero"));
    }

    #[tokio::test]
    async fn operator_success_does_not_fail_when_execution_record_write_fails() {
        let tmp = TempDir::new().unwrap();
        let state_dir = tmp.path().join(".omiga");
        fs::create_dir_all(&state_dir).unwrap();
        fs::write(state_dir.join("execution"), "not a directory").unwrap();
        let spec = argv_operator_spec(
            &tmp,
            &[
                "/bin/sh",
                "-c",
                "printf 'record-write-blocked\\n' >/dev/null",
            ],
        );
        let ctx = crate::domain::tools::ToolContext::new(tmp.path());
        let arguments =
            serde_json::to_string(&OperatorInvocation::default()).expect("serialize invocation");

        let (raw, is_error) = execute_resolved_operator_tool_call_with_context(
            &ctx,
            "x",
            ResolvedOperator {
                alias: "x".to_string(),
                spec,
            },
            &arguments,
            None,
        )
        .await;

        assert!(!is_error, "{raw}");
        let parsed: JsonValue = serde_json::from_str(&raw).expect("operator result json");
        assert_eq!(parsed["status"], "succeeded");
        assert!(state_dir.join("execution").is_file());
    }

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
cache:
  enabled: true
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
        assert_eq!(spec.cache, Some(json!({"enabled": true})));
        let schema = operator_parameters_schema(&spec);
        assert_eq!(schema["required"][0], "inputs");
        assert!(schema["properties"]["inputs"]["properties"]["reads"]["items"].is_object());
        assert_eq!(
            schema["properties"]["resources"]["properties"]["cpu"]["type"],
            "integer"
        );
    }

    #[test]
    fn discovers_operators_from_manifest_declared_path() {
        let tmp = TempDir::new().unwrap();
        fs::write(
            tmp.path().join("plugin.json"),
            r#"{"name":"custom-operator-plugin","operators":"./custom-units"}"#,
        )
        .unwrap();
        let manifest = tmp
            .path()
            .join("custom-units")
            .join("custom")
            .join("operator.yaml");
        fs::create_dir_all(manifest.parent().unwrap()).unwrap();
        fs::write(
            &manifest,
            r#"
apiVersion: omiga.ai/operator/v1alpha1
kind: Operator
metadata:
  id: custom_manifest_path
  version: 1
execution:
  argv: ["true"]
"#,
        )
        .unwrap();
        let plugin = crate::domain::plugins::LoadedPlugin {
            id: "custom-operator-plugin@local".to_string(),
            manifest_name: Some("custom-operator-plugin".to_string()),
            display_name: None,
            description: None,
            root: tmp.path().to_path_buf(),
            enabled: true,
            skill_roots: Vec::new(),
            mcp_servers: HashMap::new(),
            apps: Vec::new(),
            retrieval: None,
            error: None,
        };

        let candidates = discover_operator_candidates_from_plugins([&plugin]);

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].metadata.id, "custom_manifest_path");
    }

    #[test]
    fn discovers_bundled_smoke_operator_from_active_plugin() {
        let (plugin_root, manifest) = bundled_smoke_operator_paths();
        assert!(manifest.is_file());

        let plugin = crate::domain::plugins::LoadedPlugin {
            id: "operator-smoke@omiga-curated".to_string(),
            manifest_name: Some("operator-smoke".to_string()),
            display_name: Some("Smoke Test".to_string()),
            description: None,
            root: plugin_root,
            enabled: true,
            skill_roots: Vec::new(),
            mcp_servers: HashMap::new(),
            apps: Vec::new(),
            retrieval: None,
            error: None,
        };

        let candidates = discover_operator_candidates_from_plugins([&plugin]);
        let smoke = candidates
            .iter()
            .find(|candidate| candidate.metadata.id == "write_text_report")
            .expect("bundled smoke operator should be discovered");
        assert_eq!(smoke.source.source_plugin, "operator-smoke@omiga-curated");
        assert_eq!(smoke.metadata.version, "0.1.0");
        assert_eq!(smoke.execution.argv[0], "/bin/sh");
        assert_eq!(smoke.execution.argv[1], "./scripts/write_text_report.sh");
        assert_eq!(smoke.smoke_tests.len(), 1);
        assert_eq!(smoke.smoke_tests[0].id, "default");
        assert_eq!(
            smoke.smoke_tests[0].arguments.params["message"],
            "hello operator smoke"
        );

        let container = candidates
            .iter()
            .find(|candidate| candidate.metadata.id == "container_text_report")
            .expect("bundled container smoke operator should be discovered");
        assert_eq!(
            container.source.source_plugin,
            "operator-smoke@omiga-curated"
        );
        assert_eq!(container.metadata.version, "0.1.0");
        assert_eq!(container.execution.argv[0], "/bin/sh");
        assert_eq!(
            container.execution.argv[1],
            "./scripts/write_container_report.sh"
        );
        assert_eq!(container.smoke_tests.len(), 1);
        assert_eq!(container.smoke_tests[0].id, "default");

        let schema = operator_parameters_schema(smoke);
        assert_eq!(
            schema["properties"]["params"]["properties"]["message"]["type"],
            "string"
        );
        let argv = expand_argv(
            smoke,
            &BTreeMap::new(),
            &BTreeMap::from([
                ("message".to_string(), json!("hello")),
                ("repeat".to_string(), json!(1)),
            ]),
            &BTreeMap::new(),
            "/tmp/run",
        )
        .unwrap();
        assert!(Path::new(&argv[1]).is_absolute());
        assert!(argv[1].ends_with("scripts/write_text_report.sh"));
    }

    #[test]
    fn discovers_atomic_bundled_analysis_operator_plugins() {
        let cases = [
            (
                "operator-pca-r",
                "pca-matrix",
                "omics_pca_matrix",
                "Rscript",
            ),
            (
                "operator-differential-expression-r",
                "differential-expression-basic",
                "omics_differential_expression_basic",
                "Rscript",
            ),
            (
                "operator-enrichment-r",
                "functional-enrichment-basic",
                "omics_functional_enrichment_basic",
                "Rscript",
            ),
            (
                "operator-seqtk",
                "seqtk-sample",
                "seqtk_sample_reads",
                "/bin/sh",
            ),
            (
                "operator-pubmed-search",
                "pubmed-search",
                "pubmed_search",
                "python3",
            ),
            ("operator-geo-search", "geo-search", "geo_search", "python3"),
            (
                "operator-uniprot-search",
                "uniprot-search",
                "uniprot_search",
                "python3",
            ),
        ];

        for (plugin_name, operator_dir, operator_id, first_argv) in cases {
            let (plugin_root, manifest) =
                bundled_plugin_operator_manifest_path(plugin_name, operator_dir);
            assert!(manifest.is_file(), "{plugin_name} manifest should exist");

            let plugin = crate::domain::plugins::LoadedPlugin {
                id: format!("{plugin_name}@omiga-curated"),
                manifest_name: Some(plugin_name.to_string()),
                display_name: None,
                description: None,
                root: plugin_root,
                enabled: true,
                skill_roots: Vec::new(),
                mcp_servers: HashMap::new(),
                apps: Vec::new(),
                retrieval: None,
                error: None,
            };

            let candidates = discover_operator_candidates_from_plugins([&plugin]);
            assert_eq!(
                candidates.len(),
                1,
                "{plugin_name} should expose one operator"
            );
            let operator = &candidates[0];
            assert_eq!(operator.metadata.id, operator_id);
            assert_eq!(operator.execution.argv[0], first_argv);
            assert!(matches!(
                operator.interface.outputs["summary"].kind,
                OperatorFieldKind::Json
            ));
            if plugin_name == "operator-differential-expression-r" {
                let preflight = operator
                    .preflight
                    .as_ref()
                    .expect("differential expression choices should live in manifest preflight");
                assert_eq!(preflight.questions.len(), 4);
                assert!(preflight.questions.iter().any(|question| {
                    question.param == "de_method"
                        && question
                            .options
                            .iter()
                            .any(|option| option.value == json!("deseq2"))
                }));
                assert!(preflight.questions.iter().any(|question| {
                    question.param == "pvalue_threshold"
                        && question
                            .options
                            .iter()
                            .any(|option| option.custom && option.custom_placeholder.is_some())
                }));
                let schema = operator_parameters_schema(operator);
                assert!(schema["required"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|value| value == "params"));
                let required_params = schema["properties"]["params"]["required"]
                    .as_array()
                    .unwrap();
                assert!(required_params.iter().any(|value| value == "group_column"));
                assert!(!required_params.iter().any(|value| value == "de_method"));
                assert!(!required_params
                    .iter()
                    .any(|value| value == "input_data_type"));
                assert!(
                    schema["properties"]["params"]["properties"]["de_method"]["description"]
                        .as_str()
                        .unwrap()
                        .contains("Ask state")
                );
                let ask_state_schema = schema["properties"]["params"]["properties"]["de_method"]
                    ["oneOf"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|candidate| candidate["enum"] == json!(["ask"]))
                    .expect("preflight params should allow explicit ask state");
                assert_eq!(ask_state_schema["type"], "string");
            }
            assert_eq!(
                operator.source.source_plugin,
                format!("{plugin_name}@omiga-curated")
            );
        }
    }

    #[test]
    fn transcriptomics_analysis_plugin_does_not_expose_operator_units() {
        let plugin_root = crate::domain::plugins::dev_builtin_marketplace_path()
            .parent()
            .unwrap()
            .join("plugins")
            .join("transcriptomics");
        assert!(
            !plugin_root.join("operators").exists(),
            "transcriptomics keeps template-only public units"
        );
        assert!(
            plugin_root.join("template_backing_operators").is_dir(),
            "private backing specs preserve template validation without exposing operators"
        );
        let plugin = crate::domain::plugins::LoadedPlugin {
            id: "transcriptomics@omiga-curated".to_string(),
            manifest_name: Some("transcriptomics".to_string()),
            display_name: Some("Transcriptomics".to_string()),
            description: None,
            root: plugin_root,
            enabled: true,
            skill_roots: Vec::new(),
            mcp_servers: HashMap::new(),
            apps: Vec::new(),
            retrieval: None,
            error: None,
        };

        let candidates = discover_operator_candidates_from_plugins([&plugin]);
        assert!(
            candidates.is_empty(),
            "Transcriptomics should expose templates only; operator versions are private backing specs"
        );
    }

    #[test]
    fn preflight_rules_are_manifest_driven() {
        let question = OperatorPreflightQuestionSpec {
            id: Some("method".to_string()),
            param: "method".to_string(),
            question: "Pick method?".to_string(),
            header: "Method".to_string(),
            multi_select: false,
            ask_when: OperatorPreflightAskWhen {
                always: false,
                missing: true,
                empty: true,
                values: vec![json!("auto")],
            },
            options: vec![
                OperatorPreflightOptionSpec {
                    label: "Auto".to_string(),
                    description: "Let the operator choose.".to_string(),
                    value: json!("auto"),
                    preview: None,
                    custom: false,
                    custom_placeholder: None,
                },
                OperatorPreflightOptionSpec {
                    label: "Manual".to_string(),
                    description: "Use a fixed method.".to_string(),
                    value: json!("manual"),
                    preview: None,
                    custom: false,
                    custom_placeholder: None,
                },
            ],
        };

        assert!(preflight_question_should_ask(&question, None));
        let mut auto_params = JsonMap::new();
        auto_params.insert("method".to_string(), JsonValue::String("AUTO".to_string()));
        assert!(preflight_question_should_ask(&question, Some(&auto_params)));

        let mut manual_params = JsonMap::new();
        manual_params.insert(
            "method".to_string(),
            JsonValue::String("manual".to_string()),
        );
        assert!(!preflight_question_should_ask(
            &question,
            Some(&manual_params)
        ));

        let mut ask_params = JsonMap::new();
        ask_params.insert("method".to_string(), JsonValue::String("ASK".to_string()));
        assert!(preflight_question_should_ask(&question, Some(&ask_params)));

        let mut ask_object_params = JsonMap::new();
        ask_object_params.insert("method".to_string(), json!({"state": "ask"}));
        assert!(preflight_question_should_ask(
            &question,
            Some(&ask_object_params)
        ));

        assert_eq!(
            preflight_answer_labels(&json!("Auto, Manual"), true),
            vec!["Auto".to_string(), "Manual".to_string()]
        );

        let always_question = OperatorPreflightQuestionSpec {
            ask_when: OperatorPreflightAskWhen {
                always: true,
                missing: false,
                empty: false,
                values: Vec::new(),
            },
            ..question
        };
        assert!(preflight_question_should_ask(
            &always_question,
            Some(&manual_params)
        ));
    }

    #[test]
    fn preflight_answers_record_param_source_metadata() {
        let tmp = TempDir::new().unwrap();
        let mut spec = argv_operator_spec(&tmp, &["true"]);
        spec.metadata.id = "preflight_metadata".to_string();
        spec.interface.params.insert(
            "method".to_string(),
            OperatorFieldSpec {
                kind: OperatorFieldKind::Enum,
                enum_values: vec![json!("auto"), json!("manual")],
                ..OperatorFieldSpec::default()
            },
        );
        spec.preflight = Some(OperatorPreflightSpec {
            questions: vec![OperatorPreflightQuestionSpec {
                id: Some("method".to_string()),
                param: "method".to_string(),
                question: "Pick method?".to_string(),
                header: "Method".to_string(),
                multi_select: false,
                ask_when: OperatorPreflightAskWhen {
                    always: true,
                    missing: false,
                    empty: false,
                    values: Vec::new(),
                },
                options: vec![
                    OperatorPreflightOptionSpec {
                        label: "Auto".to_string(),
                        description: "Auto method".to_string(),
                        value: json!("auto"),
                        preview: None,
                        custom: false,
                        custom_placeholder: None,
                    },
                    OperatorPreflightOptionSpec {
                        label: "Manual".to_string(),
                        description: "Manual method".to_string(),
                        value: json!("manual"),
                        preview: None,
                        custom: false,
                        custom_placeholder: None,
                    },
                ],
            }],
        });
        let updated = apply_operator_preflight_answers_for_spec(
            &spec,
            spec.preflight.as_ref().unwrap(),
            &json!({"params": {"method": "ask"}}).to_string(),
            &json!({"answers": {"Pick method?": "Manual"}}),
        )
        .expect("apply preflight");
        let parsed = serde_json::from_str::<JsonValue>(&updated).unwrap();

        assert_eq!(parsed["params"]["method"], "manual");
        assert_eq!(
            parsed["metadata"]["preflight"]["paramsBySource"]["method"],
            OPERATOR_PARAM_SOURCE_USER_PREFLIGHT
        );
        assert_eq!(
            parsed["metadata"]["preflight"]["answeredParams"][0]["param"],
            "method"
        );
    }

    #[test]
    fn preflight_project_preferences_recommend_without_overriding_explicit_params() {
        let tmp = TempDir::new().unwrap();
        let mut spec = argv_operator_spec(&tmp, &["true"]);
        spec.metadata.id = "preflight_recommendation".to_string();
        spec.interface.params.insert(
            "method".to_string(),
            OperatorFieldSpec {
                kind: OperatorFieldKind::Enum,
                enum_values: vec![json!("auto"), json!("manual")],
                ..OperatorFieldSpec::default()
            },
        );
        spec.preflight = Some(OperatorPreflightSpec {
            questions: vec![OperatorPreflightQuestionSpec {
                id: Some("method".to_string()),
                param: "method".to_string(),
                question: "Pick method?".to_string(),
                header: "Method".to_string(),
                multi_select: false,
                ask_when: OperatorPreflightAskWhen {
                    always: false,
                    missing: true,
                    empty: true,
                    values: Vec::new(),
                },
                options: vec![
                    OperatorPreflightOptionSpec {
                        label: "Auto".to_string(),
                        description: "Auto method".to_string(),
                        value: json!("auto"),
                        preview: None,
                        custom: false,
                        custom_placeholder: None,
                    },
                    OperatorPreflightOptionSpec {
                        label: "Manual".to_string(),
                        description: "Manual method".to_string(),
                        value: json!("manual"),
                        preview: None,
                        custom: false,
                        custom_placeholder: None,
                    },
                ],
            }],
        });
        let recommended_params = BTreeMap::from([("method".to_string(), json!("manual"))]);

        let missing_params = operator_preflight_question_for_spec_with_recommended_params(
            &spec,
            Some("recommend"),
            None,
            Some(&recommended_params),
        )
        .expect("missing params should ask");
        assert_eq!(missing_params.questions[0].options[0].label, "Manual");
        assert!(missing_params.questions[0].options[0].recommended);
        assert!(missing_params.questions[0].options[0]
            .description
            .contains("推荐"));

        let mut explicit_params = JsonMap::new();
        explicit_params.insert("method".to_string(), json!("auto"));
        assert!(
            operator_preflight_question_for_spec_with_recommended_params(
                &spec,
                Some("recommend"),
                Some(&explicit_params),
                Some(&recommended_params),
            )
            .is_none(),
            "project preferences must not override explicit caller params"
        );

        let mut ask_params = JsonMap::new();
        ask_params.insert("method".to_string(), json!({"state": "ask"}));
        let ask_question = operator_preflight_question_for_spec_with_recommended_params(
            &spec,
            Some("recommend"),
            Some(&ask_params),
            Some(&recommended_params),
        )
        .expect("explicit ask state should still ask with recommendation");
        assert_eq!(ask_question.questions[0].options[0].label, "Manual");
    }

    #[test]
    fn preflight_authoring_diagnostics_flag_data_only_questions() {
        let tmp = TempDir::new().unwrap();
        let mut spec = argv_operator_spec(&tmp, &["true"]);
        spec.metadata.id = "data_only_preflight".to_string();
        spec.interface.params.insert(
            "group_column".to_string(),
            OperatorFieldSpec {
                kind: OperatorFieldKind::String,
                ..OperatorFieldSpec::default()
            },
        );
        spec.interface.params.insert(
            "sample_column".to_string(),
            OperatorFieldSpec {
                kind: OperatorFieldKind::String,
                ..OperatorFieldSpec::default()
            },
        );
        spec.preflight = Some(OperatorPreflightSpec {
            questions: vec![
                test_preflight_question("group", "group_column", "Which group column?"),
                test_preflight_question("sample", "sample_column", "Which sample column?"),
            ],
        });

        let diagnostics = operator_preflight_authoring_diagnostics(&spec);

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].severity, "warning");
        assert!(diagnostics[0].message.contains("data/grouping"));

        spec.interface.params.insert(
            "de_method".to_string(),
            OperatorFieldSpec {
                kind: OperatorFieldKind::Enum,
                enum_values: vec![json!("auto"), json!("deseq2")],
                ..OperatorFieldSpec::default()
            },
        );
        spec.preflight
            .as_mut()
            .unwrap()
            .questions
            .push(test_preflight_question(
                "method",
                "de_method",
                "Which analysis method?",
            ));
        assert!(operator_preflight_authoring_diagnostics(&spec).is_empty());
    }

    #[test]
    fn external_network_authoring_diagnostics_require_cache_and_fixture_mode() {
        let tmp = TempDir::new().unwrap();
        let mut spec = argv_operator_spec(&tmp, &["true"]);
        spec.metadata.id = "network_operator".to_string();
        spec.metadata.tags.push("external-network".to_string());
        spec.permissions = Some(json!({
            "sideEffects": ["external_network"],
            "network": {"hosts": ["example.test"], "mode": "read_only"}
        }));

        let diagnostics = operator_external_network_authoring_diagnostics(&spec);

        assert_eq!(diagnostics.len(), 2);
        assert!(diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("cache.enabled")));
        assert!(diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("offline_fixture")));

        spec.cache = Some(json!({
            "enabled": true,
            "policyVersion": "external-network/v1"
        }));
        spec.interface.params.insert(
            "mode".to_string(),
            OperatorFieldSpec {
                kind: OperatorFieldKind::Enum,
                enum_values: vec![json!("auto"), json!("live"), json!("offline_fixture")],
                default: Some(json!("auto")),
                ..OperatorFieldSpec::default()
            },
        );
        spec.interface.params.insert(
            "fixture_json".to_string(),
            OperatorFieldSpec {
                kind: OperatorFieldKind::String,
                default: Some(json!("")),
                ..OperatorFieldSpec::default()
            },
        );

        assert!(operator_external_network_authoring_diagnostics(&spec).is_empty());
    }

    fn test_preflight_question(
        id: &str,
        param: &str,
        question: &str,
    ) -> OperatorPreflightQuestionSpec {
        OperatorPreflightQuestionSpec {
            id: Some(id.to_string()),
            param: param.to_string(),
            question: question.to_string(),
            header: id.to_string(),
            multi_select: false,
            ask_when: OperatorPreflightAskWhen {
                always: true,
                missing: false,
                empty: false,
                values: Vec::new(),
            },
            options: vec![
                OperatorPreflightOptionSpec {
                    label: "A".to_string(),
                    description: "First option".to_string(),
                    value: json!("a"),
                    preview: None,
                    custom: false,
                    custom_placeholder: None,
                },
                OperatorPreflightOptionSpec {
                    label: "B".to_string(),
                    description: "Second option".to_string(),
                    value: json!("b"),
                    preview: None,
                    custom: false,
                    custom_placeholder: None,
                },
            ],
        }
    }

    #[test]
    fn preflight_custom_answers_parse_against_param_type() {
        let field = OperatorFieldSpec {
            kind: OperatorFieldKind::Number,
            minimum: Some(0.0),
            maximum: Some(1.0),
            ..OperatorFieldSpec::default()
        };
        let question = OperatorPreflightQuestionSpec {
            id: Some("fdr".to_string()),
            param: "pvalue_threshold".to_string(),
            question: "Pick FDR?".to_string(),
            header: "FDR".to_string(),
            multi_select: false,
            ask_when: OperatorPreflightAskWhen {
                always: true,
                missing: false,
                empty: false,
                values: Vec::new(),
            },
            options: vec![
                OperatorPreflightOptionSpec {
                    label: "FDR 0.05".to_string(),
                    description: "Default".to_string(),
                    value: json!(0.05),
                    preview: None,
                    custom: false,
                    custom_placeholder: None,
                },
                OperatorPreflightOptionSpec {
                    label: "自定义".to_string(),
                    description: "Typed value".to_string(),
                    value: json!(0.05),
                    preview: None,
                    custom: true,
                    custom_placeholder: Some("0.05".to_string()),
                },
            ],
        };

        assert_eq!(
            preflight_value_for_answer(&question, &field, "自定义：0.2").unwrap(),
            json!(0.2)
        );
        assert!(preflight_value_for_answer(&question, &field, "自定义：2").is_err());
    }

    #[test]
    fn bundled_container_smoke_operator_builds_docker_command() {
        let tmp = TempDir::new().unwrap();
        let (plugin_root, manifest) = bundled_container_operator_paths();
        let spec =
            load_operator_manifest(&manifest, "operator-smoke@omiga-curated", plugin_root).unwrap();
        assert_eq!(spec.metadata.id, "container_text_report");
        assert_eq!(spec.smoke_tests.len(), 1);
        assert_eq!(spec.smoke_tests[0].id, "default");

        let plain_ctx = crate::domain::tools::ToolContext::new(tmp.path());
        assert!(!runtime_supported(&plain_ctx, &spec));
        let docker_ctx =
            crate::domain::tools::ToolContext::new(tmp.path()).with_sandbox_backend("docker");
        assert!(runtime_supported(&docker_ctx, &spec));
        let singularity_ctx =
            crate::domain::tools::ToolContext::new(tmp.path()).with_sandbox_backend("singularity");
        assert!(runtime_supported(&singularity_ctx, &spec));

        let smoke = &spec.smoke_tests[0].arguments;
        let run_dir = "/tmp/oprun_container_smoke";
        let argv = expand_argv(
            &spec,
            &BTreeMap::new(),
            &smoke.params,
            &smoke.resources,
            run_dir,
        )
        .unwrap();
        assert!(Path::new(&argv[1]).is_absolute());
        assert!(argv[1].ends_with("scripts/write_container_report.sh"));

        let command = operator_execution_command(
            &docker_ctx,
            &spec,
            OperatorExecutionSurfaceKind::Local,
            run_dir,
            &argv,
            &BTreeMap::new(),
        );
        assert!(command.contains("'docker' 'run' '--rm'"));
        assert!(command.contains("'alpine:3.19'"));
        assert!(command.contains("'hello container operator smoke'"));
        assert!(command.contains("write_container_report.sh"));
        assert!(command.contains(&format!(
            "'{}:{}:ro'",
            spec.source.plugin_root.to_string_lossy(),
            spec.source.plugin_root.to_string_lossy()
        )));
        assert!(command.contains(&format!("'{run_dir}:{run_dir}'")));
    }

    #[tokio::test]
    #[ignore = "requires a running Docker daemon and access to the alpine:3.19 image"]
    async fn executes_bundled_container_smoke_operator_with_docker_runtime() {
        let tmp = TempDir::new().unwrap();
        let (plugin_root, manifest) = bundled_container_operator_paths();
        let spec =
            load_operator_manifest(&manifest, "operator-smoke@omiga-curated", plugin_root).unwrap();
        let smoke_invocation = spec
            .smoke_tests
            .iter()
            .find(|test| test.id == "default")
            .expect("container smoke test")
            .arguments
            .clone();
        let ctx = crate::domain::tools::ToolContext::new(tmp.path()).with_sandbox_backend("docker");

        let result = execute_resolved_operator(
            &ctx,
            ResolvedOperator {
                alias: "container_text_report".to_string(),
                spec,
            },
            smoke_invocation,
            Some(OperatorRunContext {
                kind: Some("smoke".to_string()),
                smoke_test_id: Some("default".to_string()),
                smoke_test_name: Some("Active container smoke".to_string()),
                parent_execution_id: None,
            }),
        )
        .await
        .unwrap();

        assert_eq!(result.status, "succeeded");
        assert_eq!(result.location, "local");
        assert_eq!(result.outputs["report"].len(), 1);
        assert_eq!(result.enforcement["container"], "docker");
        let report = fs::read_to_string(&result.outputs["report"][0].path).unwrap();
        assert!(report.contains("hello container operator smoke"));
        assert!(report.contains("container smoke runtime:"));
    }

    #[test]
    fn rejects_smoke_test_ids_with_invalid_characters() {
        let tmp = TempDir::new().unwrap();
        let manifest = tmp.path().join("operator.yaml");
        fs::write(
            &manifest,
            r#"
apiVersion: omiga.ai/operator/v1alpha1
kind: Operator
metadata:
  id: bad_smoke
  version: 1
smokeTests:
  - id: bad/id
    params: {}
execution:
  argv: ["true"]
"#,
        )
        .unwrap();

        let error = load_operator_manifest(&manifest, "p@m", tmp.path()).unwrap_err();
        assert!(error.contains("operator smoke test id `bad/id`"));
    }

    #[test]
    fn reports_invalid_operator_manifest_diagnostics() {
        let tmp = TempDir::new().unwrap();
        let manifest = tmp
            .path()
            .join("operators")
            .join("bad")
            .join("operator.yaml");
        fs::create_dir_all(manifest.parent().unwrap()).unwrap();
        fs::write(
            &manifest,
            r#"
apiVersion: wrong/v1
kind: Operator
metadata:
  id: bad
  version: 1
execution:
  argv: ["true"]
"#,
        )
        .unwrap();
        let plugin = crate::domain::plugins::LoadedPlugin {
            id: "bad-operator@local".to_string(),
            manifest_name: Some("bad-operator".to_string()),
            display_name: Some("Bad Operator".to_string()),
            description: None,
            root: tmp.path().to_path_buf(),
            enabled: true,
            skill_roots: Vec::new(),
            mcp_servers: HashMap::new(),
            apps: Vec::new(),
            retrieval: None,
            error: None,
        };

        let diagnostics = operator_manifest_diagnostics_from_plugins([&plugin]);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].source_plugin, "bad-operator@local");
        assert_eq!(diagnostics[0].severity, "error");
        assert!(diagnostics[0]
            .message
            .contains("unsupported operator apiVersion"));
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
            smoke_tests: Vec::new(),
            execution: OperatorExecutionSpec {
                argv: vec!["true".to_string()],
            },
            preflight: None,
            runtime: None,
            cache: None,
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
            smoke_tests: Vec::new(),
            execution: OperatorExecutionSpec {
                argv: vec!["true".to_string()],
            },
            preflight: None,
            runtime: None,
            cache: None,
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
        let spec = argv_operator_spec(&tmp, &["cat", "${inputs.files}"]);
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

    #[test]
    fn expands_missing_optional_argv_fields_as_empty_strings() {
        let tmp = TempDir::new().unwrap();
        let mut spec = argv_operator_spec(
            &tmp,
            &[
                "Rscript",
                "plot.R",
                "${inputs.metadata}",
                "{{ params.label }}",
                "${resources.cache_dir}",
            ],
        );
        spec.interface = OperatorInterfaceSpec {
            inputs: BTreeMap::from([(
                "metadata".to_string(),
                OperatorFieldSpec {
                    kind: OperatorFieldKind::File,
                    required: false,
                    ..OperatorFieldSpec::default()
                },
            )]),
            params: BTreeMap::from([(
                "label".to_string(),
                OperatorFieldSpec {
                    kind: OperatorFieldKind::String,
                    required: false,
                    ..OperatorFieldSpec::default()
                },
            )]),
            ..OperatorInterfaceSpec::default()
        };
        spec.resources = BTreeMap::from([(
            "cache_dir".to_string(),
            OperatorResourceSpec {
                exposed: false,
                ..OperatorResourceSpec::default()
            },
        )]);
        let argv = expand_argv(
            &spec,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            "/run",
        )
        .unwrap();
        assert_eq!(argv, vec!["Rscript", "plot.R", "", "", ""]);

        let canonical = canonicalize_inputs(
            &crate::domain::tools::ToolContext::new(tmp.path()),
            &spec,
            BTreeMap::from([("metadata".to_string(), json!(""))]),
            false,
        )
        .unwrap();
        assert!(!canonical.contains_key("metadata"));
    }

    #[test]
    fn validates_params_resources_and_container_runtime_support() {
        let tmp = TempDir::new().unwrap();
        let spec = OperatorSpec {
            api_version: OPERATOR_API_VERSION_V1ALPHA1.to_string(),
            kind: OPERATOR_KIND.to_string(),
            metadata: OperatorMetadata {
                id: "container_op".to_string(),
                version: "1".to_string(),
                name: None,
                description: None,
                tags: Vec::new(),
            },
            interface: OperatorInterfaceSpec {
                params: BTreeMap::from([(
                    "repeat".to_string(),
                    OperatorFieldSpec {
                        kind: OperatorFieldKind::Integer,
                        required: true,
                        minimum: Some(1.0),
                        maximum: Some(2.0),
                        ..OperatorFieldSpec::default()
                    },
                )]),
                ..OperatorInterfaceSpec::default()
            },
            smoke_tests: Vec::new(),
            execution: OperatorExecutionSpec {
                argv: vec!["true".to_string()],
            },
            preflight: None,
            runtime: Some(json!({
                "placement": { "supported": ["local"] },
                "container": { "supported": ["docker"] },
                "scheduler": { "supported": ["none"] }
            })),
            cache: None,
            resources: BTreeMap::from([(
                "cpu".to_string(),
                OperatorResourceSpec {
                    default: Some(json!(1)),
                    min: Some(json!(1)),
                    max: Some(json!(4)),
                    exposed: true,
                },
            )]),
            bindings: Vec::new(),
            permissions: None,
            source: OperatorSource {
                source_plugin: "p".to_string(),
                plugin_root: tmp.path().to_path_buf(),
                manifest_path: tmp.path().join("operator.yaml"),
            },
        };

        let docker_ctx = crate::domain::tools::ToolContext::new(tmp.path())
            .with_execution_environment("sandbox")
            .with_sandbox_backend("docker");
        assert!(runtime_supported(&docker_ctx, &spec));
        assert!(!runtime_supported(
            &crate::domain::tools::ToolContext::new(tmp.path()),
            &spec
        ));

        let params = BTreeMap::from([("repeat".to_string(), json!(3))]);
        let err = validate_field_values("params", &spec.interface.params, &params).unwrap_err();
        assert_eq!(err.field.as_deref(), Some("params.repeat"));

        let err = apply_resource_defaults_and_overrides(
            &spec,
            BTreeMap::from([("cpu".to_string(), json!(8))]),
        )
        .unwrap_err();
        assert_eq!(err.field.as_deref(), Some("resources.cpu"));
    }

    #[test]
    fn retry_policy_only_retries_retryable_infrastructure_errors() {
        let tmp = TempDir::new().unwrap();
        let spec = OperatorSpec {
            api_version: OPERATOR_API_VERSION_V1ALPHA1.to_string(),
            kind: OPERATOR_KIND.to_string(),
            metadata: OperatorMetadata {
                id: "retry_op".to_string(),
                version: "1".to_string(),
                name: None,
                description: None,
                tags: Vec::new(),
            },
            interface: OperatorInterfaceSpec::default(),
            smoke_tests: Vec::new(),
            execution: OperatorExecutionSpec {
                argv: vec!["true".to_string()],
            },
            preflight: None,
            runtime: Some(json!({
                "placement": { "supported": ["local"] },
                "container": { "supported": ["none"] },
                "scheduler": { "supported": ["none"] },
                "retry": { "maxAttempts": 4 }
            })),
            cache: None,
            resources: BTreeMap::new(),
            bindings: Vec::new(),
            permissions: None,
            source: OperatorSource {
                source_plugin: "p".to_string(),
                plugin_root: tmp.path().to_path_buf(),
                manifest_path: tmp.path().join("operator.yaml"),
            },
        };
        let policy = operator_retry_policy(&spec);
        assert_eq!(policy.max_attempts, 4);

        let infra = OperatorToolError::new("execution_infra_error", true, "backend failed");
        assert!(should_retry_operator_error(&infra, &policy, 1));
        assert!(!should_retry_operator_error(&infra, &policy, 4));

        let tool_exit = OperatorToolError::new("tool_exit_nonzero", false, "exit 2");
        assert!(!should_retry_operator_error(&tool_exit, &policy, 1));

        let validation = OperatorToolError::new("input_validation_failed", true, "bad input");
        assert!(!should_retry_operator_error(&validation, &policy, 1));
    }

    #[test]
    fn retry_metadata_is_recorded_in_status_and_failure_payloads() {
        let previous = OperatorRetryAttemptSummary {
            attempt: 1,
            kind: "environment_unavailable".to_string(),
            retryable: true,
            message: "temporary backend issue".to_string(),
        };
        let retry = OperatorRetryState {
            attempt: 2,
            max_attempts: 3,
            previous_errors: vec![previous.clone()],
        };
        let metadata = OperatorRunStatusMetadata {
            run_id: "oprun_20260507000000_retry".to_string(),
            location: "local".to_string(),
            operator: OperatorRunIdentity {
                alias: "retry_op".to_string(),
                id: "retry_op".to_string(),
                version: "1".to_string(),
                source_plugin: "p".to_string(),
                manifest_path: "/tmp/operator.yaml".to_string(),
            },
            run_dir: "/tmp/oprun_retry".to_string(),
            run_context: None,
            retry: Some(retry.clone()),
        };
        let mut status = json!({"status": "running"});
        apply_status_metadata(&mut status, Some(&metadata));
        assert_eq!(status["attempt"], json!(2));
        assert_eq!(status["maxAttempts"], json!(3));
        assert_eq!(
            status["previousErrors"][0]["kind"],
            "environment_unavailable"
        );

        let error = OperatorToolError::new("execution_infra_error", true, "backend failed")
            .with_retry_state(&retry);
        let raw = failure_json("retry_op", None, Some("/tmp/oprun_retry"), None, error);
        let payload = serde_json::from_str::<JsonValue>(&raw).unwrap();
        assert_eq!(payload["error"]["attempt"], json!(2));
        assert_eq!(payload["error"]["maxAttempts"], json!(3));
        assert_eq!(payload["error"]["previousErrors"][0]["attempt"], json!(1));
    }

    #[test]
    fn parses_remote_sha256_and_falls_back_to_reference_fingerprint() {
        let checksum = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let parsed = parse_remote_path_fingerprint(
            "ssh",
            "/remote/input.txt",
            &format!("__OMIGA_FILE__\n12\n1770000000\n{checksum}\n"),
        );
        assert_eq!(parsed["mode"], "sha256");
        assert_eq!(parsed["algorithm"], "sha256");
        assert_eq!(parsed["sha256"], checksum);
        assert_eq!(parsed["fingerprint"], format!("sha256:{checksum}"));
        assert_eq!(parsed["size"], json!(12));
        assert_eq!(parsed["modifiedUnixSecs"], json!(1770000000_u64));

        let missing =
            parse_remote_path_fingerprint("ssh", "/remote/missing.txt", "__OMIGA_MISSING__\n");
        assert_eq!(missing["mode"], "reference");
        assert_eq!(missing["available"], false);

        let no_checksum = parse_remote_path_fingerprint(
            "ssh",
            "/remote/input.txt",
            "__OMIGA_FILE__\n12\n1770000000\n\n",
        );
        assert_eq!(no_checksum["mode"], "stat");
        assert!(no_checksum.get("sha256").is_none());
    }

    #[test]
    fn ssh_operator_run_dirs_use_session_workspace_root() {
        let ctx = crate::domain::tools::ToolContext::new("/remote/work/data/query")
            .with_execution_environment("ssh")
            .with_ssh_server(Some("gpu".to_string()));

        let run_surface = OperatorExecutionSurface::for_context(&ctx, "oprun_123");
        assert_eq!(run_surface.kind, OperatorExecutionSurfaceKind::Ssh);
        assert_eq!(
            run_surface.run_dir,
            "/remote/work/data/query/.omiga/runs/oprun_123"
        );

        let runs_surface = OperatorExecutionSurface::for_runs_root(&ctx);
        assert_eq!(runs_surface.kind, OperatorExecutionSurfaceKind::Ssh);
        assert_eq!(runs_surface.run_dir, "/remote/work/data/query/.omiga/runs");
    }

    #[test]
    fn builds_docker_operator_command_for_local_container_runtime() {
        let tmp = TempDir::new().unwrap();
        let input = tmp.path().join("data.txt");
        fs::write(&input, "hello\n").unwrap();
        let spec = OperatorSpec {
            api_version: OPERATOR_API_VERSION_V1ALPHA1.to_string(),
            kind: OPERATOR_KIND.to_string(),
            metadata: OperatorMetadata {
                id: "container_op".to_string(),
                version: "1".to_string(),
                name: None,
                description: None,
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
                ..OperatorInterfaceSpec::default()
            },
            smoke_tests: Vec::new(),
            execution: OperatorExecutionSpec {
                argv: vec!["/bin/cat".to_string(), "${inputs.input}".to_string()],
            },
            preflight: None,
            runtime: Some(json!({
                "placement": { "supported": ["local"] },
                "container": {
                    "supported": ["docker"],
                    "image": "alpine:3.19"
                },
                "scheduler": { "supported": ["none"] }
            })),
            cache: None,
            resources: BTreeMap::new(),
            bindings: Vec::new(),
            permissions: None,
            source: OperatorSource {
                source_plugin: "p".to_string(),
                plugin_root: tmp.path().to_path_buf(),
                manifest_path: tmp.path().join("operator.yaml"),
            },
        };
        let ctx = crate::domain::tools::ToolContext::new(tmp.path()).with_sandbox_backend("docker");
        assert!(runtime_supported(&ctx, &spec));
        let inputs = BTreeMap::from([(
            "input".to_string(),
            JsonValue::String(input.to_string_lossy().into_owned()),
        )]);
        let command = operator_execution_command(
            &ctx,
            &spec,
            OperatorExecutionSurfaceKind::Local,
            "/tmp/oprun_container",
            &["/bin/cat".to_string(), input.to_string_lossy().into_owned()],
            &inputs,
        );

        assert!(command.contains("'docker' 'run' '--rm'"));
        assert!(command.contains("'alpine:3.19'"));
        assert!(command.contains("'/tmp/oprun_container:/tmp/oprun_container'"));
        assert!(command.contains(&format!(
            "'{}:{}:ro'",
            input.to_string_lossy(),
            input.to_string_lossy()
        )));
        assert!(command.contains("logs/stdout.txt"));
    }

    #[test]
    fn builds_singularity_operator_command_from_manifest_image() {
        let tmp = TempDir::new().unwrap();
        let spec = OperatorSpec {
            api_version: OPERATOR_API_VERSION_V1ALPHA1.to_string(),
            kind: OPERATOR_KIND.to_string(),
            metadata: OperatorMetadata {
                id: "singularity_op".to_string(),
                version: "1".to_string(),
                name: None,
                description: None,
                tags: Vec::new(),
            },
            interface: OperatorInterfaceSpec::default(),
            smoke_tests: Vec::new(),
            execution: OperatorExecutionSpec {
                argv: vec!["/bin/true".to_string()],
            },
            preflight: None,
            runtime: Some(json!({
                "placement": { "supported": ["local"] },
                "container": {
                    "supported": ["singularity"],
                    "images": { "singularity": "docker://alpine:3.19" }
                },
                "scheduler": { "supported": ["none"] }
            })),
            cache: None,
            resources: BTreeMap::new(),
            bindings: Vec::new(),
            permissions: None,
            source: OperatorSource {
                source_plugin: "p".to_string(),
                plugin_root: tmp.path().to_path_buf(),
                manifest_path: tmp.path().join("operator.yaml"),
            },
        };
        let ctx =
            crate::domain::tools::ToolContext::new(tmp.path()).with_sandbox_backend("singularity");
        assert!(runtime_supported(&ctx, &spec));

        let command = operator_execution_command(
            &ctx,
            &spec,
            OperatorExecutionSurfaceKind::Local,
            "/tmp/oprun_singularity",
            &["/bin/true".to_string()],
            &BTreeMap::new(),
        );

        assert!(command.contains("'singularity' 'exec'"));
        assert!(command.contains("'--pwd' '/tmp/oprun_singularity'"));
        assert!(command.contains("'docker://alpine:3.19'"));
        assert!(command.contains("logs/stdout.txt"));
    }

    fn write_conda_environment_profile(plugin_root: &Path, id: &str) {
        let env_dir = plugin_root.join("environments").join(id);
        fs::create_dir_all(&env_dir).unwrap();
        fs::write(
            env_dir.join("environment.yaml"),
            format!(
                r#"apiVersion: omiga.ai/environment/v1alpha1
kind: Environment
metadata:
  id: {id}
  version: 0.1.0
runtime:
  type: conda
  condaEnvFile: ./conda.yaml
diagnostics:
  checkCommand: [bwa, --version]
"#
            ),
        )
        .unwrap();
        fs::write(
            env_dir.join("conda.yaml"),
            "channels:\n  - conda-forge\n  - bioconda\ndependencies:\n  - bwa =0.7.17\n",
        )
        .unwrap();
    }

    #[test]
    fn local_operator_command_wraps_conda_environment_ref() {
        let tmp = TempDir::new().unwrap();
        write_conda_environment_profile(tmp.path(), "ngs-bwa");
        let mut spec = argv_operator_spec(&tmp, &["bwa", "index", "ref.fa"]);
        spec.runtime = Some(json!({
            "envRef": "ngs-bwa",
            "placement": { "supported": ["local"] },
            "container": { "supported": ["none"] }
        }));
        let ctx = crate::domain::tools::ToolContext::new(tmp.path());

        let command = operator_execution_command(
            &ctx,
            &spec,
            OperatorExecutionSurfaceKind::Local,
            "/tmp/oprun_conda",
            &spec.execution.argv,
            &BTreeMap::new(),
        );

        assert!(command.contains("$HOME/.omiga/bin/micromamba"));
        assert!(command.contains("OMIGA_MICROMAMBA"));
        assert!(command.contains("active PATH/base environment/virtual environment"));
        assert!(command.contains("env create -y -p"));
        assert!(command.contains("run -p"));
        assert!(command.contains(".omiga/operator-envs/conda"));
        assert!(command.contains("bwa"));
        assert!(command.contains("ref.fa"));
    }

    #[test]
    fn conda_environment_ref_rejects_non_yaml_environment_file() {
        let tmp = TempDir::new().unwrap();
        let env_dir = tmp.path().join("environments").join("bad-conda");
        fs::create_dir_all(&env_dir).unwrap();
        fs::write(
            env_dir.join("environment.yaml"),
            r#"apiVersion: omiga.ai/environment/v1alpha1
kind: Environment
metadata:
  id: bad-conda
  version: 0.1.0
runtime:
  type: conda
  condaEnvFile: ./requirements.txt
"#,
        )
        .unwrap();
        fs::write(env_dir.join("requirements.txt"), "bwa\n").unwrap();
        let mut spec = argv_operator_spec(&tmp, &["bwa", "index", "ref.fa"]);
        spec.runtime = Some(json!({
            "envRef": "bad-conda",
            "placement": { "supported": ["local"] }
        }));
        let ctx = crate::domain::tools::ToolContext::new(tmp.path());

        let command = operator_execution_command(
            &ctx,
            &spec,
            OperatorExecutionSurfaceKind::Local,
            "/tmp/oprun_bad_conda",
            &spec.execution.argv,
            &BTreeMap::new(),
        );

        assert!(command.contains("must use a `.yaml` or `.yml` file"));
        assert!(command.contains("requirements.txt"));
    }

    #[test]
    fn environment_profile_can_select_container_runtime() {
        let tmp = TempDir::new().unwrap();
        let env_dir = tmp.path().join("environments").join("docker-env");
        fs::create_dir_all(&env_dir).unwrap();
        fs::write(
            env_dir.join("environment.yaml"),
            r#"apiVersion: omiga.ai/environment/v1alpha1
kind: Environment
metadata:
  id: docker-env
  version: 0.1.0
runtime:
  type: docker
  image: alpine:3.19
"#,
        )
        .unwrap();
        let mut spec = argv_operator_spec(&tmp, &["echo", "hello"]);
        spec.runtime = Some(json!({
            "envRef": "docker-env",
            "placement": { "supported": ["local"] }
        }));
        let ctx = crate::domain::tools::ToolContext::new(tmp.path());

        let selection =
            operator_container_for_command(&ctx, &spec, OperatorExecutionSurfaceKind::Local)
                .expect("container selection");

        assert_eq!(selection.kind, OperatorContainerKind::Docker);
        assert_eq!(selection.image, "alpine:3.19");
    }

    #[test]
    fn docker_environment_profile_builds_from_standard_dockerfile() {
        let tmp = TempDir::new().unwrap();
        let env_dir = tmp.path().join("environments").join("docker-env");
        fs::create_dir_all(&env_dir).unwrap();
        fs::write(
            env_dir.join("environment.yaml"),
            r#"apiVersion: omiga.ai/environment/v1alpha1
kind: Environment
metadata:
  id: docker-env
  version: 0.1.0
runtime:
  type: docker
  dockerfile: ./Dockerfile
"#,
        )
        .unwrap();
        fs::write(env_dir.join("Dockerfile"), "FROM alpine:3.19\n").unwrap();
        let mut spec = argv_operator_spec(&tmp, &["echo", "hello"]);
        spec.runtime = Some(json!({
            "envRef": "docker-env",
            "placement": { "supported": ["local"] }
        }));
        let ctx = crate::domain::tools::ToolContext::new(tmp.path());

        let command = operator_execution_command(
            &ctx,
            &spec,
            OperatorExecutionSurfaceKind::Local,
            "/tmp/oprun_docker_env",
            &spec.execution.argv,
            &BTreeMap::new(),
        );

        assert!(command.contains("command -v docker"));
        assert!(command.contains("docker version"));
        assert!(command.contains("docker build -t"));
        assert!(command.contains("'docker' 'run'"));
        assert!(command.contains("omiga-env-"));
        assert!(command.contains("docker-env"));
        assert!(command.contains("logs/stderr.txt"));
    }

    #[test]
    fn singularity_environment_profile_builds_from_standard_definition() {
        let tmp = TempDir::new().unwrap();
        let env_dir = tmp.path().join("environments").join("singularity-env");
        fs::create_dir_all(&env_dir).unwrap();
        fs::write(
            env_dir.join("environment.yaml"),
            r#"apiVersion: omiga.ai/environment/v1alpha1
kind: Environment
metadata:
  id: singularity-env
  version: 0.1.0
runtime:
  type: singularity
  definitionFile: ./singularity.def
"#,
        )
        .unwrap();
        fs::write(
            env_dir.join("singularity.def"),
            "Bootstrap: docker\nFrom: alpine:3.19\n",
        )
        .unwrap();
        let mut spec = argv_operator_spec(&tmp, &["echo", "hello"]);
        spec.runtime = Some(json!({
            "envRef": "singularity-env",
            "placement": { "supported": ["local"] }
        }));
        let ctx = crate::domain::tools::ToolContext::new(tmp.path());

        let command = operator_execution_command(
            &ctx,
            &spec,
            OperatorExecutionSurfaceKind::Local,
            "/tmp/oprun_singularity_env",
            &spec.execution.argv,
            &BTreeMap::new(),
        );

        assert!(command.contains("command -v singularity"));
        assert!(command.contains("command -v apptainer"));
        assert!(command.contains("singularity build"));
        assert!(command.contains("'singularity' 'exec'"));
        assert!(command.contains(".omiga/operator-envs/singularity"));
    }

    #[test]
    fn container_environment_profile_without_image_or_file_reports_guidance() {
        let tmp = TempDir::new().unwrap();
        let env_dir = tmp.path().join("environments").join("docker-env");
        fs::create_dir_all(&env_dir).unwrap();
        fs::write(
            env_dir.join("environment.yaml"),
            r#"apiVersion: omiga.ai/environment/v1alpha1
kind: Environment
metadata:
  id: docker-env
  version: 0.1.0
runtime:
  type: docker
"#,
        )
        .unwrap();
        let mut spec = argv_operator_spec(&tmp, &["echo", "hello"]);
        spec.runtime = Some(json!({
            "envRef": "docker-env",
            "placement": { "supported": ["local"] }
        }));
        let ctx = crate::domain::tools::ToolContext::new(tmp.path());

        let command = operator_execution_command(
            &ctx,
            &spec,
            OperatorExecutionSurfaceKind::Local,
            "/tmp/oprun_missing_container_env",
            &spec.execution.argv,
            &BTreeMap::new(),
        );

        assert!(command.contains("requires runtime.image or a standard `Dockerfile`"));
        assert!(!command.contains("'echo' 'hello' > logs/stdout.txt"));
    }

    #[test]
    fn ngs_alignment_wrapper_smoke_fixture_runs_with_mock_tools() {
        let (plugin_root, _) = bundled_plugin_operator_manifest_path("ngs-alignment", "bwa-index");
        let script = plugin_root
            .join("scripts")
            .join("test_ngs_alignment_smoke.py");

        let output = std::process::Command::new("python3")
            .arg(&script)
            .env("PYTHONDONTWRITEBYTECODE", "1")
            .current_dir(plugin_root)
            .output()
            .expect("run ngs alignment smoke test");

        assert!(
            output.status.success(),
            "ngs alignment smoke failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("smoke fixture passed"));
    }

    #[test]
    fn local_runtime_prefers_no_container_when_manifest_allows_none() {
        let tmp = TempDir::new().unwrap();
        let spec = OperatorSpec {
            api_version: OPERATOR_API_VERSION_V1ALPHA1.to_string(),
            kind: OPERATOR_KIND.to_string(),
            metadata: OperatorMetadata {
                id: "portable_op".to_string(),
                version: "1".to_string(),
                name: None,
                description: None,
                tags: Vec::new(),
            },
            interface: OperatorInterfaceSpec::default(),
            smoke_tests: Vec::new(),
            execution: OperatorExecutionSpec {
                argv: vec!["/bin/true".to_string()],
            },
            preflight: None,
            runtime: Some(json!({
                "placement": { "supported": ["local"] },
                "container": { "supported": ["none", "docker"] },
                "scheduler": { "supported": ["none"] }
            })),
            cache: None,
            resources: BTreeMap::new(),
            bindings: Vec::new(),
            permissions: None,
            source: OperatorSource {
                source_plugin: "p".to_string(),
                plugin_root: tmp.path().to_path_buf(),
                manifest_path: tmp.path().join("operator.yaml"),
            },
        };
        let ctx = crate::domain::tools::ToolContext::new(tmp.path()).with_sandbox_backend("docker");
        assert!(runtime_supported(&ctx, &spec));

        let command = operator_execution_command(
            &ctx,
            &spec,
            OperatorExecutionSurfaceKind::Local,
            "/tmp/oprun_none",
            &["/bin/true".to_string()],
            &BTreeMap::new(),
        );

        assert!(!command.contains("'docker' 'run'"));
        assert!(command.starts_with("set +e"));
    }

    #[test]
    fn lists_and_reads_local_operator_runs() {
        let tmp = TempDir::new().unwrap();
        let runs_root = tmp.path().join(".omiga/runs");
        let succeeded = runs_root.join("oprun_20260506_success");
        let failed = runs_root.join("oprun_20260506_failed");
        fs::create_dir_all(&succeeded).unwrap();
        fs::create_dir_all(&failed).unwrap();
        write_json_file(
            &succeeded.join("provenance.json"),
            &json!({
                "status": "succeeded",
                "runId": "oprun_20260506_success",
                "operator": {
                    "alias": "write_text_report",
                    "id": "write_text_report",
                    "version": "0.1.0",
                    "sourcePlugin": "operator-smoke@omiga-curated"
                },
                "runDir": succeeded.to_string_lossy(),
                "provenancePath": succeeded.join("provenance.json").to_string_lossy(),
                "outputs": {
                    "report": [
                        { "location": "local", "path": succeeded.join("out/report.txt").to_string_lossy() }
                    ]
                },
                "structuredOutputs": {
                    "summary": { "lineCount": 2 },
                    "ok": true
                },
                "cache": {
                    "key": "sha256:test-cache-key",
                    "hit": true,
                    "sourceRunId": "oprun_20260506_source",
                    "sourceRunDir": succeeded.parent().unwrap().join("oprun_20260506_source").to_string_lossy()
                }
            }),
        )
        .unwrap();
        write_json_file(
            &succeeded.join("status.json"),
            &json!({
                "status": "succeeded",
                "updatedAt": "2026-05-06T12:00:00Z",
                "error": null
            }),
        )
        .unwrap();
        write_json_file(
            &failed.join("status.json"),
            &json!({
                "status": "failed",
                "updatedAt": "2026-05-06T11:00:00Z",
                "operator": {
                    "alias": "write_text_report",
                    "id": "write_text_report",
                    "version": "0.1.0",
                    "sourcePlugin": "operator-smoke@omiga-curated"
                },
                "runContext": {
                    "kind": "smoke",
                    "smokeTestId": "default",
                    "smokeTestName": "Write text report smoke"
                },
                "error": {
                    "kind": "tool_exit_nonzero",
                    "retryable": false,
                    "message": "bad input",
                    "suggestedAction": "Inspect stdout/stderr, then adjust inputs or params and retry.",
                    "stdoutTail": "partial stdout\n",
                    "stderrTail": "bad flag\n"
                }
            }),
        )
        .unwrap();

        let runs = list_local_operator_runs(tmp.path(), 10);
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].run_id, "oprun_20260506_success");
        assert_eq!(runs[0].operator_alias.as_deref(), Some("write_text_report"));
        assert_eq!(
            runs[0].source_plugin.as_deref(),
            Some("operator-smoke@omiga-curated")
        );
        assert_eq!(runs[0].output_count, 1);
        assert_eq!(runs[0].structured_output_count, 2);
        assert_eq!(runs[0].cache_key.as_deref(), Some("sha256:test-cache-key"));
        assert_eq!(runs[0].cache_hit, Some(true));
        assert_eq!(
            runs[0].cache_source_run_id.as_deref(),
            Some("oprun_20260506_source")
        );
        assert!(runs[0]
            .cache_source_run_dir
            .as_deref()
            .unwrap_or_default()
            .ends_with("oprun_20260506_source"));
        assert_eq!(runs[1].status, "failed");
        assert_eq!(runs[1].operator_alias.as_deref(), Some("write_text_report"));
        assert_eq!(runs[1].run_kind.as_deref(), Some("smoke"));
        assert_eq!(runs[1].smoke_test_id.as_deref(), Some("default"));
        assert_eq!(runs[1].error_message.as_deref(), Some("bad input"));
        assert_eq!(runs[1].error_kind.as_deref(), Some("tool_exit_nonzero"));
        assert_eq!(runs[1].retryable, Some(false));
        assert_eq!(
            runs[1].suggested_action.as_deref(),
            Some("Inspect stdout/stderr, then adjust inputs or params and retry.")
        );
        assert_eq!(runs[1].stdout_tail.as_deref(), Some("partial stdout\n"));
        assert_eq!(runs[1].stderr_tail.as_deref(), Some("bad flag\n"));

        let detail = read_local_operator_run(tmp.path(), "oprun_20260506_success").unwrap();
        assert_eq!(detail["operator"]["id"], "write_text_report");
        assert!(read_local_operator_run(tmp.path(), "../oprun_escape").is_err());
    }

    #[tokio::test]
    async fn reads_local_operator_run_detail_and_log_through_context() {
        let tmp = TempDir::new().unwrap();
        let run_id = "oprun_20260506_detail";
        let run_dir = tmp.path().join(".omiga/runs").join(run_id);
        fs::create_dir_all(run_dir.join("logs")).unwrap();
        fs::create_dir_all(run_dir.join("out")).unwrap();
        fs::write(run_dir.join("logs/stdout.txt"), "operator stdout\n").unwrap();
        fs::write(run_dir.join("logs/stderr.txt"), "").unwrap();
        fs::write(run_dir.join("out/report.txt"), "hello\n").unwrap();
        write_json_file(
            &run_dir.join("provenance.json"),
            &json!({
                "status": "succeeded",
                "runId": run_id,
                "location": "local",
                "operator": {
                    "alias": "write_text_report",
                    "id": "write_text_report",
                    "version": "0.1.0",
                    "sourcePlugin": "operator-smoke@omiga-curated"
                },
                "runDir": run_dir.to_string_lossy(),
                "provenancePath": run_dir.join("provenance.json").to_string_lossy(),
                "outputs": {
                    "report": [
                        { "location": "local", "path": run_dir.join("out/report.txt").to_string_lossy() }
                    ]
                }
            }),
        )
        .unwrap();

        let ctx = crate::domain::tools::ToolContext::new(tmp.path());
        let runs = list_operator_runs_for_context(&ctx, 10).await.unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].run_id, run_id);

        let detail = read_operator_run_for_context(&ctx, run_id).await.unwrap();
        assert_eq!(detail.location, "local");
        assert_eq!(detail.document["runId"], run_id);
        assert!(detail.source_path.ends_with("provenance.json"));

        let log = read_operator_run_log_for_context(&ctx, run_id, "stdout", 1024)
            .await
            .unwrap();
        assert_eq!(log.location, "local");
        assert_eq!(log.log_name, "stdout");
        assert_eq!(log.content, "operator stdout\n");

        let verification = verify_operator_run_for_context(&ctx, run_id).await.unwrap();
        assert!(verification.ok);
        assert!(verification
            .checks
            .iter()
            .any(|check| check.name == "output_artifact:report" && check.ok));
    }

    #[tokio::test]
    async fn cleanup_operator_runs_previews_and_deletes_workspace_scoped_candidates() {
        fn write_run(
            root: &Path,
            run_id: &str,
            status: &str,
            updated_at: &str,
            cache: Option<JsonValue>,
            operator_id: Option<&str>,
        ) -> PathBuf {
            let operator_id = operator_id.unwrap_or("write_text_report");
            let run_dir = root.join(".omiga/runs").join(run_id);
            fs::create_dir_all(run_dir.join("out")).unwrap();
            fs::create_dir_all(run_dir.join("logs")).unwrap();
            fs::write(run_dir.join("out/report.txt"), format!("{run_id}\n")).unwrap();
            fs::write(run_dir.join("logs/stdout.txt"), "").unwrap();
            fs::write(run_dir.join("logs/stderr.txt"), "").unwrap();
            let mut document = json!({
                "status": status,
                "runId": run_id,
                "location": "local",
                "operator": {
                    "alias": operator_id,
                    "id": operator_id,
                    "version": "0.1.0",
                    "sourcePlugin": "operator-smoke@omiga-curated"
                },
                "runDir": run_dir.to_string_lossy(),
                "provenancePath": run_dir.join("provenance.json").to_string_lossy(),
                "outputs": {
                    "report": [
                        { "location": "local", "path": run_dir.join("out/report.txt").to_string_lossy() }
                    ]
                }
            });
            if let Some(cache) = cache {
                document["cache"] = cache;
            }
            write_json_file(&run_dir.join("provenance.json"), &document).unwrap();
            write_json_file(
                &run_dir.join("status.json"),
                &json!({
                    "status": status,
                    "updatedAt": updated_at
                }),
            )
            .unwrap();
            run_dir
        }

        let tmp = TempDir::new().unwrap();
        let latest = write_run(
            tmp.path(),
            "oprun_20990101_latest",
            "succeeded",
            "2099-01-01T00:00:00Z",
            None,
            None,
        );
        let old_success = write_run(
            tmp.path(),
            "oprun_20000101_success",
            "succeeded",
            "2000-01-01T00:00:00Z",
            None,
            None,
        );
        let old_failed = write_run(
            tmp.path(),
            "oprun_20000101_failed",
            "failed",
            "2000-01-01T00:00:00Z",
            None,
            None,
        );
        let cache_hit = write_run(
            tmp.path(),
            "oprun_20000101_cache",
            "succeeded",
            "2000-01-02T00:00:00Z",
            Some(json!({
                "key": "sha256:test",
                "hit": true,
                "sourceRunId": "oprun_20000101_success",
                "sourceRunDir": old_success.to_string_lossy()
            })),
            None,
        );
        let other_operator = write_run(
            tmp.path(),
            "oprun_20000101_other",
            "succeeded",
            "2000-01-01T00:00:00Z",
            None,
            Some("other_operator"),
        );
        let ctx = crate::domain::tools::ToolContext::new(tmp.path());
        let request = OperatorRunCleanupRequest {
            dry_run: true,
            keep_latest: Some(1),
            max_age_days: Some(30),
            include_cache_hits: true,
            include_failed: true,
            include_succeeded: true,
            limit: Some(50),
            operator_alias: None,
            operator_id: Some("write_text_report".to_string()),
            operator_version: Some("0.1.0".to_string()),
            source_plugin: Some("operator-smoke@omiga-curated".to_string()),
        };

        let preview = cleanup_operator_runs_for_context(&ctx, request.clone())
            .await
            .unwrap();
        assert!(preview.dry_run);
        assert_eq!(preview.location, "local");
        assert_eq!(preview.scanned_count, 5);
        assert_eq!(preview.matched_count, 3);
        assert_eq!(preview.deleted_count, 0);
        assert!(preview.estimated_bytes.unwrap_or_default() > 0);
        let preview_ids = preview
            .candidates
            .iter()
            .map(|candidate| candidate.run_id.as_str())
            .collect::<BTreeSet<_>>();
        assert_eq!(
            preview_ids,
            BTreeSet::from([
                "oprun_20000101_cache",
                "oprun_20000101_failed",
                "oprun_20000101_success",
            ])
        );
        assert!(latest.is_dir());
        assert!(old_success.is_dir());
        assert!(old_failed.is_dir());
        assert!(cache_hit.is_dir());
        assert!(other_operator.is_dir());

        let result = cleanup_operator_runs_for_context(
            &ctx,
            OperatorRunCleanupRequest {
                dry_run: false,
                ..request
            },
        )
        .await
        .unwrap();
        assert_eq!(result.deleted_count, 3);
        assert_eq!(result.skipped_count, 0);
        assert!(latest.is_dir());
        assert!(!old_success.exists());
        assert!(!old_failed.exists());
        assert!(!cache_hit.exists());
        assert!(other_operator.is_dir());
    }

    #[tokio::test]
    async fn executes_bundled_smoke_operator_locally() {
        let tmp = TempDir::new().unwrap();
        let (plugin_root, manifest) = bundled_smoke_operator_paths();
        let spec =
            load_operator_manifest(&manifest, "operator-smoke@omiga-curated", plugin_root).unwrap();
        let smoke_invocation = spec.smoke_tests[0].arguments.clone();
        let ctx = crate::domain::tools::ToolContext::new(tmp.path());

        let result = execute_resolved_operator(
            &ctx,
            ResolvedOperator {
                alias: "write_text_report".to_string(),
                spec,
            },
            smoke_invocation,
            Some(OperatorRunContext {
                kind: Some("smoke".to_string()),
                smoke_test_id: Some("default".to_string()),
                smoke_test_name: Some("Write text report smoke".to_string()),
                parent_execution_id: None,
            }),
        )
        .await
        .unwrap();

        assert_eq!(result.status, "succeeded");
        assert_eq!(
            result
                .run_context
                .as_ref()
                .and_then(|context| context.kind.as_deref()),
            Some("smoke")
        );
        let runs = list_local_operator_runs(tmp.path(), 10);
        assert_eq!(runs[0].run_kind.as_deref(), Some("smoke"));
        assert_eq!(runs[0].smoke_test_id.as_deref(), Some("default"));
        let report_path = Path::new(&result.outputs["report"][0].path);
        assert_eq!(
            fs::read_to_string(report_path).unwrap(),
            "hello operator smoke\nhello operator smoke\n"
        );
        let export_dir = result
            .export_dir
            .as_deref()
            .expect("successful runs should export results to the session workspace");
        assert_eq!(
            fs::read_to_string(Path::new(export_dir).join("operator-report.txt")).unwrap(),
            "hello operator smoke\nhello operator smoke\n"
        );
        assert!(Path::new(&format!("{}/status.json", result.run_dir)).is_file());
        assert!(Path::new(&format!("{}/provenance.json", result.run_dir)).is_file());
    }

    #[tokio::test]
    async fn cacheable_local_operator_reuses_workspace_run_outputs() {
        let tmp = TempDir::new().unwrap();
        let input = tmp.path().join("input.txt");
        let marker = tmp.path().join("executions.txt");
        fs::write(&input, "first\n").unwrap();
        let spec = cached_report_operator_spec(&tmp, &marker, Some(json!({"enabled": true})));
        let ctx = crate::domain::tools::ToolContext::new(tmp.path());
        let invocation = input_file_invocation("input.txt");

        let first = execute_resolved_operator(
            &ctx,
            ResolvedOperator {
                alias: "cached_report".to_string(),
                spec: spec.clone(),
            },
            invocation.clone(),
            None,
        )
        .await
        .unwrap();
        assert_eq!(first.status, "succeeded");
        assert_eq!(
            first.cache.as_ref().map(|cache| cache.hit),
            Some(false),
            "fresh cache-enabled runs should record their cache key without claiming a hit"
        );
        assert!(first
            .run_dir
            .starts_with(&tmp.path().join(".omiga/runs").to_string_lossy().to_string()));
        assert_eq!(fs::read_to_string(&marker).unwrap(), "run\n");

        let second = execute_resolved_operator(
            &ctx,
            ResolvedOperator {
                alias: "cached_report".to_string(),
                spec: spec.clone(),
            },
            invocation.clone(),
            None,
        )
        .await
        .unwrap();
        assert_eq!(second.status, "succeeded");
        assert_ne!(second.run_id, first.run_id);
        let second_cache = second.cache.as_ref().expect("second run cache metadata");
        assert!(second_cache.hit);
        assert_eq!(
            second_cache.source_run_id.as_deref(),
            Some(first.run_id.as_str())
        );
        assert_eq!(
            second_cache.source_run_dir.as_deref(),
            Some(first.run_dir.as_str())
        );
        assert_eq!(
            second.outputs["report"][0].path, first.outputs["report"][0].path,
            "cache hits reuse the prior workspace artifact reference instead of copying outputs"
        );
        assert_eq!(
            fs::read_to_string(&marker).unwrap(),
            "run\n",
            "cache hit must not execute the operator command again"
        );
        let second_stdout = fs::read_to_string(Path::new(&second.run_dir).join("logs/stdout.txt"))
            .expect("cache hit stdout log");
        assert!(
            second_stdout.contains(&format!("Operator cache hit: reused run {}.", first.run_id))
        );

        fs::write(&input, "changed\n").unwrap();
        let third = execute_resolved_operator(
            &ctx,
            ResolvedOperator {
                alias: "cached_report".to_string(),
                spec,
            },
            invocation,
            None,
        )
        .await
        .unwrap();
        assert_eq!(third.status, "succeeded");
        assert_eq!(third.cache.as_ref().map(|cache| cache.hit), Some(false));
        assert_ne!(
            third.outputs["report"][0].path,
            first.outputs["report"][0].path
        );
        assert_eq!(
            fs::read_to_string(&marker).unwrap(),
            "run\nrun\n",
            "changed input fingerprint should miss the workspace cache"
        );
    }

    #[tokio::test]
    async fn smoke_operator_runs_bypass_cache() {
        let tmp = TempDir::new().unwrap();
        let input = tmp.path().join("input.txt");
        let marker = tmp.path().join("smoke-executions.txt");
        fs::write(&input, "smoke\n").unwrap();
        let spec = cached_report_operator_spec(&tmp, &marker, Some(json!({"enabled": true})));
        let ctx = crate::domain::tools::ToolContext::new(tmp.path());
        let invocation = input_file_invocation("input.txt");
        let run_context = Some(OperatorRunContext {
            kind: Some("smoke".to_string()),
            smoke_test_id: Some("default".to_string()),
            smoke_test_name: Some("Cache bypass smoke".to_string()),
            parent_execution_id: None,
        });

        let first = execute_resolved_operator(
            &ctx,
            ResolvedOperator {
                alias: "cached_report".to_string(),
                spec: spec.clone(),
            },
            invocation.clone(),
            run_context.clone(),
        )
        .await
        .unwrap();
        let second = execute_resolved_operator(
            &ctx,
            ResolvedOperator {
                alias: "cached_report".to_string(),
                spec,
            },
            invocation,
            run_context,
        )
        .await
        .unwrap();

        assert!(first.cache.is_none());
        assert!(second.cache.is_none());
        assert_ne!(second.run_id, first.run_id);
        assert_ne!(
            second.outputs["report"][0].path,
            first.outputs["report"][0].path
        );
        assert_eq!(fs::read_to_string(&marker).unwrap(), "run\nrun\n");
    }

    #[tokio::test]
    async fn executes_pubmed_operator_with_offline_fixture() {
        let tmp = TempDir::new().unwrap();
        let (plugin_root, manifest) =
            bundled_plugin_operator_manifest_path("operator-pubmed-search", "pubmed-search");
        let spec = load_operator_manifest(
            &manifest,
            "operator-pubmed-search@omiga-curated",
            plugin_root.clone(),
        )
        .unwrap();
        let fixture = plugin_root.join("examples").join("pubmed_fixture.json");
        let invocation = OperatorInvocation {
            inputs: BTreeMap::new(),
            params: BTreeMap::from([
                ("query".to_string(), json!("TP53 cancer")),
                ("limit".to_string(), json!(2)),
                ("mode".to_string(), json!("offline_fixture")),
                (
                    "fixture_json".to_string(),
                    json!(fixture.to_string_lossy().to_string()),
                ),
                ("email".to_string(), json!("")),
            ]),
            resources: BTreeMap::new(),
            metadata: BTreeMap::new(),
        };
        let ctx = crate::domain::tools::ToolContext::new(tmp.path());

        let result = execute_resolved_operator(
            &ctx,
            ResolvedOperator {
                alias: "pubmed_search".to_string(),
                spec,
            },
            invocation,
            Some(OperatorRunContext {
                kind: Some("operator-pilot".to_string()),
                smoke_test_id: Some("offline-fixture".to_string()),
                smoke_test_name: Some("PubMed offline fixture".to_string()),
                parent_execution_id: None,
            }),
        )
        .await
        .unwrap();

        assert_eq!(result.status, "succeeded");
        let structured_outputs = result.structured_outputs.as_ref().unwrap();
        assert_eq!(structured_outputs["summary"]["count"], 2);
        assert_eq!(structured_outputs["summary"]["mode"], "offline_fixture");
        let results_path = Path::new(&result.outputs["results"][0].path);
        let results = fs::read_to_string(results_path).expect("results");
        assert!(results.contains("31452104"));
        assert!(results.contains("25772236"));
    }

    #[tokio::test]
    async fn executes_geo_operator_with_offline_fixture() {
        let tmp = TempDir::new().unwrap();
        let (plugin_root, manifest) =
            bundled_plugin_operator_manifest_path("operator-geo-search", "geo-search");
        let spec = load_operator_manifest(
            &manifest,
            "operator-geo-search@omiga-curated",
            plugin_root.clone(),
        )
        .unwrap();
        let fixture = plugin_root.join("examples").join("geo_fixture.json");
        let invocation = OperatorInvocation {
            inputs: BTreeMap::new(),
            params: BTreeMap::from([
                ("query".to_string(), json!("TP53 cancer")),
                ("limit".to_string(), json!(2)),
                ("mode".to_string(), json!("offline_fixture")),
                (
                    "fixture_json".to_string(),
                    json!(fixture.to_string_lossy().to_string()),
                ),
                ("email".to_string(), json!("")),
            ]),
            resources: BTreeMap::new(),
            metadata: BTreeMap::new(),
        };
        let ctx = crate::domain::tools::ToolContext::new(tmp.path());

        let result = execute_resolved_operator(
            &ctx,
            ResolvedOperator {
                alias: "geo_search".to_string(),
                spec,
            },
            invocation,
            Some(OperatorRunContext {
                kind: Some("operator-pilot".to_string()),
                smoke_test_id: Some("offline-fixture".to_string()),
                smoke_test_name: Some("GEO offline fixture".to_string()),
                parent_execution_id: None,
            }),
        )
        .await
        .unwrap();

        assert_eq!(result.status, "succeeded");
        let structured_outputs = result.structured_outputs.as_ref().unwrap();
        assert_eq!(structured_outputs["summary"]["count"], 2);
        assert_eq!(structured_outputs["summary"]["mode"], "offline_fixture");
        let results_path = Path::new(&result.outputs["results"][0].path);
        let results = fs::read_to_string(results_path).expect("results");
        assert!(results.contains("GSE123456"));
        assert!(results.contains("GSE123457"));
    }

    #[tokio::test]
    async fn executes_uniprot_operator_with_offline_fixture() {
        let tmp = TempDir::new().unwrap();
        let (plugin_root, manifest) =
            bundled_plugin_operator_manifest_path("operator-uniprot-search", "uniprot-search");
        let spec = load_operator_manifest(
            &manifest,
            "operator-uniprot-search@omiga-curated",
            plugin_root.clone(),
        )
        .unwrap();
        let fixture = plugin_root.join("examples").join("uniprot_fixture.json");
        let invocation = OperatorInvocation {
            inputs: BTreeMap::new(),
            params: BTreeMap::from([
                ("query".to_string(), json!("TP53")),
                ("limit".to_string(), json!(2)),
                ("mode".to_string(), json!("offline_fixture")),
                (
                    "fixture_json".to_string(),
                    json!(fixture.to_string_lossy().to_string()),
                ),
                ("organism".to_string(), json!("")),
                ("taxon_id".to_string(), json!("9606")),
                ("reviewed".to_string(), json!("true")),
            ]),
            resources: BTreeMap::new(),
            metadata: BTreeMap::new(),
        };
        let ctx = crate::domain::tools::ToolContext::new(tmp.path());

        let result = execute_resolved_operator(
            &ctx,
            ResolvedOperator {
                alias: "uniprot_search".to_string(),
                spec,
            },
            invocation,
            Some(OperatorRunContext {
                kind: Some("operator-pilot".to_string()),
                smoke_test_id: Some("offline-fixture".to_string()),
                smoke_test_name: Some("UniProt offline fixture".to_string()),
                parent_execution_id: None,
            }),
        )
        .await
        .unwrap();

        assert_eq!(result.status, "succeeded");
        let structured_outputs = result.structured_outputs.as_ref().unwrap();
        assert_eq!(structured_outputs["summary"]["count"], 2);
        assert_eq!(structured_outputs["summary"]["mode"], "offline_fixture");
        let results_path = Path::new(&result.outputs["results"][0].path);
        let results = fs::read_to_string(results_path).expect("results");
        assert!(results.contains("P04637"));
        assert!(results.contains("P38398"));
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
                id: "render_report".to_string(),
                version: "1".to_string(),
                name: None,
                description: Some("render input to report".to_string()),
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
                        glob: Some("report.txt".to_string()),
                        ..OperatorFieldSpec::default()
                    },
                ), (
                    "summary".to_string(),
                    OperatorFieldSpec {
                        kind: OperatorFieldKind::Json,
                        required: true,
                        ..OperatorFieldSpec::default()
                    },
                ), (
                    "ok".to_string(),
                    OperatorFieldSpec {
                        kind: OperatorFieldKind::Boolean,
                        required: true,
                        ..OperatorFieldSpec::default()
                    },
                )]),
                ..OperatorInterfaceSpec::default()
            },
            smoke_tests: Vec::new(),
            execution: OperatorExecutionSpec {
                argv: vec![
                    "/bin/sh".to_string(),
                    "-c".to_string(),
                    r#"cat ${inputs.input} > ${outdir}/report.txt; printf '%s\n' '{"summary":{"lineCount":1},"ok":true}' > ${outdir}/outputs.json"#.to_string(),
                ],
            },
            preflight: None,
            runtime: None,
            cache: None,
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
                alias: "render_report".to_string(),
                spec,
            },
            OperatorInvocation {
                inputs: BTreeMap::from([(
                    "input".to_string(),
                    JsonValue::String("input.txt".to_string()),
                )]),
                params: BTreeMap::new(),
                resources: BTreeMap::new(),
                metadata: BTreeMap::new(),
            },
            None,
        )
        .await
        .unwrap();
        assert_eq!(result.status, "succeeded");
        assert_eq!(result.outputs["report"].len(), 1);
        assert!(Path::new(&result.outputs["report"][0].path).is_file());
        let structured_outputs = result.structured_outputs.as_ref().unwrap();
        assert_eq!(structured_outputs["summary"]["lineCount"], json!(1));
        assert_eq!(structured_outputs["ok"], json!(true));
        assert_eq!(
            result.effective_inputs["input"],
            json!(input.canonicalize().unwrap().to_string_lossy().into_owned())
        );
        assert_eq!(result.input_fingerprints["input"]["mode"], "sha256");
        assert_eq!(result.input_fingerprints["input"]["location"], "local");
        assert_eq!(result.input_fingerprints["input"]["algorithm"], "sha256");
        let expected_sha256 = sha256_file(&input.to_string_lossy()).unwrap();
        assert_eq!(
            result.input_fingerprints["input"]["sha256"],
            expected_sha256
        );
        assert_eq!(
            result.input_fingerprints["input"]["fingerprint"],
            format!("sha256:{expected_sha256}")
        );
    }

    #[test]
    fn rejects_invalid_local_structured_output_manifest() {
        let tmp = TempDir::new().unwrap();
        let run_dir = tmp.path().join(".omiga/runs/oprun_structured_invalid");
        let out_dir = run_dir.join("out");
        fs::create_dir_all(&out_dir).unwrap();

        fs::write(out_dir.join(OPERATOR_STRUCTURED_OUTPUTS_FILE), "[]").unwrap();
        let error =
            read_local_structured_outputs(&out_dir, &run_dir.to_string_lossy()).unwrap_err();
        assert_eq!(error.kind, "output_validation_failed");
        assert_eq!(error.field.as_deref(), Some("structuredOutputs"));
        assert!(error.message.contains("JSON object"));

        fs::write(out_dir.join(OPERATOR_STRUCTURED_OUTPUTS_FILE), "{not json").unwrap();
        let error =
            read_local_structured_outputs(&out_dir, &run_dir.to_string_lossy()).unwrap_err();
        assert_eq!(error.kind, "output_validation_failed");
        assert_eq!(error.field.as_deref(), Some("structuredOutputs"));
        assert!(error.message.contains("parse structured output manifest"));
    }

    #[test]
    fn validates_structured_outputs_against_manifest_fields() {
        let tmp = TempDir::new().unwrap();
        let run_dir = tmp.path().join(".omiga/runs/oprun_structured_schema");
        let spec = OperatorSpec {
            api_version: OPERATOR_API_VERSION_V1ALPHA1.to_string(),
            kind: OPERATOR_KIND.to_string(),
            metadata: OperatorMetadata {
                id: "structured_report".to_string(),
                version: "1".to_string(),
                name: None,
                description: None,
                tags: Vec::new(),
            },
            interface: OperatorInterfaceSpec {
                outputs: BTreeMap::from([
                    (
                        "report".to_string(),
                        OperatorFieldSpec {
                            kind: OperatorFieldKind::File,
                            required: true,
                            glob: Some("report.txt".to_string()),
                            ..OperatorFieldSpec::default()
                        },
                    ),
                    (
                        "summary".to_string(),
                        OperatorFieldSpec {
                            kind: OperatorFieldKind::Json,
                            required: true,
                            ..OperatorFieldSpec::default()
                        },
                    ),
                    (
                        "passed".to_string(),
                        OperatorFieldSpec {
                            kind: OperatorFieldKind::Boolean,
                            required: true,
                            ..OperatorFieldSpec::default()
                        },
                    ),
                    (
                        "score".to_string(),
                        OperatorFieldSpec {
                            kind: OperatorFieldKind::Number,
                            minimum: Some(0.0),
                            maximum: Some(1.0),
                            ..OperatorFieldSpec::default()
                        },
                    ),
                ]),
                ..OperatorInterfaceSpec::default()
            },
            smoke_tests: Vec::new(),
            execution: OperatorExecutionSpec {
                argv: vec!["true".to_string()],
            },
            preflight: None,
            runtime: None,
            cache: None,
            resources: BTreeMap::new(),
            bindings: Vec::new(),
            permissions: None,
            source: OperatorSource {
                source_plugin: "test@local".to_string(),
                plugin_root: tmp.path().to_path_buf(),
                manifest_path: tmp.path().join("operator.yaml"),
            },
        };

        let valid = validate_structured_outputs_against_manifest(
            Some(json!({
                "summary": { "lineCount": 2 },
                "passed": true,
                "score": 0.75,
                "extra": "allowed metadata"
            })),
            &spec,
            &run_dir.to_string_lossy(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(valid["summary"]["lineCount"], json!(2));

        let error = validate_structured_outputs_against_manifest(
            Some(json!({ "summary": { "lineCount": 2 }, "passed": "yes" })),
            &spec,
            &run_dir.to_string_lossy(),
        )
        .unwrap_err();
        assert_eq!(error.kind, "output_validation_failed");
        assert_eq!(error.field.as_deref(), Some("structuredOutputs.passed"));

        let error = validate_structured_outputs_against_manifest(
            Some(json!({ "passed": true })),
            &spec,
            &run_dir.to_string_lossy(),
        )
        .unwrap_err();
        assert_eq!(error.kind, "output_validation_failed");
        assert_eq!(error.field.as_deref(), Some("structuredOutputs.summary"));

        let error =
            validate_structured_outputs_against_manifest(None, &spec, &run_dir.to_string_lossy())
                .unwrap_err();
        assert_eq!(error.kind, "output_validation_failed");
        assert_eq!(error.field.as_deref(), Some("structuredOutputs.passed"));
    }

    #[test]
    fn rejects_output_globs_that_escape_session_outdir() {
        let tmp = TempDir::new().unwrap();
        let run_dir = tmp.path().join(".omiga/runs/oprun_escape");
        let out_dir = run_dir.join("out");
        fs::create_dir_all(&out_dir).unwrap();

        for glob in ["../*.txt", "/tmp/*.txt"] {
            let spec = OperatorSpec {
                api_version: OPERATOR_API_VERSION_V1ALPHA1.to_string(),
                kind: OPERATOR_KIND.to_string(),
                metadata: OperatorMetadata {
                    id: "bounded_outputs".to_string(),
                    version: "1".to_string(),
                    name: None,
                    description: None,
                    tags: Vec::new(),
                },
                interface: OperatorInterfaceSpec {
                    outputs: BTreeMap::from([(
                        "report".to_string(),
                        OperatorFieldSpec {
                            kind: OperatorFieldKind::File,
                            required: true,
                            glob: Some(glob.to_string()),
                            ..OperatorFieldSpec::default()
                        },
                    )]),
                    ..OperatorInterfaceSpec::default()
                },
                smoke_tests: Vec::new(),
                execution: OperatorExecutionSpec {
                    argv: vec!["true".to_string()],
                },
                preflight: None,
                runtime: None,
                cache: None,
                resources: BTreeMap::new(),
                bindings: Vec::new(),
                permissions: None,
                source: OperatorSource {
                    source_plugin: "test@local".to_string(),
                    plugin_root: tmp.path().to_path_buf(),
                    manifest_path: tmp.path().join("operator.yaml"),
                },
            };

            let error =
                collect_local_outputs(&spec, &run_dir.to_string_lossy(), &out_dir).unwrap_err();
            assert_eq!(error.kind, "output_validation_failed");
            assert_eq!(error.field.as_deref(), Some("outputs.report"));
            assert!(error.message.contains("must stay relative"));
        }
    }
}
