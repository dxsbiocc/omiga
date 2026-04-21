//! Agent/Subagent 系统 —— 从 Claude Code 迁移
//!
//! 提供可扩展的 Agent 定义、路由和执行系统。

pub mod background;
pub mod builtins;
pub mod chat_input_target;
pub mod constants;
pub mod coordinator;
pub mod definition;
pub mod hot_reload;
pub mod integration;
/// 内置 `SOUL.md` / `MEMORY.md` / `USER.md` 等模板（`include_str!`）。
pub mod markdown;
pub mod model_router;
pub mod output_formatter;
pub mod overlay;
pub mod personality;
pub mod prompt_loader;
pub mod registry;
pub mod router;
pub mod scheduler;
pub mod subagent_tool_filter;
pub mod user_context;

pub use definition::{AgentDefinition, AgentSource, PermissionMode};
pub use integration::{get_agent_registry, get_agent_router};
pub use markdown::{
    EXAMPLE_AGENT_PERSONALITIES_YAML, TEMPLATE_MEMORY_MD, TEMPLATE_SOUL_MD, TEMPLATE_USER_MD,
};
pub use overlay::build_runtime_overlay;
pub use personality::compose_full_agent_system_prompt;
pub use registry::{AgentRegistry, AgentRoleInfo};
pub use router::AgentRouter;
pub use user_context::{load_user_omiga_context, UserOmigaContext};

// Re-export chat input target for routing user messages
pub use chat_input_target::ChatInputTarget;
