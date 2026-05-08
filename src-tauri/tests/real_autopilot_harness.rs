//! Real-provider `/autopilot` harness.
//!
//! Ignored by default because it requires a valid provider config and makes a real planner call.
//! The goal is to validate that Autopilot mode can build a multi-step phased plan with reviewer
//! augmentation against the currently configured provider.

use omiga_lib::domain::agents::scheduler::{AgentScheduler, SchedulingRequest, SchedulingStrategy};
use omiga_lib::llm::load_config;

#[tokio::test]
#[ignore = "requires real provider config"]
async fn real_autopilot_builds_phased_plan() {
    let llm_config = load_config().expect("load provider config");
    eprintln!(
        "real-autopilot harness using provider={} model={}",
        llm_config.provider, llm_config.model
    );

    let scheduler = AgentScheduler::new();
    let request = SchedulingRequest::new(
        "Implement a settings sync feature, run QA, validate acceptance criteria, and surface any remaining risks before completion.",
    )
    .with_project_root(".")
    .with_mode_hint("autopilot")
    .with_strategy(SchedulingStrategy::Phased)
    .with_auto_decompose(true);

    let result = scheduler
        .schedule(request, Some(&llm_config))
        .await
        .expect("autopilot schedule plan");

    eprintln!(
        "plan_id={} tasks={} agents={:?} strategy={:?} reviewers={:?}",
        result.plan.plan_id,
        result.plan.subtasks.len(),
        result.selected_agents,
        result.recommended_strategy,
        result.reviewer_agents
    );

    assert!(
        result.plan.subtasks.len() > 1,
        "expected a multi-step autopilot plan, got {} tasks",
        result.plan.subtasks.len()
    );
    assert!(
        result
            .reviewer_agents
            .iter()
            .any(|agent| agent == "critic" || agent == "quality-reviewer"),
        "expected autopilot plan to include reviewer-family augmentation"
    );
}
