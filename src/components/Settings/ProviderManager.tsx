import { useState, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  Box,
  Typography,
  Button,
  Chip,
  IconButton,
  Card,
  CardContent,
  CardActions,
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
} from "@mui/material";
import {
  CheckCircle,
  Delete,
  Edit,
  Add,
  Visibility,
  VisibilityOff,
  RadioButtonChecked,
  RadioButtonUnchecked,
} from "@mui/icons-material";

import { notifyProviderChanged } from "../../utils/providerEvents";

// Supported LLM providers with their display names and default models
type ProviderInfo = {
  name: string;
  defaultModel: string;
  placeholder: string;
  docsUrl: string;
  modelHelper?: string;
};

const PROVIDER_INFO: Record<string, ProviderInfo> = {
  anthropic: {
    name: "Anthropic (Claude)",
    defaultModel: "claude-3-5-sonnet-20241022",
    placeholder: "sk-ant-api03-...",
    docsUrl: "https://console.anthropic.com/settings/keys",
  },
  openai: {
    name: "OpenAI (GPT)",
    defaultModel: "gpt-4o",
    placeholder: "sk-...",
    docsUrl: "https://platform.openai.com/api-keys",
  },
  azure: {
    name: "Azure OpenAI",
    defaultModel: "gpt-4",
    placeholder: "https://{resource}.openai.azure.com/",
    docsUrl: "https://portal.azure.com/",
  },
  google: {
    name: "Google (Gemini)",
    defaultModel: "gemini-1.5-pro",
    placeholder: "AIzaSy...",
    docsUrl: "https://aistudio.google.com/app/apikey",
  },
  minimax: {
    name: "MiniMax",
    defaultModel: "abab6.5-chat",
    placeholder: "Enter MiniMax API Key",
    docsUrl: "https://www.minimaxi.com/user-center/basic-information/interface-key",
  },
  alibaba: {
    name: "Alibaba (通义千问/Qwen)",
    defaultModel: "qwen-max",
    placeholder: "sk-...",
    docsUrl: "https://dashscope.console.aliyun.com/apiKey",
  },
  deepseek: {
    name: "DeepSeek",
    defaultModel: "deepseek-chat",
    placeholder: "sk-...",
    docsUrl: "https://platform.deepseek.com/api_keys",
  },
  zhipu: {
    name: "Zhipu (ChatGLM)",
    defaultModel: "glm-4",
    placeholder: "Enter API Key",
    docsUrl: "https://open.bigmodel.cn/usercenter/apikey",
  },
  moonshot: {
    name: "Moonshot (Kimi/月之暗面)",
    defaultModel: "kimi-k2-0905-preview",
    placeholder: "sk-...",
    docsUrl: "https://platform.moonshot.ai/docs/overview",
    modelHelper:
      "Do not use “Kimi For Coding” / coding-only model ids or the coding API base — they only work in Kimi CLI, Claude Code, Roo Code, etc. Use general models: kimi-k2-0905-preview, kimi-k2.5, or moonshot-v1-8k.",
  },
  custom: {
    name: "Custom (OpenAI-compatible)",
    defaultModel: "",
    placeholder: "Enter API Key",
    docsUrl: "",
  },
};

interface ProviderConfigEntry {
  name: string;
  providerType: string;
  model: string;
  apiKeyPreview: string;
  baseUrl: string | null;
  /** Moonshot / Custom：是否启用 `thinking` + `reasoning_content` */
  thinking?: boolean | null;
  enabled: boolean;
  isSessionActive: boolean;
  isDefault: boolean;
}

interface ProviderManagerProps {
  onActiveProviderChange?: (provider: string, model: string) => void;
}

