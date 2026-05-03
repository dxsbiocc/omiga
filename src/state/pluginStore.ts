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
  };
  retrievalRoutes: PluginRetrievalRouteStatus[];
  pooledProcesses: PluginProcessPoolRouteStatus[];
  notes: string[];
}

interface PluginState {
  marketplaces: PluginMarketplaceEntry[];
  retrievalStatuses: PluginRetrievalRouteStatus[];
  processPoolStatuses: PluginProcessPoolRouteStatus[];
  isLoading: boolean;
  isMutating: boolean;
  error: string | null;
  loadPlugins: (projectRoot?: string) => Promise<void>;
  loadRetrievalStatuses: (projectRoot?: string) => Promise<void>;
  loadProcessPoolStatuses: (projectRoot?: string) => Promise<void>;
  clearProcessPool: (projectRoot?: string) => Promise<number>;
  installPlugin: (plugin: PluginSummary, projectRoot?: string) => Promise<PluginInstallResult>;
  uninstallPlugin: (pluginId: string, projectRoot?: string) => Promise<void>;
  setPluginEnabled: (pluginId: string, enabled: boolean, projectRoot?: string) => Promise<void>;
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

export const usePluginStore = create<PluginState>((set, get) => ({
  marketplaces: [],
  retrievalStatuses: [],
  processPoolStatuses: [],
  isLoading: false,
  isMutating: false,
  error: null,

  loadPlugins: async (projectRoot?: string) => {
    set({ isLoading: true, error: null });
    try {
      const [marketplaces, retrievalStatuses, processPoolStatuses] = await Promise.all([
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
      ]);
      set({ marketplaces, retrievalStatuses, processPoolStatuses, isLoading: false });
    } catch (e) {
      set({ isLoading: false, error: extractErrorMessage(e) });
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
}));
