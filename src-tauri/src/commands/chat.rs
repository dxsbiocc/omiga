//! Chat commands - Send messages and stream responses with tool execution
//!
//! Multi-provider support: Anthropic, OpenAI, Azure, Google, and custom endpoints

use super::CommandResult;
use crate::app_state::{IntegrationsConfigCacheSlot, OmigaAppState, INTEGRATIONS_CONFIG_CACHE_TTL};
use crate::api::{ContentBlock, Role};
use crate::constants::agent_prompt;
use crate::constants::tool_limits::{
    large_output_persist_failed_message, large_tool_output_files_enabled, truncate_utf8_prefix,
    DEFAULT_MAX_RESULT_SIZE_CHARS, PREVIEW_SIZE_BYTES, TOOL_DISPLAY_MAX_INPUT_CHARS,
};
use crate::domain::agent_runtime::ChatInputTarget;
use crate::domain::coordinator;
use crate::domain::chat_state::{
    AskUserWaiter, McpToolCache, MCP_TOOL_CACHE_TTL, PermissionDenyCache, PermissionToolWaiter,
    PERMISSION_DENY_CACHE_TTL,
};
use crate::domain::permissions::PermissionRequest;
use crate::domain::tools::ask_user_question;
use crate::domain::session::{AgentTask, Message, MessageTokenUsage, Session, TodoItem, ToolCall};
use crate::domain::session_codec::SessionCodec;
use crate::utils::large_output_instructions::get_large_output_instructions;
use crate::domain::integrations_config;
use crate::domain::skills;
use crate::domain::subagent_tool_filter::{env_allow_nested_agent, SubagentFilterOptions};
use crate::domain::agents::scheduler::{AgentSelector, AgentScheduler, SchedulingRequest, SchedulingStrategy};
use crate::domain::tools::{all_tool_schemas, Tool, ToolContext, ToolSchema};
use crate::errors::{ApiError, ChatError, OmigaError};
use crate::infrastructure::streaming::StreamOutputItem;
use crate::llm::{create_client, load_config_from_env, LlmClient, LlmConfig, LlmContent, LlmMessage, LlmRole, LlmStreamChunk, LlmProvider};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex as StdMutex};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::{Mutex, RwLock};
use regex::Regex;

/// Arguments for the `skill` tool (JSON) — aligned with `SkillTool` input (`skill` + `args`).
#[derive(Debug, Deserialize)]
struct SkillToolArgs {
    skill: String,
    #[serde(default, rename = "args", alias = "arguments")]
    args: String,
    /// Execution mode: "inline" (default) or "forked"
    /// - inline: Execute skill in current session context
    /// - forked: Execute skill in isolated sub-agent session
    #[serde(default = "default_execution_mode")]
    execution_mode: String,
}

fn default_execution_mode() -> String {
    "inline".to_string()
}

#[derive(Debug, Deserialize, Default)]
struct ListSkillsArgs {
    query: Option<String>,
}

/// `skill_view` tool JSON — Hermes may use `name` instead of `skill`.
#[derive(Debug, Deserialize)]
struct SkillViewArgs {
    #[serde(alias = "name")]
    skill: String,
    file_path: Option<String>,
}

/// Max assistant↔tool iterations per user send (safety valve; TS query loop is bounded similarly).
const MAX_TOOL_ROUNDS: usize = 25;

/// Max tool rounds inside one `Agent` sub-session (nested Agent calls are blocked separately).
const MAX_SUBAGENT_TOOL_ROUNDS: usize = 16;

/// Max `execute_tool_calls` depth for nested `Agent` (main session = 0). TS allows deep nesting when `USER_TYPE=ant`.
const MAX_SUBAGENT_EXECUTE_DEPTH: u8 = 8;

/// LLM + stream state needed for the `Agent` tool to run an isolated sub-session (same API key as main chat).
#[derive(Clone)]
pub(crate) struct AgentLlmRuntime {
    llm_config: LlmConfig,
    round_id: String,
    cancel_flag: Arc<RwLock<bool>>,
    pending_tools: Arc<Mutex<HashMap<String, PendingToolCall>>>,
    repo: Arc<crate::domain::persistence::SessionRepository>,
    /// Same `Arc` as [`SessionRuntimeState::plan_mode`] — sub-agent filter reads plan mode for `ExitPlanMode` parity.
    plan_mode_flag: Option<Arc<Mutex<bool>>>,
    /// `USER_TYPE=ant` — nested `Agent` allowed (`ALL_AGENT_DISALLOWED_TOOLS` omits Agent).
    allow_nested_agent: bool,
}

pub use crate::domain::chat_state::{
    ChatState, PendingToolCall, RoundCancellationState, SessionRuntimeState,
};

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

    // Prefer merged config: `omiga.yaml` default_provider + env overrides (`LLM_PROVIDER`, keys, …).
    // Using only `load_config_from_env()` ignored the file and caused UI (yaml default → "Kimi") to
    // disagree with runtime (env → e.g. deepseek) and token_usage labels.
    match crate::llm::load_config() {
        Ok(config) => {
            let mut stored = chat_state.llm_config.lock().await;
            *stored = Some(config.clone());
            drop(stored);
            if let Ok(cf) = crate::llm::config::load_config_file() {
                *chat_state.active_provider_entry_name.lock().await = cf.default_provider;
            }
            Ok(config)
        }
        Err(_) => match load_config_from_env() {
            Ok(config) => {
                let mut stored = chat_state.llm_config.lock().await;
                *stored = Some(config.clone());
                drop(stored);
                *chat_state.active_provider_entry_name.lock().await = None;
                Ok(config)
            }
            Err(_e) => Err(OmigaError::Chat(ChatError::ApiKeyMissing)),
        },
    }
}

pub(crate) fn tool_results_dir_for_session(app: &AppHandle, session_id: &str) -> std::path::PathBuf {
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
            reasoning_content: msg.reasoning_content.clone(),
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
        | StreamOutputItem::ToolResult { .. }
        | StreamOutputItem::AskUserPending { .. }
        | StreamOutputItem::TurnSummary { .. }
        | StreamOutputItem::FollowUpSuggestions(_)
        | StreamOutputItem::TokenUsage { .. } => {}
    }
}

/// If grep/glob produced no text, use the same copy as `GrepTool` / `GlobTool` in `src/tools`.
fn apply_empty_structured_tool_placeholder(output: &mut String, tool_name: &str, had_error: bool) {
    if had_error || !output.trim().is_empty() {
        return;
    }
    match tool_name {
        "ripgrep" | "grep" => output.push_str("No matches found"),
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
    repo: &Arc<crate::domain::persistence::SessionRepository>,
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
    let repo_guard = &**repo;
    if let Err(e) = repo_guard
        .upsert_session_tool_state(session_id, &snapshots.0, &snapshots.1)
        .await
    {
        tracing::warn!("Failed to persist session tool state: {}", e);
    }
}

/// Index chat messages into implicit memory (PageIndex)
/// 
/// Emits events to notify frontend of indexing progress:
/// - `chat-index-start`: When indexing begins
/// - `chat-index-complete`: When indexing finishes successfully
/// - `chat-index-error`: When indexing fails
async fn index_chat_to_implicit_memory(
    app: &AppHandle,
    project_path: &str,
    session_id: &str,
    session_name: &str,
    repo: &crate::domain::persistence::SessionRepository,
) {
    // Notify frontend that indexing has started
    let _ = app.emit("chat-index-start", serde_json::json!({
        "session_id": session_id,
    }));

    // Get session messages from database
    let session_with_messages = match repo.get_session(session_id).await {
        Ok(Some(s)) => s,
        _ => {
            tracing::debug!("Session {} not found for indexing", session_id);
            let _ = app.emit("chat-index-error", serde_json::json!({
                "session_id": session_id,
                "error": "Session not found",
            }));
            return;
        }
    };

    // Convert messages to chat indexer format
    let messages: Vec<crate::domain::memory::ChatMessage> = session_with_messages
        .messages
        .into_iter()
        .map(|msg| crate::domain::memory::ChatMessage {
            id: msg.id,
            session_id: msg.session_id,
            role: match msg.role.as_str() {
                "user" => crate::domain::memory::ChatRole::User,
                "assistant" => crate::domain::memory::ChatRole::Assistant,
                "tool" => crate::domain::memory::ChatRole::Tool,
                _ => crate::domain::memory::ChatRole::User,
            },
            content: msg.content,
            timestamp: chrono::DateTime::parse_from_rfc3339(&msg.created_at)
                .map(|dt| dt.timestamp())
                .unwrap_or_else(|_| chrono::Utc::now().timestamp()),
            tool_calls: msg.tool_calls.and_then(|tc| serde_json::from_str(&tc).ok()),
        })
        .collect();

    if messages.is_empty() {
        let _ = app.emit("chat-index-complete", serde_json::json!({
            "session_id": session_id,
            "document_count": 0,
        }));
        return;
    }

    // Get memory directory path (respects ~/.omiga/memory/projects/... default)
    let project_root = resolve_session_project_root(project_path);
    let memory_dir = match crate::domain::memory::load_resolved_config(&project_root).await {
        Ok(cfg) => cfg.implicit_path(&project_root),
        Err(_) => project_root.join(".omiga/memory/implicit"),
    };

    // Initialize indexer
    let mut indexer = crate::domain::memory::ChatIndexer::new(&memory_dir);
    if let Err(e) = indexer.init().await {
        tracing::warn!("Failed to init chat indexer: {}", e);
        let _ = app.emit("chat-index-error", serde_json::json!({
            "session_id": session_id,
            "error": format!("Failed to init indexer: {}", e),
        }));
        return;
    }
    if let Err(e) = indexer.load().await {
        tracing::warn!("Failed to load chat indexer: {}", e);
        let _ = app.emit("chat-index-error", serde_json::json!({
            "session_id": session_id,
            "error": format!("Failed to load indexer: {}", e),
        }));
        return;
    }

    // Index the session
    match indexer.index_session(session_id, session_name, &messages).await {
        Ok(_) => {
            tracing::info!("Indexed chat session {} into implicit memory", session_id);
            crate::domain::memory::touch_project_registry(&project_root).await;
            let _ = app.emit("chat-index-complete", serde_json::json!({
                "session_id": session_id,
                "document_count": indexer.document_count(),
            }));
        }
        Err(e) => {
            tracing::warn!("Failed to index chat session: {}", e);
            let _ = app.emit("chat-index-error", serde_json::json!({
                "session_id": session_id,
                "error": format!("Failed to index: {}", e),
            }));
        }
    }
}

/// One built-in or custom agent entry for the composer picker.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AvailableAgentInfo {
    pub agent_type: String,
    pub description: String,
}

#[tauri::command]
pub fn list_available_agents() -> Vec<AvailableAgentInfo> {
    let router = crate::domain::agents::get_agent_router();
    let mut out: Vec<AvailableAgentInfo> = router
        .list_agents_with_description()
        .into_iter()
        .map(|(t, d)| AvailableAgentInfo {
            agent_type: t.to_string(),
            description: d.to_string(),
        })
        .collect();
    out.sort_by(|a, b| a.agent_type.cmp(&b.agent_type));
    
    // 在列表开头添加"自动"选项（调度模式）
    out.insert(0, AvailableAgentInfo {
        agent_type: "auto".to_string(),
        description: "智能调度 - 根据任务自动选择最合适的 Agent".to_string(),
    });
    
    out
}

fn composer_permission_addendum(raw: &str) -> Option<String> {
    match raw.trim() {
        "ask" => Some(
            "### Composer permission mode\nThe user chose **ask**: prefer confirming before applying edits or running destructive tools."
                .to_string(),
        ),
        "auto" => Some(
            "### Composer permission mode\nThe user chose **auto**: accept reasonable file edits without repeated confirmation when aligned with the task."
                .to_string(),
        ),
        "bypass" => Some(
            "### Composer permission mode\nThe user chose **bypass**: minimize permission prompts; still follow safety-critical constraints."
                .to_string(),
        ),
        _ => None,
    }
}

/// 格式化调度计划为 system prompt 的一部分
fn format_scheduler_plan(result: &crate::domain::agents::scheduler::SchedulingResult) -> String {
    let is_content_generation = result.plan.subtasks.iter().any(|t| {
        t.id == "generate-content" || t.id == "gather-requirements"
    });
    
    let mut plan_text = if is_content_generation {
        String::from("## 内容生成任务执行计划\n\n")
    } else {
        String::from("## 任务执行计划\n\n")
    };
    
    plan_text.push_str(&format!("此任务已自动分解为 **{}** 个子任务，将按以下顺序执行：\n\n", 
        result.plan.subtasks.len()));
    
    // 获取并行执行组
    let groups = result.plan.get_parallel_groups();
    let mut task_idx = 1;
    
    for (group_idx, group) in groups.iter().enumerate() {
        if groups.len() > 1 {
            plan_text.push_str(&format!("### 阶段 {}\n", group_idx + 1));
        }
        
        for task_id in group {
            if let Some(task) = result.plan.subtasks.iter().find(|t| &t.id == task_id) {
                plan_text.push_str(&format!(
                    "{}. **{}** - 使用 `{}` Agent\n",
                    task_idx,
                    task.description,
                    task.agent_type
                ));
                if !task.context.is_empty() {
                    plan_text.push_str(&format!("   - 要求: {}\n", task.context));
                }
                if !task.dependencies.is_empty() {
                    plan_text.push_str(&format!("   - 依赖: {}\n", task.dependencies.join(", ")));
                }
                if task.critical {
                    plan_text.push_str("   - ⚠️ 关键任务\n");
                }
                task_idx += 1;
            }
        }
        plan_text.push('\n');
    }
    
    plan_text.push_str(&format!("\n预估执行时间: ~{} 分钟\n", result.estimated_duration_secs / 60));
    
    // 对于内容生成任务，添加重要提示
    if is_content_generation {
        plan_text.push_str("\n### ⚠️ 重要提示\n");
        plan_text.push_str("这是一个**内容生成任务**。你必须：\n");
        plan_text.push_str("1. **生成完整、详细的内容**，不要只是概述或框架\n");
        plan_text.push_str("2. **包含具体的细节**：名称、地址、时间、价格、建议等\n");
        plan_text.push_str("3. **确保内容实用可读**，用户可以直接使用\n");
        plan_text.push_str("4. **按步骤执行**，先收集需求，再研究，最后生成完整内容\n");
        plan_text.push_str("\n现在开始执行第一个子任务。\n");
    } else {
        plan_text.push_str("\n你可以直接开始执行第一个子任务，或要求我调整计划。");
    }
    
    plan_text
}

