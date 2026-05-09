import type {
  ActiveTodoItem,
  BackgroundJob,
  ExecutionStep,
} from "./activityStore";

const STORAGE_PREFIX = "omiga.sessionActivitySnapshot.v1.";
const MAX_STEPS = 80;
const MAX_BACKGROUND_JOBS = 100;
const MAX_TEXT_FIELD_CHARS = 24_000;

export interface ActivitySnapshotSource {
  roundId?: string | null;
  executionSteps: ExecutionStep[];
  executionStartedAt: number | null;
  executionEndedAt: number | null;
  activeTodos: ActiveTodoItem[] | null;
  backgroundJobs: BackgroundJob[];
}

export interface SessionActivitySnapshot extends ActivitySnapshotSource {
  sessionId: string;
  savedAt: number;
}

type HistoricalActivityState = {
  isConnecting: false;
  isStreaming: false;
  waitingFirstChunk: false;
  currentToolHint: null;
  executionSteps: ExecutionStep[];
  executionStartedAt: number | null;
  executionEndedAt: number | null;
  activeTodos: ActiveTodoItem[] | null;
  backgroundJobs: BackgroundJob[];
};

const _snapshots = new Map<string, SessionActivitySnapshot>();

function storageKey(sessionId: string): string {
  return `${STORAGE_PREFIX}${sessionId}`;
}

function storage(): Storage | null {
  try {
    if (typeof window === "undefined") return null;
    return window.localStorage ?? null;
  } catch {
    return null;
  }
}

function clipText(value: string | undefined): string | undefined {
  if (value === undefined || value.length <= MAX_TEXT_FIELD_CHARS) return value;
  return `${value.slice(0, MAX_TEXT_FIELD_CHARS)}\n…[已截断以保存任务区快照]`;
}

function sanitizeStep(step: ExecutionStep): ExecutionStep {
  return {
    ...step,
    summary: clipText(step.summary),
    input: clipText(step.input),
    toolOutput: clipText(step.toolOutput),
  };
}

function sanitizeSnapshot(snapshot: SessionActivitySnapshot): SessionActivitySnapshot {
  return {
    ...snapshot,
    executionSteps: snapshot.executionSteps.slice(-MAX_STEPS).map(sanitizeStep),
    backgroundJobs: snapshot.backgroundJobs.slice(-MAX_BACKGROUND_JOBS),
    activeTodos: snapshot.activeTodos ? [...snapshot.activeTodos] : null,
  };
}

function isObject(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object";
}

function normalizeSnapshot(value: unknown): SessionActivitySnapshot | null {
  if (!isObject(value)) return null;
  const sessionId = typeof value.sessionId === "string" ? value.sessionId : "";
  if (!sessionId) return null;
  const executionSteps = Array.isArray(value.executionSteps)
    ? (value.executionSteps as ExecutionStep[])
    : [];
  const backgroundJobs = Array.isArray(value.backgroundJobs)
    ? (value.backgroundJobs as BackgroundJob[])
    : [];
  const activeTodos =
    Array.isArray(value.activeTodos) || value.activeTodos === null
      ? (value.activeTodos as ActiveTodoItem[] | null)
      : null;
  return sanitizeSnapshot({
    sessionId,
    roundId: typeof value.roundId === "string" ? value.roundId : null,
    savedAt:
      typeof value.savedAt === "number" && Number.isFinite(value.savedAt)
        ? value.savedAt
        : Date.now(),
    executionSteps,
    executionStartedAt:
      typeof value.executionStartedAt === "number" &&
      Number.isFinite(value.executionStartedAt)
        ? value.executionStartedAt
        : null,
    executionEndedAt:
      typeof value.executionEndedAt === "number" &&
      Number.isFinite(value.executionEndedAt)
        ? value.executionEndedAt
        : null,
    activeTodos,
    backgroundJobs,
  });
}

export function activitySnapshotHasRecords(
  snapshot: Pick<
    SessionActivitySnapshot,
    "executionSteps" | "backgroundJobs" | "activeTodos"
  >,
): boolean {
  return (
    snapshot.executionSteps.length > 0 ||
    snapshot.backgroundJobs.length > 0 ||
    (snapshot.activeTodos?.length ?? 0) > 0
  );
}

export function buildSessionActivitySnapshot(
  sessionId: string,
  source: ActivitySnapshotSource,
  savedAt = Date.now(),
): SessionActivitySnapshot {
  return sanitizeSnapshot({
    sessionId,
    roundId: source.roundId ?? null,
    savedAt,
    executionSteps: [...source.executionSteps],
    executionStartedAt: source.executionStartedAt,
    executionEndedAt: source.executionEndedAt,
    activeTodos: source.activeTodos ? [...source.activeTodos] : null,
    backgroundJobs: [...source.backgroundJobs],
  });
}

export function finalizeActivitySnapshot(
  snapshot: SessionActivitySnapshot,
  endedAt = Date.now(),
): SessionActivitySnapshot {
  return sanitizeSnapshot({
    ...snapshot,
    savedAt: endedAt,
    executionEndedAt: snapshot.executionEndedAt ?? endedAt,
    executionSteps: snapshot.executionSteps.map((step) =>
      step.status === "running"
        ? {
            ...step,
            status: "done" as const,
            completedAt: step.completedAt ?? endedAt,
          }
        : step,
    ),
  });
}

export function historicalActivityStateFromSnapshot(
  snapshot: SessionActivitySnapshot,
): HistoricalActivityState {
  const frozen = finalizeActivitySnapshot(snapshot, snapshot.executionEndedAt ?? snapshot.savedAt);
  return {
    isConnecting: false,
    isStreaming: false,
    waitingFirstChunk: false,
    currentToolHint: null,
    executionSteps: frozen.executionSteps,
    executionStartedAt: frozen.executionStartedAt,
    executionEndedAt: frozen.executionEndedAt,
    activeTodos: frozen.activeTodos,
    backgroundJobs: frozen.backgroundJobs,
  };
}

export function saveLatestActivitySnapshot(
  sessionId: string,
  snapshot: SessionActivitySnapshot,
): void {
  if (!activitySnapshotHasRecords(snapshot)) return;
  const sanitized = sanitizeSnapshot({ ...snapshot, sessionId });
  _snapshots.set(sessionId, sanitized);
  const s = storage();
  if (!s) return;
  try {
    s.setItem(storageKey(sessionId), JSON.stringify(sanitized));
  } catch {
    // Best-effort UI state cache; chat/tool data remain persisted in SQLite.
  }
}

export function loadLatestActivitySnapshot(
  sessionId: string,
): SessionActivitySnapshot | null {
  const memory = _snapshots.get(sessionId);
  if (memory) return memory;

  const s = storage();
  if (!s) return null;
  try {
    const raw = s.getItem(storageKey(sessionId));
    if (!raw) return null;
    const parsed = normalizeSnapshot(JSON.parse(raw));
    if (!parsed) return null;
    _snapshots.set(sessionId, parsed);
    return parsed;
  } catch {
    return null;
  }
}

export function clearLatestActivitySnapshot(sessionId: string): void {
  _snapshots.delete(sessionId);
  const s = storage();
  if (!s) return;
  try {
    s.removeItem(storageKey(sessionId));
  } catch {
    // Ignore cache cleanup failures.
  }
}
