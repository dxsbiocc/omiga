import { describe, expect, it } from "vitest";
import { selectLiveReActFoldTraceText } from "./liveFoldTrace";

describe("selectLiveReActFoldTraceText", () => {
  it("keeps visible assistant replies out of the ReAct fold", () => {
    expect(
      selectLiveReActFoldTraceText({
        isStreaming: true,
        activeReactFoldId: "rf-tool-1",
        currentResponse: "数据已到齐，以下是完整分析结果：",
        currentFoldIntermediate: "",
      }),
    ).toBe("");
  });

  it("folds hidden intermediate text while a ReAct fold is active", () => {
    expect(
      selectLiveReActFoldTraceText({
        isStreaming: true,
        activeReactFoldId: "rf-tool-1",
        currentResponse: "",
        currentFoldIntermediate: "Now I should inspect one more file.",
      }),
    ).toBe("Now I should inspect one more file.");
  });

  it("does not mix a visible answer into folded hidden thinking", () => {
    expect(
      selectLiveReActFoldTraceText({
        isStreaming: true,
        activeReactFoldId: "rf-tool-1",
        currentResponse: "最终答案正文",
        currentFoldIntermediate: "hidden thought",
      }),
    ).toBe("hidden thought");
  });
});

