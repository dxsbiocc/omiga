/**
 * Elapsed time helpers for the task panel (used by TaskStatus).
 */

/** Re-render periodically (e.g. once per second) so the label updates while running. */
export function formatExecutionElapsed(
  startedAt: number | null,
  _tick?: number,
): string {
  void _tick;
  return formatExecutionElapsedFixed(startedAt, null);
}

/** When `endedAt` is set, duration is frozen (completed run). */
export function formatExecutionElapsedFixed(
  startedAt: number | null,
  endedAt: number | null,
  _tick?: number,
): string {
  void _tick;
  if (startedAt == null) return "00:00";
  const end = endedAt ?? Date.now();
  const sec = Math.floor((end - startedAt) / 1000);
  const m = Math.floor(sec / 60);
  const s = sec % 60;
  return `${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`;
}
