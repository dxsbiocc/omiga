import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import { extractErrorMessage } from "../utils/errorMessage";

export type PluginInstallPolicy = "NOT_AVAILABLE" | "AVAILABLE" | "INSTALLED_BY_DEFAULT";
export type PluginAuthPolicy = "ON_INSTALL" | "ON_USE";

export interface PluginInterface {
  displayName?: string | null;
  shortDescription?: string | null;
  longDescription?: string | null;
  developerName?: string | null;
  category?: string | null;
  capabilities: string[];
  websiteUrl?: string | null;
  privacyPolicyUrl?: string | null;
  termsOfServiceUrl?: string | null;
  defaultPrompt: string[];
  brandColor?: string | null;
  composerIcon?: string | null;
  logo?: string | null;
  screenshots: string[];
}

export interface PluginRetrievalSourceSummary {
  id: string;
  category: string;
  label: string;
  description: string;
  subcategories: string[];
  capabilities: string[];
  requiredCredentialRefs: string[];
  optionalCredentialRefs: string[];
  defaultEnabled: boolean;
  replacesBuiltin: boolean;
}

export interface PluginRetrievalSummary {
  protocolVersion: number;
  sources: PluginRetrievalSourceSummary[];
}

export interface PluginSummary {
  id: string;
  name: string;
  marketplaceName: string;
  marketplacePath: string;
  sourcePath: string;
  installedPath?: string | null;
  installed: boolean;
  enabled: boolean;
  installPolicy: PluginInstallPolicy;
  authPolicy: PluginAuthPolicy;
  interface?: PluginInterface | null;
  retrieval?: PluginRetrievalSummary | null;
}

export interface PluginMarketplaceEntry {
  name: string;
  path: string;
  interface?: { displayName?: string | null } | null;
  plugins: PluginSummary[];
}

export interface PluginInstallResult {
  pluginId: string;
  installedPath: string;
  authPolicy: PluginAuthPolicy;
}

export type PluginRetrievalLifecycleState = "healthy" | "degraded" | "quarantined";

export interface PluginRetrievalRouteStatus {
  pluginId: string;
  category: string;
  sourceId: string;
  route: string;
  state: PluginRetrievalLifecycleState;
  quarantined: boolean;
  consecutiveFailures: number;
  remainingMs: number;
  lastError?: string | null;
}

export interface PluginProcessPoolRouteStatus {
  pluginId: string;
  category: string;
  sourceId: string;
  route: string;
  pluginRoot: string;
  remainingMs: number;
}

export interface OperatorSummary {
  id: string;
  version: string;
  name?: string | null;
  description?: string | null;
  sourcePlugin: string;
  manifestPath: string;
  smokeTests?: OperatorSmokeTestSpec[];
  enabledAliases: string[];
  exposed: boolean;
  unavailableReason?: string | null;
}

export interface OperatorSmokeTestSpec {
  id: string;
  name?: string | null;
  description?: string | null;
  arguments: OperatorInvocationArguments;
}

export interface OperatorCatalogResponse {
  registryPath: string;
  operators: OperatorSummary[];
  diagnostics: OperatorManifestDiagnostic[];
}

export interface OperatorRegistryUpdate {
  alias: string;
  operatorId?: string | null;
  sourcePlugin?: string | null;
  version?: string | null;
  enabled: boolean;
}

export interface OperatorManifestDiagnostic {
  sourcePlugin: string;
  manifestPath: string;
  severity: string;
  message: string;
}

export interface OperatorInvocationArguments {
  inputs?: Record<string, unknown>;
  params?: Record<string, unknown>;
  resources?: Record<string, unknown>;
}

export interface OperatorRunResponse {
  ok: boolean;
  result: Record<string, unknown>;
}

export interface OperatorRunContext {
  kind?: string | null;
  smokeTestId?: string | null;
  smokeTestName?: string | null;
}

