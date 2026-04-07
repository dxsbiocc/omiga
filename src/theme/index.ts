import { createTheme, ThemeOptions } from "@mui/material/styles";

// Modern dark theme for Omiga - IDE-like appearance
export const getTheme = (mode: "light" | "dark" = "dark"): ThemeOptions => {
  const isDark = mode === "dark";

  return {
    palette: {
      mode,
      primary: {
        main: "#6366f1", // Indigo
        light: "#818cf8",
        dark: "#4f46e5",
        contrastText: "#ffffff",
      },
      secondary: {
        main: "#10b981", // Emerald
        light: "#34d399",
        dark: "#059669",
        contrastText: "#ffffff",
      },
      background: {
        default: isDark ? "#0f172a" : "#f8fafc", // Slate 900 / Slate 50
        paper: isDark ? "#1e293b" : "#ffffff", // Slate 800 / White
      },
      surface: {
        main: isDark ? "#334155" : "#f1f5f9", // Slate 700 / Slate 100
        light: isDark ? "#475569" : "#e2e8f0", // Slate 600 / Slate 200
        dark: isDark ? "#1e293b" : "#cbd5e1", // Slate 800 / Slate 300
      },
      text: {
        primary: isDark ? "#f8fafc" : "#0f172a", // Slate 50 / Slate 900
        secondary: isDark ? "#94a3b8" : "#64748b", // Slate 400 / Slate 500
      },
      divider: isDark ? "rgba(148, 163, 184, 0.12)" : "rgba(15, 23, 42, 0.08)",
      error: {
        main: "#ef4444",
        light: "#f87171",
        dark: "#dc2626",
      },
      warning: {
        main: "#f59e0b",
        light: "#fbbf24",
        dark: "#d97706",
      },
      info: {
        main: "#3b82f6",
        light: "#60a5fa",
        dark: "#2563eb",
      },
      success: {
        main: "#10b981",
        light: "#34d399",
        dark: "#059669",
      },
    },
    typography: {
      fontFamily: '"Inter", "Roboto", "Helvetica", "Arial", sans-serif',
      fontSize: 14,
      fontWeightLight: 300,
      fontWeightRegular: 400,
      fontWeightMedium: 500,
      fontWeightBold: 600,
      h1: {
        fontSize: "2rem",
        fontWeight: 600,
        letterSpacing: "-0.025em",
      },
      h2: {
        fontSize: "1.5rem",
        fontWeight: 600,
        letterSpacing: "-0.025em",
      },
      h3: {
        fontSize: "1.25rem",
        fontWeight: 600,
        letterSpacing: "-0.025em",
      },
      h4: {
        fontSize: "1.125rem",
        fontWeight: 600,
        letterSpacing: "-0.025em",
      },
      h5: {
        fontSize: "1rem",
        fontWeight: 600,
        letterSpacing: "-0.025em",
      },
      h6: {
        fontSize: "0.875rem",
        fontWeight: 600,
        letterSpacing: "-0.025em",
      },
      subtitle1: {
        fontSize: "1rem",
        fontWeight: 500,
        letterSpacing: "-0.025em",
      },
      subtitle2: {
        fontSize: "0.875rem",
        fontWeight: 500,
        letterSpacing: "-0.025em",
      },
      body1: {
        fontSize: "0.875rem",
        lineHeight: 1.6,
        letterSpacing: "-0.01em",
      },
      body2: {
        fontSize: "0.8125rem",
        lineHeight: 1.5,
        letterSpacing: "-0.01em",
      },
      button: {
        fontSize: "0.875rem",
        fontWeight: 500,
        letterSpacing: "0",
        textTransform: "none",
      },
      caption: {
        fontSize: "0.75rem",
        lineHeight: 1.5,
        letterSpacing: "0",
      },
      overline: {
        fontSize: "0.75rem",
        fontWeight: 500,
        letterSpacing: "0.05em",
        textTransform: "uppercase",
      },
    },
    shape: {
      borderRadius: 8,
    },
    shadows: [
      "none",
      "0 1px 2px 0 rgba(0, 0, 0, 0.3)",
      "0 1px 3px 0 rgba(0, 0, 0, 0.4)",
      "0 4px 6px -1px rgba(0, 0, 0, 0.4)",
      "0 10px 15px -3px rgba(0, 0, 0, 0.4)",
      "0 20px 25px -5px rgba(0, 0, 0, 0.4)",
      "0 25px 50px -12px rgba(0, 0, 0, 0.5)",
      "0 25px 50px -12px rgba(0, 0, 0, 0.5)",
      "0 25px 50px -12px rgba(0, 0, 0, 0.5)",
      "0 25px 50px -12px rgba(0, 0, 0, 0.5)",
      "0 25px 50px -12px rgba(0, 0, 0, 0.5)",
      "0 25px 50px -12px rgba(0, 0, 0, 0.5)",
      "0 25px 50px -12px rgba(0, 0, 0, 0.5)",
      "0 25px 50px -12px rgba(0, 0, 0, 0.5)",
      "0 25px 50px -12px rgba(0, 0, 0, 0.5)",
      "0 25px 50px -12px rgba(0, 0, 0, 0.5)",
      "0 25px 50px -12px rgba(0, 0, 0, 0.5)",
      "0 25px 50px -12px rgba(0, 0, 0, 0.5)",
      "0 25px 50px -12px rgba(0, 0, 0, 0.5)",
      "0 25px 50px -12px rgba(0, 0, 0, 0.5)",
      "0 25px 50px -12px rgba(0, 0, 0, 0.5)",
      "0 25px 50px -12px rgba(0, 0, 0, 0.5)",
      "0 25px 50px -12px rgba(0, 0, 0, 0.5)",
      "0 25px 50px -12px rgba(0, 0, 0, 0.5)",
      "0 25px 50px -12px rgba(0, 0, 0, 0.5)",
    ],
    components: {
      MuiCssBaseline: {
        styleOverrides: `
          @import url('https://fonts.googleapis.com/css2?family=Inter:wght@300;400;500;600;700&family=JetBrains+Mono:wght@400;500;600&display=swap');
          
          * {
            scrollbar-width: thin;
            scrollbar-color: ${isDark ? "rgba(148, 163, 184, 0.3) transparent" : "rgba(15, 23, 42, 0.2) transparent"};
          }
          
          *::-webkit-scrollbar {
            width: 6px;
            height: 6px;
          }
          
          *::-webkit-scrollbar-track {
            background: transparent;
          }
          
          *::-webkit-scrollbar-thumb {
            background-color: ${isDark ? "rgba(148, 163, 184, 0.3)" : "rgba(15, 23, 42, 0.2)"};
            border-radius: 20px;
          }
          
          *::-webkit-scrollbar-thumb:hover {
            background-color: ${isDark ? "rgba(148, 163, 184, 0.5)" : "rgba(15, 23, 42, 0.3)"};
          }
          
          html, body, #root {
            height: 100%;
            overflow: hidden;
          }
        `,
      },
      MuiButton: {
        styleOverrides: {
          root: {
            borderRadius: 8,
            textTransform: "none",
            fontWeight: 500,
            padding: "8px 16px",
          },
          contained: {
            boxShadow: "none",
            "&:hover": {
              boxShadow: "0 1px 3px 0 rgba(0, 0, 0, 0.1)",
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
      MuiAppBar: {
        styleOverrides: {
          root: {
            boxShadow: "none",
            borderBottom: `1px solid ${isDark ? "rgba(148, 163, 184, 0.12)" : "rgba(15, 23, 42, 0.08)"}`,
          },
        },
      },
      MuiDrawer: {
        styleOverrides: {
          root: {
            borderRight: "none",
          },
          paper: {
            borderRight: `1px solid ${isDark ? "rgba(148, 163, 184, 0.12)" : "rgba(15, 23, 42, 0.08)"}`,
          },
        },
      },
      MuiListItem: {
        styleOverrides: {
          root: {
            borderRadius: 6,
            margin: "2px 8px",
            padding: "6px 12px",
          },
        },
      },
      MuiListItemButton: {
        styleOverrides: {
          root: {
            borderRadius: 6,
            margin: "2px 8px",
            padding: "6px 12px",
          },
        },
      },
      MuiChip: {
        styleOverrides: {
          root: {
            borderRadius: 6,
            fontWeight: 500,
          },
        },
      },
      MuiTooltip: {
        styleOverrides: {
          tooltip: {
            borderRadius: 6,
            fontSize: "0.75rem",
            fontWeight: 500,
            padding: "6px 10px",
          },
        },
      },
      MuiTextField: {
        styleOverrides: {
          root: {
            "& .MuiOutlinedInput-root": {
              borderRadius: 8,
            },
          },
        },
      },
      MuiDialog: {
        styleOverrides: {
          paper: {
            borderRadius: 12,
          },
        },
      },
    },
  };
};

// Create the default dark theme
export const theme = createTheme(getTheme("dark"));

// Export light theme for potential use
export const lightTheme = createTheme(getTheme("light"));

// Type augmentation for custom palette properties
declare module "@mui/material/styles" {
  interface Palette {
    surface: Palette["primary"];
  }
  interface PaletteOptions {
    surface?: PaletteOptions["primary"];
  }
}
