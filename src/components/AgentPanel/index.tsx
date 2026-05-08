/**
 * Agent Panel - 后台 Agent 任务管理
 * 在任务区右侧面板内，从文件夹组件上方向上滑出
 */

import { useMemo, useState } from "react";
import {
  useAgentStore,
  getAgentTypeDisplayName,
  STATUS_COLORS,
  STATUS_ICONS,
  STATUS_LABELS,
  type BackgroundAgentTask,
} from "../../state/agentStore";
import { useSessionStore } from "../../state/sessionStore";
import { RefreshCw, ExternalLink, ChevronUp, StopCircle } from "lucide-react";
import { formatDistanceToNow } from "date-fns";
import { zhCN, enUS } from "date-fns/locale";
import { useLocaleStore } from "../../state/localeStore";
import { invoke } from "@tauri-apps/api/core";
import {
  Box,
  Typography,
  IconButton,
  Chip,
  Stack,
  Paper,
  Button,
  CircularProgress,
} from "@mui/material";
import {
  Close as CloseIcon,
  DeleteOutline as TrashIcon,
  Refresh as RefreshIcon,
  DragHandle as DragHandleIcon,
} from "@mui/icons-material";

/**
 * 嵌入右侧面板内部的抽屉，从底部（文件夹组件上方）向上滑出。
 * 需要父容器有 `position: relative` 和 `overflow: hidden`。
 */
export function AgentPanel() {
  const { showTaskPanel, setTaskPanelVisible } = useAgentStore();

  return (
    <Box
      sx={{
        position: "absolute",
        bottom: 0,
        left: 0,
        right: 0,
        maxHeight: "62%",
        bgcolor: "background.paper",
        borderTop: 1,
        borderColor: "divider",
        boxShadow: showTaskPanel ? "0 -4px 20px rgba(0,0,0,0.12)" : "none",
        transform: showTaskPanel ? "translateY(0)" : "translateY(100%)",
        transition: "transform 0.22s cubic-bezier(0.4, 0, 0.2, 1), box-shadow 0.22s",
        display: "flex",
        flexDirection: "column",
        zIndex: 10,
      }}
    >
      {/* 拖拽把手 */}
      <Box
        onClick={() => setTaskPanelVisible(false)}
        sx={{
          display: "flex",
          justifyContent: "center",
          py: 0.75,
          cursor: "pointer",
          "&:hover": { bgcolor: "action.hover" },
        }}
      >
        <DragHandleIcon sx={{ color: "text.disabled", fontSize: 20 }} />
      </Box>

      <AgentPanelHeader />
      <AgentTaskList />
    </Box>
  );
}

function AgentPanelHeader() {
  const { setTaskPanelVisible, backgroundTasks } = useAgentStore();
  const hasCompleted = backgroundTasks.some(
    (t) =>
      t.status === "completed" ||
      t.status === "failed" ||
      t.status === "cancelled",
  );

  return (
    <Stack
      direction="row"
      alignItems="center"
      justifyContent="space-between"
      sx={{ px: 2, py: 1, borderBottom: 1, borderColor: "divider", flexShrink: 0 }}
    >
      <Box>
        <Typography variant="subtitle2" fontWeight={600}>
          后台 Agent 任务
        </Typography>
        <TaskCountBadge />
      </Box>
      <Stack direction="row" alignItems="center" spacing={0.5}>
        {hasCompleted && (
          <IconButton
            size="small"
            onClick={() => useAgentStore.getState().cleanupCompleted()}
            title="清理已完成任务"
            color="error"
          >
            <TrashIcon fontSize="small" />
          </IconButton>
        )}
        <IconButton size="small" onClick={() => setTaskPanelVisible(false)}>
          <CloseIcon fontSize="small" />
        </IconButton>
      </Stack>
    </Stack>
  );
}

function TaskCountBadge() {
  const { backgroundTasks, getRunningTasks } = useAgentStore();
  const runningCount = getRunningTasks().length;
  const totalCount = backgroundTasks.length;

  if (totalCount === 0) {
    return (
      <Typography variant="caption" color="text.secondary">
        暂无任务
      </Typography>
    );
  }

  return (
    <Typography variant="caption" color="text.secondary">
      {runningCount > 0 && (
        <Typography component="span" variant="caption" color="primary" fontWeight={600}>
          {runningCount} 运行中
        </Typography>
      )}
      {runningCount > 0 && totalCount > runningCount && " / "}
      {totalCount > runningCount && `${totalCount - runningCount} 已完成`}
    </Typography>
  );
}

