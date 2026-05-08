# Research Goal Migration Plan

## Requirements Summary

Migrate Codex's experimental `/goal` concept into Omiga as a research-oriented long-running objective layer. The feature should let a user set a persistent scientific goal, then repeatedly run analysis/interpreting cycles via the existing Research System until a completion audit passes, the goal is paused/cleared, or the cycle budget is exhausted.

Existing codebase anchors:
- Frontend slash commands live in `src/utils/workflowCommands.ts` and are consumed by `src/components/Chat/ChatComposer.tsx`.
- `/research` is handled as a special chat command in `src/components/Chat/index.tsx` and calls the Tauri command `run_research_command`.
- Tauri research commands live in `src-tauri/src/commands/chat/research.rs`.
- Research execution and JSON stores already exist under `src-tauri/src/domain/research_system/`.

## Decision

Implement `/goal` as a thin, research-specific command layer on top of the existing Research System, not as a Codex app-server protocol clone. Persist one active goal per session/project in `.research/goals/{session_id}.json`, and persist cycle snapshots under `.research/goal-runs/{goal_id}/`.

## Command Contract

- `/goal <objective>`: set or replace the session research goal.
- `/goal run`: continue the active goal for one bounded analysis cycle.
- `/goal status`: show current goal state and audit summary.
- `/goal pause`: pause automatic/manual continuation.
- `/goal resume`: resume a paused goal.
- `/goal clear`: remove the current goal.
- `/goal help`: show usage.

MVP defaults:
- One goal per chat session.
- Default `maxCycles = 3`.
- Each `/goal run` executes one Research System cycle. The stored goal can be run repeatedly until completion.
- Completion is determined by an LLM-backed structured scientific audit over Research System output and user-provided/default criteria. Deterministic checks are limited to hard execution-validity blockers (failed run, missing `final_output`, or failed reviewer signals), not semantic completion heuristics.

## Data Model

`ResearchGoal`:
- `goalId`, `sessionId`, `objective`
- `status`: `active | paused | budget_limited | complete`
- `successCriteria[]`
- `successCriterionIds[]`: stable IDs aligned with `successCriteria[]`, used by LLM audits so the model does not need to repeat criteria text exactly
- `secondOpinionProviderEntry`: optional per-goal named LLM provider entry for independent completion review
- `autoRunPolicy`: persisted per-goal automatic continuation policy (`enabled`, `cyclesPerRun`, `idleDelayMs`, optional `maxElapsedMinutes`, optional `maxTokens`, `startedAt`)
- `tokenUsage`: accumulated normalized token usage for Research System execution plus `/goal` LLM audit calls
- `maxCycles`, `currentCycle`
- `evidenceRefs[]`, `artifactRefs[]`
- `lastAudit`, `notes[]`
- `createdAt`, `updatedAt`, `lastRunAt`

`ResearchGoalCycle`:
- `cycleId`, `goalId`, `cycleIndex`
- continuation request sent to Research System
- Research System graph/status/output summary
- audit result
- normalized token usage breakdown: Research System, LLM audit/second opinion, total
- timestamp

## Implementation Steps

1. Add a backend domain module `src-tauri/src/domain/research_system/goals.rs`.
   - JSON file store for goals and cycles.
   - command-body parser for `/goal` subcommands.
   - continuation prompt builder tailored to scientific analysis → interpretation → re-analysis.
   - LLM completion audit with explicit missing requirements, limitations, conflicting evidence, and next actions.

2. Add Tauri command module `src-tauri/src/commands/chat/research_goal.rs`.
   - Mirror `/research` persistence behavior: save user message, create round, save assistant message, emit orchestration event.
   - Expose `run_research_goal_command`.

3. Register command in `src-tauri/src/lib.rs` and export it from `src-tauri/src/commands/chat/mod.rs`.

4. Add frontend slash command support.
   - Extend `WORKFLOW_SLASH_COMMANDS` with `/goal`.
   - Add `parseGoalCommand`.
   - Treat `/goal` as a special non-streaming command like `/research` in `src/components/Chat/index.tsx`.
   - Render `/goal` as a command chip in user bubbles.

5. Add tests.
   - Rust tests for goal command parsing, goal lifecycle, audit behavior, and persistence.
   - Frontend tests for `/goal` parser and user-bubble chip rendering.

## Acceptance Criteria

- Typing `/goal <objective>` creates a persisted active research goal and writes assistant confirmation into the chat transcript.
- `/goal status`, `/goal pause`, `/goal resume`, `/goal clear`, and `/goal run` all return deterministic assistant messages.
- `/goal run` executes the existing Research System, calls the configured LLM for structured completion audit, and stores a cycle record.
- Completion can require an independent second-opinion LLM; the per-goal settings dialog can validate the selected provider with a real lightweight LLM probe before long runs.
- Goal state survives process reload because it is saved under `.research/goals/`.
- User-visible assistant output includes current status, cycle count, audit summary, missing requirements, and next actions.
- Per-goal auto-run settings survive session reload and are bounded by cycle budget, per-run cycle count, idle delay, optional elapsed-time cap, and optional token cap.
- Unit tests cover parser and lifecycle behavior.
- `bun run test` and targeted Rust tests pass.

