import { Box, Chip, Stack, Tooltip, Typography } from "@mui/material";
import type { ChipProps } from "@mui/material";
import type { Theme } from "@mui/material/styles";
import { alpha } from "@mui/material/styles";
import {
  Bolt,
  Memory,
  SmartToy,
  Source,
  Visibility,
} from "@mui/icons-material";
import { compactLabel, isLabelCompacted } from "../../utils/compactLabel";
import {
  getFallbackAgentRoleEntry,
  useAgentRoleCatalog,
} from "../../hooks/useAgentRoleCatalog";

type AgentRuntimeStatus =
  | "Pending"
  | "Running"
  | "Completed"
  | "Failed"
  | "Cancelled"
  | "pending"
  | "running"
  | "completed"
  | "failed"
  | "cancelled";

interface AgentInfoChipProps {
  agentType: string;
  status?: AgentRuntimeStatus;
  description?: string;
  stageLabel?: string;
  supervisorLabel?: string;
  size?: ChipProps["size"];
  variant?: ChipProps["variant"];
  maxChars?: number;
  sx?: ChipProps["sx"];
}

function tierColor(theme: Theme, tier: string): string {
  const normalized = tier.toLowerCase();
  if (normalized.includes("frontier")) return theme.palette.secondary.main;
  if (normalized.includes("spark")) return theme.palette.warning.main;
  return theme.palette.info.main;
}

function statusLabel(status?: AgentRuntimeStatus): string | null {
  if (!status) return null;
  switch (status.toLowerCase()) {
    case "running":
      return "运行中";
    case "pending":
      return "等待中";
    case "completed":
      return "已完成";
    case "failed":
      return "失败";
    case "cancelled":
      return "已取消";
    default:
      return status;
  }
}

function statusColor(theme: Theme, status?: AgentRuntimeStatus): string {
  if (!status) return theme.palette.primary.main;
  switch (status.toLowerCase()) {
    case "running":
      return theme.palette.primary.main;
    case "pending":
      return theme.palette.warning.main;
    case "completed":
      return theme.palette.success.main;
    case "failed":
      return theme.palette.error.main;
    case "cancelled":
      return theme.palette.text.disabled;
    default:
      return theme.palette.primary.main;
  }
}

