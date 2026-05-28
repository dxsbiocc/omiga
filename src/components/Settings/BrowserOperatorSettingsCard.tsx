import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  Alert,
  Box,
  Button,
  Chip,
  CircularProgress,
  Divider,
  Stack,
  Typography,
} from "@mui/material";
import {
  CheckCircle,
  Error as ErrorIcon,
  Refresh,
  SettingsInputComponent,
} from "@mui/icons-material";
import { useSessionStore } from "../../state/sessionStore";
import {
  browserOperatorErrorMessage,
  isBrowserOperatorBackendReady,
  type BrowserOperatorBackendStatus,
  type BrowserOperatorInstallIntent,
  type BrowserOperatorInstallResult,
} from "../../lib/browserOperator";

interface BrowserOperatorSettingsCardProps {
  compact?: boolean;
}

type BrowserOperatorStatusSeverity = "success" | "info" | "warning" | "error";

function nonEmpty(value: unknown): string | null {
  if (typeof value !== "string") return null;
  const trimmed = value.trim();
  return trimmed ? trimmed : null;
}

function browserOperatorStatusSeverity(
  status: BrowserOperatorBackendStatus | null,
  error: string | null,
): BrowserOperatorStatusSeverity {
  if (error || status?.sidecarExists === false) return "error";
  if (!status) return "info";
  if (isBrowserOperatorBackendReady(status)) return "success";
  if (status.installerExists === false) return "error";
  return "warning";
}

function browserOperatorStatusLabel(
  status: BrowserOperatorBackendStatus | null,
  error: string | null,
): string {
  if (error) return "状态检测失败";
  if (!status) return "正在检测";
  if (status.sidecarExists === false) return "内置 sidecar 缺失";
  if (isBrowserOperatorBackendReady(status)) return "后端已就绪";
  if (status.installerExists === false) return "安装脚本缺失";
  return "需要安装";
}

function browserOperatorInstallSummary(
  result: BrowserOperatorInstallResult | null,
): string | null {
  if (!result) return null;
  if (result.ok === false) {
    return result.error || "安装命令返回失败。";
  }
  const home = nonEmpty(result.home);
  const python = nonEmpty(result.python);
  if (home && python) return `安装完成：${python} · ${home}`;
  if (home) return `安装完成：${home}`;
  if (python) return `安装完成：${python}`;
  return "安装命令已完成。";
}

