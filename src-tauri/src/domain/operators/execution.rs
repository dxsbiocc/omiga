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

pub(crate) fn runtime_axis_values(runtime: &JsonValue, axis: &str) -> HashSet<String> {
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

pub(crate) fn operator_environment_cwd(ctx: &crate::domain::tools::ToolContext) -> String {
    crate::domain::tools::env_store::remote_path(ctx, ".")
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

pub(crate) fn profile_runtime_extra_str<'a>(
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

pub(crate) fn operator_profile_relative_path(
    profile: &crate::domain::environments::EnvironmentProfileSummary,
    raw: &str,
) -> Result<PathBuf, String> {
    let path = PathBuf::from(raw);
    if path.is_absolute() {
        return Ok(path);
    }
    Ok(operator_environment_manifest_dir(profile)?.join(path))
}

pub(crate) fn operator_environment_manifest_dir(
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

pub(crate) fn operator_runtime_env_ref(spec: &OperatorSpec) -> Option<&str> {
    let runtime = spec.runtime.as_ref()?;
    runtime
        .get("envRef")
        .or_else(|| runtime.get("env_ref"))
        .and_then(JsonValue::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

pub(crate) fn safe_operator_env_component(value: &str) -> String {
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

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(bytes);
    format!("{digest:x}")
}

pub(crate) fn validate_output_glob_pattern<'a>(
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
