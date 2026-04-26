//! Sub-agent tool allowlist — parity with TypeScript `filterToolsForAgent` +
//! `ALL_AGENT_DISALLOWED_TOOLS` in `src/constants/tools.ts`.
//!
//! - MCP tools (`mcp__…`) stay available (same as TS: allowed for all agents).
//! - Exceptions (same as TS):
//!   - `ExitPlanMode` when `permissionMode === 'plan'` (here: parent session `plan_mode`).
//!   - `Agent` when `USER_TYPE === 'ant'` (here: `allow_nested_agent`).
//! - TS also omits `workflow` when `WORKFLOW_SCRIPTS` / `feature('WORKFLOW_SCRIPTS')` — see
//!   [`env_workflow_scripts_enabled`].

use crate::domain::permissions::canonical_permission_tool_name;
use crate::domain::tools::ToolSchema;

/// Options for filtering sub-agent tools (`filterToolsForAgent` parity).
#[derive(Debug, Clone, Copy, Default)]
pub struct SubagentFilterOptions {
    /// Session is in plan mode (`EnterPlanMode` called) — allows `ExitPlanMode` for sub-agents.
    pub parent_in_plan_mode: bool,
    /// `USER_TYPE=ant` — allows nested `Agent` in sub-agents.
    pub allow_nested_agent: bool,
}

#[must_use]
pub fn env_allow_nested_agent() -> bool {
    std::env::var("USER_TYPE").ok().as_deref() == Some("ant")
}

/// `feature('WORKFLOW_SCRIPTS')` parity — when true, the `workflow` tool is in the main pool and
/// must be stripped from sub-agents (`ALL_AGENT_DISALLOWED_TOOLS` in `src/constants/tools.ts`).
#[must_use]
pub fn env_workflow_scripts_enabled() -> bool {
    for key in [
        "OMIGA_WORKFLOW_SCRIPTS",
        "WORKFLOW_SCRIPTS",
        "CLAUDE_CODE_WORKFLOW_SCRIPTS",
    ] {
        if let Ok(v) = std::env::var(key) {
            let t = v.trim();
            if t.is_empty() {
                continue;
            }
            if t == "1"
                || t.eq_ignore_ascii_case("true")
                || t.eq_ignore_ascii_case("yes")
                || t.eq_ignore_ascii_case("on")
            {
                return true;
            }
        }
    }
    false
}

/// Whether a built-in tool call should be blocked inside a sub-agent (`subagent_depth > 0`).
#[must_use]
pub fn should_block_subagent_builtin_call(canonical: &str, opts: SubagentFilterOptions) -> bool {
    if canonical == "exit_plan_mode" && opts.parent_in_plan_mode {
        return false;
    }
    if canonical == "agent" && opts.allow_nested_agent {
        return false;
    }
    if canonical == "workflow" && env_workflow_scripts_enabled() {
        return true;
    }
    matches!(
        canonical,
        "agent"
            | "task_output"
            | "exit_plan_mode"
            | "enter_plan_mode"
            | "ask_user_question"
            | "task_stop"
            | "send_user_message" // workers write to blackboard; only the Leader/main agent sends to user
    )
}

#[must_use]
fn remove_schema_from_subagent_pool(canonical: &str, opts: SubagentFilterOptions) -> bool {
    should_block_subagent_builtin_call(canonical, opts)
}

/// Remove sub-agent-disallowed built-ins from the merged schema list (after `permissions.deny`).
#[must_use]
pub fn filter_tool_schemas_for_subagent(
    schemas: Vec<ToolSchema>,
    opts: SubagentFilterOptions,
) -> Vec<ToolSchema> {
    schemas
        .into_iter()
        .filter(|s| {
            if s.name.starts_with("mcp__") {
                return true;
            }
            let c = canonical_permission_tool_name(&s.name);
            !remove_schema_from_subagent_pool(&c, opts)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::tools::ToolSchema;

    #[test]
    fn default_removes_agent_task_output_and_plan_tools() {
        let v = vec![
            ToolSchema::new("bash", "x", serde_json::json!({})),
            ToolSchema::new("Agent", "x", serde_json::json!({})),
            ToolSchema::new("TaskOutput", "x", serde_json::json!({})),
            ToolSchema::new("ExitPlanMode", "x", serde_json::json!({})),
            ToolSchema::new("SendUserMessage", "x", serde_json::json!({})),
            ToolSchema::new("mcp__srv__t", "x", serde_json::json!({})),
        ];
        let out = filter_tool_schemas_for_subagent(v, SubagentFilterOptions::default());
        let names: Vec<_> = out.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"bash"));
        assert!(names.contains(&"mcp__srv__t"));
        assert!(!names.contains(&"Agent"));
        assert!(!names.contains(&"TaskOutput"));
        assert!(!names.contains(&"ExitPlanMode"));
        assert!(!names.contains(&"SendUserMessage"));
    }

    #[test]
    fn plan_mode_keeps_exit_plan_mode() {
        let v = vec![
            ToolSchema::new("ExitPlanMode", "x", serde_json::json!({})),
            ToolSchema::new("EnterPlanMode", "x", serde_json::json!({})),
        ];
        let out = filter_tool_schemas_for_subagent(
            v,
            SubagentFilterOptions {
                parent_in_plan_mode: true,
                allow_nested_agent: false,
            },
        );
        let names: Vec<_> = out.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"ExitPlanMode"));
        assert!(!names.contains(&"EnterPlanMode"));
    }

    #[test]
    fn ant_keeps_agent() {
        let v = vec![ToolSchema::new("Agent", "x", serde_json::json!({}))];
        let out = filter_tool_schemas_for_subagent(
            v,
            SubagentFilterOptions {
                parent_in_plan_mode: false,
                allow_nested_agent: true,
            },
        );
        assert_eq!(out.len(), 1);
    }

    /// Serializes env mutation — `env_workflow_scripts_enabled` is process-global.
    static WORKFLOW_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn workflow_schema_filter_follows_workflow_scripts_env() {
        let _g = WORKFLOW_ENV_LOCK.lock().expect("lock");
        let v = vec![ToolSchema::new("workflow", "x", serde_json::json!({}))];
        for k in [
            "OMIGA_WORKFLOW_SCRIPTS",
            "WORKFLOW_SCRIPTS",
            "CLAUDE_CODE_WORKFLOW_SCRIPTS",
        ] {
            std::env::remove_var(k);
        }
        assert_eq!(
            filter_tool_schemas_for_subagent(v.clone(), SubagentFilterOptions::default()).len(),
            1
        );

        std::env::set_var("OMIGA_WORKFLOW_SCRIPTS", "1");
        assert!(filter_tool_schemas_for_subagent(v, SubagentFilterOptions::default()).is_empty());
        std::env::remove_var("OMIGA_WORKFLOW_SCRIPTS");
    }
}
