use crate::domain::agents::definition::{AgentDefinition, AgentSource, ModelTier};
use crate::domain::tools::ToolContext;

pub struct ExecutorAgent;

impl AgentDefinition for ExecutorAgent {
    fn agent_type(&self) -> &str {
        "executor"
    }

    fn when_to_use(&self) -> &str {
        "Executor-supervisor for approved project plans: coordinates implementation, \
         data retrieval/download glue, analysis, visualization, debugging, and verification \
         through backend-controlled child workers without forcing a fixed pipeline. \
         Used internally by General/Ralph/Team modes."
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

    fn disallowed_tools(&self) -> Option<Vec<String>> {
        Some(vec![
            "Agent".to_string(),
            "EnterPlanMode".to_string(),
            "ExitPlanMode".to_string(),
        ])
    }

    fn color(&self) -> Option<&str> {
        Some("#4CAF50")
    }

    fn user_facing(&self) -> bool {
        false
    }
}
