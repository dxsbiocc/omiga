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
use crate::domain::session::{Message, MessageTokenUsage, Session, ToolCall};
use crate::domain::skills;
use crate::domain::tools::{
    all_tool_schemas, normalize_legacy_retrieval_tool_arguments,
    normalize_legacy_retrieval_tool_name, sort_tool_schemas_for_model, ToolContext, ToolSchema,
};
use crate::errors::{ChatError, OmigaError};
use crate::infrastructure::streaming::{FollowUpSuggestion, StreamOutputItem};
use crate::llm::{
    create_client, load_config_from_env, LlmClient, LlmConfig, LlmContent, LlmMessage, LlmRole,
};
use crate::utils::large_output_instructions::get_large_output_instructions;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::{Mutex, RwLock};

pub use crate::domain::chat_state::{
    ChatState, PendingToolCall, RoundCancellationState, SessionRuntimeState,
};

mod compaction_input;
use compaction_input::*;
mod fallback_messages;
use fallback_messages::*;
mod attachments;
use attachments::*;
mod llm_bridge;
use llm_bridge::*;
mod runtime_constraints;
use runtime_constraints::*;
mod tool_output;
pub(crate) use tool_output::tool_results_dir_for_session;
use tool_output::{
    append_truncated_results_note, apply_empty_structured_tool_placeholder,
    fold_tool_stream_item_for_model, persist_session_tool_state, process_tool_output_for_model,
};
mod agent_runtime;
pub(crate) use agent_runtime::AgentLlmRuntime;
use agent_runtime::{ActiveRoundCleanup, MAX_SUBAGENT_EXECUTE_DEPTH, MAX_SUBAGENT_TOOL_ROUNDS};
mod composer_route;
use composer_route::*;
mod orchestration;
use orchestration::*;
mod spawn_context;
use spawn_context::TurnSpawnContext;
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

mod send_pipeline;
#[tauri::command]
pub async fn send_message(
    app: AppHandle,
    app_state: State<'_, OmigaAppState>,
    request: SendMessageRequest,
) -> CommandResult<MessageResponse> {
    send_pipeline::send_message_impl(app, app_state, request).await
}
mod settings;
pub use settings::*;
mod provider;
pub use provider::*;

#[cfg(test)]
mod tests {
    #[test]
    fn tool_loop_precompact_does_not_reacquire_sessions_lock() {
        let source = include_str!("send_pipeline.rs");
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
        assert!(
            block.contains("should_prepare_for_auto_compact"),
            "tool-loop compaction should throttle pre-compact summary/archive emission \
            to avoid repeating 压缩前摘要 / 归档会话摘要 on every tool round"
        );
        assert!(
            !block.contains("archive_on_compact") && !block.contains("long_term_path"),
            "pre-compact scratchpad updates must not archive/promote long-term memory \
            while the active tool loop may still fail"
        );
    }

    #[test]
    fn stale_mcp_cache_is_not_served_to_model() {
        let source = include_str!("send_pipeline.rs");
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
}
