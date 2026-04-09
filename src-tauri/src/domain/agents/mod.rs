//! Agent/Subagent 系统 —— 从 Claude Code 迁移
//!
//! 提供可扩展的 Agent 定义、路由和执行系统。

pub mod background;
pub mod builtins;
pub mod constants;
pub mod output_formatter;
pub mod definition;
pub mod hot_reload;
pub mod integration;
pub mod router;
pub mod scheduler;

pub use definition::{AgentDefinition, AgentSource, PermissionMode};
pub use integration::{AgentSessionConfig, get_agent_router, prepare_agent_session_config};
pub use router::AgentRouter;

#[cfg(test)]
mod tests;
