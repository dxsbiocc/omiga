/**
 * Agent Store - 管理后台 Agent 任务状态
 * 
 * 用于跟踪和管理后台运行的 Agent 任务，与 Rust 后端的后台 Agent 系统配合。
 */

import { create } from "zustand";
import { listen } from "@tauri-apps/api/event";

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
      backgroundTasks: s.backgroundTasks.map((t) =>
        t.taskId === taskId
          ? {
              ...t,
              status,
              ...extra,
              ...(status === "running" && !t.startedAt
                ? { startedAt: Date.now() }
                : {}),
              ...((status === "completed" ||
                status === "failed" ||
                status === "cancelled") && { completedAt: Date.now() }),
            }
          : t,
      ),
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

  getSessionTasks: (sessionId) =>
    get().backgroundTasks.filter((t) => t.sessionId === sessionId),

  getRunningTasks: () =>
    get().backgroundTasks.filter(
      (t) => t.status === "pending" || t.status === "running",
    ),

  initEventListeners: async () => {
    const { upsertTask, updateTaskStatus } = get();

    // 监听任务更新事件
    const unlistenUpdate = await listen<BackgroundAgentTask>(
      "background-agent-update",
      (event) => {
        upsertTask(event.payload);
      },
    );

    // 监听任务完成事件
    const unlistenComplete = await listen<BackgroundAgentCompleteEvent>(
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

    // 返回清理函数
    return () => {
      unlistenUpdate();
      unlistenComplete();
    };
  },
}));

/** Agent 类型显示名称映射 */
export const AGENT_TYPE_DISPLAY_NAMES: Record<string, string> = {
  "general-purpose": "通用 Agent",
  Explore: "探索 Agent",
  Plan: "规划 Agent",
  verification: "验证 Agent",
};

/** 获取 Agent 类型的显示名称 */
export function getAgentTypeDisplayName(agentType: string): string {
  return AGENT_TYPE_DISPLAY_NAMES[agentType] || agentType;
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
