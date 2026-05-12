import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  Alert,
  Box,
  Button,
  Card,
  Chip,
  Divider,
  FormControlLabel,
  Stack,
  Switch,
  TextField,
  Typography,
} from "@mui/material";
import { DeleteOutline, Refresh, Save } from "@mui/icons-material";

type ComputerUseSettingsTabProps = {
  projectPath: string;
  onOpenPlugins?: () => void;
};

type ComputerUseSettingsPanelProps = ComputerUseSettingsTabProps & {
  showIntro?: boolean;
  showPluginButton?: boolean;
};

type ComputerUseSettings = {
  allowedApps: string[];
  logRetentionDays: number;
  saveScreenshots: boolean;
};

type ComputerUseAuditSummary = {
  auditRoot: string;
  runsRoot: string;
  runCount: number;
  flowCount: number;
  actionCount: number;
  resultOkCount: number;
  resultBlockedCount: number;
  resultNeedsAttentionCount: number;
  resultStoppedCount: number;
  resultUnknownCount: number;
  bytes: number;
  retentionDays?: number | null;
  prunedRunCount: number;
  prunedTempDirCount: number;
  prunedBytes: number;
};

type ComputerUsePermissionStatus = {
  platform: string;
  supported: boolean;
  accessibility: string;
  screenRecording: string;
  message: string;
};

type ComputerUseBackendStatus = {
  platform: string;
  runtime: string;
  wrapperPath: string;
  wrapperInstalled: boolean;
  wrapperExecutable: boolean;
  pythonBackendPath: string;
  pythonBackendInstalled: boolean;
  pythonBackendExecutable: boolean;
  message: string;
};

const SETTINGS_KEY = "omiga.computer_use.settings.v1";

const DEFAULT_SETTINGS: ComputerUseSettings = {
  allowedApps: ["Omiga", "com.omiga.desktop"],
  logRetentionDays: 14,
  saveScreenshots: false,
};

function parseSettings(raw: string | null): ComputerUseSettings {
  if (!raw) return DEFAULT_SETTINGS;
  try {
    const parsed = JSON.parse(raw) as Partial<ComputerUseSettings>;
    return {
      allowedApps: Array.isArray(parsed.allowedApps)
        ? parsed.allowedApps.map(String).filter(Boolean)
        : DEFAULT_SETTINGS.allowedApps,
      logRetentionDays:
        typeof parsed.logRetentionDays === "number" &&
        Number.isFinite(parsed.logRetentionDays)
          ? Math.max(1, Math.min(365, Math.floor(parsed.logRetentionDays)))
          : DEFAULT_SETTINGS.logRetentionDays,
      saveScreenshots:
        typeof parsed.saveScreenshots === "boolean"
          ? parsed.saveScreenshots
          : DEFAULT_SETTINGS.saveScreenshots,
    };
  } catch {
    return DEFAULT_SETTINGS;
  }
}

function appsTextToList(text: string): string[] {
  const apps = text
    .split(/\r?\n|,/u)
    .map((item) => item.trim())
    .filter(Boolean);
  return Array.from(new Set(apps));
}

function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

function statusColor(status: string): "success" | "warning" | "default" {
  if (status === "granted") return "success";
  if (status === "blocked" || status === "unsupported") return "warning";
  return "default";
}

