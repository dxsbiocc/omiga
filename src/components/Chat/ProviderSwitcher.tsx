import { useState, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  OMIGA_PROVIDER_CHANGED_EVENT,
  notifyProviderChanged,
} from "../../utils/providerEvents";
import { invokeIfTauri } from "../../utils/tauriRuntime";
import { useSessionStore } from "../../state/sessionStore";
import type { SxProps, Theme } from "@mui/material/styles";
import {
  Box,
  Button,
  Menu,
  MenuItem,
  Typography,
  Chip,
  Tooltip,
  ListItemIcon,
  ListItemText,
  CircularProgress,
  Snackbar,
  Alert,
  alpha,
  useTheme,
} from "@mui/material";
import {
  RadioButtonChecked,
  RadioButtonUnchecked,
  Settings,
} from "@mui/icons-material";

interface ProviderConfigEntry {
  name: string;
  providerType: string;
  model: string;
  apiKeyPreview: string;
  baseUrl: string | null;
  thinking?: boolean | null;
  enabled: boolean;
  /** Current chat session (quick switch / runtime). */
  isSessionActive: boolean;
  /** Saved default in omiga.yaml — used on startup. */
  isDefault: boolean;
}

interface ProviderSwitcherProps {
  onOpenSettings?: () => void;
  /** Merged into the trigger button for layout/theming (e.g. composer toolbar). */
  triggerSx?: SxProps<Theme>;
}

// Provider type to display name mapping
const PROVIDER_DISPLAY_NAMES: Record<string, string> = {
  anthropic: "Claude",
  openai: "GPT",
  azure: "Azure",
  google: "Gemini",
  minimax: "MiniMax",
  alibaba: "Qwen",
  deepseek: "DeepSeek",
  zhipu: "ChatGLM",
  moonshot: "Kimi",
  custom: "Custom",
};

