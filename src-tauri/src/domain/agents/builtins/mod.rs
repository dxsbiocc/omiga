//! 内置 Agent 定义和注册
//!
//! ## Role-level Tool Tiers & Model Tiers
//!
//! | Agent              | Tool Tier | Model Tier | Model alias | Notes                       |
//! |--------------------|-----------|------------|-------------|-----------------------------|
//! | GeneralPurposeAgent| General   | Standard   | (inherit)   | Full access, orchestrator   |
//! | ExecutorAgent      | Executor  | Standard   | sonnet      | Full code access            |
//! | ArchitectAgent     | Reviewer  | Frontier   | opus        | Verification only, no edits |
//! | ExploreAgent       | Explorer  | Spark      | haiku       | Read-only search            |
//! | DebuggerAgent      | General   | Standard   | (inherit)   | Debug specialist            |
//! | DeepResearchAgent  | Web       | Frontier   | opus        | Research review, citations  |
//! | DataAnalysisAgent  | General   | Standard   | (inherit)   | Python/R statistical analysis|
//! | DataVisualAgent    | General   | Standard   | (inherit)   | Scientific figures/plots    |
//! | LiteratureSearchAgent| Web     | Standard   | (inherit)   | PubMed/arXiv/Scholar search |

pub mod api_reviewer;
pub mod architect;
pub mod code_reviewer;
pub mod critic;
pub mod data_analysis;
pub mod data_visual;
pub mod debugger;
pub mod deep_research;
pub mod executor;
pub mod explore;
pub mod general;
pub mod literature_search;
pub mod performance_reviewer;
pub mod plan;
pub mod quality_reviewer;
pub mod security_reviewer;
pub mod test_engineer;
pub mod verification;

use super::router::AgentRouter;

/// 注册所有内置 Agent
pub fn register_built_in_agents(router: &mut AgentRouter) {
    // Core agents
    router.register(Box::new(general::GeneralPurposeAgent));
    router.register(Box::new(executor::ExecutorAgent));
    router.register(Box::new(explore::ExploreAgent));
    router.register(Box::new(plan::PlanAgent));
    router.register(Box::new(architect::ArchitectAgent));
    router.register(Box::new(api_reviewer::ApiReviewerAgent));
    router.register(Box::new(code_reviewer::CodeReviewerAgent));
    router.register(Box::new(critic::CriticAgent));
    router.register(Box::new(debugger::DebuggerAgent));
    router.register(Box::new(performance_reviewer::PerformanceReviewerAgent));
    router.register(Box::new(security_reviewer::SecurityReviewerAgent));
    router.register(Box::new(test_engineer::TestEngineerAgent));
    router.register(Box::new(quality_reviewer::QualityReviewerAgent));
    router.register(Box::new(verification::VerificationAgent));
    // Research sub-agents
    router.register(Box::new(deep_research::DeepResearchAgent));
    router.register(Box::new(data_analysis::DataAnalysisAgent));
    router.register(Box::new(data_visual::DataVisualAgent));
    router.register(Box::new(literature_search::LiteratureSearchAgent));
}

/// 检查是否为内置 Agent
pub fn is_built_in_agent(agent_type: &str) -> bool {
    matches!(
        agent_type,
        "Explore"
            | "Plan"
            | "general-purpose"
            | "verification"
            | "executor"
            | "architect"
            | "api-reviewer"
            | "code-reviewer"
            | "critic"
            | "debugger"
            | "deep-research"
            | "data-analysis"
            | "data-visual"
            | "literature-search"
            | "performance-reviewer"
            | "quality-reviewer"
            | "security-reviewer"
            | "test-engineer"
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
