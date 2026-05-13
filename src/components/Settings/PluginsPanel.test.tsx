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
  operatorEnvironmentRef,
  operatorPrimaryAlias,
  operatorResourceProfile,
  operatorResourceProfileLabel,
  operatorResourceProfileSummary,
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
  operatorShouldWarnBeforeLocalRun,
  operatorSupportsSmokeRun,
  operatorRunsForOperator,
  operatorToolName,
  pluginDetailsDialogSx,
  pluginDetailsTechnicalSectionSx,
  pluginCardSubtitle,
  pluginContentOverview,
  pluginCatalogGroupId,
  pluginEnvironmentDisplayName,
  pluginEnvironmentRuntimeFileLabel,
  pluginEnvironmentStatusColor,
  pluginRuntimeSummary,
  processPoolStatusDiagnostic,
  retrievalStatusDiagnostic,
  shouldShowPluginRuntimeSummaryCard,
  unknownRetrievalRuntimePluginIds,
  visualizationRExecuteSkeletonSx,
  visualizationRCompletionOverview,
  visualizationRTemplatePrompt,
  visualizationRTemplateToolCall,
} from "./PluginsPanel";
import type {
  OperatorRunSummary,
  OperatorSummary,
  PluginEnvironmentSummary,
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
    resourceId: "geo",
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
  it("keeps raw tool registry and run diagnostics behind the development diagnostics gate", () => {
    const source = readFileSync(new URL("./PluginsPanel.tsx", import.meta.url), "utf8");

    expect(source).toContain("SHOW_PLUGIN_DEVELOPER_DIAGNOSTICS = import.meta.env.DEV");
    expect(source).toContain("SHOW_PLUGIN_DEVELOPER_DIAGNOSTICS && (");
    expect(source).toMatch(/<OperatorCatalogSection[\s\S]*onSmokeRun=/);
    expect(source).not.toContain("{false && (\n      <OperatorCatalogSection");
  });

  it("keeps advanced tool registration behind product-oriented wording", () => {
    const source = readFileSync(new URL("./PluginsPanel.tsx", import.meta.url), "utf8");

    expect(source).toContain("Agent tools");
    expect(source).toContain("Advanced controls for plugin-defined tools");
    expect(source).toContain("Register to run smoke test");
    expect(source).not.toContain(">Operators<");
    expect(source).not.toContain(" exposed`");
    expect(source).not.toContain("Operators are plugin-defined tools");
  });

  it("keeps plugin technical content out of the default capability summary", () => {
    const source = readFileSync(new URL("./PluginsPanel.tsx", import.meta.url), "utf8");

    expect(source).toContain("Capabilities");
    expect(source).toContain("pluginContentOverview(plugin, operators)");
    expect(source).toContain("Runtime environments");
    expect(source).toContain("SettingsRounded");
    expect(source).toContain("aria-label={`${configureEnvLabel}: ${pluginEnvironmentDisplayName(environment)}`}");
    expect(source).toContain("aria-label={`${testEnvLabel}: ${pluginEnvironmentDisplayName(environment)}`}");
    expect(source).toContain("onEnvironmentToggle(plugin, environment, event.target.checked)");
    expect(source).toContain("environment edits are only allowed in the user plugin copy");
    expect(source).toContain("Developer & troubleshooting");
    expect(source).not.toContain("Route details");
    expect(source).not.toContain("Operator details");
    expect(source).not.toContain("<OperatorBundleContentList operators={operators}");
    expect(source).not.toContain("Included content");
  });

  it("anchors the plugin details dialog while troubleshooting accordions expand", () => {
    const source = readFileSync(new URL("./PluginsPanel.tsx", import.meta.url), "utf8");

    expect(pluginDetailsDialogSx["& .MuiDialog-container"]).toEqual({
      alignItems: "flex-start",
    });
    expect(pluginDetailsDialogSx["& .MuiDialog-paper"]).toMatchObject({
      mt: { xs: 2, sm: 6 },
      mb: { xs: 2, sm: 6 },
      maxHeight: { xs: "calc(100% - 32px)", sm: "calc(100% - 96px)" },
    });
    expect(source).toContain("scroll=\"paper\"");
    expect(source).toContain("sx={pluginDetailsDialogSx}");
  });

  it("keeps technical accordions visually separated from capability cards", () => {
    const source = readFileSync(new URL("./PluginsPanel.tsx", import.meta.url), "utf8");

    expect(pluginDetailsTechnicalSectionSx).toMatchObject({
      display: "flex",
      flexDirection: "column",
      gap: 1.25,
      pt: 1.25,
    });
    expect(source).toContain("<Box sx={pluginDetailsTechnicalSectionSx}>");
  });

  it("keeps visualization-r execute skeletons readable in narrow cards", () => {
    expect(visualizationRExecuteSkeletonSx).toMatchObject({
      maxHeight: 180,
      overflow: "auto",
      overflowWrap: "anywhere",
      wordBreak: "break-word",
      whiteSpace: "pre-wrap",
    });
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
        resources: [
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
    const literatureSearchPlugin = pluginSummary({
      id: "operator-pubmed-search@omiga-curated",
      name: "operator-pubmed-search",
      interface: {
        displayName: "PubMed Search",
        shortDescription: "PubMed literature search",
        longDescription: null,
        developerName: null,
        category: "Literature",
        capabilities: ["Retrieval", "Search", "PubMed"],
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
    const visualizationPlugin = pluginSummary({
      id: "visualization-r@omiga-curated",
      name: "visualization-r",
      interface: {
        displayName: "R Visualization",
        shortDescription: "Human-editable R figures",
        longDescription: null,
        developerName: null,
        category: "Visualization",
        capabilities: ["Rscript"],
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
    const analysisPlugin = pluginSummary({
      id: "statistics-analysis@omiga-curated",
      name: "statistics-analysis",
      sourcePath: "/plugins/statistics-analysis",
      interface: {
        displayName: "Statistics Analysis",
        shortDescription: "Statistical workflow analysis",
        longDescription: null,
        developerName: null,
        category: "Analysis",
        capabilities: ["Analysis", "Statistics"],
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
    const bioinformaticsPlugin = pluginSummary({
      id: "ngs-alignment@omiga-curated",
      name: "ngs-alignment",
      sourcePath: "/plugins/ngs-alignment",
      interface: {
        displayName: "Alignment",
        shortDescription: "BWA, Bowtie2, STAR, and HISAT2 alignment",
        longDescription: null,
        developerName: null,
        category: "Bioinformatics",
        capabilities: ["NGS", "Alignment", "FASTQ", "SAM/BAM"],
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
    const plugins = [
      geo,
      notebook,
      operatorPlugin,
      literatureSearchPlugin,
      functionPlugin,
      visualizationPlugin,
      analysisPlugin,
      bioinformaticsPlugin,
    ];

    expect(filterPluginsForCatalog(plugins, "geo expression", "all")).toEqual([
      geo,
    ]);
    expect(filterPluginsForCatalog(plugins, "", "resources")).toEqual([
      geo,
      literatureSearchPlugin,
    ]);
    expect(filterPluginsForCatalog(plugins, "", "general")).toEqual([
      notebook,
    ]);
    expect(filterPluginsForCatalog(plugins, "", "operators")).toEqual([
      operatorPlugin,
    ]);
    expect(filterPluginsForCatalog(plugins, "", "visualization")).toEqual([
      visualizationPlugin,
    ]);
    expect(filterPluginsForCatalog(plugins, "", "analysis")).toEqual([
      analysisPlugin,
    ]);
    expect(filterPluginsForCatalog(plugins, "", "bioinformatics")).toEqual([
      bioinformaticsPlugin,
    ]);
    expect(filterPluginsForCatalog(plugins, "", "tools")).toEqual([
      functionPlugin,
    ]);
    expect(filterPluginsForCatalog(plugins, "", "enabled")).toEqual([geo]);
    expect(filterPluginsForCatalog(plugins, "repair", "available")).toEqual([
      notebook,
    ]);
  });

  it("groups plugin cards by product category before implementation details", () => {
    const geo = pluginSummary({
      retrieval: {
        protocolVersion: 1,
        resources: [
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
    const literatureSearchPlugin = pluginSummary({
      id: "operator-pubmed-search@omiga-curated",
      name: "operator-pubmed-search",
      interface: {
        displayName: "PubMed Search",
        shortDescription: null,
        longDescription: null,
        developerName: null,
        category: "Literature",
        capabilities: ["Retrieval", "Search", "PubMed"],
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
    const visualizationPlugin = pluginSummary({
      id: "visualization-r@omiga-curated",
      name: "visualization-r",
      interface: {
        displayName: "R Visualization",
        shortDescription: null,
        longDescription: null,
        developerName: null,
        category: "Visualization",
        capabilities: ["Rscript"],
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
    const analysisWorkflowPlugin = pluginSummary({
      id: "statistics-analysis@omiga-curated",
      name: "statistics-analysis",
      sourcePath: "/plugins/statistics-analysis",
      interface: {
        displayName: "Statistics Analysis",
        shortDescription: null,
        longDescription: null,
        developerName: null,
        category: "Analysis",
        capabilities: ["Statistics"],
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
    const analysisPlugin = pluginSummary({
      id: "transcriptomics@omiga-curated",
      name: "transcriptomics",
      sourcePath: "/plugins/transcriptomics",
      interface: {
        displayName: "Transcriptomics",
        shortDescription: null,
        longDescription: null,
        developerName: null,
        category: "Analysis",
        capabilities: ["Analysis", "Transcriptomics"],
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
    const bioinformaticsPlugin = pluginSummary({
      id: "ngs-alignment@omiga-curated",
      name: "ngs-alignment",
      sourcePath: "/plugins/ngs-alignment",
      interface: {
        displayName: "Alignment",
        shortDescription: "BWA, Bowtie2, STAR, and HISAT2 alignment",
        longDescription: null,
        developerName: null,
        category: "Bioinformatics",
        capabilities: ["NGS", "Alignment", "FASTQ", "SAM/BAM"],
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
    const computerUsePlugin = pluginSummary({
      id: "computer-use@omiga-curated",
      name: "computer-use",
      interface: {
        displayName: "Computer Use",
        shortDescription: "Adds gated local computer observation and input automation tools.",
        longDescription: null,
        developerName: null,
        category: "Automation",
        capabilities: ["computer.observe", "computer.input"],
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
    const providerSourcePlugin = pluginSummary({
      id: "resource-ncbi@omiga-curated",
      name: "resource-ncbi",
      sourcePath: "/plugins/resource-ncbi",
      retrieval: {
        protocolVersion: 1,
        resources: [
          {
            id: "pubmed",
            category: "literature",
            label: "PubMed",
            description: "PubMed literature",
            subcategories: ["biomedical_literature"],
            capabilities: ["search", "query", "fetch"],
            requiredCredentialRefs: [],
            optionalCredentialRefs: [],
            defaultEnabled: false,
            replacesBuiltin: true,
          },
          {
            id: "geo",
            category: "dataset",
            label: "NCBI GEO",
            description: "Gene expression datasets",
            subcategories: ["expression"],
            capabilities: ["search", "query", "fetch"],
            requiredCredentialRefs: [],
            optionalCredentialRefs: [],
            defaultEnabled: false,
            replacesBuiltin: true,
          },
        ],
      },
      interface: {
        displayName: "NCBI",
        shortDescription: "PubMed and GEO retrieval",
        longDescription: null,
        developerName: null,
        category: "Retrieval",
        capabilities: ["Provider", "NCBI", "Retrieval"],
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

    expect(pluginCatalogGroupId(analysisWorkflowPlugin)).toBe("analysis");
    expect(pluginCatalogGroupId(analysisPlugin)).toBe("bioinformatics");
    expect(pluginCatalogGroupId(operatorPlugin)).toBe("operator");
    expect(pluginCatalogGroupId(literatureSearchPlugin)).toBe("resource");
    expect(pluginCatalogGroupId(providerSourcePlugin)).toBe("resource");
    expect(pluginCatalogGroupId(visualizationPlugin)).toBe("visualization");
    expect(pluginCatalogGroupId(bioinformaticsPlugin)).toBe("bioinformatics");
    expect(pluginCatalogGroupId(functionPlugin)).toBe("tools");
    expect(pluginCatalogGroupId(geo)).toBe("resource");
    expect(pluginCatalogGroupId(computerUsePlugin)).toBe("operator");
    expect(pluginCatalogGroupId(notebook)).toBe("other");
    expect(
      groupPluginsByCatalogGroup([
        notebook,
        geo,
        functionPlugin,
        operatorPlugin,
        computerUsePlugin,
        visualizationPlugin,
        analysisWorkflowPlugin,
        analysisPlugin,
        bioinformaticsPlugin,
        providerSourcePlugin,
      ])
        .map((group) => group.id),
    ).toEqual(["analysis", "bioinformatics", "visualization", "resource", "operator", "tools", "other"]);
    expect(
      groupPluginsByCatalogSection("analysis", [analysisWorkflowPlugin]).map((section) => section.title),
    ).toEqual(["Statistical analysis"]);
    expect(
      groupPluginsByCatalogSection("visualization", [visualizationPlugin]).map((section) => section.title),
    ).toEqual(["R visualization"]);
    expect(
      groupPluginsByCatalogSection("bioinformatics", [bioinformaticsPlugin, analysisPlugin]).map((section) => section.title),
    ).toEqual(["NGS", "Transcriptomics"]);
    expect(
      groupPluginsByCatalogSection("resource", [geo, literatureSearchPlugin, providerSourcePlugin]).map((section) => section.title),
    ).toEqual(["Provider resources", "Dataset resources", "Literature resources"]);
    expect(
      groupPluginsByCatalogSection("operator", [computerUsePlugin, operatorPlugin]).map((section) => section.title),
    ).toEqual(["Automation plugins"]);
    expect(
      groupPluginsByCatalogSection("other", [notebook]).map((section) => section.title),
    ).toEqual(["Notebook"]);
  });

  it("detects stale runtime records whose plugin id is no longer catalogued", () => {
    const known = pluginSummary({ id: "retrieval-dataset-geo@omiga-curated" });
    const staleRoute = routeStatus({
      pluginId: "stale-pubmed@legacy-mcp",
      category: "literature",
      resourceId: "pubmed",
      route: "literature.pubmed via stale-pubmed@legacy-mcp",
    });
    const stalePool: PluginProcessPoolRouteStatus = {
      pluginId: "stale-web-search@legacy",
      category: "web",
      resourceId: "search",
      route: "web.search via stale-web-search@legacy",
      pluginRoot: "/old/plugins/stale-web-search",
      remainingMs: 15_000,
    };

    expect(
      unknownRetrievalRuntimePluginIds([known], [routeStatus(), staleRoute], [stalePool]),
    ).toEqual(["stale-pubmed@legacy-mcp", "stale-web-search@legacy"]);
  });

  it("uses retrieval resource labels as card subtitles instead of verbose local-plugin copy", () => {
    const subtitle = pluginCardSubtitle(
      pluginSummary({
        interface: {
          displayName: "GEO Retrieval Resource",
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
          resources: [
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

  it("summarizes provider-level source bundles before single-category route wording", () => {
    expect(
      pluginCardSubtitle(
        pluginSummary({
          id: "resource-ncbi@omiga-curated",
          name: "resource-ncbi",
          interface: {
            displayName: "NCBI",
            shortDescription: "NCBI retrieval routes",
            longDescription: null,
            developerName: null,
            category: "Retrieval",
            capabilities: ["Provider", "NCBI", "Retrieval"],
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
            resources: [
              {
                id: "pubmed",
                category: "literature",
                label: "PubMed",
                description: "PubMed literature",
                subcategories: [],
                capabilities: ["search", "query", "fetch"],
                requiredCredentialRefs: [],
                optionalCredentialRefs: [],
                defaultEnabled: false,
                replacesBuiltin: true,
              },
              {
                id: "geo",
                category: "dataset",
                label: "NCBI GEO",
                description: "GEO datasets",
                subcategories: [],
                capabilities: ["search", "query", "fetch"],
                requiredCredentialRefs: [],
                optionalCredentialRefs: [],
                defaultEnabled: false,
                replacesBuiltin: true,
              },
              {
                id: "ncbi_gene",
                category: "knowledge",
                label: "NCBI Gene",
                description: "NCBI Gene knowledge",
                subcategories: [],
                capabilities: ["search", "query", "fetch"],
                requiredCredentialRefs: [],
                optionalCredentialRefs: [],
                defaultEnabled: false,
                replacesBuiltin: true,
              },
            ],
          },
        }),
      ),
    ).toBe("3 routes: PubMed, NCBI GEO, NCBI Gene");
  });

  it("removes redundant plugin role suffixes from display titles", () => {
    expect(
      displayName(pluginSummary({
        interface: {
          displayName: "GEO Retrieval Resource",
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
    expect(
      displayName(pluginSummary({
        interface: {
          displayName: "R Visualization Templates",
          shortDescription: null,
          longDescription: null,
          developerName: null,
          category: "Visualization",
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
    ).toBe("R Visualization");
  });

  it("summarizes plugin capabilities without defaulting to file-level content", () => {
    const plugin = pluginSummary({
      id: "visualization-r@omiga-curated",
      name: "visualization-r",
      interface: {
        displayName: "R Visualization",
        shortDescription: "Human-editable R figures",
        longDescription: null,
        developerName: null,
        category: "Visualization",
        capabilities: ["Rscript"],
        websiteUrl: null,
        privacyPolicyUrl: null,
        termsOfServiceUrl: null,
        defaultPrompt: [
          "Use visualization-r to create an editable static figure.",
        ],
        brandColor: null,
        composerIcon: null,
        logo: null,
        screenshots: [],
      },
    });

    expect(pluginContentOverview(plugin)).toEqual([
      {
        id: "visualization",
        title: "Visualization",
        detail: "Create editable figures and publication-style plots from human-editable R artifacts.",
        meta: "Figures",
      },
    ]);
    expect(pluginContentOverview(plugin).map((item) => item.title)).not.toContain(
      "Files",
    );
    expect(pluginContentOverview(plugin).map((item) => item.title)).not.toContain(
      "Template library",
    );
  });

  it("keeps visualization-r fallback generic and suppresses redundant healthy-only status", () => {
    const plugin = pluginSummary({
      id: "visualization-r@omiga-curated",
      name: "visualization-r",
      installed: true,
      enabled: true,
      interface: {
        displayName: "R Visualization",
        shortDescription: "Editable R/ggplot2 figure templates",
        longDescription: null,
        developerName: null,
        category: "Visualization",
        capabilities: ["Rscript", "ggplot2"],
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
    const overview = visualizationRCompletionOverview();
    const runtimeSummary = pluginRuntimeSummary(plugin);

    expect(overview.totalTemplates).toBe(0);
    expect(overview.supportedGroups).toEqual([]);
    expect(overview.quickStarts).toEqual([]);
    expect(overview.outputs).toEqual(["PNG", "PDF", "editable R script"]);
    expect(overview.workflow.map((step) => step.title)).toEqual([
      "1. Prepare table",
      "2. Generate figure",
      "3. Refine source",
    ]);
    expect(overview.pending).toEqual([]);
    expect(runtimeSummary.label).toBe("Healthy");
    expect(shouldShowPluginRuntimeSummaryCard(plugin, runtimeSummary)).toBe(false);
  });

  it("uses plugin template metadata when building visualization-r completion copy", () => {
    const overview = visualizationRCompletionOverview({
      count: 2,
      groups: [
        {
          id: "scatter",
          title: "Scatter",
          count: 1,
          templates: [
            {
              id: "viz_scatter_basic",
              name: "Basic Scatter Plot",
              description: "Scatter plot.",
              category: "visualization/scatter",
              tags: ["scatter"],
              execute: {
                tool: "template_execute",
                arguments: {
                  id: "visualization-r@omiga-curated/template/viz_scatter_basic",
                  inputs: { table: "/examples/scatter/basic/example.tsv" },
                  params: { x_column: "x_value" },
                  resources: {},
                },
              },
            },
          ],
        },
        {
          id: "heatmap",
          title: "Heatmap",
          count: 1,
          templates: [
            {
              id: "viz_heatmap_clustered",
              name: "Clustered Heatmap",
              description: "Clustered heatmap.",
              category: "visualization/heatmap",
              tags: ["heatmap"],
            },
          ],
        },
      ],
    });

    expect(overview.totalTemplates).toBe(2);
    expect(overview.supportedGroups).toEqual([
      {
        id: "scatter",
        title: "Scatter",
        count: 1,
        items: ["Basic Scatter Plot"],
        templates: [
          {
            id: "viz_scatter_basic",
            name: "Basic Scatter Plot",
            description: "Scatter plot.",
            execute: {
              tool: "template_execute",
              arguments: {
                id: "visualization-r@omiga-curated/template/viz_scatter_basic",
                inputs: { table: "/examples/scatter/basic/example.tsv" },
                params: { x_column: "x_value" },
                resources: {},
              },
            },
          },
        ],
      },
      {
        id: "heatmap",
        title: "Heatmap",
        count: 1,
        items: ["Clustered Heatmap"],
        templates: [
          {
            id: "viz_heatmap_clustered",
            name: "Clustered Heatmap",
            description: "Clustered heatmap.",
          },
        ],
      },
    ]);
    expect(overview.quickStarts.map((template) => template.id)).toEqual([
      "viz_scatter_basic",
      "viz_heatmap_clustered",
    ]);
  });

  it("builds copyable visualization-r execution shortcuts", () => {
    const template = {
      id: "viz_scatter_basic",
      name: "Basic Scatter Plot",
      description: "Scatter plot.",
    };

    expect(visualizationRTemplatePrompt(template)).toContain("`viz_scatter_basic`");
    expect(visualizationRTemplatePrompt(template)).toContain("template_execute");
    expect(JSON.parse(visualizationRTemplateToolCall(template))).toEqual({
      tool: "template_execute",
      arguments: {
        id: "viz_scatter_basic",
        inputs: {
          table: "path/to/data.tsv",
        },
        params: {},
        resources: {},
      },
    });
  });

  it("prefers backend-provided unit_describe execute skeletons for visualization-r shortcuts", () => {
    const template = {
      id: "viz_scatter_basic",
      name: "Basic Scatter Plot",
      execute: {
        tool: "template_execute",
        arguments: {
          id: "visualization-r@omiga-curated/template/viz_scatter_basic",
          inputs: {
            table: "/plugins/visualization-r/templates/scatter/basic/example.tsv",
          },
          params: {
            x_column: "x_value",
            y_column: "y_value",
          },
          resources: {},
        },
      },
    };

    expect(JSON.parse(visualizationRTemplateToolCall(template))).toEqual(template.execute);
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

  it("summarizes operator environment references and plugin runtime profiles", () => {
    const environment: PluginEnvironmentSummary = {
      id: "ngs-bwa",
      version: "0.1.0",
      canonicalId: "ngs-alignment@omiga-curated:ngs-bwa",
      name: "BWA",
      description: "Conda environment for BWA indexing and alignment; includes samtools.",
      manifestPath: "/plugins/ngs-alignment/environments/ngs-bwa/environment.yaml",
      runtimeType: "conda",
      runtimeFile: "/plugins/ngs-alignment/environments/ngs-bwa/conda.yaml",
      runtimeFileKind: "conda.yaml|conda.yml",
      installHint: "Install micromamba.",
      checkCommand: ["bwa", "--version"],
      availabilityStatus: "missing",
      availabilityManager: null,
      availabilityMessage: "No micromamba, mamba, or conda executable was found.",
    };
    const operator = operatorSummary({
      id: "bwa_mem_align_reads",
      runtime: { envRef: "ngs-bwa" },
    });

    expect(operatorEnvironmentRef(operator)).toBe("ngs-bwa");
    expect(pluginEnvironmentDisplayName(environment)).toBe("BWA");
    expect(pluginEnvironmentStatusColor(environment.availabilityStatus)).toBe("warning");
    expect(pluginEnvironmentRuntimeFileLabel(environment)).toBe("conda.yaml");
  });

  it("surfaces resource-heavy operator runtime profiles", () => {
    const operator = operatorSummary({
      id: "star_align_reads",
      runtime: {
        envRef: "ngs-star",
        resourceProfile: {
          tier: "hpc-recommended",
          localPolicy: "warn",
          recommendedCpu: 32,
          recommendedMemoryGb: 128,
          diskGb: 200,
          notes: ["STAR alignment against whole-genome indices is not recommended on laptops."],
        },
      },
    });

    expect(operatorResourceProfile(operator)).toMatchObject({
      tier: "hpc-recommended",
      localPolicy: "warn",
      recommendedCpu: 32,
      recommendedMemoryGb: 128,
      diskGb: 200,
    });
    expect(operatorResourceProfileLabel(operator)).toBe("HPC recommended");
    expect(operatorResourceProfileSummary(operator)).toContain("32 CPU recommended");
    expect(operatorResourceProfileSummary(operator)).toContain("128 GB RAM recommended");
    expect(operatorShouldWarnBeforeLocalRun(operator)).toBe(true);
    expect(operatorShouldWarnBeforeLocalRun(operatorSummary())).toBe(false);
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
      resourceId: "geo",
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
        resources: [
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
