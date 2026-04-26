//! Subagent session execution, skill forking, and background agent management.

use super::tool_exec::execute_tool_calls;
use super::{
    api_messages_to_llm, augment_llm_messages_with_runtime_constraints, completed_to_tool_calls,
    run_post_response_retry_text_only, stream_llm_response_with_cancel, AgentLlmRuntime,
    MAX_SUBAGENT_TOOL_ROUNDS,
};
use crate::constants::agent_prompt;
use crate::domain::agents::scheduler::AgentSelector;
use crate::domain::agents::subagent_tool_filter::{
    filter_tool_schemas_for_subagent, SubagentFilterOptions,
};
use crate::domain::integrations_config;
use crate::domain::permissions::load_merged_permission_deny_rule_entries;
use crate::domain::permissions::{
    filter_tool_schemas_by_deny_rule_entries, validate_permission_deny_entries,
};
use crate::domain::runtime_constraints::{
    RuntimeConstraintHarness, RuntimeConstraintState, ToolConstraintContext,
};
use crate::domain::session::SessionCodec;
use crate::domain::session::{AgentTask, Message, TodoItem};
use crate::domain::skills;
use crate::domain::tools::{all_tool_schemas, ToolContext, ToolSchema, WebSearchApiKeys};
use crate::llm::{create_client, LlmConfig, LlmProvider};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex as StdMutex};
use tauri::AppHandle;
use tokio::sync::{Mutex, RwLock};
pub(super) fn is_agent_tool_name(name: &str) -> bool {
    matches!(name, "Agent" | "Task" | "agent" | "task")
}

/// Parity with TS `getAgentModel` (`src/utils/model/agent.ts`): env override, `inherit`, and
/// `aliasMatchesParentTier` (sonnet/opus/haiku inherits parent's exact model id when same tier).
pub(super) fn resolve_subagent_model(base: &LlmConfig, alias: Option<&str>) -> String {
    if let Ok(env_override) = std::env::var("CLAUDE_CODE_SUBAGENT_MODEL") {
        let t = env_override.trim();
        if !t.is_empty() {
            return t.to_string();
        }
    }
    if let Ok(env_override) = std::env::var("OMIGA_SUBAGENT_MODEL") {
        let t = env_override.trim();
        if !t.is_empty() {
            return t.to_string();
        }
    }
    let Some(a) = alias.map(str::trim).filter(|s| !s.is_empty()) else {
        return base.model.clone();
    };
    if a.eq_ignore_ascii_case("inherit") {
        return base.model.clone();
    }
    let parent = base.model.as_str();
    if subagent_alias_matches_parent_tier(a, parent) {
        return base.model.clone();
    }
    let a_lower = a.to_ascii_lowercase();
    if base.provider == LlmProvider::Anthropic {
        if a_lower == "sonnet" || a_lower == "claude-sonnet" {
            return "claude-sonnet-4-20250514".to_string();
        }
        if a_lower == "opus" || a_lower == "claude-opus" {
            return "claude-opus-4-20250514".to_string();
        }
        if a_lower == "haiku" || a_lower == "claude-haiku" {
            return "claude-haiku-4-20250514".to_string();
        }
        if a.starts_with("claude-") {
            return a.to_string();
        }
    }
    if a.len() > 6 && (a.contains('-') || a.contains('/') || a.contains('.')) {
        return a.to_string();
    }
    base.model.clone()
}

pub(super) fn subagent_alias_matches_parent_tier(alias: &str, parent_model: &str) -> bool {
    let p = parent_model.to_ascii_lowercase();
    match alias.to_ascii_lowercase().as_str() {
        "opus" | "claude-opus" => p.contains("opus"),
        "sonnet" | "claude-sonnet" => p.contains("sonnet"),
        "haiku" | "claude-haiku" => p.contains("haiku"),
        _ => false,
    }
}

pub(super) fn resolve_agent_cwd(project_root: &Path, cwd: Option<&str>) -> PathBuf {
    let Some(raw) = cwd.map(str::trim).filter(|s| !s.is_empty()) else {
        return project_root.to_path_buf();
    };
    if raw.starts_with("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home).join(raw.trim_start_matches("~/"));
        }
    }
    if raw.starts_with('/') {
        return PathBuf::from(raw);
    }
    project_root.join(raw)
}

pub(super) async fn build_subagent_tool_schemas(
    project_root: &Path,
    include_skill: bool,
    subagent_opts: SubagentFilterOptions,
) -> Vec<ToolSchema> {
    let integrations_cfg = integrations_config::load_integrations_config(project_root);
    let deny_entries = load_merged_permission_deny_rule_entries(project_root);
    validate_permission_deny_entries(&deny_entries);
    let built =
        filter_tool_schemas_by_deny_rule_entries(all_tool_schemas(include_skill), &deny_entries);
    let mut built = filter_tool_schemas_for_subagent(built, subagent_opts);
    built.sort_by(|a, b| a.name.cmp(&b.name));
    let base_names: HashSet<String> = built.iter().map(|t| t.name.clone()).collect();
    let mcp_timeout = std::time::Duration::from_secs(45);
    let mcp_tools =
        crate::domain::mcp::tool_pool::discover_mcp_tool_schemas(project_root, mcp_timeout).await;
    let mcp_after_deny = filter_tool_schemas_by_deny_rule_entries(mcp_tools, &deny_entries);
    let mcp_filtered: Vec<_> = mcp_after_deny
        .into_iter()
        .filter(|t| !base_names.contains(&t.name))
        .collect();
    let mcp_filtered =
        integrations_config::filter_mcp_tools_by_integrations(mcp_filtered, &integrations_cfg);
    built.into_iter().chain(mcp_filtered).collect()
}

