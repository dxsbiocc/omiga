---
name: ultraqa
description: QA cycling — systematically test all scenarios until all tests pass and quality gates are met
when_to_use: Use when you need thorough QA verification before a release or major merge. Triggers on "ultraqa", "qa cycle", "thorough testing", "全面测试", "质量验证".
tags:
  - qa
  - testing
  - quality
  - verification
---

# UltraQA — Systematic Quality Assurance

Run systematic QA cycles until all tests pass and all quality gates are met. Repeat until green or max cycles reached.

## When to Use

- Before a release or major merge
- After a large refactor that touches many files
- User says "ultraqa", "qa cycle", "thorough testing", "全面测试", "质量验证"
- Test suite has intermittent failures that need investigation

## Quality Gates (ALL must pass)

1. **Unit tests** — all pass, 0 failures
2. **Integration tests** — all pass, 0 failures
3. **Build** — compiles/builds successfully
4. **Type checker** — 0 errors
5. **Linter** — 0 errors (warnings acceptable)
6. **Coverage** — ≥80% on new code

## Process

### Step 1 — Baseline Assessment

Run ALL quality checks and capture the full output:
```bash
# Run test suite
npm test / cargo test / pytest / go test ./...

# Run build
npm run build / cargo build / go build ./...

# Run type checker
npx tsc --noEmit / cargo check / mypy . / go vet ./...

# Run linter
npm run lint / cargo clippy / ruff . / golangci-lint run
```

Record: total tests, pass count, fail count, error messages.

### Step 2 — Triage Failures

Categorize each failure:
- **Flaky test**: Passes sometimes, fails sometimes → investigate root cause
- **Real failure**: Consistently fails → fix the implementation
- **Environment issue**: Missing dependency, wrong config → fix environment
- **Test bug**: Test is wrong → fix the test (but verify it's actually wrong)

### Step 3 — Fix Cycle (max 5 cycles)

For each failure category:
1. Fix the highest-priority failures first (real failures > flaky > environment)
2. Re-run only the affected tests to confirm fix
3. Re-run ALL tests to confirm no regressions
4. Record the fix in the QA log

If the same failure persists after 3 fix cycles: escalate to user with diagnosis.

### Step 4 — Final Verification

After all fixes:
1. Run complete test suite from clean state
2. Run build from clean state
3. Run all quality checks
4. Confirm ALL gates pass

### Step 5 — QA Report

```markdown
# QA Report

## Summary
- Tests: X passed, 0 failed (was: X failed)
- Build: SUCCESS
- Type check: 0 errors
- Linter: 0 errors

## Fixes Applied
1. Fix description (file:line)
2. ...

## Remaining Issues (if any)
- Issue description → Reason not fixed / needs user decision
```

## Cycle Tracking

Track in `.omiga/state/ultraqa-state.json`:

```json
{
  "mode": "ultraqa",
  "cycle": 1,
  "max_cycles": 5,
  "gates_passed": ["unit_tests", "build"],
  "gates_pending": ["integration_tests", "type_check"],
  "failures": []
}
```

## Scope

$ARGUMENTS