export interface OperatorRunSummary {
  runId: string;
  status: string;
  location: string;
  operatorAlias?: string | null;
  operatorId?: string | null;
  operatorVersion?: string | null;
  sourcePlugin?: string | null;
  runKind?: string | null;
  smokeTestId?: string | null;
  smokeTestName?: string | null;
  runDir: string;
  updatedAt?: string | null;
  provenancePath?: string | null;
  outputCount: number;
  structuredOutputCount?: number;
  errorMessage?: string | null;
  errorKind?: string | null;
  retryable?: boolean | null;
  suggestedAction?: string | null;
  stdoutTail?: string | null;
  stderrTail?: string | null;
  cacheKey?: string | null;
  cacheHit?: boolean | null;
  cacheSourceRunId?: string | null;
  cacheSourceRunDir?: string | null;
}

export interface OperatorExecutionSurfaceArgs {
  sessionId?: string | null;
  executionEnvironment?: "local" | "ssh" | "sandbox" | string;
  sshServer?: string | null;
  sandboxBackend?: string | null;
}

export interface OperatorRunDetail {
  runId: string;
  location: string;
  runDir: string;
  sourcePath: string;
  document: Record<string, unknown>;
}

export interface OperatorRunLog {
  runId: string;
  location: string;
  logName: string;
  path: string;
  content: string;
}

export interface OperatorRunCheck {
  name: string;
  ok: boolean;
  severity: string;
  message: string;
  path?: string | null;
}

export interface OperatorRunVerification {
  runId: string;
  location: string;
  runDir: string;
  ok: boolean;
  checks: OperatorRunCheck[];
}

export interface OperatorRunCleanupRequest {
  dryRun: boolean;
  keepLatest?: number | null;
  maxAgeDays?: number | null;
  includeCacheHits: boolean;
  includeFailed: boolean;
  includeSucceeded: boolean;
  limit?: number | null;
  operatorAlias?: string | null;
  operatorId?: string | null;
  operatorVersion?: string | null;
  sourcePlugin?: string | null;
}

export interface OperatorRunCleanupCandidate {
  runId: string;
  status: string;
  location: string;
  runDir: string;
  updatedAt?: string | null;
  cacheHit?: boolean | null;
  cacheSourceRunId?: string | null;
  outputCount: number;
  reason: string;
  estimatedBytes?: number | null;
  deleted: boolean;
  error?: string | null;
}

export interface OperatorRunCleanupResult {
  dryRun: boolean;
  location: string;
  runsRoot: string;
  scannedCount: number;
  matchedCount: number;
  deletedCount: number;
  skippedCount: number;
  estimatedBytes?: number | null;
  candidates: OperatorRunCleanupCandidate[];
}

export const RETRIEVAL_PLUGIN_PROTOCOL_DOC_PATH =
  "docs/retrieval-plugin-protocol.md";

export interface PluginDiagnosticsPayload {
  protocolDocPath: string;
  plugin: {
    id: string;
    name: string;
    marketplaceName: string;
    sourcePath: string;
    installedPath?: string | null;
    installed: boolean;
    enabled: boolean;
    retrieval?: PluginRetrievalSummary | null;
  };
  retrievalRoutes: PluginRetrievalRouteStatus[];
  pooledProcesses: PluginProcessPoolRouteStatus[];
  notes: string[];
}

export interface RetrievalRuntimeDiagnosticsPayload {
  protocolDocPath: string;
  summary: {
    pluginCount: number;
    routeCount: number;
    healthyRouteCount: number;
    degradedRouteCount: number;
    quarantinedRouteCount: number;
    pooledProcessCount: number;
    unknownPluginCount: number;
  };
  plugins: Array<{
    id: string;
    name: string;
    displayName?: string | null;
    marketplaceName: string;
    sourcePath: string;
    installedPath?: string | null;
    installed: boolean;
    enabled: boolean;
    declaredRouteCount: number;
  }>;
  unknownPluginIds: string[];
  retrievalRoutes: PluginRetrievalRouteStatus[];
  pooledProcesses: PluginProcessPoolRouteStatus[];
  notes: string[];
}

