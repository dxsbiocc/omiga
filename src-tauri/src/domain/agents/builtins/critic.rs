use crate::domain::agents::definition::{AgentDefinition, AgentSource, ModelTier};
use crate::domain::tools::ToolContext;

pub struct CriticAgent;

impl AgentDefinition for CriticAgent {
    fn agent_type(&self) -> &str {
        "critic"
    }

    fn when_to_use(&self) -> &str {
        "Critical challenge specialist for plans and designs. Use to pressure-test assumptions, \
         identify hidden failure modes, reject weak plans, and surface the highest-risk gap before execution."
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
        Some("#7C3AED")
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
