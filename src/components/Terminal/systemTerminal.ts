/** Normalize a session workspace path for the embedded terminal. */
export function normalizeTerminalWorkspacePath(
  path: string | null | undefined,
): string | null {
  const trimmed = path?.trim() ?? "";
  if (!trimmed || trimmed === ".") return null;
  return trimmed;
}

/** Compact display name for the embedded terminal workspace label. */
export function terminalWorkspaceDisplayName(
  path: string | null | undefined,
): string {
  const normalized = normalizeTerminalWorkspacePath(path);
  if (!normalized) return "未选择工作区";

  const parts = normalized.split(/[/\\]/u).filter(Boolean);
  const base = parts[parts.length - 1] ?? normalized;
  if (!base) return normalized;
  return `${base} · ${normalized}`;
}
