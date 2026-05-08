//! Plan mode tool prompts — aligned with Claude Code TypeScript:
//! - `src/tools/EnterPlanModeTool/prompt.ts` (`getEnterPlanModeToolPromptExternal`)
//! - `src/tools/ExitPlanModeTool/prompt.ts` (`EXIT_PLAN_MODE_V2_TOOL_PROMPT`)
//!
//! Omiga does not inject `plan_mode` attachments on every tool turn; the post-invocation
//! instructions below approximate `EnterPlanModeTool.mapToolResultToToolResultBlockParam` plus
//! the plan-file rules from `getPlanModeV2Instructions` in `src/utils/messages.ts`.

/// External `EnterPlanMode` tool prompt (default non-`USER_TYPE=ant` path).
pub const ENTER_PLAN_MODE_TOOL_PROMPT: &str =
    include_str!("plan_mode/enter_plan_mode_external.txt");

/// `ExitPlanMode` (V2) tool prompt.
pub const EXIT_PLAN_MODE_V2_TOOL_PROMPT: &str = include_str!("plan_mode/exit_plan_mode_v2.txt");

/// Short confirmation — matches `EnterPlanModeTool.call` return `data.message` in TypeScript.
pub const ENTER_PLAN_MODE_SUCCESS_MESSAGE: &str =
    "Entered plan mode. First build shared understanding with the user, then design an implementation approach.";

/// Follow-up after a successful `EnterPlanMode` call — combines TS `mapToolResultToToolResultBlockParam`
/// non-interview instructions with plan-file-only rules from `getPlanModeV2Instructions` (`messages.ts`).
pub const ENTER_PLAN_MODE_TOOL_RESULT_FOLLOWUP: &str = r#"In plan mode, you should:
1. Run the requirements interview gate before planning.
   - Separate facts, assumptions, unknowns, decisions, constraints, non-goals, and success criteria.
   - If a question can be answered by exploring the codebase, explore the codebase instead of asking the user.
   - Ask one user-facing question at a time. Include your recommended answer and the trade-off for that question.
   - Continue across multiple turns until the important branches of the decision tree are resolved.
   - Do **not** present a concrete plan, write a plan file, or call ExitPlanMode while material requirements are still unknown.
2. Thoroughly explore the codebase to understand existing patterns.
3. Identify similar features and architectural approaches.
4. Consider multiple approaches and their trade-offs.
5. Use AskUserQuestion as the first-choice UI if you need to clarify the approach; do not ask bounded clarification choices only in plain text when the tool is available.
6. Only after the requirements gate is satisfied, design a concrete implementation strategy.
7. Write and refine your plan in a single markdown plan file in the project (use `file_write` / `file_edit` on that path only). Do not edit implementation files or run non-readonly actions until the user approves after you exit plan mode.
8. When the plan is ready, use ExitPlanMode to request user approval.

**Important:** Use AskUserQuestion only to clarify requirements or choose between approaches, and prefer it over plain-text questions for bounded choices. Use ExitPlanMode to request plan approval. Do NOT ask "Is this plan okay?" via AskUserQuestion — that is what ExitPlanMode is for.

**Omiga:** There is no separate plan file path in a system attachment — pick a clear path under the project root (e.g. `docs/plan-<topic>.md`) and use it consistently until you call ExitPlanMode."#;

/// Full `ExitPlanMode` description including Omiga caveat (concat avoids drift from TS stub).
pub const EXIT_PLAN_MODE_DESCRIPTION: &str = concat!(
    include_str!("plan_mode/exit_plan_mode_v2.txt"),
    r#"

**Omiga:** Plan approval is reviewed in chat (not the Claude Code approval dialog). `allowedPrompts` still records semantic permission hints for later execution."#
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enter_plan_followup_requires_interview_before_planning() {
        assert!(ENTER_PLAN_MODE_SUCCESS_MESSAGE.contains("shared understanding"));
        assert!(ENTER_PLAN_MODE_TOOL_RESULT_FOLLOWUP.contains("requirements interview gate"));
        assert!(
            ENTER_PLAN_MODE_TOOL_RESULT_FOLLOWUP.contains("Ask one user-facing question at a time")
        );
        assert!(ENTER_PLAN_MODE_TOOL_RESULT_FOLLOWUP.contains("Do **not** present a concrete plan"));
        assert!(ENTER_PLAN_MODE_TOOL_RESULT_FOLLOWUP.contains("ExitPlanMode"));
    }
}
