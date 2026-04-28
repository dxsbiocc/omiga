import { renderToStaticMarkup } from "react-dom/server";
import { createTheme } from "@mui/material/styles";
import { describe, expect, it } from "vitest";
import { getChatTokens } from "./chatTokens";
import {
  AssistantTraceItem,
  CollapsibleThoughtTrace,
  LiveIntermediateTrace,
} from "./AssistantTraceItem";

const chat = getChatTokens(createTheme());

describe("AssistantTraceItem", () => {
  it("renders intermediate assistant text as a collapsed Thoughts row", () => {
    const html = renderToStaticMarkup(
      <AssistantTraceItem
        content="I will **inspect** files."
        intermediate
        chat={chat}
        components={{}}
      />,
    );

    expect(html).toContain("Thoughts");
    expect(html).toContain("I will inspect files.");
  });

  it("omits empty trace content", () => {
    expect(
      renderToStaticMarkup(
        <AssistantTraceItem content="   " chat={chat} components={{}} />,
      ),
    ).toBe("");
  });
});

describe("CollapsibleThoughtTrace", () => {
  it("renders full markdown when expanded", () => {
    const html = renderToStaticMarkup(
      <CollapsibleThoughtTrace
        content="I will **inspect** files."
        defaultExpanded
        chat={chat}
        components={{}}
      />,
    );

    expect(html).toContain("Thoughts");
    expect(html).toContain("<strong>inspect</strong>");
  });
});

describe("LiveIntermediateTrace", () => {
  it("renders streaming trace as a collapsed Thoughts row", () => {
    const html = renderToStaticMarkup(
      <LiveIntermediateTrace
        foldId="rf-1"
        content="streaming **thought**"
        chat={chat}
        components={{}}
      />,
    );

    expect(html).toContain("Thoughts");
    expect(html).toContain("streaming thought");
  });
});
