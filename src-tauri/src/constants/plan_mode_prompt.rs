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
    "Entered plan mode. You should now focus on exploring the codebase and designing an implementation approach.";

/// Follow-up after a successful `EnterPlanMode` call — combines TS `mapToolResultToToolResultBlockParam`
/// non-interview instructions with plan-file-only rules from `getPlanModeV2Instructions` (`messages.ts`).
pub const ENTER_PLAN_MODE_TOOL_RESULT_FOLLOWUP: &str = r#"In plan mode, you should:
1. Thoroughly explore the codebase to understand existing patterns
2. Identify similar features and architectural approaches
3. Consider multiple approaches and their trade-offs
4. Use AskUserQuestion as the first-choice UI if you need to clarify the approach; do not ask bounded clarification choices only in plain text when the tool is available
5. Design a concrete implementation strategy
6. Write and refine your plan in a single markdown plan file in the project (use `file_write` / `file_edit` on that path only). Do not edit implementation files or run non-readonly actions until the user approves after you exit plan mode.
7. When the plan is ready, use ExitPlanMode to request user approval.

**Important:** Use AskUserQuestion only to clarify requirements or choose between approaches, and prefer it over plain-text questions for bounded choices. Use ExitPlanMode to request plan approval. Do NOT ask "Is this plan okay?" via AskUserQuestion — that is what ExitPlanMode is for.

**Omiga:** There is no separate plan file path in a system attachment — pick a clear path under the project root (e.g. `docs/plan-<topic>.md`) and use it consistently until you call ExitPlanMode."#;

/// Full `ExitPlanMode` description including Omiga caveat (concat avoids drift from TS stub).
pub const EXIT_PLAN_MODE_DESCRIPTION: &str = concat!(
    include_str!("plan_mode/exit_plan_mode_v2.txt"),
    r#"

**Omiga:** Plan approval is reviewed in chat (not the Claude Code approval dialog). `allowedPrompts` still records semantic permission hints for later execution."#
);
