use super::*;

const OPERATOR_DEFAULT_MAX_ATTEMPTS: u32 = 2;
const OPERATOR_MAX_MAX_ATTEMPTS: u32 = 5;
const OPERATOR_CACHE_SCAN_LIMIT: usize = 200;
pub type OperatorQueueStatusSender = tokio::sync::mpsc::UnboundedSender<(String, String)>;

tokio::task_local! {
    static OPERATOR_QUEUE_STATUS_SENDER: OperatorQueueStatusSender;
}

pub async fn with_operator_queue_status_sender<F>(
    sender: OperatorQueueStatusSender,
    future: F,
) -> F::Output
where
    F: Future,
{
    OPERATOR_QUEUE_STATUS_SENDER.scope(sender, future).await
}

fn current_operator_queue_status_sender() -> Option<OperatorQueueStatusSender> {
    OPERATOR_QUEUE_STATUS_SENDER
        .try_with(|sender| sender.clone())
        .ok()
}

pub(crate) fn operator_manifest_diagnostics_from_plugins<'a>(
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

pub(crate) async fn execute_resolved_operator(
    ctx: &crate::domain::tools::ToolContext,
    resolved: ResolvedOperator,
    invocation: OperatorInvocation,
    run_context: Option<OperatorRunContext>,
) -> Result<OperatorRunResult, OperatorToolError> {
    let operation_id = operator_operation_from_invocation(&resolved.spec, &invocation)?;
    let mut resolved = resolved;
    resolved.spec = operator_spec_for_operation(&resolved.spec, &operation_id)?;
    let mut invocation = invocation;
    invocation.params.remove("operation");

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
    effective_params.insert("operation".to_string(), json!(operation_id));
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

pub(crate) struct OperatorCacheHit {
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
    if run_context.map(|ctx| ctx.bypass_cache).unwrap_or(false) {
        return false;
    }
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

pub(crate) fn cache_config_enabled(value: Option<&JsonValue>) -> bool {
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

pub(crate) fn operator_cache_key(
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

pub(crate) fn runtime_supported(
    ctx: &crate::domain::tools::ToolContext,
    spec: &OperatorSpec,
) -> bool {
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
pub(crate) enum OperatorContainerKind {
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

impl std::fmt::Display for OperatorContainerKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OperatorContainerSelection {
    pub(crate) kind: OperatorContainerKind,
    pub(crate) image: String,
    pub(crate) prepare: Option<OperatorContainerImagePrepare>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum OperatorContainerImagePrepare {
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

pub(crate) fn sha256_file(path: &str) -> Option<String> {
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

pub(crate) fn parse_remote_path_fingerprint(location: &str, path: &str, output: &str) -> JsonValue {
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

pub(crate) async fn execute_env_command(
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
    let operator_env_hygiene = if ctx.execution_environment == "local" {
        env_hygiene::calculate_env_hygiene_remove_list()
    } else {
        Vec::new()
    };
    if !operator_env_hygiene.is_empty() {
        tracing::debug!(
            names = ?operator_env_hygiene,
            "filtered sensitive env vars before operator execution"
        );
    }
    let command = if ctx.execution_environment == "local" {
        crate::domain::tools::bash::prepend_venv_activation(
            &ctx.local_venv_type,
            &ctx.local_venv_name,
            command,
        )
    } else {
        command.to_string()
    };

    let exec_opts = crate::execution::ExecOptions {
        timeout: Some(timeout_secs * 1000),
        cwd: Some(cwd.to_string()),
        stdin_data: None,
        env_remove_names: if operator_env_hygiene.is_empty() {
            None
        } else {
            Some(operator_env_hygiene.clone())
        },
    };

    let mut guard = env.lock().await;
    let result = guard.execute(&command, exec_opts).await;
    result.map_err(|err| {
        OperatorToolError::new("execution_infra_error", true, err.to_string())
            .with_suggested_action("Retry if the execution backend was temporarily unavailable.")
    })
}

fn operator_environment_cwd(ctx: &crate::domain::tools::ToolContext) -> String {
    crate::domain::tools::env_store::remote_path(ctx, ".")
}

fn operator_uses_slurm_scheduler(
    spec: &OperatorSpec,
    ctx: &crate::domain::tools::ToolContext,
) -> bool {
    if !ctx.execution_environment.trim().eq_ignore_ascii_case("ssh") {
        return false;
    }
    let Some(runtime) = spec.runtime.as_ref() else {
        return false;
    };
    runtime_axis_values(runtime, "scheduler")
        .iter()
        .any(|s| s.eq_ignore_ascii_case("slurm"))
}

struct SlurmExecResult {
    exec_result: crate::execution::ExecResult,
    diagnostic: Option<SacctDiagnostic>,
}

async fn fetch_sacct_diagnostics(
    ctx: &crate::domain::tools::ToolContext,
    job_id: &str,
) -> Option<SacctDiagnostic> {
    let job_id = job_id.trim();
    if job_id.is_empty() {
        return None;
    }
    let job_step = format!("{job_id}.batch");
    let command = format!(
        "sacct -j {} --format=State,ExitCode,MaxRSS,Elapsed,Reason,ReqMem -P --noheader",
        sh_quote(&job_step)
    );
    let result = execute_env_command(ctx, &operator_environment_cwd(ctx), &command, 30)
        .await
        .ok()?;
    if result.returncode != 0 {
        return None;
    }
    parse_sacct_diagnostic_output(&result.output)
}

pub(crate) fn parse_sacct_diagnostic_output(output: &str) -> Option<SacctDiagnostic> {
    let line = output
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())?;
    let fields = line.split('|').collect::<Vec<_>>();
    if fields.len() < 6 {
        return None;
    }
    let state = fields[0].trim().to_string();
    let exit_code = fields[1].trim().to_string();
    let max_rss_kb = parse_sacct_memory_kb(fields[2]);
    let elapsed = fields[3].trim().to_string();
    let reason = clean_sacct_optional(fields[4]);
    let req_mem = clean_sacct_optional(fields[5]);
    let category = sacct_failure_category(&state, &exit_code, reason.as_deref());
    let suggested_action = sacct_suggested_action(category, max_rss_kb, &elapsed);

    Some(SacctDiagnostic {
        state,
        exit_code,
        max_rss_kb,
        elapsed,
        reason,
        req_mem,
        category,
        suggested_action,
    })
}

fn clean_sacct_optional(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty()
        || value == "-"
        || value.eq_ignore_ascii_case("none")
        || value.eq_ignore_ascii_case("unknown")
    {
        None
    } else {
        Some(value.to_string())
    }
}

fn parse_sacct_memory_kb(value: &str) -> Option<u64> {
    let value = value.trim();
    if value.is_empty()
        || value == "-"
        || value.eq_ignore_ascii_case("none")
        || value.eq_ignore_ascii_case("unknown")
    {
        return None;
    }

    let split_at = value
        .char_indices()
        .find_map(|(idx, ch)| (!(ch.is_ascii_digit() || ch == '.')).then_some(idx))
        .unwrap_or(value.len());
    let number = value[..split_at].trim();
    if number.is_empty() {
        return None;
    }
    let numeric = number.parse::<f64>().ok()?;
    if !numeric.is_finite() || numeric < 0.0 {
        return None;
    }

    let suffix = value[split_at..].trim().to_ascii_uppercase();
    let multiplier = match suffix.chars().next().unwrap_or('K') {
        'B' => 1.0 / 1024.0,
        'K' => 1.0,
        'M' => 1024.0,
        'G' => 1024.0 * 1024.0,
        'T' => 1024.0 * 1024.0 * 1024.0,
        _ => 1.0,
    };
    let kb = (numeric * multiplier).ceil();
    (kb <= u64::MAX as f64).then_some(kb as u64)
}

fn sacct_failure_category(
    state: &str,
    exit_code: &str,
    reason: Option<&str>,
) -> SacctFailureCategory {
    let state = state.to_ascii_uppercase();
    let reason = reason.unwrap_or_default().to_ascii_uppercase();
    if state.contains("OUT_OF_MEMORY") || sacct_exit_signal(exit_code) == Some(9) {
        SacctFailureCategory::Oom
    } else if state.contains("TIMEOUT")
        || state.contains("TIME_LIMIT")
        || reason.contains("TIME_LIMIT")
        || reason.contains("TIMELIMIT")
    {
        SacctFailureCategory::Timeout
    } else if state.contains("CANCELLED") {
        SacctFailureCategory::Cancelled
    } else if exit_code.trim() != "0:0" {
        SacctFailureCategory::FailedExit
    } else {
        SacctFailureCategory::Other
    }
}

fn sacct_exit_signal(exit_code: &str) -> Option<i32> {
    exit_code
        .trim()
        .split_once(':')
        .and_then(|(_, signal)| signal.trim().parse::<i32>().ok())
}

fn sacct_returncode_from_diagnostic(diagnostic: &SacctDiagnostic) -> i32 {
    let (status, signal) = diagnostic
        .exit_code
        .trim()
        .split_once(':')
        .map(|(status, signal)| {
            (
                status.trim().parse::<i32>().unwrap_or(0),
                signal.trim().parse::<i32>().unwrap_or(0),
            )
        })
        .unwrap_or((0, 0));
    if status != 0 {
        status
    } else if signal != 0 {
        128 + signal
    } else if diagnostic.category != SacctFailureCategory::Other {
        1
    } else {
        0
    }
}

fn sacct_suggested_action(
    category: SacctFailureCategory,
    max_rss_kb: Option<u64>,
    elapsed: &str,
) -> Option<String> {
    match category {
        SacctFailureCategory::Oom => {
            let max_rss_kb = max_rss_kb?;
            let suggested_mb = ((max_rss_kb as u128) * 3).div_ceil(2048);
            Some(format!("Re-run with --mem={}MB", suggested_mb.max(1)))
        }
        SacctFailureCategory::Timeout => {
            let elapsed_secs = parse_sacct_elapsed_secs(elapsed)?;
            Some(format!(
                "Re-run with --time={}",
                format_slurm_duration(elapsed_secs.saturating_mul(2))
            ))
        }
        SacctFailureCategory::Cancelled
        | SacctFailureCategory::FailedExit
        | SacctFailureCategory::Other => None,
    }
}

fn parse_sacct_elapsed_secs(elapsed: &str) -> Option<u64> {
    let elapsed = elapsed.trim();
    if elapsed.is_empty() {
        return None;
    }
    let (days, time) = if let Some((days, time)) = elapsed.split_once('-') {
        (days.trim().parse::<u64>().ok()?, time)
    } else {
        (0, elapsed)
    };
    let parts = time
        .split(':')
        .map(|part| part.trim().parse::<u64>())
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    let seconds = match parts.as_slice() {
        [hours, minutes, seconds] => hours
            .saturating_mul(3600)
            .saturating_add(minutes.saturating_mul(60))
            .saturating_add(*seconds),
        [minutes, seconds] => minutes.saturating_mul(60).saturating_add(*seconds),
        [seconds] => *seconds,
        _ => return None,
    };
    Some(days.saturating_mul(86_400).saturating_add(seconds))
}

fn format_slurm_duration(seconds: u64) -> String {
    let days = seconds / 86_400;
    let remainder = seconds % 86_400;
    let hours = remainder / 3600;
    let minutes = (remainder % 3600) / 60;
    let seconds = remainder % 60;
    if days > 0 {
        format!("{days}-{hours:02}:{minutes:02}:{seconds:02}")
    } else {
        format!("{hours:02}:{minutes:02}:{seconds:02}")
    }
}

/// Submit the operator command via sbatch and poll squeue until completion.
/// Returns a synthetic ExecResult plus optional sacct diagnostics.
async fn execute_via_slurm(
    ctx: &crate::domain::tools::ToolContext,
    run_dir: &str,
    command: &str,
    walltime_secs: u64,
    cpus: u32,
    operator_id: &str,
    queue_status_sender: Option<OperatorQueueStatusSender>,
) -> Result<SlurmExecResult, OperatorToolError> {
    let safe_id = operator_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>();
    let walltime_hhmmss = {
        let h = walltime_secs / 3600;
        let m = (walltime_secs % 3600) / 60;
        let s = walltime_secs % 60;
        format!("{h:02}:{m:02}:{s:02}")
    };
    let script = format!(
        "#!/bin/bash\n#SBATCH --job-name=omiga_{safe_id}\n#SBATCH --output={run_dir}/logs/stdout.txt\n#SBATCH --error={run_dir}/logs/stderr.txt\n#SBATCH --cpus-per-task={cpus}\n#SBATCH --time={walltime_hhmmss}\n\nset +e\ncd {run_dir}\n{command}\necho $? > {run_dir}/logs/exit_code.txt\n",
        run_dir = sh_quote(run_dir),
    );
    // Write sbatch script to remote
    let script_path = format!("{run_dir}/omiga_slurm.sh");
    let write_cmd = format!(
        "cat > {} << 'OMIGA_SBATCH_EOF'\n{}\nOMIGA_SBATCH_EOF\nchmod +x {}",
        sh_quote(&script_path),
        script,
        sh_quote(&script_path)
    );
    execute_env_command(ctx, run_dir, &write_cmd, 30).await?;
    // Submit job
    let submit_cmd = format!("sbatch {}", sh_quote(&script_path));
    let submit_result = execute_env_command(ctx, run_dir, &submit_cmd, 30).await?;
    if submit_result.returncode != 0 {
        return Err(OperatorToolError::new(
            "slurm_submission_failed",
            false,
            format!(
                "sbatch failed with code {}: {}",
                submit_result.returncode,
                submit_result.output.trim()
            ),
        )
        .with_run_dir(run_dir)
        .with_suggested_action("Ensure SLURM is available and sbatch is on PATH."));
    }
    let job_id = submit_result
        .output
        .split('\n')
        .find_map(|line: &str| {
            line.trim()
                .strip_prefix("Submitted batch job ")
                .map(|s| s.trim().to_string())
        })
        .ok_or_else(|| {
            OperatorToolError::new(
                "slurm_job_id_missing",
                false,
                format!(
                    "Could not parse job ID from sbatch output: {}",
                    submit_result.output.trim()
                ),
            )
            .with_run_dir(run_dir)
        })?;
    // Write job ID for provenance
    let record_cmd = format!(
        "echo {} > {}/logs/slurm_job_id.txt",
        sh_quote(&job_id),
        sh_quote(run_dir)
    );
    let _ = execute_env_command(ctx, run_dir, &record_cmd, 10).await;
    // Poll squeue frequently enough for the async operator UI to show live SLURM state.
    let poll_interval = std::time::Duration::from_secs(5);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(walltime_secs + 120);
    loop {
        if std::time::Instant::now() >= deadline {
            // Cancel job on timeout
            let _ =
                execute_env_command(ctx, run_dir, &format!("scancel {}", sh_quote(&job_id)), 10)
                    .await;
            return Err(OperatorToolError::new(
                "slurm_timeout",
                false,
                "SLURM job exceeded walltime limit.",
            )
            .with_run_dir(run_dir));
        }
        let poll_cmd = format!(
            "squeue --noheader -j {} -o '%t' 2>/dev/null || true",
            sh_quote(&job_id)
        );
        let poll = execute_env_command(ctx, run_dir, &poll_cmd, 30).await?;
        let state = poll.output.trim().to_ascii_uppercase();
        if !state.is_empty() {
            if let Some(sender) = queue_status_sender.as_ref() {
                let _ = sender.send((job_id.clone(), state.clone()));
            }
        }
        let failed_state = state.split_whitespace().any(|part| {
            matches!(
                part,
                "F" | "FAILED" | "CA" | "CANCELLED" | "TO" | "TIMEOUT" | "NF" | "NODE_FAIL"
            )
        });
        if failed_state {
            let diagnostic = fetch_sacct_diagnostics(ctx, &job_id).await;
            let returncode = diagnostic
                .as_ref()
                .map(sacct_returncode_from_diagnostic)
                .filter(|code| *code != 0)
                .unwrap_or(1);
            return Ok(SlurmExecResult {
                exec_result: crate::execution::ExecResult {
                    returncode,
                    output: format!("SLURM job {job_id} ended with state: {state}"),
                },
                diagnostic,
            });
        }
        if state.is_empty() {
            // Job finished — read exit code
            let code_cmd = format!(
                "cat {}/logs/exit_code.txt 2>/dev/null || true",
                sh_quote(run_dir)
            );
            let code_result = execute_env_command(ctx, run_dir, &code_cmd, 10).await?;
            let exit_code_text = code_result.output.trim();
            let mut returncode = exit_code_text.parse::<i32>().unwrap_or(0);
            let diagnostic = if returncode != 0 || exit_code_text.is_empty() {
                fetch_sacct_diagnostics(ctx, &job_id).await
            } else {
                None
            };
            if returncode == 0 {
                if let Some(diagnostic) = diagnostic.as_ref() {
                    let sacct_returncode = sacct_returncode_from_diagnostic(diagnostic);
                    if sacct_returncode != 0 {
                        returncode = sacct_returncode;
                    }
                }
            }
            return Ok(SlurmExecResult {
                exec_result: crate::execution::ExecResult {
                    returncode,
                    output: format!("SLURM job {job_id} completed"),
                },
                diagnostic,
            });
        }
        tokio::time::sleep(poll_interval).await;
    }
}

pub(crate) fn operator_execution_command(
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

pub(crate) fn operator_container_for_command(
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

fn operator_environment_ref_error_command(
    ctx: &crate::domain::tools::ToolContext,
    spec: &OperatorSpec,
    surface_kind: OperatorExecutionSurfaceKind,
) -> Option<String> {
    if surface_kind == OperatorExecutionSurfaceKind::Sandbox {
        return None;
    }
    let env_ref = operator_runtime_env_ref(spec)?;
    let profile = operator_environment_profile(spec)?;
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

pub(crate) fn operator_environment_container_selection(
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
    let env_ref = operator_runtime_env_ref(spec)?;
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
pub(crate) struct OperatorCondaEnvironmentSelection {
    pub(crate) env_prefix: String,
    pub(crate) env_yaml_b64: String,
    pub(crate) env_hash: String,
    pub(crate) env_vars: BTreeMap<String, String>,
}

pub(crate) fn operator_container_selection_for_profile(
    ctx: &crate::domain::tools::ToolContext,
    profile: &crate::domain::environments::EnvironmentProfileSummary,
    surface_kind: OperatorExecutionSurfaceKind,
) -> Result<Option<OperatorContainerSelection>, String> {
    let kind = profile
        .runtime
        .kind
        .as_deref()
        .unwrap_or("system")
        .trim()
        .to_ascii_lowercase();
    let container_kind = container_kind_from_name(&kind).ok_or_else(|| {
        format!(
            "Environment profile `{}` runtime.type must be docker or singularity for container prewarm: `{kind}`",
            profile.canonical_id
        )
    })?;

    if let Some(image) = operator_environment_profile_image(profile) {
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
            let dockerfile = operator_dockerfile_from_environment_profile(profile)?;
            let context =
                operator_docker_build_context_from_environment_profile(profile, &dockerfile);
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
            let definition = operator_singularity_definition_from_environment_profile(profile)?;
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

pub(crate) fn operator_conda_environment_selection(
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

pub(crate) fn operator_conda_env_prefix(
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

pub(crate) const MICROMAMBA_BOOTSTRAP_SHELL: &str = r#"omiga_bootstrap_micromamba() {
  if [ "${OMIGA_DISABLE_MICROMAMBA_BOOTSTRAP:-}" = "1" ]; then
    printf 'micromamba bootstrap is disabled by OMIGA_DISABLE_MICROMAMBA_BOOTSTRAP=1\n' >&2
    return 1
  fi
  os=$(uname -s 2>/dev/null || true)
  arch=$(uname -m 2>/dev/null || true)
  case "${os}:${arch}" in
    Linux:x86_64) platform=linux-64 ;;
    Linux:arm64) platform=linux-aarch64 ;;
    Linux:aarch64) platform=linux-aarch64 ;;
    Darwin:x86_64) platform=osx-64 ;;
    Darwin:arm64) platform=osx-arm64 ;;
    *)
      printf 'unsupported platform for micromamba bootstrap: %s:%s\n' "$os" "$arch" >&2
      return 1
      ;;
  esac

  micromamba_url="${OMIGA_MICROMAMBA_URL:-https://github.com/mamba-org/micromamba-releases/releases/latest/download/micromamba-${platform}}"
  target_dir="$HOME/.omiga/bin"
  target_bin="$target_dir/micromamba"
  tmp_bin="$target_dir/.micromamba.tmp-$$"

  mkdir -p "$target_dir"
  rm -f "$tmp_bin"
  if command -v curl >/dev/null 2>&1; then
    if ! curl -fsSL --max-time 300 "$micromamba_url" -o "$tmp_bin" >/dev/null 2>&1; then
      printf 'micromamba bootstrap download failed with curl\n' >&2
      rm -f "$tmp_bin"
      return 1
    fi
  elif command -v wget >/dev/null 2>&1; then
    if ! wget -T 300 -t 2 -qO- "$micromamba_url" > "$tmp_bin" 2>/dev/null; then
      printf 'micromamba bootstrap download failed with wget\n' >&2
      rm -f "$tmp_bin"
      return 1
    fi
  else
    printf 'no supported downloader for micromamba bootstrap\n' >&2
    return 1
  fi
  if [ -n "$OMIGA_MICROMAMBA_SHA256" ]; then
    if command -v shasum >/dev/null 2>&1; then
      checksum="$(shasum -a 256 "$tmp_bin" | awk '{print $1}')"
    elif command -v sha256sum >/dev/null 2>&1; then
      checksum="$(sha256sum "$tmp_bin" | awk '{print $1}')"
    else
      printf 'micromamba bootstrap checksum unavailable: neither shasum nor sha256sum\n' >&2
      rm -f "$tmp_bin"
      return 1
    fi
    checksum_expected="$(printf '%s' "$OMIGA_MICROMAMBA_SHA256" | tr '[:upper:]' '[:lower:]')"
    checksum_actual="$(printf '%s' "$checksum" | tr '[:upper:]' '[:lower:]')"
    if [ "$checksum_actual" != "$checksum_expected" ]; then
      printf 'micromamba bootstrap checksum mismatch for downloaded binary\n' >&2
      rm -f "$tmp_bin"
      return 1
    fi
  fi

  if ! chmod +x "$tmp_bin" >/dev/null 2>&1; then
    printf 'micromamba bootstrap binary is not executable\n' >&2
    rm -f "$tmp_bin"
    return 1
  fi
  if ! "$tmp_bin" --version >/dev/null 2>&1; then
    printf 'micromamba bootstrap self-check failed\n' >&2
    rm -f "$tmp_bin"
    return 1
  fi
  if ! mv "$tmp_bin" "$target_bin"; then
    printf 'micromamba bootstrap installation failed\n' >&2
    rm -f "$tmp_bin"
    return 1
  fi
  OMIGA_CONDA_MANAGER_KIND=micromamba
  OMIGA_CONDA_BIN=$target_bin
  return 0
}
"#;

pub(crate) fn conda_environment_shell_script(
    selection: &OperatorCondaEnvironmentSelection,
    run_dir: &str,
    inner_command: &str,
) -> String {
    let env_yaml = format!("{run_dir}/env/conda-environment.yaml");
    let exports = crate::domain::env_hygiene::shell_export_lines(&selection.env_vars);
    format!(
        r#"{bootstrap}
set -e
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
Automatic micromamba installation failed (reason above).
Install the official micromamba binary at $HOME/.omiga/bin/micromamba, set OMIGA_MICROMAMBA=/absolute/path/to/micromamba, or set OMIGA_DISABLE_MICROMAMBA_BOOTSTRAP=1 to disable bootstrap.
Then rerun the Operator; Omiga will create and reuse the isolated env from conda.yaml/conda.yml under .omiga/operator-envs/conda.
OMIGA_CONDA_HINT
  exit 127
}}
omiga_find_conda_manager || true
if [ -z "$OMIGA_CONDA_BIN" ]; then
  omiga_bootstrap_micromamba || true
fi
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
        bootstrap = MICROMAMBA_BOOTSTRAP_SHELL,
    )
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

pub(crate) fn container_runtime_preflight_script(kind: OperatorContainerKind) -> &'static str {
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

pub(crate) fn container_runtime_prepare_script(prepare: &OperatorContainerImagePrepare) -> String {
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

pub(crate) fn collect_local_outputs(
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

pub(crate) fn read_local_structured_outputs(
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

pub(crate) fn validate_structured_outputs_against_manifest(
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

pub(crate) fn apply_status_metadata(
    value: &mut JsonValue,
    metadata: Option<&OperatorRunStatusMetadata>,
) {
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

pub(crate) fn operator_runs_root(project_root: &Path) -> PathBuf {
    project_root
        .join(OPERATOR_STATE_DIR_NAME)
        .join(RUNS_RELATIVE_PATH)
}

pub(crate) fn operator_run_dir(project_root: &Path, run_id: &str) -> PathBuf {
    operator_runs_root(project_root).join(run_id)
}

pub(crate) fn operator_run_relative_path(run_id: &str) -> String {
    format!("{OPERATOR_STATE_DIR_NAME}/{RUNS_RELATIVE_PATH}/{run_id}")
}

pub(crate) fn operator_runs_relative_path() -> String {
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
    let slurm_diagnostic = status_doc
        .as_ref()
        .and_then(|value| json_value_at(value, &["error", "slurmDiagnostic"]).cloned())
        .or_else(|| {
            provenance
                .as_ref()
                .and_then(|value| json_value_at(value, &["error", "slurmDiagnostic"]).cloned())
        })
        .and_then(|value| serde_json::from_value::<SacctDiagnostic>(value).ok());
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
            slurm_diagnostic,
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

pub(crate) fn operator_retry_policy(spec: &OperatorSpec) -> OperatorRetryPolicy {
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

pub(crate) fn should_retry_operator_error(
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

pub(crate) async fn execute_local(
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
        let provisioning_failure = provisioning_failure_for_error(
            Some(i64::from(result.returncode)),
            stderr_tail.as_deref(),
        );
        let error = OperatorToolError::new(
            "tool_exit_nonzero",
            false,
            format!("Operator process exited with code {}.", result.returncode),
        )
        .with_run_dir(run_dir)
        .with_logs(stdout_tail, stderr_tail)
        .with_suggested_action("Inspect stdout/stderr, then adjust inputs or params and retry.")
        .with_provisioning_failure(provisioning_failure);
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

pub(crate) async fn execute_in_environment(
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
    let slurm = operator_uses_slurm_scheduler(&resolved.spec, ctx);
    let cpu_count = effective_resources
        .get("cpu")
        .and_then(|v| v.as_u64())
        .unwrap_or(1) as u32;
    let (result, slurm_diagnostic) = if slurm {
        let slurm_result = execute_via_slurm(
            ctx,
            run_dir,
            &command,
            walltime_secs,
            cpu_count,
            &resolved.spec.metadata.id,
            current_operator_queue_status_sender(),
        )
        .await?;
        (slurm_result.exec_result, slurm_result.diagnostic)
    } else {
        (
            execute_env_command(ctx, run_dir, &command, walltime_secs).await?,
            None,
        )
    };
    let stdout_tail = remote_tail(ctx, run_dir, "logs/stdout.txt").await;
    let stderr_tail = remote_tail(ctx, run_dir, "logs/stderr.txt").await;
    if result.returncode != 0 {
        let provisioning_failure = provisioning_failure_for_error(
            Some(i64::from(result.returncode)),
            stderr_tail.as_deref(),
        );
        let mut error = OperatorToolError::new(
            "tool_exit_nonzero",
            false,
            format!("Operator process exited with code {}.", result.returncode),
        )
        .with_run_dir(run_dir)
        .with_logs(stdout_tail, stderr_tail)
        .with_provisioning_failure(provisioning_failure);
        if let Some(diagnostic) = slurm_diagnostic {
            if let Some(action) = diagnostic.suggested_action.clone() {
                error = error.with_suggested_action(action);
            } else {
                error = error.with_suggested_action(
                    "Inspect the remote run logs, then adjust inputs or params and retry.",
                );
            }
            error = error.with_slurm_diagnostic(diagnostic);
        } else {
            error = error.with_suggested_action(
                "Inspect the remote run logs, then adjust inputs or params and retry.",
            );
        }
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
