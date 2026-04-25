/**
 * TaskStatusSkeleton — shown in the task panel while a session is loading.
 *
 * Mirrors the visual structure of RunningTaskCard / RunningAgentCard:
 *   • Left accent bar (3px)
 *   • Status chip + timer icon row
 *   • Task name / description lines
 *
 * Staggered shimmer gives the panel a purposeful "booting up" feel
 * instead of an abrupt empty → content snap.
 */

import { Box, Stack, alpha, useTheme } from "@mui/material";
import { Timer } from "@mui/icons-material";
import {
  ShimmerBox,
  ShimmerChip,
  ShimmerLines,
} from "../Skeletons/OmigaSkeleton";

// ── Single card skeleton ──────────────────────────────────────────────────

function TaskCardSkeleton({
  accent = "#6366f1",
  baseDelay = 0,
}: {
  accent?: string;
  baseDelay?: number;
}) {
  const theme = useTheme();
  return (
    <Box
      sx={{
        p: 1.25,
        borderRadius: 1.5,
        bgcolor: alpha(accent, 0.06),
        border: `1px solid ${alpha(accent, 0.18)}`,
        position: "relative",
        overflow: "hidden",
        "&::before": {
          content: '""',
          position: "absolute",
          left: 0,
          top: 0,
          bottom: 0,
          width: 3,
          bgcolor: alpha(accent, 0.5),
          borderRadius: "3px 0 0 3px",
        },
        pl: "calc(1.25 * 8px + 3px + 8px)", // offset for accent bar
      }}
    >
      {/* Status row */}
      <Stack
        direction="row"
        alignItems="center"
        justifyContent="space-between"
        sx={{ mb: 0.75 }}
      >
        <Stack direction="row" alignItems="center" spacing={0.75}>
          <ShimmerChip width={54} height={20} delay={baseDelay} />
        </Stack>
        <Timer sx={{ fontSize: 14, color: alpha(theme.palette.text.primary, 0.2) }} />
      </Stack>
      {/* Task name */}
      <ShimmerBox width="72%" height={13} radius={5} delay={baseDelay + 80} />
      {/* Optional description line */}
      <ShimmerBox
        width="55%"
        height={11}
        radius={5}
        delay={baseDelay + 160}
        sx={{ mt: "8px", opacity: 0.7 }}
      />
    </Box>
  );
}

// ── Execution-step row skeleton ───────────────────────────────────────────

function StepRowSkeleton({ baseDelay = 0 }: { baseDelay?: number }) {
  const theme = useTheme();
  return (
    <Box
      sx={{
        display: "flex",
        alignItems: "center",
        gap: 1,
        py: 0.5,
        px: 0.5,
      }}
    >
      {/* Status dot */}
      <Box
        sx={{
          width: 8,
          height: 8,
          borderRadius: "50%",
          flexShrink: 0,
          bgcolor: alpha(theme.palette.text.primary, 0.1),
        }}
      />
      <ShimmerBox width="60%" height={12} radius={5} delay={baseDelay} />
    </Box>
  );
}

// ── Public component ──────────────────────────────────────────────────────

export function TaskStatusSkeleton() {
  return (
    <Box
      sx={{
        display: "flex",
        flexDirection: "column",
        gap: 1,
        p: 1.5,
      }}
    >
      {/* Section header ghost */}
      <ShimmerLines
        widths={["35%"]}
        height={11}
        gap={0}
        baseDelay={0}
      />

      {/* Two running-card skeletons */}
      <TaskCardSkeleton accent="#6366f1" baseDelay={40} />
      <TaskCardSkeleton accent="#0ea5e9" baseDelay={120} />

      {/* Execution step rows */}
      <Box sx={{ mt: 0.5 }}>
        <StepRowSkeleton baseDelay={200} />
        <StepRowSkeleton baseDelay={280} />
        <StepRowSkeleton baseDelay={360} />
      </Box>
    </Box>
  );
}
