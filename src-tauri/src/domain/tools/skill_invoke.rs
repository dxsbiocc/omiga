//! `skill` tool schema — aligned with `SkillTool` (`skill` + optional `args`) in `src/tools/SkillTool/SkillTool.ts`.
//! Execution is implemented in `domain::skills::invoke_skill` (load `SKILL.md`, frontmatter, substitutions).

use super::ToolSchema;

pub fn schema() -> ToolSchema {
    ToolSchema::new(
        "skill",
        r#"Load full skill text from disk (`~/.omiga/skills` or project `.omiga/skills`; `~/.claude/skills` only when enabled in Omiga Settings → Skills). Prefer calling `list_skills` first to pick a name. Pass optional `args` for `$ARGUMENTS`, `$0`, `$1`, and named `$foo` placeholders (see skill frontmatter `arguments`). Returns JSON metadata (inline vs fork notice) plus the full skill body for this session.

Claude Code also supports MCP/bundled/plugin skills and forked sub-agent execution; Omiga implements **file-based skills** plus the same substitution rules for markdown skills."#,
        serde_json::json!({
            "type": "object",
            "properties": {
                "skill": {
                    "type": "string",
                    "description": "Skill name (YAML `name`, or directory name). Leading `/` is accepted."
                },
                "args": {
                    "type": "string",
                    "description": "Optional arguments (same as Claude Code `args`)"
                },
                "arguments": {
                    "type": "string",
                    "description": "Alias of `args` for compatibility"
                }
            },
            "required": ["skill"]
        }),
    )
}
