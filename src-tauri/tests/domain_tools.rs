//! Integration tests for `domain::tools` (moved from `src/domain/tools/*_tests.rs`).
//!
//! Submodules live under `domain_tools/`; `#[path]` is required because Rust resolves
//! `mod foo` from `tests/domain_tools.rs` to `tests/foo.rs`, not `tests/domain_tools/foo.rs`.

#[path = "domain_tools/fetch.rs"]
mod fetch;
#[path = "domain_tools/file_edit.rs"]
mod file_edit;
#[path = "domain_tools/grep.rs"]
mod grep;
#[path = "domain_tools/notebook_edit.rs"]
mod notebook_edit;
#[path = "domain_tools/query.rs"]
mod query;
#[path = "domain_tools/search.rs"]
mod search;
#[path = "domain_tools/todo_write.rs"]
mod todo_write;
