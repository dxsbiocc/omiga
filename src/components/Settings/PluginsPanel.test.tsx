import { describe, expect, it } from "vitest";
import {
  filterPluginsForCatalog,
  pluginCardSubtitle,
  pluginRuntimeSummary,
  processPoolStatusDiagnostic,
  retrievalStatusDiagnostic,
  unknownRetrievalRuntimePluginIds,
} from "./PluginsPanel";
import type {
  PluginProcessPoolRouteStatus,
  PluginRetrievalRouteStatus,
  PluginSummary,
} from "../../state/pluginStore";

function routeStatus(
  overrides: Partial<PluginRetrievalRouteStatus> = {},
): PluginRetrievalRouteStatus {
  return {
    pluginId: "retrieval-dataset-geo@omiga-curated",
    category: "dataset",
    sourceId: "geo",
    route: "dataset.geo via retrieval-dataset-geo@omiga-curated",
    state: "healthy",
    quarantined: false,
    consecutiveFailures: 0,
    remainingMs: 0,
    lastError: null,
    ...overrides,
  };
}

function pluginSummary(overrides: Partial<PluginSummary> = {}): PluginSummary {
  return {
    id: "retrieval-dataset-geo@omiga-curated",
    name: "retrieval-dataset-geo",
    marketplaceName: "omiga-curated",
    marketplacePath: "/marketplace.json",
    sourcePath: "/plugins/retrieval-dataset-geo",
    installedPath: null,
    installed: false,
    enabled: false,
    installPolicy: "AVAILABLE",
    authPolicy: "ON_USE",
    interface: null,
    ...overrides,
  };
}

