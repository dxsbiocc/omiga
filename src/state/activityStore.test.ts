import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useActivityStore } from "./activityStore";

describe("activityStore", () => {
  let nowSpy: ReturnType<typeof vi.spyOn>;

  beforeEach(() => {
    nowSpy = vi.spyOn(Date, "now").mockReturnValue(1_000);
    useActivityStore.getState().clearAllActivity();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("clears the live tool hint when the last running tool finishes", () => {
    const activity = useActivityStore.getState();

    activity.setCurrentToolHint("fetch");
    nowSpy.mockReturnValue(2_000);
    activity.onToolUseStart("fetch-1", "fetch", { toolName: "fetch" });
    nowSpy.mockReturnValue(3_500);
    activity.onToolResultDone("fetch-1", { output: "ok" });

    const state = useActivityStore.getState();
    expect(state.currentToolHint).toBeNull();
    expect(state.executionSteps).toMatchObject([
      {
        id: "tool-fetch-1",
        status: "done",
        toolName: "fetch",
        startedAt: 2_000,
        completedAt: 3_500,
      },
    ]);
  });

  it("preserves todo timing across status transitions", () => {
    const activity = useActivityStore.getState();

    activity.setActiveTodos([
      {
        id: "todo-1",
        content: "分析输入数据",
        activeForm: "正在分析输入数据",
        status: "in_progress",
      },
    ]);
    nowSpy.mockReturnValue(5_200);
    activity.setActiveTodos([
      {
        id: "todo-1",
        content: "分析输入数据",
        activeForm: "分析输入数据",
        status: "completed",
      },
    ]);

    expect(useActivityStore.getState().activeTodos).toEqual([
      {
        id: "todo-1",
        content: "分析输入数据",
        activeForm: "分析输入数据",
        status: "completed",
        startedAt: 1_000,
        completedAt: 5_200,
        updatedAt: 5_200,
      },
    ]);
  });
});
