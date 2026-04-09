//! Domain layer - Core business logic

pub mod agents;
pub mod agent_runtime;
pub mod auto_compact;
pub mod chat_input_target;
pub mod background_shell;
pub mod chat_state;
pub mod coordinator;
pub mod follow_up_suggestions;
pub mod post_turn_settings;
pub mod integrations_catalog;
pub mod integrations_config;
pub mod mcp;
pub mod persistence;
pub mod session;
pub mod session_codec;
pub mod skills;
pub mod subagent_tool_filter;
pub mod tools;
pub mod tool_permission_rules;
pub mod memory;
pub mod pageindex;
pub mod permissions;

#[cfg(test)]
mod persistence_tests;

