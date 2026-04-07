//! Chat commands - Send messages and stream responses with tool execution
//!
//! Multi-provider support: Anthropic, OpenAI, Azure, Google, and custom endpoints

use super::CommandResult;
use crate::app_state::OmigaAppState;
use crate::api::{ContentBlock, Role};
use crate::constants::agent_prompt;
use crate::constants::tool_limits::{
    large_output_persist_failed_message, large_tool_output_files_enabled, truncate_utf8_prefix,
    DEFAULT_MAX_RESULT_SIZE_CHARS, PREVIEW_SIZE_BYTES, TOOL_DISPLAY_MAX_INPUT_CHARS,
};
use crate::domain::session::{AgentTask, Message, Session, TodoItem, ToolCall};
use crate::domain::session_codec::SessionCodec;
use crate::utils::large_output_instructions::get_large_output_instructions;
use crate::domain::integrations_config;
use crate::domain::persistence::SessionRepository;
use crate::domain::skills;
use crate::domain::subagent_tool_filter::{env_allow_nested_agent, SubagentFilterOptions};
use crate::domain::tools::{all_tool_schemas, Tool, ToolContext, ToolSchema};
use crate::errors::{ApiError, ChatError, OmigaError};
use crate::infrastructure::streaming::StreamOutputItem;
use crate::llm::{create_client, load_config_from_env, LlmClient, LlmConfig, LlmContent, LlmMessage, LlmRole, LlmStreamChunk, LlmProvider};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::{Mutex, RwLock};

/// Arguments for the `skill` tool (JSON) — aligned with `SkillTool` input (`skill` + `args`).
#[derive(Debug, Deserialize)]
struct SkillToolArgs {
    skill: String,
    #[serde(default, rename = "args", alias = "arguments")]
    args: String,
}

#[derive(Debug, Deserialize, Default)]
struct ListSkillsArgs {
    query: Option<String>,
}

/// Max assistant↔tool iterations per user send (safety valve; TS query loop is bounded similarly).
const MAX_TOOL_ROUNDS: usize = 25;

/// Max tool rounds inside one `Agent` sub-session (nested Agent calls are blocked separately).
const MAX_SUBAGENT_TOOL_ROUNDS: usize = 16;

/// Max `execute_tool_calls` depth for nested `Agent` (main session = 0). TS allows deep nesting when `USER_TYPE=ant`.
const MAX_SUBAGENT_EXECUTE_DEPTH: u8 = 8;

/// LLM + stream state needed for the `Agent` tool to run an isolated sub-session (same API key as main chat).
#[derive(Clone)]
struct AgentLlmRuntime {
    llm_config: LlmConfig,
    round_id: String,
    cancel_flag: Arc<RwLock<bool>>,
    pending_tools: Arc<Mutex<HashMap<String, PendingToolCall>>>,
    repo: Arc<Mutex<crate::domain::persistence::SessionRepository>>,
    /// Same `Arc` as [`SessionRuntimeState::plan_mode`] — sub-agent filter reads plan mode for `ExitPlanMode` parity.
    plan_mode_flag: Option<Arc<Mutex<bool>>>,
    /// `USER_TYPE=ant` — nested `Agent` allowed (`ALL_AGENT_DISALLOWED_TOOLS` omits Agent).
    allow_nested_agent: bool,
}

pub use crate::domain::chat_state::{
    ChatState, PendingToolCall, RoundCancellationState, SessionRuntimeState,
};

async fn load_claude_user_skills_enabled_from_repo(repo: &SessionRepository) -> bool {
    match repo
        .get_setting(skills::SETTING_KEY_LOAD_CLAUDE_USER_SKILLS)
        .await
    {
        Ok(v) => skills::parse_load_claude_user_skills_setting(v.as_deref()),
        Err(_) => false,
    }
}

async fn load_claude_user_skills_enabled_from_agent_runtime(
    agent_runtime: Option<&AgentLlmRuntime>,
) -> bool {
    let Some(ar) = agent_runtime else {
        return false;
    };
    let repo = ar.repo.lock().await;
    load_claude_user_skills_enabled_from_repo(&repo).await
}

/// Get or create LLM config from environment or state
async fn get_llm_config(chat_state: &ChatState) -> Result<LlmConfig, OmigaError> {
    // First check if we have a stored config
    let stored = chat_state.llm_config.lock().await;
    if let Some(config) = stored.as_ref() {
        if !config.api_key.is_empty() {
            return Ok(config.clone());
        }
    }
    drop(stored);

    // Try to load from environment
    match load_config_from_env() {
        Ok(config) => {
            // Store for future use
            let mut stored = chat_state.llm_config.lock().await;
            *stored = Some(config.clone());
            Ok(config)
        }
        Err(_e) => Err(OmigaError::Chat(ChatError::ApiKeyMissing)),
    }
}

fn tool_results_dir_for_session(app: &AppHandle, session_id: &str) -> std::path::PathBuf {
    app.path()
        .app_data_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
        .join("tool-results")
        .join(session_id)
}

/// Resolve session `project_path` to an absolute-ish root for tools (glob, bash, file_read).
fn resolve_session_project_root(project_path: &str) -> std::path::PathBuf {
    let p = project_path.trim();
    if p.is_empty() || p == "." {
        std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
    } else {
        std::path::PathBuf::from(p)
    }
}

fn completed_to_tool_calls(calls: &[(String, String, String)]) -> Option<Vec<ToolCall>> {
    if calls.is_empty() {
        return None;
    }
    Some(
        calls
            .iter()
            .map(|(id, name, args)| ToolCall {
                id: id.clone(),
                name: name.clone(),
                arguments: args.clone(),
            })
            .collect(),
    )
}

fn tool_calls_json_opt(calls: &[(String, String, String)]) -> Option<String> {
    completed_to_tool_calls(calls).and_then(|v| serde_json::to_string(&v).ok())
}

fn api_messages_to_llm(messages: &[crate::api::Message]) -> Vec<LlmMessage> {
    messages
        .iter()
        .map(|msg| LlmMessage {
            role: match msg.role {
                Role::User => LlmRole::User,
                Role::Assistant => LlmRole::Assistant,
            },
            content: msg
                .content
                .iter()
                .map(|block| match block {
                    ContentBlock::Text { text } => LlmContent::Text { text: text.clone() },
                    ContentBlock::ToolUse { id, name, input } => LlmContent::ToolUse {
                        id: id.clone(),
                        name: name.clone(),
                        arguments: input.clone(),
                    },
                    ContentBlock::ToolResult {
                        tool_use_id,
                        content,
                        is_error,
                    } => LlmContent::ToolResult {
                        tool_use_id: tool_use_id.clone(),
                        content: content.clone(),
                        is_error: *is_error,
                    },
                })
                .collect(),
            name: None,
            tool_calls: None,
        })
        .collect()
}

/// Large tool output: spill to disk + inject instructions, or truncate (TS parity).
async fn process_tool_output_for_model(
    raw: String,
    tool_use_id: &str,
    dir: &Path,
) -> String {
    let size = raw.len();
    if size <= DEFAULT_MAX_RESULT_SIZE_CHARS {
        return raw;
    }
    if !large_tool_output_files_enabled() {
        let truncated = truncate_utf8_prefix(&raw, DEFAULT_MAX_RESULT_SIZE_CHARS).to_string();
        return format!(
            "{truncated}\n\n[Output truncated: {size} bytes; large tool files disabled (OMIGA_ENABLE_LARGE_TOOL_OUTPUT_FILES=0)]"
        );
    }
    let safe_id = tool_use_id.replace(['/', '\\', ':', ' '], "_");
    let path = dir.join(format!("{safe_id}.txt"));
    if let Err(e) = tokio::fs::create_dir_all(dir).await {
        return large_output_persist_failed_message(size, &e.to_string());
    }
    match tokio::fs::write(&path, raw.as_bytes()).await {
        Ok(()) => get_large_output_instructions(
            path.to_string_lossy().as_ref(),
            size,
            "Plain text",
            None,
        ),
        Err(e) => large_output_persist_failed_message(size, &e.to_string()),
    }
}

