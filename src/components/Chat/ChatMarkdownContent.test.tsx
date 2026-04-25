import { renderToStaticMarkup } from "react-dom/server";
import { createTheme } from "@mui/material/styles";
import { describe, expect, it } from "vitest";
import {
  ChatMarkdownContent,
  hasMarkdownMath,
  normalizeChatMarkdown,
} from "./ChatMarkdownContent";
import { getChatTokens } from "./chatTokens";

const chat = getChatTokens(createTheme());

describe("ChatMarkdownContent", () => {
  it("detects math only when math syntax is present", () => {
    expect(hasMarkdownMath("plain text with $HOME and tables")).toBe(false);
    expect(hasMarkdownMath("inline math: \\(x + y\\)")).toBe(true);
    expect(hasMarkdownMath("block math:\n$$x^2$$")).toBe(true);
  });

  it("normalizes br tags and common broken table continuations", () => {
    expect(normalizeChatMarkdown("a<br>b")).toBe("a\nb");
    expect(
      normalizeChatMarkdown("| A | B |\n|---|---|\n| a | b |\ncontinued"),
    ).toContain("| a | b continued |");
  });

  it("server-renders normal markdown without forcing KaTeX output", () => {
    const html = renderToStaticMarkup(
      <ChatMarkdownContent
        content="hello **world**"
        components={{}}
        chat={chat}
      />,
    );

    expect(html).toContain("<strong>world</strong>");
    expect(html).not.toContain("katex");
  });

  it("server-renders math markdown through KaTeX only when needed", () => {
    const html = renderToStaticMarkup(
      <ChatMarkdownContent
        content="inline math: $x + y$"
        components={{}}
        chat={chat}
      />,
    );

    expect(html).toContain("katex");
    expect(html).toContain("x");
    expect(html).toContain("y");
  });
});
