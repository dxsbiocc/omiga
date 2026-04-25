import { useEffect, useMemo, useState, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  Box,
  Button,
  Chip,
  CircularProgress,
  Stack,
  Typography,
  alpha,
  useTheme,
} from "@mui/material";
import {
  ExpandMore as ExpandMoreIcon,
  ExpandLess as ExpandLessIcon,
} from "@mui/icons-material";
import {
  shortBgTaskLabel,
  type BackgroundAgentTask,
  type BgSidechainMessage,
} from "./backgroundAgentTypes";
import { normalizeAgentDisplayName } from "../../state/agentStore";
import {
  buildOpaqueSidechainFallback,
  normalizeSidechainValue,
  type BackgroundTaskSummary,
} from "./backgroundAgentTranscriptUtils";
import { extractErrorMessage } from "../../utils/errorMessage";
import { listenTauriEvent } from "../../utils/tauriEvents";
import { MarkdownTextViewer } from "../MarkdownText";
import { RightDetailDrawer } from "../RightDetailDrawer";

/** Debounce rapid `background-agent-*` events so we do not hammer `load_background_agent_transcript`. */
const LIVE_REFRESH_DEBOUNCE_MS = 380;

const TOOL_OUTPUT_PREVIEW_CHARS = 1600;
const TOOL_ARG_PREVIEW_CHARS = 360;

export interface BackgroundAgentTranscriptDrawerProps {
  open: boolean;
  onClose: () => void;
  sessionId: string | null;
  taskId: string | null;
  /** Short label for header (e.g. agent type + description) */
  taskLabel?: string;
}

