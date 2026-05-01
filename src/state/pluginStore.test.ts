import { describe, expect, it } from "vitest";
import {
  flattenMarketplacePlugins,
  type PluginMarketplaceEntry,
  type PluginSummary,
} from "./pluginStore";

function plugin(overrides: Partial<PluginSummary> = {}): PluginSummary {
  return {
    id: "notebook-helper@omiga-curated",
    name: "notebook-helper",
    marketplaceName: "omiga-curated",
    marketplacePath: "/marketplace.json",
    sourcePath: "/plugins/notebook-helper",
    installedPath: null,
    installed: false,
    enabled: false,
    installPolicy: "AVAILABLE",
    authPolicy: "ON_INSTALL",
    interface: null,
    ...overrides,
  };
}

function marketplace(
  path: string,
  plugins: PluginSummary[],
): PluginMarketplaceEntry {
  return {
    name: "omiga-curated",
    path,
    interface: null,
    plugins,
  };
}

describe("flattenMarketplacePlugins", () => {
  it("keeps the first plugin when duplicate marketplaces expose the same plugin id", () => {
    const first = plugin({ marketplacePath: "/dev/marketplace.json" });
    const duplicate = plugin({ marketplacePath: "/resource/marketplace.json" });
    const other = plugin({
      id: "other@omiga-curated",
      name: "other",
      marketplacePath: "/resource/marketplace.json",
    });

    const flattened = flattenMarketplacePlugins([
      marketplace("/dev/marketplace.json", [first]),
      marketplace("/resource/marketplace.json", [duplicate, other]),
    ]);

    expect(flattened).toEqual([first, other]);
  });
});
