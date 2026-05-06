import { beforeEach, describe, expect, it } from "vitest";
import { useActivityStore } from "./activityStore";

describe("activityStore", () => {
  beforeEach(() => {
    useActivityStore.getState().clearAllActivity();
  });

  it("clears the live tool hint when the last running tool finishes", () => {
    const activity = useActivityStore.getState();

    activity.setCurrentToolHint("fetch");
    activity.onToolUseStart("fetch-1", "fetch", { toolName: "fetch" });
    activity.onToolResultDone("fetch-1", { output: "ok" });

    const state = useActivityStore.getState();
    expect(state.currentToolHint).toBeNull();
    expect(state.executionSteps).toMatchObject([
      { id: "tool-fetch-1", status: "done", toolName: "fetch" },
    ]);
  });
});
