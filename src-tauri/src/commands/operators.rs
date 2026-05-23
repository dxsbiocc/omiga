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
use serde::Serialize;
use serde_json::{json, Value as JsonValue};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::OnceLock;
use tauri::{AppHandle, Emitter, State};
use tokio::sync::Mutex as TokioMutex;
use tokio_util::sync::CancellationToken;

static OPERATOR_TASK_MAP: OnceLock<TokioMutex<HashMap<String, CancellationToken>>> =
    OnceLock::new();

fn operator_task_map() -> &'static TokioMutex<HashMap<String, CancellationToken>> {
    OPERATOR_TASK_MAP.get_or_init(|| TokioMutex::new(HashMap::new()))
}

fn operator_error(error: String) -> AppError {
    AppError::Config(error)
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
pub async fn list_operators() -> CommandResult<OperatorCatalogResponse> {
    Ok(OperatorCatalogResponse {
        registry_path: operators::registry_path().to_string_lossy().into_owned(),
        operators: operators::list_operator_summaries(),
        diagnostics: operators::list_operator_manifest_diagnostics(),
    })
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
    operator_task_map()
        .lock()
        .await
        .insert(task_id.clone(), cancel_token.clone());

    let task_id_clone = task_id.clone();
    let alias_clone = alias.clone();
    let tool_name = if alias.starts_with(operators::OPERATOR_TOOL_PREFIX) {
        alias.clone()
    } else {
        format!("{}{}", operators::OPERATOR_TOOL_PREFIX, alias)
    };

    tokio::spawn(async move {
        let event = format!("operator-task-{}", task_id_clone);

        let _ = app.emit(
            &event,
            &OperatorTaskEvent::Started {
                task_id: task_id_clone.clone(),
                alias: alias_clone,
            },
        );

        tokio::select! {
            _ = cancel_token.cancelled() => {
                let _ = app.emit(
                    &event,
                    &OperatorTaskEvent::Cancelled {
                        task_id: task_id_clone.clone(),
                    },
                );
            }
            result = operators::execute_operator_tool_call_with_context(
                &ctx,
                &tool_name,
                &arguments_str,
                Some(run_context),
            ) => {
                let (raw, is_error) = result;
                let parsed =
                    serde_json::from_str::<JsonValue>(&raw).unwrap_or_else(|_| json!({ "raw": raw }));
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
    });

    Ok(json!({ "taskId": task_id }))
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
    use super::resolve_project_root;

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
}