export function BackgroundAgentTranscriptDrawer({
  open,
  onClose,
  sessionId,
  taskId,
  taskLabel,
}: BackgroundAgentTranscriptDrawerProps) {
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [rawMessages, setRawMessages] = useState<BgSidechainMessage[]>([]);
  const [taskSummary, setTaskSummary] = useState<BackgroundAgentTask | null>(null);
  const [liveNonce, setLiveNonce] = useState(0);
  const debounceTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    if (!open || !taskId) return;
    let cancelled = false;
    let u1: (() => void) | undefined;
    let u2: (() => void) | undefined;

    const scheduleDebouncedRefresh = () => {
      if (debounceTimerRef.current) {
        clearTimeout(debounceTimerRef.current);
      }
      debounceTimerRef.current = setTimeout(() => {
        debounceTimerRef.current = null;
        if (!cancelled) {
          setLiveNonce((n) => n + 1);
        }
      }, LIVE_REFRESH_DEBOUNCE_MS);
    };

    void (async () => {
      const [a, b] = await Promise.all([
        listenTauriEvent<BackgroundAgentTask>("background-agent-update", (e) => {
          if (!cancelled && e.payload.task_id === taskId) {
            scheduleDebouncedRefresh();
          }
        }),
        listenTauriEvent<{ task_id: string }>("background-agent-complete", (e) => {
          if (!cancelled && e.payload.task_id === taskId) {
            scheduleDebouncedRefresh();
          }
        }),
      ]);
      if (cancelled) {
        a();
        b();
        return;
      }
      u1 = a;
      u2 = b;
    })();
    return () => {
      cancelled = true;
      if (debounceTimerRef.current) {
        clearTimeout(debounceTimerRef.current);
        debounceTimerRef.current = null;
      }
      u1?.();
      u2?.();
    };
  }, [open, taskId]);

  useEffect(() => {
    if (!open || !sessionId || !taskId) {
      setRawMessages([]);
      setTaskSummary(null);
      setError(null);
      return;
    }
    let cancelled = false;
    setLoading(true);
    setError(null);

    void Promise.allSettled([
      invoke<BgSidechainMessage[]>("load_background_agent_transcript", {
        sessionId,
        taskId,
      }),
      invoke<BackgroundAgentTask[]>("list_session_background_tasks", { sessionId }),
    ])
      .then(([transcriptResult, tasksResult]) => {
        if (cancelled) return;
        if (transcriptResult.status === "fulfilled") {
          setRawMessages(transcriptResult.value ?? []);
        } else {
          setRawMessages([]);
          setError(extractErrorMessage(transcriptResult.reason));
        }

        if (tasksResult.status === "fulfilled") {
          setTaskSummary(
            (tasksResult.value ?? []).find((task) => task.task_id === taskId) ?? null,
          );
        } else {
          setTaskSummary(null);
        }
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [open, sessionId, taskId, liveNonce]);


  const effectiveTaskLabel = useMemo(() => {
    if (taskLabel?.trim()) return taskLabel;
    if (!taskSummary) return undefined;
    const agentLabel = normalizeAgentDisplayName(taskSummary.agent_type);
    return `${agentLabel}: ${shortBgTaskLabel(taskSummary, 72)}`;
  }, [taskLabel, taskSummary]);

  const messages = useMemo(() => {
    const fallbackTask: BackgroundTaskSummary | null = taskSummary;
    return rawMessages.map((row) => {
      if (row.role === "user") {
        const fallback = buildOpaqueSidechainFallback({
          kind: "message",
          task: fallbackTask,
          taskLabel: effectiveTaskLabel,
        });
        return { ...row, content: normalizeSidechainValue(row.content, fallback) };
      }
      if (row.role === "assistant") {
        const messageFallback = buildOpaqueSidechainFallback({
          kind: "message",
          task: fallbackTask,
          taskLabel: effectiveTaskLabel,
        });
        const argumentFallback = buildOpaqueSidechainFallback({
          kind: "toolArguments",
          task: fallbackTask,
          taskLabel: effectiveTaskLabel,
        });
        return {
          ...row,
          content: normalizeSidechainValue(row.content, messageFallback),
          tool_calls:
            row.tool_calls?.map((tool) => ({
              ...tool,
              arguments: normalizeSidechainValue(tool.arguments, argumentFallback),
            })) ?? null,
        };
      }
      const fallback = buildOpaqueSidechainFallback({
        kind: "toolOutput",
        task: fallbackTask,
        taskLabel: effectiveTaskLabel,
      });
      return {
        ...row,
        output: normalizeSidechainValue(row.output, fallback),
      };
    });
  }, [effectiveTaskLabel, rawMessages, taskSummary]);

  return (
    <RightDetailDrawer
      open={open}
      onClose={onClose}
      title="队友记录"
      subtitle={effectiveTaskLabel}
      closeLabel="Close transcript"
    >
      <TaskSummaryCard task={taskSummary} fallbackLabel={effectiveTaskLabel} />
      {loading ? (
        <Stack alignItems="center" py={4}>
          <CircularProgress size={28} />
        </Stack>
      ) : error ? (
        <Typography color="error" variant="body2">
          {error}
        </Typography>
      ) : messages.length === 0 ? (
        <Typography variant="body2" color="text.secondary">
          暂无记录（任务刚开始或尚未写入侧链）。
        </Typography>
      ) : (
        <Stack spacing={2}>
          {messages.map((m, i) => (
            <BgMessageBlock key={`${i}-${m.role}`} message={m} />
          ))}
        </Stack>
      )}
    </RightDetailDrawer>
  );
}


function taskStatusLabel(status?: string | null): string {
  const labels: Record<string, string> = {
    Pending: "待执行",
    Running: "执行中",
    Completed: "已完成",
    Failed: "失败",
    Cancelled: "已取消",
    pending: "待执行",
    running: "执行中",
    completed: "已完成",
    failed: "失败",
    cancelled: "已取消",
  };
  return status ? (labels[status] ?? status) : "未知";
}

function taskStatusColor(status?: string | null): "default" | "primary" | "success" | "error" | "warning" {
  switch (status) {
    case "Running":
    case "running":
      return "primary";
    case "Completed":
    case "completed":
      return "success";
    case "Failed":
    case "failed":
      return "error";
    case "Cancelled":
    case "cancelled":
      return "warning";
    default:
      return "default";
  }
}

function formatTaskField(value: unknown): string {
  const text = normalizeSidechainValue(value, undefined, { allowOpaqueFallback: false }).trim();
  return text === "[object Object]" ? "" : text;
}

function TaskSummaryCard({
  task,
  fallbackLabel,
}: {
  task: BackgroundAgentTask | null;
  fallbackLabel?: string;
}) {
  const theme = useTheme();
  const edge = alpha(
    theme.palette.mode === "dark"
      ? theme.palette.common.white
      : theme.palette.common.black,
    0.07,
  );
  if (!task && !fallbackLabel) return null;

  const description = task?.description?.trim() || fallbackLabel || "后台任务";
  const result = formatTaskField(task?.result_summary);
  const error = formatTaskField(task?.error_message);
  const agent = task?.agent_type ? normalizeAgentDisplayName(task.agent_type) : undefined;

  return (
    <Box
      sx={{
        mb: 2,
        p: 1.5,
        borderRadius: 2,
        bgcolor: alpha(theme.palette.info.main, 0.055),
        border: `1px solid ${edge}`,
      }}
    >
      <Stack direction="row" alignItems="center" spacing={0.75} sx={{ mb: 0.75 }}>
        <Typography variant="caption" color="info.dark" fontWeight={700}>
          任务详情
        </Typography>
        {agent ? (
          <Chip size="small" label={agent} sx={{ height: 20, fontSize: 10 }} />
        ) : null}
        {task?.status ? (
          <Chip
            size="small"
            color={taskStatusColor(task.status)}
            variant="outlined"
            label={taskStatusLabel(task.status)}
            sx={{ height: 20, fontSize: 10 }}
          />
        ) : null}
      </Stack>
      <Typography variant="body2" sx={{ whiteSpace: "pre-wrap", wordBreak: "break-word" }}>
        {description}
      </Typography>
      {error ? (
        <Box sx={{ mt: 1 }}>
          <Typography variant="caption" color="error" fontWeight={700}>
            错误信息
          </Typography>
          <Box sx={{ mt: 0.35 }}>
            <MarkdownTextViewer>{error}</MarkdownTextViewer>
          </Box>
        </Box>
      ) : null}
      {result ? (
        <Box sx={{ mt: 1 }}>
          <Typography variant="caption" color="text.secondary" fontWeight={700}>
            结果摘要
          </Typography>
          <Box sx={{ mt: 0.35 }}>
            <MarkdownTextViewer>{result}</MarkdownTextViewer>
          </Box>
        </Box>
      ) : null}
    </Box>
  );
}

function BgMessageBlock({ message }: { message: BgSidechainMessage }) {
  const theme = useTheme();
  const edge = alpha(
    theme.palette.mode === "dark"
      ? theme.palette.common.white
      : theme.palette.common.black,
    0.06,
  );

  if (message.role === "user") {
    return (
      <Box
        sx={{
          p: 1.5,
          borderRadius: 2,
          bgcolor: alpha(theme.palette.primary.main, 0.06),
          border: `1px solid ${edge}`,
        }}
      >
        <Typography variant="caption" color="primary" fontWeight={600}>
          User
        </Typography>
        <Box sx={{ mt: 0.5 }}>
          <MarkdownTextViewer color="text.primary">{message.content}</MarkdownTextViewer>
        </Box>
      </Box>
    );
  }

  if (message.role === "assistant") {
    return (
      <AssistantSidechainBlock message={message} edge={edge} />
    );
  }

  return (
    <ToolSidechainBlock output={message.output} toolCallId={message.tool_call_id} edge={edge} />
  );
}

function AssistantSidechainBlock({
  message,
  edge,
}: {
  message: Extract<BgSidechainMessage, { role: "assistant" }>;
  edge: string;
}) {
  const theme = useTheme();
  const calls = message.tool_calls?.filter(Boolean) ?? [];

  return (
    <Box
      sx={{
        p: 1.5,
        borderRadius: 2,
        bgcolor: alpha(theme.palette.secondary.main, 0.06),
        border: `1px solid ${edge}`,
      }}
    >
      <Typography variant="caption" color="secondary" fontWeight={600}>
        Assistant
      </Typography>
      {message.content.trim().length > 0 ? (
        <Box sx={{ mt: 0.5 }}>
          <MarkdownTextViewer color="text.primary">{message.content}</MarkdownTextViewer>
        </Box>
      ) : null}
      {calls.length > 0 ? (
        <Stack spacing={1} sx={{ mt: 1 }}>
          <Typography variant="caption" color="text.secondary" fontWeight={600}>
            工具调用 ({calls.length})
          </Typography>
          {calls.map((c) => (
            <ToolCallArgCard key={c.id} name={c.name} id={c.id} argumentsText={c.arguments} edge={edge} />
          ))}
        </Stack>
      ) : null}
    </Box>
  );
}

function ToolCallArgCard({
  name,
  id,
  argumentsText,
  edge,
}: {
  name: string;
  id: string;
  argumentsText: string;
  edge: string;
}) {
  const theme = useTheme();
  const [expanded, setExpanded] = useState(false);
  const long = argumentsText.length > TOOL_ARG_PREVIEW_CHARS;
  const shown =
    expanded || !long
      ? argumentsText
      : `${argumentsText.slice(0, TOOL_ARG_PREVIEW_CHARS)}…`;

  return (
    <Box
      sx={{
        p: 1,
        borderRadius: 1,
        bgcolor: alpha(theme.palette.secondary.main, 0.04),
        border: `1px solid ${edge}`,
      }}
    >
      <Typography variant="caption" fontWeight={700} color="secondary.dark" component="div">
        {name}
      </Typography>
      <Typography variant="caption" color="text.disabled" sx={{ display: "block", mb: 0.5 }}>
        id: {id}
      </Typography>
      <Typography
        variant="caption"
        component="pre"
        sx={{
          whiteSpace: "pre-wrap",
          wordBreak: "break-word",
          fontFamily: "ui-monospace, SFMono-Regular, Menlo, monospace",
          fontSize: 11,
          m: 0,
          display: "block",
        }}
      >
        {shown}
      </Typography>
      {long ? (
        <Button
          size="small"
          sx={{ mt: 0.5, minWidth: 0, p: 0, fontSize: 11 }}
          onClick={() => setExpanded(!expanded)}
          endIcon={expanded ? <ExpandLessIcon sx={{ fontSize: 16 }} /> : <ExpandMoreIcon sx={{ fontSize: 16 }} />}
        >
          {expanded ? "收起参数" : "展开参数"}
        </Button>
      ) : null}
    </Box>
  );
}

function ToolSidechainBlock({
  output,
  toolCallId,
  edge,
}: {
  output: string;
  toolCallId: string;
  edge: string;
}) {
  const theme = useTheme();
  const [expanded, setExpanded] = useState(false);
  const long = output.length > TOOL_OUTPUT_PREVIEW_CHARS;
  const display =
    expanded || !long ? output : `${output.slice(0, TOOL_OUTPUT_PREVIEW_CHARS)}\n…`;

  return (
    <Box
      sx={{
        p: 1.5,
        borderRadius: 2,
        bgcolor: alpha(theme.palette.warning.main, 0.06),
        border: `1px solid ${edge}`,
      }}
    >
      <Stack
        direction="row"
        alignItems="flex-start"
        justifyContent="space-between"
        gap={1}
        sx={{ mb: 0.5 }}
      >
        <Typography variant="caption" color="warning.dark" fontWeight={600} sx={{ flex: 1 }}>
          Tool · {toolCallId}
        </Typography>
        <Stack direction="row" spacing={0.5} flexShrink={0}>
          {long ? (
            <Button
              size="small"
              variant="text"
              sx={{ minWidth: 0, fontSize: 11, py: 0 }}
              onClick={() => setExpanded(!expanded)}
              endIcon={
                expanded ? (
                  <ExpandLessIcon sx={{ fontSize: 16 }} />
                ) : (
                  <ExpandMoreIcon sx={{ fontSize: 16 }} />
                )
              }
            >
              {expanded ? "收起" : "展开"}
            </Button>
          ) : null}
        </Stack>
      </Stack>
      <Box sx={{ mt: 0.5 }}>
        <MarkdownTextViewer color="text.primary" copyText={output}>
          {display}
        </MarkdownTextViewer>
      </Box>
    </Box>
  );
}
