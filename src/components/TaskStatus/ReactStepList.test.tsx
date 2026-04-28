import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { ExecutionStep } from "../../state/activityStore";
import { ReactStepList } from "./ReactStepList";

const surfaceContext = {
  isConnecting: false,
  isStreaming: true,
  waitingFirstChunk: false,
  toolHintFallback: null,
};

function renderSteps(steps: ExecutionStep[]): string {
  return renderToStaticMarkup(
    <ReactStepList steps={steps} elapsedLabel="12s" surfaceContext={surfaceContext} />,
  );
}

describe("ReactStepList", () => {
  it("labels tool errors as model-visible exceptions instead of manual work", () => {
    const html = renderSteps([
      {
        id: "tool-1",
        title: "bash",
        status: "done",
        toolName: "bash",
        failed: true,
        toolOutput: "Rscript: command not found",
      },
      {
        id: "tool-2",
        title: "bash",
        status: "done",
        toolName: "bash",
        failed: true,
        toolOutput: "No such file or directory",
      },
      {
        id: "tool-3",
        title: "file_write",
        status: "done",
        toolName: "file_write",
        failed: false,
      },
    ]);

    expect(html).toContain("异常返回（模型已接收）");
    expect(html).toContain("这些是工具调用返回的错误");
    expect(html).toContain("2 次异常");
    expect(html).toContain("bash");
    expect(html).toContain("×2");
    expect(html).toContain("成功调用（最近优先）");
    expect(html).toContain("1 次成功");
    expect(html).not.toContain("需处理");
  });

  it("marks running background operation steps with a dedicated badge", () => {
    const html = renderSteps([
      {
        id: "op-post-turn-suggestions-1",
        title: "生成下一步建议",
        status: "running",
        summary: "后台独立 LLM 正在生成下一步建议",
      },
    ]);

    expect(html).toContain("生成下一步建议");
    expect(html).toContain("后台");
  });
});
