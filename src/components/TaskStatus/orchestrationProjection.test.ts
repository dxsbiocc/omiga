import { describe, expect, it } from "vitest";
import {
  buildOrchestrationTimelineFromEvents,
  filterOrchestrationTraceEvents,
  type OrchestrationEventDto,
} from "./orchestrationProjection";
import type { BackgroundAgentTaskRow } from "../../utils/reviewerVerdict";

const at = (seconds: number) => `2026-04-25T00:00:${String(seconds).padStart(2, "0")}.000Z`;

function event(partial: Partial<OrchestrationEventDto> & Pick<OrchestrationEventDto, "id" | "event_type">): OrchestrationEventDto {
  return {
    session_id: "session-1",
    payload: {},
    created_at: at(0),
    ...partial,
  };
}

describe("orchestrationProjection", () => {
  it("projects /schedule plan and worker events into clickable timeline rows", () => {
    const tasks: BackgroundAgentTaskRow[] = [
      {
        task_id: "implement-refresh",
        agent_type: "executor",
        description: "Implement token refresh",
        status: "Completed",
        result_summary: "Implementation complete",
      },
    ];
    const timeline = buildOrchestrationTimelineFromEvents(
      [
        event({
          id: "plan-created",
          event_type: "schedule_plan_created",
          mode: "schedule",
          payload: { taskCount: 4 },
          created_at: at(1),
        }),
        event({
          id: "worker-done",
          event_type: "worker_completed",
          mode: "schedule",
          task_id: "implement-refresh",
          payload: { agentType: "executor" },
          created_at: at(2),
        }),
      ],
      tasks,
    );

    expect(timeline.map((item) => item.id)).toEqual(["worker-done", "plan-created"]);
    expect(timeline.find((item) => item.id === "plan-created")).toMatchObject({
      label: "调度计划已生成",
      detail: "4 个子任务",
      tone: "info",
      action: { type: "plan" },
    });
    expect(timeline.find((item) => item.id === "worker-done")).toMatchObject({
      label: "分析执行 已完成",
      detail: "Implement token refresh",
      tone: "success",
      action: {
        type: "task",
        taskId: "implement-refresh",
        label: "分析执行: Implement token refresh",
      },
    });
  });

  it("keeps /team verification, fixing, and synthesis events visible and filterable", () => {
    const events = [
      event({ id: "team-start", event_type: "mode_requested", mode: "team", created_at: at(1) }),
      event({ id: "team-exec", event_type: "phase_changed", mode: "team", phase: "executing", created_at: at(2) }),
      event({ id: "team-verify", event_type: "verification_started", mode: "team", phase: "verifying", created_at: at(3) }),
      event({ id: "team-fix", event_type: "fix_started", mode: "team", phase: "fixing", created_at: at(4) }),
      event({ id: "team-synth", event_type: "synthesizing_started", mode: "team", phase: "synthesizing", created_at: at(5) }),
      event({ id: "other", event_type: "phase_changed", mode: "autopilot", phase: "qa", created_at: at(6) }),
    ];

    const teamEvents = filterOrchestrationTraceEvents(events, "team", "all");
    expect(teamEvents.map((item) => item.id)).toEqual([
      "team-start",
      "team-exec",
      "team-verify",
      "team-fix",
      "team-synth",
    ]);
    expect(filterOrchestrationTraceEvents(events, "team", "fix_started")).toHaveLength(1);

    const labels = buildOrchestrationTimelineFromEvents(teamEvents, []).map((item) => item.label);
    expect(labels).toEqual([
      "综合阶段开始",
      "修复阶段开始",
      "验证阶段开始",
      "team 切换到 执行中",
      "team 模式已触发",
    ]);
  });

  it("projects /autopilot validation and reviewer blocker verdicts", () => {
    const timeline = buildOrchestrationTimelineFromEvents(
      [
        event({
          id: "autopilot-validation",
          event_type: "phase_changed",
          mode: "autopilot",
          phase: "validation",
          created_at: at(1),
        }),
        event({
          id: "critic-verdict",
          event_type: "reviewer_verdict",
          mode: "autopilot",
          task_id: "autopilot-review-critic",
          payload: {
            agentType: "critic",
            verdict: "reject",
            summary: "Missing regression evidence",
          },
          created_at: at(2),
        }),
      ],
      [],
    );

    expect(timeline.find((item) => item.id === "autopilot-validation")).toMatchObject({
      label: "autopilot 切换到 审查中",
      detail: "审查中",
      action: { type: "mode" },
    });
    expect(timeline.find((item) => item.id === "critic-verdict")).toMatchObject({
      label: "论证审查 给出 reject",
      detail: "Missing regression evidence",
      tone: "error",
      action: {
        type: "reviewer",
        taskId: "autopilot-review-critic",
        label: "论证审查: Missing regression evidence",
      },
    });
  });

  it("projects background task tool invocations as clickable trace rows", () => {
    const tasks: BackgroundAgentTaskRow[] = [
      {
        task_id: "bg-1",
        agent_type: "debugger",
        description: "Diagnose failing export",
        status: "Running",
      },
    ];
    const timeline = buildOrchestrationTimelineFromEvents(
      [
        event({
          id: "bg-start",
          event_type: "background_tool_call_started",
          mode: "background",
          task_id: "bg-1",
          payload: {
            agentType: "debugger",
            description: "Diagnose failing export",
            toolName: "rg",
            inputPreview: "export failure",
          },
          created_at: at(1),
        }),
        event({
          id: "bg-done",
          event_type: "background_tool_call_completed",
          mode: "background",
          task_id: "bg-1",
          payload: {
            agentType: "debugger",
            description: "Diagnose failing export",
            toolName: "rg",
            outputPreview: "src/export.ts:42",
          },
          created_at: at(2),
        }),
      ],
      tasks,
    );

    expect(timeline.map((item) => item.id)).toEqual(["bg-done", "bg-start"]);
    expect(timeline.find((item) => item.id === "bg-start")).toMatchObject({
      label: "问题排查 调用 rg 开始",
      detail: "Diagnose failing export",
      tone: "info",
      action: {
        type: "task",
        taskId: "bg-1",
        label: "问题排查: Diagnose failing export",
      },
    });
    expect(timeline.find((item) => item.id === "bg-done")).toMatchObject({
      label: "问题排查 调用 rg 完成",
      detail: "Diagnose failing export · src/export.ts:42",
      tone: "success",
      action: {
        type: "task",
        taskId: "bg-1",
        label: "问题排查: Diagnose failing export",
      },
    });
  });

  it("keeps lightweight preflight events as trace-only actions", () => {
    const timeline = buildOrchestrationTimelineFromEvents(
      [
        event({
          id: "compact-done",
          event_type: "preflight_stage_completed",
          mode: "preflight",
          phase: "preflight",
          payload: {
            stage: "auto_compact",
            durationMs: 0,
            payload: { compacted: true },
          },
          created_at: at(1),
        }),
      ],
      [],
    );

    expect(timeline[0]).toMatchObject({
      id: "compact-done",
      label: "上下文压缩完成",
      detail: "0 ms",
      tone: "success",
      action: { type: "trace", eventId: "compact-done" },
    });
  });
});
