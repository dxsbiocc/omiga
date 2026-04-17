//! `list_skills` — returns skill metadata only (no full SKILL.md). Pair with `skill` for full load.

use super::ToolSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ListSkillsArgs {
    /// Optional filter substring (matches name, description, when_to_use, or tags)
    pub query: Option<String>,
}

pub const DESCRIPTION: &str = r#"Discover available specialized skills. Returns metadata (`name`, `description`, `when_to_use`, optional `tags`, `source`) but not full SKILL.md.

Core principle: do not reinvent the wheel.

Discovery protocol:
1. Try `query: "keyword"` with 1-2 relevant keywords.
2. If nothing matches, **immediately** call without arguments to list the full catalog. Do **not** keep guessing keywords.
3. When you find a candidate, **use `skill_view` to read its instructions before calling `skill`**."#;

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
        DESCRIPTION,
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Optional filter substring (matches name, description, when_to_use, or tags). Use domain keywords like 'commit', 'review', 'deploy', 'pdb', 'alphafold', 'react', 'design'."
                }
            }
        }),
    )
}
