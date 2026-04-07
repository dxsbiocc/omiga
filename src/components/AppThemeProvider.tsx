import { useEffect, useMemo, useState, type ReactNode } from "react";
import { ThemeProvider, createTheme } from "@mui/material/styles";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { getTheme } from "../theme";
import { getAccentPresetOptions } from "../theme/accentPresets";
import { useColorModeStore } from "../state/themeStore";

function useSystemPrefersDark(): boolean {
  const [dark, setDark] = useState(() => {
    if (typeof window === "undefined") return false;
    return window.matchMedia("(prefers-color-scheme: dark)").matches;
  });

  useEffect(() => {
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const onChange = () => setDark(mq.matches);
    mq.addEventListener("change", onChange);
    return () => mq.removeEventListener("change", onChange);
  }, []);

  return dark;
}

function resolvePaletteMode(
  preference: "light" | "dark" | "system",
  systemDark: boolean,
): "light" | "dark" {
  if (preference === "system") return systemDark ? "dark" : "light";
  return preference;
}

export function AppThemeProvider({ children }: { children: ReactNode }) {
  const colorMode = useColorModeStore((s) => s.colorMode);
  const accentPreset = useColorModeStore((s) => s.accentPreset ?? "asana");
  const systemDark = useSystemPrefersDark();
  const resolvedMode = resolvePaletteMode(colorMode, systemDark);

  const muiTheme = useMemo(
    () =>
      createTheme(
        getTheme(resolvedMode),
        getAccentPresetOptions(accentPreset, resolvedMode),
      ),
    [resolvedMode, accentPreset],
  );

  useEffect(() => {
    document.documentElement.style.colorScheme = resolvedMode;
  }, [resolvedMode]);

  useEffect(() => {
    void getCurrentWindow()
      .setTheme(resolvedMode)
      .catch(() => {
        /* `vite dev` in browser — no Tauri window */
      });
  }, [resolvedMode]);

  return <ThemeProvider theme={muiTheme}>{children}</ThemeProvider>;
}
