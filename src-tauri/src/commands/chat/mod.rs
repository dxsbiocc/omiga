//! Chat commands - Send messages and stream responses with tool execution
//!
//! Multi-provider support: Anthropic, OpenAI, Azure, Google, and custom endpoints

use super::CommandResult;
use crate::api::{ContentBlock, Role};
use crate::app_state::{IntegrationsConfigCacheSlot, OmigaAppState, INTEGRATIONS_CONFIG_CACHE_TTL};
use crate::constants::agent_prompt;
use crate::constants::tool_limits::{
    large_output_persist_failed_message, large_tool_output_files_enabled, truncate_utf8_prefix,
    DEFAULT_MAX_RESULT_SIZE_CHARS,
};
use crate::domain::agents::coordinator;
use crate::domain::agents::scheduler::{AgentScheduler, SchedulingRequest, SchedulingStrategy};
use crate::domain::agents::subagent_tool_filter::env_allow_nested_agent;
use crate::domain::agents::ChatInputTarget;
use crate::domain::chat_state::{
    AskUserWaiter, McpToolCache, PermissionDenyCache, MCP_TOOL_CACHE_TTL, PERMISSION_DENY_CACHE_TTL,
};
use crate::domain::integrations_config;
use crate::domain::permissions::{
    filter_tool_schemas_by_deny_rule_entries, load_merged_permission_deny_rule_entries,
    validate_permission_deny_entries,
};
use crate::domain::persistence::{NewMessageRecord, NewOrchestrationEventRecord};
use crate::domain::runtime_constraints::{
    ModelConstraintContext, RuntimeConstraintHarness, RuntimeConstraintState, ToolConstraintContext,
};
use crate::domain::session::SessionCodec;
use crate::domain::session::{Session, ToolCall};
use crate::domain::skills;
use crate::domain::tools::{
    all_tool_schemas, normalize_legacy_retrieval_tool_arguments,
    normalize_legacy_retrieval_tool_name, sort_tool_schemas_for_model, ToolContext, ToolSchema,
};
use crate::errors::{ChatError, OmigaError};
use crate::infrastructure::streaming::StreamOutputItem;
use crate::llm::{
    create_client, load_config_from_env, LlmClient, LlmConfig, LlmContent, LlmMessage, LlmRole,
};
use crate::utils::large_output_instructions::get_large_output_instructions;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::{Mutex, RwLock};

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

/// Max assistant↔tool iterations per user send (safety valve; raised to support
/// longer evidence-first investigation and multi-step execution in the main agent).
const MAX_TOOL_ROUNDS: usize = 100;

/// Max tool rounds inside one `Agent` sub-session (nested Agent calls are blocked separately).
const MAX_SUBAGENT_TOOL_ROUNDS: usize = 50;

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
    /// Same token as [`RoundCancellationState::round_cancel`] for main chat; stops foreground/background bash on cancel.
    round_cancel: tokio_util::sync::CancellationToken,
    /// `local` | `ssh` | `sandbox` — from [`SessionRuntimeState::execution_environment`].
    execution_environment: String,
    /// Selected SSH server name; from [`SessionRuntimeState::ssh_server`].
    ssh_server: Option<String>,
    /// `modal` | `daytona` | `docker` | `singularity` — from [`SessionRuntimeState::sandbox_backend`].
    sandbox_backend: String,
    /// Local virtual env type: `"none"` | `"conda"` | `"venv"` | `"pyenv"`.
    local_venv_type: String,
    /// Conda env name, venv directory path, or pyenv version string.
    local_venv_name: String,
    /// Session-scoped environment cache — shared across all tool calls in this round.
    env_store: crate::domain::tools::env_store::EnvStore,
    /// Resolved runtime constraint configuration (project + session overrides).
    runtime_constraints_config: crate::domain::runtime_constraints::ResolvedRuntimeConstraintConfig,
}

impl AgentLlmRuntime {
    pub(crate) fn round_id(&self) -> &str {
        &self.round_id
    }

    pub(crate) fn repo(&self) -> &Arc<crate::domain::persistence::SessionRepository> {
        &self.repo
    }

    /// Build a runtime from app state, optionally inheriting execution environment from a parent
    /// session. If `session_id` is `Some`, the session's `execution_environment`, `ssh_server`,
    /// `sandbox_backend`, `local_venv_*`, and `env_store` are copied; otherwise defaults apply.
    pub(crate) async fn from_app(
        app: &tauri::AppHandle,
        session_id: Option<&str>,
    ) -> Result<Self, String> {
        use crate::app_state::OmigaAppState;
        use tauri::Manager;
        let state = app
            .try_state::<OmigaAppState>()
            .ok_or("OmigaAppState not available")?;
        let llm_config = {
            let guard = state.chat.llm_config.lock().await;
            guard
                .clone()
                .ok_or("LLM not configured — set an API key first")?
        };
        if llm_config.api_key.is_empty() {
            return Err("API key is empty".to_string());
        }

        let (
            execution_environment,
            ssh_server,
            sandbox_backend,
            local_venv_type,
            local_venv_name,
            env_store,
        ) = {
            let sessions = state.chat.sessions.read().await;
            let s = session_id.and_then(|id| sessions.get(id));
            (
                s.map(|x| x.execution_environment.clone())
                    .unwrap_or_else(|| "local".to_string()),
                s.and_then(|x| x.ssh_server.clone()),
                s.map(|x| x.sandbox_backend.clone())
                    .unwrap_or_else(|| "docker".to_string()),
                s.map(|x| x.local_venv_type.clone()).unwrap_or_default(),
                s.map(|x| x.local_venv_name.clone()).unwrap_or_default(),
                s.map(|x| x.env_store.clone()).unwrap_or_default(),
            )
        };

        Ok(Self {
            llm_config,
            round_id: uuid::Uuid::new_v4().to_string(),
            cancel_flag: Arc::new(RwLock::new(false)),
            pending_tools: state.chat.pending_tools.clone(),
            repo: state.repo.clone(),
            plan_mode_flag: None,
            allow_nested_agent: false,
            round_cancel: tokio_util::sync::CancellationToken::new(),
            execution_environment,
            ssh_server,
            sandbox_backend,
            local_venv_type,
            local_venv_name,
            env_store,
            runtime_constraints_config:
                crate::domain::runtime_constraints::ResolvedRuntimeConstraintConfig::default(),
        })
    }

    /// Load runtime constraint config from the project's omiga.yaml and apply it.
    /// Call this after `from_app()` when the project root and session ID are known.
    pub(crate) fn with_runtime_context(
        mut self,
        project_root: &std::path::Path,
        session_id: &str,
    ) -> Self {
        let session_cfg = crate::domain::session::load_session_config(session_id);
        self.runtime_constraints_config =
            crate::domain::runtime_constraints::resolve_runtime_constraint_config(
                project_root,
                session_cfg.runtime_constraints.as_ref(),
            );
        self
    }
}

struct ActiveRoundCleanup {
    active_rounds: Arc<Mutex<HashMap<String, RoundCancellationState>>>,
    message_id: String,
}

impl ActiveRoundCleanup {
    fn new(
        active_rounds: Arc<Mutex<HashMap<String, RoundCancellationState>>>,
        message_id: String,
    ) -> Self {
        Self {
            active_rounds,
            message_id,
        }
    }
}

impl Drop for ActiveRoundCleanup {
    fn drop(&mut self) {
        let active_rounds = self.active_rounds.clone();
        let message_id = self.message_id.clone();
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                let mut active_rounds = active_rounds.lock().await;
                active_rounds.remove(&message_id);
            });
        }
    }
}

pub use crate::domain::chat_state::{
    ChatState, PendingToolCall, RoundCancellationState, SessionRuntimeState,
};

/// Get or create LLM config from environment or state
pub(super) async fn get_llm_config(chat_state: &ChatState) -> Result<LlmConfig, OmigaError> {
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

pub(crate) fn tool_results_dir_for_session(
    app: &AppHandle,
    session_id: &str,
) -> std::path::PathBuf {
    app.path()
        .app_data_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
        .join("tool-results")
        .join(session_id)
}

/// Resolve session `project_path` to an absolute-ish root for tools (glob, bash, file_read).
pub(super) fn resolve_session_project_root(project_path: &str) -> std::path::PathBuf {
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
                    ContentBlock::ToolUse { id, name, input } => {
                        let (name, arguments) = normalize_llm_tool_history_for_model(name, input);
                        LlmContent::ToolUse {
                            id: id.clone(),
                            name,
                            arguments,
                        }
                    }
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

fn augment_llm_messages_with_runtime_constraints(
    base_messages: &[LlmMessage],
    harness: &RuntimeConstraintHarness,
    state: &mut RuntimeConstraintState,
    request_text: &str,
    project_root: &Path,
    use_tools: bool,
    is_subagent: bool,
) -> (Vec<LlmMessage>, Vec<String>) {
    let before = state
        .emitted_notice_ids()
        .into_iter()
        .map(str::to_string)
        .collect::<std::collections::HashSet<_>>();
    let messages = harness.augment_model_messages(
        base_messages,
        &ModelConstraintContext {
            request_text,
            project_root,
            use_tools,
            is_subagent,
        },
        state,
    );
    let newly_emitted = state
        .emitted_notice_ids()
        .into_iter()
        .map(str::to_string)
        .filter(|id| !before.contains(id))
        .collect();
    (messages, newly_emitted)
}

fn emit_buffered_assistant_text(app: &AppHandle, message_id: &str, text: &str) {
    if text.is_empty() {
        return;
    }
    let _ = app.emit(
        &format!("chat-stream-{}", message_id),
        &StreamOutputItem::Text(text.to_string()),
    );
}

async fn emit_runtime_constraint_metadata(
    app: &AppHandle,
    repo: &Arc<crate::domain::persistence::SessionRepository>,
    session_id: &str,
    round_id: &str,
    message_id: &str,
    key: &str,
    payload: serde_json::Value,
) {
    let payload_string = payload.to_string();
    let _ = app.emit(
        &format!("chat-stream-{}", message_id),
        &StreamOutputItem::Metadata {
            key: key.to_string(),
            value: payload_string.clone(),
        },
    );
    let constraint_id = payload
        .get("id")
        .and_then(|v| v.as_str())
        .or_else(|| payload.get("constraint_id").and_then(|v| v.as_str()));
    if let Err(e) = repo
        .append_runtime_constraint_event(
            session_id,
            round_id,
            message_id,
            key,
            constraint_id,
            &payload_string,
        )
        .await
    {
        tracing::warn!("Failed to persist runtime constraint metadata: {}", e);
    }
}

struct RuntimeConstraintBlockRequest<'a> {
    app: &'a AppHandle,
    client: &'a dyn LlmClient,
    repo: Arc<crate::domain::persistence::SessionRepository>,
    sessions: &'a Arc<RwLock<HashMap<String, SessionRuntimeState>>>,
    session_id: &'a str,
    round_id: &'a str,
    message_id: &'a str,
    user_message: &'a str,
    assistant_text: &'a str,
    assistant_reasoning: &'a str,
    tool_calls: &'a [(String, String, String)],
    block: &'a crate::domain::runtime_constraints::ConstraintToolBlock,
    tool_results_dir: &'a Path,
    ask_user_waiters: Arc<Mutex<HashMap<String, AskUserWaiter>>>,
    cancel_flag: Arc<RwLock<bool>>,
    preflight_skip_turn_summary: bool,
    turn_token_usage: &'a Option<crate::llm::TokenUsage>,
    provider_name: &'a str,
    persist_original_assistant: bool,
}

async fn handle_runtime_constraint_block_main(request: RuntimeConstraintBlockRequest<'_>) {
    let RuntimeConstraintBlockRequest {
        app,
        client,
        repo,
        sessions,
        session_id,
        round_id,
        message_id,
        user_message,
        assistant_text,
        assistant_reasoning,
        tool_calls,
        block,
        tool_results_dir,
        ask_user_waiters,
        cancel_flag,
        preflight_skip_turn_summary,
        turn_token_usage,
        provider_name,
        persist_original_assistant,
    } = request;
    let assistant_msg_id = uuid::Uuid::new_v4().to_string();
    let tool_calls_json = tool_calls_json_opt(tool_calls);
    let reasoning_save = (!assistant_reasoning.is_empty()).then_some(assistant_reasoning);
    if persist_original_assistant {
        if let Err(e) = repo
            .save_message(NewMessageRecord {
                id: &assistant_msg_id,
                session_id,
                role: "assistant",
                content: assistant_text,
                tool_calls: tool_calls_json.as_deref(),
                tool_call_id: None,
                token_usage_json: None,
                reasoning_content: reasoning_save,
                follow_up_suggestions_json: None,
                turn_summary: None,
            })
            .await
        {
            tracing::warn!(
                "Failed to save assistant message before runtime constraint block: {}",
                e
            );
        }

        {
            let mut sessions_guard = sessions.write().await;
            if let Some(runtime) = sessions_guard.get_mut(session_id) {
                let tc = completed_to_tool_calls(tool_calls);
                let rc = (!assistant_reasoning.is_empty()).then(|| assistant_reasoning.to_string());
                runtime
                    .session
                    .add_assistant_message_with_tools(assistant_text, tc, rc);
            }
        }

        let blocked_batch: Vec<(String, String, Option<String>)> = tool_calls
            .iter()
            .map(|(id, _name, _arguments)| {
                (id.clone(), block.tool_result_message.to_string(), None)
            })
            .collect();
        if let Err(e) = repo
            .save_tool_results_batch(session_id, &blocked_batch)
            .await
        {
            tracing::warn!(
                "Failed to save runtime-constraint blocked tool results batch: {}",
                e
            );
        }

        {
            let mut sessions_guard = sessions.write().await;
            if let Some(runtime) = sessions_guard.get_mut(session_id) {
                for (tool_use_id, tool_name, arguments) in tool_calls {
                    runtime.session.add_tool_result(
                        tool_use_id.clone(),
                        block.tool_result_message.to_string(),
                    );
                    let _ = app.emit(
                        &format!("chat-stream-{}", message_id),
                        &StreamOutputItem::ToolResult {
                            tool_use_id: tool_use_id.clone(),
                            name: tool_name.clone(),
                            input: arguments.clone(),
                            output: block.tool_result_message.to_string(),
                            is_error: true,
                        },
                    );
                }
            }
        }
    }

    let mut last_assistant_id = assistant_msg_id;
    let mut final_response = block.assistant_response.clone();

    if let Some(ref question_args) = block.interactive_question {
        let tool_use_id = format!("constraint-ask-user-{}", uuid::Uuid::new_v4());
        let tool_name = "ask_user_question".to_string();
        let ask_arguments = match serde_json::to_string(question_args) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("Failed to serialize runtime clarification question: {}", e);
                String::new()
            }
        };

        if !ask_arguments.is_empty() {
            let ask_assistant_id = uuid::Uuid::new_v4().to_string();
            let ask_tool_call = ToolCall {
                id: tool_use_id.clone(),
                name: tool_name.clone(),
                arguments: ask_arguments.clone(),
            };
            let ask_tool_calls = vec![ask_tool_call.clone()];

            let _ = app.emit(
                &format!("chat-stream-{}", message_id),
                &StreamOutputItem::Text(format!("\n\n{}", block.assistant_response)),
            );

            let ask_tool_calls_json = serde_json::to_string(&ask_tool_calls).ok();
            if let Err(e) = repo
                .save_message(NewMessageRecord {
                    id: &ask_assistant_id,
                    session_id,
                    role: "assistant",
                    content: &block.assistant_response,
                    tool_calls: ask_tool_calls_json.as_deref(),
                    tool_call_id: None,
                    token_usage_json: None,
                    reasoning_content: None,
                    follow_up_suggestions_json: None,
                    turn_summary: None,
                })
                .await
            {
                tracing::warn!(
                    "Failed to save runtime clarification assistant message: {}",
                    e
                );
            }
            {
                let mut sessions_guard = sessions.write().await;
                if let Some(runtime) = sessions_guard.get_mut(session_id) {
                    runtime.session.add_assistant_message_with_tools(
                        block.assistant_response.clone(),
                        Some(ask_tool_calls),
                        None,
                    );
                }
            }

            let (returned_tool_id, output, is_error) =
                execute_ask_user_question_interactive(AskUserQuestionExecution {
                    tool_use_id: tool_use_id.clone(),
                    tool_name,
                    arguments: ask_arguments,
                    app: app.clone(),
                    message_id: message_id.to_string(),
                    session_id: session_id.to_string(),
                    tool_results_dir,
                    waiters: ask_user_waiters,
                    cancel_flag: Some(cancel_flag),
                })
                .await;

            if let Err(e) = repo
                .save_tool_results_batch(
                    session_id,
                    &[(returned_tool_id.clone(), output.clone(), None)],
                )
                .await
            {
                tracing::warn!("Failed to save runtime clarification tool result: {}", e);
            }
            {
                let mut sessions_guard = sessions.write().await;
                if let Some(runtime) = sessions_guard.get_mut(session_id) {
                    runtime.session.add_tool_result(returned_tool_id, output);
                }
            }

            last_assistant_id = ask_assistant_id;

            if !is_error {
                if let Some(ref post_answer_response) = block.post_answer_response {
                    final_response = post_answer_response.clone();
                    let follow_up_id = uuid::Uuid::new_v4().to_string();
                    if let Err(e) = repo
                        .save_message(NewMessageRecord {
                            id: &follow_up_id,
                            session_id,
                            role: "assistant",
                            content: post_answer_response,
                            tool_calls: None,
                            tool_call_id: None,
                            token_usage_json: None,
                            reasoning_content: None,
                            follow_up_suggestions_json: None,
                            turn_summary: None,
                        })
                        .await
                    {
                        tracing::warn!(
                            "Failed to save runtime clarification follow-up message: {}",
                            e
                        );
                    }
                    {
                        let mut sessions_guard = sessions.write().await;
                        if let Some(runtime) = sessions_guard.get_mut(session_id) {
                            runtime
                                .session
                                .add_assistant_message(post_answer_response.clone());
                        }
                    }
                    let _ = app.emit(
                        &format!("chat-stream-{}", message_id),
                        &StreamOutputItem::Text(format!("\n\n{}", post_answer_response)),
                    );
                    last_assistant_id = follow_up_id;
                }
            }
        }
    } else {
        let _ = app.emit(
            &format!("chat-stream-{}", message_id),
            &StreamOutputItem::Text(format!("\n\n{}", block.assistant_response)),
        );
        let clarification_id = uuid::Uuid::new_v4().to_string();
        if let Err(e) = repo
            .save_message(NewMessageRecord {
                id: &clarification_id,
                session_id,
                role: "assistant",
                content: &block.assistant_response,
                tool_calls: None,
                tool_call_id: None,
                token_usage_json: None,
                reasoning_content: None,
                follow_up_suggestions_json: None,
                turn_summary: None,
            })
            .await
        {
            tracing::warn!(
                "Failed to save runtime-constraint clarification message: {}",
                e
            );
        }
        {
            let mut sessions_guard = sessions.write().await;
            if let Some(runtime) = sessions_guard.get_mut(session_id) {
                runtime
                    .session
                    .add_assistant_message(block.assistant_response.clone());
            }
        }
        last_assistant_id = clarification_id;
    }

    persist_session_tool_state(sessions, &repo, session_id).await;

    if let Err(e) = repo
        .complete_round(round_id, Some(&last_assistant_id))
        .await
    {
        tracing::warn!(
            "Failed to complete round after runtime constraint block: {}",
            e
        );
    }

    persist_and_emit_turn_token_usage(
        app,
        &repo,
        &last_assistant_id,
        message_id,
        turn_token_usage,
        provider_name,
    )
    .await;
    spawn_memory_sync(MemorySyncRequest {
        app,
        sessions,
        repo: &repo,
        session_id,
        client,
        user_message,
        assistant_reply: &final_response,
        allow_long_term_promotion: false,
    });
    emit_post_turn_meta_then_complete(PostTurnCompletionRequest {
        app,
        session_id,
        stream_message_id: message_id,
        assistant_message_id: &last_assistant_id,
        client,
        final_reply: &final_response,
        skip_summary: preflight_skip_turn_summary,
        suggestions_reply: &final_response,
        repo: repo.clone(),
    })
    .await;
    spawn_chat_indexing(app, sessions, &repo, session_id);
}

