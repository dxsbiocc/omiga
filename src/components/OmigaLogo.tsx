/**
 * OmigaLogo — theme-adaptive Organic brushstroke icon.
 *
 * The shape matches the organic.svg design: a dramatic S-curve brushstroke
 * with watercolor wash and ripple arcs, topped by a pulsing inspiration dot.
 *
 * Colors are derived from the active MUI palette so the icon responds
 * automatically to accent preset changes and light/dark mode switches.
 * Contrast logic ensures the icon is always distinguishable from its background.
 *
 * Props:
 *   size       — pixel size of the bounding box (default 40)
 *   animated   — whether the top circle pulses (default true)
 *   primaryOverride / secondaryOverride — bypass the theme colors
 */

import { useId } from "react";
import { useTheme } from "@mui/material/styles";

interface OmigaLogoProps {
  size?: number;
  animated?: boolean;
  primaryOverride?: string;
  secondaryOverride?: string;
  style?: React.CSSProperties;
  className?: string;
}

export function OmigaLogo({
  size = 40,
  animated = true,
  primaryOverride,
  secondaryOverride,
  style,
  className,
}: OmigaLogoProps) {
  const theme = useTheme();
  const uid = useId().replace(/:/g, "");
  const isDark = theme.palette.mode === "dark";

  const color = primaryOverride ?? theme.palette.primary.main;
  const accent = secondaryOverride ?? theme.palette.secondary.main;

  // Contrast plate: subtle contrasting backdrop so the icon never blends
  // into the background. In dark mode a faint white wash; in light mode a
  // faint dark wash.
  const plateColor = isDark ? "rgba(255,255,255,0.06)" : "rgba(0,0,0,0.04)";

  // Watercolor wash opacity — slightly stronger in light mode for visibility
  const washOpacity = isDark ? 0.42 : 0.55;

  const gradBg = `omiga-grad-bg-${uid}`;
  const gradStroke = `omiga-grad-stroke-${uid}`;

  return (
    <svg
      viewBox="0 0 100 100"
      width={size}
      height={size}
      aria-label="Omiga"
      role="img"
      style={style}
      className={className}
    >
      <defs>
        {/* Watercolor wash behind the stroke */}
        <radialGradient id={gradBg} cx="50%" cy="50%" r="50%">
          <stop offset="0%" stopColor={color} stopOpacity={washOpacity} />
          <stop offset="100%" stopColor={color} stopOpacity="0" />
        </radialGradient>

        {/* Stroke gradient: primary → secondary → primary */}
        <linearGradient id={gradStroke} x1="0%" y1="0%" x2="100%" y2="0%">
          <stop offset="0%" stopColor={color} />
          <stop offset="50%" stopColor={accent} />
          <stop offset="100%" stopColor={color} />
        </linearGradient>
      </defs>

      {/* Contrast plate — keeps icon legible on any background */}
      <circle cx="50" cy="50" r="45" fill={plateColor} />

      {/* Watercolor background wash */}
      <circle cx="50" cy="50" r="45" fill={`url(#${gradBg})`} />

      {/* Main brushstroke — organic S-curve from edge to edge */}
      <path
        d="M5 85 Q 40 78, 45 30 Q 50 5, 55 30 Q 60 78, 95 85"
        fill="none"
        stroke={`url(#${gradStroke})`}
        strokeWidth="6"
        strokeLinecap="round"
        opacity="0.92"
      />

      {/* Upper ripple arc */}
      <path
        d="M38 65 Q 50 58, 62 65"
        stroke={accent}
        strokeWidth="2"
        fill="none"
        strokeLinecap="round"
        opacity={isDark ? 0.72 : 0.65}
      />

      {/* Lower ripple arc */}
      <path
        d="M42 75 Q 50 71, 58 75"
        stroke={accent}
        strokeWidth="1.5"
        fill="none"
        strokeLinecap="round"
        opacity={isDark ? 0.52 : 0.45}
      />

      {/* Top inspiration dot */}
      <circle cx="50" cy="12" r="2.5" fill={color} opacity={isDark ? 0.9 : 0.85}>
        {animated && (
          <animate
            attributeName="opacity"
            values={isDark ? "0.4;1;0.4" : "0.5;0.9;0.5"}
            dur="3s"
            repeatCount="indefinite"
          />
        )}
      </circle>
    </svg>
  );
}
