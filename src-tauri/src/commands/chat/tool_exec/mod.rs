//! Tool execution dispatcher: `execute_tool_calls` and `execute_one_tool`.

use super::permissions::{
    execute_ask_user_question_interactive, matches_ask_user_question_name,
    wait_for_permission_tool_resolution, AskUserQuestionExecution, PermissionToolResolutionRequest,
};
use super::subagent::{
    is_agent_tool_name, run_skill_forked, run_subagent_session, ForkedSkillRequest,
    SubagentSessionRequest,
};
use super::{
    append_truncated_results_note, apply_empty_structured_tool_placeholder,
    fold_tool_stream_item_for_model, handle_skill_config, process_tool_output_for_model,
    AgentLlmRuntime, MAX_SUBAGENT_EXECUTE_DEPTH,
};
use crate::app_state::OmigaAppState;
use crate::constants::tool_limits::{
    truncate_utf8_prefix, PREVIEW_SIZE_BYTES, TOOL_DISPLAY_MAX_INPUT_CHARS,
};
use crate::domain::agents::subagent_tool_filter::{
    should_block_subagent_builtin_call, SubagentFilterOptions,
};
use crate::domain::chat_state::{McpToolCache, MCP_TOOL_CACHE_TTL};
use crate::domain::integrations_config;
use crate::domain::permissions::{
    canonical_permission_tool_name, load_merged_permission_deny_rule_entries, matching_deny_entry,
};
use crate::domain::session::{AgentTask, TodoItem};
use crate::domain::skills;
use crate::domain::tools::{
    all_tool_schemas, normalize_legacy_retrieval_tool_arguments,
    normalize_legacy_retrieval_tool_name, Tool, ToolContext, ToolSchema, WebSearchApiKeys,
};
use crate::infrastructure::streaming::StreamOutputItem;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex as StdMutex};
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::RwLock;

mod concurrency;
mod dispatch;
mod handlers;
mod normalize;
mod orchestrate;

pub(super) use orchestrate::execute_tool_calls;

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
    pub browser_use_enabled: bool,
    /// Optional session artifact registry for tracking file_write/file_edit operations.
    pub artifact_registry: Option<Arc<crate::domain::session::artifacts::ArtifactRegistry>>,
}
