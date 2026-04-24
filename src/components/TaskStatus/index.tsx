import { useState, useEffect, useMemo, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  Box,
  Typography,
  Stack,
  Chip,
  Fade,
  Tabs,
  Tab,
  Tooltip,
  Button,
  IconButton,
  Collapse,
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
  Hub,
  WarningAmber,
  Groups,
  ExpandMore,
} from "@mui/icons-material";
import {
  useSessionStore,
  useActivityStore,
  useChatComposerStore,
  type Message,
} from "../../state";
import type { ActiveTodoItem } from "../../state/activityStore";
import { normalizeAgentDisplayName } from "../../state/agentStore";
import { formatExecutionElapsedFixed } from "../ExecutionStepPanel";
import { PlanTodoList, type PlanTodoItem } from "./PlanTodoList";
import { ReactStepList } from "./ReactStepList";
import { SchedulerPlanPanel } from "./SchedulerPlanPanel";
import { RunningTaskCard, PendingTaskCard } from "./TaskCards";
import { RalphTeamStatusPanel } from "./RalphTeamStatusPanel";
import { BackgroundAgentTranscriptDrawer } from "../Chat/BackgroundAgentTranscriptDrawer";
import {
  aggregateReviewerVerdicts,
  isBlockerVerdict,
  overallReviewerHeadline,
  type BackgroundAgentTaskRow,
} from "../../utils/reviewerVerdict";
import { compactLabel, isLabelCompacted } from "../../utils/compactLabel";
import { stringifyUnknown } from "../../utils/stringifyUnknown";
import { parseWorkflowCommand } from "../../utils/workflowCommands";

import { notifyTaskCompleted, notifyTaskFailed } from "../../utils/notifications";
import { OMIGA_COMPOSER_DISPATCH_EVENT } from "../../utils/chatComposerEvents";

interface TodoLine {
  id: string;
  content: string;
  activeForm: string;
  status: string;
}

type ModeLaneInfo = {
  session_id: string;
  mode: string;
  lane_id: string;
  preferred_agent_type?: string | null;
  supplemental_agent_types: string[];
};

type RalphSessionInfo = {
  session_id: string;
  goal: string;
  phase: string;
  iteration: number;
  updated_at?: string;
};

type AutopilotSessionInfo = {
  session_id: string;
  goal: string;
  phase: string;
  qa_cycles: number;
  max_qa_cycles: number;
  updated_at?: string;
};

type TeamSessionInfo = {
  session_id: string;
  goal: string;
  phase: string;
  subtask_count: number;
  completed_count: number;
  failed_count: number;
  running_count: number;
  updated_at?: string;
};

type TimelineEvent = {
  id: string;
  label: string;
  detail?: string;
  tone: "info" | "success" | "warning" | "error";
  at: number;
  action?:
    | { type: "plan" }
    | { type: "mode" }
    | { type: "task"; taskId: string; label?: string }
    | { type: "reviewer"; taskId: string; label?: string };
};

type OrchestrationEventDto = {
  id: string;
  session_id: string;
  round_id?: string | null;
  message_id?: string | null;
  mode?: string | null;
  event_type: string;
  phase?: string | null;
  task_id?: string | null;
  payload: Record<string, unknown> | null;
  created_at: string;
};

type FailureDiagnosticItem = {
  id: string;
  taskId?: string;
  traceEventId?: string;
  agentLabel: string;
  title: string;
  detail?: string;
  summary: string;
  at?: number;
  source: "task" | "reviewer";
};

function orchestrationPhaseLabel(phase: string): string {
  const labels: Record<string, string> = {
    planning: "规划中",
    env_check: "环境检查",
    executing: "执行中",
    quality_check: "质量检查",
    verifying: "验证中",
    intake: "接收中",
    interview: "澄清中",
    expansion: "问题展开",
    design: "分析设计",
    plan: "分析计划",
    implementation: "分析执行",
    qa: "论证中",
    validation: "审查中",
    fixing: "修正中",
    synthesizing: "综合中",
    complete: "已完成",
    failed: "失败",
  };
  return labels[phase] ?? phase;
}

function preflightStageLabel(stage: string): string {
  const labels: Record<string, string> = {
    scheduler_plan: "计划拆解",
    mcp_tools: "MCP 工具准备",
    tool_schemas: "工具清单准备",
    send_message_ready: "进入流式阶段前准备",
  };
  return labels[stage] ?? stage;
}

function orderedPhaseTrack(mode: string, phase: string): {
  steps: Array<{ id: string; label: string; state: "done" | "current" | "pending" }>;
  failed?: boolean;
} {
  const trackMap: Record<string, string[]> = {
    ralph: ["planning", "env_check", "executing", "quality_check", "verifying", "complete"],
    autopilot: [
      "intake",
      "interview",
      "expansion",
      "design",
      "plan",
      "implementation",
      "qa",
      "validation",
      "complete",
    ],
    team: ["planning", "executing", "verifying", "fixing", "synthesizing", "complete"],
  };
  const order = trackMap[mode] ?? [];
  const currentIndex = order.indexOf(phase);
  const failed = phase === "failed";
  const steps = order.map((step, index) => ({
    id: step,
    label: orchestrationPhaseLabel(step),
    state: (
      currentIndex < 0
        ? "pending"
        : index < currentIndex
          ? "done"
          : index === currentIndex
            ? "current"
            : "pending"
    ) as "done" | "current" | "pending",
  }));
  return { steps, failed };
}

function buildResumeCommand(args: {
  ralph: RalphSessionInfo | null;
  autopilot: AutopilotSessionInfo | null;
  team: TeamSessionInfo | null;
}): string | null {
  if (args.autopilot) {
    return `/autopilot ${args.autopilot.goal}`;
  }
  if (args.team) {
    return `/team ${args.team.goal}`;
  }
  if (args.ralph) {
    return `resume ${args.ralph.goal}`;
  }
  return null;
}

function parseEventTime(value?: string | number | null): number | null {
  if (typeof value === "number" && Number.isFinite(value)) return value * 1000;
  if (typeof value === "string" && value.trim()) {
    const ts = Date.parse(value);
    if (Number.isFinite(ts)) return ts;
  }
  return null;
}

function relativeEventTime(ts: number): string {
  const deltaSec = Math.max(0, Math.round((Date.now() - ts) / 1000));
  if (deltaSec < 60) return `${deltaSec}s 前`;
  if (deltaSec < 3600) return `${Math.round(deltaSec / 60)}m 前`;
  if (deltaSec < 86400) return `${Math.round(deltaSec / 3600)}h 前`;
  return `${Math.round(deltaSec / 86400)}d 前`;
}

function stringifyTracePayload(payload: Record<string, unknown> | null): string {
  if (!payload || Object.keys(payload).length === 0) return "{}";
  try {
    return JSON.stringify(payload, null, 2);
  } catch {
    return "{}";
  }
}

function payloadText(value: unknown): string | undefined {
  if (value == null) return undefined;
  if (typeof value === "string") {
    const trimmed = value.trim();
    return trimmed && trimmed !== "[object Object]" ? trimmed : undefined;
  }
  if (typeof value === "number" || typeof value === "boolean") {
    return String(value);
  }
  const text = stringifyUnknown(value).trim();
  return text && text !== "{}" && text !== "[object Object]" ? text : undefined;
}

