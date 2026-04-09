import { describe, expect, it } from "vitest";
import {
  canSendFollowUpToTask,
  shortBgTaskLabel,
  type BackgroundAgentTask,
} from "./backgroundAgentTypes";

function task(partial: Partial<BackgroundAgentTask>): BackgroundAgentTask {
  return {
    task_id: "t1",
    agent_type: "Agent",
    description: "desc",
    status: "Running",
    created_at: 0,
    session_id: "s1",
    message_id: "m1",
    ...partial,
  };
}

describe("backgroundAgentTypes", () => {
  it("canSendFollowUpToTask allows Pending and Running only", () => {
    expect(canSendFollowUpToTask("Pending")).toBe(true);
    expect(canSendFollowUpToTask("Running")).toBe(true);
    expect(canSendFollowUpToTask("Completed")).toBe(false);
    expect(canSendFollowUpToTask("Failed")).toBe(false);
    expect(canSendFollowUpToTask("Cancelled")).toBe(false);
  });

  it("shortBgTaskLabel truncates long description", () => {
    const t = task({
      description: "abcdefghijklmnopqrstuvwxyz0123456789",
    });
    expect(shortBgTaskLabel(t, 10)).toHaveLength(10);
    expect(shortBgTaskLabel(t, 10).endsWith("…")).toBe(true);
  });

  it("shortBgTaskLabel falls back to agent_type when description empty", () => {
    const t = task({ description: "   ", agent_type: "Wiki" });
    expect(shortBgTaskLabel(t, 20)).toBe("Wiki");
  });
});
