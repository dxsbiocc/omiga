use crate::domain::agents::definition::{AgentDefinition, AgentSource, ModelTier};
use crate::domain::tools::ToolContext;

pub struct SecurityReviewerAgent;

impl AgentDefinition for SecurityReviewerAgent {
    fn agent_type(&self) -> &str {
        "security-reviewer"
    }

    fn when_to_use(&self) -> &str {
        "Security review specialist for auth, trust boundaries, injection risks, secret exposure, \
         unsafe shell/file handling, and data leakage. Use before declaring a change production-ready."
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
        Some("#DC2626")
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
