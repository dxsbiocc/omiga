/**
 * Agent Store - 管理后台 Agent 任务状态
 * 
 * 用于跟踪和管理后台运行的 Agent 任务，与 Rust 后端的后台 Agent 系统配合。
 */

import { create } from "zustand";
import { listenTauriEvent } from "../utils/tauriEvents";
import { useSessionStore } from "./sessionStore";

/** 后台 Agent 任务状态 */
export type BackgroundAgentStatus =
  | "pending"
  | "running"
  | "completed"
  | "failed"
  | "cancelled";

/** 后台 Agent 任务信息 */
export interface BackgroundAgentTask {
  /** 任务唯一 ID */
  taskId: string;
  /** Agent 类型 */
  agentType: string;
  /** 任务描述 */
  description: string;
  /** 当前状态 */
  status: BackgroundAgentStatus;
  /** 创建时间 (Unix timestamp ms) */
  createdAt: number;
  /** 开始时间 */
  startedAt?: number;
  /** 完成时间 */
  completedAt?: number;
  /** 结果摘要 */
  resultSummary?: string;
  /** 错误信息 */
  errorMessage?: string;
  /** 输出文件路径 */
  outputPath?: string;
  /** 会话 ID */
  sessionId: string;
  /** 消息 ID */
  messageId: string;
}

/** 原始调度请求（供确认后重发） */
export interface ScheduleRequest {
  userRequest: string;
  projectRoot: string;
  sessionId: string;
  maxAgents?: number;
  autoDecompose?: boolean;
  strategy?: string;
  modeHint?: string;
  skipConfirmation?: boolean;
}

/** 后端 TaskPlan 的 camelCase JSON 形态；用于确认后执行已批准计划，避免重新规划。 */
export interface AgentScheduleTaskPlan {
  planId: string;
  originalRequest: string;
  entryAgentType?: string;
  executionSupervisorAgentType?: string;
  subtasks: Array<{
    id: string;
    description: string;
    agentType: string;
    dependencies: string[];
    critical: boolean;
    estimatedSecs: number;
    timeoutSecs?: number;
    maxRetries?: number;
    supervisorAgentType?: string;
    stage?: string;
    context: string;
  }>;
  executionOrder: string[];
  allowParallel: boolean;
  globalContext: string;
}

/** 确认请求载荷（来自 agent-schedule-confirmation-required 事件） */
export interface ScheduleConfirmationPayload {
  sessionId: string;
  planId: string;
  summary: string;
  estimatedMinutes: number;
  agents: string[];
  plan: AgentScheduleTaskPlan;
  projectRoot: string;
  strategy?: string;
  modeHint?: string;
  originalRequest: ScheduleRequest;
}

/** 后台 Agent 完成事件载荷 */
interface BackgroundAgentCompleteEvent {
  sessionId: string;
  messageId: string;
  taskId: string;
  agentType: string;
  description: string;
  status: BackgroundAgentStatus;
  resultSummary?: string;
  errorMessage?: string;
  outputPath?: string;
}

interface AgentState {
  /** 所有后台 Agent 任务 */
  backgroundTasks: BackgroundAgentTask[];
  /** 当前选中的任务 ID */
  selectedTaskId: string | null;
  /** 是否显示任务详情面板 */
  showTaskPanel: boolean;
  /** 待确认的编排请求（非 null 时弹出确认对话框） */
  pendingConfirmation: ScheduleConfirmationPayload | null;
  /** 编排完成的会话 ID（非 null 时触发 Chat 滚动到底部，消费后置 null） */
  scheduleCompleteSession: string | null;

  /** 添加或更新任务 */
  upsertTask: (task: BackgroundAgentTask) => void;
  /** 更新任务状态 */
  updateTaskStatus: (
    taskId: string,
    status: BackgroundAgentStatus,
    extra?: Partial<BackgroundAgentTask>,
  ) => void;
  /** 移除任务 */
  removeTask: (taskId: string) => void;
  /** 清空已完成/失败的任务 */
  cleanupCompleted: () => void;
  /** 清空所有任务 */
  clearAll: () => void;
  /** 设置选中的任务 */
  setSelectedTask: (taskId: string | null) => void;
  /** 切换任务面板显示 */
  toggleTaskPanel: () => void;
  /** 设置面板显示状态 */
  setTaskPanelVisible: (visible: boolean) => void;
  /** 设置待确认请求 */
  setPendingConfirmation: (payload: ScheduleConfirmationPayload | null) => void;
  /** 设置编排完成会话（触发 Chat 滚到底部） */
  setScheduleCompleteSession: (sessionId: string | null) => void;
  /** 获取会话的所有任务 */
  getSessionTasks: (sessionId: string) => BackgroundAgentTask[];
  /** 获取正在运行的任务 */
  getRunningTasks: () => BackgroundAgentTask[];
  /** 初始化事件监听 */
  initEventListeners: () => Promise<() => void>;
}

