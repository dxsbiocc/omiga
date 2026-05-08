import { renderToStaticMarkup } from "react-dom/server";
import { createTheme } from "@mui/material/styles";
import { describe, expect, it } from "vitest";
import {
  AssistantMessageBubble,
  formatAssistantTokenUsage,
} from "./AssistantMessageBubble";
import { getChatTokens } from "./chatTokens";

const chat = getChatTokens(createTheme());

describe("AssistantMessageBubble", () => {
  it("formats token usage without redundant total", () => {
    expect(formatAssistantTokenUsage({ input: 10, output: 5, total: 15 })).toBe(
      "输入 10 · 输出 5",
    );
    expect(
      formatAssistantTokenUsage({ input: 1000, output: 25, total: 2000, provider: "deepseek" }),
    ).toBe("输入 1,000 · 输出 25 · Σ 2,000 · deepseek");
  });

  it("server-renders assistant markdown and token usage", () => {
    const html = renderToStaticMarkup(
      <AssistantMessageBubble
        content="hello **assistant**"
        tokenUsage={{ input: 1200, output: 34, provider: "mock" }}
        components={{}}
        chat={chat}
        bubbleRadiusPx={10}
      />,
    );

    expect(html).toContain("<strong>assistant</strong>");
    expect(html).toContain("输入 1,200");
    expect(html).toContain("输出 34");
    expect(html).toContain("mock");
  });
});
