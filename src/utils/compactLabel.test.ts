import { describe, expect, it } from "vitest";
import { compactLabel, isLabelCompacted } from "./compactLabel";

describe("compactLabel", () => {
  it("keeps short labels untouched", () => {
    expect(compactLabel("verification", 20)).toBe("verification");
  });

  it("trims and normalizes whitespace", () => {
    expect(compactLabel("  deep   research  ", 20)).toBe("deep research");
  });

  it("adds ellipsis when exceeding max chars", () => {
    expect(compactLabel("this is a very long worker label", 10)).toBe("this is a…");
  });
});

describe("isLabelCompacted", () => {
  it("returns false for same normalized text", () => {
    const compacted = compactLabel("  worker   alpha ", 20);
    expect(isLabelCompacted("  worker   alpha ", compacted)).toBe(false);
  });

  it("returns true when text was shortened", () => {
    const compacted = compactLabel("worker label too long for chip", 12);
    expect(isLabelCompacted("worker label too long for chip", compacted)).toBe(true);
  });
});

