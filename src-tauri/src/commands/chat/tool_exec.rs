//! Tool execution dispatcher: `execute_tool_calls` and `execute_one_tool`.

use super::permissions::{
    execute_ask_user_question_interactive, matches_ask_user_question_name,
    wait_for_permission_tool_resolution, AskUserQuestionExecution, PermissionToolResolutionRequest,
};
use super::subagent::{
    is_agent_tool_name, is_parallelizable_tool, run_skill_forked, run_subagent_session,
    ForkedSkillRequest, SubagentSessionRequest,
};
use super::{
    append_truncated_results_note, apply_empty_structured_tool_placeholder,
    fold_tool_stream_item_for_model, handle_skill_config, process_tool_output_for_model,
    AgentLlmRuntime, ListSkillsArgs, SkillToolArgs, SkillViewArgs, MAX_SUBAGENT_EXECUTE_DEPTH,
};
use crate::app_state::OmigaAppState;
use crate::constants::tool_limits::{
    truncate_utf8_prefix, PREVIEW_SIZE_BYTES, TOOL_DISPLAY_MAX_INPUT_CHARS,
};
use crate::domain::agents::subagent_tool_filter::{
    should_block_subagent_builtin_call, SubagentFilterOptions,
};
use crate::domain::integrations_config;
use crate::domain::permissions::{
    canonical_permission_tool_name, load_merged_permission_deny_rule_entries, matching_deny_entry,
};
use crate::domain::session::{AgentTask, TodoItem};
use crate::domain::skills;
use crate::domain::tools::{
    normalize_legacy_retrieval_tool_arguments, normalize_legacy_retrieval_tool_name, Tool,
    ToolContext, WebSearchApiKeys,
};
use crate::infrastructure::streaming::StreamOutputItem;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex as StdMutex};
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::RwLock;

const PARALLEL_TOOL_TIMEOUT_SECS: u64 = 45;

fn parallel_tool_timeout_message(tool_name: &str) -> String {
    format!("Tool `{tool_name}` timed out after {PARALLEL_TOOL_TIMEOUT_SECS}s")
}

fn normalize_legacy_web_tool_name(tool_name: &str) -> String {
    normalize_legacy_retrieval_tool_name(tool_name)
}

fn normalize_legacy_web_tool_arguments(
    original_tool_name: &str,
    normalized_tool_name: &str,
    arguments: &str,
) -> String {
    normalize_legacy_retrieval_tool_arguments(original_tool_name, normalized_tool_name, arguments)
}

fn normalize_runtime_tool_call(tool_name: &str, arguments: &str) -> (String, String) {
    let normalized_name = normalize_legacy_web_tool_name(tool_name);
    let normalized_arguments =
        normalize_legacy_web_tool_arguments(tool_name, &normalized_name, arguments);
    (normalized_name, normalized_arguments)
}

fn working_memory_query_text(
    tool_name: &str,
    arguments: &str,
    skill_task_context: Option<&str>,
) -> Option<String> {
    let trimmed = arguments.trim();
    let canonical = canonical_permission_tool_name(tool_name).to_ascii_lowercase();

    let preferred_keys: &[&str] = match canonical.as_str() {
        "recall" | "search" => &["query"],
        "query" => &["query", "id", "url", "accession"],
        "fetch" | "read_mcp_resource" => &["url", "uri"],
        _ => &[
            "query", "prompt", "message", "url", "uri", "path", "title", "text",
        ],
    };

    if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) {
        if let Some(found) = extract_first_string_field(&value, preferred_keys) {
            return Some(found);
        }
    } else if !trimmed.is_empty() && !trimmed.starts_with('{') && !trimmed.starts_with('[') {
        return Some(trimmed.to_string());
    }

    skill_task_context
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

fn extract_first_string_field(value: &serde_json::Value, keys: &[&str]) -> Option<String> {
    let object = value.as_object()?;
    for key in keys {
        let found = object
            .get(*key)
            .and_then(|candidate| candidate.as_str())
            .map(str::trim)
            .filter(|text| !text.is_empty());
        if let Some(found) = found {
            return Some(found.to_string());
        }
    }
    None
}

