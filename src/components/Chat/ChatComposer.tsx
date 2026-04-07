import { useEffect, useState, useMemo } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  Box,
  IconButton,
  Stack,
  Tooltip,
  Button,
  Menu,
  MenuItem,
  ListItemIcon,
  ListItemText,
  Divider,
  Typography,
  Select,
  FormControl,
  FormControlLabel,
  Checkbox,
  Paper,
  alpha,
  useTheme,
} from "@mui/material";
import {
  Add,
  Code,
  ExpandMore,
  Mic,
  Computer,
  PanTool,
  Assignment,
  WarningAmber,
  Extension,
  Settings as SettingsIcon,
  FolderOpen,
  Hub,
  AttachFile,
  Tag,
  Square,
} from "@mui/icons-material";
import { useUiStore, useChatComposerStore, type AgentComposerMode } from "../../state";

export interface GitWorkspaceInfo {
  isGit: boolean;
  currentBranch: string;
  branches: string[];
  displayPath: string;
}

function shortRepoLabel(path: string): string {
  const parts = path.split(/[/\\]/u).filter(Boolean);
  if (parts.length === 0) return path;
  if (parts.length <= 2) return parts.join("/");
  return `${parts[parts.length - 2]}/${parts[parts.length - 1]}`;
}

const MODE_META: Record<
  AgentComposerMode,
  { label: string; hint: string; icon: React.ReactNode }
> = {
  ask: {
    label: "询问权限",
    hint: "修改前始终询问。",
    icon: <PanTool fontSize="small" />,
  },
  auto: {
    label: "自动接受编辑",
    hint: "自动接受所有文件编辑。",
    icon: <Code fontSize="small" />,
  },
  plan: {
    label: "计划模式",
    hint: "先制定计划再修改。",
    icon: <Assignment fontSize="small" />,
  },
  bypass: {
    label: "绕过权限",
    hint: "接受所有权限（谨慎使用）。",
    icon: <WarningAmber fontSize="small" />,
  },
};

export interface ChatComposerProps {
  sessionId: string | null;
  /** Absolute workspace path when set */
  workspacePath: string;
  needsWorkspacePath: boolean;
  onPickWorkspace: () => void;
  input: string;
  onInputChange: (v: string) => void;
  onKeyDown: (e: React.KeyboardEvent<HTMLTextAreaElement>) => void;
  inputRef: React.Ref<HTMLTextAreaElement>;
  isStreaming: boolean;
  isConnecting: boolean;
  /** Stop streaming (shown in toolbar when generating; Enter 仍可发送新消息需先停止) */
  onCancel?: () => void;
}