interface PluginState {
  marketplaces: PluginMarketplaceEntry[];
  operators: OperatorSummary[];
  operatorDiagnostics: OperatorManifestDiagnostic[];
  operatorRegistryPath: string | null;
  operatorRuns: OperatorRunSummary[];
  retrievalStatuses: PluginRetrievalRouteStatus[];
  processPoolStatuses: PluginProcessPoolRouteStatus[];
  isLoading: boolean;
  isMutating: boolean;
  error: string | null;
  loadPlugins: (projectRoot?: string, surface?: OperatorExecutionSurfaceArgs) => Promise<void>;
  loadOperators: () => Promise<void>;
  loadOperatorRuns: (projectRoot?: string, surface?: OperatorExecutionSurfaceArgs) => Promise<void>;
  readOperatorRun: (
    runId: string,
    projectRoot?: string,
    surface?: OperatorExecutionSurfaceArgs,
  ) => Promise<OperatorRunDetail>;
  readOperatorRunLog: (
    runId: string,
    logName: "stdout" | "stderr",
    projectRoot?: string,
    surface?: OperatorExecutionSurfaceArgs,
  ) => Promise<OperatorRunLog>;
  verifyOperatorRun: (
    runId: string,
    projectRoot?: string,
    surface?: OperatorExecutionSurfaceArgs,
  ) => Promise<OperatorRunVerification>;
  cleanupOperatorRuns: (
    request: OperatorRunCleanupRequest,
    projectRoot?: string,
    surface?: OperatorExecutionSurfaceArgs,
  ) => Promise<OperatorRunCleanupResult>;
  loadRetrievalStatuses: (projectRoot?: string) => Promise<void>;
  loadProcessPoolStatuses: (projectRoot?: string) => Promise<void>;
  clearProcessPool: (projectRoot?: string) => Promise<number>;
  installPlugin: (plugin: PluginSummary, projectRoot?: string) => Promise<PluginInstallResult>;
  uninstallPlugin: (pluginId: string, projectRoot?: string) => Promise<void>;
  setPluginEnabled: (pluginId: string, enabled: boolean, projectRoot?: string) => Promise<void>;
  setOperatorEnabled: (
    update: OperatorRegistryUpdate,
    projectRoot?: string,
    surface?: OperatorExecutionSurfaceArgs,
  ) => Promise<void>;
  runOperator: (
    alias: string,
    invocation: OperatorInvocationArguments,
    projectRoot?: string,
    surface?: OperatorExecutionSurfaceArgs,
    runContext?: OperatorRunContext,
  ) => Promise<OperatorRunResponse>;
}

export function flattenMarketplacePlugins(
  marketplaces: PluginMarketplaceEntry[],
): PluginSummary[] {
  const seen = new Set<string>();
  const plugins: PluginSummary[] = [];
  for (const marketplace of marketplaces) {
    for (const plugin of marketplace.plugins) {
      if (seen.has(plugin.id)) continue;
      seen.add(plugin.id);
      plugins.push(plugin);
    }
  }
  return plugins;
}

export function updatePluginEnabledInMarketplaces(
  marketplaces: PluginMarketplaceEntry[],
  pluginId: string,
  enabled: boolean,
): PluginMarketplaceEntry[] {
  let changed = false;
  const next = marketplaces.map((marketplace) => {
    let marketplaceChanged = false;
    const plugins = marketplace.plugins.map((plugin) => {
      if (plugin.id !== pluginId || plugin.enabled === enabled) return plugin;
      changed = true;
      marketplaceChanged = true;
      return { ...plugin, enabled };
    });
    return marketplaceChanged ? { ...marketplace, plugins } : marketplace;
  });
  return changed ? next : marketplaces;
}

export function updateOperatorEnabledInCatalog(
  operators: OperatorSummary[],
  update: OperatorRegistryUpdate,
): OperatorSummary[] {
  const alias = update.alias.trim();
  if (!alias) return operators;

  let changed = false;
  const next = operators.map((operator) => {
    if (update.operatorId?.trim() && operator.id !== update.operatorId) return operator;
    if (update.sourcePlugin?.trim() && operator.sourcePlugin !== update.sourcePlugin) return operator;
    if (update.version?.trim() && operator.version !== update.version) return operator;
    if (
      !update.operatorId?.trim() &&
      operator.id !== alias &&
      !operator.enabledAliases.includes(alias)
    ) {
      return operator;
    }

    const aliases = operator.enabledAliases
      .map((value) => value.trim())
      .filter(Boolean);
    const nextAliases = update.enabled
      ? Array.from(new Set([...aliases, alias]))
      : aliases.filter((value) => value !== alias);
    const nextExposed = nextAliases.length > 0;
    if (
      nextAliases.length === aliases.length &&
      nextAliases.every((value, index) => value === aliases[index]) &&
      operator.exposed === nextExposed
    ) {
      return operator;
    }
    changed = true;
    return {
      ...operator,
      enabledAliases: nextAliases,
      exposed: nextExposed,
    };
  });
  return changed ? next : operators;
}

