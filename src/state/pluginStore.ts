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
  enabledAliases: string[];
  exposed: boolean;
  unavailableReason?: string | null;
}

export interface OperatorCatalogResponse {
  registryPath: string;
  operators: OperatorSummary[];
}

export interface OperatorRegistryUpdate {
  alias: string;
  operatorId?: string | null;
  sourcePlugin?: string | null;
  version?: string | null;
  enabled: boolean;
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
  operatorRegistryPath: string | null;
  retrievalStatuses: PluginRetrievalRouteStatus[];
  processPoolStatuses: PluginProcessPoolRouteStatus[];
  isLoading: boolean;
  isMutating: boolean;
  error: string | null;
  loadPlugins: (projectRoot?: string) => Promise<void>;
  loadOperators: () => Promise<void>;
  loadRetrievalStatuses: (projectRoot?: string) => Promise<void>;
  loadProcessPoolStatuses: (projectRoot?: string) => Promise<void>;
  clearProcessPool: (projectRoot?: string) => Promise<number>;
  installPlugin: (plugin: PluginSummary, projectRoot?: string) => Promise<PluginInstallResult>;
  uninstallPlugin: (pluginId: string, projectRoot?: string) => Promise<void>;
  setPluginEnabled: (pluginId: string, enabled: boolean, projectRoot?: string) => Promise<void>;
  setOperatorEnabled: (update: OperatorRegistryUpdate, projectRoot?: string) => Promise<void>;
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

export const usePluginStore = create<PluginState>((set, get) => ({
  marketplaces: [],
  operators: [],
  operatorRegistryPath: null,
  retrievalStatuses: [],
  processPoolStatuses: [],
  isLoading: false,
  isMutating: false,
  error: null,

  loadPlugins: async (projectRoot?: string) => {
    set({ isLoading: true, error: null });
    try {
      const [marketplaces, retrievalStatuses, processPoolStatuses, operatorCatalog] = await Promise.all([
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
        invoke<OperatorCatalogResponse>("list_omiga_operators"),
      ]);
      set({
        marketplaces,
        retrievalStatuses,
        processPoolStatuses,
        operators: operatorCatalog.operators,
        operatorRegistryPath: operatorCatalog.registryPath,
        isLoading: false,
      });
    } catch (e) {
      set({ isLoading: false, error: extractErrorMessage(e) });
    }
  },

  loadOperators: async () => {
    try {
      const operatorCatalog = await invoke<OperatorCatalogResponse>("list_omiga_operators");
      set({
        operators: operatorCatalog.operators,
        operatorRegistryPath: operatorCatalog.registryPath,
      });
    } catch (e) {
      set({ error: extractErrorMessage(e) });
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
    set({ isMutating: true, error: null });
    try {
      await invoke("set_omiga_plugin_enabled", { pluginId, enabled, projectRoot });
      await get().loadPlugins(projectRoot);
      set({ isMutating: false });
    } catch (e) {
      const error = extractErrorMessage(e);
      set({ isMutating: false, error });
      throw new Error(error);
    }
  },

  setOperatorEnabled: async (update: OperatorRegistryUpdate, projectRoot?: string) => {
    set({ isMutating: true, error: null });
    try {
      await invoke("set_omiga_operator_enabled", { update });
      await get().loadPlugins(projectRoot);
      set({ isMutating: false });
    } catch (e) {
      const error = extractErrorMessage(e);
      set({ isMutating: false, error });
      throw new Error(error);
    }
  },
}));
