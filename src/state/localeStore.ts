import { create } from "zustand";
import { persist } from "zustand/middleware";

/** UI locale — persisted for next launch */
export type AppLocale = "en" | "zh-CN";

interface LocaleState {
  locale: AppLocale;
  setLocale: (locale: AppLocale) => void;
}

export const useLocaleStore = create<LocaleState>()(
  persist(
    (set) => ({
      locale: "en",
      setLocale: (locale) => set({ locale }),
    }),
    { name: "omiga-locale" },
  ),
);
