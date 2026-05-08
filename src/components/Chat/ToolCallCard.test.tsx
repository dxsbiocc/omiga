import { renderToStaticMarkup } from "react-dom/server";
import { createTheme } from "@mui/material/styles";
import { describe, expect, it } from "vitest";
import { getChatTokens } from "./chatTokens";
import { ToolCallCard } from "./ToolCallCard";

const chat = getChatTokens(createTheme());

describe("ToolCallCard", () => {
  it("renders a completed tool with description, input, output, and duration", () => {
    const html = renderToStaticMarkup(
      <ToolCallCard
        foldId="rf-1"
        messageId="tool-1"
        content="fallback output"
        timestamp={1000}
        toolCall={{
          name: "bash",
          status: "completed",
          input: JSON.stringify({ description: "Run unit tests", command: "npm test" }),
          output: "all passed",
          completedAt: 2450,
        }}
        previousAssistantHasText
        nestedOpen
        showAskUserPanel={false}
        chat={chat}
        components={{}}
        onToggle={() => undefined}
      />,
    );

    expect(html).toContain("Run unit tests");
    expect(html).toContain("bash");
    expect(html).toContain("Command");
    expect(html).toContain("npm test");
    expect(html).toContain("all passed");
    expect(html).toContain("1.4s");
  });

  it("does not invent thought summaries for running tool placeholders", () => {
    const html = renderToStaticMarkup(
      <ToolCallCard
        foldId="rf-1"
        messageId="tool-2"
        content="`ask_user_question`"
        timestamp={1000}
        toolCall={{ name: "ask_user_question", status: "running" }}
        previousAssistantHasText={false}
        nestedOpen
        showAskUserPanel
        chat={chat}
        components={{}}
        onToggle={() => undefined}
      />,
    );

    expect(html).not.toContain("思考摘要");
    expect(html).not.toContain("Output");
    expect(html).not.toContain("`ask_user_question`");
    expect(html).toContain("等待你的回答");
  });

  it("renders only real assistant preface text before a tool", () => {
    const html = renderToStaticMarkup(
      <ToolCallCard
        foldId="rf-1"
        messageId="tool-3"
        content=""
        timestamp={1000}
        prefaceBeforeTools="先查看最新资料。"
        toolCall={{ name: "search", status: "running" }}
        previousAssistantHasText={false}
        nestedOpen={false}
        showAskUserPanel={false}
        chat={chat}
        components={{}}
        onToggle={() => undefined}
      />,
    );

    expect(html).toContain("Thoughts");
    expect(html).toContain("先查看最新资料。");
    expect(html).not.toContain("search");
    expect(html).not.toContain("思考摘要");
  });

  it("omits tool cards when both input and output are empty", () => {
    const html = renderToStaticMarkup(
      <ToolCallCard
        foldId="rf-1"
        messageId="tool-4"
        content="`bash`"
        timestamp={1000}
        toolCall={{ name: "bash", status: "running", input: "   ", output: "" }}
        previousAssistantHasText={false}
        nestedOpen
        showAskUserPanel={false}
        chat={chat}
        components={{}}
        onToggle={() => undefined}
      />,
    );

    expect(html).toBe("");
    expect(html).not.toContain("No command or output yet.");
  });

  it("renders structured retrieval error hints before raw JSON output", () => {
    const html = renderToStaticMarkup(
      <ToolCallCard
        foldId="rf-1"
        messageId="tool-5"
        content=""
        timestamp={1000}
        toolCall={{
          name: "search",
          status: "completed",
          input: JSON.stringify({ category: "data", source: "geo", query: "brca" }),
          output: JSON.stringify({
            error: "source_disabled",
            message: "data.geo is available as a local retrieval plugin route, but it is disabled.",
            route: "data.geo",
            next_action:
              "Enable this plugin in Settings → Plugins, then retry the same search/query/fetch call.",
            diagnostics_hint:
              "Open Settings → Plugins → Details to inspect the route.",
            recoverable: true,
            results: [],
          }),
          completedAt: 1200,
        }}
        previousAssistantHasText
        nestedOpen
        showAskUserPanel={false}
        chat={chat}
        components={{}}
        onToggle={() => undefined}
      />,
    );

    expect(html).toContain("Needs action");
    expect(html).toContain("source_disabled");
    expect(html).toContain("Route: data.geo");
    expect(html).toContain("Settings → Plugins");
    expect(html).toContain("Diagnostics:");
    expect(html).toContain("Raw output");
  });

  it("keeps tool-row hover scoped to the row surface instead of animating every icon", () => {
    const html = renderToStaticMarkup(
      <ToolCallCard
        foldId="rf-1"
        messageId="tool-hover"
        content="done"
        timestamp={1000}
        toolCall={{
          name: "bash",
          status: "completed",
          input: JSON.stringify({ description: "Run hover regression" }),
          output: "done",
          completedAt: 1200,
        }}
        previousAssistantHasText
        nestedOpen={false}
        showAskUserPanel={false}
        chat={chat}
        components={{}}
        onToggle={() => undefined}
      />,
    );

    expect(html).toContain(":hover");
    expect(html).not.toContain(":hover&gt;svg");
    expect(html).not.toContain(":hover>svg");
    expect(html).not.toContain("svg:first-of-type");
    expect(html).not.toContain("svg:nth-of-type");
  });
});
