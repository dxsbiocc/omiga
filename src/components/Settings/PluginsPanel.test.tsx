import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import {
  displayName,
  filterPluginsForCatalog,
  groupPluginsByCatalogGroup,
  groupPluginsByCatalogSection,
  operatorImplementationIconSpec,
  operatorPluginIconSpec,
  operatorDisplayName,
  operatorPrimaryAlias,
  operatorRuntimeSummary,
  operatorRunBelongsToOperator,
  operatorRunDiagnosisSummary,
  operatorRunDiagnosticsPayload,
  operatorRunIsCacheHit,
  operatorRunIsSmoke,
  operatorRunStats,
  operatorSmokeRunLabel,
  operatorSmokeTestForRun,
  operatorSmokeTestSummary,
  operatorSchemaStats,
  operatorStructuredOutputEntries,
  operatorTemplateScript,
  operatorRunStatusColor,
  operatorRunTitle,
  operatorSmokeRunArguments,
  operatorSupportsSmokeRun,
  operatorRunsForOperator,
  operatorToolName,
  pluginCardSubtitle,
  pluginCatalogGroupId,
  pluginRuntimeSummary,
  processPoolStatusDiagnostic,
  retrievalStatusDiagnostic,
  unknownRetrievalRuntimePluginIds,
} from "./PluginsPanel";
import type {
  OperatorRunSummary,
  OperatorSummary,
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

function operatorSummary(overrides: Partial<OperatorSummary> = {}): OperatorSummary {
  return {
    id: "fastqc",
    version: "1.0.0",
    name: "FastQC",
    description: "Run FastQC quality control.",
    sourcePlugin: "bio-operators@local",
    manifestPath: "/plugins/bio-operators/operators/fastqc/operator.yaml",
    smokeTests: [],
    enabledAliases: [],
    exposed: false,
    unavailableReason: null,
    ...overrides,
  };
}

function operatorRunSummary(
  overrides: Partial<OperatorRunSummary> = {},
): OperatorRunSummary {
  return {
    runId: "oprun_20260506_success",
    status: "succeeded",
    location: "local",
    operatorAlias: "write_text_report",
    operatorId: "write_text_report",
    operatorVersion: "0.1.0",
    sourcePlugin: "operator-smoke@omiga-curated",
    runKind: null,
    smokeTestId: null,
    smokeTestName: null,
    runDir: "/project/.omiga/runs/oprun_20260506_success",
    updatedAt: "2026-05-06T12:00:00Z",
    provenancePath: "/project/.omiga/runs/oprun_20260506_success/provenance.json",
    outputCount: 1,
    errorMessage: null,
    ...overrides,
  };
}

describe("PluginsPanel diagnostics helpers", () => {
  it("keeps the operator catalog section mounted for manual smoke runs", () => {
    const source = readFileSync(new URL("./PluginsPanel.tsx", import.meta.url), "utf8");

    expect(source).toMatch(/<OperatorCatalogSection[\s\S]*onSmokeRun=/);
    expect(source).not.toContain("{false && (\n      <OperatorCatalogSection");
  });

  it("extracts structured output entries from run details", () => {
    expect(
      operatorStructuredOutputEntries({
        runId: "oprun_20260507_structured",
        location: "local",
        runDir: "/project/.omiga/runs/oprun_20260507_structured",
        sourcePath: "/project/.omiga/runs/oprun_20260507_structured/provenance.json",
        document: {
          structuredOutputs: {
            summary: { lineCount: 2 },
            ok: true,
          },
        },
      }),
    ).toEqual([
      ["summary", { lineCount: 2 }],
      ["ok", true],
    ]);
    expect(
      operatorStructuredOutputEntries({
        runId: "oprun_20260507_no_structured",
        location: "local",
        runDir: "/project/.omiga/runs/oprun_20260507_no_structured",
        sourcePath: "/project/.omiga/runs/oprun_20260507_no_structured/status.json",
        document: { structuredOutputs: [] },
      }),
    ).toEqual([]);
  });

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
    const operatorPlugin = pluginSummary({
      id: "operator-smoke@omiga-curated",
      name: "operator-smoke",
      interface: {
        displayName: "Smoke Test",
        shortDescription: "Adds a write-text-report operator",
        longDescription: null,
        developerName: null,
        category: "Operator",
        capabilities: ["Operator", "Local Execution"],
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
    const functionPlugin = pluginSummary({
      id: "function-catalog@omiga-curated",
      name: "function-catalog",
      interface: {
        displayName: "Function Catalog",
        shortDescription: "Adds callable helper functions",
        longDescription: null,
        developerName: null,
        category: "Function",
        capabilities: ["Function", "Custom Tool"],
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
    const plugins = [geo, notebook, operatorPlugin, functionPlugin];

    expect(filterPluginsForCatalog(plugins, "geo expression", "all")).toEqual([
      geo,
    ]);
    expect(filterPluginsForCatalog(plugins, "", "data-sources")).toEqual([
      geo,
    ]);
    expect(filterPluginsForCatalog(plugins, "", "general")).toEqual([
      notebook,
    ]);
    expect(filterPluginsForCatalog(plugins, "", "operators")).toEqual([
      operatorPlugin,
    ]);
    expect(filterPluginsForCatalog(plugins, "", "tools")).toEqual([
      functionPlugin,
    ]);
    expect(filterPluginsForCatalog(plugins, "", "enabled")).toEqual([geo]);
    expect(filterPluginsForCatalog(plugins, "repair", "available")).toEqual([
      notebook,
    ]);
  });

  it("groups plugin cards by top-level plugin type before source category", () => {
    const geo = pluginSummary({
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
    const operatorPlugin = pluginSummary({
      id: "operator-smoke@omiga-curated",
      name: "operator-smoke",
      interface: {
        displayName: "Smoke Test",
        shortDescription: null,
        longDescription: null,
        developerName: null,
        category: "Operator",
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
    });
    const functionPlugin = pluginSummary({
      id: "function-runner@omiga-curated",
      name: "function-runner",
      interface: {
        displayName: "Function Runner",
        shortDescription: null,
        longDescription: null,
        developerName: null,
        category: "Function",
        capabilities: ["Custom Tool"],
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
    const notebook = pluginSummary({
      id: "notebook-helper@omiga-curated",
      name: "notebook-helper",
      interface: {
        displayName: "Notebook Helper",
        shortDescription: null,
        longDescription: null,
        developerName: null,
        category: "Notebook",
        capabilities: ["Workflow"],
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

    expect(pluginCatalogGroupId(operatorPlugin)).toBe("operator");
    expect(pluginCatalogGroupId(functionPlugin)).toBe("tools");
    expect(pluginCatalogGroupId(geo)).toBe("source");
    expect(pluginCatalogGroupId(notebook)).toBe("other");
    expect(
      groupPluginsByCatalogGroup([notebook, geo, functionPlugin, operatorPlugin])
        .map((group) => group.id),
    ).toEqual(["operator", "tools", "source", "other"]);
    expect(
      groupPluginsByCatalogSection("source", [geo]).map((section) => section.title),
    ).toEqual(["Dataset sources"]);
    expect(
      groupPluginsByCatalogSection("other", [notebook]).map((section) => section.title),
    ).toEqual(["Notebook"]);
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

  it("removes redundant plugin role suffixes from display titles", () => {
    expect(
      displayName(pluginSummary({
        interface: {
          displayName: "GEO Retrieval Source",
          shortDescription: null,
          longDescription: null,
          developerName: null,
          category: "Retrieval",
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
      })),
    ).toBe("GEO");
    expect(
      displayName(pluginSummary({
        interface: {
          displayName: "PCA Operator",
          shortDescription: null,
          longDescription: null,
          developerName: null,
          category: "Operator",
          capabilities: ["Operator", "Rscript"],
          websiteUrl: null,
          privacyPolicyUrl: null,
          termsOfServiceUrl: null,
          defaultPrompt: [],
          brandColor: null,
          composerIcon: null,
          logo: null,
          screenshots: [],
        },
      })),
    ).toBe("PCA");
  });

  it("uses Iconify-backed implementation icons for operator plugin cards", () => {
    expect(
      operatorPluginIconSpec(pluginSummary({
        name: "operator-pca-r",
        interface: {
          displayName: "PCA",
          shortDescription: "PCA powered by base R",
          longDescription: null,
          developerName: null,
          category: "Operator",
          capabilities: ["Operator", "Rscript"],
          websiteUrl: null,
          privacyPolicyUrl: null,
          termsOfServiceUrl: null,
          defaultPrompt: [],
          brandColor: null,
          composerIcon: null,
          logo: null,
          screenshots: [],
        },
      }))?.kind,
    ).toBe("r");
    expect(
      operatorPluginIconSpec(pluginSummary({
        name: "operator-pca-r",
        interface: {
          displayName: "PCA",
          shortDescription: "PCA powered by base R",
          longDescription: null,
          developerName: null,
          category: "Operator",
          capabilities: ["Operator", "Rscript"],
          websiteUrl: null,
          privacyPolicyUrl: null,
          termsOfServiceUrl: null,
          defaultPrompt: [],
          brandColor: null,
          composerIcon: null,
          logo: null,
          screenshots: [],
        },
      }))?.color,
    ).toBe("#276DC3");
    expect(
      operatorPluginIconSpec(pluginSummary({
        name: "operator-pca-r",
        interface: {
          displayName: "PCA",
          shortDescription: "PCA powered by base R",
          longDescription: null,
          developerName: null,
          category: "Operator",
          capabilities: ["Operator", "Rscript"],
          websiteUrl: null,
          privacyPolicyUrl: null,
          termsOfServiceUrl: null,
          defaultPrompt: [],
          brandColor: null,
          composerIcon: null,
          logo: null,
          screenshots: [],
        },
      }))?.body,
    ).toContain("<path");
    expect(
      operatorPluginIconSpec(pluginSummary({
        name: "operator-seqtk",
        interface: {
          displayName: "seqtk",
          shortDescription: "FASTQ/FASTA subsampling with seqtk",
          longDescription: null,
          developerName: null,
          category: "Operator",
          capabilities: ["Operator", "seqtk"],
          websiteUrl: null,
          privacyPolicyUrl: null,
          termsOfServiceUrl: null,
          defaultPrompt: [],
          brandColor: null,
          composerIcon: null,
          logo: null,
          screenshots: [],
        },
      }))?.kind,
    ).toBe("c");
    expect(operatorPluginIconSpec(pluginSummary())).toBeNull();
  });

  it("builds stable operator display labels and tool names", () => {
    expect(operatorDisplayName(operatorSummary())).toBe("FastQC");
    expect(operatorDisplayName(operatorSummary({ name: " " }))).toBe("fastqc");
    expect(operatorPrimaryAlias(operatorSummary())).toBe("fastqc");
    expect(
      operatorPrimaryAlias(
        operatorSummary({ enabledAliases: ["", "sample_qc"], exposed: true }),
      ),
    ).toBe("sample_qc");
    expect(operatorToolName("sample_qc")).toBe("operator__sample_qc");
  });

  it("summarizes operator scripts, runtime, and schemas for plugin details", () => {
    const operator = operatorSummary({
      execution: {
        argv: ["Rscript", "./bin/pca_matrix.R", "${inputs.matrix}", "${outdir}"],
      },
      runtime: {
        placement: { supported: ["local", "ssh"] },
        container: { supported: ["none"] },
      },
      interface: {
        inputs: {
          matrix: { kind: "file", required: true },
        },
        params: {
          scale: { kind: "boolean", default: true },
          group_column: { kind: "string", required: true },
        },
        outputs: {
          scores: { kind: "file", required: true, glob: "pca-scores.tsv" },
        },
      },
      resources: {
        cpu: { default: 1, exposed: true },
        walltime: { default: "300s", exposed: true },
      },
    });

    expect(operatorTemplateScript(operator)).toBe("./bin/pca_matrix.R");
    expect(operatorImplementationIconSpec(operator).kind).toBe("r");
    expect(operatorRuntimeSummary(operator)).toBe("placement: local, ssh · container: none");
    expect(operatorSchemaStats(operator)).toEqual({
      inputs: 1,
      requiredInputs: 1,
      params: 2,
      requiredParams: 1,
      outputs: 1,
      resources: 2,
    });
  });

  it("recognizes manifest-declared smoke tests and builds deterministic smoke args", () => {
    const smokeOperator = operatorSummary({
      smokeTests: [
        {
          id: "default",
          name: "Write text report smoke",
          description: "Generates a deterministic two-line report artifact.",
          arguments: {
            inputs: {},
            params: {
              message: "hello operator smoke",
              repeat: 2,
            },
            resources: {},
          },
        },
        {
          id: "large",
          name: "Large smoke",
          description: "Uses a larger repeat count.",
          arguments: {
            inputs: {},
            params: {
              message: "large smoke",
              repeat: 5,
            },
            resources: {},
          },
        },
      ],
    });

    expect(operatorSupportsSmokeRun(smokeOperator)).toBe(true);
    expect(operatorSmokeTestForRun(smokeOperator, "large")?.id).toBe("large");
    expect(operatorSmokeTestForRun(smokeOperator, "missing")?.id).toBe("default");
    expect(operatorSmokeRunLabel(smokeOperator)).toBe("Write text report smoke");
    expect(operatorSmokeRunLabel(smokeOperator, "large")).toBe("Large smoke");
    expect(operatorSmokeTestSummary(smokeOperator)).toBe(
      "Write text report smoke: Generates a deterministic two-line report artifact. · +1 more",
    );
    expect(operatorSmokeTestSummary(smokeOperator, "large")).toBe(
      "Large smoke: Uses a larger repeat count. · +1 more",
    );
    expect(operatorSmokeRunArguments(smokeOperator)).toEqual({
      inputs: {},
      params: {
        message: "hello operator smoke",
        repeat: 2,
      },
      resources: {},
    });
    expect(operatorSmokeRunArguments(smokeOperator, "large")).toEqual({
      inputs: {},
      params: {
        message: "large smoke",
        repeat: 5,
      },
      resources: {},
    });
    expect(operatorSmokeRunArguments(smokeOperator, "missing")).toEqual({
      inputs: {},
      params: {
        message: "hello operator smoke",
        repeat: 2,
      },
      resources: {},
    });
    expect(
      operatorSupportsSmokeRun(
        operatorSummary({
          id: "write_text_report",
          sourcePlugin: "operator-smoke@omiga-curated",
        }),
      ),
    ).toBe(false);
    expect(operatorSupportsSmokeRun(operatorSummary())).toBe(false);
    expect(operatorSmokeTestSummary(operatorSummary())).toBeNull();
    expect(operatorSmokeRunArguments(operatorSummary())).toEqual({
      inputs: {},
      params: {},
      resources: {},
    });
  });

  it("labels operator run status and titles for diagnostics", () => {
    expect(operatorRunTitle(operatorRunSummary())).toBe(
      "operator__write_text_report",
    );
    expect(
      operatorRunTitle(
        operatorRunSummary({ operatorAlias: null, operatorId: "fastqc" }),
      ),
    ).toBe("fastqc");
    expect(operatorRunStatusColor("succeeded")).toBe("success");
    expect(operatorRunStatusColor("failed")).toBe("error");
    expect(operatorRunStatusColor("running")).toBe("info");
    expect(operatorRunStatusColor("timed_out")).toBe("warning");
  });

  it("builds per-operator run statistics for cards and details", () => {
    const operator = operatorSummary({
      id: "write_text_report",
      version: "0.1.0",
      sourcePlugin: "operator-smoke@omiga-curated",
      enabledAliases: ["write_text_report"],
      exposed: true,
    });
    const success = operatorRunSummary({
      runId: "oprun_success",
      status: "succeeded",
      updatedAt: "2026-05-06T12:00:00Z",
    });
    const failed = operatorRunSummary({
      runId: "oprun_failed",
      status: "failed",
      updatedAt: "2026-05-06T13:00:00Z",
      errorMessage: "bad input",
      errorKind: "tool_exit_nonzero",
      retryable: false,
      suggestedAction: "Inspect stderr and retry.",
      stderrTail: "bad flag\n",
    });
    const running = operatorRunSummary({
      runId: "oprun_running",
      status: "running",
      updatedAt: "2026-05-06T14:00:00Z",
    });
    const smoke = operatorRunSummary({
      runId: "oprun_smoke",
      status: "succeeded",
      runKind: "smoke",
      smokeTestId: "default",
      smokeTestName: "Default smoke",
      updatedAt: "2026-05-06T15:00:00Z",
    });
    const cacheHit = operatorRunSummary({
      runId: "oprun_cache",
      status: "succeeded",
      updatedAt: "2026-05-06T16:00:00Z",
      cacheKey: "sha256:cache-key",
      cacheHit: true,
      cacheSourceRunId: "oprun_success",
      cacheSourceRunDir: "/project/.omiga/runs/oprun_success",
    });
    const unrelated = operatorRunSummary({
      runId: "oprun_fastqc",
      operatorAlias: "fastqc",
      operatorId: "fastqc",
      sourcePlugin: "bio-operators@local",
      operatorVersion: "1.0.0",
    });

    expect(operatorRunBelongsToOperator(operator, success)).toBe(true);
    expect(operatorRunBelongsToOperator(operator, unrelated)).toBe(false);
    expect(operatorRunIsSmoke(smoke)).toBe(true);
    expect(operatorRunIsSmoke(success)).toBe(false);
    expect(operatorRunIsCacheHit(cacheHit)).toBe(true);
    expect(operatorRunIsCacheHit(success)).toBe(false);
    expect(operatorRunDiagnosisSummary(failed)).toBe("bad input");
    expect(JSON.parse(operatorRunDiagnosticsPayload(failed, operator))).toMatchObject({
      operator: {
        id: "write_text_report",
      },
      run: {
        runId: "oprun_failed",
      },
      error: {
        kind: "tool_exit_nonzero",
        message: "bad input",
        retryable: false,
        suggestedAction: "Inspect stderr and retry.",
        stderrTail: "bad flag\n",
      },
    });
    expect(JSON.parse(operatorRunDiagnosticsPayload(cacheHit, operator))).toMatchObject({
      cache: {
        hit: true,
        key: "sha256:cache-key",
        sourceRunId: "oprun_success",
        sourceRunDir: "/project/.omiga/runs/oprun_success",
      },
    });
    expect(operatorRunsForOperator(operator, [success, unrelated, failed])).toEqual([
      success,
      failed,
    ]);
    expect(operatorRunStats(operator, [success, failed, running, smoke, cacheHit, unrelated])).toMatchObject({
      total: 5,
      succeeded: 3,
      failed: 1,
      running: 1,
      cacheHits: 1,
      cacheMisses: 0,
      smokeTotal: 1,
      smokeSucceeded: 1,
      smokeFailed: 0,
      regularTotal: 4,
      latestRun: cacheHit,
      latestSmokeRun: smoke,
      latestRegularRun: cacheHit,
    });
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