export const useAgentStore = create<AgentState>((set, get) => ({
  backgroundTasks: [],
  selectedTaskId: null,
  showTaskPanel: false,
  pendingConfirmation: null,
  scheduleCompleteSession: null,

  upsertTask: (task) =>
    set((s) => {
      const i = s.backgroundTasks.findIndex((t) => t.taskId === task.taskId);
      if (i >= 0) {
        const next = [...s.backgroundTasks];
        next[i] = { ...next[i], ...task };
        return { backgroundTasks: next };
      }
      return { backgroundTasks: [...s.backgroundTasks, task] };
    }),

  updateTaskStatus: (taskId, status, extra) =>
    set((s) => ({
      backgroundTasks: s.backgroundTasks.map((t) => {
        if (t.taskId !== taskId) return t;
        const next = { ...t, ...extra, status };
        if (status === "running" && !next.startedAt) {
          next.startedAt = Date.now();
        }
        if (
          (status === "completed" ||
            status === "failed" ||
            status === "cancelled") &&
          !next.completedAt
        ) {
          next.completedAt = Date.now();
        }
        return next;
      }),
    })),

  removeTask: (taskId) =>
    set((s) => ({
      backgroundTasks: s.backgroundTasks.filter((t) => t.taskId !== taskId),
      selectedTaskId:
        s.selectedTaskId === taskId ? null : s.selectedTaskId,
    })),

  cleanupCompleted: () =>
    set((s) => ({
      backgroundTasks: s.backgroundTasks.filter(
        (t) => t.status !== "completed" && t.status !== "failed" && t.status !== "cancelled",
      ),
    })),

  clearAll: () => set({ backgroundTasks: [], selectedTaskId: null }),

  setSelectedTask: (taskId) => set({ selectedTaskId: taskId }),

  toggleTaskPanel: () => set((s) => ({ showTaskPanel: !s.showTaskPanel })),

  setTaskPanelVisible: (visible) => set({ showTaskPanel: visible }),

  setPendingConfirmation: (payload) => set({ pendingConfirmation: payload }),

  setScheduleCompleteSession: (sessionId) => set({ scheduleCompleteSession: sessionId }),

  getSessionTasks: (sessionId) =>
    get().backgroundTasks.filter((t) => t.sessionId === sessionId),

  getRunningTasks: () =>
    get().backgroundTasks.filter(
      (t) => t.status === "pending" || t.status === "running",
    ),

  initEventListeners: async () => {
    const { upsertTask, updateTaskStatus } = get();

    // 监听任务更新事件
    const unlistenUpdate = await listenTauriEvent<BackgroundAgentTask>(
      "background-agent-update",
      (event) => {
        upsertTask(event.payload);
      },
    );

    // 监听任务完成事件
    const unlistenComplete = await listenTauriEvent<BackgroundAgentCompleteEvent>(
      "background-agent-complete",
      (event) => {
        const {
          taskId,
          status,
          resultSummary,
          errorMessage,
          outputPath,
        } = event.payload;
        updateTaskStatus(taskId, status, {
          resultSummary,
          errorMessage,
          outputPath,
        });
      },
    );

    // 编排完成后刷新父会话并通知 Chat 滚动到底部
    const unlistenSchedule = await listenTauriEvent<{ sessionId: string; messageId: string }>(
      "agent-schedule-complete",
      (event) => {
        const { sessionId } = event.payload;
        const current = useSessionStore.getState().currentSession;
        if (current?.id === sessionId) {
          void useSessionStore.getState().loadSession(sessionId, { silent: true }).then(() => {
            get().setScheduleCompleteSession(sessionId);
          });
        }
      },
    );

    // 编排计划需要确认时，存入 store 触发确认对话框
    const unlistenConfirm = await listenTauriEvent<ScheduleConfirmationPayload>(
      "agent-schedule-confirmation-required",
      (event) => {
        get().setPendingConfirmation(event.payload);
      },
    );

    // 返回清理函数
    return () => {
      unlistenUpdate();
      unlistenComplete();
      unlistenSchedule();
      unlistenConfirm();
    };
  },
}));

/** Agent 类型显示名称映射（显示层科研分析语义；后端 agent id 不变） */
export const AGENT_TYPE_DISPLAY_NAMES: Record<string, string> = {
  "auto": "Auto",
  "general-purpose": "General / 主调度",
  "Explore": "资料探索",
  "Plan": "分析设计",
  "verification": "证据核查",
  "executor": "分析执行",
  "architect": "科学审查",
  "debugger": "问题排查",
  "test-engineer": "论证检查",
  "critic": "论证审查",
  "code-reviewer": "完整性审查",
  "security-reviewer": "风险审查",
  "quality-reviewer": "质量审查",
  "api-reviewer": "口径审查",
  "performance-reviewer": "效率审查",
  "literature-search": "文献检索",
  "deep-research": "深度研究",
  "researcher": "研究员",
  "writer": "报告撰写",
  "data-analysis": "数据分析",
  "data-visual": "可视化",
  "data-analyst": "数据分析",
  "bioinformatics-analyst": "生信分析",
};

/** 将 agent_type 字符串规范化为面向用户的显示名 */
export function normalizeAgentDisplayName(agentType: string): string {
  if (AGENT_TYPE_DISPLAY_NAMES[agentType]) return AGENT_TYPE_DISPLAY_NAMES[agentType];
  // kebab-case → Title Case fallback: "my-agent" → "My Agent"
  return agentType
    .split("-")
    .map((w) => w.charAt(0).toUpperCase() + w.slice(1))
    .join(" ");
}

/** 获取 Agent 类型的显示名称（保留向后兼容） */
export function getAgentTypeDisplayName(agentType: string): string {
  return normalizeAgentDisplayName(agentType);
}

/** 状态颜色映射 */
export const STATUS_COLORS: Record<BackgroundAgentStatus, string> = {
  pending: "text-yellow-500",
  running: "text-blue-500 animate-pulse",
  completed: "text-green-500",
  failed: "text-red-500",
  cancelled: "text-gray-500",
};

/** 状态图标映射 */
export const STATUS_ICONS: Record<BackgroundAgentStatus, string> = {
  pending: "⏳",
  running: "🔄",
  completed: "✅",
  failed: "❌",
  cancelled: "🚫",
};

/** 状态标签映射 */
export const STATUS_LABELS: Record<BackgroundAgentStatus, string> = {
  pending: "等待中",
  running: "运行中",
  completed: "已完成",
  failed: "失败",
  cancelled: "已取消",
};
