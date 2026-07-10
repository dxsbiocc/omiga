//! Chat commands - Send messages and stream responses with tool execution
//!
//! Multi-provider support: Anthropic, OpenAI, Azure, Google, and custom endpoints

use super::CommandResult;
use crate::app_state::OmigaAppState;
use tauri::{AppHandle, State};

pub use crate::domain::chat_state::{
    ChatState, PendingToolCall, RoundCancellationState, SessionRuntimeState,
};

mod attachments;
mod compaction_input;
mod fallback_messages;
mod llm_bridge;
use llm_bridge::*;
mod runtime_constraints;
use runtime_constraints::*;
mod tool_output;
use tool_output::process_tool_output_for_model;
pub(crate) use tool_output::tool_results_dir_for_session;
mod agent_runtime;
pub(crate) use agent_runtime::AgentLlmRuntime;
use agent_runtime::MAX_SUBAGENT_TOOL_ROUNDS;
mod composer_route;
use composer_route::*;
mod orchestration;
mod spawn_context;
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
mod subagent;
pub(crate) use self::subagent::{spawn_background_agent, BackgroundAgentRequest};
pub mod research;
pub mod research_goal;
mod tool_exec;
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
            .find("async fn compact_tool_loop_history")
            .expect("compact_tool_loop_history function should exist");
        let end = source[start..]
            .find("struct FollowupMessagesInput")
            .map(|offset| start + offset)
            .expect("FollowupMessagesInput struct should follow compact_tool_loop_history");
        let block = &source[start..end];

        assert!(
            !block.contains(".read().await"),
            "compact_tool_loop_history already holds the sessions write lock; \
            acquiring any read lock inside this function deadlocks after 压缩前摘要"
        );
        assert!(
            block.contains("should_prepare_for_auto_compact"),
            "compact_tool_loop_history should throttle pre-compact summary/archive emission \
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
