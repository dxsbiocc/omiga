import { renderToStaticMarkup } from "react-dom/server";
import { createTheme } from "@mui/material/styles";
import { describe, expect, it } from "vitest";
import {
  AssistantMessageBubble,
  formatAssistantTokenUsage,
  parseAssistantInterruptionNotice,
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

  it("renders cancellation markers as a semantic interruption notice", () => {
    const parsed = parseAssistantInterruptionNotice(
      "已经完成一部分。\n\n[Cancelled]\n[Cancelled by user]",
    );

    expect(parsed.visibleContent).toBe("已经完成一部分。");
    expect(parsed.notice?.kind).toBe("cancelled");

    const html = renderToStaticMarkup(
      <AssistantMessageBubble
        content="已经完成一部分。\n\n[Cancelled by user]"
        components={{}}
        chat={chat}
        bubbleRadiusPx={10}
      />,
    );

    expect(html).toContain("已经完成一部分");
    expect(html).toContain("本轮已中断");
    expect(html).not.toContain("[Cancelled by user]");
  });

  it("renders legacy tool-round stop markers as a guided stop notice", () => {
    const parsed = parseAssistantInterruptionNotice(
      "上方记录可继续使用。\n\n[Stopped: exceeded 100 tool rounds]",
    );

    expect(parsed.visibleContent).toBe("上方记录可继续使用。");
    expect(parsed.notice?.kind).toBe("tool-limit");
    expect(parsed.notice?.title).toContain("100");

    const html = renderToStaticMarkup(
      <AssistantMessageBubble
        content="[Stopped: exceeded 100 tool rounds]"
        components={{}}
        chat={chat}
        bubbleRadiusPx={10}
      />,
    );

    expect(html).toContain("已达到工具调用上限");
    expect(html).not.toContain("[Stopped:");
  });
});
