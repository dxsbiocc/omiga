//! Omiga TemplateSpec discovery and validation.
//!
//! The first executable milestone keeps TemplateSpec deliberately small:
//! manifests are discoverable through the Unit Index, then `template_execute`
//! either delegates to a migration-target Operator or renders one local script
//! into a transient operator-shaped run workspace.

use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::BTreeMap;
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
    #[serde(default)]
    pub argv: Vec<String>,
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
    pub aliases: Vec<String>,
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
    #[serde(default)]
    pub aliases: Vec<String>,
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

pub async fn execute_template_tool_call(
    ctx: &crate::domain::tools::ToolContext,
    template_id: &str,
    arguments: &str,
) -> (String, bool) {
    let started_at = chrono::Utc::now().to_rfc3339();
    let template = match resolve_template(template_id) {
        Ok(template) => template,
        Err(error) => {
            let output = template_failure_json(template_id, None, &error, None);
            return (output, true);
        }
    };
    let invocation =
        match serde_json::from_str::<crate::domain::operators::OperatorInvocation>(arguments) {
            Ok(invocation) => invocation,
            Err(err) => {
                let message = format!(
                    "Template arguments must be JSON object {{inputs, params, resources}}: {err}"
                );
                record_template_failure_best_effort(
                    ctx,
                    &template,
                    Some(&started_at),
                    None,
                    None,
                    &message,
                    None,
                    None,
                )
                .await;
                return (
                    template_failure_json(template_id, Some(&template), &message, None),
                    true,
                );
            }
        };

    let parent_execution_id =
        record_template_start_best_effort(ctx, &template, &invocation, &started_at).await;

    let result =
        execute_template_runtime(ctx, &template, &invocation, parent_execution_id.as_deref()).await;

    match result {
        Ok((raw, is_error, metadata)) => {
            record_template_execution_best_effort(
                ctx,
                &template,
                &invocation,
                &started_at,
                &raw,
                is_error,
                metadata,
                parent_execution_id.as_deref(),
            )
            .await;
            (raw, is_error)
        }
        Err(message) => {
            record_template_failure_best_effort(
                ctx,
                &template,
                Some(&started_at),
                Some(&invocation),
                None,
                &message,
                None,
                parent_execution_id.as_deref(),
            )
            .await;
            (
                template_failure_json(template_id, Some(&template), &message, None),
                true,
            )
        }
    }
}

async fn execute_template_runtime(
    ctx: &crate::domain::tools::ToolContext,
    template: &TemplateSpecWithSource,
    invocation: &crate::domain::operators::OperatorInvocation,
    parent_execution_id: Option<&str>,
) -> Result<(String, bool, JsonValue), String> {
    let environment = template_environment_resolution(template);
    let primary_result = if uses_existing_operator(template) {
        execute_template_via_migration_target(ctx, template, invocation, parent_execution_id).await
    } else {
        execute_rendered_template(ctx, template, invocation, parent_execution_id).await
    };
    match primary_result {
        Ok((raw, true, metadata))
            if !uses_existing_operator(template)
                && template_fallback_to_migration_target(template) =>
        {
            execute_template_via_migration_target(ctx, template, invocation, parent_execution_id)
                .await
                .map(|(fallback_raw, fallback_is_error, fallback_metadata)| {
                    (
                        fallback_raw,
                        fallback_is_error,
                        attach_template_environment(
                            serde_json::json!({
                                "executionMode": "fallbackMigrationTarget",
                                "primary": metadata,
                                "primaryResult": serde_json::from_str::<JsonValue>(&raw).ok(),
                                "fallback": fallback_metadata,
                            }),
                            &environment,
                        ),
                    )
                })
        }
        Err(message)
            if !uses_existing_operator(template)
                && template_fallback_to_migration_target(template) =>
        {
            execute_template_via_migration_target(ctx, template, invocation, parent_execution_id)
                .await
                .map(|(fallback_raw, fallback_is_error, fallback_metadata)| {
                    (
                        fallback_raw,
                        fallback_is_error,
                        attach_template_environment(
                            serde_json::json!({
                                "executionMode": "fallbackMigrationTarget",
                                "primaryError": message,
                                "fallback": fallback_metadata,
                            }),
                            &environment,
                        ),
                    )
                })
        }
        Ok((raw, is_error, metadata)) => Ok((
            raw,
            is_error,
            attach_template_environment(metadata, &environment),
        )),
        other => other,
    }
}

fn template_environment_resolution(
    template: &TemplateSpecWithSource,
) -> crate::domain::environments::EnvironmentResolution {
    crate::domain::environments::resolve_environment_ref(
        template.spec.runtime.env_ref.as_deref(),
        &template.source.source_plugin,
        &template.source.plugin_root,
    )
}

fn attach_template_environment(
    metadata: JsonValue,
    environment: &crate::domain::environments::EnvironmentResolution,
) -> JsonValue {
    let environment = serde_json::to_value(environment).unwrap_or_else(|_| serde_json::json!({}));
    match metadata {
        JsonValue::Object(mut object) => {
            object.insert("environment".to_string(), environment);
            JsonValue::Object(object)
        }
        other => serde_json::json!({
            "execution": other,
            "environment": environment,
        }),
    }
}

fn resolve_template(raw_id: &str) -> Result<TemplateSpecWithSource, String> {
    let id = raw_id.trim();
    if id.is_empty() {
        return Err("template id must not be empty".to_string());
    }
    let needle = id.to_ascii_lowercase();
    let matches = discover_template_candidates()
        .into_iter()
        .filter(|candidate| template_matches_id(candidate, &needle))
        .filter(|candidate| {
            crate::domain::plugins::template_expose_to_agent(
                &candidate.source.source_plugin,
                &candidate.spec.metadata.id,
                candidate.spec.exposure.expose_to_agent,
            )
        })
        .collect::<Vec<_>>();
    select_template_match(id, matches)
}

fn select_template_match(
    id: &str,
    matches: Vec<TemplateSpecWithSource>,
) -> Result<TemplateSpecWithSource, String> {
    match matches.as_slice() {
        [only] => Ok(only.clone()),
        [] => Err(format!(
            "Template `{id}` was not found. Use unit_search kind=template to inspect templates."
        )),
        many => {
            let best_priority = many
                .iter()
                .map(template_preference_priority)
                .min()
                .unwrap_or(u8::MAX);
            let preferred = many
                .iter()
                .filter(|candidate| template_preference_priority(candidate) == best_priority)
                .collect::<Vec<_>>();
            match preferred.as_slice() {
                [only] => Ok((*only).clone()),
                _ => Err(format!(
                    "Template `{id}` is ambiguous across {} candidates; use canonical id.",
                    many.len()
                )),
            }
        }
    }
}

fn template_matches_id(candidate: &TemplateSpecWithSource, needle: &str) -> bool {
    let canonical = canonical_template_unit_id(candidate);
    normalize_match(&canonical) == needle
        || normalize_match(&candidate.spec.metadata.id) == needle
        || candidate
            .spec
            .aliases
            .iter()
            .any(|alias| normalize_match(alias) == needle)
        || candidate
            .spec
            .migration_target
            .as_deref()
            .map(normalize_match)
            .is_some_and(|alias| alias == needle)
}

