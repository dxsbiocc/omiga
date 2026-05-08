import { memo } from "react";
import { Chip, Stack, Typography } from "@mui/material";
import { alpha } from "@mui/material/styles";
import { CheckCircle, ExpandMore } from "@mui/icons-material";
import type { getChatTokens } from "./chatTokens";
import { formatToolDisplayName } from "../../utils/executionSurfaceLabel";

export type ToolCallStatus = "pending" | "running" | "completed" | "error";

export interface ToolCallLike {
  id?: string;
  name: string;
  status?: ToolCallStatus;
  input?: string;
  output?: string;
  completedAt?: number;
}

export interface ToolSummaryMessage {
  role?: string;
  content?: string;
  prefaceBeforeTools?: string;
  toolCall?: ToolCallLike;
}

type ChatTokens = ReturnType<typeof getChatTokens>;

export interface StructuredToolErrorHint {
  error: string;
  message: string | null;
  details: string | null;
  route: string | null;
  nextAction: string | null;
  diagnosticsHint: string | null;
  recoverable: boolean | null;
}

/** Pencil collapseRow summary: "Ran 2 commands, viewed a file". */
export function summarizeToolGroup(
  messages: readonly ToolSummaryMessage[],
): string {
  if (messages.length === 0) return "";
  if (messages.length === 1) {
    return messages[0].toolCall?.name ?? "tool";
  }
  const names = messages.map((m) => m.toolCall?.name ?? "tool");
  const bashCount = names.filter((n) => n === "bash").length;
  const fileOps = names.filter(
    (n) =>
      n.includes("glob") ||
      n.includes("file_read") ||
      n.includes("read_file") ||
      n.includes("file_write") ||
      n.includes("file_edit") ||
      n === "fetch" ||
      n === "query" ||
      n === "search" ||
      n.includes("todo_write") ||
      n.includes("notebook_edit") ||
      n === "file_read",
  ).length;
  const parts: string[] = [];
  if (bashCount > 0) {
    parts.push(`${bashCount} command${bashCount > 1 ? "s" : ""}`);
  }
  if (fileOps > 0) {
    parts.push(fileOps === 1 ? "viewed a file" : `viewed ${fileOps} files`);
  }
  const accounted = bashCount + fileOps;
  if (accounted < messages.length) {
    parts.push(`${messages.length - accounted} more`);
  }
  if (parts.length === 0) {
    return `Ran ${messages.length} tools`;
  }
  return `Ran ${parts.join(", ")}`;
}

export function summarizeReactFold(
  fold: readonly ToolSummaryMessage[],
): string {
  const tools = fold.filter((m) => m.role === "tool" && m.toolCall);
  const thinking = fold.filter(
    (m) =>
      m.role === "assistant" ||
      (m.role === "tool" && Boolean(m.prefaceBeforeTools?.trim())),
  ).length;
  if (tools.length === 0) {
    return thinking > 0 ? "Reasoning" : "Trace";
  }
  const toolSummary = summarizeToolGroup(tools);
  return thinking > 0 ? `Reasoning · ${toolSummary}` : toolSummary;
}

export function toolGroupAnyRunning(
  messages: readonly ToolSummaryMessage[],
): boolean {
  return messages.some((m) => m.toolCall?.status === "running");
}

/** Name of a tool call still running (prefer the latest in the fold). */
export function firstRunningToolName(
  messages: readonly ToolSummaryMessage[],
): string | null {
  for (let i = messages.length - 1; i >= 0; i--) {
    const m = messages[i];
    if (
      m.role === "tool" &&
      m.toolCall?.status === "running" &&
      m.toolCall.name?.trim()
    ) {
      return m.toolCall.name.trim();
    }
  }
  return null;
}

/**
 * Outer tool fold is "complete" when nothing is still running. Per-tool `error`
 * from `is_error` does not fail the whole fold — nested rows still show Error.
 */
export function toolGroupFlowComplete(
  messages: readonly ToolSummaryMessage[],
): boolean {
  if (messages.length === 0) return false;
  return !toolGroupAnyRunning(messages);
}

/** Prefer toolCall.output; avoid using short status-only message.content as "Output". */
export function toolDisplayOutputText(
  message: ToolSummaryMessage,
  tc: ToolCallLike,
): string {
  const fromTc = tc.output?.trim();
  if (fromTc) return fromTc;
  if (tc.status === "running") return "";
  if (message.role !== "tool" || !message.content?.trim()) return "";
  const c = message.content.trim();
  if (/^`[^`]+`$/i.test(c)) return "";
  if (/^`[^`]+`\s+(completed|failed)$/i.test(c)) return "";
  return c;
}

function stringField(value: unknown): string | null {
  return typeof value === "string" && value.trim() ? value.trim() : null;
}

function booleanField(value: unknown): boolean | null {
  return typeof value === "boolean" ? value : null;
}

