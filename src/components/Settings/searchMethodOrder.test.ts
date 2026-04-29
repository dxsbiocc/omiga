import { describe, expect, it } from "vitest";
import { moveItemToIndex } from "./searchMethodOrder";

describe("moveItemToIndex", () => {
  it("moves an item before the target index", () => {
    expect(moveItemToIndex(["tavily", "google", "ddg"], "ddg", 1)).toEqual([
      "tavily",
      "ddg",
      "google",
    ]);
  });

  it("moves an item after the target index", () => {
    expect(moveItemToIndex(["tavily", "google", "ddg"], "tavily", 2)).toEqual([
      "google",
      "ddg",
      "tavily",
    ]);
  });

  it("clamps out-of-range targets", () => {
    expect(moveItemToIndex(["tavily", "google", "ddg"], "google", 99)).toEqual([
      "tavily",
      "ddg",
      "google",
    ]);
  });
});
