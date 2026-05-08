# Mock LLM Runtime Validation

Last reviewed: 2026-04-25

This is the CI-safe companion to `docs/REAL_LLM_VALIDATION.md`.

The mock LLM runtime harness validates Omiga's orchestration planner path without secrets, billing,
or external network access. It starts a local OpenAI-compatible streaming endpoint with `wiremock`,
points `LlmProvider::Custom` at that endpoint, and exercises the same `create_client()` +
`AgentScheduler::schedule()` path used by real providers.

## What it covers

- OpenAI-compatible streaming parsing through Omiga's normal LLM client.
- `/schedule`-style planning through the LLM planner path using `SchedulingStrategy::Auto`.
- `/team` planning with a terminal `team-verify` task and reviewer-family augmentation.
- `/autopilot` planning with reviewer-family augmentation.

## What it does not cover

- Real provider availability, authentication, billing, model quality, or latency.
- Full GUI/browser rendering.
- Worker tool execution against a real model.
- Browser-level click/clipboard/scroll behavior; TraceList callback tests assert React wiring only.

Keep using `scripts/real-llm-validation.sh all` for manual real-provider acceptance.

## Run locally

```bash
./scripts/mock-llm-validation.sh
```

Equivalent direct command:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --test mock_llm_runtime_harness --quiet
```

The harness is also included in the normal Rust test suite, so CI runs it via:

```bash
cargo test --manifest-path src-tauri/Cargo.toml
```

## Test target

- `src-tauri/tests/mock_llm_runtime_harness.rs`

The tests intentionally set `NO_PROXY=127.0.0.1,localhost` before creating the local client because
some developer environments export global HTTP proxy variables that would otherwise intercept the
local mock endpoint.

## Headless UI projection companion

TaskStatus timeline/trace mapping is protected by:

- `src/components/TaskStatus/orchestrationProjection.ts`
- `src/components/TaskStatus/orchestrationProjection.test.ts`
- `src/components/TaskStatus/OrchestrationTimelineList.tsx`
- `src/components/TaskStatus/OrchestrationTimelineList.test.tsx`
- `src/components/TaskStatus/OrchestrationTraceList.tsx`
- `src/components/TaskStatus/OrchestrationTraceList.test.tsx`

This covers schedule/team/autopilot event projection without a browser dependency:

- `/schedule`: `schedule_plan_created` and worker completion become clickable timeline rows.
- `/team`: `verification_started`, `fix_started`, and `synthesizing_started` remain visible and
  filterable in trace data.
- `/autopilot`: validation phase and reviewer blocker verdicts project to the expected timeline
  labels, severity tone, and transcript action.

This is still not a substitute for browser-rendered E2E; it protects the data projection layer that
feeds the TaskStatus UI. The SSR timeline test additionally proves the timeline component renders
the projected labels/details/action hint without requiring a DOM test environment or new test
dependencies. The TraceList SSR/callback test now also proves trace filters, related failure links,
timeline jumps, task-record opening, payload copying, expansion, and back-to-failures callbacks are
wired through a reusable component without adding a DOM test dependency.
