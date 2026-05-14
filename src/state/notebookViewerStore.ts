import { create } from "zustand";
import { createJSONStorage, persist } from "zustand/middleware";
import { safeLocalStorage } from "../utils/browserStorage";

export interface NotebookViewerState {
  /** Virtualize long notebooks (TanStack Virtual) */
  virtualizeCells: boolean;
  /** Allow scripts in HTML output iframes (rich charts; only open trusted notebooks) */
  htmlSandboxAllowScripts: boolean;
  /** Transform Python line-leading `!` into subprocess (IPython-style) */
  enablePythonShellMagic: boolean;
  /** Shift+Enter / Ctrl+Enter run shortcuts in code cells */
  enableNotebookShortcuts: boolean;
  setVirtualizeCells: (v: boolean) => void;
  setHtmlSandboxAllowScripts: (v: boolean) => void;
  setEnablePythonShellMagic: (v: boolean) => void;
  setEnableNotebookShortcuts: (v: boolean) => void;
}

export const useNotebookViewerStore = create<NotebookViewerState>()(
  persist(
    (set) => ({
      virtualizeCells: true,
      htmlSandboxAllowScripts: true,
      enablePythonShellMagic: true,
      enableNotebookShortcuts: true,
      setVirtualizeCells: (virtualizeCells) => set({ virtualizeCells }),
      setHtmlSandboxAllowScripts: (htmlSandboxAllowScripts) => set({ htmlSandboxAllowScripts }),
      setEnablePythonShellMagic: (enablePythonShellMagic) => set({ enablePythonShellMagic }),
      setEnableNotebookShortcuts: (enableNotebookShortcuts) => set({ enableNotebookShortcuts }),
    }),
    {
      name: "omiga-notebook-viewer",
      storage: createJSONStorage(() => safeLocalStorage),
      partialize: (s) => ({
        virtualizeCells: s.virtualizeCells,
        htmlSandboxAllowScripts: s.htmlSandboxAllowScripts,
        enablePythonShellMagic: s.enablePythonShellMagic,
        enableNotebookShortcuts: s.enableNotebookShortcuts,
      }),
    },
  ),
);
