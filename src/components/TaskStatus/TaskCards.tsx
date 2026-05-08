import { Box, Typography, Chip, Stack } from "@mui/material";
import { alpha } from "@mui/material/styles";
import {
  RadioButtonUnchecked,
  CheckCircle,
  ErrorOutline,
  Timer,
  Circle,
} from "@mui/icons-material";
import type { PlanTodoItem } from "./PlanTodoList";
import { AgentInfoChip } from "./AgentInfoChip";

export interface RuntimeAgentTaskItem {
  id: string;
  agentType: string;
  description: string;
  status: "Pending" | "Running" | "Completed" | "Failed" | "Cancelled";
  stageLabel?: string;
  supervisorLabel?: string;
}

/** 运行中任务卡片 - 高亮显示、带动画 */
interface RunningTaskCardProps {
  item: PlanTodoItem;
  onClick?: () => void;
}

export function RunningTaskCard({ item, onClick }: RunningTaskCardProps) {
  return (
    <Box
      onClick={onClick}
      sx={{
        p: 1.25,
        borderRadius: 1.5,
        bgcolor: alpha("#6366f1", 0.08),
        border: `1px solid ${alpha("#6366f1", 0.25)}`,
        position: "relative",
        overflow: "hidden",
        cursor: onClick ? "pointer" : "default",
        transition: "transform 0.18s ease, box-shadow 0.18s ease, border-color 0.18s ease",
        "&:hover": onClick
          ? {
              transform: "translateY(-1px)",
              boxShadow: `0 10px 24px ${alpha("#6366f1", 0.12)}`,
              borderColor: alpha("#6366f1", 0.34),
            }
          : undefined,
        "&::before": {
          content: '""',
          position: "absolute",
          left: 0,
          top: 0,
          bottom: 0,
          width: 3,
          bgcolor: "#6366f1",
        },
      }}
    >
      {/* 顶部：状态标识 */}
      <Stack
        direction="row"
        alignItems="center"
        justifyContent="space-between"
        sx={{ mb: 0.75 }}
      >
        <Chip
          size="small"
          icon={
            <Box
              component="span"
              sx={{
                width: 6,
                height: 6,
                borderRadius: "50%",
                bgcolor: "#6366f1",
                animation: "pulse 1.5s ease-in-out infinite",
                "@keyframes pulse": {
                  "0%, 100%": { opacity: 1, transform: "scale(1)" },
                  "50%": { opacity: 0.5, transform: "scale(1.1)" },
                },
              }}
            />
          }
          label="进行中"
          sx={{
            height: 20,
            fontSize: 10,
            fontWeight: 600,
            bgcolor: alpha("#6366f1", 0.15),
            color: "#6366f1",
            border: `1px solid ${alpha("#6366f1", 0.3)}`,
            "& .MuiChip-icon": {
              ml: 0.5,
            },
          }}
        />
        <Timer sx={{ fontSize: 14, color: alpha("#6366f1", 0.6) }} />
      </Stack>

      {/* 任务内容 */}
      <Typography
        variant="body2"
        sx={{
          fontSize: 13,
          fontWeight: 600,
          color: "text.primary",
          lineHeight: 1.4,
          pl: 0.5,
        }}
      >
        {item.name}
      </Typography>
    </Box>
  );
}

interface RunningAgentCardProps {
  item: RuntimeAgentTaskItem;
  onClick?: () => void;
}

export function RunningAgentCard({ item, onClick }: RunningAgentCardProps) {
  const isPending = item.status === "Pending";
  const accent = isPending ? "#f59e0b" : "#2563eb";
  return (
    <Box
      onClick={onClick}
      sx={{
        p: 1.25,
        borderRadius: 1.75,
        bgcolor: alpha(accent, 0.08),
        border: `1px solid ${alpha(accent, 0.24)}`,
        position: "relative",
        overflow: "hidden",
        cursor: onClick ? "pointer" : "default",
        transition: "transform 0.18s ease, box-shadow 0.18s ease, border-color 0.18s ease",
        "&::before": {
          content: '""',
          position: "absolute",
          left: 0,
          top: 0,
          bottom: 0,
          width: 3,
          bgcolor: accent,
        },
        "&:hover": onClick
          ? {
              transform: "translateY(-1px)",
              boxShadow: `0 10px 24px ${alpha(accent, 0.12)}`,
              borderColor: alpha(accent, 0.34),
            }
          : undefined,
      }}
    >
      <Stack direction="row" alignItems="center" justifyContent="space-between" spacing={1} sx={{ mb: 0.9 }}>
        <Stack direction="row" alignItems="center" spacing={0.75}>
          <Chip
            size="small"
            icon={
              <Circle
                sx={{
                  fontSize: 8,
                  color: accent,
                  ...(item.status === "Running"
                    ? {
                        animation: "runtimeAgentPulse 1.45s ease-in-out infinite",
                        "@keyframes runtimeAgentPulse": {
                          "0%, 100%": { opacity: 1, transform: "scale(1)" },
                          "50%": { opacity: 0.5, transform: "scale(1.1)" },
                        },
                      }
                    : {}),
                }}
              />
            }
            label={item.status === "Pending" ? "等待调度" : "运行中"}
            sx={{
              height: 20,
              fontSize: 10,
              fontWeight: 700,
              bgcolor: alpha(accent, 0.14),
              color: accent,
              border: `1px solid ${alpha(accent, 0.25)}`,
              "& .MuiChip-icon": {
                ml: 0.75,
              },
            }}
          />
          <AgentInfoChip
            agentType={item.agentType}
            status={item.status}
            description={item.description}
            stageLabel={item.stageLabel}
            supervisorLabel={item.supervisorLabel}
            maxChars={16}
          />
        </Stack>
        <Timer sx={{ fontSize: 15, color: alpha(accent, 0.7) }} />
      </Stack>

      <Typography
        variant="body2"
        sx={{
          fontSize: 12.5,
          fontWeight: 600,
          color: "text.primary",
          lineHeight: 1.45,
          pl: 0.4,
        }}
      >
        {item.description}
      </Typography>
      {(item.stageLabel || item.supervisorLabel) && (
        <Typography
          variant="caption"
          sx={{
            display: "block",
            mt: 0.55,
            pl: 0.4,
            fontSize: 10,
            color: "text.secondary",
          }}
        >
          {[item.stageLabel, item.supervisorLabel ? `上级 ${item.supervisorLabel}` : null]
            .filter(Boolean)
            .join(" · ")}
        </Typography>
      )}
    </Box>
  );
}

