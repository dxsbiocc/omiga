import { create } from "zustand";
import {
  notifyBackgroundTaskCompleted,
  notifyBackgroundTaskFailed,
} from "../utils/notifications";

/** One row in the pencil-style step list (only done + current; no gray “future” rows). */
export interface ExecutionStep {
  id: string;
  title: string;
  status: "done" | "running";
  /** Tool steps only: collapsed header (e.g. sentence-style description). */
  summary?: string;
  toolName?: string;
  /** Raw tool arguments JSON or command string. */
  input?: string;
  toolOutput?: string;
  failed?: boolean;
}

export interface ToolUseStepDetail {
  summary?: string;
  input?: string;
  toolName?: string;
}

export interface ToolResultStepDetail {
  output?: string;
  failed?: boolean;
}

export interface OperationStepDetail {
  summary?: string;
  output?: string;
  failed?: boolean;
}

/** A single todo item mirroring the `todo_write` tool schema. */
export interface ActiveTodoItem {
  id: string;
  content: string;
  activeForm: string;
  status: string; // "pending" | "in_progress" | "completed"
}

/** Right-panel + header hints: connection/streaming and background shell jobs. */
export interface BackgroundJob {
  id: string;
  toolUseId: string;
  label: string;
  state: "running" | "done" | "error" | "interrupted";
  exitCode?: number;
}

function markStepDone(steps: ExecutionStep[], id: string): ExecutionStep[] {
  return steps.map((s) =>
    s.id === id && s.status === "running" ? { ...s, status: "done" as const } : s,
  );
}

interface ActivityState {
  /** After send, before stream emits `Start`. */
  isConnecting: boolean;
  isStreaming: boolean;
  /** Stream started but no text chunk and no tool row yet. */
  waitingFirstChunk: boolean;
  /** Latest tool name for sidebar hint (e.g. bash, file_read). */
  currentToolHint: string | null;
  backgroundJobs: BackgroundJob[];
  /** Pencil-style execution trace (connect → think → tools / reply). */
  executionSteps: ExecutionStep[];
  executionStartedAt: number | null;
  /** Set when a stream ends; freezes elapsed time in the task panel. */
  executionEndedAt: number | null;

  setConnecting: (v: boolean) => void;
  setStreaming: (streaming: boolean, waitingFirstChunk: boolean) => void;
  setCurrentToolHint: (hint: string | null) => void;
  upsertBackgroundJob: (job: BackgroundJob) => void;
  updateBackgroundJob: (
    toolUseId: string,
    patch: Partial<Pick<BackgroundJob, "state" | "exitCode" | "label">>,
  ) => void;

  beginExecutionRun: (connectTitle?: string) => void;
  onStreamStart: () => void;
  /** First non-empty text in this stream turn. */
  onFirstTextChunk: () => void;
  onToolUseStart: (
    toolUseId: string,
    title: string,
    detail?: ToolUseStepDetail,
  ) => void;
  onToolResultDone: (
    toolUseId: string,
    detail?: ToolResultStepDetail,
  ) => void;
  onOperationStart: (
    operationId: string,
    title: string,
    detail?: OperationStepDetail,
  ) => void;
  onOperationDone: (
    operationId: string,
    title: string,
    detail?: OperationStepDetail,
  ) => void;

  /**
   * Live todos pushed by every `todo_write` tool result during streaming.
   * Null means no call has happened yet this turn; TaskStatus falls back to
   * scanning storeMessages (post-stream).
   */
  activeTodos: ActiveTodoItem[] | null;
  setActiveTodos: (todos: ActiveTodoItem[]) => void;
  clearActiveTodos: () => void;

  clearTransient: () => void;
  /** Clear execution trace (e.g. session switch or failed send before stream). */
  resetExecutionState: () => void;
  /** Mark run finished: freeze timer and mark any running steps done. */
  finalizeExecutionRun: () => void;
  clearBackgroundJobs: () => void;
  clearAllActivity: () => void;
}

