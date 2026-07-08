use super::super::agent_runtime::AgentLlmRuntime;
use super::super::permissions::{
    execute_ask_user_question_interactive, wait_for_permission_tool_resolution,
    AskUserQuestionExecution, PermissionToolResolutionRequest,
};
use super::handlers;
use super::orchestrate::{SingleToolExecution, ToolExecutionShared};
use crate::app_state::OmigaAppState;
use crate::domain::agents::subagent_tool_filter::{
    should_block_subagent_builtin_call, SubagentFilterOptions,
};
use crate::domain::permissions::canonical_permission_tool_name;
use crate::domain::session::{AgentTask, TodoItem};
use crate::domain::skills;
use crate::domain::tools::WebSearchApiKeys;
use crate::infrastructure::streaming::StreamOutputItem;
use std::path::Path;
use std::sync::{Arc, Mutex as StdMutex};
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::RwLock;

pub(super) struct ToolDispatchContext<'a> {
    pub(super) app: &'a AppHandle,
    pub(super) tool_use_id: &'a String,
    pub(super) tool_name: &'a String,
    pub(super) arguments: &'a String,
    pub(super) message_id: &'a String,
    pub(super) session_id: &'a String,
    pub(super) tool_results_dir: &'a Path,
    pub(super) project_root: &'a Path,
    pub(super) session_todos: Option<Arc<tokio::sync::Mutex<Vec<TodoItem>>>>,
    pub(super) session_agent_tasks: Option<Arc<tokio::sync::Mutex<Vec<AgentTask>>>>,
    pub(super) subagent_depth: u8,
    pub(super) skill_task_context: Option<&'a str>,
    pub(super) web_search_api_keys: WebSearchApiKeys,
    pub(super) skill_cache: Arc<StdMutex<skills::SkillCacheMap>>,
    pub(super) cancel_flag: Option<Arc<RwLock<bool>>>,
    pub(super) round_cancel: Option<tokio_util::sync::CancellationToken>,
    pub(super) execution_environment: String,
    pub(super) ssh_server: Option<String>,
    pub(super) sandbox_backend: String,
    pub(super) local_venv_type: String,
    pub(super) local_venv_name: String,
    pub(super) env_store: crate::domain::tools::env_store::EnvStore,
    pub(super) computer_use_enabled: bool,
    pub(super) browser_use_enabled: bool,
    pub(super) agent_runtime: Option<&'a AgentLlmRuntime>,
    pub(super) hook_engine: Option<&'a crate::domain::hooks::HookEngine>,
}

