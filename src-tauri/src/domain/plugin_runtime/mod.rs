//! Plugin runtime adapters shared by Omiga domain systems.
//!
//! `domain::plugins` owns plugin package discovery and manifest loading. Runtime
//! adapters live here so concrete systems such as retrieval can consume plugin
//! contributions without owning the generic plugin concept.

pub mod retrieval;