/** 待办任务卡片 - 简洁、可点击 */
interface PendingTaskCardProps {
  item: PlanTodoItem;
  onClick?: () => void;
}

export function PendingTaskCard({ item, onClick }: PendingTaskCardProps) {
  return (
    <Box
      onClick={onClick}
      sx={{
        p: 1,
        borderRadius: 1,
        bgcolor: "background.paper",
        border: `1px solid ${alpha("#000", 0.08)}`,
        display: "flex",
        alignItems: "center",
        gap: 1,
        cursor: onClick ? "pointer" : "default",
        transition: "all 0.2s ease",
        "&:hover": onClick
          ? {
              bgcolor: alpha("#6366f1", 0.02),
              borderColor: alpha("#6366f1", 0.2),
            }
          : {},
      }}
    >
      <RadioButtonUnchecked
        sx={{
          fontSize: 18,
          color: alpha("#000", 0.15),
          flexShrink: 0,
        }}
      />
      <Typography
        variant="body2"
        sx={{
          fontSize: 12,
          color: "text.secondary",
          lineHeight: 1.4,
          flex: 1,
        }}
      >
        {item.name}
      </Typography>
    </Box>
  );
}

/** 已完成任务卡片 - 简洁、带删除线 */
interface CompletedTaskCardProps {
  item: PlanTodoItem;
  onClick?: () => void;
}

export function CompletedTaskCard({ item, onClick }: CompletedTaskCardProps) {
  return (
    <Box
      onClick={onClick}
      sx={{
        p: 1,
        borderRadius: 1,
        bgcolor: alpha("#22c55e", 0.04),
        border: `1px solid ${alpha("#22c55e", 0.15)}`,
        display: "flex",
        alignItems: "center",
        gap: 1,
        cursor: onClick ? "pointer" : "default",
        transition: "background-color 0.18s ease, border-color 0.18s ease",
        "&:hover": onClick
          ? {
              bgcolor: alpha("#22c55e", 0.07),
              borderColor: alpha("#22c55e", 0.24),
            }
          : undefined,
      }}
    >
      <CheckCircle
        sx={{
          fontSize: 18,
          color: "#22c55e",
          flexShrink: 0,
        }}
      />
      <Typography
        variant="body2"
        sx={{
          fontSize: 12,
          color: "text.secondary",
          lineHeight: 1.4,
          flex: 1,
          textDecoration: "line-through",
        }}
      >
        {item.name}
      </Typography>
    </Box>
  );
}

/** 错误任务卡片 */
interface ErrorTaskCardProps {
  item: PlanTodoItem;
  onClick?: () => void;
}

export function ErrorTaskCard({ item, onClick }: ErrorTaskCardProps) {
  return (
    <Box
      onClick={onClick}
      sx={{
        p: 1.25,
        borderRadius: 1,
        bgcolor: alpha("#ef4444", 0.04),
        border: `1px solid ${alpha("#ef4444", 0.2)}`,
        display: "flex",
        alignItems: "flex-start",
        gap: 1,
        cursor: onClick ? "pointer" : "default",
        transition: "background-color 0.18s ease, border-color 0.18s ease",
        "&:hover": onClick
          ? {
              bgcolor: alpha("#ef4444", 0.07),
              borderColor: alpha("#ef4444", 0.28),
            }
          : undefined,
      }}
    >
      <ErrorOutline
        sx={{
          fontSize: 18,
          color: "error.main",
          flexShrink: 0,
          mt: 0.1,
        }}
      />
      <Box sx={{ flex: 1, minWidth: 0 }}>
        <Typography
          variant="body2"
          sx={{
            fontSize: 12,
            color: "error.main",
            lineHeight: 1.4,
            fontWeight: 500,
          }}
        >
          {item.name}
        </Typography>
        <Typography
          variant="caption"
          sx={{
            fontSize: 10,
            color: "error.light",
            mt: 0.25,
            display: "block",
          }}
        >
          执行出错
        </Typography>
      </Box>
    </Box>
  );
}

/** 通用任务卡片 - 根据状态自动渲染 */
interface TaskCardProps {
  item: PlanTodoItem;
  onClick?: () => void;
}

export function TaskCard({ item, onClick }: TaskCardProps) {
  switch (item.status) {
    case "running":
      return <RunningTaskCard item={item} onClick={onClick} />;
    case "completed":
      return <CompletedTaskCard item={item} onClick={onClick} />;
    case "error":
      return <ErrorTaskCard item={item} onClick={onClick} />;
    case "pending":
    default:
      return <PendingTaskCard item={item} onClick={onClick} />;
  }
}
