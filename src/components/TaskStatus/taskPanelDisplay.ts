import type { OrchestrationEventDto } from "./orchestrationProjection";

export const TASK_ORCHESTRATION_MODES = new Set([
  "autopilot",
  "schedule",
  "team",
]);

export function isTaskOrchestrationMode(mode?: string | null): boolean {
  return TASK_ORCHESTRATION_MODES.has((mode ?? "").trim().toLowerCase());
}

export function isTaskOrchestrationEvent(
  event: Pick<OrchestrationEventDto, "mode" | "event_type">,
): boolean {
  return (
    isTaskOrchestrationMode(event.mode) ||
    event.event_type === "schedule_plan_created"
  );
}

export function filterTaskOrchestrationEvents(
  events: OrchestrationEventDto[],
): OrchestrationEventDto[] {
  return events.filter(isTaskOrchestrationEvent);
}
