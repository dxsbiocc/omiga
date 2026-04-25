import { describe, expect, it } from "vitest";

import { settleRunningToolCalls } from "./toolStatusUtils";

describe("settleRunningToolCalls", () => {
  it("marks lingering running tool rows as completed", () => {
    const settled = settleRunningToolCalls(
      [
        {
          role: "tool",
          content: "`web_search`",
          toolCall: { name: "web_search", status: "running" as const },
        },
        {
          role: "tool",
          content: "`web_fetch` completed",
          toolCall: {
            name: "web_fetch",
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
      content: "`web_search` completed",
      toolCall: {
        name: "web_search",
        status: "completed",
        completedAt: 99,
      },
    });
    expect(settled[1]).toMatchObject({
      content: "`web_fetch` completed",
      toolCall: {
        name: "web_fetch",
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
