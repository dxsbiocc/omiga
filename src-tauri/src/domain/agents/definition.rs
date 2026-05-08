//! Agent 定义 Trait 和类型
//!
//! 定义所有 Agent 必须实现的接口，以及 Agent 配置类型。

use crate::domain::tools::ToolContext;
use serde::{Deserialize, Serialize};

/// Three-tier model routing for sub-agents.
///
/// Maps to provider-specific model aliases resolved by `resolve_subagent_model`:
///
/// | Tier     | Alias    | Anthropic model     | Use case                            |
/// |----------|----------|---------------------|-------------------------------------|
/// | Frontier | "opus"   | claude-opus-4-*     | Deep reasoning, Architect/Reviewer  |
/// | Standard | "sonnet" | claude-sonnet-4-*   | Execution, coding, orchestration    |
/// | Spark    | "haiku"  | claude-haiku-4-*    | Fast searches, lightweight queries  |
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelTier {
    /// Highest capability — Opus. For complex reasoning and verification.
    Frontier,
    /// Balanced — Sonnet. For most execution and coding work.
    Standard,
    /// Fastest/cheapest — Haiku. For quick searches and lightweight tasks.
    Spark,
}

impl ModelTier {
    /// Return the model alias string passed to `resolve_subagent_model`.
    pub fn alias(self) -> &'static str {
        match self {
            ModelTier::Frontier => "opus",
            ModelTier::Standard => "sonnet",
            ModelTier::Spark => "haiku",
        }
    }
}

/// Agent 权限模式
#[derive(Debug, Clone, Copy)]
pub enum PermissionMode {
    Default,
    AcceptEdits,
    Plan,
    BypassPermissions,
}

/// Agent 来源
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentSource {
    /// 内置 Agent
    BuiltIn,
    /// 用户自定义 Agent (~/.claude/agents/)
    UserSettings,
    /// 项目级 Agent (.claude/agents/)
    ProjectSettings,
    /// 插件提供的 Agent
    Plugin,
}

/// Agent 定义 Trait
///
/// 所有 Agent（内置或自定义）必须实现此接口
pub trait AgentDefinition: Send + Sync {
    /// Agent 类型标识符（唯一）
    fn agent_type(&self) -> &str;

    /// 使用场景描述（用于帮助模型决定何时使用）
    fn when_to_use(&self) -> &str;

    /// 生成系统提示词
    fn system_prompt(&self, ctx: &ToolContext) -> String;

    /// 持久身份片段（类似 Hermes `SOUL.md`），描述语气与价值观；由 `compose_full_agent_system_prompt` 拼入最终提示。
    fn soul_fragment(&self) -> Option<&str> {
        None
    }

    /// 内置人格预设名（如 `concise`、`teacher`），与 Hermes `/personality` 命名对齐；未知名称在叠层阶段忽略。
    fn personality_preset(&self) -> Option<&str> {
        None
    }

    /// Agent 来源
    fn source(&self) -> AgentSource;

    /// 允许使用的工具列表（None = 全部允许）
    fn allowed_tools(&self) -> Option<Vec<String>> {
        None
    }

    /// 禁止使用的工具列表
    fn disallowed_tools(&self) -> Option<Vec<String>> {
        None
    }

    /// Model tier for this agent. Determines default model selection.
    /// Override this instead of `model()` whenever possible.
    /// `model()` falls back to the tier alias when not explicitly overridden.
    fn model_tier(&self) -> ModelTier {
        ModelTier::Standard
    }

    /// Explicit model override (None = use `model_tier()`, "inherit" = parent model).
    /// Most agents should leave this as None and implement `model_tier()` instead.
    fn model(&self) -> Option<&str> {
        None
    }

    /// Agent 颜色（用于 UI 区分）
    fn color(&self) -> Option<&str> {
        None
    }

    /// 权限模式（覆盖默认模式）
    fn permission_mode(&self) -> Option<PermissionMode> {
        None
    }

    /// 是否始终在后台运行
    fn background(&self) -> bool {
        false
    }

    /// 是否省略 CLAUDE.md（只读 Agent 不需要）
    fn omit_claude_md(&self) -> bool {
        false
    }

    /// 所需 MCP 服务器模式列表
    fn required_mcp_servers(&self) -> Option<&[String]> {
        None
    }

    /// 记忆范围（如果启用）
    fn memory_scope(&self) -> Option<&str> {
        None
    }

    /// 最大轮数限制
    fn max_turns(&self) -> Option<usize> {
        None
    }

    /// 初始提示词（添加到第一个用户消息）
    fn initial_prompt(&self) -> Option<&str> {
        None
    }

    /// 是否对用户可见（出现在 @ 选择器中）
    ///
    /// 返回 `false` 的 Agent 由编排器（leader）内部调用，不暴露给用户。
    fn user_facing(&self) -> bool {
        true
    }

    /// 隔离模式
    fn isolation(&self) -> Option<&str> {
        None
    }
}

/// 内置 Agent 定义结构（简化版）
///
/// 注意：这是一个辅助结构，用于快速创建简单的内置 Agent。
/// 对于复杂的 Agent，建议直接实现 AgentDefinition trait。
pub struct BuiltInAgent {
    pub agent_type: &'static str,
    pub when_to_use: &'static str,
    pub system_prompt_text: &'static str,
    pub source: AgentSource,
    pub allowed_tools: Option<Vec<String>>,
    pub disallowed_tools: Option<Vec<String>>,
    pub model: Option<&'static str>,
    pub color: Option<&'static str>,
    pub background: bool,
    pub omit_claude_md: bool,
    /// 可选内置人格预设键（与 `personality` 模块中的预设名一致）
    pub personality_key: Option<&'static str>,
    /// 可选身份片段（soul），叠在任务提示之前
    pub soul_text: Option<&'static str>,
}

impl AgentDefinition for BuiltInAgent {
    fn agent_type(&self) -> &str {
        self.agent_type
    }

    fn when_to_use(&self) -> &str {
        self.when_to_use
    }

    fn system_prompt(&self, _ctx: &ToolContext) -> String {
        self.system_prompt_text.to_string()
    }

    fn soul_fragment(&self) -> Option<&str> {
        self.soul_text
    }

    fn personality_preset(&self) -> Option<&str> {
        self.personality_key
    }

    fn source(&self) -> AgentSource {
        self.source
    }

    fn allowed_tools(&self) -> Option<Vec<String>> {
        self.allowed_tools.clone()
    }

    fn disallowed_tools(&self) -> Option<Vec<String>> {
        self.disallowed_tools.clone()
    }

    fn model(&self) -> Option<&str> {
        self.model
    }

    fn color(&self) -> Option<&str> {
        self.color
    }

    fn background(&self) -> bool {
        self.background
    }

    fn omit_claude_md(&self) -> bool {
        self.omit_claude_md
    }
}

/// Agent 定义容器（用于存储异构 Agent）
pub struct AgentDefEntry {
    pub inner: Box<dyn AgentDefinition>,
}

impl AgentDefEntry {
    pub fn new(agent: Box<dyn AgentDefinition>) -> Self {
        Self { inner: agent }
    }
}

impl std::ops::Deref for AgentDefEntry {
    type Target = dyn AgentDefinition;

    fn deref(&self) -> &Self::Target {
        &*self.inner
    }
}