/// Forked skill execution - runs skill in isolated sub-agent session.
///
/// Unlike inline execution which modifies current session state,
/// forked execution creates an isolated context for the skill.
pub(super) async fn run_skill_forked(
    app: &AppHandle,
    message_id: &str,
    session_id: &str,
    tool_results_dir: &Path,
    project_root: &Path,
    session_todos: Option<Arc<Mutex<Vec<TodoItem>>>>,
    session_agent_tasks: Option<Arc<Mutex<Vec<AgentTask>>>>,
    skill_name: &str,
    skill_args: &str,
    skill_content: &str,
    allowed_tools: Option<Vec<String>>,
    runtime: &AgentLlmRuntime,
    subagent_execute_depth: u8,
    web_search_api_keys: WebSearchApiKeys,
    skill_cache: Arc<StdMutex<skills::SkillCacheMap>>,
) -> Result<String, String> {
    // Build subagent configuration
    let mut sub_cfg = runtime.llm_config.clone();

    // Fast existence check for skills
    let skills_exist = skills::skills_any_exist(project_root, &skill_cache).await;

    // Build system prompt with skill content
    let mut prompt_parts: Vec<String> = Vec::new();
    prompt_parts.push(agent_prompt::build_system_prompt(
        project_root,
        &sub_cfg.model,
    ));
    if let Some(overlay) = crate::domain::agents::build_runtime_overlay(project_root).await {
        prompt_parts.push(overlay);
    }

    // Add skill-specific system prompt section
    let skill_system_prompt = format!(
        "## Skill Mode: {skill_name}\n\
        You are executing the '{skill_name}' skill in forked/isolated mode. \
        The skill content has been loaded below. \
        You have access to tools as specified. \
        Execute the task and return results.\n\n\
        ### Skill Content\n```markdown\n{skill_content}\n```",
    );
    prompt_parts.push(skill_system_prompt);

    if let Some(ref u) = sub_cfg.system_prompt {
        let t = u.trim();
        if !t.is_empty() {
            prompt_parts.push(t.to_string());
        }
    }
    if skills_exist {
        let loaded = skills::load_skills_cached(project_root, &skill_cache).await;
        prompt_parts.push(skills::format_skills_index_system_section(
            project_root,
            &loaded,
        ));
    }
    sub_cfg.system_prompt = Some(prompt_parts.join("\n\n"));

    let client = create_client(sub_cfg).map_err(|e| e.to_string())?;

    // Determine parent plan mode
    let parent_in_plan = if let Some(ref pm) = runtime.plan_mode_flag {
        *pm.lock().await
    } else {
        false
    };

    let subagent_opts = SubagentFilterOptions {
        parent_in_plan_mode: parent_in_plan,
        allow_nested_agent: runtime.allow_nested_agent,
    };

    // Build tool schemas - respect skill's allowed_tools
    let mut tools = build_subagent_tool_schemas(project_root, skills_exist, subagent_opts).await;

    // Filter tools based on skill's allowed_tools
    if let Some(ref allowed) = allowed_tools {
        if !allowed.is_empty() {
            let allowed_set: std::collections::HashSet<_> = allowed.iter().cloned().collect();
            tools.retain(|t| allowed_set.contains(&t.name));
        }
    }

    // Build user prompt from skill arguments
    let user_text = format!(
        "## Skill Task: {skill_name}\n\nExecute the skill with the following arguments:\n```\n{skill_args}\n```",
    );

    let mut transcript: Vec<Message> = vec![Message::User { content: user_text }];
    let subagent_skill_task_context = format!("{} {}", skill_name, skill_args);
    let constraint_harness =
        RuntimeConstraintHarness::from_config(runtime.runtime_constraints_config.clone());
    let mut constraint_state = RuntimeConstraintState::default();

    // Execute sub-agent loop (similar to run_subagent_session)
    for _round_idx in 0..MAX_SUBAGENT_TOOL_ROUNDS {
        if *runtime.cancel_flag.read().await {
            return Err("Skill execution cancelled.".to_string());
        }

        let api_msgs = SessionCodec::to_api_messages(&transcript);
        let llm_messages = api_messages_to_llm(&api_msgs);
        let (constrained_messages, _notice_ids) = augment_llm_messages_with_runtime_constraints(
            &llm_messages,
            &constraint_harness,
            &mut constraint_state,
            &subagent_skill_task_context,
            project_root,
            true,
            true,
        );

        let (tool_calls, assistant_text, reasoning_text, cancelled, _) =
            stream_llm_response_with_cancel(
                client.as_ref(),
                app,
                message_id,
                &runtime.round_id,
                &constrained_messages,
                &tools,
                true,
                &runtime.pending_tools,
                &runtime.cancel_flag,
                runtime.repo.clone(),
            )
            .await
            .map_err(|e| e.to_string())?;

        if cancelled {
            return Err("Skill execution cancelled.".to_string());
        }

        let pending_tool_names: Vec<String> =
            tool_calls.iter().map(|(_, name, _)| name.clone()).collect();
        if let Some(block) = constraint_harness.tool_gate(
            &ToolConstraintContext {
                request_text: &subagent_skill_task_context,
                assistant_text: &assistant_text,
                pending_tool_names: &pending_tool_names,
                is_subagent: true,
            },
            &constraint_state,
        ) {
            constraint_state.mark_clarification_requested();
            let tc = completed_to_tool_calls(&tool_calls);
            transcript.push(Message::Assistant {
                content: assistant_text,
                tool_calls: tc,
                token_usage: None,
                reasoning_content: (!reasoning_text.is_empty()).then_some(reasoning_text),
                follow_up_suggestions: None,
                turn_summary: None,
            });
            for (tool_use_id, _name, _arguments) in &tool_calls {
                transcript.push(Message::Tool {
                    tool_call_id: tool_use_id.clone(),
                    output: block.tool_result_message.clone(),
                });
            }
            transcript.push(Message::Assistant {
                content: block.assistant_response.clone(),
                tool_calls: None,
                token_usage: None,
                reasoning_content: None,
                follow_up_suggestions: None,
                turn_summary: None,
            });
            return Ok(block.assistant_response);
        }

        let tc = completed_to_tool_calls(&tool_calls);
        transcript.push(Message::Assistant {
            content: assistant_text.clone(),
            tool_calls: tc.clone(),
            token_usage: None,
            reasoning_content: (!reasoning_text.is_empty()).then(|| reasoning_text.clone()),
            follow_up_suggestions: None,
            turn_summary: None,
        });

        if tool_calls.is_empty() {
            let no_pending_tool_names: Vec<String> = Vec::new();
            if let Some(action) = constraint_harness.post_response_action(
                &crate::domain::runtime_constraints::PostResponseConstraintContext {
                    request_text: &subagent_skill_task_context,
                    assistant_text: &assistant_text,
                    pending_tool_names: &no_pending_tool_names,
                    is_subagent: true,
                },
                &constraint_state,
            ) {
                constraint_state.mark_post_action_attempted(action.id);
                let (retry_messages, _retry_notice_ids) =
                    augment_llm_messages_with_runtime_constraints(
                        &api_messages_to_llm(&SessionCodec::to_api_messages(&transcript)),
                        &constraint_harness,
                        &mut constraint_state,
                        &subagent_skill_task_context,
                        project_root,
                        true,
                        true,
                    );
                let (retry_text, retry_reasoning, _) = run_post_response_retry_text_only(
                    client.as_ref(),
                    app,
                    message_id,
                    &runtime.round_id,
                    &retry_messages,
                    &action.instruction,
                    &runtime.pending_tools,
                    &runtime.cancel_flag,
                    runtime.repo.clone(),
                )
                .await
                .map_err(|e| e.to_string())?;
                if !retry_text.trim().is_empty() {
                    transcript.push(Message::Assistant {
                        content: retry_text.clone(),
                        tool_calls: None,
                        token_usage: None,
                        reasoning_content: (!retry_reasoning.is_empty()).then_some(retry_reasoning),
                        follow_up_suggestions: None,
                        turn_summary: None,
                    });
                    return Ok(retry_text);
                }
            }
            return Ok(assistant_text);
        }

        constraint_state.record_tool_names(tool_calls.iter().map(|(_, name, _)| name.as_str()));

        // Execute tool calls
        let results = execute_tool_calls(
            &tool_calls,
            app,
            message_id,
            session_id,
            tool_results_dir,
            project_root,
            session_todos.clone(),
            session_agent_tasks.clone(),
            Some(runtime),
            subagent_execute_depth,
            Some(subagent_skill_task_context.as_str()),
            web_search_api_keys.clone(),
            skill_cache.clone(),
            runtime.execution_environment.clone(),
            runtime.ssh_server.clone(),
            runtime.sandbox_backend.clone(),
            runtime.local_venv_type.clone(),
            runtime.local_venv_name.clone(),
            runtime.env_store.clone(),
        )
        .await;

        for (tool_use_id, output, _) in &results {
            transcript.push(Message::Tool {
                tool_call_id: tool_use_id.clone(),
                output: output.clone(),
            });
        }
    }

    Err(format!(
        "Skill execution exceeded maximum tool rounds ({MAX_SUBAGENT_TOOL_ROUNDS})."
    ))
}

