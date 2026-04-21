use crate::domain::agents::definition::{AgentDefinition, AgentSource};
use crate::domain::tools::ToolContext;

pub struct DebuggerAgent;

impl AgentDefinition for DebuggerAgent {
    fn agent_type(&self) -> &str {
        "debugger"
    }

    fn when_to_use(&self) -> &str {
        "Debugging specialist for investigating bugs, errors, crashes, and unexpected behavior. \
        Use when: a bug needs root cause analysis, an error message is confusing, a test is \
        failing for unknown reasons, or behavior is different from expected. Excellent at \
        reading stack traces, tracing execution, and finding the actual root cause."
    }

    fn system_prompt(&self, ctx: &ToolContext) -> String {
        crate::domain::agents::prompt_loader::resolve(self.agent_type(), &ctx.project_root)
    }

    fn source(&self) -> AgentSource {
        AgentSource::BuiltIn
    }

    fn color(&self) -> Option<&str> {
        Some("#FF5722")
    }

    fn model(&self) -> Option<&str> {
        None
    }

    fn user_facing(&self) -> bool {
        false
    }
}
