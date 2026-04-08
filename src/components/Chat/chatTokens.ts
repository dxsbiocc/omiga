import type { Theme } from "@mui/material/styles";
import { alpha, darken, lighten } from "@mui/material/styles";

/** Default font stack — unchanged across themes */
const CHAT_FONT =
  '"Inter", system-ui, -apple-system, BlinkMacSystemFont, sans-serif';

export type ChatTokenSet = {
  font: string;
  /** User bubble gradient — translucent tinted primary → secondary */
  userGrad: string;
  /** Text on user bubble (chosen for contrast on semi-transparent fill) */
  userBubbleText: string;
  /** Hairline edge so translucent bubble stays readable on any thread bg */
  userBubbleBorder: string;
  agentBubbleBg: string;
  agentBubbleBorder: string;
  agentAvatarBg: string;
  accent: string;
  /** Tool row / fold chrome icons — slightly deeper than `accent` for legibility */
  toolIcon: string;
  /** Outer surface of each tool call card — neutral, lighter than `codeBg` */
  toolCallCardBg: string;
  textPrimary: string;
  textMuted: string;
  textLabel: string;
  labelMuted: string;
  codeBg: string;
  outputBg: string;
  doneGreen: string;
};

/**
 * Chat chrome tokens derived from the active MUI theme (light/dark + accent preset).
 * All interactive and semantic colors track `theme.palette`.
 */
export function getChatTokens(theme: Theme): ChatTokenSet {
  const p = theme.palette;
  const isDark = p.mode === "dark";
  const divider =
    typeof p.divider === "string"
      ? p.divider
      : isDark
        ? "rgba(148, 163, 184, 0.12)"
        : "rgba(15, 23, 42, 0.08)";

  // Lighter stops + higher transparency so the thread background shows through slightly
  const userGrad = isDark
    ? `linear-gradient(315deg, ${alpha(lighten(p.primary.main, 0.2), 0.38)} 0%, ${alpha(lighten(p.secondary.main, 0.16), 0.34)} 100%)`
    : `linear-gradient(315deg, ${alpha(lighten(p.primary.main, 0.08), 0.32)} 0%, ${alpha(lighten(p.secondary.main, 0.06), 0.28)} 100%)`;

  const userBubbleText = isDark
    ? alpha(p.common.white, 0.94)
    : alpha(p.text.primary, 0.92);

  const userBubbleBorder = isDark
    ? alpha(p.common.white, 0.12)
    : alpha(p.primary.main, 0.22);

  const codeBg = isDark
    ? alpha(p.common.white, 0.06)
    : alpha(p.primary.main, 0.07);

  const outputBg = isDark
    ? alpha(p.common.white, 0.09)
    : alpha(p.secondary.main, 0.1);

  const toolIcon = darken(
    p.primary.main,
    isDark ? 0.05 : 0.18,
  );

  const toolCallCardBg = isDark
    ? alpha(p.common.white, 0.055)
    : alpha(p.common.black, 0.022);

  return {
    font: CHAT_FONT,
    userGrad,
    userBubbleText,
    userBubbleBorder,
    agentBubbleBg: p.background.paper,
    agentBubbleBorder: divider,
    agentAvatarBg: alpha(p.primary.main, isDark ? 0.22 : 0.12),
    accent: p.primary.main,
    toolIcon,
    toolCallCardBg,
    textPrimary: p.text.primary,
    textMuted: p.text.secondary,
    textLabel: p.text.secondary,
    labelMuted: p.text.disabled,
    codeBg,
    outputBg,
    doneGreen: p.success.main,
  };
}
