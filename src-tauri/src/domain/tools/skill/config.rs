//! `skill_config` — read / write skill configuration variables.

use super::ToolSchema;
use serde::Deserialize;

pub const DESCRIPTION: &str = r#"Read or write skill configuration variables stored in `.omiga/config.yaml`.

Skills declare the config variables they need in their SKILL.md frontmatter under `metadata.omiga.config`. When a skill is invoked its resolved config values are automatically injected into the skill body.

**Actions:**
- `get` — list config vars for a skill and their current values. Requires `skill`.
- `set` — set a config var value. Requires `skill` (to resolve vars), `key`, and `value`.
- `list` — list all config vars across all skills with current values. No extra args needed.

**YAML format stored:**
```yaml
# <project>/.omiga/config.yaml
skills:
  config:
    wiki:
      path: /Users/alice/notes
    api:
      endpoint: https://my-api.example.com
```

**Skill declaration:**
```yaml
metadata:
  omiga:
    config:
      - key: wiki.path
        description: Path to the knowledge base
        default: ~/notes
```
"#;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigAction {
    Get,
    Set,
    List,
}

#[derive(Debug, Deserialize)]
pub struct SkillConfigArgs {
    pub action: ConfigAction,
    /// Skill name — required for `get` and `set`.
    #[serde(default)]
    pub skill: Option<String>,
    /// Config key (dotted) — required for `set`.
    #[serde(default)]
    pub key: Option<String>,
    /// New value — required for `set`.
    #[serde(default)]
    pub value: Option<String>,
}

pub fn schema() -> ToolSchema {
    ToolSchema::new(
        "skill_config",
        DESCRIPTION,
        serde_json::json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["get", "set", "list"],
                    "description": "get — show current values for a skill; set — set one value; list — show all skill configs."
                },
                "skill": {
                    "type": "string",
                    "description": "Skill name. Required for `get` and `set`."
                },
                "key": {
                    "type": "string",
                    "description": "Dotted config key (e.g. `wiki.path`). Required for `set`."
                },
                "value": {
                    "type": "string",
                    "description": "New value for the config key. Required for `set`."
                }
            },
            "required": ["action"]
        }),
    )
}
