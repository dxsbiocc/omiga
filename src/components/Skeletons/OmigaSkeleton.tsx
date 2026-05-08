/**
 * OmigaSkeleton — shared shimmer primitive for all loading states.
 *
 * Design language (from ui-ux-pro-max / Dark OLED):
 *   base  : alpha(text.primary, 0.07) — barely visible trough
 *   peak  : alpha(text.primary, 0.14) — subtle highlight wave
 *   sweep : background-position 100%→-100%, 1.6s ease-in-out infinite
 *   stagger: each line adds +80ms delay for a cascading waterfall feel
 *
 * All animations are gated behind `prefers-reduced-motion: no-preference`
 * so users with vestibular/motion sensitivities see static shapes instead.
 *
 * Performance: only `background-position` is animated → GPU composited,
 * zero layout repaints.
 */

import { Box, useTheme } from "@mui/material";
import { alpha } from "@mui/material/styles";
import type { SxProps, Theme } from "@mui/material/styles";

// ── Keyframe definition (injected once by Emotion) ────────────────────────
const SHIMMER_KF = {
  "@keyframes omigaShimmer": {
    "0%":   { backgroundPosition: "100% 0" },
    "100%": { backgroundPosition: "-100% 0" },
  },
} as const;

// ── Base ShimmerBox ───────────────────────────────────────────────────────

export interface ShimmerBoxProps {
  width?: string | number;
  height?: number;
  /** Border-radius in px (default 6) */
  radius?: number;
  /** Animation delay in ms (use for stagger) */
  delay?: number;
  sx?: SxProps<Theme>;
}

/**
 * A single shimmer rectangle.
 * Compose multiple ShimmerBox instances to build any skeleton layout.
 */
export function ShimmerBox({
  width = "100%",
  height = 14,
  radius = 6,
  delay = 0,
  sx,
}: ShimmerBoxProps) {
  const theme = useTheme();
  const base = alpha(theme.palette.text.primary, 0.07);
  const peak = alpha(theme.palette.text.primary, 0.14);

  return (
    <Box
      sx={[
        {
          width,
          height,
          borderRadius: `${radius}px`,
          flexShrink: 0,
          background: `linear-gradient(90deg, ${base} 0%, ${peak} 40%, ${base} 80%)`,
          backgroundSize: "300% 100%",
          backgroundPosition: "100% 0",
          "@media (prefers-reduced-motion: no-preference)": {
            ...SHIMMER_KF,
            animation: `omigaShimmer 1.6s ease-in-out ${delay}ms infinite`,
          },
        },
        ...(Array.isArray(sx) ? sx : [sx]),
      ]}
    />
  );
}

// ── Convenience wrappers ──────────────────────────────────────────────────

/** A circle (avatar / icon placeholder). */
export function ShimmerCircle({
  size = 24,
  delay = 0,
  sx,
}: {
  size?: number;
  delay?: number;
  sx?: SxProps<Theme>;
}) {
  return (
    <ShimmerBox
      width={size}
      height={size}
      radius={size / 2}
      delay={delay}
      sx={sx}
    />
  );
}

/** A rounded square (icon chip placeholder). */
export function ShimmerIconChip({
  size = 24,
  radius = 6,
  delay = 0,
}: {
  size?: number;
  radius?: number;
  delay?: number;
}) {
  return <ShimmerBox width={size} height={size} radius={radius} delay={delay} />;
}

/** A pill / chip skeleton. */
export function ShimmerChip({
  width = 56,
  height = 20,
  delay = 0,
}: {
  width?: number;
  height?: number;
  delay?: number;
}) {
  return <ShimmerBox width={width} height={height} radius={height / 2} delay={delay} />;
}

// ── Multi-line stack helper ───────────────────────────────────────────────

/**
 * Renders a column of shimmer lines with automatic 80 ms stagger.
 * `widths` is an array of width values (e.g. ["88%", "72%", "50%"]).
 */
export function ShimmerLines({
  widths,
  height = 14,
  gap = 8,
  baseDelay = 0,
  stagger = 80,
}: {
  widths: (string | number)[];
  height?: number;
  gap?: number;
  baseDelay?: number;
  stagger?: number;
}) {
  return (
    <Box sx={{ display: "flex", flexDirection: "column", gap: `${gap}px` }}>
      {widths.map((w, i) => (
        <ShimmerBox
          key={i}
          width={w}
          height={height}
          delay={baseDelay + i * stagger}
        />
      ))}
    </Box>
  );
}
