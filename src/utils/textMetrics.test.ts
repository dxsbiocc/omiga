import { describe, expect, it } from "vitest";
import { countTextLines } from "./textMetrics";

describe("text metrics", () => {
  it("counts lines without allocating a split array", () => {
    expect(countTextLines("")).toBe(1);
    expect(countTextLines("one")).toBe(1);
    expect(countTextLines("one\ntwo")).toBe(2);
    expect(countTextLines("one\r\ntwo\n")).toBe(3);
  });
});
