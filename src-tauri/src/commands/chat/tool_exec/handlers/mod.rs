use super::dispatch::ToolDispatchContext;
use super::*;

mod exec_ops;
mod facade_ops;
mod file_ops;
mod memory_ops;
mod misc_ops;
mod skill_invoke_ops;
mod skill_ops;
mod web_ops;

pub(super) use skill_ops::extract_skill_allowed_tools;

pub(super) async fn dispatch_tool(ctx: &ToolDispatchContext<'_>) -> (String, String, bool) {
    let tool_name = ctx.tool_name.as_str();
    if skill_ops::is_skill_tool(tool_name) {
        skill_ops::handle_skill_tool(ctx).await
    } else if is_agent_tool_name(tool_name) {
        misc_ops::handle_agent_tool(ctx).await
    } else if tool_name.starts_with(crate::domain::operators::OPERATOR_TOOL_PREFIX) {
        misc_ops::handle_operator_tool(ctx).await
    } else if crate::domain::computer_use::is_facade_tool_name(tool_name) {
        facade_ops::handle_computer_tool(ctx).await
    } else if crate::domain::browser_operator::is_facade_tool_name(tool_name) {
        facade_ops::handle_browser_tool(ctx).await
    } else if tool_name.starts_with("mcp__") {
        misc_ops::handle_mcp_tool(ctx).await
    } else if web_ops::is_web_tool(tool_name) {
        web_ops::handle_web_tool(ctx).await
    } else if memory_ops::is_memory_tool(tool_name) {
        memory_ops::handle_memory_tool(ctx).await
    } else if file_ops::is_file_tool(tool_name) {
        file_ops::handle_file_tool(ctx).await
    } else if exec_ops::is_exec_tool(tool_name) {
        exec_ops::handle_exec_tool(ctx).await
    } else {
        misc_ops::handle_builtin_tool(ctx).await
    }
}