/// Shared foreground ReAct loop for `Agent` tool (main-thread and background worker).
pub(super) async fn run_subagent_session_foreground_inner(
    app: &AppHandle,
    message_id: &str,
    session_id: &str,
    tool_results_dir: &Path,
    effective_root: &Path,
    session_todos: Option<Arc<Mutex<Vec<TodoItem>>>>,
    session_agent_tasks: Option<Arc<Mutex<Vec<AgentTask>>>>,
    args: &crate::domain::tools::agent::AgentArgs,
    runtime: &AgentLlmRuntime,
    subagent_execute_depth: u8,
    web_search_api_keys: WebSearchApiKeys,
    skill_cache: Arc<StdMutex<skills::SkillCacheMap>>,
    agent_def: &dyn crate::domain::agents::AgentDefinition,
    cancel_token: Option<&tokio_util::sync::CancellationToken>,
    background_task_id: Option<&str>,
) -> Result<String, String> {
    let subagent_skill_task_context = format!("{} {}", args.description.trim(), args.prompt.trim());

    let parent_in_plan = if let Some(ref pm) = runtime.plan_mode_flag {
        *pm.lock().await
    } else {
        false
    };

    let agent_model_config = agent_def.model();
    // When agent_def.model() is None, fall back to the agent's declared model_tier alias.
    // This implements the Frontier/Standard/Spark three-tier routing: Architect → opus,
    // Executor → sonnet, Explorer → haiku, others inherit the parent model.
    let effective_model_alias: Option<&str> = agent_model_config.or_else(|| {
        use crate::domain::agents::definition::ModelTier;
        match agent_def.model_tier() {
            ModelTier::Frontier => Some("opus"),
            ModelTier::Standard => None, // Standard inherits parent to avoid over-riding user choice
            ModelTier::Spark => Some("haiku"),
        }
    });
    let resolved_agent_model = if args
        .model
        .as_deref()
        .map(|m| !m.is_empty())
        .unwrap_or(false)
    {
        resolve_subagent_model(&runtime.llm_config, args.model.as_deref())
    } else if effective_model_alias
        .map(|m| m != "inherit")
        .unwrap_or(false)
    {
        resolve_subagent_model(&runtime.llm_config, effective_model_alias)
    } else {
        runtime.llm_config.model.clone()
    };

    let mut sub_cfg = runtime.llm_config.clone();
    sub_cfg.model = resolved_agent_model;

    let skills_exist = skills::skills_any_exist(effective_root, &skill_cache).await;

    let mut prompt_parts: Vec<String> = Vec::new();
    prompt_parts.push(agent_prompt::build_system_prompt(
        effective_root,
        &sub_cfg.model,
    ));
    if let Some(overlay) = crate::domain::agents::build_runtime_overlay(effective_root).await {
        prompt_parts.push(overlay);
    }

    let is_memory_agent = args
        .subagent_type
        .as_deref()
        .map(|t| {
            t.eq_ignore_ascii_case("memory-agent")
                || t.eq_ignore_ascii_case("memory_agent")
                || t.eq_ignore_ascii_case("wiki-agent")
                || t.eq_ignore_ascii_case("wiki_agent")
        })
        .unwrap_or(false);

    if is_memory_agent {
        let mem_cfg = crate::domain::memory::load_resolved_config(effective_root)
            .await
            .unwrap_or_default();
        prompt_parts.push(
            crate::domain::memory::memory_agent_system_prompt_with_config(effective_root, &mem_cfg),
        );
    } else {
        let tool_ctx = ToolContext::new(effective_root)
            .with_execution_environment(runtime.execution_environment.clone())
            .with_ssh_server(runtime.ssh_server.clone())
            .with_sandbox_backend(runtime.sandbox_backend.clone())
            .with_local_venv(
                runtime.local_venv_type.clone(),
                runtime.local_venv_name.clone(),
            );
        let agent_specific_prompt =
            crate::domain::agents::compose_full_agent_system_prompt(agent_def, &tool_ctx);

        let nested_agent_note = if runtime.allow_nested_agent {
            " Nested `Agent` is allowed."
        } else {
            ""
        };
        let exit_plan_note = if parent_in_plan {
            " `ExitPlanMode` is available while the parent session is in plan mode."
        } else {
            ""
        };

        let disallowed = agent_def
            .disallowed_tools()
            .map(|v| v.join(", "))
            .unwrap_or_else(|| "none".to_string());

        let subagent_mode_prompt = format!(
            "## Sub-agent mode ({})\nYou are an isolated sub-agent running as '{}'. \
             Use tools as needed. Disallowed tools: {}. \
             {}{}",
            agent_def.agent_type(),
            agent_def.agent_type(),
            disallowed,
            exit_plan_note,
            nested_agent_note
        );

        prompt_parts.push(agent_specific_prompt);
        prompt_parts.push(subagent_mode_prompt);
    }

    if let Some(ref u) = sub_cfg.system_prompt {
        let t = u.trim();
        if !t.is_empty() {
            prompt_parts.push(t.to_string());
        }
    }
    if skills_exist && !agent_def.omit_claude_md() {
        let loaded = skills::load_skills_cached(effective_root, &skill_cache).await;
        prompt_parts.push(skills::format_skills_index_system_section(
            effective_root,
            &loaded,
        ));
    }
    sub_cfg.system_prompt = Some(prompt_parts.join("\n\n"));
    let client = create_client(sub_cfg).map_err(|e| e.to_string())?;
    let subagent_opts = SubagentFilterOptions {
        parent_in_plan_mode: parent_in_plan,
        allow_nested_agent: runtime.allow_nested_agent,
    };
    let mut tools = build_subagent_tool_schemas(effective_root, skills_exist, subagent_opts).await;

    if let Some(ref allowed) = agent_def.allowed_tools() {
        let allowed_set: std::collections::HashSet<_> = allowed.iter().cloned().collect();
        tools.retain(|t| allowed_set.contains(&t.name));
    }
    if let Some(ref disallowed) = agent_def.disallowed_tools() {
        let disallowed_set: std::collections::HashSet<_> = disallowed.iter().cloned().collect();
        tools.retain(|t| !disallowed_set.contains(&t.name));
    }
    let user_text = format!(
        "## Sub-agent task: {}\n\n{}",
        args.description.trim(),
        args.prompt.trim()
    );
    let initial_user = Message::User { content: user_text };
    let mut transcript: Vec<Message> = vec![initial_user.clone()];
    let constraint_harness =
        RuntimeConstraintHarness::from_config(runtime.runtime_constraints_config.clone());
    let mut constraint_state = RuntimeConstraintState::default();
    if let Some(tid) = background_task_id {
        persist_background_transcript_message(&runtime.repo, tid, session_id, &initial_user).await;
    }

    for _round_idx in 0..MAX_SUBAGENT_TOOL_ROUNDS {
        if let Some(token) = cancel_token {
            if token.is_cancelled() {
                if let Some(tid) = background_task_id {
                    persist_background_cancel_notice(&runtime.repo, tid, session_id).await;
                }
                return Err("Sub-agent cancelled.".to_string());
            }
        }
        if let Some(tid) = background_task_id {
            let followups = crate::domain::agents::background::get_background_agent_manager()
                .drain_followups_for_task(tid)
                .await;
            for text in followups {
                let m = Message::User { content: text };
                persist_background_transcript_message(&runtime.repo, tid, session_id, &m).await;
                transcript.push(m);
            }
        }
        if *runtime.cancel_flag.read().await {
            if let Some(tid) = background_task_id {
                persist_background_cancel_notice(&runtime.repo, tid, session_id).await;
            }
            return Err("Sub-agent cancelled.".to_string());
        }
        let api_msgs = SessionCodec::to_api_messages(&transcript);
        let llm_messages = api_messages_to_llm(&api_msgs);
        let (constrained_messages, _notice_ids) = augment_llm_messages_with_runtime_constraints(
            &llm_messages,
            &constraint_harness,
            &mut constraint_state,
            &subagent_skill_task_context,
            effective_root,
            true,
            true,
        );
        let (tool_calls, assistant_text, reasoning_text, cancelled, _) =
            stream_llm_response_with_cancel(
                client.as_ref(),
                app,
                message_id,
                &runtime.round_id,
                &constrained_messages,
                &tools,
                true,
                &runtime.pending_tools,
                &runtime.cancel_flag,
                runtime.repo.clone(),
            )
            .await
            .map_err(|e| e.to_string())?;
        if cancelled {
            if let Some(tid) = background_task_id {
                persist_background_cancel_notice(&runtime.repo, tid, session_id).await;
            }
            return Err("Sub-agent cancelled.".to_string());
        }
        let pending_tool_names: Vec<String> =
            tool_calls.iter().map(|(_, name, _)| name.clone()).collect();
        if let Some(block) = constraint_harness.tool_gate(
            &ToolConstraintContext {
                request_text: &subagent_skill_task_context,
                assistant_text: &assistant_text,
                pending_tool_names: &pending_tool_names,
                is_subagent: true,
            },
            &constraint_state,
        ) {
            constraint_state.mark_clarification_requested();
            let tc = completed_to_tool_calls(&tool_calls);
            let blocked_asst = Message::Assistant {
                content: assistant_text,
                tool_calls: tc,
                token_usage: None,
                reasoning_content: (!reasoning_text.is_empty()).then_some(reasoning_text),
                follow_up_suggestions: None,
                turn_summary: None,
            };
            if let Some(tid) = background_task_id {
                persist_background_transcript_message(
                    &runtime.repo,
                    tid,
                    session_id,
                    &blocked_asst,
                )
                .await;
            }
            transcript.push(blocked_asst);
            let tool_messages: Vec<Message> = tool_calls
                .iter()
                .map(|(tool_use_id, _name, _arguments)| Message::Tool {
                    tool_call_id: tool_use_id.clone(),
                    output: block.tool_result_message.clone(),
                })
                .collect();
            if let Some(tid) = background_task_id {
                persist_background_transcript_messages(
                    &runtime.repo,
                    tid,
                    session_id,
                    &tool_messages,
                )
                .await;
            }
            transcript.extend(tool_messages);
            let clarification = Message::Assistant {
                content: block.assistant_response.clone(),
                tool_calls: None,
                token_usage: None,
                reasoning_content: None,
                follow_up_suggestions: None,
                turn_summary: None,
            };
            if let Some(tid) = background_task_id {
                persist_background_transcript_message(
                    &runtime.repo,
                    tid,
                    session_id,
                    &clarification,
                )
                .await;
            }
            transcript.push(clarification);
            return Ok(block.assistant_response);
        }
        let tc = completed_to_tool_calls(&tool_calls);
        let asst = Message::Assistant {
            content: assistant_text.clone(),
            tool_calls: tc.clone(),
            token_usage: None,
            reasoning_content: (!reasoning_text.is_empty()).then(|| reasoning_text.clone()),
            follow_up_suggestions: None,
            turn_summary: None,
        };
        if let Some(tid) = background_task_id {
            persist_background_transcript_message(&runtime.repo, tid, session_id, &asst).await;
        }
        transcript.push(asst);
        if tool_calls.is_empty() {
            let no_pending_tool_names: Vec<String> = Vec::new();
            if let Some(action) = constraint_harness.post_response_action(
                &crate::domain::runtime_constraints::PostResponseConstraintContext {
                    request_text: &subagent_skill_task_context,
                    assistant_text: &assistant_text,
                    pending_tool_names: &no_pending_tool_names,
                    is_subagent: true,
                },
                &constraint_state,
            ) {
                constraint_state.mark_post_action_attempted(action.id);
                let (retry_messages, _retry_notice_ids) =
                    augment_llm_messages_with_runtime_constraints(
                        &api_messages_to_llm(&SessionCodec::to_api_messages(&transcript)),
                        &constraint_harness,
                        &mut constraint_state,
                        &subagent_skill_task_context,
                        effective_root,
                        true,
                        true,
                    );
                let (retry_text, retry_reasoning, _) = run_post_response_retry_text_only(
                    client.as_ref(),
                    app,
                    message_id,
                    &runtime.round_id,
                    &retry_messages,
                    &action.instruction,
                    &runtime.pending_tools,
                    &runtime.cancel_flag,
                    runtime.repo.clone(),
                )
                .await
                .map_err(|e| e.to_string())?;
                if !retry_text.trim().is_empty() {
                    let retry_asst = Message::Assistant {
                        content: retry_text.clone(),
                        tool_calls: None,
                        token_usage: None,
                        reasoning_content: (!retry_reasoning.is_empty()).then_some(retry_reasoning),
                        follow_up_suggestions: None,
                        turn_summary: None,
                    };
                    if let Some(tid) = background_task_id {
                        persist_background_transcript_message(
                            &runtime.repo,
                            tid,
                            session_id,
                            &retry_asst,
                        )
                        .await;
                    }
                    transcript.push(retry_asst);
                    return Ok(retry_text);
                }
            }
            return Ok(assistant_text);
        }
        constraint_state.record_tool_names(tool_calls.iter().map(|(_, name, _)| name.as_str()));
        let results = execute_tool_calls(
            &tool_calls,
            app,
            message_id,
            session_id,
            tool_results_dir,
            effective_root,
            session_todos.clone(),
            session_agent_tasks.clone(),
            Some(runtime),
            subagent_execute_depth,
            Some(subagent_skill_task_context.as_str()),
            web_search_api_keys.clone(),
            skill_cache.clone(),
            runtime.execution_environment.clone(),
            runtime.ssh_server.clone(),
            runtime.sandbox_backend.clone(),
            runtime.local_venv_type.clone(),
            runtime.local_venv_name.clone(),
            runtime.env_store.clone(),
        )
        .await;
        let tool_messages: Vec<Message> = results
            .iter()
            .map(|(tool_use_id, output, _)| Message::Tool {
                tool_call_id: tool_use_id.clone(),
                output: output.clone(),
            })
            .collect();
        if let Some(tid) = background_task_id {
            persist_background_transcript_messages(&runtime.repo, tid, session_id, &tool_messages)
                .await;
        }
        transcript.extend(tool_messages);
    }
    Err(format!(
        "Sub-agent exceeded maximum tool rounds ({MAX_SUBAGENT_TOOL_ROUNDS})."
    ))
}

