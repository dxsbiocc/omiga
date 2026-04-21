use crate::domain::agents::definition::{AgentDefinition, AgentSource, ModelTier};
use crate::domain::tools::ToolContext;

pub struct PerformanceReviewerAgent;

impl AgentDefinition for PerformanceReviewerAgent {
    fn agent_type(&self) -> &str {
        "performance-reviewer"
    }

    fn when_to_use(&self) -> &str {
        "Performance review specialist for hotspots, unnecessary I/O, repeated work, \
         slow queries, memory churn, and scaling risks. Use when throughput or latency matters."
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
        Some("#F59E0B")
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
