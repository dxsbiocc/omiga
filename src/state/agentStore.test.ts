import { beforeEach, describe, expect, it, vi } from "vitest";
import { useAgentStore, type BackgroundAgentTask } from "./agentStore";

const baseTask: BackgroundAgentTask = {
  taskId: "task-1",
  agentType: "executor",
  description: "Implement slice",
  status: "running",
  createdAt: 1_000,
  startedAt: 1_200,
  sessionId: "session-1",
  messageId: "message-1",
};

describe("agentStore", () => {
  beforeEach(() => {
    vi.restoreAllMocks();
    useAgentStore.setState({
      backgroundTasks: [],
      selectedTaskId: null,
      showTaskPanel: false,
      pendingConfirmation: null,
      scheduleCompleteSession: null,
    });
  });

  it("preserves backend completion timestamps when updating terminal status", () => {
    vi.spyOn(Date, "now").mockReturnValue(99_999);
    useAgentStore.getState().upsertTask(baseTask);

    useAgentStore.getState().updateTaskStatus("task-1", "completed", {
      completedAt: 2_500,
      resultSummary: "done",
    });

    expect(useAgentStore.getState().backgroundTasks[0]).toMatchObject({
      status: "completed",
      completedAt: 2_500,
      resultSummary: "done",
    });
  });

  it("preserves existing startedAt across repeated running updates", () => {
    vi.spyOn(Date, "now").mockReturnValue(77_777);
    useAgentStore.getState().upsertTask({
      ...baseTask,
      status: "pending",
      startedAt: undefined,
    });

    useAgentStore.getState().updateTaskStatus("task-1", "running");
    useAgentStore.getState().updateTaskStatus("task-1", "running", {
      resultSummary: "still running",
    });

    expect(useAgentStore.getState().backgroundTasks[0]).toMatchObject({
      status: "running",
      startedAt: 77_777,
      resultSummary: "still running",
    });
  });

  it("fills missing terminal timestamps from the local clock", () => {
    vi.spyOn(Date, "now").mockReturnValue(88_888);
    useAgentStore.getState().upsertTask(baseTask);

    useAgentStore.getState().updateTaskStatus("task-1", "failed", {
      errorMessage: "boom",
    });

    expect(useAgentStore.getState().backgroundTasks[0]).toMatchObject({
      status: "failed",
      completedAt: 88_888,
      errorMessage: "boom",
    });
  });

  it("does not rewrite completedAt on repeated terminal updates", () => {
    vi.spyOn(Date, "now")
      .mockReturnValueOnce(66_000)
      .mockReturnValueOnce(99_000);
    useAgentStore.getState().upsertTask(baseTask);

    useAgentStore.getState().updateTaskStatus("task-1", "completed", {
      resultSummary: "done",
    });
    useAgentStore.getState().updateTaskStatus("task-1", "completed", {
      outputPath: "/tmp/result.txt",
    });

    expect(useAgentStore.getState().backgroundTasks[0]).toMatchObject({
      status: "completed",
      completedAt: 66_000,
      resultSummary: "done",
      outputPath: "/tmp/result.txt",
    });
  });

  it("keeps the explicit transition status when extra fields contain stale status", () => {
    vi.spyOn(Date, "now").mockReturnValue(55_000);
    useAgentStore.getState().upsertTask(baseTask);

    useAgentStore.getState().updateTaskStatus("task-1", "completed", {
      status: "running",
      resultSummary: "done",
    });

    expect(useAgentStore.getState().backgroundTasks[0]).toMatchObject({
      status: "completed",
      completedAt: 55_000,
      resultSummary: "done",
    });
  });

  it("filters running and per-session tasks", () => {
    const store = useAgentStore.getState();
    store.upsertTask({
      ...baseTask,
      taskId: "pending-1",
      status: "pending",
    });
    store.upsertTask({
      ...baseTask,
      taskId: "running-1",
      status: "running",
    });
    store.upsertTask({
      ...baseTask,
      taskId: "done-1",
      status: "completed",
      sessionId: "session-2",
    });

    expect(store.getRunningTasks().map((task) => task.taskId)).toEqual([
      "pending-1",
      "running-1",
    ]);
    expect(store.getSessionTasks("session-1").map((task) => task.taskId)).toEqual([
      "pending-1",
      "running-1",
    ]);
  });

  it("cleans terminal tasks and clears selected removed tasks", () => {
    const store = useAgentStore.getState();
    store.upsertTask({
      ...baseTask,
      taskId: "running-1",
      status: "running",
    });
    store.upsertTask({
      ...baseTask,
      taskId: "failed-1",
      status: "failed",
    });
    store.upsertTask({
      ...baseTask,
      taskId: "done-1",
      status: "completed",
    });
    store.setSelectedTask("running-1");

    store.cleanupCompleted();
    expect(useAgentStore.getState().backgroundTasks.map((task) => task.taskId)).toEqual([
      "running-1",
    ]);

    store.removeTask("running-1");
    expect(useAgentStore.getState()).toMatchObject({
      backgroundTasks: [],
      selectedTaskId: null,
    });
  });
});
