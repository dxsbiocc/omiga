import { useState, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  Box,
  Typography,
  Button,
  Chip,
  IconButton,
  List,
  ListItem,
  ListItemText,
  ListItemSecondaryAction,
  Tooltip,
  Alert,
  CircularProgress,
  Dialog,
  DialogTitle,
  DialogContent,
  DialogActions,
  TextField,
  FormControl,
  InputLabel,
  Select,
  MenuItem,
  InputAdornment,
  FormControlLabel,
  Switch,
  Autocomplete,
  alpha,
  useTheme,
} from "@mui/material";
import { darken } from "@mui/material/styles";
import {
  CheckCircle,
  Delete,
  Edit,
  Add,
  Visibility,
  VisibilityOff,
  RadioButtonChecked,
  RadioButtonUnchecked,
  Star,
  StarBorder,
} from "@mui/icons-material";

import { notifyProviderChanged } from "../../utils/providerEvents";

// Supported LLM providers with their display names and default models
type ProviderInfo = {
  name: string;
  defaultModel: string;
  models: string[];
  placeholder: string;
  docsUrl: string;
  modelHelper?: string;
  defaultContextWindowTokens?: number;
  modelContextWindowTokens?: Record<string, number>;
};

const FALLBACK_CONTEXT_WINDOW_TOKENS = 131_072;

const PROVIDER_INFO: Record<string, ProviderInfo> = {
  anthropic: {
    name: "Anthropic (Claude)",
    defaultModel: "claude-3-5-sonnet-20241022",
    models: [
      "claude-opus-4-7",
      "claude-sonnet-4-6",
      "claude-haiku-4-5-20251001",
      "claude-3-5-sonnet-20241022",
      "claude-3-5-haiku-20241022",
      "claude-3-opus-20240229",
    ],
    placeholder: "sk-ant-api03-...",
    docsUrl: "https://console.anthropic.com/settings/keys",
    defaultContextWindowTokens: 200_000,
  },
  openai: {
    name: "OpenAI (GPT)",
    defaultModel: "gpt-4o",
    models: ["gpt-4o", "gpt-4o-mini", "gpt-4-turbo", "o1", "o1-mini", "o3", "o3-mini", "o4-mini"],
    placeholder: "sk-...",
    docsUrl: "https://platform.openai.com/api-keys",
    defaultContextWindowTokens: 131_072,
  },
  azure: {
    name: "Azure OpenAI",
    defaultModel: "gpt-4",
    models: ["gpt-4", "gpt-4o", "gpt-4-turbo", "gpt-35-turbo"],
    placeholder: "https://{resource}.openai.azure.com/",
    docsUrl: "https://portal.azure.com/",
    defaultContextWindowTokens: 131_072,
  },
  google: {
    name: "Google (Gemini)",
    defaultModel: "gemini-1.5-pro",
    models: [
      "gemini-2.5-pro",
      "gemini-2.5-flash",
      "gemini-2.0-flash",
      "gemini-1.5-pro",
      "gemini-1.5-flash",
    ],
    placeholder: "AIzaSy...",
    docsUrl: "https://aistudio.google.com/app/apikey",
    defaultContextWindowTokens: 128_000,
  },
  minimax: {
    name: "MiniMax",
    defaultModel: "abab6.5-chat",
    models: ["abab6.5-chat", "abab6.5s-chat", "abab5.5-chat"],
    placeholder: "Enter MiniMax API Key",
    docsUrl: "https://www.minimaxi.com/user-center/basic-information/interface-key",
    defaultContextWindowTokens: 128_000,
  },
  alibaba: {
    name: "Alibaba (通义千问/Qwen)",
    defaultModel: "qwen-max",
    models: ["qwen-max", "qwen-plus", "qwen-turbo", "qwen-long", "qwen3-235b-a22b"],
    placeholder: "sk-...",
    docsUrl: "https://dashscope.console.aliyun.com/apiKey",
    defaultContextWindowTokens: 128_000,
  },
  deepseek: {
    name: "DeepSeek",
    defaultModel: "deepseek-v4-flash",
    models: ["deepseek-v4-flash", "deepseek-v4-pro"],
    placeholder: "sk-...",
    docsUrl: "https://platform.deepseek.com/api_keys",
    defaultContextWindowTokens: 1_000_000,
    modelContextWindowTokens: {
      "deepseek-v4-flash": 1_000_000,
      "deepseek-v4-pro": 1_000_000,
    },
    modelHelper:
      "推荐模型：deepseek-v4-flash（快速，支持思考模式）或 deepseek-v4-pro（高性能，支持思考模式）。" +
      "旧模型 deepseek-chat / deepseek-reasoner 已于 2026/07/24 弃用。",
  },
  zhipu: {
    name: "Zhipu (ChatGLM)",
    defaultModel: "glm-4",
    models: ["glm-4", "glm-4-flash", "glm-4-air", "glm-4-airx", "glm-z1"],
    placeholder: "Enter API Key",
    docsUrl: "https://open.bigmodel.cn/usercenter/apikey",
    defaultContextWindowTokens: 128_000,
  },
  moonshot: {
    name: "Moonshot (Kimi/月之暗面)",
    defaultModel: "kimi-k2-0905-preview",
    models: ["kimi-k2-0905-preview", "kimi-k2.5", "moonshot-v1-8k", "moonshot-v1-32k", "moonshot-v1-128k"],
    placeholder: "sk-...",
    docsUrl: "https://platform.moonshot.ai/docs/overview",
    defaultContextWindowTokens: 128_000,
    modelContextWindowTokens: {
      "moonshot-v1-8k": 8_192,
      "moonshot-v1-32k": 32_768,
      "moonshot-v1-128k": 131_072,
    },
    modelHelper:
      "Do not use 'Kimi For Coding' / coding-only model ids or the coding API base — they only work in Kimi CLI, Claude Code, Roo Code, etc. Use general models: kimi-k2-0905-preview, kimi-k2.5, or moonshot-v1-8k.",
  },
  custom: {
    name: "Custom (OpenAI-compatible)",
    defaultModel: "",
    models: [],
    placeholder: "Enter API Key",
    docsUrl: "",
    defaultContextWindowTokens: 131_072,
  },
};

