//! Tauri commands for Omiga operators.

use crate::app_state::OmigaAppState;
use crate::commands::CommandResult;
use crate::domain::operators::{
    self, OperatorCandidateSummary, OperatorManifestDiagnostic, OperatorRegistryUpdate,
    OperatorRunCleanupRequest, OperatorRunCleanupResult, OperatorRunContext, OperatorRunDetail,
    OperatorRunLog, OperatorRunSummary, OperatorRunVerification, OperatorSpec,
};
use crate::domain::tools::{env_store::EnvStore, ToolContext};
use crate::errors::AppError;
use futures::future::join_all;
use regex::Regex;
use serde::Serialize;
use serde_json::{json, Value as JsonValue};
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, State};
use tokio::sync::{mpsc, Mutex as TokioMutex};
use tokio_util::sync::CancellationToken;

static OPERATOR_TASK_MAP: OnceLock<TokioMutex<HashMap<String, CancellationToken>>> =
    OnceLock::new();
static OPERATOR_TASK_META: OnceLock<TokioMutex<HashMap<String, ActiveOperatorTaskInfo>>> =
    OnceLock::new();
static CHAIN_OUTPUT_REF_RE: OnceLock<Regex> = OnceLock::new();

fn operator_task_map() -> &'static TokioMutex<HashMap<String, CancellationToken>> {
    OPERATOR_TASK_MAP.get_or_init(|| TokioMutex::new(HashMap::new()))
}

fn operator_task_meta() -> &'static TokioMutex<HashMap<String, ActiveOperatorTaskInfo>> {
    OPERATOR_TASK_META.get_or_init(|| TokioMutex::new(HashMap::new()))
}

fn epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default()
}

fn operator_error(error: String) -> AppError {
    AppError::Config(error)
}

