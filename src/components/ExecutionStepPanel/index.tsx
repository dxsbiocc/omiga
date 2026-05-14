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

/** Compact per-step duration label for dense task rows. */
export function formatDurationMsCompact(durationMs: number | null | undefined): string | null {
  if (durationMs == null || !Number.isFinite(durationMs) || durationMs < 0) {
    return null;
  }
  if (durationMs < 1000) return "<1s";

  const totalSec = Math.floor(durationMs / 1000);
  if (totalSec < 60) return `${totalSec}s`;

  const totalMin = Math.floor(totalSec / 60);
  const sec = totalSec % 60;
  if (totalMin < 60) {
    return sec === 0 ? `${totalMin}m` : `${totalMin}m ${String(sec).padStart(2, "0")}s`;
  }

  const hours = Math.floor(totalMin / 60);
  const min = totalMin % 60;
  return min === 0 ? `${hours}h` : `${hours}h ${String(min).padStart(2, "0")}m`;
}

/** Per-row elapsed label; running rows update when the caller re-renders. */
export function formatStepElapsedLabel(
  startedAt: number | null | undefined,
  completedAt: number | null | undefined,
  fallbackNow = Date.now(),
): string | null {
  if (startedAt == null) return null;
  return formatDurationMsCompact((completedAt ?? fallbackNow) - startedAt);
}
