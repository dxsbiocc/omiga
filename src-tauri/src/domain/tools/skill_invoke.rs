//! `skill` tool schema — aligned with `SkillTool` (`skill` + optional `args`) in `src/tools/SkillTool/SkillTool.ts`.
//! Execution is implemented in `domain::skills::invoke_skill` (load `SKILL.md`, frontmatter, substitutions).

use serde::{Deserialize, Serialize};
use super::ToolSchema;

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

When users ask you to perform tasks, check if any of the available skills match. Skills provide specialized capabilities and domain knowledge for bioinformatics, protein structures, databases, scientific computing, design, deployment, and more.

When users reference a "slash command" or "/<something>" (e.g., "/commit", "/review-pr"), they are referring to a skill. Use this tool to invoke it.

How to invoke:
- Use this tool with the skill name and optional arguments
- Examples:
  - `skill: "pdb-database"` - invoke the pdb-database skill for protein structures
  - `skill: "alphafold-database"` - invoke the alphafold-database skill for AI predictions
  - `skill: "design-review"` - invoke the design-review skill for UI review
  - `skill: "commit", args: "-m 'Fix bug'"` - invoke with arguments
  - `skill: "review-pr", args: "123"` - invoke with arguments

Important:
- Available skills are listed via `list_skills` tool
- When a skill matches the user's request, this is a BLOCKING REQUIREMENT: invoke the relevant skill tool BEFORE generating any other response about the task
- NEVER mention a skill without actually calling this tool
- Do not invoke a skill that is already running
- Pass optional `args` for `$ARGUMENTS`, `$0`, `$1`, and named `$foo` placeholders (see skill frontmatter `arguments`)

Notes:
- Skills are loaded from `~/.omiga/skills` or project `.omiga/skills`
- To use skills from Claude Code's `~/.claude/skills`, import them in Omiga Settings → Skills
- Omiga implements **file-based skills** with the same substitution rules as Claude Code
- Forked sub-agent execution is not yet implemented; skills run inline in the main conversation"#;

pub fn schema() -> ToolSchema {
    ToolSchema::new(
        "skill",
        r#"Execute a skill within the main conversation.

When users ask you to perform tasks, check if any of the available skills match. Skills provide specialized capabilities and domain knowledge for bioinformatics, protein structures, databases, scientific computing, design, deployment, and more.

When users reference a "slash command" or "/<something>" (e.g., "/commit", "/review-pr"), they are referring to a skill. Use this tool to invoke it.

How to invoke:
- Use this tool with the skill name and optional arguments
- Examples:
  - `skill: "pdb-database"` - invoke the pdb-database skill for protein structures
  - `skill: "alphafold-database"` - invoke the alphafold-database skill for AI predictions
  - `skill: "design-review"` - invoke the design-review skill for UI review
  - `skill: "commit", args: "-m 'Fix bug'"` - invoke with arguments
  - `skill: "review-pr", args: "123"` - invoke with arguments

Important:
- Available skills are listed via `list_skills` tool
- When a skill matches the user's request, this is a BLOCKING REQUIREMENT: invoke the relevant skill tool BEFORE generating any other response about the task
- NEVER mention a skill without actually calling this tool
- Do not invoke a skill that is already running
- Pass optional `args` for `$ARGUMENTS`, `$0`, `$1`, and named `$foo` placeholders (see skill frontmatter `arguments`)

Notes:
- Skills are loaded from `~/.omiga/skills` or project `.omiga/skills`
- To use skills from Claude Code's `~/.claude/skills`, import them in Omiga Settings → Skills
- Omiga implements **file-based skills** with the same substitution rules as Claude Code
- Forked sub-agent execution is not yet implemented; skills run inline in the main conversation"#,
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
