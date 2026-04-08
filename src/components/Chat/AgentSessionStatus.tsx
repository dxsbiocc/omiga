import { Box, LinearProgress, Stack, Typography, useTheme } from "@mui/material";
import { alpha } from "@mui/material/styles";
import type { Theme } from "@mui/material/styles";
import type { SvgIconComponent } from "@mui/icons-material";
import {
  AutoAwesome,
  Autorenew,
  Assignment,
  Build,
  CheckCircleOutline,
  Construction,
  Description,
  EditNote,
  Extension,
  FolderOpen,
  Psychology,
  Search as SearchIcon,
  Terminal,
  TravelExplore,
} from "@mui/icons-material";
import type { ExecutionStep } from "../../state/activityStore";
import {
  getExecutionSurfaceView,
  type ExecutionSurfaceContext,
  type ExecutionSurfaceKind,
} from "../../utils/executionSurfaceLabel";

export interface AgentSessionStatusProps {
  executionSteps: ExecutionStep[];
  isConnecting: boolean;
  isStreaming: boolean;
  waitingFirstChunk: boolean;
  /** Raw tool name from stream when step row not yet committed */
  toolHintFallback: string | null;
}

function pickToolIcon(toolName: string | null | undefined): SvgIconComponent {
  const n = (toolName ?? "").toLowerCase();
  if (!n) return Construction;
  if (n.includes("web_search")) return SearchIcon;
  if (n.includes("web_fetch") || (n.includes("fetch") && n.includes("web")))
    return TravelExplore;
  if (n.includes("bash") || n.includes("shell")) return Terminal;
  if (n.includes("grep")) return Construction;
  if (n.includes("glob")) return FolderOpen;
  if (n.includes("file_read") || n === "read_file" || n.includes("read_file"))
    return Description;
  if (n.includes("file_write") || n.includes("write")) return EditNote;
  if (n.includes("file_edit") || n.includes("edit")) return EditNote;
  if (n.includes("todo_write") || n.includes("todowrite")) return Assignment;
  if (n.includes("task")) return Assignment;
  if (n.includes("mcp__")) return Extension;
  if (n.includes("notebook")) return EditNote;
  return Build;
}

function pickIconAndAccent(
  kind: ExecutionSurfaceKind,
  toolName: string | null,
  theme: Theme,
): { Icon: SvgIconComponent; accent: string } {
  switch (kind) {
    case "idle":
    case "finished":
      return {
        Icon: CheckCircleOutline,
        accent: theme.palette.primary.main,
      };
    case "waiting":
      return { Icon: Autorenew, accent: theme.palette.primary.main };
    case "thinking":
      return { Icon: Psychology, accent: theme.palette.secondary.main };
    case "generating":
      return { Icon: AutoAwesome, accent: theme.palette.primary.main };
    case "tool": {
      const Icon = pickToolIcon(toolName);
      return { Icon, accent: theme.palette.warning.main };
    }
    default:
      return { Icon: Construction, accent: theme.palette.warning.main };
  }
}

/**
 * Chat header status: label + icon follow execution steps (wait → think → output → tool name → …).
 */
