export type BrowserOperatorInstallIntent = "packages-only" | "full";

export interface BrowserOperatorBackendStatus {
  sidecarExists?: boolean;
  installerExists?: boolean;
  managedHome?: string;
  managedPythonExists?: boolean;
  managedBrowserUseExists?: boolean;
  configuredPython?: string | null;
  selectedPython?: string;
  playwrightBrowsersPath?: string;
  installCommand?: string;
}

export interface BrowserOperatorInstallResult {
  ok?: boolean;
  home?: string;
  python?: string;
  playwrightBrowsersPath?: string;
  error?: string;
}

export function isBrowserOperatorBackendReady(
  status: BrowserOperatorBackendStatus | null,
): boolean {
  return Boolean(
    status?.configuredPython ||
      (status?.managedPythonExists && status?.managedBrowserUseExists),
  );
}

export function browserOperatorErrorMessage(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (typeof error === "string") return error;
  try {
    return JSON.stringify(error) || "未知错误";
  } catch (_jsonError) {
    return "未知错误";
  }
}
