//! Agent 系统单元测试

use omiga_lib::domain::agents::builtins::{
    explore::ExploreAgent, general::GeneralPurposeAgent, get_agent_tool_set, plan::PlanAgent,
    resolve_agent_model, verification::VerificationAgent,
};
use omiga_lib::domain::agents::{builtins, AgentDefinition, AgentRouter, AgentSource};

#[test]
fn test_agent_router_select_default() {
    let router = AgentRouter::new();

    // 未指定类型时应返回默认 Agent
    let agent = router.select_agent(None);
    assert_eq!(agent.agent_type(), "general-purpose");

    // 指定 "general-purpose" 应返回相同结果
    let agent = router.select_agent(Some("general-purpose"));
    assert_eq!(agent.agent_type(), "general-purpose");
}

#[test]
fn test_agent_router_select_explore() {
    use omiga_lib::domain::agents::definition::ModelTier;
    let router = AgentRouter::new();

    let agent = router.select_agent(Some("Explore"));
    assert_eq!(agent.agent_type(), "Explore");
    // Explore uses model_tier() = Spark (haiku); model() returns None by default.
    assert_eq!(agent.model_tier(), ModelTier::Spark);
}

#[test]
fn test_agent_router_select_plan() {
    let router = AgentRouter::new();

    let agent = router.select_agent(Some("Plan"));
    assert_eq!(agent.agent_type(), "Plan");
    assert_eq!(agent.model(), Some("inherit"));
}

#[test]
fn test_agent_router_select_verification() {
    let router = AgentRouter::new();

    let agent = router.select_agent(Some("verification"));
    assert_eq!(agent.agent_type(), "verification");
    assert!(agent.background());
    assert_eq!(agent.color(), Some("red"));
}

#[test]
fn test_agent_router_fallback_to_default() {
    let router = AgentRouter::new();

    // 不存在的 Agent 类型应回退到默认
    let agent = router.select_agent(Some("nonexistent-agent"));
    assert_eq!(agent.agent_type(), "general-purpose");
}

#[test]
fn test_explore_agent_disallowed_tools() {
    let agent = ExploreAgent;
    let disallowed = agent.disallowed_tools().unwrap();

    assert!(disallowed.contains(&"Agent".to_string()));
    // Tool names match ToolSchema::new() identifiers (snake_case)
    assert!(disallowed.contains(&"file_edit".to_string()));
    assert!(disallowed.contains(&"file_write".to_string()));
    assert!(disallowed.contains(&"notebook_edit".to_string()));
    assert!(disallowed.contains(&"ExitPlanMode".to_string()));
}

#[test]
fn test_explore_agent_omit_claude_md() {
    let agent = ExploreAgent;
    assert!(agent.omit_claude_md());
}

#[test]
fn test_plan_agent_disallowed_tools() {
    let agent = PlanAgent;
    let disallowed = agent.disallowed_tools().unwrap();

    assert!(disallowed.contains(&"Agent".to_string()));
    assert!(disallowed.contains(&"file_edit".to_string()));
}

#[test]
fn test_general_purpose_agent_allows_most_tools() {
    let agent = GeneralPurposeAgent;

    // General-Purpose Agent 应该允许大部分工具
    assert!(agent.allowed_tools().is_none());
    assert!(agent.disallowed_tools().is_none()); // trait 方法返回 None

    // Agent 工具在运行时通过子 Agent 过滤器阻止递归（不通过 disallowed_tools 返回）
    assert!(agent.disallowed_tools().is_none());
}

#[test]
fn test_verification_agent_background() {
    let agent = VerificationAgent;
    assert!(agent.background());
    assert_eq!(agent.color(), Some("red"));
}

#[test]
fn test_agent_source() {
    assert_eq!(ExploreAgent.source(), AgentSource::BuiltIn);
    assert_eq!(PlanAgent.source(), AgentSource::BuiltIn);
    assert_eq!(GeneralPurposeAgent.source(), AgentSource::BuiltIn);
}

#[test]
fn test_is_built_in_agent() {
    assert!(builtins::is_built_in_agent("Explore"));
    assert!(builtins::is_built_in_agent("Plan"));
    assert!(builtins::is_built_in_agent("general-purpose"));
    assert!(builtins::is_built_in_agent("verification"));

    assert!(!builtins::is_built_in_agent("custom-agent"));
    assert!(!builtins::is_built_in_agent("unknown"));
}

#[test]
fn test_resolve_agent_model() {
    let parent = "claude-sonnet-4-6";

    // inherit 应该返回父模型
    assert_eq!(resolve_agent_model(Some("inherit"), parent), parent);
    assert_eq!(resolve_agent_model(None, parent), parent);

    // 具体模型名称
    assert_eq!(
        resolve_agent_model(Some("haiku"), parent),
        "claude-haiku-4-5-20251001"
    );
    assert_eq!(
        resolve_agent_model(Some("sonnet"), parent),
        "claude-sonnet-4-6"
    );
    assert_eq!(resolve_agent_model(Some("opus"), parent), "claude-opus-4-6");

    // 自定义模型 ID
    assert_eq!(
        resolve_agent_model(Some("custom-model"), parent),
        "custom-model"
    );
}

#[test]
fn test_get_agent_tool_set() {
    let all_tools = vec![
        "file_read".to_string(),
        "file_write".to_string(),
        "file_edit".to_string(),
        "bash".to_string(),
        "Agent".to_string(),
    ];

    // 允许列表（白名单）
    let allowed = Some(&["file_read".to_string(), "bash".to_string()][..]);
    let result = get_agent_tool_set(&all_tools, allowed, None);
    assert_eq!(result.len(), 2);
    assert!(result.contains(&"file_read".to_string()));
    assert!(result.contains(&"bash".to_string()));

    // 禁止列表（黑名单）
    let disallowed = Some(&["Agent".to_string(), "bash".to_string()][..]);
    let result = get_agent_tool_set(&all_tools, None, disallowed);
    assert!(!result.contains(&"Agent".to_string()));
    assert!(!result.contains(&"bash".to_string()));
    assert!(result.contains(&"file_read".to_string()));

    // 同时使用白名单和黑名单
    let allowed = Some(
        &[
            "file_read".to_string(),
            "file_write".to_string(),
            "bash".to_string(),
        ][..],
    );
    let disallowed = Some(&["bash".to_string()][..]);
    let result = get_agent_tool_set(&all_tools, allowed, disallowed);
    assert!(result.contains(&"file_read".to_string()));
    assert!(result.contains(&"file_write".to_string()));
    assert!(!result.contains(&"bash".to_string())); // 在白名单但也在黑名单
}
