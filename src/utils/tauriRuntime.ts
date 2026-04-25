import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow, type Window } from "@tauri-apps/api/window";

type TauriGlobal = typeof globalThis & {
  __TAURI_INTERNALS__?: {
    transformCallback?: unknown;
    invoke?: unknown;
  };
  __TAURI__?: {
    transformCallback?: unknown;
    invoke?: unknown;
  };
};

export function hasTauriBridge(): boolean {
  const g = globalThis as TauriGlobal;
  return Boolean(
    g.__TAURI_INTERNALS__?.transformCallback ||
      g.__TAURI_INTERNALS__?.invoke ||
      g.__TAURI__?.transformCallback ||
      g.__TAURI__?.invoke,
  );
}

export async function invokeIfTauri<T>(
  command: string,
  args?: Record<string, unknown>,
): Promise<T | null> {
  if (!hasTauriBridge()) {
    return null;
  }
  try {
    return await invoke<T>(command, args);
  } catch {
    return null;
  }
}

export async function getCurrentWindowIfTauri(): Promise<Window | null> {
  if (!hasTauriBridge()) {
    return null;
  }
  try {
    return await getCurrentWindow();
  } catch {
    return null;
  }
}
