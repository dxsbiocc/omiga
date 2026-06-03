import { beforeEach, describe, expect, it, vi } from "vitest";

const invokeMock = vi.hoisted(() => vi.fn());

vi.mock("@tauri-apps/api/core", () => ({
  invoke: invokeMock,
}));

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: vi.fn(),
}));

import { useSessionStore } from "./sessionStore";

const baseSession = {
  id: "session-1",
  name: "Session 1",
  projectPath: "/workspace/one",
  workingDirectory: "/workspace/one",
  createdAt: "2026-01-01T00:00:00.000Z",
  updatedAt: "2026-01-01T00:00:00.000Z",
};

describe("sessionStore sendMessage browserUseMode payload", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    invokeMock.mockResolvedValue({
      message_id: "message-1",
      session_id: "session-1",
      round_id: "round-1",
    });
    useSessionStore.setState({ activeRounds: new Map() });
  });

  it("passes browserUseMode through to the send_message payload", async () => {
    await useSessionStore.getState().sendMessage({
      content: "open example.com",
      use_tools: true,
      browserUseMode: "task",
      computerUseMode: "off",
    });

    expect(invokeMock).toHaveBeenCalledWith("send_message", {
      request: expect.objectContaining({
        content: "open example.com",
        use_tools: true,
        browserUseMode: "task",
        computerUseMode: "off",
      }),
    });
  });
});

describe("sessionStore workspace reset", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    invokeMock.mockResolvedValue(undefined);
    useSessionStore.setState({
      sessions: [baseSession],
      currentSession: baseSession,
      pendingProjectPathSessions: new Set(),
    });
  });

  it("marks the session as needing workspace selection when project path is reset", async () => {
    await useSessionStore
      .getState()
      .updateSessionProjectPath("session-1", ".");

    expect(invokeMock).toHaveBeenCalledWith("update_session_project_path", {
      sessionId: "session-1",
      projectPath: ".",
    });
    expect(useSessionStore.getState().currentSession?.projectPath).toBe(".");
    expect(useSessionStore.getState().currentSession?.workingDirectory).toBe(
      ".",
    );
    expect(
      useSessionStore
        .getState()
        .pendingProjectPathSessions.has("session-1"),
    ).toBe(true);
  });

  it("clears the pending workspace flag when a real project path is selected", async () => {
    useSessionStore.setState({
      pendingProjectPathSessions: new Set(["session-1"]),
    });

    await useSessionStore
      .getState()
      .updateSessionProjectPath("session-1", "/workspace/two");

    expect(
      useSessionStore
        .getState()
        .pendingProjectPathSessions.has("session-1"),
    ).toBe(false);
    expect(useSessionStore.getState().currentSession?.projectPath).toBe(
      "/workspace/two",
    );
  });
});
