import { useState, useEffect, useCallback, useMemo } from "react";
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
  Tooltip,
  alpha,
  useTheme,
} from "@mui/material";
import { Settings } from "@mui/icons-material";
import { NotificationToast } from "../NotificationToast";

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

const CUSTOM_MODEL_MENU_LABEL = "+ 配置自定义模型";

const estimateLabelWidthCh = (value: string) =>
  Array.from(value).reduce((width, char) => {
    if (char.charCodeAt(0) > 255) return width + 2;
    if (char === " ") return width + 0.5;
    return width + 1;
  }, 0);

export function ProviderSwitcher({
  onOpenSettings,
  triggerSx,
}: ProviderSwitcherProps) {
  const currentSessionId = useSessionStore((s) => s.currentSession?.id);
  const theme = useTheme();
  const isDark = theme.palette.mode === "dark";
  const [providers, setProviders] = useState<ProviderConfigEntry[]>([]);
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
    return {
      /** Raw configuration entry name, used for switching and notifications. */
      configName: provider.name,
      /** Model identifier shown as the primary label. */
      modelName: provider.model,
    };
  };

  const switcherWidth = useMemo(() => {
    const longestLabelWidth = providers.reduce(
      (width, provider) => Math.max(width, estimateLabelWidthCh(provider.model)),
      estimateLabelWidthCh(CUSTOM_MODEL_MENU_LABEL),
    );
    return `min(calc(${Math.ceil(longestLabelWidth)}ch + 18px), calc(100vw - 32px))`;
  }, [providers]);

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
    ? activeProvider.model
    : "Click to switch model provider";

  return (
    <Box>
      <Tooltip title={triggerTooltip}>
        <Button
          size="small"
          variant="outlined"
          onClick={handleClick}
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
            {
              width: switcherWidth,
              minWidth: 0,
              maxWidth: "calc(100vw - 32px)",
              px: 1,
            },
            ...(Array.isArray(triggerSx) ? triggerSx : triggerSx ? [triggerSx] : []),
          ]}
        >
          {display ? (
            <Box
              sx={{
                display: "flex",
                alignItems: "center",
                minWidth: 0,
                maxWidth: "100%",
              }}
            >
              <Typography
                variant="caption"
                sx={{
                  flex: 1,
                  minWidth: 0,
                  color: "inherit",
                  textAlign: "left",
                  fontWeight: 600,
                  maxWidth: "100%",
                  overflow: "visible",
                  textOverflow: "clip",
                  whiteSpace: "nowrap",
                }}
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
          vertical: "top",
          horizontal: "right",
        }}
        transformOrigin={{
          vertical: "bottom",
          horizontal: "right",
        }}
        PaperProps={{
          sx: {
            mt: -1,
            width: switcherWidth,
            minWidth: 0,
            maxWidth: "calc(100vw - 32px)",
            maxHeight: 360,
            borderRadius: "8px",
            border: 1,
            borderColor: "divider",
            boxShadow: isDark
              ? "0 18px 48px rgba(0, 0, 0, 0.42)"
              : "0 18px 48px rgba(15, 23, 42, 0.16)",
            overflowX: "hidden",
            overflowY: "auto",
          },
        }}
        MenuListProps={{
          dense: true,
          sx: { py: 0.5 },
        }}
      >
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
                mx: 0.75,
                my: 0.25,
                px: 1.25,
                py: 0,
                minHeight: 36,
                width: "auto",
                maxWidth: "none",
                borderRadius: "8px",
                "&.Mui-selected": {
                  bgcolor: alpha(
                    theme.palette.success.main,
                    isDark ? 0.14 : 0.08,
                  ),
                  boxShadow: `inset 0 0 0 1px ${alpha(
                    theme.palette.success.main,
                    isDark ? 0.26 : 0.2,
                  )}`,
                  "&:hover": {
                    bgcolor: alpha(
                      theme.palette.success.main,
                      isDark ? 0.18 : 0.12,
                    ),
                  },
                },
                "&:hover": {
                  bgcolor: alpha(theme.palette.text.primary, isDark ? 0.08 : 0.04),
                },
              }}
            >
              <Box
                sx={{
                  display: "flex",
                  flexDirection: "column",
                  justifyContent: "center",
                  minWidth: 0,
                  width: "100%",
                }}
              >
                <Typography
                  sx={{
                    flex: 1,
                    minWidth: 0,
                    fontSize: "0.8125rem",
                    fontWeight: provider.isSessionActive ? 600 : 500,
                    lineHeight: 1.25,
                    color: "text.primary",
                    maxWidth: "100%",
                    overflow: "visible",
                    textOverflow: "clip",
                    whiteSpace: "nowrap",
                  }}
                >
                  {display.modelName}
                </Typography>
                <Typography
                  sx={{
                    width: "100%",
                    minWidth: 0,
                    fontSize: "0.6875rem",
                    lineHeight: 1.2,
                    color: "text.secondary",
                    overflow: "hidden",
                    textOverflow: "ellipsis",
                    whiteSpace: "nowrap",
                  }}
                >
                  {display.configName}
                </Typography>
              </Box>
            </MenuItem>
          );
        })}

        <MenuItem
          onClick={() => {
            handleClose();
            onOpenSettings?.();
          }}
          sx={{
            mx: 0,
            mt: 0.5,
            px: 1.5,
            py: 0,
            minHeight: 38,
            borderRadius: 0,
            borderTop: 1,
            borderColor: "divider",
            color: "text.secondary",
            fontWeight: 600,
            "&:hover": {
              bgcolor: alpha(theme.palette.success.main, isDark ? 0.14 : 0.08),
              color: "success.main",
            },
          }}
        >
          <Typography
            sx={{
              fontSize: "0.8125rem",
              fontWeight: 600,
              lineHeight: 1.25,
              whiteSpace: "nowrap",
            }}
          >
            {CUSTOM_MODEL_MENU_LABEL}
          </Typography>
        </MenuItem>
      </Menu>

      <NotificationToast
        open={!!error}
        autoHideDuration={6000}
        onClose={() => setError(null)}
        severity="error"
        title="切换失败"
        message={error}
      />

      <NotificationToast
        open={!!success}
        autoHideDuration={3000}
        onClose={() => setSuccess(null)}
        severity="success"
        title="模型配置已切换"
        message={success}
      />
    </Box>
  );
}