#[derive(Debug, Clone)]
struct ActiveOperatorTaskInfo {
    alias: String,
    started_at_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveOperatorTaskSummary {
    pub task_id: String,
    pub alias: String,
    pub started_at_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperatorCatalogResponse {
    pub registry_path: String,
    pub operators: Vec<OperatorCandidateSummary>,
    pub diagnostics: Vec<OperatorManifestDiagnostic>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperatorDescribeResponse {
    pub alias: Option<String>,
    pub exposed: bool,
    pub tool_name: Option<String>,
    pub spec: OperatorSpec,
    pub schema: serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperatorRunResponse {
    pub ok: bool,
    pub result: JsonValue,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChainStepResult {
    pub alias: String,
    pub ok: bool,
    pub run_dir: Option<String>,
    pub result: JsonValue,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperatorChainResult {
    pub steps: Vec<ChainStepResult>,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

fn resolve_project_root(project_root: Option<String>) -> PathBuf {
    let raw = project_root.unwrap_or_default();
    let trimmed = raw.trim();
    let path = if trimmed.is_empty() || trimmed == "." {
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
    } else {
        PathBuf::from(trimmed)
    };
    path.canonicalize().unwrap_or(path)
}

async fn env_store_for_session(state: &OmigaAppState, session_id: Option<&str>) -> EnvStore {
    let Some(session_id) = session_id.filter(|value| !value.trim().is_empty()) else {
        return EnvStore::new();
    };
    let sessions = state.chat.sessions.read().await;
    sessions
        .get(session_id)
        .map(|session| session.env_store.clone())
        .unwrap_or_else(EnvStore::new)
}

async fn build_operator_context(
    state: &OmigaAppState,
    project_root: Option<String>,
    session_id: Option<String>,
    execution_environment: Option<String>,
    ssh_server: Option<String>,
    sandbox_backend: Option<String>,
    timeout_secs: u64,
) -> ToolContext {
    let env = execution_environment
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("local")
        .to_string();
    let session_id_ref = session_id.as_deref();
    let env_store = env_store_for_session(state, session_id_ref).await;
    ToolContext::new(resolve_project_root(project_root))
        .with_session_id(session_id)
        .with_execution_environment(env)
        .with_ssh_server(ssh_server.filter(|value| !value.trim().is_empty()))
        .with_sandbox_backend(
            sandbox_backend
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| "docker".to_string()),
        )
        .with_env_store(Some(env_store))
        .with_timeout(timeout_secs)
}

/// Create or replace a user-defined script operator in `~/.omiga/user-operators/`.
#[tauri::command]
pub async fn save_user_script_operator(
    id: String,
    name: String,
    description: String,
    argv: Vec<String>,
    inputs: Option<Vec<operators::UserOperatorInput>>,
    params: Option<Vec<operators::UserOperatorParam>>,
    outputs: Option<Vec<operators::UserOperatorOutput>>,
) -> CommandResult<String> {
    let path = operators::save_user_script_operator(
        &id,
        &name,
        &description,
        &argv,
        inputs.as_deref().unwrap_or(&[]),
        params.as_deref().unwrap_or(&[]),
        outputs.as_deref().unwrap_or(&[]),
    )
    .map_err(crate::errors::AppError::Config)?;
    Ok(path.to_string_lossy().into_owned())
}

/// Return the path to `~/.omiga/user-operators/` so the frontend can show it.
#[tauri::command]
pub async fn get_user_operators_dir() -> CommandResult<String> {
    Ok(operators::user_operators_dir()
        .to_string_lossy()
        .into_owned())
}

#[tauri::command]
pub async fn list_chain_templates() -> CommandResult<Vec<operators::ChainTemplate>> {
    Ok(operators::list_user_chain_templates())
}

#[tauri::command]
pub async fn save_chain_template(
    id: String,
    name: String,
    description: Option<String>,
    steps: Vec<operators::ChainStep>,
) -> CommandResult<String> {
    let path = operators::save_user_chain_template(&id, &name, description.as_deref(), &steps)
        .map_err(crate::errors::AppError::Config)?;
    Ok(path.to_string_lossy().into_owned())
}

#[tauri::command]
pub async fn delete_chain_template(id: String) -> CommandResult<()> {
    operators::delete_user_chain_template(&id).map_err(crate::errors::AppError::Config)
}

#[tauri::command]
pub async fn list_operators() -> CommandResult<OperatorCatalogResponse> {
    Ok(OperatorCatalogResponse {
        registry_path: operators::registry_path().to_string_lossy().into_owned(),
        operators: operators::list_operator_summaries(),
        diagnostics: operators::list_operator_manifest_diagnostics(),
    })
}

#[tauri::command]
pub async fn list_operator_favorites() -> Vec<String> {
    operators::operator_favorites::list_favorites()
}

#[tauri::command]
pub async fn toggle_operator_favorite(alias: String, pinned: bool) -> CommandResult<Vec<String>> {
    operators::operator_favorites::set_favorite(&alias, pinned).map_err(operator_error)
}

#[tauri::command]
pub async fn describe_operator(id: String) -> CommandResult<OperatorDescribeResponse> {
    let (alias, spec) = operators::describe_operator(&id).map_err(|error| {
        AppError::Config(
            serde_json::to_string_pretty(&error).unwrap_or_else(|_| error.message.clone()),
        )
    })?;
    let tool_name = alias
        .as_ref()
        .map(|alias| format!("{}{}", operators::OPERATOR_TOOL_PREFIX, alias));
    Ok(OperatorDescribeResponse {
        exposed: alias.is_some(),
        schema: operators::operator_parameters_schema(&spec),
        alias,
        tool_name,
        spec,
    })
}

#[tauri::command]
pub async fn set_operator_enabled(update: OperatorRegistryUpdate) -> CommandResult<()> {
    operators::set_operator_enabled(update).map_err(operator_error)
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn run_operator(
    state: State<'_, OmigaAppState>,
    alias: String,
    arguments: JsonValue,
    project_root: Option<String>,
    session_id: Option<String>,
    execution_environment: Option<String>,
    ssh_server: Option<String>,
    sandbox_backend: Option<String>,
    run_kind: Option<String>,
    smoke_test_id: Option<String>,
    smoke_test_name: Option<String>,
    bypass_cache: Option<bool>,
) -> CommandResult<OperatorRunResponse> {
    let alias = alias.trim();
    if alias.is_empty() {
        return Err(AppError::Config(
            "operator alias must not be empty".to_string(),
        ));
    }
    let tool_name = if alias.starts_with(operators::OPERATOR_TOOL_PREFIX) {
        alias.to_string()
    } else {
        format!("{}{}", operators::OPERATOR_TOOL_PREFIX, alias)
    };
    let arguments = if arguments.is_null() {
        json!({})
    } else {
        arguments
    };
    let arguments = serde_json::to_string(&arguments)
        .map_err(|err| AppError::Config(format!("serialize operator arguments: {err}")))?;
    let ctx = build_operator_context(
        &state,
        project_root,
        session_id,
        execution_environment,
        ssh_server,
        sandbox_backend,
        120,
    )
    .await;
    let run_context = OperatorRunContext {
        kind: run_kind,
        smoke_test_id,
        smoke_test_name,
        parent_execution_id: None,
        bypass_cache: bypass_cache.unwrap_or(false),
    };
    let (raw, is_error) = operators::execute_operator_tool_call_with_context(
        &ctx,
        &tool_name,
        &arguments,
        Some(run_context),
    )
    .await;
    let result = serde_json::from_str::<JsonValue>(&raw).unwrap_or_else(|_| json!({ "raw": raw }));
    Ok(OperatorRunResponse {
        ok: !is_error,
        result,
    })
}

/// Payload emitted on the `operator-task-{task_id}` Tauri event channel.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum OperatorTaskEvent {
    Started {
        task_id: String,
        alias: String,
    },
    QueueStatus {
        task_id: String,
        scheduler: String,
        job_id: Option<String>,
        state: String,
    },
    Completed {
        task_id: String,
        ok: bool,
        result: JsonValue,
    },
    Failed {
        task_id: String,
        error: String,
    },
    Cancelled {
        task_id: String,
    },
}

/// Async variant of `run_operator`.  Returns immediately with `{ taskId }` and
/// emits `operator-task-{taskId}` events as the run progresses.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn run_operator_async(
    app: AppHandle,
    state: State<'_, OmigaAppState>,
    alias: String,
    arguments: JsonValue,
    project_root: Option<String>,
    session_id: Option<String>,
    execution_environment: Option<String>,
    ssh_server: Option<String>,
    sandbox_backend: Option<String>,
    run_kind: Option<String>,
    smoke_test_id: Option<String>,
    smoke_test_name: Option<String>,
    bypass_cache: Option<bool>,
) -> CommandResult<serde_json::Value> {
    let alias = alias.trim().to_string();
    if alias.is_empty() {
        return Err(AppError::Config(
            "operator alias must not be empty".to_string(),
        ));
    }
    let task_id = uuid::Uuid::new_v4().simple().to_string();
    let arguments = if arguments.is_null() {
        json!({})
    } else {
        arguments
    };
    let arguments_str = serde_json::to_string(&arguments)
        .map_err(|err| AppError::Config(format!("serialize operator arguments: {err}")))?;

    let ctx = build_operator_context(
        &state,
        project_root,
        session_id,
        execution_environment,
        ssh_server,
        sandbox_backend,
        120,
    )
    .await;

    let run_context = OperatorRunContext {
        kind: run_kind,
        smoke_test_id,
        smoke_test_name,
        parent_execution_id: None,
        bypass_cache: bypass_cache.unwrap_or(false),
    };

    let cancel_token = CancellationToken::new();
    let started_at_ms = epoch_ms();
    operator_task_map()
        .lock()
        .await
        .insert(task_id.clone(), cancel_token.clone());
    operator_task_meta().lock().await.insert(
        task_id.clone(),
        ActiveOperatorTaskInfo {
            alias: alias.clone(),
            started_at_ms,
        },
    );

    let task_id_clone = task_id.clone();
    let alias_clone = alias.clone();
    let tool_name = if alias.starts_with(operators::OPERATOR_TOOL_PREFIX) {
        alias.clone()
    } else {
        format!("{}{}", operators::OPERATOR_TOOL_PREFIX, alias)
    };

    tokio::spawn(async move {
        let event = format!("operator-task-{}", task_id_clone);
        let (queue_status_tx, mut queue_status_rx) = mpsc::unbounded_channel::<(String, String)>();
        let queue_status_event = event.clone();
        let queue_status_task_id = task_id_clone.clone();
        let queue_status_app = app.clone();
        let queue_status_forwarder = tokio::spawn(async move {
            while let Some((job_id, state)) = queue_status_rx.recv().await {
                let _ = queue_status_app.emit(
                    &queue_status_event,
                    &OperatorTaskEvent::QueueStatus {
                        task_id: queue_status_task_id.clone(),
                        scheduler: "slurm".to_string(),
                        job_id: Some(job_id),
                        state,
                    },
                );
            }
        });

        let _ = app.emit(
            &event,
            &OperatorTaskEvent::Started {
                task_id: task_id_clone.clone(),
                alias: alias_clone,
            },
        );

        let operator_result = tokio::select! {
            _ = cancel_token.cancelled() => None,
            result = operators::with_operator_queue_status_sender(
                queue_status_tx,
                operators::execute_operator_tool_call_with_context(
                    &ctx,
                    &tool_name,
                    &arguments_str,
                    Some(run_context),
                ),
            ) => Some(result),
        };

        let _ = queue_status_forwarder.await;

        match operator_result {
            None => {
                let _ = app.emit(
                    &event,
                    &OperatorTaskEvent::Cancelled {
                        task_id: task_id_clone.clone(),
                    },
                );
            }
            Some(result) => {
                let (raw, is_error) = result;
                let parsed = serde_json::from_str::<JsonValue>(&raw)
                    .unwrap_or_else(|_| json!({ "raw": raw }));
                let _ = app.emit(
                    &event,
                    &OperatorTaskEvent::Completed {
                        task_id: task_id_clone.clone(),
                        ok: !is_error,
                        result: parsed,
                    },
                );
            }
        }

        operator_task_map().lock().await.remove(&task_id_clone);
        operator_task_meta().lock().await.remove(&task_id_clone);
    });

    Ok(json!({ "taskId": task_id }))
}

fn operator_run_dir(result: &JsonValue) -> Option<String> {
    result
        .get("runDir")
        .and_then(JsonValue::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn operator_result_error_message(result: &JsonValue) -> Option<String> {
    result
        .get("error")
        .and_then(JsonValue::as_object)
        .and_then(|error| error.get("message"))
        .and_then(JsonValue::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

#[derive(Debug, Clone)]
struct PreparedChainStep {
    index: usize,
    alias: String,
    label: String,
    arguments: JsonValue,
    inherit_prev_output_as: Option<String>,
    depends_on: Vec<String>,
}

type ChainStepFuture = Pin<Box<dyn Future<Output = ChainStepResult> + Send>>;
type ChainStepRunner = Arc<dyn Fn(PreparedChainStep, JsonValue) -> ChainStepFuture + Send + Sync>;

fn default_chain_step_label(index: usize) -> String {
    format!("step_{}", index + 1)
}

fn normalized_chain_step_label(step: &operators::ChainStep, index: usize) -> String {
    step.label
        .as_deref()
        .map(str::trim)
        .filter(|label| !label.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| default_chain_step_label(index))
}

fn prepare_chain_steps(steps: Vec<operators::ChainStep>) -> Result<Vec<PreparedChainStep>, String> {
    let labels = steps
        .iter()
        .enumerate()
        .map(|(index, step)| normalized_chain_step_label(step, index))
        .collect::<Vec<_>>();
    let mut label_indices = HashMap::with_capacity(labels.len());
    for (index, label) in labels.iter().enumerate() {
        if let Some(previous_index) = label_indices.insert(label.clone(), index) {
            return Err(format!(
                "operator chain has duplicate step label `{label}` at steps {} and {}",
                previous_index + 1,
                index + 1
            ));
        }
    }

    let mut prepared = Vec::with_capacity(steps.len());
    for (index, step) in steps.into_iter().enumerate() {
        let alias = step.alias.trim().to_string();
        if alias.is_empty() {
            return Err("operator chain step alias must not be empty".to_string());
        }

        let depends_on = if step.depends_on_declared() {
            step.depends_on
                .iter()
                .map(|dependency| dependency.trim().to_string())
                .collect::<Vec<_>>()
        } else if index > 0 {
            vec![labels[index - 1].clone()]
        } else {
            Vec::new()
        };

        let mut seen_dependencies = HashSet::with_capacity(depends_on.len());
        let mut normalized_dependencies = Vec::with_capacity(depends_on.len());
        for dependency in depends_on {
            if dependency.is_empty() {
                return Err(format!(
                    "operator chain step `{}` has an empty dependsOn label",
                    labels[index]
                ));
            }
            if !label_indices.contains_key(&dependency) {
                return Err(format!(
                    "operator chain step `{}` depends on unknown label `{dependency}`",
                    labels[index]
                ));
            }
            if seen_dependencies.insert(dependency.clone()) {
                normalized_dependencies.push(dependency);
            }
        }

        prepared.push(PreparedChainStep {
            index,
            alias,
            label: labels[index].clone(),
            arguments: step.arguments,
            inherit_prev_output_as: step.inherit_prev_output_as,
            depends_on: normalized_dependencies,
        });
    }

    validate_chain_is_acyclic(&prepared)?;
    Ok(prepared)
}

fn validate_chain_is_acyclic(steps: &[PreparedChainStep]) -> Result<(), String> {
    let label_to_index = steps
        .iter()
        .map(|step| (step.label.as_str(), step.index))
        .collect::<HashMap<_, _>>();
    let mut in_degree = steps
        .iter()
        .map(|step| step.depends_on.len())
        .collect::<Vec<_>>();
    let mut dependents = vec![Vec::<usize>::new(); steps.len()];
    for step in steps {
        for dependency in &step.depends_on {
            let Some(&dependency_index) = label_to_index.get(dependency.as_str()) else {
                return Err(format!(
                    "operator chain step `{}` depends on unknown label `{dependency}`",
                    step.label
                ));
            };
            dependents[dependency_index].push(step.index);
        }
    }

    let mut ready = in_degree
        .iter()
        .enumerate()
        .filter_map(|(index, degree)| (*degree == 0).then_some(index))
        .collect::<Vec<_>>();
    let mut visited = 0;
    while let Some(index) = ready.pop() {
        visited += 1;
        for dependent in &dependents[index] {
            in_degree[*dependent] -= 1;
            if in_degree[*dependent] == 0 {
                ready.push(*dependent);
            }
        }
    }

    if visited == steps.len() {
        Ok(())
    } else {
        Err("operator chain contains a dependency cycle".to_string())
    }
}

fn chain_output_ref_regex() -> &'static Regex {
    CHAIN_OUTPUT_REF_RE.get_or_init(|| {
        Regex::new(r"\{\{\s*([^{}]+?)\.outputDir\s*\}\}")
            .expect("valid operator chain output reference regex")
    })
}

fn inject_previous_run_dir(
    arguments: &mut JsonValue,
    field: &str,
    run_dir: &str,
) -> Result<(), String> {
    let field = field.trim();
    if field.is_empty() {
        return Err("previous output input field must not be empty".to_string());
    }
    if arguments.is_null() {
        *arguments = json!({});
    }
    let object = arguments.as_object_mut().ok_or_else(|| {
        "operator arguments must be a JSON object to inherit chain input".to_string()
    })?;
    let inputs = object.entry("inputs").or_insert_with(|| json!({}));
    if inputs.is_null() {
        *inputs = json!({});
    }
    let input_object = inputs.as_object_mut().ok_or_else(|| {
        "operator arguments.inputs must be a JSON object to inherit chain input".to_string()
    })?;
    input_object.insert(field.to_string(), JsonValue::String(run_dir.to_string()));
    Ok(())
}

fn substitute_output_references(
    value: &mut JsonValue,
    output_refs: &HashMap<String, String>,
) -> Result<(), String> {
    match value {
        JsonValue::String(raw) => {
            let mut next = String::with_capacity(raw.len());
            let mut last_end = 0;
            for captures in chain_output_ref_regex().captures_iter(raw) {
                let Some(reference_match) = captures.get(0) else {
                    continue;
                };
                let label = captures
                    .get(1)
                    .map(|match_| match_.as_str().trim())
                    .unwrap_or_default();
                let Some(run_dir) = output_refs.get(label) else {
                    return Err(format!(
                        "operator chain output reference `{{{{{label}.outputDir}}}}` is not available; add `{label}` to dependsOn or reference an earlier completed step"
                    ));
                };
                next.push_str(&raw[last_end..reference_match.start()]);
                next.push_str(run_dir);
                last_end = reference_match.end();
            }
            if last_end > 0 {
                next.push_str(&raw[last_end..]);
                *raw = next;
            }
        }
        JsonValue::Array(items) => {
            for item in items {
                substitute_output_references(item, output_refs)?;
            }
        }
        JsonValue::Object(object) => {
            for item in object.values_mut() {
                substitute_output_references(item, output_refs)?;
            }
        }
        JsonValue::Null | JsonValue::Bool(_) | JsonValue::Number(_) => {}
    }
    Ok(())
}

fn output_ref_map(
    steps: &[PreparedChainStep],
    completed_outputs: &HashMap<String, String>,
) -> HashMap<String, String> {
    let mut refs = HashMap::new();
    for step in steps {
        let Some(run_dir) = completed_outputs.get(&step.label) else {
            continue;
        };
        refs.insert(step.label.clone(), run_dir.clone());
    }
    for step in steps {
        let Some(run_dir) = completed_outputs.get(&step.label) else {
            continue;
        };
        refs.insert(format!("step{}", step.index + 1), run_dir.clone());
    }
    refs
}

fn inherited_output_source(step: &PreparedChainStep) -> Result<String, String> {
    if step.depends_on.len() == 1 {
        return Ok(step.depends_on[0].clone());
    }
    if step.depends_on.is_empty() {
        return Err(format!(
            "operator chain step `{}` cannot inherit a previous output without a dependency",
            step.label
        ));
    }
    Err(format!(
        "operator chain step `{}` cannot inherit a previous output from multiple dependencies; use label-based output placeholders instead",
        step.label
    ))
}

fn prepare_step_arguments(
    step: &PreparedChainStep,
    steps: &[PreparedChainStep],
    completed_outputs: &HashMap<String, String>,
) -> Result<JsonValue, String> {
    let mut arguments = if step.arguments.is_null() {
        json!({})
    } else {
        step.arguments.clone()
    };

    if let Some(field) = step
        .inherit_prev_output_as
        .as_deref()
        .map(str::trim)
        .filter(|field| !field.is_empty())
    {
        let source = inherited_output_source(step)?;
        let Some(run_dir) = completed_outputs.get(&source) else {
            return Err(format!(
                "operator chain dependency `{source}` did not return runDir"
            ));
        };
        inject_previous_run_dir(&mut arguments, field, run_dir)?;
    }

    substitute_output_references(&mut arguments, &output_ref_map(steps, completed_outputs))?;
    Ok(arguments)
}

fn chain_step_failure(alias: String, error: String) -> ChainStepResult {
    ChainStepResult {
        alias: alias.clone(),
        ok: false,
        run_dir: None,
        result: json!({
            "status": "failed",
            "operator": {
                "alias": alias,
            },
            "error": {
                "kind": "chain_input_injection_failed",
                "retryable": false,
                "message": error,
            },
        }),
        error: Some(error),
    }
}

fn operator_tool_name(alias: &str) -> String {
    if alias.starts_with(operators::OPERATOR_TOOL_PREFIX) {
        alias.to_string()
    } else {
        format!("{}{}", operators::OPERATOR_TOOL_PREFIX, alias)
    }
}

async fn execute_prepared_operator_chain_step(
    ctx: ToolContext,
    step: PreparedChainStep,
    arguments: JsonValue,
) -> ChainStepResult {
    let arguments = match serde_json::to_string(&arguments) {
        Ok(arguments) => arguments,
        Err(err) => {
            return chain_step_failure(step.alias, format!("serialize operator arguments: {err}"));
        }
    };
    let run_context = OperatorRunContext {
        kind: Some("chain".to_string()),
        smoke_test_id: None,
        smoke_test_name: None,
        parent_execution_id: None,
        bypass_cache: false,
    };
    let (raw, is_error) = operators::execute_operator_tool_call_with_context(
        &ctx,
        &operator_tool_name(&step.alias),
        &arguments,
        Some(run_context),
    )
    .await;
    let result = serde_json::from_str::<JsonValue>(&raw).unwrap_or_else(|_| json!({ "raw": raw }));
    let run_dir = operator_run_dir(&result);
    let error = is_error.then(|| {
        operator_result_error_message(&result)
            .unwrap_or_else(|| "operator chain step failed".to_string())
    });
    ChainStepResult {
        alias: step.alias,
        ok: !is_error,
        run_dir,
        result,
        error,
    }
}

async fn run_prepared_operator_chain(
    steps: Vec<PreparedChainStep>,
    runner: ChainStepRunner,
) -> OperatorChainResult {
    let label_to_index = steps
        .iter()
        .map(|step| (step.label.clone(), step.index))
        .collect::<HashMap<_, _>>();
    let mut in_degree = steps
        .iter()
        .map(|step| step.depends_on.len())
        .collect::<Vec<_>>();
    let mut dependents = vec![Vec::<usize>::new(); steps.len()];
    for step in &steps {
        for dependency in &step.depends_on {
            if let Some(&dependency_index) = label_to_index.get(dependency) {
                dependents[dependency_index].push(step.index);
            }
        }
    }

    let mut ready = in_degree
        .iter()
        .enumerate()
        .filter_map(|(index, degree)| (*degree == 0).then_some(index))
        .collect::<Vec<_>>();
    let mut completed_labels = HashSet::with_capacity(steps.len());
    let mut completed_outputs = HashMap::<String, String>::new();
    let mut results = Vec::with_capacity(steps.len());

    while completed_labels.len() < steps.len() {
        if ready.is_empty() {
            return OperatorChainResult {
                steps: results,
                ok: false,
                error: Some("operator chain contains a dependency cycle".to_string()),
            };
        }

        ready.sort_unstable();
        let round = std::mem::take(&mut ready);
        let mut round_jobs = Vec::with_capacity(round.len());
        for index in round {
            let step = steps[index].clone();
            match prepare_step_arguments(&step, &steps, &completed_outputs) {
                Ok(arguments) => round_jobs.push((step, arguments)),
                Err(error) => {
                    let failure = chain_step_failure(step.alias, error.clone());
                    results.push(failure);
                    return OperatorChainResult {
                        steps: results,
                        ok: false,
                        error: Some(error),
                    };
                }
            }
        }

        let handles = round_jobs
            .into_iter()
            .map(|(step, arguments)| {
                let runner = Arc::clone(&runner);
                let task_step = step.clone();
                async move {
                    (
                        step,
                        tokio::spawn(async move { runner(task_step, arguments).await }).await,
                    )
                }
            })
            .collect::<Vec<_>>();

        let mut round_failed = false;
        let mut round_error = None;
        let mut round_completed_labels = Vec::new();
        for (step, join_result) in join_all(handles).await {
            let result = match join_result {
                Ok(result) => result,
                Err(error) => chain_step_failure(
                    step.alias.clone(),
                    format!("operator chain step task failed: {error}"),
                ),
            };
            if result.ok {
                completed_labels.insert(step.label.clone());
                round_completed_labels.push(step.label.clone());
                if let Some(run_dir) = result.run_dir.clone() {
                    completed_outputs.insert(step.label.clone(), run_dir);
                }
            } else {
                round_failed = true;
                if round_error.is_none() {
                    round_error = result.error.clone();
                }
            }
            results.push(result);
        }

        if round_failed {
            return OperatorChainResult {
                steps: results,
                ok: false,
                error: round_error.or_else(|| Some("operator chain step failed".to_string())),
            };
        }

        for label in round_completed_labels {
            let Some(&index) = label_to_index.get(&label) else {
                continue;
            };
            for dependent in &dependents[index] {
                if in_degree[*dependent] == 0 {
                    continue;
                }
                in_degree[*dependent] -= 1;
                if in_degree[*dependent] == 0 {
                    ready.push(*dependent);
                }
            }
        }
    }

    OperatorChainResult {
        steps: results,
        ok: true,
        error: None,
    }
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn run_operator_chain(
    state: State<'_, OmigaAppState>,
    steps: Vec<operators::ChainStep>,
    project_root: Option<String>,
    session_id: Option<String>,
    execution_environment: Option<String>,
    ssh_server: Option<String>,
    sandbox_backend: Option<String>,
) -> CommandResult<OperatorChainResult> {
    if steps.len() < 2 {
        return Err(AppError::Config(
            "operator chain requires at least two steps".to_string(),
        ));
    }

    let steps = match prepare_chain_steps(steps) {
        Ok(steps) => steps,
        Err(error) => {
            return Ok(OperatorChainResult {
                steps: Vec::new(),
                ok: false,
                error: Some(error),
            });
        }
    };

    let ctx = build_operator_context(
        &state,
        project_root,
        session_id,
        execution_environment,
        ssh_server,
        sandbox_backend,
        120,
    )
    .await;

    let runner: ChainStepRunner = Arc::new(move |step, arguments| {
        let ctx = ctx.clone();
        Box::pin(async move { execute_prepared_operator_chain_step(ctx, step, arguments).await })
    });

    Ok(run_prepared_operator_chain(steps, runner).await)
}

/// Cancel an in-progress async operator task by its `task_id`.
#[tauri::command]
pub async fn cancel_operator_task(task_id: String) -> CommandResult<()> {
    if let Some(token) = operator_task_map().lock().await.get(&task_id) {
        token.cancel();
    }
    Ok(())
}

#[tauri::command]
pub async fn list_active_operator_tasks() -> CommandResult<Vec<ActiveOperatorTaskSummary>> {
    let mut tasks = operator_task_meta()
        .lock()
        .await
        .iter()
        .map(|(task_id, info)| ActiveOperatorTaskSummary {
            task_id: task_id.clone(),
            alias: info.alias.clone(),
            started_at_ms: info.started_at_ms,
        })
        .collect::<Vec<_>>();
    tasks.sort_by_key(|task| task.started_at_ms);
    Ok(tasks)
}

#[tauri::command]
pub async fn list_operator_runs(
    state: State<'_, OmigaAppState>,
    project_root: Option<String>,
    session_id: Option<String>,
    execution_environment: Option<String>,
    ssh_server: Option<String>,
    sandbox_backend: Option<String>,
    status_filter: Option<String>,
    after_ms: Option<u64>,
) -> CommandResult<Vec<OperatorRunSummary>> {
    let ctx = build_operator_context(
        &state,
        project_root,
        session_id,
        execution_environment,
        ssh_server,
        sandbox_backend,
        30,
    )
    .await;
    let mut runs = operators::list_operator_runs_for_context(&ctx, 100)
        .await
        .map_err(operator_error)?;
    if let Some(filter) = status_filter
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        runs.retain(|run| run.status.trim().eq_ignore_ascii_case(filter));
    }
    if let Some(after_ms) = after_ms {
        runs.retain(|run| {
            run.updated_at
                .as_deref()
                .and_then(|updated_at| chrono::DateTime::parse_from_rfc3339(updated_at).ok())
                .map(|updated_at| updated_at.timestamp_millis() >= after_ms as i64)
                .unwrap_or(true)
        });
    }
    Ok(runs)
}

#[tauri::command]
pub async fn read_operator_run(
    state: State<'_, OmigaAppState>,
    project_root: Option<String>,
    run_id: String,
    session_id: Option<String>,
    execution_environment: Option<String>,
    ssh_server: Option<String>,
    sandbox_backend: Option<String>,
) -> CommandResult<OperatorRunDetail> {
    let ctx = build_operator_context(
        &state,
        project_root,
        session_id,
        execution_environment,
        ssh_server,
        sandbox_backend,
        30,
    )
    .await;
    operators::read_operator_run_for_context(&ctx, &run_id)
        .await
        .map_err(operator_error)
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn read_operator_run_log(
    state: State<'_, OmigaAppState>,
    project_root: Option<String>,
    run_id: String,
    log_name: String,
    limit_bytes: Option<u64>,
    session_id: Option<String>,
    execution_environment: Option<String>,
    ssh_server: Option<String>,
    sandbox_backend: Option<String>,
) -> CommandResult<OperatorRunLog> {
    let ctx = build_operator_context(
        &state,
        project_root,
        session_id,
        execution_environment,
        ssh_server,
        sandbox_backend,
        30,
    )
    .await;
    operators::read_operator_run_log_for_context(
        &ctx,
        &run_id,
        &log_name,
        limit_bytes.unwrap_or(16 * 1024),
    )
    .await
    .map_err(operator_error)
}

#[tauri::command]
pub async fn verify_operator_run(
    state: State<'_, OmigaAppState>,
    project_root: Option<String>,
    run_id: String,
    session_id: Option<String>,
    execution_environment: Option<String>,
    ssh_server: Option<String>,
    sandbox_backend: Option<String>,
) -> CommandResult<OperatorRunVerification> {
    let ctx = build_operator_context(
        &state,
        project_root,
        session_id,
        execution_environment,
        ssh_server,
        sandbox_backend,
        30,
    )
    .await;
    operators::verify_operator_run_for_context(&ctx, &run_id)
        .await
        .map_err(operator_error)
}

#[tauri::command]
pub async fn cleanup_operator_runs(
    state: State<'_, OmigaAppState>,
    project_root: Option<String>,
    request: OperatorRunCleanupRequest,
    session_id: Option<String>,
    execution_environment: Option<String>,
    ssh_server: Option<String>,
    sandbox_backend: Option<String>,
) -> CommandResult<OperatorRunCleanupResult> {
    let ctx = build_operator_context(
        &state,
        project_root,
        session_id,
        execution_environment,
        ssh_server,
        sandbox_backend,
        30,
    )
    .await;
    operators::cleanup_operator_runs_for_context(&ctx, request)
        .await
        .map_err(operator_error)
}

#[cfg(test)]
mod tests {
    use super::{
        chain_step_failure, inject_previous_run_dir, prepare_chain_steps, resolve_project_root,
        run_prepared_operator_chain, ChainStepResult, ChainStepRunner, PreparedChainStep,
    };
    use crate::domain::operators;
    use serde_json::json;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    use std::time::Duration;
    use tokio::sync::Mutex;

    fn chain_steps(value: serde_json::Value) -> Vec<operators::ChainStep> {
        serde_json::from_value(value).expect("valid chain steps")
    }

    fn successful_step(alias: String, run_dir: String) -> ChainStepResult {
        ChainStepResult {
            alias,
            ok: true,
            run_dir: Some(run_dir.clone()),
            result: json!({ "runDir": run_dir }),
            error: None,
        }
    }

    fn record_max(max_active: &AtomicUsize, active: usize) {
        let mut current = max_active.load(Ordering::SeqCst);
        while active > current {
            match max_active.compare_exchange(current, active, Ordering::SeqCst, Ordering::SeqCst) {
                Ok(_) => break,
                Err(next) => current = next,
            }
        }
    }

    fn recording_runner(records: Arc<Mutex<Vec<(String, serde_json::Value)>>>) -> ChainStepRunner {
        Arc::new(move |step: PreparedChainStep, arguments| {
            let records = Arc::clone(&records);
            Box::pin(async move {
                records.lock().await.push((step.label.clone(), arguments));
                successful_step(step.alias, format!("/tmp/{}", step.label))
            })
        })
    }

    #[test]
    fn resolve_project_root_treats_empty_or_dot_as_current_directory() {
        let current = std::env::current_dir()
            .unwrap()
            .canonicalize()
            .unwrap_or_else(|_| std::env::current_dir().unwrap());

        assert_eq!(resolve_project_root(None), current);
        assert_eq!(resolve_project_root(Some(String::new())), current);
        assert_eq!(resolve_project_root(Some(".".to_string())), current);
        assert_eq!(resolve_project_root(Some("   ".to_string())), current);
    }

    #[test]
    fn inject_previous_run_dir_preserves_existing_invocation_shape() {
        let mut arguments = json!({
            "inputs": {
                "reads": "/tmp/reads",
            },
            "params": {
                "threshold": 0.8,
            },
            "resources": {},
        });

        inject_previous_run_dir(&mut arguments, "previous", "/tmp/oprun_1").unwrap();

        assert_eq!(
            arguments,
            json!({
                "inputs": {
                    "reads": "/tmp/reads",
                    "previous": "/tmp/oprun_1",
                },
                "params": {
                    "threshold": 0.8,
                },
                "resources": {},
            })
        );
    }

    #[test]
    fn inject_previous_run_dir_initializes_missing_inputs() {
        let mut arguments = json!({ "params": {} });

        inject_previous_run_dir(&mut arguments, "source", "/tmp/oprun_1").unwrap();

        assert_eq!(
            arguments,
            json!({
                "inputs": {
                    "source": "/tmp/oprun_1",
                },
                "params": {},
            })
        );
    }

    #[tokio::test]
    async fn operator_chain_linear_defaults_preserve_previous_output_pass_through() {
        let steps = prepare_chain_steps(chain_steps(json!([
            {
                "alias": "first",
                "arguments": {}
            },
            {
                "alias": "second",
                "arguments": {
                    "inputs": {
                        "legacy": "{{step1.outputDir}}"
                    }
                },
                "inheritPrevOutputAs": "source"
            }
        ])))
        .unwrap();
        assert_eq!(steps[0].label, "step_1");
        assert_eq!(steps[1].depends_on, vec!["step_1"]);

        let records = Arc::new(Mutex::new(Vec::new()));
        let result =
            run_prepared_operator_chain(steps, recording_runner(Arc::clone(&records))).await;

        assert!(result.ok);
        let records = records.lock().await;
        let (_, second_args) = records
            .iter()
            .find(|(label, _)| label == "step_2")
            .expect("second step ran");
        assert_eq!(
            second_args,
            &json!({
                "inputs": {
                    "legacy": "/tmp/step_1",
                    "source": "/tmp/step_1",
                }
            })
        );
    }

    #[tokio::test]
    async fn operator_chain_runs_ready_fan_out_steps_concurrently() {
        let steps = prepare_chain_steps(chain_steps(json!([
            {
                "alias": "root",
                "label": "root",
                "arguments": {},
                "dependsOn": []
            },
            {
                "alias": "branch_a",
                "label": "branch_a",
                "arguments": {},
                "dependsOn": ["root"]
            },
            {
                "alias": "branch_b",
                "label": "branch_b",
                "arguments": {},
                "dependsOn": ["root"]
            }
        ])))
        .unwrap();
        let active = Arc::new(AtomicUsize::new(0));
        let max_active = Arc::new(AtomicUsize::new(0));
        let runner: ChainStepRunner = Arc::new({
            let active = Arc::clone(&active);
            let max_active = Arc::clone(&max_active);
            move |step: PreparedChainStep, _arguments| {
                let active = Arc::clone(&active);
                let max_active = Arc::clone(&max_active);
                Box::pin(async move {
                    if step.label.starts_with("branch_") {
                        let current = active.fetch_add(1, Ordering::SeqCst) + 1;
                        record_max(&max_active, current);
                        tokio::time::sleep(Duration::from_millis(20)).await;
                        active.fetch_sub(1, Ordering::SeqCst);
                    }
                    successful_step(step.alias, format!("/tmp/{}", step.label))
                })
            }
        });

        let result = run_prepared_operator_chain(steps, runner).await;

        assert!(result.ok);
        assert!(max_active.load(Ordering::SeqCst) >= 2);
    }

    #[tokio::test]
    async fn operator_chain_waits_for_fan_in_dependencies_before_substitution() {
        let steps = prepare_chain_steps(chain_steps(json!([
            {
                "alias": "root",
                "label": "root",
                "arguments": {},
                "dependsOn": []
            },
            {
                "alias": "left",
                "label": "left",
                "arguments": {},
                "dependsOn": ["root"]
            },
            {
                "alias": "right",
                "label": "right",
                "arguments": {},
                "dependsOn": ["root"]
            },
            {
                "alias": "merge",
                "label": "merge",
                "arguments": {
                    "inputs": {
                        "left": "{{left.outputDir}}",
                        "right": "{{right.outputDir}}"
                    }
                },
                "dependsOn": ["left", "right"]
            }
        ])))
        .unwrap();
        let records = Arc::new(Mutex::new(Vec::new()));

        let result =
            run_prepared_operator_chain(steps, recording_runner(Arc::clone(&records))).await;

        assert!(result.ok);
        let records = records.lock().await;
        let (_, merge_args) = records
            .iter()
            .find(|(label, _)| label == "merge")
            .expect("merge step ran");
        assert_eq!(
            merge_args,
            &json!({
                "inputs": {
                    "left": "/tmp/left",
                    "right": "/tmp/right",
                }
            })
        );
    }

    #[test]
    fn operator_chain_rejects_dependency_cycles_before_running() {
        let error = prepare_chain_steps(chain_steps(json!([
            {
                "alias": "a",
                "label": "a",
                "arguments": {},
                "dependsOn": ["b"]
            },
            {
                "alias": "b",
                "label": "b",
                "arguments": {},
                "dependsOn": ["a"]
            }
        ])))
        .unwrap_err();

        assert!(error.contains("dependency cycle"));
    }

    #[test]
    fn operator_chain_rejects_unknown_dependency_labels_before_running() {
        let error = prepare_chain_steps(chain_steps(json!([
            {
                "alias": "a",
                "label": "a",
                "arguments": {},
                "dependsOn": []
            },
            {
                "alias": "b",
                "label": "b",
                "arguments": {},
                "dependsOn": ["missing"]
            }
        ])))
        .unwrap_err();

        assert!(error.contains("unknown label `missing`"));
    }

    #[tokio::test]
    async fn operator_chain_stops_unstarted_dependents_after_partial_failure() {
        let steps = prepare_chain_steps(chain_steps(json!([
            {
                "alias": "root",
                "label": "root",
                "arguments": {},
                "dependsOn": []
            },
            {
                "alias": "left",
                "label": "left",
                "arguments": {},
                "dependsOn": ["root"]
            },
            {
                "alias": "right",
                "label": "right",
                "arguments": {},
                "dependsOn": ["root"]
            },
            {
                "alias": "merge",
                "label": "merge",
                "arguments": {},
                "dependsOn": ["left", "right"]
            }
        ])))
        .unwrap();
        let started = Arc::new(Mutex::new(Vec::new()));
        let runner: ChainStepRunner = Arc::new({
            let started = Arc::clone(&started);
            move |step: PreparedChainStep, _arguments| {
                let started = Arc::clone(&started);
                Box::pin(async move {
                    started.lock().await.push(step.label.clone());
                    if step.label == "left" {
                        return chain_step_failure(step.alias, "left failed".to_string());
                    }
                    successful_step(step.alias, format!("/tmp/{}", step.label))
                })
            }
        });

        let result = run_prepared_operator_chain(steps, runner).await;

        assert!(!result.ok);
        assert_eq!(result.steps.len(), 3);
        let mut started = started.lock().await.clone();
        started.sort();
        assert_eq!(started, vec!["left", "right", "root"]);
    }
}
