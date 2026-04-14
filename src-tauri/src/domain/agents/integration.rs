//! Agent 系统集成模块
//!
//! 将 Agent 路由系统集成到现有的 Chat/Subagent 系统中。

use super::definition::PermissionMode;
use std::path::Path;

use super::personality::compose_full_agent_system_prompt;
use super::router::AgentRouter;
use crate::domain::tools::ToolContext;

/// Agent 会话配置
#[derive(Debug, Clone)]
pub struct AgentSessionConfig {
    /// Agent 类型
    pub agent_type: String,
    /// 系统提示词（Agent 特定 + 基础）
    pub system_prompt: String,
    /// 允许的工具列表（None = 全部）
    pub allowed_tools: Option<Vec<String>>,
    /// 禁止的工具列表
    pub disallowed_tools: Vec<String>,
    /// 使用的模型
    pub model: String,
    /// 权限模式
    pub permission_mode: PermissionMode,
    /// 是否为后台 Agent
    pub background: bool,
    /// 是否省略 CLAUDE.md
    pub omit_claude_md: bool,
}

/// 为子 Agent 会话准备配置
pub fn prepare_agent_session_config(
    router: &AgentRouter,
    subagent_type: Option<&str>,
    parent_model: &str,
    parent_in_plan_mode: bool,
    allow_nested_agent: bool,
    project_root: &Path,
) -> AgentSessionConfig {
    // 选择 Agent
    let agent = router.select_agent(subagent_type);
    let agent_type = agent.agent_type().to_string();
    
    // 解析模型
    let model = resolve_agent_model(agent.model(), parent_model);
    
    // 解析权限模式
    let permission_mode = agent.permission_mode().unwrap_or(
        if parent_in_plan_mode {
            PermissionMode::Plan
        } else {
            PermissionMode::AcceptEdits
        }
    );
    
    // 构建系统提示词（这里只是 Agent 特定的部分，外层会添加基础提示词）
    let tool_ctx = ToolContext::new(project_root.to_path_buf());
    let agent_specific_prompt = compose_full_agent_system_prompt(agent, &tool_ctx);
    
    // 构建子 Agent 模式说明
    let nested_agent_note = if allow_nested_agent {
        " Nested `Agent` is allowed."
    } else {
        ""
    };
    
    let exit_plan_note = if parent_in_plan_mode {
        " `ExitPlanMode` is available while the parent session is in plan mode."
    } else {
        ""
    };
    
    let subagent_mode_prompt = format!(
        "## Sub-agent mode ({})
You are an isolated sub-agent running as '{}'. \
Use tools as needed. Disallowed tools: {}. \
{}{}",
        agent_type,
        agent_type,
        format_disallowed_tools(agent.disallowed_tools()),
        exit_plan_note,
        nested_agent_note
    );
    
    // 合并提示词
    let system_prompt = format!(
        "{}\n\n{}",
        agent_specific_prompt,
        subagent_mode_prompt
    );
    
    // 处理工具限制
    let allowed_tools = agent.allowed_tools().map(|t| t.to_vec());
    let disallowed_tools = agent.disallowed_tools()
        .map(|t| t.iter().map(|s| s.to_string()).collect())
        .unwrap_or_default();
    
    AgentSessionConfig {
        agent_type,
        system_prompt,
        allowed_tools,
        disallowed_tools,
        model,
        permission_mode,
        background: agent.background(),
        omit_claude_md: agent.omit_claude_md(),
    }
}

/// 解析 Agent 模型配置
fn resolve_agent_model(agent_model: Option<&str>, parent_model: &str) -> String {
    match agent_model {
        None | Some("inherit") => parent_model.to_string(),
        Some("sonnet") => "claude-sonnet-4-6".to_string(),
        Some("opus") => "claude-opus-4-6".to_string(),
        Some("haiku") => "claude-haiku-4-5-20251001".to_string(),
        Some(model) => model.to_string(),
    }
}

/// 格式化禁止的工具列表
fn format_disallowed_tools(tools: Option<Vec<String>>) -> String {
    match tools {
        Some(t) if !t.is_empty() => t.join(", "),
        _ => "none".to_string(),
    }
}

/// 全局 Agent 路由器实例
use std::sync::OnceLock;

static AGENT_ROUTER: OnceLock<AgentRouter> = OnceLock::new();

/// 获取全局 Agent 路由器
pub fn get_agent_router() -> &'static AgentRouter {
    AGENT_ROUTER.get_or_init(AgentRouter::new)
}


