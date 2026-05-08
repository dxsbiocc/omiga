import { useEffect, useMemo, useState, type ReactNode } from "react";
import CssBaseline from "@mui/material/CssBaseline";
import GlobalStyles from "@mui/material/GlobalStyles";
import { ThemeProvider, createTheme, alpha } from "@mui/material/styles";
import { getTheme } from "../theme";
import {
  getAccentPresetOptions,
  getAccentSwatchGradient,
} from "../theme/accentPresets";
import { useColorModeStore } from "../state/themeStore";
import { getCurrentWindowIfTauri } from "../utils/tauriRuntime";

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

  const accentShellGradient = useMemo(
    () => getAccentSwatchGradient(accentPreset, resolvedMode),
    [accentPreset, resolvedMode],
  );

  useEffect(() => {
    document.documentElement.style.colorScheme = resolvedMode;
  }, [resolvedMode]);

  useEffect(() => {
    void getCurrentWindowIfTauri().then((windowHandle) => {
      if (!windowHandle) return;
      return windowHandle.setTheme(resolvedMode).catch(() => {
        /* browser / non-Tauri runtime */
      });
    });
  }, [resolvedMode]);

  return (
    <ThemeProvider theme={muiTheme}>
      {/*
        CssBaseline must run before our shell GlobalStyles so body typography/color
        and backgroundColor come from the same merged theme; gradient layers on top.
      */}
      <CssBaseline />
      <GlobalStyles
        styles={(theme) => {
          const isDark = theme.palette.mode === "dark";
          /** Frosted veil over the accent gradient (html); body stays transparent so blur samples the gradient */
          const frostedVeil = alpha(
            isDark ? theme.palette.common.black : theme.palette.common.white,
            isDark ? 0.2 : 0.32,
          );
          return {
            html: {
              minHeight: "100%",
              background: accentShellGradient,
              backgroundAttachment: "fixed",
              backgroundColor: theme.palette.background.default,
            },
            body: {
              minHeight: "100%",
              background: "none",
              backgroundColor: "transparent",
            },
            "#root": {
              minHeight: "100%",
              background: frostedVeil,
              backdropFilter: "blur(22px) saturate(165%)",
              WebkitBackdropFilter: "blur(22px) saturate(165%)",
            },
          };
        }}
      />
      {children}
    </ThemeProvider>
  );
}
