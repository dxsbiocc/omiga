---
name: cancel
description: Stop all running background agents, clear Ralph/Team state files, and report what was cancelled.
triggers:
  - cancel
  - stop
  - abort
  - 停止
  - 取消
  - 中止
---

# Cancel Skill

Stop all running background agents and clean up state.

## Steps

### Step 0 — Identify What Is Running

List all active background tasks:

```
TaskList (status: running)
```

Also check for state files:

```bash
ls .omiga/state/ 2>/dev/null && cat .omiga/state/*.json 2>/dev/null || echo "No state files found"
```

### Step 1 — Stop Background Agents

For each running task from Step 0:

```
TaskStop(task_id)
```

Stop ALL of them — do not ask for confirmation. The user said cancel.

### Step 2 — Clear State and Context Files

Remove Ralph/Team session state and context snapshots to prevent stale resumption:

```bash
rm -f .omiga/state/ralph-*.json .omiga/state/team-*.json 2>/dev/null
rm -f .omiga/context/ralph-*.md .omiga/context/team-*.md 2>/dev/null
echo "State and context files cleared"
```

### Step 3 — Report

Summarise what was stopped in one concise block:

```
Cancelled:
- <N> background agent(s) stopped: [task IDs or descriptions]
- State files cleared: [filenames or "none"]

All background work has been halted. Ready for new instructions.
```

If nothing was running:

```
Nothing was running. Ready for new instructions.
```

## Notes

- Never ask "are you sure" — cancel means cancel immediately.
- If `TaskList` returns an empty list and no state files exist, just confirm nothing was active.
- Do not start any new work during this skill — only stop and report.
