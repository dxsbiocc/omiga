/**
 * Per-session stream state registry.
 *
 * Lives outside React so stream listeners and their accumulated state survive
 * session switches.  When the user switches away from Session A (which is still
 * streaming), the listener is NOT unregistered — it keeps receiving events from
 * the backend.  The ownerSessionId guard inside the handler stores non-current
 * session events in this registry instead of mutating the visible React state.
 * When the user switches BACK to Session A, the saved snapshot restores the
 * latest accumulated stream state immediately.
 *
 * The snapshot map (save/load) handles the React state values that were cleared
 * on the previous switch-away, so the restored session shows its accumulated
 * response and task-panel state immediately.
 *
 * Reactivity for the sidebar:
 *   useStreamRegistryVersion() increments whenever any snapshot changes.
 *   SessionList subscribes to this to redraw running-session indicators without
 *   subscribing to the heavy activity store.
 */

import { create } from "zustand";
import type {
  ExecutionStep,
  ActiveTodoItem,
  BackgroundJob,
} from "./activityStore";
import {
  activitySnapshotHasRecords,
  buildSessionActivitySnapshot,
  finalizeActivitySnapshot,
  saveLatestActivitySnapshot,
} from "./sessionActivitySnapshots";

export interface SessionStreamSnapshot {
  streamId: string | null;
  roundId: string | null;
  response: string;
  foldIntermediate: string;
  pendingText: string;
  pendingFoldText: string;
  isStreaming: boolean;
  isConnecting: boolean;
  waitingFirstChunk: boolean;
  currentToolHint: string | null;
  executionSteps: ExecutionStep[];
  executionStartedAt: number | null;
  executionEndedAt: number | null;
  activeTodos: ActiveTodoItem[] | null;
  backgroundJobs: BackgroundJob[];
}

// ── Reactivity signal ────────────────────────────────────────────────────────
// A lightweight Zustand store whose sole purpose is to bump a version counter
// whenever the snapshot map changes.  Subscribers re-render exactly once per
// change without touching the heavy activityStore or sessionStore.

interface RegistryVersionState {
  version: number;
}

export const useStreamRegistryVersion = create<RegistryVersionState>(() => ({
  version: 0,
}));

function bumpVersion(): void {
  useStreamRegistryVersion.setState((s) => ({ version: s.version + 1 }));
}

// ── Module-level maps ────────────────────────────────────────────────────────
// Outside React — survive session switches and re-renders.
const _snapshots = new Map<string, SessionStreamSnapshot>();
const _listeners = new Map<string, () => void>();

// ── Snapshot API ─────────────────────────────────────────────────────────────

export function saveStreamSnapshot(
  sessionId: string,
  snap: SessionStreamSnapshot,
): void {
  _snapshots.set(sessionId, snap);
  const activitySnapshot = buildSessionActivitySnapshot(sessionId, snap);
  if (activitySnapshotHasRecords(activitySnapshot)) {
    saveLatestActivitySnapshot(sessionId, activitySnapshot);
  }
  bumpVersion();
}

export function loadStreamSnapshot(
  sessionId: string,
): SessionStreamSnapshot | null {
  return _snapshots.get(sessionId) ?? null;
}

/** True when a snapshot indicates the session is (or was recently) streaming. */
export function snapshotIsActive(snap: SessionStreamSnapshot | null): boolean {
  return !!(snap && (snap.isStreaming || snap.isConnecting));
}

/**
 * True when a background (non-current) session has an active stream.
 * For the CURRENT session, check activityStore instead.
 */
export function isBackgroundSessionRunning(sessionId: string): boolean {
  return snapshotIsActive(_snapshots.get(sessionId) ?? null);
}

/** Clear the snapshot for a session once the stream ends or the session is deleted. */
export function clearStreamSnapshot(sessionId: string): void {
  const snap = _snapshots.get(sessionId);
  if (snap) {
    const activitySnapshot = buildSessionActivitySnapshot(sessionId, snap);
    if (activitySnapshotHasRecords(activitySnapshot)) {
      saveLatestActivitySnapshot(
        sessionId,
        finalizeActivitySnapshot(activitySnapshot),
      );
    }
  }
  _snapshots.delete(sessionId);
  bumpVersion();
}

// ── Listener API ─────────────────────────────────────────────────────────────

/**
 * Register an unlisten function for a session's stream.
 * Automatically cancels any previous listener for the SAME session
 * (e.g., the user sent a second message before the first finished).
 * Does NOT cancel other sessions' listeners.
 */
export function registerStreamListener(
  sessionId: string,
  fn: () => void,
): void {
  const prev = _listeners.get(sessionId);
  if (prev) prev();
  _listeners.set(sessionId, fn);
}

/** Cancel and remove the listener for a specific session. */
export function cancelStreamListener(sessionId: string): void {
  const fn = _listeners.get(sessionId);
  if (fn) {
    fn();
    _listeners.delete(sessionId);
  }
}

/** Cancel ALL registered listeners — call on Chat component unmount. */
export function cancelAllStreamListeners(): void {
  for (const fn of _listeners.values()) fn();
  _listeners.clear();
}
