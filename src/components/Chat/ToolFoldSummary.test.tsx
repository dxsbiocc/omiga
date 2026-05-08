import { renderToStaticMarkup } from "react-dom/server";
import { createTheme } from "@mui/material/styles";
import { describe, expect, it } from "vitest";
import { getChatTokens } from "./chatTokens";
import {
  firstRunningToolName,
  formatToolDuration,
  getNestedToolPanelOpen,
  parseStructuredToolErrorHint,
  summarizeReactFold,
  summarizeToolGroup,
  ToolFoldHeader,
  toolCallPanelTitle,
  toolDisplayOutputText,
  toolGroupFlowComplete,
} from "./ToolFoldSummary";

const chat = getChatTokens(createTheme());

describe("ToolFoldSummary helpers", () => {
  it("summarizes mixed tool groups without scanning UI state", () => {
    expect(summarizeToolGroup([])).toBe("");
    expect(
      summarizeToolGroup([{ role: "tool", toolCall: { name: "search" } }]),
    ).toBe("search");
    expect(
      summarizeToolGroup([
        { role: "tool", toolCall: { name: "bash" } },
        { role: "tool", toolCall: { name: "bash" } },
        { role: "tool", toolCall: { name: "file_read" } },
        { role: "tool", toolCall: { name: "TaskCreate" } },
      ]),
    ).toBe("Ran 2 commands, viewed a file, 1 more");
  });

  it("keeps ReAct fold reasoning and latest running tool labels stable", () => {
    const fold = [
      { role: "assistant", content: "thinking" },
      { role: "tool", toolCall: { name: "search", status: "running" as const } },
      { role: "tool", toolCall: { name: "bash", status: "running" as const } },
    ];

    expect(summarizeReactFold(fold)).toBe("Reasoning · Ran 1 command, viewed a file");
    expect(firstRunningToolName(fold)).toBe("bash");
    expect(toolGroupFlowComplete(fold)).toBe(false);
    expect(toolGroupFlowComplete([{ role: "tool", toolCall: { name: "bash" } }])).toBe(true);
  });

  it("formats tool details defensively", () => {
    expect(toolCallPanelTitle('{"description":"Run tests"}', "bash")).toBe("Run tests");
    expect(toolCallPanelTitle("not-json", "bash")).toBe("bash");
    expect(
      toolDisplayOutputText(
        { role: "tool", content: "`bash` completed" },
        { name: "bash", status: "completed" },
      ),
    ).toBe("");
    expect(
      toolDisplayOutputText(
        { role: "tool", content: "`search`" },
        { name: "search", status: "running" },
      ),
    ).toBe("");
    expect(
      toolDisplayOutputText(
        { role: "tool", content: "real output" },
        { name: "bash", output: " from tool " },
      ),
    ).toBe("from tool");
    expect(getNestedToolPanelOpen("a", { name: "bash", status: "running" }, {})).toBe(true);
    expect(getNestedToolPanelOpen("a", { name: "bash", status: "running" }, { a: false })).toBe(false);
    expect(formatToolDuration(1000, 1450)).toBe("450ms");
    expect(formatToolDuration(1000, 2600)).toBe("1.6s");
  });

  it("extracts actionable hints from structured tool error JSON", () => {
    expect(parseStructuredToolErrorHint("not-json")).toBeNull();
    expect(parseStructuredToolErrorHint(JSON.stringify({ error: "plain" }))).toBeNull();

    const hint = parseStructuredToolErrorHint(
      JSON.stringify({
        error: "source_disabled",
        message: "data.geo is disabled",
        route: "data.geo",
        next_action: "Enable this plugin in Settings → Plugins.",
        diagnostics_hint: "Open plugin details.",
        recoverable: true,
      }),
    );

    expect(hint).toEqual({
      error: "source_disabled",
      message: "data.geo is disabled",
      details: null,
      route: "data.geo",
      nextAction: "Enable this plugin in Settings → Plugins.",
      diagnosticsHint: "Open plugin details.",
      recoverable: true,
    });
  });
});

describe("ToolFoldHeader", () => {
  it("server-renders memoized status labels", () => {
    const html = renderToStaticMarkup(
      <ToolFoldHeader
        foldId="rf-1"
        expanded={false}
        summary="Reasoning · Ran 2 commands"
        anyRunning
        runningToolName="bash"
        runningToolCount={2}
        showGroupDone={false}
        isLastFold={false}
        activityIsStreaming={false}
        waitingFirstChunk={false}
        chat={chat}
        onToggle={() => undefined}
      />,
    );

    expect(html).toContain("Reasoning · Ran 2 commands");
    expect(html).toContain("2 并行");
    expect(html).toContain("2 并行运行中");
  });

  it("does not fan hover effects out to every icon in the fold header", () => {
    const html = renderToStaticMarkup(
      <ToolFoldHeader
        foldId="rf-hover"
        expanded={false}
        summary="Reasoning · Ran a command"
        anyRunning={false}
        runningToolName={null}
        runningToolCount={0}
        showGroupDone={false}
        isLastFold={false}
        activityIsStreaming={false}
        waitingFirstChunk={false}
        chat={chat}
        onToggle={() => undefined}
      />,
    );

    expect(html).toContain(":hover");
    expect(html).not.toContain(":hover&gt;svg");
    expect(html).not.toContain(":hover>svg");
    expect(html).not.toContain("svg:first-of-type");
  });
});
