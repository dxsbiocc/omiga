/**
 * OmigaLogo — theme-adaptive Momentum brushstroke icon.
 *
 * Colors are derived from the active MUI palette so the icon responds
 * automatically to accent preset changes and light/dark mode switches.
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

  const color = primaryOverride ?? theme.palette.primary.main;
  const accent = secondaryOverride ?? theme.palette.secondary.main;

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
          <stop offset="0%" stopColor={color} stopOpacity="0.35" />
          <stop offset="100%" stopColor={color} stopOpacity="0" />
        </radialGradient>

        {/* Stroke gradient: primary → secondary → primary */}
        <linearGradient id={gradStroke} x1="0%" y1="0%" x2="100%" y2="0%">
          <stop offset="0%" stopColor={color} />
          <stop offset="50%" stopColor={accent} />
          <stop offset="100%" stopColor={color} />
        </linearGradient>
      </defs>

      {/* Watercolor background circle */}
      <circle cx="50" cy="45" r="35" fill={`url(#${gradBg})`} />

      {/* Main brushstroke — omega/momentum shape */}
      <path
        d="M15 80 Q 40 75, 45 40 Q 50 15, 55 40 Q 60 75, 85 80"
        fill="none"
        stroke={`url(#${gradStroke})`}
        strokeWidth="5"
        strokeLinecap="round"
        opacity="0.9"
      />

      {/* Qi ripple lines */}
      <path
        d="M40 62 Q 50 55, 60 62"
        stroke={accent}
        strokeWidth="1.8"
        fill="none"
        strokeLinecap="round"
        opacity="0.6"
      />
      <path
        d="M44 70 Q 50 66, 56 70"
        stroke={accent}
        strokeWidth="1.2"
        fill="none"
        strokeLinecap="round"
        opacity="0.4"
      />

      {/* Top inspiration dot */}
      <circle cx="50" cy="18" r="2" fill={color} opacity="0.8">
        {animated && (
          <animate
            attributeName="opacity"
            values="0.35;1;0.35"
            dur="3s"
            repeatCount="indefinite"
          />
        )}
      </circle>
    </svg>
  );
}
