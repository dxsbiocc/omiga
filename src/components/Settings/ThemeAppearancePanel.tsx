import {
  Box,
  Grid,
  Stack,
  Typography,
  alpha,
  useTheme,
  type Theme,
} from "@mui/material";
import {
  DarkMode,
  LightMode,
  SettingsBrightness,
} from "@mui/icons-material";
import type { ColorModePreference } from "../../state/themeStore";
import {
  ACCENT_PRESET_IDS,
  ACCENT_PRESET_META,
  getAccentSwatchGradient,
  getColorModePickerGradient,
  type AccentPresetId,
} from "../../theme/accentPresets";

export interface ThemeAppearancePanelProps {
  colorMode: ColorModePreference;
  onColorModeChange: (mode: ColorModePreference) => void;
  accentPreset: AccentPresetId;
  onAccentPresetChange: (preset: AccentPresetId) => void;
}

const MODE_ORDER: ColorModePreference[] = ["light", "dark", "system"];

const MODE_ICONS = {
  light: LightMode,
  dark: DarkMode,
  system: SettingsBrightness,
} as const;

const MODE_LABELS: Record<ColorModePreference, string> = {
  light: "Light",
  dark: "Dark",
  system: "System",
};

const MODE_HINTS: Record<ColorModePreference, string> = {
  light: "Always use light chrome",
  dark: "Always use dark chrome",
  system: "Match macOS / Windows appearance",
};

function glassPanelSx(isDark: boolean) {
  return {
    background: isDark
      ? alpha("#0f172a", 0.78)
      : alpha("#ffffff", 0.86),
    backdropFilter: "blur(12px)",
    WebkitBackdropFilter: "blur(12px)",
    border: "1px solid",
    borderColor: isDark ? alpha("#fff", 0.08) : alpha("#0f172a", 0.06),
  } as const;
}

/** Shared selected / idle / hover shadows for appearance mode tiles and accent preset rows */
function themePickCardShadow(
  theme: Theme,
  isDark: boolean,
  selected: boolean,
  hover: boolean,
): string {
  const p = theme.palette.primary.main;
  if (selected) {
    return hover
      ? `0 0 0 2px ${p}, 0 18px 40px ${alpha(p, 0.3)}`
      : `0 0 0 2px ${p}, 0 12px 28px ${alpha(p, 0.22)}`;
  }
  return hover
    ? `0 1px 0 ${alpha("#000", 0.08)}, 0 14px 36px ${alpha("#000", isDark ? 0.45 : 0.14)}`
    : `0 1px 0 ${alpha("#000", 0.06)}, 0 8px 24px ${alpha("#000", isDark ? 0.35 : 0.08)}`;
}

const pickCardTransition =
  "box-shadow 0.2s cubic-bezier(0.4, 0, 0.2, 1), transform 0.2s cubic-bezier(0.4, 0, 0.2, 1)";

