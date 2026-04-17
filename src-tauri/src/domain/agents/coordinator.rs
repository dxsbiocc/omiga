//! Coordinator mode — multi-agent orchestration (Claude Code `COORDINATOR_MODE` parity).
//!
//! When enabled via `OMIGA_COORDINATOR_MODE` or `CLAUDE_CODE_COORDINATOR_MODE`, the main chat
//! session receives a coordinator-style system addendum and only the tools that match upstream
//! `COORDINATOR_MODE_ALLOWED_TOOLS`: `Agent`, `TaskStop`, `SendUserMessage`, `TaskOutput`
//! (Omiga names; TS uses `SendMessage` / `SyntheticOutput`).

use std::collections::HashSet;

use crate::domain::tools::ToolSchema;

/// Tool names exposed to the model in coordinator mode (must match `ToolSchema::name` from each tool's `schema()`).
pub const COORDINATOR_ALLOWED_TOOL_NAMES: &[&str] =
    &["Agent", "TaskStop", "SendUserMessage", "TaskOutput"];

/// `OMIGA_COORDINATOR_MODE` takes precedence when set; otherwise `CLAUDE_CODE_COORDINATOR_MODE` is read.
/// Truthy: `1`, `true`, `yes`, `on` (ASCII case-insensitive). Any other non-empty value is false.
pub fn is_coordinator_mode() -> bool {
    if std::env::var_os("OMIGA_COORDINATOR_MODE").is_some() {
        return std::env::var("OMIGA_COORDINATOR_MODE")
            .map(|v| parse_truthy(&v))
            .unwrap_or(false);
    }
    std::env::var("CLAUDE_CODE_COORDINATOR_MODE")
        .map(|v| parse_truthy(&v))
        .unwrap_or(false)
}

fn parse_truthy(raw: &str) -> bool {
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

/// Keep only coordinator-allowed built-in tools; drops MCP and all other built-ins.
pub fn filter_coordinator_tool_schemas(mut schemas: Vec<ToolSchema>) -> Vec<ToolSchema> {
    let allowed: HashSet<&str> = COORDINATOR_ALLOWED_TOOL_NAMES.iter().copied().collect();
    schemas.retain(|s| allowed.contains(s.name.as_str()));
    schemas.sort_by(|a, b| a.name.cmp(&b.name));
    schemas
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_truthy_values() {
        assert!(parse_truthy("1"));
        assert!(parse_truthy("true"));
        assert!(parse_truthy("YES"));
        assert!(parse_truthy("  On  "));
        assert!(!parse_truthy("0"));
        assert!(!parse_truthy("false"));
        assert!(!parse_truthy("no"));
        assert!(!parse_truthy(""));
    }

    #[test]
    fn filter_keeps_only_allowlist() {
        let schemas = vec![
            ToolSchema::new("Agent", "a", serde_json::json!({})),
            ToolSchema::new("bash", "b", serde_json::json!({})),
            ToolSchema::new("mcp__x__y", "m", serde_json::json!({})),
            ToolSchema::new("TaskStop", "t", serde_json::json!({})),
        ];
        let out = filter_coordinator_tool_schemas(schemas);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].name, "Agent");
        assert_eq!(out[1].name, "TaskStop");
    }
}
