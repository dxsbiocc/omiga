//! Unified search/fetch adapters for built-in `search` and `fetch` tools.
//!
//! The public tool surface is intentionally `search` / `fetch`; provider-specific
//! implementations live here so source adapters can be extended without adding new
//! model-visible tool names.

pub mod pubmed;

pub mod literature;

pub mod data;

pub mod semantic_scholar;

pub mod wechat;
