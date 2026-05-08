import { describe, expect, it } from "vitest";
import { extractSuggestionTooltipMarkdown } from "./suggestionTooltip";

describe("extractSuggestionTooltipMarkdown", () => {
  it("returns null when tooltip equals label", () => {
    expect(extractSuggestionTooltipMarkdown("继续推进调度", "继续推进调度")).toBeNull();
  });

  it("strips duplicated heading line and keeps details", () => {
    const text = "### 继续推进调度\n- 补齐执行回执\n- 收敛状态展示";
    expect(extractSuggestionTooltipMarkdown(text, "继续推进调度")).toBe(
      "- 补齐执行回执\n- 收敛状态展示",
    );
  });

  it("returns null when only duplicated heading exists", () => {
    const text = "### 继续推进调度";
    expect(extractSuggestionTooltipMarkdown(text, "继续推进调度")).toBeNull();
  });

  it("keeps content after duplicated title with colon", () => {
    const text = "继续推进调度：补齐执行回执并验证状态";
    expect(extractSuggestionTooltipMarkdown(text, "继续推进调度")).toBe(
      "补齐执行回执并验证状态",
    );
  });

  it("keeps raw text when label does not match heading", () => {
    const text = "核查 reviewer 结论一致性\n并补充证据链接";
    expect(extractSuggestionTooltipMarkdown(text, "继续推进调度")).toBe(text);
  });
});

