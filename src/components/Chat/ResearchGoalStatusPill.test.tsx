import { renderToStaticMarkup } from "react-dom/server";
import { createTheme, ThemeProvider } from "@mui/material/styles";
import { describe, expect, it, vi } from "vitest";
import {
  ResearchGoalStatusPill,
  buildResearchGoalAutoRunCommand,
  compactResearchGoalObjective,
  nextResearchGoalCommand,
  researchGoalAutoRunElapsedBudgetReached,
  researchGoalAutoRunTokenBudgetReached,
  researchGoalShouldWaitForComposerDraft,
  researchGoalCanAutoRun,
  researchGoalStatusLabel,
  type ResearchGoal,
} from "./ResearchGoalStatusPill";

const baseGoal: ResearchGoal = {
  goalId: "goal-1",
  sessionId: "session-1",
  objective: "解析 QS 核心基因在肿瘤免疫微环境中的作用机制",
  status: "active",
  successCriteria: ["形成证据记录"],
  successCriterionIds: ["crit-test"],
  secondOpinionProviderEntry: "goal-second-opinion",
  tokenUsage: {
    inputTokens: 100,
    outputTokens: 50,
    totalTokens: 150,
  },
  maxCycles: 3,
  currentCycle: 1,
  evidenceRefs: ["ev-1"],
  artifactRefs: [],
  notes: [],
  lastAudit: {
    complete: false,
    reviewSource: "llm",
    confidence: "medium",
    finalReportReady: false,
    summary: "完成审计未通过：仍存在缺口。",
    criteria: [],
    missingRequirements: ["需要更多独立证据"],
    nextActions: ["继续执行 /goal run"],
    limitations: ["仅为测试审计"],
    conflictingEvidence: [],
  },
  createdAt: "2026-05-02T00:00:00Z",
  updatedAt: "2026-05-02T00:00:00Z",
  lastRunAt: "2026-05-02T00:00:00Z",
};

function renderPill(goal: ResearchGoal | null = baseGoal) {
  return renderToStaticMarkup(
    <ThemeProvider theme={createTheme()}>
      <ResearchGoalStatusPill
        goal={goal}
        onPrepareCommand={vi.fn()}
        onEditCriteria={vi.fn()}
        onOpenAuditDetails={vi.fn()}
        onToggleAutoRun={vi.fn()}
      />
    </ThemeProvider>,
  );
}

describe("ResearchGoalStatusPill", () => {
  it("maps statuses to user-facing labels and next commands", () => {
    expect(researchGoalStatusLabel("active")).toBe("进行中");
    expect(nextResearchGoalCommand(baseGoal)).toEqual({
      command: "/goal run",
      label: "推进",
    });
    expect(nextResearchGoalCommand({ ...baseGoal, status: "paused" })).toEqual({
      command: "/goal resume",
      label: "恢复",
    });
    expect(nextResearchGoalCommand({ ...baseGoal, status: "complete" })).toEqual({
      command: "/goal status",
      label: "查看状态",
    });
  });

  it("renders the active goal, cycle count, and actions", () => {
    const html = renderPill();

    expect(html).toContain("科研目标状态");
    expect(html).toContain("进行中");
    expect(html).toContain("1/3");
    expect(html).toContain("解析 QS 核心基因");
    expect(html).toContain("推进");
    expect(html).toContain("自动续跑");
    expect(html).toContain("设置");
    expect(html).toContain("状态");
    expect(html).toContain("LLM 审计");
    expect(html).toContain("二审 goal-second-opinion");
    expect(html).toContain("审计详情");
  });

  it("hides itself when there is no active session goal", () => {
    expect(renderPill(null)).toBe("");
  });

  it("compacts long objectives without breaking unicode characters", () => {
    expect(compactResearchGoalObjective("  QS   机制  ", 10)).toBe("QS 机制");
    expect(compactResearchGoalObjective("单细胞肿瘤免疫微环境机制分析", 6)).toBe(
      "单细胞肿瘤…",
    );
  });

  it("builds bounded auto-run commands only for active goals with remaining budget", () => {
    expect(researchGoalCanAutoRun(baseGoal)).toBe(true);
    expect(buildResearchGoalAutoRunCommand(baseGoal)).toBe("/goal run --cycles 2");
    expect(
      buildResearchGoalAutoRunCommand({
        ...baseGoal,
        currentCycle: 1,
        maxCycles: 9,
        autoRunPolicy: {
          enabled: true,
          cyclesPerRun: 3,
          idleDelayMs: 650,
          maxElapsedMinutes: null,
          maxTokens: null,
          startedAt: "2026-05-02T00:00:00Z",
        },
      }),
    ).toBe("/goal run --cycles 3");
    expect(
      buildResearchGoalAutoRunCommand({
        ...baseGoal,
        currentCycle: 2,
        maxCycles: 20,
      }),
    ).toBe("/goal run --cycles 10");
    expect(researchGoalCanAutoRun({ ...baseGoal, status: "paused" })).toBe(false);
    expect(researchGoalCanAutoRun({ ...baseGoal, currentCycle: 3 })).toBe(false);
  });

  it("detects elapsed auto-run budget from persisted policy", () => {
    expect(
      researchGoalAutoRunElapsedBudgetReached(
        {
          ...baseGoal,
          autoRunPolicy: {
            enabled: true,
            cyclesPerRun: 1,
            idleDelayMs: 650,
            maxElapsedMinutes: 5,
            maxTokens: null,
            startedAt: "2026-05-02T00:00:00.000Z",
          },
        },
        Date.parse("2026-05-02T00:06:00.000Z"),
      ),
    ).toBe(true);
    expect(researchGoalAutoRunElapsedBudgetReached(baseGoal)).toBe(false);
  });

  it("detects auto-run token budget from accumulated goal usage", () => {
    const exhausted: ResearchGoal = {
      ...baseGoal,
      autoRunPolicy: {
        enabled: true,
        cyclesPerRun: 1,
        idleDelayMs: 650,
        maxElapsedMinutes: null,
        maxTokens: 150,
      },
      tokenUsage: {
        inputTokens: 100,
        outputTokens: 50,
        totalTokens: 150,
      },
    };

    expect(researchGoalAutoRunTokenBudgetReached(exhausted)).toBe(true);
    expect(researchGoalCanAutoRun(exhausted)).toBe(false);
    expect(researchGoalAutoRunTokenBudgetReached(baseGoal)).toBe(false);
  });

  it("waits instead of auto-running when composer has unsent draft state", () => {
    expect(researchGoalShouldWaitForComposerDraft("  draft  ")).toBe(true);
    expect(researchGoalShouldWaitForComposerDraft("", ["src/main.rs"])).toBe(true);
    expect(researchGoalShouldWaitForComposerDraft("", [], ["plugin-1"])).toBe(true);
    expect(researchGoalShouldWaitForComposerDraft("   ", [], [])).toBe(false);
  });
});
