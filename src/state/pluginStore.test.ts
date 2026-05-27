import { beforeEach, describe, expect, it, vi } from "vitest";

const invokeMock = vi.hoisted(() => vi.fn());
const listenTauriEventMock = vi.hoisted(() => vi.fn());

vi.mock("@tauri-apps/api/core", () => ({
  invoke: invokeMock,
}));

vi.mock("../utils/tauriEvents", () => ({
  listenTauriEvent: listenTauriEventMock,
}));

import {
  RETRIEVAL_PLUGIN_PROTOCOL_DOC_PATH,
  buildPluginDiagnostics,
  buildRetrievalRuntimeDiagnostics,
  flattenMarketplacePlugins,
  summarizeOperatorRunResult,
  updatePluginEnabledInMarketplaces,
  updatePluginInstalledInMarketplaces,
  updateRetrievalResourceEnabledInMarketplaces,
  usePluginStore,
  type BuiltinMarketplaceStatus,
  type OperatorSummary,
  type MarketplaceSourceView,
  type PluginMarketplaceEntry,
  type PluginProcessPoolRouteStatus,
  type PluginRetrievalRouteStatus,
  type PluginSummary,
  type RefreshResult,
  type UserMarketplaceSource,
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

function marketplaceSource(
  overrides: Partial<UserMarketplaceSource> = {},
): UserMarketplaceSource {
  return {
    id: "source-1",
    kind: "remote",
    location: "https://github.com/omiga-dev/omiga-plugins.git",
    label: "Curated",
    enabled: true,
    addedAt: "2026-05-27T00:00:00Z",
    ...overrides,
  };
}

function marketplaceSourceView(
  overrides: Partial<MarketplaceSourceView> = {},
): MarketplaceSourceView {
  return {
    id: "builtin",
    kind: "builtin",
    location: "/workspace/omiga-plugins",
    label: "Built-in Marketplace",
    enabled: true,
    removable: false,
    addedAt: null,
    ...overrides,
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

describe("local plugin catalog updates", () => {
  it("updates one plugin enabled flag without rebuilding unrelated marketplace entries", () => {
    const target = plugin({
      id: "retrieval-dataset-geo@omiga-curated",
      name: "retrieval-dataset-geo",
      installed: true,
      enabled: false,
    });
    const other = plugin({
      id: "notebook-helper@omiga-curated",
      name: "notebook-helper",
      installed: true,
      enabled: true,
    });
    const original = [marketplace("/marketplace.json", [target, other])];

    const updated = updatePluginEnabledInMarketplaces(
      original,
      "retrieval-dataset-geo@omiga-curated",
      true,
    );

    expect(updated).not.toBe(original);
    expect(updated[0]).not.toBe(original[0]);
    expect(updated[0].plugins[0]).toMatchObject({ enabled: true });
    expect(updated[0].plugins[1]).toBe(other);
  });

  it("marks one installed plugin without rebuilding unrelated marketplace entries", () => {
    const target = plugin({
      id: "operator-pca-r@omiga-curated",
      name: "operator-pca-r",
      installed: false,
      enabled: false,
    });
    const other = plugin({
      id: "ngs-sequence-processing@omiga-curated",
      name: "ngs-sequence-processing",
      installed: false,
      enabled: false,
    });
    const original = [marketplace("/marketplace.json", [target, other])];

    const updated = updatePluginInstalledInMarketplaces(
      original,
      "operator-pca-r@omiga-curated",
      {
        pluginId: "operator-pca-r@omiga-curated",
        installedPath: "/plugins/operator-pca-r",
        authPolicy: "ON_USE",
      },
    );

    expect(updated).not.toBe(original);
    expect(updated[0]).not.toBe(original[0]);
    expect(updated[0].plugins[0]).toMatchObject({
      installed: true,
      enabled: true,
      installedPath: "/plugins/operator-pca-r",
      authPolicy: "ON_USE",
    });
    expect(updated[0].plugins[1]).toBe(other);
  });

  it("updates one retrieval route exposure without rebuilding unrelated plugins", () => {
    const target = plugin({
      id: "resource-ncbi@omiga-curated",
      name: "resource-ncbi",
      installed: true,
      enabled: true,
      retrieval: {
        protocolVersion: 1,
        resources: [
          {
            id: "geo",
            category: "dataset",
            label: "NCBI GEO",
            description: "GEO datasets",
            subcategories: [],
            capabilities: ["search", "query", "fetch"],
            requiredCredentialRefs: [],
            optionalCredentialRefs: [],
            defaultEnabled: true,
            replacesBuiltin: true,
            exposed: true,
          },
          {
            id: "pubmed",
            category: "literature",
            label: "PubMed",
            description: "PubMed abstracts",
            subcategories: [],
            capabilities: ["search", "query", "fetch"],
            requiredCredentialRefs: [],
            optionalCredentialRefs: [],
            defaultEnabled: true,
            replacesBuiltin: true,
            exposed: true,
          },
        ],
      },
    });
    const other = plugin({
      id: "resource-embl-ebi@omiga-curated",
      name: "resource-embl-ebi",
      retrieval: { protocolVersion: 1, resources: [] },
    });
    const original = [marketplace("/marketplace.json", [target, other])];

    const updated = updateRetrievalResourceEnabledInMarketplaces(
      original,
      "resource-ncbi@omiga-curated",
      "dataset",
      "geo",
      false,
    );

    expect(updated).not.toBe(original);
    expect(updated[0]).not.toBe(original[0]);
    expect(updated[0].plugins[0].retrieval?.resources[0]).toMatchObject({
      id: "geo",
      exposed: false,
    });
    expect(updated[0].plugins[0].retrieval?.resources[1]).toBe(
      target.retrieval?.resources[1],
    );
    expect(updated[0].plugins[1]).toBe(other);
  });
});

describe("summarizeOperatorRunResult", () => {
  it("returns null when operator run result is missing required fields", () => {
    expect(summarizeOperatorRunResult({})).toBeNull();
    expect(
      summarizeOperatorRunResult({
        status: "succeeded",
        runId: "oprun_missing_dir",
      }),
    ).toBeNull();
    expect(
      summarizeOperatorRunResult({
        status: "succeeded",
        runDir: "/project/.omiga/runs/oprun_missing_id",
      }),
    ).toBeNull();
  });

  it("preserves smoke run context for operator statistics", () => {
    expect(
      summarizeOperatorRunResult({
        status: "succeeded",
        runId: "oprun_20260506_smoke",
        location: "local",
        operator: {
          alias: "write_text_report",
          id: "write_text_report",
          version: "0.1.0",
          sourcePlugin: "operator-smoke@omiga-curated",
        },
        runDir: "/project/.omiga/runs/oprun_20260506_smoke",
        exportDir: "/project/operator-results/write_text_report/oprun_20260506_smoke",
        runContext: {
          kind: "smoke",
          smokeTestId: "default",
          smokeTestName: "Write text report smoke",
        },
        error: {
          kind: "tool_exit_nonzero",
          retryable: false,
          message: "bad input",
          suggestedAction: "Inspect stderr.",
          stderrTail: "bad flag\n",
        },
        outputs: {
          report: [{ path: "/project/.omiga/runs/oprun_20260506_smoke/out/report.txt" }],
        },
        structuredOutputs: {
          summary: { lineCount: 2 },
          ok: true,
        },
        cache: {
          key: "sha256:cache-key",
          hit: true,
          sourceRunId: "oprun_20260506_source",
          sourceRunDir: "/project/.omiga/runs/oprun_20260506_source",
        },
      }),
    ).toMatchObject({
      runId: "oprun_20260506_smoke",
      runKind: "smoke",
      kind: "smoke",
      smokeTestId: "default",
      smokeTestName: "Write text report smoke",
      errorKind: "tool_exit_nonzero",
      retryable: false,
      suggestedAction: "Inspect stderr.",
      stderrTail: "bad flag\n",
      exportDir: "/project/operator-results/write_text_report/oprun_20260506_smoke",
      outputCount: 1,
      structuredOutputCount: 2,
      cacheKey: "sha256:cache-key",
      cacheHit: true,
      cacheSourceRunId: "oprun_20260506_source",
      cacheSourceRunDir: "/project/.omiga/runs/oprun_20260506_source",
    });
  });

  it("preserves chain parent execution ids for timeline grouping", () => {
    expect(
      summarizeOperatorRunResult({
        status: "succeeded",
        runId: "oprun_20260506_chain_step",
        runDir: "/project/.omiga/runs/oprun_20260506_chain_step",
        runContext: {
          kind: "chain",
          parentExecutionId: "execrec_chain_parent",
        },
      }),
    ).toMatchObject({
      runId: "oprun_20260506_chain_step",
      runKind: "chain",
      kind: "chain",
      parentExecutionId: "execrec_chain_parent",
    });
  });
});

describe("usePluginStore operator actions", () => {
  beforeEach(() => {
    usePluginStore.getState().cleanupOperatorTaskListeners();
    invokeMock.mockReset();
    listenTauriEventMock.mockReset();
    listenTauriEventMock.mockResolvedValue(vi.fn());
    usePluginStore.setState({
      marketplaceSources: [],
      marketplaceSourceViews: [],
      marketplaces: [],
      operators: [],
      operatorDiagnostics: [],
      operatorRegistryPath: null,
      operatorRuns: [],
      activeOperatorTasks: {},
      activeOperatorTaskStartedAt: {},
      activeOperatorTaskStatus: {},
      retrievalStatuses: [],
      processPoolStatuses: [],
      remoteMarketplaceChecks: [],
      builtinMarketplaceStatus: null,
      isLoading: false,
      isMutating: false,
      bootstrapInProgress: false,
      error: null,
    });
  });

  it("installs plugins with a local catalog update instead of a full marketplace reload", async () => {
    const target = plugin({
      id: "operator-pca-r@omiga-curated",
      name: "operator-pca-r",
      installed: false,
      enabled: false,
    });
    const other = plugin({
      id: "ngs-sequence-processing@omiga-curated",
      name: "ngs-sequence-processing",
      installed: false,
      enabled: false,
    });
    usePluginStore.setState({
      marketplaces: [marketplace("/marketplace.json", [target, other])],
    });
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "install_omiga_plugin") {
        return {
          pluginId: "operator-pca-r@omiga-curated",
          installedPath: "/plugins/operator-pca-r",
          authPolicy: "ON_USE",
        };
      }
      if (command === "list_omiga_plugin_retrieval_statuses") return [];
      if (command === "list_omiga_plugin_process_pool_statuses") return [];
      if (command === "list_operators") {
        return {
          registryPath: "/registry.json",
          operators: [],
          diagnostics: [],
        };
      }
      throw new Error(`unexpected command ${command}`);
    });

    await usePluginStore.getState().installPlugin(target, "/project");

    expect(invokeMock).not.toHaveBeenCalledWith(
      "list_omiga_plugin_marketplaces",
      expect.anything(),
    );
    expect(invokeMock).not.toHaveBeenCalledWith(
      "list_omiga_plugin_retrieval_statuses",
      expect.anything(),
    );
    expect(invokeMock).not.toHaveBeenCalledWith(
      "list_omiga_plugin_process_pool_statuses",
      expect.anything(),
    );
    expect(invokeMock).toHaveBeenCalledWith("list_operators");
    expect(usePluginStore.getState().marketplaces[0].plugins[0]).toMatchObject({
      installed: true,
      enabled: true,
      installedPath: "/plugins/operator-pca-r",
    });
    expect(usePluginStore.getState().marketplaces[0].plugins[1]).toBe(other);
    expect(usePluginStore.getState().isMutating).toBe(false);
  });

  it("refreshes dynamic operator exposure when toggling a plugin", async () => {
    const target = plugin({
      id: "operator-pca-r@omiga-curated",
      name: "operator-pca-r",
      installed: true,
      enabled: false,
    });
    const exposedOperator: OperatorSummary = {
      id: "pca_matrix",
      version: "0.1.0",
      name: "PCA Matrix",
      description: null,
      sourcePlugin: "operator-pca-r@omiga-curated",
      manifestPath: "/plugins/operator-pca-r/operators/pca-matrix/operator.yaml",
      smokeTests: [],
      enabledAliases: ["pca_matrix"],
      exposed: true,
      unavailableReason: null,
    };
    usePluginStore.setState({
      marketplaces: [marketplace("/marketplace.json", [target])],
    });
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "set_omiga_plugin_enabled") return undefined;
      if (command === "list_omiga_plugin_retrieval_statuses") return [];
      if (command === "list_operators") {
        return {
          registryPath: "/registry.json",
          operators: [exposedOperator],
          diagnostics: [],
        };
      }
      throw new Error(`unexpected command ${command}`);
    });

    await usePluginStore.getState().setPluginEnabled(target.id, true, "/project");

    expect(invokeMock).toHaveBeenCalledWith("set_omiga_plugin_enabled", {
      pluginId: target.id,
      enabled: true,
      projectRoot: "/project",
    });
    expect(invokeMock).toHaveBeenCalledWith("list_operators");
    expect(usePluginStore.getState().operators).toEqual([exposedOperator]);
    expect(usePluginStore.getState().marketplaces[0].plugins[0]).toMatchObject({
      enabled: true,
    });
  });

  it("passes force overwrite when syncing a plugin with local edits", async () => {
    const target = plugin({
      id: "ngs-alignment@omiga-curated",
      name: "ngs-alignment",
      installed: true,
      enabled: true,
      sync: {
        state: "conflictRisk",
        label: "Review sync",
        message: "conflict",
        changedCount: 1,
        localModifiedCount: 1,
        conflictCount: 1,
      },
    });
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "sync_omiga_plugin") {
        return {
          pluginId: target.id,
          status: "forceSynced",
          installedPath: "/plugins/ngs-alignment",
          updated: ["plugin.json"],
          added: [],
          removed: [],
          keptLocal: [],
          conflicts: [],
          message: "forced",
        };
      }
      if (command === "ensure_builtin_marketplace_source") {
        return {
          ok: false,
          source: "github",
          path: null,
          message: "Offline.",
        } satisfies BuiltinMarketplaceStatus;
      }
      if (command === "list_omiga_plugin_marketplaces") {
        return [marketplace("/marketplace.json", [target])];
      }
      if (command === "list_omiga_plugin_marketplace_sources") return [];
      if (command === "list_omiga_plugin_marketplace_source_views") return [];
      if (command === "list_omiga_plugin_retrieval_statuses") return [];
      if (command === "list_omiga_plugin_process_pool_statuses") return [];
      if (command === "list_operators") {
        return { registryPath: "/registry.json", operators: [], diagnostics: [] };
      }
      if (command === "list_operator_runs") return [];
      throw new Error(`unexpected command ${command}`);
    });

    await usePluginStore.getState().syncPlugin(target, "/project", { force: true });

    expect(invokeMock).toHaveBeenCalledWith("sync_omiga_plugin", {
      pluginId: target.id,
      marketplacePath: target.marketplacePath,
      pluginName: target.name,
      force: true,
      projectRoot: "/project",
    });
    expect(usePluginStore.getState().isMutating).toBe(false);
  });

  it("checks remote marketplaces without reloading the local catalog", async () => {
    invokeMock.mockResolvedValueOnce([
      {
        name: "omiga-curated",
        path: "/marketplace.json",
        remote: { url: "https://raw.githubusercontent.com/org/repo/main/marketplace.json" },
        state: "updateAvailable",
        label: "Remote update available",
        message: "Remote marketplace differs.",
        localDigest: "sha256:local",
        remoteDigest: "sha256:remote",
        remotePluginCount: 1,
        changedPlugins: ["ngs-alignment"],
        checkedAt: "2026-05-12T00:00:00Z",
      },
    ]);

    const result = await usePluginStore.getState().checkRemoteMarketplaces("/project");

    expect(result[0].state).toBe("updateAvailable");
    expect(invokeMock).toHaveBeenCalledWith("check_omiga_remote_plugin_marketplaces", {
      projectRoot: "/project",
    });
    expect(invokeMock).not.toHaveBeenCalledWith(
      "list_omiga_plugin_marketplaces",
      expect.anything(),
    );
    expect(usePluginStore.getState().remoteMarketplaceChecks).toEqual(result);
    expect(usePluginStore.getState().isMutating).toBe(false);
  });

  it("adds marketplace sources and reloads source definitions plus the plugin catalog", async () => {
    const source = marketplaceSource();
    const sourceViews = [
      marketplaceSourceView(),
      marketplaceSourceView({
        id: source.id,
        kind: source.kind,
        location: source.location,
        label: source.label,
        enabled: source.enabled,
        removable: true,
        addedAt: source.addedAt,
      }),
    ];
    const catalog = [marketplace("/marketplace.json", [plugin()])];
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "add_omiga_plugin_marketplace_source") return source;
      if (command === "list_omiga_plugin_marketplace_sources") return [source];
      if (command === "list_omiga_plugin_marketplace_source_views") return sourceViews;
      if (command === "list_omiga_plugin_marketplaces") return catalog;
      throw new Error(`unexpected command ${command}`);
    });

    const result = await usePluginStore
      .getState()
      .addMarketplaceSource("remote", source.location, source.label, "/project");

    expect(result).toEqual(source);
    expect(invokeMock).toHaveBeenCalledWith(
      "add_omiga_plugin_marketplace_source",
      {
        kind: "remote",
        location: source.location,
        label: source.label,
      },
    );
    expect(invokeMock).toHaveBeenCalledWith(
      "list_omiga_plugin_marketplace_sources",
    );
    expect(invokeMock).toHaveBeenCalledWith(
      "list_omiga_plugin_marketplace_source_views",
    );
    expect(invokeMock).toHaveBeenCalledWith("list_omiga_plugin_marketplaces", {
      projectRoot: "/project",
    });
    expect(usePluginStore.getState().marketplaceSources).toEqual([source]);
    expect(usePluginStore.getState().marketplaceSourceViews).toEqual(sourceViews);
    expect(usePluginStore.getState().marketplaces).toEqual(catalog);
    expect(usePluginStore.getState().isMutating).toBe(false);
  });

  it("removes marketplace sources and reloads source definitions plus the plugin catalog", async () => {
    const remaining = marketplaceSource({ id: "source-2", location: "/plugins/local" });
    const sourceViews = [
      marketplaceSourceView(),
      marketplaceSourceView({
        id: remaining.id,
        kind: remaining.kind,
        location: remaining.location,
        label: remaining.label,
        enabled: remaining.enabled,
        removable: true,
        addedAt: remaining.addedAt,
      }),
    ];
    const catalog = [marketplace("/local/marketplace.json", [])];
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "remove_omiga_plugin_marketplace_source") return undefined;
      if (command === "list_omiga_plugin_marketplace_sources") return [remaining];
      if (command === "list_omiga_plugin_marketplace_source_views") return sourceViews;
      if (command === "list_omiga_plugin_marketplaces") return catalog;
      throw new Error(`unexpected command ${command}`);
    });

    await usePluginStore.getState().removeMarketplaceSource("source-1", "/project");

    expect(invokeMock).toHaveBeenCalledWith(
      "remove_omiga_plugin_marketplace_source",
      { id: "source-1" },
    );
    expect(invokeMock).toHaveBeenCalledWith(
      "list_omiga_plugin_marketplace_sources",
    );
    expect(invokeMock).toHaveBeenCalledWith(
      "list_omiga_plugin_marketplace_source_views",
    );
    expect(invokeMock).toHaveBeenCalledWith("list_omiga_plugin_marketplaces", {
      projectRoot: "/project",
    });
    expect(usePluginStore.getState().marketplaceSources).toEqual([remaining]);
    expect(usePluginStore.getState().marketplaceSourceViews).toEqual(sourceViews);
    expect(usePluginStore.getState().marketplaces).toEqual(catalog);
    expect(usePluginStore.getState().isMutating).toBe(false);
  });

  it("toggles marketplace sources and reloads source definitions plus the plugin catalog", async () => {
    const source = marketplaceSource({ enabled: false });
    const sourceViews = [
      marketplaceSourceView(),
      marketplaceSourceView({
        id: source.id,
        kind: source.kind,
        location: source.location,
        label: source.label,
        enabled: source.enabled,
        removable: true,
        addedAt: source.addedAt,
      }),
    ];
    const catalog = [marketplace("/marketplace.json", [])];
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "set_omiga_plugin_marketplace_source_enabled") return undefined;
      if (command === "list_omiga_plugin_marketplace_sources") return [source];
      if (command === "list_omiga_plugin_marketplace_source_views") return sourceViews;
      if (command === "list_omiga_plugin_marketplaces") return catalog;
      throw new Error(`unexpected command ${command}`);
    });

    await usePluginStore
      .getState()
      .setMarketplaceSourceEnabled("source-1", false, "/project");

    expect(invokeMock).toHaveBeenCalledWith(
      "set_omiga_plugin_marketplace_source_enabled",
      { id: "source-1", enabled: false },
    );
    expect(invokeMock).toHaveBeenCalledWith(
      "list_omiga_plugin_marketplace_sources",
    );
    expect(invokeMock).toHaveBeenCalledWith(
      "list_omiga_plugin_marketplace_source_views",
    );
    expect(invokeMock).toHaveBeenCalledWith("list_omiga_plugin_marketplaces", {
      projectRoot: "/project",
    });
    expect(usePluginStore.getState().marketplaceSources).toEqual([source]);
    expect(usePluginStore.getState().marketplaceSourceViews).toEqual(sourceViews);
    expect(usePluginStore.getState().marketplaces).toEqual(catalog);
    expect(usePluginStore.getState().isMutating).toBe(false);
  });

  it("refreshes marketplace sources and reloads the plugin catalog on success", async () => {
    const sourceViews = [marketplaceSourceView()];
    const refreshResult: RefreshResult = {
      id: "source-1",
      ok: true,
      message: "Remote marketplace refreshed.",
      marketplaceName: "omiga-curated",
      pluginCount: 2,
    };
    const catalog = [marketplace("/cache/marketplace.json", [plugin()])];
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "refresh_omiga_plugin_marketplace_source") return refreshResult;
      if (command === "list_omiga_plugin_marketplace_source_views") return sourceViews;
      if (command === "list_omiga_plugin_marketplaces") return catalog;
      throw new Error(`unexpected command ${command}`);
    });

    const result = await usePluginStore
      .getState()
      .refreshMarketplaceSource("source-1", "/project");

    expect(result).toEqual(refreshResult);
    expect(invokeMock).toHaveBeenCalledWith(
      "refresh_omiga_plugin_marketplace_source",
      { id: "source-1", projectRoot: "/project" },
    );
    expect(invokeMock).toHaveBeenCalledWith("list_omiga_plugin_marketplaces", {
      projectRoot: "/project",
    });
    expect(invokeMock).toHaveBeenCalledWith(
      "list_omiga_plugin_marketplace_source_views",
    );
    expect(usePluginStore.getState().marketplaceSourceViews).toEqual(sourceViews);
    expect(usePluginStore.getState().marketplaces).toEqual(catalog);
    expect(usePluginStore.getState().isMutating).toBe(false);
  });

  it("refreshes marketplace sources and reloads the plugin catalog on failure", async () => {
    const sourceViews = [marketplaceSourceView()];
    const refreshResult: RefreshResult = {
      id: "source-1",
      ok: false,
      message: "invalid remote marketplace cache",
      marketplaceName: null,
      pluginCount: null,
    };
    const catalog = [marketplace("/cache/marketplace.json", [])];
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "refresh_omiga_plugin_marketplace_source") return refreshResult;
      if (command === "list_omiga_plugin_marketplace_source_views") return sourceViews;
      if (command === "list_omiga_plugin_marketplaces") return catalog;
      throw new Error(`unexpected command ${command}`);
    });

    const result = await usePluginStore
      .getState()
      .refreshMarketplaceSource("source-1", "/project");

    expect(result).toEqual(refreshResult);
    expect(invokeMock).toHaveBeenCalledWith("list_omiga_plugin_marketplaces", {
      projectRoot: "/project",
    });
    expect(usePluginStore.getState().marketplaceSourceViews).toEqual(sourceViews);
    expect(usePluginStore.getState().marketplaces).toEqual(catalog);
    expect(usePluginStore.getState().isMutating).toBe(false);
  });

  it("ensures the built-in marketplace and reloads sources plus catalog on success", async () => {
    const status: BuiltinMarketplaceStatus = {
      ok: true,
      source: "github",
      path: "/home/user/.omiga/marketplaces/builtin",
      message: "Built-in marketplace ready.",
    };
    const source = marketplaceSource({ id: "source-remote" });
    const sourceViews = [marketplaceSourceView()];
    const catalog = [marketplace("/builtin/marketplace.json", [plugin()])];
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "ensure_builtin_marketplace_source") return status;
      if (command === "list_omiga_plugin_marketplace_sources") return [source];
      if (command === "list_omiga_plugin_marketplace_source_views") return sourceViews;
      if (command === "list_omiga_plugin_marketplaces") return catalog;
      throw new Error(`unexpected command ${command}`);
    });

    const result = await usePluginStore
      .getState()
      .ensureBuiltinMarketplace("/project");

    expect(result).toEqual(status);
    expect(invokeMock).toHaveBeenCalledWith("ensure_builtin_marketplace_source", {
      projectRoot: "/project",
    });
    expect(invokeMock).toHaveBeenCalledWith(
      "list_omiga_plugin_marketplace_sources",
    );
    expect(invokeMock).toHaveBeenCalledWith(
      "list_omiga_plugin_marketplace_source_views",
    );
    expect(invokeMock).toHaveBeenCalledWith("list_omiga_plugin_marketplaces", {
      projectRoot: "/project",
    });
    expect(usePluginStore.getState().builtinMarketplaceStatus).toEqual(status);
    expect(usePluginStore.getState().marketplaceSources).toEqual([source]);
    expect(usePluginStore.getState().marketplaceSourceViews).toEqual(sourceViews);
    expect(usePluginStore.getState().marketplaces).toEqual(catalog);
    expect(usePluginStore.getState().bootstrapInProgress).toBe(false);
  });

  it("stores a failed built-in marketplace status without throwing", async () => {
    const status: BuiltinMarketplaceStatus = {
      ok: false,
      source: "github",
      path: null,
      message: "Install git or connect to the network, then retry.",
    };
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "ensure_builtin_marketplace_source") return status;
      throw new Error(`unexpected command ${command}`);
    });

    const result = await usePluginStore
      .getState()
      .ensureBuiltinMarketplace("/project");

    expect(result).toEqual(status);
    expect(usePluginStore.getState().builtinMarketplaceStatus).toEqual(status);
    expect(usePluginStore.getState().bootstrapInProgress).toBe(false);
    expect(invokeMock).not.toHaveBeenCalledWith(
      "list_omiga_plugin_marketplaces",
      expect.anything(),
    );
  });

  it("loads marketplace source views with source definitions", async () => {
    const source = marketplaceSource();
    const sourceViews = [
      marketplaceSourceView(),
      marketplaceSourceView({
        id: source.id,
        kind: source.kind,
        location: source.location,
        label: source.label,
        enabled: source.enabled,
        removable: true,
        addedAt: source.addedAt,
      }),
    ];
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "list_omiga_plugin_marketplace_sources") return [source];
      if (command === "list_omiga_plugin_marketplace_source_views") return sourceViews;
      throw new Error(`unexpected command ${command}`);
    });

    await usePluginStore.getState().loadMarketplaceSources();

    expect(invokeMock).toHaveBeenCalledWith(
      "list_omiga_plugin_marketplace_sources",
    );
    expect(invokeMock).toHaveBeenCalledWith(
      "list_omiga_plugin_marketplace_source_views",
    );
    expect(usePluginStore.getState().marketplaceSources).toEqual([source]);
    expect(usePluginStore.getState().marketplaceSourceViews).toEqual(sourceViews);
  });

  it("loads marketplace sources during the initial plugin refresh", async () => {
    const source = marketplaceSource();
    const sourceViews = [
      marketplaceSourceView(),
      marketplaceSourceView({
        id: source.id,
        kind: source.kind,
        location: source.location,
        label: source.label,
        enabled: source.enabled,
        removable: true,
        addedAt: source.addedAt,
      }),
    ];
    const catalog = [marketplace("/marketplace.json", [plugin()])];
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "ensure_builtin_marketplace_source") {
        return {
          ok: false,
          source: "github",
          path: null,
          message: "Offline.",
        } satisfies BuiltinMarketplaceStatus;
      }
      if (command === "list_omiga_plugin_marketplace_sources") return [source];
      if (command === "list_omiga_plugin_marketplace_source_views") return sourceViews;
      if (command === "list_omiga_plugin_marketplaces") return catalog;
      if (command === "list_omiga_plugin_retrieval_statuses") return [];
      if (command === "list_omiga_plugin_process_pool_statuses") return [];
      if (command === "list_operators") {
        return { registryPath: "/registry.json", operators: [], diagnostics: [] };
      }
      if (command === "list_operator_runs") return [];
      throw new Error(`unexpected command ${command}`);
    });

    await usePluginStore.getState().loadPlugins("/project");

    expect(invokeMock).toHaveBeenNthCalledWith(
      1,
      "ensure_builtin_marketplace_source",
      { projectRoot: "/project" },
    );
    expect(invokeMock).toHaveBeenCalledWith(
      "list_omiga_plugin_marketplace_sources",
    );
    expect(invokeMock).toHaveBeenCalledWith(
      "list_omiga_plugin_marketplace_source_views",
    );
    expect(usePluginStore.getState().marketplaceSources).toEqual([source]);
    expect(usePluginStore.getState().marketplaceSourceViews).toEqual(sourceViews);
    expect(usePluginStore.getState().marketplaces).toEqual(catalog);
    expect(usePluginStore.getState().isLoading).toBe(false);
  });

  it("triggers built-in marketplace bootstrap when loading plugins", async () => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "ensure_builtin_marketplace_source") {
        return {
          ok: false,
          source: "github",
          path: null,
          message: "Offline.",
        } satisfies BuiltinMarketplaceStatus;
      }
      if (command === "list_omiga_plugin_marketplace_sources") return [];
      if (command === "list_omiga_plugin_marketplace_source_views") return [];
      if (command === "list_omiga_plugin_marketplaces") return [];
      if (command === "list_omiga_plugin_retrieval_statuses") return [];
      if (command === "list_omiga_plugin_process_pool_statuses") return [];
      if (command === "list_operators") {
        return { registryPath: "/registry.json", operators: [], diagnostics: [] };
      }
      if (command === "list_operator_runs") return [];
      throw new Error(`unexpected command ${command}`);
    });

    await usePluginStore.getState().loadPlugins("/project");

    expect(invokeMock).toHaveBeenNthCalledWith(
      1,
      "ensure_builtin_marketplace_source",
      { projectRoot: "/project" },
    );
    expect(usePluginStore.getState().builtinMarketplaceStatus).toMatchObject({
      ok: false,
      message: "Offline.",
    });
    expect(usePluginStore.getState().isLoading).toBe(false);
  });

  it("toggles one retrieval route and refreshes route diagnostics", async () => {
    const target = plugin({
      id: "resource-ncbi@omiga-curated",
      name: "resource-ncbi",
      installed: true,
      enabled: true,
      retrieval: {
        protocolVersion: 1,
        resources: [
          {
            id: "geo",
            category: "dataset",
            label: "NCBI GEO",
            description: "GEO datasets",
            subcategories: [],
            capabilities: ["search", "query", "fetch"],
            requiredCredentialRefs: [],
            optionalCredentialRefs: [],
            defaultEnabled: true,
            replacesBuiltin: true,
            exposed: true,
          },
        ],
      },
    });
    usePluginStore.setState({
      marketplaces: [marketplace("/marketplace.json", [target])],
    });
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "set_omiga_retrieval_resource_enabled") return undefined;
      if (command === "list_omiga_plugin_retrieval_statuses") return [];
      if (command === "list_omiga_plugin_process_pool_statuses") return [];
      throw new Error(`unexpected command ${command}`);
    });

    await usePluginStore
      .getState()
      .setRetrievalResourceEnabled("resource-ncbi@omiga-curated", "dataset", "geo", false, "/project");

    expect(invokeMock).toHaveBeenNthCalledWith(
      1,
      "set_omiga_retrieval_resource_enabled",
      {
        pluginId: "resource-ncbi@omiga-curated",
        category: "dataset",
        resourceId: "geo",
        enabled: false,
        projectRoot: "/project",
      },
    );
    expect(invokeMock).toHaveBeenNthCalledWith(
      2,
      "list_omiga_plugin_retrieval_statuses",
      { projectRoot: "/project" },
    );
    expect(invokeMock).toHaveBeenNthCalledWith(
      3,
      "list_omiga_plugin_process_pool_statuses",
      { projectRoot: "/project" },
    );
    expect(
      usePluginStore.getState().marketplaces[0].plugins[0].retrieval?.resources[0],
    ).toMatchObject({ exposed: false });
  });

  it("invokes smoke runs with run context and stores the returned summary", async () => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "run_operator") {
        return {
          ok: true,
          result: {
            status: "succeeded",
            runId: "oprun_20260507_smoke",
            location: "local",
            operator: {
              alias: "write_text_report",
              id: "write_text_report",
              version: "0.1.0",
              sourcePlugin: "operator-smoke@omiga-curated",
            },
            runDir: "/project/.omiga/runs/oprun_20260507_smoke",
            runContext: {
              kind: "smoke",
              smokeTestId: "default",
              smokeTestName: "Write text report smoke",
            },
            outputs: {
              report: [
                {
                  path: "/project/.omiga/runs/oprun_20260507_smoke/out/operator-report.txt",
                },
              ],
            },
          },
        };
      }
      if (command === "list_operator_runs") return [];
      throw new Error(`unexpected command ${command}`);
    });

    await usePluginStore.getState().runOperator(
      "write_text_report",
      {
        inputs: {},
        params: {
          message: "hello operator smoke",
          repeat: 2,
        },
        resources: {},
      },
      "/project",
      {
        sessionId: "session-1",
        executionEnvironment: "local",
        sshServer: null,
        sandboxBackend: "docker",
      },
      {
        kind: "smoke",
        smokeTestId: "default",
        smokeTestName: "Write text report smoke",
      },
    );

    expect(invokeMock).toHaveBeenNthCalledWith(
      1,
      "run_operator",
      expect.objectContaining({
        alias: "write_text_report",
        projectRoot: "/project",
        sessionId: "session-1",
        executionEnvironment: "local",
        sandboxBackend: "docker",
        runKind: "smoke",
        smokeTestId: "default",
        smokeTestName: "Write text report smoke",
      }),
    );
    expect(invokeMock).toHaveBeenNthCalledWith(
      2,
      "list_operator_runs",
      expect.objectContaining({
        projectRoot: "/project",
        sessionId: "session-1",
        executionEnvironment: "local",
        sandboxBackend: "docker",
      }),
    );
    expect(usePluginStore.getState().operatorRuns[0]).toMatchObject({
      runId: "oprun_20260507_smoke",
      runKind: "smoke",
      smokeTestId: "default",
      smokeTestName: "Write text report smoke",
      outputCount: 1,
    });
  });

  it("keeps failed run diagnostics when run_operator returns an error result", async () => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "run_operator") {
        return {
          ok: false,
          result: {
            status: "failed",
            runId: "oprun_20260507_failed",
            location: "local",
            operator: {
              alias: "write_text_report",
              id: "write_text_report",
              version: "0.1.0",
              sourcePlugin: "operator-smoke@omiga-curated",
            },
            runDir: "/project/.omiga/runs/oprun_20260507_failed",
            runContext: {
              kind: "smoke",
              smokeTestId: "default",
              smokeTestName: "Write text report smoke",
            },
            error: {
              kind: "tool_exit_nonzero",
              retryable: false,
              message: "bad input",
              suggestedAction: "Inspect stdout/stderr, then adjust inputs or params and retry.",
              stdoutTail: "partial stdout\n",
              stderrTail: "bad flag\n",
            },
          },
        };
      }
      if (command === "list_operator_runs") return [];
      throw new Error(`unexpected command ${command}`);
    });

    await expect(
      usePluginStore.getState().runOperator(
        "write_text_report",
        { inputs: {}, params: { repeat: 0 }, resources: {} },
        "/project",
        { executionEnvironment: "local" },
        {
          kind: "smoke",
          smokeTestId: "default",
          smokeTestName: "Write text report smoke",
        },
      ),
    ).rejects.toThrow("bad input");

    expect(usePluginStore.getState().error).toBe("bad input");
    expect(usePluginStore.getState().operatorRuns[0]).toMatchObject({
      runId: "oprun_20260507_failed",
      status: "failed",
      runKind: "smoke",
      errorMessage: "bad input",
      errorKind: "tool_exit_nonzero",
      retryable: false,
      suggestedAction: "Inspect stdout/stderr, then adjust inputs or params and retry.",
      stdoutTail: "partial stdout\n",
      stderrTail: "bad flag\n",
    });
  });

  it("detaches async operator task listeners during cleanup", async () => {
    const unlisten = vi.fn();
    listenTauriEventMock.mockResolvedValueOnce(unlisten);
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "run_operator_async") {
        return { taskId: "task-async-1" };
      }
      throw new Error(`unexpected command ${command}`);
    });

    await usePluginStore.getState().runOperatorAsync(
      "write_text_report",
      { inputs: {}, params: {}, resources: {} },
    );

    expect(listenTauriEventMock).toHaveBeenCalledWith(
      "operator-task-task-async-1",
      expect.any(Function),
    );

    usePluginStore.getState().cleanupOperatorTaskListeners();
    usePluginStore.getState().cleanupOperatorTaskListeners();

    expect(unlisten).toHaveBeenCalledTimes(1);
  });

  it("invokes operator run cleanup with the active execution surface", async () => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "cleanup_operator_runs") {
        return {
          dryRun: true,
          location: "local",
          runsRoot: "/project/.omiga/runs",
          scannedCount: 3,
          matchedCount: 1,
          deletedCount: 0,
          skippedCount: 0,
          estimatedBytes: 1024,
          candidates: [
            {
              runId: "oprun_cache",
              status: "succeeded",
              location: "local",
              runDir: "/project/.omiga/runs/oprun_cache",
              cacheHit: true,
              outputCount: 1,
              reason: "cache_hit_record",
              estimatedBytes: 1024,
              deleted: false,
            },
          ],
        };
      }
      if (command === "list_operator_runs") return [];
      throw new Error(`unexpected command ${command}`);
    });

    const request = {
      dryRun: true,
      keepLatest: 25,
      maxAgeDays: 30,
      includeCacheHits: true,
      includeFailed: true,
      includeSucceeded: true,
      limit: 500,
      operatorAlias: "write_text_report",
      operatorId: "write_text_report",
      operatorVersion: "0.1.0",
      sourcePlugin: "operator-smoke@omiga-curated",
    };
    const result = await usePluginStore.getState().cleanupOperatorRuns(
      request,
      "/project",
      {
        sessionId: "session-1",
        executionEnvironment: "ssh",
        sshServer: "gpu",
      },
    );

    expect(result.matchedCount).toBe(1);
    expect(invokeMock).toHaveBeenNthCalledWith(
      1,
      "cleanup_operator_runs",
      expect.objectContaining({
        request,
        projectRoot: "/project",
        sessionId: "session-1",
        executionEnvironment: "ssh",
        sshServer: "gpu",
      }),
    );
    expect(invokeMock).toHaveBeenNthCalledWith(
      2,
      "list_operator_runs",
      expect.objectContaining({
        projectRoot: "/project",
        sessionId: "session-1",
        executionEnvironment: "ssh",
        sshServer: "gpu",
      }),
    );
  });
});

