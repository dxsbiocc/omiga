//! Literature Search Agent
//!
//! Specialist for finding, screening, and summarizing academic papers.
//! Searches PubMed, arXiv, bioRxiv, and Google Scholar in parallel; returns
//! structured summaries with DOIs/URLs suitable for citation.

use crate::domain::agents::definition::{AgentDefinition, AgentSource, ModelTier};
use crate::domain::tools::ToolContext;

pub struct LiteratureSearchAgent;

impl AgentDefinition for LiteratureSearchAgent {
    fn agent_type(&self) -> &str {
        "literature-search"
    }

    fn when_to_use(&self) -> &str {
        "Academic literature search and summarization specialist. Use for: finding papers on a topic, \
         screening abstracts, summarizing methods and findings, building reference lists, and \
         identifying key authors and research groups. Searches PubMed, arXiv, bioRxiv, and \
         Google Scholar in parallel."
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
        Some("#8b5cf6") // violet
    }

    fn allowed_tools(&self) -> Option<Vec<String>> {
        // SendUserMessage is intentionally excluded: in Team/orchestration mode the output
        // must flow through the shared blackboard so the Leader's synthesis step can read it
        // and consolidate citations.  Streaming directly to the user bypasses the pipeline
        // and leaves the synthesis with no data.
        Some(vec![
            "web_search".to_string(),
            "web_fetch".to_string(),
            "recall".to_string(),
            "todo_write".to_string(),
        ])
    }
}
