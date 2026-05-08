import { describe, expect, it } from "vitest";
import {
  buildSchedulerPlanHierarchy,
  schedulerStageLabel,
} from "./schedulerPlanHierarchy";

describe("schedulerPlanHierarchy", () => {
  it("keeps old flat plans in legacy mode", () => {
    const hierarchy = buildSchedulerPlanHierarchy({
      subtasks: [
        {
          id: "a",
          description: "Legacy task",
          agentType: "executor",
          dependencies: [],
          critical: false,
        },
      ],
    });

    expect(hierarchy.legacyFlat).toBe(true);
    expect(hierarchy.entryAgentType).toBe("general-purpose");
    expect(hierarchy.executionSupervisorAgentType).toBe("executor");
  });

  it("renders new plans as General to Executor to children", () => {
    const hierarchy = buildSchedulerPlanHierarchy({
      entryAgentType: "general-purpose",
      executionSupervisorAgentType: "executor",
      subtasks: [
        {
          id: "retrieve",
          description: "检索 MID1IP1 肝癌证据",
          agentType: "literature-search",
          supervisorAgentType: "executor",
          stage: "retrieve",
          dependencies: [],
          critical: true,
        },
      ],
    });

    expect(hierarchy.legacyFlat).toBe(false);
    expect(hierarchy.children[0].supervisorAgentType).toBe("executor");
    expect(hierarchy.children[0].stage).toBe("retrieve");
    expect(schedulerStageLabel("retrieve")).toBe("资料/数据检索");
  });
});
