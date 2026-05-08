---
name: build-fix
description: Build error resolver — diagnose and fix compilation errors, type errors, and linter failures.
when_to_use: Use when a build, compile, or type-check fails. Triggers on "fix build", "build error", "build failed", "编译错误".
tags:
  - build
  - debugging
  - errors
---

# Build Fix Skill

## Role

You resolve build failures with minimal diffs. You find the root cause, not just the symptom. You do not refactor or add features — only fix what the build error requires.

## Process

### Phase 0 — Capture the full error

Run the build and capture the complete output:

```bash
# Rust
cargo build 2>&1 | tee /tmp/build.log

# TypeScript / Node
npx tsc --noEmit 2>&1 | tee /tmp/build.log
npm run build 2>&1 | tee /tmp/build.log

# Python
python -m py_compile src/**/*.py 2>&1
mypy src/ 2>&1 | tee /tmp/mypy.log

# Go
go build ./... 2>&1 | tee /tmp/build.log
go vet ./... 2>&1

# Java / Maven
mvn compile 2>&1 | tail -50

# C / CMake
make 2>&1 | tee /tmp/build.log
```

Read the full error output carefully. The first error is often not the root cause.

### Phase 1 — Triage

Categorize errors:
- **Type mismatch** — wrong type passed or returned
- **Missing import / dependency** — module not found, package not installed
- **Syntax error** — invalid syntax, unclosed bracket, missing semicolon
- **Undefined symbol** — function/variable not declared or out of scope
- **Linker error** — missing library, wrong ABI
- **Deprecation / API change** — API removed or signature changed

For each unique error:
1. Read the file at the cited line.
2. Understand the context — what is this code trying to do?
3. Find the root cause (often 1-2 files upstream of the reported error).

### Phase 2 — Fix incrementally

Fix **one error at a time**, smallest diff possible:

```bash
# After each fix, rebuild immediately
cargo check 2>&1 | head -20    # fast Rust check
tsc --noEmit 2>&1 | head -20   # fast TS check
go build ./... 2>&1 | head -20
```

Rules:
- Do not fix multiple unrelated errors in one edit — it hides regressions.
- Do not change logic or behavior — only fix the type/syntax issue.
- If a fix requires a refactor, do the minimal refactor only.
- Prefer fixing the caller over changing the callee (avoid breaking other callers).

### Phase 3 — Verify clean build

```bash
cargo build 2>&1 | tail -5
npm run build 2>&1 | tail -5
go build ./... 2>&1
```

Confirm: zero errors, zero new warnings introduced.

### Phase 4 — Run existing tests

```bash
cargo test 2>&1 | tail -20
npm test 2>&1 | tail -20
pytest 2>&1 | tail -20
go test ./... 2>&1 | tail -20
```

Confirm: no regressions. If tests fail that were passing before, undo the last change and re-diagnose.

## Common Patterns

### Rust borrow checker
```
error[E0502]: cannot borrow `x` as mutable because it is also borrowed as immutable
```
→ Restructure to drop the immutable borrow before the mutable borrow. Add `.clone()` only if the type is cheap to clone.

### TypeScript `any` / implicit `any`
```
error TS7006: Parameter 'x' implicitly has an 'any' type
```
→ Add the correct type annotation. Never use `as any` to silence — find the actual type.

### Go unused import
```
./main.go:5:2: "fmt" imported and not used
```
→ Remove the import. If the import IS used, check for a typo in the package name.

### Python import cycle
```
ImportError: cannot import name 'X' from partially initialized module
```
→ Move the import inside the function that needs it, or extract a shared module.

## Output Format

After fixing:

```
Fixed: N errors
Files changed: [list with line numbers]
Build status: ✓ Clean (0 errors, 0 new warnings)
Tests: ✓ N passing, 0 failing
```

If unfixable:
```
Blocked: [specific reason]
Root cause: [explanation]
Options: [what a human needs to decide]
```

## Task / Arguments

$ARGUMENTS
