import { useState, useEffect, useMemo } from "react";
import {
  Box,
  Typography,
  Stack,
  Chip,
  Fade,
  Tabs,
  Tab,
  Divider,
} from "@mui/material";
import { alpha } from "@mui/material/styles";
import {
  Terminal,
  CloudQueue,
  SmartToy,
  Assignment,
  Route,
  CheckCircle,
  Pending,
} from "@mui/icons-material";
import {
  useSessionStore,
  useActivityStore,
  useChatComposerStore,
  type Message,
} from "../../state";
import { formatExecutionElapsedFixed } from "../ExecutionStepPanel";
import { PlanTodoList, type PlanTodoItem } from "./PlanTodoList";
import { ReactStepList } from "./ReactStepList";
import { SchedulerPlanPanel } from "./SchedulerPlanPanel";
import { RunningTaskCard, PendingTaskCard } from "./TaskCards";

interface TodoLine {
  id: string;
  content: string;
  activeForm: string;
  status: string;
}

function parseTodoWriteArgs(raw: string | undefined): TodoLine[] | null {
  if (!raw?.trim()) return null;
  try {
    const j = JSON.parse(raw) as {
      todos?: Array<{
        id?: string;
        content: string;
        activeForm?: string;
        active_form?: string;
        status: string;
      }>;
    };
    if (!j.todos) return [];
    return j.todos.map((t, i) => ({
      id: t.id ?? `todo-${i}`,
      content: t.content,
      activeForm: t.activeForm ?? t.active_form ?? t.content,
      status: String(t.status),
    }));
  } catch {
    return null;
  }
}

function latestTodosFromMessages(messages: Message[]): TodoLine[] {
  for (let i = messages.length - 1; i >= 0; i--) {
    const m = messages[i];
    if (
      m.role === "tool" &&
      m.toolCall?.name === "todo_write" &&
      m.toolCall.arguments
    ) {
      const parsed = parseTodoWriteArgs(m.toolCall.arguments);
      if (parsed !== null) return parsed;
    }
  }
  return [];
}

function todoToPlanItem(t: TodoLine): PlanTodoItem {
  const s = t.status.toLowerCase();
  let status: PlanTodoItem["status"] = "pending";
  if (s.includes("progress")) status = "running";
  else if (s.includes("complete")) status = "completed";
  else if (s.includes("error") || s.includes("fail")) status = "error";
  return {
    id: t.id,
    name: t.content || t.activeForm,
    status,
  };
}

/** 获取当前消息的调度计划 */
function getCurrentSchedulerPlan(messages: Message[]) {
  // 从最新的用户消息中查找调度计划
  for (let i = messages.length - 1; i >= 0; i--) {
    const m = messages[i];
    if (m.role === "user" && m.schedulerPlan) {
      return m.schedulerPlan;
    }
  }
  return null;
}

/** 判断任务状态 */
function getTaskStatus(items: PlanTodoItem[]) {
  const running = items.filter(i => i.status === "running");
  const completed = items.filter(i => i.status === "completed");
  const pending = items.filter(i => i.status === "pending");
  const error = items.filter(i => i.status === "error");
  return { running, completed, pending, error };
}

