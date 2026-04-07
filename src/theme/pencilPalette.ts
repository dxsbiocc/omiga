import { useMemo } from "react";
import { useTheme } from "@mui/material/styles";

/**
 * Workspace / File Manager — light tokens (matches `pencil-new.pen` / File Manager).
 */
export const pencilLightPenPalette = {
  surface: "#FAFAFA",
  textTitle: "#0F172A",
  textPath: "#64748B",
  textFilename: "#1E293B",
  textHeader: "rgba(100, 116, 139, 0.95)",
  textSize: "rgba(100, 116, 139, 0.9)",
  textModified: "rgba(148, 163, 184, 0.95)",
  textLoading: "rgba(15, 23, 42, 0.45)",
  toolbarSurface: "rgba(15, 23, 42, 0.04)",
  toolbarBorder: "rgba(15, 23, 42, 0.06)",
  toolbarIconMuted: "rgba(15, 23, 42, 0.35)",
  toolbarIcon: "rgba(15, 23, 42, 0.45)",
  toolbarIconAccent: "rgba(15, 23, 42, 0.65)",
  toolbarIconHoverBg: "rgba(15, 23, 42, 0.08)",
  iconChipBg: "rgba(15, 23, 42, 0.04)",
  borderSubtle: "rgba(15, 23, 42, 0.06)",
  headerBar: "#FAFAFA",
  rowSelected: "rgba(99, 102, 241, 0.08)",
  rowHover: "rgba(15, 23, 42, 0.05)",
  rowHoverDir: "rgba(15, 23, 42, 0.04)",
  emptyStateIconBg: "rgba(15, 23, 42, 0.04)",
  emptyStateIcon: "rgba(100, 116, 139, 0.45)",
  errorRetryBg: "rgba(15, 23, 42, 0.06)",
  errorRetryHoverBg: "rgba(15, 23, 42, 0.1)",
  loadingSpinner: "rgba(15, 23, 42, 0.35)",
  fileIconFolder: "rgba(15, 23, 42, 0.55)",
  fileIconImage: "rgba(14, 165, 233, 0.85)",
  fileIconCode: "rgba(34, 197, 94, 0.9)",
  fileIconData: "rgba(99, 102, 241, 0.85)",
  fileIconR: "rgba(139, 92, 246, 0.88)",
  fileIconDefault: "rgba(100, 116, 139, 0.85)",
} as const;

/** Dark mode — aligned with `getTheme("dark")` slate surfaces */
export const pencilDarkPenPalette = {
  surface: "#0f172a",
  textTitle: "#f8fafc",
  textPath: "#94a3b8",
  textFilename: "#e2e8f0",
  textHeader: "rgba(148, 163, 184, 0.95)",
  textSize: "rgba(148, 163, 184, 0.9)",
  textModified: "rgba(148, 163, 184, 0.85)",
  textLoading: "rgba(248, 250, 252, 0.45)",
  toolbarSurface: "rgba(248, 250, 252, 0.06)",
  toolbarBorder: "rgba(148, 163, 184, 0.14)",
  toolbarIconMuted: "rgba(148, 163, 184, 0.45)",
  toolbarIcon: "rgba(148, 163, 184, 0.55)",
  toolbarIconAccent: "rgba(226, 232, 240, 0.88)",
  toolbarIconHoverBg: "rgba(248, 250, 252, 0.1)",
  iconChipBg: "rgba(248, 250, 252, 0.06)",
  borderSubtle: "rgba(148, 163, 184, 0.12)",
  headerBar: "#0f172a",
  rowSelected: "rgba(99, 102, 241, 0.2)",
  rowHover: "rgba(248, 250, 252, 0.06)",
  rowHoverDir: "rgba(248, 250, 252, 0.05)",
  emptyStateIconBg: "rgba(248, 250, 252, 0.06)",
  emptyStateIcon: "rgba(148, 163, 184, 0.5)",
  errorRetryBg: "rgba(248, 250, 252, 0.06)",
  errorRetryHoverBg: "rgba(248, 250, 252, 0.12)",
  loadingSpinner: "rgba(148, 163, 184, 0.55)",
  fileIconFolder: "rgba(226, 232, 240, 0.72)",
  fileIconImage: "rgba(56, 189, 248, 0.9)",
  fileIconCode: "rgba(52, 211, 153, 0.92)",
  fileIconData: "rgba(129, 140, 248, 0.9)",
  fileIconR: "rgba(192, 132, 252, 0.9)",
  fileIconDefault: "rgba(148, 163, 184, 0.88)",
} as const;

export type PencilPenPalette =
  | typeof pencilLightPenPalette
  | typeof pencilDarkPenPalette;

export function getPencilPalette(mode: "light" | "dark"): PencilPenPalette {
  return mode === "dark" ? pencilDarkPenPalette : pencilLightPenPalette;
}

/** File manager / workspace table tokens that track MUI palette mode */
export function usePencilPalette(): PencilPenPalette {
  const theme = useTheme();
  return useMemo(
    () => getPencilPalette(theme.palette.mode === "dark" ? "dark" : "light"),
    [theme.palette.mode],
  );
}

/** @deprecated use `pencilLightPenPalette` */
export const pencilNewPenPalette = pencilLightPenPalette;
