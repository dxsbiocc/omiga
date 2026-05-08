export function filterPersistentSessionsBySessionId<T extends { session_id: string }>(
  sessions: T[],
  sessionId?: string | null,
): T[] {
  if (!sessionId) return sessions;
  return sessions.filter((session) => session.session_id === sessionId);
}