/// Isolated sub-agent loop (same API key / stream channel as parent round).
pub(super) async fn run_subagent_session(
    app: &AppHandle,
    message_id: &str,
    session_id: &str,
    tool_results_dir: &Path,
    project_root: &Path,
    session_todos: Option<Arc<Mutex<Vec<TodoItem>>>>,
    session_agent_tasks: Option<Arc<Mutex<Vec<AgentTask>>>>,
    args: &crate::domain::tools::agent::AgentArgs,
    runtime: &AgentLlmRuntime,
    // Depth for [`execute_tool_calls`] inside this sub-session (main chat uses `0`; first sub-agent uses `1`).
    subagent_execute_depth: u8,
    web_search_api_keys: WebSearchApiKeys,
    skill_cache: Arc<StdMutex<skills::SkillCacheMap>>,
) -> Result<String, String> {
    // ===== Agent 路由系统集成（含自动调度）=====
    let router = crate::domain::agents::get_agent_router();

    // 如果用户没有指定 subagent_type，使用调度器自动选择
    let selected_agent_type = args
        .subagent_type
        .as_deref()
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            let selector = AgentSelector::new();
            let agent_type = selector.select(&args.prompt, project_root.to_str().unwrap_or("."));
            tracing::info!(
                target: "omiga::scheduler",
                prompt_preview = %args.prompt.chars().take(50).collect::<String>(),
                selected_agent = %agent_type,
                "Auto-selected agent via scheduler"
            );
            agent_type
        });

    let agent_def = router.select_agent(Some(&selected_agent_type));

    let effective_root = resolve_agent_cwd(project_root, args.cwd.as_deref());

    // 检查是否需要后台执行
    let should_run_in_background = args.run_in_background == Some(true) || agent_def.background();

    if should_run_in_background {
        // 启动后台 Agent 任务；返回 UUID 供调用方追踪，包装为 LLM 可读的字符串
        let task_id = spawn_background_agent(
            app,
            message_id,
            session_id,
            None,
            tool_results_dir,
            &effective_root,
            session_todos,
            session_agent_tasks,
            args,
            runtime,
            subagent_execute_depth,
            web_search_api_keys,
            skill_cache,
            agent_def,
        )
        .await?;
        let agent_type_name = crate::domain::agents::get_agent_router()
            .select_agent(args.subagent_type.as_deref())
            .agent_type()
            .to_string();
        return Ok(format!(
            "Background agent '{}' started with task ID: {}. \
             The task is running in the background. \
             Use the task ID to check status or retrieve results.",
            agent_type_name, task_id
        ));
    }

    run_subagent_session_foreground_inner(
        app,
        message_id,
        session_id,
        tool_results_dir,
        &effective_root,
        session_todos,
        session_agent_tasks,
        args,
        runtime,
        subagent_execute_depth,
        web_search_api_keys,
        skill_cache,
        agent_def,
        None,
        None,
    )
    .await
}

