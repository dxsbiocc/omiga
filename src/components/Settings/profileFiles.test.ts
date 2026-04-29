import { describe, expect, it } from "vitest";
import { summarizeProfileMarkdown } from "./profileFiles";

describe("summarizeProfileMarkdown", () => {
  it("counts meaningful profile lines while ignoring headings and comments", () => {
    const summary = summarizeProfileMarkdown(`# USER\n\n> intro\n\n---\n\n## 沟通偏好\n\n<!-- placeholder -->\n- **语言**：中文\n- 不要过度解释\n`);

    expect(summary.meaningfulLineCount).toBe(2);
    expect(summary.placeholderCount).toBe(1);
    expect(summary.charCount).toBeGreaterThan(0);
  });
});