export function ChatComposer({
  sessionId,
  workspacePath,
  needsWorkspacePath,
  onPickWorkspace,
  input,
  onInputChange,
  onKeyDown,
  inputRef,
  isStreaming,
  isConnecting,
  onCancel,
}: ChatComposerProps) {
  const theme = useTheme();
  const accent = theme.palette.primary.main;
  const accent2 = "#a855f7";
  const setSettingsOpen = useUiStore((s) => s.setSettingsOpen);
  const setSettingsTabIndex = useUiStore((s) => s.setSettingsTabIndex);
  const setRightPanelMode = useUiStore((s) => s.setRightPanelMode);
  const {
    agentMode,
    setAgentMode,
    useWorktree,
    setUseWorktree,
    environment,
    setEnvironment,
    selectedBranchByRoot,
    setBranchForRoot,
  } = useChatComposerStore();

  const [plusAnchor, setPlusAnchor] = useState<null | HTMLElement>(null);
  const [modeAnchor, setModeAnchor] = useState<null | HTMLElement>(null);
  const [modelAnchor, setModelAnchor] = useState<null | HTMLElement>(null);
  const [envAnchor, setEnvAnchor] = useState<null | HTMLElement>(null);
  const [gitInfo, setGitInfo] = useState<GitWorkspaceInfo | null>(null);
  const [modelLabel, setModelLabel] = useState<string>("模型");

  useEffect(() => {
    let cancelled = false;
    invoke<{ model?: string } | null>("get_llm_config_state", {})
      .then((cfg) => {
        if (cancelled || !cfg?.model) return;
        const m = cfg.model;
        setModelLabel(m.length > 24 ? `${m.slice(0, 22)}…` : m);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [sessionId]);

  useEffect(() => {
    if (!workspacePath || needsWorkspacePath) {
      setGitInfo(null);
      return;
    }
    let cancelled = false;
    invoke<GitWorkspaceInfo>("git_workspace_info", { path: workspacePath })
      .then((r) => {
        if (!cancelled) setGitInfo(r);
      })
      .catch(() => {
        if (!cancelled) setGitInfo(null);
      });
    return () => {
      cancelled = true;
    };
  }, [workspacePath, needsWorkspacePath]);

  const rootKey = gitInfo?.displayPath ?? workspacePath;
  const branchValue = useMemo(() => {
    if (!gitInfo?.isGit) return "";
    const saved = selectedBranchByRoot[rootKey];
    return saved ?? gitInfo.currentBranch;
  }, [gitInfo, rootKey, selectedBranchByRoot]);

  const pathLabel = needsWorkspacePath
    ? "选择工作目录"
    : gitInfo?.displayPath
      ? shortRepoLabel(gitInfo.displayPath)
      : shortRepoLabel(workspacePath);

  const placeholder = !sessionId
    ? "Select a session"
    : needsWorkspacePath
      ? "请先选择工作目录后再发送消息…"
      : "输入消息，或描述你想在代码库中完成的任务…";

  const inputDisabled = !sessionId || isStreaming || isConnecting;

  return (
    <Stack spacing={1.25}>
      <Paper
        elevation={0}
        sx={{
          borderRadius: 3,
          overflow: "hidden",
          position: "relative",
          bgcolor: alpha("#FFFFFF", 0.92),
          backdropFilter: "blur(12px)",
          WebkitBackdropFilter: "blur(12px)",
          border: `1px solid ${alpha("#0f172a", 0.08)}`,
          boxShadow: `
            0 1px 2px ${alpha("#0f172a", 0.04)},
            0 8px 24px ${alpha("#6366f1", 0.08)},
            inset 0 1px 0 ${alpha("#ffffff", 0.9)}
          `,
          transition: "box-shadow 0.22s ease, border-color 0.22s ease, transform 0.22s ease",
          "@media (prefers-reduced-motion: reduce)": {
            transition: "none",
          },
          "&:focus-within": {
            borderColor: alpha(accent, 0.45),
            boxShadow: `
              0 1px 2px ${alpha("#0f172a", 0.06)},
              0 0 0 3px ${alpha(accent, 0.18)},
              0 12px 32px ${alpha(accent, 0.12)}
            `,
          },
        }}
      >
        {/* Accent hairline — ties composer to chat bubble gradient */}
        <Box
          sx={{
            height: 3,
            background: `linear-gradient(90deg, ${accent} 0%, ${accent2} 55%, ${alpha(accent, 0.35)} 100%)`,
            opacity: 0.95,
          }}
        />
        <Box
          component="textarea"
          ref={inputRef}
          value={input}
          onChange={(e) => onInputChange(e.target.value)}
          onKeyDown={onKeyDown}
          disabled={inputDisabled}
          placeholder={placeholder}
          rows={3}
          aria-label="消息输入"
          sx={{
            width: "100%",
            boxSizing: "border-box",
            border: "none",
            resize: "none",
            minHeight: 80,
            maxHeight: 280,
            px: 2,
            py: 1.75,
            fontSize: 15,
            fontFamily: "inherit",
            lineHeight: 1.55,
            letterSpacing: "-0.01em",
            color: "#1C1C1E",
            bgcolor: "transparent",
            outline: "none",
            caretColor: accent,
            transition: "color 0.15s ease",
            "&::placeholder": {
              color: alpha("#3C3C43", 0.55),
              opacity: 1,
            },
            "&:disabled": {
              color: alpha("#1C1C1E", 0.38),
              cursor: "not-allowed",
            },
          }}
        />
        <Divider sx={{ borderColor: alpha("#0f172a", 0.06) }} />
        <Stack
          direction="row"
          alignItems="center"
          spacing={0.5}
          sx={{
            px: 1,
            py: 0.85,
            flexWrap: "wrap",
            gap: 0.5,
            background: `linear-gradient(180deg, ${alpha("#f8fafc", 0.98)} 0%, ${alpha("#f1f5f9", 0.95)} 100%)`,
            borderTop: `1px solid ${alpha("#ffffff", 0.65)}`,
          }}
        >
          <Tooltip title="添加附件、连接器与插件">
            <IconButton
              size="small"
              onClick={(e) => setPlusAnchor(e.currentTarget)}
              sx={{
                color: "#3C3C43",
                width: 40,
                height: 40,
                transition: "background-color 0.18s ease, color 0.18s ease",
                "&:hover": { bgcolor: alpha(accent, 0.08), color: accent },
                "&:focus-visible": {
                  outline: `2px solid ${alpha(accent, 0.45)}`,
                  outlineOffset: 2,
                },
              }}
            >
              <Add fontSize="small" />
            </IconButton>
          </Tooltip>
          <Menu
            anchorEl={plusAnchor}
            open={Boolean(plusAnchor)}
            onClose={() => setPlusAnchor(null)}
            slotProps={{ paper: { sx: { minWidth: 240, borderRadius: 2 } } }}
          >
            <MenuItem disabled>
              <ListItemIcon>
                <AttachFile fontSize="small" />
              </ListItemIcon>
              <ListItemText primary="添加文件或图片" secondary="即将推出" />
            </MenuItem>
            <MenuItem disabled>
              <ListItemIcon>
                <Hub fontSize="small" />
              </ListItemIcon>
              <ListItemText primary="连接器（MCP）" secondary="在设置中管理" />
            </MenuItem>
            <MenuItem
              onClick={() => {
                setPlusAnchor(null);
                setSettingsTabIndex(0);
                setSettingsOpen(true);
                setRightPanelMode("settings");
              }}
            >
              <ListItemIcon>
                <Extension fontSize="small" />
              </ListItemIcon>
              <ListItemText primary="插件与 MCP 服务" secondary="打开设置" />
            </MenuItem>
            <Divider />
            <MenuItem
              onClick={() => {
                setPlusAnchor(null);
                setSettingsTabIndex(0);
                setSettingsOpen(true);
                setRightPanelMode("settings");
              }}
            >
              <ListItemIcon>
                <SettingsIcon fontSize="small" />
              </ListItemIcon>
              <ListItemText primary="管理连接器与工具" />
            </MenuItem>
          </Menu>

          <Button
            size="small"
            variant="text"
            onClick={(e) => setModeAnchor(e.currentTarget)}
            startIcon={MODE_META[agentMode].icon}
            endIcon={<ExpandMore sx={{ fontSize: 18 }} />}
            sx={{
              textTransform: "none",
              color: "#3C3C43",
              fontWeight: 500,
              borderRadius: 2,
              px: 1,
              maxWidth: 200,
              transition: "background-color 0.18s ease",
              "&:hover": { bgcolor: alpha(accent, 0.06) },
            }}
          >
            <Typography variant="body2" noWrap component="span">
              {MODE_META[agentMode].label}
            </Typography>
          </Button>
          <Menu
            anchorEl={modeAnchor}
            open={Boolean(modeAnchor)}
            onClose={() => setModeAnchor(null)}
            slotProps={{ paper: { sx: { minWidth: 280, borderRadius: 2 } } }}
          >
            {(Object.keys(MODE_META) as AgentComposerMode[]).map((key) => (
              <MenuItem
                key={key}
                selected={agentMode === key}
                onClick={() => {
                  setAgentMode(key);
                  setModeAnchor(null);
                }}
              >
                <ListItemIcon>{MODE_META[key].icon}</ListItemIcon>
                <ListItemText
                  primary={MODE_META[key].label}
                  secondary={MODE_META[key].hint}
                  secondaryTypographyProps={{ variant: "caption" }}
                />
              </MenuItem>
            ))}
          </Menu>

          <Box sx={{ flex: 1, minWidth: 8 }} />

          <Button
            size="small"
            variant="outlined"
            color="inherit"
            onClick={(e) => setModelAnchor(e.currentTarget)}
            endIcon={<ExpandMore sx={{ fontSize: 18 }} />}
            sx={{
              textTransform: "none",
              fontWeight: 500,
              borderColor: alpha("#0f172a", 0.12),
              color: "#3C3C43",
              maxWidth: 160,
              bgcolor: alpha("#ffffff", 0.85),
              transition: "border-color 0.18s ease, box-shadow 0.18s ease",
              "&:hover": {
                borderColor: alpha(accent, 0.35),
                bgcolor: "#fff",
                boxShadow: `0 0 0 1px ${alpha(accent, 0.12)}`,
              },
            }}
          >
            <Typography variant="body2" noWrap component="span">
              {modelLabel}
            </Typography>
          </Button>
          <Menu
            anchorEl={modelAnchor}
            open={Boolean(modelAnchor)}
            onClose={() => setModelAnchor(null)}
          >
            <MenuItem
              onClick={() => {
                setModelAnchor(null);
                setSettingsTabIndex(0);
                setSettingsOpen(true);
                setRightPanelMode("settings");
              }}
            >
              <ListItemIcon>
                <SettingsIcon fontSize="small" />
              </ListItemIcon>
              <ListItemText primary="在设置中配置模型" />
            </MenuItem>
          </Menu>

          {isStreaming && onCancel ? (
            <Tooltip title="停止生成">
              <IconButton
                size="small"
                onClick={onCancel}
                sx={{
                  width: 40,
                  height: 40,
                  color: "#fff",
                  bgcolor: "#ef4444",
                  "&:hover": { bgcolor: "#dc2626" },
                  "&:focus-visible": {
                    outline: `2px solid ${alpha("#ef4444", 0.6)}`,
                    outlineOffset: 2,
                  },
                }}
              >
                <Square fontSize="small" />
              </IconButton>
            </Tooltip>
          ) : (
            <Tooltip title="语音输入即将推出">
              <span>
                <IconButton
                  size="small"
                  disabled
                  sx={{
                    color: "#8E8E93",
                    width: 40,
                    height: 40,
                  }}
                >
                  <Mic fontSize="small" />
                </IconButton>
              </span>
            </Tooltip>
          )}
        </Stack>
      </Paper>

      {/* Bottom: left = path + branch · right = worktree + remote/local */}
      <Stack
        direction="row"
        alignItems="center"
        justifyContent="space-between"
        flexWrap="wrap"
        rowGap={1}
        columnGap={2}
        sx={{
          px: 1.25,
          py: 1,
          borderRadius: 2.5,
          bgcolor: alpha("#f1f5f9", 0.85),
          border: `1px solid ${alpha("#0f172a", 0.07)}`,
          boxShadow: `inset 0 1px 0 ${alpha("#ffffff", 0.7)}`,
        }}
      >
        <Stack
          direction="row"
          alignItems="center"
          spacing={1}
          flexWrap="wrap"
          sx={{ flex: 1, minWidth: 0, justifyContent: "flex-start" }}
        >
          <Button
            size="small"
            variant="text"
            startIcon={<FolderOpen sx={{ fontSize: 18 }} />}
            onClick={onPickWorkspace}
            sx={{
              textTransform: "none",
              color: needsWorkspacePath ? "#FF9500" : "#1C1C1E",
              fontWeight: 600,
              maxWidth: { xs: "100%", sm: 220 },
            }}
          >
            <Typography variant="body2" noWrap component="span">
              {pathLabel}
            </Typography>
          </Button>

          {gitInfo?.isGit && !needsWorkspacePath && (
            <Stack direction="row" alignItems="center" spacing={0.5}>
              <Tag sx={{ fontSize: 18, color: "#8E8E93" }} />
              <FormControl size="small" sx={{ minWidth: 140 }}>
                <Select
                  value={branchValue || gitInfo.currentBranch}
                  displayEmpty
                  onChange={(e) => {
                    const b = String(e.target.value);
                    setBranchForRoot(rootKey, b);
                  }}
                  sx={{
                    bgcolor: "#FFFFFF",
                    borderRadius: 1.5,
                    fontSize: 13,
                    "& .MuiOutlinedInput-notchedOutline": {
                      borderColor: alpha("#000", 0.12),
                    },
                  }}
                >
                  {gitInfo.branches.map((b) => (
                    <MenuItem key={b} value={b}>
                      {b}
                    </MenuItem>
                  ))}
                </Select>
              </FormControl>
            </Stack>
          )}

          {!gitInfo?.isGit && !needsWorkspacePath && workspacePath && (
            <Typography variant="caption" color="text.secondary">
              非 Git 仓库
            </Typography>
          )}
        </Stack>

        <Stack
          direction="row"
          alignItems="center"
          spacing={1}
          flexWrap="wrap"
          sx={{ flexShrink: 0, justifyContent: "flex-end", ml: { xs: 0, sm: "auto" } }}
        >
          <FormControlLabel
            control={
              <Checkbox
                size="small"
                checked={useWorktree}
                onChange={(_, v) => setUseWorktree(v)}
                sx={{ py: 0 }}
              />
            }
            label={
              <Typography variant="body2" color="text.secondary">
                worktree
              </Typography>
            }
            sx={{ mr: 0 }}
          />

          <Button
            size="small"
            variant="outlined"
            color="inherit"
            startIcon={
              environment === "local" ? (
                <Computer fontSize="small" />
              ) : (
                <Hub fontSize="small" />
              )
            }
            endIcon={<ExpandMore sx={{ fontSize: 18 }} />}
            onClick={(e) => setEnvAnchor(e.currentTarget)}
            sx={{
              textTransform: "none",
              borderColor: "#D1D1D6",
              color: "#3C3C43",
              bgcolor: "#FFFFFF",
            }}
          >
            {environment === "local" ? "本地" : "远程"}
          </Button>
          <Menu
            anchorEl={envAnchor}
            open={Boolean(envAnchor)}
            onClose={() => setEnvAnchor(null)}
            slotProps={{ paper: { sx: { minWidth: 220 } } }}
          >
            <MenuItem
              selected={environment === "local"}
              onClick={() => {
                setEnvironment("local");
                setEnvAnchor(null);
              }}
            >
              <ListItemIcon>
                <Computer fontSize="small" />
              </ListItemIcon>
              <ListItemText primary="本地" secondary="在本机运行工具与终端" />
            </MenuItem>
            <Divider />
            <MenuItem disabled>
              <ListItemIcon>
                <Add fontSize="small" />
              </ListItemIcon>
              <ListItemText primary="添加 SSH 连接" secondary="即将推出" />
            </MenuItem>
            <MenuItem disabled>
              <ListItemText
                primaryTypographyProps={{ variant: "caption", color: "text.secondary" }}
                primary="远程控制"
              />
            </MenuItem>
            <MenuItem
              selected={environment === "remote"}
              onClick={() => {
                setEnvironment("remote");
                setEnvAnchor(null);
              }}
            >
              <ListItemIcon>
                <Hub fontSize="small" />
              </ListItemIcon>
              <ListItemText primary="远程" secondary="占位：后续对接远程环境" />
            </MenuItem>
          </Menu>
        </Stack>
      </Stack>
    </Stack>
  );
}
