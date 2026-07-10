use super::super::agent_runtime::AgentLlmRuntime;
use super::concurrency::{
    load_concurrency_safe_tool_names, partition_tool_call_indices_by_concurrency,
};
use super::dispatch::execute_one_tool;
use super::handlers;
use super::normalize::normalize_runtime_tool_call;
use super::ToolExecutionRequest;
use crate::constants::tool_limits::{truncate_utf8_prefix, TOOL_DISPLAY_MAX_INPUT_CHARS};
use crate::domain::agents::subagent_tool_filter::{
    should_block_subagent_builtin_call, SubagentFilterOptions,
};
use crate::domain::permissions::{
    canonical_permission_tool_name, load_merged_permission_deny_rule_entries, matching_deny_entry,
};
use crate::domain::session::{AgentTask, TodoItem};
use crate::domain::skills;
use crate::domain::tools::WebSearchApiKeys;
use crate::infrastructure::streaming::StreamOutputItem;
use std::path::PathBuf;
use std::sync::{Arc, Mutex as StdMutex};
use tauri::{AppHandle, Emitter};
use tokio::sync::RwLock;

const PARALLEL_TOOL_TIMEOUT_SECS: u64 = 45;

fn parallel_tool_timeout_message(tool_name: &str) -> String {
    format!("Tool `{tool_name}` timed out after {PARALLEL_TOOL_TIMEOUT_SECS}s")
}

#[derive(Clone)]
pub(super) struct ToolExecutionShared {
    pub(super) app: AppHandle,
    pub(super) message_id: String,
    pub(super) session_id: String,
    pub(super) tool_results_dir: PathBuf,
    pub(super) project_root: PathBuf,
    pub(super) session_todos: Option<Arc<tokio::sync::Mutex<Vec<TodoItem>>>>,
    pub(super) session_agent_tasks: Option<Arc<tokio::sync::Mutex<Vec<AgentTask>>>>,
    pub(super) subagent_depth: u8,
    pub(super) skill_task_context: Option<String>,
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
    pub(super) artifact_registry: Option<Arc<crate::domain::session::artifacts::ArtifactRegistry>>,
}

