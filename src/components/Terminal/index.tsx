import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  Alert,
  Box,
  Chip,
  IconButton,
  Stack,
  Tooltip,
  Typography,
  alpha,
  useTheme,
} from "@mui/material";
import {
  Circle,
  Clear,
  ContentCopy,
  PlayArrow,
  Stop,
  Terminal as TerminalIcon,
} from "@mui/icons-material";
import { extractErrorMessage } from "../../utils/errorMessage";
import { listenTauriEvent } from "../../utils/tauriEvents";
import { useChatComposerStore } from "../../state/chatComposerStore";
import {
  normalizeTerminalWorkspacePath,
  terminalWorkspaceDisplayName,
} from "./systemTerminal";
import { TerminalScreen, type TerminalRenderLine, type TerminalCellStyle } from "./terminalScreen";

interface TerminalProps {
  /** Hide the built-in title bar when embedded inside another tabbed shell (e.g. Chat). */
  embedded?: boolean;
  /** Start lazily when first shown, then keep the same terminal process while hidden. */
  active?: boolean;
  /** Current session workspace path. SSH uses a remote path; sandbox falls back to /workspace. */
  workspacePath?: string | null;
  sessionId?: string | null;
}

interface TerminalStartResponse {
  terminalId: string;
  cwd: string;
  label: string;
  executionEnvironment: string;
}

interface TerminalOutputEvent {
  terminalId: string;
  stream: "stdout" | "stderr" | "system";
  data: string;
}

interface TerminalExitEvent {
  terminalId: string;
  code?: number | null;
}

function terminalId() {
  return `term-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`;
}

interface TerminalThemeColors {
  surface: string;
  chrome: string;
  border: string;
  text: string;
  muted: string;
  chipBg: string;
  control: string;
  disabled: string;
  cursorBg: string;
  cursorText: string;
  focusRing: string;
}

function segmentSx(style: TerminalCellStyle, colors: TerminalThemeColors) {
  const fg = style.inverse ? style.bg ?? colors.surface : style.fg;
  const bg = style.cursor
    ? colors.cursorBg
    : style.inverse
      ? style.fg ?? colors.text
      : style.bg;
  return {
    color: style.cursor ? colors.cursorText : fg ?? "inherit",
    backgroundColor: bg,
    fontWeight: style.bold ? 800 : 500,
    fontStyle: style.italic ? "italic" : "normal",
    textDecoration: style.underline ? "underline" : "none",
    opacity: style.dim ? 0.72 : 1,
    animation: style.cursor ? "terminal-cursor-blink 1s steps(1) infinite" : undefined,
  };
}

