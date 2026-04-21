//! Builtin agent prompt loader
//!
//! Separates prompt *content* from Rust agent metadata, mirroring the oh-my-codex
//! `prompts/*.md` pattern: metadata (model tier, tools, color) lives in Rust; the
//! system prompt body lives in markdown files that users can edit freely.
//!
//! ## File format
//!
//! ```markdown
//! ---
//! description: Optional one-line description (ignored at load time, for human reference)
//! tools: [web_search, web_fetch]   # optional — overrides allowed_tools
//! model: standard                  # optional — overrides model tier alias
//! color: "#8b5cf6"                 # optional — overrides UI colour
//! ---
//!
//! System prompt body goes here.
//! ```
//!
//! The YAML frontmatter block is optional.  Everything after the closing `---`
//! (or the entire file if there is no frontmatter) becomes the system prompt.
//!
//! ## Loading chain (first match wins)
//!
//! 1. `<project_root>/.omiga/agents/<type>.md`   — project-level override
//! 2. `~/.omiga/agents/<type>.md`                — user-global override
//! 3. Bundled default (`src-tauri/src/domain/agents/markdown/<type>.md`)
//!
//! User-defined or third-party agents live in the same directories (steps 1–2).
//! There is no longer a separate `builtins/` subdirectory.
//!
//! Steps 2-3 are what this module resolves.  Step 1 is transparent because the
//! hot-reload registry will already have replaced the builtin before `system_prompt()`
//! is ever called on the Rust struct.

use serde::Deserialize;
use std::path::Path;

// ── Frontmatter ───────────────────────────────────────────────────────────────

/// Optional metadata that the `.md` file may override.
#[derive(Debug, Default, Deserialize)]
pub struct PromptFileMeta {
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub tools: Option<Vec<String>>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub color: Option<String>,
}

/// Parsed result of a prompt markdown file.
#[derive(Debug)]
pub struct PromptFile {
    /// Metadata extracted from the YAML frontmatter block (may be all-None).
    pub meta: PromptFileMeta,
    /// System prompt body (everything after the frontmatter).
    pub prompt: String,
}

// ── Parsing ──────────────────────────────────────────────────────────────────

