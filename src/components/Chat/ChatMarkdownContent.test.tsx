import { renderToStaticMarkup } from "react-dom/server";
import { createTheme } from "@mui/material/styles";
import { describe, expect, it } from "vitest";
import {
  ChatMarkdownContent,
  countReferenceEntries,
  hasMarkdownMath,
  hideInlineBase64Images,
  normalizeChatMarkdown,
  normalizeSafeHtmlAnchors,
  splitTerminalReferences,
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

  it("normalizes safe html citation anchors into markdown links", () => {
    expect(
      normalizeSafeHtmlAnchors(
        'IDO1 evidence <a href="https://pubmed.ncbi.nlm.nih.gov/12345678/">PubMed 12345678</a>',
      ),
    ).toBe(
      "IDO1 evidence [PubMed 12345678](<https://pubmed.ncbi.nlm.nih.gov/12345678/>)",
    );
    expect(normalizeSafeHtmlAnchors('<a href="javascript:alert(1)">bad</a>')).toContain(
      "javascript:alert",
    );
  });

  it("hides inline base64 image payloads instead of rendering huge chat text", () => {
    const payload = `iVBORw0KGgo${"A".repeat(180)}`;
    const markdown = `![火山图]\n(data:image/png;base64,${payload})`;
    const normalized = hideInlineBase64Images(markdown);

    expect(normalized).toContain("火山图");
    expect(normalized).toContain("Markdown 文件路径引用");
    expect(normalized).not.toContain(payload);
    expect(normalizeChatMarkdown(markdown)).not.toContain(payload);
  });

  it("splits a terminal references section for accordion rendering", () => {
    const split = splitTerminalReferences(
      "结论正文 [[1]](https://doi.org/10.1/example).\n\n## References\n\n[1] Paper A. [DOI](https://doi.org/10.1/example)\n[2] Paper B. [PubMed](https://pubmed.ncbi.nlm.nih.gov/123/)",
    );

    expect(split?.main).toBe("结论正文 [[1]](https://doi.org/10.1/example).");
    expect(split?.heading).toBe("References");
    expect(split?.count).toBe(2);
    expect(split?.references).toContain("Paper B");
  });

  it("counts unnumbered references by URL fallback", () => {
    expect(
      countReferenceEntries(
        "Paper A https://doi.org/10.1/example\nWrapped note\nPaper B https://pubmed.ncbi.nlm.nih.gov/123/",
      ),
    ).toBe(2);
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

  it("server-renders terminal references as a separate accordion", () => {
    const html = renderToStaticMarkup(
      <ChatMarkdownContent
        content={"正文内容。\n\n## References\n\n[1] Paper A. [DOI](https://doi.org/10.1/example)\n[2] Paper B. [PubMed](https://pubmed.ncbi.nlm.nih.gov/123/)"}
        components={{}}
        chat={chat}
      />,
    );

    expect(html).toContain("References (2)");
    expect(html).toContain("Paper A");
  });
});
