//! Data Analysis Agent
//!
//! Specialist for statistical analysis, data processing, and scientific computing.
//! Covers Python (pandas, scipy, scanpy, DESeq2 via rpy2) and R (tidyverse,
//! Seurat, edgeR, limma).  Writes reproducible analysis scripts and notebooks.

use crate::domain::agents::definition::{AgentDefinition, AgentSource, ModelTier};
use crate::domain::tools::ToolContext;

pub struct DataAnalysisAgent;

impl AgentDefinition for DataAnalysisAgent {
    fn agent_type(&self) -> &str {
        "data-analysis"
    }

    fn when_to_use(&self) -> &str {
        "Data analysis and statistical computing specialist. Use for: exploratory data analysis, \
         differential expression (DESeq2, edgeR), single-cell analysis (Seurat, scanpy), \
         statistical testing, dimensionality reduction, clustering, and result interpretation. \
         Writes Python notebooks (.ipynb) and R scripts."
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
        Some("#10b981") // emerald
    }
}