/// Write-through snapshot of a background task to SQLite (best-effort).
pub(super) async fn persist_background_agent_task_snapshot(
    repo: &Arc<crate::domain::persistence::SessionRepository>,
    task: &crate::domain::agents::background::BackgroundAgentTask,
) {
    let guard = &**repo;
    if let Err(e) = guard.upsert_background_agent_task(task).await {
        tracing::warn!(target: "omiga::bg_agent", "persist background task failed: {}", e);
    }
}

/// Sidechain transcript row for a background worker (SQLite `background_agent_messages`).
async fn persist_background_transcript_message(
    repo: &Arc<crate::domain::persistence::SessionRepository>,
    task_id: &str,
    session_id: &str,
    message: &Message,
) {
    let guard = &**repo;
    if let Err(e) = guard
        .append_background_agent_message(task_id, session_id, message)
        .await
    {
        tracing::warn!(target: "omiga::bg_agent", "persist bg transcript message failed: {}", e);
    }
}

/// Batch sidechain transcript rows for a background worker — one transaction for N messages.
async fn persist_background_transcript_messages(
    repo: &Arc<crate::domain::persistence::SessionRepository>,
    task_id: &str,
    session_id: &str,
    messages: &[Message],
) {
    if messages.is_empty() {
        return;
    }
    let guard = &**repo;
    if let Err(e) = guard
        .append_background_agent_messages_batch(task_id, session_id, messages)
        .await
    {
        tracing::warn!(target: "omiga::bg_agent", "persist bg transcript batch failed: {}", e);
    }
}

