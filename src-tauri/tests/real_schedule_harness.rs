//! Real-provider `/schedule` harness.
//!
//! Ignored by default because it requires a valid provider config and makes a real planner call.
//! The goal is to validate that the scheduler can build a genuine multi-step plan against the
//! currently configured provider.

use omiga_lib::domain::agents::scheduler::{AgentScheduler, SchedulingRequest, SchedulingStrategy};
use omiga_lib::llm::load_config;

#[tokio::test]
#[ignore = "requires real provider config"]
async fn real_schedule_builds_multi_step_plan() {
    let llm_config = load_config().expect("load provider config");
    eprintln!(
        "real-schedule harness using provider={} model={}",
        llm_config.provider, llm_config.model
    );

    let scheduler = AgentScheduler::new();
    let request = SchedulingRequest::new(
        "Refactor the login flow to add token refresh, add regression tests, and produce a verification checklist. Use multiple agents if helpful.",
    )
    .with_project_root(".")
    .with_mode_hint("schedule")
    .with_strategy(SchedulingStrategy::Phased)
    .with_auto_decompose(true);

    let result = scheduler
        .schedule(request, Some(&llm_config))
        .await
        .expect("scheduler plan");

    eprintln!(
        "plan_id={} tasks={} agents={:?} strategy={:?}",
        result.plan.plan_id,
        result.plan.subtasks.len(),
        result.selected_agents,
        result.recommended_strategy
    );

    assert!(
        result.plan.subtasks.len() > 1,
        "expected a multi-step schedule plan, got {} tasks",
        result.plan.subtasks.len()
    );
}
