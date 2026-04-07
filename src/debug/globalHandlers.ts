/**
 * Surfaces uncaught errors and unhandled promise rejections in the console
 * with a stable prefix so filtering is easy (e.g. DevTools → filter "OmigaDebug").
 */
export function installGlobalDebugHandlers(): void {
  if (typeof window === "undefined") return;

  const tag = "[OmigaDebug]";

  window.addEventListener("error", (event) => {
    console.error(tag, "window.error", {
      message: event.message,
      filename: event.filename,
      lineno: event.lineno,
      colno: event.colno,
      error: event.error,
    });
  });

  window.addEventListener("unhandledrejection", (event) => {
    console.error(tag, "unhandledrejection", event.reason);
  });

  const w = window as unknown as { __OMIGA_DEBUG_INSTALLED?: boolean };
  if (!w.__OMIGA_DEBUG_INSTALLED) {
    w.__OMIGA_DEBUG_INSTALLED = true;
    console.info(tag, "Global error handlers installed");
  }
}