/// User-visible line in the sidechain when the background worker stops due to cancellation.
const BG_SIDECHAIN_CANCEL_NOTICE: &str = "[系统] 后台任务已取消（用户或系统终止了运行）。";

async fn persist_background_cancel_notice(
    repo: &Arc<crate::domain::persistence::SessionRepository>,
    task_id: &str,
    session_id: &str,
) {
    let m = Message::User {
        content: BG_SIDECHAIN_CANCEL_NOTICE.to_string(),
    };
    persist_background_transcript_message(repo, task_id, session_id, &m).await;
}

/// 启动后台 Agent 任务
pub(crate) async fn spawn_background_agent(
    app: &AppHandle,
    message_id: &str,
    session_id: &str,
    plan_id: Option<&str>,
    tool_results_dir: &std::path::Path,
    effective_root: &std::path::Path,
    session_todos: Option<Arc<Mutex<Vec<TodoItem>>>>,
    session_agent_tasks: Option<Arc<Mutex<Vec<AgentTask>>>>,
    args: &crate::domain::tools::agent::AgentArgs,
    runtime: &AgentLlmRuntime,
    subagent_execute_depth: u8,
    web_search_api_keys: WebSearchApiKeys,
    skill_cache: Arc<StdMutex<skills::SkillCacheMap>>,
    agent_def: &dyn crate::domain::agents::AgentDefinition,
) -> Result<String, String> {
    use crate::domain::agents::background::*;

    // 注册后台任务
    let manager = crate::domain::agents::background::get_background_agent_manager();
    let task_id = manager
        .register_task(
            agent_def.agent_type().to_string(),
            args.description.clone(),
            session_id.to_string(),
            message_id.to_string(),
            Some(runtime.round_id.clone()),
            plan_id.map(|id| id.to_string()),
        )
        .await;

    let bg_repo = runtime.repo.clone();
    if let Some(task) = manager.get_task(&task_id).await {
        persist_background_agent_task_snapshot(&bg_repo, &task).await;
    }

    // 获取输出文件路径
    let output_path = crate::domain::agents::background::get_background_agent_output_path(
        app, session_id, &task_id,
    )?;

    // 更新任务状态为运行中
    manager
        .update_task_status(&task_id, BackgroundAgentStatus::Running)
        .await;
    if let Some(task) = manager.get_task(&task_id).await {
        persist_background_agent_task_snapshot(&bg_repo, &task).await;
    }

    // 发送更新事件
    if let Some(task) = manager.get_task(&task_id).await {
        let _ = emit_background_agent_update(app, &task);
    }

    // 克隆需要的变量用于异步任务
    let app_clone = app.clone();
    let message_id_clone = message_id.to_string();
    let session_id_clone = session_id.to_string();
    let tool_results_dir_clone = tool_results_dir.to_path_buf();
    let effective_root_clone = effective_root.to_path_buf();
    let args_clone = args.clone();
    let runtime_clone = runtime.clone();
    let bg_repo_spawn = bg_repo.clone();
    let web_search_api_keys_clone = web_search_api_keys.clone();
    let skill_cache_clone = skill_cache.clone();
    let task_id_clone = task_id.clone();
    let output_path_clone = output_path.clone();

    // 克隆 agent_def 的数据
    let agent_type_clone = agent_def.agent_type().to_string();

    // 创建取消令牌
    let cancel_token = manager.create_cancel_token(&task_id);

    // 在后台运行 Agent
    tokio::spawn(async move {
        // 构建运行时
        let runtime_for_task = AgentLlmRuntime {
            llm_config: runtime_clone.llm_config,
            round_id: format!("{}-bg-{}", runtime_clone.round_id, task_id_clone),
            cancel_flag: Arc::new(RwLock::new(false)),
            pending_tools: runtime_clone.pending_tools,
            repo: runtime_clone.repo,
            plan_mode_flag: runtime_clone.plan_mode_flag,
            allow_nested_agent: runtime_clone.allow_nested_agent,
            round_cancel: cancel_token.clone(),
            execution_environment: runtime_clone.execution_environment.clone(),
            ssh_server: runtime_clone.ssh_server.clone(),
            sandbox_backend: runtime_clone.sandbox_backend.clone(),
            env_store: runtime_clone.env_store.clone(),
            local_venv_type: runtime_clone.local_venv_type.clone(),
            local_venv_name: runtime_clone.local_venv_name.clone(),
            runtime_constraints_config: runtime_clone.runtime_constraints_config.clone(),
        };

        // 运行子 Agent 会话（同步等待结果）
        let result = run_subagent_session_internal(
            &app_clone,
            &message_id_clone,
            &session_id_clone,
            &tool_results_dir_clone,
            &effective_root_clone,
            session_todos,
            session_agent_tasks,
            &args_clone,
            &runtime_for_task,
            subagent_execute_depth,
            web_search_api_keys_clone,
            skill_cache_clone,
            cancel_token,
            &task_id_clone,
        )
        .await;

        let manager = crate::domain::agents::background::get_background_agent_manager();

        match result {
            Ok(output) => {
                // 写入输出文件
                let summary = format!(
                    "# Background Agent Task: {}\n\n## Agent Type\n{}\n\n## Result\n{}\n",
                    args_clone.description, agent_type_clone, output
                );

                if let Err(e) = std::fs::write(&output_path_clone, &summary) {
                    let _ = manager
                        .set_task_error(&task_id_clone, format!("Failed to write output: {}", e))
                        .await;
                } else {
                    let _ = manager
                        .set_task_result(
                            &task_id_clone,
                            output,
                            output_path_clone.to_string_lossy().to_string(),
                        )
                        .await;
                }
            }
            Err(e) => {
                let _ = manager.set_task_error(&task_id_clone, e).await;
            }
        }

        if let Some(task) = manager.get_task(&task_id_clone).await {
            persist_background_agent_task_snapshot(&bg_repo_spawn, &task).await;
        }

        // 发送完成事件
        if let Some(task) = manager.get_task(&task_id_clone).await {
            let _ = emit_background_agent_complete(&app_clone, &task);
        }
    });

    // 返回裸 UUID，调用方按需包装为 LLM 可读字符串
    Ok(task_id)
}