export function buildPluginDiagnostics(
  plugin: PluginSummary,
  retrievalRoutes: PluginRetrievalRouteStatus[] = [],
  pooledProcesses: PluginProcessPoolRouteStatus[] = [],
): string {
  const payload: PluginDiagnosticsPayload = {
    protocolDocPath: RETRIEVAL_PLUGIN_PROTOCOL_DOC_PATH,
    plugin: {
      id: plugin.id,
      name: plugin.name,
      marketplaceName: plugin.marketplaceName,
      sourcePath: plugin.sourcePath,
      installedPath: plugin.installedPath,
      installed: plugin.installed,
      enabled: plugin.enabled,
      retrieval: plugin.retrieval,
    },
    retrievalRoutes,
    pooledProcesses,
    notes: [
      "Retrieval plugins run as local JSONL child processes.",
      "No credential values are included in this diagnostics payload.",
      "See protocolDocPath for the search/query/fetch route contract.",
    ],
  };

  return JSON.stringify(payload, null, 2);
}

export function buildRetrievalRuntimeDiagnostics(
  plugins: PluginSummary[],
  retrievalRoutes: PluginRetrievalRouteStatus[] = [],
  pooledProcesses: PluginProcessPoolRouteStatus[] = [],
): string {
  const retrievalPluginIds = new Set(
    plugins
      .filter((plugin) => Boolean(plugin.retrieval?.sources.length))
      .map((plugin) => plugin.id),
  );
  for (const route of retrievalRoutes) retrievalPluginIds.add(route.pluginId);
  for (const process of pooledProcesses) retrievalPluginIds.add(process.pluginId);

  const healthyRouteCount = retrievalRoutes.filter(
    (route) => route.state === "healthy",
  ).length;
  const degradedRouteCount = retrievalRoutes.filter(
    (route) => route.state === "degraded",
  ).length;
  const quarantinedRouteCount = retrievalRoutes.filter(
    (route) => route.state === "quarantined" || route.quarantined,
  ).length;
  const knownPluginIds = new Set(plugins.map((plugin) => plugin.id));
  const unknownPluginIds = Array.from(retrievalPluginIds)
    .filter((pluginId) => !knownPluginIds.has(pluginId))
    .sort((left, right) => left.localeCompare(right));
  const diagnosticPlugins = plugins
    .filter((plugin) => retrievalPluginIds.has(plugin.id))
    .map((plugin) => ({
      id: plugin.id,
      name: plugin.name,
      displayName: plugin.interface?.displayName,
      marketplaceName: plugin.marketplaceName,
      sourcePath: plugin.sourcePath,
      installedPath: plugin.installedPath,
      installed: plugin.installed,
      enabled: plugin.enabled,
      declaredRouteCount: plugin.retrieval?.sources.length ?? 0,
    }));

  const payload: RetrievalRuntimeDiagnosticsPayload = {
    protocolDocPath: RETRIEVAL_PLUGIN_PROTOCOL_DOC_PATH,
    summary: {
      pluginCount: diagnosticPlugins.length,
      routeCount: retrievalRoutes.length,
      healthyRouteCount,
      degradedRouteCount,
      quarantinedRouteCount,
      pooledProcessCount: pooledProcesses.length,
      unknownPluginCount: unknownPluginIds.length,
    },
    plugins: diagnosticPlugins,
    unknownPluginIds,
    retrievalRoutes,
    pooledProcesses,
    notes: [
      "Retrieval runtime diagnostics include route health, quarantine windows, and warm child processes.",
      "No credential values are included in this diagnostics payload.",
      "See protocolDocPath for the local search/query/fetch plugin route contract.",
    ],
  };

  return JSON.stringify(payload, null, 2);
}

