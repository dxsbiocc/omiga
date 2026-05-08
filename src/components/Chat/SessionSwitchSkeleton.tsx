/**
 * SessionSwitchSkeleton — markdown-aware chat skeleton.
 *
 * Shows during cache-miss session switches (isSwitchingSession=true).
 * Each skeleton "turn" mirrors real markdown message shapes:
 *   • User bubble  — right-aligned, compact
 *   • Agent bubble — left-aligned, richer structure:
 *       heading line, paragraph block, code block, list items
 *
 * Animations use OmigaSkeleton primitives so they share the same
 * design language (1.6 s shimmer, 80 ms stagger, reduced-motion safe).
 */

import { Box, alpha, useTheme } from "@mui/material";
import { ShimmerBox, ShimmerLines, ShimmerChip } from "../Skeletons/OmigaSkeleton";

// Bubble style helpers matching CHAT tokens ──────────────────────────────

function useBubbleColors() {
  const theme = useTheme();
  const isDark = theme.palette.mode === "dark";
  return {
    agentBg: isDark
      ? alpha(theme.palette.common.white, 0.04)
      : alpha(theme.palette.common.black, 0.03),
    agentBorder: isDark
      ? alpha(theme.palette.common.white, 0.09)
      : alpha(theme.palette.common.black, 0.08),
    userBg: isDark
      ? alpha(theme.palette.primary.main, 0.12)
      : alpha(theme.palette.primary.main, 0.07),
    userBorder: isDark
      ? alpha(theme.palette.primary.main, 0.22)
      : alpha(theme.palette.primary.main, 0.15),
    codeBg: isDark
      ? alpha(theme.palette.common.white, 0.06)
      : alpha(theme.palette.common.black, 0.04),
    codeBorder: isDark
      ? alpha(theme.palette.common.white, 0.1)
      : alpha(theme.palette.common.black, 0.07),
  };
}

// ── Bubble wrappers ──────────────────────────────────────────────────────

function UserBubbleSkeleton({
  lines,
  baseDelay = 0,
}: {
  lines: (string | number)[];
  baseDelay?: number;
}) {
  const c = useBubbleColors();
  return (
    <Box sx={{ display: "flex", justifyContent: "flex-end", width: "100%" }}>
      <Box
        sx={{
          maxWidth: "55%",
          px: 1.75,
          py: 1.25,
          borderRadius: "12px",
          border: `1px solid ${c.userBorder}`,
          bgcolor: c.userBg,
          display: "flex",
          flexDirection: "column",
          gap: "8px",
          minWidth: 80,
        }}
      >
        <ShimmerLines widths={lines} height={13} gap={8} baseDelay={baseDelay} />
      </Box>
    </Box>
  );
}

/** Heading-style shimmer line — taller, moderate width. */
function SkeletonHeading({ width = "46%", delay = 0 }: { width?: string; delay?: number }) {
  return <ShimmerBox width={width} height={18} radius={5} delay={delay} />;
}

/** Code-block skeleton — header label + indented code lines. */
function SkeletonCodeBlock({ baseDelay = 0 }: { baseDelay?: number }) {
  const c = useBubbleColors();
  return (
    <Box
      sx={{
        borderRadius: "6px",
        border: `1px solid ${c.codeBorder}`,
        bgcolor: c.codeBg,
        overflow: "hidden",
      }}
    >
      {/* Language header bar */}
      <Box
        sx={{
          px: 1.25,
          py: 0.6,
          borderBottom: `1px solid ${c.codeBorder}`,
          display: "flex",
          alignItems: "center",
          gap: 1,
        }}
      >
        <ShimmerBox width={36} height={10} radius={4} delay={baseDelay} />
      </Box>
      {/* Code lines (slightly shorter to mimic indented code) */}
      <Box sx={{ px: 1.5, py: 1, display: "flex", flexDirection: "column", gap: "9px" }}>
        <ShimmerBox width="78%" height={12} radius={4} delay={baseDelay + 80} />
        <ShimmerBox width="62%" height={12} radius={4} delay={baseDelay + 160} />
        <ShimmerBox width="40%" height={12} radius={4} delay={baseDelay + 240} />
      </Box>
    </Box>
  );
}

/** List-items skeleton — 3 indented lines with a tiny bullet dot. */
function SkeletonListItems({ baseDelay = 0 }: { baseDelay?: number }) {
  const c = useBubbleColors();
  const dotColor = alpha(c.agentBorder, 0.5);
  const widths = ["65%", "54%", "71%"];
  return (
    <Box sx={{ display: "flex", flexDirection: "column", gap: "9px", pl: 0.5 }}>
      {widths.map((w, i) => (
        <Box key={i} sx={{ display: "flex", alignItems: "center", gap: 1 }}>
          <Box
            sx={{
              width: 5,
              height: 5,
              borderRadius: "50%",
              flexShrink: 0,
              bgcolor: dotColor,
            }}
          />
          <ShimmerBox width={w} height={13} delay={baseDelay + i * 80} />
        </Box>
      ))}
    </Box>
  );
}

function AgentBubbleSkeleton({
  variant,
  baseDelay = 0,
}: {
  variant: "plain" | "rich";
  baseDelay?: number;
}) {
  const c = useBubbleColors();
  return (
    <Box sx={{ display: "flex", justifyContent: "flex-start", width: "100%" }}>
      <Box
        sx={{
          maxWidth: "72%",
          width: "100%",
          px: 1.75,
          py: 1.5,
          borderRadius: "12px",
          border: `1px solid ${c.agentBorder}`,
          bgcolor: c.agentBg,
          display: "flex",
          flexDirection: "column",
          gap: "12px",
        }}
      >
        {variant === "plain" ? (
          <ShimmerLines
            widths={["88%", "72%", "50%"]}
            height={13}
            gap={8}
            baseDelay={baseDelay}
          />
        ) : (
          <>
            {/* Heading */}
            <SkeletonHeading width="44%" delay={baseDelay} />
            {/* Paragraph */}
            <ShimmerLines
              widths={["90%", "76%", "60%"]}
              height={13}
              gap={8}
              baseDelay={baseDelay + 80}
            />
            {/* Code block */}
            <SkeletonCodeBlock baseDelay={baseDelay + 240} />
            {/* List */}
            <SkeletonListItems baseDelay={baseDelay + 560} />
          </>
        )}
        {/* Token usage chip hint */}
        <Box sx={{ display: "flex", justifyContent: "flex-end", mt: -0.5 }}>
          <ShimmerChip width={48} height={16} delay={baseDelay + 400} />
        </Box>
      </Box>
    </Box>
  );
}

// ── Public component ──────────────────────────────────────────────────────

export function SessionSwitchSkeleton() {
  return (
    <Box
      sx={{
        display: "flex",
        flexDirection: "column",
        gap: 3,
        width: "100%",
        pt: 0.5,
      }}
    >
      {/* Turn 1: user query */}
      <UserBubbleSkeleton lines={["68%"]} baseDelay={0} />

      {/* Turn 2: plain assistant reply */}
      <AgentBubbleSkeleton variant="plain" baseDelay={60} />

      {/* Turn 3: follow-up user message */}
      <UserBubbleSkeleton lines={["52%", "38%"]} baseDelay={120} />

      {/* Turn 4: rich assistant reply (heading + code + list) */}
      <AgentBubbleSkeleton variant="rich" baseDelay={180} />
    </Box>
  );
}
