//! Deep Research Agent
//!
//! Produces comprehensive, citation-rich research reviews for any domain.
//! Runs parallel web searches across multiple sources, then synthesizes a
//! structured Markdown report.  Exposed as "deep-research" in the agent
//! picker and triggered by keyword routing ("研究现状", "综述", "state of
//! the art", etc.).

use crate::domain::agents::definition::{AgentDefinition, AgentSource, ModelTier};
use crate::domain::tools::ToolContext;

pub struct DeepResearchAgent;

impl AgentDefinition for DeepResearchAgent {
    fn agent_type(&self) -> &str {
        "deep-research"
    }

    fn when_to_use(&self) -> &str {
        "Comprehensive research review agent. Use when the user asks for domain surveys, \
         state-of-the-art analysis, literature reviews, or research overviews \
         (e.g. \"分析某领域研究现状\", \"综述\", \"state of the art in X\"). \
         Runs parallel web searches and produces a structured report with citations."
    }

    fn system_prompt(&self, ctx: &ToolContext) -> String {
        crate::domain::agents::prompt_loader::resolve(self.agent_type(), &ctx.project_root)
    }

    fn source(&self) -> AgentSource {
        AgentSource::BuiltIn
    }

    fn model_tier(&self) -> ModelTier {
        // Frontier: deep reasoning needed for synthesis across many sources
        ModelTier::Frontier
    }

    fn color(&self) -> Option<&str> {
        Some("#6366f1") // indigo
    }

    fn allowed_tools(&self) -> Option<Vec<String>> {
        // Read + web access only; no file editing (the report is the reply).
        // SendUserMessage excluded: output must land in the blackboard so the Leader
        // synthesis step can consolidate citations and present a single coherent reply.
        Some(vec![
            "web_search".to_string(),
            "web_fetch".to_string(),
            "recall".to_string(),
            "file_read".to_string(),
            "todo_write".to_string(),
        ])
    }
}
