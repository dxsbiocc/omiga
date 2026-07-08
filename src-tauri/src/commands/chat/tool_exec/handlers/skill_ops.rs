use super::super::dispatch::ToolDispatchContext;
use super::super::*;

pub(super) fn is_skill_tool(tool_name: &str) -> bool {
    tool_name.eq_ignore_ascii_case("list_skills")
        || tool_name.eq_ignore_ascii_case("skills_list")
        || tool_name.eq_ignore_ascii_case("skill_view")
        || tool_name.eq_ignore_ascii_case("skill_manage")
        || tool_name.eq_ignore_ascii_case("skill_config")
        || tool_name.eq_ignore_ascii_case("skill")
        || tool_name == "Skill"
}

/// Parse the `allowedTools` array from an inline skill's output JSON metadata header.
///
/// Skill output format (text, not Rust):
/// `Launching skill: NAME\n\n{ "success": true, "allowedTools": ["bash", "file_read"], ... }\n\n---\n\n[body]`
pub(in crate::commands::chat::tool_exec) fn extract_skill_allowed_tools(
    output: &str,
) -> Option<Vec<String>> {
    let json_start = output.find('{')?;
    let separator = "\n\n---";
    let json_end = output.find(separator).unwrap_or(output.len());
    let json_str = output.get(json_start..json_end)?;
    let val: serde_json::Value = serde_json::from_str(json_str).ok()?;
    let arr = val.get("allowedTools")?.as_array()?;
    let allowed: Vec<String> = arr
        .iter()
        .filter_map(|v| v.as_str().map(str::to_owned))
        .collect();
    if allowed.is_empty() {
        None
    } else {
        Some(allowed)
    }
}

