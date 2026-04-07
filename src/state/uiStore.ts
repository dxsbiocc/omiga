import { create } from "zustand";
import { persist } from "zustand/middleware";
import {
  LAYOUT_LEFT_MAX,
  LAYOUT_LEFT_MIN,
  LAYOUT_PANEL_MIN,
  LAYOUT_RIGHT_MAX,
  LAYOUT_RIGHT_MIN,
} from "./constants";

function clamp(n: number, min: number, max: number): number {
  return Math.max(min, Math.min(max, n));
}

export interface UiState {
  /** Settings sidebar index: 0–3 App (Model…Theme), 4–6 Integrations */
  settingsTabIndex: number;
  setSettingsTabIndex: (index: number) => void;
  settingsOpen: boolean;
  rightPanelMode: "default" | "settings";
  leftPanelWidth: number;
  rightPanelWidth: number;
  codePanelHeight: number;
  tasksPanelHeight: number;
  setSettingsOpen: (open: boolean) => void;
  setRightPanelMode: (mode: "default" | "settings") => void;
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

      settingsOpen: false,
      rightPanelMode: "default",
      leftPanelWidth: 260,
      rightPanelWidth: 300,
      codePanelHeight: 280,
      tasksPanelHeight: 320,

      setSettingsOpen: (open) => set({ settingsOpen: open }),

      setRightPanelMode: (mode) => set({ rightPanelMode: mode }),

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
      partialize: (s) => ({
        leftPanelWidth: s.leftPanelWidth,
        rightPanelWidth: s.rightPanelWidth,
        codePanelHeight: s.codePanelHeight,
        tasksPanelHeight: s.tasksPanelHeight,
      }),
    }
  )
);