export function AgentSessionStatus({
  executionSteps,
  isConnecting,
  isStreaming,
  waitingFirstChunk,
  toolHintFallback,
}: AgentSessionStatusProps) {
  const theme = useTheme();
  const ctx: ExecutionSurfaceContext = {
    isConnecting,
    isStreaming,
    waitingFirstChunk,
    toolHintFallback,
  };
  const { label: primary, kind, toolName } = getExecutionSurfaceView(
    executionSteps,
    ctx,
  );
  const { Icon, accent } = pickIconAndAccent(kind, toolName, theme);
  const busy = kind !== "idle" && kind !== "finished";

  return (
    <Box
      role="status"
      aria-label={primary}
      title={primary}
      sx={{
        position: "relative",
        minWidth: 0,
        maxWidth: { xs: "min(100%, 320px)", sm: 340 },
        borderRadius: 2.5,
        overflow: "hidden",
        border: `1px solid ${alpha(accent, busy ? 0.35 : 0.22)}`,
        background: alpha(theme.palette.background.paper, 0.55),
        backdropFilter: "blur(14px) saturate(160%)",
        WebkitBackdropFilter: "blur(14px) saturate(160%)",
        boxShadow: `0 1px 0 ${alpha(theme.palette.common.black, 0.04)}, 0 8px 24px ${alpha(theme.palette.common.black, 0.06)}`,
        transition: "border-color 0.22s ease, box-shadow 0.22s ease",
        "@media (prefers-reduced-motion: reduce)": {
          transition: "none",
        },
      }}
    >
      <Stack
        direction="row"
        alignItems="center"
        spacing={0.85}
        sx={{
          px: 1.25,
          py: 0.65,
          minHeight: 34,
        }}
      >
        <Box
          sx={{
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            width: 28,
            height: 28,
            borderRadius: 1.5,
            flexShrink: 0,
            bgcolor: alpha(accent, 0.12),
            color: accent,
            ...(busy &&
              kind !== "waiting" && {
                "@keyframes statusIconPulse": {
                  "0%, 100%": { opacity: 1, transform: "scale(1)" },
                  "50%": { opacity: 0.82, transform: "scale(0.96)" },
                },
                animation: "statusIconPulse 2.2s ease-in-out infinite",
                "@media (prefers-reduced-motion: reduce)": {
                  animation: "none",
                },
              }),
          }}
        >
          <Icon
            sx={{
              fontSize: 17,
              ...(kind === "waiting" && {
                "@keyframes statusIconSpin": {
                  from: { transform: "rotate(0deg)" },
                  to: { transform: "rotate(360deg)" },
                },
                animation: "statusIconSpin 1.05s linear infinite",
                "@media (prefers-reduced-motion: reduce)": {
                  animation: "none",
                },
              }),
            }}
          />
        </Box>
        <Box minWidth={0} sx={{ flex: 1 }}>
          <Typography
            variant="caption"
            fontWeight={700}
            letterSpacing={0.02}
            noWrap
            sx={{
              color: "text.primary",
              fontSize: "0.72rem",
              lineHeight: 1.25,
              fontFamily:
                kind === "tool"
                  ? "ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace"
                  : undefined,
            }}
          >
            {primary}
          </Typography>
        </Box>
        <Box
          aria-hidden
          sx={{
            width: 8,
            height: 8,
            borderRadius: "50%",
            flexShrink: 0,
            bgcolor: busy ? accent : alpha(accent, 0.45),
            boxShadow: busy
              ? `0 0 0 3px ${alpha(accent, 0.2)}`
              : `0 0 0 1px ${alpha(theme.palette.background.paper, 1)}`,
            ...(busy && {
              "@keyframes statusDot": {
                "0%": {
                  transform: "scale(1)",
                  boxShadow: `0 0 0 0 ${alpha(accent, 0.35)}`,
                },
                "60%": {
                  transform: "scale(1.05)",
                  boxShadow: `0 0 0 6px ${alpha(accent, 0)}`,
                },
                "100%": {
                  transform: "scale(1)",
                  boxShadow: `0 0 0 0 ${alpha(accent, 0)}`,
                },
              },
              animation: "statusDot 1.8s ease-out infinite",
              "@media (prefers-reduced-motion: reduce)": {
                animation: "none",
              },
            }),
          }}
        />
      </Stack>
      {busy && (
        <LinearProgress
          variant="indeterminate"
          sx={{
            height: 2,
            borderRadius: 0,
            bgcolor: alpha(accent, 0.08),
            "& .MuiLinearProgress-bar": {
              borderRadius: 0,
              background: `linear-gradient(90deg, ${alpha(accent, 0.15)}, ${alpha(accent, 0.85)}, ${alpha(accent, 0.25)})`,
            },
            "@media (prefers-reduced-motion: reduce)": {
              "& .MuiLinearProgress-bar": {
                animationDuration: "3.5s",
              },
            },
          }}
        />
      )}
    </Box>
  );
}
