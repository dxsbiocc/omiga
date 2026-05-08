//! Deterministic OpenAI-compatible mock LLM harness for orchestration validation.
//!
//! These tests intentionally run as normal CI tests. They exercise the same streaming client and
//! scheduler LLM-planner path as a real provider, but use a local `wiremock` endpoint so no
//! secrets, network access, or provider billing are required.

use futures::StreamExt;
use omiga_lib::domain::agents::scheduler::{
    planner::TEAM_VERIFY_TASK_ID, AgentScheduler, SchedulingRequest, SchedulingStrategy,
};
use omiga_lib::llm::{create_client, LlmConfig, LlmMessage, LlmProvider, LlmStreamChunk};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn openai_sse_text(text: &str) -> String {
    let text_chunk = serde_json::json!({
        "id": "chatcmpl-mock",
        "object": "chat.completion.chunk",
        "created": 0,
        "model": "mock-planner",
        "choices": [{
            "index": 0,
            "delta": { "content": text },
            "finish_reason": null
        }]
    });
    let stop_chunk = serde_json::json!({
        "id": "chatcmpl-mock",
        "object": "chat.completion.chunk",
        "created": 0,
        "model": "mock-planner",
        "choices": [{
            "index": 0,
            "delta": {},
            "finish_reason": "stop"
        }]
    });

    format!("data: {text_chunk}\n\ndata: {stop_chunk}\n\ndata: [DONE]\n\n")
}

async fn mock_config_for_response(response_text: &str) -> LlmConfig {
    // Test sandboxes and CI runners may set HTTP(S)_PROXY globally. Keep the local
    // mock endpoint off those proxies so failures exercise Omiga code, not host proxy policy.
    std::env::set_var("NO_PROXY", "127.0.0.1,localhost");
    std::env::set_var("no_proxy", "127.0.0.1,localhost");

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(openai_sse_text(response_text)),
        )
        .mount(&server)
        .await;

    LlmConfig::new(LlmProvider::Custom, "mock-key")
        .with_base_url(format!("{}/v1", server.uri()))
        .with_model("mock-planner")
        .with_max_tokens(1024)
}

fn phased_plan_json() -> String {
    serde_json::json!({
        "mode": "multi",
        "strategy": "phased",
        "subtasks": [
            {
                "id": "explore-login",
                "description": "Map the login/session files and existing token refresh behavior.",
                "agent": "Explore",
                "dependencies": [],
                "critical": false,
                "stage": "retrieve",
                "context": "Inspect the auth/session surface and identify affected tests.",
                "timeout_secs": 300
            },
            {
                "id": "design-refresh",
                "description": "Design the token refresh change and regression checklist.",
                "agent": "Plan",
                "dependencies": ["explore-login"],
                "critical": true,
                "stage": "intent",
                "context": "Produce a minimal implementation plan with rollback notes.",
                "timeout_secs": 300
            },
            {
                "id": "implement-refresh",
                "description": "Implement token refresh and update affected persistence paths.",
                "agent": "executor",
                "dependencies": ["design-refresh"],
                "critical": true,
                "stage": "other",
                "context": "Make the smallest safe code change and preserve existing behavior.",
                "timeout_secs": 600
            },
            {
                "id": "verify-refresh",
                "description": "Run regression tests and summarize remaining risks.",
                "agent": "verification",
                "dependencies": ["implement-refresh"],
                "critical": true,
                "stage": "verify",
                "context": "Verify the acceptance criteria and record evidence.",
                "timeout_secs": 300
            }
        ]
    })
    .to_string()
}

fn team_plan_json_without_terminal_verify() -> String {
    serde_json::json!({
        "mode": "multi",
        "strategy": "team",
        "subtasks": [
            {
                "id": "worker-race",
                "description": "Investigate the export race condition from the state-management side.",
                "agent": "debugger",
                "dependencies": [],
                "critical": true,
                "stage": "debug",
                "context": "Find the likely race and propose a targeted fix.",
                "timeout_secs": 300
            },
            {
                "id": "worker-api",
                "description": "Review export API contracts and compatibility constraints.",
                "agent": "architect",
                "dependencies": [],
                "critical": false,
                "stage": "intent",
                "context": "Identify design and contract risks before implementation.",
                "timeout_secs": 300
            },
            {
                "id": "worker-fix",
                "description": "Apply the safest export race fix after investigation.",
                "agent": "executor",
                "dependencies": ["worker-race", "worker-api"],
                "critical": true,
                "stage": "other",
                "context": "Implement the minimal fix and note verification steps.",
                "timeout_secs": 600
            }
        ]
    })
    .to_string()
}