export function BrowserOperatorSettingsCard({
  compact = false,
}: BrowserOperatorSettingsCardProps) {
  const sessionId = useSessionStore((s) => s.currentSession?.id ?? null);
  const projectRoot = useSessionStore((s) => s.currentSession?.projectPath ?? ".");
  const [status, setStatus] = useState<BrowserOperatorBackendStatus | null>(null);
  const [statusLoading, setStatusLoading] = useState(true);
  const [installIntent, setInstallIntent] =
    useState<BrowserOperatorInstallIntent | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [installResult, setInstallResult] =
    useState<BrowserOperatorInstallResult | null>(null);

  const loadStatus = useCallback(async () => {
    setStatusLoading(true);
    setError(null);
    try {
      const next = await invoke<BrowserOperatorBackendStatus>(
        "browser_operator_backend_status",
      );
      setStatus(next);
      return next;
    } catch (err) {
      setStatus(null);
      setError(browserOperatorErrorMessage(err));
      return null;
    } finally {
      setStatusLoading(false);
    }
  }, []);

  useEffect(() => {
    void loadStatus();
  }, [loadStatus]);

  const severity = browserOperatorStatusSeverity(status, error);
  const ready = isBrowserOperatorBackendReady(status);
  const installBlocked =
    statusLoading ||
    installIntent !== null ||
    !status ||
    status?.sidecarExists === false ||
    status?.installerExists === false;

  const managedHome = nonEmpty(status?.managedHome);
  const selectedPython = nonEmpty(status?.selectedPython);
  const configuredPython = nonEmpty(status?.configuredPython);
  const playwrightBrowsersPath = nonEmpty(status?.playwrightBrowsersPath);
  const installCommand = nonEmpty(status?.installCommand);
  const installSummary = useMemo(
    () => browserOperatorInstallSummary(installResult),
    [installResult],
  );

  const handleInstall = useCallback(
    async (intent: BrowserOperatorInstallIntent) => {
      const fullInstall = intent === "full";
      const confirmationText = fullInstall
        ? "确认执行 Browser Operator 完整安装？这会写入用户目录，并可能联网下载 Playwright 浏览器。"
        : "确认只安装 Browser Operator Python 后端？这会写入用户目录，但跳过 Playwright 浏览器下载。";
      if (
        typeof confirm === "function" &&
        !confirm(confirmationText)
      ) {
        return;
      }

      setInstallIntent(intent);
      setError(null);
      setInstallResult(null);
      try {
        const result = await invoke<BrowserOperatorInstallResult>(
          "browser_operator_install_backend",
          {
            confirmInstallIntent: true,
            skipBrowserInstall: intent === "packages-only",
            projectRoot: projectRoot?.trim() ? projectRoot : undefined,
            sessionId: sessionId ?? undefined,
          },
        );
        if (result?.ok === false) {
          throw new Error(result.error || "Browser Operator 后端安装失败。");
        }
        setInstallResult(result);

        const nextStatus = await loadStatus();
        if (nextStatus?.sidecarExists === false) {
          throw new Error("安装完成，但内置 Browser Operator sidecar 缺失，无法启用。");
        }
        if (nextStatus && !isBrowserOperatorBackendReady(nextStatus)) {
          throw new Error("安装命令已完成，但仍未检测到 Browser Operator Python 后端。");
        }
      } catch (err) {
        setError(browserOperatorErrorMessage(err));
      } finally {
        setInstallIntent(null);
      }
    },
    [loadStatus, projectRoot, sessionId],
  );

  return (
    <Alert
      severity={severity}
      icon={ready ? <CheckCircle fontSize="small" /> : <SettingsInputComponent fontSize="small" />}
      sx={{ mb: compact ? 1.5 : 2, borderRadius: 2 }}
    >
      <Stack spacing={1.25} useFlexGap>
        <Stack
          direction={{ xs: "column", sm: "row" }}
          spacing={1}
          alignItems={{ xs: "flex-start", sm: "center" }}
          justifyContent="space-between"
        >
          <Box>
            <Stack direction="row" spacing={1} alignItems="center" flexWrap="wrap">
              <Typography variant="subtitle2" fontWeight={700}>
                Browser Operator
              </Typography>
              <Chip
                size="small"
                color={severity === "success" ? "success" : severity === "error" ? "error" : "warning"}
                variant={ready ? "filled" : "outlined"}
                label={browserOperatorStatusLabel(status, error)}
              />
            </Stack>
            <Typography variant="body2" color="text.secondary" sx={{ mt: 0.25 }}>
              管理 browser_* 工具所需的本机 Python 后端；Composer 仍负责按本次/会话开启浏览器控制。
            </Typography>
          </Box>
          <Button
            size="small"
            startIcon={
              statusLoading ? (
                <CircularProgress size={14} color="inherit" />
              ) : (
                <Refresh fontSize="small" />
              )
            }
            onClick={() => void loadStatus()}
            disabled={statusLoading || installIntent !== null}
            sx={{ textTransform: "none" }}
          >
            重新检测
          </Button>
        </Stack>

        {error ? (
          <Alert severity="error" icon={<ErrorIcon fontSize="small" />} sx={{ borderRadius: 1.5 }}>
            {error}
          </Alert>
        ) : null}

        {installSummary ? (
          <Typography variant="body2" fontWeight={600}>
            {installSummary}
          </Typography>
        ) : null}

        <Stack direction="row" gap={0.75} flexWrap="wrap">
          <Chip
            size="small"
            variant="outlined"
            label={`sidecar: ${status?.sidecarExists === false ? "missing" : "bundled"}`}
          />
          <Chip
            size="small"
            variant="outlined"
            label={`installer: ${status?.installerExists === false ? "missing" : "available"}`}
          />
          <Chip
            size="small"
            variant="outlined"
            label={`backend: ${ready ? "ready" : "not ready"}`}
          />
        </Stack>

        <Stack spacing={0.35}>
          {configuredPython ? (
            <Typography variant="caption" color="text.secondary">
              外部 Python：{configuredPython}
            </Typography>
          ) : null}
          {selectedPython ? (
            <Typography variant="caption" color="text.secondary">
              当前 Python：{selectedPython}
            </Typography>
          ) : null}
          {managedHome ? (
            <Typography variant="caption" color="text.secondary">
              管理目录：{managedHome}
            </Typography>
          ) : null}
          {playwrightBrowsersPath ? (
            <Typography variant="caption" color="text.secondary">
              Playwright 浏览器缓存：{playwrightBrowsersPath}
            </Typography>
          ) : null}
          {installCommand ? (
            <Typography variant="caption" color="text.secondary">
              安装命令：{installCommand}
            </Typography>
          ) : null}
        </Stack>

        <Divider />

        <Stack direction="row" gap={1} flexWrap="wrap" alignItems="center">
          <Button
            size="small"
            variant="outlined"
            onClick={() => void handleInstall("packages-only")}
            disabled={installBlocked}
            startIcon={
              installIntent === "packages-only" ? (
                <CircularProgress size={14} color="inherit" />
              ) : undefined
            }
            sx={{ textTransform: "none" }}
          >
            只安装后端
          </Button>
          <Button
            size="small"
            variant="contained"
            onClick={() => void handleInstall("full")}
            disabled={installBlocked}
            startIcon={
              installIntent === "full" ? (
                <CircularProgress size={14} color="inherit" />
              ) : undefined
            }
            sx={{ textTransform: "none" }}
          >
            安装并下载浏览器
          </Button>
          <Typography variant="caption" color="text.secondary">
            两个按钮都会写入用户目录；完整安装可能联网下载浏览器。
          </Typography>
        </Stack>
      </Stack>
    </Alert>
  );
}
