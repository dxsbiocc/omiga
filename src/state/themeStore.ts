import { create } from "zustand";
import { persist } from "zustand/middleware";
import {
  ACCENT_PRESET_IDS,
  type AccentPresetId,
} from "../theme/accentPresets";

/** User preference; `system` follows OS light/dark */
export type ColorModePreference = "light" | "dark" | "system";

interface ThemePreferenceState {
  colorMode: ColorModePreference;
  setColorMode: (mode: ColorModePreference) => void;
  accentPreset: AccentPresetId;
  setAccentPreset: (preset: AccentPresetId) => void;
}

export const useColorModeStore = create<ThemePreferenceState>()(
  persist(
    (set) => ({
      colorMode: "dark",
      setColorMode: (colorMode) => set({ colorMode }),
      accentPreset: "asana" satisfies AccentPresetId,
      setAccentPreset: (accentPreset) => set({ accentPreset }),
    }),
    {
      name: "omiga-theme",
      /** Keep new fields when localStorage predates them (e.g. `accentPreset`). */
      merge: (persisted, current) => {
        const merged = {
          ...current,
          ...(persisted as object),
        } as ThemePreferenceState & { accentPreset?: string };
        const rawUnknown = merged.accentPreset as unknown;
        let raw = typeof rawUnknown === "string" ? rawUnknown : "";
        if (raw === "default") raw = "asana";
        if (!raw || !ACCENT_PRESET_IDS.includes(raw as AccentPresetId)) {
          merged.accentPreset = "asana";
        } else {
          merged.accentPreset = raw as AccentPresetId;
        }
        return merged;
      },
    },
  ),
);
