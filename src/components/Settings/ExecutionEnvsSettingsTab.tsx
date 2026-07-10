import { useState, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  Box,
  Stack,
  Typography,
  TextField,
  Button,
  Alert,
  Divider,
  CircularProgress,
  FormControlLabel,
  Switch,
  IconButton,
  List,
  ListItem,
  ListItemText,
  ListItemSecondaryAction,
  Dialog,
  DialogTitle,
  DialogContent,
  DialogActions,
} from "@mui/material";
import {
  Add,
  Edit,
  Delete,
  Refresh,
} from "@mui/icons-material";
import { RSYNC_INSTALL_HELP_URL } from "../../lib/rsyncSsh";
import { BrowserOperatorSettingsCard } from "./BrowserOperatorSettingsCard";

interface SshConfig {
  // Host pattern (the name used to reference this config)
  host?: string;
  // Hostname (actual server address) - matches SSH config HostName
  HostName?: string;
  // Username - matches SSH config User
  User?: string;
  // Port - matches SSH config Port
  Port: number;
  // Path to private key - matches SSH config IdentityFile
  IdentityFile?: string;
  // Whether this config is enabled
  enabled: boolean;
  // Source of config: 'ssh_config' for ~/.ssh/config, 'user' for omiga.yaml
  source?: 'ssh_config' | 'user';
}

interface SshConfigsMap {
  [name: string]: SshConfig;
}

/** 本机 `rsync` 是否可用（SSH 同步依赖） */
type RsyncCheckStatus = "loading" | "ok" | "missing" | "unknown";

interface ExecutionEnvsSettingsTabProps {
  /** When true, renders in compact mode for embedding in Advanced tab */
  embedded?: boolean;
  /** Reserved for legacy deep links; only SSH is exposed until cloud execution is real. */
  initialSubTab?: number;
}