/// Fold one streamed tool item into the string persisted for the model / next LLM turn.
/// Matches the main repo strategy (`mapToolResultToToolResultBlockParam`, `extractSearchText`):
/// structured tools (grep/glob) emit `GrepMatch` / `GlobMatch` chunks; we concatenate them into
/// one plain-text tool result instead of dropping them on `_ => {}`.
fn fold_tool_stream_item_for_model(
    output: &mut String,
    item: StreamOutputItem,
    stream_error: &mut bool,
    exit_code: &mut Option<i32>,
    truncated_note: &mut bool,
) {
    match item {
        StreamOutputItem::Content(text) | StreamOutputItem::Text(text) => {
            output.push_str(&text);
        }
        StreamOutputItem::Stdout(text) => {
            output.push_str(&text);
        }
        StreamOutputItem::Stderr(text) => {
            if !text.is_empty() {
                if !output.is_empty() && !output.ends_with('\n') {
                    output.push('\n');
                }
                output.push_str(&text);
            }
        }
        StreamOutputItem::ExitCode(code) => {
            *exit_code = Some(code);
        }
        StreamOutputItem::Error { message, .. } => {
            output.push_str(&format!("[error] {}\n", message));
            *stream_error = true;
        }
        StreamOutputItem::GrepMatch(m) => {
            if !output.is_empty() && !output.ends_with('\n') {
                output.push('\n');
            }
            // ripgrep-like `path:line:content` (same spirit as TS content / match lines).
            use std::fmt::Write;
            let _ = write!(output, "{}:{}:{}", m.file, m.line, m.content);
        }
        StreamOutputItem::GlobMatch(m) => {
            if !output.is_empty() && !output.ends_with('\n') {
                output.push('\n');
            }
            output.push_str(&m.path);
        }
        StreamOutputItem::FileList(entries) => {
            for e in entries {
                if !output.is_empty() && !output.ends_with('\n') {
                    output.push('\n');
                }
                output.push_str(&e.path);
            }
        }
        StreamOutputItem::Metadata { key, value } => {
            if key == "truncated" && value == "true" {
                *truncated_note = true;
            }
        }
        StreamOutputItem::Start
        | StreamOutputItem::Complete
        | StreamOutputItem::Cancelled
        | StreamOutputItem::Thinking(_)
        | StreamOutputItem::ToolUse { .. }
        | StreamOutputItem::ToolResult { .. } => {}
    }
}

/// If grep/glob produced no text, use the same copy as `GrepTool` / `GlobTool` in `src/tools`.
fn apply_empty_structured_tool_placeholder(output: &mut String, tool_name: &str, had_error: bool) {
    if had_error || !output.trim().is_empty() {
        return;
    }
    match tool_name {
        "grep" => output.push_str("No matches found"),
        "glob" => output.push_str("No files found"),
        _ => {}
    }
}

/// Same trailing note as `GlobTool.mapToolResultToToolResultBlockParam` when `truncated` is true.
fn append_truncated_results_note(output: &mut String, truncated: bool) {
    if !truncated {
        return;
    }
    if !output.is_empty() && !output.ends_with('\n') {
        output.push('\n');
    }
    output.push_str(
        "(Results are truncated. Consider using a more specific path or pattern.)",
    );
}

/// Persist `todo_write` + V2 task list so the next `send_message` turn reloads from SQLite.
async fn persist_session_tool_state(
    sessions: &Arc<RwLock<HashMap<String, SessionRuntimeState>>>,
    repo: &Arc<Mutex<crate::domain::persistence::SessionRepository>>,
    session_id: &str,
) {
    let snapshots = {
        let sessions_guard = sessions.read().await;
        let Some(runtime) = sessions_guard.get(session_id) else {
            return;
        };
        let todos = runtime.todos.lock().await.clone();
        let tasks = runtime.agent_tasks.lock().await.clone();
        (todos, tasks)
    };
    let repo_guard = repo.lock().await;
    if let Err(e) = repo_guard
        .upsert_session_tool_state(session_id, &snapshots.0, &snapshots.1)
        .await
    {
        tracing::warn!("Failed to persist session tool state: {}", e);
    }
}

