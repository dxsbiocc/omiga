use super::super::dispatch::ToolDispatchContext;
use super::super::*;

pub(super) async fn handle_agent_tool(ctx: &ToolDispatchContext<'_>) -> (String, String, bool) {
    let app = ctx.app.clone();
    let tool_use_id = ctx.tool_use_id;
    let tool_name = ctx.tool_name;
    let arguments = ctx.arguments;
    let message_id = ctx.message_id;
    let session_id = ctx.session_id;
    let tool_results_dir = ctx.tool_results_dir;
    let project_root = ctx.project_root;
    let session_todos = ctx.session_todos.clone();
    let session_agent_tasks = ctx.session_agent_tasks.clone();
    let subagent_depth = ctx.subagent_depth;
    let web_search_api_keys = ctx.web_search_api_keys.clone();
    let skill_cache = ctx.skill_cache.clone();
    let agent_runtime = ctx.agent_runtime;

    let nested_allowed = agent_runtime.map(|r| r.allow_nested_agent).unwrap_or(false);
    if subagent_depth >= MAX_SUBAGENT_EXECUTE_DEPTH {
        let error_msg =
            format!("Agent tool: maximum nested depth ({MAX_SUBAGENT_EXECUTE_DEPTH}) reached.");
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
        (tool_use_id.clone(), error_msg, true)
    } else if subagent_depth > 0 && !nested_allowed {
        let error_msg =
            "Nested Agent tool is not allowed (set `USER_TYPE=ant` for nested Agent parity)."
                .to_string();
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
        (tool_use_id.clone(), error_msg, true)
    } else if let Some(ar) = agent_runtime {
        match serde_json::from_str::<crate::domain::tools::agent::AgentArgs>(arguments) {
            Ok(agent_args) => {
                match run_subagent_session(SubagentSessionRequest {
                    app: &app,
                    message_id,
                    session_id,
                    tool_results_dir,
                    project_root,
                    session_todos: session_todos.clone(),
                    session_agent_tasks: session_agent_tasks.clone(),
                    args: &agent_args,
                    runtime: ar,
                    subagent_execute_depth: subagent_depth.saturating_add(1),
                    web_search_api_keys: web_search_api_keys.clone(),
                    skill_cache: skill_cache.clone(),
                })
                .await
                {
                    Ok(output_text) => {
                        let is_error = false;
                        let display_output = if output_text.len() > PREVIEW_SIZE_BYTES {
                            let prefix = truncate_utf8_prefix(&output_text, PREVIEW_SIZE_BYTES);
                            format!(
                                "{}\n\n[Output truncated... {} total characters]",
                                prefix,
                                output_text.len()
                            )
                        } else {
                            output_text.clone()
                        };
                        let display_input = if arguments.len() > TOOL_DISPLAY_MAX_INPUT_CHARS {
                            let prefix =
                                truncate_utf8_prefix(arguments, TOOL_DISPLAY_MAX_INPUT_CHARS);
                            format!(
                                "{}\n\n[Input truncated... {} total characters]",
                                prefix,
                                arguments.len()
                            )
                        } else {
                            arguments.clone()
                        };
                        let _ = app.emit(
                            &format!("chat-stream-{}", message_id),
                            &StreamOutputItem::ToolResult {
                                tool_use_id: tool_use_id.clone(),
                                name: tool_name.clone(),
                                input: display_input,
                                output: display_output,
                                is_error,
                            },
                        );
                        let model_output = process_tool_output_for_model(
                            output_text.clone(),
                            tool_use_id,
                            tool_results_dir,
                        )
                        .await;
                        (tool_use_id.clone(), model_output, is_error)
                    }
                    Err(e) => {
                        let error_msg = format!("Agent tool: {}", e);
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
                        (tool_use_id.clone(), error_msg, true)
                    }
                }
            }
            Err(e) => {
                let error_msg = format!("Failed to parse Agent arguments: {}", e);
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
                (tool_use_id.clone(), error_msg, true)
            }
        }
    } else {
        let error_msg =
            "Agent tool requires an active chat session (LLM runtime missing).".to_string();
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
        (tool_use_id.clone(), error_msg, true)
    }
}

