import { useEffect, useState, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  Box,
  Button,
  CircularProgress,
  Drawer,
  IconButton,
  Stack,
  Typography,
  alpha,
  useTheme,
} from "@mui/material";
import {
  Close as CloseIcon,
  ContentCopy as ContentCopyIcon,
  ExpandMore as ExpandMoreIcon,
  ExpandLess as ExpandLessIcon,
} from "@mui/icons-material";
import type { BackgroundAgentTask, BgSidechainMessage } from "./backgroundAgentTypes";

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
  const theme = useTheme();
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [messages, setMessages] = useState<BgSidechainMessage[]>([]);
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
        listen<BackgroundAgentTask>("background-agent-update", (e) => {
          if (!cancelled && e.payload.task_id === taskId) {
            scheduleDebouncedRefresh();
          }
        }),
        listen<{ task_id: string }>("background-agent-complete", (e) => {
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
      setMessages([]);
      setError(null);
      return;
    }
    let cancelled = false;
    setLoading(true);
    setError(null);
    invoke<BgSidechainMessage[]>("load_background_agent_transcript", {
      sessionId,
      taskId,
    })
      .then((rows) => {
        if (!cancelled) setMessages(rows ?? []);
      })
      .catch((e: unknown) => {
        if (!cancelled) {
          setError(e instanceof Error ? e.message : String(e));
          setMessages([]);
        }
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [open, sessionId, taskId, liveNonce]);

  const paper = theme.palette.background.paper;
  const edge = alpha(
    theme.palette.mode === "dark"
      ? theme.palette.common.white
      : theme.palette.common.black,
    0.08,
  );

  return (
    <Drawer
      anchor="right"
      open={open}
      onClose={onClose}
      PaperProps={{
        sx: {
          width: { xs: "100%", sm: 440 },
          maxWidth: "100vw",
          bgcolor: alpha(paper, 0.98),
          borderLeft: `1px solid ${edge}`,
        },
      }}
    >
      <Stack
        direction="row"
        alignItems="center"
        justifyContent="space-between"
        sx={{
          px: 2,
          py: 1.5,
          borderBottom: `1px solid ${edge}`,
        }}
      >
        <Typography variant="subtitle1" fontWeight={600} sx={{ pr: 1 }}>
          队友记录
          {taskLabel ? (
            <Typography
              component="span"
              variant="body2"
              color="text.secondary"
              sx={{ display: "block", fontWeight: 400, mt: 0.25 }}
            >
              {taskLabel}
            </Typography>
          ) : null}
        </Typography>
        <IconButton size="small" aria-label="Close transcript" onClick={onClose}>
          <CloseIcon />
        </IconButton>
      </Stack>

      <Box sx={{ flex: 1, overflow: "auto", p: 2 }}>
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
      </Box>
    </Drawer>
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
        <Typography
          variant="body2"
          sx={{ whiteSpace: "pre-wrap", wordBreak: "break-word", mt: 0.5 }}
        >
          {message.content}
        </Typography>
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
        <Typography
          variant="body2"
          sx={{ whiteSpace: "pre-wrap", wordBreak: "break-word", mt: 0.5 }}
        >
          {message.content}
        </Typography>
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
  const [copied, setCopied] = useState(false);
  const long = output.length > TOOL_OUTPUT_PREVIEW_CHARS;
  const display =
    expanded || !long ? output : `${output.slice(0, TOOL_OUTPUT_PREVIEW_CHARS)}\n…`;

  const handleCopy = async () => {
    try {
      await navigator.clipboard.writeText(output);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 2000);
    } catch {
      /* ignore */
    }
  };

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
          <Button
            size="small"
            variant="text"
            sx={{ minWidth: 0, fontSize: 11, py: 0 }}
            startIcon={<ContentCopyIcon sx={{ fontSize: 14 }} />}
            onClick={handleCopy}
          >
            {copied ? "已复制" : "复制"}
          </Button>
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
      <Typography
        variant="body2"
        component="pre"
        sx={{
          whiteSpace: "pre-wrap",
          wordBreak: "break-word",
          mt: 0.5,
          fontFamily: "ui-monospace, SFMono-Regular, Menlo, monospace",
          fontSize: 12,
          m: 0,
        }}
      >
        {display}
      </Typography>
    </Box>
  );
}
