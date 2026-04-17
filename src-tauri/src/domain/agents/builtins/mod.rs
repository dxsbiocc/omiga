//! 内置 Agent 定义和注册

pub mod explore;
pub mod general;
pub mod plan;
pub mod verification;

use super::router::AgentRouter;

/// 注册所有内置 Agent
pub fn register_built_in_agents(router: &mut AgentRouter) {
    // Explore Agent - 代码探索
    router.register(Box::new(explore::ExploreAgent));

    // Plan Agent - 架构设计
    router.register(Box::new(plan::PlanAgent));

    // General-Purpose Agent - 通用任务
    router.register(Box::new(general::GeneralPurposeAgent));

    // Verification Agent - 代码验证（对抗性测试）
    router.register(Box::new(verification::VerificationAgent));
}

/// 检查是否为内置 Agent
pub fn is_built_in_agent(agent_type: &str) -> bool {
    matches!(
        agent_type,
        "Explore" | "Plan" | "general-purpose" | "verification"
    )
}

/// 获取内置 Agent 的模型配置
///
/// 规则：
/// - "inherit" → 继承父会话模型
/// - None → 使用默认策略
/// - 具体值 → 使用该模型
pub fn resolve_agent_model(agent_model: Option<&str>, parent_model: &str) -> String {
    match agent_model {
        None | Some("inherit") => parent_model.to_string(),
        Some("sonnet") => "claude-sonnet-4-6".to_string(),
        Some("opus") => "claude-opus-4-6".to_string(),
        Some("haiku") => "claude-haiku-4-5-20251001".to_string(),
        Some(model) => model.to_string(),
    }
}

/// 获取 Agent 的工具集
///
/// 根据 Agent 的 allowlist/denylist 过滤工具
pub fn get_agent_tool_set(
    all_tools: &[String],
    allowed: Option<&[String]>,
    disallowed: Option<&[String]>,
) -> Vec<String> {
    let mut tools: Vec<String> = match allowed {
        Some(list) => list.to_vec(),
        None => all_tools.to_vec(),
    };

    // 应用 denylist
    if let Some(denylist) = disallowed {
        let deny_set: std::collections::HashSet<_> = denylist.iter().collect();
        tools.retain(|t| !deny_set.contains(t));
    }

    tools
}
