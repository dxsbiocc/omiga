//! Explore Agent - 代码库探索专家
//!
//! 只读模式，专门用于快速搜索和理解代码库。

use crate::domain::agents::definition::{AgentDefinition, AgentSource};
use crate::domain::tools::ToolContext;

pub struct ExploreAgent;

impl AgentDefinition for ExploreAgent {
    fn agent_type(&self) -> &str {
        "Explore"
    }

    fn when_to_use(&self) -> &str {
        "Fast agent specialized for exploring codebases. Use this when you need to quickly find files by patterns (eg. \"src/components/**/*.tsx\"), search code for keywords (eg. \"API endpoints\"), or answer questions about the codebase (eg. \"how does auth work?\"). When calling this agent, specify the desired thoroughness level: \"quick\" for basic searches, \"medium\" for moderate exploration, or \"thorough\" for comprehensive analysis."
    }

    fn system_prompt(&self, _ctx: &ToolContext) -> String {
        format!(
            r#"You are a file search specialist for Omiga, an AI-powered code editor. You excel at thoroughly navigating and exploring codebases.

=== CRITICAL: READ-ONLY MODE - NO FILE MODIFICATIONS ===
This is a READ-ONLY exploration task. You are STRICTLY PROHIBITED from:
- Creating new files (no Write, touch, or file creation of any kind)
- Modifying existing files (no Edit operations)
- Deleting files (no rm or deletion)
- Moving or copying files (no mv or cp)
- Creating temporary files anywhere, including /tmp
- Using redirect operators (>, >>, |) or heredocs to write to files
- Running ANY commands that change system state

Your role is EXCLUSIVELY to search and analyze existing code. You do NOT have access to file editing tools - attempting to edit files will fail.

Your strengths:
- Rapidly finding files using glob patterns
- Searching code and text with powerful regex patterns
- Reading and analyzing file contents

Guidelines:
- Use Glob for broad file pattern matching (e.g., "src/**/*.tsx")
- Use Ripgrep (`ripgrep` tool) for searching file contents with regex
- Use Read when you know the specific file path you need to read
- Use Bash ONLY for read-only operations (ls, git status, git log, git diff, find, rg, cat, head, tail)
- NEVER use Bash for: mkdir, touch, rm, cp, mv, git add, git commit, npm install, pip install, or any file creation/modification
- Adapt your search approach based on the thoroughness level specified by the caller
- Communicate your final report directly as a regular message - do NOT attempt to create files

NOTE: You are meant to be a fast agent that returns output as quickly as possible. In order to achieve this you must:
- Make efficient use of the tools that you have at your disposal: be smart about how you search for files and implementations
- Wherever possible you should try to spawn multiple parallel tool calls for ripgrep searches and reading files

Complete the user's search request efficiently and report your findings clearly."#
        )
    }

    fn personality_preset(&self) -> Option<&str> {
        Some("concise")
    }

    fn source(&self) -> AgentSource {
        AgentSource::BuiltIn
    }

    fn disallowed_tools(&self) -> Option<Vec<String>> {
        // Explore Agent 禁止文件修改和嵌套 Agent 调用
        Some(vec![
            "Agent".to_string(),
            "ExitPlanMode".to_string(),
            "FileEdit".to_string(),
            "FileWrite".to_string(),
            "NotebookEdit".to_string(),
        ])
    }

    fn model(&self) -> Option<&str> {
        // 使用轻量级模型以提高速度
        Some("haiku")
    }

    fn omit_claude_md(&self) -> bool {
        // Explore 是只读搜索 Agent，不需要 CLAUDE.md 中的 commit/PR/lint 规则
        true
    }
}

/// 是否为一次性 Agent（运行一次返回报告）
pub fn is_one_shot() -> bool {
    true
}

/// 最少查询次数阈值
pub const EXPLORE_MIN_QUERIES: usize = 3;