/// Send a message to Claude and get a streaming response
#[tauri::command]
pub async fn send_message(
    app: AppHandle,
    app_state: State<'_, OmigaAppState>,
    request: SendMessageRequest,
) -> CommandResult<MessageResponse> {
    // Get or create session (database is single source of truth)
    let repo = app_state.repo.lock().await;

    let (session_id, mut session, user_message_id, project_path) = if let Some(ref id) = request.session_id {
        // Load existing session from database
        let db_session = repo.get_session(id).await.map_err(|e| {
            OmigaError::Chat(ChatError::StreamError(format!("Failed to load session: {}", e)))
        })?;

        if let Some(db_session) = db_session {
            let mut session = SessionCodec::db_to_domain(db_session);
            session.add_user_message(&request.content);

            // Save user message to database
            let msg_id = uuid::Uuid::new_v4().to_string();
            let _now = chrono::Utc::now().to_rfc3339();
            repo.save_message(&msg_id, &session.id, "user", &request.content, None, None)
                .await
                .map_err(|e| {
                    OmigaError::Chat(ChatError::StreamError(format!("Failed to save message: {}", e)))
                })?;

            // Update session timestamp
            repo.touch_session(&session.id).await.ok();

            // Cache in memory — keep todo/task Arcs if already present; else load from SQLite
            {
                let mut sessions = app_state.chat.sessions.write().await;
                if let Some(runtime) = sessions.get_mut(&session.id) {
                    runtime.session = session.clone();
                    runtime.active_round_ids.clear();
                } else {
                    let (todos_v, tasks_v) = repo.get_session_tool_state(&session.id).await.map_err(
                        |e| {
                            OmigaError::Chat(ChatError::StreamError(format!(
                                "Failed to load session tool state: {}",
                                e
                            )))
                        },
                    )?;
                    sessions.insert(
                        session.id.clone(),
                        SessionRuntimeState {
                            session: session.clone(),
                            active_round_ids: vec![],
                            todos: Arc::new(tokio::sync::Mutex::new(todos_v)),
                            agent_tasks: Arc::new(tokio::sync::Mutex::new(tasks_v)),
                            plan_mode: Arc::new(Mutex::new(false)),
                        },
                    );
                }
            }

            let session_id_cloned = session.id.clone();
            let project_path_cloned = session.project_path.clone();
            (session_id_cloned, session, msg_id, project_path_cloned)
        } else {
            return Err(OmigaError::Chat(ChatError::StreamError(
                "Session not found".to_string(),
            )));
        }
    } else {
        // Create new session with explicit metadata
        let project_path = request.project_path.unwrap_or_else(|| ".".to_string());
        let session_name = request.session_name.unwrap_or_else(|| {
            request.content.chars().take(50).collect::<String>()
        });

        let mut session = Session::new(session_name, project_path);
        session.add_user_message(&request.content);

        // Save session to database
        repo.create_session(&session.id, &session.name, &session.project_path)
            .await
            .map_err(|e| {
                OmigaError::Chat(ChatError::StreamError(format!("Failed to create session: {}", e)))
            })?;

        // Save user message
        let msg_id = uuid::Uuid::new_v4().to_string();
        repo.save_message(&msg_id, &session.id, "user", &request.content, None, None)
            .await
            .map_err(|e| {
                OmigaError::Chat(ChatError::StreamError(format!("Failed to save message: {}", e)))
            })?;

        // Cache in memory
        let runtime_state = SessionRuntimeState {
            session: session.clone(),
            active_round_ids: vec![],
            todos: Arc::new(tokio::sync::Mutex::new(vec![])),
            agent_tasks: Arc::new(tokio::sync::Mutex::new(vec![])),
            plan_mode: Arc::new(Mutex::new(false)),
        };
        {
            let mut sessions = app_state.chat.sessions.write().await;
            sessions.insert(session.id.clone(), runtime_state);
        }

        let session_id_cloned = session.id.clone();
        let project_path_cloned = session.project_path.clone();
        (session_id_cloned, session, msg_id, project_path_cloned)
    };

    let project_root = resolve_session_project_root(&project_path);
    let mut llm_config = get_llm_config(&app_state.chat).await?;
    let integrations_cfg = integrations_config::load_integrations_config(&project_root);
    let include_claude_user_skills =
        load_claude_user_skills_enabled_from_repo(&repo).await;
    let skill_entries =
        skills::load_skills_for_project(&project_root, include_claude_user_skills).await;
    let skill_entries =
        integrations_config::filter_skill_entries(skill_entries, &integrations_cfg);

    // Ported agent system prompt from `src/constants/prompts.ts` — injected when tools are enabled.
    let mut prompt_parts: Vec<String> = Vec::new();
    if request.use_tools {
        prompt_parts.push(agent_prompt::build_system_prompt(
            &project_root,
            &llm_config.model,
        ));
    }
    if let Some(ref u) = llm_config.system_prompt {
        let t = u.trim();
        if !t.is_empty() {
            prompt_parts.push(t.to_string());
        }
    }
    if !skill_entries.is_empty() {
        prompt_parts.push(skills::format_skills_discovery_system_section());
        if let Some(task_sec) =
            skills::format_skills_task_relevance_section(&skill_entries, &request.content)
        {
            prompt_parts.push(task_sec);
        }
    }
    llm_config.system_prompt = if prompt_parts.is_empty() {
        None
    } else {
        Some(prompt_parts.join("\n\n"))
    };

    let compact_outcome = crate::domain::auto_compact::compact_session_and_persist(
        &repo,
        &session_id,
        &mut session,
        &llm_config,
        request.use_tools,
        &user_message_id,
    )
    .await
    .map_err(|e| {
        OmigaError::Chat(ChatError::StreamError(format!(
            "Auto-compact persist failed: {}",
            e
        )))
    })?;

    let user_message_id_for_round = compact_outcome
        .as_ref()
        .map(|p| p.last_user_message_id.clone())
        .unwrap_or_else(|| user_message_id.clone());

    {
        let mut sessions = app_state.chat.sessions.write().await;
        if let Some(runtime) = sessions.get_mut(&session_id) {
            runtime.session = session.clone();
        }
    }

    let messages = SessionCodec::to_api_messages(&session.messages);

    let llm_config_for_agent = llm_config.clone();
    let client = create_client(llm_config)?;

    let compact_log_for_stream = compact_outcome.map(|p| p.log_line);

    // Generate round and message IDs
    let round_id = uuid::Uuid::new_v4().to_string();
    let message_id = uuid::Uuid::new_v4().to_string();

    // Create conversation round record
    repo.create_round(
        &round_id,
        &session_id,
        &message_id,
        Some(&user_message_id_for_round),
    )
        .await
        .map_err(|e| {
            OmigaError::Chat(ChatError::StreamError(format!("Failed to create round: {}", e)))
        })?;

    drop(repo); // Release lock

    // Set up cancellation tracking
    let cancel_flag = Arc::new(RwLock::new(false));
    let cancellation_state = RoundCancellationState {
        round_id: round_id.clone(),
        message_id: message_id.clone(),
        session_id: session_id.clone(),
        cancelled: cancel_flag.clone(),
    };

    {
        let mut active_rounds = app_state.chat.active_rounds.lock().await;
        active_rounds.insert(message_id.clone(), cancellation_state);
    }

    // Update runtime state with active round
    {
        let mut sessions = app_state.chat.sessions.write().await;
        if let Some(runtime) = sessions.get_mut(&session_id) {
            runtime.active_round_ids.push(round_id.clone());
        }
    }

    // Prepare tools if enabled (`list_skills` + `skill` when skills exist on disk).
    // Merge MCP `tools/list` from Omiga MCP config (stdio / HTTP), same naming as Claude Code (`mcp__server__tool`).
    // Filter with `permissions.deny` from Claude-style settings (`filterToolsByDenyRules` parity).
    let tools: Vec<ToolSchema> = if request.use_tools {
        let deny_entries =
            crate::domain::tool_permission_rules::load_merged_permission_deny_rule_entries(
                &project_root,
            );
        crate::domain::tool_permission_rules::validate_permission_deny_entries(&deny_entries);
        let all_schemas = all_tool_schemas(!skill_entries.is_empty());
        let n_builtin_before = all_schemas.len();
        let mut built = crate::domain::tool_permission_rules::filter_tool_schemas_by_deny_rule_entries(
            all_schemas,
            &deny_entries,
        );
        let n_builtin_after = built.len();
        if n_builtin_after < n_builtin_before {
            tracing::debug!(
                target: "omiga::permissions",
                before = n_builtin_before,
                after = n_builtin_after,
                "built-in tool schemas after permissions.deny filter"
            );
        }
        built.sort_by(|a, b| a.name.cmp(&b.name));
        let base_names: HashSet<String> = built.iter().map(|t| t.name.clone()).collect();
        let mcp_timeout = std::time::Duration::from_secs(45);
        let mcp_tools =
            crate::domain::mcp_tool_pool::discover_mcp_tool_schemas(&project_root, mcp_timeout)
                .await;
        let n_mcp_before = mcp_tools.len();
        let mcp_after_deny = crate::domain::tool_permission_rules::filter_tool_schemas_by_deny_rule_entries(
            mcp_tools,
            &deny_entries,
        );
        let n_mcp_after = mcp_after_deny.len();
        if n_mcp_after < n_mcp_before {
            tracing::debug!(
                target: "omiga::permissions",
                before = n_mcp_before,
                after = n_mcp_after,
                "MCP tool schemas after permissions.deny filter"
            );
        }
        let mcp_filtered: Vec<_> = mcp_after_deny
            .into_iter()
            .filter(|t| !base_names.contains(&t.name))
            .collect();
        let mcp_filtered = integrations_config::filter_mcp_tools_by_integrations(
            mcp_filtered,
            &integrations_cfg,
        );
        built.into_iter().chain(mcp_filtered).collect()
    } else {
        vec![]
    };

    // Convert messages to LLM format
    let llm_messages: Vec<LlmMessage> = messages
        .iter()
        .map(|msg| LlmMessage {
            role: match msg.role {
                Role::User => LlmRole::User,
                Role::Assistant => LlmRole::Assistant,
            },
            content: msg.content.iter().map(|block| {
                match block {
                    ContentBlock::Text { text } => LlmContent::Text { text: text.clone() },
                    ContentBlock::ToolUse { id, name, input } => LlmContent::ToolUse {
                        id: id.clone(),
                        name: name.clone(),
                        arguments: input.clone(),
                    },
                    ContentBlock::ToolResult { tool_use_id, content, is_error } => LlmContent::ToolResult {
                        tool_use_id: tool_use_id.clone(),
                        content: content.clone(),
                        is_error: *is_error,
                    },
                }
            }).collect(),
            name: None,
            tool_calls: None,
        })
        .collect();

    // Start streaming in background
    let app_clone = app.clone();
    let message_id_clone = message_id.clone();
    let round_id_clone = round_id.clone();
    let session_id_clone = session_id.clone();
    let pending_tools_clone = app_state.chat.pending_tools.clone();
    let active_rounds_clone = app_state.chat.active_rounds.clone();
    let sessions_clone = app_state.chat.sessions.clone();
    let repo_clone = app_state.repo.clone();
    let llm_config_for_spawn = llm_config_for_agent;
    let skill_task_context = request.content.clone();
    let brave_search_api_key = app_state.chat.brave_search_api_key.lock().await.clone();

    tokio::spawn(async move {
        let _ = app_clone.emit(
            &format!("chat-stream-{}", message_id_clone),
            &StreamOutputItem::Start,
        );

        if let Some(note) = compact_log_for_stream {
            let _ = app_clone.emit(
                &format!("chat-stream-{}", message_id_clone),
                &StreamOutputItem::Metadata {
                    key: "omiga_auto_compact".to_string(),
                    value: note,
                },
            );
        }

        let tool_results_dir = tool_results_dir_for_session(&app_clone, &session_id_clone);

        let plan_mode_flag = sessions_clone
            .read()
            .await
            .get(&session_id_clone)
            .map(|s| s.plan_mode.clone());

        let agent_runtime = AgentLlmRuntime {
            llm_config: llm_config_for_spawn.clone(),
            round_id: round_id_clone.clone(),
            cancel_flag: cancel_flag.clone(),
            pending_tools: pending_tools_clone.clone(),
            repo: repo_clone.clone(),
            plan_mode_flag,
            allow_nested_agent: env_allow_nested_agent(),
        };

        // Stream the response with cancellation support
        let (mut pending_tool_calls, assistant_text, was_cancelled) = match stream_llm_response_with_cancel(
            client.as_ref(),
            &app_clone,
            &message_id_clone,
            &round_id_clone,
            &llm_messages,
            &tools,
            &pending_tools_clone,
            &cancel_flag,
            repo_clone.clone(),
        )
        .await
        {
            Ok(result) => result,
            Err(e) => {
                let repo = repo_clone.lock().await;
                let _ = repo.cancel_round(&round_id_clone, Some(&e.to_string())).await;

                let _ = app_clone.emit(
                    &format!("chat-stream-{}", message_id_clone),
                    &StreamOutputItem::Error {
                        message: e.to_string(),
                        code: None,
                    },
                );
                return;
            }
        };

        {
            let mut active_rounds = active_rounds_clone.lock().await;
            active_rounds.remove(&message_id_clone);
        }

        if was_cancelled {
            persist_session_tool_state(&sessions_clone, &repo_clone, &session_id_clone).await;
            let _ = app_clone.emit(
                &format!("chat-stream-{}", message_id_clone),
                &StreamOutputItem::Text("\n\n[Cancelled]".to_string()),
            );
            let _ = app_clone.emit(
                &format!("chat-stream-{}", message_id_clone),
                &StreamOutputItem::Cancelled,
            );
            return;
        }

        // First assistant turn: persist with tool_calls JSON for reload
        let assistant_msg_id = uuid::Uuid::new_v4().to_string();
        let tool_calls_json = tool_calls_json_opt(&pending_tool_calls);
        {
            let repo = repo_clone.lock().await;
            if let Err(e) = repo
                .save_message(
                    &assistant_msg_id,
                    &session_id_clone,
                    "assistant",
                    &assistant_text,
                    tool_calls_json.as_deref(),
                    None,
                )
                .await
            {
                tracing::warn!("Failed to save assistant message: {}", e);
            }
        }

        {
            let mut sessions = sessions_clone.write().await;
            if let Some(runtime) = sessions.get_mut(&session_id_clone) {
                let tc = completed_to_tool_calls(&pending_tool_calls);
                runtime
                    .session
                    .add_assistant_message_with_tools(&assistant_text, tc);
            }
        }

        let mut last_assistant_id = assistant_msg_id.clone();

        if pending_tool_calls.is_empty() {
            persist_session_tool_state(&sessions_clone, &repo_clone, &session_id_clone).await;
            let repo = repo_clone.lock().await;
            if let Err(e) = repo
                .complete_round(&round_id_clone, Some(&last_assistant_id))
                .await
            {
                tracing::warn!("Failed to complete round: {}", e);
            }
            let _ = app_clone.emit(
                &format!("chat-stream-{}", message_id_clone),
                &StreamOutputItem::Complete,
            );
            return;
        }

        for _round_idx in 0..MAX_TOOL_ROUNDS {
            let (project_root, todos_for_tools, agent_tasks_for_tools) = {
                let sessions = sessions_clone.read().await;
                let project_root = sessions
                    .get(&session_id_clone)
                    .map(|r| resolve_session_project_root(&r.session.project_path))
                    .unwrap_or_else(|| {
                        std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
                    });
                let todos = sessions
                    .get(&session_id_clone)
                    .map(|r| r.todos.clone());
                let agent_tasks = sessions
                    .get(&session_id_clone)
                    .map(|r| r.agent_tasks.clone());
                (project_root, todos, agent_tasks)
            };

            let tool_results = execute_tool_calls(
                &pending_tool_calls,
                &app_clone,
                &message_id_clone,
                &session_id_clone,
                &tool_results_dir,
                &project_root,
                todos_for_tools,
                agent_tasks_for_tools,
                Some(&agent_runtime),
                0,
                Some(skill_task_context.as_str()),
                brave_search_api_key.clone(),
            )
            .await;

            {
                let repo = repo_clone.lock().await;
                for (tool_use_id, output, _) in &tool_results {
                    let msg_id = uuid::Uuid::new_v4().to_string();
                    if let Err(e) = repo
                        .save_message(
                            &msg_id,
                            &session_id_clone,
                            "tool",
                            output,
                            None,
                            Some(tool_use_id),
                        )
                        .await
                    {
                        tracing::warn!("Failed to save tool result: {}", e);
                    }
                }
            }

            {
                let mut sessions = sessions_clone.write().await;
                if let Some(runtime) = sessions.get_mut(&session_id_clone) {
                    for (tool_use_id, output, _) in &tool_results {
                        runtime.session.add_tool_result(tool_use_id, output);
                    }
                }
            }

            persist_session_tool_state(&sessions_clone, &repo_clone, &session_id_clone).await;

            // Shrink history before the next model call when tool rounds push toward context limits.
            {
                let mut sessions = sessions_clone.write().await;
                if let Some(runtime) = sessions.get_mut(&session_id_clone) {
                    let repo = repo_clone.lock().await;
                    if let Err(e) = crate::domain::auto_compact::compact_session_and_persist(
                        &repo,
                        &session_id_clone,
                        &mut runtime.session,
                        &llm_config_for_spawn,
                        !tools.is_empty(),
                        "",
                    )
                    .await
                    {
                        tracing::warn!(
                            target: "omiga::auto_compact",
                            "tool-loop auto-compact failed: {}",
                            e
                        );
                    }
                }
            }

            let updated_messages = {
                let sessions = sessions_clone.read().await;
                if let Some(runtime) = sessions.get(&session_id_clone) {
                    SessionCodec::to_api_messages(&runtime.session.messages)
                } else {
                    let repo = repo_clone.lock().await;
                    if let Ok(Some(db_session)) = repo.get_session(&session_id_clone).await {
                        let session = SessionCodec::db_to_domain(db_session);
                        SessionCodec::to_api_messages(&session.messages)
                    } else {
                        vec![]
                    }
                }
            };

            let updated_llm_messages: Vec<LlmMessage> = api_messages_to_llm(&updated_messages);

            let (next_tools, next_text, follow_cancelled) = match stream_llm_response_with_cancel(
                client.as_ref(),
                &app_clone,
                &message_id_clone,
                &round_id_clone,
                &updated_llm_messages,
                &tools,
                &pending_tools_clone,
                &cancel_flag,
                repo_clone.clone(),
            )
            .await
            {
                Ok(r) => r,
                Err(e) => {
                    let repo = repo_clone.lock().await;
                    let _ = repo.cancel_round(&round_id_clone, Some(&e.to_string())).await;
                    let _ = app_clone.emit(
                        &format!("chat-stream-{}", message_id_clone),
                        &StreamOutputItem::Error {
                            message: e.to_string(),
                            code: None,
                        },
                    );
                    return;
                }
            };

            if follow_cancelled {
                persist_session_tool_state(&sessions_clone, &repo_clone, &session_id_clone).await;
                let _ = app_clone.emit(
                    &format!("chat-stream-{}", message_id_clone),
                    &StreamOutputItem::Text("\n\n[Cancelled]".to_string()),
                );
                let _ = app_clone.emit(
                    &format!("chat-stream-{}", message_id_clone),
                    &StreamOutputItem::Cancelled,
                );
                return;
            }

            let next_assistant_id = uuid::Uuid::new_v4().to_string();
            let next_tc_json = tool_calls_json_opt(&next_tools);
            {
                let repo = repo_clone.lock().await;
                if let Err(e) = repo
                    .save_message(
                        &next_assistant_id,
                        &session_id_clone,
                        "assistant",
                        &next_text,
                        next_tc_json.as_deref(),
                        None,
                    )
                    .await
                {
                    tracing::warn!("Failed to save follow-up assistant: {}", e);
                }
            }

            {
                let mut sessions = sessions_clone.write().await;
                if let Some(runtime) = sessions.get_mut(&session_id_clone) {
                    let tc = completed_to_tool_calls(&next_tools);
                    runtime.session.add_assistant_message_with_tools(&next_text, tc);
                }
            }

            last_assistant_id = next_assistant_id.clone();
            pending_tool_calls = next_tools;

            if pending_tool_calls.is_empty() {
                persist_session_tool_state(&sessions_clone, &repo_clone, &session_id_clone).await;
                let repo = repo_clone.lock().await;
                if let Err(e) = repo
                    .complete_round(&round_id_clone, Some(&last_assistant_id))
                    .await
                {
                    tracing::warn!("Failed to complete round: {}", e);
                }
                let _ = app_clone.emit(
                    &format!("chat-stream-{}", message_id_clone),
                    &StreamOutputItem::Complete,
                );
                return;
            }
        }

        persist_session_tool_state(&sessions_clone, &repo_clone, &session_id_clone).await;

        let _ = app_clone.emit(
            &format!("chat-stream-{}", message_id_clone),
            &StreamOutputItem::Text(format!(
                "\n\n[Stopped: exceeded {} tool rounds]\n",
                MAX_TOOL_ROUNDS
            )),
        );
        let repo = repo_clone.lock().await;
        let _ = repo
            .complete_round(&round_id_clone, Some(&last_assistant_id))
            .await;
        let _ = app_clone.emit(
            &format!("chat-stream-{}", message_id_clone),
            &StreamOutputItem::Complete,
        );
    });

    Ok(MessageResponse {
        message_id,
        session_id,
        round_id,
    })
}