pub(super) struct PostResponseRetryRequest<'a> {
    pub client: &'a dyn LlmClient,
    pub app: &'a AppHandle,
    pub message_id: &'a str,
    pub round_id: &'a str,
    pub base_messages: &'a [LlmMessage],
    pub instruction: &'a str,
    pub pending_tools: &'a Arc<Mutex<HashMap<String, PendingToolCall>>>,
    pub cancel_flag: &'a Arc<RwLock<bool>>,
    pub repo: Arc<crate::domain::persistence::SessionRepository>,
}

async fn run_post_response_retry_text_only(
    request: PostResponseRetryRequest<'_>,
) -> Result<(String, String, Option<crate::llm::TokenUsage>), OmigaError> {
    let PostResponseRetryRequest {
        client,
        app,
        message_id,
        round_id,
        base_messages,
        instruction,
        pending_tools,
        cancel_flag,
        repo,
    } = request;
    let mut retry_messages = base_messages.to_vec();
    retry_messages.push(LlmMessage::system(format!(
        "## Runtime validator correction\n{}",
        instruction
    )));
    let (tool_calls, text, reasoning, cancelled, usage) =
        stream_llm_response_with_cancel(StreamLlmRequest {
            client,
            app,
            message_id,
            round_id,
            messages: &retry_messages,
            tools: &[],
            emit_text_chunks: false,
            pending_tools,
            cancel_flag,
            repo,
        })
        .await?;

    if cancelled {
        return Ok((String::new(), String::new(), usage));
    }
    if !tool_calls.is_empty() {
        tracing::warn!(
            "Runtime post-response retry unexpectedly produced tool calls; ignoring them."
        );
    }
    Ok((text, reasoning, usage))
}

/// Large tool output: spill to disk + inject instructions, or truncate (TS parity).
async fn process_tool_output_for_model(raw: String, tool_use_id: &str, dir: &Path) -> String {
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
        Ok(()) => {
            get_large_output_instructions(path.to_string_lossy().as_ref(), size, "Plain text", None)
        }
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
        | StreamOutputItem::SuggestionsGenerating
        | StreamOutputItem::SuggestionsComplete { .. }
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
    output.push_str("(Results are truncated. Consider using a more specific path or pattern.)");
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

mod orchestration;
use orchestration::*;
macro_rules! mode_lifecycle_context {
    ($is_active:expr, $sessions:expr, $repo:expr, $project_root:expr, $session_id:expr, $env_label:expr, $round_id:expr $(,)?) => {
        ModeLifecycleContext {
            is_active: $is_active,
            sessions: $sessions,
            repo: $repo,
            project_root: $project_root,
            session_id: $session_id,
            env_label: $env_label,
            round_id: $round_id,
        }
    };
}
mod turn;
use turn::*;
mod commands;
pub use commands::*;
mod permissions;
use self::permissions::{execute_ask_user_question_interactive, AskUserQuestionExecution};
mod subagent;
pub(crate) use self::subagent::{spawn_background_agent, BackgroundAgentRequest};
mod tool_exec;
use self::tool_exec::{execute_tool_calls, ToolExecutionRequest};
pub mod research;
pub mod research_goal;
pub use self::research::{
    AgentRoleInfoDto, AvailableAgentInfo, ResearchCommandRequest, ResearchCommandResponse,
};
pub use self::research_goal::{ResearchGoalCommandRequest, ResearchGoalCommandResponse};

/// Normalize composer `sandboxBackend` from the UI (`modal` | `daytona` | `docker` | `singularity`).
/// Note: `ssh` is no longer a sandbox backend - it's now a separate execution environment.
fn normalize_sandbox_backend(raw: Option<&String>) -> String {
    let Some(r) = raw else {
        return "docker".to_string();
    };
    let s = r.trim().to_lowercase();
    if s.is_empty() {
        return "docker".to_string();
    }
    match s.as_str() {
        "modal" | "daytona" | "docker" | "singularity" | "auto" => s,
        // Legacy: ssh was moved to be an execution environment, not a sandbox backend
        "ssh" => "docker".to_string(),
        _ => "docker".to_string(),
    }
}

/// Normalize composer `executionEnvironment` from the UI (`local` | `ssh` | `sandbox`).
///
/// - `local`: Run tools and terminal on the local machine
/// - `ssh`: Run tools and terminal on a remote SSH server
/// - `sandbox`: Run tools and terminal in a remote sandbox (Modal, Daytona, Docker, Singularity)
fn normalize_execution_environment(raw: Option<&String>) -> String {
    match raw.map(|s| s.trim().to_ascii_lowercase()).as_deref() {
        Some("ssh") => "ssh".to_string(),
        Some("sandbox") | Some("remote") => "sandbox".to_string(),
        _ => "local".to_string(),
    }
}

fn composer_execution_addendum(env: &str, ssh_server: Option<&str>) -> Option<String> {
    match env {
        "ssh" => {
            let server_info = ssh_server.map(|s| format!(" (server: `{}`)", s)).unwrap_or_default();
            Some(format!(
                "### Composer execution environment\nThe user chose **SSH**{} for this session turn: assume tools and shell should run on the configured SSH server when available; local-only tools may error until remote is fully wired.",
                server_info
            ))
        }
        "sandbox" => Some(
            "### Composer execution environment\nThe user chose **sandbox** for this session turn: assume tools and shell should run on the configured remote sandbox when available; local-only tools may error until remote is fully wired."
                .to_string(),
        ),
        _ => Some(
            "### Composer execution environment\nThe user chose **local**: run terminal commands and workspace tools on this machine."
                .to_string(),
        ),
    }
}

/// 格式化调度计划为 system prompt 的一部分
fn format_scheduler_plan(result: &crate::domain::agents::scheduler::SchedulingResult) -> String {
    let is_content_generation = result
        .plan
        .subtasks
        .iter()
        .any(|t| t.id == "generate-content" || t.id == "gather-requirements");

    let mut plan_text = if is_content_generation {
        String::from("## 内容生成任务执行计划\n\n")
    } else {
        String::from("## 任务执行计划\n\n")
    };

    plan_text.push_str(&format!(
        "此任务已自动分解为 **{}** 个子任务，将按以下顺序执行：\n\n",
        result.plan.subtasks.len()
    ));
    plan_text.push_str(&format!(
        "调度层级：`{}`（主入口） → `{}`（执行监督） → 专职子 Agent。\
         项目计划是执行依据；阶段标签仅用于观测，不代表固定流水线。\
         真实派发、重试、取消和状态汇总由后端编排器负责。\n\n",
        result
            .plan
            .entry_agent_type
            .as_deref()
            .unwrap_or("general-purpose"),
        result
            .plan
            .execution_supervisor_agent_type
            .as_deref()
            .unwrap_or("executor")
    ));

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
                    task_idx, task.description, task.agent_type
                ));
                if let Some(stage) = task.stage.as_ref().and_then(|s| {
                    serde_json::to_value(s)
                        .ok()
                        .and_then(|value| value.as_str().map(ToString::to_string))
                }) {
                    plan_text.push_str(&format!("   - 阶段: `{}`\n", stage));
                }
                if let Some(supervisor) = task.supervisor_agent_type.as_deref() {
                    plan_text.push_str(&format!("   - 上级: `{}`\n", supervisor));
                }
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

    plan_text.push_str(&format!(
        "\n预估执行时间: ~{} 分钟\n",
        result.estimated_duration_secs / 60
    ));
    if !result.reviewer_agents.is_empty() {
        plan_text.push_str(&format!(
            "Reviewer 结构化结论将由: {}\n",
            result.reviewer_agents.join(", ")
        ));
    }

    // 对于内容生成任务，添加重要提示
    if is_content_generation {
        plan_text.push_str("\n### ⚠️ 重要提示\n");
        plan_text.push_str("这是一个**内容生成任务**。你必须：\n");
        plan_text.push_str("1. **生成完整、详细的内容**，不要只是概述或框架\n");
        plan_text.push_str("2. **包含具体的细节**：名称、地址、时间、价格、建议等\n");
        plan_text.push_str("3. **确保内容实用可读**，用户可以直接使用\n");
        plan_text.push_str(
            "4. 如果这是默认 General 路径，请先向用户展示计划，等待计划卡片按钮确认后再执行\n",
        );
    } else {
        plan_text.push_str(
            "\n请向用户展示该计划并说明可通过计划卡片按钮执行；不要在默认 General 路径中自行执行子任务。",
        );
    }

    plan_text
}

fn looks_like_resume_request(text: &str) -> bool {
    let lower = text.to_lowercase();
    [
        "resume",
        "continue",
        "继续",
        "恢复",
        "从上次继续",
        "继续上次",
        "pick up where",
    ]
    .iter()
    .any(|token| lower.contains(token))
}

pub(super) struct ChatOrchestrationEvent<'a> {
    session_id: &'a str,
    round_id: Option<&'a str>,
    message_id: Option<&'a str>,
    mode: Option<&'a str>,
    event_type: &'a str,
    phase: Option<&'a str>,
    task_id: Option<&'a str>,
    payload: serde_json::Value,
}

pub(super) async fn append_orchestration_event(
    repo: &crate::domain::persistence::SessionRepository,
    event: ChatOrchestrationEvent<'_>,
) {
    let payload_json = serde_json::to_string(&event.payload).unwrap_or_else(|_| "{}".to_string());
    if let Err(e) = repo
        .append_orchestration_event(NewOrchestrationEventRecord {
            session_id: event.session_id,
            round_id: event.round_id,
            message_id: event.message_id,
            mode: event.mode,
            event_type: event.event_type,
            phase: event.phase,
            task_id: event.task_id,
            payload_json: &payload_json,
        })
        .await
    {
        tracing::warn!(target: "omiga::orchestration_events", session_id = event.session_id, event_type = event.event_type, error = %e, "append_orchestration_event failed");
    }
}

async fn append_preflight_stage_event(
    repo: &crate::domain::persistence::SessionRepository,
    session_id: &str,
    message_id: &str,
    mode: Option<&str>,
    stage: &str,
    duration_ms: u128,
    payload: serde_json::Value,
) {
    append_orchestration_event(
        repo,
        ChatOrchestrationEvent {
            session_id,
            round_id: None,
            message_id: Some(message_id),
            mode,
            event_type: "preflight_stage_completed",
            phase: Some("preflight"),
            task_id: None,
            payload: serde_json::json!({
                "stage": stage,
                "durationMs": duration_ms,
                "payload": payload,
            }),
        },
    )
    .await;
}

async fn append_preflight_stage_failed_event(
    repo: &crate::domain::persistence::SessionRepository,
    session_id: &str,
    message_id: &str,
    mode: Option<&str>,
    stage: &str,
    duration_ms: u128,
    error: &str,
) {
    append_orchestration_event(
        repo,
        ChatOrchestrationEvent {
            session_id,
            round_id: None,
            message_id: Some(message_id),
            mode,
            event_type: "preflight_stage_failed",
            phase: Some("preflight"),
            task_id: None,
            payload: serde_json::json!({
                "stage": stage,
                "durationMs": duration_ms,
                "error": error,
            }),
        },
    )
    .await;
}

fn normalize_llm_tool_history_for_model(
    name: &str,
    input: &serde_json::Value,
) -> (String, serde_json::Value) {
    let normalized_name = normalize_legacy_retrieval_tool_name(name);
    let serialized_input = serde_json::to_string(input).unwrap_or_else(|_| "{}".to_string());
    let normalized_input =
        normalize_legacy_retrieval_tool_arguments(name, &normalized_name, &serialized_input);
    let value = serde_json::from_str(&normalized_input).unwrap_or_else(|_| input.clone());
    (normalized_name, value)
}

