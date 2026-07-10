import { describe, expect, it } from "vitest";
import { buildPendingExecutionFeedback } from "./pendingExecutionFeedback";

describe("buildPendingExecutionFeedback", () => {
  it("returns plan-specific feedback for /plan", () => {
    expect(buildPendingExecutionFeedback({ workflowCommand: "plan" })).toEqual({
      connectLabel: "生成计划中",
      assistantHint: "正在生成结构化计划与待办清单…",
    });
  });

  it("returns plan-specific feedback for Plan composer agent", () => {
    expect(
      buildPendingExecutionFeedback({ composerAgentType: "Plan" }),
    ).toMatchObject({
      connectLabel: "生成计划中",
    });
  });

  it("returns team-specific feedback for /team", () => {
    expect(buildPendingExecutionFeedback({ workflowCommand: "team" })).toEqual({
      connectLabel: "组建团队中",
      assistantHint: "正在拆解任务并分配 Team 角色…",
    });
  });

  it("returns live research feedback for /research", () => {
    expect(buildPendingExecutionFeedback({ workflowCommand: "research" })).toEqual({
      connectLabel: "科研分析中",
      assistantHint: "正在围绕科研问题生成分析、证据边界和下一步建议…",
    });
  });

  it("falls back to generic feedback for ordinary chat", () => {
    expect(buildPendingExecutionFeedback({ workflowCommand: null })).toEqual({
      connectLabel: "等待响应",
      assistantHint: null,
    });
  });
});