export const useActivityStore = create<ActivityState>((set) => ({
  isConnecting: false,
  isStreaming: false,
  waitingFirstChunk: false,
  currentToolHint: null,
  backgroundJobs: [],
  executionSteps: [],
  executionStartedAt: null,
  executionEndedAt: null,
  activeTodos: null,

  setConnecting: (v) => set({ isConnecting: v }),

  setStreaming: (streaming, waitingFirstChunk) =>
    set({ isStreaming: streaming, waitingFirstChunk }),

  setCurrentToolHint: (hint) => set({ currentToolHint: hint }),

  upsertBackgroundJob: (job) =>
    set((s) => {
      const i = s.backgroundJobs.findIndex((j) => j.id === job.id);
      if (i >= 0) {
        const next = [...s.backgroundJobs];
        next[i] = { ...next[i], ...job };
        return { backgroundJobs: next };
      }
      return { backgroundJobs: [...s.backgroundJobs, job] };
    }),

  updateBackgroundJob: (toolUseId, patch) =>
    set((s) => {
      const prevJob = s.backgroundJobs.find((j) => j.toolUseId === toolUseId);
      const nextJobs = s.backgroundJobs.map((j) =>
        j.toolUseId === toolUseId ? { ...j, ...patch } : j,
      );
      const nextJob = nextJobs.find((j) => j.toolUseId === toolUseId);

      // 后台任务完成时发送通知
      if (prevJob && nextJob && prevJob.state === "running" && nextJob.state !== "running") {
        if (nextJob.state === "done") {
          void notifyBackgroundTaskCompleted(nextJob.label);
        } else if (nextJob.state === "error" || nextJob.state === "interrupted") {
          void notifyBackgroundTaskFailed(nextJob.label, nextJob.exitCode !== undefined ? `exit ${nextJob.exitCode}` : undefined);
        }
      }

      return { backgroundJobs: nextJobs };
    }),

  beginExecutionRun: (connectTitle = "等待响应") =>
    set({
      executionSteps: [
        { id: "connect", title: connectTitle, status: "running" },
      ],
      executionStartedAt: Date.now(),
      executionEndedAt: null,
    }),

  onStreamStart: () =>
    set((s) => {
      const next = s.executionSteps.map((st) =>
        st.id === "connect" && st.status === "running"
          ? { ...st, status: "done" as const }
          : st,
      );
      if (next.some((x) => x.id === "think")) {
        return { executionSteps: next };
      }
      return {
        executionSteps: [
          ...next,
          { id: "think", title: "推理中", status: "running" },
        ],
      };
    }),

  onFirstTextChunk: () =>
    set((s) => {
      const running = s.executionSteps.find((x) => x.status === "running");
      if (running?.id === "think") {
        return {
          executionSteps: [
            ...markStepDone(s.executionSteps, "think"),
            { id: "reply", title: "解析输出", status: "running" },
          ],
        };
      }
      if (!running) {
        return {
          executionSteps: [
            ...s.executionSteps,
            {
              id: `reply-${Date.now()}`,
              title: "解析输出",
              status: "running",
            },
          ],
        };
      }
      return s;
    }),

  onToolUseStart: (toolUseId, title, detail) =>
    set((s) => {
      const tid = `tool-${toolUseId}`;
      let next = s.executionSteps.map((st) => {
        if (st.status !== "running") return st;
        if (st.id === "think" || st.id === "reply" || st.id.startsWith("reply-")) {
          return { ...st, status: "done" as const };
        }
        // Don't mark other tool steps as done here — parallel tools may still be running.
        // onToolResultDone is responsible for marking each tool step done when its result arrives.
        return st;
      });
      const exists = next.some((x) => x.id === tid);
      const mergeDetail = (base: ExecutionStep): ExecutionStep => ({
        ...base,
        title,
        status: "running" as const,
        ...(detail?.summary !== undefined ? { summary: detail.summary } : {}),
        ...(detail?.input !== undefined ? { input: detail.input } : {}),
        ...(detail?.toolName !== undefined ? { toolName: detail.toolName } : {}),
      });
      if (exists) {
        next = next.map((st) => (st.id === tid ? mergeDetail(st) : st));
      } else {
        next = [
          ...next,
          mergeDetail({
            id: tid,
            title,
            status: "running",
            summary: detail?.summary ?? title,
            input: detail?.input,
            toolName: detail?.toolName,
          }),
        ];
      }
      return { executionSteps: next };
    }),

  onToolResultDone: (toolUseId, detail) =>
    set((s) => ({
      executionSteps: markStepDone(s.executionSteps, `tool-${toolUseId}`).map(
        (st) => {
          if (st.id !== `tool-${toolUseId}`) return st;
          return {
            ...st,
            ...(detail?.output !== undefined ? { toolOutput: detail.output } : {}),
            ...(detail?.failed !== undefined ? { failed: detail.failed } : {}),
          };
        },
      ),
    })),

  onOperationStart: (operationId, title, detail) =>
    set((s) => {
      const oid = `op-${operationId}`;
      const exists = s.executionSteps.some((step) => step.id === oid);
      const nextStep: ExecutionStep = {
        id: oid,
        title,
        status: "running",
        ...(detail?.summary !== undefined ? { summary: detail.summary } : {}),
        ...(detail?.output !== undefined ? { toolOutput: detail.output } : {}),
      };
      return {
        executionSteps: exists
          ? s.executionSteps.map((step) => (step.id === oid ? nextStep : step))
          : [...s.executionSteps, nextStep],
      };
    }),

  onOperationDone: (operationId, title, detail) =>
    set((s) => {
      const oid = `op-${operationId}`;
      const exists = s.executionSteps.some((step) => step.id === oid);
      const next = exists
        ? markStepDone(s.executionSteps, oid).map((step) =>
            step.id !== oid
              ? step
              : {
                  ...step,
                  ...(detail?.summary !== undefined ? { summary: detail.summary } : {}),
                  ...(detail?.output !== undefined ? { toolOutput: detail.output } : {}),
                  ...(detail?.failed !== undefined ? { failed: detail.failed } : {}),
                },
          )
        : [
            ...s.executionSteps,
            {
              id: oid,
              title,
              status: "done",
              ...(detail?.summary !== undefined ? { summary: detail.summary } : {}),
              ...(detail?.output !== undefined ? { toolOutput: detail.output } : {}),
              ...(detail?.failed !== undefined ? { failed: detail.failed } : {}),
            } satisfies ExecutionStep,
          ];
      return { executionSteps: next };
    }),

  setActiveTodos: (todos) => set({ activeTodos: todos }),
  clearActiveTodos: () => set({ activeTodos: null }),

  clearTransient: () =>
    set({
      isConnecting: false,
      isStreaming: false,
      waitingFirstChunk: false,
      currentToolHint: null,
    }),

  resetExecutionState: () =>
    set({
      executionSteps: [],
      executionStartedAt: null,
      executionEndedAt: null,
      activeTodos: null,
    }),

  finalizeExecutionRun: () =>
    set((s) => {
      const now = Date.now();
      const next = s.executionSteps.map((st) =>
        st.status === "running" ? { ...st, status: "done" as const } : st,
      );
      return {
        executionSteps: next,
        executionEndedAt: now,
      };
    }),

  clearBackgroundJobs: () => set({ backgroundJobs: [] }),

  clearAllActivity: () =>
    set({
      isConnecting: false,
      isStreaming: false,
      waitingFirstChunk: false,
      currentToolHint: null,
      backgroundJobs: [],
      executionSteps: [],
      executionStartedAt: null,
      executionEndedAt: null,
      activeTodos: null,
    }),
}));
