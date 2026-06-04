import { beforeEach, describe, expect, it, vi } from "vitest";

const invokeMock = vi.hoisted(() => vi.fn());

vi.mock("@tauri-apps/api/core", () => ({
  invoke: invokeMock,
}));

import {
  executionWorkspaceScopeKey,
  shouldResetWorkspaceForExecutionScopeChange,
  useChatComposerStore,
} from "./chatComposerStore";

describe("chatComposerStore permissionMode", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    invokeMock.mockResolvedValue(undefined);
    useChatComposerStore.getState().resetToDefaults();
  });

  it("syncs permission mode changes to the active backend session immediately", () => {
    useChatComposerStore.getState().initForSession("session-stance", {
      composer_agent_type: "auto",
      permission_mode: "ask",
      execution_environment: "local",
      sandbox_backend: "docker",
      local_venv_type: "none",
      local_venv_name: "",
      use_worktree: false,
    });
    invokeMock.mockClear();

    useChatComposerStore.getState().setPermissionMode("auto");

    expect(invokeMock).toHaveBeenCalledWith("save_session_config_command", {
      sessionId: "session-stance",
      config: expect.objectContaining({ permission_mode: "auto" }),
    });
    expect(invokeMock).toHaveBeenCalledWith("permission_set_session_stance", {
      sessionId: "session-stance",
      stance: "auto",
    });
  });

  it("does not sync permission stance before a session is active", () => {
    invokeMock.mockClear();

    useChatComposerStore.getState().setPermissionMode("auto");

    expect(invokeMock).not.toHaveBeenCalledWith(
      "permission_set_session_stance",
      expect.anything(),
    );
  });
});

describe("chatComposerStore execution workspace scopes", () => {
  it("keeps local virtual environment changes in the same workspace scope", () => {
    const localScope = executionWorkspaceScopeKey("local", null, "docker");

    expect(
      shouldResetWorkspaceForExecutionScopeChange(localScope, localScope),
    ).toBe(false);
  });

  it("resets workspace when switching between local and SSH scopes", () => {
    const localScope = executionWorkspaceScopeKey("local", null, "docker");
    const sshScope = executionWorkspaceScopeKey("ssh", "lab-a", "docker");

    expect(
      shouldResetWorkspaceForExecutionScopeChange(localScope, sshScope),
    ).toBe(true);
    expect(
      shouldResetWorkspaceForExecutionScopeChange(sshScope, localScope),
    ).toBe(true);
  });

  it("resets workspace when changing SSH server or container backend", () => {
    expect(
      shouldResetWorkspaceForExecutionScopeChange(
        executionWorkspaceScopeKey("ssh", "lab-a", "docker"),
        executionWorkspaceScopeKey("ssh", "lab-a", "docker"),
      ),
    ).toBe(false);
    expect(
      shouldResetWorkspaceForExecutionScopeChange(
        executionWorkspaceScopeKey("ssh", "lab-a", "docker"),
        executionWorkspaceScopeKey("ssh", "lab-b", "docker"),
      ),
    ).toBe(true);
    expect(
      shouldResetWorkspaceForExecutionScopeChange(
        executionWorkspaceScopeKey("sandbox", null, "docker"),
        executionWorkspaceScopeKey("sandbox", null, "singularity"),
      ),
    ).toBe(true);
  });
});

describe("chatComposerStore attachments", () => {
  beforeEach(() => {
    useChatComposerStore.getState().resetToDefaults();
  });

  it("preserves absolute uploaded paths for model-facing attachment context", () => {
    const path =
      "/cluster/facility/yzhang/WorkSpace/code/EukDetect/附件1.xlsx";

    useChatComposerStore.getState().addComposerAttachedPath(path);

    expect(useChatComposerStore.getState().composerAttachedPaths).toEqual([
      path,
    ]);
  });
});