/// 后台 Worker：与前台共享同一套子 Agent ReAct 循环（含取消与跟进队列）。
async fn run_subagent_session_internal(
    app: &AppHandle,
    message_id: &str,
    session_id: &str,
    tool_results_dir: &std::path::Path,
    effective_root: &std::path::Path,
    session_todos: Option<Arc<Mutex<Vec<TodoItem>>>>,
    session_agent_tasks: Option<Arc<Mutex<Vec<AgentTask>>>>,
    args: &crate::domain::tools::agent::AgentArgs,
    runtime: &AgentLlmRuntime,
    subagent_execute_depth: u8,
    web_search_api_keys: WebSearchApiKeys,
    skill_cache: Arc<StdMutex<skills::SkillCacheMap>>,
    cancel_token: tokio_util::sync::CancellationToken,
    background_task_id: &str,
) -> Result<String, String> {
    let router = crate::domain::agents::get_agent_router();
    let agent_def = router.select_agent(args.subagent_type.as_deref());
    run_subagent_session_foreground_inner(
        app,
        message_id,
        session_id,
        tool_results_dir,
        effective_root,
        session_todos,
        session_agent_tasks,
        args,
        runtime,
        subagent_execute_depth,
        web_search_api_keys,
        skill_cache,
        agent_def,
        Some(&cancel_token),
        Some(background_task_id),
    )
    .await
}

/// Returns true for tools that are safe to execute concurrently:
/// - pure I/O (network fetch, file read, search) with no shared mutable state.
/// - MCP tools (PubMed, bioRxiv, Tavily, …) are the primary parallelism target.
pub(super) fn is_parallelizable_tool(tool_name: &str) -> bool {
    // MCP tools are parallelizable only when they're clearly read-only.
    // Write-capable MCP tools (e.g. mcp__claude_ai_Gmail__send, mcp__pencil__batch_design)
    // must run sequentially to avoid concurrent side-effects.
    let is_safe_mcp = tool_name.starts_with("mcp__")
        && !tool_name.contains("__send")
        && !tool_name.contains("__create")
        && !tool_name.contains("__delete")
        && !tool_name.contains("__update")
        && !tool_name.contains("__write")
        && !tool_name.contains("__batch_design")
        && !tool_name.contains("__set_");

    is_safe_mcp
        || matches!(
            tool_name,
            "web_search"
                | "WebSearch"
                | "web_fetch"
                | "WebFetch"
                | "file_read"
                | "Read"
                | "glob"
                | "Glob"
                | "ripgrep"
                | "Ripgrep"
                | "grep"
                | "Grep"
                | "recall"
                | "Recall"
        )
}
