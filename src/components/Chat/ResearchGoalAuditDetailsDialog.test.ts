import { describe, expect, it } from "vitest";
import { researchGoalAuditDetailSections } from "./ResearchGoalAuditDetailsDialog";
import type { ResearchGoalAudit } from "./ResearchGoalStatusPill";

const audit: ResearchGoalAudit = {
  complete: false,
  reviewSource: "llm",
  confidence: "medium",
  finalReportReady: false,
  summary: "仍需推进。",
  criteria: [],
  missingRequirements: ["缺少独立验证"],
  nextActions: ["继续检索"],
  limitations: ["样本量不足", "机制解释仍偏推断"],
  conflictingEvidence: ["研究 A 与研究 B 对通路方向结论不一致"],
  secondOpinion: {
    reviewSource: "llm_second_opinion",
    agreesComplete: false,
    confidence: "high",
    summary: "二次审计不同意完成。",
    blockingConcerns: ["缺少独立验证"],
    requiredNextActions: ["补充交叉验证"],
  },
};

describe("ResearchGoalAuditDetailsDialog helpers", () => {
  it("keeps LLM limitations and conflicting evidence as first-class sections", () => {
    const sections = researchGoalAuditDetailSections(audit);

    expect(sections.map((section) => section.title)).toEqual([
      "缺口",
      "下一步",
      "局限性",
      "冲突证据",
    ]);
    expect(sections.find((section) => section.title === "局限性")?.items).toEqual([
      "样本量不足",
      "机制解释仍偏推断",
    ]);
    expect(sections.find((section) => section.title === "冲突证据")?.items).toEqual([
      "研究 A 与研究 B 对通路方向结论不一致",
    ]);
    expect(audit.secondOpinion?.blockingConcerns).toEqual(["缺少独立验证"]);
  });

  it("returns explicit empty-state text when an audit omits optional fields", () => {
    const sections = researchGoalAuditDetailSections({
      ...audit,
      limitations: undefined,
      conflictingEvidence: undefined,
    });

    expect(sections.find((section) => section.title === "局限性")?.emptyText).toContain(
      "未报告局限性",
    );
    expect(sections.find((section) => section.title === "冲突证据")?.emptyText).toContain(
      "未报告冲突证据",
    );
  });
});
