---
name: tdd
description: Test-Driven Development — write failing tests first, then implement to pass them.
when_to_use: Use when adding a new feature, fixing a bug, or refactoring. Triggers on "write tests first", "tdd", "test driven", "测试驱动".
tags:
  - testing
  - tdd
  - quality
  - development
---

# TDD — Test-Driven Development Skill

## Role

You enforce the Red → Green → Refactor cycle. No implementation code is written until a failing test exists that demands it.

## Mandatory Workflow

### Phase 0 — Understand the scope

```bash
# Understand existing test patterns
find . -name "*.test.*" -o -name "*_test.*" -o -name "*.spec.*" | head -20
ls tests/ src/**/__tests__/ 2>/dev/null | head -20
# Understand the target code area
```

Spawn an **Explore** agent to read the relevant source files and existing tests before writing anything.

### Phase 1 — RED: Write the failing test

1. Write the smallest test that captures the requirement.
2. Run the test suite — confirm the new test **fails** with the expected error.
3. Do NOT write implementation code yet.

```bash
# Run tests — expect failure
cargo test <test_name> 2>&1 | tail -20        # Rust
pytest tests/test_feature.py -v               # Python
npm test -- --testPathPattern=feature         # JS/TS
go test ./... -run TestFeature                # Go
```

Verify: test output shows the new test failing, existing tests still pass.

### Phase 2 — GREEN: Write minimal implementation

Write the **minimum code** to make the failing test pass. No extra features, no over-engineering.

```bash
# Run tests again — all must pass
cargo test 2>&1 | tail -20
pytest 2>&1 | tail -10
npm test 2>&1 | tail -10
```

Verify: new test passes, no regressions in other tests.

### Phase 3 — REFACTOR: Clean up without breaking tests

1. Improve readability, remove duplication, rename for clarity.
2. Run full test suite after each refactor change.
3. Tests must stay green throughout.

### Phase 4 — Coverage check

```bash
cargo tarpaulin --out Stdout 2>/dev/null | grep "coverage"     # Rust
pytest --cov=src --cov-report=term-missing 2>&1 | tail -20    # Python
npm test -- --coverage 2>&1 | grep "Statements\|Lines"        # JS/TS
go test -cover ./... 2>&1 | tail -10                          # Go
```

Target: 80%+ coverage on modified files. Report coverage delta.

## Rules

- **Never** skip Phase 1. If you cannot write a failing test, clarify requirements first.
- **Never** write more implementation than the current failing test requires.
- Each test should test **one thing** — small, fast, isolated.
- Use real assertions, not just `assert True`.
- Name tests `test_<what>_<when>_<expected>`.

## Output Format

After each phase, report:

```
Phase: RED / GREEN / REFACTOR
Tests: N passing, M failing
New test: test_name — [FAIL ✗ / PASS ✓]
Coverage: X% (+Y% delta)
Next: [what to do next]
```

## Task / Arguments

$ARGUMENTS