export function ProviderSwitcher({
  onOpenSettings,
  triggerSx,
}: ProviderSwitcherProps) {
  const currentSessionId = useSessionStore((s) => s.currentSession?.id);
  const theme = useTheme();
  const isDark = theme.palette.mode === "dark";
  const [providers, setProviders] = useState<ProviderConfigEntry[]>([]);
  const [loading, _setLoading] = useState(false);
  const [switching, setSwitching] = useState<string | null>(null);
  const [anchorEl, setAnchorEl] = useState<null | HTMLElement>(null);
  const [activeProvider, setActiveProvider] = useState<ProviderConfigEntry | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useState<string | null>(null);

  const open = Boolean(anchorEl);

  const loadProviders = useCallback(async () => {
    const configs = await invokeIfTauri<ProviderConfigEntry[]>("list_provider_configs");
    if (configs && configs.length > 0) {
      setProviders(configs);
      const active = configs.find((p) => p.isSessionActive);
      // Always sync: avoids stale "DeepSeek" chip after switching elsewhere (e.g. Settings).
      setActiveProvider(active ?? null);
    } else {
      setProviders([]);
      setActiveProvider(null);
    }
  }, []);

  useEffect(() => {
    loadProviders();
    // Refresh every 30 seconds to catch changes from other windows/sessions
    const interval = setInterval(loadProviders, 30000);
    return () => clearInterval(interval);
  }, [loadProviders]);

  useEffect(() => {
    const onChanged = () => {
      void loadProviders();
    };
    window.addEventListener(OMIGA_PROVIDER_CHANGED_EVENT, onChanged);
    return () => window.removeEventListener(OMIGA_PROVIDER_CHANGED_EVENT, onChanged);
  }, [loadProviders]);

  const handleClick = (event: React.MouseEvent<HTMLElement>) => {
    setAnchorEl(event.currentTarget);
    // Refresh list when opening
    loadProviders();
  };

  const handleClose = () => {
    setAnchorEl(null);
  };

  const handleSwitch = async (provider: ProviderConfigEntry) => {
    if (provider.isSessionActive) {
      handleClose();
      return;
    }

    try {
      setSwitching(provider.name);
      setError(null);

      const result = await invoke<{
        provider: string;
        model: string | null;
        apiKeyPreview: string;
      }>("quick_switch_provider", {
        providerName: provider.name,
        sessionId: currentSessionId ?? null,
      });

      // Update local state
      setProviders((prev) =>
        prev.map((p) => ({
          ...p,
          isSessionActive: p.name === provider.name,
        }))
      );
      setActiveProvider(provider);
      notifyProviderChanged();
      setSuccess(`Switched to ${provider.name} (${result.provider})`);
      setTimeout(() => setSuccess(null), 3000);
    } catch (err: any) {
      console.error("Failed to switch provider:", err);
      setError(err?.message || String(err) || "Failed to switch provider");
    } finally {
      setSwitching(null);
      handleClose();
    }
  };

  const getProviderDisplay = (provider: ProviderConfigEntry) => {
    const key = provider.providerType.toLowerCase();
    const typeName = PROVIDER_DISPLAY_NAMES[key] || provider.providerType;
    return {
      /** 配置名称（菜单主行） */
      configName: provider.name,
      /** 供应商 / 品牌（Chip） */
      supplierLabel: typeName,
      /** 模型名（Chip 右侧，不重复供应商文案） */
      modelName: provider.model,
    };
  };

  // If no providers configured, show a simple button to open settings
  if (providers.length === 0) {
    return (
      <Tooltip title="Configure Model Providers">
        <Button
          size="small"
          variant="outlined"
          startIcon={<Settings />}
          onClick={onOpenSettings}
          sx={[
            {
              borderRadius: 2,
              textTransform: "none",
              fontSize: "0.75rem",
            },
            ...(Array.isArray(triggerSx) ? triggerSx : triggerSx ? [triggerSx] : []),
          ]}
        >
          Setup Models
        </Button>
      </Tooltip>
    );
  }

  const display = activeProvider ? getProviderDisplay(activeProvider) : null;
  const triggerTooltip = activeProvider
    ? `${activeProvider.name} · ${activeProvider.model}`
    : "Click to switch model provider";

  return (
    <Box>
      <Tooltip title={triggerTooltip}>
        <Button
          size="small"
          variant="outlined"
          onClick={handleClick}
          endIcon={loading ? <CircularProgress size={12} /> : null}
          sx={[
            {
              borderRadius: 2,
              textTransform: "none",
              fontSize: "0.75rem",
              px: 1.5,
              py: 0.5,
              borderColor: "divider",
              bgcolor: "background.paper",
              "&:hover": {
                bgcolor: "action.hover",
              },
            },
            ...(Array.isArray(triggerSx) ? triggerSx : triggerSx ? [triggerSx] : []),
          ]}
        >
          {display ? (
            <Box sx={{ display: "flex", alignItems: "center", gap: 0.5 }}>
              <Chip
                size="small"
                color="success"
                variant="outlined"
                label={display.supplierLabel}
                sx={{
                  height: 20,
                  fontSize: "0.7rem",
                  "& .MuiChip-label": { px: 0.5 },
                }}
              />
              <Typography
                variant="caption"
                color="text.secondary"
                noWrap
                sx={{ maxWidth: 180 }}
              >
                {display.modelName}
              </Typography>
            </Box>
          ) : (
            "Select Model"
          )}
        </Button>
      </Tooltip>

      <Menu
        anchorEl={anchorEl}
        open={open}
        onClose={handleClose}
        anchorOrigin={{
          vertical: "bottom",
          horizontal: "right",
        }}
        transformOrigin={{
          vertical: "top",
          horizontal: "right",
        }}
        PaperProps={{
          sx: {
            minWidth: 280,
            maxHeight: 400,
          },
        }}
      >
        <Box sx={{ px: 2, py: 1, borderBottom: 1, borderColor: "divider" }}>
          <Typography variant="subtitle2" fontWeight={600}>
            Switch Model Provider
          </Typography>
          <Typography variant="caption" color="text.secondary">
            Current session only — default in Settings / omiga.yaml unchanged
          </Typography>
        </Box>

        {providers.map((provider) => {
          const display = getProviderDisplay(provider);
          const isSwitching = switching === provider.name;

          return (
            <MenuItem
              key={provider.name}
              onClick={() => handleSwitch(provider)}
              disabled={isSwitching}
              selected={provider.isSessionActive}
              sx={{
                py: 1.5,
                borderBottom: 1,
                borderColor: "divider",
                "&:last-child": { borderBottom: 0 },
                "&.Mui-selected": {
                  bgcolor: alpha(
                    theme.palette.primary.main,
                    isDark ? 0.16 : 0.12,
                  ),
                  "&:hover": {
                    bgcolor: alpha(
                      theme.palette.primary.main,
                      isDark ? 0.22 : 0.16,
                    ),
                  },
                },
              }}
            >
              <ListItemIcon>
                {isSwitching ? (
                  <CircularProgress size={16} />
                ) : provider.isSessionActive ? (
                  <RadioButtonChecked color="success" fontSize="small" />
                ) : (
                  <RadioButtonUnchecked fontSize="small" />
                )}
              </ListItemIcon>
              <ListItemText
                primary={
                  <Box sx={{ display: "flex", alignItems: "center", gap: 1, flexWrap: "wrap" }}>
                    <Typography fontWeight={provider.isSessionActive ? 600 : 400}>
                      {display.configName}
                    </Typography>
                    {provider.isSessionActive && (
                      <Chip
                        size="small"
                        color="success"
                        label="In use"
                        sx={{ height: 18, fontSize: "0.65rem" }}
                      />
                    )}
                    {provider.isDefault && (
                      <Chip
                        size="small"
                        variant="outlined"
                        label="Default"
                        sx={{ height: 18, fontSize: "0.65rem" }}
                      />
                    )}
                  </Box>
                }
                secondary={display.modelName}
              />
            </MenuItem>
          );
        })}

        <Box
          sx={{
            px: 2,
            py: 1,
            borderTop: 1,
            borderColor: "divider",
            bgcolor: "background.default",
          }}
        >
          <Button
            fullWidth
            size="small"
            startIcon={<Settings />}
            onClick={() => {
              handleClose();
              onOpenSettings?.();
            }}
            sx={{ textTransform: "none" }}
          >
            Manage Configurations
          </Button>
        </Box>
      </Menu>

      {/* Error Snackbar */}
      <Snackbar
        open={!!error}
        autoHideDuration={6000}
        onClose={() => setError(null)}
        anchorOrigin={{ vertical: "bottom", horizontal: "center" }}
      >
        <Alert severity="error" onClose={() => setError(null)}>
          {error}
        </Alert>
      </Snackbar>

      {/* Success Snackbar */}
      <Snackbar
        open={!!success}
        autoHideDuration={3000}
        onClose={() => setSuccess(null)}
        anchorOrigin={{ vertical: "bottom", horizontal: "center" }}
      >
        <Alert severity="success" onClose={() => setSuccess(null)}>
          {success}
        </Alert>
      </Snackbar>
    </Box>
  );
}
