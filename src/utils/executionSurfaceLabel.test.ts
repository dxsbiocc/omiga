import { describe, expect, it } from "vitest";
import { getExecutionSurfaceView } from "./executionSurfaceLabel";

describe("getExecutionSurfaceView", () => {
  it("uses the live connect-step title while connecting", () => {
    expect(
      getExecutionSurfaceView(
        [{ id: "connect", title: "生成计划中", status: "running" }],
        {
          isConnecting: true,
          isStreaming: false,
          waitingFirstChunk: false,
          toolHintFallback: null,
        },
      ),
    ).toMatchObject({
      label: "生成计划中",
      kind: "waiting",
    });
  });
});
