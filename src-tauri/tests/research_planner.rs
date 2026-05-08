use omiga_lib::domain::research_system::{
    AgentRegistry, ExecutionRoute, MockAgentRunner, Planner, ResearchDirector, ResultStatus,
};

#[test]
fn planner_turns_simple_request_into_solo_task() {
    let planner = Planner::new();
    let graph = planner.plan("写一个简短总结").expect("plan");

    assert_eq!(graph.tasks.len(), 1);
    assert_eq!(graph.tasks[0].assigned_agent, "reporter.final");
    assert!(graph.edges.is_empty());
    assert!(matches!(graph.execution_route, ExecutionRoute::Solo));
}

#[test]
fn planner_splits_complex_request_into_multiple_tasks() {
    let planner = Planner::new();
    let graph = planner
        .plan("帮我检索单细胞 RNA-seq 差异分析方法，分析适用场景，生成可视化建议和报告")
        .expect("plan");

    assert!(graph.tasks.len() >= 4);
    assert!(graph
        .tasks
        .iter()
        .any(|task| task.assigned_agent == "seeker.web_research"));
    assert!(graph
        .tasks
        .iter()
        .any(|task| task.assigned_agent == "analyzer.data"));
    assert!(graph
        .tasks
        .iter()
        .any(|task| task.assigned_agent == "painter.visualization"));
    assert!(graph
        .tasks
        .iter()
        .any(|task| task.assigned_agent == "reporter.final"));
    assert!(matches!(
        graph.execution_route,
        ExecutionRoute::Workflow | ExecutionRoute::MultiAgent
    ));
}

#[test]
fn planner_exposes_intake_route_for_multi_agent_requests() {
    let planner = Planner::new();
    let intake = planner.analyze_intake("并行检索文献、分析适用场景、生成可视化并输出报告");

    assert!(matches!(intake.execution_route, ExecutionRoute::MultiAgent));
    assert!(intake.complexity_score >= 4);
}

#[test]
fn research_director_uses_runner_for_control_plane() {
    let registry = AgentRegistry::default_registry().expect("default registry");
    let director = ResearchDirector::new(
        registry,
        MockAgentRunner::new().with_forced_status("control-plane.intake", ResultStatus::Failed),
    );

    let error = director
        .prepare("写一个简短总结")
        .expect_err("failed intake runner status should stop preparation");
    assert!(error.contains("control-plane intake runner status"));
}
