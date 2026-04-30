import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import {
  DEFAULT_ICON_THEME_ID,
  getIconThemeContributions,
  resolveIconTheme,
  type IconThemeContribution,
  type InstalledVscodeExtension,
  type ResolvedIconTheme,
  type VscodeIconThemeDocument,
} from "../utils/vscodeExtensions";
import { extractErrorMessage } from "../utils/errorMessage";

const ICON_THEME_STORAGE_KEY = "omiga_vscode_icon_theme";

function readStoredIconThemeId(): string {
  if (typeof localStorage === "undefined") return DEFAULT_ICON_THEME_ID;
  return localStorage.getItem(ICON_THEME_STORAGE_KEY) || DEFAULT_ICON_THEME_ID;
}

function writeStoredIconThemeId(themeId: string): void {
  if (typeof localStorage === "undefined") return;
  localStorage.setItem(ICON_THEME_STORAGE_KEY, themeId);
}

async function loadResolvedIconTheme(
  extensions: InstalledVscodeExtension[],
  themeId: string,
): Promise<ResolvedIconTheme | null> {
  if (!themeId || themeId === DEFAULT_ICON_THEME_ID) return null;

  const contribution = getIconThemeContributions(extensions).find(
    (theme) => theme.id === themeId,
  );
  if (!contribution) return null;

  const raw = await invoke<string>("read_vscode_extension_file", {
    extensionId: contribution.extensionId,
    relativePath: contribution.path,
  });
  const document = JSON.parse(raw) as VscodeIconThemeDocument;
  return resolveIconTheme(extensions, themeId, document);
}

interface ExtensionState {
  extensionsDir: string | null;
  installedExtensions: InstalledVscodeExtension[];
  iconThemes: IconThemeContribution[];
  activeIconThemeId: string;
  activeIconTheme: ResolvedIconTheme | null;
  isLoading: boolean;
  isInstalling: boolean;
  error: string | null;
  loadExtensions: () => Promise<void>;
  installVsix: (vsixPath: string) => Promise<InstalledVscodeExtension>;
  uninstallExtension: (extensionId: string) => Promise<void>;
  setActiveIconTheme: (themeId: string) => Promise<void>;
}

export const useExtensionStore = create<ExtensionState>((set, get) => ({
  extensionsDir: null,
  installedExtensions: [],
  iconThemes: [],
  activeIconThemeId: readStoredIconThemeId(),
  activeIconTheme: null,
  isLoading: false,
  isInstalling: false,
  error: null,

  loadExtensions: async () => {
    set({ isLoading: true, error: null });
    try {
      const [extensionsDir, installedExtensions] = await Promise.all([
        invoke<string>("vscode_extensions_dir"),
        invoke<InstalledVscodeExtension[]>("list_vscode_extensions"),
      ]);
      const iconThemes = getIconThemeContributions(installedExtensions);
      const storedThemeId = get().activeIconThemeId;
      const hasStoredTheme =
        storedThemeId === DEFAULT_ICON_THEME_ID ||
        iconThemes.some((theme) => theme.id === storedThemeId);
      const activeIconThemeId = hasStoredTheme
        ? storedThemeId
        : DEFAULT_ICON_THEME_ID;
      let activeIconTheme: ResolvedIconTheme | null = null;
      let themeError: string | null = null;
      try {
        activeIconTheme = await loadResolvedIconTheme(
          installedExtensions,
          activeIconThemeId,
        );
      } catch (e) {
        themeError = `Icon theme failed to load: ${extractErrorMessage(e)}`;
      }

      if (!hasStoredTheme) writeStoredIconThemeId(DEFAULT_ICON_THEME_ID);
      set({
        extensionsDir,
        installedExtensions,
        iconThemes,
        activeIconThemeId,
        activeIconTheme,
        isLoading: false,
        error: themeError,
      });
    } catch (e) {
      set({ isLoading: false, error: extractErrorMessage(e) });
    }
  },

  installVsix: async (vsixPath: string) => {
    set({ isInstalling: true, error: null });
    try {
      const installed = await invoke<InstalledVscodeExtension>(
        "install_vscode_extension",
        { vsixPath },
      );

      let activeIconThemeId = get().activeIconThemeId;
      const newlyInstalledIconThemes = getIconThemeContributions([installed]);
      if (
        activeIconThemeId === DEFAULT_ICON_THEME_ID &&
        newlyInstalledIconThemes.length > 0
      ) {
        activeIconThemeId = newlyInstalledIconThemes[0].id;
        writeStoredIconThemeId(activeIconThemeId);
      }

      const installedExtensions = await invoke<InstalledVscodeExtension[]>(
        "list_vscode_extensions",
      );
      const iconThemes = getIconThemeContributions(installedExtensions);
      let activeIconTheme: ResolvedIconTheme | null = null;
      let themeError: string | null = null;
      try {
        activeIconTheme = await loadResolvedIconTheme(
          installedExtensions,
          activeIconThemeId,
        );
      } catch (e) {
        themeError = `Icon theme failed to load: ${extractErrorMessage(e)}`;
      }

      set({
        installedExtensions,
        iconThemes,
        activeIconThemeId,
        activeIconTheme,
        isInstalling: false,
        error: themeError,
      });
      return installed;
    } catch (e) {
      const error = extractErrorMessage(e);
      set({ isInstalling: false, error });
      throw new Error(error);
    }
  },

  uninstallExtension: async (extensionId: string) => {
    set({ isLoading: true, error: null });
    try {
      await invoke("uninstall_vscode_extension", { extensionId });
      const installedExtensions = await invoke<InstalledVscodeExtension[]>(
        "list_vscode_extensions",
      );
      const iconThemes = getIconThemeContributions(installedExtensions);
      const currentThemeId = get().activeIconThemeId;
      const activeIconThemeId = iconThemes.some((theme) => theme.id === currentThemeId)
        ? currentThemeId
        : DEFAULT_ICON_THEME_ID;
      if (activeIconThemeId !== currentThemeId) writeStoredIconThemeId(activeIconThemeId);
      let activeIconTheme: ResolvedIconTheme | null = null;
      let themeError: string | null = null;
      try {
        activeIconTheme = await loadResolvedIconTheme(
          installedExtensions,
          activeIconThemeId,
        );
      } catch (e) {
        themeError = `Icon theme failed to load: ${extractErrorMessage(e)}`;
      }
      set({
        installedExtensions,
        iconThemes,
        activeIconThemeId,
        activeIconTheme,
        isLoading: false,
        error: themeError,
      });
    } catch (e) {
      set({ isLoading: false, error: extractErrorMessage(e) });
    }
  },

  setActiveIconTheme: async (themeId: string) => {
    const activeIconThemeId = themeId || DEFAULT_ICON_THEME_ID;
    writeStoredIconThemeId(activeIconThemeId);
    set({ activeIconThemeId, error: null });
    try {
      const activeIconTheme = await loadResolvedIconTheme(
        get().installedExtensions,
        activeIconThemeId,
      );
      set({ activeIconTheme, error: null });
    } catch (e) {
      set({ activeIconTheme: null, error: extractErrorMessage(e) });
    }
  },
}));
