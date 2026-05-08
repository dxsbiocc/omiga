//! Internal retrieval plugin support.
//!
//! Phase 1 adds manifest parsing plus stdio JSONL process execution here.

pub mod ipc;
pub mod lifecycle;
pub mod manifest;
pub mod process;
pub mod validation;
