import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  Alert,
  Box,
  Button,
  Chip,
  CircularProgress,
  Divider,
  Grid,
  Paper,
  Stack,
  Tab,
  Tabs,
  TextField,
  Tooltip,
  Typography,
  alpha,
  useTheme,
} from "@mui/material";
import {
  FolderOpen as FolderIcon,
  Refresh as RefreshIcon,
  Save as SaveIcon,
  TextSnippet as FileIcon,
} from "@mui/icons-material";
import {
  PROFILE_FILE_DEFINITIONS,
  type ProfileFilename,
  getProfileFileDefinition,
  summarizeProfileMarkdown,
} from "./profileFiles";

interface UserOmigaFileResponse {
  filename: ProfileFilename;
  path: string;
  content: string;
  exists: boolean;
}

interface ProfileDocumentState extends UserOmigaFileResponse {
  draft: string;
}

type ProfileDocuments = Record<ProfileFilename, ProfileDocumentState>;

function emptyDocuments(): ProfileDocuments {
  return PROFILE_FILE_DEFINITIONS.reduce((acc, definition) => {
    acc[definition.filename] = {
      filename: definition.filename,
      path: `~/.omiga/${definition.filename}`,
      content: "",
      draft: "",
      exists: false,
    };
    return acc;
  }, {} as ProfileDocuments);
}

function normalizeProfileFileResponse(response: UserOmigaFileResponse): ProfileDocumentState {
  return {
    ...response,
    draft: response.content,
  };
}

