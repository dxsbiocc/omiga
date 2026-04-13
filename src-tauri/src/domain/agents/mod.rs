//! Agent/Subagent 系统 —— 从 Claude Code 迁移
//!
//! 提供可扩展的 Agent 定义、路由和执行系统。

pub mod background;
pub mod builtins;
pub mod chat_input_target;
pub mod constants;
pub mod coordinator;
pub mod output_formatter;
pub mod definition;
pub mod hot_reload;
pub mod integration;
pub mod router;
pub mod scheduler;
pub mod subagent_tool_filter;

pub use definition::{AgentDefinition, AgentSource, PermissionMode};
pub use integration::{AgentSessionConfig, get_agent_router, prepare_agent_session_config};
pub use router::AgentRouter;

// Re-export chat input target for routing user messages
pub use chat_input_target::ChatInputTarget;
