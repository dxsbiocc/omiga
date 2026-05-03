import { describe, expect, it } from "vitest";
import {
  RETRIEVAL_PLUGIN_PROTOCOL_DOC_PATH,
  buildPluginDiagnostics,
  flattenMarketplacePlugins,
  type PluginMarketplaceEntry,
  type PluginProcessPoolRouteStatus,
  type PluginRetrievalRouteStatus,
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

describe("buildPluginDiagnostics", () => {
  it("builds a copyable diagnostics payload without credential values", () => {
    const retrievalRoute: PluginRetrievalRouteStatus = {
      pluginId: "retrieval-protocol-example@local",
      category: "dataset",
      sourceId: "example_dataset",
      route: "dataset.example_dataset via retrieval-protocol-example",
      state: "degraded",
      quarantined: false,
      consecutiveFailures: 1,
      remainingMs: 0,
      lastError: "plugin error forced_error: forced fixture error",
    };
    const pooledProcess: PluginProcessPoolRouteStatus = {
      pluginId: "retrieval-protocol-example@local",
      category: "dataset",
      sourceId: "example_dataset",
      route: "dataset.example_dataset via retrieval-protocol-example",
      pluginRoot: "/plugins/retrieval-protocol-example",
      remainingMs: 30_000,
    };

    const diagnostics = JSON.parse(
      buildPluginDiagnostics(
        plugin({
          id: "retrieval-protocol-example@local",
          name: "retrieval-protocol-example",
          marketplaceName: "local",
          sourcePath: "/marketplace/retrieval-protocol-example",
          installedPath: "/plugins/retrieval-protocol-example",
          installed: true,
          enabled: true,
        }),
        [retrievalRoute],
        [pooledProcess],
      ),
    );

    expect(diagnostics.protocolDocPath).toBe(
      RETRIEVAL_PLUGIN_PROTOCOL_DOC_PATH,
    );
    expect(diagnostics.plugin).toMatchObject({
      id: "retrieval-protocol-example@local",
      installed: true,
      enabled: true,
    });
    expect(diagnostics.retrievalRoutes).toEqual([retrievalRoute]);
    expect(diagnostics.pooledProcesses).toEqual([pooledProcess]);
    expect(JSON.stringify(diagnostics)).not.toContain("secret");
    expect(diagnostics.notes.join(" ")).toContain("No credential values");
  });

  it("includes declared retrieval source summaries without process internals", () => {
    const diagnostics = JSON.parse(
      buildPluginDiagnostics(
        plugin({
          id: "public-dataset-sources@omiga-curated",
          name: "public-dataset-sources",
          retrieval: {
            protocolVersion: 1,
            sources: [
              {
                id: "biosample",
                category: "dataset",
                label: "NCBI BioSample",
                description: "Sample metadata",
                subcategories: ["sample_metadata"],
                capabilities: ["search", "query", "fetch"],
                requiredCredentialRefs: [],
                optionalCredentialRefs: ["pubmed_email"],
                defaultEnabled: false,
                replacesBuiltin: true,
              },
            ],
          },
        }),
      ),
    );

    expect(diagnostics.plugin.retrieval.sources[0]).toMatchObject({
      id: "biosample",
      category: "dataset",
      label: "NCBI BioSample",
      replacesBuiltin: true,
    });
    expect(JSON.stringify(diagnostics)).not.toContain("runtime");
    expect(JSON.stringify(diagnostics)).not.toContain("command");
  });
});
