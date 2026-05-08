import { useMemo, useState } from "react";
import {
  Box,
  Typography,
  Chip,
  Collapse,
  IconButton,
  Tooltip,
  Stack,
  useTheme,
} from "@mui/material";
import { alpha } from "@mui/material/styles";
import {
  ExpandMore,
  SmartToy,
  Route,
  RadioButtonUnchecked,
} from "@mui/icons-material";
import {
  aggregateReviewerVerdicts,
  overallReviewerHeadline,
  type BackgroundAgentTaskRow,
} from "../../utils/reviewerVerdict";
import { ReviewerVerdictList } from "../ReviewerVerdictList";
import { normalizeAgentDisplayName } from "../../state/agentStore";
import { compactLabel, isLabelCompacted } from "../../utils/compactLabel";
import {
  buildSchedulerPlanHierarchy,
  schedulerStageLabel,
} from "../../utils/schedulerPlanHierarchy";
import { AgentInfoChip } from "./AgentInfoChip";

interface SchedulerPlan {
  planId: string;
  entryAgentType?: string;
  executionSupervisorAgentType?: string;
  subtasks: Array<{
    id: string;
    description: string;
    agentType: string;
    supervisorAgentType?: string;
    stage?: string;
    dependencies: string[];
    critical: boolean;
    estimatedSecs: number;
  }>;
  selectedAgents: string[];
  estimatedDurationSecs: number;
  reviewerAgents?: string[];
}

interface SchedulerPlanPanelProps {
  plan: SchedulerPlan;
  taskRows?: BackgroundAgentTaskRow[];
  onOpenReviewerTranscript?: (taskId: string, label?: string) => void;
}

