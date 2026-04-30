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

  it("does not report a stale tool as running after streaming is already idle", () => {
    expect(
      getExecutionSurfaceView(
        [{ id: "tool-bash-1", title: "bash", status: "running", toolName: "bash" }],
        {
          isConnecting: false,
          isStreaming: false,
          waitingFirstChunk: false,
          toolHintFallback: null,
        },
      ),
    ).toMatchObject({
      label: "已完成",
      kind: "finished",
    });
  });
});