/// Parse a markdown file that optionally starts with a YAML frontmatter block
/// delimited by `---` lines.
fn parse_prompt_file(content: &str) -> PromptFile {
    let content = content.trim_start();
    if content.starts_with("---") {
        // Find the closing ---
        let after_open = &content[3..];
        if let Some(close_pos) = after_open.find("\n---") {
            let yaml_src = after_open[..close_pos].trim();
            let body = after_open[close_pos + 4..].trim_start();
            let meta: PromptFileMeta = serde_yaml::from_str(yaml_src).unwrap_or_default();
            return PromptFile {
                meta,
                prompt: body.to_string(),
            };
        }
    }
    // No frontmatter — entire file is the prompt.
    PromptFile {
        meta: PromptFileMeta::default(),
        prompt: content.to_string(),
    }
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Try to load an agent's prompt from the file override chain.
///
/// Returns `None` if no override file is found; the caller should then fall
/// back to the bundled default.
///
/// Checks in order:
/// 1. `<project_root>/.omiga/agents/<type>.md`
/// 2. `~/.omiga/agents/<type>.md`
pub fn load_builtin_prompt(agent_type: &str, project_root: &Path) -> Option<String> {
    load_builtin_override(agent_type, project_root).map(|pf| pf.prompt)
}

/// Like [`load_builtin_prompt`] but returns the full [`PromptFile`] including
/// any metadata overrides declared in the frontmatter.
pub fn load_builtin_override(agent_type: &str, project_root: &Path) -> Option<PromptFile> {
    let filename = format!("{}.md", agent_type);

    // 1. Project-level override
    let project_path = project_root.join(".omiga").join("agents").join(&filename);
    if let Ok(content) = std::fs::read_to_string(&project_path) {
        tracing::debug!(
            target: "omiga::prompt_loader",
            path = %project_path.display(),
            "loaded agent prompt override (project level)"
        );
        return Some(parse_prompt_file(&content));
    }

    // 2. User-global override
    if let Some(home) = dirs::home_dir() {
        let global_path = home.join(".omiga").join("agents").join(&filename);
        if let Ok(content) = std::fs::read_to_string(&global_path) {
            tracing::debug!(
                target: "omiga::prompt_loader",
                path = %global_path.display(),
                "loaded agent prompt override (user-global level)"
            );
            return Some(parse_prompt_file(&content));
        }
    }

    None
}

/// Return the prompt body from the bundled `include_str!` constant for `agent_type`.
/// Falls back to an empty string if the type is not in the bundled list.
pub fn bundled_prompt(agent_type: &str) -> String {
    BUNDLED_DEFAULTS
        .iter()
        .find(|(t, _)| *t == agent_type)
        .map(|(_, content)| parse_prompt_file(content).prompt)
        .unwrap_or_default()
}

/// Load a builtin prompt (file override → bundled default) and substitute
/// `{cwd}` with the project root path.
///
/// This is the preferred single call-site for all builtin `system_prompt()` impls.
pub fn resolve(agent_type: &str, project_root: &std::path::Path) -> String {
    let raw =
        load_builtin_prompt(agent_type, project_root).unwrap_or_else(|| bundled_prompt(agent_type));
    raw.replace("{cwd}", &project_root.display().to_string())
}

// ── Bundled default files ─────────────────────────────────────────────────────
//
// Canonical prompts co-located with the Rust source in
// `src-tauri/src/domain/agents/markdown/`.  Users override them by placing a
// file at `.omiga/agents/<type>.md` (project) or `~/.omiga/agents/<type>.md`
// (global).  User-defined and third-party agents live in the same directories.

pub const EXECUTOR_PROMPT: &str = include_str!("markdown/executor.md");
pub const ARCHITECT_PROMPT: &str = include_str!("markdown/architect.md");
pub const DEBUGGER_PROMPT: &str = include_str!("markdown/debugger.md");
pub const EXPLORE_PROMPT: &str = include_str!("markdown/explore.md");
pub const PLAN_PROMPT: &str = include_str!("markdown/plan.md");
pub const VERIFICATION_PROMPT: &str = include_str!("markdown/verification.md");
pub const GENERAL_PROMPT: &str = include_str!("markdown/general-purpose.md");
pub const LITERATURE_SEARCH_PROMPT: &str = include_str!("markdown/literature-search.md");
pub const DEEP_RESEARCH_PROMPT: &str = include_str!("markdown/deep-research.md");
pub const DATA_ANALYSIS_PROMPT: &str = include_str!("markdown/data-analysis.md");
pub const DATA_VISUAL_PROMPT: &str = include_str!("markdown/data-visual.md");
pub const CODE_REVIEWER_PROMPT: &str = include_str!("markdown/code-reviewer.md");
pub const API_REVIEWER_PROMPT: &str = include_str!("markdown/api-reviewer.md");
pub const CRITIC_PROMPT: &str = include_str!("markdown/critic.md");
pub const SECURITY_REVIEWER_PROMPT: &str = include_str!("markdown/security-reviewer.md");
pub const PERFORMANCE_REVIEWER_PROMPT: &str = include_str!("markdown/performance-reviewer.md");
pub const QUALITY_REVIEWER_PROMPT: &str = include_str!("markdown/quality-reviewer.md");
pub const TEST_ENGINEER_PROMPT: &str = include_str!("markdown/test-engineer.md");

const BUNDLED_DEFAULTS: &[(&str, &str)] = &[
    ("executor", EXECUTOR_PROMPT),
    ("architect", ARCHITECT_PROMPT),
    ("debugger", DEBUGGER_PROMPT),
    ("Explore", EXPLORE_PROMPT),
    ("Plan", PLAN_PROMPT),
    ("verification", VERIFICATION_PROMPT),
    ("general-purpose", GENERAL_PROMPT),
    ("literature-search", LITERATURE_SEARCH_PROMPT),
    ("deep-research", DEEP_RESEARCH_PROMPT),
    ("data-analysis", DATA_ANALYSIS_PROMPT),
    ("data-visual", DATA_VISUAL_PROMPT),
    ("code-reviewer", CODE_REVIEWER_PROMPT),
    ("api-reviewer", API_REVIEWER_PROMPT),
    ("critic", CRITIC_PROMPT),
    ("security-reviewer", SECURITY_REVIEWER_PROMPT),
    ("performance-reviewer", PERFORMANCE_REVIEWER_PROMPT),
    ("quality-reviewer", QUALITY_REVIEWER_PROMPT),
    ("test-engineer", TEST_ENGINEER_PROMPT),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_with_frontmatter() {
        let src = "---\ntools: [web_search]\nmodel: standard\n---\n\nHello prompt.";
        let pf = parse_prompt_file(src);
        assert_eq!(pf.prompt, "Hello prompt.");
        assert_eq!(pf.meta.tools, Some(vec!["web_search".to_string()]));
        assert_eq!(pf.meta.model, Some("standard".to_string()));
    }

    #[test]
    fn parse_without_frontmatter() {
        let src = "Just a plain prompt.";
        let pf = parse_prompt_file(src);
        assert_eq!(pf.prompt, "Just a plain prompt.");
        assert!(pf.meta.tools.is_none());
    }

    #[test]
    fn parse_no_closing_fence() {
        let src = "---\nmodel: fast\nNo closing fence — treat as plain text.";
        let pf = parse_prompt_file(src);
        // No valid frontmatter → whole content is the prompt
        assert!(pf.prompt.contains("No closing fence"));
    }
}