/// Send a message to Claude and get a streaming response
#[tauri::command]
pub async fn send_message(
    app: AppHandle,
    app_state: State<'_, OmigaAppState>,
    request: SendMessageRequest,
) -> CommandResult<MessageResponse> {
    let send_message_started_at = std::time::Instant::now();
    let input_target = match ChatInputTarget::parse(request.input_target.as_deref()) {
        Ok(t) => t,
        Err(msg) => {
            return Err(OmigaError::Chat(ChatError::StreamError(msg.to_string())));
        }
    };
    let computer_use_mode = crate::domain::computer_use::ComputerUseMode::from_request(
        request.computer_use_mode.as_deref(),
    );
    tracing::debug!(
        target: "omiga::computer_use",
        mode = computer_use_mode.as_str(),
        enabled = computer_use_mode.is_enabled(),
        "computer use request gate resolved"
    );

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

    // ===== Keyword-to-Skill routing =====
    // Detect orchestration keywords (ralph, team, literature-search, etc.) and store the route
    // so the skill body can be injected directly into the system prompt later.
    // The user message is left unchanged — the skill instructions arrive via the system prompt,
    // which means the LLM's very first token is already operating under skill guidance (OMX-style
    // auto-invocation) rather than having to decide whether to call the Skill tool.
    let routing_content = request
        .routing_content
        .as_deref()
        .unwrap_or(&request.content);
    let explicit_workflow_command = request
        .workflow_command
        .as_deref()
        .map(str::trim)
        .filter(|cmd| !cmd.is_empty());
    let direct_skill_route = crate::domain::routing::parse_direct_skill_command(routing_content);
    let keyword_skill_route = if request.use_tools {
        match explicit_workflow_command {
            Some("plan") => Some(crate::domain::routing::SkillRoute {
                skill_name: "plan".to_string(),
                args: routing_content.to_string(),
                priority: 12,
            }),
            Some("team") => Some(crate::domain::routing::SkillRoute {
                skill_name: "team".to_string(),
                args: routing_content.to_string(),
                priority: 12,
            }),
            Some("autopilot") => Some(crate::domain::routing::SkillRoute {
                skill_name: "autopilot".to_string(),
                args: routing_content.to_string(),
                priority: 12,
            }),
            _ => direct_skill_route
                .or_else(|| crate::domain::routing::detect_skill_route(routing_content)),
        }
    } else {
        None
    };
    let trace_mode = explicit_workflow_command.map(str::to_string).or_else(|| {
        keyword_skill_route
            .as_ref()
            .map(|route| route.skill_name.clone())
    });
    if let Some(ref route) = keyword_skill_route {
        tracing::info!(
            target: "omiga::routing",
            skill = %route.skill_name,
            priority = route.priority,
            "Keyword routing: will inject '{}' skill body into system prompt",
            route.skill_name
        );
    }

    // Get or create session (database is single source of truth)
    let repo = &*app_state.repo;
    let exec_env = normalize_execution_environment(request.execution_environment.as_ref());
    let sandbox_backend = normalize_sandbox_backend(request.sandbox_backend.as_ref());

    let (session_id, mut session, user_message_id, project_path) = if let Some(ref id) =
        request.session_id
    {
        // Load existing session from database
        let db_session = repo.get_session(id).await.map_err(|e| {
            OmigaError::Chat(ChatError::StreamError(format!(
                "Failed to load session: {}",
                e
            )))
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
                repo.save_message(NewMessageRecord {
                    id: &msg_id,
                    session_id: &session.id,
                    role: "user",
                    content: &request.content,
                    tool_calls: None,
                    tool_call_id: None,
                    token_usage_json: None,
                    reasoning_content: None,
                    follow_up_suggestions_json: None,
                    turn_summary: None,
                })
                .await
                .map_err(|e| {
                    OmigaError::Chat(ChatError::StreamError(format!(
                        "Failed to save message: {}",
                        e
                    )))
                })?;
            }

            // Update session timestamp
            repo.touch_session(&session.id).await.ok();

            // Cache in memory — keep todo/task Arcs if already present; else load from SQLite
            {
                let mut sessions = app_state.chat.sessions.write().await;
                let ssh_server = request.ssh_server.clone();
                if let Some(runtime) = sessions.get_mut(&session.id) {
                    runtime.session = session.clone();
                    runtime.active_round_ids.clear();
                    runtime.execution_environment = exec_env.clone();
                    runtime.ssh_server = ssh_server.clone();
                    runtime.sandbox_backend = sandbox_backend.clone();
                    runtime.local_venv_type = request.local_venv_type.clone().unwrap_or_default();
                    runtime.local_venv_name = request.local_venv_name.clone().unwrap_or_default();
                } else {
                    let (todos_v, tasks_v) = repo
                        .get_session_tool_state(&session.id)
                        .await
                        .map_err(|e| {
                            OmigaError::Chat(ChatError::StreamError(format!(
                                "Failed to load session tool state: {}",
                                e
                            )))
                        })?;
                    sessions.insert(
                        session.id.clone(),
                        SessionRuntimeState {
                            session: session.clone(),
                            active_round_ids: vec![],
                            todos: Arc::new(tokio::sync::Mutex::new(todos_v)),
                            agent_tasks: Arc::new(tokio::sync::Mutex::new(tasks_v)),
                            plan_mode: Arc::new(Mutex::new(false)),
                            execution_environment: exec_env.clone(),
                            ssh_server: ssh_server.clone(),
                            sandbox_backend: sandbox_backend.clone(),
                            local_venv_type: request.local_venv_type.clone().unwrap_or_default(),
                            local_venv_name: request.local_venv_name.clone().unwrap_or_default(),
                            env_store: crate::domain::tools::env_store::EnvStore::new(),
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
            crate::domain::chat_session_title::fallback_title_from_message(&request.content)
        });

        let mut session = Session::new(session_name, project_path);
        session.add_user_message(&request.content);

        // Save session to database
        repo.create_session(&session.id, &session.name, &session.project_path)
            .await
            .map_err(|e| {
                OmigaError::Chat(ChatError::StreamError(format!(
                    "Failed to create session: {}",
                    e
                )))
            })?;

        // Save user message
        let msg_id = uuid::Uuid::new_v4().to_string();
        repo.save_message(NewMessageRecord {
            id: &msg_id,
            session_id: &session.id,
            role: "user",
            content: &request.content,
            tool_calls: None,
            tool_call_id: None,
            token_usage_json: None,
            reasoning_content: None,
            follow_up_suggestions_json: None,
            turn_summary: None,
        })
        .await
        .map_err(|e| {
            OmigaError::Chat(ChatError::StreamError(format!(
                "Failed to save message: {}",
                e
            )))
        })?;

        // Cache in memory
        let ssh_server = request.ssh_server.clone();
        let runtime_state = SessionRuntimeState {
            session: session.clone(),
            active_round_ids: vec![],
            todos: Arc::new(tokio::sync::Mutex::new(vec![])),
            agent_tasks: Arc::new(tokio::sync::Mutex::new(vec![])),
            plan_mode: Arc::new(Mutex::new(false)),
            execution_environment: exec_env.clone(),
            ssh_server: ssh_server.clone(),
            sandbox_backend: sandbox_backend.clone(),
            local_venv_type: request.local_venv_type.clone().unwrap_or_default(),
            local_venv_name: request.local_venv_name.clone().unwrap_or_default(),
            env_store: crate::domain::tools::env_store::EnvStore::new(),
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
    let has_existing_ralph_state =
        crate::domain::ralph_state::read_state(&project_root, &session_id)
            .await
            .is_some();
    let has_existing_autopilot_state =
        crate::domain::autopilot_state::read_state(&project_root, &session_id)
            .await
            .is_some();
    let has_existing_team_state = crate::domain::team_state::read_state(&project_root, &session_id)
        .await
        .is_some();

    // Composer「权限模式」→ PermissionManager：无用户规则命中时按本会话立场硬拦截（与前端输入框同步）
    app_state
        .permission_manager
        .set_session_composer_stance(&session_id, request.permission_mode.as_deref())
        .await;

    let session_plan_mode_flag = {
        let sessions = app_state.chat.sessions.read().await;
        sessions
            .get(&session_id)
            .map(|runtime| runtime.plan_mode.clone())
    };
    let session_plan_mode_active = match session_plan_mode_flag {
        Some(flag) => *flag.lock().await,
        None => false,
    };

    // 检测是否为 Plan mode（Composer Plan Agent、显式 /plan 命令、或 EnterPlanMode 后的后续轮次）
    let is_plan_mode = request.composer_agent_type.as_deref() == Some("Plan")
        || matches!(explicit_workflow_command, Some("plan"))
        || session_plan_mode_active;

    // ===== 智能调度系统集成 =====
    // 检测是否使用自动调度模式（用户选择 auto 或未指定特定 Agent）
    // Team mode keyword routing also triggers the scheduler so parallel workers are spawned.
    let is_team_keyword_route = keyword_skill_route
        .as_ref()
        .map(|r| r.skill_name == "team")
        .unwrap_or(false);
    let is_ralph_keyword_route = keyword_skill_route
        .as_ref()
        .map(|r| r.skill_name == "ralph")
        .unwrap_or(false);
    let is_autopilot_keyword_route = keyword_skill_route
        .as_ref()
        .map(|r| r.skill_name == "autopilot")
        .unwrap_or(false);
    let is_plan_command = matches!(explicit_workflow_command, Some("plan"));
    let is_schedule_command = matches!(explicit_workflow_command, Some("schedule"));
    let is_explicit_execution_workflow = is_schedule_command
        || is_team_keyword_route
        || is_autopilot_keyword_route
        || is_ralph_keyword_route;
    let is_default_general_route = !is_plan_command
        && !is_explicit_execution_workflow
        && request
            .composer_agent_type
            .as_deref()
            .map(|t| t == "auto" || t == "general-purpose" || t.is_empty())
            .unwrap_or(true);

    if is_ralph_keyword_route {
        if looks_like_resume_request(routing_content) || has_existing_ralph_state {
            append_orchestration_event(
                repo,
                ChatOrchestrationEvent {
                    session_id: &session_id,
                    round_id: None,
                    message_id: Some(&user_message_id),
                    mode: Some("ralph"),
                    event_type: "resume_requested",
                    phase: None,
                    task_id: None,
                    payload: serde_json::json!({ "goal": request.content }),
                },
            )
            .await;
        }
        begin_ralph_turn_if_needed(
            mode_lifecycle_context!(
                true,
                &app_state.chat.sessions,
                repo,
                &project_root,
                &session_id,
                ralph_runtime_env_label(
                    exec_env.as_str(),
                    request.ssh_server.as_deref(),
                    request.local_venv_type.as_deref().unwrap_or(""),
                    request.local_venv_name.as_deref().unwrap_or(""),
                ),
                None,
            ),
            &request.content,
        )
        .await;
        append_orchestration_event(
            repo,
            ChatOrchestrationEvent {
                session_id: &session_id,
                round_id: None,
                message_id: Some(&user_message_id),
                mode: Some("ralph"),
                event_type: "mode_requested",
                phase: Some("planning"),
                task_id: None,
                payload: serde_json::json!({ "goal": request.content }),
            },
        )
        .await;
    }
    if is_autopilot_keyword_route {
        if matches!(explicit_workflow_command, Some("autopilot"))
            || looks_like_resume_request(routing_content)
            || has_existing_autopilot_state
        {
            append_orchestration_event(
                repo,
                ChatOrchestrationEvent {
                    session_id: &session_id,
                    round_id: None,
                    message_id: Some(&user_message_id),
                    mode: Some("autopilot"),
                    event_type: "resume_requested",
                    phase: None,
                    task_id: None,
                    payload: serde_json::json!({ "goal": request.content }),
                },
            )
            .await;
        }
        begin_autopilot_turn_if_needed(
            mode_lifecycle_context!(
                true,
                &app_state.chat.sessions,
                repo,
                &project_root,
                &session_id,
                ralph_runtime_env_label(
                    exec_env.as_str(),
                    request.ssh_server.as_deref(),
                    request.local_venv_type.as_deref().unwrap_or(""),
                    request.local_venv_name.as_deref().unwrap_or(""),
                ),
                None,
            ),
            &request.content,
        )
        .await;
        append_orchestration_event(
            repo,
            ChatOrchestrationEvent {
                session_id: &session_id,
                round_id: None,
                message_id: Some(&user_message_id),
                mode: Some("autopilot"),
                event_type: "mode_requested",
                phase: Some("intake"),
                task_id: None,
                payload: serde_json::json!({ "goal": request.content }),
            },
        )
        .await;
    }
    if is_team_keyword_route {
        if matches!(explicit_workflow_command, Some("team"))
            || looks_like_resume_request(routing_content)
            || has_existing_team_state
        {
            append_orchestration_event(
                repo,
                ChatOrchestrationEvent {
                    session_id: &session_id,
                    round_id: None,
                    message_id: Some(&user_message_id),
                    mode: Some("team"),
                    event_type: "resume_requested",
                    phase: None,
                    task_id: None,
                    payload: serde_json::json!({ "goal": request.content }),
                },
            )
            .await;
        }
        begin_team_turn_if_needed(
            true,
            repo,
            &project_root,
            &session_id,
            &request.content,
            None,
        )
        .await;
        append_orchestration_event(
            repo,
            ChatOrchestrationEvent {
                session_id: &session_id,
                round_id: None,
                message_id: Some(&user_message_id),
                mode: Some("team"),
                event_type: "mode_requested",
                phase: Some("planning"),
                task_id: None,
                payload: serde_json::json!({ "goal": request.content }),
            },
        )
        .await;
    }

    let mode_strategy_override = if let Some(route) = keyword_skill_route.as_ref() {
        crate::domain::mode_resume::suggested_mode_strategy(
            &project_root,
            &session_id,
            &route.skill_name,
        )
        .await
    } else {
        None
    };
    let mode_execution_lane = if let Some(route) = keyword_skill_route.as_ref() {
        match route.skill_name.as_str() {
            "ralph" => {
                crate::domain::orchestration::ralph::RalphOrchestrator::current_execution_lane(
                    &project_root,
                    &session_id,
                )
                .await
            }
            "autopilot" => crate::domain::orchestration::autopilot::AutopilotOrchestrator::current_execution_lane(
                &project_root,
                &session_id,
            )
            .await,
            "team" => {
                crate::domain::orchestration::team::TeamOrchestrator::current_execution_lane(
                    &project_root,
                    &session_id,
                )
                .await
            }
            _ => None,
        }
    } else {
        None
    };

    // Detect strategy-specific keyword routes (phased / competitive / verification-first)
    let keyword_strategy: Option<SchedulingStrategy> = keyword_skill_route
        .as_ref()
        .and_then(|r| {
            match r.skill_name.as_str() {
                "team" => Some(SchedulingStrategy::Team),
                "plan" => Some(SchedulingStrategy::Phased),
                // No dedicated keyword rules for competitive yet — reserved for future skill routing
                _ => None,
            }
        })
        .or(if is_schedule_command {
            Some(SchedulingStrategy::Phased)
        } else {
            None
        })
        .or(mode_strategy_override);

    let use_scheduler = is_schedule_command
        || is_team_keyword_route
        || (!is_plan_mode
            && request
                .composer_agent_type
                .as_deref()
                .map(|t| t == "auto" || t == "general-purpose" || t.is_empty())
                .unwrap_or(true));

    // 如果是自动模式或 Team 关键词路由，检测任务复杂度并可能进行任务分解
    // Pre-fetch LLM config for the planner (needed before llm_config is built below).
    let planner_llm_config = crate::commands::chat::get_llm_config(&app_state.chat)
        .await
        .ok();

    let scheduler_result: Option<crate::domain::agents::scheduler::SchedulingResult> =
        if use_scheduler && request.use_tools {
            let scheduler = AgentScheduler::new();
            let scheduler_stage_started_at = std::time::Instant::now();
            // Strategy priority: keyword route > composer_agent_type > Auto
            let (strategy, force_decompose) = if let Some(s) = keyword_strategy {
                (s, true)
            } else {
                match request.composer_agent_type.as_deref() {
                    Some(t) if t != "auto" && t != "general-purpose" && !t.is_empty() => {
                        let s = SchedulingStrategy::from_planner_hint(t);
                        let force = s != SchedulingStrategy::Auto;
                        (s, force)
                    }
                    _ => (SchedulingStrategy::Auto, false),
                }
            };

            let scheduling_req = SchedulingRequest::new(routing_content)
                .with_project_root(project_root.to_string_lossy().as_ref())
                .with_mode_hint(match keyword_skill_route.as_ref() {
                    Some(route) => route.skill_name.clone(),
                    None => explicit_workflow_command.unwrap_or_default().to_string(),
                })
                .with_strategy(strategy)
                .with_auto_decompose(force_decompose);

            match scheduler
                .schedule(scheduling_req, planner_llm_config.as_ref())
                .await
            {
                Ok(result) => {
                    let classified_complex = result.selected_agents.len() > 1
                        || !matches!(
                            result.recommended_strategy,
                            SchedulingStrategy::Single | SchedulingStrategy::Auto
                        );
                    append_orchestration_event(
                        repo,
                        ChatOrchestrationEvent {
                            session_id: &session_id,
                            round_id: None,
                            message_id: Some(&user_message_id),
                            mode: keyword_skill_route.as_ref().map(|r| r.skill_name.as_str()).or(
                                if is_schedule_command {
                                    Some("schedule")
                                } else if is_plan_command || is_default_general_route {
                                    Some("plan")
                                } else {
                                    None
                                },
                            ),
                            event_type: "leader_intent_classified",
                            phase: if classified_complex {
                                Some("planning")
                            } else {
                                Some("solo")
                            },
                            task_id: None,
                            payload: serde_json::json!({
                                "entryAgentType": "general-purpose",
                                "classification": if classified_complex { "complex" } else { "simple" },
                                "strategy": format!("{:?}", result.recommended_strategy),
                                "taskCount": result.plan.subtasks.len(),
                                "agentCount": result.selected_agents.len(),
                                "willAutoExecute": is_explicit_execution_workflow,
                            }),
                        },
                    )
                    .await;
                    if trace_mode.is_some() {
                        append_preflight_stage_event(
                            repo,
                            &session_id,
                            &user_message_id,
                            trace_mode.as_deref(),
                            "scheduler_plan",
                            scheduler_stage_started_at.elapsed().as_millis(),
                            serde_json::json!({
                                "taskCount": result.plan.subtasks.len(),
                                "agentCount": result.selected_agents.len(),
                                "strategy": format!("{:?}", result.recommended_strategy),
                            }),
                        )
                        .await;
                    }
                    // Accept the result when:
                    //  a) explicit team keyword route (user typed /team)
                    //  b) planner produced > 1 agent (real multi-agent plan)
                    //  c) planner recommended a non-single strategy
                    let is_real_multiagent = result.selected_agents.len() > 1;
                    let strategy_demands_orchestration = !matches!(
                        result.recommended_strategy,
                        SchedulingStrategy::Single | SchedulingStrategy::Auto
                    );
                    if is_plan_command
                        || is_team_keyword_route
                        || is_real_multiagent
                        || strategy_demands_orchestration
                    {
                        tracing::info!(
                            target: "omiga::scheduler",
                            task_count = result.plan.subtasks.len(),
                            agents = ?result.selected_agents,
                            recommended_strategy = ?result.recommended_strategy,
                            team_mode = is_team_keyword_route,
                            "Task decomposed into subtasks"
                        );
                        append_orchestration_event(
                            repo,
                            ChatOrchestrationEvent {
                                session_id: &session_id,
                                round_id: None,
                                message_id: Some(&user_message_id),
                                mode: keyword_skill_route
                                    .as_ref()
                                    .map(|r| r.skill_name.as_str())
                                    .or(if is_schedule_command {
                                        Some("schedule")
                                    } else {
                                        None
                                    }),
                                event_type: "schedule_plan_created",
                                phase: None,
                                task_id: None,
                                payload: serde_json::json!({
                                    "planId": result.plan.plan_id,
                                    "taskCount": result.plan.subtasks.len(),
                                    "agents": result.selected_agents,
                                    "strategy": format!("{:?}", result.recommended_strategy),
                                }),
                            },
                        )
                        .await;
                        if is_plan_command || is_default_general_route {
                            append_orchestration_event(
                                repo,
                                ChatOrchestrationEvent {
                                    session_id: &session_id,
                                    round_id: None,
                                    message_id: Some(&user_message_id),
                                    mode: Some("plan"),
                                    event_type: "plan_ready_for_approval",
                                    phase: Some("planning"),
                                    task_id: None,
                                    payload: serde_json::json!({
                                        "planId": result.plan.plan_id,
                                        "entryAgentType": result.plan.entry_agent_type.clone(),
                                        "executionSupervisorAgentType": result.plan.execution_supervisor_agent_type.clone(),
                                        "taskCount": result.plan.subtasks.len(),
                                        "approvalSurface": "plan_card_buttons",
                                    }),
                                },
                            )
                            .await;
                        }
                        Some(result)
                    } else {
                        None
                    }
                }
                Err(e) => {
                    tracing::warn!(target: "omiga::scheduler", "Scheduling failed: {}", e);
                    if trace_mode.is_some() {
                        append_preflight_stage_failed_event(
                            repo,
                            &session_id,
                            &user_message_id,
                            trace_mode.as_deref(),
                            "scheduler_plan",
                            scheduler_stage_started_at.elapsed().as_millis(),
                            &e,
                        )
                        .await;
                    }
                    None
                }
            }
        } else {
            None
        };
    let preflight_event_mode = trace_mode.as_deref().or(Some("preflight"));

    if is_schedule_command {
        if let Some(schedule_result) = scheduler_result.clone() {
            let stream_message_id = uuid::Uuid::new_v4().to_string();
            let schedule_round_id = uuid::Uuid::new_v4().to_string();
            let session_id_for_bg = session_id.clone();
            let project_root_for_bg = project_root.to_string_lossy().to_string();
            let request_for_bg = crate::commands::chat::RunAgentScheduleRequest {
                user_request: routing_content.to_string(),
                project_root: project_root_for_bg.clone(),
                session_id: session_id_for_bg.clone(),
                max_agents: Some(schedule_result.plan.subtasks.len()),
                auto_decompose: true,
                strategy: Some(SchedulingStrategy::Phased),
                mode_hint: Some("schedule".to_string()),
                skip_confirmation: true,
            };
            let app_for_bg = app.clone();
            let stream_message_id_for_bg = stream_message_id.clone();
            tokio::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                let _ = app_for_bg.emit(
                    &format!("chat-stream-{}", stream_message_id_for_bg),
                    &StreamOutputItem::Start,
                );
                let _ = app_for_bg.emit(
                    &format!("chat-stream-{}", stream_message_id_for_bg),
                    &StreamOutputItem::Complete,
                );
                if let Some(state) = app_for_bg.try_state::<OmigaAppState>() {
                    if let Err(e) = self::provider::run_agent_schedule_inner(
                        app_for_bg.clone(),
                        &state,
                        request_for_bg,
                    )
                    .await
                    {
                        tracing::warn!(
                            target: "omiga::scheduler",
                            session_id = %session_id_for_bg,
                            error = %e,
                            "Direct /schedule orchestration failed"
                        );
                    }
                } else {
                    tracing::warn!(target: "omiga::scheduler", "OmigaAppState unavailable for direct /schedule orchestration");
                }
            });

            return Ok(MessageResponse {
                message_id: stream_message_id,
                session_id,
                round_id: schedule_round_id,
                user_message_id: Some(user_message_id),
                input_kind: Some("schedule_orchestration_started".to_string()),
                scheduler_plan: Some(schedule_result),
                initial_todos: None,
            });
        }
    }

    if is_autopilot_keyword_route {
        if let Some(phase) = crate::domain::orchestration::autopilot::AutopilotOrchestrator::phase_for_scheduler_result(
            &project_root,
            &session_id,
            scheduler_result.is_some(),
        )
        .await
        {
            update_autopilot_phase_if_needed(
                mode_lifecycle_context!(
                    true,
                    &app_state.chat.sessions,
                    repo,
                    &project_root,
                    &session_id,
                    ralph_runtime_env_label(
                        exec_env.as_str(),
                        request.ssh_server.as_deref(),
                        request.local_venv_type.as_deref().unwrap_or(""),
                        request.local_venv_name.as_deref().unwrap_or(""),
                    ),
                    None,
                ),
                phase,
            )
            .await;
        }
    }

    // Lazy provider restoration: if this session has a stored provider that differs from the
    // current global, restore it now (first message after session switch).  This moves the
    // ~100 ms config-file read out of load_session (which blocks the UI) and into send_message
    // (where the user is already waiting for the LLM response anyway).
    if let Some(ref desired) = request.active_provider_entry_name {
        let desired = desired.trim();
        if !desired.is_empty() {
            let provider_restore_started_at = std::time::Instant::now();
            let current = app_state
                .chat
                .active_provider_entry_name
                .lock()
                .await
                .clone();
            let matches = current.as_deref().map(str::trim) == Some(desired);
            drop(current);
            if !matches {
                match tokio::time::timeout(
                    std::time::Duration::from_secs(3),
                    apply_named_provider_runtime(&app_state, desired),
                )
                .await
                {
                    Ok(Ok(_)) => {
                        append_preflight_stage_event(
                            repo,
                            &session_id,
                            &user_message_id,
                            preflight_event_mode,
                            "provider_restore",
                            provider_restore_started_at.elapsed().as_millis(),
                            serde_json::json!({ "provider": desired, "status": "ok" }),
                        )
                        .await;
                    }
                    Ok(Err(e)) => {
                        tracing::warn!(
                            target: "omiga::llm",
                            "Lazy provider restore for session {} failed ({}), using current config",
                            session_id, e
                        );
                        append_preflight_stage_failed_event(
                            repo,
                            &session_id,
                            &user_message_id,
                            preflight_event_mode,
                            "provider_restore",
                            provider_restore_started_at.elapsed().as_millis(),
                            &e.to_string(),
                        )
                        .await;
                    }
                    Err(_) => {
                        tracing::warn!(
                            target: "omiga::llm",
                            "Lazy provider restore for session {} timed out; using current config",
                            session_id
                        );
                        append_preflight_stage_failed_event(
                            repo,
                            &session_id,
                            &user_message_id,
                            preflight_event_mode,
                            "provider_restore",
                            provider_restore_started_at.elapsed().as_millis(),
                            "provider restore timed out",
                        )
                        .await;
                    }
                }
            }
        }
    }

    let llm_config_started_at = std::time::Instant::now();
    let mut llm_config = tokio::time::timeout(
        std::time::Duration::from_secs(3),
        get_llm_config(&app_state.chat),
    )
    .await
    .map_err(|_| {
        OmigaError::Chat(ChatError::StreamError(
            "Timed out while loading LLM configuration".to_string(),
        ))
    })??;
    append_preflight_stage_event(
        repo,
        &session_id,
        &user_message_id,
        preflight_event_mode,
        "llm_config",
        llm_config_started_at.elapsed().as_millis(),
        serde_json::json!({ "provider": format!("{:?}", llm_config.provider), "model": llm_config.model }),
    )
    .await;
    let session_runtime_cfg = crate::domain::session::load_session_config(&session_id);
    let resolved_runtime_constraints =
        crate::domain::runtime_constraints::resolve_runtime_constraint_config(
            &project_root,
            session_runtime_cfg.runtime_constraints.as_ref(),
        );
    let integrations_cfg = {
        let hit = app_state
            .integrations_config_cache
            .lock()
            .expect("integrations config cache poisoned")
            .get(&project_root)
            .filter(|s| s.cached_at.elapsed() < INTEGRATIONS_CONFIG_CACHE_TTL)
            .map(|s| s.config.clone());
        hit.unwrap_or_else(|| {
            // Lock is released above; safe to do a blocking file read here.
            let cfg = integrations_config::load_integrations_config(&project_root);
            app_state
                .integrations_config_cache
                .lock()
                .expect("integrations config cache poisoned")
                .insert(
                    project_root.clone(),
                    IntegrationsConfigCacheSlot {
                        config: cfg.clone(),
                        cached_at: std::time::Instant::now(),
                    },
                );
            cfg
        })
    };
    let working_memory_started_at = std::time::Instant::now();
    match tokio::time::timeout(
        std::time::Duration::from_secs(2),
        crate::domain::memory::working_memory::mark_user_turn_started(repo, &session_id),
    )
    .await
    {
        Ok(Ok(_)) => {
            append_preflight_stage_event(
                repo,
                &session_id,
                &user_message_id,
                preflight_event_mode,
                "working_memory_mark",
                working_memory_started_at.elapsed().as_millis(),
                serde_json::json!({ "status": "ok" }),
            )
            .await;
        }
        Ok(Err(e)) => {
            tracing::warn!(
                target: "omiga::memory",
                session_id = %session_id,
                error = %e,
                "Working memory mark_user_turn_started failed; continuing without blocking chat"
            );
            append_preflight_stage_failed_event(
                repo,
                &session_id,
                &user_message_id,
                preflight_event_mode,
                "working_memory_mark",
                working_memory_started_at.elapsed().as_millis(),
                &e.to_string(),
            )
            .await;
        }
        Err(_) => {
            tracing::warn!(
                target: "omiga::memory",
                session_id = %session_id,
                "Working memory mark_user_turn_started timed out; continuing without blocking chat"
            );
            append_preflight_stage_failed_event(
                repo,
                &session_id,
                &user_message_id,
                preflight_event_mode,
                "working_memory_mark",
                working_memory_started_at.elapsed().as_millis(),
                "working memory mark timed out",
            )
            .await;
        }
    }
    // Run independent async I/O in parallel to reduce pre-LLM latency.
    let skill_cache_ref = &app_state.skill_cache;
    let memory_lookup_started_at = std::time::Instant::now();
    let (skills_exist, memory_ctx, memory_nav) =
        match tokio::time::timeout(std::time::Duration::from_secs(3), async {
            tokio::join!(
                skills::skills_any_exist(&project_root, skill_cache_ref),
                crate::commands::memory::get_memory_context_cached(
                    repo,
                    &project_root,
                    Some(&session_id),
                    &request.content,
                    3,
                    Some(&app_state.memory_preflight_cache),
                ),
                crate::commands::memory::memory_navigation_section(&project_root),
            )
        })
        .await
        {
            Ok(result) => {
                append_preflight_stage_event(
                    repo,
                    &session_id,
                    &user_message_id,
                    preflight_event_mode,
                    "memory_context",
                    memory_lookup_started_at.elapsed().as_millis(),
                    serde_json::json!({
                        "skillsExist": result.0,
                        "memoryContext": result.1.is_some(),
                        "memoryNavChars": result.2.len(),
                    }),
                )
                .await;
                result
            }
            Err(_) => {
                tracing::warn!(
                    target: "omiga::memory",
                    session_id = %session_id,
                    "Memory/skill preflight timed out; continuing with no injected memory context"
                );
                append_preflight_stage_failed_event(
                    repo,
                    &session_id,
                    &user_message_id,
                    preflight_event_mode,
                    "memory_context",
                    memory_lookup_started_at.elapsed().as_millis(),
                    "memory context timed out",
                )
                .await;
                (false, None, String::new())
            }
        };

    let skills_system_section = if skills_exist {
        "This project has skills available. For non-trivial tasks, use `list_skills` to discover specialized workflows before falling back to generic tools.".to_string()
    } else {
        String::new()
    };

    // Ported agent system prompt from `src/constants/prompts.ts` — injected when tools are enabled.
    let mut prompt_parts: Vec<String> = Vec::new();
    if request.use_tools {
        prompt_parts.push(agent_prompt::build_system_prompt(
            &project_root,
            &llm_config.model,
        ));
        if is_plan_mode {
            prompt_parts.push(agent_prompt::active_plan_mode_turn_addendum().to_string());
        }
        if coordinator::is_coordinator_mode() {
            prompt_parts.push(agent_prompt::coordinator_mode_addendum().to_string());
        }
    }
    // 用户级 SOUL / MEMORY / USER 与 ~/.omiga + 项目 .omiga 人格配置（compose_full_agent_system_prompt 会读取同目录下的 personalities）
    let user_omiga_ctx = crate::domain::agents::load_user_omiga_context();
    for sec in user_omiga_ctx.main_system_prompt_sections() {
        prompt_parts.push(sec);
    }
    if let Some(ref u) = llm_config.system_prompt {
        let t = u.trim();
        if !t.is_empty() {
            prompt_parts.push(t.to_string());
        }
    }
    if skills_exist {
        prompt_parts.push(skills_system_section);
    }
    let plugin_load_outcome = crate::domain::plugins::plugin_load_outcome();
    if let Some(plugins_system_section) =
        crate::domain::plugins::format_plugins_system_section(&plugin_load_outcome)
    {
        prompt_parts.push(plugins_system_section);
    }
    if let Some(selected_plugins_system_section) =
        crate::domain::plugins::format_selected_plugins_system_section(
            &plugin_load_outcome,
            &request.selected_plugin_ids,
        )
    {
        prompt_parts.push(selected_plugins_system_section);
    }
    let connector_catalog = crate::domain::connectors::list_connector_catalog();
    if let Some(connectors_system_section) =
        crate::domain::connectors::format_connectors_system_section(&connector_catalog)
    {
        prompt_parts.push(connectors_system_section);
    }
    // Memory navigation guide — always injected to override the model's default
    // "I have no cross-session memory" belief and tell it where to look.
    let nav = memory_nav.trim().to_string();
    if !nav.is_empty() {
        prompt_parts.push(nav);
    }
    if let Some(ctx) = memory_ctx {
        prompt_parts.push(ctx);
    }
    let overlay_started_at = std::time::Instant::now();
    match tokio::time::timeout(
        std::time::Duration::from_secs(1),
        crate::domain::agents::build_runtime_overlay(&project_root),
    )
    .await
    {
        Ok(Some(overlay)) => {
            prompt_parts.push(overlay);
            append_preflight_stage_event(
                repo,
                &session_id,
                &user_message_id,
                preflight_event_mode,
                "runtime_overlay",
                overlay_started_at.elapsed().as_millis(),
                serde_json::json!({ "status": "ok" }),
            )
            .await;
        }
        Ok(None) => {}
        Err(_) => {
            tracing::warn!(
                target: "omiga::overlay",
                session_id = %session_id,
                "Runtime overlay preflight timed out; continuing without overlay"
            );
            append_preflight_stage_failed_event(
                repo,
                &session_id,
                &user_message_id,
                preflight_event_mode,
                "runtime_overlay",
                overlay_started_at.elapsed().as_millis(),
                "runtime overlay timed out",
            )
            .await;
        }
    }
    if is_ralph_keyword_route {
        if let Some(resume_ctx) =
            crate::domain::mode_resume::build_ralph_resume_context(&project_root, &session_id).await
        {
            prompt_parts.push(resume_ctx);
        }
        if let Some(phase_guidance) =
            crate::domain::mode_resume::build_ralph_phase_guidance(&project_root, &session_id).await
        {
            prompt_parts.push(phase_guidance);
        }
    }
    if is_autopilot_keyword_route {
        if let Some(resume_ctx) =
            crate::domain::mode_resume::build_autopilot_resume_context(&project_root, &session_id)
                .await
        {
            prompt_parts.push(resume_ctx);
        }
        if let Some(phase_guidance) =
            crate::domain::mode_resume::build_autopilot_phase_guidance(&project_root, &session_id)
                .await
        {
            prompt_parts.push(phase_guidance);
        }
    }
    if is_team_keyword_route {
        if let Some(resume_ctx) =
            crate::domain::mode_resume::build_team_resume_context(&project_root, &session_id).await
        {
            prompt_parts.push(resume_ctx);
        }
        if let Some(phase_guidance) =
            crate::domain::mode_resume::build_team_phase_guidance(&project_root, &session_id).await
        {
            prompt_parts.push(phase_guidance);
        }
    }
    if let Some(lane) = mode_execution_lane {
        prompt_parts.push(format!(
            "## Execution Lane: {}\n{}",
            lane.lane_id, lane.instructions
        ));
    }

    // 如果有调度计划（任务分解），添加到 system prompt
    if let Some(ref schedule_result) = scheduler_result {
        let plan_description = format_scheduler_plan(schedule_result);
        prompt_parts.push(plan_description);
    }

    // Keyword-routed skill: inject SKILL.md body directly into system prompt.
    // This implements OMX-style auto-invocation — the skill's instructions are active
    // from token 0, no LLM decision needed to call the Skill tool first.
    if let Some(ref route) = keyword_skill_route {
        let skill_body =
            crate::domain::routing::load_skill_body(&route.skill_name, &route.args, &project_root)
                .await;
        if let Some(body) = skill_body {
            tracing::info!(
                target: "omiga::routing",
                skill = %route.skill_name,
                body_len = body.len(),
                "Injected skill body into system prompt"
            );
            prompt_parts.push(format!(
                "## Active Skill: {}\n\n{}\n\n---\nThe user's task (for $ARGUMENTS context): {}",
                route.skill_name, body, route.args
            ));
        } else {
            // Skill file not found — fall back to a plain instruction
            tracing::warn!(
                target: "omiga::routing",
                skill = %route.skill_name,
                "Skill body not found on disk; falling back to hint"
            );
            prompt_parts.push(format!(
                "## Active Skill: {}\n\nInvoke the `{}` skill immediately as your first action to handle this task.",
                route.skill_name, route.skill_name
            ));
        }
    }

    if request.use_tools {
        let selected_composer = request
            .composer_agent_type
            .as_deref()
            .map(str::trim)
            .unwrap_or("");
        if let Some(lane) = mode_execution_lane {
            if (selected_composer.is_empty()
                || selected_composer == "auto"
                || selected_composer == "general-purpose")
                && (lane.preferred_agent_type.is_some()
                    || !lane.supplemental_agent_types.is_empty())
            {
                let router = crate::domain::agents::get_agent_router();
                let tool_ctx = ToolContext::new(project_root.clone())
                    .with_execution_environment(exec_env.clone())
                    .with_ssh_server(request.ssh_server.clone())
                    .with_sandbox_backend(sandbox_backend.clone())
                    .with_local_venv(
                        request.local_venv_type.as_deref().unwrap_or(""),
                        request.local_venv_name.as_deref().unwrap_or(""),
                    );
                let mut injected: Vec<&str> = Vec::new();
                if let Some(primary) = lane.preferred_agent_type {
                    injected.push(primary);
                }
                for supplemental in lane.supplemental_agent_types {
                    if !injected.contains(supplemental) {
                        injected.push(supplemental);
                    }
                }
                for agent_type in injected {
                    if let Some(agent) = router.get_agent(agent_type) {
                        prompt_parts.push(crate::domain::agents::compose_full_agent_system_prompt(
                            agent, &tool_ctx,
                        ));
                    }
                }
            }
        }
        if let Some(ref at) = request.composer_agent_type {
            let t = at.trim();
            if !t.is_empty() && t != "general-purpose" {
                let router = crate::domain::agents::get_agent_router();
                let agent = router.select_agent(Some(t));
                let tool_ctx = ToolContext::new(project_root.clone())
                    .with_execution_environment(exec_env.clone())
                    .with_ssh_server(request.ssh_server.clone())
                    .with_sandbox_backend(sandbox_backend.clone())
                    .with_local_venv(
                        request.local_venv_type.as_deref().unwrap_or(""),
                        request.local_venv_name.as_deref().unwrap_or(""),
                    );
                prompt_parts.push(crate::domain::agents::compose_full_agent_system_prompt(
                    agent, &tool_ctx,
                ));
            }
        }
        if let Some(line) =
            composer_execution_addendum(exec_env.as_str(), request.ssh_server.as_deref())
        {
            prompt_parts.push(line);
        }
    }
    llm_config.system_prompt = if prompt_parts.is_empty() {
        None
    } else {
        Some(prompt_parts.join("\n\n"))
    };

    if let Some(removed_messages) =
        crate::domain::auto_compact::preview_removed_messages_for_compaction(
            &session.messages,
            &llm_config,
            request.use_tools,
        )
    {
        let op_id = format!("memory-precompact-{}", uuid::Uuid::new_v4());
        emit_activity_operation(
            &app,
            &session_id,
            &op_id,
            "压缩前摘要",
            "running",
            Some(format!(
                "准备提炼 {} 条即将压缩的消息",
                removed_messages.len()
            )),
        );
        match tokio::time::timeout(
            std::time::Duration::from_secs(3),
            crate::domain::memory::working_memory::prepare_for_auto_compact(
                repo,
                &session_id,
                &removed_messages,
            ),
        )
        .await
        {
            Ok(Ok(compact_state)) => {
                emit_activity_operation(
                    &app,
                    &session_id,
                    &op_id,
                    "压缩前摘要",
                    "done",
                    Some("已提炼即将被压缩的上下文".to_string()),
                );
                // Context compression is a semantic trigger: archive session summary now.
                let project_root_for_compact = resolve_session_project_root(&project_path);
                if let Ok(cfg) =
                    crate::domain::memory::load_resolved_config(&project_root_for_compact).await
                {
                    let lt_path = cfg.long_term_path(&project_root_for_compact);
                    crate::commands::chat::turn::archive_on_compact(
                        &app,
                        &session_id,
                        &lt_path,
                        &compact_state,
                    )
                    .await;
                }
            }
            Ok(Err(e)) => {
                tracing::warn!(
                    target: "omiga::memory",
                    session_id = %session_id,
                    error = %e,
                    "Working memory prepare_for_auto_compact failed; continuing without blocking chat"
                );
                emit_activity_operation(
                    &app,
                    &session_id,
                    &op_id,
                    "压缩前摘要",
                    "error",
                    Some(e.to_string()),
                );
            }
            Err(_) => {
                tracing::warn!(
                    target: "omiga::memory",
                    session_id = %session_id,
                    "Working memory prepare_for_auto_compact timed out; continuing without blocking chat"
                );
                emit_activity_operation(
                    &app,
                    &session_id,
                    &op_id,
                    "压缩前摘要",
                    "error",
                    Some("prepare_for_auto_compact timed out".to_string()),
                );
            }
        }
    }

    let compact_started_at = std::time::Instant::now();
    let compact_outcome = match tokio::time::timeout(
        std::time::Duration::from_secs(5),
        crate::domain::auto_compact::compact_session_and_persist(
            repo,
            &session_id,
            &mut session,
            &llm_config,
            request.use_tools,
            &user_message_id,
        ),
    )
    .await
    {
        Ok(Ok(outcome)) => {
            append_preflight_stage_event(
                repo,
                &session_id,
                &user_message_id,
                preflight_event_mode,
                "auto_compact",
                compact_started_at.elapsed().as_millis(),
                serde_json::json!({ "compacted": outcome.is_some() }),
            )
            .await;
            outcome
        }
        Ok(Err(e)) => {
            return Err(OmigaError::Chat(ChatError::StreamError(format!(
                "Auto-compact failed: {}",
                e
            ))));
        }
        Err(_) => {
            tracing::warn!(
                target: "omiga::auto_compact",
                session_id = %session_id,
                "Auto-compact timed out; continuing with current transcript"
            );
            append_preflight_stage_failed_event(
                repo,
                &session_id,
                &user_message_id,
                preflight_event_mode,
                "auto_compact",
                compact_started_at.elapsed().as_millis(),
                "auto compact timed out",
            )
            .await;
            None
        }
    };

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

    if trace_mode.is_some() {
        append_preflight_stage_event(
            repo,
            &session_id,
            &user_message_id,
            trace_mode.as_deref(),
            "send_message_ready",
            send_message_started_at.elapsed().as_millis(),
            serde_json::json!({
                "toolsEnabled": request.use_tools,
                "computerUseMode": computer_use_mode.as_str(),
                "computerUseEnabled": computer_use_mode.is_enabled(),
                "schedulerBuiltPlan": scheduler_result.is_some(),
            }),
        )
        .await;
    }

    // Generate round and message IDs
    let round_id = uuid::Uuid::new_v4().to_string();
    let message_id = uuid::Uuid::new_v4().to_string();

    // Create conversation round record
    tokio::time::timeout(
        std::time::Duration::from_secs(3),
        repo.create_round(
            &round_id,
            &session_id,
            &message_id,
            Some(&user_message_id_for_round),
        ),
    )
    .await
    .map_err(|_| {
        OmigaError::Chat(ChatError::StreamError(
            "Timed out while creating conversation round".to_string(),
        ))
    })?
    .map_err(|e| {
        OmigaError::Chat(ChatError::StreamError(format!(
            "Failed to create round: {}",
            e
        )))
    })?;

    // Set up cancellation tracking
    let cancel_flag = Arc::new(RwLock::new(false));
    let round_cancel = tokio_util::sync::CancellationToken::new();
    let cancellation_state = RoundCancellationState {
        round_id: round_id.clone(),
        message_id: message_id.clone(),
        session_id: session_id.clone(),
        cancelled: cancel_flag.clone(),
        round_cancel: round_cancel.clone(),
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
        let tool_schema_stage_started_at = std::time::Instant::now();
        let deny_entries = {
            let hit = app_state
                .chat
                .permission_deny_cache
                .lock()
                .await
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
                    let entries = load_merged_permission_deny_rule_entries(&project_root);
                    app_state.chat.permission_deny_cache.lock().await.insert(
                        project_root.clone(),
                        PermissionDenyCache {
                            entries: entries.clone(),
                            cached_at: std::time::Instant::now(),
                        },
                    );
                    entries
                }
            }
        };
        validate_permission_deny_entries(&deny_entries);
        let mut all_schemas = all_tool_schemas(skills_exist);
        if computer_use_mode.is_enabled() {
            all_schemas.extend(crate::domain::computer_use::facade_tool_schemas());
        }
        let n_builtin_before = all_schemas.len();
        let mut built = filter_tool_schemas_by_deny_rule_entries(all_schemas, &deny_entries);
        let n_builtin_after = built.len();
        if n_builtin_after < n_builtin_before {
            tracing::debug!(
                target: "omiga::permissions",
                before = n_builtin_before,
                after = n_builtin_after,
                "built-in tool schemas after permissions.deny filter"
            );
        }
        sort_tool_schemas_for_model(&mut built);
        let operator_schemas = crate::domain::operators::enabled_operator_tool_schemas();
        let n_operator_before = operator_schemas.len();
        let operator_after_deny =
            filter_tool_schemas_by_deny_rule_entries(operator_schemas, &deny_entries);
        let n_operator_after = operator_after_deny.len();
        if n_operator_after < n_operator_before {
            tracing::debug!(
                target: "omiga::operators",
                before = n_operator_before,
                after = n_operator_after,
                "operator tool schemas after permissions.deny filter"
            );
        }
        let mut base_names: HashSet<String> = built.iter().map(|t| t.name.clone()).collect();
        let operator_filtered: Vec<_> = operator_after_deny
            .into_iter()
            .filter(|schema| base_names.insert(schema.name.clone()))
            .collect();
        let mcp_stage_started_at = std::time::Instant::now();
        let current_mcp_config_signature =
            crate::domain::mcp::merged_mcp_servers_signature(&project_root);
        let (mcp_tools, mcp_cache_status) = {
            let cached = app_state
                .chat
                .mcp_tool_cache
                .lock()
                .await
                .get(&project_root)
                .map(|e| {
                    (
                        e.schemas.clone(),
                        e.cached_at.elapsed() < MCP_TOOL_CACHE_TTL,
                        e.config_signature == current_mcp_config_signature,
                    )
                });
            match cached {
                Some((schemas, true, true)) => {
                    tracing::debug!(target: "omiga::mcp", "MCP tool schemas served from cache");
                    (schemas, "fresh")
                }
                Some((schemas, true, false)) => {
                    tracing::info!(
                        target: "omiga::mcp",
                        cached = schemas.len(),
                        "MCP tool cache config signature changed; ignoring stale schemas and refreshing in background"
                    );
                    let mcp_tool_cache = app_state.chat.mcp_tool_cache.clone();
                    let root = project_root.clone();
                    tokio::spawn(async move {
                        let config_signature =
                            crate::domain::mcp::merged_mcp_servers_signature(&root);
                        let schemas = crate::domain::mcp::tool_pool::discover_mcp_tool_schemas(
                            &root,
                            std::time::Duration::from_secs(10),
                        )
                        .await;
                        mcp_tool_cache.lock().await.insert(
                            root,
                            McpToolCache {
                                schemas,
                                cached_at: std::time::Instant::now(),
                                config_signature,
                            },
                        );
                    });
                    (vec![], "config-changed")
                }
                Some((schemas, false, true)) => {
                    tracing::info!(
                        target: "omiga::mcp",
                        cached = schemas.len(),
                        "MCP tool cache stale; withholding stale schemas and refreshing in background"
                    );
                    let mcp_tool_cache = app_state.chat.mcp_tool_cache.clone();
                    let root = project_root.clone();
                    tokio::spawn(async move {
                        let config_signature =
                            crate::domain::mcp::merged_mcp_servers_signature(&root);
                        let schemas = crate::domain::mcp::tool_pool::discover_mcp_tool_schemas(
                            &root,
                            std::time::Duration::from_secs(10),
                        )
                        .await;
                        mcp_tool_cache.lock().await.insert(
                            root,
                            McpToolCache {
                                schemas,
                                cached_at: std::time::Instant::now(),
                                config_signature,
                            },
                        );
                    });
                    (vec![], "stale-refreshing")
                }
                Some((schemas, false, false)) => {
                    tracing::info!(
                        target: "omiga::mcp",
                        cached = schemas.len(),
                        "MCP tool cache stale and config signature changed; ignoring schemas and refreshing in background"
                    );
                    let mcp_tool_cache = app_state.chat.mcp_tool_cache.clone();
                    let root = project_root.clone();
                    tokio::spawn(async move {
                        let config_signature =
                            crate::domain::mcp::merged_mcp_servers_signature(&root);
                        let schemas = crate::domain::mcp::tool_pool::discover_mcp_tool_schemas(
                            &root,
                            std::time::Duration::from_secs(10),
                        )
                        .await;
                        mcp_tool_cache.lock().await.insert(
                            root,
                            McpToolCache {
                                schemas,
                                cached_at: std::time::Instant::now(),
                                config_signature,
                            },
                        );
                    });
                    (vec![], "config-changed")
                }
                None => {
                    tracing::info!(
                        target: "omiga::mcp",
                        "MCP tool cache cold; warming in background without blocking first response"
                    );
                    let mcp_tool_cache = app_state.chat.mcp_tool_cache.clone();
                    let root = project_root.clone();
                    tokio::spawn(async move {
                        let config_signature =
                            crate::domain::mcp::merged_mcp_servers_signature(&root);
                        let schemas = crate::domain::mcp::tool_pool::discover_mcp_tool_schemas(
                            &root,
                            std::time::Duration::from_secs(10),
                        )
                        .await;
                        mcp_tool_cache.lock().await.insert(
                            root,
                            McpToolCache {
                                schemas,
                                cached_at: std::time::Instant::now(),
                                config_signature,
                            },
                        );
                    });
                    (vec![], "cold")
                }
            }
        };
        if trace_mode.is_some() {
            append_preflight_stage_event(
                repo,
                &session_id,
                &user_message_id,
                trace_mode.as_deref(),
                "mcp_tools",
                mcp_stage_started_at.elapsed().as_millis(),
                serde_json::json!({
                    "toolCount": mcp_tools.len(),
                    "cacheStatus": mcp_cache_status,
                }),
            )
            .await;
        }
        let n_mcp_before = mcp_tools.len();
        let mcp_current = crate::domain::mcp::tool_pool::filter_mcp_tool_schemas_for_current_config(
            &project_root,
            mcp_tools,
        );
        if mcp_current.len() < n_mcp_before {
            tracing::info!(
                target: "omiga::mcp",
                before = n_mcp_before,
                after = mcp_current.len(),
                "filtered MCP tool schemas that no longer belong to the effective MCP config"
            );
        }
        let mcp_after_deny = filter_tool_schemas_by_deny_rule_entries(mcp_current, &deny_entries);
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
        let mcp_filtered =
            integrations_config::filter_mcp_tools_by_integrations(mcp_filtered, &integrations_cfg);
        let mut combined: Vec<ToolSchema> = built
            .into_iter()
            .chain(operator_filtered)
            .chain(mcp_filtered)
            .collect();
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
        if trace_mode.is_some() {
            append_preflight_stage_event(
                repo,
                &session_id,
                &user_message_id,
                trace_mode.as_deref(),
                "tool_schemas",
                tool_schema_stage_started_at.elapsed().as_millis(),
                serde_json::json!({
                    "toolCount": combined.len(),
                    "builtinCount": n_builtin_after,
                    "operatorCount": n_operator_after,
                    "mcpCount": n_mcp_after,
                }),
            )
            .await;
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
            content: msg
                .content
                .iter()
                .map(|block| match block {
                    ContentBlock::Text { text } => LlmContent::Text { text: text.clone() },
                    ContentBlock::ToolUse { id, name, input } => {
                        let (name, arguments) = normalize_llm_tool_history_for_model(name, input);
                        LlmContent::ToolUse {
                            id: id.clone(),
                            name,
                            arguments,
                        }
                    }
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
        .collect();

    // Start streaming in background
    let app_clone = app.clone();
    let message_id_clone = message_id.clone();
    let round_id_clone = round_id.clone();
    let session_id_clone = session_id.clone();
    let pending_tools_clone = app_state.chat.pending_tools.clone();
    let ask_user_waiters_clone = app_state.chat.ask_user_waiters.clone();
    let active_rounds_clone = app_state.chat.active_rounds.clone();
    let active_orchestrations_clone = app_state.chat.active_orchestrations.clone();
    let sessions_clone = app_state.chat.sessions.clone();
    let repo_clone = app_state.repo.clone();
    let llm_config_for_spawn = llm_config_for_agent;
    let skill_task_context = request.content.clone();
    let request_text_for_constraints = request.content.clone();
    let project_root_for_constraints = project_root.clone();
    let web_search_api_keys = app_state.chat.web_search_api_keys.lock().await.clone();
    let skill_cache_for_spawn = app_state.skill_cache.clone();
    // 回合开始前预判：短确认类输入可跳过回合结束后的 Output Formatter，加快到 Complete。
    let preflight_skip_turn_summary =
        crate::domain::agents::output_formatter::preflight_skip_turn_summary(&request.content);

    // Prepare orchestration variables for the spawn (scheduler_result stays on the stack for
    // MessageResponse; we clone only when a real multi-agent plan was built).
    let scheduler_for_spawn = if is_plan_mode || !is_explicit_execution_workflow {
        None
    } else {
        scheduler_result.clone()
    };
    let project_root_str_for_spawn = project_root.to_string_lossy().to_string();
    let project_root_for_ralph = project_root.clone();
    let project_root_for_autopilot = project_root.clone();
    let project_root_for_team = project_root.clone();
    let is_team_mode_for_spawn = is_team_keyword_route;
    let is_ralph_mode_for_spawn = is_ralph_keyword_route;
    let is_autopilot_mode_for_spawn = is_autopilot_keyword_route;
    let is_explicit_execution_workflow_for_spawn = is_explicit_execution_workflow;
    let ralph_env_for_spawn = ralph_runtime_env_label(
        exec_env.as_str(),
        request.ssh_server.as_deref(),
        request.local_venv_type.as_deref().unwrap_or(""),
        request.local_venv_name.as_deref().unwrap_or(""),
    );
    let autopilot_env_for_spawn = ralph_env_for_spawn.clone();
    // Capture effective strategy: LLM planner recommendation takes priority over Auto default.
    let strategy_for_spawn = scheduler_result
        .as_ref()
        .map(|r| r.recommended_strategy)
        .unwrap_or(if is_team_keyword_route {
            crate::domain::agents::scheduler::SchedulingStrategy::Team
        } else {
            crate::domain::agents::scheduler::SchedulingStrategy::Auto
        });

    let round_cancel_spawn = round_cancel.clone();
    tokio::spawn(async move {
        // Keep this round cancellable for the entire assistant/tool loop. Previously the
        // active-round entry was removed after the first model response, so cancelling during
        // tool execution only updated SQLite; the background task kept writing follow-up
        // assistant messages and could interleave another round's tool result sequence.
        let _active_round_cleanup =
            ActiveRoundCleanup::new(active_rounds_clone.clone(), message_id_clone.clone());

        // Give the frontend a short window to receive `message_id` from `send_message`
        // and subscribe to `chat-stream-{message_id}` before the first stream event.
        // Without this, very fast failures/responses can be emitted before the listener
        // exists, leaving the UI stuck on “waiting for response”.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
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

        // Store tool output artifacts inside the project so they're visible to the user.
        // Fall back to app_data_dir only when the project root is unavailable.
        let tool_results_dir = {
            let sessions = sessions_clone.read().await;
            let pr = sessions
                .get(&session_id_clone)
                .map(|r| resolve_session_project_root(&r.session.project_path));
            drop(sessions);
            match pr {
                Some(p) if p.as_os_str() != std::ffi::OsStr::new(".") => p
                    .join(".omiga")
                    .join("tool-results")
                    .join(&session_id_clone),
                _ => tool_results_dir_for_session(&app_clone, &session_id_clone),
            }
        };

        let (
            plan_mode_flag,
            execution_environment,
            ssh_server_rt,
            sandbox_backend_rt,
            local_venv_type_rt,
            local_venv_name_rt,
            env_store_rt,
        ) = {
            let sessions = sessions_clone.read().await;
            let s = sessions.get(&session_id_clone);
            (
                s.map(|x| x.plan_mode.clone()),
                s.map(|x| x.execution_environment.clone())
                    .unwrap_or_else(|| "local".to_string()),
                s.and_then(|x| x.ssh_server.clone()),
                s.map(|x| x.sandbox_backend.clone())
                    .unwrap_or_else(|| "docker".to_string()),
                s.map(|x| x.local_venv_type.clone()).unwrap_or_default(),
                s.map(|x| x.local_venv_name.clone()).unwrap_or_default(),
                s.map(|x| x.env_store.clone())
                    .unwrap_or_else(crate::domain::tools::env_store::EnvStore::new),
            )
        };

        let agent_runtime = AgentLlmRuntime {
            llm_config: llm_config_for_spawn.clone(),
            round_id: round_id_clone.clone(),
            cancel_flag: cancel_flag.clone(),
            pending_tools: pending_tools_clone.clone(),
            repo: repo_clone.clone(),
            plan_mode_flag,
            allow_nested_agent: env_allow_nested_agent(),
            round_cancel: round_cancel_spawn.clone(),
            execution_environment,
            ssh_server: ssh_server_rt,
            sandbox_backend: sandbox_backend_rt,
            local_venv_type: local_venv_type_rt,
            local_venv_name: local_venv_name_rt,
            env_store: env_store_rt,
            runtime_constraints_config: resolved_runtime_constraints.clone(),
        };

        let mut turn_token_usage: Option<crate::llm::TokenUsage> = None;
        let constraint_harness =
            RuntimeConstraintHarness::from_config(agent_runtime.runtime_constraints_config.clone());
        let mut constraint_state = RuntimeConstraintState::default();
        let (initial_llm_messages, initial_notices) = augment_llm_messages_with_runtime_constraints(
            &llm_messages,
            &constraint_harness,
            &mut constraint_state,
            &request_text_for_constraints,
            &project_root_for_constraints,
            !tools.is_empty(),
            false,
        );
        emit_runtime_constraint_metadata(
            &app_clone,
            &repo_clone,
            &session_id_clone,
            &round_id_clone,
            &message_id_clone,
            "runtime_constraints.config",
            serde_json::json!({
                "enabled": agent_runtime.runtime_constraints_config.enabled,
                "buffer_responses": agent_runtime.runtime_constraints_config.buffer_responses,
                "policy_pack": agent_runtime.runtime_constraints_config.policy_pack,
                "registry": constraint_harness.registry().into_iter().map(|m| serde_json::json!({
                    "id": m.id,
                    "severity": m.severity,
                    "enabled": m.enabled,
                })).collect::<Vec<_>>(),
            }),
        )
        .await;
        if !initial_notices.is_empty() {
            emit_runtime_constraint_metadata(
                &app_clone,
                &repo_clone,
                &session_id_clone,
                &round_id_clone,
                &message_id_clone,
                "runtime_constraints.notices",
                serde_json::json!({ "ids": initial_notices }),
            )
            .await;
        }

        update_ralph_phase_if_needed(
            mode_lifecycle_context!(
                is_ralph_mode_for_spawn,
                &sessions_clone,
                &repo_clone,
                &project_root_for_ralph,
                &session_id_clone,
                ralph_env_for_spawn.clone(),
                Some(&round_id_clone),
            ),
            crate::domain::ralph_state::RalphPhase::EnvCheck,
        )
        .await;
        update_autopilot_phase_if_needed(
            mode_lifecycle_context!(
                is_autopilot_mode_for_spawn,
                &sessions_clone,
                &repo_clone,
                &project_root_for_autopilot,
                &session_id_clone,
                autopilot_env_for_spawn.clone(),
                Some(&round_id_clone),
            ),
            crate::domain::autopilot_state::AutopilotPhase::Design,
        )
        .await;

        // Stream the response with cancellation support
        let (
            mut pending_tool_calls,
            assistant_text,
            assistant_reasoning,
            was_cancelled,
            usage_first,
        ) = match stream_llm_response_with_cancel(StreamLlmRequest {
            client: client.as_ref(),
            app: &app_clone,
            message_id: &message_id_clone,
            round_id: &round_id_clone,
            messages: &initial_llm_messages,
            tools: &tools,
            emit_text_chunks: !agent_runtime.runtime_constraints_config.buffer_responses,
            pending_tools: &pending_tools_clone,
            cancel_flag: &cancel_flag,
            repo: repo_clone.clone(),
        })
        .await
        {
            Ok(result) => result,
            Err(e) => {
                let repo = &*repo_clone;
                let _ = repo
                    .cancel_round(&round_id_clone, Some(&e.to_string()))
                    .await;
                fail_ralph_turn_if_needed(
                    mode_lifecycle_context!(
                        is_ralph_mode_for_spawn,
                        &sessions_clone,
                        &repo_clone,
                        &project_root_for_ralph,
                        &session_id_clone,
                        ralph_env_for_spawn.clone(),
                        Some(&round_id_clone),
                    ),
                    crate::domain::ralph_state::RalphPhase::EnvCheck,
                    &e.to_string(),
                )
                .await;
                fail_autopilot_turn_if_needed(
                    mode_lifecycle_context!(
                        is_autopilot_mode_for_spawn,
                        &sessions_clone,
                        &repo_clone,
                        &project_root_for_autopilot,
                        &session_id_clone,
                        autopilot_env_for_spawn.clone(),
                        Some(&round_id_clone),
                    ),
                    crate::domain::autopilot_state::AutopilotPhase::Design,
                    &e.to_string(),
                )
                .await;
                fail_team_turn_if_needed(
                    is_team_mode_for_spawn,
                    &repo_clone,
                    &project_root_for_team,
                    &session_id_clone,
                    &e.to_string(),
                    Some(&round_id_clone),
                )
                .await;

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

        if was_cancelled {
            persist_session_tool_state(&sessions_clone, &repo_clone, &session_id_clone).await;
            update_ralph_phase_if_needed(
                mode_lifecycle_context!(
                    is_ralph_mode_for_spawn,
                    &sessions_clone,
                    &repo_clone,
                    &project_root_for_ralph,
                    &session_id_clone,
                    ralph_env_for_spawn.clone(),
                    Some(&round_id_clone),
                ),
                crate::domain::ralph_state::RalphPhase::Executing,
            )
            .await;
            update_autopilot_phase_if_needed(
                mode_lifecycle_context!(
                    is_autopilot_mode_for_spawn,
                    &sessions_clone,
                    &repo_clone,
                    &project_root_for_autopilot,
                    &session_id_clone,
                    autopilot_env_for_spawn.clone(),
                    Some(&round_id_clone),
                ),
                crate::domain::autopilot_state::AutopilotPhase::Qa,
            )
            .await;
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

        let pending_tool_names: Vec<String> = pending_tool_calls
            .iter()
            .map(|(_, name, _)| name.clone())
            .collect();
        if let Some(block) = constraint_harness.tool_gate(
            &ToolConstraintContext {
                request_text: &request_text_for_constraints,
                assistant_text: &assistant_text,
                pending_tool_names: &pending_tool_names,
                is_subagent: false,
            },
            &constraint_state,
        ) {
            constraint_state.mark_clarification_requested();
            emit_runtime_constraint_metadata(
                &app_clone,
                &repo_clone,
                &session_id_clone,
                &round_id_clone,
                &message_id_clone,
                "runtime_constraints.gate",
                serde_json::json!({
                    "id": block.id,
                    "assistant_response": block.assistant_response,
                }),
            )
            .await;
            handle_runtime_constraint_block_main(RuntimeConstraintBlockRequest {
                app: &app_clone,
                client: client.as_ref(),
                repo: repo_clone.clone(),
                sessions: &sessions_clone,
                session_id: &session_id_clone,
                round_id: &round_id_clone,
                message_id: &message_id_clone,
                user_message: &request_text_for_constraints,
                assistant_text: &assistant_text,
                assistant_reasoning: &assistant_reasoning,
                tool_calls: &pending_tool_calls,
                block: &block,
                tool_results_dir: &tool_results_dir,
                ask_user_waiters: ask_user_waiters_clone.clone(),
                cancel_flag: cancel_flag.clone(),
                preflight_skip_turn_summary,
                turn_token_usage: &turn_token_usage,
                provider_name: &llm_config_for_spawn.provider.to_string(),
                persist_original_assistant: true,
            })
            .await;
            return;
        }

        if pending_tool_calls.is_empty() {
            let no_pending_tool_names: Vec<String> = Vec::new();
            if let Some(block) = constraint_harness.post_response_block(
                &crate::domain::runtime_constraints::PostResponseConstraintContext {
                    request_text: &request_text_for_constraints,
                    assistant_text: &assistant_text,
                    pending_tool_names: &no_pending_tool_names,
                    is_subagent: false,
                },
                &constraint_state,
            ) {
                constraint_state.mark_clarification_requested();
                emit_runtime_constraint_metadata(
                    &app_clone,
                    &repo_clone,
                    &session_id_clone,
                    &round_id_clone,
                    &message_id_clone,
                    "runtime_constraints.post_response_block",
                    serde_json::json!({
                        "id": block.id,
                        "assistant_response": block.assistant_response,
                    }),
                )
                .await;
                handle_runtime_constraint_block_main(RuntimeConstraintBlockRequest {
                    app: &app_clone,
                    client: client.as_ref(),
                    repo: repo_clone.clone(),
                    sessions: &sessions_clone,
                    session_id: &session_id_clone,
                    round_id: &round_id_clone,
                    message_id: &message_id_clone,
                    user_message: &request_text_for_constraints,
                    assistant_text: &assistant_text,
                    assistant_reasoning: &assistant_reasoning,
                    tool_calls: &pending_tool_calls,
                    block: &block,
                    tool_results_dir: &tool_results_dir,
                    ask_user_waiters: ask_user_waiters_clone.clone(),
                    cancel_flag: cancel_flag.clone(),
                    preflight_skip_turn_summary,
                    turn_token_usage: &turn_token_usage,
                    provider_name: &llm_config_for_spawn.provider.to_string(),
                    persist_original_assistant: false,
                })
                .await;
                return;
            }
        }

        if agent_runtime.runtime_constraints_config.buffer_responses
            && !pending_tool_calls.is_empty()
        {
            emit_buffered_assistant_text(&app_clone, &message_id_clone, &assistant_text);
            emit_runtime_constraint_metadata(
                &app_clone,
                &repo_clone,
                &session_id_clone,
                &round_id_clone,
                &message_id_clone,
                "runtime_constraints.commit",
                serde_json::json!({
                    "mode": "buffered",
                    "phase": "pre_tool",
                }),
            )
            .await;
        }

        // First assistant turn: persist with tool_calls JSON for reload
        let assistant_msg_id = uuid::Uuid::new_v4().to_string();
        let tool_calls_json = tool_calls_json_opt(&pending_tool_calls);
        let reasoning_save =
            (!assistant_reasoning.is_empty()).then_some(assistant_reasoning.as_str());
        {
            let repo = &*repo_clone;
            if let Err(e) = repo
                .save_message(NewMessageRecord {
                    id: &assistant_msg_id,
                    session_id: &session_id_clone,
                    role: "assistant",
                    content: &assistant_text,
                    tool_calls: tool_calls_json.as_deref(),
                    tool_call_id: None,
                    token_usage_json: None,
                    reasoning_content: reasoning_save,
                    follow_up_suggestions_json: None,
                    turn_summary: None,
                })
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

        update_ralph_phase_if_needed(
            mode_lifecycle_context!(
                is_ralph_mode_for_spawn,
                &sessions_clone,
                &repo_clone,
                &project_root_for_ralph,
                &session_id_clone,
                ralph_env_for_spawn.clone(),
                Some(&round_id_clone),
            ),
            if pending_tool_calls.is_empty() {
                crate::domain::ralph_state::RalphPhase::Verifying
            } else {
                crate::domain::ralph_state::RalphPhase::Executing
            },
        )
        .await;
        update_autopilot_phase_if_needed(
            mode_lifecycle_context!(
                is_autopilot_mode_for_spawn,
                &sessions_clone,
                &repo_clone,
                &project_root_for_autopilot,
                &session_id_clone,
                autopilot_env_for_spawn.clone(),
                Some(&round_id_clone),
            ),
            if pending_tool_calls.is_empty() {
                crate::domain::autopilot_state::AutopilotPhase::Validation
            } else {
                crate::domain::autopilot_state::AutopilotPhase::Implementation
            },
        )
        .await;

        let mut last_assistant_id = assistant_msg_id.clone();

        if pending_tool_calls.is_empty() {
            let no_pending_tool_names: Vec<String> = Vec::new();
            if let Some(action) = constraint_harness.post_response_action(
                &crate::domain::runtime_constraints::PostResponseConstraintContext {
                    request_text: &request_text_for_constraints,
                    assistant_text: &assistant_text,
                    pending_tool_names: &no_pending_tool_names,
                    is_subagent: false,
                },
                &constraint_state,
            ) {
                constraint_state.mark_post_action_attempted(action.id);
                emit_runtime_constraint_metadata(
                    &app_clone,
                    &repo_clone,
                    &session_id_clone,
                    &round_id_clone,
                    &message_id_clone,
                    "runtime_constraint_retry",
                    serde_json::json!({ "id": action.id }),
                )
                .await;
                let updated_messages = {
                    let sessions = sessions_clone.read().await;
                    sessions
                        .get(&session_id_clone)
                        .map(|r| SessionCodec::to_api_messages(&r.session.messages))
                        .unwrap_or_default()
                };
                let updated_llm_messages = api_messages_to_llm(&updated_messages);
                match run_post_response_retry_text_only(PostResponseRetryRequest {
                    client: client.as_ref(),
                    app: &app_clone,
                    message_id: &message_id_clone,
                    round_id: &round_id_clone,
                    base_messages: &updated_llm_messages,
                    instruction: &action.instruction,
                    pending_tools: &pending_tools_clone,
                    cancel_flag: &cancel_flag,
                    repo: repo_clone.clone(),
                })
                .await
                {
                    Ok((retry_text, retry_reasoning, usage_retry))
                        if !retry_text.trim().is_empty() =>
                    {
                        merge_turn_token_usage(&mut turn_token_usage, usage_retry);
                        let retry_id = uuid::Uuid::new_v4().to_string();
                        let retry_reasoning_save =
                            (!retry_reasoning.is_empty()).then_some(retry_reasoning.as_str());
                        if let Err(e) = repo_clone
                            .save_message(NewMessageRecord {
                                id: &retry_id,
                                session_id: &session_id_clone,
                                role: "assistant",
                                content: &retry_text,
                                tool_calls: None,
                                tool_call_id: None,
                                token_usage_json: None,
                                reasoning_content: retry_reasoning_save,
                                follow_up_suggestions_json: None,
                                turn_summary: None,
                            })
                            .await
                        {
                            tracing::warn!("Failed to save runtime retry assistant message: {}", e);
                        }
                        {
                            let mut sessions = sessions_clone.write().await;
                            if let Some(runtime) = sessions.get_mut(&session_id_clone) {
                                let rc =
                                    (!retry_reasoning.is_empty()).then(|| retry_reasoning.clone());
                                runtime.session.add_assistant_message_with_tools(
                                    &retry_text,
                                    None,
                                    rc,
                                );
                            }
                        }
                        final_reply_for_follow_up = retry_text.clone();
                        last_assistant_id = retry_id;
                    }
                    Ok(_) => {}
                    Err(e) => {
                        tracing::warn!("Runtime post-response retry failed: {}", e);
                    }
                }
            }

            if agent_runtime.runtime_constraints_config.buffer_responses {
                emit_buffered_assistant_text(
                    &app_clone,
                    &message_id_clone,
                    &final_reply_for_follow_up,
                );
                emit_runtime_constraint_metadata(
                    &app_clone,
                    &repo_clone,
                    &session_id_clone,
                    &round_id_clone,
                    &message_id_clone,
                    "runtime_constraints.commit",
                    serde_json::json!({
                        "mode": "buffered",
                        "phase": "final",
                    }),
                )
                .await;
            }

            persist_session_tool_state(&sessions_clone, &repo_clone, &session_id_clone).await;
            update_ralph_phase_if_needed(
                mode_lifecycle_context!(
                    is_ralph_mode_for_spawn,
                    &sessions_clone,
                    &repo_clone,
                    &project_root_for_ralph,
                    &session_id_clone,
                    ralph_env_for_spawn.clone(),
                    Some(&round_id_clone),
                ),
                crate::domain::ralph_state::RalphPhase::Verifying,
            )
            .await;
            update_autopilot_phase_if_needed(
                mode_lifecycle_context!(
                    is_autopilot_mode_for_spawn,
                    &sessions_clone,
                    &repo_clone,
                    &project_root_for_autopilot,
                    &session_id_clone,
                    autopilot_env_for_spawn.clone(),
                    Some(&round_id_clone),
                ),
                crate::domain::autopilot_state::AutopilotPhase::Validation,
            )
            .await;

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
            complete_ralph_turn_if_needed(
                is_ralph_mode_for_spawn,
                &sessions_clone,
                &repo_clone,
                &project_root_for_ralph,
                &session_id_clone,
                Some(&round_id_clone),
            )
            .await;
            complete_autopilot_turn_if_needed(
                is_autopilot_mode_for_spawn,
                &sessions_clone,
                &repo_clone,
                &project_root_for_autopilot,
                &session_id_clone,
                Some(&round_id_clone),
            )
            .await;
            complete_team_turn_if_needed(
                is_team_mode_for_spawn,
                &repo_clone,
                &project_root_for_team,
                &session_id_clone,
                Some(&round_id_clone),
            )
            .await;
            spawn_memory_sync(MemorySyncRequest {
                app: &app_clone,
                sessions: &sessions_clone,
                repo: &repo_clone,
                session_id: &session_id_clone,
                client: client.as_ref(),
                user_message: &request_text_for_constraints,
                assistant_reply: &final_reply_for_follow_up,
                allow_long_term_promotion: true,
            });
            emit_post_turn_meta_then_complete(PostTurnCompletionRequest {
                app: &app_clone,
                session_id: &session_id_clone,
                stream_message_id: &message_id_clone,
                assistant_message_id: &last_assistant_id,
                client: client.as_ref(),
                final_reply: &final_reply_for_follow_up,
                skip_summary: preflight_skip_turn_summary,
                suggestions_reply: &final_reply_for_follow_up,
                repo: repo_clone.clone(),
            })
            .await;
            spawn_chat_indexing(&app_clone, &sessions_clone, &repo_clone, &session_id_clone);
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
                let todos = sessions.get(&session_id_clone).map(|r| r.todos.clone());
                let agent_tasks = sessions
                    .get(&session_id_clone)
                    .map(|r| r.agent_tasks.clone());
                (project_root, todos, agent_tasks)
            };

            constraint_state.record_tool_names(
                pending_tool_calls
                    .iter()
                    .map(|(_, tool_name, _)| tool_name.as_str()),
            );

            let mut tool_results = execute_tool_calls(ToolExecutionRequest {
                tool_calls: &pending_tool_calls,
                app: &app_clone,
                message_id: &message_id_clone,
                session_id: &session_id_clone,
                tool_results_dir: &tool_results_dir,
                project_root: &project_root,
                session_todos: todos_for_tools,
                session_agent_tasks: agent_tasks_for_tools,
                agent_runtime: Some(&agent_runtime),
                subagent_depth: 0,
                skill_task_context: Some(skill_task_context.as_str()),
                web_search_api_keys: web_search_api_keys.clone(),
                skill_cache: skill_cache_for_spawn.clone(),
                execution_environment: agent_runtime.execution_environment.clone(),
                ssh_server: agent_runtime.ssh_server.clone(),
                sandbox_backend: agent_runtime.sandbox_backend.clone(),
                local_venv_type: agent_runtime.local_venv_type.clone(),
                local_venv_name: agent_runtime.local_venv_name.clone(),
                env_store: agent_runtime.env_store.clone(),
                computer_use_enabled: computer_use_mode.is_enabled(),
            })
            .await;

            // Preserve the provider-required assistant(tool_calls) -> tool(...) sequence even
            // when a cancellation-aware tool exits without returning a row for every call.
            let returned_tool_ids: HashSet<String> = tool_results
                .iter()
                .map(|(tool_use_id, _, _)| tool_use_id.clone())
                .collect();
            for (tool_use_id, tool_name, _) in &pending_tool_calls {
                if !returned_tool_ids.contains(tool_use_id) {
                    tool_results.push((
                        tool_use_id.clone(),
                        format!("Tool `{tool_name}` was cancelled before it returned a result."),
                        true,
                    ));
                }
            }

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
            if *cancel_flag.read().await || round_cancel_spawn.is_cancelled() {
                let _ = repo_clone
                    .cancel_round(&round_id_clone, Some("User cancelled"))
                    .await;
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
            update_ralph_phase_if_needed(
                mode_lifecycle_context!(
                    is_ralph_mode_for_spawn,
                    &sessions_clone,
                    &repo_clone,
                    &project_root_for_ralph,
                    &session_id_clone,
                    ralph_env_for_spawn.clone(),
                    Some(&round_id_clone),
                ),
                crate::domain::ralph_state::RalphPhase::Executing,
            )
            .await;
            let autopilot_state = update_autopilot_phase_if_needed(
                mode_lifecycle_context!(
                    is_autopilot_mode_for_spawn,
                    &sessions_clone,
                    &repo_clone,
                    &project_root_for_autopilot,
                    &session_id_clone,
                    autopilot_env_for_spawn.clone(),
                    Some(&round_id_clone),
                ),
                crate::domain::autopilot_state::AutopilotPhase::Qa,
            )
            .await;

            if let Some(state) = autopilot_state {
                if state.qa_limit_reached() {
                    let stop_text = format!(
                        "Autopilot stopped after exceeding max argumentation cycles ({}/{}). Last known goal: {}",
                        state.qa_cycles, state.max_qa_cycles, state.goal
                    );
                    let stop_msg_id = uuid::Uuid::new_v4().to_string();
                    let reasoning_save = None::<&str>;
                    if let Err(e) = repo_clone
                        .save_message(NewMessageRecord {
                            id: &stop_msg_id,
                            session_id: &session_id_clone,
                            role: "assistant",
                            content: &stop_text,
                            tool_calls: None,
                            tool_call_id: None,
                            token_usage_json: None,
                            reasoning_content: reasoning_save,
                            follow_up_suggestions_json: None,
                            turn_summary: None,
                        })
                        .await
                    {
                        tracing::warn!(
                            target: "omiga::autopilot",
                            "Failed to save autopilot argumentation limit stop message: {}",
                            e
                        );
                    }
                    {
                        let mut sessions = sessions_clone.write().await;
                        if let Some(runtime) = sessions.get_mut(&session_id_clone) {
                            runtime
                                .session
                                .add_assistant_message_with_tools(&stop_text, None, None);
                        }
                    }
                    persist_session_tool_state(&sessions_clone, &repo_clone, &session_id_clone)
                        .await;
                    fail_autopilot_turn_if_needed(
                        mode_lifecycle_context!(
                            true,
                            &sessions_clone,
                            &repo_clone,
                            &project_root_for_autopilot,
                            &session_id_clone,
                            autopilot_env_for_spawn.clone(),
                            Some(&round_id_clone),
                        ),
                        crate::domain::autopilot_state::AutopilotPhase::Qa,
                        &stop_text,
                    )
                    .await;
                    {
                        let repo = &*repo_clone;
                        let _ = repo
                            .complete_round(&round_id_clone, Some(&stop_msg_id))
                            .await;
                    }
                    persist_and_emit_turn_token_usage(
                        &app_clone,
                        &repo_clone,
                        &stop_msg_id,
                        &message_id_clone,
                        &turn_token_usage,
                        &llm_config_for_spawn.provider.to_string(),
                    )
                    .await;
                    spawn_memory_sync(MemorySyncRequest {
                        app: &app_clone,
                        sessions: &sessions_clone,
                        repo: &repo_clone,
                        session_id: &session_id_clone,
                        client: client.as_ref(),
                        user_message: &request_text_for_constraints,
                        assistant_reply: &stop_text,
                        allow_long_term_promotion: true,
                    });
                    emit_post_turn_meta_then_complete(PostTurnCompletionRequest {
                        app: &app_clone,
                        session_id: &session_id_clone,
                        stream_message_id: &message_id_clone,
                        assistant_message_id: &stop_msg_id,
                        client: client.as_ref(),
                        final_reply: &stop_text,
                        skip_summary: preflight_skip_turn_summary,
                        suggestions_reply: &stop_text,
                        repo: repo_clone.clone(),
                    })
                    .await;
                    return;
                }
            }

            // Shrink history before the next model call when tool rounds push toward context limits.
            {
                let mut sessions = sessions_clone.write().await;
                if let Some(runtime) = sessions.get_mut(&session_id_clone) {
                    let repo = &*repo_clone;
                    if let Some(removed_messages) =
                        crate::domain::auto_compact::preview_removed_messages_for_compaction(
                            &runtime.session.messages,
                            &llm_config_for_spawn,
                            !tools.is_empty(),
                        )
                    {
                        let op_id = format!("memory-precompact-{}", uuid::Uuid::new_v4());
                        emit_activity_operation(
                            &app_clone,
                            &session_id_clone,
                            &op_id,
                            "压缩前摘要",
                            "running",
                            Some(format!(
                                "准备提炼 {} 条即将压缩的消息",
                                removed_messages.len()
                            )),
                        );
                        match tokio::time::timeout(
                            std::time::Duration::from_secs(3),
                            crate::domain::memory::working_memory::prepare_for_auto_compact(
                                &repo_clone,
                                &session_id_clone,
                                &removed_messages,
                            ),
                        )
                        .await
                        {
                            Ok(Err(e)) => {
                                tracing::warn!(
                                    target: "omiga::working_memory",
                                    "tool-loop pre-compact summary failed: {}",
                                    e
                                );
                                emit_activity_operation(
                                    &app_clone,
                                    &session_id_clone,
                                    &op_id,
                                    "压缩前摘要",
                                    "error",
                                    Some(e.to_string()),
                                );
                            }
                            Ok(Ok(compact_state)) => {
                                emit_activity_operation(
                                    &app_clone,
                                    &session_id_clone,
                                    &op_id,
                                    "压缩前摘要",
                                    "done",
                                    Some("已提炼即将被压缩的上下文".to_string()),
                                );
                                // Compression is a semantic trigger for session summary.
                                //
                                // Important: this block already holds the sessions write lock.
                                // Do not re-acquire `sessions_clone.read()` here; tokio RwLock is
                                // not re-entrant and that self-read deadlocks the tool loop right
                                // after the UI shows “压缩前摘要” as the last successful step.
                                let project_root_for_compact =
                                    resolve_session_project_root(&runtime.session.project_path);
                                if let Ok(cfg) = crate::domain::memory::load_resolved_config(
                                    &project_root_for_compact,
                                )
                                .await
                                {
                                    let lt_path = cfg.long_term_path(&project_root_for_compact);
                                    crate::commands::chat::turn::archive_on_compact(
                                        &app_clone,
                                        &session_id_clone,
                                        &lt_path,
                                        &compact_state,
                                    )
                                    .await;
                                }
                            }
                            Err(_) => {
                                tracing::warn!(
                                    target: "omiga::working_memory",
                                    "tool-loop pre-compact summary timed out; continuing without blocking chat"
                                );
                                emit_activity_operation(
                                    &app_clone,
                                    &session_id_clone,
                                    &op_id,
                                    "压缩前摘要",
                                    "error",
                                    Some("prepare_for_auto_compact timed out".to_string()),
                                );
                            }
                        }
                    }
                    match tokio::time::timeout(
                        std::time::Duration::from_secs(5),
                        crate::domain::auto_compact::compact_session_and_persist(
                            repo,
                            &session_id_clone,
                            &mut runtime.session,
                            &llm_config_for_spawn,
                            !tools.is_empty(),
                            "",
                        ),
                    )
                    .await
                    {
                        Ok(Ok(_)) => {}
                        Ok(Err(e)) => {
                            tracing::warn!(
                                target: "omiga::auto_compact",
                                "tool-loop auto-compact failed: {}",
                                e
                            );
                        }
                        Err(_) => {
                            tracing::warn!(
                                target: "omiga::auto_compact",
                                "tool-loop auto-compact timed out; continuing with current transcript"
                            );
                        }
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
            let (constrained_followup_messages, followup_notices) =
                augment_llm_messages_with_runtime_constraints(
                    &updated_llm_messages,
                    &constraint_harness,
                    &mut constraint_state,
                    &request_text_for_constraints,
                    &project_root_for_constraints,
                    !tools.is_empty(),
                    false,
                );
            if !followup_notices.is_empty() {
                emit_runtime_constraint_metadata(
                    &app_clone,
                    &repo_clone,
                    &session_id_clone,
                    &round_id_clone,
                    &message_id_clone,
                    "runtime_constraints.notices",
                    serde_json::json!({ "ids": followup_notices }),
                )
                .await;
            }

            let (next_tools, next_text, next_reasoning, follow_cancelled, usage_next) =
                match stream_llm_response_with_cancel(StreamLlmRequest {
                    client: client.as_ref(),
                    app: &app_clone,
                    message_id: &message_id_clone,
                    round_id: &round_id_clone,
                    messages: &constrained_followup_messages,
                    tools: &tools,
                    emit_text_chunks: !agent_runtime.runtime_constraints_config.buffer_responses,
                    pending_tools: &pending_tools_clone,
                    cancel_flag: &cancel_flag,
                    repo: repo_clone.clone(),
                })
                .await
                {
                    Ok(r) => r,
                    Err(e) => {
                        let repo = &*repo_clone;
                        let _ = repo
                            .cancel_round(&round_id_clone, Some(&e.to_string()))
                            .await;
                        fail_ralph_turn_if_needed(
                            mode_lifecycle_context!(
                                is_ralph_mode_for_spawn,
                                &sessions_clone,
                                &repo_clone,
                                &project_root_for_ralph,
                                &session_id_clone,
                                ralph_env_for_spawn.clone(),
                                Some(&round_id_clone),
                            ),
                            crate::domain::ralph_state::RalphPhase::Executing,
                            &e.to_string(),
                        )
                        .await;
                        fail_autopilot_turn_if_needed(
                            mode_lifecycle_context!(
                                is_autopilot_mode_for_spawn,
                                &sessions_clone,
                                &repo_clone,
                                &project_root_for_autopilot,
                                &session_id_clone,
                                autopilot_env_for_spawn.clone(),
                                Some(&round_id_clone),
                            ),
                            crate::domain::autopilot_state::AutopilotPhase::Qa,
                            &e.to_string(),
                        )
                        .await;
                        fail_team_turn_if_needed(
                            is_team_mode_for_spawn,
                            &repo_clone,
                            &project_root_for_team,
                            &session_id_clone,
                            &e.to_string(),
                            Some(&round_id_clone),
                        )
                        .await;
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

            let next_tool_names: Vec<String> =
                next_tools.iter().map(|(_, name, _)| name.clone()).collect();
            if let Some(block) = constraint_harness.tool_gate(
                &ToolConstraintContext {
                    request_text: &request_text_for_constraints,
                    assistant_text: &next_text,
                    pending_tool_names: &next_tool_names,
                    is_subagent: false,
                },
                &constraint_state,
            ) {
                constraint_state.mark_clarification_requested();
                emit_runtime_constraint_metadata(
                    &app_clone,
                    &repo_clone,
                    &session_id_clone,
                    &round_id_clone,
                    &message_id_clone,
                    "runtime_constraints.gate",
                    serde_json::json!({
                        "id": block.id,
                        "assistant_response": block.assistant_response,
                    }),
                )
                .await;
                handle_runtime_constraint_block_main(RuntimeConstraintBlockRequest {
                    app: &app_clone,
                    client: client.as_ref(),
                    repo: repo_clone.clone(),
                    sessions: &sessions_clone,
                    session_id: &session_id_clone,
                    round_id: &round_id_clone,
                    message_id: &message_id_clone,
                    user_message: &request_text_for_constraints,
                    assistant_text: &next_text,
                    assistant_reasoning: &next_reasoning,
                    tool_calls: &next_tools,
                    block: &block,
                    tool_results_dir: &tool_results_dir,
                    ask_user_waiters: ask_user_waiters_clone.clone(),
                    cancel_flag: cancel_flag.clone(),
                    preflight_skip_turn_summary,
                    turn_token_usage: &turn_token_usage,
                    provider_name: &llm_config_for_spawn.provider.to_string(),
                    persist_original_assistant: true,
                })
                .await;
                return;
            }

            if next_tools.is_empty() {
                let no_pending_tool_names: Vec<String> = Vec::new();
                if let Some(block) = constraint_harness.post_response_block(
                    &crate::domain::runtime_constraints::PostResponseConstraintContext {
                        request_text: &request_text_for_constraints,
                        assistant_text: &next_text,
                        pending_tool_names: &no_pending_tool_names,
                        is_subagent: false,
                    },
                    &constraint_state,
                ) {
                    constraint_state.mark_clarification_requested();
                    emit_runtime_constraint_metadata(
                        &app_clone,
                        &repo_clone,
                        &session_id_clone,
                        &round_id_clone,
                        &message_id_clone,
                        "runtime_constraints.post_response_block",
                        serde_json::json!({
                            "id": block.id,
                            "assistant_response": block.assistant_response,
                        }),
                    )
                    .await;
                    handle_runtime_constraint_block_main(RuntimeConstraintBlockRequest {
                        app: &app_clone,
                        client: client.as_ref(),
                        repo: repo_clone.clone(),
                        sessions: &sessions_clone,
                        session_id: &session_id_clone,
                        round_id: &round_id_clone,
                        message_id: &message_id_clone,
                        user_message: &request_text_for_constraints,
                        assistant_text: &next_text,
                        assistant_reasoning: &next_reasoning,
                        tool_calls: &next_tools,
                        block: &block,
                        tool_results_dir: &tool_results_dir,
                        ask_user_waiters: ask_user_waiters_clone.clone(),
                        cancel_flag: cancel_flag.clone(),
                        preflight_skip_turn_summary,
                        turn_token_usage: &turn_token_usage,
                        provider_name: &llm_config_for_spawn.provider.to_string(),
                        persist_original_assistant: false,
                    })
                    .await;
                    return;
                }
            }

            if agent_runtime.runtime_constraints_config.buffer_responses && !next_tools.is_empty() {
                emit_buffered_assistant_text(&app_clone, &message_id_clone, &next_text);
                emit_runtime_constraint_metadata(
                    &app_clone,
                    &repo_clone,
                    &session_id_clone,
                    &round_id_clone,
                    &message_id_clone,
                    "runtime_constraints.commit",
                    serde_json::json!({
                        "mode": "buffered",
                        "phase": "pre_tool",
                    }),
                )
                .await;
            }

            if follow_cancelled {
                persist_session_tool_state(&sessions_clone, &repo_clone, &session_id_clone).await;
                update_ralph_phase_if_needed(
                    mode_lifecycle_context!(
                        is_ralph_mode_for_spawn,
                        &sessions_clone,
                        &repo_clone,
                        &project_root_for_ralph,
                        &session_id_clone,
                        ralph_env_for_spawn.clone(),
                        Some(&round_id_clone),
                    ),
                    crate::domain::ralph_state::RalphPhase::Executing,
                )
                .await;
                update_autopilot_phase_if_needed(
                    mode_lifecycle_context!(
                        is_autopilot_mode_for_spawn,
                        &sessions_clone,
                        &repo_clone,
                        &project_root_for_autopilot,
                        &session_id_clone,
                        autopilot_env_for_spawn.clone(),
                        Some(&round_id_clone),
                    ),
                    crate::domain::autopilot_state::AutopilotPhase::Qa,
                )
                .await;
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
            let next_reasoning_save =
                (!next_reasoning.is_empty()).then_some(next_reasoning.as_str());
            {
                let repo = &*repo_clone;
                if let Err(e) = repo
                    .save_message(NewMessageRecord {
                        id: &next_assistant_id,
                        session_id: &session_id_clone,
                        role: "assistant",
                        content: &next_text,
                        tool_calls: next_tc_json.as_deref(),
                        tool_call_id: None,
                        token_usage_json: None,
                        reasoning_content: next_reasoning_save,
                        follow_up_suggestions_json: None,
                        turn_summary: None,
                    })
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
                    runtime
                        .session
                        .add_assistant_message_with_tools(&next_text, tc, rc);
                }
            }

            last_assistant_id = next_assistant_id.clone();
            pending_tool_calls = next_tools;

            if pending_tool_calls.is_empty() {
                let no_pending_tool_names: Vec<String> = Vec::new();
                if let Some(action) = constraint_harness.post_response_action(
                    &crate::domain::runtime_constraints::PostResponseConstraintContext {
                        request_text: &request_text_for_constraints,
                        assistant_text: &next_text,
                        pending_tool_names: &no_pending_tool_names,
                        is_subagent: false,
                    },
                    &constraint_state,
                ) {
                    constraint_state.mark_post_action_attempted(action.id);
                    emit_runtime_constraint_metadata(
                        &app_clone,
                        &repo_clone,
                        &session_id_clone,
                        &round_id_clone,
                        &message_id_clone,
                        "runtime_constraint_retry",
                        serde_json::json!({ "id": action.id }),
                    )
                    .await;
                    let updated_messages = {
                        let sessions = sessions_clone.read().await;
                        sessions
                            .get(&session_id_clone)
                            .map(|r| SessionCodec::to_api_messages(&r.session.messages))
                            .unwrap_or_default()
                    };
                    let updated_llm_messages = api_messages_to_llm(&updated_messages);
                    match run_post_response_retry_text_only(PostResponseRetryRequest {
                        client: client.as_ref(),
                        app: &app_clone,
                        message_id: &message_id_clone,
                        round_id: &round_id_clone,
                        base_messages: &updated_llm_messages,
                        instruction: &action.instruction,
                        pending_tools: &pending_tools_clone,
                        cancel_flag: &cancel_flag,
                        repo: repo_clone.clone(),
                    })
                    .await
                    {
                        Ok((retry_text, retry_reasoning, usage_retry))
                            if !retry_text.trim().is_empty() =>
                        {
                            merge_turn_token_usage(&mut turn_token_usage, usage_retry);
                            let retry_id = uuid::Uuid::new_v4().to_string();
                            let retry_reasoning_save =
                                (!retry_reasoning.is_empty()).then_some(retry_reasoning.as_str());
                            if let Err(e) = repo_clone
                                .save_message(NewMessageRecord {
                                    id: &retry_id,
                                    session_id: &session_id_clone,
                                    role: "assistant",
                                    content: &retry_text,
                                    tool_calls: None,
                                    tool_call_id: None,
                                    token_usage_json: None,
                                    reasoning_content: retry_reasoning_save,
                                    follow_up_suggestions_json: None,
                                    turn_summary: None,
                                })
                                .await
                            {
                                tracing::warn!(
                                    "Failed to save runtime retry assistant message: {}",
                                    e
                                );
                            }
                            {
                                let mut sessions = sessions_clone.write().await;
                                if let Some(runtime) = sessions.get_mut(&session_id_clone) {
                                    let rc = (!retry_reasoning.is_empty())
                                        .then(|| retry_reasoning.clone());
                                    runtime.session.add_assistant_message_with_tools(
                                        &retry_text,
                                        None,
                                        rc,
                                    );
                                }
                            }
                            final_reply_for_follow_up = retry_text.clone();
                            last_assistant_id = retry_id;
                        }
                        Ok(_) => {}
                        Err(e) => {
                            tracing::warn!("Runtime post-response retry failed: {}", e);
                        }
                    }
                }

                if agent_runtime.runtime_constraints_config.buffer_responses {
                    emit_buffered_assistant_text(
                        &app_clone,
                        &message_id_clone,
                        &final_reply_for_follow_up,
                    );
                    emit_runtime_constraint_metadata(
                        &app_clone,
                        &repo_clone,
                        &session_id_clone,
                        &round_id_clone,
                        &message_id_clone,
                        "runtime_constraints.commit",
                        serde_json::json!({
                            "mode": "buffered",
                            "phase": "final",
                        }),
                    )
                    .await;
                }

                persist_session_tool_state(&sessions_clone, &repo_clone, &session_id_clone).await;
                update_ralph_phase_if_needed(
                    mode_lifecycle_context!(
                        is_ralph_mode_for_spawn,
                        &sessions_clone,
                        &repo_clone,
                        &project_root_for_ralph,
                        &session_id_clone,
                        ralph_env_for_spawn.clone(),
                        Some(&round_id_clone),
                    ),
                    crate::domain::ralph_state::RalphPhase::Verifying,
                )
                .await;
                update_autopilot_phase_if_needed(
                    mode_lifecycle_context!(
                        is_autopilot_mode_for_spawn,
                        &sessions_clone,
                        &repo_clone,
                        &project_root_for_autopilot,
                        &session_id_clone,
                        autopilot_env_for_spawn.clone(),
                        Some(&round_id_clone),
                    ),
                    crate::domain::autopilot_state::AutopilotPhase::Validation,
                )
                .await;

                // Index chat to implicit memory
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
                complete_ralph_turn_if_needed(
                    is_ralph_mode_for_spawn,
                    &sessions_clone,
                    &repo_clone,
                    &project_root_for_ralph,
                    &session_id_clone,
                    Some(&round_id_clone),
                )
                .await;
                complete_autopilot_turn_if_needed(
                    is_autopilot_mode_for_spawn,
                    &sessions_clone,
                    &repo_clone,
                    &project_root_for_autopilot,
                    &session_id_clone,
                    Some(&round_id_clone),
                )
                .await;
                complete_team_turn_if_needed(
                    is_team_mode_for_spawn,
                    &repo_clone,
                    &project_root_for_team,
                    &session_id_clone,
                    Some(&round_id_clone),
                )
                .await;
                spawn_memory_sync(MemorySyncRequest {
                    app: &app_clone,
                    sessions: &sessions_clone,
                    repo: &repo_clone,
                    session_id: &session_id_clone,
                    client: client.as_ref(),
                    user_message: &request_text_for_constraints,
                    assistant_reply: &final_reply_for_follow_up,
                    allow_long_term_promotion: true,
                });
                emit_post_turn_meta_then_complete(PostTurnCompletionRequest {
                    app: &app_clone,
                    session_id: &session_id_clone,
                    stream_message_id: &message_id_clone,
                    assistant_message_id: &last_assistant_id,
                    client: client.as_ref(),
                    final_reply: &final_reply_for_follow_up,
                    skip_summary: preflight_skip_turn_summary,
                    suggestions_reply: &final_reply_for_follow_up,
                    repo: repo_clone.clone(),
                })
                .await;
                spawn_chat_indexing(&app_clone, &sessions_clone, &repo_clone, &session_id_clone);
                return;
            }
        }

        persist_session_tool_state(&sessions_clone, &repo_clone, &session_id_clone).await;
        let max_rounds_error = format!("Exceeded maximum tool rounds ({MAX_TOOL_ROUNDS})");
        fail_ralph_turn_if_needed(
            mode_lifecycle_context!(
                is_ralph_mode_for_spawn,
                &sessions_clone,
                &repo_clone,
                &project_root_for_ralph,
                &session_id_clone,
                ralph_env_for_spawn.clone(),
                Some(&round_id_clone),
            ),
            crate::domain::ralph_state::RalphPhase::Executing,
            &max_rounds_error,
        )
        .await;
        fail_autopilot_turn_if_needed(
            mode_lifecycle_context!(
                is_autopilot_mode_for_spawn,
                &sessions_clone,
                &repo_clone,
                &project_root_for_autopilot,
                &session_id_clone,
                autopilot_env_for_spawn.clone(),
                Some(&round_id_clone),
            ),
            crate::domain::autopilot_state::AutopilotPhase::Qa,
            &max_rounds_error,
        )
        .await;
        fail_team_turn_if_needed(
            is_team_mode_for_spawn,
            &repo_clone,
            &project_root_for_team,
            &session_id_clone,
            &max_rounds_error,
            Some(&round_id_clone),
        )
        .await;

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
        persist_and_emit_turn_token_usage(
            &app_clone,
            &repo_clone,
            &last_assistant_id,
            &message_id_clone,
            &turn_token_usage,
            &llm_config_for_spawn.provider.to_string(),
        )
        .await;
        spawn_memory_sync(MemorySyncRequest {
            app: &app_clone,
            sessions: &sessions_clone,
            repo: &repo_clone,
            session_id: &session_id_clone,
            client: client.as_ref(),
            user_message: &request_text_for_constraints,
            assistant_reply: &final_reply_for_follow_up,
            allow_long_term_promotion: true,
        });
        emit_post_turn_meta_then_complete(PostTurnCompletionRequest {
            app: &app_clone,
            session_id: &session_id_clone,
            stream_message_id: &message_id_clone,
            assistant_message_id: &last_assistant_id,
            client: client.as_ref(),
            final_reply: &final_reply_for_follow_up,
            skip_summary: preflight_skip_turn_summary,
            suggestions_reply: &final_reply_for_follow_up,
            repo: repo_clone.clone(),
        })
        .await;

        // After the main LLM turn completes, fire real multi-agent orchestration if the
        // scheduler produced a multi-subtask plan.  Each sub-agent runs in its own background
        // session and emits independent stream events; we use a fresh runtime so the
        // sub-agents' cancel token is independent from the parent turn's.
        // Team keyword route always fires orchestration (even 1 subtask) so the worker agent
        // runs with the Architect → review loop rather than the inline skill SKILL.md path.
        if let Some(sched) = scheduler_for_spawn {
            if is_team_mode_for_spawn || sched.plan.subtasks.len() > 1 {
                // Confirmation gate: when the plan is large and skip_confirmation was not
                // set (team keyword route always skips since the user explicitly requested it),
                // emit the confirmation event and defer execution.
                let needs_confirm = sched.requires_confirmation
                    && !is_team_mode_for_spawn
                    && !is_explicit_execution_workflow_for_spawn;
                if needs_confirm {
                    let pending_plan = sched.plan.clone();
                    let pending_plan_id = pending_plan.plan_id.clone();
                    let pending_original_request = pending_plan.original_request.clone();
                    let pending_task_count = pending_plan.subtasks.len();
                    let pending_mode_hint = keyword_skill_route
                        .as_ref()
                        .map(|r| r.skill_name.clone())
                        .unwrap_or_else(|| "schedule".to_string());
                    let _ = app_clone.emit(
                        "agent-schedule-confirmation-required",
                        serde_json::json!({
                            "sessionId": session_id_clone.clone(),
                            "planId": pending_plan_id,
                            "summary": sched.confirmation_message
                                .as_deref()
                                .unwrap_or("此计划需要用户确认后才能执行"),
                            "estimatedMinutes": sched.estimated_duration_secs.div_ceil(60),
                            "agents": sched.selected_agents.clone(),
                            // Send the reviewed plan so confirmation executes exactly this decomposition.
                            "plan": pending_plan,
                            "projectRoot": project_root_str_for_spawn.clone(),
                            "strategy": sched.recommended_strategy,
                            "modeHint": pending_mode_hint.clone(),
                            "originalRequest": {
                                "userRequest": pending_original_request,
                                "projectRoot": project_root_str_for_spawn.clone(),
                                "sessionId": session_id_clone.clone(),
                                "maxAgents": pending_task_count,
                                "autoDecompose": true,
                                "strategy": serde_json::to_value(
                                    sched.recommended_strategy
                                ).unwrap_or(serde_json::Value::Null),
                                "modeHint": pending_mode_hint,
                                "skipConfirmation": true,
                            }
                        }),
                    );
                } else {
                    let app_for_orch = app_clone.clone();
                    let session_id_for_orch = session_id_clone.clone();
                    let sched_plan = sched.plan.clone();
                    let original_request = sched_plan.original_request.clone();
                    let orch_runtime = AgentLlmRuntime {
                        llm_config: llm_config_for_spawn.clone(),
                        round_id: uuid::Uuid::new_v4().to_string(),
                        cancel_flag: std::sync::Arc::new(tokio::sync::RwLock::new(false)),
                        pending_tools: pending_tools_clone.clone(),
                        repo: repo_clone.clone(),
                        plan_mode_flag: None,
                        allow_nested_agent: env_allow_nested_agent(),
                        round_cancel: tokio_util::sync::CancellationToken::new(),
                        execution_environment: agent_runtime.execution_environment.clone(),
                        ssh_server: agent_runtime.ssh_server.clone(),
                        sandbox_backend: agent_runtime.sandbox_backend.clone(),
                        local_venv_type: agent_runtime.local_venv_type.clone(),
                        local_venv_name: agent_runtime.local_venv_name.clone(),
                        env_store: agent_runtime.env_store.clone(),
                        runtime_constraints_config: agent_runtime
                            .runtime_constraints_config
                            .clone(),
                    };
                    let active_orch_for_spawn = active_orchestrations_clone.clone();
                    tokio::spawn(async move {
                        use crate::domain::agents::scheduler::{AgentScheduler, SchedulingRequest};

                        // Register cancel token so cancel_agent_schedule can abort this orchestration.
                        let orch_cancel = tokio_util::sync::CancellationToken::new();
                        let orch_id = uuid::Uuid::new_v4().to_string();
                        {
                            let mut map = active_orch_for_spawn.lock().await;
                            map.entry(session_id_for_orch.clone())
                                .or_default()
                                .insert(orch_id.clone(), orch_cancel.clone());
                        }

                        let sched_req = SchedulingRequest::new(original_request)
                            .with_project_root(project_root_str_for_spawn)
                            .with_mode_hint(
                                keyword_skill_route
                                    .as_ref()
                                    .map(|r| r.skill_name.clone())
                                    .unwrap_or_default(),
                            )
                            .with_strategy(strategy_for_spawn);
                        let scheduler = AgentScheduler::new();
                        let orch_result = scheduler
                            .execute_plan_with_runtime(
                                &sched_plan,
                                &sched_req,
                                &app_for_orch,
                                &orch_runtime,
                                &session_id_for_orch,
                                orch_cancel,
                            )
                            .await;

                        // Deregister cancel token.
                        {
                            let mut map = active_orch_for_spawn.lock().await;
                            if let Some(inner) = map.get_mut(&session_id_for_orch) {
                                inner.remove(&orch_id);
                                if inner.is_empty() {
                                    map.remove(&session_id_for_orch);
                                }
                            }
                        }

                        match orch_result {
                            Ok(result) => {
                                // Inject summary message and fire agent-schedule-complete event
                                // so the frontend refreshes the conversation history.
                                crate::commands::chat::inject_schedule_summary_message(
                                    &app_for_orch,
                                    &session_id_for_orch,
                                    &sched_req.user_request,
                                    &result,
                                    &orch_runtime,
                                )
                                .await;
                            }
                            Err(e) => {
                                tracing::error!(
                                    target: "omiga::scheduler",
                                    "Multi-agent orchestration failed: {}",
                                    e
                                );
                            }
                        }
                    });
                } // close else { (needs_confirm false path)
            }
        }
    });

    // 如果是 Plan mode，生成初始 todo items
    let initial_todos = if is_plan_mode {
        scheduler_result.as_ref().map(|result| {
            result
                .plan
                .subtasks
                .iter()
                .enumerate()
                .map(|(idx, subtask)| InitialTodoItem {
                    id: format!("plan-todo-{}", idx),
                    content: subtask.description.clone(),
                    status: if idx == 0 {
                        "in_progress".to_string()
                    } else {
                        "pending".to_string()
                    },
                })
                .collect()
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

mod settings;
pub use settings::*;
mod provider;
pub use provider::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_loop_precompact_does_not_reacquire_sessions_lock() {
        let source = include_str!("mod.rs");
        let start = source
            .find("Shrink history before the next model call")
            .expect("tool-loop compaction block marker should exist");
        let end = source[start..]
            .find("let updated_messages =")
            .map(|offset| start + offset)
            .expect("post-compaction updated_messages marker should exist");
        let block = &source[start..end];

        assert!(
            !block.contains("sessions_clone.read().await"),
            "tool-loop compaction already holds sessions_clone.write(); \
            reading the same RwLock inside this block deadlocks after 压缩前摘要"
        );
    }

    #[test]
    fn stale_mcp_cache_is_not_served_to_model() {
        let source = include_str!("mod.rs");
        let forbidden_phrase = ["MCP tool cache stale; using", " stale schemas"].concat();
        assert!(
            source.contains("MCP tool cache stale; withholding stale schemas"),
            "stale MCP cache entries should trigger refresh without exposing removed tools"
        );
        assert!(
            !source.contains(&forbidden_phrase),
            "stale MCP cache entries must not be sent to the model"
        );
    }

    #[test]
    fn legacy_mcp_tool_history_is_normalized_before_model_context() {
        let (name, input) = normalize_llm_tool_history_for_model(
            "mcp__pubmed__pubmed_search_articles",
            &serde_json::json!({"term":"TP53","retmax":2}),
        );

        assert_eq!(name, "search");
        assert_eq!(input["category"], "literature");
        assert_eq!(input["source"], "pubmed");
        assert_eq!(input["query"], "TP53");
        assert_eq!(input["max_results"], 2);
    }
}