fn normalize_match(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

fn template_preference_priority(candidate: &TemplateSpecWithSource) -> u8 {
    let source = candidate.source.source_plugin.to_ascii_lowercase();
    let marketplace = source.rsplit_once('@').map(|(_, marketplace)| marketplace);
    match marketplace {
        Some("omiga-project") => 0,
        Some("omiga-user") => 1,
        Some("omiga-curated") => 2,
        _ => 3,
    }
}

fn uses_existing_operator(template: &TemplateSpecWithSource) -> bool {
    template.spec.migration_target.is_some()
        && template
            .spec
            .execution
            .interpreter
            .as_deref()
            .map(|interpreter| {
                matches!(
                    interpreter.trim().to_ascii_lowercase().as_str(),
                    "existing-operator" | "operator"
                )
            })
            .unwrap_or(false)
}

fn template_fallback_to_migration_target(template: &TemplateSpecWithSource) -> bool {
    template
        .spec
        .execution
        .extra
        .get("fallbackToMigrationTarget")
        .and_then(JsonValue::as_bool)
        .unwrap_or(false)
        && template.spec.migration_target.is_some()
}

pub fn template_preflight_question(
    arguments: &str,
) -> Option<crate::domain::tools::ask_user_question::AskUserQuestionArgs> {
    let value = serde_json::from_str::<JsonValue>(arguments).ok()?;
    let root = value.as_object()?;
    let id = root.get("id").and_then(JsonValue::as_str)?;
    let template = resolve_template(id).ok()?;
    template_preflight_question_for_template(&template, root, None)
}

pub fn template_preflight_question_with_project_preferences(
    project_root: &Path,
    arguments: &str,
) -> Option<crate::domain::tools::ask_user_question::AskUserQuestionArgs> {
    let value = serde_json::from_str::<JsonValue>(arguments).ok()?;
    let root = value.as_object()?;
    let id = root.get("id").and_then(JsonValue::as_str)?;
    let template = resolve_template(id).ok()?;
    let recommended_params = template_project_preference_params(project_root, &template);
    template_preflight_question_for_template(&template, root, recommended_params.as_ref())
}

fn template_preflight_question_for_template(
    template: &TemplateSpecWithSource,
    root: &serde_json::Map<String, JsonValue>,
    recommended_params: Option<&BTreeMap<String, JsonValue>>,
) -> Option<crate::domain::tools::ask_user_question::AskUserQuestionArgs> {
    let (_alias, spec) = describe_template_migration_target(template).ok()?;
    let params = root.get("params").and_then(JsonValue::as_object);
    let mut question_args =
        crate::domain::operators::operator_preflight_question_for_spec_with_recommended_params(
            &spec,
            Some(spec.metadata.id.as_str()),
            params,
            recommended_params,
        )?;
    question_args.metadata = Some(serde_json::json!({
        "source": "template_preflight",
        "template_id": template.spec.metadata.id,
        "template_aliases": template.spec.aliases,
        "migration_target": spec.metadata.id,
    }));
    Some(question_args)
}

fn template_project_preference_params(
    project_root: &Path,
    template: &TemplateSpecWithSource,
) -> Option<BTreeMap<String, JsonValue>> {
    let canonical_id = format!(
        "{}/template/{}",
        template.source.source_plugin,
        template.spec.metadata.id.trim()
    );
    let hints = crate::domain::learning_proposals::matching_learning_project_preference_hints(
        project_root,
        Some(template.spec.metadata.id.as_str()),
        Some(canonical_id.as_str()),
        Some(template.source.source_plugin.as_str()),
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

pub fn apply_template_preflight_answers(
    arguments: &str,
    ask_user_output: &JsonValue,
) -> Result<String, String> {
    let mut value = serde_json::from_str::<JsonValue>(arguments)
        .map_err(|err| format!("Invalid template_execute arguments JSON: {err}"))?;
    let root = value
        .as_object_mut()
        .ok_or_else(|| "template_execute arguments must be a JSON object".to_string())?;
    let id = root
        .get("id")
        .and_then(JsonValue::as_str)
        .map(str::to_string)
        .ok_or_else(|| "template_execute arguments must include string id".to_string())?;
    let template = resolve_template(&id)?;
    apply_template_preflight_answers_for_template(root, &template, ask_user_output)?;
    serde_json::to_string(&value).map_err(|err| err.to_string())
}

fn apply_template_preflight_answers_for_template(
    root: &mut serde_json::Map<String, JsonValue>,
    template: &TemplateSpecWithSource,
    ask_user_output: &JsonValue,
) -> Result<(), String> {
    let (_alias, spec) = match describe_template_migration_target(template) {
        Ok(value) => value,
        Err(_) => return Ok(()),
    };
    let Some(preflight) = spec.preflight.as_ref() else {
        return Ok(());
    };
    let operator_invocation = serde_json::json!({
        "inputs": root
            .get("inputs")
            .cloned()
            .unwrap_or_else(|| JsonValue::Object(serde_json::Map::new())),
        "params": root
            .get("params")
            .cloned()
            .unwrap_or_else(|| JsonValue::Object(serde_json::Map::new())),
        "resources": root
            .get("resources")
            .cloned()
            .unwrap_or_else(|| JsonValue::Object(serde_json::Map::new())),
    });
    let updated_invocation = crate::domain::operators::apply_operator_preflight_answers_for_spec(
        &spec,
        preflight,
        &serde_json::to_string(&operator_invocation).map_err(|err| err.to_string())?,
        ask_user_output,
    )?;
    let updated = serde_json::from_str::<JsonValue>(&updated_invocation)
        .map_err(|err| format!("Invalid updated operator invocation JSON: {err}"))?;
    root.insert(
        "params".to_string(),
        updated
            .get("params")
            .cloned()
            .unwrap_or_else(|| JsonValue::Object(serde_json::Map::new())),
    );
    if let Some(metadata) = updated.get("metadata").cloned() {
        root.insert("metadata".to_string(), metadata);
    }
    Ok(())
}

fn describe_template_migration_target(
    template: &TemplateSpecWithSource,
) -> Result<(Option<String>, crate::domain::operators::OperatorSpec), String> {
    let target = template.spec.migration_target.as_deref().ok_or_else(|| {
        "template migrationTarget is required for existing-operator execution".to_string()
    })?;
    let local_specs =
        load_local_template_migration_target_specs(template, target).map_err(|err| {
            format!(
                "load local migrationTarget `{target}` for template `{}`: {err}",
                template.spec.metadata.id
            )
        })?;
    match local_specs.as_slice() {
        [only] => return Ok((Some(target.to_string()), only.clone())),
        [] => {}
        many => {
            let count = many.len();
            return Err(format!(
                "template migrationTarget `{target}` has {count} local operator candidates in plugin `{}`",
                template.source.source_plugin
            ));
        }
    }
    crate::domain::operators::describe_operator(target).map_err(|error| error.message)
}

fn load_local_template_migration_target_specs(
    template: &TemplateSpecWithSource,
    target: &str,
) -> Result<Vec<crate::domain::operators::OperatorSpec>, String> {
    let mut manifests = Vec::new();
    for dir in [
        template
            .source
            .plugin_root
            .join("template_backing_operators"),
        template.source.plugin_root.join("operators"),
    ] {
        collect_operator_manifests(&dir, &mut manifests)?;
    }
    let mut matches = Vec::new();
    for manifest in manifests {
        let spec = crate::domain::operators::load_operator_manifest(
            &manifest,
            template.source.source_plugin.clone(),
            template.source.plugin_root.clone(),
        )?;
        if spec.metadata.id == target {
            matches.push(spec);
        }
    }
    Ok(matches)
}

fn collect_operator_manifests(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(dir).map_err(|err| format!("read `{}`: {err}", dir.display()))? {
        let entry = entry.map_err(|err| format!("read `{}` entry: {err}", dir.display()))?;
        let path = entry.path();
        if path.is_dir() {
            collect_operator_manifests(&path, out)?;
        } else if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| matches!(name, "operator.yaml" | "operator.yml"))
        {
            out.push(path);
        }
    }
    Ok(())
}

async fn execute_template_via_migration_target(
    ctx: &crate::domain::tools::ToolContext,
    template: &TemplateSpecWithSource,
    invocation: &crate::domain::operators::OperatorInvocation,
    parent_execution_id: Option<&str>,
) -> Result<(String, bool, JsonValue), String> {
    let target = template.spec.migration_target.as_deref().ok_or_else(|| {
        "template migrationTarget is required for existing-operator execution".to_string()
    })?;
    let (alias, spec) = describe_template_migration_target(template)?;
    let alias = alias.unwrap_or_else(|| target.to_string());
    let arguments = serde_json::to_string(invocation)
        .map_err(|err| format!("serialize delegated operator invocation: {err}"))?;
    let run_context = crate::domain::operators::OperatorRunContext {
        kind: Some("template".to_string()),
        smoke_test_id: Some(template.spec.metadata.id.clone()),
        smoke_test_name: template.spec.metadata.name.clone(),
        parent_execution_id: parent_execution_id.map(str::to_string),
    };
    let (raw, is_error) =
        crate::domain::operators::execute_resolved_operator_tool_call_with_context(
            ctx,
            &alias,
            crate::domain::operators::ResolvedOperator {
                alias: alias.clone(),
                spec,
            },
            &arguments,
            Some(run_context),
        )
        .await;
    Ok((
        raw,
        is_error,
        serde_json::json!({
            "executionMode": "migrationTarget",
            "migrationTarget": target,
        }),
    ))
}

async fn execute_rendered_template(
    ctx: &crate::domain::tools::ToolContext,
    template: &TemplateSpecWithSource,
    invocation: &crate::domain::operators::OperatorInvocation,
    parent_execution_id: Option<&str>,
) -> Result<(String, bool, JsonValue), String> {
    let rendered = render_template_script(ctx, template, invocation)?;
    let interface = parse_template_operator_interface(template)?;
    let runtime = serde_json::to_value(&template.spec.runtime).ok();
    let backing = describe_template_migration_target(template)
        .ok()
        .map(|(_, spec)| spec);
    let mut argv = Vec::new();
    if let Some(interpreter) = template
        .spec
        .execution
        .interpreter
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        argv.push(interpreter.to_string());
    }
    argv.push(rendered.rendered_script.to_string_lossy().into_owned());
    argv.extend(template.spec.execution.argv.clone());
    let spec = crate::domain::operators::OperatorSpec {
        api_version: crate::domain::operators::OPERATOR_API_VERSION_V1ALPHA1.to_string(),
        kind: crate::domain::operators::OPERATOR_KIND.to_string(),
        metadata: crate::domain::operators::OperatorMetadata {
            id: template.spec.metadata.id.clone(),
            version: template.spec.metadata.version.clone(),
            name: template.spec.metadata.name.clone(),
            description: template.spec.metadata.description.clone(),
            tags: template.spec.metadata.tags.clone(),
        },
        interface,
        smoke_tests: Vec::new(),
        execution: crate::domain::operators::OperatorExecutionSpec { argv },
        preflight: backing.as_ref().and_then(|spec| spec.preflight.clone()),
        runtime,
        cache: None,
        resources: backing
            .as_ref()
            .map(|spec| spec.resources.clone())
            .unwrap_or_default(),
        bindings: backing
            .as_ref()
            .map(|spec| spec.bindings.clone())
            .unwrap_or_default(),
        permissions: backing.and_then(|spec| spec.permissions),
        source: crate::domain::operators::OperatorSource {
            source_plugin: template.source.source_plugin.clone(),
            plugin_root: rendered.render_root.clone(),
            manifest_path: template.source.manifest_path.clone(),
        },
    };
    let arguments = serde_json::to_string(invocation)
        .map_err(|err| format!("serialize template invocation: {err}"))?;
    let run_context = crate::domain::operators::OperatorRunContext {
        kind: Some("template".to_string()),
        smoke_test_id: Some(template.spec.metadata.id.clone()),
        smoke_test_name: template.spec.metadata.name.clone(),
        parent_execution_id: parent_execution_id.map(str::to_string),
    };
    let (raw, is_error) =
        crate::domain::operators::execute_resolved_operator_tool_call_with_context(
            ctx,
            &template.spec.metadata.id,
            crate::domain::operators::ResolvedOperator {
                alias: template.spec.metadata.id.clone(),
                spec,
            },
            &arguments,
            Some(run_context),
        )
        .await;
    Ok((
        raw,
        is_error,
        serde_json::json!({
            "executionMode": "renderedTemplate",
            "renderRoot": rendered.render_root,
            "renderedScript": rendered.rendered_script,
            "templateEngine": template.spec.template.engine,
        }),
    ))
}

struct RenderedTemplateScript {
    render_root: PathBuf,
    rendered_script: PathBuf,
}

fn render_template_script(
    ctx: &crate::domain::tools::ToolContext,
    template: &TemplateSpecWithSource,
    invocation: &crate::domain::operators::OperatorInvocation,
) -> Result<RenderedTemplateScript, String> {
    let source = resolve_template_entry_path(template)?;
    let raw = fs::read_to_string(&source)
        .map_err(|err| format!("read template entry `{}`: {err}", source.display()))?;
    let rendered = render_template_text(&raw, template, invocation)?;
    let render_root = ctx
        .project_root
        .join(".omiga")
        .join("template-renders")
        .join(format!(
            "{}_{}",
            safe_component(&template.spec.metadata.id),
            uuid::Uuid::new_v4().simple()
        ));
    fs::create_dir_all(&render_root).map_err(|err| {
        format!(
            "create template render dir `{}`: {err}",
            render_root.display()
        )
    })?;
    let extension = source
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| format!(".{value}"))
        .unwrap_or_else(|| ".sh".to_string());
    let rendered_script = render_root.join(format!("rendered_template{extension}"));
    fs::write(&rendered_script, rendered).map_err(|err| {
        format!(
            "write rendered template script `{}`: {err}",
            rendered_script.display()
        )
    })?;
    Ok(RenderedTemplateScript {
        render_root,
        rendered_script,
    })
}