function AgentTaskList() {
  const { backgroundTasks, selectedTaskId, setSelectedTask } = useAgentStore();
  const { currentSession } = useSessionStore();
  const currentSessionId = currentSession?.id ?? null;

  const tasks = useMemo(() => {
    return backgroundTasks
      .filter((t) => t.sessionId === currentSessionId)
      .sort((a, b) => b.createdAt - a.createdAt);
  }, [backgroundTasks, currentSessionId]);

  if (tasks.length === 0) {
    return (
      <Stack
        flex={1}
        alignItems="center"
        justifyContent="center"
        spacing={2}
        sx={{ p: 3, color: "text.disabled" }}
      >
        <RefreshIcon sx={{ fontSize: 36, opacity: 0.2 }} />
        <Typography variant="body2" align="center" color="text.secondary">
          暂无后台 Agent 任务
          <br />
          <Typography component="span" variant="caption" sx={{ opacity: 0.7 }}>
            可在设置 → Agents 中启动编排任务
          </Typography>
        </Typography>
      </Stack>
    );
  }

  return (
    <Box sx={{ flex: 1, overflowY: "auto" }}>
      {tasks.map((task) => (
        <TaskItem
          key={task.taskId}
          task={task}
          isSelected={task.taskId === selectedTaskId}
          onClick={() => setSelectedTask(task.taskId)}
        />
      ))}
    </Box>
  );
}

interface TaskItemProps {
  task: BackgroundAgentTask;
  isSelected: boolean;
  onClick: () => void;
}

function TaskItem({ task, isSelected, onClick }: TaskItemProps) {
  const { locale } = useLocaleStore();
  const dateLocale = locale === "zh-CN" ? zhCN : enUS;

  const timeText = useMemo(() => {
    if (task.completedAt) {
      return formatDistanceToNow(task.completedAt, { addSuffix: true, locale: dateLocale });
    }
    return formatDistanceToNow(task.createdAt, { addSuffix: true, locale: dateLocale });
  }, [task.completedAt, task.createdAt, dateLocale]);

  return (
    <Box
      onClick={onClick}
      sx={{
        px: 2,
        py: 1.5,
        borderBottom: 1,
        borderColor: "divider",
        borderLeft: 4,
        borderLeftColor: isSelected ? "primary.main" : "transparent",
        bgcolor: isSelected ? "action.selected" : "transparent",
        cursor: "pointer",
        "&:hover": { bgcolor: "action.hover" },
        transition: "background-color 0.15s",
      }}
    >
      <Stack direction="row" spacing={1.5} alignItems="flex-start">
        <Box sx={{ fontSize: 18, lineHeight: 1, mt: 0.25 }} title={STATUS_LABELS[task.status]}>
          {STATUS_ICONS[task.status]}
        </Box>
        <Box flex={1} minWidth={0}>
          <Stack direction="row" spacing={1} alignItems="center">
            <Chip
              label={getAgentTypeDisplayName(task.agentType)}
              size="small"
              sx={{ height: 18, fontSize: 10, fontWeight: 600 }}
            />
            <Typography variant="caption" sx={{ color: STATUS_COLORS[task.status] }}>
              {STATUS_LABELS[task.status]}
            </Typography>
          </Stack>
          <Typography variant="body2" fontWeight={500} noWrap sx={{ mt: 0.5 }}>
            {task.description}
          </Typography>
          <Typography variant="caption" color="text.secondary" sx={{ mt: 0.25, display: "block" }}>
            {timeText}
          </Typography>
          {isSelected && <TaskDetails task={task} />}
        </Box>
      </Stack>
    </Box>
  );
}

