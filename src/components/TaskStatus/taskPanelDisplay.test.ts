import { describe, expect, it } from "vitest";
import {
  filterTaskOrchestrationEvents,
  isTaskOrchestrationEvent,
} from "./taskPanelDisplay";
import type { OrchestrationEventDto } from "./orchestrationProjection";

function event(
  partial: Pick<OrchestrationEventDto, "id" | "event_type"> &
    Partial<OrchestrationEventDto>,
): OrchestrationEventDto {
  return {
    session_id: "session-1",
    payload: {},
    created_at: "2026-04-29T00:00:00.000Z",
    ...partial,
  };
}

describe("taskPanelDisplay", () => {
  it("keeps raw timeline/trace surfaces limited to task orchestration modes", () => {
    const events = [
      event({ id: "team-phase", event_type: "phase_changed", mode: "team" }),
      event({
        id: "autopilot-review",
        event_type: "reviewer_verdict",
        mode: "autopilot",
      }),
      event({
        id: "schedule-plan",
        event_type: "schedule_plan_created",
        mode: "schedule",
      }),
      event({
        id: "research-done",
        event_type: "research_command_completed",
        mode: "research",
      }),
      event({
        id: "plain-review",
        event_type: "reviewer_verdict",
        mode: null,
      }),
    ];

    expect(filterTaskOrchestrationEvents(events).map((item) => item.id)).toEqual([
      "team-phase",
      "autopilot-review",
      "schedule-plan",
    ]);
  });

  it("recognizes legacy schedule plan events even when mode is missing", () => {
    expect(
      isTaskOrchestrationEvent(
        event({
          id: "legacy-schedule",
          event_type: "schedule_plan_created",
          mode: null,
        }),
      ),
    ).toBe(true);
  });
});
