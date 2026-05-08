import { describe, expect, it } from "vitest";
import {
  autoRunPolicyDraftFromGoal,
  criteriaDraftFromGoal,
  criteriaLinesFromDraft,
  maxCyclesDraftFromGoal,
  parseOptionalPositiveIntegerDraft,
  parseMaxCyclesDraft,
  providerEntryOptionLabel,
  providerTestResultMessage,
  secondOpinionProviderEntryDraftFromGoal,
  validateAutoRunPolicyDraft,
  validateCriteriaLines,
  validateMaxCycles,
} from "./ResearchGoalCriteriaDialog";
import type { ResearchGoal } from "./ResearchGoalStatusPill";

const goal: ResearchGoal = {
  goalId: "goal-1",
  sessionId: "session-1",
  objective: "解析 QS 核心基因机制",
  status: "active",
  successCriteria: ["形成证据链", "解释局限与下一步"],
  secondOpinionProviderEntry: "goal-second-opinion",
  autoRunPolicy: {
    enabled: true,
    cyclesPerRun: 2,
    idleDelayMs: 1000,
    maxElapsedMinutes: 30,
    maxTokens: 5000,
    startedAt: "2026-05-02T00:00:00Z",
  },
  maxCycles: 3,
  currentCycle: 0,
  evidenceRefs: [],
  artifactRefs: [],
  notes: [],
  createdAt: "2026-05-02T00:00:00Z",
  updatedAt: "2026-05-02T00:00:00Z",
};

describe("ResearchGoalCriteriaDialog helpers", () => {
  it("builds an editable draft from the goal criteria", () => {
    expect(criteriaDraftFromGoal(goal)).toBe("形成证据链\n解释局限与下一步");
    expect(criteriaDraftFromGoal(null)).toBe("");
    expect(maxCyclesDraftFromGoal(goal)).toBe("3");
    expect(secondOpinionProviderEntryDraftFromGoal(goal)).toBe(
      "goal-second-opinion",
    );
    expect(secondOpinionProviderEntryDraftFromGoal(null)).toBe("");
    expect(autoRunPolicyDraftFromGoal(goal)).toMatchObject({
      enabled: true,
      cyclesPerRun: 2,
      idleDelayMs: 1000,
      maxElapsedMinutes: 30,
      maxTokens: 5000,
    });
    expect(autoRunPolicyDraftFromGoal(null)).toMatchObject({
      enabled: false,
      cyclesPerRun: 10,
      idleDelayMs: 650,
      maxElapsedMinutes: null,
      maxTokens: null,
    });
  });

  it("formats provider entry picker options", () => {
    expect(
      providerEntryOptionLabel({
        name: "goal-second-opinion",
        providerType: "deepseek",
        model: "deepseek-v4-flash",
        enabled: true,
      }),
    ).toBe("goal-second-opinion · deepseek/deepseek-v4-flash");
    expect(
      providerEntryOptionLabel({
        name: "manual-entry",
        providerType: "",
        model: "",
      }),
    ).toBe("manual-entry");
  });

  it("formats provider test results", () => {
    expect(
      providerTestResultMessage({
        available: true,
        provider: "Deepseek",
        model: "deepseek-v4-flash",
        latencyMs: 12,
      }),
    ).toBe(
      "二审 provider 真实 LLM 调用通过：Deepseek / deepseek-v4-flash，12ms",
    );
    expect(
      providerTestResultMessage({
        available: false,
        error: "Provider entry `x` was not found",
      }),
    ).toContain("not found");
  });

  it("normalizes, filters, and deduplicates draft lines", () => {
    expect(criteriaLinesFromDraft("  形成证据链  \n\n形成证据链\n解释   局限")).toEqual([
      "形成证据链",
      "解释 局限",
    ]);
  });

  it("validates empty, oversized, and valid criteria lists", () => {
    expect(validateCriteriaLines([])).toContain("至少");
    expect(validateCriteriaLines(Array.from({ length: 13 }, (_, i) => `标准 ${i}`))).toContain(
      "最多",
    );
    expect(validateCriteriaLines(["x".repeat(241)])).toContain("240");
    expect(validateCriteriaLines(["形成证据链"])).toBeNull();
  });

  it("parses and validates max cycle budget drafts", () => {
    expect(parseMaxCyclesDraft("5")).toBe(5);
    expect(parseMaxCyclesDraft("1.5")).toBeNull();
    expect(validateMaxCycles(null, 0)).toContain("整数");
    expect(validateMaxCycles(0, 0)).toContain("大于");
    expect(validateMaxCycles(2, 3)).toContain("不能小于");
    expect(validateMaxCycles(21, 0)).toContain("最多");
    expect(validateMaxCycles(5, 3)).toBeNull();
  });

  it("parses and validates persisted auto-run policy drafts", () => {
    expect(parseOptionalPositiveIntegerDraft("")).toBeNull();
    expect(parseOptionalPositiveIntegerDraft("30")).toBe(30);
    expect(Number.isNaN(parseOptionalPositiveIntegerDraft("1.5"))).toBe(true);
    expect(validateAutoRunPolicyDraft(false, null, null, Number.NaN)).toBeNull();
    expect(validateAutoRunPolicyDraft(true, 0, 650, null)).toContain("每次轮数");
    expect(validateAutoRunPolicyDraft(true, 1, 100, null)).toContain("空闲延迟");
    expect(validateAutoRunPolicyDraft(true, 1, 650, 0)).toContain("最长耗时");
    expect(validateAutoRunPolicyDraft(true, 1, 650, null, 0)).toContain("token");
    expect(validateAutoRunPolicyDraft(true, 3, 1000, 30, 5000)).toBeNull();
  });

  it("keeps success criteria validation independent from generation strategy", () => {
    const llmSuggestions = [
      "明确研究对象、关键变量、样本/实验条件与科研目标边界。",
      "形成可追溯证据链，记录文献、数据、分析步骤和证据强度。",
    ];

    expect(validateCriteriaLines(llmSuggestions)).toBeNull();
  });
});
