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
  Divider,
  Link,
  List,
  ListItemButton,
  ListItemText,
  ListSubheader,
  FormControlLabel,
  Switch,
  alpha,
} from "@mui/material";
import {
  Visibility,
  VisibilityOff,
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
import { ExecutionEnvsSettingsTab } from "./ExecutionEnvsSettingsTab";
import { RuntimeConstraintsPanel } from "./RuntimeConstraintsPanel";
import { AgentScheduleLauncher } from "../AgentSchedule/AgentScheduleLauncher";
import { MockScenarioLauncher } from "../AgentSchedule/MockScenarioLauncher";
import { AgentRolesPanel } from "../AgentRoles/AgentRolesPanel";

interface SettingsProps {
  open: boolean;
  onClose: () => void;
  /** See `openSettingsTabMap.ts`: 0–10 */
  initialTab?: number;
  /** When `initialTab` is Execution (9): inner tab 0 Modal / 1 Daytona / 2 SSH */
  initialExecutionSubTab?: number;
}

/** Persisted JSON for built-in `web_search` provider keys (Settings → Advanced). */
const WEB_SEARCH_KEYS_STORAGE = "omiga_web_search_api_keys";

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
      { index: 10, label: "Harness" },
      { index: 7, label: "Notebook" },
    ],
  },
  {
    header: "Integrations",
    items: [
      { index: 4, label: "Plugins" },
      { index: 5, label: "MCP" },
      { index: 6, label: "Skills" },
      { index: 9, label: "Execution" },
    ],
  },
  {
    header: "Knowledge",
    items: [{ index: 8, label: "Memory" }],
  },
  {
    header: "Agents",
    items: [{ index: 11, label: "Orchestration" }],
  },
];

const SETTINGS_NAV_FLAT = SETTINGS_SECTIONS.flatMap((s) => s.items);
const SETTINGS_TAB_MAX = 11;

function clampSettingsTab(i: number): number {
  return Math.min(
    Math.max(0, Math.floor(Number.isFinite(i) ? i : 0)),
    SETTINGS_TAB_MAX,
  );
}

