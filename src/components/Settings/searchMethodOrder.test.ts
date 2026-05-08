import { describe, expect, it } from "vitest";
import {
  DEFAULT_WEB_SEARCH_METHODS,
  moveItemToIndex,
  normalizeWebSearchMethods,
  primaryPublicSearchEngine,
} from "./searchMethodOrder";

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

describe("normalizeWebSearchMethods", () => {
  it("defaults to public no-key providers instead of every provider", () => {
    expect(normalizeWebSearchMethods(undefined)).toEqual([
      "ddg",
      "google",
      "bing",
    ]);
    expect(DEFAULT_WEB_SEARCH_METHODS).toEqual(["ddg", "google", "bing"]);
  });

  it("migrates the legacy all-provider default to the new public default", () => {
    expect(
      normalizeWebSearchMethods([
        "tavily",
        "exa",
        "firecrawl",
        "parallel",
        "google",
        "bing",
        "ddg",
      ]),
    ).toEqual(["ddg", "google", "bing"]);
  });

  it("preserves explicit user selections and removes duplicates", () => {
    expect(normalizeWebSearchMethods(["tavily", "ddg", "ddg"])).toEqual([
      "tavily",
      "ddg",
    ]);
  });
});

describe("primaryPublicSearchEngine", () => {
  it("uses the first enabled public engine", () => {
    expect(primaryPublicSearchEngine(["tavily", "google", "ddg"])).toBe(
      "google",
    );
  });
});
