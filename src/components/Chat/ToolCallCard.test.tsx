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
        toolCall={{ name: "web_search", status: "running" }}
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
    expect(html).not.toContain("思考摘要");
  });
});
