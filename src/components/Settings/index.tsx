import { Fragment, useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  TextField,
  Button,
  Box,
  Typography,
  Alert,
  InputAdornment,
  IconButton,
  Chip,
  Divider,
  Link,
  CircularProgress,
  FormControl,
  InputLabel,
  Select,
  MenuItem,
  SelectChangeEvent,
  List,
  ListItemButton,
  ListItemText,
  ListSubheader,
} from "@mui/material";
import {
  Visibility,
  VisibilityOff,
  CheckCircle,
  OpenInNew,
  ArrowBack,
} from "@mui/icons-material";
import { useSessionStore } from "../../state/sessionStore";
import { useColorModeStore } from "../../state/themeStore";
import { PermissionSettingsTab } from "./PermissionSettingsTab";
import { NotebookSettingsTab } from "./NotebookSettingsTab";
import { ClaudeCodeImportPanel } from "./ClaudeCodeImportPanel";
import { IntegrationsCatalogPanel } from "./IntegrationsCatalogPanel";
import { UnifiedMemoryTab } from "./UnifiedMemoryTab";
import { ThemeAppearancePanel } from "./ThemeAppearancePanel";
import { ProviderManager } from "./ProviderManager";

interface SettingsProps {
  open: boolean;
  onClose: () => void;
  /** See `openSettingsTabMap.ts`: 0–7 */
  initialTab?: number;
}

/** Grouped sidebar — indices must match `openSettingsTabMap` */
const SETTINGS_SECTIONS: {
  header: string;
  items: { index: number; label: string }[];
}[] = [
  {
    header: "App",
    items: [
      { index: 0, label: "Model" },
      { index: 1, label: "Advanced" },
      { index: 2, label: "Permissions" },
      { index: 3, label: "Theme" },
      { index: 7, label: "Notebook" },
    ],
  },
  {
    header: "Integrations",
    items: [
      { index: 4, label: "Plugins" },
      { index: 5, label: "MCP" },
      { index: 6, label: "Skills" },
    ],
  },
  {
    header: "Knowledge",
    items: [{ index: 8, label: "Memory" }],
  },
];

const SETTINGS_NAV_FLAT = SETTINGS_SECTIONS.flatMap((s) => s.items);
const SETTINGS_TAB_MAX = 8;

function clampSettingsTab(i: number): number {
  return Math.min(
    Math.max(0, Math.floor(Number.isFinite(i) ? i : 0)),
    SETTINGS_TAB_MAX,
  );
}

// Supported LLM providers with their display names and required fields
type ProviderConfig = {
  name: string;
  requiresSecretKey: boolean;
  requiresAppId: boolean;
  defaultModel: string;
  placeholder: string;
  docsUrl: string;
};

