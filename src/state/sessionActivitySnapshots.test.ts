import { describe, expect, it } from "vitest";
import {
  activitySnapshotHasRecords,
  buildSessionActivitySnapshot,
  finalizeActivitySnapshot,
  historicalActivityStateFromSnapshot,
} from "./sessionActivitySnapshots";

describe("sessionActivitySnapshots", () => {
  it("captures the latest per-session task-panel records", () => {
    const snapshot = buildSessionActivitySnapshot("session-1", {
      roundId: "round-1",
      executionSteps: [
        { id: "connect", title: "等待响应", status: "done" },
        {
          id: "tool-call-1",
          title: "bash",
          status: "running",
          toolName: "bash",
          input: "{\"command\":\"echo ok\"}",
        },
      ],
      executionStartedAt: 1000,
      executionEndedAt: null,
      activeTodos: null,
      backgroundJobs: [
        {
          id: "bg-1",
          toolUseId: "bg-1",
          label: "后台任务",
          state: "running",
        },
      ],
    }, 2000);

    expect(snapshot.sessionId).toBe("session-1");
    expect(snapshot.roundId).toBe("round-1");
    expect(activitySnapshotHasRecords(snapshot)).toBe(true);
    expect(snapshot.executionSteps).toHaveLength(2);
    expect(snapshot.backgroundJobs).toHaveLength(1);
  });

  it("freezes restored refresh snapshots as historical records", () => {
    const snapshot = buildSessionActivitySnapshot("session-1", {
      roundId: "round-1",
      executionSteps: [
        { id: "connect", title: "等待响应", status: "done" },
        { id: "think", title: "推理中", status: "running" },
      ],
      executionStartedAt: 1000,
      executionEndedAt: null,
      activeTodos: null,
      backgroundJobs: [],
    }, 2500);

    const restored = historicalActivityStateFromSnapshot(snapshot);

    expect(restored.isStreaming).toBe(false);
    expect(restored.executionEndedAt).toBe(2500);
    expect(restored.executionSteps.every((step) => step.status === "done")).toBe(
      true,
    );
  });

  it("finalizes active snapshots before preserving them as latest history", () => {
    const snapshot = buildSessionActivitySnapshot("session-1", {
      roundId: null,
      executionSteps: [{ id: "reply", title: "解析输出", status: "running" }],
      executionStartedAt: 1000,
      executionEndedAt: null,
      activeTodos: null,
      backgroundJobs: [],
    }, 1500);

    const finalized = finalizeActivitySnapshot(snapshot, 3000);

    expect(finalized.executionEndedAt).toBe(3000);
    expect(finalized.executionSteps[0].status).toBe("done");
  });
});
