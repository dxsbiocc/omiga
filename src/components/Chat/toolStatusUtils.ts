export type ToolCallStatus = "pending" | "running" | "completed" | "error";

export interface ToolStatusMessageLike {
  role: string;
  content: string;
  timestamp?: number;
  toolCall?: {
    name: string;
    status?: ToolCallStatus;
    completedAt?: number;
  };
}

export function settleRunningToolCalls<T extends ToolStatusMessageLike>(
  messages: T[],
  status: Extract<ToolCallStatus, "completed" | "error"> = "completed",
  now = Date.now(),
): T[] {
  const suffix = status === "error" ? "failed" : "completed";
  return messages.map((message) => {
    if (message.role !== "tool" || message.toolCall?.status !== "running") {
      return message;
    }

    const content =
      message.content.trim() === `\`${message.toolCall.name}\``
        ? `\`${message.toolCall.name}\` ${suffix}`
        : message.content;

    return {
      ...message,
      content,
      toolCall: {
        ...message.toolCall,
        status,
        completedAt: message.toolCall.completedAt ?? now,
      },
    };
  });
}