pub(super) struct SingleToolExecution {
    pub(super) tool_use_id: String,
    pub(super) tool_name: String,
    pub(super) arguments: String,
    pub(super) shared: ToolExecutionShared,
    pub(super) agent_runtime: Option<AgentLlmRuntime>,
}
pub(in crate::commands::chat) async fn execute_tool_calls(
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
        browser_use_enabled,
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
        browser_use_enabled,
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
    let include_mcp_tools = tool_calls
        .iter()
        .any(|(_, tool_name, _)| tool_name.starts_with("mcp__"));
    let concurrency_safe_tool_names =
        load_concurrency_safe_tool_names(app, project_root, include_mcp_tools).await;

    // Pre-compute permission + subagent-filter results for every call (fast, sequential).
    // Calls that pass become futures; blocked calls become immediate error results.
    enum CallPrep {
        Blocked(String, String, bool), // (tool_use_id, error_msg, is_error=true)
        Ready,
    }

    let prepped: Vec<CallPrep> = tool_calls
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
            CallPrep::Ready
        })
        .collect();

    // Emit ToolResult for every pre-blocked call and record it in results at correct index.
    // We need to maintain index alignment so we can merge parallel results back in order.
    let mut ordered_results: Vec<Option<(String, String, bool)>> = vec![None; tool_calls.len()];

    let mut ready_indices: Vec<usize> = Vec::new();

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
            CallPrep::Ready => ready_indices.push(idx),
        }
    }
    let (parallel_indices, sequential_indices) = partition_tool_call_indices_by_concurrency(
        ready_indices,
        tool_calls,
        &concurrency_safe_tool_names,
    );

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
    let mut bash_call_count: u32 = 0;

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
                let permitted = allowed
                    .iter()
                    .any(|a| canonical_permission_tool_name(a) == canonical || a == tool_name);
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
        let mut res = if exempt_from_timeout {
            execute_one_tool(SingleToolExecution {
                tool_use_id: tool_use_id.clone(),
                tool_name: tool_name.clone(),
                arguments: arguments.clone(),
                shared: shared.clone(),
                agent_runtime: agent_runtime.cloned(),
            })
            .await
        } else {
            let timeout_dur = std::time::Duration::from_secs(SEQUENTIAL_TOOL_DEFAULT_TIMEOUT_SECS);
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
            if let Some(filter) = handlers::extract_skill_allowed_tools(&res.1) {
                active_skill_allowed_tools = Some(filter);
            }
        }

        // --- Feature 2 Step C: record file artifacts ---
        if !res.2 {
            let tname_lower = tool_name.to_ascii_lowercase();
            let is_write_tool = matches!(tname_lower.as_str(), "file_write" | "write_file");
            let is_edit_tool = matches!(
                tname_lower.as_str(),
                "file_edit" | "edit_file" | "str_replace_editor"
            );
            if is_write_tool || is_edit_tool {
                if let Some(ref registry) = shared.artifact_registry {
                    if let Ok(args_val) = serde_json::from_str::<serde_json::Value>(arguments) {
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

        // --- Runtime behaviour hints injected into tool output (not errors) ---
        if tool_name.eq_ignore_ascii_case("bash") {
            bash_call_count += 1;
            let count_hint: Option<&str> = if bash_call_count == 6 {
                Some("\n\n[System hint: 6 Bash calls this turn. For multi-step tasks prefer a single call with && chaining, or write a script with file_write and run it once.]")
            } else if bash_call_count == 12 {
                Some("\n\n[System hint: 12 Bash calls this turn. If you are in a loop, stop — diagnose the root cause, write a script file, or change approach entirely.]")
            } else {
                None
            };
            if let Some(h) = count_hint {
                res.1.push_str(h);
            }
            // Flag large inline scripts (python3 -c / node -e / etc.) on successful runs
            if !res.2 {
                if let Ok(args_val) = serde_json::from_str::<serde_json::Value>(arguments) {
                    if let Some(cmd) = args_val.get("command").and_then(|v| v.as_str()) {
                        let is_inline = cmd.len() > 150
                            && (cmd.contains("python3 -c")
                                || cmd.contains("python -c")
                                || cmd.contains("node -e")
                                || cmd.contains("ruby -e")
                                || cmd.contains("perl -e"));
                        if is_inline {
                            res.1.push_str(
                                "\n\n[System hint: Inline script detected. For code longer than 3 lines use file_write to save it to a .py/.js file and execute — avoids quoting errors and produces an auditable file.]",
                            );
                        }
                    }
                }
            }
        }

        // --- Forced verification gate ---
        // When todo_write marks ≥3 tasks completed and no verification agent ran
        // this turn, inject a prompt requiring an independent verification Agent.
        //
        // Detection uses an exact sentinel string emitted by the verification agent's
        // skill file, not a broad substring, to avoid false-positive suppression from
        // arbitrary tool output that happens to contain "verif…".
        const VERIFICATION_SENTINEL: &str = "[VERIFICATION-AGENT-RAN]";
        if tool_name.eq_ignore_ascii_case("todo_write") && !res.2 {
            if let Ok(args_val) = serde_json::from_str::<serde_json::Value>(arguments) {
                let todos_raw = args_val.get("todos");

                // Warn when the key exists but is not an array — silently swallowing
                // malformed input would hide logic errors.
                if todos_raw.is_some() && todos_raw.and_then(|v| v.as_array()).is_none() {
                    tracing::warn!(
                        target: "omiga::verification_gate",
                        "todo_write `todos` key exists but is not an array — skipping gate"
                    );
                }

                let todos = todos_raw
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default();
                let completed_count = todos
                    .iter()
                    .filter(|t| t.get("status").and_then(|s| s.as_str()) == Some("completed"))
                    .count();
                let all_completed = completed_count == todos.len() && completed_count >= 3;

                // Check whether a verification agent already ran this turn by looking
                // for the exact sentinel token, not a broad substring.
                let verification_ran = ordered_results
                    .iter()
                    .flatten()
                    .any(|(_, out, _)| out.contains(VERIFICATION_SENTINEL));

                if all_completed && !verification_ran {
                    res.1.push_str(
                        "\n\n[System: All tasks marked completed. Before writing your final \
                         summary, spawn an independent verification agent:\n\
                         Agent({ subagent_type: \"verification\",\n\
                         prompt: \"Verify all claimed completed tasks. For each task, confirm \
                         the change exists at the expected file:line and passes a sanity check. \
                         Issue PASS/PARTIAL/FAIL verdict with evidence. \
                         Begin your response with the token [VERIFICATION-AGENT-RAN].\" })\n\
                         Self-verification is not accepted — only the verifier's verdict counts.]",
                    );
                }
            }
        }

        ordered_results[idx] = Some(res);
    }

    results.extend(ordered_results.into_iter().flatten());
    results
}

#[cfg(test)]
mod tests {
    use super::parallel_tool_timeout_message;

    #[test]
    fn parallel_tool_timeout_message_names_tool_and_budget() {
        assert_eq!(
            parallel_tool_timeout_message("search"),
            "Tool `search` timed out after 45s"
        );
    }
}
