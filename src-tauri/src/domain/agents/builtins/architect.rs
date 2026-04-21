use crate::domain::agents::definition::{AgentDefinition, AgentSource, ModelTier};
use crate::domain::tools::ToolContext;

pub struct ArchitectAgent;

impl AgentDefinition for ArchitectAgent {
    fn agent_type(&self) -> &str {
        "architect"
    }

    fn when_to_use(&self) -> &str {
        "Verification authority and design specialist. Issues APPROVED/REJECTED verdicts \
         for analysis results, code implementations, and pipeline outputs. Used at the end \
         of ralph loops and team modes to sign off on completed work."
    }

    fn system_prompt(&self, ctx: &ToolContext) -> String {
        crate::domain::agents::prompt_loader::resolve(self.agent_type(), &ctx.project_root)
    }

    fn source(&self) -> AgentSource {
        AgentSource::BuiltIn
    }

    fn model_tier(&self) -> ModelTier {
        // Frontier: verification requires deep reasoning to catch subtle statistical errors.
        ModelTier::Frontier
    }

    fn color(&self) -> Option<&str> {
        Some("#9C27B0")
    }

    fn disallowed_tools(&self) -> Option<Vec<String>> {
        // Reviewer tier: verification-only — can read files and run bash checks,
        // but cannot modify source code or notebooks.
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
