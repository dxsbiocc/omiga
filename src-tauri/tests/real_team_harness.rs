//! Real-provider `/team` harness.
//!
//! Ignored by default because it requires a valid provider config and makes a real planner call.
//! The goal is to validate that Team mode can build a multi-worker plan with a terminal
//! verification phase against the currently configured provider.

use omiga_lib::domain::agents::scheduler::{
    planner::TEAM_VERIFY_TASK_ID, AgentScheduler, SchedulingRequest, SchedulingStrategy,
};
use omiga_lib::llm::load_config;

#[tokio::test]
#[ignore = "requires real provider config"]
async fn real_team_builds_team_plan() {
    let llm_config = load_config().expect("load provider config");
    eprintln!(
        "real-team harness using provider={} model={}",
        llm_config.provider, llm_config.model
    );

    let scheduler = AgentScheduler::new();
    let request = SchedulingRequest::new(
        "Fix the export race condition, verify the fix, and prepare a concise final synthesis. Use a coordinated team of specialized agents.",
    )
    .with_project_root(".")
    .with_mode_hint("team")
    .with_strategy(SchedulingStrategy::Team)
    .with_auto_decompose(true);

    let result = scheduler
        .schedule(request, Some(&llm_config))
        .await
        .expect("team schedule plan");

    eprintln!(
        "plan_id={} tasks={} agents={:?} strategy={:?}",
        result.plan.plan_id,
        result.plan.subtasks.len(),
        result.selected_agents,
        result.recommended_strategy
    );

    assert!(
        result.plan.subtasks.len() > 1,
        "expected a multi-step team plan, got {} tasks",
        result.plan.subtasks.len()
    );
    assert!(
        result
            .plan
            .subtasks
            .iter()
            .any(|task| task.id == TEAM_VERIFY_TASK_ID),
        "expected team plan to include terminal team-verify subtask"
    );
}
