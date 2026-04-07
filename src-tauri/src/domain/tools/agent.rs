//! Spawn a specialized sub-agent — aligned with `AgentTool` (TypeScript, name `Agent`, legacy `Task`).
//!
//! Execution is handled in `commands::chat` (`run_subagent_session`): an isolated LLM loop with tools (no nested Agent).
//! The `ToolImpl` here is only used by IPC `execute_tool`; chat uses the dedicated path.

use super::{ToolContext, ToolError, ToolSchema};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

pub const DESCRIPTION: &str = r#"Launch a specialized sub-agent to work on a sub-task in isolation.

Provide a short `description` (3–5 words), the full `prompt`, and optionally `subagent_type` (e.g. built-in Explore, Plan, general-purpose), `model` override (`sonnet` / `opus` / `haiku` on Anthropic), or `cwd`.

The sub-agent runs in Omiga with the same tool set as the main session except **nested Agent calls are disabled**. `run_in_background` is not supported yet."#;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentArgs {
    pub description: String,
    pub prompt: String,
    #[serde(default)]
    pub subagent_type: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub run_in_background: Option<bool>,
    #[serde(default)]
    pub cwd: Option<String>,
}

pub struct AgentTool;

#[async_trait]
impl super::ToolImpl for AgentTool {
    type Args = AgentArgs;

    const DESCRIPTION: &'static str = DESCRIPTION;

    async fn execute(
        _ctx: &ToolContext,
        args: Self::Args,
    ) -> Result<crate::infrastructure::streaming::StreamOutputBox, ToolError> {
        if args.description.trim().is_empty() || args.prompt.trim().is_empty() {
            return Err(ToolError::InvalidArguments {
                message: "`description` and `prompt` must be non-empty.".to_string(),
            });
        }

        Err(ToolError::ExecutionFailed {
            message: "The Agent tool runs inside the main chat session. Use it from the assistant (not via execute_tool IPC).".to_string(),
        })
    }
}

pub fn schema() -> ToolSchema {
    ToolSchema::new(
        "Agent",
        DESCRIPTION,
        serde_json::json!({
            "type": "object",
            "properties": {
                "description": {
                    "type": "string",
                    "description": "Short (3–5 word) summary of the task"
                },
                "prompt": {
                    "type": "string",
                    "description": "Instructions for the sub-agent"
                },
                "subagent_type": {
                    "type": "string",
                    "description": "Optional agent type (e.g. Explore, general-purpose)"
                },
                "model": {
                    "type": "string",
                    "description": "Optional model: `inherit` uses the parent chat model; `sonnet`/`opus`/`haiku` resolve like Claude Code `getAgentModel` (same tier as parent inherits exact id); or pass a full model id. Env `CLAUDE_CODE_SUBAGENT_MODEL` / `OMIGA_SUBAGENT_MODEL` overrides when set."
                },
                "run_in_background": {
                    "type": "boolean",
                    "description": "Run asynchronously when supported"
                },
                "cwd": {
                    "type": "string",
                    "description": "Optional working directory for the agent"
                }
            },
            "required": ["description", "prompt"]
        }),
    )
}