/// Emit full `ToolUse` and append to `completed_tool_calls` when a tool block ends.
/// Call when `BlockStop` fires, when a new `ToolStart` supersedes the previous tool, or when the stream ends without `BlockStop` (provider quirk).
async fn finalize_pending_tool_by_id(
    app: &AppHandle,
    message_id: &str,
    pending_tools: &Arc<Mutex<HashMap<String, PendingToolCall>>>,
    id: &str,
    completed_tool_calls: &mut Vec<(String, String, String)>,
) -> bool {
    let tool = {
        let mut pending = pending_tools.lock().await;
        pending.remove(id)
    };
    let Some(tool) = tool else {
        return false;
    };
    let args = tool.arguments.join("");
    completed_tool_calls.push((
        tool.id.clone(),
        tool.name.clone(),
        args.clone(),
    ));
    let _ = app.emit(
        &format!("chat-stream-{}", message_id),
        &StreamOutputItem::ToolUse {
            id: tool.id.clone(),
            name: tool.name.clone(),
            arguments: args,
        },
    );
    true
}

/// Stream LLM response and collect tool calls with cancellation support
/// Returns: (tool_calls, assistant_text, was_cancelled)
async fn stream_llm_response_with_cancel(
    client: &dyn LlmClient,
    app: &AppHandle,
    message_id: &str,
    round_id: &str,
    messages: &[LlmMessage],
    tools: &[ToolSchema],
    pending_tools: &Arc<Mutex<HashMap<String, PendingToolCall>>>,
    cancel_flag: &Arc<RwLock<bool>>,
    repo: Arc<Mutex<crate::domain::persistence::SessionRepository>>,
) -> Result<(Vec<(String, String, String)>, String, bool), OmigaError> {
    use futures::StreamExt;

    let stream = client
        .send_message_streaming(messages.to_vec(), tools.to_vec())
        .await
        .map_err(|e| OmigaError::Chat(ChatError::StreamError(e.to_string())))?;

    let mut stream = stream;
    let mut assistant_text = String::new();
    let mut completed_tool_calls: Vec<(String, String, String)> = Vec::new();
    let mut current_tool_id: Option<String> = None;
    let mut was_cancelled = false;

    // Mark round as partial after receiving first chunk
    let mut marked_partial = false;

    while let Some(result) = stream.next().await {
        // Check cancellation flag
        if *cancel_flag.read().await {
            was_cancelled = true;
            // Mark round as cancelled in database
            let repo = repo.lock().await;
            let _ = repo.cancel_round(round_id, Some("User cancelled")).await;
            break;
        }

        match result {
            Ok(chunk) => {
                match chunk {
                    LlmStreamChunk::Text(text) => {
                        if !marked_partial && !text.is_empty() {
                            // Mark as partial in database
                            let repo = repo.lock().await;
                            let _ = repo.mark_round_partial(round_id, None).await;
                            marked_partial = true;
                        }
                        assistant_text.push_str(&text);
                        let _ = app.emit(
                            &format!("chat-stream-{}", message_id),
                            &StreamOutputItem::Text(text),
                        );
                    }
                    LlmStreamChunk::ToolStart { id, name } => {
                        // Some streams start the next tool without BlockStop; finalize the previous one.
                        if let Some(prev_id) = current_tool_id.take() {
                            if prev_id != id {
                                let _ = finalize_pending_tool_by_id(
                                    app,
                                    message_id,
                                    pending_tools,
                                    &prev_id,
                                    &mut completed_tool_calls,
                                )
                                .await;
                            }
                        }
                        let mut pending = pending_tools.lock().await;
                        pending.insert(
                            id.clone(),
                            PendingToolCall {
                                id: id.clone(),
                                name: name.clone(),
                                arguments: Vec::new(),
                            },
                        );
                        current_tool_id = Some(id.clone());

                        let _ = app.emit(
                            &format!("chat-stream-{}", message_id),
                            &StreamOutputItem::ToolUse {
                                id: id.clone(),
                                name: name.clone(),
                                arguments: String::new(),
                            },
                        );
                    }
                    LlmStreamChunk::ToolArguments(json) => {
                        // Collect JSON fragments
                        if let Some(ref id) = current_tool_id {
                            let mut pending = pending_tools.lock().await;
                            if let Some(tool) = pending.get_mut(id) {
                                tool.arguments.push(json);
                            }
                        }
                    }
                    LlmStreamChunk::BlockStop => {
                        if let Some(id) = current_tool_id.take() {
                            let _ = finalize_pending_tool_by_id(
                                app,
                                message_id,
                                pending_tools,
                                &id,
                                &mut completed_tool_calls,
                            )
                            .await;
                        }
                    }
                    LlmStreamChunk::Stop { stop_reason: _ } => break,
                    _ => {}
                }
            }
            Err(e) => {
                return Err(OmigaError::Chat(ChatError::StreamError(e.to_string())));
            }
        }
    }

    // Stream ended without BlockStop for the last tool (e.g. OpenAI sends [DONE] before finish_reason in some buffers).
    if !was_cancelled {
        let leftover_ids: Vec<String> = {
            let pending = pending_tools.lock().await;
            pending.keys().cloned().collect()
        };
        for lid in leftover_ids {
            let _ = finalize_pending_tool_by_id(
                app,
                message_id,
                pending_tools,
                &lid,
                &mut completed_tool_calls,
            )
            .await;
        }
    }

    Ok((completed_tool_calls, assistant_text, was_cancelled))
}

