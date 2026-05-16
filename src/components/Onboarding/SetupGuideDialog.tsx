import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  Dialog,
  DialogTitle,
  DialogContent,
  DialogActions,
  Button,
  Typography,
  Box,
  Alert,
  Chip,
  Stack,
  Divider,
  Tooltip,
} from "@mui/material";
import {
  SettingsOutlined,
  ContentCopyOutlined,
  CheckCircleOutlined,
  WarningAmberOutlined,
} from "@mui/icons-material";

interface SetupStatus {
  configFileFound: boolean;
  configFilePath: string | null;
  hasEnabledProvider: boolean;
  setupHint: string;
}

const EXAMPLE_CONFIG = `version: "1.0"
default: "deepseek"

providers:
  deepseek:
    type: deepseek
    api_key: \${DEEPSEEK_API_KEY}
    model: deepseek-chat
    enabled: true

  openai:
    type: openai
    api_key: \${OPENAI_API_KEY}
    model: gpt-4o
    enabled: false

  gemini:
    type: google
    api_key: \${GOOGLE_API_KEY}
    model: gemini-2.0-flash
    enabled: false

settings:
  max_tokens: 4096
  temperature: 0.7
  timeout: 600
  enable_tools: true`;

const ENV_VARS = [
  { label: "DeepSeek", cmd: 'export DEEPSEEK_API_KEY="sk-..."' },
  { label: "OpenAI", cmd: 'export OPENAI_API_KEY="sk-..."' },
  { label: "Google Gemini", cmd: 'export GOOGLE_API_KEY="AIza..."' },
];

export function SetupGuideDialog() {
  const [open, setOpen] = useState(false);
  const [status, setStatus] = useState<SetupStatus | null>(null);
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    invoke<SetupStatus>("get_setup_status")
      .then((s) => {
        if (!s.hasEnabledProvider) {
          setStatus(s);
          setOpen(true);
        }
      })
      .catch(() => {
        // Tauri not available (web dev mode) — skip silently
      });
  }, []);

  const handleCopyConfig = async () => {
    try {
      await navigator.clipboard.writeText(EXAMPLE_CONFIG);
      setCopied(true);
      setTimeout(() => setCopied(false), 2500);
    } catch {
      // Clipboard unavailable
    }
  };

  if (!status || !open) return null;

  const severity = status.configFileFound ? "warning" : "error";

  return (
    <Dialog open={open} maxWidth="sm" fullWidth>
      <DialogTitle sx={{ display: "flex", alignItems: "center", gap: 1, pb: 1 }}>
        <WarningAmberOutlined color={severity} />
        <Typography variant="h6" component="span">
          欢迎使用 Omiga
        </Typography>
      </DialogTitle>

      <DialogContent sx={{ pt: 0 }}>
        <Stack spacing={2}>
          {/* Status chips */}
          <Stack direction="row" spacing={1} flexWrap="wrap" useFlexGap>
            <Chip
              size="small"
              icon={status.configFileFound ? <CheckCircleOutlined /> : <WarningAmberOutlined />}
              label={status.configFileFound ? "配置文件已找到" : "配置文件缺失"}
              color={status.configFileFound ? "success" : "error"}
              variant="outlined"
            />
            <Chip
              size="small"
              icon={status.hasEnabledProvider ? <CheckCircleOutlined /> : <WarningAmberOutlined />}
              label={status.hasEnabledProvider ? "Provider 已配置" : "未检测到有效 API Key"}
              color={status.hasEnabledProvider ? "success" : "warning"}
              variant="outlined"
            />
          </Stack>

          <Alert severity={severity} sx={{ fontSize: 13, whiteSpace: "pre-line" }}>
            {status.setupHint ||
              "请配置至少一个 LLM Provider 才能开始使用 Omiga。"}
          </Alert>

          <Divider />

          {/* Quick start steps */}
          <Box>
            <Typography variant="subtitle2" gutterBottom fontWeight={600}>
              快速配置
            </Typography>

            <Typography variant="body2" color="text.secondary" gutterBottom>
              1. 在项目根目录创建{" "}
              <Box component="code" sx={{ bgcolor: "action.hover", px: 0.5, borderRadius: 0.5 }}>
                omiga.yaml
              </Box>
              （点击右下角按钮复制示例）
            </Typography>

            <Typography variant="body2" color="text.secondary" sx={{ mb: 1 }}>
              2. 设置 API Key 环境变量后重启应用：
            </Typography>

            <Stack spacing={0.5} sx={{ pl: 1 }}>
              {ENV_VARS.map(({ label, cmd }) => (
                <Box key={label} sx={{ display: "flex", alignItems: "center", gap: 1.5 }}>
                  <Typography
                    variant="caption"
                    color="text.secondary"
                    sx={{ width: 88, flexShrink: 0 }}
                  >
                    {label}
                  </Typography>
                  <Box
                    component="code"
                    sx={{
                      fontSize: 11,
                      fontFamily: "monospace",
                      bgcolor: "action.hover",
                      px: 0.75,
                      py: 0.3,
                      borderRadius: 1,
                      color: "text.primary",
                      whiteSpace: "nowrap",
                    }}
                  >
                    {cmd}
                  </Box>
                </Box>
              ))}
            </Stack>

            <Typography variant="body2" color="text.secondary" sx={{ mt: 1.5 }}>
              3. 或直接在{" "}
              <Box component="strong">Settings → Providers</Box>
              {" "}中粘贴 API Key 并保存。
            </Typography>
          </Box>
        </Stack>
      </DialogContent>

      <DialogActions sx={{ px: 3, pb: 2.5, pt: 1 }}>
        <Tooltip title={copied ? "已复制到剪贴板" : "复制 omiga.yaml 示例配置"}>
          <Button
            startIcon={copied ? <CheckCircleOutlined /> : <ContentCopyOutlined />}
            onClick={() => void handleCopyConfig()}
            color={copied ? "success" : "inherit"}
            variant="outlined"
            size="small"
          >
            {copied ? "已复制" : "复制示例配置"}
          </Button>
        </Tooltip>

        <Box sx={{ flex: 1 }} />

        <Button size="small" onClick={() => setOpen(false)} color="inherit">
          稍后配置
        </Button>
        <Button
          size="small"
          variant="contained"
          onClick={() => setOpen(false)}
          startIcon={<SettingsOutlined />}
        >
          已配置，关闭
        </Button>
      </DialogActions>
    </Dialog>
  );
}
