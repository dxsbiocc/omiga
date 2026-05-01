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

interface PluginState {
  marketplaces: PluginMarketplaceEntry[];
  isLoading: boolean;
  isMutating: boolean;
  error: string | null;
  loadPlugins: (projectRoot?: string) => Promise<void>;
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

export const usePluginStore = create<PluginState>((set, get) => ({
  marketplaces: [],
  isLoading: false,
  isMutating: false,
  error: null,

  loadPlugins: async (projectRoot?: string) => {
    set({ isLoading: true, error: null });
    try {
      const marketplaces = await invoke<PluginMarketplaceEntry[]>(
        "list_omiga_plugin_marketplaces",
        { projectRoot },
      );
      set({ marketplaces, isLoading: false });
    } catch (e) {
      set({ isLoading: false, error: extractErrorMessage(e) });
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
