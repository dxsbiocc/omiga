---
name: autopilot
description: Full autonomous execution from idea to working verified code — handles planning, implementation, testing, and validation
when_to_use: Use when the user wants hands-off end-to-end execution. Triggers on "autopilot", "autonomous", "build me", "create me", "make me", "full auto", "自动执行", "全自动".
tags:
  - orchestration
  - autonomous
  - full-pipeline
  - end-to-end
---

# Autopilot — Full Autonomous Pipeline

Autopilot takes a product idea and autonomously handles the full lifecycle: requirements analysis, technical design, planning, parallel implementation, QA cycling, and multi-perspective validation. It produces working, verified code from a brief description.

## When to Use

- User wants end-to-end autonomous execution from idea to working code
- User says "autopilot", "autonomous", "build me", "create me", "make me", "full auto", "handle it all", "自动执行", "全自动"
- Task requires multiple phases: planning, coding, testing, and validation
- User wants hands-off execution

## Do Not Use When

- User wants to explore or brainstorm — use `plan` skill instead
- User wants a single focused code change — use `ralph` or direct delegation
- User says "just explain", "draft only", "what would you suggest" — respond conversationally

## Execution Policy

- Each phase must complete before the next begins
- Parallel execution is used within phases where possible (Phase 2 and Phase 4)
- QA cycles repeat up to 5 times; if the same error persists 3 times, stop and report the fundamental issue
- Validation requires approval from all reviewers; rejected items get fixed and re-validated
- Do not enter execution phases until pre-context grounding exists

## Phases

### Phase 0 — Pre-context Intake

Create context snapshot at `.omiga/context/{slug}-{timestamp}.md`:
- Task statement and desired outcome
- Known facts and constraints
- Unknowns and open questions
- Likely codebase touchpoints

If ambiguity is high, run `deep-interview` first to gather requirements.

### Phase 1 — Expansion

Turn the user's idea into a detailed spec:
- Analyze existing codebase patterns (use Explore agent)
- Identify constraints and dependencies
- Define acceptance criteria (must be testable)
- Produce spec at `.omiga/plans/spec-{slug}.md`

### Phase 2 — Design (parallel)

Simultaneously:
- **Architect agent**: Technical architecture, component design, API contracts
- **Test engineer agent**: Test strategy, test cases to be written

Both agents produce their artifacts before Phase 3 begins.

### Phase 3 — Plan

Convert the design into an ordered task list:
- Break work into independent implementation tasks
- Order by dependency (foundational first)
- Assign each task to the best-fit agent type
- Identify which tasks can run in parallel

### Phase 4 — Implementation (parallel where possible)

Execute implementation tasks. Run independent tasks simultaneously. Each executor:
- Writes tests first (TDD approach)
- Implements to make tests pass
- Does not touch other agents' assigned areas

### Phase 5 — QA Cycle (up to 5 iterations)

Repeat until all tests pass:
1. Run full test suite
2. Run build
3. Run linter/type checker
4. If failures: fix and retry (max 5 cycles)
5. If same error persists 3× cycles: escalate to user

### Phase 6 — Validation (parallel reviewers)

Simultaneously delegate to:
- **Code reviewer**: Quality, maintainability, best practices
- **Security reviewer**: Security vulnerabilities, data exposure risks

Fix any CRITICAL or HIGH issues found. Re-validate after fixes.

### Phase 7 — Complete

All verification passes. Deliver:
- Summary of what was built
- Evidence (test output, build output)
- List of files created/modified
- Any known limitations or follow-up work

## State

Track progress in `.omiga/state/autopilot-state.json`:

```json
{
  "mode": "autopilot",
  "phase": "expansion",
  "spec_path": ".omiga/plans/spec-{slug}.md",
  "qa_cycles": 0,
  "max_qa_cycles": 5,
  "started_at": "<ISO timestamp>"
}
```

## Task

$ARGUMENTS
