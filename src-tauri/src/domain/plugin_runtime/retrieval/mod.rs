//! Retrieval plugin runtime adapter.
//!
//! This module owns the retrieval-specific JSONL child-process protocol. It is
//! intentionally outside `domain::retrieval` so retrieval can consume plugin
//! contributions without owning the broader plugin package model.

pub mod ipc;
pub mod lifecycle;
pub mod manifest;
pub mod process;
pub mod validation;
