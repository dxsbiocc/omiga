import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  Alert,
  Box,
  Button,
  Chip,
  Divider,
  IconButton,
  Stack,
  Tooltip,
  Typography,
  alpha,
  useTheme,
} from "@mui/material";
import {
  CheckCircleOutline,
  ContentCopy,
  FolderOpen,
  Launch,
  Terminal as TerminalIcon,
} from "@mui/icons-material";
import { extractErrorMessage } from "../../utils/errorMessage";
import { useChatComposerStore } from "../../state/chatComposerStore";
import {
  normalizeTerminalWorkspacePath,
  terminalWorkspaceDisplayName,
} from "./systemTerminal";

interface TerminalProps {
  /** Hide the built-in title bar when embedded inside another tabbed shell (e.g. Chat). */
  embedded?: boolean;
  /** Local workspace path for the current session; empty / "." means no folder selected. */
  workspacePath?: string | null;
  /** Optional session id used only for copy/status context in this view. */
  sessionId?: string | null;
}

interface OpenSystemTerminalResponse {
  cwd: string;
  terminal: string;
  execution_environment: string;
}

const recentAutoOpenKeys = new Map<string, number>();
const AUTO_OPEN_DEDUP_MS = 2_500;

function formatOpenedAt(timestamp: number | null): string | null {
  if (!timestamp) return null;
  try {
    return new Date(timestamp).toLocaleTimeString(undefined, {
      hour: "2-digit",
      minute: "2-digit",
      second: "2-digit",
    });
  } catch {
    return null;
  }
}