export function AgentInfoChip({
  agentType,
  status,
  description,
  stageLabel,
  supervisorLabel,
  size = "small",
  variant = "filled",
  maxChars = 14,
  sx,
}: AgentInfoChipProps) {
  const catalog = useAgentRoleCatalog();
  const meta = catalog[agentType] ?? getFallbackAgentRoleEntry(agentType);
  const fullLabel = meta.displayName;
  const shortLabel = compactLabel(fullLabel, maxChars);
  const compacted = isLabelCompacted(fullLabel, shortLabel);

  return (
    <Tooltip
      arrow
      placement="top-start"
      enterDelay={120}
      title={
        <Box sx={{ minWidth: 240, maxWidth: 320, p: 0.25 }}>
          <Stack spacing={1}>
            <Stack direction="row" alignItems="center" spacing={0.9}>
              <Box
                sx={{
                  width: 32,
                  height: 32,
                  borderRadius: 2,
                  display: "flex",
                  alignItems: "center",
                  justifyContent: "center",
                  bgcolor: (theme) => alpha(statusColor(theme, status), 0.12),
                  color: (theme) => statusColor(theme, status),
                  flexShrink: 0,
                }}
              >
                <SmartToy sx={{ fontSize: 18 }} />
              </Box>
              <Box sx={{ minWidth: 0, flex: 1 }}>
                <Typography sx={{ fontSize: 13, fontWeight: 700, lineHeight: 1.2 }}>
                  {meta.displayName}
                </Typography>
                <Typography sx={{ fontSize: 11, color: "rgba(255,255,255,0.72)" }}>
                  {agentType}
                </Typography>
              </Box>
            </Stack>

            <Stack direction="row" spacing={0.5} flexWrap="wrap" useFlexGap>
              {statusLabel(status) && (
                <Chip
                  size="small"
                  label={statusLabel(status)}
                  sx={{
                    height: 18,
                    fontSize: 10,
                    bgcolor: (theme) => alpha(statusColor(theme, status), 0.16),
                    color: (theme) => statusColor(theme, status),
                    fontWeight: 700,
                  }}
                />
              )}
              <Chip
                size="small"
                icon={<Bolt sx={{ fontSize: 11 }} />}
                label={meta.modelTier}
                sx={{
                  height: 18,
                  fontSize: 10,
                  bgcolor: (theme) => alpha(tierColor(theme, meta.modelTier), 0.16),
                  color: (theme) => tierColor(theme, meta.modelTier),
                  "& .MuiChip-icon": { color: "inherit" },
                }}
              />
              <Chip
                size="small"
                icon={<Source sx={{ fontSize: 11 }} />}
                label={meta.source}
                sx={{
                  height: 18,
                  fontSize: 10,
                  bgcolor: "rgba(255,255,255,0.08)",
                  color: "rgba(255,255,255,0.82)",
                  "& .MuiChip-icon": { color: "inherit" },
                }}
              />
              <Chip
                size="small"
                icon={<Visibility sx={{ fontSize: 11 }} />}
                label={meta.background ? "后台" : "前台"}
                sx={{
                  height: 18,
                  fontSize: 10,
                  bgcolor: "rgba(255,255,255,0.08)",
                  color: "rgba(255,255,255,0.82)",
                  "& .MuiChip-icon": { color: "inherit" },
                }}
              />
            </Stack>

            {description ? (
              <Box
                sx={{
                  p: 1,
                  borderRadius: 1.5,
                  bgcolor: "rgba(255,255,255,0.06)",
                  border: "1px solid rgba(255,255,255,0.08)",
                }}
              >
                <Typography sx={{ fontSize: 10, fontWeight: 700, color: "rgba(255,255,255,0.74)", mb: 0.35 }}>
                  当前任务
                </Typography>
                <Typography sx={{ fontSize: 11.5, lineHeight: 1.5 }}>
                  {description}
                </Typography>
              </Box>
            ) : null}

            <Box
              sx={{
                p: 1,
                borderRadius: 1.5,
                bgcolor: "rgba(255,255,255,0.05)",
                border: "1px solid rgba(255,255,255,0.08)",
              }}
            >
              <Typography sx={{ fontSize: 10, fontWeight: 700, color: "rgba(255,255,255,0.74)", mb: 0.35 }}>
                适用场景
              </Typography>
              <Typography sx={{ fontSize: 11.5, lineHeight: 1.5 }}>
                {meta.whenToUse}
              </Typography>
            </Box>

            {(stageLabel || supervisorLabel || meta.explicitModel) && (
              <Stack spacing={0.45}>
                {stageLabel ? (
                  <Typography sx={{ fontSize: 10.5, color: "rgba(255,255,255,0.72)" }}>
                    阶段: {stageLabel}
                  </Typography>
                ) : null}
                {supervisorLabel ? (
                  <Typography sx={{ fontSize: 10.5, color: "rgba(255,255,255,0.72)" }}>
                    上级: {supervisorLabel}
                  </Typography>
                ) : null}
                {meta.explicitModel ? (
                  <Stack direction="row" alignItems="center" spacing={0.5}>
                    <Memory sx={{ fontSize: 12, color: "rgba(255,255,255,0.62)" }} />
                    <Typography sx={{ fontSize: 10.5, color: "rgba(255,255,255,0.72)" }}>
                      显式模型: {meta.explicitModel}
                    </Typography>
                  </Stack>
                ) : null}
              </Stack>
            )}
          </Stack>
        </Box>
      }
      slotProps={{
        tooltip: {
          sx: {
            bgcolor: "rgba(20, 24, 37, 0.96)",
            color: "#fff",
            boxShadow: "0 18px 40px rgba(7, 12, 20, 0.32)",
            border: "1px solid rgba(255,255,255,0.08)",
            borderRadius: 2.5,
            px: 1.25,
            py: 1,
            maxWidth: 360,
            backdropFilter: "blur(16px)",
          },
        },
        arrow: {
          sx: {
            color: "rgba(20, 24, 37, 0.96)",
          },
        },
      }}
    >
      <Box sx={{ display: "inline-flex" }}>
        <Chip
          size={size}
          variant={variant}
          icon={<SmartToy sx={{ fontSize: size === "small" ? 12 : 14 }} />}
          label={shortLabel}
          sx={{
            height: size === "small" ? 20 : 24,
            fontSize: size === "small" ? 10 : 11,
            fontWeight: 600,
            bgcolor: (theme) => alpha(statusColor(theme, status), variant === "filled" ? 0.12 : 0.06),
            color: (theme) => statusColor(theme, status),
            borderColor: (theme) => alpha(statusColor(theme, status), 0.28),
            "& .MuiChip-icon": {
              color: "inherit",
            },
            ...sx,
          }}
        />
        {compacted ? (
          <Box component="span" sx={{ display: "none" }}>
            {fullLabel}
          </Box>
        ) : null}
      </Box>
    </Tooltip>
  );
}
