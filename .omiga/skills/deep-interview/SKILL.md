---
name: deep-interview
description: Socratic requirements gathering — ask targeted questions to close ambiguity before any implementation starts
when_to_use: Use when requirements are vague or ambiguous. Triggers on "deep interview", "clarify requirements", "需求澄清", "interview me about". Use --quick for a faster, more compact pass.
tags:
  - planning
  - requirements
  - interview
  - clarification
---

# Deep Interview — Socratic Requirements Gathering

Close critical ambiguity gaps through targeted, one-at-a-time questions before any implementation begins. Produces a requirements spec that gates subsequent execution.

## When to Use

- Requirements are vague or contradictory
- User says "deep interview", "clarify requirements", "需求澄清", "what do I actually need"
- Autopilot or Ralph would be risky to run without more clarity
- Task touches multiple systems or has unclear success criteria

## Do Not Use When

- Requirements are already clear and specific — start planning or implementing directly
- User explicitly says "just do it" or "start coding"

## Modes

| Mode | Trigger | Description |
|------|---------|-------------|
| Full | Default | Complete Socratic interview, full spec output |
| Quick | `--quick` | Compact pass, 3-5 focused questions, abbreviated spec |

## Execution Policy

- Ask ONE question at a time — never batch multiple questions
- Questions must be concrete and answerable in 1-3 sentences
- Gather codebase facts via Explore agent BEFORE asking the user about them
- Stop interviewing when you have enough to write a complete spec
- Produce the spec BEFORE any implementation begins

## Interview Categories

Ask about (in order of importance):

1. **Goals and Success Criteria**: What does "done" look like? How will we know it works?
2. **Scope Boundaries**: What is explicitly out of scope? What must NOT change?
3. **Constraints**: Performance requirements? Backwards compatibility? Deadline?
4. **User Context**: Who uses this? What are their workflows?
5. **Edge Cases**: What happens when X fails? What about empty inputs?
6. **Integration Points**: What systems does this touch? What APIs must it use?

## Interview Flow

```
1. Analyze the request for ambiguity signals
2. Explore codebase facts (before asking user about them)
3. Ask most critical ambiguity-resolving question
4. Wait for user answer
5. If answer resolves the ambiguity: move to next question
6. If answer raises new ambiguity: follow up on that first
7. Stop when: all critical ambiguities resolved AND success criteria are testable
8. Generate spec document
```

## Spec Output

When interview is complete, produce a spec at `.omiga/plans/deep-interview-{slug}-{timestamp}.md`:

```markdown
# Spec: {Task Title}

## Stated Goal
{What the user asked for}

## Agreed Requirements
- Requirement 1 (testable acceptance criterion)
- Requirement 2 ...

## Success Criteria
- [ ] Criterion 1 (measurable)
- [ ] Criterion 2 ...

## Scope
### In Scope
- ...

### Out of Scope
- ...

## Constraints
- Performance: ...
- Backwards compatibility: ...
- Dependencies: ...

## Open Questions (resolved during interview)
- Q: {question} → A: {answer}

## Implementation Notes
{Key technical decisions agreed during interview}
```

## Quick Mode (`--quick`)

Run a compact 3-5 question pass covering only:
1. Core goal + success criteria
2. Scope boundaries (what must NOT change)
3. Biggest unknown or risk

Produce an abbreviated spec without the full format.

## Task / Context

$ARGUMENTS
