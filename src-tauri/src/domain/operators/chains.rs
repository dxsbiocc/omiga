use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

use super::{current_epoch_ms, load_operator_manifest, OperatorSpec, OPERATOR_STATE_DIR_NAME};

/// Directory for user-created script operators: `~/.omiga/user-operators/`
const USER_OPERATORS_SUBDIR: &str = "user-operators";
/// Directory for user-created operator chain templates: `~/.omiga/user-chains/`
const USER_CHAINS_SUBDIR: &str = "user-chains";

/// `~/.omiga/user-operators/` — where user-created script operators live.
pub fn user_operators_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(OPERATOR_STATE_DIR_NAME)
        .join(USER_OPERATORS_SUBDIR)
}

/// `~/.omiga/user-chains/` — where user-created operator chain templates live.
pub fn user_chains_dir() -> PathBuf {
    let dir = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(OPERATOR_STATE_DIR_NAME)
        .join(USER_CHAINS_SUBDIR);
    if let Err(err) = fs::create_dir_all(&dir) {
        tracing::warn!("create user chains dir {:?}: {}", dir, err);
    }
    dir
}

/// Scan `~/.omiga/user-operators/*.yaml` and load each as an OperatorSpec.
pub(super) fn discover_user_operator_candidates() -> Vec<OperatorSpec> {
    let dir = user_operators_dir();
    let Ok(entries) = fs::read_dir(&dir) else {
        return Vec::new();
    };
    entries
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path()
                .extension()
                .is_some_and(|ext| ext == "yaml" || ext == "yml")
        })
        .filter_map(|e| {
            let path = e.path();
            match load_operator_manifest(&path, "user", &dir) {
                Ok(spec) => Some(spec),
                Err(err) => {
                    tracing::warn!("user operator {:?}: {}", path, err);
                    None
                }
            }
        })
        .collect()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserOperatorInput {
    pub name: String,
    pub kind: String,
    pub required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserOperatorParam {
    pub name: String,
    pub kind: String,
    pub default: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserOperatorOutput {
    pub name: String,
    pub glob: String,
}

#[derive(Debug, Clone)]
pub struct ChainStep {
    pub alias: String,
    pub label: Option<String>,
    pub arguments: JsonValue,
    /// Optional input field name that should receive the previous step's outputDir.
    pub inherit_prev_output_as: Option<String>,
    pub depends_on: Vec<String>,
    #[doc(hidden)]
    pub depends_on_declared: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChainStepWire {
    pub alias: String,
    #[serde(default)]
    pub label: Option<String>,
    pub arguments: JsonValue,
    #[serde(default)]
    pub inherit_prev_output_as: Option<String>,
    #[serde(default)]
    pub depends_on: Option<Vec<String>>,
}

impl ChainStep {
    pub fn depends_on_declared(&self) -> bool {
        self.depends_on_declared
    }
}

impl<'de> Deserialize<'de> for ChainStep {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = ChainStepWire::deserialize(deserializer)?;
        let depends_on_declared = wire.depends_on.is_some();
        Ok(Self {
            alias: wire.alias,
            label: wire.label,
            arguments: wire.arguments,
            inherit_prev_output_as: wire.inherit_prev_output_as,
            depends_on: wire.depends_on.unwrap_or_default(),
            depends_on_declared,
        })
    }
}

impl Serialize for ChainStep {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;

        let mut len = 2;
        if self.label.is_some() {
            len += 1;
        }
        if self.inherit_prev_output_as.is_some() {
            len += 1;
        }
        if self.depends_on_declared {
            len += 1;
        }

        let mut step = serializer.serialize_struct("ChainStep", len)?;
        step.serialize_field("alias", &self.alias)?;
        if let Some(label) = &self.label {
            step.serialize_field("label", label)?;
        }
        step.serialize_field("arguments", &self.arguments)?;
        if let Some(inherit_prev_output_as) = &self.inherit_prev_output_as {
            step.serialize_field("inheritPrevOutputAs", inherit_prev_output_as)?;
        }
        if self.depends_on_declared {
            step.serialize_field("dependsOn", &self.depends_on)?;
        }
        step.end()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChainTemplate {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub steps: Vec<ChainStep>,
    pub updated_at_ms: u64,
}

/// Create or replace an operator chain template YAML in `~/.omiga/user-chains/`.
pub fn save_user_chain_template(
    id: &str,
    name: &str,
    description: Option<&str>,
    steps: &[ChainStep],
) -> Result<PathBuf, String> {
    let id = id.trim();
    let name = name.trim();
    if id.is_empty() {
        return Err("chain template id must not be empty".to_string());
    }
    if name.is_empty() {
        return Err("chain template name must not be empty".to_string());
    }
    if steps.is_empty() {
        return Err("chain template must include at least one step".to_string());
    }

    let template = ChainTemplate {
        id: id.to_string(),
        name: name.to_string(),
        description: description
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string),
        steps: steps.to_vec(),
        updated_at_ms: current_epoch_ms(),
    };

    let dir = user_chains_dir();
    fs::create_dir_all(&dir).map_err(|err| format!("create user-chains dir: {err}"))?;
    let path = dir.join(format!("{}.yaml", sanitize_id(id)));
    let raw = serde_yaml::to_string(&template)
        .map_err(|err| format!("serialize chain template: {err}"))?;
    fs::write(&path, raw).map_err(|err| format!("write chain template: {err}"))?;
    tracing::info!("user chain template saved: {:?}", path);
    Ok(path)
}

/// Scan `~/.omiga/user-chains/*.yaml` and load chain templates.
pub fn list_user_chain_templates() -> Vec<ChainTemplate> {
    let dir = user_chains_dir();
    let Ok(entries) = fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut templates = entries
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry
                .path()
                .extension()
                .is_some_and(|ext| ext == "yaml" || ext == "yml")
        })
        .filter_map(|entry| {
            let path = entry.path();
            match fs::read_to_string(&path)
                .map_err(|err| err.to_string())
                .and_then(|raw| {
                    serde_yaml::from_str::<ChainTemplate>(&raw).map_err(|err| err.to_string())
                }) {
                Ok(template) => Some(template),
                Err(err) => {
                    tracing::warn!("user chain template {:?}: {}", path, err);
                    None
                }
            }
        })
        .collect::<Vec<_>>();
    templates.sort_by(|left, right| {
        right
            .updated_at_ms
            .cmp(&left.updated_at_ms)
            .then_with(|| left.name.cmp(&right.name))
            .then_with(|| left.id.cmp(&right.id))
    });
    templates
}

