import { describe, expect, it } from "vitest";

import { settleRunningToolCalls } from "./toolStatusUtils";

describe("settleRunningToolCalls", () => {
  it("marks lingering running tool rows as completed", () => {
    const settled = settleRunningToolCalls(
      [
        {
          role: "tool",
          content: "`search`",
          toolCall: { name: "search", status: "running" as const },
        },
        {
          role: "tool",
          content: "`fetch` completed",
          toolCall: {
            name: "fetch",
            status: "completed" as const,
            completedAt: 12,
          },
        },
        {
          role: "assistant",
          content: "done",
        },
      ],
      "completed",
      99,
    );

    expect(settled[0]).toMatchObject({
      content: "`search` completed",
      toolCall: {
        name: "search",
        status: "completed",
        completedAt: 99,
      },
    });
    expect(settled[1]).toMatchObject({
      content: "`fetch` completed",
      toolCall: {
        name: "fetch",
        status: "completed",
        completedAt: 12,
      },
    });
    expect(settled[2]).toMatchObject({
      role: "assistant",
      content: "done",
    });
  });
});
