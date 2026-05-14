import React, { useEffect, useMemo, useState } from "react";
import {
  Button,
  Typography,
  Alert,
  Box,
  Chip,
  Stack,
  CircularProgress,
} from "@mui/material";
import {
  Warning as WarningIcon,
  Error as ErrorIcon,
  CheckCircle as CheckIcon,
  Info as InfoIcon,
} from "@mui/icons-material";
import {
  usePermissionStore,
  type ToolPermissionMode,
  type RiskLevel,
} from "../../state/permissionStore";
import {
  inferConnectorPermissionIntent,
  type ConnectorPermissionIntent,
  type PermissionArgs,
} from "../../utils/connectorPermissionIntent";

type AnyArgs = PermissionArgs;
type Intent = {
  title: string;
  detail?: string;
  connector?: ConnectorPermissionIntent;
  /** 用户真正要判断的动作，优先显示在标题区。 */
  operation?: string;
  /** `detail` 的语义标签，例如运行内容/目标路径。 */
  contentLabel?: string;
};

export const PERMISSION_RUN_CONTENT_MAX_HEIGHT =
  "clamp(96px, 22vh, 220px)";
export const PERMISSION_CONNECTOR_PREVIEW_MAX_HEIGHT =
  "clamp(96px, 18vh, 180px)";
export const PERMISSION_PROMPT_ROOT_OVERFLOW_Y = "visible";
export const PERMISSION_PROMPT_ACTION_BUTTON_HEIGHT = 32;
export const PERMISSION_PROMPT_ACTION_BUTTON_FONT_SIZE = "0.8rem";

function firstString(v: unknown): string | null {
  if (typeof v === "string" && v.trim()) return v;
  return null;
}

function getPrimaryPath(args: AnyArgs): string | null {
  if (!args) return null;
  // Common shapes across tools
  const direct =
    firstString(args.path) ??
    firstString(args.filePath) ??
    firstString(args.file_path) ??
    firstString(args.targetPath) ??
    firstString(args.target_path);
  if (direct) return direct;

  const paths = args.paths;
  if (Array.isArray(paths)) {
    for (const p of paths) {
      const s = firstString(p);
      if (s) return s;
    }
  }
  return null;
}

function firstNamedString(args: AnyArgs, names: string[]): string | null {
  if (!args) return null;
  for (const name of names) {
    const s = firstString(args[name]);
    if (s) return s.trim();
  }
  return null;
}

function summarizeShellCommand(command: string): string {
  const firstLine = command.trim().split(/\r?\n/).find(Boolean) ?? "";
  if (!firstLine) return "Shell 命令";
  return firstLine.length > 72 ? `${firstLine.slice(0, 72)}…` : firstLine;
}

