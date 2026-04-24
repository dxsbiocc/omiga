---
name: plan
description: Strategic planning with structured requirements gathering before any code is written
when_to_use: Use when the user wants to plan before implementing. Triggers on "plan this", "let's plan", "规划", "制定计划", "plan the".
tags:
  - planning
  - requirements
  - design
---

# Plan — Strategic Planning Skill

Plan creates comprehensive, actionable work plans through structured requirements gathering. It auto-detects whether to interview the user (broad/vague requests) or plan directly (detailed/specific requests).

## When to Use

- User wants to plan before implementing — "plan this", "plan the", "let's plan", "规划", "制定计划"
- Task is broad or vague and needs scoping before any code is written
- User wants structured requirements gathering
- User wants an existing plan reviewed

## Do Not Use When

- User wants autonomous end-to-end execution — use `autopilot` instead
- User wants to start coding immediately with a clear task — use `ralph` or delegate to executor
- Task is a single focused fix with obvious scope — skip planning, just do it

## Mode Selection

| Mode | Trigger | Behavior |
|------|---------|----------|
| Interview | Default for broad/vague requests | Interactive requirements gathering |
| Direct | `--direct` or detailed request | Skip interview, generate plan directly |
| Consensus | `--consensus` | Planner → Architect → Critic loop until agreement |
| Review | `--review` | Critic evaluation of an existing plan |

## Execution Policy

- Auto-detect interview vs direct mode based on request specificity
- Ask ONE question at a time during interviews — never batch multiple questions
- Gather codebase facts via Explore agent BEFORE asking the user about them
- Plans must meet quality standards: 80%+ claims cite file/line evidence, 90%+ criteria are testable
- Implementation step count must be right-sized to task scope

## Steps

### Interview Mode (broad/vague requests)

1. **Classify the request**: Broad (vague verbs, no specific files, touches 3+ areas) → interview mode
2. **Gather codebase facts first**: Spawn Explore agent to understand the codebase before asking the user
3. **Ask one focused question** at a time: preferences, scope, constraints, success criteria
4. **Synthesize** when enough information is gathered: proceed to plan generation

### Direct Mode (detailed requests)

1. **Explore codebase**: Understand existing patterns, conventions, file structure
2. **Generate plan** with:
   - Problem statement and goals
   - Technical approach (with file/line citations)
   - Implementation phases with ordered steps
   - Success criteria (all testable)
   - Risk assessment and mitigation
   - Estimated complexity and scope

### Consensus Mode (`--consensus`)

Multi-perspective validation loop:
1. **Planner**: Generate initial plan
2. **Architect**: Technical review — feasibility, architecture, risks
3. **Critic**: Challenge assumptions, identify gaps, edge cases
4. Loop until all three agree on the plan

### Review Mode (`--review`)

Critic agent evaluates an existing plan for:
- Feasibility and completeness
- Missing edge cases or risks
- Testability of acceptance criteria
- Scope creep or under-scoping

## Plan Output Format

```markdown
# Plan: {Task Title}

## Problem Statement
...

## Goals
- [ ] Goal 1 (testable)
- [ ] Goal 2 (testable)

## Technical Approach
### Phase 1: {Name}
1. Step 1 (file: path/to/file.rs:42)
2. Step 2 ...

### Phase 2: {Name}
...

## Success Criteria
- [ ] All existing tests pass
- [ ] New tests cover {feature}
- [ ] Build succeeds with 0 errors

## Risks
- Risk 1: {description} → Mitigation: {approach}

## Scope
- In scope: ...
- Out of scope: ...
```

## Task / Arguments

$ARGUMENTS
