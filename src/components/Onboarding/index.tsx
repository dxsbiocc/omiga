import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  Dialog,
  DialogContent,
  Box,
  Typography,
  Button,
  TextField,
  InputAdornment,
  IconButton,
  FormControl,
  InputLabel,
  Select,
  MenuItem,
  Link,
  Alert,
  alpha,
  useTheme,
  CircularProgress,
} from "@mui/material";
import {
  Visibility,
  VisibilityOff,
  OpenInNew,
  CheckCircle,
  AutoAwesome,
  ChatBubbleOutline,
} from "@mui/icons-material";
import { notifyProviderChanged } from "../../utils/providerEvents";

const PROVIDER_OPTIONS: {
  value: string;
  label: string;
  placeholder: string;
  defaultModel: string;
  docsUrl: string;
}[] = [
  {
    value: "anthropic",
    label: "Anthropic (Claude)",
    placeholder: "sk-ant-api03-...",
    defaultModel: "claude-3-5-sonnet-20241022",
    docsUrl: "https://console.anthropic.com/settings/keys",
  },
  {
    value: "deepseek",
    label: "DeepSeek",
    placeholder: "sk-...",
    defaultModel: "deepseek-chat",
    docsUrl: "https://platform.deepseek.com/api_keys",
  },
  {
    value: "openai",
    label: "OpenAI (GPT)",
    placeholder: "sk-...",
    defaultModel: "gpt-4o",
    docsUrl: "https://platform.openai.com/api-keys",
  },
  {
    value: "google",
    label: "Google (Gemini)",
    placeholder: "AIzaSy...",
    defaultModel: "gemini-1.5-pro",
    docsUrl: "https://aistudio.google.com/app/apikey",
  },
  {
    value: "alibaba",
    label: "Alibaba (通义千问)",
    placeholder: "sk-...",
    defaultModel: "qwen-max",
    docsUrl: "https://dashscope.console.aliyun.com/apiKey",
  },
  {
    value: "moonshot",
    label: "Moonshot (Kimi)",
    placeholder: "sk-...",
    defaultModel: "kimi-k2-0905-preview",
    docsUrl: "https://platform.moonshot.ai/docs/overview",
  },
  {
    value: "custom",
    label: "Custom (OpenAI-compatible)",
    placeholder: "Enter API Key",
    defaultModel: "",
    docsUrl: "",
  },
];

type Step = "welcome" | "model" | "done";

interface OnboardingWizardProps {
  onComplete: () => void;
}

