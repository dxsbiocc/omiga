use crate::domain::agents::definition::{AgentDefinition, AgentSource, ModelTier};
use crate::domain::tools::ToolContext;

pub struct TestEngineerAgent;

impl AgentDefinition for TestEngineerAgent {
    fn agent_type(&self) -> &str {
        "test-engineer"
    }

    fn when_to_use(&self) -> &str {
        "Testing specialist for identifying missing coverage, proposing regression tests, \
         checking flaky behavior, and evaluating whether evidence is enough to trust a change."
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
        Some("#0EA5E9")
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
