export type ProgressiveRenderPhase = "phase-1" | "phase-2";

export interface ProgressiveRenderPerfEvent {
  phase: ProgressiveRenderPhase;
  sessionId?: string | null;
  totalItems: number;
  renderedItems: number;
  instantRenderCount: number;
  durationMs?: number | null;
  addedHeightPx?: number | null;
}

export function shouldLogProgressiveRenderPerf(
  totalItems: number,
  instantRenderCount: number,
): boolean {
  return totalItems > instantRenderCount;
}

export function progressiveOlderItemCount(
  event: Pick<ProgressiveRenderPerfEvent, "totalItems" | "renderedItems">,
): number {
  return Math.max(0, event.totalItems - event.renderedItems);
}

export function formatProgressiveRenderPerf(
  event: ProgressiveRenderPerfEvent,
): string {
  const session = event.sessionId?.trim()
    ? `${event.sessionId.trim().slice(0, 8)} | `
    : "";
  const hiddenOrMounted =
    event.phase === "phase-1"
      ? `deferred ${progressiveOlderItemCount(event)}`
      : `restored ${Math.max(0, event.totalItems - event.instantRenderCount)}`;
  const duration =
    event.durationMs == null ? "duration ?ms" : `duration ${Math.round(event.durationMs)}ms`;
  const addedHeight =
    event.addedHeightPx == null
      ? ""
      : ` | added-height ${Math.round(event.addedHeightPx)}px`;

  return `[RenderPerf] ${session}${event.phase} | rendered ${event.renderedItems}/${event.totalItems} items | ${hiddenOrMounted} | ${duration}${addedHeight}`;
}