export function ProviderManager({ onActiveProviderChange }: ProviderManagerProps) {
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
  const [formThinking, setFormThinking] = useState(false);
  const [showApiKey, setShowApiKey] = useState(false);
  const [formSetAsDefault, setFormSetAsDefault] = useState(true);

  const providerSupportsThinking = (providerType: string) =>
    providerType === "moonshot" || providerType === "custom";

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
      }>("quick_switch_provider", { providerName: name });

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

  // Open add dialog
  const handleAddNew = () => {
    setEditingProvider(null);
    setFormName("");
    setFormProviderType("deepseek");
    setFormApiKey("");
    setFormModel(PROVIDER_INFO["deepseek"].defaultModel);
    setFormBaseUrl("");
    setFormThinking(false);
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
    setFormThinking(provider.thinking === true);
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

    try {
      setLoading(true);
      setDialogError(null);

      // Use placeholder if editing and key not changed
      const apiKeyToSave = formApiKey.trim() || (editingProvider ? "${KEEP_EXISTING}" : "");

      // Tauri maps camelCase keys to Rust snake_case. Pass explicit booleans for Moonshot/Custom so
      // `thinking` is never omitted when false (Some(false) in Rust).
      await invoke("save_provider_config", {
        name: formName.trim(),
        providerType: formProviderType,
        apiKey: apiKeyToSave,
        model: formModel.trim(),
        baseUrl: formBaseUrl.trim() || undefined,
        setAsDefault: formSetAsDefault,
        thinking: providerSupportsThinking(formProviderType) ? formThinking : null,
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
        <List sx={{ bgcolor: "background.paper", borderRadius: 1 }}>
          {providers.map((provider) => {
            const info = getProviderDisplay(provider.providerType);

            return (
              <ListItem
                key={provider.name}
                sx={{
                  borderBottom: 1,
                  borderColor: "divider",
                  bgcolor: provider.isSessionActive ? "action.selected" : "inherit",
                  "&:hover": { bgcolor: "action.hover" },
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
                        <Chip label="Default" size="small" variant="outlined" sx={{ ml: 0.5 }} />
                      )}
                    </Box>
                  }
                  secondary={
                    <Box sx={{ mt: 0.5 }}>
                      <Typography variant="body2" color="text.secondary">
                        {info.name} · {provider.model}
                      </Typography>
                      <Typography variant="caption" color="text.secondary" display="block">
                        API Key: {provider.apiKeyPreview || "***"}
                      </Typography>
                    </Box>
                  }
                />
                <ListItemSecondaryAction>
                  <Box sx={{ display: "flex", gap: 0.5 }}>
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
                          color={provider.isSessionActive ? "success" : "default"}
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
                      <IconButton size="small" onClick={() => handleEdit(provider)} disabled={loading}>
                        <Edit fontSize="small" />
                      </IconButton>
                    </Tooltip>
                    <Tooltip title="Delete">
                      <IconButton
                        size="small"
                        onClick={() => handleDelete(provider.name)}
                        disabled={loading || provider.isSessionActive}
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
                  setFormModel(PROVIDER_INFO[v]?.defaultModel || "");
                  if (!providerSupportsThinking(v)) {
                    setFormThinking(false);
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

            <TextField
              label="Model *"
              value={formModel}
              onChange={(e) => setFormModel(e.target.value)}
              placeholder={PROVIDER_INFO[formProviderType]?.defaultModel}
              helperText={
                (PROVIDER_INFO[formProviderType]?.modelHelper
                  ? `${PROVIDER_INFO[formProviderType].modelHelper} `
                  : "") +
                `Exact model ID for ${PROVIDER_INFO[formProviderType]?.name || formProviderType}.`
              }
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
                <Typography
                  variant="caption"
                  color="text.secondary"
                  sx={{ display: "block", fontWeight: 600 }}
                >
                  Thinking（本配置专用）
                </Typography>
                <Typography
                  variant="caption"
                  color="text.disabled"
                  sx={{ display: "block", mb: 0.5, lineHeight: 1.5 }}
                >
                  仅对模型 ID 含{" "}
                  <Typography component="span" fontFamily="monospace" fontSize="0.7rem">
                    kimi-k2.5
                  </Typography>{" "}
                  的 Kimi 请求生效：接口要求 thinking 为对象 type 为 enabled/disabled（不能传布尔）。流式字段{" "}
                  <Typography component="span" fontFamily="monospace" fontSize="0.7rem">
                    reasoning_content
                  </Typography>
                  。自定义 Base 需为 Moonshot 域名时才会附带。
                </Typography>
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
                    <Box>
                      <Typography variant="body2" fontWeight={600}>
                        启用 Thinking
                      </Typography>
                      <Typography variant="caption" color="text.secondary">
                        关闭时请求传 thinking: false（仅 Moonshot / 自定义；DeepSeek 无此项）
                      </Typography>
                    </Box>
                  }
                  sx={{
                    alignItems: "flex-start",
                    ml: 0,
                    "& .MuiFormControlLabel-label": { mt: 0.25 },
                  }}
                />
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