const PROVIDERS: Record<string, ProviderConfig> = {
  anthropic: {
    name: "Anthropic (Claude)",
    requiresSecretKey: false,
    requiresAppId: false,
    defaultModel: "claude-3-5-sonnet-20241022",
    placeholder: "sk-ant-api03-...",
    docsUrl: "https://console.anthropic.com/settings/keys",
  },
  openai: {
    name: "OpenAI (GPT)",
    requiresSecretKey: false,
    requiresAppId: false,
    defaultModel: "gpt-4o",
    placeholder: "sk-...",
    docsUrl: "https://platform.openai.com/api-keys",
  },
  azure: {
    name: "Azure OpenAI",
    requiresSecretKey: false,
    requiresAppId: false,
    defaultModel: "gpt-4",
    placeholder: "https://{resource}.openai.azure.com/",
    docsUrl: "https://portal.azure.com/",
  },
  google: {
    name: "Google (Gemini)",
    requiresSecretKey: false,
    requiresAppId: false,
    defaultModel: "gemini-1.5-pro",
    placeholder: "AIzaSy...",
    docsUrl: "https://aistudio.google.com/app/apikey",
  },
  minimax: {
    name: "MiniMax",
    requiresSecretKey: false,
    requiresAppId: false,
    defaultModel: "abab6.5-chat",
    placeholder: "Enter MiniMax API Key",
    docsUrl:
      "https://www.minimaxi.com/user-center/basic-information/interface-key",
  },
  alibaba: {
    name: "Alibaba (通义千问)",
    requiresSecretKey: false,
    requiresAppId: false,
    defaultModel: "qwen-max",
    placeholder: "sk-...",
    docsUrl: "https://dashscope.console.aliyun.com/apiKey",
  },
  deepseek: {
    name: "DeepSeek",
    requiresSecretKey: false,
    requiresAppId: false,
    defaultModel: "deepseek-chat",
    placeholder: "sk-...",
    docsUrl: "https://platform.deepseek.com/api_keys",
  },
  zhipu: {
    name: "Zhipu (ChatGLM)",
    requiresSecretKey: false,
    requiresAppId: false,
    defaultModel: "glm-4",
    placeholder: "Enter API Key",
    docsUrl: "https://open.bigmodel.cn/usercenter/apikey",
  },
  moonshot: {
    name: "Moonshot (月之暗面)",
    requiresSecretKey: false,
    requiresAppId: false,
    defaultModel: "moonshot-v1-8k",
    placeholder: "sk-...",
    docsUrl: "https://platform.moonshot.cn/console/api-keys",
  },
  custom: {
    name: "Custom (OpenAI-compatible)",
    requiresSecretKey: false,
    requiresAppId: false,
    defaultModel: "",
    placeholder: "Enter API Key",
    docsUrl: "",
  },
};

interface LlmConfig {
  provider: string;
  apiKey: string;
  secretKey?: string;
  appId?: string;
  model: string;
  baseUrl?: string;
}

