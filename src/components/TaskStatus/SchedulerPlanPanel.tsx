import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
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
  type ReviewerVerdictChip,
} from "../../utils/reviewerVerdict";
import { ReviewerVerdictList } from "../ReviewerVerdictList";
import { normalizeAgentDisplayName } from "../../state/agentStore";
import { compactLabel, isLabelCompacted } from "../../utils/compactLabel";

interface SchedulerPlan {
  planId: string;
  subtasks: Array<{
    id: string;
    description: string;
    agentType: string;
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
  sessionId?: string;
  onOpenReviewerTranscript?: (taskId: string, label?: string) => void;
}

/** 调度计划面板 - 显示多 Agent 编排的执行计划 */
export function SchedulerPlanPanel({
  plan,
  sessionId,
  onOpenReviewerTranscript,
}: SchedulerPlanPanelProps) {
  const [expanded, setExpanded] = useState(true);
  const [reviewerHeadline, setReviewerHeadline] = useState<{ label: string; color: string } | null>(null);
  const [reviewerVerdicts, setReviewerVerdicts] = useState<ReviewerVerdictChip[]>([]);
  const theme = useTheme();

  useEffect(() => {
    if (!sessionId) {
      setReviewerHeadline(null);
      setReviewerVerdicts([]);
      return;
    }
    let cancelled = false;
    invoke<BackgroundAgentTaskRow[]>("list_session_background_tasks", {
      sessionId,
    })
      .then((rows) => {
        if (cancelled) return;
        const scopedRows = (rows ?? []).filter(
          (row) => row.plan_id && row.plan_id === plan.planId,
        );
        const verdicts = aggregateReviewerVerdicts(scopedRows);
        setReviewerVerdicts(verdicts);
        setReviewerHeadline(overallReviewerHeadline(verdicts));
      })
      .catch(() => {
        if (!cancelled) {
          setReviewerHeadline(null);
          setReviewerVerdicts([]);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [plan.planId, sessionId]);

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
              transition: "transform 0.2s",
              p: 0.5,
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

                  return (
                    <Box
                      key={task.id}
                      sx={{
                        p: 1,
                        borderRadius: 1,
                        bgcolor: "background.paper",
                        border: `1px solid ${alpha(theme.palette.divider, 0.5)}`,
                        display: "flex",
                        alignItems: "flex-start",
                        gap: 1,
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
                        <Typography
                          variant="body2"
                          sx={{ fontSize: 12, lineHeight: 1.4, fontWeight: 500 }}
                        >
                          {task.description}
                        </Typography>
                        {task.dependencies.length > 0 && (
                          <Typography
                            variant="caption"
                            sx={{ fontSize: 10, color: "text.secondary", mt: 0.25, display: "block" }}
                          >
                            依赖: {task.dependencies.join(", ")}
                          </Typography>
                        )}
                      </Box>
                      <Stack direction="row" alignItems="center" spacing={0.5}>
                        {taskRole.compacted ? (
                          <Tooltip title={taskRole.full}>
                            <Box>
                              <Chip
                                size="small"
                                label={taskRole.short}
                                sx={{
                                  height: 18,
                                  fontSize: 9,
                                  bgcolor: alpha(getAgentColor(task.agentType), 0.1),
                                  color: getAgentColor(task.agentType),
                                  fontWeight: 500,
                                }}
                              />
                            </Box>
                          </Tooltip>
                        ) : (
                          <Chip
                            size="small"
                            label={taskRole.short}
                            sx={{
                              height: 18,
                              fontSize: 9,
                              bgcolor: alpha(getAgentColor(task.agentType), 0.1),
                              color: getAgentColor(task.agentType),
                              fontWeight: 500,
                            }}
                          />
                        )}
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
