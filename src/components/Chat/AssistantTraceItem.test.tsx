import { renderToStaticMarkup } from "react-dom/server";
import { createTheme } from "@mui/material/styles";
import { describe, expect, it } from "vitest";
import { getChatTokens } from "./chatTokens";
import { AssistantTraceItem, LiveIntermediateTrace } from "./AssistantTraceItem";

const chat = getChatTokens(createTheme());

describe("AssistantTraceItem", () => {
  it("renders intermediate assistant markdown with the thinking label", () => {
    const html = renderToStaticMarkup(
      <AssistantTraceItem
        content="I will **inspect** files."
        intermediate
        chat={chat}
        components={{}}
      />,
    );

    expect(html).toContain("思考");
    expect(html).toContain("<strong>inspect</strong>");
  });

  it("omits empty trace content", () => {
    expect(
      renderToStaticMarkup(
        <AssistantTraceItem content="   " chat={chat} components={{}} />,
      ),
    ).toBe("");
  });
});

describe("LiveIntermediateTrace", () => {
  it("renders streaming trace copy and cursor", () => {
    const html = renderToStaticMarkup(
      <LiveIntermediateTrace
        foldId="rf-1"
        content="streaming **thought**"
        chat={chat}
        components={{}}
      />,
    );

    expect(html).toContain("思考中");
    expect(html).toContain("流式中；下一次行动会接在这里");
    expect(html).toContain("<strong>thought</strong>");
  });
});
