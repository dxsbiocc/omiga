import { describe, expect, it } from "vitest";
import {
  defaultWebSearchQuerySettings,
  parseStoredWebSearchSettings,
} from "./webSearchSettings";

describe("web search settings rehydration", () => {
  it("preserves stored query source selections, including empty selections", () => {
    const parsed = parseStoredWebSearchSettings(
      JSON.stringify({
        queryDatasetTypes: [],
        queryDatasetSources: ["cbioportal", "geo", "../escape"],
        queryKnowledgeSources: [],
      }),
    );

    expect(parsed).toMatchObject({
      queryDatasetTypes: [],
      queryDatasetSources: ["geo", "cbioportal"],
      queryKnowledgeSources: [],
    });
  });

  it("falls back to product defaults when stored query selections are absent", () => {
    expect(parseStoredWebSearchSettings("{}")).toMatchObject(
      defaultWebSearchQuerySettings(),
    );
  });

  it("ignores corrupt stored settings", () => {
    expect(parseStoredWebSearchSettings("{")).toBeNull();
  });
});
