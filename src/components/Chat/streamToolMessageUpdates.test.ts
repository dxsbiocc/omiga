import { describe, expect, it } from "vitest";
import {
  applyToolResultMessage,
  normalizeAssistantToolCallPrefaces,
  type StreamToolMessageLike,
  upsertToolUseMessage,
} from "./streamToolMessageUpdates";

describe("stream tool message updates", () => {
  it("keeps pre-tool thought text on the tool row when the tool result arrives", () => {
    const initial: StreamToolMessageLike[] = [
      { id: "user-1", role: "user", content: "run it" },
    ];
    const withTool = upsertToolUseMessage(
      initial,
      {
        toolData: { id: "call-1", name: "bash", arguments: "" },
        newToolId: "tool-1",
        prefaceBeforeTools: "Let me check the logs first.",
        timestamp: 1000,
      },
    );

    const updated = applyToolResultMessage(withTool, {
      resultData: {
        tool_use_id: "call-1",
        name: "bash",
        input: JSON.stringify({ command: "tail -30 /tmp/app.log" }),
        output: "ok",
        is_error: false,
      },
      matchId: "call-1",
      completedAt: 1400,
    });

    expect(updated).toHaveLength(2);
    expect(updated[1].prefaceBeforeTools).toBe("Let me check the logs first.");
    expect(updated[1].toolCall?.status).toBe("completed");
    expect(updated[1].toolCall?.output).toBe("ok");
  });

  it("archives live thought text on tool_result when the tool row already exists", () => {
    const running: StreamToolMessageLike[] = [
      {
        id: "tool-1",
        role: "tool",
        content: "`search`",
        toolCall: { id: "call-1", name: "search", status: "running" },
      },
    ];

    const updated = applyToolResultMessage(running, {
      resultData: {
        tool_use_id: "call-1",
        name: "search",
        input: JSON.stringify({ query: "tet(M)" }),
        output: "timeout",
        is_error: true,
      },
      matchId: "call-1",
      completedAt: 3000,
      prefaceBeforeTools: "搜索超时了，不过没关系，我有足够的信息继续。",
    });

    expect(updated[0].prefaceBeforeTools).toBe(
      "搜索超时了，不过没关系，我有足够的信息继续。",
    );
    expect(updated[0].toolCall?.status).toBe("error");
  });

  it("preserves preface text when a second tool_use frame fills arguments", () => {
    const initial: StreamToolMessageLike[] = [];
    const withPlaceholder = upsertToolUseMessage(initial, {
      toolData: { id: "call-2", name: "search", arguments: "" },
      newToolId: "tool-2",
      prefaceBeforeTools: "Search current docs before answering.",
      timestamp: 2000,
    });

    const withArguments = upsertToolUseMessage(withPlaceholder, {
      toolData: {
        id: "call-2",
        name: "search",
        arguments: JSON.stringify({ query: "latest docs" }),
      },
      newToolId: "tool-unused",
      prefaceBeforeTools: "",
      timestamp: 2100,
    });

    expect(withArguments).toHaveLength(1);
    expect(withArguments[0].id).toBe("tool-2");
    expect(withArguments[0].prefaceBeforeTools).toBe(
      "Search current docs before answering.",
    );
    expect(withArguments[0].toolCall?.input).toBe(
      JSON.stringify({ query: "latest docs" }),
    );
  });

  it("archives delayed live preface when an existing tool_use is updated", () => {
    const withPlaceholder: StreamToolMessageLike[] = [
      {
        id: "tool-3",
        role: "tool",
        content: "`bash`",
        toolCall: { id: "call-3", name: "bash", status: "running" },
      },
    ];

    const withArguments = upsertToolUseMessage(withPlaceholder, {
      toolData: {
        id: "call-3",
        name: "bash",
        arguments: JSON.stringify({ command: "Rscript inspect.R" }),
      },
      newToolId: "tool-unused",
      prefaceBeforeTools:
        "PharmacoGx is not installed. Let me try to read the RDS file without the package.",
      timestamp: 3100,
    });

    expect(withArguments).toHaveLength(1);
    expect(withArguments[0].prefaceBeforeTools).toBe(
      "PharmacoGx is not installed. Let me try to read the RDS file without the package.",
    );
    expect(withArguments[0].toolCall?.input).toBe(
      JSON.stringify({ command: "Rscript inspect.R" }),
    );
  });

  it("normalizes persisted assistant tool-call text into the first matching tool preface", () => {
    const dbShape: StreamToolMessageLike[] = [
      {
        id: "assistant-1",
        role: "assistant",
        content: "先搜索两篇论文，再综合回答。",
        toolCallsList: [
          { id: "call-1", name: "search", arguments: "{}" },
          { id: "call-2", name: "search", arguments: "{}" },
        ],
      },
      {
        id: "tool-1",
        role: "tool",
        content: "timeout",
        toolCall: {
          id: "call-1",
          name: "search",
          status: "error",
          output: "timeout",
        },
      },
      {
        id: "tool-2",
        role: "tool",
        content: "ok",
        toolCall: {
          id: "call-2",
          name: "search",
          status: "completed",
          output: "ok",
        },
      },
    ];

    const normalized = normalizeAssistantToolCallPrefaces(dbShape);

    expect(normalized[0].content).toBe("");
    expect(normalized[1].prefaceBeforeTools).toBe(
      "先搜索两篇论文，再综合回答。",
    );
    expect(normalized[2].prefaceBeforeTools).toBeUndefined();
  });

  it("normalizes persisted reasoning_content preface into the first matching tool row", () => {
    const dbShape: StreamToolMessageLike[] = [
      {
        id: "assistant-2",
        role: "assistant",
        content: "",
        prefaceBeforeTools:
          "I need to verify the CAS1-specific RSL3 response before answering.",
        toolCallsList: [{ id: "call-3", name: "bash", arguments: "{}" }],
      },
      {
        id: "tool-3",
        role: "tool",
        content: "ok",
        toolCall: {
          id: "call-3",
          name: "bash",
          status: "completed",
          output: "ok",
        },
      },
    ];

    const normalized = normalizeAssistantToolCallPrefaces(dbShape);

    expect(normalized[0].content).toBe("");
    expect(normalized[0].prefaceBeforeTools).toBeUndefined();
    expect(normalized[1].prefaceBeforeTools).toBe(
      "I need to verify the CAS1-specific RSL3 response before answering.",
    );
  });
});