export function ProfileSettingsTab() {
  const theme = useTheme();
  const [activeFile, setActiveFile] = useState<ProfileFilename>("USER.md");
  const [documents, setDocuments] = useState<ProfileDocuments>(() => emptyDocuments());
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [message, setMessage] = useState<{
    type: "success" | "error" | "info";
    text: string;
  } | null>(null);

  const loadFiles = useCallback(async () => {
    setLoading(true);
    setMessage(null);
    try {
      const responses = await Promise.all(
        PROFILE_FILE_DEFINITIONS.map((definition) =>
          invoke<UserOmigaFileResponse>("read_user_omiga_file", {
            filename: definition.filename,
          }),
        ),
      );
      setDocuments((prev) => {
        const next = { ...prev };
        for (const response of responses) {
          next[response.filename] = normalizeProfileFileResponse(response);
        }
        return next;
      });
    } catch (error) {
      setMessage({ type: "error", text: `读取 Profile 失败：${error}` });
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void loadFiles();
  }, [loadFiles]);

  const activeDocument = documents[activeFile];
  const activeDefinition = getProfileFileDefinition(activeFile);
  const activeSummary = useMemo(
    () => summarizeProfileMarkdown(activeDocument.draft),
    [activeDocument.draft],
  );
  const hasDirtyDocument = Object.values(documents).some(
    (document) => document.draft !== document.content,
  );
  const missingFiles = PROFILE_FILE_DEFINITIONS.filter(
    (definition) => !documents[definition.filename].exists,
  );

  const handleDraftChange = (value: string) => {
    setDocuments((prev) => ({
      ...prev,
      [activeFile]: {
        ...prev[activeFile],
        draft: value,
      },
    }));
  };

  const handleSave = async () => {
    setSaving(true);
    setMessage(null);
    try {
      await invoke("write_user_omiga_file", {
        filename: activeFile,
        content: activeDocument.draft,
      });
      setDocuments((prev) => ({
        ...prev,
        [activeFile]: {
          ...prev[activeFile],
          content: prev[activeFile].draft,
          exists: true,
        },
      }));
      setMessage({
        type: "success",
        text: `${activeDefinition.filename} 已保存，下一轮对话会自动读取。`,
      });
    } catch (error) {
      setMessage({ type: "error", text: `保存失败：${error}` });
    } finally {
      setSaving(false);
    }
  };

  const handleEnsureTemplates = async () => {
    setLoading(true);
    setMessage(null);
    try {
      const responses = await invoke<UserOmigaFileResponse[]>("ensure_user_profile_files");
      setDocuments((prev) => {
        const next = { ...prev };
        for (const response of responses) {
          next[response.filename] = normalizeProfileFileResponse(response);
        }
        return next;
      });
      setMessage({
        type: "success",
        text: "默认 Profile 模板已创建或补齐（不会重新触发首次引导）。",
      });
    } catch (error) {
      setMessage({ type: "error", text: `创建模板失败：${error}` });
    } finally {
      setLoading(false);
    }
  };

  const heroBorder = `1px solid ${alpha(theme.palette.primary.main, 0.22)}`;
  const surface = {
    borderRadius: 2.5,
    border: `1px solid ${alpha(theme.palette.divider, 0.65)}`,
    bgcolor: alpha(theme.palette.background.paper, 0.76),
  } as const;

  return (
    <Box sx={{ maxWidth: 980, mx: "auto", pb: 2 }}>
      <Paper
        elevation={0}
        sx={{
          p: { xs: 2, sm: 2.5 },
          mb: 2,
          borderRadius: 3,
          border: heroBorder,
          overflow: "hidden",
          position: "relative",
          background: `linear-gradient(125deg, ${alpha(theme.palette.primary.main, 0.12)} 0%, ${alpha(theme.palette.secondary.main, 0.08)} 44%, ${alpha(theme.palette.background.paper, 0.9)} 100%)`,
        }}
      >
        <Box
          aria-hidden
          sx={{
            position: "absolute",
            right: -50,
            top: -80,
            width: 240,
            height: 240,
            borderRadius: "50%",
            background: `radial-gradient(circle, ${alpha(theme.palette.primary.main, 0.18)} 0%, transparent 70%)`,
            pointerEvents: "none",
          }}
        />
        <Stack spacing={1.25} sx={{ position: "relative", zIndex: 1 }}>
          <Typography variant="overline" sx={{ letterSpacing: 0.14, color: "text.secondary", fontWeight: 700 }}>
            Profile · 个性化上下文
          </Typography>
          <Typography variant="h6" fontWeight={750}>
            管理首次使用时引导配置的用户偏好、习惯与智能体身份
          </Typography>
          <Typography variant="body2" color="text.secondary" sx={{ lineHeight: 1.7, maxWidth: 820 }}>
            这些 Markdown 文件保存在 <strong>~/.omiga/</strong>，会被编译成 Permanent Profile 并注入后续对话。
            适合写稳定偏好、工作习惯、Agent 风格与边界；请不要保存 API Key、令牌或其他敏感信息。
          </Typography>
          <Stack direction="row" spacing={1} flexWrap="wrap" useFlexGap>
            {PROFILE_FILE_DEFINITIONS.map((definition) => {
              const document = documents[definition.filename];
              const dirty = document.draft !== document.content;
              return (
                <Chip
                  key={definition.filename}
                  size="small"
                  icon={<FileIcon fontSize="small" />}
                  label={`${definition.filename}${dirty ? " · 未保存" : document.exists ? "" : " · 未创建"}`}
                  color={dirty ? "warning" : document.exists ? "default" : "info"}
                  variant={dirty || !document.exists ? "outlined" : "filled"}
                />
              );
            })}
          </Stack>
        </Stack>
      </Paper>

      {message && (
        <Alert severity={message.type} sx={{ mb: 2, borderRadius: 2 }}>
          {message.text}
        </Alert>
      )}

      {missingFiles.length > 0 && (
        <Alert
          severity="info"
          sx={{ mb: 2, borderRadius: 2 }}
          action={
            <Button color="inherit" size="small" onClick={() => void handleEnsureTemplates()} disabled={loading || saving}>
              创建模板
            </Button>
          }
        >
          缺少 {missingFiles.map((file) => file.filename).join("、")}。可以先创建默认模板，再按需编辑。
        </Alert>
      )}

      <Paper elevation={0} sx={{ p: 0.75, mb: 2, ...surface }}>
        <Tabs
          value={activeFile}
          onChange={(_, value) => setActiveFile(value as ProfileFilename)}
          variant="scrollable"
          scrollButtons="auto"
          aria-label="Profile files"
          sx={{
            minHeight: 58,
            "& .MuiTabs-flexContainer": { gap: 0.5 },
            "& .MuiTabs-indicator": { display: "none" },
            "& .MuiTab-root": {
              minHeight: 56,
              alignItems: "flex-start",
              justifyContent: "flex-start",
              textAlign: "left",
              borderRadius: 2,
              textTransform: "none",
              px: 1.5,
              py: 1,
              flex: { xs: "0 0 auto", md: 1 },
              maxWidth: "none",
              transition: "background-color 160ms ease, box-shadow 160ms ease",
            },
            "& .Mui-selected": {
              bgcolor: alpha(theme.palette.primary.main, theme.palette.mode === "dark" ? 0.18 : 0.1),
              boxShadow: `inset 0 0 0 1px ${alpha(theme.palette.primary.main, 0.22)}`,
            },
          }}
        >
          {PROFILE_FILE_DEFINITIONS.map((definition) => {
            const Icon = definition.icon;
            const document = documents[definition.filename];
            const dirty = document.draft !== document.content;
            return (
              <Tab
                key={definition.filename}
                value={definition.filename}
                label={
                  <Stack spacing={0.35} sx={{ minWidth: { xs: 180, md: 0 }, width: "100%" }}>
                    <Stack direction="row" alignItems="center" spacing={1}>
                      <Icon sx={{ fontSize: 18, color: activeFile === definition.filename ? "primary.main" : "text.secondary" }} />
                      <Typography variant="body2" fontWeight={750}>
                        {definition.filename}
                      </Typography>
                      {dirty && <Chip size="small" label="未保存" color="warning" variant="outlined" sx={{ ml: "auto" }} />}
                      {!dirty && !document.exists && <Chip size="small" label="未创建" color="info" variant="outlined" sx={{ ml: "auto" }} />}
                    </Stack>
                    <Typography variant="caption" color="text.secondary" sx={{ lineHeight: 1.35 }}>
                      {definition.label} · {definition.role}
                    </Typography>
                  </Stack>
                }
              />
            );
          })}
        </Tabs>
      </Paper>

      <Paper elevation={0} sx={{ p: { xs: 2, sm: 2.25 }, ...surface }}>
        <Stack spacing={2}>
          <Stack direction={{ xs: "column", sm: "row" }} spacing={1.5} alignItems={{ sm: "flex-start" }}>
            <Box sx={{ flex: 1, minWidth: 0 }}>
              <Typography variant="overline" sx={{ letterSpacing: 0.12, color: "text.secondary", fontWeight: 700 }}>
                {activeDefinition.filename}
              </Typography>
              <Typography variant="subtitle1" fontWeight={750} sx={{ mb: 0.5 }}>
                {activeDefinition.label}
              </Typography>
              <Typography variant="body2" color="text.secondary" sx={{ lineHeight: 1.65 }}>
                {activeDefinition.description}
              </Typography>
            </Box>
            <Stack direction="row" spacing={1} sx={{ flexShrink: 0 }}>
              <Tooltip title="重新读取磁盘内容；会丢弃未保存草稿">
                <span>
                  <Button
                    size="small"
                    variant="outlined"
                    startIcon={loading ? <CircularProgress size={14} /> : <RefreshIcon />}
                    onClick={() => void loadFiles()}
                    disabled={loading || saving}
                    sx={{ borderRadius: 2, textTransform: "none", fontWeight: 600 }}
                  >
                    重新加载
                  </Button>
                </span>
              </Tooltip>
              <Button
                size="small"
                variant="contained"
                startIcon={saving ? <CircularProgress size={14} color="inherit" /> : <SaveIcon />}
                onClick={() => void handleSave()}
                disabled={loading || saving || activeDocument.draft === activeDocument.content}
                sx={{ borderRadius: 2, textTransform: "none", fontWeight: 650 }}
              >
                保存
              </Button>
            </Stack>
          </Stack>

          <Stack direction="row" spacing={1} alignItems="center" flexWrap="wrap" useFlexGap>
            <Chip
              size="small"
              icon={<FolderIcon fontSize="small" />}
              label={activeDocument.path}
              variant="outlined"
              sx={{ maxWidth: "100%", "& .MuiChip-label": { overflow: "hidden", textOverflow: "ellipsis" } }}
            />
            <Chip size="small" label={`${activeSummary.meaningfulLineCount} 条有效行`} />
            <Chip size="small" label={`${activeSummary.charCount} 字符`} />
            {activeSummary.placeholderCount > 0 && (
              <Chip size="small" color="warning" variant="outlined" label={`${activeSummary.placeholderCount} 个占位提示`} />
            )}
            {!activeDocument.exists && <Chip size="small" color="info" variant="outlined" label="文件尚未创建" />}
            {hasDirtyDocument && <Chip size="small" color="warning" variant="outlined" label="存在未保存改动" />}
          </Stack>

          <TextField
            value={activeDocument.draft}
            onChange={(event) => handleDraftChange(event.target.value)}
            disabled={loading || saving}
            multiline
            minRows={18}
            maxRows={34}
            fullWidth
            placeholder={`编辑 ${activeDefinition.filename}…`}
            inputProps={{ spellCheck: false }}
            sx={{
              "& textarea": {
                fontFamily: "ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace",
                fontSize: "0.86rem",
                lineHeight: 1.6,
              },
            }}
          />

          <Divider />

          <Box>
            <Typography variant="body2" fontWeight={700} sx={{ mb: 1 }}>
              建议填写内容
            </Typography>
            <Grid container spacing={1}>
              {activeDefinition.prompts.map((prompt) => (
                <Grid item xs={12} sm={4} key={prompt}>
                  <Box
                    sx={{
                      p: 1.25,
                      height: "100%",
                      borderRadius: 2,
                      border: `1px dashed ${alpha(theme.palette.divider, 0.9)}`,
                      bgcolor: alpha(theme.palette.action.hover, 0.05),
                    }}
                  >
                    <Typography variant="caption" color="text.secondary" sx={{ lineHeight: 1.5 }}>
                      {prompt}
                    </Typography>
                  </Box>
                </Grid>
              ))}
            </Grid>
          </Box>
        </Stack>
      </Paper>
    </Box>
  );
}
