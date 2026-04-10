//! `list_skills` — returns skill metadata only (no full SKILL.md). Pair with `skill` for full load.

use serde::{Deserialize, Serialize};
use super::ToolSchema;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ListSkillsArgs {
    /// Optional filter substring (matches name, description, when_to_use, or tags)
    pub query: Option<String>,
}

pub const DESCRIPTION: &str = r#"List available skills with metadata only (`name`, `description`, `when_to_use`, optional `tags`, `source`). Does not load full SKILL.md. Use this to discover skills before invoking with `skill` tool.

When to use:
- At the start of a conversation to see what skills are available
- When the user asks about a specialized domain (bioinformatics, databases, design, deployment, etc.)
- When you're unsure how to approach a task

How to use:
- Call without `query` to get all skills ordered by relevance to current task
- Call with `query: "keyword"` to filter by domain (e.g., `query: "pdb"` for protein structures, `query: "react"` for frontend, `query: "deploy"` for deployment)

After finding a relevant skill, use `skill_view` to read full `SKILL.md` (or a reference file) without executing, or call `skill` to run the workflow."#;

/// Hermes wire name `skills_list` — same parameters and behavior as [`schema`].
pub fn skills_list_schema() -> ToolSchema {
    let mut s = schema();
    s.name = "skills_list".to_string();
    s.description = format!(
        "Alias of `list_skills` (Hermes-compatible name). {}",
        s.description.trim_start()
    );
    s
}

pub fn schema() -> ToolSchema {
    ToolSchema::new(
        "list_skills",
        r#"List available skills with metadata only (`name`, `description`, `when_to_use`, optional `tags`, `source`). Does not load full SKILL.md. Use this to discover skills before invoking with `skill` tool.

When to use:
- At the start of a conversation to see what skills are available
- When the user asks about a specialized domain (bioinformatics, databases, design, deployment, etc.)
- When you're unsure how to approach a task

How to use:
- Call without `query` to get all skills ordered by relevance to current task
- Call with `query: "keyword"` to filter by domain (e.g., `query: "pdb"` for protein structures, `query: "react"` for frontend, `query: "deploy"` for deployment)

After finding a relevant skill, use `skill_view` to read full `SKILL.md` (or a reference file) without executing, or call `skill` to run the workflow."#,
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Optional filter substring (matches name, description, when_to_use, or tags). Use domain keywords like 'pdb', 'alphafold', 'react', 'design', 'deploy'."
                }
            }
        }),
    )
}
