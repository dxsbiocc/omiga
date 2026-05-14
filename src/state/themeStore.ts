import { create } from "zustand";
import { createJSONStorage, persist } from "zustand/middleware";
import {
  ACCENT_PRESET_IDS,
  type AccentPresetId,
} from "../theme/accentPresets";
import { safeLocalStorage } from "../utils/browserStorage";

/** User preference; `system` follows OS light/dark */
export type ColorModePreference = "light" | "dark" | "system";
export const APP_SKIN_IDS = ["classic-capybara", "warm-capybara"] as const;
export type AppSkinId = (typeof APP_SKIN_IDS)[number];

const DEFAULT_APP_SKIN: AppSkinId = "classic-capybara";

interface ThemePreferenceState {
  colorMode: ColorModePreference;
  setColorMode: (mode: ColorModePreference) => void;
  accentPreset: AccentPresetId;
  setAccentPreset: (preset: AccentPresetId) => void;
  appSkin: AppSkinId;
  setAppSkin: (skin: AppSkinId) => void;
}

export const useColorModeStore = create<ThemePreferenceState>()(
  persist(
    (set) => ({
      colorMode: "dark",
      setColorMode: (colorMode) => set({ colorMode }),
      accentPreset: "asana" satisfies AccentPresetId,
      setAccentPreset: (accentPreset) => set({ accentPreset }),
      appSkin: DEFAULT_APP_SKIN,
      setAppSkin: (appSkin) => set({ appSkin }),
    }),
    {
      name: "omiga-theme",
      storage: createJSONStorage(() => safeLocalStorage),
      /** Keep new fields when localStorage predates them (e.g. `accentPreset`, `appSkin`). */
      merge: (persisted, current) => {
        const merged = {
          ...current,
          ...(persisted as object),
        } as ThemePreferenceState & { accentPreset?: string; appSkin?: string };
        const rawUnknown = merged.accentPreset as unknown;
        let raw = typeof rawUnknown === "string" ? rawUnknown : "";
        if (raw === "default") raw = "asana";
        if (!raw || !ACCENT_PRESET_IDS.includes(raw as AccentPresetId)) {
          merged.accentPreset = "asana";
        } else {
          merged.accentPreset = raw as AccentPresetId;
        }
        const rawSkin =
          typeof (merged.appSkin as unknown) === "string" ? merged.appSkin : "";
        merged.appSkin = APP_SKIN_IDS.includes(rawSkin as AppSkinId)
          ? (rawSkin as AppSkinId)
          : DEFAULT_APP_SKIN;
        return merged;
      },
    },
  ),
);
