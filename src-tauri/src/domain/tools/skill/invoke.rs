//! `skill` tool schema — aligned with `SkillTool` (`skill` + optional `args`) in `src/tools/SkillTool/SkillTool.ts`.
//! Execution is implemented in `domain::skills::invoke_skill` (load `SKILL.md`, frontmatter, substitutions).

use super::ToolSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillInvokeArgs {
    /// The skill name (e.g., 'pdb-database', 'design-review', or 'commit')
    pub skill: String,
    /// Optional arguments for the skill
    pub args: Option<String>,
    /// Alias of `args` for Claude Code compatibility
    pub arguments: Option<String>,
}

pub const DESCRIPTION: &str = r#"Execute a skill within the main conversation.

When users reference a "slash command" or "/<something>" (e.g., "/commit", "/review-pr"), they are referring to a skill. Use this tool to invoke it.

Examples:
- `skill: "pdb-database"`
- `skill: "commit", args: "-m 'Fix bug'"`
- `skill: "review-pr", args: "123"`

Important:
- When a skill matches the user's request, this is a BLOCKING REQUIREMENT: invoke it BEFORE generating any other response.
- NEVER mention a skill without actually calling this tool.
- Do not invoke a skill that is already running.
- Pass optional `args` for `$ARGUMENTS`, `$0`, `$1`, and named `$foo` placeholders (see skill frontmatter `arguments`)."#;

pub fn schema() -> ToolSchema {
    ToolSchema::new(
        "skill",
        DESCRIPTION,
        serde_json::json!({
            "type": "object",
            "properties": {
                "skill": {
                    "type": "string",
                    "description": "The skill name. E.g., 'pdb-database', 'design-review', or 'commit'. Leading '/' is accepted and will be stripped."
                },
                "args": {
                    "type": "string",
                    "description": "Optional arguments for the skill (same as Claude Code `args`). Supports `$ARGUMENTS`, `$0`, `$1`, and named `$foo` substitutions."
                },
                "arguments": {
                    "type": "string",
                    "description": "Alias of `args` for Claude Code compatibility"
                }
            },
            "required": ["skill"]
        }),
    )
}
