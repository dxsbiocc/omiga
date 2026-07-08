use super::super::dispatch::ToolDispatchContext;
use super::super::*;

pub(super) async fn handle_skill_invoke(ctx: &ToolDispatchContext<'_>) -> (String, String, bool) {
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
    let cancel_flag = ctx.cancel_flag.clone();
    let agent_runtime = ctx.agent_runtime;

    if tool_name.eq_ignore_ascii_case("skill") || tool_name == "Skill" {
        match serde_json::from_str::<SkillToolArgs>(arguments) {
            Ok(args) => {
                if args.skill.trim().is_empty() {
                    let error_msg = "skill tool: missing or empty `skill` field".to_string();
                    tracing::warn!(
                        tool = "skill",
                        error = "empty_skill_name",
                        "Skill tool called with empty skill name"
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
                    (tool_use_id.clone(), error_msg, true)
                } else {
                    let icfg = integrations_config::load_integrations_config(project_root);
                    let all_skills = skills::load_skills_cached(project_root, &skill_cache).await;
                    let resolved_name =
                        skills::resolve_skill_display_name(&all_skills, &args.skill);
                    let blocked = resolved_name
                        .as_ref()
                        .map(|nm| integrations_config::is_skill_name_disabled(&icfg, nm))
                        .unwrap_or(false);

                    // Telemetry for skill invocation (aligned with SkillTool in TS)
                    tracing::info!(
                        tool = "skill",
                        raw_skill = %args.skill,
                        resolved_name = ?resolved_name,
                        has_args = !args.args.is_empty(),
                        total_available_skills = all_skills.len(),
                        blocked_by_config = blocked,
                        "Skill tool invoked"
                    );

                    if blocked {
                        let skill_display =
                            resolved_name.unwrap_or_else(|| args.skill.trim().to_string());
                        let error_msg = format!(
                                "Skill `{skill_display}` is disabled in Omiga Settings → Integrations (Skills)."
                            );
                        tracing::warn!(skill_name = %skill_display, "Skill invocation blocked by user config");
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
                    } else {
                        // === Permission Check (New PermissionManager) ===
                        let skill_display = resolved_name
                            .clone()
                            .unwrap_or_else(|| args.skill.trim().to_string());

                        let Some(app_state_skill) = app.try_state::<OmigaAppState>() else {
                            let error_msg = "内部错误：无法获取应用状态".to_string();
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
                        };
                        let permission_manager = app_state_skill.permission_manager.clone();

                        let args_value = serde_json::json!({
                            "skill": args.skill,
                            "args": args.args,
                            "execution_mode": args.execution_mode,
                        });

                        loop {
                            let perm_decision = permission_manager
                                .check_tool_with_root(
                                    session_id,
                                    &skill_display,
                                    &args_value,
                                    Some(project_root),
                                )
                                .await;

                            match perm_decision {
                                crate::domain::permissions::PermissionDecision::Deny(
                                    ref reason,
                                ) => {
                                    tracing::warn!(
                                        skill = %skill_display,
                                        reason = %reason,
                                        "Skill denied by permission manager"
                                    );
                                    let error_msg = format!("权限被拒绝: {}", reason);
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
                                crate::domain::permissions::PermissionDecision::RequireApproval(
                                    ref req,
                                ) => {
                                    tracing::info!(
                                        skill = %skill_display,
                                        risk_level = ?req.risk.level,
                                        "Skill requires user approval — blocking until UI resolves"
                                    );
                                    match wait_for_permission_tool_resolution(
                                        PermissionToolResolutionRequest {
                                            app: &app,
                                            app_state: &app_state_skill,
                                            session_id,
                                            message_id,
                                            tool_use_id,
                                            stream_tool_name: tool_name,
                                            tool_name_for_event: skill_display.as_str(),
                                            arguments_display: arguments,
                                            args_value: &args_value,
                                            req,
                                            cancel_flag: cancel_flag.clone(),
                                        },
                                    )
                                    .await
                                    {
                                        Ok(()) => continue,
                                        Err(e) => {
                                            let _ = app.emit(
                                                &format!("chat-stream-{}", message_id),
                                                &StreamOutputItem::ToolResult {
                                                    tool_use_id: tool_use_id.clone(),
                                                    name: tool_name.clone(),
                                                    input: arguments.clone(),
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
                                        skill = %skill_display,
                                        "Skill allowed by permission manager"
                                    );
                                    break;
                                }
                            }
                        }
                        // === End permission check ===

                        // Determine execution mode
                        let execution_mode = args.execution_mode.trim().to_lowercase();
                        let is_forked = execution_mode == "forked";

                        if is_forked {
                            // === FORKED EXECUTION MODE ===
                            // Execute skill in isolated sub-agent session
                            tracing::info!(
                                skill = %skill_display,
                                "Executing skill in forked mode (isolated sub-agent)"
                            );

                            // Load skill content for forked execution
                            let skill_content = resolved_name
                                .as_ref()
                                .and_then(|name| {
                                    skills::find_skill_entry(&all_skills, name).map(|entry| {
                                        let skill_path = entry.skill_dir.join("SKILL.md");
                                        std::fs::read_to_string(&skill_path).unwrap_or_else(|_| {
                                            format!(
                                                "# {}\n\nSkill content not available",
                                                entry.name
                                            )
                                        })
                                    })
                                })
                                .unwrap_or_else(|| {
                                    format!("Skill: {}\n\nArgs: {}", args.skill, args.args)
                                });

                            // Get allowed_tools for forked execution
                            let skill_allowed_tools: Option<Vec<String>> =
                                resolved_name.as_ref().and_then(|name| {
                                    skills::find_skill_entry(&all_skills, name)
                                        .map(|entry| entry.allowed_tools.clone())
                                });

                            // Need Agent runtime for forked execution
                            if let Some(ar) = agent_runtime {
                                match run_skill_forked(ForkedSkillRequest {
                                    app: &app,
                                    message_id,
                                    session_id,
                                    tool_results_dir,
                                    project_root,
                                    session_todos: session_todos.clone(),
                                    session_agent_tasks: session_agent_tasks.clone(),
                                    skill_name: &skill_display,
                                    skill_args: &args.args,
                                    skill_content: &skill_content,
                                    allowed_tools: skill_allowed_tools,
                                    runtime: ar,
                                    subagent_execute_depth: subagent_depth.saturating_add(1),
                                    web_search_api_keys: web_search_api_keys.clone(),
                                    skill_cache: skill_cache.clone(),
                                })
                                .await
                                {
                                    Ok(output_text) => {
                                        let is_error = false;
                                        tracing::info!(
                                            skill = %skill_display,
                                            output_len = output_text.len(),
                                            "Skill forked execution completed"
                                        );
                                        // Process and return result (similar to existing inline result handling)
                                        let display_output =
                                            if output_text.len() > PREVIEW_SIZE_BYTES {
                                                let prefix = truncate_utf8_prefix(
                                                    &output_text,
                                                    PREVIEW_SIZE_BYTES,
                                                );
                                                format!(
                                                "{}\n\n[Output truncated... {} total characters]",
                                                prefix,
                                                output_text.len()
                                            )
                                            } else {
                                                output_text.clone()
                                            };
                                        let display_input =
                                            if arguments.len() > TOOL_DISPLAY_MAX_INPUT_CHARS {
                                                let prefix = truncate_utf8_prefix(
                                                    arguments,
                                                    TOOL_DISPLAY_MAX_INPUT_CHARS,
                                                );
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
                                            output_text,
                                            tool_use_id,
                                            tool_results_dir,
                                        )
                                        .await;
                                        return (tool_use_id.clone(), model_output, is_error);
                                    }
                                    Err(e) => {
                                        let error_msg =
                                            format!("Skill forked execution failed: {}", e);
                                        tracing::warn!(
                                            skill = %skill_display,
                                            error = %error_msg,
                                            "Forked execution error"
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
                                }
                            } else {
                                // No agent runtime available - fall back to inline
                                tracing::warn!(
                                    skill = %skill_display,
                                    "Forked mode requested but no agent runtime available, falling back to inline"
                                );
                                // Continue to inline execution below
                            }
                        }

                        // === INLINE EXECUTION MODE (default) ===
                        // Use invoke_skill_detailed_with_cache so we can inspect `status`
                        // and route `needs_fork` skills to the forked sub-agent path.
                        let skill_invoke_result = skills::invoke_skill_detailed_with_cache(
                            project_root,
                            &args.skill,
                            &args.args,
                            Some(&all_skills),
                        )
                        .await;

                        // If the skill declares `context: fork` and we have an agent runtime,
                        // execute it as a forked sub-agent session.
                        if let Ok(ref detail) = skill_invoke_result {
                            if detail.status == "needs_fork" {
                                if let Some(ar) = agent_runtime {
                                    tracing::info!(
                                        skill = %skill_display,
                                        "Skill declares context:fork — routing to forked sub-agent"
                                    );
                                    let skill_content = detail
                                        .skill_body
                                        .clone()
                                        .unwrap_or_else(|| detail.formatted_tool_result.clone());
                                    let skill_allowed_tools = if detail.allowed_tools.is_empty() {
                                        None
                                    } else {
                                        Some(detail.allowed_tools.clone())
                                    };
                                    match run_skill_forked(ForkedSkillRequest {
                                        app: &app,
                                        message_id,
                                        session_id,
                                        tool_results_dir,
                                        project_root,
                                        session_todos: session_todos.clone(),
                                        session_agent_tasks: session_agent_tasks.clone(),
                                        skill_name: &skill_display,
                                        skill_args: &args.args,
                                        skill_content: &skill_content,
                                        allowed_tools: skill_allowed_tools,
                                        runtime: ar,
                                        subagent_execute_depth: subagent_depth.saturating_add(1),
                                        web_search_api_keys: web_search_api_keys.clone(),
                                        skill_cache: skill_cache.clone(),
                                    })
                                    .await
                                    {
                                        Ok(output_text) => {
                                            let task_id = uuid::Uuid::new_v4().to_string();
                                            let fork_result = serde_json::json!({
                                                "success": true,
                                                "commandName": skill_display,
                                                "status": "forked",
                                                "task_id": task_id,
                                                "message": format!(
                                                    "Skill '{}' executed as forked sub-agent. Output follows.",
                                                    skill_display
                                                ),
                                                "output": output_text,
                                            });
                                            let fork_result_str =
                                                serde_json::to_string_pretty(&fork_result)
                                                    .unwrap_or_else(|_| output_text.clone());
                                            tracing::info!(
                                                skill = %skill_display,
                                                output_len = fork_result_str.len(),
                                                "Skill fork execution completed"
                                            );
                                            let display_output = if fork_result_str.len()
                                                > PREVIEW_SIZE_BYTES
                                            {
                                                let prefix = truncate_utf8_prefix(
                                                    &fork_result_str,
                                                    PREVIEW_SIZE_BYTES,
                                                );
                                                format!(
                                                        "{}\n\n[Output truncated... {} total characters]",
                                                        prefix,
                                                        fork_result_str.len()
                                                    )
                                            } else {
                                                fork_result_str.clone()
                                            };
                                            let _ = app.emit(
                                                &format!("chat-stream-{}", message_id),
                                                &StreamOutputItem::ToolResult {
                                                    tool_use_id: tool_use_id.clone(),
                                                    name: tool_name.clone(),
                                                    input: arguments.clone(),
                                                    output: display_output,
                                                    is_error: false,
                                                },
                                            );
                                            let model_output = process_tool_output_for_model(
                                                fork_result_str,
                                                tool_use_id,
                                                tool_results_dir,
                                            )
                                            .await;
                                            return (tool_use_id.clone(), model_output, false);
                                        }
                                        Err(e) => {
                                            let error_msg =
                                                format!("Skill fork execution failed: {}", e);
                                            tracing::warn!(
                                                skill = %skill_display,
                                                error = %error_msg,
                                                "Skill fork execution error"
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
                                    }
                                } else {
                                    // No agent runtime — fall through to inline with a note
                                    tracing::warn!(
                                        skill = %skill_display,
                                        "Skill declares context:fork but no agent runtime available; executing inline"
                                    );
                                }
                            }
                        }

                        match skill_invoke_result.map(|d| d.formatted_tool_result) {
                            Ok(output_text) => {
                                let is_error = false;
                                tracing::info!(
                                    tool = "skill",
                                    skill = %args.skill,
                                    output_len = output_text.len(),
                                    success = true,
                                    "Skill tool execution completed successfully"
                                );
                                let display_output = if output_text.len() > PREVIEW_SIZE_BYTES {
                                    let prefix =
                                        truncate_utf8_prefix(&output_text, PREVIEW_SIZE_BYTES);
                                    format!(
                                        "{}\n\n[Output truncated... {} total characters]",
                                        prefix,
                                        output_text.len()
                                    )
                                } else {
                                    output_text.clone()
                                };
                                let display_input =
                                    if arguments.len() > TOOL_DISPLAY_MAX_INPUT_CHARS {
                                        let prefix = truncate_utf8_prefix(
                                            arguments,
                                            TOOL_DISPLAY_MAX_INPUT_CHARS,
                                        );
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
                                let error_msg = e;
                                tracing::warn!(
                                    tool = "skill",
                                    skill = %args.skill,
                                    error = %error_msg,
                                    "Skill tool execution failed"
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
                                (tool_use_id.clone(), error_msg, true)
                            }
                        }
                    }
                }
            }
            Err(e) => {
                let error_msg = format!("skill tool: invalid JSON: {}", e);
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
        super::execute_domain_tool(ctx).await
    }
}