/// Execute a single tool call. Called from both the parallel and sequential paths.
#[async_recursion::async_recursion]
pub(super) async fn execute_one_tool(request: SingleToolExecution) -> (String, String, bool) {
    let SingleToolExecution {
        tool_use_id,
        tool_name,
        arguments,
        shared,
        agent_runtime,
    } = request;
    let agent_runtime = agent_runtime.as_ref();
    let ToolExecutionShared {
        app,
        message_id,
        session_id,
        tool_results_dir,
        project_root,
        session_todos,
        session_agent_tasks,
        subagent_depth,
        skill_task_context,
        web_search_api_keys,
        skill_cache,
        cancel_flag,
        round_cancel,
        execution_environment,
        ssh_server,
        sandbox_backend,
        local_venv_type,
        local_venv_name,
        env_store,
        computer_use_enabled,
        browser_use_enabled,
        artifact_registry: _artifact_registry,
    } = shared;
    let tool_use_id = &tool_use_id;
    let tool_name = &tool_name;
    let mut effective_arguments = arguments;
    let message_id = &message_id;
    let session_id = &session_id;
    let tool_results_dir = tool_results_dir.as_path();
    let project_root = project_root.as_path();
    let skill_task_context = skill_task_context.as_deref();

    // Subagent plan-mode re-check (fast, per-call)
    if subagent_depth > 0 {
        let c = canonical_permission_tool_name(tool_name);
        let parent_in_plan = if let Some(ar) = agent_runtime {
            if let Some(ref pm) = ar.plan_mode_flag {
                *pm.lock().await
            } else {
                false
            }
        } else {
            false
        };
        let allow_nested = agent_runtime.map(|r| r.allow_nested_agent).unwrap_or(false);
        let sub_opts = SubagentFilterOptions {
            parent_in_plan_mode: parent_in_plan,
            allow_nested_agent: allow_nested,
        };
        if should_block_subagent_builtin_call(&c, sub_opts) {
            let error_msg = format!(
                "Tool `{tool_name}` is not available to sub-agents (Claude Code `ALL_AGENT_DISALLOWED_TOOLS`)."
            );
            let _ = app.emit(
                &format!("chat-stream-{}", message_id),
                &StreamOutputItem::ToolResult {
                    tool_use_id: tool_use_id.clone(),
                    name: tool_name.clone(),
                    input: effective_arguments.clone(),
                    output: error_msg.clone(),
                    is_error: true,
                },
            );
            return (tool_use_id.clone(), error_msg, true);
        }
    }

    let preflight_question_args =
        if tool_name.starts_with(crate::domain::operators::OPERATOR_TOOL_PREFIX) {
            crate::domain::operators::operator_preflight_question_with_project_preferences(
                project_root,
                tool_name,
                &effective_arguments,
            )
        } else if tool_name == crate::domain::operators::OPERATOR_EXECUTE_TOOL_NAME {
            crate::domain::operators::operator_execute_preflight_question_with_project_preferences(
                project_root,
                &effective_arguments,
            )
        } else if tool_name == "template_execute" {
            crate::domain::templates::template_preflight_question_with_project_preferences(
                project_root,
                &effective_arguments,
            )
        } else {
            None
        };

    if let Some(question_args) = preflight_question_args {
        let Some(app_state) = app.try_state::<OmigaAppState>() else {
            let error_msg = "内部错误：无法获取应用状态以显示参数选择".to_string();
            let _ = app.emit(
                &format!("chat-stream-{}", message_id),
                &StreamOutputItem::ToolResult {
                    tool_use_id: tool_use_id.clone(),
                    name: tool_name.clone(),
                    input: effective_arguments.clone(),
                    output: error_msg.clone(),
                    is_error: true,
                },
            );
            return (tool_use_id.clone(), error_msg, true);
        };
        let ask_arguments = match serde_json::to_string(&question_args) {
            Ok(value) => value,
            Err(err) => {
                let error_msg = format!("Failed to serialize preflight questions: {err}");
                let _ = app.emit(
                    &format!("chat-stream-{}", message_id),
                    &StreamOutputItem::ToolResult {
                        tool_use_id: tool_use_id.clone(),
                        name: tool_name.clone(),
                        input: effective_arguments.clone(),
                        output: error_msg.clone(),
                        is_error: true,
                    },
                );
                return (tool_use_id.clone(), error_msg, true);
            }
        };
        let ask_tool_use_id = format!(
            "preflight-{}-{}",
            tool_use_id,
            uuid::Uuid::new_v4().simple()
        );
        let (_ask_id, ask_output, ask_is_error) =
            execute_ask_user_question_interactive(AskUserQuestionExecution {
                tool_use_id: ask_tool_use_id,
                tool_name: "ask_user_question".to_string(),
                arguments: ask_arguments,
                app: app.clone(),
                message_id: message_id.to_string(),
                session_id: session_id.to_string(),
                tool_results_dir,
                waiters: app_state.chat.ask_user_waiters.clone(),
                cancel_flag: cancel_flag.clone(),
            })
            .await;
        if ask_is_error {
            let _ = app.emit(
                &format!("chat-stream-{}", message_id),
                &StreamOutputItem::ToolResult {
                    tool_use_id: tool_use_id.clone(),
                    name: tool_name.clone(),
                    input: effective_arguments.clone(),
                    output: ask_output.clone(),
                    is_error: true,
                },
            );
            return (tool_use_id.clone(), ask_output, true);
        }
        let ask_output_json = match serde_json::from_str::<serde_json::Value>(&ask_output) {
            Ok(value) => value,
            Err(err) => {
                let error_msg = format!("Failed to parse preflight answers: {err}");
                let _ = app.emit(
                    &format!("chat-stream-{}", message_id),
                    &StreamOutputItem::ToolResult {
                        tool_use_id: tool_use_id.clone(),
                        name: tool_name.clone(),
                        input: effective_arguments.clone(),
                        output: error_msg.clone(),
                        is_error: true,
                    },
                );
                return (tool_use_id.clone(), error_msg, true);
            }
        };
        let updated_arguments =
            if tool_name.starts_with(crate::domain::operators::OPERATOR_TOOL_PREFIX) {
                crate::domain::operators::apply_operator_preflight_answers(
                    tool_name,
                    &effective_arguments,
                    &ask_output_json,
                )
            } else if tool_name == crate::domain::operators::OPERATOR_EXECUTE_TOOL_NAME {
                crate::domain::operators::apply_operator_execute_preflight_answers(
                    &effective_arguments,
                    &ask_output_json,
                )
            } else if tool_name == "template_execute" {
                crate::domain::templates::apply_template_preflight_answers(
                    &effective_arguments,
                    &ask_output_json,
                )
            } else {
                Ok(effective_arguments.clone())
            };
        match updated_arguments {
            Ok(updated) => effective_arguments = updated,
            Err(err) => {
                let error_msg = format!("Failed to apply preflight answers: {err}");
                let _ = app.emit(
                    &format!("chat-stream-{}", message_id),
                    &StreamOutputItem::ToolResult {
                        tool_use_id: tool_use_id.clone(),
                        name: tool_name.clone(),
                        input: effective_arguments.clone(),
                        output: error_msg.clone(),
                        is_error: true,
                    },
                );
                return (tool_use_id.clone(), error_msg, true);
            }
        }
    }

    // G11 hooks are loaded on demand from `.omiga/hooks.toml` because AppState is
    // outside this task's allowed edit set. Missing config is a fast path: no
    // engine is constructed and execution continues unchanged.
    let hook_engine = if crate::domain::hooks::hook_config_path(project_root).exists() {
        Some(crate::domain::hooks::HookEngine::load_for_project(
            project_root,
        ))
    } else {
        None
    };
    if let Some(engine) = hook_engine.as_ref().filter(|engine| !engine.is_empty()) {
        match engine
            .run_pre_tool_use(tool_name, &effective_arguments)
            .await
        {
            crate::domain::hooks::PreHookOutcome::Proceed => {}
            crate::domain::hooks::PreHookOutcome::ModifyArgs { new_args_json } => {
                effective_arguments = new_args_json;
            }
            crate::domain::hooks::PreHookOutcome::Block { reason } => {
                let _ = app.emit(
                    &format!("chat-stream-{}", message_id),
                    &StreamOutputItem::ToolResult {
                        tool_use_id: tool_use_id.clone(),
                        name: tool_name.clone(),
                        input: effective_arguments.clone(),
                        output: reason.clone(),
                        is_error: true,
                    },
                );
                return (tool_use_id.clone(), reason, true);
            }
        }
    }

    let arguments = &effective_arguments;

    if crate::domain::browser_operator::is_facade_tool_name(tool_name) && !browser_use_enabled {
        let args_value: serde_json::Value = serde_json::from_str(arguments)
            .unwrap_or_else(|_| serde_json::json!({"raw": arguments}));
        let display_input =
            crate::domain::browser_operator::redact_arguments_for_display(&args_value).to_string();
        let error_msg = format!(
            "Browser Operator facade tool `{tool_name}` is not enabled for this task. Select the Browser plugin or use a compatible Browser Operator gate before using `browser_*` tools."
        );
        let _ = app.emit(
            &format!("chat-stream-{}", message_id),
            &StreamOutputItem::ToolResult {
                tool_use_id: tool_use_id.clone(),
                name: tool_name.clone(),
                input: display_input,
                output: error_msg.clone(),
                is_error: true,
            },
        );
        return (tool_use_id.clone(), error_msg, true);
    }

    if crate::domain::mcp::names::is_reserved_computer_mcp_tool(tool_name) {
        let error_msg = format!(
            "Raw Computer Use MCP backend tool `{tool_name}` is not directly callable. Enable Computer Use for the task and use the `computer_*` facade tools so Omiga can enforce permissions, audit logging, stop handling, and target-window validation."
        );
        let _ = app.emit(
            &format!("chat-stream-{}", message_id),
            &StreamOutputItem::ToolResult {
                tool_use_id: tool_use_id.clone(),
                name: tool_name.clone(),
                input: arguments.clone(),
                output: error_msg.clone(),
                is_error: true,
            },
        );
        return (tool_use_id.clone(), error_msg, true);
    }

    // === Permission Check for ALL tools (not just skill) ===
    // Skip permission check for certain safe tools
    let needs_permission_check = !matches!(
        tool_name.as_str(),
        "list_skills" | "skills_list" | "skill_view" | "ask_user" | "ask_user_question"
    );

    if needs_permission_check {
        let Some(app_state) = app.try_state::<OmigaAppState>() else {
            let display_input = if crate::domain::browser_operator::is_facade_tool_name(tool_name) {
                let args_value: serde_json::Value = serde_json::from_str(arguments)
                    .unwrap_or_else(|_| serde_json::json!({"raw": arguments}));
                crate::domain::browser_operator::redact_arguments_for_display(&args_value)
                    .to_string()
            } else {
                arguments.clone()
            };
            let error_msg = "内部错误：无法获取应用状态".to_string();
            let _ = app.emit(
                &format!("chat-stream-{}", message_id),
                &StreamOutputItem::ToolResult {
                    tool_use_id: tool_use_id.clone(),
                    name: tool_name.clone(),
                    input: display_input,
                    output: error_msg.clone(),
                    is_error: true,
                },
            );
            return (tool_use_id.clone(), error_msg, true);
        };
        let permission_manager = app_state.permission_manager.clone();

        let args_value: serde_json::Value = serde_json::from_str(arguments)
            .unwrap_or_else(|_| serde_json::json!({"raw": arguments}));
        let redacted_args_value_for_display =
            if crate::domain::browser_operator::is_facade_tool_name(tool_name) {
                crate::domain::browser_operator::redact_arguments_for_display(&args_value)
            } else {
                args_value.clone()
            };
        let arguments_display = if crate::domain::browser_operator::is_facade_tool_name(tool_name) {
            redacted_args_value_for_display.to_string()
        } else {
            arguments.clone()
        };

        loop {
            let perm_decision = permission_manager
                .check_tool_with_root(session_id, tool_name, &args_value, Some(project_root))
                .await;

            match perm_decision {
                crate::domain::permissions::PermissionDecision::Deny(ref reason) => {
                    let project_root_label = project_root.to_string_lossy().to_string();
                    let arguments_json = serde_json::to_string(&redacted_args_value_for_display)
                        .unwrap_or_else(|_| "{}".to_string());
                    crate::commands::permissions::append_permission_audit_event(
                        &app_state,
                        session_id,
                        None,
                        Some(&project_root_label),
                        "denied",
                        tool_name,
                        None,
                        Some(reason),
                        &arguments_json,
                    )
                    .await;
                    tracing::warn!(
                        tool = %tool_name,
                        reason = %reason,
                        "Tool denied by permission manager"
                    );
                    let error_msg = format!("权限被拒绝: {}", reason);
                    let _ = app.emit(
                        &format!("chat-stream-{}", message_id),
                        &StreamOutputItem::ToolResult {
                            tool_use_id: tool_use_id.clone(),
                            name: tool_name.clone(),
                            input: arguments_display.clone(),
                            output: error_msg.clone(),
                            is_error: true,
                        },
                    );
                    return (tool_use_id.clone(), error_msg, true);
                }
                crate::domain::permissions::PermissionDecision::RequireApproval(ref req) => {
                    tracing::info!(
                        tool = %tool_name,
                        risk_level = ?req.risk.level,
                        "Tool requires user approval — blocking until UI resolves"
                    );
                    match wait_for_permission_tool_resolution(PermissionToolResolutionRequest {
                        app: &app,
                        app_state: &app_state,
                        session_id,
                        message_id,
                        tool_use_id,
                        stream_tool_name: tool_name,
                        tool_name_for_event: tool_name,
                        arguments_display: &arguments_display,
                        args_value: &args_value,
                        req,
                        cancel_flag: cancel_flag.clone(),
                    })
                    .await
                    {
                        Ok(()) => continue,
                        Err(e) => {
                            let _ = app.emit(
                                &format!("chat-stream-{}", message_id),
                                &StreamOutputItem::ToolResult {
                                    tool_use_id: tool_use_id.clone(),
                                    name: tool_name.clone(),
                                    input: arguments_display.clone(),
                                    output: e.clone(),
                                    is_error: true,
                                },
                            );
                            return (tool_use_id.clone(), e, true);
                        }
                    }
                }
                crate::domain::permissions::PermissionDecision::Allow => {
                    tracing::debug!(
                        tool = %tool_name,
                        "Tool allowed by permission manager"
                    );
                    break;
                }
            }
        }
    }
    // === End permission check ===

    let ctx = ToolDispatchContext {
        app: &app,
        tool_use_id,
        tool_name,
        arguments,
        message_id,
        session_id,
        tool_results_dir,
        project_root,
        session_todos,
        session_agent_tasks,
        subagent_depth,
        skill_task_context,
        web_search_api_keys,
        skill_cache,
        cancel_flag,
        round_cancel,
        execution_environment,
        ssh_server,
        sandbox_backend,
        local_venv_type,
        local_venv_name,
        env_store,
        computer_use_enabled,
        browser_use_enabled,
        agent_runtime,
        hook_engine: hook_engine.as_ref(),
    };

    handlers::dispatch_tool(&ctx).await
}
