/**
 * Extract a human-readable string from a Tauri invoke error.
 *
 * Tauri commands returning `Result<T, AppError>` serialise the error as a
 * tagged JSON object, e.g.:
 *   { type: "Fs", details: { kind: "IoError", message: "..." } }
 *   { type: "Config", details: "some string" }
 *
 * Plain string errors (e.g. from commands that return `Result<T, String>`)
 * arrive as a bare string and are returned as-is.
 */
export function extractErrorMessage(err: unknown): string {
  if (typeof err === "string") return err;
  if (err == null) return "Unknown error";

  // Tauri AppError: { type: string, details: string | { kind: string, message?: string, ... } }
  const e = err as Record<string, unknown>;
  const details = e.details;

  if (typeof details === "string") return details;

  if (details != null && typeof details === "object") {
    const d = details as Record<string, unknown>;
    if (typeof d.message === "string") return d.message;
    // Some variants carry other named fields (e.g. { kind: "NotFound", path: "..." })
    const kind = typeof d.kind === "string" ? d.kind : "";
    const path = typeof d.path === "string" ? ` (${d.path})` : "";
    const extra =
      typeof d.resource === "string"
        ? ` (${d.resource})`
        : typeof d.size === "number"
          ? ` (${d.size} bytes)`
          : "";
    return `${kind}${path}${extra}` || JSON.stringify(details);
  }

  // Fallback: e.message (standard JS Error) or full JSON
  if (typeof e.message === "string") return e.message;
  try {
    return JSON.stringify(err);
  } catch {
    return String(err);
  }
}
