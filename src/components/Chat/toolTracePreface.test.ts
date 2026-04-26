import { describe, expect, it } from "vitest";
import { toolTracePrefaceFromText } from "./toolTracePreface";

describe("toolTracePrefaceFromText", () => {
  it("preserves the full assistant text before a tool call", () => {
    const text = `
我会先检查当前实现，然后再修改。

计划：
- 定位流式事件处理
- 验证工具调用前的文本是否完整保留

\`\`\`ts
const shouldStay = true;
\`\`\`
`;

    expect(toolTracePrefaceFromText(text)).toBe(
      [
        "我会先检查当前实现，然后再修改。",
        "",
        "计划：",
        "- 定位流式事件处理",
        "- 验证工具调用前的文本是否完整保留",
        "",
        "```ts",
        "const shouldStay = true;",
        "```",
      ].join("\n"),
    );
  });

  it("does not collapse to the first sentence or first block", () => {
    const text = "第一句。第二句也要保留。\n\n第二段不能丢。";

    expect(toolTracePrefaceFromText(text)).toBe(
      "第一句。第二句也要保留。\n\n第二段不能丢。",
    );
  });
});
