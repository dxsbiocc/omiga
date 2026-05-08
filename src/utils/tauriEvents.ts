import { listen as tauriListen, type Event, type UnlistenFn } from "@tauri-apps/api/event";

function hasTauriEventBridge(): boolean {
  const g = globalThis as typeof globalThis & {
    __TAURI_INTERNALS__?: { transformCallback?: unknown };
    __TAURI__?: { transformCallback?: unknown };
  };
  return Boolean(
    g.__TAURI_INTERNALS__?.transformCallback ||
      g.__TAURI__?.transformCallback,
  );
}

export async function listenTauriEvent<T>(
  eventName: string,
  handler: (event: Event<T>) => void | Promise<void>,
): Promise<UnlistenFn> {
  if (!hasTauriEventBridge()) {
    return () => {};
  }
  try {
    return await tauriListen<T>(eventName, handler);
  } catch (error) {
    console.warn(`[Omiga] listenTauriEvent skipped for ${eventName}`, error);
    return () => {};
  }
}

export function canListenToTauriEvents(): boolean {
  return hasTauriEventBridge();
}
