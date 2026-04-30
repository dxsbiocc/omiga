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

  it("preserves registry-backed grouped selections", () => {
    const parsed = parseStoredWebSearchSettings(
      JSON.stringify({
        enabledSourcesByCategory: {
          dataset: ["cbioportal", "gtex"],
          knowledge: ["project_wiki", "uniprot", "unknown"],
          social: ["wechat"],
        },
        enabledSubcategoriesByCategory: {
          dataset: ["multi_omics"],
          knowledge: ["protein"],
        },
      }),
    );

    expect(parsed?.enabledSourcesByCategory).toMatchObject({
      dataset: ["cbioportal", "gtex"],
      knowledge: ["project_wiki", "uniprot"],
      social: ["wechat"],
    });
    expect(parsed?.enabledSubcategoriesByCategory).toMatchObject({
      dataset: ["multi_omics"],
      knowledge: ["protein"],
    });
  });

  it("ignores corrupt stored settings", () => {
    expect(parseStoredWebSearchSettings("{")).toBeNull();
  });
});