export function Terminal({
  embedded = false,
  workspacePath,
  sessionId,
}: TerminalProps) {
  const theme = useTheme();
  const environment = useChatComposerStore((s) => s.environment);
  const sshServer = useChatComposerStore((s) => s.sshServer);
  const sandboxBackend = useChatComposerStore((s) => s.sandboxBackend);
  const normalizedWorkspace = useMemo(
    () => normalizeTerminalWorkspacePath(workspacePath),
    [workspacePath],
  );
  const workspace = environment === "sandbox"
    ? normalizedWorkspace ?? "/workspace"
    : normalizedWorkspace;
  const workspaceLabel = useMemo(
    () =>
      environment === "sandbox"
        ? terminalWorkspaceDisplayName(workspace)
        : terminalWorkspaceDisplayName(workspacePath),
    [environment, workspace, workspacePath],
  );
  const [isOpening, setIsOpening] = useState(false);
  const [openError, setOpenError] = useState<string | null>(null);
  const [lastOpened, setLastOpened] = useState<{
    cwd: string;
    terminal: string;
    at: number;
  } | null>(null);
  const [copied, setCopied] = useState(false);

  const autoOpenStartedRef = useRef(false);

  const environmentLabel =
    environment === "ssh"
      ? sshServer
        ? `SSH · ${sshServer}`
        : "SSH"
      : environment === "sandbox"
        ? `容器 · ${sandboxBackend}`
        : "本地";

  const canOpen =
    !isOpening &&
    (environment === "sandbox"
      ? Boolean(sessionId && sandboxBackend)
      : environment === "ssh"
        ? Boolean(workspace && sshServer)
        : Boolean(workspace));
  const openedAtLabel = formatOpenedAt(lastOpened?.at ?? null);

  const openTargetKey = `${sessionId ?? ""}|${environment}|${sshServer ?? ""}|${sandboxBackend}|${workspace ?? ""}`;

  useEffect(() => {
    autoOpenStartedRef.current = false;
  }, [openTargetKey]);

  const handleOpenSystemTerminal = useCallback(async () => {
    if (!workspace || isOpening) return;
    if (environment === "ssh" && !sshServer?.trim()) {
      setOpenError("请先在执行环境中选择 SSH 服务器。");
      return;
    }
    if (environment === "sandbox" && !sessionId?.trim()) {
      setOpenError("打开容器终端需要当前会话。");
      return;
    }
    setIsOpening(true);
    setOpenError(null);
    try {
      const result = await invoke<OpenSystemTerminalResponse>(
        "open_system_terminal",
        {
          request: {
            cwd: workspace,
            executionEnvironment: environment,
            sshProfileName: sshServer,
            sandboxBackend,
            sessionId,
          },
        },
      );
      setLastOpened({ ...result, at: Date.now() });
    } catch (error) {
      setOpenError(extractErrorMessage(error));
    } finally {
      setIsOpening(false);
    }
  }, [environment, isOpening, sandboxBackend, sessionId, sshServer, workspace]);

  useEffect(() => {
    if (!canOpen || autoOpenStartedRef.current) return;
    const now = Date.now();
    const last = recentAutoOpenKeys.get(openTargetKey) ?? 0;
    if (now - last < AUTO_OPEN_DEDUP_MS) return;
    autoOpenStartedRef.current = true;
    recentAutoOpenKeys.set(openTargetKey, now);
    void handleOpenSystemTerminal();
  }, [canOpen, handleOpenSystemTerminal, openTargetKey]);

  const handleCopyWorkspace = async () => {
    if (!workspace) return;
    try {
      await navigator.clipboard.writeText(workspace);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1600);
    } catch (error) {
      setOpenError(`复制路径失败：${extractErrorMessage(error)}`);
    }
  };

  return (
    <Box
      sx={{
        height: "100%",
        minHeight: 0,
        display: "flex",
        flexDirection: "column",
        bgcolor: "background.default",
      }}
    >
      {!embedded && (
        <Box
          sx={{
            px: 2,
            py: 1,
            display: "flex",
            alignItems: "center",
            justifyContent: "space-between",
            borderBottom: 1,
            borderColor: "divider",
            bgcolor: alpha(theme.palette.background.paper, 0.86),
          }}
        >
          <Box sx={{ display: "flex", alignItems: "center", gap: 1 }}>
            <TerminalIcon fontSize="small" color="primary" />
            <Typography variant="body2" fontWeight={700}>
              系统终端
            </Typography>
          </Box>
          {workspace && (
            <Tooltip title="复制工作区路径">
              <IconButton
                size="small"
                onClick={() => void handleCopyWorkspace()}
                aria-label="复制工作区路径"
              >
                {copied ? (
                  <CheckCircleOutline fontSize="small" color="success" />
                ) : (
                  <ContentCopy fontSize="small" />
                )}
              </IconButton>
            </Tooltip>
          )}
        </Box>
      )}

      <Box
        sx={{
          flex: 1,
          minHeight: 0,
          overflow: "auto",
          p: embedded ? 2 : 2.5,
        }}
      >
        <Stack spacing={2}>
          <Stack direction="row" alignItems="center" spacing={1.25}>
            <Box
              sx={{
                width: 38,
                height: 38,
                borderRadius: 2,
                display: "grid",
                placeItems: "center",
                bgcolor: alpha(theme.palette.primary.main, 0.1),
                color: "primary.main",
                flexShrink: 0,
              }}
            >
              <TerminalIcon fontSize="small" />
            </Box>
            <Box sx={{ minWidth: 0 }}>
              <Typography variant="subtitle2" fontWeight={800} noWrap>
                Terminal
              </Typography>
              <Typography variant="caption" color="text.secondary">
                切换到系统默认终端，并连接当前执行环境。
              </Typography>
            </Box>
          </Stack>

          <Box
            sx={{
              border: 1,
              borderColor: "divider",
              borderRadius: 2,
              bgcolor: alpha(theme.palette.background.paper, 0.72),
              overflow: "hidden",
            }}
          >
            <Stack
              direction="row"
              alignItems="center"
              justifyContent="space-between"
              spacing={1}
              sx={{ px: 1.5, py: 1.25 }}
            >
              <Stack direction="row" alignItems="center" spacing={1} sx={{ minWidth: 0 }}>
                <FolderOpen
                  fontSize="small"
                  sx={{ color: workspace ? "primary.main" : "text.disabled" }}
                />
                <Box sx={{ minWidth: 0 }}>
                  <Typography variant="caption" color="text.secondary">
                    {environment === "ssh"
                      ? "远端工作区"
                      : environment === "sandbox"
                        ? "容器工作区"
                        : "工作区"}
                  </Typography>
                  <Typography
                    variant="body2"
                    fontWeight={700}
                    noWrap
                    title={workspace ?? undefined}
                    sx={{ fontFamily: "JetBrains Mono, Monaco, Consolas, monospace" }}
                  >
                    {workspaceLabel}
                  </Typography>
                </Box>
              </Stack>
              {sessionId && (
                <Chip
                  size="small"
                  label={environmentLabel}
                  sx={{ height: 22, fontSize: 11, flexShrink: 0 }}
                />
              )}
            </Stack>

            <Divider />

            <Stack spacing={1.25} sx={{ p: 1.5 }}>
              {!workspace && environment !== "sandbox" && (
                <Alert severity="warning" variant="outlined">
                  {environment === "ssh"
                    ? "请先为当前会话选择远端工作区，再打开 SSH 终端。"
                    : "请先为当前会话选择本地工作区，再打开系统终端。"}
                </Alert>
              )}

              {environment === "ssh" && !sshServer && (
                <Alert severity="warning" variant="outlined">
                  当前是 SSH 执行环境，请先选择 SSH 服务器。
                </Alert>
              )}

              {openError && (
                <Alert severity="error" variant="outlined">
                  {openError}
                </Alert>
              )}

              {lastOpened && (
                <Alert severity="success" variant="outlined">
                  已通过 {lastOpened.terminal} 打开：{lastOpened.cwd}
                  {openedAtLabel ? `（${openedAtLabel}）` : ""}
                </Alert>
              )}

              <Stack direction={{ xs: "column", sm: "row" }} spacing={1}>
                <Button
                  variant="contained"
                  disableElevation
                  startIcon={<Launch />}
                  disabled={!canOpen}
                  onClick={() => void handleOpenSystemTerminal()}
                  sx={{ textTransform: "none", fontWeight: 700 }}
                >
                  {isOpening ? "正在连接…" : "重新打开系统终端"}
                </Button>
                <Button
                  variant="outlined"
                  startIcon={
                    copied ? <CheckCircleOutline color="success" /> : <ContentCopy />
                  }
                  disabled={!workspace}
                  onClick={() => void handleCopyWorkspace()}
                  sx={{ textTransform: "none", fontWeight: 700 }}
                >
                  {copied ? "已复制" : "复制路径"}
                </Button>
              </Stack>
            </Stack>
          </Box>

          <Box
            sx={{
              borderRadius: 2,
              px: 1.5,
              py: 1.25,
              bgcolor: alpha(theme.palette.info.main, 0.06),
              border: 1,
              borderColor: alpha(theme.palette.info.main, 0.16),
            }}
          >
            <Typography variant="caption" fontWeight={800} color="info.main">
              代码分析提示
            </Typography>
            <Typography
              component="div"
              variant="caption"
              color="text.secondary"
              sx={{ mt: 0.5, lineHeight: 1.7 }}
            >
              打开后可直接运行{" "}
              <Box component="code" sx={{ fontFamily: "JetBrains Mono, monospace" }}>
                rg
              </Box>
              、{" "}
              <Box component="code" sx={{ fontFamily: "JetBrains Mono, monospace" }}>
                git status
              </Box>
              、测试命令或项目脚本；SSH/容器环境会在系统终端中自动连接到对应 shell。
            </Typography>
          </Box>
        </Stack>
      </Box>
    </Box>
  );
}