function operatorRunErrorMessage(result: Record<string, unknown>): string {
  const error = result.error;
  if (error && typeof error === "object" && "message" in error) {
    const message = (error as { message?: unknown }).message;
    if (typeof message === "string" && message.trim()) return message;
  }
  const status = result.status;
  if (typeof status === "string" && status.trim()) {
    return `Operator run ${status}.`;
  }
  return "Operator run failed.";
}

function operatorSurfacePayload(surface?: OperatorExecutionSurfaceArgs) {
  return {
    sessionId: surface?.sessionId ?? null,
    executionEnvironment: surface?.executionEnvironment ?? "local",
    sshServer: surface?.sshServer ?? null,
    sandboxBackend: surface?.sandboxBackend ?? "docker",
  };
}

function stringField(value: unknown): string | null {
  return typeof value === "string" && value.trim() ? value : null;
}

function booleanField(value: unknown): boolean | null {
  return typeof value === "boolean" ? value : null;
}

function outputArtifactCount(outputs: unknown): number {
  if (!outputs || typeof outputs !== "object" || Array.isArray(outputs)) return 0;
  return Object.values(outputs).reduce((count, value) => (
    count + (Array.isArray(value) ? value.length : 0)
  ), 0);
}

function structuredOutputCount(outputs: unknown): number {
  if (!outputs || typeof outputs !== "object" || Array.isArray(outputs)) return 0;
  return Object.keys(outputs).length;
}

export function summarizeOperatorRunResult(result: Record<string, unknown>): OperatorRunSummary | null {
  const runId = stringField(result.runId);
  const status = stringField(result.status);
  const runDir = stringField(result.runDir);
  if (!runId || !status || !runDir) return null;
  const operator = result.operator && typeof result.operator === "object"
    ? result.operator as Record<string, unknown>
    : {};
  const error = result.error && typeof result.error === "object"
    ? result.error as Record<string, unknown>
    : {};
  const runContext = result.runContext && typeof result.runContext === "object"
    ? result.runContext as Record<string, unknown>
    : {};
  const cache = result.cache && typeof result.cache === "object"
    ? result.cache as Record<string, unknown>
    : {};
  return {
    runId,
    status,
    location: stringField(result.location) ?? "local",
    operatorAlias: stringField(operator.alias),
    operatorId: stringField(operator.id),
    operatorVersion: stringField(operator.version),
    sourcePlugin: stringField(operator.sourcePlugin),
    runKind: stringField(runContext.kind),
    smokeTestId: stringField(runContext.smokeTestId),
    smokeTestName: stringField(runContext.smokeTestName),
    runDir,
    provenancePath: stringField(result.provenancePath),
    outputCount: outputArtifactCount(result.outputs),
    structuredOutputCount: structuredOutputCount(result.structuredOutputs),
    errorMessage: stringField(error.message),
    errorKind: stringField(error.kind),
    retryable: booleanField(error.retryable),
    suggestedAction: stringField(error.suggestedAction),
    stdoutTail: stringField(error.stdoutTail),
    stderrTail: stringField(error.stderrTail),
    cacheKey: stringField(cache.key),
    cacheHit: booleanField(cache.hit),
    cacheSourceRunId: stringField(cache.sourceRunId),
    cacheSourceRunDir: stringField(cache.sourceRunDir),
  };
}