export function ComputerUseSettingsPanel({
  projectPath,
  onOpenPlugins,
  showIntro = true,
  showPluginButton = Boolean(onOpenPlugins),
}: ComputerUseSettingsPanelProps) {
  const [settings, setSettings] =
    useState<ComputerUseSettings>(DEFAULT_SETTINGS);
  const [allowedAppsText, setAllowedAppsText] = useState(
    DEFAULT_SETTINGS.allowedApps.join("\n"),
  );
  const [audit, setAudit] = useState<ComputerUseAuditSummary | null>(null);
  const [permissionStatus, setPermissionStatus] =
    useState<ComputerUsePermissionStatus | null>(null);
  const [backendStatus, setBackendStatus] =
    useState<ComputerUseBackendStatus | null>(null);
  const [loading, setLoading] = useState(false);
  const [permissionLoading, setPermissionLoading] = useState(false);
  const [backendLoading, setBackendLoading] = useState(false);
  const [message, setMessage] = useState<{
    type: "success" | "error" | "info";
    text: string;
  } | null>(null);

  const hasProject = projectPath.trim().length > 0 && projectPath.trim() !== ".";
  const isMacPlatform =
    typeof navigator !== "undefined" &&
    navigator.platform.toLowerCase().includes("mac");
  const normalizedSettings = useMemo(
    () => ({ ...settings, allowedApps: appsTextToList(allowedAppsText) }),
    [allowedAppsText, settings],
  );

  const loadAudit = useCallback(async (retentionDays?: number) => {
    if (!hasProject) {
      setAudit(null);
      return;
    }
    const summary = await invoke<ComputerUseAuditSummary>(
      "computer_use_audit_summary",
      { projectRoot: projectPath, retentionDays },
    );
    setAudit(summary);
  }, [hasProject, projectPath]);

  const loadBackendStatus = useCallback(async (showMessage = false) => {
    setBackendLoading(true);
    if (showMessage) setMessage(null);
    try {
      const status = await invoke<ComputerUseBackendStatus>(
        "computer_use_backend_status",
      );
      setBackendStatus(status);
      if (showMessage) {
        setMessage({
          type: "info",
          text: "已刷新 Computer Use backend 状态；该操作不会触发权限探测或启动后端进程。",
        });
      }
    } catch (error) {
      setBackendStatus(null);
      if (showMessage) {
        setMessage({
          type: "error",
          text: `刷新 Computer Use backend 状态失败：${String(error)}`,
        });
      }
    } finally {
      setBackendLoading(false);
    }
  }, []);

  const load = useCallback(async () => {
    setLoading(true);
    setMessage(null);
    try {
      const raw = await invoke<string | null>("get_setting", {
        key: SETTINGS_KEY,
      });
      const next = parseSettings(raw);
      setSettings(next);
      setAllowedAppsText(next.allowedApps.join("\n"));
      await loadAudit(next.logRetentionDays);
      await loadBackendStatus(false);
    } catch (error) {
      setMessage({
        type: "error",
        text: `加载 Computer Use 设置失败：${String(error)}`,
      });
    } finally {
      setLoading(false);
    }
  }, [loadAudit, loadBackendStatus]);

  useEffect(() => {
    void load();
  }, [load]);

  const checkPermissionStatus = useCallback(async () => {
    setPermissionLoading(true);
    setMessage(null);
    try {
      const status = await invoke<ComputerUsePermissionStatus>(
        "computer_use_permission_status",
      );
      setPermissionStatus(status);
      setMessage({
        type: "info",
        text: "已完成 Computer Use 权限检测。Screen Recording 检测只在你点击检测时运行。",
      });
    } catch (error) {
      setPermissionStatus(null);
      setMessage({
        type: "error",
        text: `检测 Computer Use 权限失败：${String(error)}`,
      });
    } finally {
      setPermissionLoading(false);
    }
  }, []);

  const saveSettings = async () => {
    setLoading(true);
    setMessage(null);
    try {
      await invoke("set_setting", {
        key: SETTINGS_KEY,
        value: JSON.stringify(normalizedSettings),
      });
      setSettings(normalizedSettings);
      setAllowedAppsText(normalizedSettings.allowedApps.join("\n"));
      setMessage({ type: "success", text: "Computer Use 设置已保存。" });
    } catch (error) {
      setMessage({
        type: "error",
        text: `保存 Computer Use 设置失败：${String(error)}`,
      });
    } finally {
      setLoading(false);
    }
  };

  const clearAudit = async () => {
    if (!hasProject) return;
    setLoading(true);
    setMessage(null);
    try {
      const cleared = await invoke<ComputerUseAuditSummary>(
        "computer_use_clear_audit",
        { projectRoot: projectPath },
      );
      await loadAudit(normalizedSettings.logRetentionDays);
      setMessage({
        type: "success",
        text: `已清理 ${cleared.runCount} 个 Computer Use 结果留痕。`,
      });
    } catch (error) {
      setMessage({
        type: "error",
        text: `清理 Computer Use 运行记录失败：${String(error)}`,
      });
    } finally {
      setLoading(false);
    }
  };

  return (
    <Stack spacing={2.25}>
      {showIntro && (
        <Alert severity="info" sx={{ borderRadius: 2 }}>
          Computer Use 是可选本机自动化扩展。插件安装/启用只代表能力可用；每条任务仍需要在聊天输入区显式开启
          Task 或 Session 模式。
        </Alert>
      )}

      {message && (
        <Alert severity={message.type} sx={{ borderRadius: 2 }}>
          {message.text}
        </Alert>
      )}

      <Card variant="outlined" sx={{ p: 2, borderRadius: 2.5 }}>
        <Stack spacing={1.5}>
          <Stack direction="row" justifyContent="space-between" spacing={1}>
            <Box>
              <Typography variant="subtitle1" fontWeight={700}>
                插件与平台状态
              </Typography>
              <Typography variant="body2" color="text.secondary">
                第一版后端目标是 macOS；真实操作仍由可选
                <Typography component="span" fontFamily="monospace" fontSize="0.85em">
                  {" "}
                  computer-use{" "}
                </Typography>
                插件提供。
              </Typography>
            </Box>
            <Chip
              label={isMacPlatform ? "macOS target" : "unsupported target"}
              color={isMacPlatform ? "success" : "warning"}
              variant="outlined"
              sx={{ flexShrink: 0 }}
            />
          </Stack>
          <Stack direction="row" spacing={1}>
            {showPluginButton && onOpenPlugins && (
              <Button variant="outlined" onClick={onOpenPlugins}>
                打开插件设置
              </Button>
            )}
            <Button
              variant="text"
              startIcon={<Refresh />}
              disabled={loading}
              onClick={() => void load()}
            >
              刷新设置/记录
            </Button>
            <Button
              variant="text"
              startIcon={<Refresh />}
              disabled={permissionLoading}
              onClick={() => void checkPermissionStatus()}
            >
              检测权限
            </Button>
            <Button
              variant="text"
              startIcon={<Refresh />}
              disabled={backendLoading}
              onClick={() => void loadBackendStatus(true)}
            >
              刷新后端状态
            </Button>
          </Stack>
          <Stack spacing={1}>
            <Typography variant="subtitle2" fontWeight={700}>
              Backend 诊断
            </Typography>
            <Stack direction="row" spacing={1} flexWrap="wrap" useFlexGap>
              <Chip
                label="Runtime: Python"
                color="success"
                variant="outlined"
              />
              <Chip
                label={`Wrapper: ${
                  backendStatus?.wrapperExecutable
                    ? "ready"
                    : backendStatus?.wrapperInstalled
                      ? "not executable"
                      : "missing"
                }`}
                variant="outlined"
              />
              <Chip
                label={`Python backend: ${
                  backendStatus?.pythonBackendExecutable
                    ? "executable"
                    : backendStatus?.pythonBackendInstalled
                      ? "not executable"
                      : "missing"
                }`}
                variant="outlined"
              />
            </Stack>
            {backendStatus?.message && (
              <Typography variant="caption" color="text.secondary">
                {backendStatus.message}
              </Typography>
            )}
            <Typography
              component="code"
              sx={{
                display: "block",
                p: 1,
                borderRadius: 1,
                bgcolor: "action.hover",
                fontSize: 12,
                overflowWrap: "anywhere",
              }}
            >
              Python backend:{" "}
              {backendStatus?.pythonBackendPath ??
                "src-tauri/bundled_plugins/plugins/computer-use/bin/computer-use-macos.py"}
            </Typography>
          </Stack>
          <Divider />
          <Stack direction="row" spacing={1} flexWrap="wrap" useFlexGap>
            <Chip
              label={`Accessibility: ${permissionStatus?.accessibility ?? "unknown"}`}
              color={statusColor(permissionStatus?.accessibility ?? "unknown")}
              variant="outlined"
            />
            <Chip
              label={`Screen Recording: ${permissionStatus?.screenRecording ?? "unknown"}`}
              color={statusColor(permissionStatus?.screenRecording ?? "unknown")}
              variant="outlined"
            />
          </Stack>
          {permissionStatus?.message && (
            <Typography variant="caption" color="text.secondary">
              {permissionStatus.message}
            </Typography>
          )}
          <Typography variant="caption" color="text.secondary">
            为避免后台窥屏，Settings 不会自动检测 Screen Recording；只有点击“检测权限”时才会执行一次系统权限探测，临时截图文件会立即删除。
          </Typography>
        </Stack>
      </Card>

      <Card variant="outlined" sx={{ p: 2, borderRadius: 2.5 }}>
        <Stack spacing={1.5}>
          <Typography variant="subtitle1" fontWeight={700}>
            安全策略
          </Typography>
          <TextField
            label="Allowed apps"
            helperText="每行一个 App 名称或 bundle id。Computer Use observe/validate/action 会强制检查当前 target 是否在 allowlist 内。"
            value={allowedAppsText}
            onChange={(event) => setAllowedAppsText(event.target.value)}
            minRows={3}
            multiline
            fullWidth
          />
          <TextField
            label="日志保留天数"
            type="number"
            value={settings.logRetentionDays}
            onChange={(event) =>
              setSettings((current) => ({
                ...current,
                logRetentionDays: Math.max(
                  1,
                  Math.min(365, Number(event.target.value) || 1),
                ),
              }))
            }
            inputProps={{ min: 1, max: 365 }}
            sx={{ maxWidth: 220 }}
          />
          <FormControlLabel
            control={
              <Switch
                checked={settings.saveScreenshots}
                onChange={(event) =>
                  setSettings((current) => ({
                    ...current,
                    saveScreenshots: event.target.checked,
                  }))
                }
              />
            }
            label="允许保存 observation 截图到本地审计记录"
          />
          <Typography variant="caption" color="text.secondary">
            UI 默认只关注任务结果与本地留痕路径，不展示截图或逐步操作流程。
            computer_type 会优先使用 macOS 直接按键输入；只有普通文本在直接输入不可用时才使用受控剪贴板 fallback。疑似
            secret/token/password 会禁用剪贴板 fallback，避免被剪贴板历史工具记录。
          </Typography>
          <Box>
            <Button
              variant="contained"
              startIcon={<Save />}
              disabled={loading}
              onClick={() => void saveSettings()}
            >
              保存 Computer Use 设置
            </Button>
          </Box>
        </Stack>
      </Card>

      <Card variant="outlined" sx={{ p: 2, borderRadius: 2.5 }}>
        <Stack spacing={1.5}>
          <Typography variant="subtitle1" fontWeight={700}>
            本项目结果留痕
          </Typography>
          <Typography variant="body2" color="text.secondary">
            这里不展示截图、逐步操作流程或输入内容。用户主要查看结果；如需审计或排障，只保留本机留痕路径。
          </Typography>
          <Stack direction="row" spacing={1} flexWrap="wrap" useFlexGap>
            <Chip label={`Result records: ${audit?.runCount ?? 0}`} />
            <Chip
              label={`OK: ${audit?.resultOkCount ?? 0}`}
              color={(audit?.resultOkCount ?? 0) > 0 ? "success" : "default"}
              variant="outlined"
            />
            <Chip
              label={`Needs attention: ${audit?.resultNeedsAttentionCount ?? 0}`}
              color={
                (audit?.resultNeedsAttentionCount ?? 0) > 0
                  ? "warning"
                  : "default"
              }
              variant="outlined"
            />
            <Chip
              label={`Blocked: ${audit?.resultBlockedCount ?? 0}`}
              color={(audit?.resultBlockedCount ?? 0) > 0 ? "warning" : "default"}
              variant="outlined"
            />
            <Chip label={`Stopped: ${audit?.resultStoppedCount ?? 0}`} />
            <Chip label={`Evidence size: ${formatBytes(audit?.bytes ?? 0)}`} />
            <Chip
              label={`Retention: ${audit?.retentionDays ?? settings.logRetentionDays}d`}
            />
            {(audit?.resultUnknownCount ?? 0) > 0 && (
              <Chip
                label={`Unknown: ${audit?.resultUnknownCount ?? 0}`}
                variant="outlined"
              />
            )}
          </Stack>
          {((audit?.prunedRunCount ?? 0) > 0 ||
            (audit?.prunedTempDirCount ?? 0) > 0) && (
            <Typography variant="caption" color="text.secondary">
              本次刷新已按保留策略清理 {audit?.prunedRunCount ?? 0} 个旧 run、
              {audit?.prunedTempDirCount ?? 0} 个临时截图目录，释放{" "}
              {formatBytes(audit?.prunedBytes ?? 0)}。
            </Typography>
          )}
          <Typography
            component="code"
            sx={{
              display: "block",
              p: 1,
              borderRadius: 1,
              bgcolor: "action.hover",
              fontSize: 12,
              overflowWrap: "anywhere",
            }}
          >
            Evidence path:{" "}
            {audit?.runsRoot ?? "<project>/.omiga/computer-use/runs"}
          </Typography>
          <Divider />
          <Stack direction="row" spacing={1}>
            <Button
              variant="outlined"
              startIcon={<Refresh />}
              disabled={loading || !hasProject}
              onClick={() => void loadAudit(normalizedSettings.logRetentionDays)}
            >
              刷新结果留痕
            </Button>
            <Button
              color="error"
              variant="outlined"
              startIcon={<DeleteOutline />}
              disabled={loading || !hasProject || (audit?.runCount ?? 0) === 0}
              onClick={() => void clearAudit()}
            >
              清理结果留痕
            </Button>
          </Stack>
        </Stack>
      </Card>
    </Stack>
  );
}

export function ComputerUseSettingsTab(props: ComputerUseSettingsTabProps) {
  return <ComputerUseSettingsPanel {...props} />;
}