#[tokio::test]
async fn mock_openai_compatible_streaming_smoke() {
    let config = mock_config_for_response("ok").await;
    let client = create_client(config).expect("create mock client");
    let mut stream = client
        .send_message_streaming(vec![LlmMessage::user("Reply with exactly: ok")], vec![])
        .await
        .expect("start mock stream");

    let mut output = String::new();
    while let Some(chunk) = stream.next().await {
        match chunk.expect("mock stream chunk") {
            LlmStreamChunk::Text(text) | LlmStreamChunk::ReasoningContent(text) => {
                output.push_str(&text)
            }
            LlmStreamChunk::Stop { .. } => break,
            _ => {}
        }
    }

    assert_eq!(output.trim(), "ok");
}

#[tokio::test]
async fn mock_schedule_uses_llm_planner_without_real_provider() {
    let llm_config = mock_config_for_response(&phased_plan_json()).await;
    let scheduler = AgentScheduler::new();
    let request = SchedulingRequest::new(
        "Refactor the login flow to add token refresh, add regression tests, and produce a verification checklist.",
    )
    .with_project_root(".")
    .with_mode_hint("schedule")
    .with_strategy(SchedulingStrategy::Auto)
    .with_auto_decompose(true);

    let result = scheduler
        .schedule(request, Some(&llm_config))
        .await
        .expect("mock schedule plan");

    assert_eq!(result.recommended_strategy, SchedulingStrategy::Phased);
    assert_eq!(result.plan.subtasks.len(), 4);
    assert_eq!(
        result.plan.execution_order,
        vec![
            "explore-login".to_string(),
            "design-refresh".to_string(),
            "implement-refresh".to_string(),
            "verify-refresh".to_string()
        ]
    );
    assert!(result.selected_agents.contains(&"verification".to_string()));
    assert!(result.requires_confirmation);
}

#[tokio::test]
async fn mock_team_adds_terminal_verification_and_reviewers() {
    let llm_config = mock_config_for_response(&team_plan_json_without_terminal_verify()).await;
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
        .expect("mock team plan");

    assert_eq!(result.recommended_strategy, SchedulingStrategy::Team);
    assert!(result
        .plan
        .subtasks
        .iter()
        .any(|task| task.id == TEAM_VERIFY_TASK_ID));

    let team_review_tasks: Vec<_> = result
        .plan
        .subtasks
        .iter()
        .filter(|task| task.id.starts_with("team-review-"))
        .collect();
    assert_eq!(team_review_tasks.len(), 5);
    assert!(team_review_tasks
        .iter()
        .all(|task| task.dependencies == vec![TEAM_VERIFY_TASK_ID.to_string()]));
    assert!(result.reviewer_agents.contains(&"critic".to_string()));
    assert!(result
        .reviewer_agents
        .contains(&"security-reviewer".to_string()));
}

#[tokio::test]
async fn mock_autopilot_plan_gets_reviewer_family() {
    let llm_config = mock_config_for_response(&phased_plan_json()).await;
    let scheduler = AgentScheduler::new();
    let request = SchedulingRequest::new(
        "Autopilot this feature: implement settings sync, run QA, validate acceptance criteria, and surface remaining risks.",
    )
    .with_project_root(".")
    .with_mode_hint("autopilot")
    .with_strategy(SchedulingStrategy::Auto)
    .with_auto_decompose(true);

    let result = scheduler
        .schedule(request, Some(&llm_config))
        .await
        .expect("mock autopilot plan");

    assert_eq!(result.recommended_strategy, SchedulingStrategy::Phased);
    for agent in [
        "quality-reviewer",
        "api-reviewer",
        "code-reviewer",
        "security-reviewer",
        "critic",
    ] {
        assert!(
            result.selected_agents.contains(&agent.to_string()),
            "expected autopilot reviewer family to include {agent}; got {:?}",
            result.selected_agents
        );
    }
    assert!(result
        .plan
        .subtasks
        .iter()
        .filter(|task| task.id.starts_with("autopilot-review-"))
        .all(|task| task.dependencies == vec!["verify-refresh".to_string()]));
}
