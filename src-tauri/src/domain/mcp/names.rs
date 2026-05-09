//! MCP tool name normalization — aligned with `normalizeNameForMCP` + `buildMcpToolName`
//! in `src/services/mcp/normalization.ts` and `mcpStringUtils.ts`.

/// Normalize names to `^[a-zA-Z0-9_-]{1,64}$`-friendly tokens (invalid chars → `_`).
#[must_use]
pub fn normalize_name_for_mcp(name: &str) -> String {
    const CLAUDEAI_PREFIX: &str = "claude.ai ";
    let mut normalized: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();

    if name.starts_with(CLAUDEAI_PREFIX) {
        let mut collapsed = String::new();
        let mut prev_us = false;
        for ch in normalized.chars() {
            if ch == '_' {
                if !prev_us {
                    collapsed.push('_');
                }
                prev_us = true;
            } else {
                prev_us = false;
                collapsed.push(ch);
            }
        }
        normalized = collapsed.trim_matches('_').to_string();
    }
    normalized
}

/// `mcp__{normalize(server)}__{normalize(tool)}`
#[must_use]
pub fn build_mcp_tool_name(server_name: &str, tool_name: &str) -> String {
    format!(
        "mcp__{}__{}",
        normalize_name_for_mcp(server_name),
        normalize_name_for_mcp(tool_name)
    )
}

/// MCP server + optional tool segment from a wire string (`mcpInfoFromString` in TS).
/// `mcp__server` → tool `None`; `mcp__server__tool` → tool `Some("tool")` (rest joined with `__`).
#[must_use]
pub fn mcp_info_from_string(tool_string: &str) -> Option<(String, Option<String>)> {
    let parts: Vec<&str> = tool_string.split("__").collect();
    if parts.len() < 2 {
        return None;
    }
    if parts.first().copied() != Some("mcp") {
        return None;
    }
    let server_name = parts[1].to_string();
    if server_name.is_empty() {
        return None;
    }
    if parts.len() == 2 {
        return Some((server_name, None));
    }
    Some((server_name, Some(parts[2..].join("__"))))
}

/// Parse a fully qualified MCP tool name into `(normalized_server, normalized_tool)`.
/// Tool segment may contain `__` (joined from remaining parts), matching TS `mcpInfoFromString`.
#[must_use]
pub fn parse_mcp_tool_name(full: &str) -> Option<(String, String)> {
    let (server, tool) = mcp_info_from_string(full)?;
    Some((server, tool?))
}

/// Computer Use uses MCP as an internal transport, but model-facing calls must
/// go through Omiga's `computer_*` facade so core policy, audit, stop handling,
/// and target-window revalidation cannot be bypassed.
#[must_use]
pub fn is_reserved_computer_mcp_tool(full: &str) -> bool {
    mcp_info_from_string(full)
        .is_some_and(|(server, tool)| server.eq_ignore_ascii_case("computer") && tool.is_some())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_replaces_invalid_chars() {
        assert_eq!(normalize_name_for_mcp("user-Figma"), "user-Figma");
        assert_eq!(normalize_name_for_mcp("a.b"), "a_b");
    }

    #[test]
    fn build_and_parse_roundtrip() {
        let fq = build_mcp_tool_name("my-server", "do_thing");
        assert_eq!(fq, "mcp__my-server__do_thing");
        let (s, t) = parse_mcp_tool_name(&fq).unwrap();
        assert_eq!(s, "my-server");
        assert_eq!(t, "do_thing");
    }

    #[test]
    fn detects_reserved_computer_tools() {
        assert!(is_reserved_computer_mcp_tool("mcp__computer__click"));
        assert!(is_reserved_computer_mcp_tool("mcp__Computer__click"));
        assert!(is_reserved_computer_mcp_tool("mcp__computer__type_text"));
        assert!(!is_reserved_computer_mcp_tool("mcp__computer"));
        assert!(!is_reserved_computer_mcp_tool("mcp__playwright__click"));
        assert!(!is_reserved_computer_mcp_tool("computer_click"));
    }

    #[test]
    fn mcp_info_server_only_and_wildcard() {
        let (s, t) = mcp_info_from_string("mcp__figma").unwrap();
        assert_eq!(s, "figma");
        assert!(t.is_none());
        let (s, t) = mcp_info_from_string("mcp__figma__*").unwrap();
        assert_eq!(s, "figma");
        assert_eq!(t.as_deref(), Some("*"));
    }
}
