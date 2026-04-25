import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { OrchestrationTimelineList } from "./OrchestrationTimelineList";
import {
  buildOrchestrationTimelineFromEvents,
  type OrchestrationEventDto,
} from "./orchestrationProjection";

const baseAt = Date.parse("2026-04-25T00:00:00.000Z");
const at = (seconds: number) => `2026-04-25T00:00:${String(seconds).padStart(2, "0")}.000Z`;

function event(partial: Partial<OrchestrationEventDto> & Pick<OrchestrationEventDto, "id" | "event_type">): OrchestrationEventDto {
  return {
    session_id: "session-1",
    payload: {},
    created_at: at(0),
    ...partial,
  };
}

describe("OrchestrationTimelineList", () => {
  it("server-renders clickable schedule timeline evidence", () => {
    const events = buildOrchestrationTimelineFromEvents(
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
          payload: { agentType: "executor", description: "Implement token refresh" },
          created_at: at(2),
        }),
      ],
      [],
    );

    const html = renderToStaticMarkup(
      <OrchestrationTimelineList events={events} onEventClick={() => undefined} now={baseAt + 3000} />,
    );

    expect(html).toContain("编排事件时间线");
    expect(html).toContain("调度计划已生成");
    expect(html).toContain("分析执行 已完成");
    expect(html).toContain("Implement token refresh");
    expect(html).toContain("打开关联证据");
  });

  it("server-renders team phase timeline labels", () => {
    const events = buildOrchestrationTimelineFromEvents(
      [
        event({ id: "team-verify", event_type: "verification_started", mode: "team", phase: "verifying", created_at: at(1) }),
        event({ id: "team-fix", event_type: "fix_started", mode: "team", phase: "fixing", created_at: at(2) }),
        event({ id: "team-synth", event_type: "synthesizing_started", mode: "team", phase: "synthesizing", created_at: at(3) }),
      ],
      [],
    );

    const html = renderToStaticMarkup(<OrchestrationTimelineList events={events} now={baseAt + 4000} />);

    expect(html).toContain("验证阶段开始");
    expect(html).toContain("修复阶段开始");
    expect(html).toContain("综合阶段开始");
    expect(html).not.toContain("打开关联证据");
  });

  it("server-renders empty timeline fallback", () => {
    const html = renderToStaticMarkup(<OrchestrationTimelineList events={[]} />);

    expect(html).toContain("当前会话暂无时间线事件");
  });
});
