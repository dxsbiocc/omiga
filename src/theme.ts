import { createTheme } from "@mui/material/styles";

/**
 * Workspace / File Manager — matches `pencil-new.pen` (File Manager `avZs4`).
 * Use these tokens anywhere you want the same look as the design file.
 */
export const pencilNewPenPalette = {
  surface: "#FAFAFA",
  textTitle: "#0F172A",
  textPath: "#64748B",
  /** Primary filename color in the file list */
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
  /** 与 surface 一致的不透明底；sticky 表头若用半透明会与滚动内容叠字 */
  headerBar: "#FAFAFA",
  rowSelected: "rgba(99, 102, 241, 0.08)",
  rowHover: "rgba(15, 23, 42, 0.05)",
  rowHoverDir: "rgba(15, 23, 42, 0.04)",
  emptyStateIconBg: "rgba(15, 23, 42, 0.04)",
  emptyStateIcon: "rgba(100, 116, 139, 0.45)",
  errorRetryBg: "rgba(15, 23, 42, 0.06)",
  errorRetryHoverBg: "rgba(15, 23, 42, 0.1)",
  loadingSpinner: "rgba(15, 23, 42, 0.35)",
  /** File-type tint on icon chips (subtle, not rainbow) */
  fileIconFolder: "rgba(15, 23, 42, 0.55)",
  fileIconImage: "rgba(14, 165, 233, 0.85)",
  fileIconCode: "rgba(34, 197, 94, 0.9)",
  fileIconData: "rgba(99, 102, 241, 0.85)",
  fileIconR: "rgba(139, 92, 246, 0.88)",
  fileIconDefault: "rgba(100, 116, 139, 0.85)",
} as const;

// OmicsAgent Design System - Based on pencil.pen
export const theme = createTheme({
  palette: {
    mode: "light",
    primary: {
      main: "#6366f1", // Indigo
      light: "#818cf8",
      dark: "#4f46e5",
      contrastText: "#ffffff",
    },
    secondary: {
      main: "#a855f7", // Purple
      light: "#c084fc",
      dark: "#9333ea",
    },
    background: {
      default: "#F5F5F7", // Light gray background
      paper: "#FFFFFF",
    },
    text: {
      primary: "#1C1C1E", // Dark text
      secondary: "#6C6C70", // Secondary text
      disabled: "#AEAEB2", // Placeholder text
    },
    divider: "#E5E5EA",
    success: {
      main: "#34C759", // Green
      light: "#E9FBF0",
    },
    warning: {
      main: "#FF9500", // Orange
    },
    error: {
      main: "#FF3B30", // Red
    },
    info: {
      main: "#6366f1",
    },
  },
  typography: {
    fontFamily: '"Inter", -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif',
    h6: {
      fontWeight: 600,
    },
    subtitle1: {
      fontWeight: 600,
    },
    body1: {
      fontSize: 14,
      lineHeight: 1.5,
    },
    body2: {
      fontSize: 13,
      lineHeight: 1.5,
    },
    caption: {
      fontSize: 11,
      letterSpacing: 0.5,
    },
  },
  shape: {
    borderRadius: 8,
  },
  components: {
    MuiCssBaseline: {
      styleOverrides: {
        body: {
          scrollbarWidth: "thin",
          "&::-webkit-scrollbar": {
            width: "6px",
            height: "6px",
          },
          "&::-webkit-scrollbar-track": {
            background: "transparent",
          },
          "&::-webkit-scrollbar-thumb": {
            background: "#C1C1C5",
            borderRadius: "3px",
          },
          "&::-webkit-scrollbar-thumb:hover": {
            background: "#8E8E93",
          },
        },
      },
    },
    MuiIconButton: {
      styleOverrides: {
        root: {
          borderRadius: 8,
        },
      },
    },
    MuiPaper: {
      styleOverrides: {
        root: {
          backgroundImage: "none",
        },
      },
    },
  },
});