function parseSettingBool(raw: unknown, defaultVal: boolean): boolean {
  if (raw == null || raw === "") return defaultVal;
  const t = String(raw).trim().toLowerCase();
  if (t === "false" || t === "0" || t === "no" || t === "off") return false;
  if (t === "true" || t === "1" || t === "yes" || t === "on") return true;
  return defaultVal;
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
    defaultModel: "kimi-k2-0905-preview",
    placeholder: "sk-...",
    docsUrl: "https://platform.moonshot.ai/docs/overview",
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

export function Settings({
  open,
  onClose,
  initialTab = 0,
  initialExecutionSubTab = 0,
}: SettingsProps) {
  const [activeTab, setActiveTab] = useState(() =>
    clampSettingsTab(initialTab),
  );
  const isLlmTab = activeTab === 0 || activeTab === 1;
  const currentSessionId = useSessionStore((s) => s.currentSession?.id ?? null);

  const projectPath = useSessionStore(
    (s) => s.currentSession?.projectPath ?? ".",
  );
  const colorMode = useColorModeStore((s) => s.colorMode);
  const setColorMode = useColorModeStore((s) => s.setColorMode);
  const accentPreset = useColorModeStore((s) => s.accentPreset ?? "asana");
  const setAccentPreset = useColorModeStore((s) => s.setAccentPreset);

  // Provider selection
  const [_provider, setProvider] = useState("anthropic");

  // API credentials
  const [_apiKey, setApiKey] = useState("");
  const [_secretKey, setSecretKey] = useState("");
  const [_appId, setAppId] = useState("");
  const [_model, setModel] = useState("");

  // API credentials
  const [tavilyApiKey, setTavilyApiKey] = useState("");
  const [showTavilyKey, setShowTavilyKey] = useState(false);
  const [exaApiKey, setExaApiKey] = useState("");
  const [showExaKey, setShowExaKey] = useState(false);
  const [parallelApiKey, setParallelApiKey] = useState("");
  const [showParallelKey, setShowParallelKey] = useState(false);
  const [firecrawlApiKey, setFirecrawlApiKey] = useState("");
  const [showFirecrawlKey, setShowFirecrawlKey] = useState(false);
  const [firecrawlUrl, setFirecrawlUrl] = useState("");
  /** 回合结束后第二次模型调用：要点摘要 */
  const [postTurnSummaryEnabled, setPostTurnSummaryEnabled] = useState(true);
  /** 回合结束后第二次模型调用：输入框上方「下一步」建议 */
  const [followUpSuggestionsEnabled, setFollowUpSuggestionsEnabled] =
    useState(true);
  /** LLM 请求超时（秒）——长对话 / 复杂任务需要更大值 */
  const [requestTimeoutSecs, setRequestTimeoutSecs] = useState(600);
  /** 网页访问是否使用系统/环境代理；默认关闭 */
  const [webUseProxy, setWebUseProxy] = useState(false);

  // UI state
  const [isLoading, setIsLoading] = useState(false);
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
  // after the user had saved DeepSeek + deepseek-chat. First-time defaults are set in
  // loadSavedConfig when nothing is stored.

  const loadSavedConfig = async () => {
    try {
      // Try to load from localStorage first for fast response
      const stored = localStorage.getItem("omiga_llm_config");
      if (stored) {
        const config: LlmConfig = JSON.parse(stored);
        setProvider(config.provider);
        setApiKey(config.apiKey);
        setSecretKey(config.secretKey || "");
        setAppId(config.appId || "");
        setModel(
          config.model || PROVIDERS[config.provider]?.defaultModel || "",
        );
      }

      const rawWebKeys = localStorage.getItem(WEB_SEARCH_KEYS_STORAGE);
      if (rawWebKeys) {
        try {
          const j = JSON.parse(rawWebKeys) as Record<string, string>;
          setTavilyApiKey(j.tavily ?? "");
          setExaApiKey(j.exa ?? "");
          setParallelApiKey(j.parallel ?? "");
          setFirecrawlApiKey(j.firecrawl ?? "");
          setFirecrawlUrl(j.firecrawlUrl ?? "");
        } catch {
          /* ignore */
        }
      } else {
        const tavilyStored =
          localStorage.getItem("omiga_tavily_search_api_key") ??
          localStorage.getItem("omiga_brave_search_api_key");
        if (tavilyStored !== null) {
          setTavilyApiKey(tavilyStored);
        }
      }

      const backendConfig = await invoke<{
        provider?: string;
        model?: string;
      } | null>("get_llm_config_state", {});
      // Migrate localStorage into backend only when Rust has no config yet (e.g. first run).
      if (stored && !backendConfig?.provider?.trim()) {
        try {
          const config: LlmConfig = JSON.parse(stored);
          await invoke("set_llm_config", {
            provider: config.provider,
            apiKey: config.apiKey.trim(),
            secretKey: config.secretKey,
            appId: config.appId,
            model: config.model,
            baseUrl: config.baseUrl,
          });
        } catch {
          /* non-fatal */
        }
      }
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
      let wsPayload: {
        tavily: string;
        exa: string;
        parallel: string;
        firecrawl: string;
        firecrawlUrl: string;
      };
      if (rawWebKeys) {
        try {
          const j = JSON.parse(rawWebKeys) as Record<string, string>;
          wsPayload = {
            tavily: (j.tavily ?? "").trim(),
            exa: (j.exa ?? "").trim(),
            parallel: (j.parallel ?? "").trim(),
            firecrawl: (j.firecrawl ?? "").trim(),
            firecrawlUrl: (j.firecrawlUrl ?? "").trim(),
          };
        } catch {
          wsPayload = {
            tavily: "",
            exa: "",
            parallel: "",
            firecrawl: "",
            firecrawlUrl: "",
          };
        }
      } else {
        const legacy = (
          localStorage.getItem("omiga_tavily_search_api_key") ??
          localStorage.getItem("omiga_brave_search_api_key") ??
          ""
        ).trim();
        wsPayload = {
          tavily: legacy,
          exa: "",
          parallel: "",
          firecrawl: "",
          firecrawlUrl: "",
        };
      }
      if (
        wsPayload.tavily ||
        wsPayload.exa ||
        wsPayload.parallel ||
        wsPayload.firecrawl ||
        wsPayload.firecrawlUrl
      ) {
        try {
          await invoke("set_web_search_api_keys", wsPayload);
        } catch {
          /* non-fatal */
        }
      }

      try {
        const sSum = await invoke<string | null>("get_setting", {
          key: "omiga.post_turn_summary_enabled",
        });
        const sFol = await invoke<string | null>("get_setting", {
          key: "omiga.follow_up_suggestions_enabled",
        });
        setPostTurnSummaryEnabled(parseSettingBool(sSum, true));
        setFollowUpSuggestionsEnabled(parseSettingBool(sFol, true));
      } catch {
        /* ignore */
      }

      try {
        const gs = await invoke<{
          timeout?: number;
          webUseProxy?: boolean;
        }>("get_global_settings", {});
        if (gs.timeout != null && gs.timeout > 0) {
          setRequestTimeoutSecs(gs.timeout);
        }
        setWebUseProxy(gs.webUseProxy === true);
      } catch {
        /* ignore */
      }
    } catch (error) {
      console.log("No saved config found");
    }
  };

  const handleSaveAdvanced = async () => {
    setIsLoading(true);
    setMessage(null);
    try {
      await invoke("set_setting", {
        key: "omiga.post_turn_summary_enabled",
        value: postTurnSummaryEnabled ? "true" : "false",
      });
      await invoke("set_setting", {
        key: "omiga.follow_up_suggestions_enabled",
        value: followUpSuggestionsEnabled ? "true" : "false",
      });

      const ws = {
        tavily: tavilyApiKey.trim(),
        exa: exaApiKey.trim(),
        parallel: parallelApiKey.trim(),
        firecrawl: firecrawlApiKey.trim(),
        firecrawlUrl: firecrawlUrl.trim(),
      };
      await invoke("set_web_search_api_keys", {
        tavily: ws.tavily,
        exa: ws.exa,
        parallel: ws.parallel,
        firecrawl: ws.firecrawl,
        firecrawlUrl: ws.firecrawlUrl,
      });
      await invoke("save_global_settings_to_config", {
        timeout: Math.max(30, requestTimeoutSecs),
        webUseProxy,
      });
      localStorage.setItem(WEB_SEARCH_KEYS_STORAGE, JSON.stringify(ws));
      localStorage.removeItem("omiga_tavily_search_api_key");
      localStorage.removeItem("omiga_brave_search_api_key");

      setMessage({
        type: "success",
        text: "Advanced settings saved (request timeout, web search keys, web proxy, post-turn summary & follow-up suggestions)",
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
                        bgcolor: (t) =>
                          alpha(
                            t.palette.primary.main,
                            t.palette.mode === "dark" ? 0.3 : 0.4,
                          ),
                      },
                      "&.Mui-selected:hover": {
                        bgcolor: (t) =>
                          alpha(
                            t.palette.primary.main,
                            t.palette.mode === "dark" ? 0.5 : 0.4,
                          ),
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
                sessionId={currentSessionId}
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

                <Typography
                  variant="caption"
                  color="text.secondary"
                  sx={{ display: "block", mb: 1, fontWeight: 600 }}
                >
                  回合结束后的二次模型调用
                </Typography>
                <Typography
                  variant="caption"
                  color="text.disabled"
                  sx={{ display: "block", mb: 1.5, lineHeight: 1.5 }}
                >
                  与主回复无关的独立请求，用于「本轮要点」摘要与输入框下方的「下一步建议」按钮。关闭可减少额外调用与延迟。环境变量{" "}
                  <Typography
                    component="span"
                    fontFamily="monospace"
                    fontSize="0.7rem"
                  >
                    OMIGA_DISABLE_POST_TURN_SUMMARY
                  </Typography>{" "}
                  /{" "}
                  <Typography
                    component="span"
                    fontFamily="monospace"
                    fontSize="0.7rem"
                  >
                    OMIGA_DISABLE_FOLLOW_UP_SUGGESTIONS
                  </Typography>{" "}
                  设为 1 时仍会强制关闭。
                </Typography>
                <FormControlLabel
                  control={
                    <Switch
                      checked={postTurnSummaryEnabled}
                      onChange={(_, v) => setPostTurnSummaryEnabled(v)}
                      disabled={isLoading}
                      color="primary"
                    />
                  }
                  label={
                    <Box>
                      <Typography variant="body2" fontWeight={600}>
                        回合要点摘要
                      </Typography>
                      <Typography variant="caption" color="text.secondary">
                        仅在模型判断需要时生成简短回顾（计划/大段代码等会自动跳过）
                      </Typography>
                    </Box>
                  }
                  sx={{
                    alignItems: "flex-start",
                    mb: 1.5,
                    ml: 0,
                    "& .MuiFormControlLabel-label": { mt: 0.25 },
                  }}
                />
                <FormControlLabel
                  control={
                    <Switch
                      checked={followUpSuggestionsEnabled}
                      onChange={(_, v) => setFollowUpSuggestionsEnabled(v)}
                      disabled={isLoading}
                      color="primary"
                    />
                  }
                  label={
                    <Box>
                      <Typography variant="body2" fontWeight={600}>
                        下一步建议（快捷按钮）
                      </Typography>
                      <Typography variant="caption" color="text.secondary">
                        关闭后仅使用本地启发式建议（若存在）
                      </Typography>
                    </Box>
                  }
                  sx={{
                    alignItems: "flex-start",
                    mb: 3,
                    ml: 0,
                    "& .MuiFormControlLabel-label": { mt: 0.25 },
                  }}
                />

                <Typography
                  variant="caption"
                  color="text.secondary"
                  sx={{ display: "block", mb: 1, fontWeight: 600 }}
                >
                  长对话 / 复杂任务
                </Typography>
                <TextField
                  fullWidth
                  type="number"
                  label="请求超时（秒）"
                  value={requestTimeoutSecs}
                  onChange={(e) => {
                    const v = parseInt(e.target.value, 10);
                    setRequestTimeoutSecs(Number.isFinite(v) ? Math.max(30, v) : 600);
                  }}
                  disabled={isLoading}
                  helperText="流式响应的总超时。长对话、代码生成、测序/数据分析等复杂任务建议设为 1800–3600 秒。"
                  inputProps={{ min: 30, step: 60 }}
                  sx={{ mb: 3 }}
                />

                <FormControlLabel
                  control={
                    <Switch
                      checked={webUseProxy}
                      onChange={(_, v) => setWebUseProxy(v)}
                      disabled={isLoading}
                      color="primary"
                    />
                  }
                  label={
                    <Box>
                      <Typography variant="body2" fontWeight={600}>
                        网页访问使用代理
                      </Typography>
                      <Typography variant="caption" color="text.secondary">
                        仅影响内置 web_search / web_fetch。关闭时强制直连；开启后才会读取系统或环境代理设置。
                      </Typography>
                    </Box>
                  }
                  sx={{
                    alignItems: "flex-start",
                    mb: 3,
                    ml: 0,
                    "& .MuiFormControlLabel-label": { mt: 0.25 },
                  }}
                />

                <Divider sx={{ mb: 3 }} />

                <TextField
                  fullWidth
                  type={showTavilyKey ? "text" : "password"}
                  label="Tavily API key (optional)"
                  placeholder="tvly-..."
                  value={tavilyApiKey}
                  onChange={(e) => setTavilyApiKey(e.target.value)}
                  disabled={isLoading}
                  helperText="Used by built-in web_search (provider order: Tavily → Exa → Firecrawl → Parallel → DuckDuckGo). Overrides OMIGA_TAVILY_API_KEY / TAVILY_API_KEY when set."
                  InputProps={{
                    endAdornment: (
                      <InputAdornment position="end">
                        <IconButton
                          onClick={() => setShowTavilyKey(!showTavilyKey)}
                          edge="end"
                          size="small"
                        >
                          {showTavilyKey ? <VisibilityOff /> : <Visibility />}
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
                    href="https://tavily.com/"
                    target="_blank"
                    rel="noopener noreferrer"
                    sx={{
                      display: "inline-flex",
                      alignItems: "center",
                      gap: 0.5,
                    }}
                  >
                    Tavily
                    <OpenInNew fontSize="inherit" />
                  </Link>
                  . Stored locally.
                </Typography>

                <TextField
                  fullWidth
                  type={showExaKey ? "text" : "password"}
                  label="Exa API key (optional)"
                  placeholder="exa-..."
                  value={exaApiKey}
                  onChange={(e) => setExaApiKey(e.target.value)}
                  disabled={isLoading}
                  helperText="Overrides OMIGA_EXA_API_KEY / EXA_API_KEY."
                  InputProps={{
                    endAdornment: (
                      <InputAdornment position="end">
                        <IconButton
                          onClick={() => setShowExaKey(!showExaKey)}
                          edge="end"
                          size="small"
                        >
                          {showExaKey ? <VisibilityOff /> : <Visibility />}
                        </IconButton>
                      </InputAdornment>
                    ),
                  }}
                  sx={{ mb: 2 }}
                />
                <Typography variant="caption" color="text.secondary" sx={{ display: "block", mb: 2 }}>
                  <Link href="https://exa.ai/" target="_blank" rel="noopener noreferrer" sx={{ display: "inline-flex", alignItems: "center", gap: 0.5 }}>
                    Exa
                    <OpenInNew fontSize="inherit" />
                  </Link>
                </Typography>

                <TextField
                  fullWidth
                  type={showParallelKey ? "text" : "password"}
                  label="Parallel API key (optional)"
                  value={parallelApiKey}
                  onChange={(e) => setParallelApiKey(e.target.value)}
                  disabled={isLoading}
                  helperText="Overrides OMIGA_PARALLEL_API_KEY / PARALLEL_API_KEY."
                  InputProps={{
                    endAdornment: (
                      <InputAdornment position="end">
                        <IconButton
                          onClick={() => setShowParallelKey(!showParallelKey)}
                          edge="end"
                          size="small"
                        >
                          {showParallelKey ? <VisibilityOff /> : <Visibility />}
                        </IconButton>
                      </InputAdornment>
                    ),
                  }}
                  sx={{ mb: 2 }}
                />
                <Typography variant="caption" color="text.secondary" sx={{ display: "block", mb: 2 }}>
                  <Link href="https://parallel.ai/" target="_blank" rel="noopener noreferrer" sx={{ display: "inline-flex", alignItems: "center", gap: 0.5 }}>
                    Parallel
                    <OpenInNew fontSize="inherit" />
                  </Link>
                </Typography>

                <TextField
                  fullWidth
                  type={showFirecrawlKey ? "text" : "password"}
                  label="Firecrawl API key (optional)"
                  value={firecrawlApiKey}
                  onChange={(e) => setFirecrawlApiKey(e.target.value)}
                  disabled={isLoading}
                  helperText="Overrides OMIGA_FIRECRAWL_API_KEY / FIRECRAWL_API_KEY."
                  InputProps={{
                    endAdornment: (
                      <InputAdornment position="end">
                        <IconButton
                          onClick={() => setShowFirecrawlKey(!showFirecrawlKey)}
                          edge="end"
                          size="small"
                        >
                          {showFirecrawlKey ? <VisibilityOff /> : <Visibility />}
                        </IconButton>
                      </InputAdornment>
                    ),
                  }}
                  sx={{ mb: 2 }}
                />
                <TextField
                  fullWidth
                  label="Firecrawl API base URL (optional)"
                  placeholder="https://api.firecrawl.dev"
                  value={firecrawlUrl}
                  onChange={(e) => setFirecrawlUrl(e.target.value)}
                  disabled={isLoading}
                  helperText="Self-hosted or alternate endpoint. Overrides OMIGA_FIRECRAWL_API_URL. Default: https://api.firecrawl.dev"
                  sx={{ mb: 2 }}
                />
                <Typography variant="caption" color="text.secondary" sx={{ display: "block", mb: 2 }}>
                  <Link href="https://firecrawl.dev/" target="_blank" rel="noopener noreferrer" sx={{ display: "inline-flex", alignItems: "center", gap: 0.5 }}>
                    Firecrawl
                    <OpenInNew fontSize="inherit" />
                  </Link>
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
                  Saving the model on the Model tab also persists settings from this
                  page. Use the button above if you only changed advanced options.
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

            {activeTab === 10 && (
              <RuntimeConstraintsPanel
                projectPath={projectPath}
                sessionId={currentSessionId}
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

            {activeTab === 9 && (
              <Box>
                <ExecutionEnvsSettingsTab
                  initialSubTab={Math.max(
                    0,
                    Math.min(2, Math.floor(Number(initialExecutionSubTab) || 0)),
                  )}
                />
              </Box>
            )}

            {activeTab === 11 && (
              <Box>
                <Typography variant="subtitle2" fontWeight={600} sx={{ mb: 2 }}>
                  Agent 编排
                </Typography>
                <Typography variant="body2" color="text.secondary" sx={{ mb: 2 }}>
                  启动多 Agent 协作任务，由调度器自动分配子任务并并行执行。
                </Typography>
                {currentSessionId && projectPath ? (
                  <Box display="flex" flexDirection="column" gap={2}>
                    <AgentScheduleLauncher
                      sessionId={currentSessionId}
                      projectRoot={projectPath}
                    />
                    <MockScenarioLauncher
                      sessionId={currentSessionId}
                      projectRoot={projectPath}
                    />
                    <AgentRolesPanel projectRoot={projectPath} />
                  </Box>
                ) : (
                  <Alert severity="info" sx={{ borderRadius: 2 }}>
                    请先打开一个会话
                  </Alert>
                )}
              </Box>
            )}

            {/* Status Message */}
            {message && isLlmTab && (
              <Alert severity={message.type} sx={{ mt: 2, borderRadius: 2 }}>
                {message.text}
              </Alert>
            )}
          </Box>
        </Box>
      </Box>
    </Box>
  );
}