export function inferIntent(
  toolNameRaw: string,
  args: AnyArgs,
): Intent {
  const toolName = (toolNameRaw || "").trim();
  const connector = inferConnectorPermissionIntent(toolNameRaw, args);
  if (connector) {
    return {
      title: connector.isWrite ? "外部服务写入确认" : "外部服务访问确认",
      detail: [
        connector.connectorLabel,
        connector.operationLabel,
        connector.target,
      ]
        .filter(Boolean)
        .join(" · "),
      connector,
      operation: `${connector.connectorLabel} · ${connector.operationLabel}`,
      contentLabel: connector.isWrite ? "写入内容" : "访问内容",
    };
  }
  const path = getPrimaryPath(args);

  // Built-in Omiga tools (Rust names)
  if (toolName === "file_read" || toolName === "Read" || toolName === "fileRead") {
    return {
      title: "读取文件",
      detail: path ? path : undefined,
      operation: path ? `读取文件：${path}` : "读取文件",
      contentLabel: "目标路径",
    };
  }
  if (toolName === "file_edit" || toolName === "Edit") {
    return {
      title: "修改文件",
      detail: path ? path : undefined,
      operation: path ? `修改文件：${path}` : "修改文件",
      contentLabel: "目标路径",
    };
  }
  if (toolName === "file_write" || toolName === "Write") {
    return {
      title: "写入文件",
      detail: path ? path : undefined,
      operation: path ? `写入文件：${path}` : "写入文件",
      contentLabel: "目标路径",
    };
  }
  if (toolName === "glob" || toolName === "Glob") {
    const pattern = firstNamedString(args, ["pattern", "glob", "path"]);
    return {
      title: "查找文件/目录",
      detail: pattern ?? undefined,
      operation: pattern ? `查找：${pattern}` : "查找文件/目录",
      contentLabel: "查找范围",
    };
  }
  if (toolName === "grep" || toolName === "Grep" || toolName === "ripgrep" || toolName === "Ripgrep") {
    const pattern = firstNamedString(args, ["pattern", "query", "regex"]);
    return {
      title: "搜索内容",
      detail: pattern ?? undefined,
      operation: pattern ? `搜索：${pattern}` : "搜索内容",
      contentLabel: "搜索内容",
    };
  }
  if (toolName === "fetch" || toolName === "Fetch") {
    const url = firstNamedString(args, ["url", "uri"]);
    return {
      title: "访问网页",
      detail: url ?? undefined,
      operation: url ? `访问网页：${url}` : "访问网页",
      contentLabel: "目标 URL",
    };
  }
  if (toolName === "query" || toolName === "Query") {
    const query = firstNamedString(args, ["query", "sql"]);
    return {
      title: "查询数据库",
      detail: query ?? undefined,
      operation: "查询数据库",
      contentLabel: "查询内容",
    };
  }
  if (toolName === "search" || toolName === "Search") {
    const query = firstNamedString(args, ["query", "q", "search"]);
    return {
      title: "联网搜索",
      detail: query ?? undefined,
      operation: query ? `联网搜索：${query}` : "联网搜索",
      contentLabel: "搜索关键词",
    };
  }
  if (toolName === "bash" || toolName === "Bash") {
    const cmd = firstString(args?.command) ?? firstString(args?.cmd) ?? "";
    const cmdTrim = cmd.trim();
    const lower = cmdTrim.toLowerCase();
    const description = firstNamedString(args, [
      "description",
      "summary",
      "task",
      "title",
      "name",
    ]);
    const operation = description
      ? `运行：${description}`
      : `执行：${summarizeShellCommand(cmdTrim)}`;

    // Helper to check if command contains a destructive operation
    // Uses word boundaries to reduce false positives and bypasses
    const hasCommand = (target: string): boolean => {
      // Match at start, after pipe, after semicolon, after &&, after ||, after backtick, in $()
      const patterns = [
        `^${target}\\s`, // at start
        `\\|\\s*${target}\\s`, // after pipe
        `;\\s*${target}\\s`, // after semicolon
        `&&\\s*${target}\\s`, // after &&
        `\\|\\|\\s*${target}\\s`, // after ||
        `\\\`${target}\\s`, // in backticks
        `\\$\\(\\s*${target}\\s`, // in $()
      ];
      return patterns.some(p => new RegExp(p, "i").test(lower));
    };

    // Check for deletion operations (high risk)
    if (
      hasCommand("rm") ||
      /(^|[;|&]|\$\(|`)\s*find\s+.*-delete/.test(lower) ||
      /(^|[;|&]|\$\(|`)\s*find\s+.*-exec\s+rm/.test(lower)
    ) {
      return {
        title: "删除文件/目录",
        detail: cmdTrim || undefined,
        operation: description ? `删除/清理：${description}` : "删除文件/目录",
        contentLabel: "命令内容",
      };
    }

    // Check for move/rename operations
    if (hasCommand("mv") || /(^|[;|&]|\$\(|`)\s*rename\s/.test(lower)) {
      return {
        title: "移动/重命名文件",
        detail: cmdTrim || undefined,
        operation: description ? `移动/重命名：${description}` : "移动/重命名文件",
        contentLabel: "命令内容",
      };
    }

    // Check for copy operations
    if (hasCommand("cp") || hasCommand("scp") || hasCommand("rsync")) {
      return {
        title: "复制文件",
        detail: cmdTrim || undefined,
        operation: description ? `复制：${description}` : "复制文件",
        contentLabel: "命令内容",
      };
    }

    // Check for network operations
    if (
      hasCommand("curl") ||
      hasCommand("wget") ||
      hasCommand("fetch") ||
      hasCommand("ftp") ||
      hasCommand("ssh") ||
      hasCommand("nc") ||
      /(^|[;|&]|\$\(|`)\s*nc\s/.test(lower)
    ) {
      return {
        title: "网络/远程操作",
        detail: cmdTrim || undefined,
        operation: description ? `网络/远程：${description}` : "网络/远程操作",
        contentLabel: "命令内容",
      };
    }

    // Check for package installation
    if (
      hasCommand("npm") ||
      hasCommand("yarn") ||
      hasCommand("pnpm") ||
      hasCommand("pip") ||
      hasCommand("apt") ||
      hasCommand("brew") ||
      /(^|[;|&]|\$\(|`)\s*(apt-get|yum|dnf|pacman|apk)\s/.test(lower)
    ) {
      return {
        title: "安装/包管理操作",
        detail: cmdTrim || undefined,
        operation: description ? `安装/包管理：${description}` : "安装/包管理操作",
        contentLabel: "命令内容",
      };
    }

    return {
      title: "执行命令",
      detail: cmdTrim || undefined,
      operation,
      contentLabel: "运行内容",
    };
  }

  // MCP tools (prefix-based)
  if (toolName.startsWith("mcp__")) {
    return {
      title: "调用外部工具",
      detail: toolName,
      operation: `调用工具：${toolName.replace(/^mcp__/, "")}`,
      contentLabel: "工具名称",
    };
  }

  // Fallback
  return {
    title: "执行敏感操作",
    detail: toolName || undefined,
    operation: toolName ? `执行：${toolName}` : "执行敏感操作",
    contentLabel: "请求内容",
  };
}

const getRiskColor = (level: RiskLevel) => {
  switch (level) {
    case "safe":
      return "success";
    case "low":
      return "info";
    case "medium":
      return "warning";
    case "high":
    case "critical":
      return "error";
    default:
      return "warning";
  }
};

const getRiskIcon = (level: RiskLevel) => {
  switch (level) {
    case "safe":
      return <CheckIcon color="success" />;
    case "low":
      return <InfoIcon color="info" />;
    case "medium":
      return <WarningIcon color="warning" />;
    case "high":
    case "critical":
      return <ErrorIcon color="error" />;
    default:
      return <WarningIcon color="warning" />;
  }
};

const getRiskLabel = (level: RiskLevel) => {
  switch (level) {
    case "safe":
      return "安全";
    case "low":
      return "低风险";
    case "medium":
      return "中等风险";
    case "high":
      return "高风险";
    case "critical":
      return "严重风险";
    default:
      return "未知风险";
  }
};

type ModeChoice = "askEveryTime" | "session" | "timeWindow" | "plan";

const convertModeToBackend = (
  modeValue: ModeChoice,
  minutes: number,
): ToolPermissionMode => {
  switch (modeValue) {
    case "askEveryTime":
      return "AskEveryTime";
    case "session":
      return "Session";
    case "timeWindow":
      return { TimeWindow: { minutes } };
    case "plan":
      return "Plan";
    default:
      return "Session";
  }
};

export function permissionPromptLabels(
  connectorIntent: ConnectorPermissionIntent | undefined,
  isCritical: boolean,
  processing: boolean,
): {
  approveLabel: string;
  allowOnceLabel: string;
  sessionLabel: string;
  timeWindowLabel: string;
  connectorWarning?: string;
} {
  const isConnectorWrite = connectorIntent?.isWrite === true;
  return {
    approveLabel: processing
      ? "处理中…"
      : isConnectorWrite
        ? "允许写入"
        : connectorIntent
          ? "允许访问"
          : isCritical
            ? "运行（高风险）"
            : "运行",
    allowOnceLabel: isConnectorWrite ? "仅允许这一次写入" : "仅这次允许",
    sessionLabel: connectorIntent
      ? "本次会话内允许同一连接器操作"
      : "本次会话内允许",
    timeWindowLabel: connectorIntent
      ? "在选定时间窗口内允许同一连接器操作"
      : "在选定时间窗口内允许",
    connectorWarning: isConnectorWrite
      ? `这会修改 ${connectorIntent.connectorLabel} 中的数据。请确认账号、目标对象和内容无误；批准或拒绝都会写入连接器审计记录。`
      : undefined,
  };
}

function ScrollableCodeBlock({
  label,
  children,
  maxHeight = 320,
  minHeight,
}: {
  label: string;
  children: string;
  maxHeight?: number | string;
  minHeight?: number | string;
}) {
  return (
    <Box
      component="pre"
      tabIndex={0}
      aria-label={label}
      sx={{
        m: 0,
        px: 1.25,
        py: 1,
        borderRadius: 1.25,
        border: 1,
        borderColor: "divider",
        bgcolor: (t) =>
          t.palette.mode === "dark"
            ? "rgba(255,255,255,0.05)"
            : "rgba(0,0,0,0.035)",
        color: "text.primary",
        fontFamily:
          'ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, "Liberation Mono", monospace',
        fontSize: "0.78rem",
        lineHeight: 1.45,
        whiteSpace: "pre-wrap",
        wordBreak: "break-word",
        minHeight,
        maxHeight,
        maxWidth: "100%",
        overflowX: "hidden",
        overflowY: "auto",
        overscrollBehavior: "contain",
      }}
    >
      {children}
    </Box>
  );
}

const permissionActionButtonSx = {
  height: PERMISSION_PROMPT_ACTION_BUTTON_HEIGHT,
  minHeight: PERMISSION_PROMPT_ACTION_BUTTON_HEIGHT,
  px: 1.35,
  py: 0,
  borderRadius: 1.25,
  fontSize: PERMISSION_PROMPT_ACTION_BUTTON_FONT_SIZE,
  lineHeight: 1.2,
  "& .MuiButton-startIcon": {
    ml: -0.25,
    mr: 0.5,
  },
};

/** 内联在输入框上方，非弹窗 */
export const PermissionPromptBar: React.FC = () => {
  const { pendingRequest, approveRequest, denyRequest, error, clearError } =
    usePermissionStore();
  const [processingAction, setProcessingAction] = useState<
    ModeChoice | "deny" | null
  >(null);
  const intent = useMemo(
    () =>
      pendingRequest
        ? inferIntent(
            pendingRequest.tool_name,
            pendingRequest.arguments as AnyArgs,
          )
        : null,
    [pendingRequest],
  );

  const connectorIntent = intent?.connector;

  useEffect(() => {
    if (!pendingRequest) return;
    setProcessingAction(null);
  }, [pendingRequest?.request_id, pendingRequest?.tool_name]);

  if (!pendingRequest || !intent) return null;

  const isDangerous =
    pendingRequest.risk_level === "high" ||
    pendingRequest.risk_level === "critical";
  const isCritical = pendingRequest.risk_level === "critical";
  const isConnectorWrite = connectorIntent?.isWrite === true;
  const processing = processingAction !== null;

  const handleApprove = async (modeValue: ModeChoice) => {
    setProcessingAction(modeValue);
    clearError();
    try {
      const mode = convertModeToBackend(modeValue, 60);
      await approveRequest(mode);
    } catch {
      // store 已记录
    } finally {
      setProcessingAction(null);
    }
  };

  const handleDeny = async () => {
    setProcessingAction("deny");
    clearError();
    try {
      await denyRequest("用户拒绝");
    } catch {
      // store 已记录
    } finally {
      setProcessingAction(null);
    }
  };

  const detail = intent.detail;
  const connectorTargetLabel = connectorIntent?.target ?? "未提供目标对象";
  const connectorPreview = connectorIntent?.payloadPreview ?? null;
  const labels = permissionPromptLabels(connectorIntent, isCritical, processing);
  const actionTitle = intent.operation ?? intent.title;
  const contentLabel = intent.contentLabel ?? "请求内容";
  const allowOnceButtonLabel = isConnectorWrite
    ? "仅本次写入"
    : connectorIntent
      ? "仅本次访问"
      : "仅本次运行";

  return (
    <Box
      sx={{
        px: 2,
        py: 1.25,
        borderBottom: 1,
        borderColor: "divider",
        bgcolor: (t) =>
          t.palette.mode === "dark"
            ? "rgba(255,255,255,0.04)"
            : "rgba(0,0,0,0.02)",
        overflowY: PERMISSION_PROMPT_ROOT_OVERFLOW_Y,
      }}
    >
      <Stack spacing={1.1}>
        {/* 标题行：只放用户需要判断的具体操作，避免展示 request id / raw payload 等噪音。 */}
        <Stack direction="row" alignItems="flex-start" gap={1}>
          <Box sx={{ display: "flex", pt: 0.1 }}>
            {getRiskIcon(pendingRequest.risk_level)}
          </Box>
          <Box sx={{ minWidth: 0, flex: 1 }}>
            <Typography
              variant="caption"
              color="text.secondary"
              sx={{ display: "block", lineHeight: 1.3 }}
            >
              权限请求
            </Typography>
            <Typography
              variant="subtitle2"
              fontWeight={700}
              sx={{
                lineHeight: 1.35,
                overflow: "hidden",
                textOverflow: "ellipsis",
                display: "-webkit-box",
                WebkitLineClamp: 2,
                WebkitBoxOrient: "vertical",
              }}
            >
              {actionTitle}
            </Typography>
          </Box>
          <Chip
            label={getRiskLabel(pendingRequest.risk_level)}
            color={getRiskColor(pendingRequest.risk_level) as never}
            size="small"
            variant={isDangerous ? "filled" : "outlined"}
          />
        </Stack>

        {/* Human-readable plain description from backend (e.g. "AI wants to run: rm -rf /tmp/build") */}
        {pendingRequest.plain_description && (
          <Typography
            variant="body2"
            sx={{
              px: 0.5,
              py: 0.5,
              borderRadius: 1,
              bgcolor: (t) =>
                isDangerous
                  ? t.palette.mode === "dark"
                    ? "rgba(244,67,54,0.10)"
                    : "rgba(244,67,54,0.07)"
                  : t.palette.mode === "dark"
                    ? "rgba(255,255,255,0.06)"
                    : "rgba(0,0,0,0.04)",
              color: isDangerous ? "error.main" : "text.primary",
              fontWeight: 500,
              fontSize: "0.875rem",
              lineHeight: 1.45,
              wordBreak: "break-word",
            }}
          >
            {pendingRequest.plain_description}
          </Typography>
        )}

        {connectorIntent && (
          <Box
            sx={{
              border: 1,
              borderColor: isConnectorWrite ? "error.main" : "divider",
              borderRadius: 1.5,
              bgcolor: (t) =>
                isConnectorWrite
                  ? t.palette.mode === "dark"
                    ? "rgba(244,67,54,0.08)"
                    : "rgba(244,67,54,0.05)"
                  : "action.hover",
              p: 1,
            }}
          >
            <Stack spacing={0.75}>
              <Stack direction="row" spacing={0.75} flexWrap="wrap" useFlexGap>
                <Chip
                  size="small"
                  label={connectorIntent.connectorLabel}
                  color={isConnectorWrite ? "error" : "default"}
                  variant={isConnectorWrite ? "filled" : "outlined"}
                />
                <Chip
                  size="small"
                  label={connectorIntent.operationLabel}
                  variant="outlined"
                />
                <Chip
                  size="small"
                  label={isConnectorWrite ? "会修改外部数据" : "只读访问"}
                  color={isConnectorWrite ? "error" : "info"}
                  variant="outlined"
                />
              </Stack>

              <Box>
                <Typography variant="caption" color="text.secondary">
                  目标对象
                </Typography>
                <Box sx={{ mt: 0.25 }}>
                  <ScrollableCodeBlock label="目标对象" maxHeight={96}>
                    {connectorTargetLabel}
                  </ScrollableCodeBlock>
                </Box>
              </Box>

              {connectorPreview && (
                <Box>
                  <Typography variant="caption" color="text.secondary">
                    内容预览
                  </Typography>
                  <Box sx={{ mt: 0.25 }}>
                    <ScrollableCodeBlock
                      label="内容预览"
                      maxHeight={PERMISSION_CONNECTOR_PREVIEW_MAX_HEIGHT}
                    >
                      {connectorPreview}
                    </ScrollableCodeBlock>
                  </Box>
                </Box>
              )}
            </Stack>
          </Box>
        )}

        {/* 具体操作内容：始终放入独立滚动区域，长命令/脚本不再撑高权限卡片。 */}
        {!connectorIntent && detail && (
          <Box>
            <Typography variant="caption" color="text.secondary">
              {contentLabel}
            </Typography>
            <Box sx={{ mt: 0.25 }}>
              <ScrollableCodeBlock
                label={contentLabel}
                minHeight={96}
                maxHeight={PERMISSION_RUN_CONTENT_MAX_HEIGHT}
              >
                {detail}
              </ScrollableCodeBlock>
            </Box>
          </Box>
        )}

        {error && (
          <Alert severity="error" onClose={clearError} sx={{ py: 0 }}>
            {error}
          </Alert>
        )}

        {isConnectorWrite ? (
          <Alert severity="error" sx={{ py: 0.5 }}>
            {labels.connectorWarning}
          </Alert>
        ) : null}

        <Stack
          direction="row"
          justifyContent="flex-end"
          alignItems="center"
          sx={{ flexShrink: 0 }}
        >
          <Stack direction="row" justifyContent="flex-end" spacing={1}>
            <Button
              size="small"
              onClick={handleDeny}
              color="inherit"
              variant="outlined"
              disabled={processing}
              sx={permissionActionButtonSx}
              startIcon={
                processingAction === "deny" ? (
                  <CircularProgress size={14} color="inherit" />
                ) : null
              }
            >
              拒绝
            </Button>
            <Button
              size="small"
              onClick={() => void handleApprove("session")}
              color={isDangerous ? "error" : "primary"}
              variant="outlined"
              disabled={processing}
              sx={permissionActionButtonSx}
              startIcon={
                processingAction === "session" ? (
                  <CircularProgress size={14} color="inherit" />
                ) : null
              }
            >
              本会话允许
            </Button>
            <Button
              size="small"
              onClick={() => void handleApprove("askEveryTime")}
              color={isDangerous ? "error" : "primary"}
              variant="contained"
              disabled={processing}
              sx={permissionActionButtonSx}
              startIcon={
                processingAction === "askEveryTime" ? (
                  <CircularProgress size={14} color="inherit" />
                ) : null
              }
            >
              {processingAction === "askEveryTime"
                ? "处理中…"
                : allowOnceButtonLabel}
            </Button>
          </Stack>
        </Stack>
      </Stack>
    </Box>
  );
};
