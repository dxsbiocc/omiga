import { normalizeAgentDisplayName } from "../../state/agentStore";
import type { BackgroundAgentTaskRow } from "../../utils/reviewerVerdict";
import { stringifyUnknown } from "../../utils/stringifyUnknown";

export type TimelineEvent = {
  id: string;
  label: string;
  detail?: string;
  tone: "info" | "success" | "warning" | "error";
  at: number;
  action?:
    | { type: "plan" }
    | { type: "mode" }
    | { type: "trace"; eventId: string }
    | { type: "task"; taskId: string; label?: string }
    | { type: "reviewer"; taskId: string; label?: string };
};

export type OrchestrationEventDto = {
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

export type SchedulerPlanDispatchSnapshot = {
  subtasks: Array<{
    id: string;
    description: string;
    agentType: string;
    dependencies?: string[];
    stage?: string;
    critical?: boolean;
  }>;
};

export type DispatchTaskStatus =
  | BackgroundAgentTaskRow["status"]
  | "NotStarted";

export type DispatchSummaryTask = {
  id: string;
  description: string;
  agentType: string;
  status: DispatchTaskStatus;
  stage?: string;
  critical?: boolean;
  dependencies: string[];
  blockedBy: string[];
};

export type TaskDispatchSummary = {
  total: number;
  completed: number;
  running: number;
  pending: number;
  failed: number;
  cancelled: number;
  notStarted: number;
  ready: number;
  blocked: number;
  latestEventAt?: number;
  readyTasks: DispatchSummaryTask[];
  blockedTasks: DispatchSummaryTask[];
  failedTasks: DispatchSummaryTask[];
};

export function orchestrationPhaseLabel(phase: string): string {
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

export function buildTaskDispatchSummary(
  plan: SchedulerPlanDispatchSnapshot | null | undefined,
  taskRows: BackgroundAgentTaskRow[],
  events: OrchestrationEventDto[] = [],
): TaskDispatchSummary | null {
  if (!plan && taskRows.length === 0) return null;

  const latestEventAt = events
    .map((event) => parseEventTime(event.created_at) ?? 0)
    .filter((value) => value > 0)
    .sort((a, b) => b - a)[0];

  if (!plan) {
    const tasks = taskRows.map(rowToDispatchSummaryTask);
    return buildTaskDispatchSummaryFromTasks(tasks, latestEventAt);
  }

  const rowsByTaskId = new Map(taskRows.map((row) => [row.task_id, row]));
  const completedIds = new Set(
    taskRows
      .filter((row) => row.status === "Completed")
      .map((row) => row.task_id),
  );
  const tasks = plan.subtasks.map((task) => {
    const row = rowsByTaskId.get(task.id);
    const dependencies = task.dependencies ?? [];
    const status = row?.status ?? "NotStarted";
    const blockedBy =
      status === "Completed" ||
      status === "Running" ||
      status === "Failed" ||
      status === "Cancelled"
        ? []
        : dependencies.filter((dependency) => !completedIds.has(dependency));
    return {
      id: task.id,
      description: row?.description ?? task.description,
      agentType: row?.agent_type ?? task.agentType,
      status,
      stage: task.stage,
      critical: task.critical,
      dependencies,
      blockedBy,
    } satisfies DispatchSummaryTask;
  });

  return buildTaskDispatchSummaryFromTasks(tasks, latestEventAt);
}

function rowToDispatchSummaryTask(row: BackgroundAgentTaskRow): DispatchSummaryTask {
  return {
    id: row.task_id,
    description: row.description,
    agentType: row.agent_type,
    status: row.status,
    dependencies: [],
    blockedBy: [],
  };
}

function buildTaskDispatchSummaryFromTasks(
  tasks: DispatchSummaryTask[],
  latestEventAt?: number,
): TaskDispatchSummary {
  const readyTasks = tasks.filter(
    (task) =>
      (task.status === "NotStarted" || task.status === "Pending") &&
      task.blockedBy.length === 0,
  );
  const blockedTasks = tasks.filter(
    (task) =>
      (task.status === "NotStarted" || task.status === "Pending") &&
      task.blockedBy.length > 0,
  );
  const failedTasks = tasks.filter(
    (task) => task.status === "Failed" || task.status === "Cancelled",
  );

  return {
    total: tasks.length,
    completed: tasks.filter((task) => task.status === "Completed").length,
    running: tasks.filter((task) => task.status === "Running").length,
    pending: tasks.filter((task) => task.status === "Pending").length,
    failed: tasks.filter((task) => task.status === "Failed").length,
    cancelled: tasks.filter((task) => task.status === "Cancelled").length,
    notStarted: tasks.filter((task) => task.status === "NotStarted").length,
    ready: readyTasks.length,
    blocked: blockedTasks.length,
    latestEventAt,
    readyTasks,
    blockedTasks,
    failedTasks,
  };
}

function preflightStageLabel(stage: string): string {
  const labels: Record<string, string> = {
    scheduler_plan: "计划拆解",
    mcp_tools: "MCP 工具准备",
    tool_schemas: "工具清单准备",
    send_message_ready: "进入流式阶段前准备",
    auto_compact: "上下文压缩",
  };
  return labels[stage] ?? stage;
}

export function parseEventTime(value?: string | number | null): number | null {
  if (typeof value === "number" && Number.isFinite(value)) return value * 1000;
  if (typeof value === "string" && value.trim()) {
    const ts = Date.parse(value);
    if (Number.isFinite(ts)) return ts;
  }
  return null;
}

export function stringifyTracePayload(payload: Record<string, unknown> | null): string {
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

export function buildOrchestrationTimelineFromEvents(
  events: OrchestrationEventDto[],
  sessionBackgroundTasks: BackgroundAgentTaskRow[],
): TimelineEvent[] {
  return events
    .map((event) => {
      const at = parseEventTime(event.created_at) ?? Date.now();
      const payload = event.payload ?? {};
      const task = event.task_id
        ? sessionBackgroundTasks.find((row) => row.task_id === event.task_id)
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
      const toolName =
        payloadText(payload.toolName) ??
        payloadText(payload.tool_name) ??
        payloadText(payload.name);
      const toolOutputPreview =
        payloadText(payload.outputPreview) ??
        payloadText(payload.output_preview) ??
        payloadText(payload.error);
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
      const backgroundTaskAction =
        event.event_type.startsWith("background_") && event.task_id
          ? ({
              type: "task" as const,
              taskId: event.task_id,
              label: taskLabel,
            })
          : undefined;
      const action =
        event.event_type === "schedule_plan_created"
          ? ({ type: "plan" } as const)
          : event.event_type === "mode_requested"
            ? ({ type: "mode" } as const)
            : backgroundTaskAction
              ? backgroundTaskAction
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
                  : event.event_type === "preflight_stage_completed" ||
                      event.event_type === "preflight_stage_failed"
                    ? ({ type: "trace" as const, eventId: event.id })
                    : event.event_type === "phase_changed"
                      ? ({ type: "mode" as const })
                    : undefined;

      const toolDisplayName = toolName ?? "工具";
      const label =
        event.event_type === "schedule_plan_created"
          ? "调度计划已生成"
          : event.event_type === "background_agent_started"
            ? `${agentLabel} 后台任务已启动`
            : event.event_type === "background_agent_completed"
              ? `${agentLabel} 后台任务已完成`
              : event.event_type === "background_agent_failed"
                ? `${agentLabel} 后台任务失败`
                : event.event_type === "background_agent_cancelled"
                  ? `${agentLabel} 后台任务已取消`
                  : event.event_type === "background_tool_call_started"
                    ? `${agentLabel} 调用 ${toolDisplayName} 开始`
                    : event.event_type === "background_tool_call_completed"
                      ? `${agentLabel} 调用 ${toolDisplayName} 完成`
                      : event.event_type === "background_tool_call_failed"
                        ? `${agentLabel} 调用 ${toolDisplayName} 失败`
                        : event.event_type === "background_tool_call_blocked"
                          ? `${agentLabel} 调用 ${toolDisplayName} 被拦截`
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
                                          ? `${event.mode ?? "编排"} 切换到 ${orchestrationPhaseLabel(
                                              event.phase ?? "",
                                            )}`
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
                                                      : event.event_type ===
                                                            "preflight_stage_completed" && stage
                                                        ? `${preflightStageLabel(stage)}完成`
                                                        : event.event_type ===
                                                              "preflight_stage_failed" && stage
                                                          ? `${preflightStageLabel(stage)}失败`
                                                          : event.event_type;

      const backgroundToolDetail = [
        description,
        toolOutputPreview,
      ]
        .filter((part): part is string => Boolean(part))
        .join(" · ");
      const detail =
        event.event_type === "preflight_stage_completed" ||
        event.event_type === "preflight_stage_failed"
          ? [durationMs, cacheStatus, toolCount, stageError]
              .filter((part): part is string => Boolean(part))
              .join(" · ")
          : event.event_type.startsWith("background_tool_call_")
            ? backgroundToolDetail || toolName || undefined
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
            : event.event_type === "background_tool_call_blocked"
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

export function filterOrchestrationTraceEvents(
  events: OrchestrationEventDto[],
  traceModeFilter: string,
  traceEventTypeFilter: string,
): OrchestrationEventDto[] {
  return events.filter((event) => {
    const modeOk = traceModeFilter === "all" || (event.mode ?? "unknown") === traceModeFilter;
    const typeOk = traceEventTypeFilter === "all" || event.event_type === traceEventTypeFilter;
    return modeOk && typeOk;
  });
}