pub(super) async fn handle_skill_tool(ctx: &ToolDispatchContext<'_>) -> (String, String, bool) {
    let app = ctx.app.clone();
    let tool_use_id = ctx.tool_use_id;
    let tool_name = ctx.tool_name;
    let arguments = ctx.arguments;
    let message_id = ctx.message_id;
    let tool_results_dir = ctx.tool_results_dir;
    let project_root = ctx.project_root;
    let skill_task_context = ctx.skill_task_context;
    let skill_cache = ctx.skill_cache.clone();

    if tool_name.eq_ignore_ascii_case("list_skills")
        || tool_name.eq_ignore_ascii_case("skills_list")
    {
        let args: ListSkillsArgs = if arguments.trim().is_empty() {
            ListSkillsArgs::default()
        } else {
            serde_json::from_str(arguments).unwrap_or_default()
        };
        let icfg = integrations_config::load_integrations_config(project_root);
        let mut all_skills = skills::load_skills_cached(project_root, &skill_cache).await;
        let total_skills_before_filter = all_skills.len();
        all_skills = integrations_config::filter_skill_entries(all_skills, &icfg);
        let filtered_count = all_skills.len();

        // Telemetry for list_skills / skills_list (aligned with SkillTool telemetry)
        tracing::info!(
            tool = %tool_name,
            query = ?args.query,
            has_task_context = skill_task_context.is_some(),
            total_skills = total_skills_before_filter,
            after_filter = filtered_count,
            disabled_count = total_skills_before_filter - filtered_count,
            "list skills tool invoked"
        );

        let json = skills::list_skills_metadata_json(
            &all_skills,
            args.query.as_deref(),
            skill_task_context,
        );
        let is_error = false;
        let display_output = if json.len() > PREVIEW_SIZE_BYTES {
            let prefix = truncate_utf8_prefix(&json, PREVIEW_SIZE_BYTES);
            format!(
                "{}\n\n[Output truncated... {} total characters]",
                prefix,
                json.len()
            )
        } else {
            json.clone()
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
        let model_output = process_tool_output_for_model(json, tool_use_id, tool_results_dir).await;
        (tool_use_id.clone(), model_output, is_error)
    } else if tool_name.eq_ignore_ascii_case("skill_view") {
        match serde_json::from_str::<SkillViewArgs>(arguments) {
            Err(e) => {
                let error_msg = format!("skill_view: invalid JSON: {e}");
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
            Ok(args) => {
                if args.skill.trim().is_empty() {
                    let error_msg =
                        "skill_view: missing or empty `skill` (or `name`) field".to_string();
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
                    let mut all_skills =
                        skills::load_skills_cached(project_root, &skill_cache).await;
                    all_skills = integrations_config::filter_skill_entries(all_skills, &icfg);
                    match skills::execute_skill_view(
                        &all_skills,
                        args.skill.trim(),
                        args.file_path.as_deref(),
                    )
                    .await
                    {
                        Ok(json_val) => {
                            let json = serde_json::to_string_pretty(&json_val)
                                .unwrap_or_else(|_| "{\"success\":false}".to_string());
                            let is_error = false;
                            let display_output = if json.len() > PREVIEW_SIZE_BYTES {
                                let prefix = truncate_utf8_prefix(&json, PREVIEW_SIZE_BYTES);
                                format!(
                                    "{}\n\n[Output truncated... {} total characters]",
                                    prefix,
                                    json.len()
                                )
                            } else {
                                json.clone()
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
                            let model_output =
                                process_tool_output_for_model(json, tool_use_id, tool_results_dir)
                                    .await;
                            (tool_use_id.clone(), model_output, is_error)
                        }
                        Err(e) => {
                            tracing::warn!(tool = "skill_view", error = %e, "skill_view failed");
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
                            (tool_use_id.clone(), e, true)
                        }
                    }
                }
            }
        }
    } else if tool_name.eq_ignore_ascii_case("skill_manage") {
        let out = skills::execute_skill_manage(project_root, arguments, &skill_cache).await;
        match out {
            Ok(json_val) => {
                let json = serde_json::to_string_pretty(&json_val)
                    .unwrap_or_else(|_| "{\"success\":false}".to_string());
                let is_error = false;
                let display_output = if json.len() > PREVIEW_SIZE_BYTES {
                    let prefix = truncate_utf8_prefix(&json, PREVIEW_SIZE_BYTES);
                    format!(
                        "{}\n\n[Output truncated... {} total characters]",
                        prefix,
                        json.len()
                    )
                } else {
                    json.clone()
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
                    process_tool_output_for_model(json, tool_use_id, tool_results_dir).await;
                (tool_use_id.clone(), model_output, is_error)
            }
            Err(e) => {
                tracing::warn!(tool = "skill_manage", error = %e, "skill_manage failed");
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
                (tool_use_id.clone(), e, true)
            }
        }
    } else if tool_name.eq_ignore_ascii_case("skill_config") {
        let result = handle_skill_config(project_root, arguments, &skill_cache).await;
        let (json, is_error) = match result {
            Ok(v) => (
                serde_json::to_string_pretty(&v)
                    .unwrap_or_else(|_| "{\"success\":false}".to_string()),
                false,
            ),
            Err(e) => {
                tracing::warn!(tool = "skill_config", error = %e, "skill_config failed");
                (e, true)
            }
        };
        let display_output = if json.len() > PREVIEW_SIZE_BYTES {
            let prefix = truncate_utf8_prefix(&json, PREVIEW_SIZE_BYTES);
            format!(
                "{}\n\n[Output truncated... {} total characters]",
                prefix,
                json.len()
            )
        } else {
            json.clone()
        };
        let _ = app.emit(
            &format!("chat-stream-{}", message_id),
            &StreamOutputItem::ToolResult {
                tool_use_id: tool_use_id.clone(),
                name: tool_name.clone(),
                input: arguments.clone(),
                output: display_output,
                is_error,
            },
        );
        let model_output = process_tool_output_for_model(json, tool_use_id, tool_results_dir).await;
        (tool_use_id.clone(), model_output, is_error)
    } else if tool_name.eq_ignore_ascii_case("skill") || tool_name == "Skill" {
        super::skill_invoke_ops::handle_skill_invoke(ctx).await
    } else {
        super::execute_domain_tool(ctx).await
    }
}