function defaultContextWindowTokens(providerType: string, model: string): number {
  const info = PROVIDER_INFO[providerType];
  const exact = info?.modelContextWindowTokens?.[model.trim()];
  if (exact) return exact;
  if (providerType === "deepseek" && model.toLowerCase().includes("v4")) {
    return 1_000_000;
  }
  return info?.defaultContextWindowTokens ?? FALLBACK_CONTEXT_WINDOW_TOKENS;
}

function formatTokenCount(tokens?: number | null): string {
  if (!tokens || !Number.isFinite(tokens)) return "auto";
  if (tokens >= 1_000_000) {
    const millions = tokens / 1_000_000;
    return `${Number.isInteger(millions) ? millions.toFixed(0) : millions.toFixed(1)}M`;
  }
  if (tokens >= 1_000) {
    return `${Math.round(tokens / 1_000)}K`;
  }
  return tokens.toLocaleString();
}

interface ProviderConfigEntry {
  name: string;
  providerType: string;
  model: string;
  apiKeyPreview: string;
  baseUrl: string | null;
  /** Model context window capacity in tokens, used by auto-compaction. */
  contextWindowTokens?: number | null;
  /** Moonshot / Custom / DeepSeek：是否启用 `thinking` + `reasoning_content` */
  thinking?: boolean | null;
  /** DeepSeek only: "high" or "max" */
  reasoningEffort?: string | null;
  enabled: boolean;
  isSessionActive: boolean;
  isDefault: boolean;
}

interface ProviderManagerProps {
  onActiveProviderChange?: (provider: string, model: string) => void;
  /** When set, quick-switch is persisted for this chat session only. */
  sessionId?: string | null;
}