pub(super) async fn handle_operator_tool(ctx: &ToolDispatchContext<'_>) -> (String, String, bool) {
    let app = ctx.app.clone();
    let tool_use_id = ctx.tool_use_id;
    let tool_name = ctx.tool_name;
    let arguments = ctx.arguments;
    let message_id = ctx.message_id;
    let session_id = ctx.session_id;
    let tool_results_dir = ctx.tool_results_dir;
    let project_root = ctx.project_root;
    let session_todos = ctx.session_todos.clone();
    let session_agent_tasks = ctx.session_agent_tasks.clone();
    let web_search_api_keys = ctx.web_search_api_keys.clone();
    let skill_cache = ctx.skill_cache.clone();
    let round_cancel = ctx.round_cancel.clone();
    let execution_environment = ctx.execution_environment.clone();
    let ssh_server = ctx.ssh_server.clone();
    let sandbox_backend = ctx.sandbox_backend.clone();
    let local_venv_type = ctx.local_venv_type.clone();
    let local_venv_name = ctx.local_venv_name.clone();
    let env_store = ctx.env_store.clone();
    let agent_runtime = ctx.agent_runtime;

    let ctx = {
        let web_use_proxy = crate::llm::config::load_web_use_proxy_setting();
        let web_search_engine = crate::llm::config::load_web_search_engine_setting();
        let web_search_methods = crate::llm::config::load_web_search_methods_setting();
        let db_pool = app
            .try_state::<OmigaAppState>()
            .map(|s| s.repo.pool().clone());
        let base = ToolContext::new(project_root.to_path_buf())
            .with_session_id(Some(session_id.to_string()))
            .with_todos(session_todos.clone())
            .with_agent_tasks(session_agent_tasks.clone())
            .with_plan_mode(agent_runtime.and_then(|r| r.plan_mode_flag.clone()))
            .with_web_search_api_keys(web_search_api_keys.clone())
            .with_web_use_proxy(web_use_proxy)
            .with_web_search_engine(web_search_engine)
            .with_web_search_methods(web_search_methods)
            .with_tool_results_dir(tool_results_dir.to_path_buf())
            .with_execution_environment(execution_environment.clone())
            .with_ssh_server(ssh_server.clone())
            .with_sandbox_backend(sandbox_backend.clone())
            .with_local_venv(local_venv_type.clone(), local_venv_name.clone())
            .with_env_store(Some(env_store.clone()))
            .with_skill_cache(skill_cache.clone())
            .with_db(db_pool);
        match &round_cancel {
            Some(t) => base.with_cancel_token(t.clone()),
            None => base,
        }
    };
    let (output_text, is_error) =
        crate::domain::operators::execute_operator_tool_call(&ctx, tool_name, arguments).await;
    let display_output = if output_text.len() > PREVIEW_SIZE_BYTES {
        let prefix = truncate_utf8_prefix(&output_text, PREVIEW_SIZE_BYTES);
        format!(
            "{}\n\n[Output truncated... {} total characters]",
            prefix,
            output_text.len()
        )
    } else {
        output_text.clone()
    };
    let display_input = if arguments.len() > TOOL_DISPLAY_MAX_INPUT_CHARS {
        let prefix = truncate_utf8_prefix(arguments, TOOL_DISPLAY_MAX_INPUT_CHARS);
        format!(
            "{}\n\n[Input truncated... {} total characters]",
            prefix,
            arguments.len()
        )
    } else {
        arguments.clone()
    };
    let _ = app.emit(
        &format!("chat-stream-{}", message_id),
        &StreamOutputItem::ToolResult {
            tool_use_id: tool_use_id.clone(),
            name: tool_name.clone(),
            input: display_input,
            output: display_output,
            is_error,
        },
    );
    let model_output =
        process_tool_output_for_model(output_text.clone(), tool_use_id, tool_results_dir).await;
    (tool_use_id.clone(), model_output, is_error)
}

pub(super) async fn handle_mcp_tool(ctx: &ToolDispatchContext<'_>) -> (String, String, bool) {
    let app = ctx.app.clone();
    let tool_use_id = ctx.tool_use_id;
    let tool_name = ctx.tool_name;
    let arguments = ctx.arguments;
    let message_id = ctx.message_id;
    let session_id = ctx.session_id;
    let tool_results_dir = ctx.tool_results_dir;
    let project_root = ctx.project_root;

    let timeout = std::time::Duration::from_secs(120);
    // Use the session-aware MCP connection manager to avoid spawning new processes
    // while properly handling session boundaries and stdio lifecycle.
    let (mcp_manager, session_id_opt) = app
        .try_state::<crate::app_state::OmigaAppState>()
        .map(|s| {
            (
                Some(s.chat.mcp_manager.clone()),
                Some(session_id.to_string()),
            )
        })
        .unwrap_or((None, None));

    // Legacy fallback (for backwards compatibility during migration)
    // Note: This is a placeholder - the legacy pool is no longer used
    // as all connections go through the new manager
    let mcp_pool_legacy: Option<
        std::sync::Arc<
            tokio::sync::Mutex<
                std::collections::HashMap<String, crate::domain::mcp::client::McpLiveConnection>,
            >,
        >,
    > = None;

    match crate::domain::mcp::tool_dispatch::execute_mcp_tool_call(
        project_root,
        tool_name,
        arguments,
        timeout,
        mcp_manager,
        mcp_pool_legacy,
        session_id_opt,
    )
    .await
    {
        Ok((output_text, mcp_is_error)) => {
            let display_output = if output_text.len() > PREVIEW_SIZE_BYTES {
                let prefix = truncate_utf8_prefix(&output_text, PREVIEW_SIZE_BYTES);
                format!(
                    "{}\n\n[Output truncated... {} total characters]",
                    prefix,
                    output_text.len()
                )
            } else {
                output_text.clone()
            };
            let display_input = if arguments.len() > TOOL_DISPLAY_MAX_INPUT_CHARS {
                let prefix = truncate_utf8_prefix(arguments, TOOL_DISPLAY_MAX_INPUT_CHARS);
                format!(
                    "{}\n\n[Input truncated... {} total characters]",
                    prefix,
                    arguments.len()
                )
            } else {
                arguments.clone()
            };
            let _ = app.emit(
                &format!("chat-stream-{}", message_id),
                &StreamOutputItem::ToolResult {
                    tool_use_id: tool_use_id.clone(),
                    name: tool_name.clone(),
                    input: display_input,
                    output: display_output,
                    is_error: mcp_is_error,
                },
            );
            let model_output =
                process_tool_output_for_model(output_text.clone(), tool_use_id, tool_results_dir)
                    .await;
            (tool_use_id.clone(), model_output, mcp_is_error)
        }
        Err(e) => {
            let error_msg = format!("MCP tool error: {e}");
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
            (tool_use_id.clone(), error_msg, true)
        }
    }
}

pub(super) async fn handle_builtin_tool(ctx: &ToolDispatchContext<'_>) -> (String, String, bool) {
    super::execute_domain_tool(ctx).await
}