/// Send a message to Claude and get a streaming response
#[tauri::command]
pub async fn send_message(
    app: AppHandle,
    app_state: State<'_, OmigaAppState>,
    request: SendMessageRequest,
) -> CommandResult<MessageResponse> {
    let input_target = match ChatInputTarget::parse(request.input_target.as_deref()) {
        Ok(t) => t,
        Err(msg) => {
            return Err(OmigaError::Chat(ChatError::StreamError(msg.to_string())));
        }
    };

    if let ChatInputTarget::BackgroundAgentFollowup { task_id } = input_target {
        let session_id = request.session_id.clone().ok_or_else(|| {
            OmigaError::Chat(ChatError::StreamError(
                "session_id is required when using input_target bg:<task_id>".to_string(),
            ))
        })?;
        let manager = crate::domain::agents::background::get_background_agent_manager();
        manager
            .enqueue_followup(&task_id, &session_id, request.content.clone())
            .await
            .map_err(|e| OmigaError::Chat(ChatError::StreamError(e.to_string())))?;
        return Ok(MessageResponse {
            message_id: uuid::Uuid::new_v4().to_string(),
            session_id,
            round_id: uuid::Uuid::new_v4().to_string(),
            input_kind: Some("background_followup_queued".to_string()),
            scheduler_plan: None,
            initial_todos: None,
            user_message_id: None,
        });
    }

    // Get or create session (database is single source of truth)
    let repo = &*app_state.repo;

    let (session_id, mut session, user_message_id, project_path) = if let Some(ref id) = request.session_id {
        // Load existing session from database
        let db_session = repo.get_session(id).await.map_err(|e| {
            OmigaError::Chat(ChatError::StreamError(format!("Failed to load session: {}", e)))
        })?;

        if let Some(db_session) = db_session {
            let mut session;
            let msg_id: String;

            if let Some(ref anchor) = request.retry_from_user_message_id {
                let anchor_row = db_session.messages.iter().find(|m| m.id == *anchor);
                let Some(anchor_row) = anchor_row else {
                    return Err(OmigaError::Chat(ChatError::StreamError(
                        "retry_from_user_message_id not found in session".to_string(),
                    )));
                };
                if anchor_row.role != "user" {
                    return Err(OmigaError::Chat(ChatError::StreamError(
                        "retry_from_user_message_id must reference a user message".to_string(),
                    )));
                }
                repo.delete_messages_after_anchor(id, anchor)
                    .await
                    .map_err(|e| {
                        OmigaError::Chat(ChatError::StreamError(format!(
                            "Failed to truncate session for retry: {}",
                            e
                        )))
                    })?;
                if anchor_row.content != request.content {
                    repo.update_message_content(anchor, &request.content)
                        .await
                        .map_err(|e| {
                            OmigaError::Chat(ChatError::StreamError(format!(
                                "Failed to update user message for retry: {}",
                                e
                            )))
                        })?;
                }
                let db_session = repo
                    .get_session(id)
                    .await
                    .map_err(|e| {
                        OmigaError::Chat(ChatError::StreamError(format!(
                            "Failed to reload session after retry: {}",
                            e
                        )))
                    })?
                    .ok_or_else(|| {
                        OmigaError::Chat(ChatError::StreamError(
                            "Session not found after retry truncate".to_string(),
                        ))
                    })?;
                session = SessionCodec::db_to_domain(db_session);
                msg_id = anchor.clone();
            } else {
                session = SessionCodec::db_to_domain(db_session);
                session.add_user_message(&request.content);

                msg_id = uuid::Uuid::new_v4().to_string();
                repo.save_message(&msg_id, &session.id, "user", &request.content, None, None, None, None)
                    .await
                    .map_err(|e| {
                        OmigaError::Chat(ChatError::StreamError(format!("Failed to save message: {}", e)))
                    })?;
            }

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
        repo.save_message(&msg_id, &session.id, "user", &request.content, None, None, None, None)
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
    
    // 检测是否为 Plan mode
    let is_plan_mode = request.composer_agent_type.as_deref() == Some("Plan");
    
    // ===== 智能调度系统集成 =====
    // 检测是否使用自动调度模式（用户选择 auto 或未指定特定 Agent）
    let use_scheduler = request.composer_agent_type.as_deref().map(|t| {
        t == "auto" || t == "general-purpose" || t.is_empty()
    }).unwrap_or(true);
    
    // 如果是自动模式，检测任务复杂度并可能进行任务分解
    let scheduler_result: Option<crate::domain::agents::scheduler::SchedulingResult> = if use_scheduler && request.use_tools {
        let scheduler = AgentScheduler::new();
        let scheduling_req = SchedulingRequest::new(&request.content)
            .with_project_root(project_root.to_string_lossy().as_ref())
            .with_strategy(SchedulingStrategy::Auto)
            .with_auto_decompose(true);
        
        match scheduler.schedule(scheduling_req).await {
            Ok(result) => {
                // 如果任务被分解为多个子任务，记录日志
                if result.selected_agents.len() > 1 {
                    tracing::info!(
                        target: "omiga::scheduler",
                        task_count = result.plan.subtasks.len(),
                        agents = ?result.selected_agents,
                        "Complex task decomposed into subtasks"
                    );
                    Some(result)
                } else {
                    None
                }
            }
            Err(e) => {
                tracing::warn!(target: "omiga::scheduler", "Scheduling failed: {}", e);
                None
            }
        }
    } else {
        None
    };
    
    let mut llm_config = get_llm_config(&app_state.chat).await?;
    let integrations_cfg = {
        let hit = app_state.integrations_config_cache
            .lock().expect("integrations config cache poisoned")
            .get(&project_root)
            .filter(|s| s.cached_at.elapsed() < INTEGRATIONS_CONFIG_CACHE_TTL)
            .map(|s| s.config.clone());
        hit.unwrap_or_else(|| {
            // Lock is released above; safe to do a blocking file read here.
            let cfg = integrations_config::load_integrations_config(&project_root);
            app_state.integrations_config_cache
                .lock().expect("integrations config cache poisoned")
                .insert(project_root.clone(), IntegrationsConfigCacheSlot { config: cfg.clone(), cached_at: std::time::Instant::now() });
            cfg
        })
    };
    // Run independent async I/O in parallel to reduce pre-LLM latency.
    let skill_cache_ref = &app_state.skill_cache;
    let (skills_exist, memory_ctx) = tokio::join!(
        skills::skills_any_exist(&project_root, skill_cache_ref),
        crate::commands::memory::get_memory_context(&project_root, &request.content, 3),
    );

    // Ported agent system prompt from `src/constants/prompts.ts` — injected when tools are enabled.
    let mut prompt_parts: Vec<String> = Vec::new();
    if request.use_tools {
        prompt_parts.push(agent_prompt::build_system_prompt(
            &project_root,
            &llm_config.model,
        ));
        if coordinator::is_coordinator_mode() {
            prompt_parts.push(agent_prompt::coordinator_mode_addendum().to_string());
        }
    }
    if let Some(ref u) = llm_config.system_prompt {
        let t = u.trim();
        if !t.is_empty() {
            prompt_parts.push(t.to_string());
        }
    }
    if skills_exist {
        prompt_parts.push(skills::format_skills_discovery_system_section());
    }
    if let Some(ctx) = memory_ctx {
        prompt_parts.push(ctx);
    }
    
    // 如果有调度计划（任务分解），添加到 system prompt
    if let Some(ref schedule_result) = scheduler_result {
        let plan_description = format_scheduler_plan(schedule_result);
        prompt_parts.push(plan_description);
    }
    
    if request.use_tools {
        if let Some(ref at) = request.composer_agent_type {
            let t = at.trim();
            if !t.is_empty() && t != "general-purpose" {
                let router = crate::domain::agents::get_agent_router();
                let agent = router.select_agent(Some(t));
                let tool_ctx = ToolContext::new(project_root.clone());
                prompt_parts.push(agent.system_prompt(&tool_ctx));
            }
        }
        if let Some(ref pm) = request.permission_mode {
            if let Some(line) = composer_permission_addendum(pm) {
                prompt_parts.push(line);
            }
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
        let deny_entries = {
            let hit = app_state.chat.permission_deny_cache.lock().await
                .get(&project_root)
                .filter(|e| e.cached_at.elapsed() < PERMISSION_DENY_CACHE_TTL)
                .map(|e| e.entries.clone());
            match hit {
                Some(entries) => {
                    tracing::debug!(target: "omiga::permissions", "permission deny rules served from cache");
                    entries
                }
                None => {
                    // Lock released above; safe to do blocking file reads here.
                    let entries = crate::domain::tool_permission_rules::load_merged_permission_deny_rule_entries(&project_root);
                    app_state.chat.permission_deny_cache.lock().await
                        .insert(project_root.clone(), PermissionDenyCache { entries: entries.clone(), cached_at: std::time::Instant::now() });
                    entries
                }
            }
        };
        crate::domain::tool_permission_rules::validate_permission_deny_entries(&deny_entries);
        let all_schemas = all_tool_schemas(skills_exist);
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
        let mcp_tools = {
            let hit = app_state.chat.mcp_tool_cache.lock().await
                .get(&project_root)
                .filter(|e| e.cached_at.elapsed() < MCP_TOOL_CACHE_TTL)
                .map(|e| e.schemas.clone());
            match hit {
                Some(schemas) => {
                    tracing::debug!(target: "omiga::mcp", "MCP tool schemas served from cache");
                    schemas
                }
                None => {
                    let schemas = crate::domain::mcp::tool_pool::discover_mcp_tool_schemas(
                        &project_root,
                        mcp_timeout,
                    )
                    .await;
                    app_state.chat.mcp_tool_cache.lock().await
                        .insert(project_root.clone(), McpToolCache { schemas: schemas.clone(), cached_at: std::time::Instant::now() });
                    schemas
                }
            }
        };
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
        let mut combined: Vec<ToolSchema> = built.into_iter().chain(mcp_filtered).collect();
        if coordinator::is_coordinator_mode() {
            let before = combined.len();
            combined = coordinator::filter_coordinator_tool_schemas(combined);
            tracing::info!(
                target: "omiga::coordinator",
                before,
                after = combined.len(),
                "coordinator mode: tool list restricted to orchestration tools"
            );
        }
        combined
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
            reasoning_content: msg.reasoning_content.clone(),
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
    let skill_cache_for_spawn = app_state.skill_cache.clone();
    // 回合开始前预判：短确认类输入可跳过回合结束后的 Output Formatter，加快到 Complete。
    let preflight_skip_turn_summary =
        crate::domain::agents::output_formatter::preflight_skip_turn_summary(&request.content);

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

        let mut turn_token_usage: Option<crate::llm::TokenUsage> = None;

        // Stream the response with cancellation support
        let (mut pending_tool_calls, assistant_text, assistant_reasoning, was_cancelled, usage_first) = match stream_llm_response_with_cancel(
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
                let repo = &*repo_clone;
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
        merge_turn_token_usage(&mut turn_token_usage, usage_first);

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

        let mut final_reply_for_follow_up = assistant_text.clone();

        // First assistant turn: persist with tool_calls JSON for reload
        let assistant_msg_id = uuid::Uuid::new_v4().to_string();
        let tool_calls_json = tool_calls_json_opt(&pending_tool_calls);
        let reasoning_save = (!assistant_reasoning.is_empty()).then_some(assistant_reasoning.as_str());
        {
            let repo = &*repo_clone;
            if let Err(e) = repo
                .save_message(
                    &assistant_msg_id,
                    &session_id_clone,
                    "assistant",
                    &assistant_text,
                    tool_calls_json.as_deref(),
                    None,
                    None,
                    reasoning_save,
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
                let rc = (!assistant_reasoning.is_empty()).then(|| assistant_reasoning.clone());
                runtime
                    .session
                    .add_assistant_message_with_tools(&assistant_text, tc, rc);
            }
        }

        let mut last_assistant_id = assistant_msg_id.clone();

        if pending_tool_calls.is_empty() {
            persist_session_tool_state(&sessions_clone, &repo_clone, &session_id_clone).await;
            
            // Index chat to implicit memory
            {
                let project_path = {
                    let sessions = sessions_clone.read().await;
                    sessions.get(&session_id_clone)
                        .map(|r| r.session.project_path.clone())
                        .unwrap_or_else(|| ".".to_string())
                };
                let repo = &*repo_clone;
                let session_name = {
                    let sessions = sessions_clone.read().await;
                    sessions.get(&session_id_clone)
                        .map(|r| r.session.name.clone())
                        .unwrap_or_else(|| "Unnamed".to_string())
                };
                index_chat_to_implicit_memory(&app_clone, &project_path, &session_id_clone, &session_name, &repo).await;
            }
            
            {
                let repo = &*repo_clone;
                if let Err(e) = repo
                    .complete_round(&round_id_clone, Some(&last_assistant_id))
                    .await
                {
                    tracing::warn!("Failed to complete round: {}", e);
                }
            } // repo guard dropped before emit_post_turn_meta_then_complete to avoid deadlock
            persist_and_emit_turn_token_usage(
                &app_clone,
                &repo_clone,
                &last_assistant_id,
                &message_id_clone,
                &turn_token_usage,
                &llm_config_for_spawn.provider.to_string(),
            )
            .await;
            emit_post_turn_meta_then_complete(
                &app_clone,
                &message_id_clone,
                client.as_ref(),
                &final_reply_for_follow_up,
                preflight_skip_turn_summary,
                &final_reply_for_follow_up,
            )
            .await;
            return;
        }

        let mut send_user_message_called = false;
        let mut send_user_message_text: Option<String> = None;
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

            // SendUserMessage 调用：标记已直接交付内容，并提取 message 供 suggestions 使用
            if let Some((_, _, args)) = pending_tool_calls
                .iter()
                .find(|(_, name, _)| name == "SendUserMessage")
            {
                send_user_message_called = true;
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(args) {
                    if let Some(msg) = v.get("message").and_then(|m| m.as_str()) {
                        send_user_message_text = Some(msg.to_string());
                    }
                }
            }

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
                skill_cache_for_spawn.clone(),
            )
            .await;

            {
                let repo = &*repo_clone;
                // Write all tool results in a single transaction (one fsync instead of N).
                let batch: Vec<(String, String, Option<String>)> = tool_results
                    .iter()
                    .map(|(id, out, _)| (id.clone(), out.clone(), None))
                    .collect();
                if let Err(e) = repo
                    .save_tool_results_batch(&session_id_clone, &batch)
                    .await
                {
                    tracing::warn!("Failed to save tool results batch: {}", e);
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
                    let repo = &*repo_clone;
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
                    let repo = &*repo_clone;
                    if let Ok(Some(db_session)) = repo.get_session(&session_id_clone).await {
                        let session = SessionCodec::db_to_domain(db_session);
                        SessionCodec::to_api_messages(&session.messages)
                    } else {
                        vec![]
                    }
                }
            };

            let updated_llm_messages: Vec<LlmMessage> = api_messages_to_llm(&updated_messages);

            let (next_tools, next_text, next_reasoning, follow_cancelled, usage_next) = match stream_llm_response_with_cancel(
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
                    let repo = &*repo_clone;
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
            merge_turn_token_usage(&mut turn_token_usage, usage_next);

            final_reply_for_follow_up = next_text.clone();

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
            let next_reasoning_save = (!next_reasoning.is_empty()).then_some(next_reasoning.as_str());
            {
                let repo = &*repo_clone;
                if let Err(e) = repo
                    .save_message(
                        &next_assistant_id,
                        &session_id_clone,
                        "assistant",
                        &next_text,
                        next_tc_json.as_deref(),
                        None,
                        None,
                        next_reasoning_save,
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
                    let rc = (!next_reasoning.is_empty()).then(|| next_reasoning.clone());
                    runtime.session.add_assistant_message_with_tools(&next_text, tc, rc);
                }
            }

            last_assistant_id = next_assistant_id.clone();
            pending_tool_calls = next_tools;

            if pending_tool_calls.is_empty() {
                persist_session_tool_state(&sessions_clone, &repo_clone, &session_id_clone).await;
                
                // Index chat to implicit memory
                {
                    let project_path = {
                        let sessions = sessions_clone.read().await;
                        sessions.get(&session_id_clone)
                            .map(|r| r.session.project_path.clone())
                            .unwrap_or_else(|| ".".to_string())
                    };
                    let repo = &*repo_clone;
                    let session_name = {
                        let sessions = sessions_clone.read().await;
                        sessions.get(&session_id_clone)
                            .map(|r| r.session.name.clone())
                            .unwrap_or_else(|| "Unnamed".to_string())
                    };
                    index_chat_to_implicit_memory(&app_clone, &project_path, &session_id_clone, &session_name, &repo).await;
                }
                
                {
                    let repo = &*repo_clone;
                    if let Err(e) = repo
                        .complete_round(&round_id_clone, Some(&last_assistant_id))
                        .await
                    {
                        tracing::warn!("Failed to complete round: {}", e);
                    }
                } // repo guard dropped before emit_post_turn_meta_then_complete to avoid deadlock
                let suggestions_text = send_user_message_text
                    .as_deref()
                    .unwrap_or(&final_reply_for_follow_up);
                persist_and_emit_turn_token_usage(
                    &app_clone,
                    &repo_clone,
                    &last_assistant_id,
                    &message_id_clone,
                    &turn_token_usage,
                    &llm_config_for_spawn.provider.to_string(),
                )
                .await;
                emit_post_turn_meta_then_complete(
                    &app_clone,
                    &message_id_clone,
                    client.as_ref(),
                    &final_reply_for_follow_up,
                    preflight_skip_turn_summary || send_user_message_called,
                    suggestions_text,
                )
                .await;
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
        {
            let repo = &*repo_clone;
            let _ = repo
                .complete_round(&round_id_clone, Some(&last_assistant_id))
                .await;
        } // repo guard dropped before emit_post_turn_meta_then_complete to avoid deadlock
        let suggestions_text = send_user_message_text
            .as_deref()
            .unwrap_or(&final_reply_for_follow_up);
        persist_and_emit_turn_token_usage(
            &app_clone,
            &repo_clone,
            &last_assistant_id,
            &message_id_clone,
            &turn_token_usage,
            &llm_config_for_spawn.provider.to_string(),
        )
        .await;
        emit_post_turn_meta_then_complete(
            &app_clone,
            &message_id_clone,
            client.as_ref(),
            &final_reply_for_follow_up,
            preflight_skip_turn_summary || send_user_message_called,
            suggestions_text,
        )
        .await;
    });

    // 如果是 Plan mode，生成初始 todo items
    let initial_todos = if is_plan_mode {
        scheduler_result.as_ref().map(|result| {
            result.plan.subtasks.iter().enumerate().map(|(idx, subtask)| {
                InitialTodoItem {
                    id: format!("plan-todo-{}", idx),
                    content: subtask.description.clone(),
                    status: if idx == 0 { "in_progress".to_string() } else { "pending".to_string() },
                }
            }).collect()
        })
    } else {
        None
    };

    Ok(MessageResponse {
        message_id,
        session_id,
        round_id,
        user_message_id: Some(user_message_id),
        input_kind: None,
        scheduler_plan: scheduler_result,
        initial_todos,
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

/// Merge per-request usage into a running total for one user turn (multi tool-round).
fn merge_turn_token_usage(
    acc: &mut Option<crate::llm::TokenUsage>,
    stream: Option<crate::llm::TokenUsage>,
) {
    let Some(s) = stream else {
        return;
    };
    let t = acc.get_or_insert(crate::llm::TokenUsage::default());
    t.prompt_tokens = t.prompt_tokens.saturating_add(s.prompt_tokens);
    t.completion_tokens = t.completion_tokens.saturating_add(s.completion_tokens);
    t.total_tokens = t.prompt_tokens.saturating_add(t.completion_tokens);
}

/// Stream LLM response and collect tool calls with cancellation support
/// Returns: (tool_calls, assistant_text, reasoning_content, was_cancelled, usage_this_request)
async fn stream_llm_response_with_cancel(
    client: &dyn LlmClient,
    app: &AppHandle,
    message_id: &str,
    round_id: &str,
    messages: &[LlmMessage],
    tools: &[ToolSchema],
    pending_tools: &Arc<Mutex<HashMap<String, PendingToolCall>>>,
    cancel_flag: &Arc<RwLock<bool>>,
    repo: Arc<crate::domain::persistence::SessionRepository>,
) -> Result<(Vec<(String, String, String)>, String, String, bool, Option<crate::llm::TokenUsage>), OmigaError> {
    use futures::StreamExt;

    let stream = client
        .send_message_streaming(messages.to_vec(), tools.to_vec())
        .await
        .map_err(|e| OmigaError::Chat(ChatError::StreamError(e.to_string())))?;

    let mut stream = stream;
    let mut assistant_text = String::new();
    let mut reasoning_content = String::new();
    let mut completed_tool_calls: Vec<(String, String, String)> = Vec::new();
    let mut current_tool_id: Option<String> = None;
    let mut was_cancelled = false;
    let mut usage_this_request: Option<crate::llm::TokenUsage> = None;

    // Mark round as partial after receiving first chunk
    let mut marked_partial = false;

    while let Some(result) = stream.next().await {
        // Check cancellation flag
        if *cancel_flag.read().await {
            was_cancelled = true;
            // Mark round as cancelled in database
            let _ = repo.cancel_round(round_id, Some("User cancelled")).await;
            break;
        }

        match result {
            Ok(chunk) => {
                match chunk {
                    LlmStreamChunk::Text(text) => {
                        if !marked_partial && !text.is_empty() {
                            // Mark as partial in database
                            let _ = repo.mark_round_partial(round_id, None).await;
                            marked_partial = true;
                        }
                        assistant_text.push_str(&text);
                        let _ = app.emit(
                            &format!("chat-stream-{}", message_id),
                            &StreamOutputItem::Text(text),
                        );
                    }
                    LlmStreamChunk::ReasoningContent(text) => {
                        reasoning_content.push_str(&text);
                        let _ = app.emit(
                            &format!("chat-stream-{}", message_id),
                            &StreamOutputItem::Thinking(text),
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
                    LlmStreamChunk::Usage(u) => {
                        usage_this_request = Some(u);
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

    Ok((
        completed_tool_calls,
        assistant_text,
        reasoning_content,
        was_cancelled,
        usage_this_request,
    ))
}

/// Persist aggregated main-agent token usage on the final assistant DB row for this turn, then emit for live UI.
async fn persist_and_emit_turn_token_usage(
    app: &AppHandle,
    repo: &Arc<crate::domain::persistence::SessionRepository>,
    last_assistant_message_id: &str,
    stream_message_id: &str,
    usage: &Option<crate::llm::TokenUsage>,
    provider: &str,
) {
    let Some(u) = usage else {
        return;
    };
    if u.prompt_tokens == 0 && u.completion_tokens == 0 {
        return;
    }
    let total = if u.total_tokens > 0 {
        u.total_tokens
    } else {
        u.prompt_tokens.saturating_add(u.completion_tokens)
    };
    let payload = MessageTokenUsage {
        input: u.prompt_tokens,
        output: u.completion_tokens,
        total: Some(total),
        provider: Some(provider.to_string()),
    };
    let json = match serde_json::to_string(&payload) {
        Ok(s) => Some(s),
        Err(e) => {
            tracing::warn!(target: "omiga::chat", "token usage json: {}", e);
            None
        }
    };
    if let Some(ref j) = json {
        let r = &**repo;
        if let Err(e) = r
            .update_message_token_usage(last_assistant_message_id, Some(j.as_str()))
            .await
        {
            tracing::warn!("Failed to persist token usage on message: {}", e);
        }
    }
    let _ = app.emit(
        &format!("chat-stream-{}", stream_message_id),
        &StreamOutputItem::TokenUsage {
            prompt_tokens: u.prompt_tokens,
            completion_tokens: u.completion_tokens,
            total_tokens: total,
            provider: provider.to_string(),
        },
    );
}

/// After the visible assistant reply is finalized: optional recap (independent LLM), then follow-up chips (independent LLM), then [`StreamOutputItem::Complete`].
///
/// - `skip_summary`：跳过摘要 LLM 调用（preflight 判定为无需摘要，或本轮已通过 SendUserMessage 直接交付内容）。
/// - `suggestions_reply`：生成 follow-up suggestions 所用的文本；当本轮使用了 SendUserMessage 时传其 message 内容，
///   而非 LLM 的空壳收尾文本；其余情况与 `final_reply` 相同。
async fn emit_post_turn_meta_then_complete(
    app: &AppHandle,
    message_id: &str,
    client: &dyn LlmClient,
    final_reply: &str,
    skip_summary: bool,
    suggestions_reply: &str,
) {
    let (summary_enabled, follow_enabled) = match app.try_state::<OmigaAppState>() {
        Some(state) => {
            let repo = &*state.repo;
            crate::domain::post_turn_settings::load_post_turn_meta_flags(&*repo)
                .await
                .unwrap_or((true, true))
        }
        None => (true, true),
    };

    let (summary_res, follow_res) = tokio::join!(
        async {
            if skip_summary {
                return Ok(None);
            }
            crate::domain::agents::output_formatter::run_turn_summary_pass(
                client,
                final_reply,
                summary_enabled,
            )
            .await
        },
        crate::domain::follow_up_suggestions::generate_follow_up_suggestions(
            client,
            suggestions_reply,
            follow_enabled,
        ),
    );

    let summary_text = match summary_res {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(target: "omiga::post_turn", "post-turn summary: {}", e);
            None
        }
    };
    let _ = app.emit(
        &format!("chat-stream-{}", message_id),
        &StreamOutputItem::TurnSummary {
            text: summary_text,
        },
    );

    match follow_res {
        Ok(items) if !items.is_empty() => {
            let _ = app.emit(
                &format!("chat-stream-{}", message_id),
                &StreamOutputItem::FollowUpSuggestions(items),
            );
        }
        Ok(_) => {}
        Err(e) => tracing::warn!(target: "omiga::follow_up", "follow-up suggestions: {}", e),
    }
    let _ = app.emit(
        &format!("chat-stream-{}", message_id),
        &StreamOutputItem::Complete,
    );
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
        crate::domain::mcp::tool_pool::discover_mcp_tool_schemas(project_root, mcp_timeout).await;
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

/// Forked skill execution - runs skill in isolated sub-agent session.
///
/// Unlike inline execution which modifies current session state,
/// forked execution creates an isolated context for the skill.
async fn run_skill_forked(
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
    brave_search_api_key: Option<String>,
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
        prompt_parts.push(skills::format_skills_discovery_system_section());
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
    let mut tools = build_subagent_tool_schemas(
        project_root,
        skills_exist,
        subagent_opts,
    )
    .await;

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

    // Execute sub-agent loop (similar to run_subagent_session)
    for _round_idx in 0..MAX_SUBAGENT_TOOL_ROUNDS {
        if *runtime.cancel_flag.read().await {
            return Err("Skill execution cancelled.".to_string());
        }

        let api_msgs = SessionCodec::to_api_messages(&transcript);
        let llm_messages = api_messages_to_llm(&api_msgs);

        let (tool_calls, assistant_text, reasoning_text, cancelled, _) = stream_llm_response_with_cancel(
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
            return Err("Skill execution cancelled.".to_string());
        }

        let tc = completed_to_tool_calls(&tool_calls);
        transcript.push(Message::Assistant {
            content: assistant_text.clone(),
            tool_calls: tc.clone(),
            token_usage: None,
            reasoning_content: (!reasoning_text.is_empty()).then(|| reasoning_text.clone()),
        });

        if tool_calls.is_empty() {
            return Ok(assistant_text);
        }

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
            brave_search_api_key.clone(),
            skill_cache.clone(),
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
async fn run_subagent_session_foreground_inner(
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
    brave_search_api_key: Option<String>,
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
    let resolved_agent_model = if args.model.as_deref().map(|m| !m.is_empty()).unwrap_or(false) {
        resolve_subagent_model(&runtime.llm_config, args.model.as_deref())
    } else if agent_model_config.map(|m| m != "inherit").unwrap_or(false) {
        resolve_subagent_model(&runtime.llm_config, agent_model_config)
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
        prompt_parts.push(crate::domain::memory::memory_agent_system_prompt_with_config(
            effective_root,
            &mem_cfg,
        ));
    } else {
        let tool_ctx = ToolContext::new(effective_root);
        let agent_specific_prompt = agent_def.system_prompt(&tool_ctx);

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
        prompt_parts.push(skills::format_skills_discovery_system_section());
    }
    sub_cfg.system_prompt = Some(prompt_parts.join("\n\n"));
    let client = create_client(sub_cfg).map_err(|e| e.to_string())?;
    let subagent_opts = SubagentFilterOptions {
        parent_in_plan_mode: parent_in_plan,
        allow_nested_agent: runtime.allow_nested_agent,
    };
    let mut tools = build_subagent_tool_schemas(
        effective_root,
        skills_exist,
        subagent_opts,
    )
    .await;

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
        let (tool_calls, assistant_text, reasoning_text, cancelled, _) = stream_llm_response_with_cancel(
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
            if let Some(tid) = background_task_id {
                persist_background_cancel_notice(&runtime.repo, tid, session_id).await;
            }
            return Err("Sub-agent cancelled.".to_string());
        }
        let tc = completed_to_tool_calls(&tool_calls);
        let asst = Message::Assistant {
            content: assistant_text.clone(),
            tool_calls: tc.clone(),
            token_usage: None,
            reasoning_content: (!reasoning_text.is_empty()).then(|| reasoning_text.clone()),
        };
        if let Some(tid) = background_task_id {
            persist_background_transcript_message(&runtime.repo, tid, session_id, &asst).await;
        }
        transcript.push(asst);
        if tool_calls.is_empty() {
            return Ok(assistant_text);
        }
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
            brave_search_api_key.clone(),
            skill_cache.clone(),
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
    skill_cache: Arc<StdMutex<skills::SkillCacheMap>>,
) -> Result<String, String> {
    // ===== Agent 路由系统集成（含自动调度）=====
    let router = crate::domain::agents::get_agent_router();
    
    // 如果用户没有指定 subagent_type，使用调度器自动选择
    let selected_agent_type = args.subagent_type.as_deref().map(|s| s.to_string()).unwrap_or_else(|| {
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
        // 启动后台 Agent 任务
        return spawn_background_agent(
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
            brave_search_api_key,
            skill_cache,
            agent_def,
        ).await;
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
        brave_search_api_key,
        skill_cache,
        agent_def,
        None,
        None,
    )
    .await
}

/// Write-through snapshot of a background task to SQLite (best-effort).
async fn persist_background_agent_task_snapshot(
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
const BG_SIDECHAIN_CANCEL_NOTICE: &str =
    "[系统] 后台任务已取消（用户或系统终止了运行）。";

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
    tool_results_dir: &std::path::Path,
    effective_root: &std::path::Path,
    session_todos: Option<Arc<Mutex<Vec<TodoItem>>>>,
    session_agent_tasks: Option<Arc<Mutex<Vec<AgentTask>>>>,
    args: &crate::domain::tools::agent::AgentArgs,
    runtime: &AgentLlmRuntime,
    subagent_execute_depth: u8,
    brave_search_api_key: Option<String>,
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
        )
        .await;

    let bg_repo = runtime.repo.clone();
    if let Some(task) = manager.get_task(&task_id).await {
        persist_background_agent_task_snapshot(&bg_repo, &task).await;
    }
    
    // 获取输出文件路径
    let output_path = crate::domain::agents::background::get_background_agent_output_path(app, session_id, &task_id)?;
    
    // 更新任务状态为运行中
    manager.update_task_status(&task_id, BackgroundAgentStatus::Running).await;
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
    let brave_search_api_key_clone = brave_search_api_key.clone();
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
            brave_search_api_key_clone,
            skill_cache_clone,
            cancel_token,
            &task_id_clone,
        ).await;
        
        let manager = crate::domain::agents::background::get_background_agent_manager();
        
        match result {
            Ok(output) => {
                // 写入输出文件
                let summary = format!(
                    "# Background Agent Task: {}\n\n## Agent Type\n{}\n\n## Result\n{}\n",
                    args_clone.description,
                    agent_type_clone,
                    output
                );
                
                if let Err(e) = std::fs::write(&output_path_clone, &summary) {
                    let _ = manager.set_task_error(&task_id_clone, format!("Failed to write output: {}", e)).await;
                } else {
                    let _ = manager.set_task_result(
                        &task_id_clone,
                        output,
                        output_path_clone.to_string_lossy().to_string(),
                    ).await;
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
    
    // 立即返回任务 ID
    Ok(format!(
        "Background agent '{}' started with task ID: {}. \
         The task is running in the background. \
         Use the task ID to check status or retrieve results.",
        agent_def.agent_type(),
        task_id
    ))
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
    brave_search_api_key: Option<String>,
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
        brave_search_api_key,
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
fn is_parallelizable_tool(tool_name: &str) -> bool {
    tool_name.starts_with("mcp__")
        || matches!(
            tool_name,
            "web_search" | "WebSearch" | "web_fetch" | "WebFetch"
            | "file_read" | "Read" | "glob" | "Glob" | "ripgrep" | "Ripgrep" | "grep" | "Grep"
        )
}

fn matches_ask_user_question_name(name: &str) -> bool {
    let n = name.trim();
    n.eq_ignore_ascii_case("ask_user_question")
        || n.eq_ignore_ascii_case("AskUserQuestion")
}

fn ask_user_waiter_key(session_id: &str, message_id: &str, tool_use_id: &str) -> String {
    format!("{}\x1f{}\x1f{}", session_id, message_id, tool_use_id)
}

async fn cancel_ask_user_waiters_for_message(
    waiters: &Arc<Mutex<HashMap<String, AskUserWaiter>>>,
    session_id: &str,
    message_id: &str,
) {
    let prefix = format!("{}\x1f{}\x1f", session_id, message_id);
    let mut map = waiters.lock().await;
    let keys: Vec<String> = map
        .keys()
        .filter(|k| k.starts_with(&prefix))
        .cloned()
        .collect();
    for k in keys {
        if let Some(w) = map.remove(&k) {
            let _ = w.tx.send(Err("User cancelled before answering.".to_string()));
        }
    }
}

async fn cancel_permission_tool_waiters_for_message(
    waiters: &Arc<Mutex<HashMap<String, PermissionToolWaiter>>>,
    session_id: &str,
    message_id: &str,
) {
    let mut map = waiters.lock().await;
    let keys: Vec<String> = map
        .iter()
        .filter(|(_, w)| w.session_id == session_id && w.message_id == message_id)
        .map(|(k, _)| k.clone())
        .collect();
    for k in keys {
        if let Some(w) = map.remove(&k) {
            let _ = w.tx.send(Err("用户已取消".to_string()));
        }
    }
}

async fn cancel_permission_tool_waiters_for_session(
    waiters: &Arc<Mutex<HashMap<String, PermissionToolWaiter>>>,
    session_id: &str,
) {
    let mut map = waiters.lock().await;
    let keys: Vec<String> = map
        .iter()
        .filter(|(_, w)| w.session_id == session_id)
        .map(|(k, _)| k.clone())
        .collect();
    for k in keys {
        if let Some(w) = map.remove(&k) {
            let _ = w.tx.send(Err("会话已关闭".to_string()));
        }
    }
}

fn permission_risk_level_event_str(level: crate::domain::permissions::RiskLevel) -> &'static str {
    match level {
        crate::domain::permissions::RiskLevel::Safe => "safe",
        crate::domain::permissions::RiskLevel::Low => "low",
        crate::domain::permissions::RiskLevel::Medium => "medium",
        crate::domain::permissions::RiskLevel::High => "high",
        crate::domain::permissions::RiskLevel::Critical => "critical",
    }
}

fn build_permission_request_event_json(
    tool_name: &str,
    session_id: &str,
    args_value: &serde_json::Value,
    req: &PermissionRequest,
) -> serde_json::Value {
    let risk_level_str = permission_risk_level_event_str(req.risk.level);
    serde_json::json!({
        "type": "permission_request",
        "request_id": req.request_id,
        "tool_name": tool_name,
        "risk_level": risk_level_str,
        "risk_description": req.risk.description,
        "session_id": session_id,
        "arguments": args_value.clone(),
        "detected_risks": req.risk.detected_risks.iter().map(|r| {
            let severity_str = permission_risk_level_event_str(r.severity);
            serde_json::json!({
                "category": format!("{:?}", r.category),
                "severity": severity_str,
                "description": r.description,
                "mitigation": r.mitigation,
            })
        }).collect::<Vec<_>>(),
        "recommendations": req.risk.recommendations,
    })
}

/// Register a waiter, emit `permission-request`, then block until approve/deny/cancel.
async fn wait_for_permission_tool_resolution(
    app: &AppHandle,
    app_state: &OmigaAppState,
    session_id: &str,
    message_id: &str,
    tool_use_id: &str,
    stream_tool_name: &str,
    tool_name_for_event: &str,
    arguments_display: &str,
    args_value: &serde_json::Value,
    req: &PermissionRequest,
    cancel_flag: Option<Arc<RwLock<bool>>>,
) -> Result<(), String> {
    let request_id = req.request_id.clone();
    let (tx, mut rx) = tokio::sync::oneshot::channel::<Result<(), String>>();
    {
        let mut map = app_state.chat.permission_tool_waiters.lock().await;
        map.insert(
            request_id.clone(),
            PermissionToolWaiter {
                tx,
                session_id: session_id.to_string(),
                message_id: message_id.to_string(),
            },
        );
    }

    let permission_event =
        build_permission_request_event_json(tool_name_for_event, session_id, args_value, req);
    let _ = app.emit("permission-request", &permission_event);

    let pending_msg = format!(
        "⏳ 需要权限确认: {}\n\n风险级别: {:?}\n{}\n\n请在输入框上方批准或拒绝。",
        tool_name_for_event,
        req.risk.level,
        req.risk.description
    );
    let _ = app.emit(
        &format!("chat-stream-{}", message_id),
        &StreamOutputItem::ToolResult {
            tool_use_id: tool_use_id.to_string(),
            name: stream_tool_name.to_string(),
            input: arguments_display.to_string(),
            output: pending_msg,
            is_error: false,
        },
    );

    let mut interval = tokio::time::interval(std::time::Duration::from_millis(120));
    interval.tick().await;
    let outcome = loop {
        tokio::select! {
            res = &mut rx => break res,
            _ = interval.tick() => {
                if let Some(ref f) = cancel_flag {
                    if *f.read().await {
                        let mut map = app_state.chat.permission_tool_waiters.lock().await;
                        map.remove(&request_id);
                        return Err("用户已取消".to_string());
                    }
                }
            }
        }
    };

    {
        let mut map = app_state.chat.permission_tool_waiters.lock().await;
        map.remove(&request_id);
    }

    match outcome {
        Ok(inner) => inner,
        Err(_) => Err("权限确认通道意外关闭".to_string()),
    }
}

fn build_ask_user_success_output(
    questions: &[ask_user_question::QuestionItem],
    answers: &serde_json::Value,
) -> Result<String, String> {
    let obj = answers
        .as_object()
        .ok_or_else(|| "answers must be a JSON object".to_string())?;
    for q in questions {
        let qt = q.question.trim();
        if !obj.contains_key(qt) {
            return Err(format!("Missing answer for question: {}", q.question));
        }
    }
    let mut body = serde_json::Map::new();
    body.insert(
        "questions".to_string(),
        serde_json::to_value(questions).map_err(|e| e.to_string())?,
    );
    body.insert(
        "answers".to_string(),
        serde_json::Value::Object(obj.clone()),
    );
    body.insert(
        "_omiga".to_string(),
        serde_json::json!("User answered via Omiga chat UI."),
    );
    serde_json::to_string_pretty(&serde_json::Value::Object(body)).map_err(|e| e.to_string())
}

/// Chat path: block until the user submits answers in the Omiga UI (or cancel).
async fn execute_ask_user_question_interactive(
    tool_use_id: String,
    tool_name: String,
    arguments: String,
    app: AppHandle,
    message_id: String,
    session_id: String,
    tool_results_dir: &Path,
    waiters: Arc<Mutex<HashMap<String, AskUserWaiter>>>,
    cancel_flag: Option<Arc<RwLock<bool>>>,
) -> (String, String, bool) {
    let args: ask_user_question::AskUserQuestionArgs = match serde_json::from_str(&arguments) {
        Err(e) => {
            let error_msg = format!("Failed to parse ask_user_question arguments: {}", e);
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
            return (tool_use_id, error_msg, true);
        }
        Ok(a) => a,
    };
    if let Err(e) = ask_user_question::validate_ask_user_question_args(&args) {
        let error_msg = format!("Invalid ask_user_question arguments: {}", e);
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
        return (tool_use_id, error_msg, true);
    }

    let key = ask_user_waiter_key(&session_id, &message_id, &tool_use_id);
    let (tx, mut rx) = tokio::sync::oneshot::channel::<Result<serde_json::Value, String>>();
    {
        let mut map = waiters.lock().await;
        map.insert(key.clone(), AskUserWaiter { tx });
    }

    let questions_value = match serde_json::to_value(&args.questions) {
        Ok(v) => v,
        Err(e) => {
            let mut map = waiters.lock().await;
            map.remove(&key);
            let error_msg = format!("Failed to serialize questions: {}", e);
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
            return (tool_use_id, error_msg, true);
        }
    };

    let _ = app.emit(
        &format!("chat-stream-{}", message_id),
        &StreamOutputItem::AskUserPending {
            session_id: session_id.clone(),
            message_id: message_id.clone(),
            tool_use_id: tool_use_id.clone(),
            questions: questions_value,
        },
    );

    let mut interval = tokio::time::interval(std::time::Duration::from_millis(120));
    interval.tick().await;
    let outcome = loop {
        tokio::select! {
            res = &mut rx => {
                break res;
            }
            _ = interval.tick() => {
                if let Some(ref f) = cancel_flag {
                    if *f.read().await {
                        let mut map = waiters.lock().await;
                        if let Some(w) = map.remove(&key) {
                            let _ = w.tx.send(Err(
                                "User cancelled before answering.".to_string(),
                            ));
                        }
                    }
                }
            }
        }
    };

    {
        let mut map = waiters.lock().await;
        map.remove(&key);
    }

    let answers_val = match outcome {
        Ok(Ok(v)) => v,
        Ok(Err(e)) => {
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
            return (tool_use_id, e, true);
        }
        Err(_) => {
            let err = "Ask user channel closed unexpectedly.".to_string();
            let _ = app.emit(
                &format!("chat-stream-{}", message_id),
                &StreamOutputItem::ToolResult {
                    tool_use_id: tool_use_id.clone(),
                    name: tool_name.clone(),
                    input: arguments.clone(),
                    output: err.clone(),
                    is_error: true,
                },
            );
            return (tool_use_id, err, true);
        }
    };

    let output_text = match build_ask_user_success_output(&args.questions, &answers_val) {
        Ok(s) => s,
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
            return (tool_use_id, e, true);
        }
    };

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
        let prefix = truncate_utf8_prefix(&arguments, TOOL_DISPLAY_MAX_INPUT_CHARS);
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
            is_error: false,
        },
    );

    let model_output = process_tool_output_for_model(
        output_text.clone(),
        &tool_use_id,
        tool_results_dir,
    )
    .await;
    (tool_use_id, model_output, false)
}

/// Execute tool calls and return results.
/// Parallelizable tools (MCP, web_search, web_fetch, file_read, glob, ripgrep) run concurrently;
/// stateful tools (Agent, file_edit, file_write, bash, todo_write, …) run sequentially.
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
    skill_cache: Arc<StdMutex<skills::SkillCacheMap>>,
) -> Vec<(String, String, bool)> {
    use futures::future::join_all;

    let cancel_flag = agent_runtime.map(|r| r.cancel_flag.clone());

    // (tool_use_id, output, is_error)
    let mut results = Vec::new();
    let deny_entries =
        crate::domain::tool_permission_rules::load_merged_permission_deny_rule_entries(project_root);

    // Pre-compute permission + subagent-filter results for every call (fast, sequential).
    // Calls that pass become futures; blocked calls become immediate error results.
    enum CallPrep<'a> {
        Blocked(String, String, bool), // (tool_use_id, error_msg, is_error=true)
        Ready(&'a str), // tool_name only (indices carry id+args via tool_calls[idx])
    }

    let prepped: Vec<CallPrep<'_>> = tool_calls
        .iter()
        .map(|(tool_use_id, tool_name, _arguments)| {
            if let Some(hit) = crate::domain::tool_permission_rules::matching_deny_entry(
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
                let c = crate::domain::tool_permission_rules::canonical_permission_tool_name(
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
                if crate::domain::subagent_tool_filter::should_block_subagent_builtin_call(
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
    let mut ordered_results: Vec<Option<(String, String, bool)>> =
        vec![None; tool_calls.len()];

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
                ordered_results[idx] =
                    Some((tool_use_id.clone(), error_msg.clone(), *is_error));
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
                execute_one_tool(
                    tool_use_id.clone(),
                    tool_name.clone(),
                    arguments.clone(),
                    app.clone(),
                    message_id.to_string(),
                    session_id.to_string(),
                    tool_results_dir.to_path_buf(),
                    project_root.to_path_buf(),
                    session_todos.clone(),
                    session_agent_tasks.clone(),
                    None, // parallelizable tools don't need agent_runtime
                    subagent_depth,
                    skill_task_context.map(str::to_owned),
                    brave_search_api_key.clone(),
                    skill_cache.clone(),
                    cancel_flag.clone(),
                )
            })
            .collect();

        let parallel_results = join_all(parallel_futures).await;
        for (&idx, res) in parallel_indices.iter().zip(parallel_results) {
            ordered_results[idx] = Some(res);
        }
    }

    // --- Sequential batch: stateful tools run one-by-one ---
    for idx in sequential_indices {
        let (tool_use_id, tool_name, arguments) = &tool_calls[idx];
        let res = execute_one_tool(
            tool_use_id.clone(),
            tool_name.clone(),
            arguments.clone(),
            app.clone(),
            message_id.to_string(),
            session_id.to_string(),
            tool_results_dir.to_path_buf(),
            project_root.to_path_buf(),
            session_todos.clone(),
            session_agent_tasks.clone(),
            agent_runtime,
            subagent_depth,
            skill_task_context.map(str::to_owned),
            brave_search_api_key.clone(),
            skill_cache.clone(),
            cancel_flag.clone(),
        )
        .await;
        ordered_results[idx] = Some(res);
    }

    results.extend(ordered_results.into_iter().flatten());
    results
}

/// Execute a single tool call. Called from both the parallel and sequential paths.
#[async_recursion::async_recursion]
async fn execute_one_tool(
    tool_use_id: String,
    tool_name: String,
    arguments: String,
    app: AppHandle,
    message_id: String,
    session_id: String,
    tool_results_dir: PathBuf,
    project_root: PathBuf,
    session_todos: Option<Arc<tokio::sync::Mutex<Vec<TodoItem>>>>,
    session_agent_tasks: Option<Arc<tokio::sync::Mutex<Vec<AgentTask>>>>,
    agent_runtime: Option<&AgentLlmRuntime>,
    subagent_depth: u8,
    skill_task_context: Option<String>,
    brave_search_api_key: Option<String>,
    skill_cache: Arc<StdMutex<skills::SkillCacheMap>>,
    cancel_flag: Option<Arc<RwLock<bool>>>,
) -> (String, String, bool) {
    let tool_use_id = &tool_use_id;
    let tool_name = &tool_name;
    let arguments = &arguments;
    let message_id = &message_id;
    let session_id = &session_id;
    let tool_results_dir = tool_results_dir.as_path();
    let project_root = project_root.as_path();
    let skill_task_context = skill_task_context.as_deref();

    // Subagent plan-mode re-check (fast, per-call)
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
        let allow_nested = agent_runtime
            .map(|r| r.allow_nested_agent)
            .unwrap_or(false);
        let sub_opts = SubagentFilterOptions {
            parent_in_plan_mode: parent_in_plan,
            allow_nested_agent: allow_nested,
        };
        if crate::domain::subagent_tool_filter::should_block_subagent_builtin_call(&c, sub_opts) {
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
            return (tool_use_id.clone(), error_msg, true);
        }
    }

    // === Permission Check for ALL tools (not just skill) ===
    // Skip permission check for certain safe tools
    let needs_permission_check = !matches!(tool_name.as_str(),
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
                .check_tool(session_id, &tool_name, &args_value)
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
                    match wait_for_permission_tool_resolution(
                        &app,
                        &app_state,
                        session_id,
                        message_id,
                        tool_use_id,
                        tool_name,
                        tool_name,
                        arguments,
                        &args_value,
                        req,
                        cancel_flag.clone(),
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
            let mut all_skills =
                skills::load_skills_cached(project_root, &skill_cache).await;
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
                                    json,
                                    tool_use_id,
                                    tool_results_dir,
                                )
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
                Ok(v) => (serde_json::to_string_pretty(&v).unwrap_or_else(|_| "{\"success\":false}".to_string()), false),
                Err(e) => {
                    tracing::warn!(tool = "skill_config", error = %e, "skill_config failed");
                    (e, true)
                }
            };
            let display_output = if json.len() > PREVIEW_SIZE_BYTES {
                let prefix = truncate_utf8_prefix(&json, PREVIEW_SIZE_BYTES);
                format!("{}\n\n[Output truncated... {} total characters]", prefix, json.len())
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
                        tracing::warn!(tool = "skill", error = "empty_skill_name", "Skill tool called with empty skill name");
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
                        let all_skills =
                            skills::load_skills_cached(project_root, &skill_cache).await;
                        let resolved_name = skills::resolve_skill_display_name(&all_skills, &args.skill);
                        let blocked = resolved_name.as_ref()
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
                            let skill_display = resolved_name
                                .unwrap_or_else(|| args.skill.trim().to_string());
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
                                    .check_tool(session_id, &skill_display, &args_value)
                                    .await;

                                match perm_decision {
                                    crate::domain::permissions::PermissionDecision::Deny(ref reason) => {
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
                                            &app,
                                            &app_state_skill,
                                            session_id,
                                            message_id,
                                            tool_use_id,
                                            tool_name,
                                            skill_display.as_str(),
                                            arguments,
                                            &args_value,
                                            req,
                                            cancel_flag.clone(),
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
                                        skills::find_skill_entry(&all_skills, name)
                                            .map(|entry| {
                                                let skill_path = entry.skill_dir.join("SKILL.md");
                                                std::fs::read_to_string(&skill_path)
                                                    .unwrap_or_else(|_| {
                                                        format!("# {}\n\nSkill content not available", entry.name)
                                                    })
                                            })
                                    })
                                    .unwrap_or_else(|| {
                                        format!("Skill: {}\n\nArgs: {}", args.skill, args.args)
                                    });

                                // Get allowed_tools for forked execution
                                let skill_allowed_tools: Option<Vec<String>> = resolved_name
                                    .as_ref()
                                    .and_then(|name| {
                                        skills::find_skill_entry(&all_skills, name)
                                            .map(|entry| entry.allowed_tools.clone())
                                    });

                                // Need Agent runtime for forked execution
                                if let Some(ref ar) = agent_runtime {
                                    match run_skill_forked(
                                        &app,
                                        message_id,
                                        session_id,
                                        tool_results_dir,
                                        project_root,
                                        session_todos.clone(),
                                        session_agent_tasks.clone(),
                                        &skill_display,
                                        &args.args,
                                        &skill_content,
                                        skill_allowed_tools,
                                        ar,
                                        subagent_depth.saturating_add(1),
                                        brave_search_api_key.clone(),
                                        skill_cache.clone(),
                                    )
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
                                                output_text,
                                                tool_use_id,
                                                tool_results_dir,
                                            )
                                            .await;
                                            return (tool_use_id.clone(), model_output, is_error);
                                        }
                                        Err(e) => {
                                            let error_msg = format!("Skill forked execution failed: {}", e);
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
                            // Original inline execution code continues...
                            match skills::invoke_skill_with_cache(
                                project_root,
                                &args.skill,
                                &args.args,
                                &all_skills,
                            )
                            .await
                            {
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
                            &app,
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
                            skill_cache.clone(),
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
                std::sync::Arc<tokio::sync::Mutex<std::collections::HashMap<String, crate::domain::mcp::client::McpLiveConnection>>>
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
                    return execute_ask_user_question_interactive(
                        tool_use_id.to_string(),
                        tool_name.to_string(),
                        arguments.to_string(),
                        app.clone(),
                        message_id.to_string(),
                        session_id.to_string(),
                        tool_results_dir,
                        state.chat.ask_user_waiters.clone(),
                        cancel_flag.clone(),
                    )
                    .await;
                }
            }
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

    result
}

/// Submit answers for a blocked `ask_user_question` tool call (chat UI).
#[tauri::command]
pub async fn submit_ask_user_answer(
    app_state: State<'_, OmigaAppState>,
    session_id: String,
    message_id: String,
    tool_use_id: String,
    answers: serde_json::Value,
) -> CommandResult<()> {
    let key = ask_user_waiter_key(&session_id, &message_id, &tool_use_id);
    let mut map = app_state.chat.ask_user_waiters.lock().await;
    let Some(waiter) = map.remove(&key) else {
        return Err(OmigaError::Chat(ChatError::StreamError(
            "No pending ask_user_question for this tool call (already answered or expired)."
                .to_string(),
        )));
    };
    drop(map);
    let _ = waiter.tx.send(Ok(answers));
    Ok(())
}

/// Cancel an in-progress stream by message_id
#[tauri::command]
pub async fn cancel_stream(
    app_state: State<'_, OmigaAppState>,
    message_id: String,
) -> CommandResult<()> {
    let session_for_waiters = {
        let ar = app_state.chat.active_rounds.lock().await;
        ar.get(&message_id).map(|r| r.session_id.clone())
    };
    if let Some(ref sid) = session_for_waiters {
        cancel_ask_user_waiters_for_message(
            &app_state.chat.ask_user_waiters,
            sid,
            &message_id,
        )
        .await;
        cancel_permission_tool_waiters_for_message(
            &app_state.chat.permission_tool_waiters,
            sid,
            &message_id,
        )
        .await;
    } else {
        let repo = &*app_state.repo;
        if let Ok(Some(round)) = repo.get_round_by_message_id(&message_id).await {
            cancel_ask_user_waiters_for_message(
                &app_state.chat.ask_user_waiters,
                &round.session_id,
                &message_id,
            )
            .await;
            cancel_permission_tool_waiters_for_message(
                &app_state.chat.permission_tool_waiters,
                &round.session_id,
                &message_id,
            )
            .await;
        }
    }

    // Look up the round by message_id
    let repo = &*app_state.repo;

    // Find active round
    if let Ok(Some(round)) = repo.get_round_by_message_id(&message_id).await {
        if round.is_active() {
            // Cancel in database
            repo.cancel_round(&round.id, Some("User requested cancellation"))
                .await
                .map_err(|e| {
                    OmigaError::Chat(ChatError::StreamError(format!("Failed to cancel round: {}", e)))
                })?;

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

            let repo = &*app_state.repo;
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
    let repo = &*app_state.repo;

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

    cancel_permission_tool_waiters_for_session(
        &app_state.chat.permission_tool_waiters,
        &session_id,
    )
    .await;

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
    thinking: Option<bool>,
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
    // Moonshot/Custom: always keep an explicit bool in memory (default false) so runtime matches API.
    // DeepSeek and others do not use `thinking`.
    match provider_enum {
        crate::llm::LlmProvider::Moonshot | crate::llm::LlmProvider::Custom => {
            config.thinking = Some(thinking.unwrap_or(false));
        }
        _ => {
            config.thinking = None;
        }
    }

    let mut config_guard = state.chat.llm_config.lock().await;
    *config_guard = Some(config);
    *state.chat.active_provider_entry_name.lock().await = None;
    Ok(())
}

/// Persist the Settings panel LLM choice to `omiga.yaml` as `default_provider`.
/// Does **not** overwrite the in-memory provider used for the current chat when the user has
/// switched to another named config via `quick_switch_provider` — only updates `omiga.yaml` and
/// refreshes runtime when no active entry is set or the active entry is the same one being saved.
#[tauri::command]
pub async fn save_llm_settings_to_config(
    state: State<'_, OmigaAppState>,
    provider: String,
    api_key: String,
    secret_key: Option<String>,
    app_id: Option<String>,
    model: Option<String>,
    base_url: Option<String>,
    thinking: Option<bool>,
) -> CommandResult<()> {
    let provider_enum = provider
        .parse::<LlmProvider>()
        .map_err(|e| OmigaError::Config(format!("Invalid provider: {}", e)))?;

    let model_str = model
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| provider_enum.default_model().to_string());

    let sk_opt = secret_key.and_then(|s| {
        let t = s.trim();
        if t.is_empty() {
            None
        } else {
            Some(t.to_string())
        }
    });
    let aid_opt = app_id.and_then(|s| {
        let t = s.trim();
        if t.is_empty() {
            None
        } else {
            Some(t.to_string())
        }
    });
    let bu_opt = base_url.and_then(|s| {
        let t = s.trim();
        if t.is_empty() {
            None
        } else {
            Some(t.to_string())
        }
    });

    let mut config_file = crate::llm::config::load_config_file().unwrap_or_default();
    if config_file.providers.is_none() {
        config_file.providers = Some(HashMap::new());
    }
    let providers = config_file.providers.as_mut().unwrap();

    let entry_key = match config_file.default_provider.clone() {
        Some(ref k) if providers.contains_key(k) => k.clone(),
        _ => "default".to_string(),
    };

    let prev_thinking = providers.get(&entry_key).and_then(|p| p.thinking);
    let thinking_resolved = match provider_enum {
        crate::llm::LlmProvider::Moonshot | crate::llm::LlmProvider::Custom => {
            thinking.or(prev_thinking).or(Some(false))
        }
        _ => None,
    };

    let provider_cfg = crate::llm::config::ProviderConfig {
        provider_type: provider,
        api_key: Some(api_key.clone()),
        secret_key: sk_opt.clone(),
        app_id: aid_opt.clone(),
        base_url: bu_opt.clone(),
        model: Some(model_str.clone()),
        thinking: thinking_resolved,
        enabled: true,
        ..Default::default()
    };
    providers.insert(entry_key.clone(), provider_cfg);
    config_file.default_provider = Some(entry_key.clone());

    let config_path = crate::llm::config::find_config_file()
        .or_else(|| dirs::config_dir().map(|d| d.join("omiga").join("omiga.yaml")))
        .ok_or_else(|| OmigaError::Config("Could not determine config file path".to_string()))?;

    if let Some(parent) = config_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    crate::llm::config::save_config_file(&config_file, &config_path)
        .map_err(|e| OmigaError::Config(format!("Failed to save config: {}", e)))?;

    let mut config = LlmConfig::new(provider_enum, api_key);
    config.model = model_str;
    config.secret_key = sk_opt;
    config.app_id = aid_opt;
    config.base_url = bu_opt;
    config.thinking = thinking_resolved;

    let active_opt = state.chat.active_provider_entry_name.lock().await.clone();
    let should_apply_runtime = match active_opt.as_deref() {
        None => true,
        Some(active) => active == entry_key.as_str(),
    };

    if should_apply_runtime {
        let mut config_guard = state.chat.llm_config.lock().await;
        *config_guard = Some(config);
        drop(config_guard);
        let mut active_guard = state.chat.active_provider_entry_name.lock().await;
        if active_guard.is_none() {
            *active_guard = Some(entry_key.clone());
        }
    }
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
        thinking: config.thinking,
    }))
}

/// LLM configuration response for frontend
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmConfigResponse {
    pub provider: String,
    pub api_key_preview: String,
    pub model: Option<String>,
    pub base_url: Option<String>,
    pub thinking: Option<bool>,
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
            drop(stored);
            *state.chat.active_provider_entry_name.lock().await = None;
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

/// 初始 Todo 项（用于 Plan mode）
#[derive(Debug, Clone, Serialize)]
pub struct InitialTodoItem {
    pub id: String,
    pub content: String,
    pub status: String, // "pending" | "in_progress" | "completed"
}

/// Response from send_message
#[derive(Debug, Serialize)]
pub struct MessageResponse {
    pub message_id: String,
    pub session_id: String,
    pub round_id: String,
    /// Persisted SQLite row id for the user message for this turn (for client-side retry anchoring).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_message_id: Option<String>,
    /// `leader` (default) | `background_followup_queued` when text was routed to a bg task queue.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_kind: Option<String>,
    /// 调度系统生成的任务执行计划（如果有）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scheduler_plan: Option<crate::domain::agents::scheduler::SchedulingResult>,
    /// Plan mode 下的初始 todo 列表
    #[serde(skip_serializing_if = "Option::is_none")]
    pub initial_todos: Option<Vec<InitialTodoItem>>,
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
#[serde(rename_all = "camelCase")]
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
    /// Optional routing: omit / `leader` = main session; `bg:<task_id>` = queue follow-up for that background Agent task.
    #[serde(default, rename = "inputTarget")]
    pub input_target: Option<String>,
    /// Specialist agent id from [`list_available_agents`] (e.g. Explore, Plan). Omit or `general-purpose` for default.
    #[serde(default, rename = "composerAgentType")]
    pub composer_agent_type: Option<String>,
    /// `ask` | `auto` | `bypass` — user-facing permission stance for this turn.
    #[serde(default, rename = "permissionMode")]
    pub permission_mode: Option<String>,
    /// When set, truncate SQLite transcript after this user row and reuse it instead of inserting a new user message.
    #[serde(default, rename = "retryFromUserMessageId")]
    pub retry_from_user_message_id: Option<String>,
}

/// Background Agent tasks for one session (Claude Code–style teammate follow-ups).
/// Merges SQLite rows with in-memory manager so live tasks overlay DB after restart.
#[tauri::command]
pub async fn list_session_background_tasks(
    app_state: State<'_, OmigaAppState>,
    session_id: String,
) -> CommandResult<Vec<crate::domain::agents::background::BackgroundAgentTask>> {
    let mgr = crate::domain::agents::background::get_background_agent_manager();
    let from_mem = mgr.get_session_tasks(&session_id).await;

    let mut from_db = {
        let repo = &*app_state.repo;
        repo.list_background_agent_tasks_for_session(&session_id)
            .await
            .map_err(|e| OmigaError::Persistence(e.to_string()))?
    };

    for t in from_mem {
        if let Some(existing) = from_db.iter_mut().find(|x| x.task_id == t.task_id) {
            *existing = t;
        } else {
            from_db.push(t);
        }
    }
    from_db.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(from_db)
}

/// Load persisted sidechain transcript for one background Agent task (teammate view).
#[tauri::command]
pub async fn load_background_agent_transcript(
    app_state: State<'_, OmigaAppState>,
    session_id: String,
    task_id: String,
) -> CommandResult<Vec<Message>> {
    let repo = &*app_state.repo;
    let task = repo
        .get_background_agent_task_by_id(&task_id)
        .await
        .map_err(|e| OmigaError::Persistence(e.to_string()))?;
    let Some(task) = task else {
        return Err(OmigaError::NotFound {
            resource: format!("background task {}", task_id),
        });
    };
    if task.session_id != session_id {
        return Err(OmigaError::Chat(ChatError::StreamError(
            "background task does not belong to this session".to_string(),
        )));
    }
    repo.list_background_agent_messages_for_task(&task_id)
        .await
        .map_err(|e| OmigaError::Persistence(e.to_string()))
}

/// Cancel a background Agent task: cancellation token + memory state, write-through to SQLite,
/// and DB-only reconcile when the worker is gone (e.g. after app restart) but the row is still pending/running.
#[tauri::command]
pub async fn cancel_background_agent_task(
    app: tauri::AppHandle,
    app_state: State<'_, OmigaAppState>,
    session_id: String,
    task_id: String,
) -> CommandResult<crate::domain::agents::background::BackgroundAgentTask> {
    use crate::domain::agents::background::{
        emit_background_agent_complete, emit_background_agent_update,
        get_background_agent_manager, BackgroundAgentStatus,
    };

    let mgr = get_background_agent_manager();

    if let Some(existing) = mgr.get_task(&task_id).await {
        if existing.session_id != session_id {
            return Err(OmigaError::Chat(ChatError::StreamError(
                "background task does not belong to this session".to_string(),
            )));
        }
    }

    if let Some(task) = mgr.cancel_task(&task_id).await {
        persist_background_agent_task_snapshot(&app_state.repo, &task).await;
        if let Err(e) = emit_background_agent_update(&app, &task) {
            tracing::warn!(target: "omiga::bg_agent", "emit background-agent-update failed: {}", e);
        }
        if let Err(e) = emit_background_agent_complete(&app, &task) {
            tracing::warn!(target: "omiga::bg_agent", "emit background-agent-complete failed: {}", e);
        }
        return Ok(task);
    }

    let repo = &*app_state.repo;
    let row = repo
        .get_background_agent_task_by_id(&task_id)
        .await
        .map_err(|e| OmigaError::Persistence(e.to_string()))?;

    let Some(mut task) = row else {
        return Err(OmigaError::NotFound {
            resource: format!("background task {}", task_id),
        });
    };

    if task.session_id != session_id {
        return Err(OmigaError::Chat(ChatError::StreamError(
            "background task does not belong to this session".to_string(),
        )));
    }

    match task.status {
        BackgroundAgentStatus::Completed
        | BackgroundAgentStatus::Failed
        | BackgroundAgentStatus::Cancelled => {
            return Ok(task);
        }
        BackgroundAgentStatus::Pending | BackgroundAgentStatus::Running => {
            task.status = BackgroundAgentStatus::Cancelled;
            task.completed_at = Some(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
            );
            repo
                .upsert_background_agent_task(&task)
                .await
                .map_err(|e| OmigaError::Persistence(e.to_string()))?;
            if let Err(e) = emit_background_agent_update(&app, &task) {
                tracing::warn!(target: "omiga::bg_agent", "emit background-agent-update failed: {}", e);
            }
            if let Err(e) = emit_background_agent_complete(&app, &task) {
                tracing::warn!(target: "omiga::bg_agent", "emit background-agent-complete failed: {}", e);
            }
            Ok(task)
        }
    }
}

/// Provider configuration entry for multi-provider management
#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ProviderConfigEntry {
    pub name: String,
    pub provider_type: String,
    pub model: String,
    pub api_key_preview: String,
    pub base_url: Option<String>,
    /// Moonshot / Custom only: request `thinking: true` and stream `reasoning_content`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<bool>,
    pub enabled: bool,
    /// Matches in-memory runtime config (current chat session / quick switch).
    pub is_session_active: bool,
    /// `default` in `omiga.yaml` — the default used on startup and after Settings save.
    pub is_default: bool,
}

/// List all configured providers from the config file
#[tauri::command]
pub async fn list_provider_configs(
    state: State<'_, OmigaAppState>,
) -> CommandResult<Vec<ProviderConfigEntry>> {
    // Runtime LLM config (quick-switch / last resolved load) — used to mark which row matches reality
    let current_config = state.chat.llm_config.lock().await;
    let runtime = current_config.clone();
    drop(current_config);

    let active_entry = state.chat.active_provider_entry_name.lock().await.clone();

    // Try to load from config file
    let config_file = match crate::llm::config::load_config_file() {
        Ok(cf) => cf,
        Err(_) => {
            // No config file exists, return empty list
            return Ok(vec![]);
        }
    };

    let providers = config_file.providers.unwrap_or_default();
    let default_provider = config_file.default_provider.unwrap_or_default();

    let entries: Vec<ProviderConfigEntry> = providers
        .iter()
        .map(|(name, config)| {
            let api_key_preview = config
                .api_key
                .as_ref()
                .map(|k| {
                    if k.len() > 8 {
                        format!("{}...", &k[..8])
                    } else {
                        k.clone()
                    }
                })
                .unwrap_or_default();

            // Prefer the explicit saved config name so two entries with the same provider+model
            // cannot both show "In use".
            let matches_runtime = match active_entry.as_deref() {
                Some(active) => name == active,
                None => runtime.as_ref().map_or(false, |c| {
                    let Ok(pt) = config.provider_type.parse::<LlmProvider>() else {
                        return false;
                    };
                    let entry_model = config.model.as_deref().unwrap_or("").trim();
                    let run_model = c.model.trim();
                    pt == c.provider && entry_model == run_model
                }),
            };

            ProviderConfigEntry {
                name: name.clone(),
                provider_type: config.provider_type.clone(),
                model: config.model.clone().unwrap_or_default(),
                api_key_preview,
                base_url: config.base_url.clone(),
                thinking: config.thinking,
                enabled: config.enabled,
                is_session_active: matches_runtime,
                is_default: name == &default_provider,
            }
        })
        .collect();

    Ok(entries)
}

/// Switch to a different provider by name
#[tauri::command]
pub async fn switch_provider(
    state: State<'_, OmigaAppState>,
    provider_name: String,
) -> CommandResult<LlmConfigResponse> {
    // Load config file
    let config_file = crate::llm::config::load_config_file()
        .map_err(|e| OmigaError::Config(format!("Failed to load config file: {}", e)))?;

    // Find the provider in the config - extract all values by cloning
    let provider_config = config_file
        .providers
        .as_ref()
        .and_then(|p| p.get(&provider_name))
        .cloned()
        .ok_or_else(|| OmigaError::Config(format!("Provider '{}' not found", provider_name)))?;

    if !provider_config.enabled {
        return Err(OmigaError::Config(format!(
            "Provider '{}' is disabled",
            provider_name
        )));
    }

    // Parse provider type
    let provider_enum = provider_config
        .provider_type
        .parse::<LlmProvider>()
        .map_err(|e| OmigaError::Config(format!("Invalid provider type: {}", e)))?;

    // Get API key with env var expansion
    let api_key = provider_config
        .api_key
        .as_ref()
        .map(|k| expand_env_vars(k))
        .filter(|k| !k.is_empty())
        .ok_or_else(|| OmigaError::Config(format!("API key not set for provider '{}'", provider_name)))?;

    // Build config
    let mut config = LlmConfig::new(provider_enum, api_key);

    if let Some(secret) = provider_config.secret_key {
        config.secret_key = Some(expand_env_vars(&secret));
    }
    if let Some(app_id) = provider_config.app_id {
        config.app_id = Some(expand_env_vars(&app_id));
    }
    if let Some(model) = provider_config.model {
        config.model = expand_env_vars(&model);
    }
    if let Some(url) = provider_config.base_url {
        config.base_url = Some(expand_env_vars(&url));
    }
    if let Some(tokens) = provider_config.max_tokens {
        config.max_tokens = tokens;
    }
    if let Some(temp) = provider_config.temperature {
        config.temperature = Some(temp);
    }
    if let Some(timeout) = provider_config.timeout {
        config.timeout_secs = timeout;
    }
    match provider_enum {
        LlmProvider::Moonshot | LlmProvider::Custom => {
            config.thinking = Some(provider_config.thinking.unwrap_or(false));
        }
        _ => {
            config.thinking = None;
        }
    }

    // Update active config in state (session only — does not change `default` in omiga.yaml)
    let mut config_guard = state.chat.llm_config.lock().await;
    *config_guard = Some(config.clone());
    drop(config_guard);
    *state.chat.active_provider_entry_name.lock().await = Some(provider_name);

    Ok(LlmConfigResponse {
        provider: format!("{:?}", provider_enum),
        api_key_preview: if config.api_key.len() > 8 {
            format!("{}...", &config.api_key[..8])
        } else {
            config.api_key.clone()
        },
        model: Some(config.model),
        base_url: config.base_url,
        thinking: config.thinking,
    })
}

/// Save a provider configuration to the multi-provider config file.
/// `thinking`: when set, applies to Moonshot/Custom only; other provider types clear stored thinking.
#[tauri::command]
pub async fn save_provider_config(
    state: State<'_, OmigaAppState>,
    name: String,
    provider_type: String,
    api_key: String,
    model: String,
    secret_key: Option<String>,
    app_id: Option<String>,
    base_url: Option<String>,
    set_as_default: Option<bool>,
    thinking: Option<bool>,
) -> CommandResult<()> {
    // Validate required fields
    if name.trim().is_empty() {
        return Err(OmigaError::Config("Configuration name is required".to_string()));
    }
    if provider_type.trim().is_empty() {
        return Err(OmigaError::Config("Provider type is required".to_string()));
    }
    if model.trim().is_empty() {
        return Err(OmigaError::Config("Model name is required".to_string()));
    }

    let provider_enum = provider_type
        .parse::<LlmProvider>()
        .map_err(|e| OmigaError::Config(format!("Invalid provider type: {}", e)))?;

    // Load existing config or create new one
    let mut config_file = crate::llm::config::load_config_file().unwrap_or_default();

    // Ensure providers map exists
    if config_file.providers.is_none() {
        config_file.providers = Some(std::collections::HashMap::new());
    }

    let providers = config_file.providers.as_mut().unwrap();

    // Check if we're updating an existing provider and need to preserve the existing API key
    let final_api_key = if api_key == "${KEEP_EXISTING}" {
        // Preserve existing API key when editing
        let existing = providers
            .get(&name)
            .and_then(|p| p.api_key.clone())
            .filter(|k| !k.is_empty());
        if existing.is_none() {
            return Err(OmigaError::Config("API key is required (existing key not found)".to_string()));
        }
        existing.unwrap()
    } else {
        if api_key.trim().is_empty() {
            return Err(OmigaError::Config("API key is required".to_string()));
        }
        api_key
    };

    let existing_thinking = providers.get(&name).and_then(|p| p.thinking);
    let thinking_for_entry = match provider_enum {
        crate::llm::LlmProvider::Moonshot | crate::llm::LlmProvider::Custom => match thinking {
            Some(t) => Some(t),
            // New row: default false; existing row: keep file value when TS omits the field.
            None => existing_thinking.or(Some(false)),
        },
        // DeepSeek et al.: no thinking mode — never persist.
        _ => None,
    };

    // Create or update provider config
    let provider_config = crate::llm::config::ProviderConfig {
        provider_type: provider_type.clone(),
        api_key: Some(final_api_key),
        secret_key,
        app_id,
        base_url,
        model: Some(model),
        enabled: true,
        thinking: thinking_for_entry,
        ..Default::default()
    };

    providers.insert(name.clone(), provider_config);

    // Set as default if requested
    if set_as_default.unwrap_or(false) {
        config_file.default_provider = Some(name.clone());

        // Also update the active config in state
        let saved = providers.get(&name).unwrap();
        let mut new_config = LlmConfig::new(
            provider_enum,
            saved.api_key.clone().unwrap_or_default(),
        )
        .with_model(saved.model.clone().unwrap_or_default());
        if let Some(url) = &saved.base_url {
            new_config.base_url = Some(expand_env_vars(url));
        }
        new_config.thinking = match provider_enum {
            LlmProvider::Moonshot | LlmProvider::Custom => Some(saved.thinking.unwrap_or(false)),
            _ => None,
        };
        let mut config_guard = state.chat.llm_config.lock().await;
        *config_guard = Some(new_config);
        *state.chat.active_provider_entry_name.lock().await = Some(name.clone());
    }

    // Save to config file - use standard location if not found
    let config_path = crate::llm::config::find_config_file()
        .or_else(|| {
            dirs::config_dir().map(|d| d.join("omiga").join("omiga.yaml"))
        })
        .ok_or_else(|| OmigaError::Config("Could not determine config file path".to_string()))?;

    // Ensure directory exists
    if let Some(parent) = config_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    crate::llm::config::save_config_file(&config_file, &config_path)
        .map_err(|e| OmigaError::Config(format!("Failed to save config: {}", e)))?;

    Ok(())
}

/// Delete a provider configuration
#[tauri::command]
pub async fn delete_provider_config(
    state: State<'_, OmigaAppState>,
    name: String,
) -> CommandResult<()> {
    // Load existing config
    let mut config_file = crate::llm::config::load_config_file()
        .map_err(|e| OmigaError::Config(format!("Failed to load config: {}", e)))?;

    let providers = config_file
        .providers
        .as_mut()
        .ok_or_else(|| OmigaError::Config("No providers configured".to_string()))?;

    if providers.remove(&name).is_none() {
        return Err(OmigaError::Config(format!(
            "Provider '{}' not found",
            name
        )));
    }

    // If we removed the default, pick a new one
    if config_file.default_provider.as_ref() == Some(&name) {
        config_file.default_provider = providers.keys().next().cloned();
    }

    // Clear "in use" tracking if we deleted that row (or follow the new default key).
    {
        let mut active = state.chat.active_provider_entry_name.lock().await;
        if active.as_deref() == Some(name.as_str()) {
            *active = config_file.default_provider.clone();
        }
    }

    // Save back to file
    if let Some(path) = crate::llm::config::find_config_file() {
        crate::llm::config::save_config_file(&config_file, &path)
            .map_err(|e| OmigaError::Config(format!("Failed to save config: {}", e)))?;
    }

    Ok(())
}

/// Apply a named `omiga.yaml` provider entry to in-memory chat state (no file write).
pub(crate) async fn apply_named_provider_runtime(
    state: &OmigaAppState,
    provider_name: &str,
) -> Result<LlmConfigResponse, OmigaError> {
    let name = provider_name.trim();
    if name.is_empty() {
        return Err(OmigaError::Config("Provider name is required".to_string()));
    }

    let config_file = crate::llm::config::load_config_file()
        .map_err(|e| OmigaError::Config(format!("Failed to load config file: {}", e)))?;

    let providers = config_file
        .providers
        .ok_or_else(|| OmigaError::Config("No providers configured".to_string()))?;

    let provider_config = providers
        .get(name)
        .ok_or_else(|| OmigaError::Config(format!("Provider '{}' not found", name)))?;

    if !provider_config.enabled {
        return Err(OmigaError::Config(format!(
            "Provider '{}' is disabled",
            name
        )));
    }

    let provider_enum = provider_config
        .provider_type
        .parse::<LlmProvider>()
        .map_err(|e| OmigaError::Config(format!("Invalid provider type: {}", e)))?;

    let api_key = provider_config
        .api_key
        .as_ref()
        .map(|k| expand_env_vars(k))
        .filter(|k| !k.is_empty())
        .ok_or_else(|| OmigaError::Config(format!("API key not set for provider '{}'", name)))?;

    let mut config = LlmConfig::new(provider_enum, api_key);

    if let Some(model) = &provider_config.model {
        config.model = expand_env_vars(model);
    }
    if let Some(url) = &provider_config.base_url {
        config.base_url = Some(expand_env_vars(url));
    }
    match provider_enum {
        LlmProvider::Moonshot | LlmProvider::Custom => {
            config.thinking = Some(provider_config.thinking.unwrap_or(false));
        }
        _ => {
            config.thinking = None;
        }
    }

    let mut config_guard = state.chat.llm_config.lock().await;
    *config_guard = Some(config.clone());
    drop(config_guard);
    *state.chat.active_provider_entry_name.lock().await = Some(name.to_string());

    Ok(LlmConfigResponse {
        provider: format!("{:?}", provider_enum),
        api_key_preview: if config.api_key.len() > 8 {
            format!("{}...", &config.api_key[..8])
        } else {
            config.api_key.clone()
        },
        model: Some(config.model),
        base_url: config.base_url,
        thinking: config.thinking,
    })
}

/// Same as app startup: `omiga.yaml` merged default (and env), plus `default_provider` key for UI.
pub(crate) async fn apply_yaml_default_runtime(state: &OmigaAppState) -> Result<(), OmigaError> {
    let cfg = crate::llm::load_config().map_err(OmigaError::from)?;
    *state.chat.llm_config.lock().await = Some(cfg);
    let cf = crate::llm::config::load_config_file()
        .map_err(|e| OmigaError::Config(format!("Failed to load config file: {}", e)))?;
    *state.chat.active_provider_entry_name.lock().await = cf.default_provider.clone();
    Ok(())
}

/// When switching sessions, restore that session's quick-switched provider or yaml default.
/// Returns `true` when the active provider was actually changed (UI needs refresh),
/// `false` when the desired provider was already active (no-op, skip UI event).
pub(crate) async fn restore_session_llm_after_load(
    state: &OmigaAppState,
    active_provider_entry_name: Option<String>,
) -> Result<bool, OmigaError> {
    // Skip re-initializing the provider when it hasn't changed — avoids redundant disk I/O on
    // every session switch when the user stays on the same model.
    let current_name = state.chat.active_provider_entry_name.lock().await.clone();
    let desired_name = active_provider_entry_name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    if current_name == desired_name {
        return Ok(false); // unchanged — caller can skip notifyProviderChanged
    }
    drop(current_name);

    match active_provider_entry_name {
        Some(name) if !name.trim().is_empty() => {
            if let Err(e) = apply_named_provider_runtime(state, name.trim()).await {
                tracing::warn!(
                    target: "omiga::llm",
                    "Session provider {:?} unavailable ({}), falling back to yaml default",
                    name,
                    e
                );
                apply_yaml_default_runtime(state).await?;
            }
        }
        _ => apply_yaml_default_runtime(state).await?,
    }
    Ok(true) // provider changed — caller should fire notifyProviderChanged
}

/// Quick switch provider - set active without saving to file (for UI quick-switch).
/// Persists the choice on the given `session_id` when provided (per-session model).
#[tauri::command]
pub async fn quick_switch_provider(
    state: State<'_, OmigaAppState>,
    provider_name: String,
    session_id: Option<String>,
) -> CommandResult<LlmConfigResponse> {
    let name = provider_name.trim();
    if name.is_empty() {
        return Err(OmigaError::Config("Provider name is required".to_string()));
    }

    let resp = apply_named_provider_runtime(&state, name).await?;

    if let Some(sid) = session_id {
        let sid = sid.trim();
        if !sid.is_empty() {
            let repo = &*state.repo;
            repo.set_session_active_provider(sid, Some(name))
                .await
                .map_err(|e| {
                    OmigaError::Persistence(format!("Failed to save session provider: {}", e))
                })?;
        }
    }

    Ok(resp)
}

/// Set `default_provider` in `omiga.yaml` only — which model starts as default on next launch.
/// Does **not** change [`OmigaAppState::llm_config`] or [`active_provider_entry_name`]
/// (use `quick_switch_provider` for the current session).
#[tauri::command]
pub async fn set_default_provider_config(
    _state: State<'_, OmigaAppState>,
    provider_name: String,
) -> CommandResult<()> {
    let name = provider_name.trim();
    if name.is_empty() {
        return Err(OmigaError::Config("Provider name is required".to_string()));
    }

    let mut config_file = crate::llm::config::load_config_file()
        .map_err(|e| OmigaError::Config(format!("Failed to load config file: {}", e)))?;

    let providers = config_file
        .providers
        .as_mut()
        .ok_or_else(|| OmigaError::Config("No providers configured".to_string()))?;

    let entry = providers
        .get(name)
        .ok_or_else(|| OmigaError::Config(format!("Provider '{}' not found", name)))?;

    if !entry.enabled {
        return Err(OmigaError::Config(format!(
            "Provider '{}' is disabled",
            name
        )));
    }

    config_file.default_provider = Some(name.to_string());

    let config_path = crate::llm::config::find_config_file()
        .or_else(|| dirs::config_dir().map(|d| d.join("omiga").join("omiga.yaml")))
        .ok_or_else(|| OmigaError::Config("Could not determine config file path".to_string()))?;

    if let Some(parent) = config_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    crate::llm::config::save_config_file(&config_file, &config_path)
        .map_err(|e| OmigaError::Config(format!("Failed to save config: {}", e)))?;

    Ok(())
}

// ─── Agent Scheduler ────────────────────────────────────────────────────────

/// Request body for `run_agent_schedule`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunAgentScheduleRequest {
    /// Free-form user request used for task decomposition.
    pub user_request: String,
    /// Working directory (project root) for all subtasks.
    pub project_root: String,
    /// Session id for tool-results dir and background-task attribution.
    pub session_id: String,
    /// Max agents to spawn in parallel (default 5).
    #[serde(default)]
    pub max_agents: Option<usize>,
    /// Whether to let the planner decompose the request (default true).
    #[serde(default = "default_auto_decompose")]
    pub auto_decompose: bool,
}

fn default_auto_decompose() -> bool { true }

/// Run the agent scheduler: decompose `user_request`, then execute subtasks in
/// parallel using real LLM sub-agents backed by `spawn_background_agent`.
///
/// The caller must have an active LLM session (API key configured). Returns the
/// full `OrchestrationResult` when all subtasks finish.
#[tauri::command]
pub async fn run_agent_schedule(
    app: tauri::AppHandle,
    app_state: State<'_, OmigaAppState>,
    request: RunAgentScheduleRequest,
) -> CommandResult<crate::domain::agents::scheduler::OrchestrationResult> {
    use crate::domain::agents::scheduler::{AgentScheduler, SchedulingRequest};
    use crate::errors::{ChatError, OmigaError};

    // Build LLM config from app state.
    let llm_config = {
        let guard = app_state.chat.llm_config.lock().await;
        guard.clone().ok_or_else(|| OmigaError::Chat(ChatError::ApiKeyMissing))?
    };
    if llm_config.api_key.is_empty() {
        return Err(OmigaError::Chat(ChatError::ApiKeyMissing));
    }

    // Build a minimal AgentLlmRuntime for the scheduler.
    let runtime = AgentLlmRuntime {
        llm_config,
        round_id: uuid::Uuid::new_v4().to_string(),
        cancel_flag: std::sync::Arc::new(tokio::sync::RwLock::new(false)),
        pending_tools: app_state.chat.pending_tools.clone(),
        repo: app_state.repo.clone(),
        plan_mode_flag: None,
        allow_nested_agent: false,
    };

    let max_agents = request.max_agents.unwrap_or(5);
    let sched_req = SchedulingRequest::new(request.user_request.clone())
        .with_project_root(request.project_root.clone())
        .with_parallel(true)
        .with_max_agents(max_agents)
        .with_auto_decompose(request.auto_decompose);

    let scheduler = AgentScheduler::new();

    // Step 1: build the execution plan.
    let sched_result = scheduler
        .schedule(sched_req)
        .await
        .map_err(|e| OmigaError::Chat(ChatError::StreamError(e)))?;

    tracing::info!(
        target: "omiga::scheduler",
        plan_id = %sched_result.plan.plan_id,
        subtasks = sched_result.plan.subtasks.len(),
        agents = ?sched_result.selected_agents,
        "Agent schedule plan built"
    );

    // Step 2: execute with real agents.
    let orch_result = scheduler
        .execute_plan_with_runtime(
            &sched_result.plan,
            &SchedulingRequest::new(request.user_request)
                .with_project_root(request.project_root),
            &app,
            &runtime,
            &request.session_id,
        )
        .await
        .map_err(|e| OmigaError::Chat(ChatError::StreamError(e)))?;

    Ok(orch_result)
}

/// Handle the `skill_config` tool: get / set / list skill configuration variables.
async fn handle_skill_config(
    project_root: &std::path::Path,
    arguments: &str,
    skill_cache: &std::sync::Arc<std::sync::Mutex<skills::SkillCacheMap>>,
) -> Result<serde_json::Value, String> {
    use crate::domain::tools::skill_config::{ConfigAction, SkillConfigArgs};

    let args: SkillConfigArgs = serde_json::from_str(arguments)
        .map_err(|e| format!("skill_config: invalid JSON: {e}"))?;

    match args.action {
        ConfigAction::List => {
            let all_skills = skills::load_skills_cached(project_root, skill_cache).await;
            let mut entries = Vec::new();
            for skill in &all_skills {
                if skill.config_vars.is_empty() {
                    continue;
                }
                let resolved = skills::skill_config::resolve_config_vars(&skill.config_vars, project_root);
                entries.push(serde_json::json!({
                    "skill": skill.name,
                    "config_vars": resolved,
                }));
            }
            Ok(serde_json::json!({ "success": true, "skills": entries, "count": entries.len() }))
        }
        ConfigAction::Get => {
            let skill_name = args.skill
                .as_deref()
                .filter(|s| !s.trim().is_empty())
                .ok_or_else(|| "skill_config: `skill` is required for action `get`".to_string())?;
            let all_skills = skills::load_skills_cached(project_root, skill_cache).await;
            let entry = skills::find_skill_entry(&all_skills, skill_name)
                .ok_or_else(|| format!("skill_config: unknown skill `{skill_name}`"))?;
            if entry.config_vars.is_empty() {
                return Ok(serde_json::json!({
                    "success": true,
                    "skill": entry.name,
                    "config_vars": [],
                    "message": "This skill declares no config variables."
                }));
            }
            let resolved = skills::skill_config::resolve_config_vars(&entry.config_vars, project_root);
            Ok(serde_json::json!({
                "success": true,
                "skill": entry.name,
                "config_vars": resolved,
                "config_file": skills::project_config_path(project_root),
            }))
        }
        ConfigAction::Set => {
            let skill_name = args.skill
                .as_deref()
                .filter(|s| !s.trim().is_empty())
                .ok_or_else(|| "skill_config: `skill` is required for action `set`".to_string())?;
            let key = args.key
                .as_deref()
                .filter(|s| !s.trim().is_empty())
                .ok_or_else(|| "skill_config: `key` is required for action `set`".to_string())?;
            let value = args.value
                .as_deref()
                .ok_or_else(|| "skill_config: `value` is required for action `set`".to_string())?;

            // Validate the key is declared by the skill.
            let all_skills = skills::load_skills_cached(project_root, skill_cache).await;
            let entry = skills::find_skill_entry(&all_skills, skill_name)
                .ok_or_else(|| format!("skill_config: unknown skill `{skill_name}`"))?;
            if !entry.config_vars.is_empty() && !entry.config_vars.iter().any(|v| v.key == key) {
                return Err(format!(
                    "skill_config: key `{key}` is not declared by skill `{skill_name}`. \
                     Declared keys: {}",
                    entry.config_vars.iter().map(|v| v.key.as_str()).collect::<Vec<_>>().join(", ")
                ));
            }

            skills::set_config_var(project_root, key, value).await?;
            Ok(serde_json::json!({
                "success": true,
                "skill": skill_name,
                "key": key,
                "value": value,
                "config_file": skills::project_config_path(project_root),
            }))
        }
    }
}

/// Helper to expand environment variables
fn expand_env_vars(s: &str) -> String {
    let mut result = s.to_string();

    // Expand ${VAR}
    while let Some(start) = result.find("${") {
        if let Some(end) = result[start..].find("}") {
            let var_name = &result[start + 2..start + end];
            let var_value = std::env::var(var_name).unwrap_or_default();
            result.replace_range(start..start + end + 1, &var_value);
        } else {
            break;
        }
    }

    // Expand $VAR
    let re = Regex::new(r"\$([A-Za-z_][A-Za-z0-9_]*)").unwrap();
    result = re
        .replace_all(&result, |caps: &regex::Captures| {
            std::env::var(&caps[1]).unwrap_or_else(|_| caps[0].to_string())
        })
        .to_string();

    result
}
