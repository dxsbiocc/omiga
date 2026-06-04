import { beforeEach, describe, expect, it, vi } from "vitest";

const invokeMock = vi.hoisted(() => vi.fn());

vi.mock("@tauri-apps/api/core", () => ({
  invoke: invokeMock,
}));

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: vi.fn(),
}));

import { UNUSED_SESSION_LABEL, useSessionStore } from "./sessionStore";

const baseSession = {
  id: "session-1",
  name: "Session 1",
  projectPath: "/workspace/one",
  workingDirectory: "/workspace/one",
  createdAt: "2026-01-01T00:00:00.000Z",
  updatedAt: "2026-01-01T00:00:00.000Z",
};

describe("sessionStore sendMessage computerUseMode payload", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    invokeMock.mockResolvedValue({
      message_id: "message-1",
      session_id: "session-1",
      round_id: "round-1",
    });
    useSessionStore.setState({ activeRounds: new Map() });
  });

  it("passes computerUseMode through to the send_message payload", async () => {
    await useSessionStore.getState().sendMessage({
      content: "open example.com",
      use_tools: true,
      computerUseMode: "task",
    });

    expect(invokeMock).toHaveBeenCalledWith("send_message", {
      request: expect.objectContaining({
        content: "open example.com",
        use_tools: true,
        computerUseMode: "task",
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

describe("sessionStore projectized quick sessions", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    invokeMock.mockResolvedValue({
      id: "session-project-new",
      name: UNUSED_SESSION_LABEL,
      project_path: "/workspace/project-a",
      created_at: "2026-01-02T00:00:00.000Z",
      updated_at: "2026-01-02T00:00:00.000Z",
      session_config: undefined,
    });
    useSessionStore.setState({
      sessions: [],
      currentSession: null,
      messages: [],
      storeMessages: [],
      pendingProjectPathSessions: new Set(),
    });
  });

  it("creates a fresh placeholder session in the requested project folder", async () => {
    await useSessionStore.getState().createSessionQuick("/workspace/project-a");

    expect(invokeMock).toHaveBeenCalledWith("create_session", {
      name: UNUSED_SESSION_LABEL,
      projectPath: "/workspace/project-a",
    });
    expect(useSessionStore.getState().currentSession?.projectPath).toBe(
      "/workspace/project-a",
    );
  });

  it("does not reuse an empty placeholder from another project", async () => {
    useSessionStore.setState({
      sessions: [
        {
          ...baseSession,
          id: "other-placeholder",
          name: UNUSED_SESSION_LABEL,
          projectPath: "/workspace/other",
          workingDirectory: "/workspace/other",
          messageCount: 0,
        },
      ],
      currentSession: null,
      storeMessages: [],
    });

    await useSessionStore.getState().createSessionQuick("/workspace/project-a");

    expect(invokeMock).toHaveBeenCalledWith("create_session", {
      name: UNUSED_SESSION_LABEL,
      projectPath: "/workspace/project-a",
    });
    expect(useSessionStore.getState().currentSession?.id).toBe(
      "session-project-new",
    );
  });
});