fn resolve_template_entry_path(template: &TemplateSpecWithSource) -> Result<PathBuf, String> {
    let raw = template.spec.template.entry.to_string_lossy();
    let rel = raw
        .strip_prefix("./")
        .ok_or_else(|| "template.entry must start with `./`".to_string())?;
    let parent = template
        .source
        .manifest_path
        .parent()
        .ok_or_else(|| "template manifest has no parent directory".to_string())?;
    Ok(parent.join(rel))
}

fn render_template_text(
    raw: &str,
    template: &TemplateSpecWithSource,
    invocation: &crate::domain::operators::OperatorInvocation,
) -> Result<String, String> {
    let engine = template.spec.template.engine.trim().to_ascii_lowercase();
    if matches!(engine.as_str(), "static" | "raw") {
        return Ok(raw.to_string());
    }
    if !matches!(engine.as_str(), "jinja2" | "jinja" | "minijinja") {
        return Err(format!(
            "unsupported template engine `{}`; supported MVP engines are static and jinja2-compatible simple replacement",
            template.spec.template.engine
        ));
    }
    let mut rendered = raw.to_string();
    for (prefix, values) in [
        ("inputs", &invocation.inputs),
        ("params", &invocation.params),
        ("resources", &invocation.resources),
    ] {
        for (key, value) in values {
            let rendered_value = template_value_to_string(value);
            for pattern in [
                format!("{{{{ {prefix}.{key} }}}}"),
                format!("{{{{{prefix}.{key}}}}}"),
                format!("${{{prefix}.{key}}}"),
            ] {
                rendered = rendered.replace(&pattern, &rendered_value);
            }
        }
    }
    let manifest_dir = template
        .source
        .manifest_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| template.source.plugin_root.clone());
    for (key, value) in [
        (
            "pluginRoot",
            template.source.plugin_root.to_string_lossy().into_owned(),
        ),
        ("manifestDir", manifest_dir.to_string_lossy().into_owned()),
        (
            "manifestPath",
            template.source.manifest_path.to_string_lossy().into_owned(),
        ),
        ("sourcePlugin", template.source.source_plugin.clone()),
    ] {
        for pattern in [
            format!("{{{{ template.{key} }}}}"),
            format!("{{{{template.{key}}}}}"),
            format!("${{template.{key}}}"),
        ] {
            rendered = rendered.replace(&pattern, &value);
        }
    }
    Ok(rendered)
}

fn template_value_to_string(value: &JsonValue) -> String {
    match value {
        JsonValue::Null => String::new(),
        JsonValue::String(value) => value.clone(),
        JsonValue::Bool(value) => value.to_string(),
        JsonValue::Number(value) => value.to_string(),
        _ => serde_json::to_string(value).unwrap_or_default(),
    }
}

fn parse_template_operator_interface(
    template: &TemplateSpecWithSource,
) -> Result<crate::domain::operators::OperatorInterfaceSpec, String> {
    let parsed = if template.spec.interface.is_null() {
        crate::domain::operators::OperatorInterfaceSpec::default()
    } else {
        serde_json::from_value(template.spec.interface.clone())
            .map_err(|err| format!("template interface is not operator-compatible: {err}"))?
    };
    if parsed.inputs.is_empty() && parsed.params.is_empty() && parsed.outputs.is_empty() {
        if let Ok((_alias, backing)) = describe_template_migration_target(template) {
            return Ok(backing.interface);
        }
    }
    Ok(parsed)
}