function taskRowText(value: unknown): string | undefined {
  const text = payloadText(value);
  return text && text !== "{...}" ? text : undefined;
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
  let latestUserIndex = -1;
  for (let i = messages.length - 1; i >= 0; i -= 1) {
    if (messages[i].role === "user") {
      latestUserIndex = i;
      break;
    }
  }

  const startIndex = latestUserIndex >= 0 ? latestUserIndex : 0;
  for (let i = messages.length - 1; i >= startIndex; i--) {
    const m = messages[i];
    if (m.role === "user" && m.initialTodos && m.initialTodos.length > 0) {
      return m.initialTodos.map((todo, idx) => ({
        id: todo.id ?? `plan-todo-${idx}`,
        content: todo.content,
        activeForm: todo.content,
        status: todo.status,
      }));
    }
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

function activeTodoToPlanItem(t: ActiveTodoItem): PlanTodoItem {
  const s = t.status.toLowerCase();
  let status: PlanTodoItem["status"] = "pending";
  if (s.includes("progress")) status = "running";
  else if (s.includes("complete")) status = "completed";
  else if (s.includes("error") || s.includes("fail")) status = "error";
  return { id: t.id, name: t.content || t.activeForm, status };
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

function isPlanRequestMessage(message: Message | null): boolean {
  if (!message || message.role !== "user") return false;
  if (message.composerAgentType === "Plan") return true;
  return parseWorkflowCommand(message.content)?.command === "plan";
}

function getLatestUserMessage(messages: Message[]): Message | null {
  for (let i = messages.length - 1; i >= 0; i -= 1) {
    if (messages[i].role === "user") return messages[i];
  }
  return null;
}

/** 判断任务状态 */
function getTaskStatus(items: PlanTodoItem[]) {
  const running = items.filter((i) => i.status === "running");
  const completed = items.filter((i) => i.status === "completed");
  const pending = items.filter((i) => i.status === "pending");
  const error = items.filter((i) => i.status === "error");
  return { running, completed, pending, error };
}

export function TaskStatus() {
  const composerAgentType = useChatComposerStore((s) => s.composerAgentType);
  /** 与输入框底部「本地 / 沙箱」同一 store，发消息时随 `executionEnvironment` 同步到后端 */
  const executionEnvironment = useChatComposerStore((s) => s.environment);
  const storeMessages = useSessionStore((s) => s.storeMessages);
  const currentSession = useSessionStore((s) => s.currentSession);
  const projectRoot =
    currentSession?.workingDirectory ?? currentSession?.projectPath;
  const executionSteps = useActivityStore((s) => s.executionSteps);
  const executionStartedAt = useActivityStore((s) => s.executionStartedAt);
  const executionEndedAt = useActivityStore((s) => s.executionEndedAt);
  const backgroundJobs = useActivityStore((s) => s.backgroundJobs);
  const activeTodosLive = useActivityStore((s) => s.activeTodos);
  const isConnecting = useActivityStore((s) => s.isConnecting);
  const isStreaming = useActivityStore((s) => s.isStreaming);
  const waitingFirstChunk = useActivityStore((s) => s.waitingFirstChunk);
  const currentToolHint = useActivityStore((s) => s.currentToolHint);

  const [elapsedTick, setElapsedTick] = useState(0);
  const [activeTab, setActiveTab] = useState(0);
  const [modeLanes, setModeLanes] = useState<ModeLaneInfo[]>([]);
  const [reviewerHeadline, setReviewerHeadline] = useState<{ label: string; color: string } | null>(null);
  const [sessionBackgroundTasks, setSessionBackgroundTasks] = useState<BackgroundAgentTaskRow[]>([]);
  const [ralphSessions, setRalphSessions] = useState<RalphSessionInfo[]>([]);
  const [autopilotSessions, setAutopilotSessions] = useState<AutopilotSessionInfo[]>([]);
  const [teamSessions, setTeamSessions] = useState<TeamSessionInfo[]>([]);
  const [orchestrationEvents, setOrchestrationEvents] = useState<OrchestrationEventDto[]>([]);
  const [traceModeFilter, setTraceModeFilter] = useState<string>("all");
  const [traceEventTypeFilter, setTraceEventTypeFilter] = useState<string>("all");
  const [expandedTraceEventId, setExpandedTraceEventId] = useState<string | null>(null);
  const [orchestrationTab, setOrchestrationTab] = useState(0);
  const [statusPanelTab, setStatusPanelTab] = useState(0);
  const [reviewerTranscriptTask, setReviewerTranscriptTask] = useState<{
    taskId: string;
    label?: string;
  } | null>(null);
  const [copiedFailureId, setCopiedFailureId] = useState<string | null>(null);
  const [copiedTraceEventId, setCopiedTraceEventId] = useState<string | null>(null);
  const [dashboardRefreshTick, setDashboardRefreshTick] = useState(0);

  const runActive = executionSteps.length > 0 && executionEndedAt == null;

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
  const latestUserMessage = useMemo(
    () => getLatestUserMessage(storeMessages),
    [storeMessages],
  );

  const todoItems = useMemo(() => {
    // Prefer live activeTodos (updated in real-time during streaming via tool_result events).
    // Fall back to scanning storeMessages after the stream ends and storeMessages syncs.
    if (activeTodosLive !== null) {
      return activeTodosLive.map(activeTodoToPlanItem);
    }
    const todos = latestTodosFromMessages(storeMessages);
    return todos.map(todoToPlanItem);
  }, [activeTodosLive, storeMessages]);

  const schedulerPlan = latestUserMessage?.schedulerPlan ?? null;

  const taskStatus = useMemo(() => {
    return getTaskStatus(todoItems);
  }, [todoItems]);

  // 检测任务状态变化，完成时发送通知
  const prevTaskStatusRef = useRef(taskStatus);
  useEffect(() => {
    const prev = prevTaskStatusRef.current;
    const wasActive = prev.pending.length + prev.running.length > 0;
    const isNowInactive = taskStatus.pending.length + taskStatus.running.length === 0;
    const hasCompleted = taskStatus.completed.length > 0 || taskStatus.error.length > 0;

    if (wasActive && isNowInactive && hasCompleted) {
      if (taskStatus.error.length > 0) {
        void notifyTaskFailed();
      } else {
        void notifyTaskCompleted();
      }
    }
    prevTaskStatusRef.current = taskStatus;
  }, [taskStatus]);

  const hasExecution = executionSteps.length > 0;
  const hasBackground = backgroundJobs.length > 0;
  const hasTodos = todoItems.length > 0;
  const hasSchedulerPlan = schedulerPlan && schedulerPlan.subtasks.length > 1;
  const schedulerTabIndex = hasTodos && hasExecution ? 2 : hasTodos || hasExecution ? 1 : 0;

  // 判断当前模式
  const isPlanMode = composerAgentType === "Plan";
  const isAutoMode = composerAgentType === "auto";
  const isExploreMode = composerAgentType === "Explore";

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
    if (isPlanMode)
      return {
        label: "Plan",
        icon: <Assignment fontSize="small" />,
        color: "warning" as const,
      };
    if (isExploreMode)
      return {
        label: "Explore",
        icon: <Route fontSize="small" />,
        color: "info" as const,
      };
    if (isAutoMode)
      return {
        label: "Auto",
        icon: <SmartToy fontSize="small" />,
        color: "primary" as const,
      };
    // Named non-background agent (e.g. Executor, Architect, Debugger)
    if (
      composerAgentType &&
      composerAgentType !== "general-purpose" &&
      composerAgentType !== "auto"
    )
      return {
        label: normalizeAgentDisplayName(composerAgentType),
        icon: <SmartToy fontSize="small" />,
        color: "secondary" as const,
      };
    if (hasExecution)
      return {
        label: "ReAct",
        icon: <Terminal fontSize="small" />,
        color: "default" as const,
      };
    return {
      label: "就绪",
      icon: <Pending fontSize="small" />,
      color: "default" as const,
    };
  };

  const modeInfo = getModeInfo();
  const currentTurnStartedAt = latestUserMessage?.timestamp ?? 0;
  const scopedSessionBackgroundTasks = useMemo(
    () =>
      sessionBackgroundTasks.filter(
        (task) => (task.created_at ?? 0) * 1000 >= currentTurnStartedAt,
      ),
    [currentTurnStartedAt, sessionBackgroundTasks],
  );
  const scopedOrchestrationEvents = useMemo(
    () =>
      orchestrationEvents.filter(
        (event) => (parseEventTime(event.created_at) ?? 0) >= currentTurnStartedAt,
      ),
    [currentTurnStartedAt, orchestrationEvents],
  );
  const activeLanes = useMemo(() => {
    if (!currentSession?.id) return [];
    return modeLanes.filter((lane) => lane.session_id === currentSession.id);
  }, [modeLanes, currentSession?.id]);

  const currentRalphSession = useMemo(() => {
    if (!currentSession?.id) return null;
    const session =
      ralphSessions.find((item) => item.session_id === currentSession.id) ?? null;
    if (!session) return null;
    const updatedAt = parseEventTime(session.updated_at) ?? 0;
    return updatedAt >= currentTurnStartedAt ? session : null;
  }, [currentSession?.id, currentTurnStartedAt, ralphSessions]);
  const currentAutopilotSession = useMemo(() => {
    if (!currentSession?.id) return null;
    const session =
      autopilotSessions.find((item) => item.session_id === currentSession.id) ?? null;
    if (!session) return null;
    const updatedAt = parseEventTime(session.updated_at) ?? 0;
    return updatedAt >= currentTurnStartedAt ? session : null;
  }, [autopilotSessions, currentSession?.id, currentTurnStartedAt]);
  const currentTeamSession = useMemo(() => {
    if (!currentSession?.id) return null;
    const session =
      teamSessions.find((item) => item.session_id === currentSession.id) ?? null;
    if (!session) return null;
    const updatedAt = parseEventTime(session.updated_at) ?? 0;
    return updatedAt >= currentTurnStartedAt ? session : null;
  }, [currentSession?.id, currentTurnStartedAt, teamSessions]);
  const currentOrchestration = useMemo(() => {
    if (currentAutopilotSession) {
      return {
        mode: "autopilot",
        phase: currentAutopilotSession.phase,
        detail: `论证 ${currentAutopilotSession.qa_cycles}/${currentAutopilotSession.max_qa_cycles}`,
        updatedAt: parseEventTime(currentAutopilotSession.updated_at) ?? Date.now(),
      };
    }
    if (currentTeamSession) {
      return {
        mode: "team",
        phase: currentTeamSession.phase,
        detail: `${currentTeamSession.completed_count}/${currentTeamSession.subtask_count} 子任务完成`,
        updatedAt: parseEventTime(currentTeamSession.updated_at) ?? Date.now(),
      };
    }
    if (currentRalphSession) {
      return {
        mode: "ralph",
        phase: currentRalphSession.phase,
        detail: `第 ${currentRalphSession.iteration} 轮`,
        updatedAt: parseEventTime(currentRalphSession.updated_at) ?? Date.now(),
      };
    }
    return null;
  }, [currentAutopilotSession, currentRalphSession, currentTeamSession]);
  const visibleActiveLanes = useMemo(
    () => (currentOrchestration ? activeLanes : []),
    [activeLanes, currentOrchestration],
  );
  const visibleActiveRoleSummary = useMemo(() => {
    const set = new Set<string>();
    for (const lane of visibleActiveLanes) {
      if (lane.preferred_agent_type) set.add(lane.preferred_agent_type);
      for (const role of lane.supplemental_agent_types) set.add(role);
    }
    return Array.from(set);
  }, [visibleActiveLanes]);
  const currentPhaseTrack = useMemo(
    () =>
      currentOrchestration
        ? orderedPhaseTrack(currentOrchestration.mode, currentOrchestration.phase)
        : null,
    [currentOrchestration],
  );
  const recordedPhaseEvents = useMemo(() => {
    if (!currentOrchestration) return [];
    const phaseEvents = scopedOrchestrationEvents
      .filter((event) => event.mode === currentOrchestration.mode)
      .filter((event) =>
        ["mode_requested", "phase_changed", "mode_completed", "mode_failed"].includes(
          event.event_type,
        ),
      )
      .map((event) => ({
        phase:
          event.phase ||
          (event.event_type === "mode_completed"
            ? "complete"
            : event.event_type === "mode_failed"
              ? "failed"
              : null),
        at: parseEventTime(event.created_at) ?? Date.now(),
      }))
      .filter((event): event is { phase: string; at: number } => Boolean(event.phase))
      .sort((a, b) => a.at - b.at);

    const deduped: Array<{ phase: string; at: number }> = [];
    for (const event of phaseEvents) {
      const last = deduped[deduped.length - 1];
      if (last?.phase === event.phase) continue;
      deduped.push(event);
    }
    return deduped;
  }, [currentOrchestration, scopedOrchestrationEvents]);
  const phaseTrackRows = useMemo(() => {
    if (!currentPhaseTrack) return [];
    const visitedAt = new Map<string, number>();
    for (const event of recordedPhaseEvents) {
      if (!visitedAt.has(event.phase)) {
        visitedAt.set(event.phase, event.at);
      }
    }
    return currentPhaseTrack.steps.map((step) => {
      const ts = visitedAt.get(step.id);
      if (ts != null) {
        return {
          ...step,
          state:
            !currentPhaseTrack.failed && currentOrchestration?.phase === step.id
              ? ("current" as const)
              : ("done" as const),
          at: ts,
        };
      }
      return { ...step, at: undefined };
    });
  }, [currentOrchestration?.phase, currentPhaseTrack, recordedPhaseEvents]);
  const reviewerVerdicts = useMemo(
    () => aggregateReviewerVerdicts(scopedSessionBackgroundTasks),
    [scopedSessionBackgroundTasks],
  );
  const blockerVerdicts = useMemo(
    () => reviewerVerdicts.filter(isBlockerVerdict),
    [reviewerVerdicts],
  );
  const runningWorkerTasks = useMemo(
    () =>
      scopedSessionBackgroundTasks.filter(
        (task) => task.status === "Running" || task.status === "Pending",
      ),
    [scopedSessionBackgroundTasks],
  );
  const persistentTeamJobs = useMemo(
    () =>
      backgroundJobs.filter(
        (job) =>
          job.label.startsWith("executor") ||
          job.label.startsWith("worker") ||
          job.label.startsWith("subtask"),
      ),
    [backgroundJobs],
  );
  const hasPersistentStatus = Boolean(
    currentRalphSession ||
      currentAutopilotSession ||
      currentTeamSession ||
      persistentTeamJobs.length > 0,
  );
  const orchestrationLaneSummary = useMemo(() => {
    if (visibleActiveLanes.length === 0) return null;
    return visibleActiveLanes[0];
  }, [visibleActiveLanes]);
  const headerMetaSummary = useMemo(() => {
    const parts: string[] = [];
    if (visibleActiveLanes.length > 0) {
      const primaryLane = `${visibleActiveLanes[0].mode}:${visibleActiveLanes[0].lane_id}`;
      const extraLaneCount = visibleActiveLanes.length - 1;
      parts.push(
        extraLaneCount > 0
          ? `lane ${primaryLane} +${extraLaneCount}`
          : `lane ${primaryLane}`,
      );
    }
    if (visibleActiveRoleSummary.length > 0) {
      const roleNames = visibleActiveRoleSummary.map((role) =>
        normalizeAgentDisplayName(role),
      );
      const visibleRoles = roleNames.slice(0, 2).join("、");
      const extraRoleCount = roleNames.length - Math.min(roleNames.length, 2);
      parts.push(
        extraRoleCount > 0
          ? `角色 ${visibleRoles} +${extraRoleCount}`
          : `角色 ${visibleRoles}`,
      );
    }
    if (reviewerHeadline) {
      parts.push(`审查 ${reviewerHeadline.label}`);
    }
    return parts.join(" · ");
  }, [reviewerHeadline, visibleActiveLanes, visibleActiveRoleSummary]);
  const orchestrationSummaryText = useMemo(() => {
    const parts: string[] = [];
    if (currentOrchestration) {
      parts.push(
        `${currentOrchestration.mode} · ${orchestrationPhaseLabel(currentOrchestration.phase)}`,
      );
      if (currentOrchestration.detail) {
        parts.push(currentOrchestration.detail);
      }
    }
    if (orchestrationLaneSummary) {
      parts.push(
        `lane ${orchestrationLaneSummary.mode}:${orchestrationLaneSummary.lane_id}`,
      );
      if (orchestrationLaneSummary.preferred_agent_type) {
        parts.push(
          `主角色 ${normalizeAgentDisplayName(
            orchestrationLaneSummary.preferred_agent_type,
          )}`,
        );
      }
    }
    return parts.join(" · ");
  }, [currentOrchestration, orchestrationLaneSummary]);
  const workerTaskPreview = useMemo(() => {
    if (runningWorkerTasks.length === 0) return null;
    const first = runningWorkerTasks[0];
    const prefix = normalizeAgentDisplayName(first.agent_type);
    const summary = compactLabel(first.description, 22);
    if (runningWorkerTasks.length === 1) {
      return `${prefix}：${summary}`;
    }
    return `${prefix}：${summary} 等 ${runningWorkerTasks.length} 个任务`;
  }, [runningWorkerTasks]);
  const blockerPreview = useMemo(() => {
    if (blockerVerdicts.length === 0) return null;
    const first = blockerVerdicts[0];
    const verdictLabel = first.verdict.toUpperCase();
    const summary = compactLabel(first.summary, 24);
    const agentLabel = normalizeAgentDisplayName(first.agentType);
    if (blockerVerdicts.length === 1) {
      return `${agentLabel} ${verdictLabel} · ${summary}`;
    }
    return `${agentLabel} ${verdictLabel} · ${summary} 等 ${blockerVerdicts.length} 条`;
  }, [blockerVerdicts]);
  const failureDiagnostics = useMemo<FailureDiagnosticItem[]>(() => {
    const items: FailureDiagnosticItem[] = [];
    const coveredTaskIds = new Set<string>();
    const findTraceForTask = (taskId?: string) =>
      taskId
        ? scopedOrchestrationEvents.find(
            (event) =>
              event.task_id === taskId &&
              (event.event_type.includes("failed") ||
                event.event_type.includes("cancelled") ||
                event.event_type === "worker_launch_failed"),
          ) ?? scopedOrchestrationEvents.find((event) => event.task_id === taskId)
        : undefined;

    for (const task of scopedSessionBackgroundTasks) {
      const isFailed = task.status === "Failed" || task.status === "Cancelled";
      if (!isFailed && !task.error_message) continue;
      const agentLabel = normalizeAgentDisplayName(task.agent_type);
      const summary =
        taskRowText(task.error_message) ??
        taskRowText(task.result_summary) ??
        (task.status === "Cancelled" ? "任务已取消。" : "后台任务失败，但没有写入错误摘要。");
      const trace = findTraceForTask(task.task_id);
      coveredTaskIds.add(task.task_id);
      items.push({
        id: `task-${task.task_id}`,
        taskId: task.task_id,
        traceEventId: trace?.id,
        agentLabel,
        title: `${agentLabel} ${task.status === "Cancelled" ? "已取消" : "失败"}`,
        detail: task.description,
        summary,
        at:
          parseEventTime(task.completed_at) ??
          parseEventTime(task.created_at) ??
          undefined,
        source: "task",
      });
    }

    for (const verdict of blockerVerdicts) {
      if (verdict.taskId && coveredTaskIds.has(verdict.taskId)) continue;
      const agentLabel = normalizeAgentDisplayName(verdict.agentType);
      const trace = findTraceForTask(verdict.taskId);
      items.push({
        id: `reviewer-${verdict.taskId ?? verdict.agentType}-${verdict.verdict}`,
        taskId: verdict.taskId,
        traceEventId: trace?.id,
        agentLabel,
        title: `${agentLabel} ${verdict.verdict.toUpperCase()}`,
        detail: verdict.taskDescription,
        summary: verdict.summary,
        at: verdict.completedAt ?? verdict.createdAt,
        source: "reviewer",
      });
    }

    return items.sort((a, b) => (b.at ?? 0) - (a.at ?? 0)).slice(0, 4);
  }, [blockerVerdicts, scopedOrchestrationEvents, scopedSessionBackgroundTasks]);
  const latestScheduledMessage =
    latestUserMessage?.schedulerPlan && latestUserMessage.role === "user"
      ? latestUserMessage
      : null;
  const currentPlanTaskRows = useMemo(() => {
    if (!schedulerPlan) return [];
    return scopedSessionBackgroundTasks.filter(
      (task) => task.plan_id && task.plan_id === schedulerPlan.planId,
    );
  }, [schedulerPlan, scopedSessionBackgroundTasks]);
  const isPurePlanReviewState = useMemo(
    () =>
      Boolean(
        hasSchedulerPlan &&
          isPlanRequestMessage(latestScheduledMessage) &&
          currentPlanTaskRows.length === 0 &&
          !currentOrchestration &&
          runningWorkerTasks.length === 0 &&
          blockerVerdicts.length === 0,
      ),
    [
      blockerVerdicts.length,
      currentOrchestration,
      currentPlanTaskRows.length,
      hasSchedulerPlan,
      latestScheduledMessage,
      runningWorkerTasks.length,
    ],
  );
  const hasOrchestrationStatus =
    !isPurePlanReviewState &&
    Boolean(
      currentOrchestration ||
        orchestrationLaneSummary ||
        scopedOrchestrationEvents.length > 0 ||
        blockerVerdicts.length > 0 ||
        runningWorkerTasks.length > 0,
    );
  const showStatusPanel =
    !isPurePlanReviewState && (hasOrchestrationStatus || hasPersistentStatus);
  const selectedStatusPanelTab = hasPersistentStatus
    ? hasOrchestrationStatus
      ? statusPanelTab
      : 1
    : 0;
  const orchestrationTimeline = useMemo<TimelineEvent[]>(() => {
    if (scopedOrchestrationEvents.length > 0) {
      return scopedOrchestrationEvents
        .map((event) => {
          const at = parseEventTime(event.created_at) ?? Date.now();
          const payload = event.payload ?? {};
          const task = event.task_id
            ? scopedSessionBackgroundTasks.find((row) => row.task_id === event.task_id)
            : undefined;
          const payloadAgentType =
            typeof payload.agentType === "string" ? payloadText(payload.agentType) : undefined;
          const agentType = payloadAgentType ?? task?.agent_type ?? "agent";
          const agentLabel = normalizeAgentDisplayName(agentType);
          const reviewerAgentType = payloadAgentType ?? task?.agent_type ?? "reviewer";
          const reviewerAgentLabel = normalizeAgentDisplayName(reviewerAgentType);
          const description = payloadText(payload.description) ?? task?.description;
          const summary =
            payloadText(payload.summary) ??
            taskRowText(task?.result_summary) ??
            taskRowText(task?.error_message);
          const goal = payloadText(payload.goal);
          const verdict = payloadText(payload.verdict) ?? "结论";
          const taskCount =
            typeof payload.taskCount === "number"
              ? `${payload.taskCount} 个子任务`
              : payloadText(payload.taskCount);
          const stage = payloadText(payload.stage);
          const durationMs =
            typeof payload.durationMs === "number"
              ? `${payload.durationMs} ms`
              : payloadText(payload.durationMs);
          const nestedPayload =
            payload.payload && typeof payload.payload === "object"
              ? (payload.payload as Record<string, unknown>)
              : null;
          const cacheStatus = payloadText(nestedPayload?.cacheStatus);
          const toolCount =
            typeof nestedPayload?.toolCount === "number"
              ? `${nestedPayload.toolCount} 个工具`
              : payloadText(nestedPayload?.toolCount);
          const stageError = payloadText(payload.error);
          const taskLabel = description ? `${agentLabel}: ${description}` : undefined;
          const reviewerLabel = summary ? `${reviewerAgentLabel}: ${summary}` : taskLabel;
          const action =
            event.event_type === "schedule_plan_created"
              ? ({ type: "plan" } as const)
              : event.event_type === "mode_requested"
                ? ({ type: "mode" } as const)
                : event.event_type.startsWith("worker_") && event.task_id
                  ? ({
                      type: "task" as const,
                      taskId: event.task_id,
                      label: taskLabel,
                    })
                  : event.event_type === "reviewer_verdict" && event.task_id
                    ? ({
                        type: "reviewer" as const,
                        taskId: event.task_id,
                        label: reviewerLabel,
                      })
                    : event.phase
                      ? ({ type: "mode" as const })
                      : undefined;

          const label =
            event.event_type === "schedule_plan_created"
              ? "调度计划已生成"
              : event.event_type === "resume_requested"
                ? `${event.mode ?? "编排"} 恢复请求`
                : event.event_type === "cancel_requested"
                  ? `${event.mode ?? "编排"} 取消请求`
                  : event.event_type === "cancel_completed"
                    ? `${event.mode ?? "编排"} 已取消`
                    : event.event_type === "verification_started"
                      ? "验证阶段开始"
                      : event.event_type === "fix_started"
                        ? "修复阶段开始"
                        : event.event_type === "synthesizing_started"
                          ? "综合阶段开始"
              : event.event_type === "mode_requested"
                ? `${event.mode ?? "编排"} 模式已触发`
                : event.event_type === "phase_changed"
                  ? `${event.mode ?? "编排"} 切换到 ${orchestrationPhaseLabel(event.phase ?? "")}`
                : event.event_type === "worker_started"
                  ? `${agentLabel} 已启动`
                  : event.event_type === "worker_completed"
                    ? `${agentLabel} 已完成`
                    : event.event_type === "worker_failed"
                      ? `${agentLabel} 失败`
                      : event.event_type === "worker_cancelled"
                        ? `${agentLabel} 已取消`
                        : event.event_type === "worker_launch_failed"
                          ? `${agentLabel} 启动失败`
                          : event.event_type === "reviewer_verdict"
                            ? `${reviewerAgentLabel} 给出 ${verdict}`
                            : event.event_type === "preflight_stage_completed" && stage
                              ? `${preflightStageLabel(stage)}完成`
                              : event.event_type === "preflight_stage_failed" && stage
                                ? `${preflightStageLabel(stage)}失败`
                            : event.event_type;

          const detail =
            event.event_type === "preflight_stage_completed" ||
            event.event_type === "preflight_stage_failed"
              ? [durationMs, cacheStatus, toolCount, stageError]
                  .filter((part): part is string => Boolean(part))
                  .join(" · ")
              : description ||
                summary ||
                goal ||
                taskCount ||
                (event.phase ? orchestrationPhaseLabel(event.phase) : undefined);

          const verdictLower = verdict.toLowerCase();
          const tone =
            event.event_type === "preflight_stage_failed"
              ? "error"
              : event.event_type === "preflight_stage_completed" &&
                  typeof payload.durationMs === "number" &&
                  payload.durationMs >= 1500
                ? "warning"
            : event.event_type.includes("failed")
              ? "error"
              : event.event_type.includes("cancelled") || event.event_type === "cancel_requested"
                ? "warning"
                : event.event_type === "resume_requested"
                  ? "info"
                : event.event_type === "reviewer_verdict" &&
                    ["reject", "fail"].includes(verdictLower)
                  ? "error"
                  : event.event_type === "reviewer_verdict" && verdictLower === "partial"
                    ? "warning"
                    : ["verification_started", "fix_started", "synthesizing_started"].includes(
                        event.event_type,
                      )
                      ? "info"
                    : event.event_type.includes("completed")
                      ? "success"
                      : "info";
          return {
            id: event.id,
            label,
            detail,
            tone,
            at,
            action,
          } satisfies TimelineEvent;
        })
        .sort((a, b) => b.at - a.at)
        .slice(0, 8);
    }

    const events: TimelineEvent[] = [];

    if (latestScheduledMessage?.schedulerPlan) {
      events.push({
        id: `schedule-${latestScheduledMessage.schedulerPlan.planId}`,
        label: "调度计划已生成",
        detail: `${latestScheduledMessage.schedulerPlan.subtasks.length} 个子任务`,
        tone: "info",
        at: latestScheduledMessage.timestamp ?? Date.now(),
        action: { type: "plan" },
      });
    }

    if (currentAutopilotSession) {
      events.push({
        id: `autopilot-${currentAutopilotSession.session_id}`,
        label: "Autopilot 模式活跃",
        detail: `${orchestrationPhaseLabel(currentAutopilotSession.phase)} · 论证 ${currentAutopilotSession.qa_cycles}/${currentAutopilotSession.max_qa_cycles}`,
        tone: "info",
        at:
          parseEventTime(currentAutopilotSession.updated_at) ??
          Date.now(),
        action: { type: "mode" },
      });
    } else if (currentTeamSession) {
      events.push({
        id: `team-${currentTeamSession.session_id}`,
        label: "Team 模式活跃",
        detail: `${orchestrationPhaseLabel(currentTeamSession.phase)} · ${currentTeamSession.completed_count}/${currentTeamSession.subtask_count} 完成`,
        tone: currentTeamSession.failed_count > 0 ? "warning" : "info",
        at: parseEventTime(currentTeamSession.updated_at) ?? Date.now(),
        action: { type: "mode" },
      });
    } else if (currentRalphSession) {
      events.push({
        id: `ralph-${currentRalphSession.session_id}`,
        label: "Ralph 模式活跃",
        detail: `${orchestrationPhaseLabel(currentRalphSession.phase)} · 第 ${currentRalphSession.iteration} 轮`,
        tone: "info",
        at: parseEventTime(currentRalphSession.updated_at) ?? Date.now(),
        action: { type: "mode" },
      });
    }

    for (const task of scopedSessionBackgroundTasks) {
      const baseAt =
        parseEventTime(task.completed_at) ??
        parseEventTime(task.created_at) ??
        Date.now();
      const taskAgentLabel = normalizeAgentDisplayName(task.agent_type);
      if (task.status === "Running" || task.status === "Pending") {
        events.push({
          id: `worker-running-${task.task_id}`,
          label: `${taskAgentLabel} 已启动`,
          detail: task.description,
          tone: "info",
          at: baseAt,
          action: {
            type: "task",
            taskId: task.task_id,
            label: `${taskAgentLabel}: ${task.description}`,
          },
        });
      } else if (task.status === "Completed") {
        events.push({
          id: `worker-complete-${task.task_id}`,
          label: `${taskAgentLabel} 已完成`,
          detail: task.description,
          tone: "success",
          at: baseAt,
          action: {
            type: "task",
            taskId: task.task_id,
            label: `${taskAgentLabel}: ${task.description}`,
          },
        });
      } else if (task.status === "Failed" || task.status === "Cancelled") {
        events.push({
          id: `worker-failed-${task.task_id}`,
          label: `${taskAgentLabel} ${task.status === "Failed" ? "失败" : "已取消"}`,
          detail: task.description,
          tone: "error",
          at: baseAt,
          action: {
            type: "task",
            taskId: task.task_id,
            label: `${taskAgentLabel}: ${task.description}`,
          },
        });
      }
    }

    for (const verdict of reviewerVerdicts) {
      const at = verdict.completedAt ?? verdict.createdAt ?? Date.now();
      const reviewerAgentLabel = normalizeAgentDisplayName(verdict.agentType);
      events.push({
        id: `reviewer-${verdict.taskId ?? verdict.agentType}-${verdict.verdict}`,
        label: `${reviewerAgentLabel} 给出 ${verdict.verdict.toUpperCase()}`,
        detail: verdict.summary,
        tone:
          verdict.verdict === "reject" || verdict.verdict === "fail"
            ? "error"
            : verdict.verdict === "partial"
              ? "warning"
              : "success",
        at,
        action: verdict.taskId
          ? {
              type: "reviewer",
              taskId: verdict.taskId,
              label: `${reviewerAgentLabel}: ${verdict.taskDescription ?? verdict.summary}`,
            }
          : undefined,
      });
    }

    return events
      .sort((a, b) => b.at - a.at)
      .slice(0, 8);
  }, [
    currentAutopilotSession,
    currentRalphSession,
    currentTeamSession,
    latestScheduledMessage,
    scopedOrchestrationEvents,
    reviewerVerdicts,
    scopedSessionBackgroundTasks,
  ]);
  const traceModes = useMemo(
    () =>
      Array.from(
        new Set(
          scopedOrchestrationEvents
            .map((event) => event.mode?.trim())
            .filter((mode): mode is string => Boolean(mode)),
        ),
      ),
    [scopedOrchestrationEvents],
  );
  const traceEventTypes = useMemo(
    () => Array.from(new Set(scopedOrchestrationEvents.map((event) => event.event_type))),
    [scopedOrchestrationEvents],
  );
  const filteredTraceEvents = useMemo(() => {
    return scopedOrchestrationEvents.filter((event) => {
      const modeOk =
        traceModeFilter === "all" || (event.mode ?? "unknown") === traceModeFilter;
      const typeOk =
        traceEventTypeFilter === "all" || event.event_type === traceEventTypeFilter;
      return modeOk && typeOk;
    });
  }, [scopedOrchestrationEvents, traceEventTypeFilter, traceModeFilter]);

  useEffect(() => {
    if (!projectRoot) {
      setModeLanes([]);
      setRalphSessions([]);
      setAutopilotSessions([]);
      setTeamSessions([]);
      return;
    }
    let cancelled = false;
    Promise.all([
      invoke<ModeLaneInfo[]>("list_active_mode_lanes", { projectRoot }),
      invoke<RalphSessionInfo[]>("list_ralph_sessions", { projectRoot }),
      invoke<AutopilotSessionInfo[]>("list_autopilot_sessions", { projectRoot }),
      invoke<TeamSessionInfo[]>("list_team_sessions", { projectRoot }),
    ])
      .then(([lanes, ralph, autopilot, team]) => {
        if (cancelled) return;
        setModeLanes(lanes ?? []);
        setRalphSessions(ralph ?? []);
        setAutopilotSessions(autopilot ?? []);
        setTeamSessions(team ?? []);
      })
      .catch(() => {
        if (!cancelled) {
          setModeLanes([]);
          setRalphSessions([]);
          setAutopilotSessions([]);
          setTeamSessions([]);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [projectRoot, currentSession?.id, isStreaming, isConnecting, dashboardRefreshTick]);

  useEffect(() => {
    if (!currentSession?.id) {
      setReviewerHeadline(null);
      setSessionBackgroundTasks([]);
      setOrchestrationEvents([]);
      return;
    }
    let cancelled = false;
    Promise.all([
      invoke<BackgroundAgentTaskRow[]>("list_session_background_tasks", {
        sessionId: currentSession.id,
      }),
      invoke<OrchestrationEventDto[]>("list_orchestration_events", {
        sessionId: currentSession.id,
        limit: 80,
      }),
    ])
      .then(([rows, events]) => {
        if (cancelled) return;
        const taskRows = rows ?? [];
        setSessionBackgroundTasks(taskRows);
        const verdicts = aggregateReviewerVerdicts(taskRows);
        setReviewerHeadline(overallReviewerHeadline(verdicts));
        setOrchestrationEvents(events ?? []);
      })
      .catch(() => {
        if (!cancelled) {
          setReviewerHeadline(null);
          setSessionBackgroundTasks([]);
          setOrchestrationEvents([]);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [currentSession?.id, isStreaming, isConnecting, dashboardRefreshTick]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    void (async () => {
      unlisten = await listen<{ sessionId: string }>(
        "mock-orchestration-scenario-loaded",
        (event) => {
          if (event.payload.sessionId === currentSession?.id) {
            setDashboardRefreshTick((n) => n + 1);
          }
        },
      );
    })();
    return () => unlisten?.();
  }, [currentSession?.id]);

  const handleCancelCurrentOrchestration = async () => {
    if (!currentSession?.id || !projectRoot) return;
    try {
      if (currentTeamSession) {
        await invoke("cancel_team_session", {
          projectRoot,
          sessionId: currentSession.id,
        });
      } else {
        await invoke("cancel_agent_schedule", { sessionId: currentSession.id });
        if (currentAutopilotSession) {
          await invoke("clear_autopilot_session", {
            projectRoot,
            sessionId: currentSession.id,
          });
        }
        if (currentRalphSession) {
          await invoke("clear_ralph_session", {
            projectRoot,
            sessionId: currentSession.id,
          });
        }
      }
    } catch (error) {
      console.error("cancel current orchestration failed:", error);
    } finally {
      setDashboardRefreshTick((n) => n + 1);
    }
  };

  const handleOpenPrimaryBlocker = () => {
    const blocker = blockerVerdicts[0];
    if (!blocker?.taskId) return;
    setReviewerTranscriptTask({
      taskId: blocker.taskId,
      label: `${blocker.agentType}: ${blocker.taskDescription ?? blocker.summary}`,
    });
  };

  const handleInspectOrchestration = () => {
    if (hasSchedulerPlan) {
      setActiveTab(schedulerTabIndex);
      return;
    }
    setStatusPanelTab(hasPersistentStatus ? 1 : 0);
    setOrchestrationTab(0);
  };

  const handleResumeCurrentMode = () => {
    const content = buildResumeCommand({
      ralph: currentRalphSession,
      autopilot: currentAutopilotSession,
      team: currentTeamSession,
    });
    if (!content) return;
    window.dispatchEvent(
      new CustomEvent(OMIGA_COMPOSER_DISPATCH_EVENT, {
        detail: {
          content,
          autoSend: true,
        },
      }),
    );
  };

  const handleOpenFailureTrace = (item: FailureDiagnosticItem) => {
    setStatusPanelTab(0);
    setOrchestrationTab(2);
    setTraceModeFilter("all");
    setTraceEventTypeFilter("all");
    if (item.traceEventId) {
      setExpandedTraceEventId(item.traceEventId);
    }
  };

  const handleCopyFailure = async (item: FailureDiagnosticItem) => {
    const lines = [
      item.title,
      item.detail ? `任务：${item.detail}` : null,
      `来源：${item.source === "reviewer" ? "Reviewer" : "后台任务"}`,
      `摘要：${item.summary}`,
    ].filter((line): line is string => Boolean(line));
    try {
      await navigator.clipboard.writeText(lines.join("\n"));
      setCopiedFailureId(item.id);
      window.setTimeout(() => setCopiedFailureId(null), 1800);
    } catch {
      /* ignore clipboard failures */
    }
  };

  const handleCopyTracePayload = async (event: OrchestrationEventDto) => {
    try {
      await navigator.clipboard.writeText(stringifyTracePayload(event.payload));
      setCopiedTraceEventId(event.id);
      window.setTimeout(() => setCopiedTraceEventId(null), 1800);
    } catch {
      /* ignore clipboard failures */
    }
  };

  const handleTimelineEvent = (event: TimelineEvent) => {
    const action = event.action;
    if (!action) return;
    switch (action.type) {
      case "plan":
        if (hasSchedulerPlan) {
          setActiveTab(schedulerTabIndex);
        } else {
          setStatusPanelTab(0);
          setOrchestrationTab(2);
          setTraceModeFilter("all");
          setTraceEventTypeFilter("all");
          setExpandedTraceEventId(event.id);
        }
        break;
      case "mode":
        if (hasPersistentStatus) {
          setStatusPanelTab(1);
        } else {
          setStatusPanelTab(0);
          setOrchestrationTab(0);
        }
        break;
      case "task":
      case "reviewer":
        setReviewerTranscriptTask({
          taskId: action.taskId,
          label: action.label,
        });
        break;
      default:
        break;
    }
  };

  return (
    <>
    <Box
      sx={{
        height: "100%",
        display: "flex",
        flexDirection: "column",
        minHeight: 0,
        position: "relative",
      }}
    >
      {/* 头部：模式标识 + 统计 */}
      <Box
        sx={{
          px: 1.5,
          pt: 1.75,
          pb: 1.25,
          borderBottom: 1,
          borderColor: "divider",
        }}
      >
        <Stack spacing={1}>
        <Stack
          direction="row"
          alignItems="center"
          justifyContent="space-between"
          spacing={1}
        >
          <Stack direction="row" alignItems="center" spacing={0.75}>
            <Typography
              variant="body2"
              fontWeight={700}
              sx={{ fontSize: 12, letterSpacing: "0.02em" }}
            >
              任务 / 编排
            </Typography>
            <Chip
              size="small"
              icon={modeInfo.icon}
              label={modeInfo.label}
              color={modeInfo.color}
              sx={{ height: 20, fontSize: 10, fontWeight: 600 }}
            />
            <Tooltip title="与输入区一致" placement="bottom" enterDelay={400}>
              <Chip
                size="small"
                label={
                  executionEnvironment === "local"
                    ? "本地"
                    : executionEnvironment === "ssh"
                      ? "SSH"
                      : "沙箱"
                }
                variant="outlined"
                sx={{
                  height: 20,
                  fontSize: 10,
                  fontWeight: 600,
                  borderColor: alpha(
                    executionEnvironment === "local"
                      ? "#64748b"
                      : "#0ea5e9",
                    0.45,
                  ),
                  color:
                    executionEnvironment === "local"
                      ? "text.secondary"
                      : "#0ea5e9",
                }}
              />
            </Tooltip>
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

        {!isPurePlanReviewState && headerMetaSummary && (
          <Tooltip title={headerMetaSummary} placement="bottom-start" enterDelay={350}>
            <Typography
              variant="caption"
              color="text.secondary"
              sx={{
                display: "block",
                mt: 0.35,
                fontSize: 10,
                lineHeight: 1.45,
              }}
            >
              {headerMetaSummary}
            </Typography>
          </Tooltip>
        )}
        {showStatusPanel && (
          <Box
            sx={{
              mt: 0.5,
              p: 1,
              borderRadius: 1.5,
              border: 1,
              borderColor: alpha("#6366f1", 0.16),
              bgcolor: alpha("#6366f1", 0.04),
            }}
          >
            <Stack spacing={0.75}>
              <Tabs
                value={selectedStatusPanelTab}
                onChange={(_, value) => setStatusPanelTab(value)}
                variant="fullWidth"
                sx={{
                  minHeight: 28,
                  "& .MuiTab-root": {
                    minHeight: 28,
                    py: 0.25,
                    fontSize: 10.5,
                    textTransform: "none",
                  },
                }}
              >
                <Tab label="编排概览" disabled={!hasOrchestrationStatus} />
                <Tab label="持久任务" disabled={!hasPersistentStatus} />
              </Tabs>

              {selectedStatusPanelTab === 0 && hasOrchestrationStatus && (
                <Stack spacing={0.75}>
              <Stack
                direction="row"
                alignItems="center"
                spacing={0.75}
                flexWrap="wrap"
                useFlexGap
              >
                <Hub sx={{ fontSize: 14, color: "#6366f1" }} />
                <Typography variant="caption" sx={{ fontSize: 11, fontWeight: 700 }}>
                  编排概览
                </Typography>
                {currentOrchestration && (
                  <Chip
                    size="small"
                    color="primary"
                    label={`${currentOrchestration.mode} · ${orchestrationPhaseLabel(
                      currentOrchestration.phase,
                    )}`}
                    sx={{ height: 18, fontSize: 9, fontWeight: 700 }}
                  />
                )}
                {blockerVerdicts.length > 0 && (
                  <Chip
                    size="small"
                    icon={<WarningAmber sx={{ fontSize: 12 }} />}
                    label={`${blockerVerdicts.length} 阻断`}
                    sx={{
                      height: 18,
                      fontSize: 9,
                      fontWeight: 700,
                      bgcolor: alpha("#ef4444", 0.12),
                      color: "#ef4444",
                    }}
                  />
                )}
                {runningWorkerTasks.length > 0 && (
                  <Chip
                    size="small"
                    icon={<Groups sx={{ fontSize: 12 }} />}
                    label={`${runningWorkerTasks.length} 运行中`}
                    sx={{
                      height: 18,
                      fontSize: 9,
                      fontWeight: 700,
                      bgcolor: alpha("#0ea5e9", 0.12),
                      color: "#0ea5e9",
                    }}
                  />
                )}
              </Stack>

              {(orchestrationSummaryText || workerTaskPreview || blockerPreview) && (
                <Stack direction="row" spacing={0.6} flexWrap="wrap" useFlexGap>
                  {orchestrationSummaryText && (() => {
                    const short = compactLabel(orchestrationSummaryText, 28);
                    const compacted = isLabelCompacted(orchestrationSummaryText, short);
                    const chip = (
                      <Chip
                        size="small"
                        variant="outlined"
                        label={`上下文: ${short}`}
                        sx={{ height: 18, fontSize: 9, maxWidth: "100%" }}
                      />
                    );
                    return compacted ? (
                      <Tooltip
                        title={orchestrationSummaryText}
                        placement="bottom-start"
                        enterDelay={350}
                      >
                        <Box>{chip}</Box>
                      </Tooltip>
                    ) : chip;
                  })()}

                  {workerTaskPreview && (() => {
                    const short = compactLabel(workerTaskPreview, 20);
                    const compacted = isLabelCompacted(workerTaskPreview, short);
                    const chip = (
                      <Chip
                        size="small"
                        label={`任务: ${short}`}
                        sx={{
                          height: 18,
                          fontSize: 9,
                          bgcolor: alpha("#0ea5e9", 0.08),
                          color: "#0369a1",
                        }}
                      />
                    );
                    return compacted ? (
                      <Tooltip title={`运行任务：${workerTaskPreview}`} placement="bottom-start">
                        <Box>{chip}</Box>
                      </Tooltip>
                    ) : chip;
                  })()}

                  {blockerPreview && (() => {
                    const short = compactLabel(blockerPreview, 18);
                    const compacted = isLabelCompacted(blockerPreview, short);
                    const chip = (
                      <Chip
                        size="small"
                        icon={<WarningAmber sx={{ fontSize: 11 }} />}
                        label={`阻断: ${short}`}
                        sx={{
                          height: 18,
                          fontSize: 9,
                          bgcolor: alpha("#ef4444", 0.08),
                          color: "#dc2626",
                        }}
                      />
                    );
                    return compacted ? (
                      <Tooltip title={`阻断提示：${blockerPreview}`} placement="bottom-start">
                        <Box>{chip}</Box>
                      </Tooltip>
                    ) : chip;
                  })()}
                </Stack>
              )}

              <Stack direction="row" spacing={0.75} flexWrap="wrap" useFlexGap>
                {(currentOrchestration || runningWorkerTasks.length > 0) && (
                  <Button
                    size="small"
                    color="warning"
                    variant="outlined"
                    onClick={handleCancelCurrentOrchestration}
                    sx={{ fontSize: 11, py: 0.25 }}
                  >
                    取消编排
                  </Button>
                )}
                {(hasSchedulerPlan || currentOrchestration) && (
                  <Button
                    size="small"
                    variant="outlined"
                    onClick={handleInspectOrchestration}
                    sx={{ fontSize: 11, py: 0.25 }}
                  >
                    {hasSchedulerPlan ? "计划详情" : "查看状态"}
                  </Button>
                )}
                {currentOrchestration && (
                  <Button
                    size="small"
                    variant="outlined"
                    onClick={handleResumeCurrentMode}
                    sx={{ fontSize: 11, py: 0.25 }}
                  >
                    恢复
                  </Button>
                )}
                {blockerVerdicts.length > 0 && (
                  <Button
                    size="small"
                    color="error"
                    variant="outlined"
                    onClick={handleOpenPrimaryBlocker}
                    sx={{ fontSize: 11, py: 0.25 }}
                  >
                    查看阻断
                  </Button>
                )}
              </Stack>

              <Tabs
                value={orchestrationTab}
                onChange={(_, value) => setOrchestrationTab(value)}
                variant="fullWidth"
                sx={{
                  minHeight: 28,
                  mt: 0.25,
                  "& .MuiTab-root": {
                    minHeight: 28,
                    py: 0.25,
                    fontSize: 10,
                    textTransform: "none",
                  },
                }}
              >
                <Tab label="总览" />
                <Tab label="时间线" />
                <Tab label="Trace" />
              </Tabs>

              <Typography
                variant="caption"
                color="text.secondary"
                sx={{ display: "block", fontSize: 9.5, lineHeight: 1.4 }}
              >
                当前显示：仅当前会话的状态、时间线与原始事件。
              </Typography>

              {orchestrationTab === 0 && currentOrchestration && currentPhaseTrack && (
                <Box
                  sx={{
                    mt: 0.25,
                    pt: 0.75,
                    borderTop: 1,
                    borderColor: alpha("#6366f1", 0.12),
                  }}
                >
                  <Stack
                    direction="row"
                    alignItems="center"
                    justifyContent="space-between"
                    spacing={1}
                    sx={{ mb: 0.75 }}
                  >
                    <Typography
                      variant="caption"
                      color="text.secondary"
                      sx={{ fontSize: 10 }}
                    >
                      阶段进度
                    </Typography>
                    <Typography
                      variant="caption"
                      color="text.secondary"
                      sx={{ fontSize: 9 }}
                    >
                      最近更新：{relativeEventTime(currentOrchestration.updatedAt)}
                    </Typography>
                  </Stack>

                  <Stack spacing={0.6}>
                    {phaseTrackRows.map((step) => {
                      const color =
                        step.state === "done"
                          ? "#22c55e"
                          : step.state === "current"
                            ? "#6366f1"
                            : "#94a3b8";
                      return (
                        <Stack
                          key={step.id}
                          direction="row"
                          alignItems="center"
                          spacing={0.75}
                        >
                          <Box
                            sx={{
                              width: 9,
                              height: 9,
                              borderRadius: "50%",
                              bgcolor:
                                step.state === "pending"
                                  ? alpha(color, 0.18)
                                  : color,
                              border:
                                step.state === "current"
                                  ? `2px solid ${alpha(color, 0.22)}`
                                  : undefined,
                              flexShrink: 0,
                            }}
                          />
                          <Typography
                            variant="caption"
                            sx={{
                              fontSize: 10,
                              fontWeight: step.state === "current" ? 700 : 500,
                              color:
                                step.state === "pending"
                                  ? "text.secondary"
                                  : "text.primary",
                            }}
                          >
                            {step.label}
                          </Typography>
                          {step.at && (
                            <Typography
                              variant="caption"
                              color="text.secondary"
                              sx={{ fontSize: 9 }}
                            >
                              {relativeEventTime(step.at)}
                            </Typography>
                          )}
                          <Chip
                            size="small"
                            label={
                              step.state === "done"
                                ? "done"
                                : step.state === "current"
                                  ? "current"
                                  : "pending"
                            }
                            sx={{
                              ml: "auto",
                              height: 16,
                              fontSize: 8.5,
                              bgcolor: alpha(color, step.state === "pending" ? 0.08 : 0.14),
                              color,
                            }}
                          />
                        </Stack>
                      );
                    })}
                    {recordedPhaseEvents.length > 0 && (
                      <Typography
                        variant="caption"
                        color="text.secondary"
                        sx={{ fontSize: 9 }}
                      >
                        已按真实事件更新时间线。
                      </Typography>
                    )}
                    {currentPhaseTrack.failed && (
                      <Typography
                        variant="caption"
                        sx={{ fontSize: 9.5, color: "#ef4444", fontWeight: 600 }}
                      >
                        当前运行时已进入失败态，需要人工处理或重新恢复。
                      </Typography>
                    )}
                  </Stack>

                  {failureDiagnostics.length > 0 && (
                    <Box
                      sx={{
                        mt: 1,
                        pt: 0.85,
                        borderTop: 1,
                        borderColor: alpha("#ef4444", 0.14),
                      }}
                    >
                      <Stack
                        direction="row"
                        alignItems="center"
                        spacing={0.5}
                        sx={{ mb: 0.75 }}
                      >
                        <WarningAmber sx={{ fontSize: 13, color: "#ef4444" }} />
                        <Typography
                          variant="caption"
                          sx={{ fontSize: 10, fontWeight: 700, color: "#ef4444" }}
                        >
                          失败诊断
                        </Typography>
                        <Chip
                          size="small"
                          label={failureDiagnostics.length}
                          sx={{
                            height: 16,
                            fontSize: 8.5,
                            bgcolor: alpha("#ef4444", 0.1),
                            color: "#ef4444",
                          }}
                        />
                      </Stack>
                      <Typography
                        variant="caption"
                        color="text.secondary"
                        sx={{ display: "block", fontSize: 9.5, lineHeight: 1.45, mb: 0.75 }}
                      >
                        建议先打开队友记录看完整过程；需要原始事件时再看 Trace。
                      </Typography>
                      <Stack spacing={0.6}>
                        {failureDiagnostics.map((item) => (
                          <Box
                            key={item.id}
                            sx={{
                              p: 0.75,
                              borderRadius: 1.25,
                              border: 1,
                              borderColor: alpha("#ef4444", 0.18),
                              bgcolor: alpha("#ef4444", 0.045),
                            }}
                          >
                            <Stack
                              direction="row"
                              spacing={0.75}
                              alignItems="flex-start"
                              justifyContent="space-between"
                            >
                              <Box sx={{ minWidth: 0, flex: 1 }}>
                                <Stack
                                  direction="row"
                                  spacing={0.5}
                                  alignItems="center"
                                  flexWrap="wrap"
                                  useFlexGap
                                  sx={{ mb: 0.25 }}
                                >
                                  <Chip
                                    size="small"
                                    label={item.agentLabel}
                                    sx={{
                                      height: 16,
                                      fontSize: 8.5,
                                      bgcolor: alpha("#ef4444", 0.11),
                                      color: "#ef4444",
                                      fontWeight: 700,
                                    }}
                                  />
                                  <Chip
                                    size="small"
                                    label={item.source === "reviewer" ? "reviewer" : "worker"}
                                    variant="outlined"
                                    sx={{ height: 16, fontSize: 8.5 }}
                                  />
                                  {item.at ? (
                                    <Typography
                                      variant="caption"
                                      color="text.secondary"
                                      sx={{ fontSize: 8.5 }}
                                    >
                                      {relativeEventTime(item.at)}
                                    </Typography>
                                  ) : null}
                                </Stack>
                                <Typography
                                  variant="caption"
                                  sx={{ display: "block", fontSize: 10.5, fontWeight: 700 }}
                                >
                                  {item.title}
                                </Typography>
                                {item.detail ? (
                                  <Typography
                                    variant="caption"
                                    color="text.secondary"
                                    sx={{
                                      display: "block",
                                      fontSize: 9.5,
                                      lineHeight: 1.35,
                                      mt: 0.15,
                                    }}
                                  >
                                    {item.detail}
                                  </Typography>
                                ) : null}
                                <Typography
                                  variant="caption"
                                  sx={{
                                    display: "block",
                                    fontSize: 9.5,
                                    lineHeight: 1.45,
                                    color: "#ef4444",
                                    mt: 0.25,
                                  }}
                                >
                                  {compactLabel(item.summary, 120)}
                                </Typography>
                              </Box>
                            </Stack>
                            <Stack direction="row" spacing={0.5} flexWrap="wrap" useFlexGap sx={{ mt: 0.55 }}>
                              {item.taskId ? (
                                <Button
                                  size="small"
                                  color="error"
                                  variant="outlined"
                                  onClick={() =>
                                    setReviewerTranscriptTask({
                                      taskId: item.taskId!,
                                      label: `${item.agentLabel}: ${item.detail ?? item.summary}`,
                                    })
                                  }
                                  sx={{ minWidth: 0, fontSize: 10, py: 0.15 }}
                                >
                                  队友记录
                                </Button>
                              ) : null}
                              <Button
                                size="small"
                                variant="outlined"
                                onClick={() => handleOpenFailureTrace(item)}
                                disabled={!item.traceEventId && scopedOrchestrationEvents.length === 0}
                                sx={{ minWidth: 0, fontSize: 10, py: 0.15 }}
                              >
                                原始 Trace
                              </Button>
                              <Button
                                size="small"
                                variant="text"
                                onClick={() => void handleCopyFailure(item)}
                                sx={{ minWidth: 0, fontSize: 10, py: 0.15 }}
                              >
                                {copiedFailureId === item.id ? "已复制" : "复制错误"}
                              </Button>
                            </Stack>
                          </Box>
                        ))}
                      </Stack>
                    </Box>
                  )}
                </Box>
              )}
              {orchestrationTab === 0 && (!currentOrchestration || !currentPhaseTrack) && (
                <Box
                  sx={{
                    mt: 0.25,
                    pt: 0.9,
                    borderTop: 1,
                    borderColor: alpha("#6366f1", 0.12),
                  }}
                >
                  <Typography
                    variant="caption"
                    color="text.secondary"
                    sx={{ display: "block", fontSize: 10, lineHeight: 1.6 }}
                  >
                    当前会话暂无可展示的编排总览。发送新任务后，这里会显示阶段进度与失败诊断。
                  </Typography>
                </Box>
              )}

              {orchestrationTab === 1 && orchestrationTimeline.length > 0 && (
                <Box
                  sx={{
                    mt: 0.25,
                    pt: 0.75,
                    borderTop: 1,
                    borderColor: alpha("#6366f1", 0.12),
                  }}
                >
                  <Typography
                    variant="caption"
                    color="text.secondary"
                    sx={{ display: "block", mb: 0.75, fontSize: 10 }}
                  >
                    编排事件时间线
                  </Typography>
                  <Stack spacing={0.75}>
                    {orchestrationTimeline.map((event) => {
                      const color =
                        event.tone === "error"
                          ? "#ef4444"
                          : event.tone === "warning"
                            ? "#f59e0b"
                            : event.tone === "success"
                              ? "#22c55e"
                              : "#6366f1";
                      const clickable = Boolean(event.action);
                      return (
                        <Stack
                          key={event.id}
                          direction="row"
                          spacing={0.75}
                          alignItems="flex-start"
                          onClick={clickable ? () => handleTimelineEvent(event) : undefined}
                          sx={{
                            cursor: clickable ? "pointer" : "default",
                            borderRadius: 1,
                            px: 0.5,
                            py: 0.25,
                            transition: "background-color 0.15s ease",
                            "&:hover": clickable
                              ? {
                                  bgcolor: alpha(color, 0.06),
                                }
                              : undefined,
                          }}
                        >
                          <Box
                            sx={{
                              width: 8,
                              height: 8,
                              borderRadius: "50%",
                              bgcolor: color,
                              mt: 0.4,
                              flexShrink: 0,
                            }}
                          />
                          <Box sx={{ minWidth: 0, flex: 1 }}>
                            <Stack
                              direction="row"
                              spacing={0.75}
                              alignItems="center"
                              justifyContent="space-between"
                            >
                              <Typography
                                variant="caption"
                                sx={{ fontSize: 10.5, fontWeight: 600 }}
                              >
                                {event.label}
                              </Typography>
                              <Typography
                                variant="caption"
                                color="text.secondary"
                                sx={{ fontSize: 9, flexShrink: 0 }}
                              >
                                {relativeEventTime(event.at)}
                              </Typography>
                            </Stack>
                            {event.detail && (
                              <Typography
                                variant="caption"
                                color="text.secondary"
                                sx={{
                                  display: "block",
                                  fontSize: 9.5,
                                  lineHeight: 1.45,
                                  mt: 0.15,
                                }}
                              >
                                {event.detail}
                              </Typography>
                            )}
                            {clickable && (
                              <Typography
                                variant="caption"
                                sx={{
                                  display: "block",
                                  mt: 0.2,
                                  fontSize: 9,
                                  color,
                                  fontWeight: 600,
                                }}
                              >
                                打开关联证据
                              </Typography>
                            )}
                          </Box>
                        </Stack>
                      );
                    })}
                  </Stack>
                </Box>
              )}
              {orchestrationTab === 1 && orchestrationTimeline.length === 0 && (
                <Box
                  sx={{
                    mt: 0.25,
                    pt: 0.9,
                    borderTop: 1,
                    borderColor: alpha("#6366f1", 0.12),
                  }}
                >
                  <Typography
                    variant="caption"
                    color="text.secondary"
                    sx={{ display: "block", fontSize: 10, lineHeight: 1.6 }}
                  >
                    当前会话暂无时间线事件。开始执行或恢复任务后，这里会记录关键编排节点。
                  </Typography>
                </Box>
              )}

              {orchestrationTab === 2 && scopedOrchestrationEvents.length > 0 && (
                <Box
                  sx={{
                    mt: 0.25,
                    pt: 0.75,
                    borderTop: 1,
                    borderColor: alpha("#6366f1", 0.12),
                  }}
                >
                  <Typography
                    variant="caption"
                    color="text.secondary"
                    sx={{ display: "block", mb: 0.75, fontSize: 10 }}
                  >
                    Trace 原始事件
                  </Typography>

                  <Stack spacing={0.75}>
                    <Stack direction="row" spacing={0.5} flexWrap="wrap" useFlexGap>
                      <Chip
                        size="small"
                        label={
                          traceModeFilter === "all"
                            ? `全部模式 ${scopedOrchestrationEvents.length}`
                            : traceModeFilter
                        }
                        color={traceModeFilter === "all" ? "primary" : "default"}
                        variant={traceModeFilter === "all" ? "filled" : "outlined"}
                        onClick={() => setTraceModeFilter("all")}
                        sx={{ height: 18, fontSize: 9, cursor: "pointer" }}
                      />
                      {traceModes.map((mode) => (
                        <Chip
                          key={mode}
                          size="small"
                          label={mode}
                          color={traceModeFilter === mode ? "primary" : "default"}
                          variant={traceModeFilter === mode ? "filled" : "outlined"}
                          onClick={() => setTraceModeFilter(mode)}
                          sx={{ height: 18, fontSize: 9, cursor: "pointer" }}
                        />
                      ))}
                    </Stack>

                    <Stack direction="row" spacing={0.5} flexWrap="wrap" useFlexGap>
                      <Chip
                        size="small"
                        label={
                          traceEventTypeFilter === "all"
                            ? `全部事件 ${filteredTraceEvents.length}`
                            : traceEventTypeFilter
                        }
                        color={traceEventTypeFilter === "all" ? "secondary" : "default"}
                        variant={traceEventTypeFilter === "all" ? "filled" : "outlined"}
                        onClick={() => setTraceEventTypeFilter("all")}
                        sx={{ height: 18, fontSize: 9, cursor: "pointer" }}
                      />
                      {traceEventTypes.slice(0, 10).map((eventType) => (
                        <Chip
                          key={eventType}
                          size="small"
                          label={eventType}
                          color={traceEventTypeFilter === eventType ? "secondary" : "default"}
                          variant={traceEventTypeFilter === eventType ? "filled" : "outlined"}
                          onClick={() => setTraceEventTypeFilter(eventType)}
                          sx={{ height: 18, fontSize: 9, cursor: "pointer" }}
                        />
                      ))}
                    </Stack>

                    {filteredTraceEvents.length === 0 ? (
                      <Typography variant="caption" color="text.secondary">
                        当前筛选条件下暂无 trace 事件。
                      </Typography>
                    ) : (
                      <Stack spacing={0.5}>
                        {filteredTraceEvents.slice(0, 12).map((event) => {
                          const ts = parseEventTime(event.created_at) ?? Date.now();
                          const isExpanded = expandedTraceEventId === event.id;
                          const payloadText = stringifyTracePayload(event.payload);
                          const relatedFailure = failureDiagnostics.find(
                            (item) =>
                              item.traceEventId === event.id ||
                              (event.task_id && item.taskId === event.task_id),
                          );
                          const matchingTimeline = orchestrationTimeline.find(
                            (item) =>
                              item.id === event.id ||
                              (event.task_id &&
                                item.action &&
                                "taskId" in item.action &&
                                item.action.taskId === event.task_id),
                          );
                          return (
                            <Box
                              key={event.id}
                              sx={{
                                border: 1,
                                borderColor: alpha("#6366f1", 0.12),
                                borderRadius: 1.25,
                                bgcolor: alpha("#6366f1", 0.03),
                                overflow: "hidden",
                              }}
                            >
                              <Stack
                                direction="row"
                                alignItems="center"
                                spacing={0.75}
                                sx={{ px: 1, py: 0.75 }}
                              >
                                <Box sx={{ minWidth: 0, flex: 1 }}>
                                  <Stack
                                    direction="row"
                                    spacing={0.5}
                                    alignItems="center"
                                    flexWrap="wrap"
                                    useFlexGap
                                    sx={{ mb: 0.35 }}
                                  >
                                    {event.mode && (
                                      <Chip
                                        size="small"
                                        label={event.mode}
                                        sx={{ height: 16, fontSize: 8.5 }}
                                      />
                                    )}
                                    <Chip
                                      size="small"
                                      label={event.event_type}
                                      color="primary"
                                      variant="outlined"
                                      sx={{ height: 16, fontSize: 8.5 }}
                                    />
                                    {event.phase && (
                                      <Chip
                                        size="small"
                                        label={orchestrationPhaseLabel(event.phase)}
                                        sx={{ height: 16, fontSize: 8.5 }}
                                      />
                                    )}
                                    {relatedFailure && (
                                      <Chip
                                        size="small"
                                        icon={<WarningAmber sx={{ fontSize: 10 }} />}
                                        label="关联失败"
                                        sx={{
                                          height: 16,
                                          fontSize: 8.5,
                                          bgcolor: alpha("#ef4444", 0.1),
                                          color: "#ef4444",
                                        }}
                                      />
                                    )}
                                  </Stack>
                                  <Typography
                                    variant="caption"
                                    sx={{ display: "block", fontSize: 10, fontWeight: 600 }}
                                  >
                                    {matchingTimeline?.label ?? event.event_type}
                                  </Typography>
                                  <Typography
                                    variant="caption"
                                    color="text.secondary"
                                    sx={{ display: "block", fontSize: 9 }}
                                  >
                                    {relativeEventTime(ts)}
                                    {matchingTimeline?.detail ? ` · ${matchingTimeline.detail}` : ""}
                                  </Typography>
                                  {relatedFailure && (
                                    <Typography
                                      variant="caption"
                                      sx={{
                                        display: "block",
                                        mt: 0.2,
                                        fontSize: 9,
                                        lineHeight: 1.35,
                                        color: "#ef4444",
                                      }}
                                    >
                                      {compactLabel(relatedFailure.summary, 96)}
                                    </Typography>
                                  )}
                                </Box>

                                {matchingTimeline?.action && (
                                  <Button
                                    size="small"
                                    variant="outlined"
                                    onClick={() => handleTimelineEvent(matchingTimeline)}
                                    sx={{ fontSize: 10, py: 0.2, minWidth: 0 }}
                                  >
                                    跳转
                                  </Button>
                                )}
                                {relatedFailure?.taskId && (
                                  <Button
                                    size="small"
                                    color="error"
                                    variant="outlined"
                                    onClick={() =>
                                      setReviewerTranscriptTask({
                                        taskId: relatedFailure.taskId!,
                                        label: `${relatedFailure.agentLabel}: ${relatedFailure.detail ?? relatedFailure.summary}`,
                                      })
                                    }
                                    sx={{ fontSize: 10, py: 0.2, minWidth: 0 }}
                                  >
                                    记录
                                  </Button>
                                )}
                                <IconButton
                                  size="small"
                                  onClick={() =>
                                    setExpandedTraceEventId((cur) =>
                                      cur === event.id ? null : event.id,
                                    )
                                  }
                                  sx={{
                                    transform: isExpanded ? "rotate(180deg)" : "rotate(0deg)",
                                    transition: "transform 0.2s",
                                  }}
                                >
                                  <ExpandMore sx={{ fontSize: 16 }} />
                                </IconButton>
                              </Stack>

                              <Collapse in={isExpanded}>
                                <Box
                                  sx={{
                                    px: 1,
                                    pb: 1,
                                    borderTop: 1,
                                    borderColor: alpha("#6366f1", 0.08),
                                    bgcolor: alpha("#6366f1", 0.02),
                                  }}
                                >
                                  <Typography
                                    variant="caption"
                                    color="text.secondary"
                                    sx={{ display: "block", mb: 0.5, mt: 0.75, fontSize: 9 }}
                                  >
                                    原始 payload
                                  </Typography>
                                  <Stack direction="row" spacing={0.5} flexWrap="wrap" useFlexGap sx={{ mb: 0.75 }}>
                                    <Button
                                      size="small"
                                      variant="outlined"
                                      onClick={() => void handleCopyTracePayload(event)}
                                      sx={{ minWidth: 0, fontSize: 10, py: 0.15 }}
                                    >
                                      {copiedTraceEventId === event.id ? "已复制" : "复制 payload"}
                                    </Button>
                                    {relatedFailure && (
                                      <Button
                                        size="small"
                                        color="error"
                                        variant="outlined"
                                        onClick={() => setOrchestrationTab(0)}
                                        sx={{ minWidth: 0, fontSize: 10, py: 0.15 }}
                                      >
                                        回到失败诊断
                                      </Button>
                                    )}
                                    {relatedFailure?.taskId && (
                                      <Button
                                        size="small"
                                        color="error"
                                        variant="text"
                                        onClick={() =>
                                          setReviewerTranscriptTask({
                                            taskId: relatedFailure.taskId!,
                                            label: `${relatedFailure.agentLabel}: ${relatedFailure.detail ?? relatedFailure.summary}`,
                                          })
                                        }
                                        sx={{ minWidth: 0, fontSize: 10, py: 0.15 }}
                                      >
                                        打开队友记录
                                      </Button>
                                    )}
                                  </Stack>
                                  <Box
                                    component="pre"
                                    sx={{
                                      m: 0,
                                      p: 0.75,
                                      borderRadius: 1,
                                      bgcolor: alpha("#0f172a", 0.06),
                                      fontSize: 10,
                                      lineHeight: 1.45,
                                      whiteSpace: "pre-wrap",
                                      wordBreak: "break-word",
                                      overflowX: "auto",
                                    }}
                                  >
                                    {payloadText}
                                  </Box>
                                </Box>
                              </Collapse>
                            </Box>
                          );
                        })}
                      </Stack>
                    )}
                  </Stack>
                </Box>
              )}
              {orchestrationTab === 2 && scopedOrchestrationEvents.length === 0 && (
                <Box
                  sx={{
                    mt: 0.25,
                    pt: 0.9,
                    borderTop: 1,
                    borderColor: alpha("#6366f1", 0.12),
                  }}
                >
                  <Typography
                    variant="caption"
                    color="text.secondary"
                    sx={{ display: "block", fontSize: 10, lineHeight: 1.6 }}
                  >
                    当前会话暂无 Trace 原始事件。只有在实际触发编排后，才会写入底层事件与 payload。
                  </Typography>
                </Box>
              )}
                </Stack>
              )}

              {selectedStatusPanelTab === 1 && hasPersistentStatus && (
                <RalphTeamStatusPanel
                  projectRoot={projectRoot}
                  sessionId={currentSession?.id ?? null}
                  embedded
                />
              )}
            </Stack>
          </Box>
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
          {hasTodos && (
            <Tab
              label="计划清单"
              icon={<Assignment sx={{ fontSize: 14 }} />}
              iconPosition="start"
            />
          )}
          {hasExecution && (
            <Tab
              label="执行步骤"
              icon={<Terminal sx={{ fontSize: 14 }} />}
              iconPosition="start"
            />
          )}
          {hasSchedulerPlan && (
            <Tab
              label="调度计划"
              icon={<Route sx={{ fontSize: 14 }} />}
              iconPosition="start"
            />
          )}
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
                <PlanTodoList items={taskStatus.completed} />
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
                <PlanTodoList items={taskStatus.error} />
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
        {hasSchedulerPlan &&
          activeTab ===
            (hasTodos && hasExecution
              ? 2
              : hasTodos || hasExecution
                ? 1
                : 0) && (
            <Box sx={{ p: 1.5 }}>
              <SchedulerPlanPanel
                plan={schedulerPlan}
                sessionId={currentSession?.id}
                onOpenReviewerTranscript={(taskId, label) =>
                  setReviewerTranscriptTask({ taskId, label })
                }
              />
            </Box>
          )}

        {/* 空状态 */}
        {!hasTodos && !hasExecution && !hasSchedulerPlan && (
          <Box sx={{ p: 2, textAlign: "center" }}>
            <Typography
              variant="body2"
              color="text.secondary"
              sx={{ fontSize: 12 }}
            >
              当前会话暂无任务
            </Typography>
            <Typography
              variant="caption"
              color="text.disabled"
              sx={{ fontSize: 11, mt: 0.5, display: "block" }}
            >
              发送消息后，本会话的任务与执行状态将显示在这里
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
            <Stack
              direction="row"
              alignItems="center"
              spacing={0.75}
              sx={{ mb: 1 }}
            >
              <CloudQueue fontSize="small" sx={{ color: "#6366f1" }} />
              <Typography
                variant="body2"
                fontWeight={600}
                sx={{ fontSize: 12 }}
              >
                后台任务
              </Typography>
              <Chip
                size="small"
                label={backgroundJobs.length}
                sx={{ height: 18, fontSize: 10 }}
              />
            </Stack>
            <Stack spacing={0.75}>
              {backgroundJobs.map((job) => {
                const shortJobLabel = compactLabel(job.label, 36);
                const labelCompacted = isLabelCompacted(job.label, shortJobLabel);
                const labelNode = (
                  <Typography
                    variant="body2"
                    sx={{ fontSize: 12, lineHeight: 1.35 }}
                    noWrap
                  >
                    {shortJobLabel}
                  </Typography>
                );

                return (
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
                      {labelCompacted ? (
                        <Tooltip title={job.label} placement="top">
                          <Box>{labelNode}</Box>
                        </Tooltip>
                      ) : (
                        labelNode
                      )}
                      <Stack
                        direction="row"
                        alignItems="center"
                        spacing={0.5}
                        sx={{ mt: 0.5 }}
                      >
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
                        {(job.state === "error" ||
                          job.state === "interrupted") && (
                          <Chip
                            size="small"
                            label={
                              job.state === "interrupted" ? "已中断" : "失败"
                            }
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
              )})}
            </Stack>
          </Box>
        </Box>
      )}

    </Box>
    <BackgroundAgentTranscriptDrawer
      open={reviewerTranscriptTask !== null}
      onClose={() => setReviewerTranscriptTask(null)}
      sessionId={currentSession?.id ?? null}
      taskId={reviewerTranscriptTask?.taskId ?? null}
      taskLabel={reviewerTranscriptTask?.label}
    />
    </>
  );
}
