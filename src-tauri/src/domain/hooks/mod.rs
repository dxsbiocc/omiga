//! Minimal tool lifecycle hook engine.
//!
//! Project hooks are loaded from `<project>/.omiga/hooks.toml`:
//!
//! ```toml
//! [[hooks]]
//! event = "PreToolUse"
//! command = "jq '.cmd = \"echo changed\"'"
//! timeout_ms = 5000
//!
//! [hooks.matcher]
//! tool_name = "bash"
//! ```
//!
//! Hook commands run as `bash -c <command>`. The engine writes one JSON event
//! object to stdin and captures stdout, stderr, and exit status.
//!
//! PreToolUse convention:
//! - non-zero exit status blocks the tool; stderr is used as the reason, then
//!   stdout, then a status fallback.
//! - zero exit status plus empty stdout proceeds unchanged.
//! - zero exit status plus stdout containing a JSON object rewrites the tool
//!   arguments to that object.
//!
//! PostToolUse convention:
//! - zero exit status plus non-empty stdout appends that text to the tool output
//!   returned to the model.
//! - empty stdout, non-zero status, spawn errors, and timeouts keep the original
//!   tool output.

pub mod engine;
pub mod schema;

pub use engine::{hook_config_path, HookEngine, PostHookOutcome, PreHookOutcome};
pub use schema::{HookDeclaration, HookEvent, HookMatcher};