function TaskDetails({ task }: { task: BackgroundAgentTask }) {
  const { removeTask } = useAgentStore();
  const [fullOutput, setFullOutput] = useState<string | null>(null);
  const [loadingOutput, setLoadingOutput] = useState(false);
  const [outputExpanded, setOutputExpanded] = useState(false);

  const loadFullOutput = async (e: React.MouseEvent) => {
    e.stopPropagation();
    if (fullOutput !== null) {
      setOutputExpanded((v) => !v);
      return;
    }
    if (!task.outputPath) return;
    setLoadingOutput(true);
    try {
      const res = await invoke<{ content: string }>("read_file", {
        path: task.outputPath,
        offset: 0,
        limit: 5000,
      });
      setFullOutput(res.content);
      setOutputExpanded(true);
    } catch (err) {
      setFullOutput(`读取失败: ${err}`);
      setOutputExpanded(true);
    } finally {
      setLoadingOutput(false);
    }
  };

  return (
    <Box sx={{ mt: 1.5, pt: 1.5, borderTop: 1, borderColor: "divider" }}>
      <Stack spacing={1}>
        <Typography variant="caption" color="text.secondary">
          <strong>任务 ID:</strong> {task.taskId.slice(0, 8)}...
        </Typography>

        {task.resultSummary && (
          <Paper
            variant="outlined"
            sx={{ p: 1, maxHeight: 100, overflowY: "auto", bgcolor: "action.hover" }}
          >
            <Typography
              component="pre"
              variant="caption"
              sx={{ m: 0, whiteSpace: "pre-wrap", wordBreak: "break-word" }}
            >
              {task.resultSummary.slice(0, 500)}
              {task.resultSummary.length > 500 && "..."}
            </Typography>
          </Paper>
        )}

        {outputExpanded && fullOutput !== null && (
          <Paper
            variant="outlined"
            sx={{ p: 1, maxHeight: 160, overflowY: "auto", bgcolor: "action.hover" }}
          >
            <Typography
              component="pre"
              variant="caption"
              sx={{ m: 0, whiteSpace: "pre-wrap", wordBreak: "break-word" }}
            >
              {fullOutput}
            </Typography>
          </Paper>
        )}

        {task.errorMessage && (
          <Paper variant="outlined" sx={{ p: 1, bgcolor: "error.light", borderColor: "error.main" }}>
            <Typography variant="caption" color="error.contrastText">
              {task.errorMessage}
            </Typography>
          </Paper>
        )}

        <Stack direction="row" spacing={1} sx={{ pt: 0.5 }}>
          {task.outputPath && (
            <Button
              size="small"
              variant="text"
              disabled={loadingOutput}
              onClick={loadFullOutput}
              startIcon={
                loadingOutput ? (
                  <CircularProgress size={12} />
                ) : outputExpanded ? (
                  <ChevronUp size={12} />
                ) : (
                  <ExternalLink size={12} />
                )
              }
              sx={{ fontSize: 11, py: 0.25 }}
            >
              {outputExpanded ? "收起输出" : "查看完整输出"}
            </Button>
          )}

          {(task.status === "pending" || task.status === "running") && (
            <CancelOrchestrationButton sessionId={task.sessionId} />
          )}

          {(task.status === "completed" || task.status === "failed" || task.status === "cancelled") && (
            <Button
              size="small"
              variant="text"
              color="error"
              startIcon={<TrashIcon sx={{ fontSize: 12 }} />}
              onClick={(e) => {
                e.stopPropagation();
                removeTask(task.taskId);
              }}
              sx={{ fontSize: 11, py: 0.25 }}
            >
              删除
            </Button>
          )}
        </Stack>
      </Stack>
    </Box>
  );
}

function CancelOrchestrationButton({ sessionId }: { sessionId: string }) {
  const [cancelling, setCancelling] = useState(false);

  const handleCancel = async (e: React.MouseEvent) => {
    e.stopPropagation();
    setCancelling(true);
    try {
      await invoke("cancel_agent_schedule", { sessionId });
    } catch (err) {
      console.error("cancel_agent_schedule failed:", err);
    } finally {
      setCancelling(false);
    }
  };

  return (
    <Button
      size="small"
      variant="text"
      color="warning"
      disabled={cancelling}
      onClick={handleCancel}
      startIcon={cancelling ? <CircularProgress size={12} /> : <StopCircle size={12} />}
      sx={{ fontSize: 11, py: 0.25 }}
    >
      取消编排
    </Button>
  );
}

/** Agent 面板触发按钮 — 嵌入任务区顶部 */
export function AgentPanelButton() {
  const showTaskPanel = useAgentStore((s) => s.showTaskPanel);
  const toggleTaskPanel = useAgentStore((s) => s.toggleTaskPanel);
  const backgroundTasks = useAgentStore((s) => s.backgroundTasks);
  const runningCount = backgroundTasks.filter(
    (t) => t.status === "pending" || t.status === "running",
  ).length;
  const totalCount = backgroundTasks.length;

  return (
    <button
      onClick={toggleTaskPanel}
      title="后台 Agent 任务"
      style={{
        display: "flex",
        alignItems: "center",
        gap: 4,
        padding: "2px 8px",
        borderRadius: 6,
        fontSize: 11,
        fontWeight: 600,
        border: "none",
        cursor: "pointer",
        background: showTaskPanel ? "rgba(99,102,241,0.12)" : "transparent",
        color: showTaskPanel ? "#6366f1" : "inherit",
      }}
    >
      <RefreshCw
        size={13}
        style={{ animation: runningCount > 0 ? "spin 1.5s linear infinite" : "none" }}
      />
      <span>Agent</span>
      {totalCount > 0 && (
        <span
          style={{
            minWidth: 16,
            height: 16,
            padding: "0 4px",
            borderRadius: 8,
            fontSize: 9,
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            background: runningCount > 0 ? "#6366f1" : "#9ca3af",
            color: "#fff",
          }}
        >
          {totalCount}
        </span>
      )}
    </button>
  );
}
