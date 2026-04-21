//! Commander Agent — 主指挥官 Agent
//!
//! 取代原 general-purpose。分析任务复杂度，决定执行模式：
//!   Solo   — 直接处理简单/单步任务
//!   Ralph  — 持久循环直到完成（复杂分析、流水线、"必须跑完"类任务）
//!   Team   — 并行协作（大规模任务、多组样本同时处理）

use crate::domain::agents::definition::{AgentDefinition, AgentSource};
use crate::domain::tools::ToolContext;

pub struct GeneralPurposeAgent;

impl AgentDefinition for GeneralPurposeAgent {
    fn agent_type(&self) -> &str {
        "general-purpose"
    }

    fn when_to_use(&self) -> &str {
        "Main commander agent. Analyzes task complexity and routes to solo execution, \
         ralph persistence loop, or team parallel mode. Use for any research, analysis, \
         coding, or multi-step task."
    }

    fn system_prompt(&self, ctx: &ToolContext) -> String {
        crate::domain::agents::prompt_loader::resolve(self.agent_type(), &ctx.project_root)
    }

    fn source(&self) -> AgentSource {
        AgentSource::BuiltIn
    }

    fn allowed_tools(&self) -> Option<Vec<String>> {
        None
    }

    fn model(&self) -> Option<&str> {
        None
    }
}
