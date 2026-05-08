use crate::domain::agents::definition::{AgentDefinition, AgentSource, ModelTier};
use crate::domain::tools::ToolContext;

pub struct ApiReviewerAgent;

impl AgentDefinition for ApiReviewerAgent {
    fn agent_type(&self) -> &str {
        "api-reviewer"
    }

    fn when_to_use(&self) -> &str {
        "API review specialist for contracts, compatibility, versioning, response shapes, \
         input validation, and caller-facing semantics. Use when changes affect interfaces \
         consumed by users, plugins, CLI tools, or other modules."
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
        Some("#14B8A6")
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