pub(super) struct ToolExecutionRequest<'a> {
    pub tool_calls: &'a [(String, String, String)], // (id, name, arguments)
    pub app: &'a AppHandle,
    pub message_id: &'a str,
    pub session_id: &'a str,
    pub tool_results_dir: &'a Path,
    pub project_root: &'a std::path::Path,
    pub session_todos: Option<Arc<tokio::sync::Mutex<Vec<TodoItem>>>>,
    pub session_agent_tasks: Option<Arc<tokio::sync::Mutex<Vec<AgentTask>>>>,
    pub agent_runtime: Option<&'a AgentLlmRuntime>,
    pub subagent_depth: u8,
    /// Task text for `list_skills` ordering (main user message or sub-agent description+prompt).
    pub skill_task_context: Option<&'a str>,
    pub web_search_api_keys: WebSearchApiKeys,
    pub skill_cache: Arc<StdMutex<skills::SkillCacheMap>>,
    pub execution_environment: String,
    pub ssh_server: Option<String>,
    pub sandbox_backend: String,
    pub local_venv_type: String,
    pub local_venv_name: String,
    pub env_store: crate::domain::tools::env_store::EnvStore,
    pub computer_use_enabled: bool,
    /// Optional session artifact registry for tracking file_write/file_edit operations.
    pub artifact_registry: Option<Arc<crate::domain::session::artifacts::ArtifactRegistry>>,
}

#[derive(Clone)]
struct ToolExecutionShared {
    app: AppHandle,
    message_id: String,
    session_id: String,
    tool_results_dir: PathBuf,
    project_root: PathBuf,
    session_todos: Option<Arc<tokio::sync::Mutex<Vec<TodoItem>>>>,
    session_agent_tasks: Option<Arc<tokio::sync::Mutex<Vec<AgentTask>>>>,
    subagent_depth: u8,
    skill_task_context: Option<String>,
    web_search_api_keys: WebSearchApiKeys,
    skill_cache: Arc<StdMutex<skills::SkillCacheMap>>,
    cancel_flag: Option<Arc<RwLock<bool>>>,
    round_cancel: Option<tokio_util::sync::CancellationToken>,
    execution_environment: String,
    ssh_server: Option<String>,
    sandbox_backend: String,
    local_venv_type: String,
    local_venv_name: String,
    env_store: crate::domain::tools::env_store::EnvStore,
    computer_use_enabled: bool,
    artifact_registry: Option<Arc<crate::domain::session::artifacts::ArtifactRegistry>>,
}

struct SingleToolExecution {
    tool_use_id: String,
    tool_name: String,
    arguments: String,
    shared: ToolExecutionShared,
    agent_runtime: Option<AgentLlmRuntime>,
}

