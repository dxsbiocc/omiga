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

function renderSteps(
  steps: ExecutionStep[],
  context = surfaceContext,
): string {
  return renderToStaticMarkup(
    <ReactStepList steps={steps} elapsedLabel="12s" surfaceContext={context} />,
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
        startedAt: 1_000,
        completedAt: 3_500,
      },
    ]);

    expect(html).toContain("异常返回（模型已接收）");
    expect(html).toContain("这些是工具调用返回的错误");
    expect(html).toContain("2 次异常");
    expect(html).toContain("bash");
    expect(html).toContain("×2");
    expect(html).toContain("成功调用（最近优先）");
    expect(html).toContain("1 次成功");
    expect(html).toContain("2s");
    expect(html).not.toContain("需处理");
  });

  it("shows per-step execution time for running and completed rows", () => {
    const html = renderSteps([
      {
        id: "tool-read",
        title: "file_read",
        status: "done",
        toolName: "file_read",
        startedAt: 1_000,
        completedAt: 4_100,
      },
      {
        id: "tool-bash",
        title: "bash",
        status: "running",
        toolName: "bash",
        startedAt: Date.now() - 2_200,
      },
    ]);

    expect(html).toContain("3s");
    expect(html).toContain("2s");
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

  it("folds stale running tool rows into completed calls once the stream is idle", () => {
    const html = renderSteps(
      [
        {
          id: "tool-bash-1",
          title: "bash",
          status: "running",
          toolName: "bash",
        },
      ],
      {
        isConnecting: false,
        isStreaming: false,
        waitingFirstChunk: false,
        toolHintFallback: null,
      },
    );

    expect(html).toContain("已完成");
    expect(html).not.toContain("正在执行");
    expect(html).toContain("成功调用（最近优先）");
    expect(html).toContain("1 次成功");
  });

  it("renders execution record insights inside the task execution step list", () => {
    const html = renderSteps([
      {
        id: "tool-record-detail",
        title: "execution_record_detail",
        status: "done",
        toolName: "execution_record_detail",
        toolOutput: JSON.stringify({
          found: true,
          recordId: "execrec_parent",
          record: {
            id: "execrec_parent",
            unitId: "bulk_de",
            kind: "template",
            status: "success",
          },
          parsed: {
            metadata: {
              paramSources: {
                method: "user_preflight",
                fdr: "default",
              },
              preflight: {
                answeredParams: [{ param: "method" }],
              },
            },
            outputSummary: {
              outputs: {
                table: ["de.tsv"],
              },
            },
          },
          lineage: {
            parentExecutionId: null,
            childCount: 1,
          },
          children: [{ id: "execrec_child" }],
        }),
      },
    ]);

    expect(html).toContain("Execution record detail");
    expect(html).toContain("Execution detail");
    expect(html).toContain("paramSources user_preflight: 1");
    expect(html).toContain("1 answered question");
    expect(html).toContain("table: 1");
  });
});
