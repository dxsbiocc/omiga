//! Planner Agent — 分析方案设计专家
//!
//! 只读模式：探索数据/代码结构，输出带 TodoWrite 待办项的实施计划。
//! 由 Commander 在需要正式规划时调用，也可用户直接选择。

use crate::domain::agents::definition::{AgentDefinition, AgentSource};
use crate::domain::tools::ToolContext;

pub struct PlanAgent;

impl AgentDefinition for PlanAgent {
    fn agent_type(&self) -> &str {
        "Plan"
    }

    fn when_to_use(&self) -> &str {
        "Planning specialist. Explores the codebase/data structure and produces a \
         step-by-step implementation plan with TodoWrite items. Read-only — no file edits."
    }

    fn system_prompt(&self, ctx: &ToolContext) -> String {
        crate::domain::agents::prompt_loader::resolve(self.agent_type(), &ctx.project_root)
    }

    fn personality_preset(&self) -> Option<&str> {
        Some("technical")
    }

    fn source(&self) -> AgentSource {
        AgentSource::BuiltIn
    }

    fn disallowed_tools(&self) -> Option<Vec<String>> {
        Some(vec![
            "Agent".to_string(),
            "ExitPlanMode".to_string(),
            "file_edit".to_string(),
            "file_write".to_string(),
            "notebook_edit".to_string(),
        ])
    }

    fn model(&self) -> Option<&str> {
        Some("inherit")
    }

    fn omit_claude_md(&self) -> bool {
        true
    }

    fn user_facing(&self) -> bool {
        false
    }
}