describe("PluginsPanel diagnostics helpers", () => {
  it("filters plugin cards by search text and catalog state", () => {
    const geo = pluginSummary({
      installed: true,
      enabled: true,
      retrieval: {
        protocolVersion: 1,
        sources: [
          {
            id: "geo",
            category: "dataset",
            label: "NCBI GEO",
            description: "Gene expression datasets",
            subcategories: ["expression"],
            capabilities: ["search", "query", "fetch"],
            requiredCredentialRefs: [],
            optionalCredentialRefs: [],
            defaultEnabled: true,
            replacesBuiltin: true,
          },
        ],
      },
    });
    const notebook = pluginSummary({
      id: "notebook-helper@omiga-curated",
      name: "notebook-helper",
      installed: false,
      enabled: false,
      interface: {
        displayName: "Notebook Helper",
        shortDescription: "Create and repair notebooks",
        longDescription: null,
        developerName: null,
        category: "notebook",
        capabilities: ["repair"],
        websiteUrl: null,
        privacyPolicyUrl: null,
        termsOfServiceUrl: null,
        defaultPrompt: [],
        brandColor: null,
        composerIcon: null,
        logo: null,
        screenshots: [],
      },
    });
    const plugins = [geo, notebook];

    expect(filterPluginsForCatalog(plugins, "geo expression", "all")).toEqual([
      geo,
    ]);
    expect(filterPluginsForCatalog(plugins, "", "data-sources")).toEqual([
      geo,
    ]);
    expect(filterPluginsForCatalog(plugins, "", "general")).toEqual([
      notebook,
    ]);
    expect(filterPluginsForCatalog(plugins, "", "enabled")).toEqual([geo]);
    expect(filterPluginsForCatalog(plugins, "repair", "available")).toEqual([
      notebook,
    ]);
  });

  it("detects stale runtime records whose plugin id is no longer catalogued", () => {
    const known = pluginSummary({ id: "retrieval-dataset-geo@omiga-curated" });
    const staleRoute = routeStatus({
      pluginId: "stale-pubmed@legacy-mcp",
      category: "literature",
      sourceId: "pubmed",
      route: "literature.pubmed via stale-pubmed@legacy-mcp",
    });
    const stalePool: PluginProcessPoolRouteStatus = {
      pluginId: "stale-web-search@legacy",
      category: "web",
      sourceId: "search",
      route: "web.search via stale-web-search@legacy",
      pluginRoot: "/old/plugins/stale-web-search",
      remainingMs: 15_000,
    };

    expect(
      unknownRetrievalRuntimePluginIds([known], [routeStatus(), staleRoute], [stalePool]),
    ).toEqual(["stale-pubmed@legacy-mcp", "stale-web-search@legacy"]);
  });

  it("uses retrieval source labels as card subtitles instead of verbose local-plugin copy", () => {
    const subtitle = pluginCardSubtitle(
      pluginSummary({
        interface: {
          displayName: "GEO Retrieval Source",
          shortDescription: "NCBI GEO as a local retrieval plugin",
          longDescription: null,
          developerName: null,
          category: null,
          capabilities: [],
          websiteUrl: null,
          privacyPolicyUrl: null,
          termsOfServiceUrl: null,
          defaultPrompt: [],
          brandColor: null,
          composerIcon: null,
          logo: null,
          screenshots: [],
        },
        retrieval: {
          protocolVersion: 1,
          sources: [
            {
              id: "geo",
              category: "dataset",
              label: "NCBI GEO",
              description: "NCBI GEO as a local retrieval plugin",
              subcategories: [],
              capabilities: ["search", "query", "fetch"],
              requiredCredentialRefs: [],
              optionalCredentialRefs: [],
              defaultEnabled: true,
              replacesBuiltin: true,
            },
          ],
        },
      }),
    );

    expect(subtitle).toBe("NCBI GEO");
  });

  it("summarizes quarantined routes with actionable timing and last error", () => {
    const diagnostic = retrievalStatusDiagnostic(
      routeStatus({
        state: "quarantined",
        quarantined: true,
        consecutiveFailures: 3,
        remainingMs: 30_000,
        lastError: "plugin error upstream_failed: forced fixture error",
      }),
    );

    expect(diagnostic.title).toBe(
      "dataset.geo via retrieval-dataset-geo@omiga-curated",
    );
    expect(diagnostic.detail).toContain("Quarantined for 30s");
    expect(diagnostic.detail).toContain("3 consecutive failures");
    expect(diagnostic.lastError).toContain("forced fixture error");
  });

  it("summarizes healthy and pooled process diagnostics compactly", () => {
    expect(retrievalStatusDiagnostic(routeStatus()).detail).toContain(
      "Healthy",
    );

    const pooled: PluginProcessPoolRouteStatus = {
      pluginId: "retrieval-dataset-geo@omiga-curated",
      category: "dataset",
      sourceId: "geo",
      route: "dataset.geo via retrieval-dataset-geo@omiga-curated",
      pluginRoot: "/plugins/retrieval-dataset-geo",
      remainingMs: 90_000,
    };

    const diagnostic = processPoolStatusDiagnostic(pooled);
    expect(diagnostic.title).toBe(
      "dataset.geo via retrieval-dataset-geo@omiga-curated",
    );
    expect(diagnostic.detail).toContain("2m");
    expect(diagnostic.pluginRoot).toBe("/plugins/retrieval-dataset-geo");
  });

  it("summarizes plugin runtime health for detail cards", () => {
    const plugin = pluginSummary({
      installed: true,
      enabled: true,
      retrieval: {
        protocolVersion: 1,
        sources: [
          {
            id: "geo",
            category: "dataset",
            label: "NCBI GEO",
            description: "Gene expression datasets",
            subcategories: [],
            capabilities: ["search", "query", "fetch"],
            requiredCredentialRefs: [],
            optionalCredentialRefs: [],
            defaultEnabled: true,
            replacesBuiltin: true,
          },
        ],
      },
    });

    expect(pluginRuntimeSummary(plugin, [], []).label).toBe("No calls yet");

    const degraded = pluginRuntimeSummary(
      plugin,
      [
        routeStatus({
          state: "degraded",
          consecutiveFailures: 1,
          lastError: "plugin exited before response",
        }),
      ],
      [],
    );

    expect(degraded.label).toBe("Needs attention");
    expect(degraded.issueCount).toBe(1);
    expect(degraded.lastError).toBe("plugin exited before response");
  });
});