describe("buildPluginDiagnostics", () => {
  it("builds a copyable diagnostics payload without credential values", () => {
    const retrievalRoute: PluginRetrievalRouteStatus = {
      pluginId: "retrieval-protocol-example@local",
      category: "dataset",
      resourceId: "example_dataset",
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
      resourceId: "example_dataset",
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
          environments: [
            {
              id: "example-conda",
              version: "0.1.0",
              canonicalId: "retrieval-protocol-example@local:example-conda",
              name: "Example conda",
              description: "Example runtime profile",
              manifestPath: "/plugins/retrieval-protocol-example/environments/example-conda/environment.yaml",
              runtimeType: "conda",
              runtimeFile: "/plugins/retrieval-protocol-example/environments/example-conda/conda.yaml",
              runtimeFileKind: "conda.yaml|conda.yml",
              installHint: "Install micromamba.",
              checkCommand: ["python", "--version"],
              availabilityStatus: "missing",
              availabilityManager: null,
              availabilityMessage: "No conda manager found.",
            },
          ],
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
    expect(diagnostics.plugin.environments[0]).toMatchObject({
      id: "example-conda",
      runtimeType: "conda",
      availabilityStatus: "missing",
    });
    expect(diagnostics.retrievalRoutes).toEqual([retrievalRoute]);
    expect(diagnostics.pooledProcesses).toEqual([pooledProcess]);
    expect(JSON.stringify(diagnostics)).not.toContain("secret");
    expect(diagnostics.notes.join(" ")).toContain("No credential values");
  });

  it("includes declared retrieval resource summaries without process internals", () => {
    const diagnostics = JSON.parse(
      buildPluginDiagnostics(
        plugin({
          id: "public-dataset-sources@omiga-curated",
          name: "public-dataset-sources",
          retrieval: {
            protocolVersion: 1,
            resources: [
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

    expect(diagnostics.plugin.retrieval.resources[0]).toMatchObject({
      id: "biosample",
      category: "dataset",
      label: "NCBI BioSample",
      replacesBuiltin: true,
    });
    expect(JSON.stringify(diagnostics)).not.toContain("runtime");
    expect(JSON.stringify(diagnostics)).not.toContain("command");
  });
});

describe("buildRetrievalRuntimeDiagnostics", () => {
  it("builds a global route health payload without credential values", () => {
    const route: PluginRetrievalRouteStatus = {
      pluginId: "retrieval-dataset-geo@omiga-curated",
      category: "dataset",
      resourceId: "geo",
      route: "dataset.geo via retrieval-dataset-geo@omiga-curated",
      state: "quarantined",
      quarantined: true,
      consecutiveFailures: 3,
      remainingMs: 45_000,
      lastError: "plugin error upstream_failed: forced fixture error",
    };
    const pooled: PluginProcessPoolRouteStatus = {
      pluginId: "retrieval-dataset-geo@omiga-curated",
      category: "dataset",
      resourceId: "geo",
      route: "dataset.geo via retrieval-dataset-geo@omiga-curated",
      pluginRoot: "/plugins/retrieval-dataset-geo",
      remainingMs: 30_000,
    };

    const diagnostics = JSON.parse(
      buildRetrievalRuntimeDiagnostics(
        [
          plugin({
            id: "retrieval-dataset-geo@omiga-curated",
            name: "retrieval-dataset-geo",
            marketplaceName: "omiga-curated",
            sourcePath: "/marketplace/retrieval-dataset-geo",
            installedPath: "/plugins/retrieval-dataset-geo",
            installed: true,
            enabled: true,
            retrieval: {
              protocolVersion: 1,
              resources: [
                {
                  id: "geo",
                  category: "dataset",
                  label: "NCBI GEO",
                  description: "NCBI GEO datasets",
                  subcategories: [],
                  capabilities: ["search", "query", "fetch"],
                  requiredCredentialRefs: [],
                  optionalCredentialRefs: ["secret_token"],
                  defaultEnabled: true,
                  replacesBuiltin: true,
                },
              ],
            },
          }),
          plugin({ id: "notebook-helper@omiga-curated" }),
        ],
        [route],
        [pooled],
      ),
    );

    expect(diagnostics.protocolDocPath).toBe(
      RETRIEVAL_PLUGIN_PROTOCOL_DOC_PATH,
    );
    expect(diagnostics.summary).toMatchObject({
      pluginCount: 1,
      routeCount: 1,
      quarantinedRouteCount: 1,
      pooledProcessCount: 1,
      unknownPluginCount: 0,
    });
    expect(diagnostics.plugins).toHaveLength(1);
    expect(diagnostics.unknownPluginIds).toEqual([]);
    expect(diagnostics.plugins[0]).toMatchObject({
      id: "retrieval-dataset-geo@omiga-curated",
      installed: true,
      enabled: true,
      declaredRouteCount: 1,
    });
    expect(diagnostics.retrievalRoutes).toEqual([route]);
    expect(diagnostics.pooledProcesses).toEqual([pooled]);
    expect(JSON.stringify(diagnostics)).not.toContain("secret_token");
    expect(diagnostics.notes.join(" ")).toContain("No credential values");

    const staleDiagnostics = JSON.parse(
      buildRetrievalRuntimeDiagnostics([], [route], []),
    );
    expect(staleDiagnostics.summary.unknownPluginCount).toBe(1);
    expect(staleDiagnostics.unknownPluginIds).toEqual([
      "retrieval-dataset-geo@omiga-curated",
    ]);
  });
});
