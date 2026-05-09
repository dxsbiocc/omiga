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
                )
                .await;
                return (
                    template_failure_json(template_id, Some(&template), &message, None),
                    true,
                );
            }
        };

    let result = if uses_existing_operator(&template) {
        execute_template_via_migration_target(ctx, &template, &invocation).await
    } else {
        execute_rendered_template(ctx, &template, &invocation).await
    };

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
            )
            .await;
            (
                template_failure_json(template_id, Some(&template), &message, None),
                true,
            )
        }
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
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [only] => Ok(only.clone()),
        [] => Err(format!(
            "Template `{id}` was not found. Use unit_search kind=template to inspect templates."
        )),
        many => Err(format!(
            "Template `{id}` is ambiguous across {} candidates; use canonical id.",
            many.len()
        )),
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

async fn execute_template_via_migration_target(
    ctx: &crate::domain::tools::ToolContext,
    template: &TemplateSpecWithSource,
    invocation: &crate::domain::operators::OperatorInvocation,
) -> Result<(String, bool, JsonValue), String> {
    let target = template.spec.migration_target.as_deref().ok_or_else(|| {
        "template migrationTarget is required for existing-operator execution".to_string()
    })?;
    let (alias, spec) =
        crate::domain::operators::describe_operator(target).map_err(|error| error.message)?;
    let alias = alias.unwrap_or_else(|| target.to_string());
    let arguments = serde_json::to_string(invocation)
        .map_err(|err| format!("serialize delegated operator invocation: {err}"))?;
    let run_context = crate::domain::operators::OperatorRunContext {
        kind: Some("template".to_string()),
        smoke_test_id: Some(template.spec.metadata.id.clone()),
        smoke_test_name: template.spec.metadata.name.clone(),
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
) -> Result<(String, bool, JsonValue), String> {
    let rendered = render_template_script(ctx, template, invocation)?;
    let interface = parse_template_operator_interface(template)?;
    let runtime = serde_json::to_value(&template.spec.runtime).ok();
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
        preflight: None,
        runtime,
        cache: None,
        resources: BTreeMap::new(),
        bindings: Vec::new(),
        permissions: None,
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
    if template.spec.interface.is_null() {
        return Ok(crate::domain::operators::OperatorInterfaceSpec::default());
    }
    serde_json::from_value(template.spec.interface.clone())
        .map_err(|err| format!("template interface is not operator-compatible: {err}"))
}

async fn record_template_execution_best_effort(
    ctx: &crate::domain::tools::ToolContext,
    template: &TemplateSpecWithSource,
    invocation: &crate::domain::operators::OperatorInvocation,
    started_at: &str,
    raw: &str,
    is_error: bool,
    metadata: JsonValue,
) {
    let parsed = serde_json::from_str::<JsonValue>(raw).ok();
    let status = parsed
        .as_ref()
        .and_then(|value| value.get("status"))
        .and_then(JsonValue::as_str)
        .unwrap_or(if is_error { "failed" } else { "succeeded" });
    let output_summary = template_output_summary(parsed.as_ref(), is_error);
    let record = crate::domain::execution_records::ExecutionRecordInput {
        kind: "template".to_string(),
        unit_id: Some(template.spec.metadata.id.clone()),
        canonical_id: Some(canonical_template_unit_id(template)),
        provider_plugin: Some(template.source.source_plugin.clone()),
        status: status.to_string(),
        session_id: ctx.session_id.clone(),
        parent_execution_id: None,
        started_at: Some(started_at.to_string()),
        ended_at: Some(chrono::Utc::now().to_rfc3339()),
        input_hash: crate::domain::execution_records::hash_execution_map(&invocation.inputs),
        param_hash: crate::domain::execution_records::hash_execution_map(&invocation.params),
        output_summary_json: Some(output_summary),
        runtime_json: serde_json::to_value(&template.spec.runtime).ok(),
        metadata_json: Some(serde_json::json!({
            "templateId": template.spec.metadata.id,
            "sourcePlugin": template.source.source_plugin,
            "manifestPath": template.source.manifest_path,
            "migrationTarget": template.spec.migration_target,
            "execution": metadata,
            "operatorResult": parsed,
        })),
    };
    crate::domain::execution_records::record_execution_best_effort(&ctx.project_root, record).await;
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
    crate::domain::execution_records::record_execution_best_effort(&ctx.project_root, record).await;
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

fn canonical_template_unit_id(template: &TemplateSpecWithSource) -> String {
    format!(
        "{}/template/{}",
        template.source.source_plugin, template.spec.metadata.id
    )
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

    #[tokio::test]
    async fn execute_rendered_template_runs_shell_script() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let plugin_root = tmp.path().join("plugin");
        let template_dir = plugin_root.join("templates").join("demo");
        fs::create_dir_all(&template_dir).expect("mkdir");
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
        };

        let (raw, is_error, metadata) = execute_rendered_template(&ctx, &template, &invocation)
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
        };

        record_template_execution_best_effort(
            &ctx,
            &template,
            &invocation,
            "2026-05-09T00:00:00Z",
            r#"{"status":"succeeded","runId":"oprun_template","outputs":{"report":[]}}"#,
            false,
            json!({"executionMode": "migrationTarget"}),
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
