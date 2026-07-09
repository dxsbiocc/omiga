import { memo, useEffect, useState } from "react";
import { Box, CircularProgress, Typography } from "@mui/material";
import { alpha, useTheme } from "@mui/material/styles";

function formatDurationMs(ms: number): string {
  if (!Number.isFinite(ms) || ms < 0) return "0:00";
  const totalSeconds = Math.floor(ms / 1000);
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  return `${minutes}:${seconds.toString().padStart(2, "0")}`;
}

export interface AgentWorkingIndicatorProps {
  active: boolean;
  startedAt?: number | null;
  label?: string | null;
}

export const AgentWorkingIndicator = memo(function AgentWorkingIndicator({
  active,
  startedAt = null,
  label,
}: AgentWorkingIndicatorProps) {
  const theme = useTheme();
  const isDark = theme.palette.mode === "dark";
  const [elapsedMs, setElapsedMs] = useState(0);

  useEffect(() => {
    if (!active || !startedAt) {
      setElapsedMs(0);
      return undefined;
    }
    setElapsedMs(Date.now() - startedAt);
    const timer = window.setInterval(() => {
      setElapsedMs(Date.now() - startedAt);
    }, 1000);
    return () => window.clearInterval(timer);
  }, [active, startedAt]);

  if (!active) return null;

  return (
    <Box
      sx={{
        display: "inline-flex",
        alignItems: "center",
        gap: 1,
        px: 1.25,
        py: 0.5,
        borderRadius: 999,
        border: `1px solid ${alpha(theme.palette.primary.main, isDark ? 0.28 : 0.2)}`,
        bgcolor: alpha(theme.palette.primary.main, isDark ? 0.1 : 0.06),
        maxWidth: "100%",
      }}
    >
      <CircularProgress size={12} thickness={5} sx={{ color: "primary.main" }} />
      <Typography
        variant="caption"
        sx={{
          fontSize: 11,
          fontWeight: 600,
          color: "text.secondary",
          fontVariantNumeric: "tabular-nums",
        }}
      >
        {formatDurationMs(elapsedMs)}
      </Typography>
      <Typography
        variant="caption"
        noWrap
        sx={{
          fontSize: 11,
          fontWeight: 600,
          color: "text.primary",
          backgroundImage: isDark
            ? `linear-gradient(90deg, ${theme.palette.text.primary} 0%, ${alpha(theme.palette.primary.light, 0.85)} 50%, ${theme.palette.text.primary} 100%)`
            : undefined,
          backgroundSize: isDark ? "200% auto" : undefined,
          WebkitBackgroundClip: isDark ? "text" : undefined,
          WebkitTextFillColor: isDark ? "transparent" : undefined,
          animation: isDark ? "omigaWorkingShimmer 2.2s ease-in-out infinite" : undefined,
          "@keyframes omigaWorkingShimmer": {
            "0%": { backgroundPosition: "200% center" },
            "100%": { backgroundPosition: "-200% center" },
          },
        }}
      >
        {label?.trim() || "处理中…"}
      </Typography>
    </Box>
  );
});