pub fn delete_user_chain_template(id: &str) -> Result<(), String> {
    let id = id.trim();
    if id.is_empty() {
        return Err("chain template id must not be empty".to_string());
    }

    let path = user_chains_dir().join(format!("{}.yaml", sanitize_id(id)));
    match fs::remove_file(&path) {
        Ok(()) => {
            tracing::info!("user chain template deleted: {:?}", path);
            Ok(())
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(format!("delete chain template: {err}")),
    }
}

/// Create or replace a user script operator YAML in `~/.omiga/user-operators/`.
///
/// `argv` is the command array, e.g. `["bash", "-c", "echo hello"]`.
pub fn save_user_script_operator(
    id: &str,
    name: &str,
    description: &str,
    argv: &[String],
    inputs: &[UserOperatorInput],
    params: &[UserOperatorParam],
    outputs: &[UserOperatorOutput],
) -> Result<PathBuf, String> {
    if id.is_empty() {
        return Err("operator id must not be empty".to_string());
    }
    if argv.is_empty() {
        return Err("operator argv must not be empty".to_string());
    }

    let dir = user_operators_dir();
    fs::create_dir_all(&dir).map_err(|e| format!("create user-operators dir: {e}"))?;

    let argv_yaml = argv
        .iter()
        .map(|a| format!("    - {}", serde_yaml_escape(a)))
        .collect::<Vec<_>>()
        .join("\n");

    let desc_line = if description.is_empty() {
        String::new()
    } else {
        format!("\n  description: {}", serde_yaml_escape(description))
    };
    let inputs_yaml = user_operator_inputs_yaml(inputs);
    let params_yaml = user_operator_params_yaml(params);
    let outputs_yaml = user_operator_outputs_yaml(outputs);

    let content = format!(
        "apiVersion: omiga.ai/operator/v1alpha1\nkind: Operator\nmetadata:\n  id: {id}\n  version: 0.1.0\n  name: {name}{desc_line}\nexecution:\n  argv:\n{argv_yaml}\ninterface:\n{inputs_yaml}\n{params_yaml}\n{outputs_yaml}\n",
        id = serde_yaml_escape(id),
        name = serde_yaml_escape(name),
    );

    let file_name = format!("{}.yaml", sanitize_id(id));
    let path = dir.join(&file_name);
    fs::write(&path, content).map_err(|e| format!("write user operator: {e}"))?;
    tracing::info!("user operator saved: {:?}", path);
    Ok(path)
}

fn user_operator_inputs_yaml(inputs: &[UserOperatorInput]) -> String {
    if inputs.is_empty() {
        return "  inputs: {}".to_string();
    }

    let mut lines = vec!["  inputs:".to_string()];
    for input in inputs {
        lines.push(format!("    {}:", serde_yaml_escape(&input.name)));
        lines.push(format!("      kind: {}", serde_yaml_escape(&input.kind)));
        lines.push(format!("      required: {}", input.required));
    }
    lines.join("\n")
}

fn user_operator_params_yaml(params: &[UserOperatorParam]) -> String {
    if params.is_empty() {
        return "  params: {}".to_string();
    }

    let mut lines = vec!["  params:".to_string()];
    for param in params {
        lines.push(format!("    {}:", serde_yaml_escape(&param.name)));
        lines.push(format!("      kind: {}", serde_yaml_escape(&param.kind)));
        let default = param.default.trim();
        if !default.is_empty() {
            lines.push(format!("      default: {}", serde_yaml_escape(default)));
        }
    }
    lines.join("\n")
}

fn user_operator_outputs_yaml(outputs: &[UserOperatorOutput]) -> String {
    if outputs.is_empty() {
        return "  outputs: {}".to_string();
    }

    let mut lines = vec!["  outputs:".to_string()];
    for output in outputs {
        lines.push(format!("    {}:", serde_yaml_escape(&output.name)));
        lines.push(format!("      glob: {}", serde_yaml_escape(&output.glob)));
    }
    lines.join("\n")
}

fn sanitize_id(id: &str) -> String {
    id.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn serde_yaml_escape(s: &str) -> String {
    if s.contains([
        '"', '\'', '\n', ':', '#', '&', '*', '!', '|', '>', '{', '}', '[', ']', ',',
    ]) {
        format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        s.to_string()
    }
}