## Risks and Mitigations

- **Risk: premature scientific completion.** Mitigation: use LLM semantic audit fields and do not mark complete when reviewer results fail, output is missing, criteria are uncovered, or the LLM does not set `finalReportReady=true`.
- **Risk: LLM unavailable.** Mitigation: `/goal run` fails loudly instead of falling back to heuristic completion; non-run commands (`status`, `pause`, settings, etc.) remain available.
- **Risk: long-running UI block.** Mitigation: auto-run remains idle-gated in the Chat UI, persists a bounded policy, and uses `/goal run --cycles N` only when the main session is not busy.
- **Risk: duplicating Research System state.** Mitigation: goal cycles store only pointers/summaries; Research System artifacts remain in `.research/artifacts`, `.research/evidence`, and `.research/traces`.
- **Risk: token budget is not real yet.** Mitigation: name MVP budget as `maxCycles`; leave token accounting as a follow-up.

## Verification Steps

- `cargo test --manifest-path src-tauri/Cargo.toml research_goal`
- `bun run test -- src/utils/workflowCommands.test.ts src/components/Chat/UserMessageBubble.test.tsx`
- `bun run build`

## Progress

- Implemented MVP `/goal` parser, backend persistence, Research System continuation, command bridge, frontend command routing, and command-chip rendering.
- Added a read-only goal status command and Chat composer status pill so active goals remain visible without writing extra chat messages.
- Added editable success criteria UI and a persistence command; saving new criteria clears stale completion audit so the next `/goal run` re-evaluates against the updated standard.
- Added configurable cycle budget and bounded auto-continuation: `/goal budget N`, `/goal run --cycles N`, and the settings dialog now edit `maxCycles`.
- Replaced deterministic success-criteria suggestions with LLM-generated criteria. The settings dialog now calls the configured model and fails loudly instead of using heuristic fallback when generation is unavailable.
- Added explicit idle auto-run control in the goal status pill: users can enable bounded automatic continuation, and Chat only fires `/goal run --cycles N` while the main session is idle.
- Replaced heuristic completion audit with configured-LLM structured scientific audit. `/goal run` now records `reviewSource=llm`, confidence, `finalReportReady`, limitations, and conflicting evidence, and errors instead of using heuristic fallback when LLM audit is unavailable.
- Added stable criterion IDs for success criteria. LLM audit now matches completion coverage by `criterionId`, so short paraphrases or translated snippets do not fail solely because the model did not repeat the criterion text verbatim.
- Added a frontend audit details dialog from the goal status pill. Users can inspect success-criteria coverage, missing requirements, next actions, limitations, and conflicting evidence without re-running `/goal status`.
- Added second-opinion LLM completion gating. When the primary audit says complete, `/goal run` performs a separate stricter LLM review; disagreement blocks completion and records `secondOpinion` concerns/actions in the audit details.
- Added optional independent second-opinion model/provider configuration. Set `OMIGA_GOAL_SECOND_OPINION_PROVIDER_ENTRY` to a named `omiga.yaml` provider entry, or use `OMIGA_GOAL_SECOND_OPINION_PROVIDER` / `MODEL` / `API_KEY` / `BASE_URL`; otherwise the second opinion reuses the primary model. If the provider differs from the primary LLM, an explicit second-opinion API key is required.
- Added Settings → Advanced UI for `/goal` second-opinion provider entry. The UI persists `settings.goal_second_opinion_provider_entry` in `omiga.yaml`, and `/goal run` reads it before falling back to environment variables.
- Added per-goal second-opinion provider overrides. The goal settings dialog can save `secondOpinionProviderEntry` on the active research goal, so different科研目标 can use different independent review models while still falling back to the global Advanced setting.
- Added save-time provider-entry validation for both global and per-goal `/goal` second-opinion settings. Invalid, disabled, missing, or keyless provider entries are rejected before they can be persisted.
- Added a per-goal provider entry picker. The research-goal settings dialog now loads enabled Model-page provider entries via `list_provider_configs`, supports free-form fallback, and still relies on backend save-time validation.
- Added a "test second-opinion provider" action in the per-goal settings dialog. The button calls `test_research_goal_second_opinion_provider`, validates the selected provider entry, creates its client, and runs a real lightweight no-tools LLM JSON probe before a long `/goal run`.
- Added persisted per-goal auto-run policy. The goal settings dialog now saves enabled/disabled state, per-run cycle cap, idle delay, and optional elapsed-time/token budgets into the goal JSON; the status pill resumes that policy after reload instead of relying on local React state.
- Added normalized `/goal` token accounting. Each cycle now stores Research System token usage plus primary/second-opinion LLM audit usage; the goal accumulates totals and auto-run can stop when the persisted token cap is reached.

## Follow-ups

- Add provider-specific cost estimation once pricing metadata exists; current budgeting is token-count only.
- Surface the successful provider probe summary in the UI if users need more diagnostic detail than provider/model/latency.
