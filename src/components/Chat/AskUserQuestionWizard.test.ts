import { describe, expect, it } from "vitest";
import {
  askUserOptionShouldShowRecommendedChip,
  type AskUserQuestionItem,
} from "./AskUserQuestionWizard";

const baseQuestion: AskUserQuestionItem = {
  header: "Mode",
  question: "请选择执行方式？",
  options: [
    { label: "快速处理", description: "优先速度。" },
    { label: "严格验证", description: "优先验证。" },
  ],
};

describe("AskUserQuestionWizard option recommendations", () => {
  it("marks the first single-select option as recommended when the model omits a recommendation", () => {
    expect(
      askUserOptionShouldShowRecommendedChip(
        baseQuestion,
        baseQuestion.options[0],
        0,
      ),
    ).toBe(true);
    expect(
      askUserOptionShouldShowRecommendedChip(
        baseQuestion,
        baseQuestion.options[1],
        1,
      ),
    ).toBe(false);
  });

  it("honors explicit recommended options without duplicating label-based markers", () => {
    const explicit: AskUserQuestionItem = {
      ...baseQuestion,
      options: [
        { label: "快速处理", description: "优先速度。" },
        { label: "严格验证", description: "优先验证。", recommended: true },
        { label: "手动选择（推荐）", description: "标签里已有推荐标记。" },
      ],
    };

    expect(
      askUserOptionShouldShowRecommendedChip(explicit, explicit.options[0], 0),
    ).toBe(false);
    expect(
      askUserOptionShouldShowRecommendedChip(explicit, explicit.options[1], 1),
    ).toBe(true);
    expect(
      askUserOptionShouldShowRecommendedChip(explicit, explicit.options[2], 2),
    ).toBe(false);
  });

  it("does not invent a default recommendation for multi-select questions", () => {
    const multi: AskUserQuestionItem = { ...baseQuestion, multiSelect: true };

    expect(
      askUserOptionShouldShowRecommendedChip(multi, multi.options[0], 0),
    ).toBe(false);
  });
});
