//! `skill_view` — Hermes-style read-only load of full `SKILL.md` or a file under the skill dir.
//! Pair with `skills_list` / `list_skills` for discovery; use `skill` to execute.

use super::ToolSchema;

pub fn schema() -> ToolSchema {
    ToolSchema::new(
        "skill_view",
        r#"Read full SKILL.md text for one skill, or a relative file under that skill directory (e.g. references/notes.md). Does **not** run substitutions or the skill workflow — use `skill` to execute.

Progressive disclosure (Hermes-aligned):
1. Call `skills_list` or `list_skills` to discover names and short metadata.
2. Call `skill_view` when you need the full SKILL.md or a bundled reference file before deciding to run the workflow.
3. Call `skill` with optional args to execute.

Parameters:
- `skill`: skill name (same as `skill` tool).
- `file_path` (optional): relative path under the skill folder; omit or use `SKILL.md` for the main file."#,
        serde_json::json!({
            "type": "object",
            "properties": {
                "skill": {
                    "type": "string",
                    "description": "Skill name (e.g. pdb-database, design-review)."
                },
                "file_path": {
                    "type": "string",
                    "description": "Optional relative path under the skill directory (e.g. references/foo.md). Omit for full SKILL.md."
                }
            },
            "required": ["skill"]
        }),
    )
}
