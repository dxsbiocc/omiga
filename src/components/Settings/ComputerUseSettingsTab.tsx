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

type ComputerUseSettings = {
  allowedApps: string[];
  logRetentionDays: number;
  saveScreenshots: boolean;
};

type ComputerUseAuditSummary = {
  auditRoot: string;
  runsRoot: string;
  runCount: number;
  actionCount: number;
  bytes: number;
};

type ComputerUsePermissionStatus = {
  platform: string;
  supported: boolean;
  accessibility: string;
  screenRecording: string;
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

export function ComputerUseSettingsTab({
  projectPath,
  onOpenPlugins,
}: ComputerUseSettingsTabProps) {
  const [settings, setSettings] =
    useState<ComputerUseSettings>(DEFAULT_SETTINGS);
  const [allowedAppsText, setAllowedAppsText] = useState(
    DEFAULT_SETTINGS.allowedApps.join("\n"),
  );
  const [audit, setAudit] = useState<ComputerUseAuditSummary | null>(null);
  const [permissionStatus, setPermissionStatus] =
    useState<ComputerUsePermissionStatus | null>(null);
  const [loading, setLoading] = useState(false);
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

  const loadAudit = useCallback(async () => {
    if (!hasProject) {
      setAudit(null);
      return;
    }
    const summary = await invoke<ComputerUseAuditSummary>(
      "computer_use_audit_summary",
      { projectRoot: projectPath },
    );
    setAudit(summary);
  }, [hasProject, projectPath]);

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
      try {
        const status = await invoke<ComputerUsePermissionStatus>(
          "computer_use_permission_status",
        );
        setPermissionStatus(status);
      } catch {
        setPermissionStatus(null);
      }
      await loadAudit();
    } catch (error) {
      setMessage({
        type: "error",
        text: `加载 Computer Use 设置失败：${String(error)}`,
      });
    } finally {
      setLoading(false);
    }
  }, [loadAudit]);

  useEffect(() => {
    void load();
  }, [load]);

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
      await loadAudit();
      setMessage({
        type: "success",
        text: `已清理 ${cleared.runCount} 个 Computer Use 运行记录。`,
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
      <Alert severity="info" sx={{ borderRadius: 2 }}>
        Computer Use 是可选本机自动化扩展。插件安装/启用只代表能力可用；每条任务仍需要在聊天输入区显式开启
        Task 或 Session 模式。
      </Alert>

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
            <Button variant="outlined" onClick={onOpenPlugins}>
              打开插件设置
            </Button>
            <Button
              variant="text"
              startIcon={<Refresh />}
              disabled={loading}
              onClick={() => void load()}
            >
              刷新
            </Button>
          </Stack>
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
            本项目运行记录
          </Typography>
          <Stack direction="row" spacing={1} flexWrap="wrap" useFlexGap>
            <Chip label={`Runs: ${audit?.runCount ?? 0}`} />
            <Chip label={`Actions: ${audit?.actionCount ?? 0}`} />
            <Chip label={`Size: ${formatBytes(audit?.bytes ?? 0)}`} />
          </Stack>
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
            {audit?.runsRoot ?? "<project>/.omiga/computer-use/runs"}
          </Typography>
          <Divider />
          <Stack direction="row" spacing={1}>
            <Button
              variant="outlined"
              startIcon={<Refresh />}
              disabled={loading || !hasProject}
              onClick={() => void loadAudit()}
            >
              刷新记录
            </Button>
            <Button
              color="error"
              variant="outlined"
              startIcon={<DeleteOutline />}
              disabled={loading || !hasProject || (audit?.runCount ?? 0) === 0}
              onClick={() => void clearAudit()}
            >
              清理运行记录
            </Button>
          </Stack>
        </Stack>
      </Card>
    </Stack>
  );
}
