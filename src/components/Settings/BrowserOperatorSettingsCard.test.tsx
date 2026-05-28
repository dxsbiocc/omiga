import type { ReactElement, ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type {
  HookRuntimeApi,
  RenderedNode,
} from "../../test/__tests__/componentHarness";
import {
  ComponentHarness,
  createHookRuntime,
  findAllNodes,
  installComponentTestWindow,
  textContent,
} from "../../test/__tests__/componentHarness";

const invokeMock = vi.hoisted(() => vi.fn());
const hookRuntimeRef = vi.hoisted(() => ({
  current: null as HookRuntimeApi | null,
}));

vi.mock("react", async (importOriginal) => {
  const actual = await importOriginal<typeof import("react")>();
  return {
    ...actual,
    useCallback: <T extends (...args: never[]) => unknown>(callback: T) => callback,
    useEffect: (effect: () => void | (() => void), deps?: readonly unknown[]) =>
      hookRuntimeRef.current?.useEffect(effect, deps),
    useMemo: <T,>(factory: () => T, deps?: readonly unknown[]) =>
      hookRuntimeRef.current?.useMemo(factory, deps),
    useState: <T,>(initial: T | (() => T)) =>
      hookRuntimeRef.current?.useState(initial),
  };
});

vi.mock("@tauri-apps/api/core", () => ({
  invoke: invokeMock,
}));

vi.mock("../../state/sessionStore", () => ({
  useSessionStore: <T,>(selector: (state: {
    currentSession: { id: string; projectPath: string } | null;
  }) => T): T =>
    selector({
      currentSession: {
        id: "session-browser",
        projectPath: "/project",
      },
    }),
}));

vi.mock("@mui/material", async () => {
  const React = await import("react");
  const { createMuiMaterialMock } = await import("../../test/__tests__/muiMocks");
  const base = createMuiMaterialMock();
  const passthrough = (type: string) => {
    const Component = ({
      children,
      label,
      ...props
    }: Record<string, unknown> & { children?: ReactNode; label?: ReactNode }) =>
      React.createElement(type, props, children ?? label);
    Component.displayName = `Mock${type}`;
    return Component;
  };
  return {
    ...base,
    Alert: passthrough("alert"),
    Divider: passthrough("divider"),
  };
});

vi.mock("@mui/icons-material", async () => {
  const { createIconMock } = await import("../../test/__tests__/muiMocks");
  return createIconMock([
    "CheckCircle",
    "Error",
    "Refresh",
    "SettingsInputComponent",
  ]);
});

import { BrowserOperatorSettingsCard } from "./BrowserOperatorSettingsCard";

function createHarness() {
  const runtime = createHookRuntime();
  hookRuntimeRef.current = runtime;
  const harness = new ComponentHarness(
    runtime,
    () => <BrowserOperatorSettingsCard /> as ReactElement,
  );
  harness.render();
  return harness;
}

function getButtonByText(harness: ComponentHarness, label: string): RenderedNode {
  const button = findAllNodes(
    harness.tree,
    (node) => node.type === "button" && textContent(node).includes(label),
  )[0];
  if (!button) throw new Error(`button not found: ${label}`);
  return button;
}

describe("BrowserOperatorSettingsCard", () => {
  beforeEach(() => {
    installComponentTestWindow();
    hookRuntimeRef.current = null;
    invokeMock.mockReset();
    vi.stubGlobal("confirm", vi.fn(() => true));
  });

  afterEach(() => {
    hookRuntimeRef.current?.cleanup();
    hookRuntimeRef.current = null;
    vi.unstubAllGlobals();
  });

  it("loads backend status and exposes explicit install actions", async () => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "browser_operator_backend_status") {
        return {
          sidecarExists: true,
          installerExists: true,
          managedHome: "/home/.omiga/browser-operator",
          managedPythonExists: false,
          managedBrowserUseExists: false,
          selectedPython: "python3",
          playwrightBrowsersPath: "/home/.omiga/browser-operator/ms-playwright",
          installCommand: "python3 install_browser_operator.py --json",
        };
      }
      if (command === "browser_operator_install_backend") {
        return {
          ok: true,
          home: "/home/.omiga/browser-operator",
          python: "/home/.omiga/browser-operator/.venv/bin/python",
        };
      }
      throw new Error(`unexpected command ${command}`);
    });

    const harness = createHarness();
    await Promise.resolve();
    harness.flush();

    expect(textContent(harness.tree)).toContain("Browser Operator");
    expect(textContent(harness.tree)).toContain("需要安装");
    expect(textContent(harness.tree)).toContain("只安装后端");

    await harness.click(getButtonByText(harness, "只安装后端"));
    await Promise.resolve();
    harness.flush();

    expect(globalThis.confirm).toHaveBeenCalled();
    expect(invokeMock).toHaveBeenCalledWith("browser_operator_install_backend", {
      confirmInstallIntent: true,
      skipBrowserInstall: true,
      projectRoot: "/project",
      sessionId: "session-browser",
    });
  });
});
