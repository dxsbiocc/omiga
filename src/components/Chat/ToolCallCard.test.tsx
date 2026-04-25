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
        generatedThoughtSummary=""
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

  it("renders generated thought summaries and pending ask-user state", () => {
    const html = renderToStaticMarkup(
      <ToolCallCard
        foldId="rf-1"
        messageId="tool-2"
        content=""
        timestamp={1000}
        toolCall={{ name: "ask_user_question", status: "running" }}
        previousAssistantHasText={false}
        generatedThoughtSummary="需要向用户确认下一步。"
        nestedOpen
        showAskUserPanel
        chat={chat}
        components={{}}
        onToggle={() => undefined}
      />,
    );

    expect(html).toContain("思考摘要");
    expect(html).toContain("需要向用户确认下一步。");
    expect(html).toContain("等待你的回答");
  });
});