/** 调度计划面板 - 显示多 Agent 编排的执行计划 */
export function SchedulerPlanPanel({
  plan,
  taskRows = [],
  onOpenReviewerTranscript,
}: SchedulerPlanPanelProps) {
  const [expanded, setExpanded] = useState(true);
  const theme = useTheme();
  const reviewerVerdicts = useMemo(
    () => aggregateReviewerVerdicts(taskRows),
    [taskRows],
  );
  const reviewerHeadline = useMemo(
    () => overallReviewerHeadline(reviewerVerdicts),
    [reviewerVerdicts],
  );
  const runtimeRowsByTaskId = useMemo(
    () =>
      Object.fromEntries(taskRows.map((row) => [row.task_id, row])),
    [taskRows],
  );

  // 获取并行执行组
  const getParallelGroups = () => {
    const groups: string[][] = [];
    const completed = new Set<string>();
    const remaining = plan.subtasks.map((t) => t.id);

    while (remaining.length > 0) {
      const currentGroup: string[] = [];
      const stillRemaining: string[] = [];

      for (const taskId of remaining) {
        const task = plan.subtasks.find((t) => t.id === taskId);
        if (task) {
          const depsSatisfied = task.dependencies.every((dep) =>
            completed.has(dep),
          );
          if (depsSatisfied) {
            currentGroup.push(taskId);
          } else {
            stillRemaining.push(taskId);
          }
        }
      }

      if (currentGroup.length === 0 && stillRemaining.length > 0) {
        currentGroup.push(stillRemaining.shift()!);
      }

      currentGroup.forEach((id) => completed.add(id));
      groups.push(currentGroup);
      remaining.length = 0;
      remaining.push(...stillRemaining);
    }

    return groups;
  };

  const groups = getParallelGroups();
  const hierarchy = buildSchedulerPlanHierarchy(plan);
  const compactAgentChip = (agent: string, maxChars = 14) => {
    const full = normalizeAgentDisplayName(agent);
    const short = compactLabel(full, maxChars);
    return { full, short, compacted: isLabelCompacted(full, short) };
  };

  // Agent 颜色映射
  const getAgentColor = (agentType: string) => {
    const colors: Record<string, string> = {
      Explore: theme.palette.info.main,
      Plan: theme.palette.warning.main,
      verification: theme.palette.success.main,
      "general-purpose": theme.palette.primary.main,
    };
    return colors[agentType] || theme.palette.grey[500];
  };

  return (
    <Box>
      {/* 头部信息 */}
      <Box
        sx={{
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
          mb: 1.5,
        }}
      >
        <Stack direction="row" alignItems="center" spacing={1}>
          <Route sx={{ fontSize: 16, color: "primary.main" }} />
          <Typography variant="body2" fontWeight={600} sx={{ fontSize: 12 }}>
            多 Agent 编排
          </Typography>
          <Chip
            size="small"
            label={`${plan.subtasks.length} 个子任务`}
            sx={{
              height: 18,
              fontSize: 10,
              bgcolor: alpha(theme.palette.primary.main, 0.1),
              color: "primary.main",
            }}
          />
        </Stack>
        <Tooltip title={expanded ? "收起" : "展开"}>
          <IconButton
            size="small"
            onClick={() => setExpanded(!expanded)}
            sx={{
              transform: expanded ? "rotate(180deg)" : "rotate(0deg)",
              transition: "transform 0.2s ease, color 150ms ease",
              p: 0.5,
              color: "text.secondary",
              opacity: 0.65,
              "&:hover": {
                color: "primary.main",
                opacity: 1,
                bgcolor: alpha(theme.palette.primary.main, 0.08),
              },
            }}
          >
            <ExpandMore sx={{ fontSize: 16 }} />
          </IconButton>
        </Tooltip>
      </Box>

      {/* 计划概览 */}
      <Box
        sx={{
          p: 1.25,
          borderRadius: 1.5,
          bgcolor: alpha(theme.palette.primary.main, 0.04),
          border: `1px solid ${alpha(theme.palette.primary.main, 0.12)}`,
          mb: 1.5,
        }}
      >
        <Typography variant="caption" sx={{ color: "text.secondary", display: "block", mb: 0.75 }}>
          预估执行时间: ~{Math.round(plan.estimatedDurationSecs / 60)} 分钟
        </Typography>
        {plan.reviewerAgents && plan.reviewerAgents.length > 0 && (
          <Box sx={{ mb: 1 }}>
            <Typography
              variant="caption"
              sx={{ color: "text.secondary", display: "block", mb: 0.5 }}
            >
              Reviewer 结构化结论将来自以下角色：
            </Typography>
            <Stack direction="row" spacing={0.5} flexWrap="wrap" useFlexGap>
              {plan.reviewerAgents.map((agent) => {
                const role = compactAgentChip(agent);
                const chip = (
                  <Chip
                    key={agent}
                    size="small"
                    label={role.short}
                    color="secondary"
                    variant="outlined"
                    sx={{ height: 18, fontSize: 9 }}
                  />
                );
                return role.compacted ? (
                  <Tooltip key={agent} title={role.full}>
                    <Box>{chip}</Box>
                  </Tooltip>
                ) : (
                  chip
                );
              })}
            </Stack>
          </Box>
        )}
        {reviewerHeadline && (
          <Box sx={{ mb: 1 }}>
            <Typography
              variant="caption"
              sx={{ color: "text.secondary", display: "block", mb: 0.5 }}
            >
              当前 reviewer 聚合结论：
            </Typography>
            <Chip
              size="small"
              label={reviewerHeadline.label}
              sx={{
                height: 20,
                fontSize: 10,
                fontWeight: 600,
                bgcolor: alpha(reviewerHeadline.color, 0.12),
                color: reviewerHeadline.color,
              }}
            />
          </Box>
        )}
        {reviewerVerdicts.length > 0 && (
          <ReviewerVerdictList
            verdicts={reviewerVerdicts}
            onSelectVerdict={(verdict) => {
              if (!verdict.taskId) return;
              onOpenReviewerTranscript?.(
                verdict.taskId,
                `${normalizeAgentDisplayName(verdict.agentType)}: ${verdict.taskDescription ?? verdict.summary}`,
              );
            }}
          />
        )}
        <Stack direction="row" spacing={0.5} flexWrap="wrap" useFlexGap>
          {plan.selectedAgents.map((agent) => {
            const role = compactAgentChip(agent);
            const chip = (
              <Chip
                key={agent}
                size="small"
                icon={<SmartToy sx={{ fontSize: 10 }} />}
                label={role.short}
                sx={{
                  height: 20,
                  fontSize: 10,
                  bgcolor: alpha(getAgentColor(agent), 0.1),
                  color: getAgentColor(agent),
                }}
              />
            );
            return role.compacted ? (
              <Tooltip key={agent} title={role.full}>
                <Box>{chip}</Box>
              </Tooltip>
            ) : (
              chip
            );
          })}
        </Stack>
        {!hierarchy.legacyFlat && (
          <Box
            sx={{
              mt: 1,
              p: 1,
              borderRadius: 1.25,
              bgcolor: alpha(theme.palette.background.paper, 0.55),
              border: `1px solid ${alpha(theme.palette.primary.main, 0.12)}`,
            }}
          >
            <Typography
              variant="caption"
              sx={{ display: "block", color: "text.secondary", mb: 0.75 }}
            >
              指挥链
            </Typography>
            <Stack direction="row" spacing={0.5} alignItems="center" flexWrap="wrap" useFlexGap>
              <Chip
                size="small"
                label={normalizeAgentDisplayName(hierarchy.entryAgentType)}
                sx={{
                  height: 20,
                  fontSize: 10,
                  bgcolor: alpha(theme.palette.primary.main, 0.12),
                  color: theme.palette.primary.main,
                  fontWeight: 600,
                }}
              />
              <Typography variant="caption" sx={{ color: "text.disabled" }}>
                →
              </Typography>
              <Chip
                size="small"
                label={normalizeAgentDisplayName(hierarchy.executionSupervisorAgentType)}
                sx={{
                  height: 20,
                  fontSize: 10,
                  bgcolor: alpha(theme.palette.success.main, 0.12),
                  color: theme.palette.success.main,
                  fontWeight: 600,
                }}
              />
              <Typography variant="caption" sx={{ color: "text.disabled" }}>
                →
              </Typography>
              <Typography variant="caption" sx={{ color: "text.secondary" }}>
                {hierarchy.children.length} 个专职子 Agent
              </Typography>
            </Stack>
          </Box>
        )}
      </Box>

      {/* 执行阶段 */}
      <Collapse in={expanded}>
        <Box sx={{ display: "flex", flexDirection: "column", gap: 1.5 }}>
          {groups.map((group, groupIdx) => (
            <Box key={groupIdx}>
              {/* 阶段标题 */}
              <Box sx={{ display: "flex", alignItems: "center", gap: 1, mb: 0.75 }}>
                <Box
                  sx={{
                    width: 20,
                    height: 20,
                    borderRadius: "50%",
                    bgcolor: alpha(theme.palette.primary.main, 0.1),
                    color: "primary.main",
                    display: "flex",
                    alignItems: "center",
                    justifyContent: "center",
                    fontSize: 10,
                    fontWeight: 600,
                  }}
                >
                  {groupIdx + 1}
                </Box>
                <Typography
                  variant="caption"
                  sx={{
                    fontWeight: 600,
                    color: "text.primary",
                    textTransform: "uppercase",
                    letterSpacing: 0.5,
                  }}
                >
                  阶段 {groupIdx + 1}
                  {group.length > 1 && (
                    <span style={{ fontWeight: 400, color: "#666", marginLeft: 4 }}>
                      ({group.length} 个并行)
                    </span>
                  )}
                </Typography>
              </Box>

              {/* 阶段任务 */}
              <Box
                sx={{
                  ml: 1.25,
                  pl: 1.5,
                  borderLeft: `2px solid ${alpha(theme.palette.primary.main, 0.2)}`,
                  display: "flex",
                  flexDirection: "column",
                  gap: 0.75,
                }}
              >
                {group.map((taskId) => {
                  const task = plan.subtasks.find((t) => t.id === taskId);
                  if (!task) return null;
                  const taskRole = compactAgentChip(task.agentType, 12);
                  const runtime = runtimeRowsByTaskId[task.id];
                  const isRunning =
                    runtime?.status === "Running" || runtime?.status === "Pending";
                  const isCompleted = runtime?.status === "Completed";
                  const isFailed =
                    runtime?.status === "Failed" || runtime?.status === "Cancelled";
                  const rowAccent = isRunning
                    ? theme.palette.primary.main
                    : isCompleted
                      ? theme.palette.success.main
                      : isFailed
                        ? theme.palette.error.main
                        : theme.palette.divider;

                  return (
                    <Box
                      key={task.id}
                      sx={{
                        p: 1,
                        borderRadius: 1.5,
                        bgcolor: isRunning
                          ? alpha(theme.palette.primary.main, 0.06)
                          : isCompleted
                            ? alpha(theme.palette.success.main, 0.05)
                            : isFailed
                              ? alpha(theme.palette.error.main, 0.05)
                              : "background.paper",
                        border: `1px solid ${alpha(rowAccent, isRunning ? 0.28 : 0.18)}`,
                        display: "flex",
                        alignItems: "flex-start",
                        gap: 1,
                        transition: "border-color 0.18s ease, background-color 0.18s ease, transform 0.18s ease",
                        ...(isRunning && {
                          boxShadow: `0 10px 26px ${alpha(theme.palette.primary.main, 0.08)}`,
                          transform: "translateY(-1px)",
                        }),
                      }}
                    >
                      <RadioButtonUnchecked
                        sx={{
                          fontSize: 16,
                          color: alpha(theme.palette.text.secondary, 0.3),
                          mt: 0.1,
                        }}
                      />
                      <Box sx={{ flex: 1, minWidth: 0 }}>
                        <Stack
                          direction="row"
                          spacing={0.75}
                          alignItems="center"
                          flexWrap="wrap"
                          useFlexGap
                          sx={{ mb: 0.35 }}
                        >
                          <Typography
                            variant="body2"
                            sx={{ fontSize: 12, lineHeight: 1.4, fontWeight: 600 }}
                          >
                            {task.description}
                          </Typography>
                          {runtime && (
                            <Chip
                              size="small"
                              label={
                                runtime.status === "Running"
                                  ? "运行中"
                                  : runtime.status === "Pending"
                                    ? "等待中"
                                    : runtime.status === "Completed"
                                      ? "已完成"
                                      : runtime.status === "Cancelled"
                                        ? "已取消"
                                        : "失败"
                              }
                              sx={{
                                height: 18,
                                fontSize: 9,
                                fontWeight: 700,
                                bgcolor: alpha(rowAccent, 0.12),
                                color: rowAccent,
                              }}
                            />
                          )}
                        </Stack>
                        {task.dependencies.length > 0 && (
                          <Typography
                            variant="caption"
                            sx={{ fontSize: 10, color: "text.secondary", mt: 0.25, display: "block" }}
                          >
                            依赖: {task.dependencies.join(", ")}
                          </Typography>
                        )}
                        {!hierarchy.legacyFlat && (
                          <Typography
                            variant="caption"
                            sx={{ fontSize: 10, color: "text.secondary", mt: 0.25, display: "block" }}
                          >
                            {schedulerStageLabel(task.stage)} · 上级{" "}
                            {normalizeAgentDisplayName(
                              task.supervisorAgentType ??
                                hierarchy.executionSupervisorAgentType,
                            )}
                          </Typography>
                        )}
                        {runtime?.description && runtime.description !== task.description && (
                          <Typography
                            variant="caption"
                            sx={{ fontSize: 10, color: "text.secondary", mt: 0.35, display: "block" }}
                          >
                            当前执行: {runtime.description}
                          </Typography>
                        )}
                      </Box>
                      <Stack direction="row" alignItems="center" spacing={0.5}>
                        <AgentInfoChip
                          agentType={task.agentType}
                          status={runtime?.status}
                          description={runtime?.description ?? task.description}
                          stageLabel={schedulerStageLabel(task.stage)}
                          supervisorLabel={normalizeAgentDisplayName(
                            task.supervisorAgentType ??
                              hierarchy.executionSupervisorAgentType,
                          )}
                          maxChars={taskRole.compacted ? 12 : 16}
                          sx={{
                            bgcolor: alpha(getAgentColor(task.agentType), 0.1),
                            color: getAgentColor(task.agentType),
                          }}
                        />
                        {task.critical && (
                          <Tooltip title="关键任务">
                            <Box
                              sx={{
                                width: 6,
                                height: 6,
                                borderRadius: "50%",
                                bgcolor: "warning.main",
                              }}
                            />
                          </Tooltip>
                        )}
                      </Stack>
                    </Box>
                  );
                })}
              </Box>
            </Box>
          ))}
        </Box>
      </Collapse>
    </Box>
  );
}
