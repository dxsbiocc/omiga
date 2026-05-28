import { beforeEach, describe, expect, it, vi } from "vitest";

const invokeMock = vi.hoisted(() => vi.fn());

vi.mock("@tauri-apps/api/core", () => ({
  invoke: invokeMock,
}));

import { useChatComposerStore } from "./chatComposerStore";

describe("chatComposerStore browserUseMode", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    useChatComposerStore.getState().resetToDefaults();
  });

  it("defaults browserUseMode to off", () => {
    expect(useChatComposerStore.getState().browserUseMode).toBe("off");
  });

  it("resets task-scoped browserUseMode after send-style cleanup", () => {
    const store = useChatComposerStore.getState();

    store.setBrowserUseMode("task");
    store.resetTaskBrowserUseMode();

    expect(useChatComposerStore.getState().browserUseMode).toBe("off");
  });

  it("keeps session-scoped browserUseMode enabled across task reset", () => {
    const store = useChatComposerStore.getState();

    store.setBrowserUseMode("session");
    store.resetTaskBrowserUseMode();

    expect(useChatComposerStore.getState().browserUseMode).toBe("session");
  });

  it("clears browserUseMode when switching sessions", () => {
    useChatComposerStore.getState().setBrowserUseMode("session");

    useChatComposerStore.getState().initForSession("session-2", {
      composer_agent_type: "auto",
      permission_mode: "auto",
      execution_environment: "local",
      sandbox_backend: "docker",
      local_venv_type: "none",
      local_venv_name: "",
      use_worktree: false,
    });

    expect(useChatComposerStore.getState().browserUseMode).toBe("off");
  });
});
