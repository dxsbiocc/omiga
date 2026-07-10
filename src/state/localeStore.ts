import { create } from "zustand";
import { createJSONStorage, persist } from "zustand/middleware";
import { safeLocalStorage } from "../utils/browserStorage";

/** UI locale — persisted for next launch */
export type AppLocale = "en" | "zh-CN";

export function normalizeAppLocale(locale: unknown): AppLocale {
  return typeof locale === "string" &&
    locale.toLowerCase().replace("_", "-").startsWith("zh")
    ? "zh-CN"
    : "en";
}

function defaultAppLocale(): AppLocale {
  if (typeof navigator === "undefined") return "en";
  return normalizeAppLocale(navigator.language);
}

interface LocaleState {
  locale: AppLocale;
  setLocale: (locale: AppLocale) => void;
}

export const useLocaleStore = create<LocaleState>()(
  persist(
    (set) => ({
      locale: defaultAppLocale(),
      setLocale: (locale) => set({ locale: normalizeAppLocale(locale) }),
    }),
    {
      name: "omiga-locale",
      version: 1,
      storage: createJSONStorage(() => safeLocalStorage),
      migrate: (persisted) => {
        const state =
          persisted && typeof persisted === "object"
            ? (persisted as Partial<LocaleState>)
            : {};
        return { locale: normalizeAppLocale(state.locale) };
      },
    },
  ),
);
