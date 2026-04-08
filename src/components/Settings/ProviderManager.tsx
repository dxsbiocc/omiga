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

// Supported LLM providers with their display names and default models
type ProviderInfo = {
  name: string;
  defaultModel: string;
  placeholder: string;
  docsUrl: string;
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
    defaultModel: "moonshot-v1-8k",
    placeholder: "sk-...",
    docsUrl: "https://platform.moonshot.cn/console/api-keys",
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
  enabled: boolean;
  isActive: boolean;
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
  const [showApiKey, setShowApiKey] = useState(false);
  const [formSetAsDefault, setFormSetAsDefault] = useState(true);

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
          isActive: p.name === name,
        }))
      );

      setSuccess(`Switched to ${name} (${result.provider})`);
      setTimeout(() => setSuccess(null), 3000);

      // Notify parent
      if (onActiveProviderChange && result.model) {
        onActiveProviderChange(result.provider, result.model);
      }
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
    setFormSetAsDefault(true);
    setDialogError(null);
    setDialogOpen(true);
  };

  // Open edit dialog
  const handleEdit = (provider: ProviderConfigEntry) => {
    setEditingProvider(provider);
    setFormName(provider.name);
    setFormProviderType(provider.providerType);
    setFormApiKey(""); // Don't show existing key
    setFormModel(provider.model);
    setFormBaseUrl(provider.baseUrl || "");
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

      await invoke("save_provider_config", {
        name: formName.trim(),
        providerType: formProviderType,
        apiKey: apiKeyToSave,
        model: formModel.trim(),
        baseUrl: formBaseUrl.trim() || undefined,
        setAsDefault: formSetAsDefault,
      });

      await loadProviders();
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
                  bgcolor: provider.isActive ? "action.selected" : "inherit",
                  "&:hover": { bgcolor: "action.hover" },
                }}
              >
                <ListItemText
                  primary={
                    <Box sx={{ display: "flex", alignItems: "center", gap: 1 }}>
                      <Typography fontWeight={600}>{provider.name}</Typography>
                      {provider.isActive && (
                        <Chip
                          icon={<CheckCircle />}
                          label="Active"
                          size="small"
                          color="success"
                          variant="outlined"
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
                        API Key: {provider.apiKeyPreview || "***"}
                      </Typography>
                    </Box>
                  }
                />
                <ListItemSecondaryAction>
                  <Box sx={{ display: "flex", gap: 0.5 }}>
                    <Tooltip title={provider.isActive ? "Already Active" : "Switch to this provider"}>
                      <span>
                        <IconButton
                          size="small"
                          onClick={() => handleSwitchProvider(provider.name)}
                          disabled={provider.isActive || loading}
                          color={provider.isActive ? "success" : "default"}
                        >
                          {provider.isActive ? (
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
                        disabled={loading || provider.isActive}
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
                  setFormProviderType(e.target.value);
                  // Auto-fill default model
                  setFormModel(PROVIDER_INFO[e.target.value]?.defaultModel || "");
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

            {/* API Key */}
            <TextField
              label={editingProvider ? "API Key (leave blank to keep existing)" : "API Key *"}
              type={showApiKey ? "text" : "password"}
              value={formApiKey}
              onChange={(e) => setFormApiKey(e.target.value)}
              placeholder={PROVIDER_INFO[formProviderType]?.placeholder}
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

            {/* Model */}
            <TextField
              label="Model *"
              value={formModel}
              onChange={(e) => setFormModel(e.target.value)}
              placeholder={PROVIDER_INFO[formProviderType]?.defaultModel}
              helperText={`Exact model ID for ${PROVIDER_INFO[formProviderType]?.name || formProviderType}`}
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
