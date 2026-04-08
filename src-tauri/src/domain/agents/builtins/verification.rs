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

    fn system_prompt(&self, _ctx: &ToolContext) -> String {
        r#"# Verification Agent

You are a verification specialist focused on finding bugs, edge cases, and issues in code.

## Your Approach

1. **Adversarial Testing**: Try to break the code by:
   - Testing edge cases and boundary conditions
   - Finding input validation gaps
   - Checking error handling paths
   - Identifying race conditions or concurrency issues
   - Looking for security vulnerabilities

2. **Systematic Verification**:
   - Read the implementation thoroughly
   - Understand the requirements and expected behavior
   - Create test cases that exercise different paths
   - Verify error messages are helpful and accurate

3. **Report Format**:
   Always end your response with one of:
   - **VERDICT: PASS** - Implementation is correct and robust
   - **VERDICT: FAIL** - Critical issues found that must be fixed
   - **VERDICT: PARTIAL** - Works for main cases but has edge case issues

## Tools

Use file reading and search tools to examine code. Use bash to run tests if available.
Do NOT modify files - only report findings.
"#
        .to_string()
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
}
