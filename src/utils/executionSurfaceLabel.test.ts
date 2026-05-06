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

  it("does not keep showing the previous tool hint after the tool row completed", () => {
    expect(
      getExecutionSurfaceView(
        [
          { id: "connect", title: "等待响应", status: "done" },
          { id: "think", title: "推理中", status: "done" },
          {
            id: "tool-fetch-1",
            title: "fetch",
            status: "done",
            toolName: "fetch",
          },
        ],
        {
          isConnecting: false,
          isStreaming: true,
          waitingFirstChunk: false,
          toolHintFallback: "fetch",
        },
      ),
    ).toMatchObject({
      label: "解析输出",
      kind: "generating",
      toolName: null,
    });
  });
});
