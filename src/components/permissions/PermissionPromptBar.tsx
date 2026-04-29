import React, { useState } from "react";
import {
  Button,
  Typography,
  Alert,
  Box,
  Chip,
  Divider,
  FormControl,
  FormControlLabel,
  FormLabel,
  Radio,
  RadioGroup,
  Stack,
  Accordion,
  AccordionSummary,
  AccordionDetails,
  CircularProgress,
} from "@mui/material";
import {
  Warning as WarningIcon,
  Error as ErrorIcon,
  CheckCircle as CheckIcon,
  Info as InfoIcon,
  ExpandMore as ExpandMoreIcon,
} from "@mui/icons-material";
import {
  usePermissionStore,
  type ToolPermissionMode,
  type RiskLevel,
} from "../../state/permissionStore";

type AnyArgs = Record<string, unknown> | undefined;

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

function inferIntent(toolNameRaw: string, args: AnyArgs): { title: string; detail?: string } {
  const toolName = (toolNameRaw || "").trim();
  const path = getPrimaryPath(args);

  // Built-in Omiga tools (Rust names)
  if (toolName === "file_read" || toolName === "Read" || toolName === "fileRead") {
    return {
      title: "读取文件",
      detail: path ? path : undefined,
    };
  }
  if (toolName === "file_edit" || toolName === "Edit") {
    return {
      title: "修改文件",
      detail: path ? path : undefined,
    };
  }
  if (toolName === "file_write" || toolName === "Write") {
    return {
      title: "写入文件",
      detail: path ? path : undefined,
    };
  }
  if (toolName === "glob" || toolName === "Glob") {
    return { title: "查找文件/目录" };
  }
  if (toolName === "grep" || toolName === "Grep" || toolName === "ripgrep" || toolName === "Ripgrep") {
    return { title: "搜索内容" };
  }
  if (toolName === "fetch" || toolName === "Fetch") {
    return { title: "访问网页" };
  }
  if (toolName === "search" || toolName === "Search") {
    return { title: "联网搜索" };
  }
  if (toolName === "bash" || toolName === "Bash") {
    const cmd = firstString(args?.command) ?? firstString(args?.cmd) ?? "";
    const cmdTrim = cmd.trim();
    const lower = cmdTrim.toLowerCase();

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
      return { title: "删除文件/目录", detail: cmdTrim || undefined };
    }

    // Check for move/rename operations
    if (hasCommand("mv") || /(^|[;|&]|\$\(|`)\s*rename\s/.test(lower)) {
      return { title: "移动/重命名文件", detail: cmdTrim || undefined };
    }

    // Check for copy operations
    if (hasCommand("cp") || hasCommand("scp") || hasCommand("rsync")) {
      return { title: "复制文件", detail: cmdTrim || undefined };
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
      return { title: "网络/远程操作", detail: cmdTrim || undefined };
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
      return { title: "安装/包管理操作", detail: cmdTrim || undefined };
    }

    return { title: "执行命令", detail: cmdTrim || undefined };
  }

  // MCP tools (prefix-based)
  if (toolName.startsWith("mcp__")) {
    return { title: "调用外部工具（MCP）", detail: toolName };
  }

  // Fallback
  return { title: "执行敏感操作", detail: toolName || undefined };
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

/** 内联在输入框上方，非弹窗 */
export const PermissionPromptBar: React.FC = () => {
  const { pendingRequest, approveRequest, denyRequest, error, clearError } =
    usePermissionStore();
  const [modeValue, setModeValue] = useState<ModeChoice>("session");
  const [timeWindowMinutes, setTimeWindowMinutes] = useState<number>(60);
  const [showDetails, setShowDetails] = useState(false);
  const [processing, setProcessing] = useState(false);
  const [cmdExpanded, setCmdExpanded] = useState(false);

  if (!pendingRequest) return null;

  const isDangerous =
    pendingRequest.risk_level === "high" ||
    pendingRequest.risk_level === "critical";
  const isCritical = pendingRequest.risk_level === "critical";
  const intent = inferIntent(
    pendingRequest.tool_name,
    pendingRequest.arguments as AnyArgs,
  );

  const handleApprove = async () => {
    setProcessing(true);
    clearError();
    try {
      const mode = convertModeToBackend(modeValue, timeWindowMinutes);
      await approveRequest(mode);
    } catch {
      // store 已记录
    } finally {
      setProcessing(false);
    }
  };

  const handleDeny = async () => {
    setProcessing(true);
    clearError();
    try {
      await denyRequest("用户拒绝");
    } catch {
      // store 已记录
    } finally {
      setProcessing(false);
    }
  };

  const detail = intent.detail ?? pendingRequest.tool_name;
  const DETAIL_TRUNCATE = 120;
  const detailTruncated = detail.length > DETAIL_TRUNCATE
    ? detail.slice(0, DETAIL_TRUNCATE) + "…"
    : detail;
  const hasLongDetail = detail.length > DETAIL_TRUNCATE;

  return (
    <Box
      sx={{
        px: 2,
        py: 1.5,
        borderBottom: 1,
        borderColor: "divider",
        bgcolor: (t) =>
          t.palette.mode === "dark"
            ? "rgba(255,255,255,0.04)"
            : "rgba(0,0,0,0.02)",
        maxHeight: "50vh",
        overflowY: "auto",
      }}
    >
      <Stack spacing={1}>
        {/* 标题行：只显示操作类型 + 风险等级 */}
        <Stack direction="row" alignItems="center" gap={1}>
          {getRiskIcon(pendingRequest.risk_level)}
          <Typography variant="subtitle2" fontWeight={700} sx={{ flex: 1 }}>
            {intent.title}
          </Typography>
          <Chip
            label={getRiskLabel(pendingRequest.risk_level)}
            color={getRiskColor(pendingRequest.risk_level) as never}
            size="small"
          />
        </Stack>

        {/* 具体操作内容：路径或命令，限制展示 */}
        {detail && (
          <Box
            component="code"
            sx={{
              display: "block",
              px: 1,
              py: 0.5,
              borderRadius: 1,
              bgcolor: "action.hover",
              fontSize: "0.78rem",
              whiteSpace: "pre-wrap",
              wordBreak: "break-all",
              cursor: hasLongDetail ? "pointer" : "default",
              maxHeight: cmdExpanded ? 160 : "none",
              overflowY: cmdExpanded ? "auto" : "visible",
            }}
            onClick={hasLongDetail ? () => setCmdExpanded((v) => !v) : undefined}
            title={hasLongDetail && !cmdExpanded ? "点击展开完整内容" : undefined}
          >
            {cmdExpanded ? detail : detailTruncated}
          </Box>
        )}

        {error && (
          <Alert severity="error" onClose={clearError} sx={{ py: 0 }}>
            {error}
          </Alert>
        )}

        {isDangerous && (
          <Alert severity={isCritical ? "error" : "warning"} sx={{ py: 0.5 }}>
            {isCritical
              ? "此操作可能导致数据丢失，请格外谨慎！"
              : "高风险操作，请确认您了解其后果。"}
          </Alert>
        )}

        {pendingRequest.detected_risks.length > 0 && (
          <Accordion
            expanded={showDetails}
            onChange={() => setShowDetails(!showDetails)}
            variant="outlined"
            disableGutters
            sx={{ "&:before": { display: "none" } }}
          >
            <AccordionSummary expandIcon={<ExpandMoreIcon />} sx={{ minHeight: 32, "& .MuiAccordionSummary-content": { my: 0.5 } }}>
              <Typography variant="caption">
                {pendingRequest.detected_risks.length} 个风险点
              </Typography>
            </AccordionSummary>
            <AccordionDetails sx={{ pt: 0.5 }}>
              <Stack spacing={0.75}>
                {pendingRequest.detected_risks.map((risk, idx) => (
                  <Alert
                    key={idx}
                    severity={getRiskColor(risk.severity) as never}
                    variant="outlined"
                    sx={{ py: 0.25 }}
                  >
                    <Typography variant="caption" fontWeight={600} display="block">
                      {risk.category}
                    </Typography>
                    <Typography variant="caption">{risk.description}</Typography>
                  </Alert>
                ))}
              </Stack>
            </AccordionDetails>
          </Accordion>
        )}

        <Divider flexItem />

        <FormControl component="fieldset" variant="standard" disabled={processing}>
          <FormLabel component="legend" sx={{ typography: "caption", mb: 0.5 }}>
            记住我的选择
          </FormLabel>
          <RadioGroup
            value={modeValue}
            onChange={(e) => setModeValue(e.target.value as ModeChoice)}
          >
            <FormControlLabel
              value="askEveryTime"
              control={<Radio size="small" />}
              label="仅这次允许"
            />
            <FormControlLabel
              value="session"
              control={<Radio size="small" />}
              label="本次会话内允许"
            />
            <FormControlLabel
              value="timeWindow"
              control={<Radio size="small" />}
              label="在选定时间窗口内允许"
            />
            <FormControlLabel
              value="plan"
              control={<Radio size="small" />}
              label="Plan 模式（批量确认）"
            />
          </RadioGroup>
        </FormControl>

        {modeValue === "timeWindow" && (
          <FormControl component="fieldset" variant="standard" disabled={processing}>
            <FormLabel component="legend" sx={{ typography: "caption", mb: 0.5 }}>
              时长
            </FormLabel>
            <RadioGroup
              row
              value={String(timeWindowMinutes)}
              onChange={(e) => setTimeWindowMinutes(Number(e.target.value))}
            >
              <FormControlLabel
                value="60"
                control={<Radio size="small" />}
                label="1 小时"
              />
              <FormControlLabel
                value="240"
                control={<Radio size="small" />}
                label="4 小时"
              />
              <FormControlLabel
                value="1440"
                control={<Radio size="small" />}
                label="24 小时"
              />
            </RadioGroup>
          </FormControl>
        )}

        <Stack direction="row" justifyContent="flex-end" spacing={1}>
          <Button
            size="small"
            onClick={handleDeny}
            color="inherit"
            variant="outlined"
            disabled={processing}
          >
            拒绝
          </Button>
          <Button
            size="small"
            onClick={handleApprove}
            color={isDangerous ? "error" : "primary"}
            variant="contained"
            disabled={processing}
            startIcon={
              processing ? <CircularProgress size={14} color="inherit" /> : null
            }
          >
            {processing ? "处理中…" : isCritical ? "运行（高风险）" : "运行"}
          </Button>
        </Stack>
      </Stack>
    </Box>
  );
};
