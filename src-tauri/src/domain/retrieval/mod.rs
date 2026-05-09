//! Unified retrieval kernel for `search`, `fetch`, and `query`.
//!
//! This module owns source-agnostic request/response types plus provider
//! dispatch. Concrete built-in adapters remain under `domain::search` while
//! plugin-backed sources are reached through Omiga's internal retrieval IPC.

pub mod core;
pub mod credentials;
pub mod normalize;
pub mod output;
pub mod providers;
pub mod registry;
pub mod tool_bridge;
pub mod types;

pub use core::{RetrievalCore, RetrievalProvider};
pub use types::{
    RetrievalError, RetrievalItem, RetrievalOperation, RetrievalProviderKind,
    RetrievalProviderOutput, RetrievalRequest, RetrievalResponse, RetrievalTool,
    RetrievalWebOptions,
};
