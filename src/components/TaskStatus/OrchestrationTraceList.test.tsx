import { isValidElement, type ReactNode } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { OrchestrationTraceList } from "./OrchestrationTraceList";
import {
  buildOrchestrationTimelineFromEvents,
  type OrchestrationEventDto,
  type TimelineEvent,
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

function textFromChildren(children: ReactNode): string {
  if (children == null || typeof children === "boolean") return "";
  if (typeof children === "string" || typeof children === "number") return String(children);
  if (Array.isArray(children)) return children.map(textFromChildren).join("");
  if (isValidElement<{ children?: ReactNode; label?: ReactNode; "aria-label"?: string }>(children)) {
    return [
      children.props["aria-label"] ?? "",
      textFromChildren(children.props.label),
      textFromChildren(children.props.children),
    ].join("");
  }
  return "";
}

function findOnClickByText(node: ReactNode, text: string): (() => void) | null {
  if (node == null || typeof node === "boolean") return null;
  if (Array.isArray(node)) {
    for (const child of node) {
      const found = findOnClickByText(child, text);
      if (found) return found;
    }
    return null;
  }
  if (!isValidElement<{ children?: ReactNode; onClick?: () => void }>(node)) return null;
  if (textFromChildren(node).includes(text) && node.props.onClick) {
    return node.props.onClick;
  }
  return findOnClickByText(node.props.children, text);
}

function traceListProps(overrides: Partial<Parameters<typeof OrchestrationTraceList>[0]> = {}) {
  const events = [
    event({
      id: "critic-verdict",
      event_type: "reviewer_verdict",
      mode: "autopilot",
      phase: "validation",
      task_id: "autopilot-review-critic",
      payload: {
        agentType: "critic",
        verdict: "reject",
        summary: "Missing regression evidence",
      },
      created_at: at(2),
    }),
  ];
  const timelineEvents = buildOrchestrationTimelineFromEvents(events, []);
  const props: Parameters<typeof OrchestrationTraceList>[0] = {
    scopedEvents: events,
    filteredEvents: events,
    timelineEvents,
    failureDiagnostics: [
      {
        id: "failure-critic",
        taskId: "autopilot-review-critic",
        traceEventId: "critic-verdict",
        agentLabel: "论证审查",
        title: "Critic rejected",
        detail: "Autopilot critic review",
        summary: "Missing regression evidence",
        at: baseAt + 2000,
        source: "reviewer",
      },
    ],
    traceModes: ["autopilot"],
    traceEventTypes: ["reviewer_verdict"],
    traceModeFilter: "all",
    traceEventTypeFilter: "all",
    expandedTraceEventId: "critic-verdict",
    copiedTraceEventId: null,
    onTraceModeFilterChange: () => undefined,
    onTraceEventTypeFilterChange: () => undefined,
    onToggleTraceEvent: () => undefined,
    onTimelineEvent: () => undefined,
    onOpenTaskRecord: () => undefined,
    onCopyTracePayload: () => undefined,
    onBackToFailures: () => undefined,
    now: baseAt + 3000,
    ...overrides,
  };
  return props;
}

function renderTraceList(overrides: Partial<Parameters<typeof OrchestrationTraceList>[0]> = {}) {
  return <OrchestrationTraceList {...traceListProps(overrides)} />;
}

describe("OrchestrationTraceList", () => {
  it("server-renders trace event evidence and related failure affordances", () => {
    const html = renderToStaticMarkup(renderTraceList());

    expect(html).toContain("Trace 原始事件");
    expect(html).toContain("autopilot");
    expect(html).toContain("reviewer_verdict");
    expect(html).toContain("关联失败");
    expect(html).toContain("论证审查 给出 reject");
    expect(html).toContain("Missing regression evidence");
    expect(html).toContain("任务");
    expect(html).toContain("记录");
    expect(html).toContain("复制 payload");
    expect(html).toContain("打开队友记录");
  });

  it("wires jump, record, copy, expand, and filter callbacks without a browser", () => {
    const actions: string[] = [];
    const opened: Array<{ taskId: string; label: string }> = [];
    const copied: string[] = [];
    const timeline: TimelineEvent[] = [];
    const element = OrchestrationTraceList(traceListProps({
      onTraceModeFilterChange: (filter) => actions.push(`mode:${filter}`),
      onTraceEventTypeFilterChange: (filter) => actions.push(`type:${filter}`),
      onToggleTraceEvent: (id) => actions.push(`toggle:${id}`),
      onTimelineEvent: (event) => timeline.push(event),
      onOpenTaskRecord: (taskId, label) => opened.push({ taskId, label }),
      onCopyTracePayload: (event) => copied.push(event.id),
      onBackToFailures: () => actions.push("back"),
    }));

    findOnClickByText(element, "autopilot")?.();
    findOnClickByText(element, "reviewer_verdict")?.();
    findOnClickByText(element, "任务")?.();
    findOnClickByText(element, "记录")?.();
    findOnClickByText(element, "复制 payload")?.();
    findOnClickByText(element, "展开 trace payload")?.();
    findOnClickByText(element, "回到失败诊断")?.();
    findOnClickByText(element, "打开队友记录")?.();

    expect(actions).toContain("mode:autopilot");
    expect(actions).toContain("type:reviewer_verdict");
    expect(actions).toContain("toggle:critic-verdict");
    expect(actions).toContain("back");
    expect(timeline[0]?.id).toBe("critic-verdict");
    expect(copied).toEqual(["critic-verdict"]);
    expect(opened).toEqual([
      {
        taskId: "autopilot-review-critic",
        label: "论证审查: Autopilot critic review",
      },
      {
        taskId: "autopilot-review-critic",
        label: "论证审查: Autopilot critic review",
      },
    ]);
  });

  it("server-renders empty filtered trace state", () => {
    const html = renderToStaticMarkup(renderTraceList({ filteredEvents: [] }));

    expect(html).toContain("当前筛选条件下暂无 trace 事件");
  });
});
