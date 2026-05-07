import { beforeEach, describe, expect, it, vi } from "vitest";

const invokeMock = vi.hoisted(() => vi.fn());

vi.mock("@tauri-apps/api/core", () => ({
  invoke: invokeMock,
}));

import {
  RETRIEVAL_PLUGIN_PROTOCOL_DOC_PATH,
  buildPluginDiagnostics,
  buildRetrievalRuntimeDiagnostics,
  flattenMarketplacePlugins,
  summarizeOperatorRunResult,
  updateOperatorEnabledInCatalog,
  updatePluginEnabledInMarketplaces,
  usePluginStore,
  type OperatorSummary,
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

  it("updates one operator registry entry without requiring a full plugin reload", () => {
    const operator: OperatorSummary = {
      id: "write_text_report",
      version: "0.1.0",
      name: "Write Text Report",
      description: null,
      sourcePlugin: "operator-smoke@omiga-curated",
      manifestPath: "/plugins/operator-smoke/operators/write_text_report.yaml",
      smokeTests: [],
      enabledAliases: [],
      exposed: false,
      unavailableReason: null,
    };
    const other: OperatorSummary = {
      ...operator,
      id: "fastqc",
      sourcePlugin: "bio-operators@local",
      manifestPath: "/plugins/bio/operators/fastqc.yaml",
    };

    const enabled = updateOperatorEnabledInCatalog([operator, other], {
      alias: "write_text_report",
      operatorId: "write_text_report",
      sourcePlugin: "operator-smoke@omiga-curated",
      version: "0.1.0",
      enabled: true,
    });

    expect(enabled[0]).toMatchObject({
      exposed: true,
      enabledAliases: ["write_text_report"],
    });
    expect(enabled[1]).toBe(other);

    const disabled = updateOperatorEnabledInCatalog(enabled, {
      alias: "write_text_report",
      operatorId: "write_text_report",
      sourcePlugin: "operator-smoke@omiga-curated",
      version: "0.1.0",
      enabled: false,
    });
    expect(disabled[0]).toMatchObject({
      exposed: false,
      enabledAliases: [],
    });
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
      smokeTestId: "default",
      smokeTestName: "Write text report smoke",
      errorKind: "tool_exit_nonzero",
      retryable: false,
      suggestedAction: "Inspect stderr.",
      stderrTail: "bad flag\n",
      outputCount: 1,
      cacheKey: "sha256:cache-key",
      cacheHit: true,
      cacheSourceRunId: "oprun_20260506_source",
      cacheSourceRunDir: "/project/.omiga/runs/oprun_20260506_source",
    });
  });
});

describe("usePluginStore operator actions", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    usePluginStore.setState({
      marketplaces: [],
      operators: [],
      operatorDiagnostics: [],
      operatorRegistryPath: null,
      operatorRuns: [],
      retrievalStatuses: [],
      processPoolStatuses: [],
      isLoading: false,
      isMutating: false,
      error: null,
    });
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

describe("buildRetrievalRuntimeDiagnostics", () => {
  it("builds a global route health payload without credential values", () => {
    const route: PluginRetrievalRouteStatus = {
      pluginId: "retrieval-dataset-geo@omiga-curated",
      category: "dataset",
      sourceId: "geo",
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
      sourceId: "geo",
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
              sources: [
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