/** Parse structured tool error JSON emitted by backend tools into actionable UI hints. */
export function parseStructuredToolErrorHint(
  output: string | undefined,
): StructuredToolErrorHint | null {
  if (!output?.trim()) return null;
  let parsed: unknown;
  try {
    parsed = JSON.parse(output);
  } catch {
    return null;
  }
  if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) return null;
  const obj = parsed as Record<string, unknown>;
  const error = stringField(obj.error);
  if (!error) return null;

  const message = stringField(obj.message);
  const nextAction = stringField(obj.next_action) ?? stringField(obj.nextAction);
  const diagnosticsHint =
    stringField(obj.diagnostics_hint) ?? stringField(obj.diagnosticsHint);
  const route = stringField(obj.route);
  const details = stringField(obj.details);
  const recoverable = booleanField(obj.recoverable);

  if (!message && !nextAction && !diagnosticsHint && !route && !details) {
    return null;
  }

  return {
    error,
    message,
    details,
    route,
    nextAction,
    diagnosticsHint,
    recoverable,
  };
}

/** Stable key for nested expand state inside a react_fold. */
export function toolNestedPanelKey(foldId: string, messageId: string): string {
  return `${foldId}::${messageId}`;
}

/** Read `description` from tool JSON (bash, file_read, etc.). */
export function parseToolDescriptionFromInput(
  input: string | undefined,
): string | null {
  if (!input?.trim()) return null;
  try {
    const j = JSON.parse(input) as Record<string, unknown>;
    const d = j.description;
    if (typeof d === "string" && d.trim()) return d.trim();
  } catch {
    /* not JSON */
  }
  return null;
}

/** Display title for a tool row: `description` from arguments JSON, else tool name. */
export function toolCallPanelTitle(
  input: string | undefined,
  toolName: string,
): string {
  return parseToolDescriptionFromInput(input) ?? toolName;
}

export function getNestedToolPanelOpen(
  key: string,
  tc: ToolCallLike,
  overrides: Readonly<Record<string, boolean>>,
): boolean {
  if (key in overrides) return overrides[key];
  return tc.status === "running";
}

export function formatToolDuration(
  startedAt: number | null | undefined,
  completedAt: number | null | undefined,
): string | null {
  if (startedAt == null || completedAt == null) return null;
  const durationMs = completedAt - startedAt;
  return durationMs >= 1000
    ? `${(durationMs / 1000).toFixed(1)}s`
    : `${durationMs}ms`;
}

export interface ToolFoldHeaderProps {
  foldId: string;
  expanded: boolean;
  summary: string;
  anyRunning: boolean;
  runningToolName: string | null;
  runningToolCount: number;
  showGroupDone: boolean;
  isLastFold: boolean;
  activityIsStreaming: boolean;
  waitingFirstChunk: boolean;
  chat: ChatTokens;
  onToggle: (foldId: string) => void;
}

export const ToolFoldHeader = memo(function ToolFoldHeader({
  foldId,
  expanded,
  summary,
  anyRunning,
  runningToolName,
  runningToolCount,
  showGroupDone,
  isLastFold,
  activityIsStreaming,
  waitingFirstChunk,
  chat,
  onToggle,
}: ToolFoldHeaderProps) {
  const runningSuffix = anyRunning
    ? runningToolCount > 1
      ? ` · ${runningToolCount} 并行`
      : runningToolName
        ? ` · ${formatToolDisplayName(runningToolName)}`
        : ""
    : "";
  const streamingSuffix =
    !anyRunning && isLastFold && activityIsStreaming
      ? ` · ${waitingFirstChunk ? "推理中" : "解析输出"}`
      : "";
  const runningLabel = runningToolCount > 1
    ? `${runningToolCount} 并行运行中`
    : runningToolName
      ? formatToolDisplayName(runningToolName)
      : "运行中";

  return (
    <Stack
      direction="row"
      alignItems="center"
      spacing={1}
      onClick={() => onToggle(foldId)}
      sx={{
        cursor: "pointer",
        userSelect: "none",
        minWidth: 0,
        borderRadius: "8px",
        mx: -0.75,
        px: 0.75,
        py: 0.5,
        transition: "background-color 150ms ease",
        "&:hover": {
          bgcolor: alpha(chat.accent, 0.07),
          "& > svg:first-of-type": {
            color: chat.accent,
            opacity: 1,
          },
          "& > .MuiTypography-root:first-of-type": {
            color: chat.textPrimary,
          },
        },
      }}
    >
      <ExpandMore
        sx={{
          fontSize: 14,
          color: chat.toolIcon,
          opacity: 0.65,
          transform: expanded ? "rotate(0deg)" : "rotate(-90deg)",
          transition: "transform 0.2s ease, color 150ms ease, opacity 150ms ease",
        }}
      />
      <Typography
        sx={{
          fontSize: 12,
          color: chat.textMuted,
          flex: 1,
          minWidth: 0,
          overflowWrap: "anywhere",
          wordBreak: "break-word",
          transition: "color 150ms ease",
        }}
      >
        {summary}
        {runningSuffix}
        {streamingSuffix}
      </Typography>
      {anyRunning && (
        <Chip
          size="small"
          label={runningLabel}
          sx={{ height: 22, fontSize: 11 }}
        />
      )}
      {showGroupDone && (
        <Chip
          size="small"
          icon={<CheckCircle fontSize="small" />}
          label="Done"
          color="primary"
          variant="outlined"
          sx={{ height: 22, fontSize: 11 }}
        />
      )}
    </Stack>
  );
});