export function Terminal({
  embedded = false,
  active = true,
  workspacePath,
  sessionId,
}: TerminalProps) {
  const theme = useTheme();
  const terminalColors = useMemo<TerminalThemeColors>(() => {
    const isDark = theme.palette.mode === "dark";
    return {
      surface: isDark
        ? alpha(theme.palette.background.default, 0.96)
        : alpha(theme.palette.background.paper, 0.92),
      chrome: isDark
        ? alpha(theme.palette.background.paper, 0.72)
        : alpha(theme.palette.background.paper, 0.82),
      border: alpha(theme.palette.text.primary, isDark ? 0.16 : 0.1),
      text: theme.palette.text.primary,
      muted: theme.palette.text.secondary,
      chipBg: alpha(theme.palette.primary.main, isDark ? 0.2 : 0.08),
      control: theme.palette.text.secondary,
      disabled: alpha(theme.palette.text.primary, isDark ? 0.28 : 0.22),
      cursorBg: theme.palette.text.primary,
      cursorText: theme.palette.background.default,
      focusRing: alpha(theme.palette.primary.main, isDark ? 0.48 : 0.36),
    };
  }, [theme]);
  const environment = useChatComposerStore((s) => s.environment);
  const sshServer = useChatComposerStore((s) => s.sshServer);
  const sandboxBackend = useChatComposerStore((s) => s.sandboxBackend);
  const normalizedWorkspace = useMemo(
    () => normalizeTerminalWorkspacePath(workspacePath),
    [workspacePath],
  );
  const workspace =
    environment === "sandbox" ? normalizedWorkspace ?? "/workspace" : normalizedWorkspace;
  const workspaceLabel = useMemo(
    () => terminalWorkspaceDisplayName(workspace),
    [workspace],
  );

  const [activeTerminalId, setActiveTerminalId] = useState<string | null>(null);
  const [terminalInfo, setTerminalInfo] = useState<TerminalStartResponse | null>(null);
  const screenRef = useRef(new TerminalScreen());
  const [lines, setLines] = useState<TerminalRenderLine[]>(() => screenRef.current.snapshot());
  const [status, setStatus] = useState<"connecting" | "running" | "exited" | "error">(
    "connecting",
  );
  const [error, setError] = useState<string | null>(null);
  const scrollRef = useRef<HTMLDivElement>(null);
  const terminalRef = useRef<HTMLDivElement>(null);
  const cleanupRef = useRef<(() => void) | undefined>();
  const startedTargetRef = useRef<string | null>(null);

  const targetKey = `${sessionId ?? ""}|${environment}|${sshServer ?? ""}|${sandboxBackend}|${workspace ?? ""}`;
  const startKey = active || startedTargetRef.current === targetKey ? targetKey : "__terminal_idle__";
  const envLabel =
    environment === "ssh"
      ? sshServer
        ? `SSH · ${sshServer}`
        : "SSH"
      : environment === "sandbox"
        ? `容器 · ${sandboxBackend}`
        : "本地";

  const canStart =
    environment === "sandbox"
      ? Boolean(sessionId && sandboxBackend)
      : environment === "ssh"
        ? Boolean(workspace && sshServer)
        : Boolean(workspace);

  const writeToScreen = useCallback((data: string) => {
    screenRef.current.write(data);
    setLines(screenRef.current.snapshot());
  }, []);

  const writeSystem = useCallback((message: string, color = "94") => {
    writeToScreen(`\r\n\x1b[${color}m${message}\x1b[0m\r\n`);
  }, [writeToScreen]);

  const stopTerminal = useCallback(async (id: string | null = activeTerminalId) => {
    if (!id) return;
    try {
      await invoke("terminal_stop", { terminalId: id });
    } catch {
      // Ignore shutdown races.
    }
  }, [activeTerminalId]);

  const startTerminal = useCallback(async () => {
    cleanupRef.current?.();
    cleanupRef.current = undefined;

    const id = terminalId();
    setActiveTerminalId(id);
    setTerminalInfo(null);
    screenRef.current.clear();
    setLines(screenRef.current.snapshot());
    setStatus("connecting");
    setError(null);

    if (!canStart) {
      const message =
        environment === "ssh"
          ? "请先选择 SSH 服务器和远端工作区。"
          : environment === "sandbox"
            ? "请先创建会话并选择容器/沙箱后端。"
            : "请先选择本地工作区。";
      setStatus("error");
      setError(message);
      writeSystem(message, "91");
      return;
    }

    let unlistenOutput: (() => void) | undefined;
    let unlistenExit: (() => void) | undefined;
    try {
      unlistenOutput = await listenTauriEvent<TerminalOutputEvent>(
        `terminal-output-${id}`,
        (event) => {
          writeToScreen(event.payload.data);
        },
      );
      unlistenExit = await listenTauriEvent<TerminalExitEvent>(
        `terminal-exit-${id}`,
        (event) => {
          if (event.payload.terminalId !== id) return;
          setStatus("exited");
          writeSystem(
            `[terminal exited${event.payload.code != null ? `: ${event.payload.code}` : ""}]`,
          );
        },
      );

      const response = await invoke<TerminalStartResponse>("terminal_start", {
        request: {
          terminalId: id,
          cwd: workspace,
          executionEnvironment: environment,
          sshProfileName: sshServer,
          sandboxBackend,
          sessionId,
        },
      });
      setTerminalInfo(response);
      setStatus("running");
      queueMicrotask(() => terminalRef.current?.focus());
    } catch (err) {
      unlistenOutput?.();
      unlistenExit?.();
      void stopTerminal(id);
      const message = extractErrorMessage(err);
      setStatus("error");
      setError(message);
      writeSystem(message, "91");
    }

    let cleaned = false;
    const cleanup = () => {
      if (cleaned) return;
      cleaned = true;
      unlistenOutput?.();
      unlistenExit?.();
      void stopTerminal(id);
      if (cleanupRef.current === cleanup) {
        cleanupRef.current = undefined;
      }
    };
    cleanupRef.current = cleanup;
    return cleanup;
  }, [
    canStart,
    environment,
    sandboxBackend,
    sessionId,
    sshServer,
    stopTerminal,
    writeSystem,
    writeToScreen,
    workspace,
  ]);

  useEffect(() => {
    if (startKey === "__terminal_idle__") return undefined;
    const startedKey = startKey;
    startedTargetRef.current = startedKey;
    let disposed = false;
    let cleanup: (() => void) | undefined;
    void startTerminal().then((fn) => {
      if (disposed) {
        fn?.();
      } else {
        cleanup = fn;
      }
    });
    return () => {
      disposed = true;
      cleanupRef.current?.();
      cleanupRef.current = undefined;
      cleanup?.();
      if (startedTargetRef.current === startedKey) {
        startedTargetRef.current = null;
      }
    };
    // startKey intentionally captures the session/environment surface after lazy activation.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [startKey]);

  useEffect(() => {
    scrollRef.current?.scrollIntoView({ behavior: "auto" });
  }, [lines]);

  const writeTerminalData = useCallback(async (data: string) => {
    if (!activeTerminalId || status !== "running") return;
    try {
      await invoke("terminal_write", {
        terminalId: activeTerminalId,
        data,
      });
    } catch (err) {
      writeSystem(extractErrorMessage(err), "91");
    }
  }, [activeTerminalId, status, writeSystem]);

  const handleKeyDown = (event: React.KeyboardEvent) => {
    const native = event.nativeEvent;
    if (native.isComposing || native.keyCode === 229) return;
    if (event.metaKey) return;

    let data: string | null = null;
    if (event.ctrlKey && event.key.length === 1) {
      data = String.fromCharCode(event.key.toUpperCase().charCodeAt(0) - 64);
    } else {
      switch (event.key) {
        case "Enter":
          data = "\r";
          break;
        case "Backspace":
          data = "\x7f";
          break;
        case "Tab":
          data = "\t";
          break;
        case "Escape":
          data = "\x1b";
          break;
        case "ArrowUp":
          data = "\x1b[A";
          break;
        case "ArrowDown":
          data = "\x1b[B";
          break;
        case "ArrowRight":
          data = "\x1b[C";
          break;
        case "ArrowLeft":
          data = "\x1b[D";
          break;
        case "Home":
          data = "\x1b[H";
          break;
        case "End":
          data = "\x1b[F";
          break;
        case "Delete":
          data = "\x1b[3~";
          break;
        default:
          if (event.key.length === 1) data = `${event.altKey ? "\x1b" : ""}${event.key}`;
      }
    }

    if (!data) return;
    event.preventDefault();
    void writeTerminalData(data);
  };

  const handlePaste = (event: React.ClipboardEvent) => {
    const text = event.clipboardData.getData("text");
    if (!text) return;
    event.preventDefault();
    void writeTerminalData(text);
  };

  const copyOutput = async () => {
    try {
      await navigator.clipboard.writeText(screenRef.current.toPlainText());
    } catch (err) {
      writeSystem(`复制输出失败：${extractErrorMessage(err)}`, "91");
    }
  };

  const statusColor =
    status === "running"
      ? "success.main"
      : status === "connecting"
        ? "warning.main"
        : status === "error"
          ? "error.main"
          : "text.disabled";

  return (
    <Box
      sx={{
        height: "100%",
        minHeight: 0,
        display: "flex",
        flexDirection: "column",
        bgcolor: terminalColors.surface,
        color: terminalColors.text,
      }}
    >
      <Stack
        direction="row"
        alignItems="center"
        justifyContent="space-between"
        sx={{
          minHeight: embedded ? 38 : 42,
          px: 1.25,
          borderBottom: 1,
          borderColor: terminalColors.border,
          bgcolor: terminalColors.chrome,
          backdropFilter: "blur(12px) saturate(140%)",
          WebkitBackdropFilter: "blur(12px) saturate(140%)",
        }}
      >
        <Stack direction="row" alignItems="center" spacing={1} sx={{ minWidth: 0 }}>
          <TerminalIcon fontSize="small" />
          <Typography variant="body2" fontWeight={800} noWrap>
            内嵌终端
          </Typography>
          <Chip
            size="small"
            label={terminalInfo?.label ?? envLabel}
            sx={{
              height: 22,
              color: terminalColors.text,
              bgcolor: terminalColors.chipBg,
              fontSize: 11,
            }}
          />
          <Circle sx={{ fontSize: 9, color: statusColor }} />
          <Typography
            variant="caption"
            noWrap
            sx={{ color: terminalColors.muted, maxWidth: 460 }}
          >
            {terminalInfo?.cwd ?? workspaceLabel}
          </Typography>
        </Stack>

        <Stack direction="row" alignItems="center" spacing={0.25}>
          <Tooltip title="重启终端">
            <IconButton
              size="small"
              onClick={() => void startTerminal()}
              sx={{ color: terminalColors.control }}
            >
              <PlayArrow fontSize="small" />
            </IconButton>
          </Tooltip>
          <Tooltip title="停止终端">
            <span>
              <IconButton
                size="small"
                disabled={!activeTerminalId || status !== "running"}
                onClick={() => void stopTerminal()}
                sx={{
                  color: terminalColors.control,
                  "&.Mui-disabled": { color: terminalColors.disabled },
                }}
              >
                <Stop fontSize="small" />
              </IconButton>
            </span>
          </Tooltip>
          <Tooltip title="复制输出">
            <IconButton
              size="small"
              onClick={() => void copyOutput()}
              sx={{ color: terminalColors.control }}
            >
              <ContentCopy fontSize="small" />
            </IconButton>
          </Tooltip>
          <Tooltip title="清空输出">
            <IconButton
              size="small"
              onClick={() => {
                screenRef.current.clear();
                setLines(screenRef.current.snapshot());
              }}
              sx={{ color: terminalColors.control }}
            >
              <Clear fontSize="small" />
            </IconButton>
          </Tooltip>
        </Stack>
      </Stack>

      {error && (
        <Alert severity="error" variant="filled" sx={{ borderRadius: 0 }}>
          {error}
        </Alert>
      )}

      <Box
        ref={terminalRef}
        tabIndex={0}
        onClick={() => terminalRef.current?.focus()}
        onKeyDown={handleKeyDown}
        onPaste={handlePaste}
        sx={{
          flex: 1,
          minHeight: 0,
          overflow: "auto",
          px: 1.5,
          py: 1.25,
          fontFamily: "JetBrains Mono, Monaco, Consolas, monospace",
          fontSize: 12.5,
          lineHeight: 1.55,
          cursor: "text",
          outline: "none",
          "&:focus": {
            boxShadow: `inset 0 0 0 1px ${terminalColors.focusRing}`,
          },
          "@keyframes terminal-cursor-blink": {
            "0%, 49%": { opacity: 1 },
            "50%, 100%": { opacity: 0 },
          },
        }}
      >
        {lines.map((line) => (
          <Box
            key={line.key}
            component="div"
            sx={{
              display: "block",
              minHeight: "1.55em",
              whiteSpace: "pre",
              wordBreak: "break-word",
              color: terminalColors.text,
            }}
          >
            {line.segments.map((segment) => (
              <Box
                key={segment.key}
                component="span"
                sx={segmentSx(segment.style, terminalColors)}
              >
                {segment.text}
              </Box>
            ))}
          </Box>
        ))}
        <div ref={scrollRef} />
      </Box>

      <Box
        sx={{
          px: 1.25,
          py: 1,
          borderTop: 1,
          borderColor: terminalColors.border,
          bgcolor: terminalColors.chrome,
          backdropFilter: "blur(12px) saturate(140%)",
          WebkitBackdropFilter: "blur(12px) saturate(140%)",
        }}
      >
        <Typography
          variant="caption"
          sx={{
            color: terminalColors.muted,
            fontFamily: "JetBrains Mono, Monaco, Consolas, monospace",
          }}
        >
          {status === "running"
            ? "已启用按键直通：Enter / Tab / ↑↓ / Ctrl+C 会发送到 shell；点击终端后粘贴可直接输入。"
            : "终端未运行"}
        </Typography>
      </Box>
    </Box>
  );
}
