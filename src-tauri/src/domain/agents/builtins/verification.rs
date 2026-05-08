//! Verification Agent - 代码验证和对抗性测试
//!
//! 用于验证实现工作的正确性，执行对抗性测试。

use crate::domain::agents::definition::{AgentDefinition, AgentSource};
use crate::domain::tools::ToolContext;

pub struct VerificationAgent;

impl AgentDefinition for VerificationAgent {
    fn agent_type(&self) -> &str {
        "verification"
    }

    fn when_to_use(&self) -> &str {
        "Use this agent to verify that implementation work is correct. \
         The verification agent runs in the background and attempts to break your code. \
         It is an adversarial process that tests edge cases, security issues, and potential bugs. \
         This agent produces PASS/FAIL/PARTIAL reports."
    }

    fn system_prompt(&self, ctx: &ToolContext) -> String {
        crate::domain::agents::prompt_loader::resolve(self.agent_type(), &ctx.project_root)
    }

    fn personality_preset(&self) -> Option<&str> {
        Some("technical")
    }

    fn source(&self) -> AgentSource {
        AgentSource::BuiltIn
    }

    fn disallowed_tools(&self) -> Option<Vec<String>> {
        Some(vec![
            "Agent".to_string(),
            "FileEdit".to_string(),
            "FileWrite".to_string(),
            "NotebookEdit".to_string(),
            "ExitPlanMode".to_string(),
        ])
    }

    fn model(&self) -> Option<&str> {
        Some("inherit") // 使用与父会话相同的模型，确保验证质量
    }

    fn color(&self) -> Option<&str> {
        Some("red")
    }

    fn background(&self) -> bool {
        true // Verification Agent 始终在后台运行
    }

    fn omit_claude_md(&self) -> bool {
        true
    }

    fn user_facing(&self) -> bool {
        false
    }
}
