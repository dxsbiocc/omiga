//! Data Visualization Agent
//!
//! Specialist for publication-quality scientific figures.
//! Covers R/ggplot2, Python matplotlib/seaborn/plotly, and Omiga's
//! built-in visualization renderer (ECharts, Plotly, Mermaid).

use crate::domain::agents::definition::{AgentDefinition, AgentSource, ModelTier};
use crate::domain::tools::ToolContext;

pub struct DataVisualAgent;

impl AgentDefinition for DataVisualAgent {
    fn agent_type(&self) -> &str {
        "data-visual"
    }

    fn when_to_use(&self) -> &str {
        "Scientific visualization and figure generation specialist. Use for: volcano plots, \
         heatmaps, UMAP/tSNE embeddings, bar/box/violin plots, Kaplan-Meier curves, \
         pathway enrichment dot plots, network graphs, and interactive dashboards. \
         Produces publication-ready figures in PDF/SVG/PNG and interactive omiga:viz blocks."
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
        Some("#f59e0b") // amber
    }
}