export function OnboardingWizard({ onComplete }: OnboardingWizardProps) {
  const theme = useTheme();
  const [step, setStep] = useState<Step>("welcome");

  // Model config state
  const [provider, setProvider] = useState("anthropic");
  const [apiKey, setApiKey] = useState("");
  const [model, setModel] = useState("claude-3-5-sonnet-20241022");
  const [baseUrl, setBaseUrl] = useState("");
  const [showApiKey, setShowApiKey] = useState(false);
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const currentProvider = PROVIDER_OPTIONS.find((p) => p.value === provider);

  const handleProviderChange = (v: string) => {
    setProvider(v);
    const info = PROVIDER_OPTIONS.find((p) => p.value === v);
    if (info?.defaultModel) setModel(info.defaultModel);
    setSaved(false);
    setError(null);
  };

  const handleSaveModel = async () => {
    if (!apiKey.trim()) { setError("请输入 API Key"); return; }
    setSaving(true);
    setError(null);
    try {
      await invoke("save_provider_config", {
        name: currentProvider?.label ?? provider,
        providerType: provider,
        apiKey: apiKey.trim(),
        model: model.trim() || currentProvider?.defaultModel || "",
        baseUrl: baseUrl.trim() || null,
        secretKey: null,
        appId: null,
        thinking: null,
        setAsDefault: true,
      });
      await invoke("switch_provider", {
        name: currentProvider?.label ?? provider,
        sessionId: null,
      }).catch(() => {});
      notifyProviderChanged();
      setSaved(true);
    } catch (err) {
      setError(String(err));
    } finally {
      setSaving(false);
    }
  };

  /** 模型保存后：写 BOOTSTRAP.md + 三个模板文件，进入完成页 */
  const handleContinue = async () => {
    setSaving(true);
    try {
      await invoke("init_user_context_files");
    } catch {
      /* non-fatal: files are optional */
    } finally {
      setSaving(false);
    }
    setStep("done");
  };

  // ── Welcome ──────────────────────────────────────────────────────────────
  if (step === "welcome") {
    return (
      <Dialog open maxWidth="sm" fullWidth disableEscapeKeyDown
        PaperProps={{ sx: { borderRadius: 3 } }}>
        <DialogContent sx={{ p: 5, textAlign: "center" }}>
          <Box
            sx={{
              width: 72, height: 72, borderRadius: "50%",
              bgcolor: alpha(theme.palette.primary.main, 0.12),
              display: "flex", alignItems: "center", justifyContent: "center",
              mx: "auto", mb: 3,
            }}
          >
            <AutoAwesome sx={{ fontSize: 36, color: "primary.main" }} />
          </Box>
          <Typography variant="h4" fontWeight={700} gutterBottom>
            欢迎使用 Omiga
          </Typography>
          <Typography variant="body1" color="text.secondary"
            sx={{ maxWidth: 400, mx: "auto", lineHeight: 1.9, mb: 4 }}>
            先配置 LLM 模型，然后和 Agent 直接聊——
            <br />
            它会在第一次对话中引导你完成个性化设置。
          </Typography>
          <Button variant="contained" size="large"
            onClick={() => setStep("model")}
            sx={{ px: 5, borderRadius: 2, fontWeight: 600 }}>
            配置模型
          </Button>
          <Box sx={{ mt: 1.5 }}>
            <Button size="small" sx={{ color: "text.disabled", fontSize: "0.75rem" }}
              onClick={onComplete}>
              跳过，稍后配置
            </Button>
          </Box>
        </DialogContent>
      </Dialog>
    );
  }

  // ── Model config ──────────────────────────────────────────────────────────
  if (step === "model") {
    return (
      <Dialog open maxWidth="sm" fullWidth disableEscapeKeyDown
        PaperProps={{ sx: { borderRadius: 3 } }}>
        <DialogContent sx={{ p: 4 }}>
          <Typography variant="h6" fontWeight={600} gutterBottom>
            配置 LLM 模型
          </Typography>
          <Typography variant="body2" color="text.secondary" sx={{ mb: 3 }}>
            选择提供商，输入 API Key。保存成功后即可开始对话。
          </Typography>

          <FormControl fullWidth sx={{ mb: 2 }}>
            <InputLabel>提供商</InputLabel>
            <Select value={provider} label="提供商"
              onChange={(e) => handleProviderChange(e.target.value)}>
              {PROVIDER_OPTIONS.map((p) => (
                <MenuItem key={p.value} value={p.value}>{p.label}</MenuItem>
              ))}
            </Select>
          </FormControl>

          {provider === "custom" && (
            <TextField fullWidth label="API Base URL"
              placeholder="https://your-endpoint/v1"
              value={baseUrl}
              onChange={(e) => { setBaseUrl(e.target.value); setSaved(false); }}
              sx={{ mb: 2 }} />
          )}

          <TextField fullWidth label="API Key"
            type={showApiKey ? "text" : "password"}
            placeholder={currentProvider?.placeholder ?? "Enter API Key"}
            value={apiKey}
            onChange={(e) => { setApiKey(e.target.value); setSaved(false); }}
            InputProps={{
              endAdornment: (
                <InputAdornment position="end">
                  <IconButton size="small" onClick={() => setShowApiKey(!showApiKey)}>
                    {showApiKey ? <VisibilityOff /> : <Visibility />}
                  </IconButton>
                </InputAdornment>
              ),
            }}
            sx={{ mb: 2 }} />

          <TextField fullWidth label="模型名称"
            placeholder={currentProvider?.defaultModel ?? ""}
            value={model}
            onChange={(e) => { setModel(e.target.value); setSaved(false); }}
            sx={{ mb: 1 }} />

          {currentProvider?.docsUrl && (
            <Typography variant="caption" color="text.secondary"
              sx={{ display: "block", mb: 2 }}>
              获取 API Key：
              <Link href={currentProvider.docsUrl} target="_blank" rel="noopener noreferrer"
                sx={{ ml: 0.5, display: "inline-flex", alignItems: "center", gap: 0.25 }}>
                {currentProvider.label}
                <OpenInNew fontSize="inherit" />
              </Link>
            </Typography>
          )}

          {error && <Alert severity="error" sx={{ mb: 2 }}>{error}</Alert>}
          {saved && (
            <Alert severity="success" icon={<CheckCircle fontSize="small" />} sx={{ mb: 2 }}>
              模型配置已保存！
            </Alert>
          )}

          <Box sx={{ display: "flex", gap: 1, mt: 1 }}>
            <Button onClick={() => setStep("welcome")} color="inherit" disabled={saving}>
              返回
            </Button>
            <Box sx={{ flex: 1 }} />
            {!saved ? (
              <Button variant="outlined"
                onClick={() => void handleSaveModel()}
                disabled={saving || !apiKey.trim()}
                startIcon={saving ? <CircularProgress size={16} color="inherit" /> : undefined}>
                保存配置
              </Button>
            ) : (
              <Button variant="contained"
                onClick={() => void handleContinue()}
                disabled={saving}
                startIcon={saving ? <CircularProgress size={16} color="inherit" /> : undefined}
                sx={{ fontWeight: 600 }}>
                开始使用
              </Button>
            )}
          </Box>

          <Box sx={{ textAlign: "center", mt: 1.5 }}>
            <Button size="small" sx={{ color: "text.disabled", fontSize: "0.75rem" }}
              onClick={onComplete}>
              跳过，稍后在设置中配置
            </Button>
          </Box>
        </DialogContent>
      </Dialog>
    );
  }

  // ── Done ──────────────────────────────────────────────────────────────────
  return (
    <Dialog open maxWidth="sm" fullWidth disableEscapeKeyDown
      PaperProps={{ sx: { borderRadius: 3 } }}>
      <DialogContent sx={{ p: 5, textAlign: "center" }}>
        <Box
          sx={{
            width: 72, height: 72, borderRadius: "50%",
            bgcolor: alpha(theme.palette.success.main, 0.12),
            display: "flex", alignItems: "center", justifyContent: "center",
            mx: "auto", mb: 3,
          }}
        >
          <ChatBubbleOutline sx={{ fontSize: 36, color: "success.main" }} />
        </Box>
        <Typography variant="h5" fontWeight={700} gutterBottom>
          配置完成！
        </Typography>
        <Typography variant="body2" color="text.secondary"
          sx={{ maxWidth: 380, mx: "auto", lineHeight: 1.9, mb: 1 }}>
          发送第一条消息，Agent 会自然地引导你完成个性化设置——
          给它起个名字、告诉它你是谁、确认沟通风格。
        </Typography>
        <Typography variant="caption" color="text.disabled"
          sx={{ display: "block", mb: 4 }}>
          配置保存在 <code>~/.omiga/</code>，随时可在 Settings → Memory 编辑。
        </Typography>
        <Button variant="contained" size="large"
          onClick={onComplete}
          sx={{ px: 5, borderRadius: 2, fontWeight: 600 }}>
          开始对话
        </Button>
      </DialogContent>
    </Dialog>
  );
}
