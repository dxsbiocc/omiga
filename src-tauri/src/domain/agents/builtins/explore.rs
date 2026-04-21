//! Explore Agent - 代码库探索专家
//!
//! 只读模式，专门用于快速搜索和理解代码库。

use crate::domain::agents::definition::{AgentDefinition, AgentSource, ModelTier};
use crate::domain::tools::ToolContext;

pub struct ExploreAgent;

impl AgentDefinition for ExploreAgent {
    fn agent_type(&self) -> &str {
        "Explore"
    }

    fn when_to_use(&self) -> &str {
        "Fast agent specialized for exploring codebases. Use this when you need to quickly find files by patterns (eg. \"src/components/**/*.tsx\"), search code for keywords (eg. \"API endpoints\"), or answer questions about the codebase (eg. \"how does auth work?\"). When calling this agent, specify the desired thoroughness level: \"quick\" for basic searches, \"medium\" for moderate exploration, or \"thorough\" for comprehensive analysis."
    }

    fn system_prompt(&self, ctx: &ToolContext) -> String {
        crate::domain::agents::prompt_loader::resolve(self.agent_type(), &ctx.project_root)
    }

    fn personality_preset(&self) -> Option<&str> {
        Some("concise")
    }

    fn source(&self) -> AgentSource {
        AgentSource::BuiltIn
    }

    fn disallowed_tools(&self) -> Option<Vec<String>> {
        // Explorer tier: read-only, no file mutations, no nested agents
        Some(vec![
            "Agent".to_string(),
            "ExitPlanMode".to_string(),
            "EnterPlanMode".to_string(),
            "file_edit".to_string(),
            "file_write".to_string(),
            "notebook_edit".to_string(),
            "todo_write".to_string(),
        ])
    }

    fn model_tier(&self) -> ModelTier {
        // Spark: fast read-only searches don't need heavy reasoning.
        ModelTier::Spark
    }

    fn omit_claude_md(&self) -> bool {
        // Explore 是只读搜索 Agent，不需要 CLAUDE.md 中的 commit/PR/lint 规则
        true
    }

    fn user_facing(&self) -> bool {
        false
    }
}