async fn record_template_start_best_effort(
    ctx: &crate::domain::tools::ToolContext,
    template: &TemplateSpecWithSource,
    invocation: &crate::domain::operators::OperatorInvocation,
    started_at: &str,
) -> Option<String> {
    let record = template_execution_record(
        ctx,
        template,
        invocation,
        started_at,
        None,
        "running",
        serde_json::json!({ "status": "running" }),
        serde_json::json!({
            "templateId": template.spec.metadata.id,
            "sourcePlugin": template.source.source_plugin,
            "manifestPath": template.source.manifest_path,
            "migrationTarget": template.spec.migration_target,
            "environment": template_environment_resolution(template),
            "execution": { "phase": "started" },
        }),
    );
    let id = format!("execrec_{}", uuid::Uuid::new_v4().simple());
    match crate::domain::execution_records::record_execution_with_id(
        &ctx.project_root,
        id.clone(),
        record,
    )
    .await
    {
        Ok(id) => Some(id),
        Err(err) => {
            tracing::warn!("template execution record start failed: {err}");
            None
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn record_template_execution_best_effort(
    ctx: &crate::domain::tools::ToolContext,
    template: &TemplateSpecWithSource,
    invocation: &crate::domain::operators::OperatorInvocation,
    started_at: &str,
    raw: &str,
    is_error: bool,
    metadata: JsonValue,
    execution_id: Option<&str>,
) {
    let parsed = serde_json::from_str::<JsonValue>(raw).ok();
    let status = parsed
        .as_ref()
        .and_then(|value| value.get("status"))
        .and_then(JsonValue::as_str)
        .unwrap_or(if is_error { "failed" } else { "succeeded" });
    let output_summary = template_output_summary(parsed.as_ref(), is_error);
    let record = template_execution_record(
        ctx,
        template,
        invocation,
        started_at,
        Some(chrono::Utc::now().to_rfc3339()),
        status,
        output_summary,
        serde_json::json!({
            "templateId": template.spec.metadata.id,
            "sourcePlugin": template.source.source_plugin,
            "manifestPath": template.source.manifest_path,
            "migrationTarget": template.spec.migration_target,
            "execution": metadata,
            "operatorResult": parsed,
        }),
    );
    if let Some(id) = execution_id {
        crate::domain::execution_records::update_execution_record_best_effort(
            &ctx.project_root,
            id,
            record,
        )
        .await;
    } else {
        crate::domain::execution_records::record_execution_best_effort(&ctx.project_root, record)
            .await;
    }
}

#[allow(clippy::too_many_arguments)]
async fn record_template_failure_best_effort(
    ctx: &crate::domain::tools::ToolContext,
    template: &TemplateSpecWithSource,
    started_at: Option<&str>,
    invocation: Option<&crate::domain::operators::OperatorInvocation>,
    metadata: Option<JsonValue>,
    message: &str,
    status: Option<&str>,
    execution_id: Option<&str>,
) {
    let (input_hash, param_hash) = invocation
        .map(|invocation| {
            (
                crate::domain::execution_records::hash_execution_map(&invocation.inputs),
                crate::domain::execution_records::hash_execution_map(&invocation.params),
            )
        })
        .unwrap_or((None, None));
    let record = crate::domain::execution_records::ExecutionRecordInput {
        kind: "template".to_string(),
        unit_id: Some(template.spec.metadata.id.clone()),
        canonical_id: Some(canonical_template_unit_id(template)),
        provider_plugin: Some(template.source.source_plugin.clone()),
        status: status.unwrap_or("failed").to_string(),
        session_id: ctx.session_id.clone(),
        parent_execution_id: None,
        started_at: started_at.map(str::to_string),
        ended_at: Some(chrono::Utc::now().to_rfc3339()),
        input_hash,
        param_hash,
        output_summary_json: Some(serde_json::json!({ "error": message })),
        runtime_json: serde_json::to_value(&template.spec.runtime).ok(),
        metadata_json: Some(serde_json::json!({
            "templateId": template.spec.metadata.id,
            "sourcePlugin": template.source.source_plugin,
            "manifestPath": template.source.manifest_path,
            "migrationTarget": template.spec.migration_target,
            "execution": metadata,
            "error": message,
        })),
    };
    if let Some(id) = execution_id {
        crate::domain::execution_records::update_execution_record_best_effort(
            &ctx.project_root,
            id,
            record,
        )
        .await;
    } else {
        crate::domain::execution_records::record_execution_best_effort(&ctx.project_root, record)
            .await;
    }
}

#[allow(clippy::too_many_arguments)]
fn template_execution_record(
    ctx: &crate::domain::tools::ToolContext,
    template: &TemplateSpecWithSource,
    invocation: &crate::domain::operators::OperatorInvocation,
    started_at: &str,
    ended_at: Option<String>,
    status: &str,
    output_summary_json: JsonValue,
    metadata_json: JsonValue,
) -> crate::domain::execution_records::ExecutionRecordInput {
    let metadata_json = attach_invocation_provenance_metadata(metadata_json, invocation);
    crate::domain::execution_records::ExecutionRecordInput {
        kind: "template".to_string(),
        unit_id: Some(template.spec.metadata.id.clone()),
        canonical_id: Some(canonical_template_unit_id(template)),
        provider_plugin: Some(template.source.source_plugin.clone()),
        status: status.to_string(),
        session_id: ctx.session_id.clone(),
        parent_execution_id: None,
        started_at: Some(started_at.to_string()),
        ended_at,
        input_hash: crate::domain::execution_records::hash_execution_map(&invocation.inputs),
        param_hash: crate::domain::execution_records::hash_execution_map(&invocation.params),
        output_summary_json: Some(output_summary_json),
        runtime_json: serde_json::to_value(&template.spec.runtime).ok(),
        metadata_json: Some(metadata_json),
    }
}

fn attach_invocation_provenance_metadata(
    metadata_json: JsonValue,
    invocation: &crate::domain::operators::OperatorInvocation,
) -> JsonValue {
    let Some(preflight) =
        crate::domain::operators::operator_invocation_preflight_metadata(invocation)
    else {
        return metadata_json;
    };
    let param_sources =
        crate::domain::operators::operator_invocation_preflight_param_sources(invocation);
    let selected_params = invocation
        .params
        .iter()
        .filter_map(|(param, value)| {
            param_sources
                .get(param)
                .filter(|source| source.as_str() == "user_preflight")
                .map(|_| (param.clone(), value.clone()))
        })
        .collect::<serde_json::Map<String, JsonValue>>();
    match metadata_json {
        JsonValue::Object(mut object) => {
            object.insert("preflight".to_string(), preflight);
            if !param_sources.is_empty() {
                object.insert("paramSources".to_string(), serde_json::json!(param_sources));
            }
            if !selected_params.is_empty() {
                object.insert(
                    "selectedParams".to_string(),
                    JsonValue::Object(selected_params),
                );
            }
            JsonValue::Object(object)
        }
        other => serde_json::json!({
            "execution": other,
            "preflight": preflight,
            "paramSources": param_sources,
            "selectedParams": selected_params,
        }),
    }
}

fn template_output_summary(parsed: Option<&JsonValue>, is_error: bool) -> JsonValue {
    let output_keys = parsed
        .and_then(|value| value.get("outputs"))
        .and_then(JsonValue::as_object)
        .map(|object| object.keys().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    serde_json::json!({
        "status": parsed.and_then(|value| value.get("status")).and_then(JsonValue::as_str).unwrap_or(if is_error { "failed" } else { "succeeded" }),
        "runId": parsed.and_then(|value| value.get("runId")).and_then(JsonValue::as_str),
        "runDir": parsed.and_then(|value| value.get("runDir")).and_then(JsonValue::as_str),
        "outputKeys": output_keys,
    })
}

pub(crate) fn canonical_template_unit_id(template: &TemplateSpecWithSource) -> String {
    format!(
        "{}/template/{}",
        template.source.source_plugin, template.spec.metadata.id
    )
}

pub(crate) fn template_execute_example(
    template: &TemplateSpecWithSource,
    canonical_id: &str,
) -> JsonValue {
    serde_json::json!({
        "tool": "template_execute",
        "arguments": {
            "id": canonical_id,
            "inputs": template_example_inputs(template),
            "params": interface_defaults(&template.spec.interface, "params"),
            "resources": interface_defaults(&template.spec.interface, "resources"),
        }
    })
}

fn template_example_inputs(
    template: &TemplateSpecWithSource,
) -> serde_json::Map<String, JsonValue> {
    let mut values = interface_defaults(&template.spec.interface, "inputs");
    let Some(inputs) = interface_section(&template.spec.interface, "inputs") else {
        return values;
    };
    let file_inputs = inputs
        .iter()
        .filter(|(_, field)| {
            field
                .get("kind")
                .and_then(|value| value.as_str())
                .map(|kind| kind.eq_ignore_ascii_case("file"))
                .unwrap_or(false)
        })
        .map(|(name, _)| name)
        .collect::<Vec<_>>();
    if file_inputs.len() == 1 {
        if let Some(example) = template
            .source
            .manifest_path
            .parent()
            .map(|dir| dir.join("example.tsv"))
            .filter(|path| path.is_file())
        {
            values.insert(
                file_inputs[0].clone(),
                JsonValue::String(example.to_string_lossy().into_owned()),
            );
        }
    }
    values
}

fn interface_defaults(interface: &JsonValue, section: &str) -> serde_json::Map<String, JsonValue> {
    let mut values = serde_json::Map::new();
    let Some(fields) = interface_section(interface, section) else {
        return values;
    };
    for (name, field) in fields {
        if let Some(default) = field.get("default") {
            values.insert(name.clone(), default.clone());
        }
    }
    values
}

fn interface_section<'a>(
    interface: &'a JsonValue,
    section: &str,
) -> Option<&'a serde_json::Map<String, JsonValue>> {
    interface.get(section)?.as_object()
}

fn template_failure_json(
    id: &str,
    template: Option<&TemplateSpecWithSource>,
    message: &str,
    metadata: Option<JsonValue>,
) -> String {
    serde_json::to_string_pretty(&serde_json::json!({
        "status": "failed",
        "templateId": id,
        "canonicalId": template.map(canonical_template_unit_id),
        "sourcePlugin": template.map(|template| template.source.source_plugin.clone()),
        "error": {
            "kind": "template_execution_failed",
            "message": message,
        },
        "metadata": metadata,
    }))
    .unwrap_or_else(|_| "{\"status\":\"failed\"}".to_string())
}

fn safe_component(value: &str) -> String {
    let out = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    if out.trim().is_empty() {
        "template".to_string()
    } else {
        out
    }
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
    let exposure = TemplateExposure {
        expose_to_agent: crate::domain::plugins::template_expose_to_agent(
            &candidate.source.source_plugin,
            &candidate.spec.metadata.id,
            candidate.spec.exposure.expose_to_agent,
        ),
    };
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
        exposure,
        runtime: candidate.spec.runtime,
        template: candidate.spec.template,
        aliases: candidate.spec.aliases,
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
    use serde_json::json;
    use std::collections::{BTreeMap, HashMap, HashSet};

    fn repo_plugin_root(plugin_name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root")
            .join(".omiga/plugins")
            .join(plugin_name)
    }

    fn legacy_plugin_root(plugin_name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures/plugins/legacy")
            .join(plugin_name)
    }

    fn bundled_loaded_plugin(plugin_name: &str, display_name: &str) -> LoadedPlugin {
        let plugin_root = legacy_plugin_root(plugin_name);
        LoadedPlugin {
            id: format!("{plugin_name}@omiga-curated"),
            manifest_name: Some(plugin_name.to_string()),
            display_name: Some(display_name.to_string()),
            description: None,
            root: plugin_root,
            enabled: true,
            skill_roots: Vec::new(),
            mcp_servers: HashMap::new(),
            apps: Vec::new(),
            retrieval: None,
            error: None,
        }
    }

    fn project_loaded_plugin(plugin_name: &str, display_name: &str) -> LoadedPlugin {
        LoadedPlugin {
            id: format!("{plugin_name}@omiga-curated"),
            manifest_name: Some(plugin_name.to_string()),
            display_name: Some(display_name.to_string()),
            description: None,
            root: repo_plugin_root(plugin_name),
            enabled: true,
            skill_roots: Vec::new(),
            mcp_servers: HashMap::new(),
            apps: Vec::new(),
            retrieval: None,
            error: None,
        }
    }

    fn bundled_template_and_operator(
        plugin_name: &str,
        template_dir: &str,
        operator_dir: &str,
    ) -> (
        TemplateSpecWithSource,
        crate::domain::operators::OperatorSpec,
    ) {
        let plugin_root = legacy_plugin_root(plugin_name);
        let template = load_template_manifest(
            &plugin_root
                .join("templates")
                .join(template_dir)
                .join("template.yaml"),
            format!("{plugin_name}@omiga-curated"),
            plugin_root.clone(),
        )
        .expect("template");
        let operator = crate::domain::operators::load_operator_manifest(
            &plugin_root
                .join("operators")
                .join(operator_dir)
                .join("operator.yaml"),
            format!("{plugin_name}@omiga-curated"),
            plugin_root,
        )
        .expect("operator");
        (template, operator)
    }

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

    fn temp_loaded_plugin(id: &str, root: PathBuf) -> LoadedPlugin {
        LoadedPlugin {
            id: id.to_string(),
            manifest_name: Some(id.split('@').next().unwrap_or(id).to_string()),
            display_name: None,
            description: None,
            root,
            enabled: true,
            skill_roots: Vec::new(),
            mcp_servers: HashMap::new(),
            apps: Vec::new(),
            retrieval: None,
            error: None,
        }
    }

    fn write_temp_template_plugin(root: &Path, template_id: &str) {
        let template_dir = root.join("templates").join(template_id);
        fs::create_dir_all(&template_dir).expect("mkdir template plugin");
        fs::write(
            root.join("plugin.json"),
            r#"{"name":"visualization-r-preference","version":"0.1.0","templates":"./templates"}"#,
        )
        .expect("plugin manifest");
        write_valid_template(&template_dir.join("template.yaml"), template_id);
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
        let plugin = bundled_loaded_plugin(
            "operator-differential-expression-r",
            "Differential Expression",
        );

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

    #[test]
    fn discovers_bundled_analysis_template_migration_candidates() {
        let plugins = [
            bundled_loaded_plugin(
                "operator-differential-expression-r",
                "Differential Expression",
            ),
            bundled_loaded_plugin("operator-pca-r", "PCA"),
            bundled_loaded_plugin("operator-enrichment-r", "Functional Enrichment"),
        ];
        let candidates = discover_template_candidates_from_plugins(plugins.iter());
        let by_id = candidates
            .iter()
            .map(|candidate| (candidate.spec.metadata.id.as_str(), candidate))
            .collect::<HashMap<_, _>>();

        let expected = [
            (
                "bulk_differential_expression_basic",
                "omics_differential_expression_basic",
                "omics/transcriptomics/differential",
            ),
            (
                "pca_matrix_basic",
                "omics_pca_matrix",
                "omics/transcriptomics/dimensionality-reduction",
            ),
            (
                "functional_enrichment_basic",
                "omics_functional_enrichment_basic",
                "omics/knowledge/functional-enrichment",
            ),
        ];

        for (template_id, migration_target, category) in expected {
            let template = by_id
                .get(template_id)
                .unwrap_or_else(|| panic!("missing bundled template `{template_id}`"));
            assert_eq!(
                template.spec.migration_target.as_deref(),
                Some(migration_target)
            );
            assert_eq!(
                template.spec.classification.category.as_deref(),
                Some(category)
            );
            assert!(template.spec.exposure.expose_to_agent);
        }
    }

    #[test]
    fn discovers_aggregated_transcriptomics_analysis_templates() {
        let plugin = project_loaded_plugin("transcriptomics", "Transcriptomics");
        let candidates = discover_template_candidates_from_plugins([&plugin]);
        let by_id = candidates
            .iter()
            .map(|candidate| (candidate.spec.metadata.id.as_str(), candidate))
            .collect::<HashMap<_, _>>();

        let expected = [
            (
                "bulk_differential_expression_basic",
                "omics_differential_expression_basic",
                "omics/transcriptomics/differential",
            ),
            (
                "pca_matrix_basic",
                "omics_pca_matrix",
                "omics/transcriptomics/dimensionality-reduction",
            ),
            (
                "functional_enrichment_basic",
                "omics_functional_enrichment_basic",
                "omics/knowledge/functional-enrichment",
            ),
        ];

        assert_eq!(by_id.len(), expected.len());
        for (template_id, migration_target, category) in expected {
            let template = by_id
                .get(template_id)
                .unwrap_or_else(|| panic!("missing transcriptomics template `{template_id}`"));
            assert_eq!(
                template.source.source_plugin,
                "transcriptomics@omiga-curated"
            );
            assert_eq!(
                template.spec.migration_target.as_deref(),
                Some(migration_target)
            );
            assert_eq!(
                template.spec.classification.category.as_deref(),
                Some(category)
            );
            assert!(template.spec.exposure.expose_to_agent);
        }
    }

    #[test]
    fn discovers_omiga_plugin_visualization_r_templates() {
        let plugin = project_loaded_plugin("visualization-r", "R Visualization");
        let candidates = discover_template_candidates_from_plugins([&plugin]);
        let ids = candidates
            .iter()
            .map(|candidate| candidate.spec.metadata.id.as_str())
            .collect::<HashSet<_>>();

        let required = [
            "viz_scatter_basic",
            "viz_distribution_boxplot",
            "viz_bar_basic",
            "viz_heatmap_basic",
            "viz_line_time_series",
        ];

        assert!(candidates.len() >= required.len());
        assert_eq!(candidates.len(), ids.len(), "template ids should be unique");
        for id in required {
            assert!(ids.contains(id), "missing visualization template `{id}`");
        }
        assert!(candidates.iter().all(|candidate| {
            candidate.spec.runtime.env_ref.as_deref() == Some("r-base")
                && candidate
                    .spec
                    .template
                    .entry
                    .to_string_lossy()
                    .ends_with("template.R.j2")
                && candidate.spec.exposure.expose_to_agent
        }));
    }

    #[test]
    fn short_template_ids_prefer_project_then_user_then_bundled_templates() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let project_root = tmp.path().join("project");
        let user_root = tmp.path().join("user");
        let bundled_root = tmp.path().join("bundled");
        for root in [&project_root, &user_root, &bundled_root] {
            write_temp_template_plugin(root, "viz_scatter_basic");
        }

        let project = temp_loaded_plugin(
            "project-visualization-r@omiga-project",
            project_root.clone(),
        );
        let user = temp_loaded_plugin("user-visualization-r@omiga-user", user_root.clone());
        let bundled = temp_loaded_plugin("visualization-r@omiga-curated", bundled_root.clone());

        let all = discover_template_candidates_from_plugins([&bundled, &user, &project]);
        let selected =
            select_template_match("viz_scatter_basic", all).expect("project preference template");
        assert_eq!(
            selected.source.source_plugin,
            "project-visualization-r@omiga-project"
        );

        let without_project = discover_template_candidates_from_plugins([&bundled, &user]);
        let selected =
            select_template_match("viz_scatter_basic", without_project).expect("user preference");
        assert_eq!(
            selected.source.source_plugin,
            "user-visualization-r@omiga-user"
        );

        let bundled_only = discover_template_candidates_from_plugins([&bundled]);
        let selected =
            select_template_match("viz_scatter_basic", bundled_only).expect("bundled fallback");
        assert_eq!(
            selected.source.source_plugin,
            "visualization-r@omiga-curated"
        );
    }

    #[test]
    fn duplicate_template_ids_at_same_preference_level_stay_ambiguous() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let first_root = tmp.path().join("first");
        let second_root = tmp.path().join("second");
        write_temp_template_plugin(&first_root, "viz_scatter_basic");
        write_temp_template_plugin(&second_root, "viz_scatter_basic");
        let first = temp_loaded_plugin("first-visualization-r@lab", first_root);
        let second = temp_loaded_plugin("second-visualization-r@lab", second_root);

        let candidates = discover_template_candidates_from_plugins([&first, &second]);
        let error = select_template_match("viz_scatter_basic", candidates)
            .expect_err("same-level duplicates should remain ambiguous");
        assert!(error.contains("ambiguous"), "{error}");
    }

    #[tokio::test]
    async fn execute_omiga_plugin_visualization_r_template_smoke() {
        let r_ready = std::process::Command::new("Rscript")
            .args([
                "-e",
                "if (!requireNamespace('ggplot2', quietly = TRUE)) quit(status = 1)",
            ])
            .status()
            .map(|status| status.success())
            .unwrap_or(false);
        if !r_ready {
            eprintln!("skipping visualization-r execution smoke: Rscript/ggplot2 unavailable");
            return;
        }

        let plugin_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root")
            .join(".omiga/plugins/visualization-r");
        let template_dir = plugin_root.join("templates/scatter/basic");
        let template = load_template_manifest(
            &template_dir.join("template.yaml"),
            "visualization-r@omiga-curated",
            plugin_root,
        )
        .expect("visualization template");
        let tmp = tempfile::tempdir().expect("tempdir");
        let input = tmp.path().join("scatter-basic-example.tsv");
        fs::copy(template_dir.join("example.tsv"), &input).expect("copy example");
        let invocation = crate::domain::operators::OperatorInvocation {
            inputs: BTreeMap::from([("table".to_string(), json!(input.to_string_lossy()))]),
            params: BTreeMap::new(),
            resources: BTreeMap::new(),
            metadata: BTreeMap::new(),
        };
        let ctx = crate::domain::tools::ToolContext::new(tmp.path());

        let (raw, is_error, metadata) =
            execute_template_runtime(&ctx, &template, &invocation, None)
                .await
                .expect("visualization-r execution");

        assert!(!is_error, "{raw}");
        assert_eq!(metadata["executionMode"], "renderedTemplate");
        assert_eq!(metadata["environment"]["status"], "resolved");
        let parsed: JsonValue = serde_json::from_str(&raw).expect("template result json");
        assert_eq!(parsed["status"], "succeeded");
        for output in ["figure_png", "figure_pdf"] {
            let path = parsed["outputs"][output][0]["path"]
                .as_str()
                .unwrap_or_else(|| panic!("{output} output path"));
            assert!(
                fs::metadata(path)
                    .map(|metadata| metadata.len())
                    .unwrap_or(0)
                    > 0,
                "{output} should be non-empty at {path}"
            );
        }
        let export_dir = parsed["exportDir"].as_str().expect("exportDir");
        for file in ["figure.png", "figure.pdf", "plot-script.R", "README.md"] {
            assert!(
                fs::metadata(Path::new(export_dir).join(file))
                    .map(|metadata| metadata.len())
                    .unwrap_or(0)
                    > 0,
                "exported {file} should be non-empty under {export_dir}"
            );
        }
        let markdown_report = parsed["markdownReport"].as_str().expect("markdownReport");
        assert!(
            markdown_report.contains("![Basic Scatter Plot]"),
            "{markdown_report}"
        );
        assert!(
            markdown_report.contains("plot-script.R"),
            "{markdown_report}"
        );
        assert!(markdown_report.contains("figure.pdf"), "{markdown_report}");
    }

    #[test]
    fn rendered_analysis_templates_preserve_backing_operator_contracts() {
        let cases = [
            (
                "operator-differential-expression-r",
                "differential-expression-basic",
                "differential-expression-basic",
            ),
            ("operator-pca-r", "pca-matrix", "pca-matrix"),
            (
                "operator-enrichment-r",
                "functional-enrichment-basic",
                "functional-enrichment-basic",
            ),
        ];

        for (plugin_name, template_dir, operator_dir) in cases {
            let (template, operator) =
                bundled_template_and_operator(plugin_name, template_dir, operator_dir);
            assert!(
                !uses_existing_operator(&template),
                "{plugin_name} should default to rendered execution in v2"
            );
            assert!(template_fallback_to_migration_target(&template));
            assert_eq!(
                template.spec.migration_target.as_deref(),
                Some(operator.metadata.id.as_str())
            );
            assert_eq!(
                template.spec.execution.interpreter.as_deref(),
                Some("Rscript")
            );
            assert_eq!(
                template.spec.execution.argv,
                operator.execution.argv[2..].to_vec(),
                "{plugin_name} rendered argv must preserve backing operator args"
            );
            let rendered_interface = parse_template_operator_interface(&template)
                .expect("template interface should inherit target interface");
            assert_eq!(
                serde_json::to_value(rendered_interface).unwrap(),
                serde_json::to_value(operator.interface).unwrap(),
                "{plugin_name} rendered template must inherit backing operator interface"
            );
            let template_source =
                fs::read_to_string(resolve_template_entry_path(&template).unwrap())
                    .expect("template source");
            assert!(
                !template_source.contains("system2("),
                "{plugin_name} V3 template body must not shell out to a legacy operator script"
            );
            let legacy_script_name = Path::new(&operator.execution.argv[1])
                .file_name()
                .and_then(|value| value.to_str())
                .expect("legacy script name");
            assert!(
                !template_source.contains(legacy_script_name),
                "{plugin_name} V3 template body must not reference legacy script `{legacy_script_name}`"
            );
        }
    }

    #[test]
    fn bundled_template_contract_snapshot_is_stable() {
        let cases = [
            (
                "operator-differential-expression-r",
                "differential-expression-basic",
                "differential-expression-basic",
            ),
            ("operator-pca-r", "pca-matrix", "pca-matrix"),
            (
                "operator-enrichment-r",
                "functional-enrichment-basic",
                "functional-enrichment-basic",
            ),
        ];
        let snapshots = cases
            .into_iter()
            .map(|(plugin_name, template_dir, operator_dir)| {
                let (template, operator) =
                    bundled_template_and_operator(plugin_name, template_dir, operator_dir);
                (
                    template.spec.metadata.id.clone(),
                    template_contract_snapshot(&template, &operator),
                )
            })
            .collect::<BTreeMap<_, _>>();

        assert_eq!(
            serde_json::to_value(snapshots).unwrap(),
            json!({
                "bulk_differential_expression_basic": {
                    "version": "0.3.0",
                    "envRef": "r-bioc",
                    "migrationTarget": "omics_differential_expression_basic",
                    "fallbackToMigrationTarget": true,
                    "inputs": ["matrix", "metadata"],
                    "params": ["case_group", "comparisons", "control_group", "de_method", "delimiter", "group_column", "input_data_type", "log2fc_threshold", "pseudocount", "pvalue_threshold", "row_names", "sample_column"],
                    "outputs": ["beeswarm_plot", "beeswarm_plot_pdf", "plot", "plot_pdf", "quadrant_plot", "quadrant_plot_pdf", "results", "significant", "summary", "volcano_plot", "volcano_plot_pdf"],
                    "preflightParams": ["input_data_type", "de_method", "pvalue_threshold", "log2fc_threshold"],
                    "argvAfterScript": ["${inputs.matrix}", "${inputs.metadata}", "${outdir}", "${params.group_column}", "${params.case_group}", "${params.control_group}", "${params.sample_column}", "${params.delimiter}", "${params.row_names}", "${params.pseudocount}", "${params.log2fc_threshold}", "${params.pvalue_threshold}", "${params.comparisons}", "${params.input_data_type}", "${params.de_method}"]
                },
                "functional_enrichment_basic": {
                    "version": "0.3.0",
                    "envRef": "r-bioc",
                    "migrationTarget": "omics_functional_enrichment_basic",
                    "fallbackToMigrationTarget": true,
                    "inputs": ["gene_sets", "genes"],
                    "params": ["analysis_mode", "display_top_n", "gene_column", "gene_sets_format", "gsea_weight", "max_size", "min_size", "plot_style", "pvalue_threshold", "score_column"],
                    "outputs": ["barplot", "dotplot", "gsea_curve", "plot", "results", "summary", "top"],
                    "preflightParams": [],
                    "argvAfterScript": ["${inputs.genes}", "${inputs.gene_sets}", "${outdir}", "${params.gene_sets_format}", "${params.min_size}", "${params.max_size}", "${params.pvalue_threshold}", "${params.analysis_mode}", "${params.gene_column}", "${params.score_column}", "${params.display_top_n}", "${params.plot_style}", "${params.gsea_weight}"]
                },
                "pca_matrix_basic": {
                    "version": "0.3.0",
                    "envRef": "r-bioc",
                    "migrationTarget": "omics_pca_matrix",
                    "fallbackToMigrationTarget": true,
                    "inputs": ["matrix", "metadata"],
                    "params": ["center", "confidence_hulls", "delimiter", "features_by_rows", "group_column", "plot_labels", "row_names", "sample_column", "scale", "top_variable_features"],
                    "outputs": ["group_summary", "loadings", "plot", "scores", "scree_plot", "summary", "variance"],
                    "preflightParams": [],
                    "argvAfterScript": ["${inputs.matrix}", "${outdir}", "${params.delimiter}", "${params.row_names}", "${params.features_by_rows}", "${params.center}", "${params.scale}", "${params.top_variable_features}", "${inputs.metadata}", "${params.sample_column}", "${params.group_column}", "${params.plot_labels}", "${params.confidence_hulls}"]
                }
            })
        );
    }

    fn template_contract_snapshot(
        template: &TemplateSpecWithSource,
        operator: &crate::domain::operators::OperatorSpec,
    ) -> JsonValue {
        let rendered_interface =
            parse_template_operator_interface(template).expect("operator-compatible interface");
        json!({
            "version": template.spec.metadata.version,
            "envRef": template.spec.runtime.env_ref,
            "migrationTarget": template.spec.migration_target,
            "fallbackToMigrationTarget": template_fallback_to_migration_target(template),
            "inputs": rendered_interface.inputs.keys().cloned().collect::<Vec<_>>(),
            "params": rendered_interface.params.keys().cloned().collect::<Vec<_>>(),
            "outputs": rendered_interface.outputs.keys().cloned().collect::<Vec<_>>(),
            "preflightParams": operator
                .preflight
                .as_ref()
                .map(|preflight| preflight
                    .questions
                    .iter()
                    .map(|question| question.param.clone())
                    .collect::<Vec<_>>())
                .unwrap_or_default(),
            "argvAfterScript": template.spec.execution.argv,
        })
    }

    #[test]
    fn bundled_templates_resolve_environment_profiles() {
        for (plugin_name, template_dir, operator_dir) in [
            (
                "operator-differential-expression-r",
                "differential-expression-basic",
                "differential-expression-basic",
            ),
            ("operator-pca-r", "pca-matrix", "pca-matrix"),
            (
                "operator-enrichment-r",
                "functional-enrichment-basic",
                "functional-enrichment-basic",
            ),
        ] {
            let (template, _operator) =
                bundled_template_and_operator(plugin_name, template_dir, operator_dir);
            let resolved = template_environment_resolution(&template);
            assert_eq!(resolved.status, "resolved", "{plugin_name}");
            assert_eq!(resolved.env_ref.as_deref(), Some("r-bioc"));
            assert!(
                resolved
                    .canonical_id
                    .as_deref()
                    .unwrap_or_default()
                    .ends_with("/environment/r-bioc"),
                "{plugin_name}"
            );
            assert_eq!(
                resolved
                    .profile
                    .as_ref()
                    .unwrap()
                    .runtime
                    .command
                    .as_deref(),
                Some("Rscript")
            );
        }
    }

    #[test]
    fn render_template_text_injects_template_source_context() {
        let (template, _operator) =
            bundled_template_and_operator("operator-pca-r", "pca-matrix", "pca-matrix");
        let rendered = render_template_text(
            "{{ template.pluginRoot }}|{{template.manifestDir}}|${template.sourcePlugin}",
            &template,
            &crate::domain::operators::OperatorInvocation::default(),
        )
        .expect("render");

        assert!(rendered.contains("operator-pca-r"));
        assert!(rendered.contains("templates/pca-matrix"));
        assert!(rendered.contains("operator-pca-r@omiga-curated"));
    }

    #[test]
    fn template_preflight_reuses_migration_target_questions() {
        let (template, _operator) = bundled_template_and_operator(
            "operator-differential-expression-r",
            "differential-expression-basic",
            "differential-expression-basic",
        );
        let args = serde_json::to_string(&json!({
            "id": "bulk_differential_expression_basic",
            "inputs": {},
            "params": {
                "input_data_type": "auto",
                "de_method": "auto"
            },
            "resources": {}
        }))
        .unwrap();
        let value = serde_json::from_str::<JsonValue>(&args).unwrap();
        let root = value.as_object().unwrap();

        let question_args = template_preflight_question_for_template(&template, root, None)
            .expect("DE template should inherit backing operator preflight");

        assert_eq!(question_args.questions.len(), 4);
        assert_eq!(
            question_args.metadata.as_ref().unwrap()["source"],
            "template_preflight"
        );
        assert_eq!(
            question_args.metadata.as_ref().unwrap()["migration_target"],
            "omics_differential_expression_basic"
        );
        assert!(question_args
            .questions
            .iter()
            .any(|question| question.question.contains("主要统计方法")));
    }

    #[test]
    fn apply_template_preflight_answers_updates_template_params() {
        let (template, _operator) = bundled_template_and_operator(
            "operator-differential-expression-r",
            "differential-expression-basic",
            "differential-expression-basic",
        );
        let args = serde_json::to_string(&json!({
            "id": "bulk_differential_expression_basic",
            "inputs": {},
            "params": {
                "input_data_type": "auto",
                "de_method": "auto"
            },
            "resources": {}
        }))
        .unwrap();
        let ask_output = json!({
            "answers": {
                "差异表达前，请选择输入矩阵的数据类型？": "Counts",
                "差异表达前，请选择主要统计方法？": "DESeq2",
                "显著差异基因使用哪个 FDR 阈值？": "FDR 0.01",
                "显著差异基因使用哪个 |log2FC| 阈值？": "|log2FC|≥2"
            }
        });

        let mut parsed = serde_json::from_str::<JsonValue>(&args).unwrap();
        let root = parsed.as_object_mut().unwrap();
        apply_template_preflight_answers_for_template(root, &template, &ask_output)
            .expect("template preflight answers should apply");

        assert_eq!(parsed["params"]["input_data_type"], "counts");
        assert_eq!(parsed["params"]["de_method"], "deseq2");
        assert_eq!(parsed["params"]["pvalue_threshold"], 0.01);
        assert_eq!(parsed["params"]["log2fc_threshold"], 2);
        assert_eq!(
            parsed["metadata"]["preflight"]["paramsBySource"]["de_method"],
            "user_preflight"
        );
        assert_eq!(
            parsed["metadata"]["preflight"]["answeredParams"]
                .as_array()
                .unwrap()
                .len(),
            4
        );
    }

    #[tokio::test]
    async fn execute_rendered_template_runs_shell_script() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let plugin_root = tmp.path().join("plugin");
        let environment_dir = plugin_root.join("environments").join("r-bioc");
        let template_dir = plugin_root.join("templates").join("demo");
        fs::create_dir_all(&environment_dir).expect("env dir");
        fs::create_dir_all(&template_dir).expect("mkdir");
        fs::write(
            environment_dir.join("environment.yaml"),
            r#"apiVersion: omiga.ai/environment/v1alpha1
kind: Environment
metadata:
  id: r-bioc
  version: 0.1.0
runtime:
  type: system
  command: /bin/sh
"#,
        )
        .expect("env");
        fs::write(
            template_dir.join("run.sh"),
            r#"#!/bin/sh
set -eu
outdir="$1"
mkdir -p "$outdir"
printf 'hello %s\n' "{{ params.message }}" > "$outdir/report.txt"
printf '%s\n' '{"ok":true,"message":"{{ params.message }}"}' > "$outdir/outputs.json"
"#,
        )
        .expect("script");
        fs::write(
            template_dir.join("template.yaml"),
            r#"apiVersion: omiga.ai/unit/v1alpha1
kind: Template
metadata:
  id: rendered_demo
  version: 0.1.0
  name: Rendered Demo
interface:
  params:
    message:
      kind: string
      required: true
  outputs:
    report:
      kind: file
      required: true
      glob: report.txt
    ok:
      kind: boolean
      required: true
    message:
      kind: string
      required: true
runtime:
  envRef: r-bioc
template:
  engine: jinja2
  entry: ./run.sh
execution:
  interpreter: /bin/sh
  argv:
    - ${outdir}
"#,
        )
        .expect("manifest");
        let template = load_template_manifest(
            &template_dir.join("template.yaml"),
            "demo@local",
            plugin_root,
        )
        .expect("template");
        let ctx = crate::domain::tools::ToolContext::new(tmp.path());
        let invocation = crate::domain::operators::OperatorInvocation {
            inputs: BTreeMap::new(),
            params: BTreeMap::from([("message".to_string(), json!("world"))]),
            resources: BTreeMap::new(),
            metadata: BTreeMap::new(),
        };

        let (raw, is_error, metadata) =
            execute_template_runtime(&ctx, &template, &invocation, None)
                .await
                .expect("rendered execution");

        assert!(!is_error, "{raw}");
        let parsed: JsonValue = serde_json::from_str(&raw).expect("operator result json");
        assert_eq!(parsed["status"], "succeeded");
        assert_eq!(parsed["structuredOutputs"]["message"], "world");
        let report = parsed["outputs"]["report"][0]["path"]
            .as_str()
            .expect("report path");
        assert_eq!(fs::read_to_string(report).expect("report"), "hello world\n");
        assert_eq!(metadata["executionMode"], "renderedTemplate");
        assert_eq!(metadata["environment"]["status"], "resolved");
        assert_eq!(
            metadata["environment"]["canonicalId"],
            "demo@local/environment/r-bioc"
        );
    }

    #[tokio::test]
    async fn rendered_pca_template_matches_migration_target_fixture_outputs() {
        if !rscript_available() {
            eprintln!("skipping PCA rendered parity fixture: Rscript unavailable");
            return;
        }

        let tmp = tempfile::tempdir().expect("tempdir");
        let matrix = tmp.path().join("pca-matrix.tsv");
        let metadata = tmp.path().join("pca-metadata.tsv");
        fs::write(
            &matrix,
            "gene\ts1\ts2\ts3\ts4\n\
             g1\t10\t11\t2\t1\n\
             g2\t5\t6\t7\t8\n\
             g3\t100\t98\t30\t33\n\
             g4\t3\t4\t20\t21\n",
        )
        .expect("matrix");
        fs::write(&metadata, "sample\tgroup\ns1\tA\ns2\tA\ns3\tB\ns4\tB\n").expect("metadata");
        let (template, _operator) =
            bundled_template_and_operator("operator-pca-r", "pca-matrix", "pca-matrix");
        let invocation = crate::domain::operators::OperatorInvocation {
            inputs: BTreeMap::from([
                ("matrix".to_string(), json!(matrix.to_string_lossy())),
                ("metadata".to_string(), json!(metadata.to_string_lossy())),
            ]),
            params: BTreeMap::from([
                ("delimiter".to_string(), json!("tab")),
                ("row_names".to_string(), json!(true)),
                ("features_by_rows".to_string(), json!(true)),
                ("center".to_string(), json!(true)),
                ("scale".to_string(), json!(false)),
                ("top_variable_features".to_string(), json!(4)),
                ("sample_column".to_string(), json!("sample")),
                ("group_column".to_string(), json!("group")),
                ("plot_labels".to_string(), json!(false)),
                ("confidence_hulls".to_string(), json!(false)),
            ]),
            resources: BTreeMap::new(),
            metadata: BTreeMap::new(),
        };
        let ctx = crate::domain::tools::ToolContext::new(tmp.path());

        let (rendered_raw, rendered_is_error, rendered_metadata) =
            execute_rendered_template(&ctx, &template, &invocation, None)
                .await
                .expect("rendered template");
        let (target_raw, target_is_error, target_metadata) =
            execute_template_via_migration_target(&ctx, &template, &invocation, None)
                .await
                .expect("migration target");

        assert!(!rendered_is_error, "{rendered_raw}");
        assert!(!target_is_error, "{target_raw}");
        assert_eq!(rendered_metadata["executionMode"], "renderedTemplate");
        assert_eq!(target_metadata["executionMode"], "migrationTarget");

        let rendered: JsonValue = serde_json::from_str(&rendered_raw).expect("rendered json");
        let target: JsonValue = serde_json::from_str(&target_raw).expect("target json");
        assert_eq!(rendered["status"], "succeeded");
        assert_eq!(target["status"], "succeeded");
        assert_eq!(
            rendered["structuredOutputs"]["samples"],
            target["structuredOutputs"]["samples"]
        );
        assert_eq!(
            rendered["structuredOutputs"]["groups"],
            target["structuredOutputs"]["groups"]
        );
        assert_eq!(
            rendered["structuredOutputs"]["featuresUsed"],
            target["structuredOutputs"]["featuresUsed"]
        );
        for output_name in ["scores", "variance", "group_summary"] {
            assert_eq!(
                output_file_contents(&rendered, output_name),
                output_file_contents(&target, output_name),
                "PCA output `{output_name}` should match between rendered template and migration target"
            );
        }
    }

    #[tokio::test]
    async fn rendered_de_template_matches_migration_target_fixture_outputs_when_limma_available() {
        if !r_packages_available(&["limma"]) {
            eprintln!("skipping DE rendered parity fixture: Rscript/limma unavailable");
            return;
        }

        let tmp = tempfile::tempdir().expect("tempdir");
        let matrix = tmp.path().join("de-matrix.tsv");
        let metadata = tmp.path().join("de-metadata.tsv");
        fs::write(
            &matrix,
            "gene\ts1\ts2\ts3\ts4\ts5\ts6\n\
             g1\t10.2\t11.1\t10.7\t21.4\t22.0\t20.9\n\
             g2\t44.0\t43.2\t45.1\t42.8\t43.7\t44.4\n\
             g3\t5.1\t5.4\t5.2\t1.8\t1.7\t1.9\n\
             g4\t30.0\t31.2\t29.8\t33.1\t34.0\t32.5\n\
             g5\t8.0\t8.3\t8.1\t16.1\t16.5\t15.8\n",
        )
        .expect("matrix");
        fs::write(
            &metadata,
            "sample\tgroup\ns1\tControl\ns2\tControl\ns3\tControl\ns4\tCase\ns5\tCase\ns6\tCase\n",
        )
        .expect("metadata");
        let (template, _operator) = bundled_template_and_operator(
            "operator-differential-expression-r",
            "differential-expression-basic",
            "differential-expression-basic",
        );
        let invocation = crate::domain::operators::OperatorInvocation {
            inputs: BTreeMap::from([
                ("matrix".to_string(), json!(matrix.to_string_lossy())),
                ("metadata".to_string(), json!(metadata.to_string_lossy())),
            ]),
            params: BTreeMap::from([
                ("group_column".to_string(), json!("group")),
                ("case_group".to_string(), json!("Case")),
                ("control_group".to_string(), json!("Control")),
                ("sample_column".to_string(), json!("sample")),
                ("delimiter".to_string(), json!("tab")),
                ("row_names".to_string(), json!(true)),
                ("pseudocount".to_string(), json!(1)),
                ("log2fc_threshold".to_string(), json!(0.5)),
                ("pvalue_threshold".to_string(), json!(0.25)),
                ("comparisons".to_string(), json!("")),
                ("input_data_type".to_string(), json!("quantitative")),
                ("de_method".to_string(), json!("limma")),
            ]),
            resources: BTreeMap::new(),
            metadata: BTreeMap::new(),
        };
        let ctx = crate::domain::tools::ToolContext::new(tmp.path());

        let (rendered_raw, rendered_is_error, rendered_metadata) =
            execute_rendered_template(&ctx, &template, &invocation, None)
                .await
                .expect("rendered template");
        let (target_raw, target_is_error, target_metadata) =
            execute_template_via_migration_target(&ctx, &template, &invocation, None)
                .await
                .expect("migration target");

        assert!(!rendered_is_error, "{rendered_raw}");
        assert!(!target_is_error, "{target_raw}");
        assert_eq!(rendered_metadata["executionMode"], "renderedTemplate");
        assert_eq!(target_metadata["executionMode"], "migrationTarget");

        let rendered: JsonValue = serde_json::from_str(&rendered_raw).expect("rendered json");
        let target: JsonValue = serde_json::from_str(&target_raw).expect("target json");
        assert_eq!(rendered["status"], "succeeded");
        assert_eq!(target["status"], "succeeded");
        for key in [
            "featuresTested",
            "comparisons",
            "significantRows",
            "inputDataType",
            "method",
            "requestedMethod",
        ] {
            assert_eq!(
                rendered["structuredOutputs"][key], target["structuredOutputs"][key],
                "DE structured output `{key}` should match"
            );
        }
        for output_name in ["results", "significant"] {
            assert_eq!(
                output_file_contents(&rendered, output_name),
                output_file_contents(&target, output_name),
                "DE output `{output_name}` should match between rendered template and migration target"
            );
        }
    }

    #[tokio::test]
    async fn rendered_enrichment_template_matches_migration_target_fixture_outputs_when_clusterprofiler_available(
    ) {
        if !r_packages_available(&["clusterProfiler"]) {
            eprintln!(
                "skipping enrichment rendered parity fixture: Rscript/clusterProfiler unavailable"
            );
            return;
        }

        let tmp = tempfile::tempdir().expect("tempdir");
        let genes = tmp.path().join("genes.txt");
        let gene_sets = tmp.path().join("gene-sets.tsv");
        fs::write(&genes, "gene\nTP53\nBRCA1\nBRCA2\nATM\nCHEK2\n").expect("genes");
        fs::write(
            &gene_sets,
            "term\tgene\n\
             DNA_REPAIR\tTP53\n\
             DNA_REPAIR\tBRCA1\n\
             DNA_REPAIR\tBRCA2\n\
             DNA_REPAIR\tATM\n\
             DNA_REPAIR\tCHEK2\n\
             APOPTOSIS\tTP53\n\
             APOPTOSIS\tBAX\n\
             APOPTOSIS\tCASP3\n\
             CELL_CYCLE\tCDK1\n\
             CELL_CYCLE\tCCNB1\n\
             CELL_CYCLE\tCHEK2\n",
        )
        .expect("gene sets");
        let (template, _operator) = bundled_template_and_operator(
            "operator-enrichment-r",
            "functional-enrichment-basic",
            "functional-enrichment-basic",
        );
        let invocation = crate::domain::operators::OperatorInvocation {
            inputs: BTreeMap::from([
                ("genes".to_string(), json!(genes.to_string_lossy())),
                ("gene_sets".to_string(), json!(gene_sets.to_string_lossy())),
            ]),
            params: BTreeMap::from([
                ("gene_sets_format".to_string(), json!("tsv")),
                ("min_size".to_string(), json!(1)),
                ("max_size".to_string(), json!(20)),
                ("pvalue_threshold".to_string(), json!(1.0)),
                ("analysis_mode".to_string(), json!("ora")),
                ("gene_column".to_string(), json!("auto")),
                ("score_column".to_string(), json!("auto")),
                ("display_top_n".to_string(), json!(10)),
                ("plot_style".to_string(), json!("bar")),
                ("gsea_weight".to_string(), json!(1)),
            ]),
            resources: BTreeMap::new(),
            metadata: BTreeMap::new(),
        };
        let ctx = crate::domain::tools::ToolContext::new(tmp.path());

        let (rendered_raw, rendered_is_error, rendered_metadata) =
            execute_rendered_template(&ctx, &template, &invocation, None)
                .await
                .expect("rendered template");
        let (target_raw, target_is_error, target_metadata) =
            execute_template_via_migration_target(&ctx, &template, &invocation, None)
                .await
                .expect("migration target");

        assert!(!rendered_is_error, "{rendered_raw}");
        assert!(!target_is_error, "{target_raw}");
        assert_eq!(rendered_metadata["executionMode"], "renderedTemplate");
        assert_eq!(target_metadata["executionMode"], "migrationTarget");

        let rendered: JsonValue = serde_json::from_str(&rendered_raw).expect("rendered json");
        let target: JsonValue = serde_json::from_str(&target_raw).expect("target json");
        assert_eq!(rendered["status"], "succeeded");
        assert_eq!(target["status"], "succeeded");
        for key in [
            "analysisMode",
            "method",
            "queryGenes",
            "geneSetsTested",
            "significant",
            "displayTopN",
        ] {
            assert_eq!(
                rendered["structuredOutputs"][key], target["structuredOutputs"][key],
                "enrichment structured output `{key}` should match"
            );
        }
        for output_name in ["results", "top"] {
            assert_eq!(
                output_file_contents(&rendered, output_name),
                output_file_contents(&target, output_name),
                "enrichment output `{output_name}` should match between rendered template and migration target"
            );
        }
    }

    fn output_file_contents(result: &JsonValue, output_name: &str) -> String {
        let path = result["outputs"][output_name][0]["path"]
            .as_str()
            .unwrap_or_else(|| panic!("missing output path `{output_name}`"));
        fs::read_to_string(path)
            .unwrap_or_else(|err| panic!("read output `{output_name}` at `{path}`: {err}"))
    }

    fn rscript_available() -> bool {
        std::process::Command::new("Rscript")
            .arg("--version")
            .output()
            .is_ok()
    }

    fn r_packages_available(packages: &[&str]) -> bool {
        if packages.is_empty() {
            return rscript_available();
        }
        let quoted = packages
            .iter()
            .map(|package| format!("'{package}'"))
            .collect::<Vec<_>>()
            .join(",");
        let script = format!(
            "pkgs <- c({quoted}); ok <- all(vapply(pkgs, requireNamespace, logical(1), quietly = TRUE)); if (!ok) quit(status = 1)"
        );
        std::process::Command::new("Rscript")
            .args(["-e", script.as_str()])
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    }

    #[tokio::test]
    async fn rendered_template_failure_falls_back_and_records_child_lineage() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let plugin_root = tmp.path().join("fallback-plugin");
        let script_dir = plugin_root.join("scripts");
        let operator_dir = plugin_root.join("operators").join("fallback-report");
        let template_dir = plugin_root.join("templates").join("fallback-report");
        fs::create_dir_all(&script_dir).expect("scripts");
        fs::create_dir_all(&operator_dir).expect("operator dir");
        fs::create_dir_all(&template_dir).expect("template dir");
        fs::write(
            script_dir.join("write_report.sh"),
            r#"#!/bin/sh
set -eu
outdir="$1"
message="$2"
mkdir -p "$outdir"
printf '%s\n' "$message" > "$outdir/report.txt"
printf '{"message":"%s"}\n' "$message" > "$outdir/outputs.json"
"#,
        )
        .expect("operator script");
        fs::write(
            operator_dir.join("operator.yaml"),
            r#"apiVersion: omiga.ai/operator/v1alpha1
kind: Operator
metadata:
  id: local_fallback_report
  version: 0.1.0
  name: Local fallback report
interface:
  params:
    message:
      kind: string
      required: true
  outputs:
    report:
      kind: file
      required: true
      glob: report.txt
    message:
      kind: string
      required: true
runtime:
  placement:
    supported: [local]
  container:
    supported: [none]
execution:
  argv:
    - /bin/sh
    - ./scripts/write_report.sh
    - ${outdir}
    - ${params.message}
"#,
        )
        .expect("operator manifest");
        fs::write(
            template_dir.join("fail.sh"),
            r#"#!/bin/sh
echo "primary rendered path failed intentionally" >&2
exit 7
"#,
        )
        .expect("template script");
        fs::write(
            template_dir.join("template.yaml"),
            r#"apiVersion: omiga.ai/unit/v1alpha1
kind: Template
metadata:
  id: fallback_template
  version: 0.1.0
  name: Fallback Template
interface:
  inputs: {}
  params: {}
  outputs: {}
template:
  engine: jinja2
  entry: ./fail.sh
execution:
  interpreter: /bin/sh
  fallbackToMigrationTarget: true
migrationTarget: local_fallback_report
"#,
        )
        .expect("template manifest");
        let template = load_template_manifest(
            &template_dir.join("template.yaml"),
            "fallback@local",
            plugin_root,
        )
        .expect("template");
        let ctx = crate::domain::tools::ToolContext::new(tmp.path());
        let invocation = crate::domain::operators::OperatorInvocation {
            inputs: BTreeMap::new(),
            params: BTreeMap::from([("message".to_string(), json!("fallback ok"))]),
            resources: BTreeMap::new(),
            metadata: BTreeMap::new(),
        };
        let started_at = "2026-05-09T00:00:00Z";
        let parent_id = record_template_start_best_effort(&ctx, &template, &invocation, started_at)
            .await
            .expect("parent record");

        let (raw, is_error, metadata) =
            execute_template_runtime(&ctx, &template, &invocation, Some(parent_id.as_str()))
                .await
                .expect("fallback execution");
        record_template_execution_best_effort(
            &ctx,
            &template,
            &invocation,
            started_at,
            &raw,
            is_error,
            metadata.clone(),
            Some(parent_id.as_str()),
        )
        .await;

        assert!(!is_error, "{raw}");
        let parsed: JsonValue = serde_json::from_str(&raw).expect("fallback json");
        assert_eq!(parsed["status"], "succeeded");
        assert_eq!(parsed["structuredOutputs"]["message"], "fallback ok");
        assert_eq!(metadata["executionMode"], "fallbackMigrationTarget");
        assert_eq!(metadata["fallback"]["executionMode"], "migrationTarget");

        let children = crate::domain::execution_records::list_child_execution_records(
            tmp.path(),
            &parent_id,
            10,
        )
        .await
        .expect("children");
        assert_eq!(children.len(), 2);
        assert!(children.iter().any(|record| {
            record.kind == "operator"
                && record.unit_id.as_deref() == Some("fallback_template")
                && record.status == "failed"
        }));
        assert!(children.iter().any(|record| {
            record.kind == "operator"
                && record.unit_id.as_deref() == Some("local_fallback_report")
                && record.status == "succeeded"
        }));
        let parent =
            crate::domain::execution_records::list_recent_execution_records(tmp.path(), 10)
                .await
                .expect("records")
                .into_iter()
                .find(|record| record.id == parent_id)
                .expect("parent");
        assert_eq!(parent.status, "succeeded");
    }

    #[tokio::test]
    async fn execute_template_via_migration_target_reuses_operator_runtime() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let fixture_root = legacy_plugin_root("operator-smoke");
        let operator_dir = tmp.path().join("operators").join("write-text-report");
        let script_dir = tmp.path().join("scripts");
        fs::create_dir_all(&operator_dir).expect("operator dir");
        fs::create_dir_all(&script_dir).expect("script dir");
        fs::copy(
            fixture_root
                .join("operators")
                .join("write-text-report")
                .join("operator.yaml"),
            operator_dir.join("operator.yaml"),
        )
        .expect("operator fixture");
        fs::copy(
            fixture_root.join("scripts").join("write_text_report.sh"),
            script_dir.join("write_text_report.sh"),
        )
        .expect("operator script fixture");
        let template = TemplateSpecWithSource {
            spec: TemplateSpec {
                api_version: TEMPLATE_API_VERSION_V1ALPHA1.to_string(),
                kind: TEMPLATE_KIND.to_string(),
                metadata: TemplateMetadata {
                    id: "smoke_template".to_string(),
                    version: "0.1.0".to_string(),
                    name: Some("Smoke Template".to_string()),
                    description: None,
                    tags: Vec::new(),
                },
                classification: TemplateClassification::default(),
                exposure: TemplateExposure::default(),
                interface: JsonValue::Null,
                runtime: TemplateRuntime::default(),
                template: TemplateBody {
                    engine: "static".to_string(),
                    entry: PathBuf::from("./template.yaml"),
                },
                aliases: Vec::new(),
                execution: TemplateExecution::default(),
                migration_target: Some("write_text_report".to_string()),
            },
            source: TemplateSource {
                source_plugin: "template-smoke@local".to_string(),
                plugin_root: tmp.path().to_path_buf(),
                manifest_path: tmp.path().join("template.yaml"),
            },
        };
        let ctx = crate::domain::tools::ToolContext::new(tmp.path());
        let invocation = crate::domain::operators::OperatorInvocation {
            inputs: BTreeMap::new(),
            params: BTreeMap::from([
                ("message".to_string(), json!("delegated template")),
                ("repeat".to_string(), json!(2)),
            ]),
            resources: BTreeMap::new(),
            metadata: BTreeMap::new(),
        };
        let started_at = "2026-05-09T00:00:00Z";
        let parent_id = record_template_start_best_effort(&ctx, &template, &invocation, started_at)
            .await
            .expect("template parent record");

        let (raw, is_error, metadata) =
            execute_template_via_migration_target(&ctx, &template, &invocation, Some(&parent_id))
                .await
                .expect("migration target execution");

        assert!(!is_error, "{raw}");
        assert_eq!(metadata["executionMode"], "migrationTarget");
        assert_eq!(metadata["migrationTarget"], "write_text_report");
        let parsed: JsonValue = serde_json::from_str(&raw).expect("operator result json");
        assert_eq!(parsed["status"], "succeeded");
        assert_eq!(parsed["operator"]["id"], "write_text_report");
        assert_eq!(parsed["runContext"]["kind"], "template");
        assert_eq!(parsed["runContext"]["parentExecutionId"], parent_id);
        let report = parsed["outputs"]["report"][0]["path"]
            .as_str()
            .expect("report path");
        assert_eq!(
            fs::read_to_string(report).expect("report"),
            "delegated template\ndelegated template\n"
        );
        record_template_execution_best_effort(
            &ctx,
            &template,
            &invocation,
            started_at,
            &raw,
            is_error,
            metadata,
            Some(&parent_id),
        )
        .await;

        let records =
            crate::domain::execution_records::list_recent_execution_records(tmp.path(), 10)
                .await
                .expect("records");
        assert_eq!(records.len(), 2);
        let parent = records
            .iter()
            .find(|record| record.id == parent_id)
            .expect("parent record");
        assert_eq!(parent.kind, "template");
        assert_eq!(parent.status, "succeeded");
        let child = records
            .iter()
            .find(|record| record.kind == "operator")
            .expect("child operator record");
        assert_eq!(child.unit_id.as_deref(), Some("write_text_report"));
        assert_eq!(
            child.parent_execution_id.as_deref(),
            Some(parent_id.as_str())
        );
        assert_eq!(child.status, "succeeded");
    }

    #[tokio::test]
    async fn record_template_execution_writes_project_scoped_record() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let manifest = tmp.path().join("template.yaml");
        write_valid_template(&manifest, "recorded_template");
        let template =
            load_template_manifest(&manifest, "demo@local", tmp.path()).expect("template");
        let ctx = crate::domain::tools::ToolContext::new(tmp.path())
            .with_session_id(Some("session-template".to_string()));
        let invocation = crate::domain::operators::OperatorInvocation {
            inputs: BTreeMap::new(),
            params: BTreeMap::from([("alpha".to_string(), json!(0.05))]),
            resources: BTreeMap::new(),
            metadata: BTreeMap::new(),
        };

        record_template_execution_best_effort(
            &ctx,
            &template,
            &invocation,
            "2026-05-09T00:00:00Z",
            r#"{"status":"succeeded","runId":"oprun_template","outputs":{"report":[]}}"#,
            false,
            json!({"executionMode": "migrationTarget"}),
            None,
        )
        .await;

        let rows = crate::domain::execution_records::list_recent_execution_records(tmp.path(), 10)
            .await
            .expect("records");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].kind, "template");
        assert_eq!(rows[0].unit_id.as_deref(), Some("recorded_template"));
        assert_eq!(
            rows[0].canonical_id.as_deref(),
            Some("demo@local/template/recorded_template")
        );
        assert_eq!(rows[0].status, "succeeded");
        assert_eq!(rows[0].session_id.as_deref(), Some("session-template"));
    }
}