pub(super) async fn execute_domain_tool(ctx: &ToolDispatchContext<'_>) -> (String, String, bool) {
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
    let skill_task_context = ctx.skill_task_context;
    let web_search_api_keys = ctx.web_search_api_keys.clone();
    let skill_cache = ctx.skill_cache.clone();
    let cancel_flag = ctx.cancel_flag.clone();
    let round_cancel = ctx.round_cancel.clone();
    let execution_environment = ctx.execution_environment.clone();
    let ssh_server = ctx.ssh_server.clone();
    let sandbox_backend = ctx.sandbox_backend.clone();
    let local_venv_type = ctx.local_venv_type.clone();
    let local_venv_name = ctx.local_venv_name.clone();
    let env_store = ctx.env_store.clone();
    let agent_runtime = ctx.agent_runtime;
    let hook_engine = ctx.hook_engine;

    if matches_ask_user_question_name(tool_name) {
        if let Some(state) = app.try_state::<OmigaAppState>() {
            return execute_ask_user_question_interactive(AskUserQuestionExecution {
                tool_use_id: tool_use_id.to_string(),
                tool_name: tool_name.to_string(),
                arguments: arguments.to_string(),
                app: app.clone(),
                message_id: message_id.to_string(),
                session_id: session_id.to_string(),
                tool_results_dir,
                waiters: state.chat.ask_user_waiters.clone(),
                cancel_flag: cancel_flag.clone(),
            })
            .await;
        }
    }
    let working_memory_query =
        memory_ops::working_memory_query_text(tool_name, arguments, skill_task_context);
    let working_memory_context = if let Some(ref query_text) = working_memory_query {
        let state = app.state::<OmigaAppState>();
        crate::domain::memory::working_memory::render_context(
            &state.repo,
            session_id,
            query_text,
            crate::domain::memory::working_memory::DEFAULT_CONTEXT_TOKENS,
        )
        .await
        .ok()
        .flatten()
    } else {
        None
    };
    let ctx = {
        let web_use_proxy = crate::llm::config::load_web_use_proxy_setting();
        let web_search_engine = crate::llm::config::load_web_search_engine_setting();
        let web_search_methods = crate::llm::config::load_web_search_methods_setting();
        let db_pool = app
            .try_state::<OmigaAppState>()
            .map(|s| s.repo.pool().clone());
        let base = ToolContext::new(project_root.to_path_buf())
            .with_session_id(Some(session_id.to_string()))
            .with_working_memory_context(working_memory_context)
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
            .with_db(db_pool)
            .with_background_shell(
                crate::domain::background_shell::BackgroundShellHandle {
                    app: app.clone(),
                    chat_stream_event: format!("chat-stream-{}", message_id),
                    session_id: session_id.to_string(),
                    tool_use_id: tool_use_id.clone(),
                },
                tool_results_dir.to_path_buf(),
            );
        let base = if let Some(ctx_str) = skill_task_context {
            base.with_skill_task_context(ctx_str)
        } else {
            base
        };
        match &round_cancel {
            Some(t) => base.with_cancel_token(t.clone()),
            None => base,
        }
    };
    // ── Source Registry pre-check harness ──────────────────────────────────
    // Before fetch, check if this URL has been previously accessed and
    // has a cached gist.  If so, prepend it so the model can avoid redundant fetches.
    let src_prefix: Option<String> = if matches!(tool_name.as_str(), "fetch" | "Fetch") {
        let url = web_ops::fetch_source_url_from_args(arguments);
        if let Some(url) = url {
            if let Ok(cfg) = crate::domain::memory::load_resolved_config(project_root).await {
                let lt_root = cfg.long_term_path(project_root);
                let hits =
                    crate::domain::memory::source_registry::search_sources(&lt_root, &url, 1).await;
                hits.into_iter().next().and_then(|m| {
                    m.gist.map(|gist| {
                        format!(
                            "## Previously accessed source\n\
                                 **URL**: {}\n**Title**: {}\n\
                                 **Cached summary**: {}\n\n\
                                 ---\n## Current fetch results\n",
                            m.url,
                            m.title.as_deref().unwrap_or("(no title)"),
                            gist
                        )
                    })
                })
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };
    // ── End Source Registry pre-check harness ───────────────────────────────

    // ── Knowledge-base search harness ───────────────────────────────────────
    // Every search call first queries the local knowledge base (wiki +
    // implicit memory).  Results, if any, are prepended to the tool output so
    // the model is forced to see KB content before the web results.
    // This is a hard harness — it runs unconditionally regardless of what the
    // system prompt says.
    let kb_prefix: Option<String> = if matches!(tool_name.as_str(), "search" | "Search") {
        let query = serde_json::from_str::<serde_json::Value>(arguments)
            .ok()
            .and_then(|v| v.get("query").and_then(|q| q.as_str()).map(str::to_owned))
            .unwrap_or_default();
        if !query.trim().is_empty() {
            crate::commands::memory::get_memory_context_cached(
                &app.state::<OmigaAppState>().repo,
                project_root,
                Some(session_id),
                &query,
                5,
                Some(&app.state::<OmigaAppState>().memory_preflight_cache),
            )
            .await
            .map(|kb| {
                format!(
                    "## Knowledge base results (searched before web)\n\
                             > Query: {query}\n\n\
                             {kb}\n\n\
                             ---\n\
                             ## Web search results\n"
                )
            })
        } else {
            None
        }
    } else {
        None
    };
    // ── End knowledge-base harness ──────────────────────────────────────────

    match Tool::from_json_str(tool_name, arguments) {
        Ok(tool) => {
            match tool.execute(&ctx).await {
                Ok(mut output_stream) => {
                    use futures::StreamExt;

                    let mut output_text = String::new();
                    let mut stream_error = false;
                    let mut exit_code: Option<i32> = None;
                    let mut truncated_note = false;

                    // Collect output from the tool stream (see `fold_tool_stream_item_for_model`).
                    while let Some(item) = output_stream.next().await {
                        fold_tool_stream_item_for_model(
                            &mut output_text,
                            item,
                            &mut stream_error,
                            &mut exit_code,
                            &mut truncated_note,
                        );
                    }

                    append_truncated_results_note(&mut output_text, truncated_note);
                    apply_empty_structured_tool_placeholder(
                        &mut output_text,
                        tool_name,
                        stream_error || exit_code.map(|c| c != 0).unwrap_or(false),
                    );
                    let registry_output_text = output_text.clone();

                    // Prepend source registry prefix (fetch only).
                    if let Some(prefix) = src_prefix {
                        output_text = format!("{prefix}{output_text}");
                    }
                    // Prepend KB harness prefix (search only).
                    if let Some(prefix) = kb_prefix {
                        output_text = format!("{prefix}{output_text}");
                    }

                    let is_error = stream_error || exit_code.map(|c| c != 0).unwrap_or(false);
                    if let Some(engine) = hook_engine.filter(|engine| !engine.is_empty()) {
                        if let crate::domain::hooks::PostHookOutcome::AppendFeedback { text } =
                            engine
                                .run_post_tool_use(tool_name, arguments, &output_text, is_error)
                                .await
                        {
                            if !text.trim().is_empty() {
                                output_text.push_str("\n\n");
                                output_text.push_str(&text);
                            }
                        }
                    }

                    // Truncate streamed UI preview — align with TS `PREVIEW_SIZE_BYTES` (2000 bytes).
                    // Full `output_text` is still returned for DB persistence; large-result
                    // file spill threshold is `DEFAULT_MAX_RESULT_SIZE_CHARS` in `tool_limits`.
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

                    // Align with TS MCPTool UI `maxChars: 2000` (`TOOL_DISPLAY_MAX_INPUT_CHARS`).
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

                    // Source Registry: record web sources used in this turn.
                    if !is_error {
                        web_ops::register_web_source_async(
                            tool_name,
                            arguments,
                            &registry_output_text,
                            session_id,
                            project_root,
                        );
                    }

                    let model_output = process_tool_output_for_model(
                        output_text.clone(),
                        tool_use_id,
                        tool_results_dir,
                    )
                    .await;

                    (tool_use_id.clone(), model_output, is_error)
                }
                Err(e) => {
                    let error_msg = format!("Tool execution failed: {}", e);
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
            let error_msg = format!("Failed to parse tool arguments: {}", e);
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
