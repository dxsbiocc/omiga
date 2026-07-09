import { memo } from "react";
import { Box, Typography } from "@mui/material";
import { alpha, useTheme } from "@mui/material/styles";

function formatDurationMs(ms: number): string {
  if (!Number.isFinite(ms) || ms < 0) return "0:00";
  const totalSeconds = Math.floor(ms / 1000);
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  return `${minutes}:${seconds.toString().padStart(2, "0")}`;
}

export interface TurnCompleteIndicatorProps {
  durationMs: number;
  label?: string;
}

export const TurnCompleteIndicator = memo(function TurnCompleteIndicator({
  durationMs,
  label,
}: TurnCompleteIndicatorProps) {
  const theme = useTheme();
  const isDark = theme.palette.mode === "dark";
  const lineColor = alpha(
    isDark ? theme.palette.common.white : theme.palette.common.black,
    isDark ? 0.14 : 0.1,
  );
  const textColor = theme.palette.text.secondary;

  return (
    <Box
      role="status"
      aria-live="polite"
      sx={{
        display: "flex",
        alignItems: "center",
        gap: 1.25,
        width: "100%",
        py: 0.75,
        px: 0.5,
      }}
    >
      <Box
        aria-hidden
        sx={{
          flex: 1,
          height: "1px",
          background: `linear-gradient(90deg, transparent, ${lineColor})`,
        }}
      />
      <Typography
        variant="caption"
        sx={{
          flexShrink: 0,
          fontSize: 11,
          fontWeight: 600,
          letterSpacing: "0.02em",
          color: textColor,
          whiteSpace: "nowrap",
        }}
      >
        {label ?? `完成 · ${formatDurationMs(durationMs)}`}
      </Typography>
      <Box
        aria-hidden
        sx={{
          flex: 1,
          height: "1px",
          background: `linear-gradient(270deg, transparent, ${lineColor})`,
        }}
      />
    </Box>
  );
});
