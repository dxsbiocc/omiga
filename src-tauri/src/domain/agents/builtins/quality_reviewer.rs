use crate::domain::agents::definition::{AgentDefinition, AgentSource, ModelTier};
use crate::domain::tools::ToolContext;

pub struct QualityReviewerAgent;

impl AgentDefinition for QualityReviewerAgent {
    fn agent_type(&self) -> &str {
        "quality-reviewer"
    }

    fn when_to_use(&self) -> &str {
        "Quality review specialist for maintainability, consistency, unnecessary complexity, \
         naming, weak boundaries, and non-security correctness risks. Use after implementation \
         when deciding whether the solution is robust enough to keep."
    }

    fn system_prompt(&self, ctx: &ToolContext) -> String {
        crate::domain::agents::prompt_loader::resolve(self.agent_type(), &ctx.project_root)
    }

    fn source(&self) -> AgentSource {
        AgentSource::BuiltIn
    }

    fn model_tier(&self) -> ModelTier {
        ModelTier::Standard
    }

    fn color(&self) -> Option<&str> {
        Some("#8B5CF6")
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