export function ExecutionEnvsSettingsTab({
  embedded = false,
}: ExecutionEnvsSettingsTabProps) {
  const [isLoading, setIsLoading] = useState(false);
  const [message, setMessage] = useState<{
    type: "success" | "error";
    text: string;
  } | null>(null);

  // SSH state
  const [sshConfigs, setSshConfigs] = useState<SshConfigsMap>({});
  const [sandboxEscalationEnabled, setSandboxEscalationEnabled] = useState(true);
  const [sshDialogOpen, setSshDialogOpen] = useState(false);
  const [editingSshName, setEditingSshName] = useState<string | null>(null);
  const [sshForm, setSshForm] = useState<SshConfig & { name: string }>({
    name: "",
    host: "",
    HostName: "",
    User: "",
    Port: 22,
    IdentityFile: "",
    enabled: true,
  });

  const [rsyncCheckStatus, setRsyncCheckStatus] =
    useState<RsyncCheckStatus>("loading");

  const refreshRsyncStatus = useCallback(async () => {
    setRsyncCheckStatus("loading");
    try {
      const ok = await invoke<boolean>("is_rsync_available");
      setRsyncCheckStatus(ok ? "ok" : "missing");
    } catch {
      setRsyncCheckStatus("unknown");
    }
  }, []);

  useEffect(() => {
    void refreshRsyncStatus();
  }, [refreshRsyncStatus]);

  // Load configs on mount
  useEffect(() => {
    loadConfigs();
  }, []);

  const loadConfigs = async () => {
    try {
      // Load SSH configs
      const ssh = await invoke<SshConfigsMap>("get_ssh_configs");
      setSshConfigs(ssh || {});
    } catch (error) {
      console.error("Failed to load execution env configs:", error);
    }

    try {
      const enabled = await invoke<boolean>("get_sandbox_escalation_enabled");
      setSandboxEscalationEnabled(enabled);
    } catch (error) {
      console.error("Failed to load sandbox escalation setting:", error);
      setSandboxEscalationEnabled(true);
    }
  };

  const handleSandboxEscalationToggle = async (enabled: boolean) => {
    const previous = sandboxEscalationEnabled;
    setSandboxEscalationEnabled(enabled);
    setIsLoading(true);
    setMessage(null);

    try {
      await invoke("set_sandbox_escalation_enabled", { enabled });
      setMessage({
        type: "success",
        text: enabled
          ? "沙箱提权审批已开启"
          : "沙箱提权审批已关闭",
      });
    } catch (error) {
      console.error("Failed to save sandbox escalation setting:", error);
      setSandboxEscalationEnabled(previous);
      setMessage({
        type: "error",
        text: `Failed to save sandbox escalation setting: ${error}`,
      });
    } finally {
      setIsLoading(false);
    }
  };

  const handleSaveSsh = async () => {
    setIsLoading(true);
    setMessage(null);
    try {
      const name = sshForm.name.trim();
      if (!name) {
        setMessage({ type: "error", text: "Host name is required" });
        return;
      }
      if (!sshForm.HostName && !sshForm.host) {
        setMessage({ type: "error", text: "Hostname is required" });
        return;
      }

      const config: SshConfig = {
        host: name, // The reference name
        HostName: sshForm.HostName?.trim() || sshForm.host?.trim() || name,
        User: sshForm.User?.trim() || undefined,
        Port: sshForm.Port || 22,
        IdentityFile: sshForm.IdentityFile?.trim() || undefined,
        enabled: sshForm.enabled,
      };

      await invoke("save_ssh_config", { name, sshConfig: config });
      setSshConfigs((prev) => ({ ...prev, [name]: { ...config, source: 'user' } }));
      setSshDialogOpen(false);
      setMessage({ type: "success", text: `SSH config "${name}" saved` });
    } catch (error) {
      console.error("Failed to save SSH config:", error);
      setMessage({ type: "error", text: `Failed to save: ${error}` });
    } finally {
      setIsLoading(false);
    }
  };

  const handleDeleteSsh = async (name: string) => {
    if (!confirm(`Delete SSH config "${name}"?`)) return;
    setIsLoading(true);
    try {
      await invoke("delete_ssh_config", { name });
      setSshConfigs((prev) => {
        const next = { ...prev };
        delete next[name];
        return next;
      });
      setMessage({ type: "success", text: `SSH config "${name}" deleted` });
    } catch (error) {
      console.error("Failed to delete SSH config:", error);
      setMessage({ type: "error", text: `Failed to delete: ${error}` });
    } finally {
      setIsLoading(false);
    }
  };

  const openSshDialog = (name?: string) => {
    if (name && sshConfigs[name]) {
      const config = sshConfigs[name];
      setEditingSshName(name);
      setSshForm({
        name,
        host: config.host || name,
        HostName: config.HostName || config.host || "",
        User: config.User || "",
        Port: config.Port || 22,
        IdentityFile: config.IdentityFile || "",
        enabled: config.enabled,
      });
    } else {
      setEditingSshName(null);
      setSshForm({
        name: "",
        host: "",
        HostName: "",
        User: "",
        Port: 22,
        IdentityFile: "",
        enabled: true,
      });
    }
    setSshDialogOpen(true);
  };

  return (
    <Box>
      <Typography variant={embedded ? "body2" : "subtitle2"} fontWeight={600} sx={{ mb: 1 }}>
        Execution Environments
      </Typography>
      <Typography variant="caption" color="text.secondary" sx={{ mb: 2, display: "block" }}>
        Configure sandbox approval and SSH remote execution. User settings are stored in{" "}
        <code>omiga.yaml</code>; SSH profiles are also read from <code>~/.ssh/config</code>.
      </Typography>
      <BrowserOperatorSettingsCard compact={embedded} />
      <Box
        sx={{
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
          gap: 2,
          mb: 2,
          p: 1.5,
          border: 1,
          borderColor: "divider",
          borderRadius: 1,
          bgcolor: "background.paper",
        }}
      >
        <Box>
          <Typography variant="body2" fontWeight={600}>
            沙箱拒绝时请求提权审批
          </Typography>
          <Typography variant="caption" color="text.secondary">
            关闭后，本地沙箱拒绝的命令会直接失败，不再弹出单次无沙箱重跑审批。
          </Typography>
        </Box>
        <Switch
          checked={sandboxEscalationEnabled}
          onChange={(e) => void handleSandboxEscalationToggle(e.target.checked)}
          disabled={isLoading}
          inputProps={{ "aria-label": "沙箱拒绝时请求提权审批" }}
        />
      </Box>
      <Box>
          <Alert
            severity={
              rsyncCheckStatus === "ok"
                ? "success"
                : rsyncCheckStatus === "missing"
                  ? "warning"
                  : rsyncCheckStatus === "unknown"
                    ? "info"
                    : "info"
            }
            sx={{ mb: 2 }}
            action={
              <Stack direction="row" spacing={1} alignItems="center">
                <Button
                  size="small"
                  startIcon={
                    rsyncCheckStatus === "loading" ? (
                      <CircularProgress size={14} color="inherit" />
                    ) : (
                      <Refresh fontSize="small" />
                    )
                  }
                  onClick={() => void refreshRsyncStatus()}
                  disabled={rsyncCheckStatus === "loading"}
                >
                  重新检测
                </Button>
                <Button
                  size="small"
                  variant="outlined"
                  onClick={() => void openUrl(RSYNC_INSTALL_HELP_URL)}
                >
                  安装说明
                </Button>
              </Stack>
            }
          >
            <Typography variant="subtitle2" fontWeight={600} gutterBottom>
              rsync（SSH 文件同步）
            </Typography>
            {rsyncCheckStatus === "loading" && (
              <Typography variant="body2">正在检测本机是否已安装 rsync…</Typography>
            )}
            {rsyncCheckStatus === "ok" && (
              <Typography variant="body2">
                已检测到 rsync。使用 SSH 执行环境时，技能、credentials、缓存等会同步到远端{" "}
                <code>~/.omiga</code>。
              </Typography>
            )}
            {rsyncCheckStatus === "missing" && (
              <Typography variant="body2">
                未检测到 rsync：SSH 远程仍可执行命令，但上述文件<strong>不会</strong>
                同步。请安装 rsync 后点击「重新检测」，或打开「安装说明」查看各系统安装方式。
              </Typography>
            )}
            {rsyncCheckStatus === "unknown" && (
              <Typography variant="body2">
                无法检测 rsync（例如在非桌面环境打开）。请在 Omiga 桌面应用中使用 SSH
                执行环境；安装说明仍可在浏览器中查看。
              </Typography>
            )}
          </Alert>

          <Box sx={{ display: "flex", alignItems: "center", justifyContent: "space-between", mb: 2 }}>
            <Box>
              <Typography variant="body2" fontWeight={600}>
                SSH Configurations
              </Typography>
              <Typography variant="caption" color="text.secondary">
                Reads from ~/.ssh/config automatically
              </Typography>
            </Box>
            <Button
              startIcon={<Add />}
              size="small"
              onClick={() => openSshDialog()}
              disabled={isLoading}
            >
              Add SSH Config
            </Button>
          </Box>

          <List dense>
            {Object.entries(sshConfigs).map(([name, config]) => (
              <ListItem
                key={name}
                sx={{
                  bgcolor: "background.paper",
                  borderRadius: 1,
                  mb: 1,
                  border: 1,
                  borderColor: "divider",
                }}
              >
                <ListItemText
                  primary={
                    <Box sx={{ display: "flex", alignItems: "center", gap: 1 }}>
                      {name}
                      {config.source === 'ssh_config' && (
                        <Typography component="span" variant="caption" color="text.secondary" sx={{ fontSize: '0.7rem', bgcolor: 'action.hover', px: 0.5, borderRadius: 0.5 }}>
                          ~/.ssh/config
                        </Typography>
                      )}
                    </Box>
                  }
                  secondary={`${config.User || 'user'}@${config.HostName || config.host || name}:${config.Port || 22}`}
                />
                <ListItemSecondaryAction>
                  <IconButton edge="end" size="small" onClick={() => openSshDialog(name)} sx={{ mr: 1 }}>
                    <Edit fontSize="small" />
                  </IconButton>
                  <IconButton edge="end" size="small" onClick={() => handleDeleteSsh(name)} color="error">
                    <Delete fontSize="small" />
                  </IconButton>
                </ListItemSecondaryAction>
              </ListItem>
            ))}
            {Object.keys(sshConfigs).length === 0 && (
              <Typography variant="body2" color="text.secondary" align="center" sx={{ py: 4 }}>
                No SSH configurations found. Add one above or create ~/.ssh/config
              </Typography>
            )}
          </List>
      </Box>

      {/* SSH Dialog */}
      <Dialog open={sshDialogOpen} onClose={() => setSshDialogOpen(false)} maxWidth="sm" fullWidth>
        <DialogTitle>{editingSshName ? "Edit SSH Config" : "Add SSH Config"}</DialogTitle>
        <DialogContent>
          <Typography variant="caption" color="text.secondary" sx={{ display: "block", mb: 2, mt: 1 }}>
            Config format matches ~/.ssh/config (Host, HostName, User, Port, IdentityFile)
          </Typography>
          <TextField
            fullWidth
            label="Host (Config Name)"
            value={sshForm.name}
            onChange={(e) => setSshForm((prev) => ({ ...prev, name: e.target.value }))}
            disabled={isLoading || !!editingSshName}
            placeholder="my-server"
            helperText="The name used to reference this config"
            sx={{ mb: 2 }}
          />
          <TextField
            fullWidth
            label="HostName"
            value={sshForm.HostName}
            onChange={(e) => setSshForm((prev) => ({ ...prev, HostName: e.target.value }))}
            disabled={isLoading}
            placeholder="192.168.1.100 or example.com"
            helperText="Actual server address (can be same as Host)"
            sx={{ mb: 2 }}
          />
          <TextField
            fullWidth
            label="User"
            value={sshForm.User}
            onChange={(e) => setSshForm((prev) => ({ ...prev, User: e.target.value }))}
            disabled={isLoading}
            placeholder="ubuntu"
            sx={{ mb: 2 }}
          />
          <TextField
            fullWidth
            type="number"
            label="Port"
            value={sshForm.Port}
            onChange={(e) => setSshForm((prev) => ({ ...prev, Port: parseInt(e.target.value) || 22 }))}
            disabled={isLoading}
            sx={{ mb: 2 }}
          />
          <TextField
            fullWidth
            label="IdentityFile (optional)"
            value={sshForm.IdentityFile}
            onChange={(e) => setSshForm((prev) => ({ ...prev, IdentityFile: e.target.value }))}
            disabled={isLoading}
            placeholder="~/.ssh/id_rsa"
            helperText="Path to private key (leave empty for SSH agent)"
            sx={{ mb: 2 }}
          />
          <FormControlLabel
            control={
              <Switch
                checked={sshForm.enabled}
                onChange={(e) => setSshForm((prev) => ({ ...prev, enabled: e.target.checked }))}
                disabled={isLoading}
              />
            }
            label="Enabled"
          />
        </DialogContent>
        <DialogActions>
          <Button onClick={() => setSshDialogOpen(false)} disabled={isLoading}>
            Cancel
          </Button>
          <Button onClick={handleSaveSsh} variant="contained" disabled={isLoading}>
            Save
          </Button>
        </DialogActions>
      </Dialog>

      {!embedded && <Divider sx={{ my: 2 }} />}

      {message && (
        <Alert severity={message.type} sx={{ mt: 2 }}>
          {message.text}
        </Alert>
      )}
    </Box>
  );
}
