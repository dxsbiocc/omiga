import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  Box,
  Typography,
  TextField,
  Button,
  Alert,
  Divider,
  FormControlLabel,
  Switch,
  IconButton,
  InputAdornment,
  Tabs,
  Tab,
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
  Visibility,
  VisibilityOff,
  Add,
  Edit,
  Delete,
  CheckCircle,
  Error as ErrorIcon,
} from "@mui/icons-material";

// Types matching Rust structs
interface ModalConfig {
  token_id?: string;
  token_secret?: string;
  default_image?: string;
  enabled: boolean;
}

interface DaytonaConfig {
  server_url?: string;
  api_key?: string;
  default_image?: string;
  enabled: boolean;
}

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

interface ExecutionEnvsSettingsTabProps {
  /** When true, renders in compact mode for embedding in Advanced tab */
  embedded?: boolean;
}

export function ExecutionEnvsSettingsTab({ embedded = false }: ExecutionEnvsSettingsTabProps) {
  const [activeTab, setActiveTab] = useState(0);
  const [isLoading, setIsLoading] = useState(false);
  const [message, setMessage] = useState<{
    type: "success" | "error";
    text: string;
  } | null>(null);

  // Modal state
  const [modalConfig, setModalConfig] = useState<ModalConfig>({
    enabled: false,
  });
  const [showModalTokenId, setShowModalTokenId] = useState(false);
  const [showModalTokenSecret, setShowModalTokenSecret] = useState(false);
  const [isModalConfigured, setIsModalConfigured] = useState(false);

  // Daytona state
  const [daytonaConfig, setDaytonaConfig] = useState<DaytonaConfig>({
    enabled: false,
  });
  const [showDaytonaApiKey, setShowDaytonaApiKey] = useState(false);
  const [isDaytonaConfigured, setIsDaytonaConfigured] = useState(false);

  // SSH state
  const [sshConfigs, setSshConfigs] = useState<SshConfigsMap>({});
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
  // Load configs on mount
  useEffect(() => {
    loadConfigs();
  }, []);

  const loadConfigs = async () => {
    try {
      // Load Modal config
      const modal = await invoke<ModalConfig | null>("get_modal_config");
      if (modal) {
        setModalConfig(modal);
      }
      const modalConfigured = await invoke<boolean>("is_modal_configured");
      setIsModalConfigured(modalConfigured);

      // Load Daytona config
      const daytona = await invoke<DaytonaConfig | null>("get_daytona_config");
      if (daytona) {
        setDaytonaConfig(daytona);
      }
      const daytonaConfigured = await invoke<boolean>("is_daytona_configured");
      setIsDaytonaConfigured(daytonaConfigured);

      // Load SSH configs
      const ssh = await invoke<SshConfigsMap>("get_ssh_configs");
      setSshConfigs(ssh || {});
    } catch (error) {
      console.error("Failed to load execution env configs:", error);
    }
  };

  const handleSaveModal = async () => {
    setIsLoading(true);
    setMessage(null);
    try {
      await invoke("save_modal_config", {
        modalConfig: {
          ...modalConfig,
          token_id: modalConfig.token_id?.trim(),
          token_secret: modalConfig.token_secret?.trim(),
          default_image: modalConfig.default_image?.trim() || "python:3.11",
        },
      });
      const configured = await invoke<boolean>("is_modal_configured");
      setIsModalConfigured(configured);
      setMessage({ type: "success", text: "Modal configuration saved" });
    } catch (error) {
      console.error("Failed to save Modal config:", error);
      setMessage({ type: "error", text: `Failed to save: ${error}` });
    } finally {
      setIsLoading(false);
    }
  };

  const handleSaveDaytona = async () => {
    setIsLoading(true);
    setMessage(null);
    try {
      await invoke("save_daytona_config", {
        daytonaConfig: {
          ...daytonaConfig,
          server_url: daytonaConfig.server_url?.trim(),
          api_key: daytonaConfig.api_key?.trim(),
          default_image: daytonaConfig.default_image?.trim() || "ubuntu:22.04",
        },
      });
      const configured = await invoke<boolean>("is_daytona_configured");
      setIsDaytonaConfigured(configured);
      setMessage({ type: "success", text: "Daytona configuration saved" });
    } catch (error) {
      console.error("Failed to save Daytona config:", error);
      setMessage({ type: "error", text: `Failed to save: ${error}` });
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

  const handleTabChange = (_: React.SyntheticEvent, newValue: number) => {
    setActiveTab(newValue);
    setMessage(null);
  };

  return (
    <Box>
      <Typography variant={embedded ? "body2" : "subtitle2"} fontWeight={600} sx={{ mb: 1 }}>
        Execution Environments
      </Typography>
      <Typography variant="caption" color="text.secondary" sx={{ mb: 2, display: "block" }}>
        Configure API keys for remote execution environments (Modal, Daytona, SSH).
        Stored in <code>omiga.yaml</code>.
      </Typography>

      <Tabs value={activeTab} onChange={handleTabChange} sx={{ mb: 2, minHeight: embedded ? '36px' : '48px' }}>
        <Tab label="Modal" />
        <Tab label="Daytona" />
        <Tab label="SSH" />
      </Tabs>

      {/* Modal Tab */}
      {activeTab === 0 && (
        <Box>
          <Box sx={{ display: "flex", alignItems: "center", gap: 1, mb: 2 }}>
            <Typography variant="body2" fontWeight={600}>
              Modal Cloud
            </Typography>
            {isModalConfigured ? (
              <CheckCircle fontSize="small" color="success" />
            ) : (
              <ErrorIcon fontSize="small" color="disabled" />
            )}
            <Typography variant="caption" color={isModalConfigured ? "success.main" : "text.disabled"}>
              {isModalConfigured ? "Configured" : "Not configured"}
            </Typography>
          </Box>

          <FormControlLabel
            control={
              <Switch
                checked={modalConfig.enabled}
                onChange={(e) =>
                  setModalConfig((prev) => ({ ...prev, enabled: e.target.checked }))
                }
                disabled={isLoading}
              />
            }
            label="Enable Modal"
            sx={{ mb: 2, display: "block" }}
          />

          <TextField
            fullWidth
            type={showModalTokenId ? "text" : "password"}
            label="Modal Token ID"
            value={modalConfig.token_id || ""}
            onChange={(e) =>
              setModalConfig((prev) => ({ ...prev, token_id: e.target.value }))
            }
            disabled={isLoading}
            placeholder="ak-..."
            sx={{ mb: 2 }}
            InputProps={{
              endAdornment: (
                <InputAdornment position="end">
                  <IconButton onClick={() => setShowModalTokenId(!showModalTokenId)} edge="end" size="small">
                    {showModalTokenId ? <VisibilityOff /> : <Visibility />}
                  </IconButton>
                </InputAdornment>
              ),
            }}
          />

          <TextField
            fullWidth
            type={showModalTokenSecret ? "text" : "password"}
            label="Modal Token Secret"
            value={modalConfig.token_secret || ""}
            onChange={(e) =>
              setModalConfig((prev) => ({ ...prev, token_secret: e.target.value }))
            }
            disabled={isLoading}
            placeholder="..."
            sx={{ mb: 2 }}
            InputProps={{
              endAdornment: (
                <InputAdornment position="end">
                  <IconButton
                    onClick={() => setShowModalTokenSecret(!showModalTokenSecret)}
                    edge="end"
                    size="small"
                  >
                    {showModalTokenSecret ? <VisibilityOff /> : <Visibility />}
                  </IconButton>
                </InputAdornment>
              ),
            }}
          />

          <TextField
            fullWidth
            label="Default Image (optional)"
            value={modalConfig.default_image || ""}
            onChange={(e) =>
              setModalConfig((prev) => ({ ...prev, default_image: e.target.value }))
            }
            disabled={isLoading}
            placeholder="python:3.11"
            helperText="Default container image for Modal sandboxes"
            sx={{ mb: 2 }}
          />

          <Button variant="contained" onClick={handleSaveModal} disabled={isLoading} sx={{ mb: 2 }}>
            Save Modal Settings
          </Button>

          <Typography variant="caption" color="text.secondary" display="block">
            Get your Modal tokens from{" "}
            <a href="https://modal.com/settings/tokens" target="_blank" rel="noopener noreferrer">
              Modal Dashboard
            </a>
          </Typography>
        </Box>
      )}

      {/* Daytona Tab */}
      {activeTab === 1 && (
        <Box>
          <Box sx={{ display: "flex", alignItems: "center", gap: 1, mb: 2 }}>
            <Typography variant="body2" fontWeight={600}>
              Daytona
            </Typography>
            {isDaytonaConfigured ? (
              <CheckCircle fontSize="small" color="success" />
            ) : (
              <ErrorIcon fontSize="small" color="disabled" />
            )}
            <Typography variant="caption" color={isDaytonaConfigured ? "success.main" : "text.disabled"}>
              {isDaytonaConfigured ? "Configured" : "Not configured"}
            </Typography>
          </Box>

          <FormControlLabel
            control={
              <Switch
                checked={daytonaConfig.enabled}
                onChange={(e) =>
                  setDaytonaConfig((prev) => ({ ...prev, enabled: e.target.checked }))
                }
                disabled={isLoading}
              />
            }
            label="Enable Daytona"
            sx={{ mb: 2, display: "block" }}
          />

          <TextField
            fullWidth
            label="Daytona Server URL"
            value={daytonaConfig.server_url || ""}
            onChange={(e) =>
              setDaytonaConfig((prev) => ({ ...prev, server_url: e.target.value }))
            }
            disabled={isLoading}
            placeholder="https://api.daytona.io"
            sx={{ mb: 2 }}
          />

          <TextField
            fullWidth
            type={showDaytonaApiKey ? "text" : "password"}
            label="Daytona API Key"
            value={daytonaConfig.api_key || ""}
            onChange={(e) =>
              setDaytonaConfig((prev) => ({ ...prev, api_key: e.target.value }))
            }
            disabled={isLoading}
            placeholder="..."
            sx={{ mb: 2 }}
            InputProps={{
              endAdornment: (
                <InputAdornment position="end">
                  <IconButton
                    onClick={() => setShowDaytonaApiKey(!showDaytonaApiKey)}
                    edge="end"
                    size="small"
                  >
                    {showDaytonaApiKey ? <VisibilityOff /> : <Visibility />}
                  </IconButton>
                </InputAdornment>
              ),
            }}
          />

          <TextField
            fullWidth
            label="Default Image (optional)"
            value={daytonaConfig.default_image || ""}
            onChange={(e) =>
              setDaytonaConfig((prev) => ({ ...prev, default_image: e.target.value }))
            }
            disabled={isLoading}
            placeholder="ubuntu:22.04"
            helperText="Default container image for Daytona workspaces"
            sx={{ mb: 2 }}
          />

          <Button variant="contained" onClick={handleSaveDaytona} disabled={isLoading} sx={{ mb: 2 }}>
            Save Daytona Settings
          </Button>

          <Typography variant="caption" color="text.secondary" display="block">
            Get your Daytona API key from{" "}
            <a href="https://daytona.io/docs/administration/api-keys/" target="_blank" rel="noopener noreferrer">
              Daytona Docs
            </a>
          </Typography>
        </Box>
      )}

      {/* SSH Tab */}
      {activeTab === 2 && (
        <Box>
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
      )}

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
