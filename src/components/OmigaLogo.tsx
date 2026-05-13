/**
 * OmigaLogo - theme-adaptive omega hub mark.
 *
 * The mark combines an omega-shaped workbench outline with a compact
 * orchestration graph: one kernel connected to three agent nodes.
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
  const node = isDark ? theme.palette.info.light : theme.palette.info.main;
  const kernel = isDark ? theme.palette.warning.light : theme.palette.warning.main;

  const plateColor = isDark ? "rgba(255,255,255,0.07)" : "rgba(8,18,31,0.05)";
  const plateStroke = isDark ? "rgba(255,255,255,0.14)" : "rgba(8,18,31,0.08)";
  const washOpacity = isDark ? 0.28 : 0.24;

  const gradBg = `omiga-grad-bg-${uid}`;
  const gradOmega = `omiga-grad-omega-${uid}`;
  const gradLink = `omiga-grad-link-${uid}`;

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
        <radialGradient id={gradBg} cx="50%" cy="45%" r="54%">
          <stop offset="0%" stopColor={color} stopOpacity={washOpacity} />
          <stop offset="100%" stopColor={color} stopOpacity="0" />
        </radialGradient>

        <linearGradient id={gradOmega} x1="18%" y1="20%" x2="86%" y2="82%">
          <stop offset="0%" stopColor={color} />
          <stop offset="56%" stopColor={accent} />
          <stop offset="100%" stopColor={node} />
        </linearGradient>

        <linearGradient id={gradLink} x1="28%" y1="30%" x2="72%" y2="70%">
          <stop offset="0%" stopColor={node} />
          <stop offset="100%" stopColor={kernel} />
        </linearGradient>
      </defs>

      <circle cx="50" cy="50" r="45" fill={plateColor} stroke={plateStroke} strokeWidth="1" />
      <circle cx="50" cy="50" r="45" fill={`url(#${gradBg})`} />

      <path
        d="M24 72H37V63C27 57 22 48 22 38C22 23 34 14 50 14C66 14 78 23 78 38C78 48 73 57 63 63V72H76"
        fill="none"
        stroke={`url(#${gradOmega})`}
        strokeWidth="7.5"
        strokeLinecap="round"
        strokeLinejoin="round"
        opacity="0.92"
      />

      <path
        d="M50 44L37 35M50 44L63 35M50 44V59"
        stroke={`url(#${gradLink})`}
        strokeWidth="2.5"
        fill="none"
        strokeLinecap="round"
        strokeLinejoin="round"
        opacity={isDark ? 0.78 : 0.7}
      />

      <circle cx="37" cy="35" r="4.2" fill={node} />
      <circle cx="63" cy="35" r="4.2" fill={accent} />
      <circle cx="50" cy="59" r="4.2" fill={color} />

      <rect
        x="45.5"
        y="39.5"
        width="9"
        height="9"
        rx="2.3"
        fill={kernel}
        transform="rotate(45 50 44)"
        opacity={isDark ? 0.96 : 0.94}
      >
        {animated && (
          <animate
            attributeName="opacity"
            values={isDark ? "0.65;1;0.65" : "0.72;0.96;0.72"}
            dur="2.8s"
            repeatCount="indefinite"
          />
        )}
      </rect>
    </svg>
  );
}