export function ProviderManager({
  onActiveProviderChange,
  sessionId,
}: ProviderManagerProps) {
  const theme = useTheme();
  const isDark = theme.palette.mode === "dark";
  const selectedRowBg = alpha(
    theme.palette.primary.main,
    isDark ? 0.14 : 0.1,
  );
  const selectedRowHoverBg = alpha(
    theme.palette.primary.main,
    isDark ? 0.2 : 0.14,
  );
  // List of configured providers
  const [providers, setProviders] = useState<ProviderConfigEntry[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useState<string | null>(null);

  // Add/Edit dialog state
  const [dialogOpen, setDialogOpen] = useState(false);
  const [editingProvider, setEditingProvider] = useState<ProviderConfigEntry | null>(null);
  const [dialogError, setDialogError] = useState<string | null>(null);

  // Form state
  const [formName, setFormName] = useState("");
  const [formProviderType, setFormProviderType] = useState("deepseek");
  const [formApiKey, setFormApiKey] = useState("");
  const [formModel, setFormModel] = useState("");
  const [formBaseUrl, setFormBaseUrl] = useState("");
  const [formContextWindowTokens, setFormContextWindowTokens] = useState("");
  const [formContextWindowTouched, setFormContextWindowTouched] = useState(false);
  const [formThinking, setFormThinking] = useState(true);
  const [formReasoningEffort, setFormReasoningEffort] = useState<"high" | "max">("high");
  const [showApiKey, setShowApiKey] = useState(false);
  const [formSetAsDefault, setFormSetAsDefault] = useState(true);

  const providerSupportsThinking = (providerType: string) =>
    providerType === "moonshot" || providerType === "custom" || providerType === "deepseek";

  // Load providers on mount
  const loadProviders = useCallback(async () => {
    try {
      setLoading(true);
      setError(null);
      const configs = await invoke<ProviderConfigEntry[]>("list_provider_configs");
      setProviders(configs || []);
    } catch (err) {
      console.error("Failed to load providers:", err);
      setError(`Failed to load configurations: ${err}`);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    loadProviders();
  }, [loadProviders]);

  // Handle quick switch to a provider
  const handleSwitchProvider = async (name: string) => {
    try {
      setLoading(true);
      setError(null);

      const result = await invoke<{
        provider: string;
        model: string | null;
        apiKeyPreview: string;
      }>("quick_switch_provider", {
        providerName: name,
        sessionId: sessionId ?? null,
      });

      // Update local state
      setProviders((prev) =>
        prev.map((p) => ({
          ...p,
          isSessionActive: p.name === name,
        }))
      );

      setSuccess(`Switched to ${name} (${result.provider})`);
      setTimeout(() => setSuccess(null), 3000);

      // Notify parent
      if (onActiveProviderChange && result.model) {
        onActiveProviderChange(result.provider, result.model);
      }
      notifyProviderChanged();
    } catch (err) {
      setError(`Failed to switch: ${err}`);
    } finally {
      setLoading(false);
    }
  };

  /** 仅写入 omiga.yaml 的 default_provider，不改变当前会话正在用的模型 */
  const handleSetDefaultProvider = async (name: string) => {
    try {
      setLoading(true);
      setError(null);
      await invoke("set_default_provider_config", { providerName: name });
      await loadProviders();
      setSuccess(`「${name}」已设为默认启动模型`);
      setTimeout(() => setSuccess(null), 3000);
      notifyProviderChanged();
    } catch (err) {
      setError(`设置默认失败: ${err}`);
    } finally {
      setLoading(false);
    }
  };

  // Open add dialog
  const handleAddNew = () => {
    setEditingProvider(null);
    setFormName("");
    setFormProviderType("deepseek");
    setFormApiKey("");
    setFormModel(PROVIDER_INFO["deepseek"].defaultModel);
    setFormBaseUrl("");
    setFormContextWindowTokens(
      String(defaultContextWindowTokens("deepseek", PROVIDER_INFO["deepseek"].defaultModel)),
    );
    setFormContextWindowTouched(false);
    setFormThinking(true);
    setFormReasoningEffort("high");
    setFormSetAsDefault(true);
    setDialogError(null);
    setDialogOpen(true);
  };

  // Open edit dialog — API key field is empty on purpose: full key is never sent to the UI.
  // Saving with a blank field sends `${KEEP_EXISTING}` so the file-backed key is preserved.
  const handleEdit = (provider: ProviderConfigEntry) => {
    setEditingProvider(provider);
    setFormName(provider.name);
    setFormProviderType(provider.providerType);
    setFormApiKey("");
    setFormModel(provider.model);
    setFormBaseUrl(provider.baseUrl || "");
    setFormContextWindowTokens(
      String(
        provider.contextWindowTokens ??
          defaultContextWindowTokens(provider.providerType, provider.model),
      ),
    );
    setFormContextWindowTouched(false);
    setFormThinking(provider.thinking === true);
    setFormReasoningEffort(
      provider.reasoningEffort === "max" ? "max" : "high",
    );
    setFormSetAsDefault(false);
    setDialogError(null);
    setDialogOpen(true);
  };

  // Handle delete
  const handleDelete = async (name: string) => {
    if (!confirm(`Delete provider configuration "${name}"?`)) return;

    try {
      setLoading(true);
      await invoke("delete_provider_config", { name });
      await loadProviders();
      setSuccess(`Deleted ${name}`);
      setTimeout(() => setSuccess(null), 3000);
    } catch (err) {
      setError(`Failed to delete: ${err}`);
    } finally {
      setLoading(false);
    }
  };

  // Handle save
  const handleSave = async () => {
    if (!formName.trim()) {
      setDialogError("Please enter a configuration name");
      return;
    }
    if (!formApiKey.trim() && !editingProvider) {
      setDialogError("Please enter an API key");
      return;
    }
    if (!formModel.trim()) {
      setDialogError("Please enter a model name");
      return;
    }
    const contextWindowTokens = Number.parseInt(formContextWindowTokens, 10);
    if (!Number.isFinite(contextWindowTokens) || contextWindowTokens < 8192) {
      setDialogError("最大上下文容量至少需要 8192 tokens");
      return;
    }

    try {
      setLoading(true);
      setDialogError(null);

      // Use placeholder if editing and key not changed
      const apiKeyToSave = formApiKey.trim() || (editingProvider ? "${KEEP_EXISTING}" : "");

      // Tauri maps camelCase keys to Rust snake_case. Pass explicit booleans for Moonshot/Custom so
      // `thinking` is never omitted when false (Some(false) in Rust).
      await invoke("save_provider_config", {
        request: {
          name: formName.trim(),
          providerType: formProviderType,
          apiKey: apiKeyToSave,
          model: formModel.trim(),
          baseUrl: formBaseUrl.trim() || undefined,
          contextWindowTokens,
          setAsDefault: formSetAsDefault,
          thinking: providerSupportsThinking(formProviderType) ? formThinking : null,
          reasoningEffort:
            formProviderType === "deepseek" && formThinking
              ? formReasoningEffort
              : null,
        },
      });

      await loadProviders();
      notifyProviderChanged();
      setDialogOpen(false);
      setDialogError(null);
      setSuccess(`Saved ${formName}`);
      setTimeout(() => setSuccess(null), 3000);
    } catch (err) {
      setDialogError(`Failed to save: ${err}`);
    } finally {
      setLoading(false);
    }
  };

  // Get display info for a provider type
  const getProviderDisplay = (type: string) => {
    return PROVIDER_INFO[type] || { name: type, defaultModel: "" };
  };

  return (
    <Box>
      {/* Status messages */}
      {error && (
        <Alert severity="error" sx={{ mb: 2 }} onClose={() => setError(null)}>
          {error}
        </Alert>
      )}
      {success && (
        <Alert severity="success" sx={{ mb: 2 }} onClose={() => setSuccess(null)}>
          {success}
        </Alert>
      )}

      {/* Header */}
      <Box sx={{ display: "flex", justifyContent: "space-between", alignItems: "center", mb: 2 }}>
        <Typography variant="h6" fontWeight={600}>
          Model Configurations
        </Typography>
        <Button
          variant="contained"
          size="small"
          startIcon={<Add />}
          onClick={handleAddNew}
          disabled={loading}
        >
          Add New
        </Button>
      </Box>

      {/* Provider List */}
      {providers.length === 0 ? (
        <Alert severity="info" sx={{ mb: 2 }}>
          No model configurations saved yet. Click "Add New" to add your first provider
          (DeepSeek, Kimi, Qwen, etc.).
        </Alert>
      ) : (
        <>
          <Alert severity="info" sx={{ mb: 2 }}>
            <Typography variant="body2" component="div">
              <strong>默认</strong>：下次启动、新会话使用的配置（点击星标设置）。
              <strong> 当前会话</strong>：右侧圆圈仅切换本轮对话，不会改默认。
            </Typography>
          </Alert>
          <List sx={{ bgcolor: "background.paper", borderRadius: 1 }}>
          {providers.map((provider) => {
            const info = getProviderDisplay(provider.providerType);

            return (
              <ListItem
                key={provider.name}
                sx={{
                  borderBottom: 1,
                  borderColor: "divider",
                  bgcolor: provider.isSessionActive ? selectedRowBg : "inherit",
                  "&:hover": {
                    bgcolor: provider.isSessionActive
                      ? selectedRowHoverBg
                      : "action.hover",
                  },
                }}
              >
                <ListItemText
                  primary={
                    <Box sx={{ display: "flex", alignItems: "center", gap: 1 }}>
                      <Typography fontWeight={600}>{provider.name}</Typography>
                      {provider.isSessionActive && (
                        <Chip
                          icon={<CheckCircle />}
                          label="In use"
                          size="small"
                          color="success"
                          variant="outlined"
                        />
                      )}
                      {provider.isDefault && (
                        <Chip
                          label="Default"
                          size="small"
                          color="warning"
                          variant="filled"
                          sx={{
                            ml: 0.5,
                            fontWeight: 700,
                            color: (t) =>
                              t.palette.mode === "dark"
                                ? t.palette.warning.contrastText
                                : darken(t.palette.warning.main, 0.58),
                            bgcolor: (t) =>
                              alpha(
                                t.palette.warning.main,
                                t.palette.mode === "dark" ? 0.45 : 0.28,
                              ),
                            border: (t) =>
                              `1px solid ${alpha(darken(t.palette.warning.main, 0.2), 0.45)}`,
                            boxShadow: (t) =>
                              `inset 0 1px 0 ${alpha(t.palette.common.white, 0.15)}`,
                          }}
                        />
                      )}
                    </Box>
                  }
                  secondary={
                    <Box sx={{ mt: 0.5 }}>
                      <Typography variant="body2" color="text.secondary">
                        {info.name} · {provider.model}
                      </Typography>
                      <Typography variant="caption" color="text.secondary" display="block">
                        Context window:{" "}
                        {formatTokenCount(
                          provider.contextWindowTokens ??
                            defaultContextWindowTokens(provider.providerType, provider.model),
                        )}{" "}
                        tokens
                      </Typography>
                      <Typography variant="caption" color="text.secondary" display="block">
                        API Key: {provider.apiKeyPreview || "***"}
                      </Typography>
                    </Box>
                  }
                />
                <ListItemSecondaryAction>
                  <Box
                    sx={{
                      display: "flex",
                      gap: 0.25,
                      alignItems: "center",
                    }}
                  >
                    <Tooltip
                      title={
                        provider.isDefault
                          ? "已是默认启动模型"
                          : "设为默认启动模型（写入配置，不切换当前会话）"
                      }
                    >
                      <span>
                        <IconButton
                          size="small"
                          onClick={() => handleSetDefaultProvider(provider.name)}
                          disabled={loading || provider.isDefault}
                          aria-label={
                            provider.isDefault
                              ? "默认启动模型"
                              : "设为默认启动模型"
                          }
                          sx={(t) => {
                            const deepWarnFilled =
                              t.palette.mode === "dark"
                                ? t.palette.warning.main
                                : darken(t.palette.warning.main, 0.55);
                            return {
                              /* 默认模型：琥珀/金，与会话(绿)、编辑(主色)、删除(红) 区分 */
                              ...(provider.isDefault && {
                                bgcolor: alpha(
                                  t.palette.warning.main,
                                  t.palette.mode === "dark" ? 0.32 : 0.24,
                                ),
                                color: deepWarnFilled,
                              }),
                              ...(!provider.isDefault && {
                                color:
                                  t.palette.mode === "dark"
                                    ? alpha(t.palette.warning.light, 0.95)
                                    : darken(t.palette.warning.main, 0.12),
                              }),
                              "&:hover": {
                                bgcolor: provider.isDefault
                                  ? alpha(t.palette.warning.main, 0.4)
                                  : alpha(t.palette.warning.main, 0.16),
                              },
                              "&.Mui-disabled": {
                                color: provider.isDefault
                                  ? deepWarnFilled
                                  : alpha(t.palette.warning.main, 0.38),
                                opacity: provider.isDefault ? 1 : undefined,
                                ...(provider.isDefault && {
                                  bgcolor: alpha(
                                    t.palette.warning.main,
                                    t.palette.mode === "dark" ? 0.32 : 0.24,
                                  ),
                                }),
                              },
                            };
                          }}
                        >
                          {provider.isDefault ? (
                            <Star fontSize="small" />
                          ) : (
                            <StarBorder fontSize="small" />
                          )}
                        </IconButton>
                      </span>
                    </Tooltip>
                    <Tooltip
                      title={
                        provider.isSessionActive
                          ? "Current session uses this provider"
                          : "Use for this session only (does not change default)"
                      }
                    >
                      <span>
                        <IconButton
                          size="small"
                          onClick={() => handleSwitchProvider(provider.name)}
                          disabled={provider.isSessionActive || loading}
                          sx={(t) => {
                            const deepSuccessIcon =
                              t.palette.mode === "dark"
                                ? t.palette.success.main
                                : darken(t.palette.success.main, 0.48);
                            return {
                              ...(provider.isSessionActive && {
                                bgcolor: alpha(
                                  t.palette.success.main,
                                  t.palette.mode === "dark" ? 0.26 : 0.2,
                                ),
                                color: deepSuccessIcon,
                              }),
                              ...(!provider.isSessionActive && {
                                color:
                                  t.palette.mode === "dark"
                                    ? alpha(t.palette.primary.light, 0.92)
                                    : t.palette.primary.dark,
                              }),
                              "&:hover": {
                                bgcolor: provider.isSessionActive
                                  ? alpha(t.palette.success.main, 0.32)
                                  : alpha(t.palette.primary.main, 0.14),
                              },
                              "&.Mui-disabled": {
                                color: provider.isSessionActive
                                  ? deepSuccessIcon
                                  : alpha(t.palette.primary.main, 0.38),
                                opacity: provider.isSessionActive ? 1 : undefined,
                                ...(provider.isSessionActive && {
                                  bgcolor: alpha(
                                    t.palette.success.main,
                                    t.palette.mode === "dark" ? 0.26 : 0.2,
                                  ),
                                }),
                              },
                            };
                          }}
                        >
                          {provider.isSessionActive ? (
                            <RadioButtonChecked fontSize="small" />
                          ) : (
                            <RadioButtonUnchecked fontSize="small" />
                          )}
                        </IconButton>
                      </span>
                    </Tooltip>
                    <Tooltip title="Edit">
                      <IconButton
                        size="small"
                        onClick={() => handleEdit(provider)}
                        disabled={loading}
                        sx={(t) => ({
                          color:
                            t.palette.mode === "dark"
                              ? alpha(t.palette.primary.light, 0.92)
                              : t.palette.primary.dark,
                          "&:hover": {
                            bgcolor: alpha(t.palette.primary.main, 0.14),
                          },
                          "&.Mui-disabled": {
                            color: alpha(t.palette.primary.main, 0.38),
                          },
                        })}
                      >
                        <Edit fontSize="small" />
                      </IconButton>
                    </Tooltip>
                    <Tooltip title="Delete">
                      <IconButton
                        size="small"
                        onClick={() => handleDelete(provider.name)}
                        disabled={loading || provider.isSessionActive}
                        sx={(t) => ({
                          color:
                            t.palette.mode === "dark"
                              ? t.palette.error.light
                              : t.palette.error.dark,
                          "&:hover": {
                            bgcolor: alpha(t.palette.error.main, 0.14),
                          },
                          "&.Mui-disabled": {
                            color: alpha(t.palette.error.main, 0.38),
                          },
                        })}
                      >
                        <Delete fontSize="small" />
                      </IconButton>
                    </Tooltip>
                  </Box>
                </ListItemSecondaryAction>
              </ListItem>
            );
          })}
          </List>
        </>
      )}

      {/* Add/Edit Dialog */}
      <Dialog
        open={dialogOpen}
        onClose={() => {
          setDialogOpen(false);
          setDialogError(null);
        }}
        maxWidth="sm"
        fullWidth
      >
        <DialogTitle>
          {editingProvider ? "Edit Configuration" : "Add New Model Configuration"}
        </DialogTitle>
        <DialogContent>
          <Box sx={{ display: "flex", flexDirection: "column", gap: 2, mt: 1 }}>
            {/* Dialog Error Message */}
            {dialogError && (
              <Alert severity="error" onClose={() => setDialogError(null)}>
                {dialogError}
              </Alert>
            )}

            {/* Config Name */}
            <TextField
              label="Configuration Name"
              value={formName}
              onChange={(e) => setFormName(e.target.value)}
              disabled={!!editingProvider}
              placeholder="e.g., DeepSeek-Prod, Kimi-Dev, Qwen-Max"
              helperText="A unique name to identify this configuration"
              fullWidth
            />

            {/* Provider Type */}
            <FormControl fullWidth>
              <InputLabel>Provider</InputLabel>
              <Select
                value={formProviderType}
                label="Provider"
                onChange={(e) => {
                  const v = e.target.value;
                  setFormProviderType(v);
                  const nextModel = PROVIDER_INFO[v]?.defaultModel || "";
                  setFormModel(nextModel);
                  setFormContextWindowTokens(
                    String(defaultContextWindowTokens(v, nextModel)),
                  );
                  setFormContextWindowTouched(false);
                  if (!providerSupportsThinking(v)) {
                    setFormThinking(false);
                  } else {
                    setFormThinking(v === "deepseek");
                  }
                  if (v !== "deepseek") {
                    setFormReasoningEffort("high");
                  }
                }}
              >
                <MenuItem value="deepseek">DeepSeek</MenuItem>
                <MenuItem value="moonshot">Moonshot (Kimi/月之暗面)</MenuItem>
                <MenuItem value="alibaba">Alibaba (通义千问/Qwen)</MenuItem>
                <MenuItem value="zhipu">Zhipu (智谱/ChatGLM)</MenuItem>
                <MenuItem value="minimax">MiniMax</MenuItem>
                <MenuItem value="anthropic">Anthropic (Claude)</MenuItem>
                <MenuItem value="openai">OpenAI (GPT)</MenuItem>
                <MenuItem value="azure">Azure OpenAI</MenuItem>
                <MenuItem value="google">Google (Gemini)</MenuItem>
                <MenuItem value="custom">Custom (OpenAI-compatible)</MenuItem>
              </Select>
            </FormControl>

            {/* API Key — when editing, field is empty: key is not gone; see helperText below */}
            <TextField
              label={editingProvider ? "API Key" : "API Key *"}
              type={showApiKey ? "text" : "password"}
              value={formApiKey}
              onChange={(e) => setFormApiKey(e.target.value)}
              placeholder={
                editingProvider
                  ? "Leave blank to keep your saved key"
                  : PROVIDER_INFO[formProviderType]?.placeholder
              }
              helperText={
                editingProvider
                  ? `The full key is not shown for security. It is still saved. Preview: ${editingProvider.apiKeyPreview || "••••"}. Leave blank and save to keep it, or paste a new key to replace.`
                  : undefined
              }
              fullWidth
              InputProps={{
                endAdornment: (
                  <InputAdornment position="end">
                    <IconButton onClick={() => setShowApiKey(!showApiKey)} edge="end" size="small">
                      {showApiKey ? <VisibilityOff /> : <Visibility />}
                    </IconButton>
                  </InputAdornment>
                ),
              }}
            />

            <Autocomplete
              freeSolo
              options={PROVIDER_INFO[formProviderType]?.models ?? []}
              value={formModel}
              inputValue={formModel}
              onInputChange={(_, value) => {
                setFormModel(value);
                if (!formContextWindowTouched) {
                  setFormContextWindowTokens(
                    String(defaultContextWindowTokens(formProviderType, value)),
                  );
                }
              }}
              onChange={(_, value) => {
                if (typeof value === "string") {
                  setFormModel(value);
                  if (!formContextWindowTouched) {
                    setFormContextWindowTokens(
                      String(defaultContextWindowTokens(formProviderType, value)),
                    );
                  }
                }
              }}
              renderInput={(params) => (
                <TextField
                  {...params}
                  label="Model *"
                  placeholder={PROVIDER_INFO[formProviderType]?.defaultModel}
                  helperText={
                    (PROVIDER_INFO[formProviderType]?.modelHelper
                      ? `${PROVIDER_INFO[formProviderType].modelHelper} `
                      : "") +
                    `Exact model ID for ${PROVIDER_INFO[formProviderType]?.name || formProviderType}.`
                  }
                  fullWidth
                />
              )}
            />

            <TextField
              label="最大上下文容量（tokens）"
              type="number"
              value={formContextWindowTokens}
              onChange={(e) => {
                setFormContextWindowTokens(e.target.value);
                setFormContextWindowTouched(true);
              }}
              helperText={
                `用于自动压缩预算，不是输出 max_tokens。` +
                `当前模型建议值：${formatTokenCount(defaultContextWindowTokens(formProviderType, formModel))} tokens。`
              }
              inputProps={{ min: 8192, step: 1024 }}
              fullWidth
            />

            {/* Base URL (optional) */}
            <TextField
              label="Base URL (optional)"
              value={formBaseUrl}
              onChange={(e) => setFormBaseUrl(e.target.value)}
              placeholder="Override default API endpoint"
              helperText="Only needed for Azure or custom endpoints"
              fullWidth
            />

            {providerSupportsThinking(formProviderType) && (
              <>
                <FormControlLabel
                  control={
                    <Switch
                      checked={formThinking}
                      onChange={(_, v) => setFormThinking(v)}
                      disabled={loading}
                      color="primary"
                    />
                  }
                  label={
                    <Typography variant="body2" fontWeight={600}>
                      启用 Thinking
                    </Typography>
                  }
                  sx={{ ml: 0 }}
                />
                {formProviderType === "deepseek" && formThinking && (
                  <FormControl fullWidth size="small">
                    <InputLabel>Reasoning Effort</InputLabel>
                    <Select
                      value={formReasoningEffort}
                      label="Reasoning Effort"
                      onChange={(e) =>
                        setFormReasoningEffort(e.target.value as "high" | "max")
                      }
                      disabled={loading}
                    >
                      <MenuItem value="high">
                        high — 标准推理（适合大多数场景）
                      </MenuItem>
                      <MenuItem value="max">
                        max — 深度推理（适合复杂 Agent 任务）
                      </MenuItem>
                    </Select>
                  </FormControl>
                )}
              </>
            )}

            {/* Set as default */}
            {!editingProvider && (
              <Box sx={{ display: "flex", alignItems: "center", gap: 1 }}>
                <Button
                  variant={formSetAsDefault ? "contained" : "outlined"}
                  size="small"
                  onClick={() => setFormSetAsDefault(!formSetAsDefault)}
                >
                  {formSetAsDefault ? "Set as Active" : "Save Only"}
                </Button>
                <Typography variant="caption" color="text.secondary">
                  {formSetAsDefault
                    ? "This configuration will be activated immediately"
                    : "Save without switching to this configuration"}
                </Typography>
              </Box>
            )}

            {/* Help Link */}
            {PROVIDER_INFO[formProviderType]?.docsUrl && (
              <Typography variant="caption" color="text.secondary">
                Need an API key?{" "}
                <a
                  href={PROVIDER_INFO[formProviderType].docsUrl}
                  target="_blank"
                  rel="noopener noreferrer"
                  style={{ color: "inherit" }}
                >
                  Get one from {PROVIDER_INFO[formProviderType].name}
                </a>
              </Typography>
            )}
          </Box>
        </DialogContent>
        <DialogActions>
          <Button
            onClick={() => {
              setDialogOpen(false);
              setDialogError(null);
            }}
          >
            Cancel
          </Button>
          <Button onClick={handleSave} variant="contained" disabled={loading}>
            {loading ? <CircularProgress size={16} sx={{ mr: 1 }} /> : null}
            Save
          </Button>
        </DialogActions>
      </Dialog>
    </Box>
  );
}