export function ThemeAppearancePanel({
  colorMode,
  onColorModeChange,
  accentPreset,
  onAccentPresetChange,
}: ThemeAppearancePanelProps) {
  const theme = useTheme();
  const isDark = theme.palette.mode === "dark";

  return (
    <Stack spacing={3} sx={{ maxWidth: 640 }}>
      <Box>
        <Typography
          variant="overline"
          sx={{ letterSpacing: "0.12em", color: "text.secondary", fontWeight: 700 }}
        >
          Interface
        </Typography>
        <Typography variant="h6" sx={{ mt: 0.5, mb: 0.5, fontWeight: 700 }}>
          Appearance
        </Typography>
        <Typography variant="body2" color="text.secondary" sx={{ mb: 2, lineHeight: 1.65 }}>
          Choose light or dark UI, or follow the OS. Saved locally for the next launch.
        </Typography>

        <Grid container spacing={1.5}>
          {MODE_ORDER.map((mode) => {
            const selected = colorMode === mode;
            const Icon = MODE_ICONS[mode];
            return (
              <Grid item xs={12} sm={4} key={mode}>
                <Box
                  component="button"
                  type="button"
                  onClick={() => onColorModeChange(mode)}
                  aria-pressed={selected}
                  aria-label={`${MODE_LABELS[mode]} theme`}
                  sx={{
                    width: "100%",
                    cursor: "pointer",
                    textAlign: "left",
                    border: "none",
                    p: 0,
                    borderRadius: 3,
                    overflow: "hidden",
                    position: "relative",
                    minHeight: 112,
                    background: getColorModePickerGradient(mode),
                    boxShadow: themePickCardShadow(theme, isDark, selected, false),
                    transform: "translateZ(0)",
                    transition: pickCardTransition,
                    "@media (prefers-reduced-motion: reduce)": {
                      transition: "none",
                      "&:hover": { transform: "none" },
                    },
                    "&:hover": {
                      transform: "translateY(-3px)",
                      boxShadow: themePickCardShadow(theme, isDark, selected, true),
                    },
                    "&:active": {
                      transform: "translateY(-1px)",
                      transitionDuration: "0.12s",
                    },
                    "&:focus-visible": {
                      outline: `2px solid ${theme.palette.primary.main}`,
                      outlineOffset: 2,
                    },
                  }}
                >
                  <Stack
                    sx={{
                      ...glassPanelSx(isDark),
                      p: 1.5,
                      height: "100%",
                      minHeight: 112,
                    }}
                  >
                    <Stack direction="row" alignItems="center" spacing={1} sx={{ mb: 0.75 }}>
                      <Box
                        sx={{
                          width: 36,
                          height: 36,
                          borderRadius: 2,
                          display: "flex",
                          alignItems: "center",
                          justifyContent: "center",
                          bgcolor: selected
                            ? alpha(theme.palette.primary.main, 0.18)
                            : alpha(theme.palette.text.primary, 0.06),
                          color: selected ? "primary.main" : "text.secondary",
                        }}
                      >
                        <Icon sx={{ fontSize: 20 }} />
                      </Box>
                      <Typography variant="subtitle2" fontWeight={800} color="text.primary">
                        {MODE_LABELS[mode]}
                      </Typography>
                    </Stack>
                    <Typography variant="caption" color="text.secondary" sx={{ lineHeight: 1.45 }}>
                      {MODE_HINTS[mode]}
                    </Typography>
                  </Stack>
                </Box>
              </Grid>
            );
          })}
        </Grid>
      </Box>

      <Box>
        <Typography
          variant="overline"
          sx={{ letterSpacing: "0.12em", color: "text.secondary", fontWeight: 700 }}
        >
          Accent
        </Typography>
        <Typography variant="h6" sx={{ mt: 0.5, mb: 0.5, fontWeight: 700 }}>
          Color palette
        </Typography>
        <Typography variant="body2" color="text.secondary" sx={{ mb: 2, lineHeight: 1.65 }}>
          Each preset maps brand-inspired swatches to primary, secondary, and semantic colors across the app.
        </Typography>

        <Stack spacing={1.75}>
          {ACCENT_PRESET_IDS.map((id) => {
            const meta = ACCENT_PRESET_META[id];
            const bg = getAccentSwatchGradient(id, isDark ? "dark" : "light");
            const selected = accentPreset === id;
            return (
              <Box
                key={id}
                component="button"
                type="button"
                onClick={() => onAccentPresetChange(id)}
                aria-pressed={selected}
                aria-label={`${meta.label} accent`}
                sx={{
                  width: "100%",
                  cursor: "pointer",
                  textAlign: "left",
                  border: "none",
                  p: 0,
                  borderRadius: 3,
                  overflow: "hidden",
                  position: "relative",
                  background: bg,
                  boxShadow: themePickCardShadow(theme, isDark, selected, false),
                  transform: "translateZ(0)",
                  transition: pickCardTransition,
                  "@media (prefers-reduced-motion: reduce)": {
                    transition: "none",
                    "&:hover": { transform: "none" },
                  },
                  "&:hover": {
                    transform: "translateY(-3px)",
                    boxShadow: themePickCardShadow(theme, isDark, selected, true),
                  },
                  "&:active": {
                    transform: "translateY(-1px)",
                    transitionDuration: "0.12s",
                  },
                  "&:focus-visible": {
                    outline: `2px solid ${theme.palette.primary.main}`,
                    outlineOffset: 2,
                  },
                }}
              >
                <Stack
                  direction={{ xs: "column", sm: "row" }}
                  alignItems={{ xs: "stretch", sm: "center" }}
                  spacing={2}
                  sx={{
                    ...glassPanelSx(isDark),
                    p: 2,
                    gap: 2,
                    pr: selected ? { xs: 9, sm: 10 } : 2,
                  }}
                >
                  <Box
                    sx={{
                      width: { xs: "100%", sm: 136 },
                      flexShrink: 0,
                      display: "flex",
                      flexDirection: "row",
                      flexWrap: "wrap",
                      gap: 0.75,
                      alignItems: "center",
                      alignContent: "center",
                    }}
                  >
                    {meta.swatches.map((hex) => (
                      <Box
                        key={hex}
                        sx={{
                          width: 14,
                          height: 14,
                          borderRadius: "50%",
                          flexShrink: 0,
                          bgcolor: hex,
                        }}
                        title={hex}
                      />
                    ))}
                  </Box>
                  <Box sx={{ minWidth: 0, flex: 1, textAlign: "left" }}>
                    <Typography
                      variant="subtitle1"
                      fontWeight={800}
                      color="text.primary"
                      sx={{ textAlign: "left" }}
                    >
                      {meta.label}
                    </Typography>
                    <Typography
                      variant="body2"
                      color="text.secondary"
                      sx={{ mt: 0.35, lineHeight: 1.55, textAlign: "left" }}
                    >
                      {meta.description}
                    </Typography>
                  </Box>
                  {selected && (
                    <Box
                      sx={{
                        position: "absolute",
                        top: 12,
                        right: 12,
                        zIndex: 1,
                        px: 1.25,
                        py: 0.35,
                        borderRadius: 10,
                        bgcolor: alpha(theme.palette.primary.main, 0.16),
                        color: "primary.main",
                        typography: "caption",
                        fontWeight: 800,
                        letterSpacing: "0.04em",
                        textTransform: "uppercase",
                      }}
                    >
                      Active
                    </Box>
                  )}
                </Stack>
              </Box>
            );
          })}
        </Stack>
      </Box>
    </Stack>
  );
}
