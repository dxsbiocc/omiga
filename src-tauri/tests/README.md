# Integration tests (`omiga/src-tauri/tests/`)

## Running

```bash
cd omiga/src-tauri

# Default for CI / fast feedback (domain + lib unit tests)
cargo test --lib

# Includes this crate’s integration tests (currently `smoke.rs` only)
cargo test
```

## History

The previous `session_flow_integration_tests` + `common/` harness targeted an older
`SessionRepository` API (e.g. `create_round(..., RoundStatus)`). It was removed so
`cargo test` stays green. Re-add flow tests against the **current** signatures in
`src/domain/persistence/mod.rs` when you need end-to-end DB coverage.

## Smoke test

`smoke.rs` only checks that integration tests link to `omiga_lib` (regression guard for the old
`app_lib` crate-name typo).