export function Settings({ open, onClose, initialTab = 0 }: SettingsProps) {
  const [activeTab, setActiveTab] = useState(() =>
    clampSettingsTab(initialTab),
  );
  const isLlmTab = activeTab === 0 || activeTab === 1;
  const projectPath = useSessionStore(
    (s) => s.currentSession?.projectPath ?? ".",
  );
  const colorMode = useColorModeStore((s) => s.colorMode);
  const setColorMode = useColorModeStore((s) => s.setColorMode);
  const accentPreset = useColorModeStore((s) => s.accentPreset ?? "asana");
  const setAccentPreset = useColorModeStore((s) => s.setAccentPreset);

  // Provider selection
  const [provider, setProvider] = useState("anthropic");

  // API credentials
  const [apiKey, setApiKey] = useState("");
  const [secretKey, setSecretKey] = useState("");
  const [appId, setAppId] = useState("");

  // Advanced settings
  const [model, setModel] = useState("");
  const [baseUrl, setBaseUrl] = useState("");
  const [braveApiKey, setBraveApiKey] = useState("");
  const [showBraveKey, setShowBraveKey] = useState(false);

  // UI state
  const [savedConfig, setSavedConfig] = useState<LlmConfig | null>(null);
  const [isLoading, setIsLoading] = useState(false);
  const [showKey, setShowKey] = useState(false);
  const [showSecret, setShowSecret] = useState(false);
  const [message, setMessage] = useState<{
    type: "success" | "error";
    text: string;
  } | null>(null);
  // Load saved config on mount
  useEffect(() => {
    loadSavedConfig();
  }, []);

  // Clear message when dialog opens; sync tab from parent (e.g. openSettings detail)
  useEffect(() => {
    if (open) {
      setMessage(null);
      loadSavedConfig();
      setActiveTab(clampSettingsTab(initialTab));
    }
  }, [open, initialTab]);

  // Do NOT auto-fill model in a useEffect([provider]) when model is empty.
  // On restart, loadSavedConfig applies localStorage synchronously before the first await,
  // but a [provider] effect would still see the initial render (anthropic + empty model) and
  // overwrite the restored model with Anthropic's default — e.g. claude-3-5-sonnet-20241022
  // after the user had saved DeepSeek + deepseek-chat. Provider changes are handled in
  // handleProviderChange; first-time defaults are set in loadSavedConfig when nothing is stored.

  const loadSavedConfig = async () => {
    try {
      // Try to load from localStorage first for fast response
      const stored = localStorage.getItem("omiga_llm_config");
      if (stored) {
        const config: LlmConfig = JSON.parse(stored);
        setSavedConfig(config);
        setProvider(config.provider);
        setApiKey(config.apiKey);
        setSecretKey(config.secretKey || "");
        setAppId(config.appId || "");
        setModel(
          config.model || PROVIDERS[config.provider]?.defaultModel || "",
        );
        setBaseUrl(config.baseUrl || "");
        try {
          await invoke("set_llm_config", {
            provider: config.provider,
            apiKey: config.apiKey.trim(),
            secretKey: config.secretKey,
            appId: config.appId,
            model: config.model,
            baseUrl: config.baseUrl,
          });
        } catch {
          /* non-fatal: backend may already match from App.tsx bootstrap */
        }
      }

      const braveStored = localStorage.getItem("omiga_brave_search_api_key");
      if (braveStored !== null) {
        setBraveApiKey(braveStored);
      }

      const backendConfig = await invoke<{
        provider?: string;
        model?: string;
      } | null>("get_llm_config_state", {});
      // In-memory backend may match the current session (same tab); no full key here — do not
      // replace a restored localStorage config. If there was no localStorage, merge model label
      // only when backend has something and UI model is still empty.
      if (!stored && backendConfig?.model?.trim()) {
        if (backendConfig.provider) {
          setProvider(backendConfig.provider);
        }
        setModel(backendConfig.model);
      } else if (!stored && !backendConfig?.model) {
        // First launch, no backend session: placeholder for initial provider (anthropic)
        setModel((prev) =>
          prev.trim() ? prev : PROVIDERS.anthropic.defaultModel,
        );
      }
      if (braveStored?.trim()) {
        try {
          await invoke("set_brave_search_api_key", {
            apiKey: braveStored.trim(),
          });
        } catch {
          /* non-fatal */
        }
      }
    } catch (error) {
      console.log("No saved config found");
    }
  };

  const handleProviderChange = (event: SelectChangeEvent) => {
    const newProvider = event.target.value;
    setProvider(newProvider);
    // Reset optional fields when switching providers
    setSecretKey("");
    setAppId("");
    setModel(PROVIDERS[newProvider]?.defaultModel || "");
  };

  const handleSave = async () => {
    const providerConfig = PROVIDERS[provider];

    if (!apiKey.trim()) {
      setMessage({
        type: "error",
        text: `Please enter ${providerConfig.name} API key`,
      });
      return;
    }

    if (providerConfig.requiresSecretKey && !secretKey.trim()) {
      setMessage({
        type: "error",
        text: `Please enter ${providerConfig.name} Secret Key`,
      });
      return;
    }

    if (providerConfig.requiresAppId && !appId.trim()) {
      setMessage({
        type: "error",
        text: `Please enter ${providerConfig.name} App ID`,
      });
      return;
    }

    if (!model.trim()) {
      setMessage({
        type: "error",
        text: "Please enter a model ID",
      });
      return;
    }

    setIsLoading(true);
    setMessage(null);

    const config: LlmConfig = {
      provider,
      apiKey: apiKey.trim(),
      secretKey: secretKey.trim() || undefined,
      appId: appId.trim() || undefined,
      model: model.trim(),
      baseUrl: baseUrl.trim() || undefined,
    };

    try {
      // Save to backend
      await invoke("set_llm_config", {
        provider: config.provider,
        apiKey: config.apiKey,
        secretKey: config.secretKey,
        appId: config.appId,
        model: config.model,
        baseUrl: config.baseUrl,
      });

      // Save to localStorage for persistence
      localStorage.setItem("omiga_llm_config", JSON.stringify(config));
      setSavedConfig(config);

      const braveTrim = braveApiKey.trim();
      await invoke("set_brave_search_api_key", {
        apiKey: braveTrim,
      });
      if (braveTrim) {
        localStorage.setItem("omiga_brave_search_api_key", braveApiKey);
      } else {
        localStorage.removeItem("omiga_brave_search_api_key");
      }

      setMessage({
        type: "success",
        text: `${providerConfig.name} configuration saved successfully`,
      });

      // Close after a short delay
      setTimeout(() => {
        onClose();
      }, 1500);
    } catch (error) {
      console.error("Failed to save config:", error);
      setMessage({ type: "error", text: `Failed to save: ${error}` });
    } finally {
      setIsLoading(false);
    }
  };

  const handleClear = () => {
    setApiKey("");
    setSecretKey("");
    setAppId("");
    setModel("");
    setBaseUrl("");
    setBraveApiKey("");
    localStorage.removeItem("omiga_llm_config");
    localStorage.removeItem("omiga_brave_search_api_key");
    void invoke("set_brave_search_api_key", { apiKey: "" }).catch(() => {});
    setSavedConfig(null);
    setMessage({ type: "success", text: "Configuration cleared" });
  };

  const handleSaveAdvanced = async () => {
    setIsLoading(true);
    setMessage(null);
    try {
      const braveTrim = braveApiKey.trim();
      await invoke("set_brave_search_api_key", {
        apiKey: braveTrim,
      });
      if (braveTrim) {
        localStorage.setItem("omiga_brave_search_api_key", braveApiKey);
      } else {
        localStorage.removeItem("omiga_brave_search_api_key");
      }

      const stored = localStorage.getItem("omiga_llm_config");
      if (stored) {
        const config: LlmConfig = JSON.parse(stored);
        const next: LlmConfig = {
          ...config,
          baseUrl: baseUrl.trim() || undefined,
        };
        await invoke("set_llm_config", {
          provider: next.provider,
          apiKey: next.apiKey,
          secretKey: next.secretKey,
          appId: next.appId,
          model: next.model,
          baseUrl: next.baseUrl,
        });
        localStorage.setItem("omiga_llm_config", JSON.stringify(next));
        setSavedConfig(next);
      }

      setMessage({
        type: "success",
        text: "Advanced settings saved (Brave Search key + base URL)",
      });
    } catch (error) {
      console.error("Failed to save advanced settings:", error);
      setMessage({
        type: "error",
        text: `Failed to save: ${error}`,
      });
    } finally {
      setIsLoading(false);
    }
  };

  const currentProvider = PROVIDERS[provider];

  if (!open) return null;

  return (
    <Box
      role="dialog"
      aria-labelledby="omiga-settings-title"
      sx={{
        height: "100%",
        minHeight: 0,
        display: "flex",
        flexDirection: "column",
        bgcolor: "background.paper",
        color: "text.primary",
      }}
    >
      {/* Top bar — back + title (matches settings hub pattern) */}
      <Box
        sx={{
          flexShrink: 0,
          display: "flex",
          alignItems: "center",
          gap: 1,
          px: 1.5,
          py: 1,
          borderBottom: 1,
          borderColor: "divider",
          bgcolor: "background.default",
        }}
      >
        <IconButton
          size="small"
          onClick={onClose}
          aria-label="Close settings"
          sx={{ color: "text.secondary" }}
        >
          <ArrowBack fontSize="small" />
        </IconButton>
        <Typography id="omiga-settings-title" variant="h6" fontWeight={600}>
          Settings
        </Typography>
      </Box>

      {/* Sidebar + main — Claude-style two-column settings */}
      <Box
        sx={{
          flex: 1,
          minHeight: 0,
          display: "flex",
          flexDirection: "row",
        }}
      >
        <Box
          component="nav"
          aria-label="Settings sections"
          sx={{
            width: 260,
            flexShrink: 0,
            borderRight: 1,
            borderColor: "divider",
            bgcolor: "background.paper",
            overflow: "auto",
            py: 1,
          }}
        >
          <List disablePadding dense>
            {SETTINGS_SECTIONS.map((section) => (
              <Fragment key={section.header}>
                <ListSubheader
                  sx={{
                    typography: "caption",
                    fontWeight: 700,
                    color: "text.secondary",
                    lineHeight: 2,
                    bgcolor: "transparent",
                  }}
                >
                  {section.header}
                </ListSubheader>
                {section.items.map(({ index, label }) => (
                  <ListItemButton
                    key={index}
                    selected={activeTab === index}
                    onClick={() => setActiveTab(index)}
                    sx={{
                      mx: 1,
                      borderRadius: 1,
                      mb: 0.25,
                      "&.Mui-selected": {
                        bgcolor: "action.selected",
                      },
                      "&.Mui-selected:hover": {
                        bgcolor: "action.hover",
                      },
                    }}
                  >
                    <ListItemText
                      primary={label}
                      primaryTypographyProps={{
                        fontSize: "0.875rem",
                        fontWeight: activeTab === index ? 600 : 400,
                      }}
                    />
                  </ListItemButton>
                ))}
              </Fragment>
            ))}
          </List>
        </Box>

        <Box
          sx={{
            flex: 1,
            minWidth: 0,
            display: "flex",
            flexDirection: "column",
            minHeight: 0,
            bgcolor: "background.paper",
          }}
        >
          <Box sx={{ flex: 1, minHeight: 0, overflow: "auto", px: 3, py: 2.5 }}>
            <Typography
              variant="h5"
              fontWeight={600}
              sx={{ mb: 2.5, letterSpacing: "-0.02em", color: "text.primary" }}
            >
              {SETTINGS_NAV_FLAT.find((n) => n.index === activeTab)?.label ??
                "Settings"}
            </Typography>
            {activeTab === 0 && (
              <ProviderManager
                onActiveProviderChange={(provider, model) => {
                  // Update local state for compatibility
                  setProvider(provider);
                  setModel(model);
                }}
              />
            )}

            {activeTab === 1 && (
              <Box>
                <Typography variant="subtitle2" fontWeight={600} sx={{ mb: 2 }}>
                  Advanced Settings
                </Typography>

                {/* Base URL (for custom endpoints) */}
                <TextField
                  fullWidth
                  label="Base URL (optional)"
                  placeholder="https://api.example.com/v1"
                  value={baseUrl}
                  onChange={(e) => setBaseUrl(e.target.value)}
                  disabled={isLoading}
                  helperText="Override the default API endpoint. For Azure or custom OpenAI-compatible services."
                  sx={{ mb: 3 }}
                />

                <TextField
                  fullWidth
                  type={showBraveKey ? "text" : "password"}
                  label="Brave Search API key (optional)"
                  placeholder="BSA..."
                  value={braveApiKey}
                  onChange={(e) => setBraveApiKey(e.target.value)}
                  disabled={isLoading}
                  helperText={`Used by the built-in web_search tool (Brave API). If empty, Omiga tries $OMIGA_BRAVE_API_KEY / $BRAVE_API_KEY, then falls back to DuckDuckGo.`}
                  InputProps={{
                    endAdornment: (
                      <InputAdornment position="end">
                        <IconButton
                          onClick={() => setShowBraveKey(!showBraveKey)}
                          edge="end"
                          size="small"
                        >
                          {showBraveKey ? <VisibilityOff /> : <Visibility />}
                        </IconButton>
                      </InputAdornment>
                    ),
                  }}
                  sx={{ mb: 2 }}
                />
                <Typography
                  variant="caption"
                  color="text.secondary"
                  sx={{ display: "block", mb: 2 }}
                >
                  Get a key from{" "}
                  <Link
                    href="https://brave.com/search/api/"
                    target="_blank"
                    rel="noopener noreferrer"
                    sx={{
                      display: "inline-flex",
                      alignItems: "center",
                      gap: 0.5,
                    }}
                  >
                    Brave Search API
                    <OpenInNew fontSize="inherit" />
                  </Link>
                  . Stored locally.
                </Typography>

                <Button
                  variant="contained"
                  onClick={() => void handleSaveAdvanced()}
                  disabled={isLoading}
                  sx={{ mb: 2 }}
                >
                  Save advanced settings
                </Button>

                <Divider sx={{ my: 2 }} />

                <Typography variant="body2" color="text.secondary">
                  Saving the model on the Model tab also saves the Brave key and
                  base URL from this page. Use the button above if you only
                  changed advanced options.
                </Typography>
              </Box>
            )}

            {activeTab === 2 && (
              <Box>
                <PermissionSettingsTab projectPath={projectPath} />
              </Box>
            )}

            {activeTab === 3 && (
              <ThemeAppearancePanel
                colorMode={colorMode}
                onColorModeChange={setColorMode}
                accentPreset={accentPreset}
                onAccentPresetChange={setAccentPreset}
              />
            )}

            {activeTab === 4 && (
              <Box>
                <Alert severity="info" sx={{ mb: 2, borderRadius: 2 }}>
                  In-app plugin management (install, enable, and configure
                  extensions for Omiga) is planned. Until then, use the host
                  environment and project tooling you already rely on alongside
                  Omiga.
                </Alert>
                <Typography variant="body2" color="text.secondary">
                  Plugin-related options will appear here in a future release.
                </Typography>
              </Box>
            )}

            {activeTab === 5 && (
              <Box>
                <Alert severity="info" sx={{ mb: 2, borderRadius: 2 }}>
                  Omiga 仅从以下位置合并 MCP（同名以后者为准）：应用内置{" "}
                  <Typography component="span" fontWeight={600}>
                    bundled_mcp.json
                  </Typography>
                  {" → "}用户{" "}
                  <Typography component="span" fontWeight={600}>
                    ~/.omiga/mcp.json
                  </Typography>
                  {" → "}当前项目{" "}
                  <Typography component="span" fontWeight={600}>
                    .omiga/mcp.json
                  </Typography>
                  。不再读取 ~/.claude.json、~/.cursor 或项目 .mcp.json。
                </Alert>
                <Typography variant="body2" color="text.secondary">
                  下方可将外部 JSON（如 Claude Code 导出）合并到当前项目的{" "}
                  <Typography component="span" fontWeight={600}>
                    .omiga/mcp.json
                  </Typography>
                  ；保存后新对话即可加载 MCP。
                </Typography>
                <ClaudeCodeImportPanel projectPath={projectPath} mode="mcp" />
                <IntegrationsCatalogPanel
                  projectPath={projectPath}
                  mode="mcp"
                />
              </Box>
            )}

            {activeTab === 6 && (
              <Box>
                <Alert severity="info" sx={{ mb: 2, borderRadius: 2 }}>
                  <Typography variant="body2" color="text.secondary">
                    使用下方「从 Claude
                    默认目录导入」或「从任意文件夹导入」将技能复制到 Omiga
                    目录后，新对话即可使用。
                  </Typography>
                </Alert>

                <ClaudeCodeImportPanel
                  projectPath={projectPath}
                  mode="skills"
                />
                <IntegrationsCatalogPanel
                  projectPath={projectPath}
                  mode="skills"
                />
              </Box>
            )}

            {activeTab === 7 && (
              <Box>
                <NotebookSettingsTab />
              </Box>
            )}

            {activeTab === 8 && (
              <Box>
                <UnifiedMemoryTab projectPath={projectPath} />
              </Box>
            )}

            {/* Status Message */}
            {message && isLlmTab && (
              <Alert severity={message.type} sx={{ mt: 2, borderRadius: 2 }}>
                {message.text}
              </Alert>
            )}
          </Box>

          {isLlmTab && (
            <Box
              sx={{
                flexShrink: 0,
                borderTop: 1,
                borderColor: "divider",
                px: 3,
                py: 2,
                display: "flex",
                flexWrap: "wrap",
                gap: 1,
                alignItems: "center",
                justifyContent: "flex-end",
                bgcolor: "background.paper",
              }}
            >
              {savedConfig && (
                <Button
                  onClick={handleClear}
                  disabled={isLoading}
                  color="error"
                  variant="outlined"
                  sx={{ mr: "auto" }}
                >
                  Clear
                </Button>
              )}
              <Button onClick={onClose} disabled={isLoading}>
                Cancel
              </Button>
              <Button
                onClick={handleSave}
                disabled={isLoading || !apiKey.trim()}
                variant="contained"
                startIcon={isLoading ? <CircularProgress size={16} /> : null}
              >
                {isLoading ? "Saving..." : "Save"}
              </Button>
            </Box>
          )}
        </Box>
      </Box>
    </Box>
  );
}
