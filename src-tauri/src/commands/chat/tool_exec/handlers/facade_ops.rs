use super::super::dispatch::ToolDispatchContext;
use super::super::*;

pub(super) async fn handle_computer_tool(ctx: &ToolDispatchContext<'_>) -> (String, String, bool) {
    let app = ctx.app.clone();
    let tool_use_id = ctx.tool_use_id;
    let tool_name = ctx.tool_name;
    let arguments = ctx.arguments;
    let message_id = ctx.message_id;
    let session_id = ctx.session_id;
    let tool_results_dir = ctx.tool_results_dir;
    let project_root = ctx.project_root;
    let computer_use_enabled = ctx.computer_use_enabled;

    if !computer_use_enabled {
        let error_msg = format!(
                "Computer Use facade tool `{tool_name}` is not enabled for this task. Enable Computer Use as `task` or `session` before using `computer_*` tools."
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
    let timeout = std::time::Duration::from_secs(120);
    let Some(facade_tool) =
        crate::domain::computer_use::ComputerFacadeTool::from_model_name(tool_name)
    else {
        let error_msg = format!("Unknown Computer Use facade tool: {tool_name}");
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

    let (mcp_manager, session_id_opt, settings_repo) = app
        .try_state::<crate::app_state::OmigaAppState>()
        .map(|s| {
            (
                Some(s.chat.mcp_manager.clone()),
                Some(session_id.to_string()),
                Some(s.repo.clone()),
            )
        })
        .unwrap_or((None, None, None));
    let settings_raw = if let Some(repo) = settings_repo {
        repo.get_setting(crate::domain::computer_use::SETTINGS_KEY)
            .await
            .ok()
            .flatten()
    } else {
        None
    };
    let computer_use_settings =
        crate::domain::computer_use::ComputerUseSettings::from_stored_json(settings_raw.as_deref());
    let _ = crate::domain::computer_use::prune_audit_retention(
        project_root,
        computer_use_settings.log_retention_days,
    );
    let mcp_pool_legacy: Option<
        std::sync::Arc<
            tokio::sync::Mutex<
                std::collections::HashMap<String, crate::domain::mcp::client::McpLiveConnection>,
            >,
        >,
    > = None;
    let mut prepared = match crate::domain::computer_use::prepare_facade_call(
        session_id,
        facade_tool,
        arguments,
    ) {
        Ok(prepared) => prepared,
        Err(error) => {
            crate::domain::computer_use::record_policy_rejection(
                project_root,
                session_id,
                facade_tool,
                arguments,
                &error,
            );
            let error_msg = error.model_output();
            let _ = app.emit(
                &format!("chat-stream-{}", message_id),
                &StreamOutputItem::ToolResult {
                    tool_use_id: tool_use_id.clone(),
                    name: tool_name.clone(),
                    input: crate::domain::computer_use::redact_json_value(
                        &serde_json::from_str::<serde_json::Value>(arguments)
                            .unwrap_or_else(|_| serde_json::json!({})),
                    )
                    .to_string(),
                    output: error_msg.clone(),
                    is_error: true,
                },
            );
            return (tool_use_id.clone(), error_msg, true);
        }
    };
    prepared.inject_settings(&computer_use_settings);

    if prepared.requires_backend_validate() {
        let validation_tool_name = prepared.validate_backend_tool_name();
        let validation_arguments = prepared.validate_backend_arguments_json();
        match crate::domain::mcp::tool_dispatch::execute_mcp_tool_call(
            project_root,
            &validation_tool_name,
            &validation_arguments,
            timeout,
            mcp_manager.clone(),
            mcp_pool_legacy.clone(),
            session_id_opt.clone(),
        )
        .await
        {
            Ok((validation_output_text, validation_is_error)) => {
                let validation_result =
                    serde_json::from_str::<serde_json::Value>(&validation_output_text)
                        .unwrap_or_else(|_| serde_json::json!({ "text": validation_output_text }));
                let safe_validation_result =
                    crate::domain::computer_use::sanitize_backend_result_for_model(
                        &validation_result,
                    );
                if let Some(violation) =
                    crate::domain::computer_use::app_policy_violation_from_backend_result(
                        &computer_use_settings,
                        &safe_validation_result,
                        true,
                    )
                {
                    let output_value = crate::domain::computer_use::app_not_allowed_output(
                        &prepared.run_id,
                        facade_tool,
                        &validation_tool_name,
                        &violation,
                    );
                    let output_text = output_value.to_string();
                    crate::domain::computer_use::record_facade_result(
                        project_root,
                        session_id,
                        &prepared,
                        false,
                        &output_value,
                    );
                    let _ = app.emit(
                        &format!("chat-stream-{}", message_id),
                        &StreamOutputItem::ToolResult {
                            tool_use_id: tool_use_id.clone(),
                            name: tool_name.clone(),
                            input: prepared.redacted_arguments.to_string(),
                            output: output_text.clone(),
                            is_error: true,
                        },
                    );
                    let model_output = process_tool_output_for_model(
                        output_text.clone(),
                        tool_use_id,
                        tool_results_dir,
                    )
                    .await;
                    return (tool_use_id.clone(), model_output, true);
                }
                if validation_is_error
                    || !crate::domain::computer_use::backend_validation_allows_action(
                        &safe_validation_result,
                    )
                {
                    let output_text = serde_json::json!({
                        "ok": false,
                        "error": "target_validation_failed",
                        "requiresObserve": true,
                        "runId": prepared.run_id,
                        "facadeTool": facade_tool.model_name(),
                        "backendTool": validation_tool_name,
                        "backendResult": safe_validation_result,
                    })
                    .to_string();
                    crate::domain::computer_use::record_facade_result(
                        project_root,
                        session_id,
                        &prepared,
                        false,
                        &serde_json::from_str::<serde_json::Value>(&output_text)
                            .unwrap_or_else(|_| serde_json::json!({ "ok": false })),
                    );
                    let _ = app.emit(
                        &format!("chat-stream-{}", message_id),
                        &StreamOutputItem::ToolResult {
                            tool_use_id: tool_use_id.clone(),
                            name: tool_name.clone(),
                            input: prepared.redacted_arguments.to_string(),
                            output: output_text.clone(),
                            is_error: true,
                        },
                    );
                    let model_output = process_tool_output_for_model(
                        output_text.clone(),
                        tool_use_id,
                        tool_results_dir,
                    )
                    .await;
                    return (tool_use_id.clone(), model_output, true);
                }
            }
            Err(e) => {
                let error_msg = format!("Computer Use target validation error: {e}");
                crate::domain::computer_use::record_facade_result(
                    project_root,
                    session_id,
                    &prepared,
                    false,
                    &serde_json::json!({ "error": error_msg }),
                );
                let _ = app.emit(
                    &format!("chat-stream-{}", message_id),
                    &StreamOutputItem::ToolResult {
                        tool_use_id: tool_use_id.clone(),
                        name: tool_name.clone(),
                        input: prepared.redacted_arguments.to_string(),
                        output: error_msg.clone(),
                        is_error: true,
                    },
                );
                return (tool_use_id.clone(), error_msg, true);
            }
        }
    }

    match crate::domain::mcp::tool_dispatch::execute_mcp_tool_call(
        project_root,
        &prepared.backend_tool_name,
        &prepared.backend_arguments_json,
        timeout,
        mcp_manager,
        mcp_pool_legacy,
        session_id_opt,
    )
    .await
    {
        Ok((backend_output_text, backend_is_error)) => {
            let backend_result = serde_json::from_str::<serde_json::Value>(&backend_output_text)
                .unwrap_or_else(|_| serde_json::json!({ "text": backend_output_text }));
            let safe_backend_result =
                crate::domain::computer_use::sanitize_backend_result_for_model(&backend_result);
            if let Some(violation) =
                crate::domain::computer_use::app_policy_violation_from_backend_result(
                    &computer_use_settings,
                    &safe_backend_result,
                    facade_tool.backend_result_requires_target_identity(),
                )
            {
                let output_value = crate::domain::computer_use::app_not_allowed_output(
                    &prepared.run_id,
                    facade_tool,
                    &prepared.backend_tool_name,
                    &violation,
                );
                let output_text = output_value.to_string();
                let _ = app.emit(
                    &format!("chat-stream-{}", message_id),
                    &StreamOutputItem::ToolResult {
                        tool_use_id: tool_use_id.clone(),
                        name: tool_name.clone(),
                        input: prepared.redacted_arguments.to_string(),
                        output: output_text.clone(),
                        is_error: true,
                    },
                );
                crate::domain::computer_use::record_facade_result(
                    project_root,
                    session_id,
                    &prepared,
                    false,
                    &output_value,
                );
                let model_output = process_tool_output_for_model(
                    output_text.clone(),
                    tool_use_id,
                    tool_results_dir,
                )
                .await;
                return (tool_use_id.clone(), model_output, true);
            }
            let output_text = serde_json::json!({
                "ok": !backend_is_error,
                "facadeTool": facade_tool.model_name(),
                "runId": prepared.run_id,
                "backendTool": prepared.backend_tool_name,
                "backendResult": safe_backend_result,
            })
            .to_string();
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
            let safe_input = prepared.redacted_arguments.to_string();
            let display_input = if safe_input.len() > TOOL_DISPLAY_MAX_INPUT_CHARS {
                let prefix = truncate_utf8_prefix(&safe_input, TOOL_DISPLAY_MAX_INPUT_CHARS);
                format!(
                    "{}\n\n[Input truncated... {} total characters]",
                    prefix,
                    safe_input.len()
                )
            } else {
                safe_input
            };
            let _ = app.emit(
                &format!("chat-stream-{}", message_id),
                &StreamOutputItem::ToolResult {
                    tool_use_id: tool_use_id.clone(),
                    name: tool_name.clone(),
                    input: display_input,
                    output: display_output,
                    is_error: backend_is_error,
                },
            );
            crate::domain::computer_use::record_facade_result(
                project_root,
                session_id,
                &prepared,
                !backend_is_error,
                &safe_backend_result,
            );
            let model_output =
                process_tool_output_for_model(output_text.clone(), tool_use_id, tool_results_dir)
                    .await;
            (tool_use_id.clone(), model_output, backend_is_error)
        }
        Err(e) => {
            let error_msg = format!(
                    "Computer Use backend error: {e}. Install and enable the `computer-use` plugin, then retry with Computer Use enabled."
                );
            crate::domain::computer_use::record_facade_result(
                project_root,
                session_id,
                &prepared,
                false,
                &serde_json::json!({ "error": error_msg }),
            );
            let _ = app.emit(
                &format!("chat-stream-{}", message_id),
                &StreamOutputItem::ToolResult {
                    tool_use_id: tool_use_id.clone(),
                    name: tool_name.clone(),
                    input: prepared.redacted_arguments.to_string(),
                    output: error_msg.clone(),
                    is_error: true,
                },
            );
            (tool_use_id.clone(), error_msg, true)
        }
    }
}

pub(super) async fn handle_browser_tool(ctx: &ToolDispatchContext<'_>) -> (String, String, bool) {
    let app = ctx.app.clone();
    let tool_use_id = ctx.tool_use_id;
    let tool_name = ctx.tool_name;
    let arguments = ctx.arguments;
    let message_id = ctx.message_id;
    let session_id = ctx.session_id;
    let tool_results_dir = ctx.tool_results_dir;
    let browser_use_enabled = ctx.browser_use_enabled;

    if !browser_use_enabled {
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
    let Some(facade_tool) =
        crate::domain::browser_operator::BrowserFacadeTool::from_model_name(tool_name)
    else {
        let args_value: serde_json::Value = serde_json::from_str(arguments)
            .unwrap_or_else(|_| serde_json::json!({"raw": arguments}));
        let display_input =
            crate::domain::browser_operator::redact_arguments_for_display(&args_value).to_string();
        let error_msg = format!("Unknown Browser Operator facade tool: {tool_name}");
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
    let Some(app_state) = app.try_state::<crate::app_state::OmigaAppState>() else {
        let args_value: serde_json::Value = serde_json::from_str(arguments)
            .unwrap_or_else(|_| serde_json::json!({"raw": arguments}));
        let display_input =
            crate::domain::browser_operator::redact_arguments_for_display(&args_value).to_string();
        let error_msg = "Browser Operator manager is unavailable in app state.".to_string();
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
    let manager = app_state.chat.browser_operator_manager.clone();
    let execution = crate::domain::browser_operator::execute_facade_tool(
        manager.as_ref(),
        session_id,
        facade_tool,
        arguments,
    )
    .await;
    let output_text = execution.output.to_string();
    let safe_input = execution.redacted_arguments.to_string();
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
    let display_input = if safe_input.len() > TOOL_DISPLAY_MAX_INPUT_CHARS {
        let prefix = truncate_utf8_prefix(&safe_input, TOOL_DISPLAY_MAX_INPUT_CHARS);
        format!(
            "{}\n\n[Input truncated... {} total characters]",
            prefix,
            safe_input.len()
        )
    } else {
        safe_input
    };
    let _ = app.emit(
        &format!("chat-stream-{}", message_id),
        &StreamOutputItem::ToolResult {
            tool_use_id: tool_use_id.clone(),
            name: tool_name.clone(),
            input: display_input,
            output: display_output,
            is_error: execution.is_error,
        },
    );
    let model_output =
        process_tool_output_for_model(output_text.clone(), tool_use_id, tool_results_dir).await;
    (tool_use_id.clone(), model_output, execution.is_error)
}