pub(super) async fn execute_tool_calls(
    request: ToolExecutionRequest<'_>,
) -> Vec<(String, String, bool)> {
    use futures::future::join_all;

    let ToolExecutionRequest {
        tool_calls,
        app,
        message_id,
        session_id,
        tool_results_dir,
        project_root,
        session_todos,
        session_agent_tasks,
        agent_runtime,
        subagent_depth,
        skill_task_context,
        web_search_api_keys,
        skill_cache,
        execution_environment,
        ssh_server,
        sandbox_backend,
        local_venv_type,
        local_venv_name,
        env_store,
        computer_use_enabled,
        artifact_registry,
    } = request;

    let cancel_flag = agent_runtime.map(|r| r.cancel_flag.clone());
    let round_cancel = agent_runtime.map(|r| r.round_cancel.clone());
    let shared = ToolExecutionShared {
        app: app.clone(),
        message_id: message_id.to_string(),
        session_id: session_id.to_string(),
        tool_results_dir: tool_results_dir.to_path_buf(),
        project_root: project_root.to_path_buf(),
        session_todos,
        session_agent_tasks,
        subagent_depth,
        skill_task_context: skill_task_context.map(str::to_owned),
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
        artifact_registry,
    };

    // (tool_use_id, output, is_error)
    let mut results = Vec::new();
    let deny_entries = load_merged_permission_deny_rule_entries(project_root);
    let normalized_tool_calls = tool_calls
        .iter()
        .map(|(tool_use_id, tool_name, arguments)| {
            let (tool_name, arguments) = normalize_runtime_tool_call(tool_name, arguments);
            (tool_use_id.clone(), tool_name, arguments)
        })
        .collect::<Vec<_>>();
    let tool_calls = normalized_tool_calls.as_slice();

    // Pre-compute permission + subagent-filter results for every call (fast, sequential).
    // Calls that pass become futures; blocked calls become immediate error results.
    enum CallPrep<'a> {
        Blocked(String, String, bool), // (tool_use_id, error_msg, is_error=true)
        Ready(&'a str),                // tool_name only (indices carry id+args via tool_calls[idx])
    }

    let prepped: Vec<CallPrep<'_>> = tool_calls
        .iter()
        .map(|(tool_use_id, tool_name, _arguments)| {
            if let Some(hit) = matching_deny_entry(
                tool_name,
                &deny_entries,
            ) {
                let error_msg = format!(
                    "Tool `{tool_name}` is denied by `permissions.deny` (rule `{}` from {}).",
                    hit.rule,
                    hit.source.display()
                );
                return CallPrep::Blocked(tool_use_id.clone(), error_msg, true);
            }
            if subagent_depth > 0 {
                let c = canonical_permission_tool_name(
                    tool_name,
                );
                let parent_in_plan = false; // checked per-call below in execute_one_tool
                let allow_nested = agent_runtime
                    .map(|r| r.allow_nested_agent)
                    .unwrap_or(false);
                let sub_opts = SubagentFilterOptions {
                    parent_in_plan_mode: parent_in_plan,
                    allow_nested_agent: allow_nested,
                };
                if should_block_subagent_builtin_call(
                    &c, sub_opts,
                ) {
                    let error_msg = format!(
                        "Tool `{tool_name}` is not available to sub-agents (Claude Code `ALL_AGENT_DISALLOWED_TOOLS`)."
                    );
                    return CallPrep::Blocked(tool_use_id.clone(), error_msg, true);
                }
            }
            CallPrep::Ready(tool_name)
        })
        .collect();

    // Emit ToolResult for every pre-blocked call and record it in results at correct index.
    // We need to maintain index alignment so we can merge parallel results back in order.
    let mut ordered_results: Vec<Option<(String, String, bool)>> = vec![None; tool_calls.len()];

    let mut parallel_indices: Vec<usize> = Vec::new();
    let mut sequential_indices: Vec<usize> = Vec::new();

    for (idx, prep) in prepped.iter().enumerate() {
        match prep {
            CallPrep::Blocked(tool_use_id, error_msg, is_error) => {
                let (_, tool_name, arguments) = &tool_calls[idx];
                let _ = app.emit(
                    &format!("chat-stream-{}", message_id),
                    &StreamOutputItem::ToolResult {
                        tool_use_id: tool_use_id.clone(),
                        name: tool_name.clone(),
                        input: arguments.clone(),
                        output: error_msg.clone(),
                        is_error: *is_error,
                    },
                );
                ordered_results[idx] = Some((tool_use_id.clone(), error_msg.clone(), *is_error));
            }
            CallPrep::Ready(tool_name) => {
                if is_parallelizable_tool(tool_name) {
                    parallel_indices.push(idx);
                } else {
                    sequential_indices.push(idx);
                }
            }
        }
    }

    // --- Parallel batch: spawn all parallelizable futures at once ---
    if !parallel_indices.is_empty() {
        let parallel_futures: Vec<_> = parallel_indices
            .iter()
            .map(|&idx| {
                let (tool_use_id, tool_name, arguments) = &tool_calls[idx];
                execute_one_tool(SingleToolExecution {
                    tool_use_id: tool_use_id.clone(),
                    tool_name: tool_name.clone(),
                    arguments: arguments.clone(),
                    shared: shared.clone(),
                    agent_runtime: None, // parallelizable tools don't need agent_runtime
                })
            })
            .collect();

        // Wrap each future with a bounded timeout so one slow fetch can't stall
        // the whole batch.  On timeout we return an error result for that tool slot
        // instead of blocking every other tool indefinitely.
        let timed_futures: Vec<_> = parallel_futures
            .into_iter()
            .zip(parallel_indices.iter())
            .map(|(fut, &idx)| {
                let (tool_use_id, tool_name, arguments) = &tool_calls[idx];
                let tuid = tool_use_id.clone();
                let tname = tool_name.clone();
                let args = arguments.clone();
                let app = app.clone();
                let message_id = message_id.to_string();
                async move {
                    match tokio::time::timeout(
                        tokio::time::Duration::from_secs(PARALLEL_TOOL_TIMEOUT_SECS),
                        fut,
                    )
                    .await
                    {
                        Ok(res) => res,
                        Err(_) => {
                            let error_msg = parallel_tool_timeout_message(&tname);
                            let display_input = if args.len() > TOOL_DISPLAY_MAX_INPUT_CHARS {
                                let prefix =
                                    truncate_utf8_prefix(&args, TOOL_DISPLAY_MAX_INPUT_CHARS);
                                format!(
                                    "{}\n\n[Input truncated... {} total characters]",
                                    prefix,
                                    args.len()
                                )
                            } else {
                                args.clone()
                            };
                            let _ = app.emit(
                                &format!("chat-stream-{}", message_id),
                                &StreamOutputItem::ToolResult {
                                    tool_use_id: tuid.clone(),
                                    name: tname.clone(),
                                    input: display_input,
                                    output: error_msg.clone(),
                                    is_error: true,
                                },
                            );
                            (tuid.clone(), error_msg, true)
                        }
                    }
                }
            })
            .collect();
        let parallel_results = join_all(timed_futures).await;
        for (&idx, res) in parallel_indices.iter().zip(parallel_results) {
            ordered_results[idx] = Some(res);
        }
    }

    // --- Sequential batch: stateful tools run one-by-one ---
    // `active_skill_allowed_tools`: set when an inline skill with `allowed-tools` frontmatter
    // executes; restricts subsequent tool calls in the same batch to that list.
    let mut active_skill_allowed_tools: Option<Vec<String>> = None;

    for idx in sequential_indices {
        let (tool_use_id, tool_name, arguments) = &tool_calls[idx];

        // Enforce skill allowed-tools restriction for all tools except `skill` itself.
        if let Some(ref allowed) = active_skill_allowed_tools {
            let is_skill_tool = matches!(
                tool_name.to_ascii_lowercase().as_str(),
                "skill" | "listskills" | "list_skills"
            );
            if !is_skill_tool {
                let canonical = canonical_permission_tool_name(tool_name);
                let permitted = allowed.iter().any(|a| {
                    canonical_permission_tool_name(a) == canonical || a == tool_name
                });
                if !permitted {
                    let error_msg = format!(
                        "Tool `{tool_name}` is blocked by the active skill's \
                         `allowed-tools` restriction. Permitted tools: [{}]. \
                         Complete the current skill context before using other tools.",
                        allowed.join(", ")
                    );
                    let _ = app.emit(
                        &format!("chat-stream-{message_id}"),
                        &StreamOutputItem::ToolResult {
                            tool_use_id: tool_use_id.clone(),
                            name: tool_name.clone(),
                            input: arguments.clone(),
                            output: error_msg.clone(),
                            is_error: true,
                        },
                    );
                    ordered_results[idx] = Some((tool_use_id.clone(), error_msg, true));
                    continue;
                }
            }
        }

        // --- Feature 1: per-call timeout for sequential tools ---
        // Skills, bash, and agent tools are exempt: they routinely run for many minutes.
        const SEQUENTIAL_TOOL_DEFAULT_TIMEOUT_SECS: u64 = 120;
        let exempt_from_timeout = matches!(
            tool_name.to_ascii_lowercase().as_str(),
            "skill" | "bash" | "agent" | "task"
        );
        let res = if exempt_from_timeout {
            execute_one_tool(SingleToolExecution {
                tool_use_id: tool_use_id.clone(),
                tool_name: tool_name.clone(),
                arguments: arguments.clone(),
                shared: shared.clone(),
                agent_runtime: agent_runtime.cloned(),
            })
            .await
        } else {
            let timeout_dur =
                std::time::Duration::from_secs(SEQUENTIAL_TOOL_DEFAULT_TIMEOUT_SECS);
            match tokio::time::timeout(
                timeout_dur,
                execute_one_tool(SingleToolExecution {
                    tool_use_id: tool_use_id.clone(),
                    tool_name: tool_name.clone(),
                    arguments: arguments.clone(),
                    shared: shared.clone(),
                    agent_runtime: agent_runtime.cloned(),
                }),
            )
            .await
            {
                Ok(result) => result,
                Err(_) => {
                    let error_msg = format!(
                        "Tool `{tool_name}` timed out after {} seconds. \
                         Use a longer-running background task or reduce scope.",
                        SEQUENTIAL_TOOL_DEFAULT_TIMEOUT_SECS
                    );
                    let _ = app.emit(
                        &format!("chat-stream-{message_id}"),
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
        };

        // After a skill executes successfully, check whether it declared allowed-tools.
        if matches!(tool_name.to_ascii_lowercase().as_str(), "skill") && !res.2 {
            if let Some(filter) = extract_skill_allowed_tools(&res.1) {
                active_skill_allowed_tools = Some(filter);
            }
        }

        // --- Feature 2 Step C: record file artifacts ---
        if !res.2 {
            let tname_lower = tool_name.to_ascii_lowercase();
            let is_write_tool = matches!(
                tname_lower.as_str(),
                "file_write" | "write_file"
            );
            let is_edit_tool = matches!(
                tname_lower.as_str(),
                "file_edit" | "edit_file" | "str_replace_editor"
            );
            if is_write_tool || is_edit_tool {
                if let Some(ref registry) = shared.artifact_registry {
                    if let Ok(args_val) =
                        serde_json::from_str::<serde_json::Value>(arguments)
                    {
                        let path = args_val
                            .get("path")
                            .or_else(|| args_val.get("file_path"))
                            .and_then(|v| v.as_str());
                        if let Some(path) = path {
                            let op = if is_write_tool { "write" } else { "edit" };
                            registry.record(path, op);
                        }
                    }
                }
            }
        }

        // Audit log: fire-and-forget record of sensitive tool executions.
        if crate::domain::audit::should_audit(tool_name) {
            let entry = crate::domain::audit::AuditEntry {
                ts: chrono::Utc::now().to_rfc3339(),
                session_id: session_id.to_string(),
                tool: tool_name.clone(),
                args_summary: crate::domain::audit::summarize_args(tool_name, arguments),
                status: if res.2 { "error" } else { "ok" },
                message: if res.2 {
                    Some(res.1.chars().take(200).collect())
                } else {
                    None
                },
            };
            tokio::spawn(crate::domain::audit::log(entry));
        }

        ordered_results[idx] = Some(res);
    }

    results.extend(ordered_results.into_iter().flatten());
    results
}

/// Parse the `allowedTools` array from an inline skill's output JSON metadata header.
///
/// Skill output format (text, not Rust):
/// `Launching skill: NAME\n\n{ "success": true, "allowedTools": ["bash", "file_read"], ... }\n\n---\n\n[body]`
fn extract_skill_allowed_tools(output: &str) -> Option<Vec<String>> {
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
    if allowed.is_empty() { None } else { Some(allowed) }
}

/// Execute a single tool call. Called from both the parallel and sequential paths.
#[async_recursion::async_recursion]
async fn execute_one_tool(request: SingleToolExecution) -> (String, String, bool) {
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

    let arguments = &effective_arguments;

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
        let permission_manager = app_state.permission_manager.clone();

        let args_value: serde_json::Value = serde_json::from_str(arguments)
            .unwrap_or_else(|_| serde_json::json!({"raw": arguments}));

        loop {
            let perm_decision = permission_manager
                .check_tool_with_root(session_id, tool_name, &args_value, Some(project_root))
                .await;

            match perm_decision {
                crate::domain::permissions::PermissionDecision::Deny(ref reason) => {
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
                            input: arguments.clone(),
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
                        arguments_display: arguments,
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
                        tool = %tool_name,
                        "Tool allowed by permission manager"
                    );
                    break;
                }
            }
        }
    }
    // === End permission check ===

    // Parse and execute the tool
    let result = if tool_name.eq_ignore_ascii_case("list_skills")
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
                                            let display_output =
                                                if fork_result_str.len() > PREVIEW_SIZE_BYTES {
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
    } else if is_agent_tool_name(tool_name) {
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
    } else if tool_name.starts_with(crate::domain::operators::OPERATOR_TOOL_PREFIX) {
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
    } else if crate::domain::computer_use::is_facade_tool_name(tool_name) {
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
            crate::domain::computer_use::ComputerUseSettings::from_stored_json(
                settings_raw.as_deref(),
            );
        let _ = crate::domain::computer_use::prune_audit_retention(
            project_root,
            computer_use_settings.log_retention_days,
        );
        let mcp_pool_legacy: Option<
            std::sync::Arc<
                tokio::sync::Mutex<
                    std::collections::HashMap<
                        String,
                        crate::domain::mcp::client::McpLiveConnection,
                    >,
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
                            .unwrap_or_else(
                                |_| serde_json::json!({ "text": validation_output_text }),
                            );
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
                let backend_result =
                    serde_json::from_str::<serde_json::Value>(&backend_output_text)
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
                let model_output = process_tool_output_for_model(
                    output_text.clone(),
                    tool_use_id,
                    tool_results_dir,
                )
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
    } else if tool_name.starts_with("mcp__") {
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
                    std::collections::HashMap<
                        String,
                        crate::domain::mcp::client::McpLiveConnection,
                    >,
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
                let model_output = process_tool_output_for_model(
                    output_text.clone(),
                    tool_use_id,
                    tool_results_dir,
                )
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
    } else {
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
            working_memory_query_text(tool_name, arguments, skill_task_context);
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
            let url = fetch_source_url_from_args(arguments);
            if let Some(url) = url {
                if let Ok(cfg) = crate::domain::memory::load_resolved_config(project_root).await {
                    let lt_root = cfg.long_term_path(project_root);
                    let hits =
                        crate::domain::memory::source_registry::search_sources(&lt_root, &url, 1)
                            .await;
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

                        // Source Registry: record web sources used in this turn.
                        if !is_error {
                            register_web_source_async(
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
    };

    result
}

/// Fire-and-forget: register a web source after a successful tool call.
/// Spawns a Tokio task so it never blocks the tool execution pipeline.
fn fetch_source_url_from_args(arguments: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(arguments)
        .ok()
        .and_then(|value| source_url_from_fetch_value(&value))
}

fn fetch_source_url_from_args_and_output(arguments: &str, output: &str) -> Option<String> {
    fetch_source_url_from_args(arguments).or_else(|| {
        parse_json_value_from_tool_output(output)
            .and_then(|value| source_url_from_fetch_value(&value))
    })
}

fn parse_json_value_from_tool_output(output: &str) -> Option<serde_json::Value> {
    serde_json::from_str::<serde_json::Value>(output)
        .ok()
        .or_else(|| {
            let start = output.find('{')?;
            let end = output.rfind('}')?;
            if end <= start {
                return None;
            }
            serde_json::from_str::<serde_json::Value>(&output[start..=end]).ok()
        })
}

fn source_url_from_fetch_value(value: &serde_json::Value) -> Option<String> {
    string_field(value, &["url", "link", "href"])
        .or_else(|| value.get("result").and_then(source_url_from_fetch_value))
        .or_else(|| pubmed_url_from_fetch_value(value))
}

fn pubmed_url_from_fetch_value(value: &serde_json::Value) -> Option<String> {
    let category = string_field(value, &["category"]).unwrap_or_default();
    let source = string_field(value, &["source", "effective_source"]).unwrap_or_default();
    let looks_pubmed =
        category.eq_ignore_ascii_case("literature") || source.eq_ignore_ascii_case("pubmed");
    let pmid = string_field(value, &["id", "pmid"])
        .or_else(|| {
            value
                .get("metadata")
                .and_then(|metadata| string_field(metadata, &["pmid"]))
        })
        .filter(|id| id.chars().all(|c| c.is_ascii_digit()));
    if looks_pubmed {
        pmid.map(|id| format!("https://pubmed.ncbi.nlm.nih.gov/{id}/"))
    } else {
        None
    }
}

fn string_field(value: &serde_json::Value, keys: &[&str]) -> Option<String> {
    for key in keys {
        let candidate = value.get(*key).and_then(serde_json::Value::as_str);
        if let Some(value) = candidate.map(str::trim).filter(|s| !s.is_empty()) {
            return Some(value.to_string());
        }
    }
    None
}

fn register_web_source_async(
    tool_name: &str,
    arguments: &str,
    output_text: &str,
    session_id: &str,
    project_root: &std::path::Path,
) {
    let tool_name = tool_name.to_string();
    let arguments = arguments.to_string();
    let output = output_text.to_string();
    let session_id = session_id.to_string();
    let project_root = project_root.to_path_buf();

    tokio::spawn(async move {
        let Ok(cfg) = crate::domain::memory::load_resolved_config(&project_root).await else {
            return;
        };
        let lt_root = cfg.long_term_path(&project_root);

        match tool_name.as_str() {
            "fetch" | "Fetch" => {
                // Extract URL from args or the structured result. `fetch` also accepts
                // search-result objects and PubMed PMIDs, so top-level `url` is not enough.
                let url = fetch_source_url_from_args_and_output(&arguments, &output);
                if let Some(url) = url {
                    let entry = crate::domain::memory::source_registry::entry_from_fetch(
                        &url,
                        &output,
                        Some(&session_id),
                        None,
                    );
                    crate::domain::memory::source_registry::upsert_source(&lt_root, entry).await;
                }
            }
            "search" | "Search" => {
                // Extract query from args: {"query":"..."}
                let query = serde_json::from_str::<serde_json::Value>(&arguments)
                    .ok()
                    .and_then(|v| v.get("query").and_then(|q| q.as_str()).map(str::to_owned))
                    .unwrap_or_default();
                let entries = crate::domain::memory::source_registry::entries_from_search_output(
                    &output,
                    Some(&session_id),
                    &query,
                );
                for entry in entries {
                    crate::domain::memory::source_registry::upsert_source(&lt_root, entry).await;
                }
            }
            _ => {}
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn working_memory_query_prefers_recall_query_field() {
        let query = working_memory_query_text(
            "recall",
            r#"{"query":"氧化还原节律","scope":"all","limit":5}"#,
            Some("fallback task"),
        );

        assert_eq!(query.as_deref(), Some("氧化还原节律"));
    }

    #[test]
    fn working_memory_query_reads_query_tool_identifiers() {
        let query = working_memory_query_text(
            "query",
            r#"{"category":"dataset","operation":"fetch","id":"GSE12345"}"#,
            None,
        );

        assert_eq!(query.as_deref(), Some("GSE12345"));
    }

    #[test]
    fn working_memory_query_falls_back_to_task_context_when_needed() {
        let query =
            working_memory_query_text("todo_write", r#"{"todos":[]}"#, Some("整理记忆分层"));

        assert_eq!(query.as_deref(), Some("整理记忆分层"));
    }

    #[test]
    fn parallel_tool_timeout_message_names_tool_and_budget() {
        assert_eq!(
            parallel_tool_timeout_message("search"),
            "Tool `search` timed out after 45s"
        );
    }

    #[test]
    fn runtime_normalizes_legacy_pubmed_mcp_to_unified_search() {
        let (name, args) = normalize_runtime_tool_call(
            "mcp__pubmed__pubmed_search_articles",
            r#"{"term":"lung cancer","retmax":4}"#,
        );

        assert_eq!(name, "search");
        let value: serde_json::Value = serde_json::from_str(&args).unwrap();
        assert_eq!(value["category"], "literature");
        assert_eq!(value["source"], "pubmed");
        assert_eq!(value["query"], "lung cancer");
        assert_eq!(value["max_results"], 4);
    }

    #[test]
    fn fetch_source_url_resolves_search_result_locator() {
        let args = r#"{
            "category": "web",
            "result": {
                "title": "Article",
                "link": "https://example.org/article",
                "favicon": "https://www.google.com/s2/favicons?domain=example.org&sz=64"
            }
        }"#;

        assert_eq!(
            fetch_source_url_from_args(args).as_deref(),
            Some("https://example.org/article")
        );
    }

    #[test]
    fn fetch_source_url_resolves_pubmed_id_locator() {
        let args = r#"{"category":"literature","source":"pubmed","id":"12345678"}"#;

        assert_eq!(
            fetch_source_url_from_args(args).as_deref(),
            Some("https://pubmed.ncbi.nlm.nih.gov/12345678/")
        );
    }

    #[test]
    fn fetch_source_url_falls_back_to_structured_fetch_output() {
        let args = r#"{"category":"web","prompt":"summarize"}"#;
        let output = r#"{
            "category": "web",
            "title": "Fetched",
            "url": "https://example.net/final",
            "favicon": "https://www.google.com/s2/favicons?domain=example.net&sz=64"
        }"#;

        assert_eq!(
            fetch_source_url_from_args_and_output(args, output).as_deref(),
            Some("https://example.net/final")
        );
    }
}
