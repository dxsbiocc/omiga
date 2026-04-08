import { useState, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
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
} from "@mui/material";
import {
  CheckCircle,
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
  enabled: boolean;
  isActive: boolean;
}

interface ProviderSwitcherProps {
  onOpenSettings?: () => void;
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

export function ProviderSwitcher({ onOpenSettings }: ProviderSwitcherProps) {
  const [providers, setProviders] = useState<ProviderConfigEntry[]>([]);
  const [loading, setLoading] = useState(false);
  const [switching, setSwitching] = useState<string | null>(null);
  const [anchorEl, setAnchorEl] = useState<null | HTMLElement>(null);
  const [activeProvider, setActiveProvider] = useState<ProviderConfigEntry | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useState<string | null>(null);

  const open = Boolean(anchorEl);

  const loadProviders = useCallback(async () => {
    try {
      const configs = await invoke<ProviderConfigEntry[]>("list_provider_configs");
      if (configs && configs.length > 0) {
        setProviders(configs);
        const active = configs.find((p) => p.isActive);
        if (active) {
          setActiveProvider(active);
        }
      }
    } catch (err) {
      console.error("Failed to load providers:", err);
    }
  }, []);

  useEffect(() => {
    loadProviders();
    // Refresh every 30 seconds to catch changes from other windows/sessions
    const interval = setInterval(loadProviders, 30000);
    return () => clearInterval(interval);
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
    if (provider.isActive) {
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
      }>("quick_switch_provider", { providerName: provider.name });

      // Update local state
      setProviders((prev) =>
        prev.map((p) => ({
          ...p,
          isActive: p.name === provider.name,
        }))
      );
      setActiveProvider(provider);
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
    const typeName = PROVIDER_DISPLAY_NAMES[provider.providerType] || provider.providerType;
    return {
      label: provider.name,
      sublabel: `${typeName} · ${provider.model}`,
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
          sx={{
            borderRadius: 2,
            textTransform: "none",
            fontSize: "0.75rem",
          }}
        >
          Setup Models
        </Button>
      </Tooltip>
    );
  }

  const display = activeProvider ? getProviderDisplay(activeProvider) : null;

  return (
    <Box>
      <Tooltip title="Click to switch model provider">
        <Button
          size="small"
          variant="outlined"
          onClick={handleClick}
          endIcon={loading ? <CircularProgress size={12} /> : null}
          sx={{
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
          }}
        >
          {display ? (
            <Box sx={{ display: "flex", alignItems: "center", gap: 0.5 }}>
              <Chip
                size="small"
                color="success"
                variant="outlined"
                label={display.label}
                sx={{
                  height: 20,
                  fontSize: "0.7rem",
                  "& .MuiChip-label": { px: 0.5 },
                }}
              />
              <Typography variant="caption" color="text.secondary" noWrap sx={{ maxWidth: 100 }}>
                {display.sublabel}
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
            Choose an active configuration
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
              selected={provider.isActive}
              sx={{
                py: 1.5,
                borderBottom: 1,
                borderColor: "divider",
                "&:last-child": { borderBottom: 0 },
              }}
            >
              <ListItemIcon>
                {isSwitching ? (
                  <CircularProgress size={16} />
                ) : provider.isActive ? (
                  <RadioButtonChecked color="success" fontSize="small" />
                ) : (
                  <RadioButtonUnchecked fontSize="small" />
                )}
              </ListItemIcon>
              <ListItemText
                primary={
                  <Box sx={{ display: "flex", alignItems: "center", gap: 1 }}>
                    <Typography fontWeight={provider.isActive ? 600 : 400}>
                      {display.label}
                    </Typography>
                    {provider.isActive && (
                      <Chip
                        size="small"
                        color="success"
                        label="Active"
                        sx={{ height: 18, fontSize: "0.65rem" }}
                      />
                    )}
                  </Box>
                }
                secondary={display.sublabel}
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
