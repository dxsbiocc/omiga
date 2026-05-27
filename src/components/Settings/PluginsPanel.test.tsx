import type { ReactElement, ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { readFileSync } from "node:fs";
import type {
  HookRuntimeApi,
  RenderedNode,
} from "../../test/__tests__/componentHarness";
import {
  ComponentHarness,
  createHookRuntime,
  findAllNodes,
  installComponentTestWindow,
  textContent,
} from "../../test/__tests__/componentHarness";

const hookRuntimeRef = vi.hoisted(() => ({
  current: null as HookRuntimeApi | null,
}));

const pluginStoreMock = vi.hoisted(() => ({
  state: {
    marketplaceSources: [] as unknown[],
    marketplaceSourceViews: [] as unknown[],
    marketplaces: [] as unknown[],
    operators: [] as unknown[],
    operatorRuns: [] as unknown[],
    retrievalStatuses: [] as unknown[],
    processPoolStatuses: [] as unknown[],
    remoteMarketplaceChecks: [] as unknown[],
    builtinMarketplaceStatus: null as {
      ok: boolean;
      source: string;
      path?: string | null;
      message: string;
    } | null,
    isLoading: false,
    isMutating: false,
    bootstrapInProgress: false,
    error: null as string | null,
    ensureBuiltinMarketplace: vi.fn().mockResolvedValue({
      ok: true,
      source: "github",
      path: "/builtin",
      message: "Built-in marketplace ready.",
    }),
    loadPlugins: vi.fn().mockResolvedValue(undefined),
    loadOperatorRuns: vi.fn().mockResolvedValue(undefined),
    readOperatorRun: vi.fn(),
    readOperatorRunLog: vi.fn(),
    verifyOperatorRun: vi.fn(),
    clearProcessPool: vi.fn().mockResolvedValue(0),
    installPlugin: vi.fn(),
    syncPlugin: vi.fn(),
    checkRemoteMarketplaces: vi.fn(),
    addMarketplaceSource: vi.fn(),
    removeMarketplaceSource: vi.fn(),
    setMarketplaceSourceEnabled: vi.fn(),
    refreshMarketplaceSource: vi.fn(),
    uninstallPlugin: vi.fn(),
    setPluginEnabled: vi.fn(),
    setTemplateEnabled: vi.fn(),
    setRetrievalResourceEnabled: vi.fn(),
    setEnvironmentEnabled: vi.fn(),
    checkPluginEnvironment: vi.fn(),
  },
}));

vi.mock("react", async (importOriginal) => {
  const actual = await importOriginal<typeof import("react")>();
  return {
    ...actual,
    useEffect: (effect: () => void | (() => void), deps?: readonly unknown[]) =>
      hookRuntimeRef.current?.useEffect(effect, deps),
    useMemo: <T,>(factory: () => T, deps?: readonly unknown[]) =>
      hookRuntimeRef.current?.useMemo(factory, deps),
    useState: <T,>(initial: T | (() => T)) =>
      hookRuntimeRef.current?.useState(initial),
  };
});

vi.mock("@tauri-apps/plugin-opener", () => ({
  revealItemInDir: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("../../state/pluginStore", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../state/pluginStore")>();
  const usePluginStore = (() => pluginStoreMock.state) as typeof actual.usePluginStore;
  usePluginStore.setState = (partial: unknown) => {
    const next =
      typeof partial === "function"
        ? (partial as (state: typeof pluginStoreMock.state) => Partial<typeof pluginStoreMock.state>)(
            pluginStoreMock.state,
          )
        : partial;
    if (next && typeof next === "object") {
      Object.assign(pluginStoreMock.state, next);
    }
  };
  return {
    ...actual,
    usePluginStore,
  };
});

vi.mock("../../state/chatComposerStore", () => ({
  useChatComposerStore: <T,>(selector: (state: {
    environment: string;
    sshServer: string | null;
    sandboxBackend: string;
  }) => T): T =>
    selector({
      environment: "local",
      sshServer: null,
      sandboxBackend: "docker",
    }),
}));

vi.mock("../../state/sessionStore", () => ({
  useSessionStore: <T,>(selector: (state: { currentSession: { id: string } | null }) => T): T =>
    selector({ currentSession: null }),
}));

vi.mock("@mui/material", async () => {
  const React = await import("react");
  const { createMuiMaterialMock } = await import("../../test/__tests__/muiMocks");
  const base = createMuiMaterialMock();
  const passthrough = (type: string) => {
    const Component = ({ children, ...props }: Record<string, unknown> & { children?: ReactNode }) =>
      React.createElement(type, props, children);
    Component.displayName = `Mock${type}`;
    return Component;
  };
  const Snackbar = ({
    children,
    open,
    ...props
  }: Record<string, unknown> & { children?: ReactNode; open?: boolean }) =>
    open ? React.createElement("snackbar", props, children) : null;
  const Switch = ({
    inputProps,
    checked,
    ...props
  }: Record<string, unknown> & { inputProps?: Record<string, unknown>; checked?: boolean }) =>
    React.createElement("input", {
      ...props,
      ...inputProps,
      type: "checkbox",
      checked,
    });

  return {
    ...base,
    Accordion: passthrough("accordion"),
    AccordionDetails: passthrough("accordion-details"),
    AccordionSummary: passthrough("accordion-summary"),
    Collapse: passthrough("collapse"),
    InputAdornment: passthrough("input-adornment"),
    Portal: passthrough("portal"),
    Snackbar,
    Switch,
    ToggleButton: passthrough("toggle-button"),
    ToggleButtonGroup: passthrough("toggle-button-group"),
  };
});

vi.mock("@mui/material/styles", async () => {
  const React = await import("react");
  const theme = {
    palette: {
      mode: "light",
      common: { black: "#000", white: "#fff" },
      primary: { main: "#1976d2" },
      secondary: { main: "#7b1fa2" },
      success: { main: "#2e7d32" },
      warning: { main: "#ed6c02" },
      error: { main: "#d32f2f" },
      info: { main: "#0288d1" },
      text: { primary: "#111", secondary: "#555" },
      background: { paper: "#fff", default: "#fafafa" },
      action: { hover: "#f5f5f5" },
      divider: "#ddd",
    },
    shadows: Array.from({ length: 25 }, () => "none"),
    zIndex: { drawer: 1200, tooltip: 1500 },
  };
  return {
    __esModule: true,
    ThemeProvider: ({ children }: { children?: ReactNode }) =>
      React.createElement("theme-provider", {}, children),
    alpha: (color: string, value: number) => `${color}/${value}`,
    createTheme: () => theme,
    useTheme: () => theme,
  };
});

vi.mock("@mui/icons-material", async () => {
  const { createIconMock } = await import("../../test/__tests__/muiMocks");
  return createIconMock([
    "AddRounded",
    "ClearRounded",
    "CloseRounded",
    "ContentCopyRounded",
    "DeleteOutlineRounded",
    "DescriptionOutlined",
    "ExtensionRounded",
    "ExpandMoreRounded",
    "KeyboardArrowDownRounded",
    "PublishedWithChangesRounded",
    "RefreshRounded",
    "SearchRounded",
    "SettingsRounded",
    "SyncRounded",
    "TroubleshootRounded",
  ]);
});
import {
  PluginsPanel,
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
  pluginHasRemoteMarketplaceUpdate,
  pluginRuntimeSummary,
  processPoolStatusDiagnostic,
  remoteMarketplaceChangedPluginNames,
  remoteMarketplaceCheckSignature,
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
  MarketplaceRemoteCheckResult,
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

function remoteMarketplaceCheck(
  overrides: Partial<MarketplaceRemoteCheckResult> = {},
): MarketplaceRemoteCheckResult {
  return {
    name: "omiga-curated",
    path: "/workspace/omiga-plugins/marketplace.json",
    remote: {
      url: "https://raw.githubusercontent.com/dxsbiocc/omiga-plugins/main/marketplace.json",
    },
    state: "updateAvailable",
    label: "Remote update available",
    message: "Remote marketplace differs.",
    localDigest: "sha256:local",
    remoteDigest: "sha256:remote",
    remotePluginCount: 2,
    changedPlugins: ["alignment", "transcriptomics"],
    checkedAt: "2026-05-13T00:00:00Z",
    ...overrides,
  };
}

const resetPanelStore = () => {
  pluginStoreMock.state.marketplaceSources = [];
  pluginStoreMock.state.marketplaceSourceViews = [];
  pluginStoreMock.state.marketplaces = [];
  pluginStoreMock.state.operators = [];
  pluginStoreMock.state.operatorRuns = [];
  pluginStoreMock.state.retrievalStatuses = [];
  pluginStoreMock.state.processPoolStatuses = [];
  pluginStoreMock.state.remoteMarketplaceChecks = [];
  pluginStoreMock.state.builtinMarketplaceStatus = null;
  pluginStoreMock.state.isLoading = false;
  pluginStoreMock.state.isMutating = false;
  pluginStoreMock.state.bootstrapInProgress = false;
  pluginStoreMock.state.error = null;
  pluginStoreMock.state.ensureBuiltinMarketplace = vi.fn().mockResolvedValue({
    ok: true,
    source: "github",
    path: "/builtin",
    message: "Built-in marketplace ready.",
  });
  pluginStoreMock.state.loadPlugins = vi.fn().mockResolvedValue(undefined);
  pluginStoreMock.state.loadOperatorRuns = vi.fn().mockResolvedValue(undefined);
  pluginStoreMock.state.readOperatorRun = vi.fn();
  pluginStoreMock.state.readOperatorRunLog = vi.fn();
  pluginStoreMock.state.verifyOperatorRun = vi.fn();
  pluginStoreMock.state.clearProcessPool = vi.fn().mockResolvedValue(0);
  pluginStoreMock.state.installPlugin = vi.fn();
  pluginStoreMock.state.syncPlugin = vi.fn();
  pluginStoreMock.state.checkRemoteMarketplaces = vi.fn();
  pluginStoreMock.state.addMarketplaceSource = vi.fn().mockResolvedValue({
    id: "source-local",
    kind: "local",
    location: "/tmp/omiga-plugins",
    label: null,
    enabled: true,
    addedAt: "2026-05-27T00:00:00Z",
  });
  pluginStoreMock.state.removeMarketplaceSource = vi.fn().mockResolvedValue(undefined);
  pluginStoreMock.state.setMarketplaceSourceEnabled = vi.fn().mockResolvedValue(undefined);
  pluginStoreMock.state.refreshMarketplaceSource = vi.fn().mockResolvedValue({
    id: "source-local",
    ok: true,
    message: "Refreshed",
    marketplaceName: "omiga-curated",
    pluginCount: 1,
  });
  pluginStoreMock.state.uninstallPlugin = vi.fn();
  pluginStoreMock.state.setPluginEnabled = vi.fn();
  pluginStoreMock.state.setTemplateEnabled = vi.fn();
  pluginStoreMock.state.setRetrievalResourceEnabled = vi.fn();
  pluginStoreMock.state.setEnvironmentEnabled = vi.fn();
  pluginStoreMock.state.checkPluginEnvironment = vi.fn();
};

const createPanelHarness = () => {
  installComponentTestWindow();
  const runtime = createHookRuntime();
  hookRuntimeRef.current = runtime;
  const harness = new ComponentHarness(
    runtime,
    (): ReactElement => <PluginsPanel projectPath="/project" />,
  );
  harness.render();
  return harness;
};

const getNodeByAriaLabel = (
  harness: ComponentHarness,
  label: string,
): RenderedNode => {
  const node = findAllNodes(
    harness.tree,
    (candidate) => candidate.props["aria-label"] === label,
  )[0];
  if (!node) throw new Error(`Unable to find node labelled "${label}".`);
  return node;
};

const getButtonByText = (
  harness: ComponentHarness,
  label: string,
): RenderedNode => {
  const button = findAllNodes(
    harness.tree,
    (candidate) =>
      candidate.type === "button" && textContent(candidate).includes(label),
  )[0];
  if (!button) throw new Error(`Unable to find button containing "${label}".`);
  return button;
};

beforeEach(() => {
  resetPanelStore();
});

afterEach(() => {
  hookRuntimeRef.current?.cleanup();
  hookRuntimeRef.current = null;
  vi.clearAllMocks();
});

describe("PluginsPanel marketplace sources UI", () => {
  it("renders the sources section and invokes the add source action", () => {
    const harness = createPanelHarness();

    expect(textContent(harness.tree)).toContain("Marketplace Sources");
    expect(textContent(harness.tree)).toContain("Add");

    harness.change(getNodeByAriaLabel(harness, "Local path"), "/tmp/omiga-plugins");
    harness.click(getButtonByText(harness, "Add"));

    expect(pluginStoreMock.state.addMarketplaceSource).toHaveBeenCalledWith(
      "local",
      "/tmp/omiga-plugins",
      undefined,
      "/project",
    );
  });

  it("renders a built-in source without remove controls", () => {
    pluginStoreMock.state.marketplaceSourceViews = [
      {
        id: "builtin",
        kind: "builtin",
        location: "/workspace/omiga-plugins",
        label: "Built-in Marketplace",
        enabled: true,
        removable: false,
        addedAt: null,
      },
    ];

    const harness = createPanelHarness();

    expect(textContent(harness.tree)).toContain("Built-in Marketplace");
    expect(textContent(harness.tree)).toContain("Always enabled");
    expect(textContent(harness.tree)).toContain("Refresh");
    expect(
      findAllNodes(
        harness.tree,
        (candidate) =>
          candidate.props["aria-label"] ===
          "Remove marketplace source Built-in Marketplace",
      ),
    ).toHaveLength(0);
  });

  it("shows built-in bootstrap failure details and retries bootstrap", async () => {
    pluginStoreMock.state.marketplaceSourceViews = [
      {
        id: "builtin",
        kind: "builtin",
        location: "/workspace/omiga-plugins",
        label: "Built-in Marketplace",
        enabled: true,
        removable: false,
        addedAt: null,
      },
    ];
    pluginStoreMock.state.builtinMarketplaceStatus = {
      ok: false,
      source: "github",
      path: null,
      message: "Install git or connect to the network, then retry.",
    };

    const harness = createPanelHarness();

    expect(textContent(harness.tree)).toContain(
      "Install git or connect to the network, then retry.",
    );
    await harness.click(getNodeByAriaLabel(
      harness,
      "Refresh marketplace source Built-in Marketplace",
    ));
    await harness.click(getButtonByText(harness, "Retry"));

    expect(pluginStoreMock.state.ensureBuiltinMarketplace).toHaveBeenCalledTimes(2);
    expect(pluginStoreMock.state.ensureBuiltinMarketplace).toHaveBeenCalledWith("/project");
  });

  it("disables built-in source refresh while bootstrap is in progress", () => {
    pluginStoreMock.state.bootstrapInProgress = true;
    pluginStoreMock.state.marketplaceSourceViews = [
      {
        id: "builtin",
        kind: "builtin",
        location: "/workspace/omiga-plugins",
        label: "Built-in Marketplace",
        enabled: true,
        removable: false,
        addedAt: null,
      },
    ];

    const harness = createPanelHarness();
    const refreshButton = getNodeByAriaLabel(
      harness,
      "Refresh marketplace source Built-in Marketplace",
    );

    expect(refreshButton.props.disabled).toBe(true);
    expect(textContent(harness.tree)).toContain("Refreshing built-in marketplace");
  });
});

describe("PluginsPanel diagnostics helpers", () => {
  it("removes the duplicate global operator registration surface", () => {
    const source = readFileSync(new URL("./PluginsPanel.tsx", import.meta.url), "utf8");

    expect(source).not.toContain("OperatorCatalogSection");
    expect(source).not.toContain("SHOW_PLUGIN_DEVELOPER_DIAGNOSTICS");
    expect(source).not.toContain("onOperatorRegistrationChange");
    expect(source).not.toContain("New Script Operator");
  });

  it("keeps operator exposure plugin-owned instead of manually registered", () => {
    const source = readFileSync(new URL("./PluginsPanel.tsx", import.meta.url), "utf8");

    expect(source).toContain("operations exposed");
    expect(source).toContain("Exposed by plugin");
    expect(source).toContain("Exposed");
    expect(source).toContain('plugin.enabled ? "Disable" : "Enable"');
    expect(source).toContain('plugin.enabled ? "Enabled" : "Disabled"');
    expect(source).toContain("Operator programs are exposed automatically while this plugin is enabled");
    expect(source).toContain("operation categories come from the plugin manifest");
    expect(source).not.toContain("Agent tools");
    expect(source).not.toContain("Register to run smoke test");
    expect(source).not.toContain("Register");
    expect(source).not.toContain("Unregister");
    expect(source).not.toContain("Operators are plugin-defined tools");
  });

  it("keeps run history explained without another operator catalog", () => {
    const source = readFileSync(new URL("./PluginsPanel.tsx", import.meta.url), "utf8");
    const timelineSource = readFileSync(new URL("./OperatorRunsTimeline.tsx", import.meta.url), "utf8");

    expect(source).toContain("<OperatorRunsTimeline");
    expect(timelineSource).toContain("Operator run history");
    expect(timelineSource).toContain("Chronological history for smoke tests");
    expect(timelineSource).not.toContain('position: "sticky"');
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

  it("keeps plugin feedback visible above modal dialogs", () => {
    const source = readFileSync(new URL("./PluginsPanel.tsx", import.meta.url), "utf8");

    expect(source).toContain("<Portal>");
    expect(source).toContain("t.zIndex.tooltip + 1");
    expect(source).toContain("Rendered through Portal and lifted above Dialog + Backdrop");
  });

  it("derives bioinformatics subgroups from marketplace paths instead of hard-coded plugin content", () => {
    const source = readFileSync(new URL("./PluginsPanel.tsx", import.meta.url), "utf8");

    expect(source).toContain("pluginMarketplaceTaxonomySegments");
    expect(source).toContain("marketplace directory taxonomy");
    expect(source).not.toContain("NGS ·");
    expect(source).not.toContain("ngs-sequence-processing");
    expect(source).not.toContain("ngs-quality-control");
  });

  it("summarizes remote marketplace updates as durable UI state", () => {
    const checks = [
      remoteMarketplaceCheck({ changedPlugins: ["alignment", "transcriptomics", "alignment"] }),
      remoteMarketplaceCheck({
        name: "secondary",
        path: "/secondary.json",
        changedPlugins: ["r-visualization"],
      }),
      remoteMarketplaceCheck({
        name: "stable",
        path: "/stable.json",
        state: "upToDate",
        changedPlugins: ["ignored"],
      }),
    ];

    const changedPlugins = remoteMarketplaceChangedPluginNames(checks);

    expect([...changedPlugins].sort()).toEqual([
      "alignment",
      "r-visualization",
      "transcriptomics",
    ]);
    expect(
      pluginHasRemoteMarketplaceUpdate(
        pluginSummary({
          id: "alignment@omiga-curated",
          name: "alignment",
        }),
        changedPlugins,
      ),
    ).toBe(true);
    expect(
      pluginHasRemoteMarketplaceUpdate(
        pluginSummary({
          id: "unrelated@omiga-curated",
          name: "unrelated",
        }),
        changedPlugins,
      ),
    ).toBe(false);
  });

  it("builds stable remote marketplace check signatures to suppress repeated toasts", () => {
    const left = [
      remoteMarketplaceCheck({ name: "b", changedPlugins: ["star", "bwa"] }),
      remoteMarketplaceCheck({ name: "a", path: "/a.json", changedPlugins: ["transcriptomics"] }),
    ];
    const right = [
      remoteMarketplaceCheck({ name: "a", path: "/a.json", changedPlugins: ["transcriptomics"] }),
      remoteMarketplaceCheck({ name: "b", changedPlugins: ["bwa", "star"] }),
    ];

    expect(remoteMarketplaceCheckSignature(left)).toBe(remoteMarketplaceCheckSignature(right));
    expect(
      remoteMarketplaceCheckSignature([
        remoteMarketplaceCheck({ name: "b", changedPlugins: ["bwa", "star", "hisat2"] }),
      ]),
    ).not.toBe(remoteMarketplaceCheckSignature([remoteMarketplaceCheck({ name: "b", changedPlugins: ["bwa", "star"] })]));
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
      sourcePath: "/plugins/bioinformatics/ngs/alignment",
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
      sourcePath: "/plugins/bioinformatics/ngs/transcriptomics",
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
      sourcePath: "/plugins/bioinformatics/ngs/alignment",
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
    ).toEqual(["NGS"]);
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

  it("does not let domain keywords move resources or visualization into analysis/bioinformatics", () => {
    const visualizationWithGeneTemplates = pluginSummary({
      id: "visualization-r@omiga-curated",
      name: "visualization-r",
      interface: {
        displayName: "R Visualization",
        shortDescription: "Publication figure templates",
        longDescription: "Includes enrichment dot plots, gene terms, volcano plots, and heatmaps.",
        developerName: null,
        category: "Visualization",
        capabilities: ["Rscript", "Static Figures", "Publication Figures"],
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
    const pathwayDatabaseResource = pluginSummary({
      id: "resource-pathways@omiga-curated",
      name: "resource-pathways",
      retrieval: {
        protocolVersion: 1,
        resources: [
          {
            id: "reactome",
            category: "knowledge",
            label: "Reactome",
            description: "Pathway and gene-set lookup.",
            subcategories: ["pathway"],
            capabilities: ["search", "query", "fetch"],
            requiredCredentialRefs: [],
            optionalCredentialRefs: [],
            defaultEnabled: false,
            replacesBuiltin: true,
          },
        ],
      },
      interface: {
        displayName: "Pathway Databases",
        shortDescription: "4 Knowledge routes",
        longDescription: "Pathway analysis and annotation database retrieval.",
        developerName: null,
        category: "Analysis",
        capabilities: ["Knowledge", "Analysis", "Retrieval"],
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
    const ncbiProviderResource = pluginSummary({
      id: "resource-ncbi@omiga-curated",
      name: "resource-ncbi",
      retrieval: {
        protocolVersion: 1,
        resources: [
          {
            id: "ncbi_gene",
            category: "knowledge",
            label: "NCBI Gene",
            description: "Gene, variant, and assembly metadata lookup.",
            subcategories: ["gene", "variant"],
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
            description: "Gene expression datasets.",
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
        shortDescription: "PubMed, GEO, BioSample, Datasets, and Gene retrieval",
        longDescription: "Aggregates literature, datasets, gene, variant, and genome assembly routes.",
        developerName: null,
        category: "Retrieval",
        capabilities: ["Provider", "Dataset", "Knowledge", "Retrieval"],
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

    expect(pluginCatalogGroupId(visualizationWithGeneTemplates)).toBe("visualization");
    expect(pluginCatalogGroupId(pathwayDatabaseResource)).toBe("resource");
    expect(pluginCatalogGroupId(ncbiProviderResource)).toBe("resource");
    expect(
      groupPluginsByCatalogSection("resource", [pathwayDatabaseResource, ncbiProviderResource]).map(
        (section) => section.title,
      ),
    ).toEqual(["Provider resources", "Knowledge resources"]);
  });

  it("groups NGS bioinformatics plugins by marketplace directory taxonomy instead of hard-coded stages", () => {
    const sequenceProcessingPlugin = pluginSummary({
      id: "seqtk-convert@omiga-curated",
      name: "seqtk-convert",
      sourcePath: "/plugins/bioinformatics/ngs/sequence-processing",
      interface: {
        displayName: "Seqtk Convert",
        shortDescription: "Convert FASTQ and FASTA reads for downstream analysis.",
        longDescription: null,
        developerName: null,
        category: "Bioinformatics",
        capabilities: ["NGS"],
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
    const qualityControlPlugin = pluginSummary({
      id: "multiqc-reports@omiga-curated",
      name: "multiqc-reports",
      sourcePath: "/plugins/bioinformatics/ngs/quality-control",
      interface: {
        displayName: "MultiQC Reports",
        shortDescription: "Aggregate FastQC and fqchk quality control summaries.",
        longDescription: null,
        developerName: null,
        category: "Bioinformatics",
        capabilities: ["NGS"],
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
    const alignmentPlugin = pluginSummary({
      id: "alignment-bundle@omiga-curated",
      name: "alignment-bundle",
      sourcePath: "/plugins/bioinformatics/ngs/alignment",
      interface: {
        displayName: "Alignment",
        shortDescription: "BWA, Bowtie2, STAR, and HISAT2 alignment",
        longDescription: null,
        developerName: null,
        category: "Bioinformatics",
        capabilities: ["NGS", "Alignment", "SAM/BAM"],
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
    const quantificationPlugin = pluginSummary({
      id: "salmon-quant@omiga-curated",
      name: "salmon-quant",
      sourcePath: "/plugins/bioinformatics/ngs/quantification",
      interface: {
        displayName: "Salmon Quant",
        shortDescription: "Transcript abundance quantification with Salmon.",
        longDescription: null,
        developerName: null,
        category: "Bioinformatics",
        capabilities: ["NGS", "Quantification"],
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
    const variantCallingPlugin = pluginSummary({
      id: "deepvariant-caller@omiga-curated",
      name: "deepvariant-caller",
      sourcePath: "/plugins/bioinformatics/ngs/variant-calling",
      interface: {
        displayName: "DeepVariant Caller",
        shortDescription: null,
        longDescription: "DeepVariant variant calling for germline small variants.",
        developerName: null,
        category: "Bioinformatics",
        capabilities: ["NGS"],
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
    const fusionPlugin = pluginSummary({
      id: "star-fusion@omiga-curated",
      name: "star-fusion",
      sourcePath: "/plugins/bioinformatics/ngs/fusion-sv",
      interface: {
        displayName: "STAR-Fusion",
        shortDescription: "Detect fusion transcripts and structural variants.",
        longDescription: null,
        developerName: null,
        category: "Bioinformatics",
        capabilities: ["NGS"],
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
    const copyNumberVariationPlugin = pluginSummary({
      id: "somatic-workflow@omiga-curated",
      name: "somatic-workflow",
      sourcePath: "/plugins/bioinformatics/ngs/copy-number-variation",
      interface: {
        displayName: "Somatic Workflow",
        shortDescription: "Somatic DNA analysis workflow.",
        longDescription: null,
        developerName: null,
        category: "Bioinformatics",
        capabilities: ["NGS"],
        websiteUrl: null,
        privacyPolicyUrl: null,
        termsOfServiceUrl: null,
        defaultPrompt: [],
        brandColor: null,
        composerIcon: null,
        logo: null,
        screenshots: [],
      },
      operators: [
        operatorSummary({
          id: "cnvkit",
          name: "CNVkit",
          description: "Copy-number segmentation",
          tags: ["cnvkit", "copy number"],
        }),
      ],
    });
    const assemblyAnnotationPlugin = pluginSummary({
      id: "assembly-annotation@omiga-curated",
      name: "assembly-annotation",
      sourcePath: "/plugins/bioinformatics/ngs/assembly-annotation",
      interface: {
        displayName: "Assembly Annotation",
        shortDescription: null,
        longDescription: "Genome assembly and annotation with SPAdes and Prokka.",
        developerName: null,
        category: "Bioinformatics",
        capabilities: ["NGS"],
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

    const sections = groupPluginsByCatalogSection("bioinformatics", [
      sequenceProcessingPlugin,
      qualityControlPlugin,
      alignmentPlugin,
      quantificationPlugin,
      variantCallingPlugin,
      fusionPlugin,
      copyNumberVariationPlugin,
      assemblyAnnotationPlugin,
    ]);

    expect(sections.map((section) => section.title)).toEqual(["NGS"]);
    expect(sections[0]?.plugins.map(displayName)).toEqual([
      "Seqtk Convert",
      "MultiQC Reports",
      "Alignment",
      "Salmon Quant",
      "DeepVariant Caller",
      "STAR-Fusion",
      "Somatic Workflow",
      "Assembly Annotation",
    ]);
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
        name: "c-program-adapter",
        interface: {
          displayName: "C Program",
          shortDescription: "Adapter for a C-based command-line program",
          longDescription: null,
          developerName: null,
          category: "Operator",
          capabilities: ["Operator", "C"],
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