export function TaskStatus() {
  const composerAgentType = useChatComposerStore((s) => s.composerAgentType);
  const storeMessages = useSessionStore((s) => s.storeMessages);
  const executionSteps = useActivityStore((s) => s.executionSteps);
  const executionStartedAt = useActivityStore((s) => s.executionStartedAt);
  const executionEndedAt = useActivityStore((s) => s.executionEndedAt);
  const backgroundJobs = useActivityStore((s) => s.backgroundJobs);
  const isConnecting = useActivityStore((s) => s.isConnecting);
  const isStreaming = useActivityStore((s) => s.isStreaming);
  const waitingFirstChunk = useActivityStore((s) => s.waitingFirstChunk);
  const currentToolHint = useActivityStore((s) => s.currentToolHint);

  const [elapsedTick, setElapsedTick] = useState(0);
  const [activeTab, setActiveTab] = useState(0);
  
  const runActive =
    executionSteps.length > 0 && executionEndedAt == null;
    
  useEffect(() => {
    if (!runActive) return;
    const id = window.setInterval(() => setElapsedTick((n) => n + 1), 1000);
    return () => window.clearInterval(id);
  }, [runActive]);

  const elapsedLabel = useMemo(
    () =>
      formatExecutionElapsedFixed(
        executionStartedAt,
        executionEndedAt,
        elapsedTick,
      ),
    [executionStartedAt, executionEndedAt, elapsedTick],
  );

  const todoItems = useMemo(() => {
    const todos = latestTodosFromMessages(storeMessages);
    return todos.map(todoToPlanItem);
  }, [storeMessages]);

  const schedulerPlan = useMemo(() => {
    return getCurrentSchedulerPlan(storeMessages);
  }, [storeMessages]);

  const taskStatus = useMemo(() => {
    return getTaskStatus(todoItems);
  }, [todoItems]);

  const hasExecution = executionSteps.length > 0;
  const hasBackground = backgroundJobs.length > 0;
  const hasTodos = todoItems.length > 0;
  const hasSchedulerPlan = schedulerPlan && schedulerPlan.subtasks.length > 1;

  // 判断当前模式
  const isPlanMode = composerAgentType === "Plan";
  const isAutoMode = composerAgentType === "auto";
  const isExploreMode = composerAgentType === "Explore";
  const isReActMode = !isPlanMode && hasExecution;

  const surfaceContext = useMemo(
    () => ({
      isConnecting,
      isStreaming,
      waitingFirstChunk,
      toolHintFallback: currentToolHint,
    }),
    [isConnecting, isStreaming, waitingFirstChunk, currentToolHint],
  );

  // 获取模式标签和图标
  const getModeInfo = () => {
    if (isPlanMode) return { label: "计划模式", icon: <Assignment fontSize="small" />, color: "warning" as const };
    if (isExploreMode) return { label: "探索模式", icon: <Route fontSize="small" />, color: "info" as const };
    if (isAutoMode) return { label: "智能调度", icon: <SmartToy fontSize="small" />, color: "primary" as const };
    if (hasExecution) return { label: "ReAct", icon: <Terminal fontSize="small" />, color: "default" as const };
    return { label: "就绪", icon: <Pending fontSize="small" />, color: "default" as const };
  };

  const modeInfo = getModeInfo();

  return (
    <Box sx={{ height: "100%", display: "flex", flexDirection: "column", minHeight: 0 }}>
      {/* 头部：模式标识 + 统计 */}
      <Box sx={{ px: 1.5, pt: 1.25, pb: 0.75, borderBottom: 1, borderColor: "divider" }}>
        <Stack direction="row" alignItems="center" justifyContent="space-between" spacing={1}>
          <Stack direction="row" alignItems="center" spacing={0.75}>
            <Typography
              variant="body2"
              fontWeight={700}
              sx={{ fontSize: 12, letterSpacing: "0.02em" }}
            >
              任务区
            </Typography>
            <Chip
              size="small"
              icon={modeInfo.icon}
              label={modeInfo.label}
              color={modeInfo.color}
              sx={{ height: 20, fontSize: 10, fontWeight: 600 }}
            />
          </Stack>
          
          {/* 统计显示 */}
          {(hasTodos || hasExecution) && (
            <Stack direction="row" alignItems="center" spacing={0.5}>
              {taskStatus.running.length > 0 && (
                <Chip
                  size="small"
                  label={`${taskStatus.running.length} 运行中`}
                  sx={{ 
                    height: 18, 
                    fontSize: 9, 
                    bgcolor: alpha("#6366f1", 0.1),
                    color: "#6366f1",
                  }}
                />
              )}
              {taskStatus.completed.length > 0 && (
                <Chip
                  size="small"
                  icon={<CheckCircle sx={{ fontSize: 10 }} />}
                  label={taskStatus.completed.length}
                  sx={{ 
                    height: 18, 
                    fontSize: 9, 
                    bgcolor: alpha("#22c55e", 0.1),
                    color: "#22c55e",
                  }}
                />
              )}
              {taskStatus.pending.length > 0 && (
                <Chip
                  size="small"
                  label={`${taskStatus.pending.length} 待办`}
                  variant="outlined"
                  sx={{ height: 18, fontSize: 9 }}
                />
              )}
            </Stack>
          )}
        </Stack>
      </Box>

      {/* Tab 导航 - 当有多个视图时显示 */}
      {(hasTodos || hasExecution || hasSchedulerPlan) && (
        <Tabs
          value={activeTab}
          onChange={(_, v) => setActiveTab(v)}
          variant="fullWidth"
          sx={{ 
            minHeight: 32,
            borderBottom: 1, 
            borderColor: "divider",
            "& .MuiTabs-flexContainer": { gap: 0 },
            "& .MuiTab-root": { 
              minHeight: 32, 
              py: 0.5,
              fontSize: 11,
              textTransform: "none",
            },
          }}
        >
          {hasTodos && <Tab label="计划清单" icon={<Assignment sx={{ fontSize: 14 }} />} iconPosition="start" />}
          {hasExecution && <Tab label="执行步骤" icon={<Terminal sx={{ fontSize: 14 }} />} iconPosition="start" />}
          {hasSchedulerPlan && <Tab label="调度计划" icon={<Route sx={{ fontSize: 14 }} />} iconPosition="start" />}
        </Tabs>
      )}

      {/* 内容区 */}
      <Box sx={{ flex: 1, overflow: "auto", minHeight: 0 }}>
        {/* Plan 模式：待办列表 */}
        {hasTodos && activeTab === 0 && (
          <Box sx={{ p: 1.5 }}>
            {/* 运行中的任务卡片 */}
            {taskStatus.running.length > 0 && (
              <Box sx={{ mb: 2 }}>
                <Typography
                  variant="caption"
                  sx={{
                    fontSize: 10,
                    fontWeight: 600,
                    color: "primary.main",
                    textTransform: "uppercase",
                    letterSpacing: 0.5,
                    mb: 0.75,
                    display: "block",
                  }}
                >
                  进行中
                </Typography>
                <Stack spacing={0.75}>
                  {taskStatus.running.map((item) => (
                    <RunningTaskCard key={item.id} item={item} />
                  ))}
                </Stack>
              </Box>
            )}

            {/* 待办任务 */}
            {taskStatus.pending.length > 0 && (
              <Box sx={{ mb: taskStatus.completed.length > 0 ? 2 : 0 }}>
                <Typography
                  variant="caption"
                  sx={{
                    fontSize: 10,
                    fontWeight: 600,
                    color: "text.secondary",
                    textTransform: "uppercase",
                    letterSpacing: 0.5,
                    mb: 0.75,
                    display: "block",
                  }}
                >
                  待完成
                </Typography>
                <Stack spacing={0.5}>
                  {taskStatus.pending.map((item) => (
                    <PendingTaskCard key={item.id} item={item} />
                  ))}
                </Stack>
              </Box>
            )}

            {/* 已完成任务 */}
            {taskStatus.completed.length > 0 && (
              <Box>
                <Typography
                  variant="caption"
                  sx={{
                    fontSize: 10,
                    fontWeight: 600,
                    color: "success.main",
                    textTransform: "uppercase",
                    letterSpacing: 0.5,
                    mb: 0.75,
                    display: "block",
                  }}
                >
                  已完成
                </Typography>
                <PlanTodoList items={taskStatus.completed.map(id => todoItems.find(i => i.id === id)!).filter(Boolean)} />
              </Box>
            )}

            {/* 错误任务 */}
            {taskStatus.error.length > 0 && (
              <Box sx={{ mt: 2 }}>
                <Typography
                  variant="caption"
                  sx={{
                    fontSize: 10,
                    fontWeight: 600,
                    color: "error.main",
                    textTransform: "uppercase",
                    letterSpacing: 0.5,
                    mb: 0.75,
                    display: "block",
                  }}
                >
                  出错
                </Typography>
                <PlanTodoList items={taskStatus.error.map(id => todoItems.find(i => i.id === id)!).filter(Boolean)} />
              </Box>
            )}
          </Box>
        )}

        {/* ReAct 模式：执行步骤 */}
        {hasExecution && activeTab === (hasTodos ? 1 : 0) && (
          <Box sx={{ p: 1.5 }}>
            <ReactStepList
              steps={executionSteps}
              elapsedLabel={elapsedLabel}
              surfaceContext={surfaceContext}
            />
          </Box>
        )}

        {/* 调度计划视图 */}
        {hasSchedulerPlan && activeTab === (hasTodos && hasExecution ? 2 : hasTodos || hasExecution ? 1 : 0) && (
          <Box sx={{ p: 1.5 }}>
            <SchedulerPlanPanel plan={schedulerPlan} />
          </Box>
        )}

        {/* 空状态 */}
        {!hasTodos && !hasExecution && !hasSchedulerPlan && (
          <Box sx={{ p: 2, textAlign: "center" }}>
            <Typography variant="body2" color="text.secondary" sx={{ fontSize: 12 }}>
              暂无任务
            </Typography>
            <Typography variant="caption" color="text.disabled" sx={{ fontSize: 11, mt: 0.5, display: "block" }}>
              发送消息后任务将显示在这里
            </Typography>
          </Box>
        )}
      </Box>

      {/* 后台任务区 */}
      {hasBackground && (
        <Box
          sx={{
            flexShrink: 0,
            borderTop: 1,
            borderColor: "divider",
            bgcolor: alpha("#6366f1", 0.02),
          }}
        >
          <Box sx={{ px: 1.5, py: 1 }}>
            <Stack direction="row" alignItems="center" spacing={0.75} sx={{ mb: 1 }}>
              <CloudQueue fontSize="small" sx={{ color: "#6366f1" }} />
              <Typography variant="body2" fontWeight={600} sx={{ fontSize: 12 }}>
                后台任务
              </Typography>
              <Chip size="small" label={backgroundJobs.length} sx={{ height: 18, fontSize: 10 }} />
            </Stack>
            <Stack spacing={0.75}>
              {backgroundJobs.map((job) => (
                <Fade key={job.id} in timeout={200}>
                  <Box
                    sx={{
                      display: "flex",
                      alignItems: "flex-start",
                      gap: 1,
                      p: 1,
                      borderRadius: 1.5,
                      bgcolor: alpha("#6366f1", 0.04),
                      border: 1,
                      borderColor: alpha("#6366f1", 0.12),
                    }}
                  >
                    <Terminal
                      fontSize="small"
                      sx={{ color: "#6366f1", mt: 0.15, flexShrink: 0 }}
                    />
                    <Box sx={{ minWidth: 0, flex: 1 }}>
                      <Typography variant="body2" sx={{ fontSize: 12, lineHeight: 1.35 }}>
                        {job.label}
                      </Typography>
                      <Stack direction="row" alignItems="center" spacing={0.5} sx={{ mt: 0.5 }}>
                        {job.state === "running" && (
                          <Chip
                            size="small"
                            label="运行中"
                            sx={{ height: 20, fontSize: 10 }}
                          />
                        )}
                        {job.state === "done" && (
                          <Chip
                            size="small"
                            label="已完成"
                            color="success"
                            variant="outlined"
                            sx={{ height: 20, fontSize: 10 }}
                          />
                        )}
                        {(job.state === "error" || job.state === "interrupted") && (
                          <Chip
                            size="small"
                            label={job.state === "interrupted" ? "已中断" : "失败"}
                            color="warning"
                            variant="outlined"
                            sx={{ height: 20, fontSize: 10 }}
                          />
                        )}
                        {job.exitCode != null && job.state !== "running" && (
                          <Typography variant="caption" color="text.secondary">
                            exit {job.exitCode}
                          </Typography>
                        )}
                      </Stack>
                    </Box>
                  </Box>
                </Fade>
              ))}
            </Stack>
          </Box>
        </Box>
      )}
    </Box>
  );
}
