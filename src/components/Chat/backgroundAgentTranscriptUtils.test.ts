import { describe, expect, it } from "vitest";
import {
  buildOpaqueSidechainFallback,
  isOpaqueObjectText,
  normalizeSidechainValue,
} from "./backgroundAgentTranscriptUtils";

describe("backgroundAgentTranscriptUtils", () => {
  it("detects opaque object strings", () => {
    expect(isOpaqueObjectText("[object Object]")).toBe(true);
    expect(isOpaqueObjectText("  [object Object]  ")).toBe(true);
    expect(isOpaqueObjectText("{\"a\":1}")).toBe(false);
  });

  it("pretty prints JSON strings instead of showing raw escaped payloads", () => {
    expect(normalizeSidechainValue('{"a":1,"b":"x"}')).toBe('{\n  "a": 1,\n  "b": "x"\n}');
  });

  it("replaces [object Object] with task context", () => {
    const fallback = buildOpaqueSidechainFallback({
      kind: "message",
      task: {
        description: "分析设计方案",
        status: "Failed",
        error_message: "模型返回异常",
      },
    });

    const normalized = normalizeSidechainValue("[object Object]", fallback);

    expect(normalized).not.toContain("[object Object]");
    expect(normalized).toContain("分析设计方案");
    expect(normalized).toContain("模型返回异常");
  });
});