export const usePluginStore = create<PluginState>((set, get) => ({
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

  loadPlugins: async (projectRoot?: string, surface?: OperatorExecutionSurfaceArgs) => {
    set({ isLoading: true, error: null });
    try {
      const [
        marketplaces,
        retrievalStatuses,
        processPoolStatuses,
        operatorCatalog,
        operatorRuns,
      ] = await Promise.all([
        invoke<PluginMarketplaceEntry[]>("list_omiga_plugin_marketplaces", {
          projectRoot,
        }),
        invoke<PluginRetrievalRouteStatus[]>(
          "list_omiga_plugin_retrieval_statuses",
          { projectRoot },
        ),
        invoke<PluginProcessPoolRouteStatus[]>(
          "list_omiga_plugin_process_pool_statuses",
          { projectRoot },
        ),
        invoke<OperatorCatalogResponse>("list_operators"),
        invoke<OperatorRunSummary[]>("list_operator_runs", {
          projectRoot,
          ...operatorSurfacePayload(surface),
        }),
      ]);
      set({
        marketplaces,
        retrievalStatuses,
        processPoolStatuses,
        operators: operatorCatalog.operators,
        operatorDiagnostics: operatorCatalog.diagnostics ?? [],
        operatorRegistryPath: operatorCatalog.registryPath,
        operatorRuns,
        isLoading: false,
      });
    } catch (e) {
      set({ isLoading: false, error: extractErrorMessage(e) });
    }
  },

  loadOperators: async () => {
    try {
      const operatorCatalog = await invoke<OperatorCatalogResponse>("list_operators");
      set({
        operators: operatorCatalog.operators,
        operatorDiagnostics: operatorCatalog.diagnostics ?? [],
        operatorRegistryPath: operatorCatalog.registryPath,
      });
    } catch (e) {
      set({ error: extractErrorMessage(e) });
    }
  },

  loadOperatorRuns: async (
    projectRoot?: string,
    surface?: OperatorExecutionSurfaceArgs,
  ) => {
    try {
      const operatorRuns = await invoke<OperatorRunSummary[]>(
        "list_operator_runs",
        {
          projectRoot,
          ...operatorSurfacePayload(surface),
        },
      );
      set({ operatorRuns });
    } catch (e) {
      set({ error: extractErrorMessage(e) });
    }
  },

  readOperatorRun: async (
    runId: string,
    projectRoot?: string,
    surface?: OperatorExecutionSurfaceArgs,
  ) => {
    try {
      return await invoke<OperatorRunDetail>("read_operator_run", {
        runId,
        projectRoot,
        ...operatorSurfacePayload(surface),
      });
    } catch (e) {
      const error = extractErrorMessage(e);
      set({ error });
      throw new Error(error);
    }
  },

  readOperatorRunLog: async (
    runId: string,
    logName: "stdout" | "stderr",
    projectRoot?: string,
    surface?: OperatorExecutionSurfaceArgs,
  ) => {
    try {
      return await invoke<OperatorRunLog>("read_operator_run_log", {
        runId,
        logName,
        projectRoot,
        limitBytes: 16 * 1024,
        ...operatorSurfacePayload(surface),
      });
    } catch (e) {
      const error = extractErrorMessage(e);
      set({ error });
      throw new Error(error);
    }
  },

  verifyOperatorRun: async (
    runId: string,
    projectRoot?: string,
    surface?: OperatorExecutionSurfaceArgs,
  ) => {
    try {
      return await invoke<OperatorRunVerification>("verify_operator_run", {
        runId,
        projectRoot,
        ...operatorSurfacePayload(surface),
      });
    } catch (e) {
      const error = extractErrorMessage(e);
      set({ error });
      throw new Error(error);
    }
  },

  cleanupOperatorRuns: async (
    request: OperatorRunCleanupRequest,
    projectRoot?: string,
    surface?: OperatorExecutionSurfaceArgs,
  ) => {
    set({ isMutating: true, error: null });
    try {
      const result = await invoke<OperatorRunCleanupResult>("cleanup_operator_runs", {
        request,
        projectRoot,
        ...operatorSurfacePayload(surface),
      });
      await get().loadOperatorRuns(projectRoot, surface);
      set({ isMutating: false });
      return result;
    } catch (e) {
      const error = extractErrorMessage(e);
      set({ isMutating: false, error });
      throw new Error(error);
    }
  },

  loadRetrievalStatuses: async (projectRoot?: string) => {
    try {
      const retrievalStatuses = await invoke<PluginRetrievalRouteStatus[]>(
        "list_omiga_plugin_retrieval_statuses",
        { projectRoot },
      );
      set({ retrievalStatuses });
    } catch (e) {
      set({ error: extractErrorMessage(e) });
    }
  },

  loadProcessPoolStatuses: async (projectRoot?: string) => {
    try {
      const processPoolStatuses = await invoke<PluginProcessPoolRouteStatus[]>(
        "list_omiga_plugin_process_pool_statuses",
        { projectRoot },
      );
      set({ processPoolStatuses });
    } catch (e) {
      set({ error: extractErrorMessage(e) });
    }
  },

  clearProcessPool: async (projectRoot?: string) => {
    set({ isMutating: true, error: null });
    try {
      const cleared = await invoke<number>("clear_omiga_plugin_process_pool", {
        projectRoot,
      });
      await get().loadProcessPoolStatuses(projectRoot);
      set({ isMutating: false });
      return cleared;
    } catch (e) {
      const error = extractErrorMessage(e);
      set({ isMutating: false, error });
      throw new Error(error);
    }
  },

  installPlugin: async (plugin: PluginSummary, projectRoot?: string) => {
    set({ isMutating: true, error: null });
    try {
      const result = await invoke<PluginInstallResult>("install_omiga_plugin", {
        marketplacePath: plugin.marketplacePath,
        pluginName: plugin.name,
        projectRoot,
      });
      await get().loadPlugins(projectRoot);
      set({ isMutating: false });
      return result;
    } catch (e) {
      const error = extractErrorMessage(e);
      set({ isMutating: false, error });
      throw new Error(error);
    }
  },

  uninstallPlugin: async (pluginId: string, projectRoot?: string) => {
    set({ isMutating: true, error: null });
    try {
      await invoke("uninstall_omiga_plugin", { pluginId, projectRoot });
      await get().loadPlugins(projectRoot);
      set({ isMutating: false });
    } catch (e) {
      const error = extractErrorMessage(e);
      set({ isMutating: false, error });
      throw new Error(error);
    }
  },

  setPluginEnabled: async (pluginId: string, enabled: boolean, projectRoot?: string) => {
    const previousMarketplaces = get().marketplaces;
    set({
      marketplaces: updatePluginEnabledInMarketplaces(
        previousMarketplaces,
        pluginId,
        enabled,
      ),
      error: null,
    });
    try {
      await invoke("set_omiga_plugin_enabled", { pluginId, enabled, projectRoot });
      await get().loadRetrievalStatuses(projectRoot);
    } catch (e) {
      const error = extractErrorMessage(e);
      set({ marketplaces: previousMarketplaces, error });
      throw new Error(error);
    }
  },

  setOperatorEnabled: async (
    update: OperatorRegistryUpdate,
    _projectRoot?: string,
    _surface?: OperatorExecutionSurfaceArgs,
  ) => {
    const previousOperators = get().operators;
    set({
      operators: updateOperatorEnabledInCatalog(previousOperators, update),
      error: null,
    });
    try {
      await invoke("set_operator_enabled", { update });
    } catch (e) {
      const error = extractErrorMessage(e);
      set({ operators: previousOperators, error });
      throw new Error(error);
    }
  },

  runOperator: async (
    alias: string,
    invocation: OperatorInvocationArguments,
    projectRoot?: string,
    surface?: OperatorExecutionSurfaceArgs,
    runContext?: OperatorRunContext,
  ) => {
    set({ isMutating: true, error: null });
    try {
      const response = await invoke<OperatorRunResponse>("run_operator", {
        alias,
        arguments: invocation,
        projectRoot,
        ...operatorSurfacePayload(surface),
        runKind: runContext?.kind ?? null,
        smokeTestId: runContext?.smokeTestId ?? null,
        smokeTestName: runContext?.smokeTestName ?? null,
      });
      await get().loadOperatorRuns(projectRoot, surface);
      const summary = summarizeOperatorRunResult(response.result);
      if (summary && !get().operatorRuns.some((run) => run.runId === summary.runId)) {
        set({ operatorRuns: [summary, ...get().operatorRuns].slice(0, 25) });
      }
      if (!response.ok) {
        const error = operatorRunErrorMessage(response.result);
        set({ isMutating: false, error });
        throw new Error(error);
      }
      set({ isMutating: false });
      return response;
    } catch (e) {
      const error = extractErrorMessage(e);
      set({ isMutating: false, error });
      throw new Error(error);
    }
  },
}));
