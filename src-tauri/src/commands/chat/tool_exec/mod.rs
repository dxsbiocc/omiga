//! Tool execution dispatcher: `execute_tool_calls` and `execute_one_tool`.

use super::agent_runtime::{AgentLlmRuntime, MAX_SUBAGENT_EXECUTE_DEPTH};
use super::permissions::{wait_for_permission_tool_resolution, PermissionToolResolutionRequest};
use super::subagent::{
    run_skill_forked, run_subagent_session, ForkedSkillRequest, SubagentSessionRequest,
};
use super::tool_output::process_tool_output_for_model;
use crate::app_state::OmigaAppState;
use crate::constants::tool_limits::{
    truncate_utf8_prefix, PREVIEW_SIZE_BYTES, TOOL_DISPLAY_MAX_INPUT_CHARS,
};
use crate::domain::integrations_config;
use crate::domain::session::{AgentTask, TodoItem};
use crate::domain::skills;
use crate::domain::tools::{ToolContext, WebSearchApiKeys};
use crate::infrastructure::streaming::StreamOutputItem;
use std::path::Path;
use std::sync::{Arc, Mutex as StdMutex};
use tauri::{AppHandle, Emitter, Manager};

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
