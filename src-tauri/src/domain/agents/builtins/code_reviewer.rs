use crate::domain::agents::definition::{AgentDefinition, AgentSource, ModelTier};
use crate::domain::tools::ToolContext;

pub struct CodeReviewerAgent;

impl AgentDefinition for CodeReviewerAgent {
    fn agent_type(&self) -> &str {
        "code-reviewer"
    }

    fn when_to_use(&self) -> &str {
        "Read-only code review specialist for correctness, maintainability, and design quality. \
         Use after implementation to identify logic bugs, risky assumptions, or cleanup opportunities."
    }

    fn system_prompt(&self, ctx: &ToolContext) -> String {
        crate::domain::agents::prompt_loader::resolve(self.agent_type(), &ctx.project_root)
    }

    fn source(&self) -> AgentSource {
        AgentSource::BuiltIn
    }

    fn model_tier(&self) -> ModelTier {
        ModelTier::Frontier
    }

    fn color(&self) -> Option<&str> {
        Some("#6366F1")
    }

    fn disallowed_tools(&self) -> Option<Vec<String>> {
        Some(vec![
            "file_edit".to_string(),
            "file_write".to_string(),
            "notebook_edit".to_string(),
            "Agent".to_string(),
            "EnterPlanMode".to_string(),
        ])
    }

    fn user_facing(&self) -> bool {
        false
    }
}
