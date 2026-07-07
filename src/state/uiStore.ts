import { create } from "zustand";
import { createJSONStorage, persist } from "zustand/middleware";
import {
  LAYOUT_LEFT_MAX,
  LAYOUT_LEFT_MIN,
  LAYOUT_PANEL_MIN,
  LAYOUT_RIGHT_MAX,
  LAYOUT_RIGHT_MIN,
} from "./constants";
import { safeLocalStorage } from "../utils/browserStorage";

function clamp(n: number, min: number, max: number): number {
  return Math.max(min, Math.min(max, n));
}

export interface UiState {
  /** Settings sidebar index — see `Settings/index.tsx` SETTINGS_SECTIONS (0–15) */
  settingsTabIndex: number;
  setSettingsTabIndex: (index: number) => void;
  /** When sidebar tab is Execution (9): inner tab; currently SSH-only */
  settingsExecutionSubTab: number;
  setSettingsExecutionSubTab: (index: number) => void;
  settingsOpen: boolean;
  rightPanelMode: "default" | "settings";
  leftPanelCollapsed: boolean;
  rightPanelCollapsed: boolean;
  terminalPanelOpen: boolean;
  onboardingCompleted: boolean;
  setOnboardingCompleted: (completed: boolean) => void;
  leftPanelWidth: number;
  rightPanelWidth: number;
  codePanelHeight: number;
  tasksPanelHeight: number;
  setSettingsOpen: (open: boolean) => void;
  setRightPanelMode: (mode: "default" | "settings") => void;
  setLeftPanelCollapsed: (collapsed: boolean) => void;
  toggleLeftPanelCollapsed: () => void;
  setRightPanelCollapsed: (collapsed: boolean) => void;
  toggleRightPanelCollapsed: () => void;
  setTerminalPanelOpen: (open: boolean) => void;
  toggleTerminalPanelOpen: () => void;
  setLeftWidth: (w: number) => void;
  setRightWidth: (w: number) => void;
  setCodeHeight: (h: number) => void;
  setTasksHeight: (h: number) => void;
  resizeLeftBy: (delta: number) => void;
  resizeRightBy: (delta: number) => void;
  /** Pass `maxHeight` from the center column ref when available (fallback 600). */
  resizeCodeBy: (delta: number, maxHeight?: number) => void;
  /** Pass `maxHeight` from the right column ref when available (fallback 500). */
  resizeTasksBy: (delta: number, maxHeight?: number) => void;
  /** When a file opens, ensure code panel height is at least `LAYOUT_PANEL_MIN`. */
  ensureCodePanelMin: () => void;
}

export const useUiStore = create<UiState>()(
  persist(
    (set, get) => ({
      settingsTabIndex: 0,
      setSettingsTabIndex: (index) => set({ settingsTabIndex: index }),

      settingsExecutionSubTab: 0,
      setSettingsExecutionSubTab: (index) =>
        set({ settingsExecutionSubTab: Math.max(0, Math.min(2, Math.floor(index))) }),

      settingsOpen: false,
      rightPanelMode: "default",
      leftPanelCollapsed: false,
      rightPanelCollapsed: false,
      terminalPanelOpen: false,
      onboardingCompleted: false,
      setOnboardingCompleted: (completed) => set({ onboardingCompleted: completed }),
      leftPanelWidth: 260,
      rightPanelWidth: 300,
      codePanelHeight: 280,
      tasksPanelHeight: 320,

      setSettingsOpen: (open) => set({ settingsOpen: open }),

      setRightPanelMode: (mode) => set({ rightPanelMode: mode }),
      setLeftPanelCollapsed: (collapsed) => set({ leftPanelCollapsed: collapsed }),
      toggleLeftPanelCollapsed: () =>
        set((s) => ({ leftPanelCollapsed: !s.leftPanelCollapsed })),
      setRightPanelCollapsed: (collapsed) => set({ rightPanelCollapsed: collapsed }),
      toggleRightPanelCollapsed: () =>
        set((s) => ({ rightPanelCollapsed: !s.rightPanelCollapsed })),
      setTerminalPanelOpen: (open) => set({ terminalPanelOpen: open }),
      toggleTerminalPanelOpen: () =>
        set((s) => ({ terminalPanelOpen: !s.terminalPanelOpen })),

      setLeftWidth: (w) =>
        set({ leftPanelWidth: clamp(w, LAYOUT_LEFT_MIN, LAYOUT_LEFT_MAX) }),

      setRightWidth: (w) =>
        set({ rightPanelWidth: clamp(w, LAYOUT_RIGHT_MIN, LAYOUT_RIGHT_MAX) }),

      setCodeHeight: (h) =>
        set({
          codePanelHeight: Math.max(LAYOUT_PANEL_MIN, h),
        }),

      setTasksHeight: (h) =>
        set({
          tasksPanelHeight: Math.max(LAYOUT_PANEL_MIN, h),
        }),

      resizeLeftBy: (delta) =>
        set((s) => ({
          leftPanelWidth: clamp(
            s.leftPanelWidth + delta,
            LAYOUT_LEFT_MIN,
            LAYOUT_LEFT_MAX
          ),
        })),

      resizeRightBy: (delta) =>
        set((s) => ({
          rightPanelWidth: clamp(
            s.rightPanelWidth - delta,
            LAYOUT_RIGHT_MIN,
            LAYOUT_RIGHT_MAX
          ),
        })),

      resizeCodeBy: (delta, maxHeight) => {
        const maxH = maxHeight ?? 600;
        set((s) => ({
          codePanelHeight: clamp(
            s.codePanelHeight + delta,
            LAYOUT_PANEL_MIN,
            maxH
          ),
        }));
      },

      resizeTasksBy: (delta, maxHeight) => {
        const maxH = maxHeight ?? 500;
        set((s) => ({
          tasksPanelHeight: clamp(
            s.tasksPanelHeight + delta,
            LAYOUT_PANEL_MIN,
            maxH
          ),
        }));
      },

      ensureCodePanelMin: () => {
        const { codePanelHeight } = get();
        if (codePanelHeight < LAYOUT_PANEL_MIN) {
          set({ codePanelHeight: LAYOUT_PANEL_MIN });
        }
      },
    }),
    {
      name: "omiga-ui",
      storage: createJSONStorage(() => safeLocalStorage),
      partialize: (s) => ({
        leftPanelWidth: s.leftPanelWidth,
        leftPanelCollapsed: s.leftPanelCollapsed,
        rightPanelWidth: s.rightPanelWidth,
        rightPanelCollapsed: s.rightPanelCollapsed,
        terminalPanelOpen: s.terminalPanelOpen,
        codePanelHeight: s.codePanelHeight,
        tasksPanelHeight: s.tasksPanelHeight,
        onboardingCompleted: s.onboardingCompleted,
      }),
    }
  )
);