fn is_agent_tool_name(name: &str) -> bool {
    matches!(name, "Agent" | "Task" | "agent" | "task")
}

/// Parity with TS `getAgentModel` (`src/utils/model/agent.ts`): env override, `inherit`, and
/// `aliasMatchesParentTier` (sonnet/opus/haiku inherits parent's exact model id when same tier).
fn resolve_subagent_model(base: &LlmConfig, alias: Option<&str>) -> String {
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

fn subagent_alias_matches_parent_tier(alias: &str, parent_model: &str) -> bool {
    let p = parent_model.to_ascii_lowercase();
    match alias.to_ascii_lowercase().as_str() {
        "opus" | "claude-opus" => p.contains("opus"),
        "sonnet" | "claude-sonnet" => p.contains("sonnet"),
        "haiku" | "claude-haiku" => p.contains("haiku"),
        _ => false,
    }
}

fn resolve_agent_cwd(project_root: &Path, cwd: Option<&str>) -> PathBuf {
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

async fn build_subagent_tool_schemas(
    project_root: &Path,
    include_skill: bool,
    subagent_opts: SubagentFilterOptions,
) -> Vec<ToolSchema> {
    let integrations_cfg = integrations_config::load_integrations_config(project_root);
    let deny_entries =
        crate::domain::tool_permission_rules::load_merged_permission_deny_rule_entries(
            project_root,
        );
    crate::domain::tool_permission_rules::validate_permission_deny_entries(&deny_entries);
    let built = crate::domain::tool_permission_rules::filter_tool_schemas_by_deny_rule_entries(
        all_tool_schemas(include_skill),
        &deny_entries,
    );
    let mut built =
        crate::domain::subagent_tool_filter::filter_tool_schemas_for_subagent(built, subagent_opts);
    built.sort_by(|a, b| a.name.cmp(&b.name));
    let base_names: HashSet<String> = built.iter().map(|t| t.name.clone()).collect();
    let mcp_timeout = std::time::Duration::from_secs(45);
    let mcp_tools =
        crate::domain::mcp_tool_pool::discover_mcp_tool_schemas(project_root, mcp_timeout).await;
    let mcp_after_deny = crate::domain::tool_permission_rules::filter_tool_schemas_by_deny_rule_entries(
        mcp_tools,
        &deny_entries,
    );
    let mcp_filtered: Vec<_> = mcp_after_deny
        .into_iter()
        .filter(|t| !base_names.contains(&t.name))
        .collect();
    let mcp_filtered = integrations_config::filter_mcp_tools_by_integrations(
        mcp_filtered,
        &integrations_cfg,
    );
    built.into_iter().chain(mcp_filtered).collect()
}

/// Isolated sub-agent loop (same API key / stream channel as parent round).
async fn run_subagent_session(
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
    brave_search_api_key: Option<String>,
) -> Result<String, String> {
    if args.run_in_background == Some(true) {
        return Err(
            "`run_in_background` is not supported for the Agent tool in Omiga yet.".to_string(),
        );
    }
    let effective_root = resolve_agent_cwd(project_root, args.cwd.as_deref());
    let subagent_skill_task_context = format!("{} {}", args.description.trim(), args.prompt.trim());
    let mut sub_cfg = runtime.llm_config.clone();
    sub_cfg.model = resolve_subagent_model(&runtime.llm_config, args.model.as_deref());
    let integrations_cfg = integrations_config::load_integrations_config(&effective_root);
    let include_claude_user_skills = {
        let repo = runtime.repo.lock().await;
        load_claude_user_skills_enabled_from_repo(&repo).await
    };
    let skill_entries =
        skills::load_skills_for_project(&effective_root, include_claude_user_skills).await;
    let skill_entries =
        integrations_config::filter_skill_entries(skill_entries, &integrations_cfg);
    let mut prompt_parts: Vec<String> = Vec::new();
    prompt_parts.push(agent_prompt::build_system_prompt(
        &effective_root,
        &sub_cfg.model,
    ));
    let parent_in_plan = if let Some(ref pm) = runtime.plan_mode_flag {
        *pm.lock().await
    } else {
        false
    };
    let nested_agent_note = if runtime.allow_nested_agent {
        " Nested `Agent` is allowed when `USER_TYPE=ant`."
    } else {
        ""
    };
    let exit_plan_note = if parent_in_plan {
        " `ExitPlanMode` is available while the parent session is in plan mode."
    } else {
        ""
    };
    prompt_parts.push(format!(
        "## Sub-agent mode\nYou are an isolated sub-agent (Claude Code parity). \
         Use tools as needed. Disallowed tools match `ALL_AGENT_DISALLOWED_TOOLS`: \
         TaskOutput, EnterPlanMode, ExitPlanMode (unless in plan mode), AskUserQuestion, TaskStop. \
         {exit_plan_note}{nested_agent_note}"
    ));
    if let Some(ref u) = sub_cfg.system_prompt {
        let t = u.trim();
        if !t.is_empty() {
            prompt_parts.push(t.to_string());
        }
    }
    if !skill_entries.is_empty() {
        prompt_parts.push(skills::format_skills_discovery_system_section());
        let task_blob = format!("{} {}", args.description.trim(), args.prompt.trim());
        if let Some(task_sec) =
            skills::format_skills_task_relevance_section(&skill_entries, &task_blob)
        {
            prompt_parts.push(task_sec);
        }
    }
    sub_cfg.system_prompt = Some(prompt_parts.join("\n\n"));
    let client = create_client(sub_cfg).map_err(|e| e.to_string())?;
    let subagent_opts = SubagentFilterOptions {
        parent_in_plan_mode: parent_in_plan,
        allow_nested_agent: runtime.allow_nested_agent,
    };
    let tools = build_subagent_tool_schemas(
        &effective_root,
        !skill_entries.is_empty(),
        subagent_opts,
    )
    .await;
    let user_text = format!(
        "## Sub-agent task: {}\n\n{}",
        args.description.trim(),
        args.prompt.trim()
    );
    let mut transcript: Vec<Message> = vec![Message::User { content: user_text }];

    for _round_idx in 0..MAX_SUBAGENT_TOOL_ROUNDS {
        if *runtime.cancel_flag.read().await {
            return Err("Sub-agent cancelled.".to_string());
        }
        let api_msgs = SessionCodec::to_api_messages(&transcript);
        let llm_messages = api_messages_to_llm(&api_msgs);
        let (tool_calls, assistant_text, cancelled) = stream_llm_response_with_cancel(
            client.as_ref(),
            app,
            message_id,
            &runtime.round_id,
            &llm_messages,
            &tools,
            &runtime.pending_tools,
            &runtime.cancel_flag,
            runtime.repo.clone(),
        )
        .await
        .map_err(|e| e.to_string())?;
        if cancelled {
            return Err("Sub-agent cancelled.".to_string());
        }
        let tc = completed_to_tool_calls(&tool_calls);
        transcript.push(Message::Assistant {
            content: assistant_text.clone(),
            tool_calls: tc.clone(),
        });
        if tool_calls.is_empty() {
            return Ok(assistant_text);
        }
        let results = execute_tool_calls(
            &tool_calls,
            app,
            message_id,
            session_id,
            tool_results_dir,
            &effective_root,
            session_todos.clone(),
            session_agent_tasks.clone(),
            Some(runtime),
            subagent_execute_depth,
            Some(subagent_skill_task_context.as_str()),
            brave_search_api_key.clone(),
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
        "Sub-agent exceeded maximum tool rounds ({MAX_SUBAGENT_TOOL_ROUNDS})."
    ))
}

/// Execute tool calls and return results
#[async_recursion::async_recursion]
async fn execute_tool_calls(
    tool_calls: &[(String, String, String)], // (id, name, arguments)
    app: &AppHandle,
    message_id: &str,
    session_id: &str,
    tool_results_dir: &Path,
    project_root: &std::path::Path,
    session_todos: Option<Arc<tokio::sync::Mutex<Vec<TodoItem>>>>,
    session_agent_tasks: Option<Arc<tokio::sync::Mutex<Vec<AgentTask>>>>,
    agent_runtime: Option<&AgentLlmRuntime>,
    subagent_depth: u8,
    // Task text for `list_skills` ordering (main user message or sub-agent description+prompt).
    skill_task_context: Option<&str>,
    brave_search_api_key: Option<String>,
) -> Vec<(String, String, bool)> {
    // (tool_use_id, output, is_error)
    let mut results = Vec::new();
    let include_claude_user_skills =
        load_claude_user_skills_enabled_from_agent_runtime(agent_runtime).await;
    let deny_entries =
        crate::domain::tool_permission_rules::load_merged_permission_deny_rule_entries(project_root);

    for (tool_use_id, tool_name, arguments) in tool_calls {
        if let Some(hit) =
            crate::domain::tool_permission_rules::matching_deny_entry(tool_name, &deny_entries)
        {
            tracing::debug!(
                target: "omiga::permissions",
                tool = %tool_name,
                matched_rule = %hit.rule,
                source = %hit.source.display(),
                "tool call blocked at execute time by permissions.deny"
            );
            let error_msg = format!(
                "Tool `{tool_name}` is denied by `permissions.deny` (rule `{}` from {}).",
                hit.rule,
                hit.source.display()
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
            results.push((tool_use_id.clone(), error_msg, true));
            continue;
        }

        // TS `filterToolsForAgent` / `ALL_AGENT_DISALLOWED_TOOLS` — block at execute time if the model still calls these.
        if subagent_depth > 0 {
            let c = crate::domain::tool_permission_rules::canonical_permission_tool_name(tool_name);
            let parent_in_plan = if let Some(ar) = agent_runtime {
                if let Some(ref pm) = ar.plan_mode_flag {
                    *pm.lock().await
                } else {
                    false
                }
            } else {
                false
            };
            let sub_opts = SubagentFilterOptions {
                parent_in_plan_mode: parent_in_plan,
                allow_nested_agent: agent_runtime
                    .map(|r| r.allow_nested_agent)
                    .unwrap_or(false),
            };
            if crate::domain::subagent_tool_filter::should_block_subagent_builtin_call(&c, sub_opts)
            {
                let error_msg = format!(
                    "Tool `{tool_name}` is not available to sub-agents (Claude Code `ALL_AGENT_DISALLOWED_TOOLS`)."
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
                results.push((tool_use_id.clone(), error_msg, true));
                continue;
            }
        }

        // Parse and execute the tool
        let result = if tool_name.eq_ignore_ascii_case("list_skills") {
            let args: ListSkillsArgs = if arguments.trim().is_empty() {
                ListSkillsArgs::default()
            } else {
                serde_json::from_str(arguments).unwrap_or_default()
            };
            let icfg = integrations_config::load_integrations_config(project_root);
            let mut all_skills =
                skills::load_skills_for_project(project_root, include_claude_user_skills).await;
            all_skills = integrations_config::filter_skill_entries(all_skills, &icfg);
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
                json,
                tool_use_id,
                tool_results_dir,
            )
            .await;
            (tool_use_id.clone(), model_output, is_error)
        } else if tool_name.eq_ignore_ascii_case("skill") || tool_name == "Skill" {
            match serde_json::from_str::<SkillToolArgs>(arguments) {
                Ok(args) => {
                    if args.skill.trim().is_empty() {
                        let error_msg = "skill tool: missing or empty `skill` field".to_string();
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
                        let all_skills = skills::load_skills_for_project(
                            project_root,
                            include_claude_user_skills,
                        )
                        .await;
                        let blocked =
                            if let Some(ref display) =
                                skills::resolve_skill_display_name(&all_skills, &args.skill)
                            {
                                integrations_config::is_skill_name_disabled(&icfg, display)
                            } else {
                                false
                            };
                        if blocked {
                            let display =
                                skills::resolve_skill_display_name(&all_skills, &args.skill)
                                    .unwrap_or_else(|| args.skill.trim().to_string());
                            let error_msg = format!(
                                "Skill `{display}` is disabled in Omiga Settings → Integrations (Skills)."
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
                            match skills::invoke_skill(
                                project_root,
                                &args.skill,
                                &args.args,
                                include_claude_user_skills,
                            )
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
                                let error_msg = e;
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
                let error_msg = format!(
                    "Agent tool: maximum nested depth ({MAX_SUBAGENT_EXECUTE_DEPTH}) reached."
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
                        match run_subagent_session(
                            app,
                            message_id,
                            session_id,
                            tool_results_dir,
                            project_root,
                            session_todos.clone(),
                            session_agent_tasks.clone(),
                            &agent_args,
                            ar,
                            subagent_depth.saturating_add(1),
                            brave_search_api_key.clone(),
                        )
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
        } else if tool_name.starts_with("mcp__") {
            let timeout = std::time::Duration::from_secs(120);
            match crate::domain::mcp_tool_dispatch::execute_mcp_tool_call(
                project_root,
                tool_name,
                arguments,
                timeout,
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
            let ctx = ToolContext::new(project_root.to_path_buf())
                .with_todos(session_todos.clone())
                .with_agent_tasks(session_agent_tasks.clone())
                .with_plan_mode(agent_runtime.and_then(|r| r.plan_mode_flag.clone()))
                .with_brave_search_api_key(brave_search_api_key.clone())
                .with_tool_results_dir(tool_results_dir.to_path_buf())
                .with_background_shell(
                    crate::domain::background_shell::BackgroundShellHandle {
                        app: app.clone(),
                        chat_stream_event: format!("chat-stream-{}", message_id),
                        session_id: session_id.to_string(),
                        tool_use_id: tool_use_id.clone(),
                    },
                    tool_results_dir.to_path_buf(),
                );
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

                        let is_error =
                            stream_error || exit_code.map(|c| c != 0).unwrap_or(false);

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

        results.push(result);
    }

    results
}

/// Cancel an in-progress stream by message_id
#[tauri::command]
pub async fn cancel_stream(
    app_state: State<'_, OmigaAppState>,
    message_id: String,
) -> CommandResult<()> {
    // Look up the round by message_id
    let repo = app_state.repo.lock().await;

    // Find active round
    if let Ok(Some(round)) = repo.get_round_by_message_id(&message_id).await {
        if round.is_active() {
            // Cancel in database
            repo.cancel_round(&round.id, Some("User requested cancellation"))
                .await
                .map_err(|e| {
                    OmigaError::Chat(ChatError::StreamError(format!("Failed to cancel round: {}", e)))
                })?;

            drop(repo);

            // Set cancellation flag for in-memory tracking
            let active_rounds = app_state.chat.active_rounds.lock().await;
            if let Some(round_state) = active_rounds.get(&message_id) {
                let mut cancelled = round_state.cancelled.write().await;
                *cancelled = true;
            }

            tracing::info!("Cancelled round {} for message {}", round.id, message_id);
        }
    } else {
        // Try to cancel by looking up in active rounds directly
        let active_rounds = app_state.chat.active_rounds.lock().await;
        if let Some(round_state) = active_rounds.get(&message_id) {
            let mut cancelled = round_state.cancelled.write().await;
            *cancelled = true;
            drop(cancelled);

            // Also mark in database
            let round_id = round_state.round_id.clone();
            drop(active_rounds);

            let repo = app_state.repo.lock().await;
            let _ = repo.cancel_round(&round_id, Some("User requested cancellation")).await;
        }
    }

    Ok(())
}

/// Cancel all active rounds for a session (used when closing session)
#[tauri::command]
pub async fn cancel_session_rounds(
    app_state: State<'_, OmigaAppState>,
    session_id: String,
) -> CommandResult<Vec<String>> {
    let repo = app_state.repo.lock().await;

    // Get all active rounds for this session
    let active_rounds_db = repo.get_active_rounds(&session_id).await.map_err(|e| {
        OmigaError::Chat(ChatError::StreamError(format!("Failed to get active rounds: {}", e)))
    })?;

    let mut cancelled_round_ids = Vec::new();

    for round in active_rounds_db {
        // Cancel in database
        if let Err(e) = repo.cancel_round(&round.id, Some("Session closed")).await {
            tracing::warn!("Failed to cancel round {}: {}", round.id, e);
        } else {
            cancelled_round_ids.push(round.id.clone());
        }

        // Set cancellation flag
        let active_rounds = app_state.chat.active_rounds.lock().await;
        if let Some(round_state) = active_rounds.get(&round.message_id) {
            let mut cancelled = round_state.cancelled.write().await;
            *cancelled = true;
        }
    }

    // Clean up in-memory session cache
    {
        let mut sessions = app_state.chat.sessions.write().await;
        sessions.remove(&session_id);
    }

    Ok(cancelled_round_ids)
}

/// Set LLM configuration (provider + API key + optional settings)
#[tauri::command]
pub async fn set_llm_config(
    state: State<'_, OmigaAppState>,
    provider: String,
    api_key: String,
    secret_key: Option<String>,
    app_id: Option<String>,
    model: Option<String>,
    base_url: Option<String>,
) -> CommandResult<()> {
    let provider_enum = provider.parse::<LlmProvider>()
        .map_err(|e| OmigaError::Config(format!("Invalid provider: {}", e)))?;

    let mut config = LlmConfig::new(provider_enum, api_key);

    // Apply optional settings
    if let Some(secret) = secret_key {
        config.secret_key = Some(secret);
    }
    if let Some(id) = app_id {
        config.app_id = Some(id);
    }
    if let Some(m) = model {
        config.model = m;
    }
    if let Some(url) = base_url {
        config.base_url = Some(url);
    }

    let mut config_guard = state.chat.llm_config.lock().await;
    *config_guard = Some(config);
    Ok(())
}

/// Get current LLM configuration
#[tauri::command]
pub async fn get_llm_config_state(
    state: State<'_, OmigaAppState>,
) -> CommandResult<Option<LlmConfigResponse>> {
    let config_guard = state.chat.llm_config.lock().await;
    Ok(config_guard.as_ref().map(|config| LlmConfigResponse {
        provider: format!("{}", config.provider),
        api_key_preview: if config.api_key.len() > 8 {
            format!("{}...", &config.api_key[..8])
        } else {
            config.api_key.clone()
        },
        model: Some(config.model.clone()),
        base_url: config.base_url.clone(),
    }))
}

/// LLM configuration response for frontend
#[derive(Debug, Serialize)]
pub struct LlmConfigResponse {
    pub provider: String,
    pub api_key_preview: String,
    pub model: Option<String>,
    pub base_url: Option<String>,
}

/// Brave Search API key status for Settings UI (never returns full secret).
#[derive(Debug, Serialize)]
pub struct BraveSearchKeyState {
    pub configured: bool,
    pub preview: String,
}

/// Store Brave Search API key for built-in `web_search` (empty clears user override; env still works).
#[tauri::command]
pub async fn set_brave_search_api_key(
    state: State<'_, OmigaAppState>,
    api_key: String,
) -> CommandResult<()> {
    let t = api_key.trim();
    let mut g = state.chat.brave_search_api_key.lock().await;
    if t.is_empty() {
        *g = None;
    } else {
        *g = Some(t.to_string());
    }
    Ok(())
}

#[tauri::command]
pub async fn get_brave_search_api_key_state(
    state: State<'_, OmigaAppState>,
) -> CommandResult<BraveSearchKeyState> {
    let g = state.chat.brave_search_api_key.lock().await;
    let Some(ref key) = *g else {
        return Ok(BraveSearchKeyState {
            configured: false,
            preview: String::new(),
        });
    };
    let preview = if key.len() > 8 {
        format!("{}...", &key[..8])
    } else {
        key.clone()
    };
    Ok(BraveSearchKeyState {
        configured: true,
        preview,
    })
}

/// Legacy: Set API key (deprecated, use set_llm_config)
#[tauri::command]
pub async fn set_api_key(
    state: State<'_, OmigaAppState>,
    api_key: String,
) -> CommandResult<()> {
    let mut config_guard = state.chat.llm_config.lock().await;
    let mut config = config_guard.clone().unwrap_or_default();
    config.api_key = api_key;
    *config_guard = Some(config);
    Ok(())
}

/// Get API key status - checks if API key is configured via environment or state
#[tauri::command]
pub async fn get_api_key_status(
    state: State<'_, OmigaAppState>,
) -> CommandResult<ApiKeyStatus> {
    // First check if we have a stored config with API key
    let stored = state.chat.llm_config.lock().await;
    if let Some(config) = stored.as_ref() {
        if !config.api_key.is_empty() {
            return Ok(ApiKeyStatus {
                configured: true,
                source: Some("state".to_string()),
                provider: Some(format!("{:?}", config.provider)),
                message: None,
            });
        }
    }
    drop(stored);

    // Try to load from environment
    match load_config_from_env() {
        Ok(config) => {
            // Store for future use
            let mut stored = state.chat.llm_config.lock().await;
            *stored = Some(config.clone());
            Ok(ApiKeyStatus {
                configured: true,
                source: Some("environment".to_string()),
                provider: Some(format!("{:?}", config.provider)),
                message: None,
            })
        }
        Err(_e) => Ok(ApiKeyStatus {
            configured: false,
            source: None,
            provider: None,
            message: Some(format!(
                "未配置 API key。请设置环境变量: ANTHROPIC_API_KEY, OPENAI_API_KEY, 或 LLM_API_KEY"
            )),
        }),
    }
}

/// API key status response
#[derive(Debug, Serialize)]
pub struct ApiKeyStatus {
    pub configured: bool,
    pub source: Option<String>,
    pub provider: Option<String>,
    pub message: Option<String>,
}

/// Response from send_message
#[derive(Debug, Serialize)]
pub struct MessageResponse {
    pub message_id: String,
    pub session_id: String,
    pub round_id: String,
}

/// Test if the LLM model is available and responding
#[tauri::command]
pub async fn test_model(
    state: State<'_, OmigaAppState>,
) -> CommandResult<ModelTestResult> {
    let config_guard = state.chat.llm_config.lock().await;
    
    let config = match config_guard.as_ref() {
        Some(c) if !c.api_key.is_empty() => c.clone(),
        _ => {
            return Ok(ModelTestResult {
                available: false,
                provider: None,
                model: None,
                latency_ms: None,
                error: Some("No API key configured".to_string()),
            });
        }
    };
    drop(config_guard);

    let provider = config.provider;
    let model = config.model.clone();

    match create_client(config) {
        Ok(client) => {
            let start = std::time::Instant::now();
            match client.health_check().await {
                Ok(true) => {
                    let latency_ms = start.elapsed().as_millis() as u64;
                    Ok(ModelTestResult {
                        available: true,
                        provider: Some(format!("{:?}", provider)),
                        model: Some(model),
                        latency_ms: Some(latency_ms),
                        error: None,
                    })
                }
                Ok(false) => Ok(ModelTestResult {
                    available: false,
                    provider: Some(format!("{:?}", provider)),
                    model: Some(model),
                    latency_ms: None,
                    error: Some("Health check returned false".to_string()),
                }),
                Err(e) => Ok(ModelTestResult {
                    available: false,
                    provider: Some(format!("{:?}", provider)),
                    model: Some(model),
                    latency_ms: None,
                    error: Some(match e {
                        ApiError::Http { message, .. } => message.clone(),
                        ApiError::Network { message } => message.clone(),
                        ApiError::Authentication => "Authentication failed".to_string(),
                        ApiError::Timeout => "Request timeout".to_string(),
                        ApiError::RateLimited => "Rate limited".to_string(),
                        ApiError::Server { message } => message.clone(),
                        ApiError::SseParse { message } => message.clone(),
                        ApiError::Config { message } => message.clone(),
                    }),
                })
            }
        }
        Err(e) => Ok(ModelTestResult {
            available: false,
            provider: Some(format!("{:?}", provider)),
            model: Some(model),
            latency_ms: None,
            error: Some(e.to_string()),
        }),
    }
}

/// Result of model test
#[derive(Debug, Serialize)]
pub struct ModelTestResult {
    pub available: bool,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub latency_ms: Option<u64>,
    pub error: Option<String>,
}

/// Request to send a message
#[derive(Debug, Deserialize)]
pub struct SendMessageRequest {
    pub content: String,
    pub session_id: Option<String>,
    /// Explicit project path (required for new sessions)
    pub project_path: Option<String>,
    /// Optional session name (defaults to first 50 chars of content)
    pub session_name: Option<String>,
    #[serde(default)]
    pub use_tools: bool,
}
