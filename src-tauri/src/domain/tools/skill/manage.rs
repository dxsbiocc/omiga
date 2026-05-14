//! `skill_manage` — create, patch, edit, or delete skills under `<project>/.omiga/skills/`.

use super::ToolSchema;

pub const DESCRIPTION: &str = r#"Manage file-based skills in the current project (procedural memory).

**Scope:** Only `<project>/.omiga/skills/` — flat `<name>/` or nested `<category>/<name>/` when `category` is set on create. Skills installed only under `~/.omiga/skills` cannot be edited here; copy them into the project first or use `file_write`.

**Actions:**
- `create` — `name`, `content` (full SKILL.md with YAML frontmatter). Optional `category` writes to `skills/<category>/<name>/`. Frontmatter must include non-empty `name` and `description`. Optional `tags` (YAML list or comma-separated string) helps `list_skills` search — e.g. `tags: [pdb, structure]` or `tags: react, frontend`.
- `patch` — `name`, `old_string`, `new_string`. Targets `SKILL.md` by default. Set `file_path` to a relative path under the skill dir to patch another file (e.g. `references/notes.md`). By default exactly one occurrence of `old_string` must match; set `replace_all`: true to replace every occurrence.
- `edit` — `name`, `content` (replace entire SKILL.md); same frontmatter requirements as `create` (you may add or change optional `tags`, `when_to_use`, etc.).
- `delete` — `name` (removes the whole skill directory)
- `write_file` — `name`, `file_path` (relative path under skill dir), `file_content` (not for SKILL.md — use patch/edit)
- `remove_file` — `name`, `file_path` (cannot remove SKILL.md)

**When to use:** After a non-trivial workflow succeeds, when the user corrects your approach, or when you want to reuse a procedure in future sessions.

**Naming:** `name` and `category` (if set) must use only `a-z`, `0-9`, `_`, `-` (max 64 chars each); `name` is the leaf directory name."#;

pub fn schema() -> ToolSchema {
    ToolSchema::new(
        "skill_manage",
        DESCRIPTION,
        serde_json::json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "description": "One of: create, patch, edit, delete, write_file, remove_file",
                    "enum": ["create", "patch", "edit", "delete", "write_file", "remove_file"]
                },
                "name": {
                    "type": "string",
                    "description": "Skill name (directory name for create; existing skill name for other actions)"
                },
                "content": {
                    "type": "string",
                    "description": "Full SKILL.md for create or edit: YAML frontmatter with required name and description; optional tags (list or comma-separated), when_to_use, allowed-tools, etc."
                },
                "old_string": {
                    "type": "string",
                    "description": "Substring to replace (patch) — must match exactly once unless replace_all is true"
                },
                "new_string": {
                    "type": "string",
                    "description": "Replacement for old_string (patch)"
                },
                "replace_all": {
                    "type": "boolean",
                    "description": "If true (patch), replace every occurrence of old_string. Default false (exactly one match)."
                },
                "file_path": {
                    "type": "string",
                    "description": "Relative path under the skill directory: for write_file / remove_file; for patch, optional target file (default SKILL.md)"
                },
                "file_content": {
                    "type": "string",
                    "description": "File contents for write_file"
                },
                "category": {
                    "type": "string",
                    "description": "Optional — create only. Places the skill under skills/<category>/<name>/ (Hermes-style). Omit for skills/<name>/."
                }
            },
            "required": ["action", "name"]
        }),
    )
}
