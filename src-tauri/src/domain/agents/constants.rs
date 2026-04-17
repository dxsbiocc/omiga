//! Agent 系统常量

/// Agent 工具名称
pub const AGENT_TOOL_NAME: &str = "Agent";

/// 旧版别名（向后兼容）
pub const LEGACY_AGENT_TOOL_NAME: &str = "Task";

/// Verification Agent 类型
pub const VERIFICATION_AGENT_TYPE: &str = "verification";

/// 一次性 Agent 类型（运行一次返回报告，无需继续对话）
pub const ONE_SHOT_AGENT_TYPES: &[&str] = &["Explore", "Plan"];

/// 默认模型继承标识
pub const MODEL_INHERIT: &str = "inherit";

/// 内置 Agent 颜色
pub const AGENT_COLORS: &[&str] = &[
    "blue", "green", "yellow", "red", "purple", "orange", "pink", "cyan",
];
