import { describe, expect, it } from "vitest";
import { filterPersistentSessionsBySessionId } from "./persistentSessionScope";

describe("filterPersistentSessionsBySessionId", () => {
  const rows = [
    { session_id: "s1", goal: "a" },
    { session_id: "s2", goal: "b" },
    { session_id: "s1", goal: "c" },
  ];

  it("returns all rows when sessionId is empty", () => {
    expect(filterPersistentSessionsBySessionId(rows, null)).toEqual(rows);
    expect(filterPersistentSessionsBySessionId(rows, undefined)).toEqual(rows);
  });

  it("keeps only the current session rows", () => {
    expect(filterPersistentSessionsBySessionId(rows, "s1")).toEqual([
      { session_id: "s1", goal: "a" },
      { session_id: "s1", goal: "c" },
    ]);
  });
});
