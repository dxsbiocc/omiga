export type StreamToolCallStatus = "pending" | "running" | "completed" | "error";

export interface StreamToolCallLike {
  id?: string;
  name: string;
  status?: StreamToolCallStatus;
  input?: string;
  output?: string;
  completedAt?: number;
}

export interface StreamToolMessageLike {
  id: string;
  role: "user" | "assistant" | "tool";
  content: string;
  prefaceBeforeTools?: string;
  toolCallsList?: Array<{ id: string; name: string; arguments: string }>;
  toolCall?: StreamToolCallLike;
  timestamp?: number;
}

export interface StreamToolUsePayload {
  id?: string;
  name?: string;
  arguments?: string;
}

export interface StreamToolResultPayload {
  tool_use_id?: string;
  name?: string;
  input?: string;
  output?: string;
  is_error?: boolean;
}

export function upsertToolUseMessage<T extends StreamToolMessageLike>(
  prev: readonly T[],
  params: {
    toolData: StreamToolUsePayload | undefined;
    newToolId: string;
    prefaceBeforeTools: string;
    timestamp: number;
  },
): T[] {
  const { toolData, newToolId, timestamp } = params;
  const tuId = (toolData?.id ?? "").trim();
  const toolName = toolData?.name || "tool";

  if (tuId) {
    const existingIndex = prev.findIndex(
      (m) => m.role === "tool" && m.toolCall?.id === tuId,
    );
    if (existingIndex >= 0) {
      const current = prev[existingIndex];
      if (!current.toolCall) return prev as T[];
      const preface = params.prefaceBeforeTools.trim();
      const next = [...prev] as T[];
      next[existingIndex] = {
        ...current,
        ...(preface && !current.prefaceBeforeTools?.trim()
          ? { prefaceBeforeTools: preface }
          : {}),
        content: `\`${toolName || current.toolCall.name}\``,
        toolCall: {
          ...current.toolCall,
          id: tuId,
          name: toolName || current.toolCall.name,
          input:
            toolData?.arguments !== undefined
              ? toolData.arguments
              : current.toolCall.input,
          status: current.toolCall.status ?? "running",
        },
      } as T;
      return next;
    }
  }

  const preface = params.prefaceBeforeTools.trim();
  const toolMsg = {
    id: newToolId,
    role: "tool" as const,
    content: `\`${toolName}\``,
    ...(preface ? { prefaceBeforeTools: preface } : {}),
    toolCall: {
      id: tuId || undefined,
      name: toolName,
      status: "running" as const,
      input: toolData?.arguments,
    },
    timestamp,
  } satisfies StreamToolMessageLike;

  return [...prev, toolMsg as T];
}

export function applyToolResultMessage<T extends StreamToolMessageLike>(
  prev: readonly T[],
  params: {
    resultData: StreamToolResultPayload | undefined;
    matchId: string | null;
    completedAt: number;
    prefaceBeforeTools?: string;
  },
): T[] {
  const { resultData, matchId, completedAt } = params;
  let idx = -1;
  if (matchId) {
    for (let i = prev.length - 1; i >= 0; i--) {
      const m = prev[i];
      if (m.role === "tool" && m.toolCall && m.toolCall.id === matchId) {
        idx = i;
        break;
      }
    }
  }
  if (idx < 0) {
    for (let i = prev.length - 1; i >= 0; i--) {
      const m = prev[i];
      if (m.role === "tool" && m.toolCall && m.toolCall.status === "running") {
        idx = i;
        break;
      }
    }
  }
  if (idx < 0) return prev as T[];

  const lastMsg = prev[idx];
  if (!lastMsg.toolCall) return prev as T[];

  const toolName = resultData?.name || lastMsg.toolCall.name;
  const preface = params.prefaceBeforeTools?.trim();
  const nextInput =
    resultData != null &&
    typeof resultData.input === "string" &&
    resultData.input.trim() !== ""
      ? resultData.input
      : lastMsg.toolCall.input;

  const updated = [...prev] as T[];
  updated[idx] = {
    ...lastMsg,
    ...(preface && !lastMsg.prefaceBeforeTools?.trim()
      ? { prefaceBeforeTools: preface }
      : {}),
    content: `\`${toolName}\` ${resultData?.is_error ? "failed" : "completed"}`,
    toolCall: {
      ...lastMsg.toolCall,
      name: toolName,
      status: resultData?.is_error ? "error" : "completed",
      input: nextInput,
      output: resultData?.output,
      completedAt,
    },
  } as T;
  return updated;
}

/**
 * DB reloads store pre-tool assistant text on the assistant row that owns
 * `tool_calls`; live rendering stores the same text on the next tool row as
 * `prefaceBeforeTools`. Normalize the DB shape to the live shape so the ReAct
 * fold looks the same after completion, refresh, or session switch.
 */
export function normalizeAssistantToolCallPrefaces<T extends StreamToolMessageLike>(
  messages: readonly T[],
): T[] {
  let next: T[] | null = null;
  const ensureNext = () => {
    if (!next) next = [...messages] as T[];
    return next;
  };

  for (let i = 0; i < messages.length; i++) {
    const source = (next ?? messages)[i];
    if (source.role !== "assistant" || !source.toolCallsList?.length) {
      continue;
    }

    const sourcePreface = [source.prefaceBeforeTools, source.content]
      .map((value) => value?.trim())
      .filter((value): value is string => Boolean(value))
      .join("\n\n");
    if (!sourcePreface) continue;

    const toolIds = new Set(source.toolCallsList.map((tc) => tc.id));
    for (let j = i + 1; j < messages.length; j++) {
      const candidate = (next ?? messages)[j];
      if (candidate.role === "user") break;
      if (
        candidate.role === "tool" &&
        candidate.toolCall?.id &&
        toolIds.has(candidate.toolCall.id)
      ) {
        const out = ensureNext();
        if (!candidate.prefaceBeforeTools?.trim()) {
          out[j] = {
            ...candidate,
            prefaceBeforeTools: sourcePreface,
          } as T;
        }
        out[i] = {
          ...source,
          content: "",
          prefaceBeforeTools: undefined,
        } as T;
        break;
      }
    }
  }

  return next ?? (messages as T[]);
}
