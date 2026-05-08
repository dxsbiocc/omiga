import { describe, expect, it } from "vitest";
import { stringifyUnknown } from "./stringifyUnknown";

describe("stringifyUnknown", () => {
  it("keeps plain strings", () => {
    expect(stringifyUnknown("hello")).toBe("hello");
  });

  it("serializes objects to JSON", () => {
    expect(stringifyUnknown({ a: 1, b: "x" })).toContain('"a": 1');
  });

  it("handles circular objects", () => {
    const obj: { self?: unknown; name: string } = { name: "x" };
    obj.self = obj;
    const out = stringifyUnknown(obj);
    expect(out).toContain('"self": "[Circular]"');
  });
});

