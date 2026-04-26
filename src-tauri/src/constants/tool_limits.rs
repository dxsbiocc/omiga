//! Tool result size limits — mirrors `src/constants/toolLimits.ts` and
//! `PREVIEW_SIZE_BYTES` from `src/utils/toolResultStorage.ts`.
//!
//! **Behavior reference (TS is authoritative):**
//! - Large-result file spill: `maybePersistLargeToolResult` / `persistToolResult` in
//!   `src/utils/toolResultStorage.ts`
//! - Disable file spill, use truncation only: `ENABLE_MCP_LARGE_OUTPUT_FILES` +
//!   `isEnvDefinedFalsy` in `src/services/mcp/client.ts` (`processMCPResult`)
//! - Persist failure (MCP path): error string + pagination hint, not silent huge payload
//!   (`isPersistError` branch in `processMCPResult`)
//!
// Keep numeric values in sync with:
// - claude-code-main/src/constants/toolLimits.ts
// - claude-code-main/src/utils/toolResultStorage.ts (PREVIEW_SIZE_BYTES)

/// Default maximum size in characters for tool results before they get persisted
/// to disk. When exceeded, the result should be saved to a file and the model
/// receives a preview with the file path instead of the full content.
pub const DEFAULT_MAX_RESULT_SIZE_CHARS: usize = 50_000;

/// Maximum size for tool results in tokens (TS: `MAX_TOOL_RESULT_TOKENS`).
pub const MAX_TOOL_RESULT_TOKENS: u64 = 100_000;

/// Bytes per token estimate for calculating token count from byte size.
pub const BYTES_PER_TOKEN: u64 = 4;

/// Maximum size for tool results in bytes (derived from token limit).
pub const MAX_TOOL_RESULT_BYTES: u64 = MAX_TOOL_RESULT_TOKENS * BYTES_PER_TOKEN;

/// Maximum aggregate size in characters for tool_result blocks within a single
/// user message (one turn's batch of parallel tool results).
pub const MAX_TOOL_RESULTS_PER_MESSAGE_CHARS: usize = 200_000;

/// Maximum character length for tool summary strings in compact views.
pub const TOOL_SUMMARY_MAX_LENGTH: usize = 50;

/// Preview size in bytes for persisted large results and UI preview snippets.
pub const PREVIEW_SIZE_BYTES: usize = 2000;

/// Max characters for tool argument echo in streamed tool UI (TS: `MCPTool/UI.tsx` `maxChars: 2000`).
pub const TOOL_DISPLAY_MAX_INPUT_CHARS: usize = 2000;

/// Truncate UTF-8 `s` to at most `max_bytes` bytes without splitting a codepoint.
/// Matches TS `generatePreview` behavior when taking a prefix by byte length.
#[must_use]
pub fn truncate_utf8_prefix(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// Mirrors `isEnvDefinedFalsy` from `src/utils/envUtils.ts` (used for feature gates).
/// `None` / unset → **not** falsy (feature stays on). `Some("0"|"false"|"no"|"off")` → falsy.
#[must_use]
pub fn is_env_defined_falsy(var: Option<&str>) -> bool {
    match var {
        None => false,
        Some("") => false,
        Some(s) => matches!(s.to_lowercase().trim(), "0" | "false" | "no" | "off"),
    }
}

/// When `false`, skip writing large tool output to disk and use in-memory truncation only
/// (same idea as `ENABLE_MCP_LARGE_OUTPUT_FILES` in `src/services/mcp/client.ts`).
/// Unset → enabled (`isEnvDefinedFalsy(undefined)` is false in TS).
///
/// Set to `0` in sandboxes or environments where the filesystem denies writes under
/// the session directory (then match MCP: `truncateMcpContentIfNeeded`-style path instead of persist).
#[must_use]
pub fn large_tool_output_files_enabled() -> bool {
    !is_env_defined_falsy(
        std::env::var("OMIGA_ENABLE_LARGE_TOOL_OUTPUT_FILES")
            .ok()
            .as_deref(),
    )
}

/// User-visible fallback when persist fails (align with `processMCPResult` in
/// `src/services/mcp/client.ts`, `persist_failed` branch): explicit error + hint, not unbounded text.
///
/// On **success**, use [`crate::utils::large_output_instructions::get_large_output_instructions`]
/// so the model reads from file in chunks instead of receiving the full payload inline.
#[must_use]
pub fn large_output_persist_failed_message(char_count: usize, error: &str) -> String {
    format!(
        "Error: result ({char_count} characters) exceeds maximum allowed size. Failed to save output to file: {error}. \
If the tool supports pagination or filtering, use them to retrieve a smaller portion."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_utf8_prefix_ascii() {
        let s = "a".repeat(PREVIEW_SIZE_BYTES + 100);
        assert_eq!(
            truncate_utf8_prefix(&s, PREVIEW_SIZE_BYTES).len(),
            PREVIEW_SIZE_BYTES
        );
    }

    #[test]
    fn truncate_utf8_prefix_multibyte_boundary() {
        let s = "你好".repeat(2000);
        let t = truncate_utf8_prefix(&s, PREVIEW_SIZE_BYTES);
        assert!(t.len() <= PREVIEW_SIZE_BYTES);
        assert!(s.starts_with(t));
    }

    #[test]
    fn is_env_defined_falsy_matches_ts() {
        assert!(!is_env_defined_falsy(None));
        assert!(!is_env_defined_falsy(Some("")));
        assert!(is_env_defined_falsy(Some("0")));
        assert!(is_env_defined_falsy(Some("false")));
        assert!(is_env_defined_falsy(Some(" OFF ")));
    }
}
