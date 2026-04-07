//! `list_skills` — returns skill metadata only (no full SKILL.md). Pair with `skill` for full load.

use super::ToolSchema;

pub fn schema() -> ToolSchema {
    ToolSchema::new(
        "list_skills",
        r#"List available skills with metadata only (`name`, `description`, `when_to_use`, `source`). Does not load full SKILL.md. Optional `query` filters by case-insensitive substring on name, description, or when_to_use. When `query` is omitted, the server orders results by relevance to the **current chat task** (same heuristic as the task-ranked system hint). Call `skill` with a chosen name to load full instructions."#,
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Optional filter substring (matches name, description, or when_to_use)"
                }
            }
        }),
    )
}
