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
  const paper = theme.palette.background.paper;
  const def = theme.palette.background.default;
  const ink = theme.palette.text.primary;
  const mut = theme.palette.text.secondary;
  const isDark = theme.palette.mode === "dark";
  /** Hairline border / shadow tint — theme-aware */
  const edge = (a: number) =>
    alpha(isDark ? theme.palette.common.white : theme.palette.common.black, a);
  /** Input card — closer to solid paper so the typing area reads lighter */
  const composerBg = alpha(paper, isDark ? 0.97 : 0.99);
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
    <Stack spacing={0.75}>
      <Paper
        elevation={0}
        sx={{
          borderRadius: 3,
          overflow: "hidden",
          position: "relative",
          bgcolor: composerBg,
          backdropFilter: "blur(12px)",
          WebkitBackdropFilter: "blur(12px)",
          border: `1px solid ${edge(0.12)}`,
          boxShadow: `
            0 1px 2px ${edge(0.06)},
            0 8px 24px ${alpha(accent, 0.08)},
            inset 0 1px 0 ${edge(0.08)}
          `,
          transition: "box-shadow 0.22s ease, border-color 0.22s ease, transform 0.22s ease",
          "@media (prefers-reduced-motion: reduce)": {
            transition: "none",
          },
          "&:focus-within": {
            borderColor: alpha(accent, 0.45),
            boxShadow: `
              0 1px 2px ${edge(0.08)},
              0 0 0 3px ${alpha(accent, 0.18)},
              0 12px 32px ${alpha(accent, 0.12)}
            `,
          },
        }}
      >
        <Box
          component="textarea"
          ref={inputRef}
          value={input}
          onChange={(e) => onInputChange(e.target.value)}
          onKeyDown={onKeyDown}
          disabled={inputDisabled}
          placeholder={placeholder}
          rows={2}
          aria-label="消息输入"
          sx={{
            width: "100%",
            boxSizing: "border-box",
            border: "none",
            resize: "none",
            minHeight: 56,
            maxHeight: 280,
            px: 1.75,
            py: 1.15,
            fontSize: 15,
            fontFamily: "inherit",
            lineHeight: 1.55,
            letterSpacing: "-0.01em",
            color: ink,
            bgcolor: "transparent",
            outline: "none",
            caretColor: accent,
            transition: "color 0.15s ease",
            "&::placeholder": {
              color: alpha(mut, 0.65),
              opacity: 1,
            },
            "&:disabled": {
              color: alpha(ink, 0.38),
              cursor: "not-allowed",
            },
          }}
        />
        <Divider sx={{ borderColor: edge(0.08) }} />
        <Stack
          direction="row"
          alignItems="center"
          spacing={0.5}
          sx={{
            px: 1,
            py: 0.5,
            flexWrap: "wrap",
            gap: 0.5,
            background: isDark
              ? `linear-gradient(165deg, ${alpha(paper, 0.48)} 0%, ${alpha(def, 0.94)} 48%, ${alpha(def, 0.72)} 100%)`
              : `linear-gradient(165deg, ${alpha(paper, 0.72)} 0%, ${alpha(def, 0.97)} 48%, ${alpha(paper, 0.65)} 100%)`,
            borderTop: `1px solid ${edge(0.12)}`,
            boxShadow: `inset 0 1px 0 ${edge(0.06)}`,
          }}
        >
          <Tooltip title="添加附件、连接器与插件">
            <IconButton
              size="small"
              onClick={(e) => setPlusAnchor(e.currentTarget)}
              sx={{
                color: mut,
                width: 36,
                height: 36,
                borderRadius: 2,
                bgcolor: alpha(paper, isDark ? 0.25 : 0.72),
                border: `1px solid ${edge(0.12)}`,
                boxShadow: `0 1px 2px ${edge(0.05)}`,
                transition:
                  "background-color 0.2s ease, color 0.2s ease, box-shadow 0.2s ease, transform 0.2s ease",
                "@media (prefers-reduced-motion: reduce)": {
                  transition: "none",
                },
                "&:hover": {
                  bgcolor: alpha(accent, 0.1),
                  color: accent,
                  borderColor: alpha(accent, 0.22),
                  boxShadow: `0 2px 8px ${alpha(accent, 0.12)}`,
                  transform: "translateY(-1px)",
                },
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
              color: mut,
              fontWeight: 600,
              borderRadius: 2.5,
              px: 1,
              minHeight: 32,
              maxWidth: 200,
              border: `1px solid ${edge(0.1)}`,
              bgcolor: alpha(paper, isDark ? 0.35 : 0.65),
              boxShadow: `inset 0 1px 0 ${edge(0.06)}`,
              transition:
                "background-color 0.2s ease, border-color 0.2s ease, box-shadow 0.2s ease",
              "@media (prefers-reduced-motion: reduce)": {
                transition: "none",
              },
              "&:hover": {
                bgcolor: alpha(accent, 0.07),
                borderColor: alpha(accent, 0.2),
                boxShadow: `0 1px 4px ${alpha(accent, 0.08)}`,
              },
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
              fontWeight: 600,
              borderRadius: 2.5,
              minHeight: 32,
              px: 1,
              borderColor: edge(0.14),
              color: ink,
              maxWidth: 180,
              bgcolor: alpha(paper, isDark ? 0.45 : 0.88),
              boxShadow: `0 1px 2px ${edge(0.05)}, inset 0 1px 0 ${edge(0.06)}`,
              transition: "border-color 0.2s ease, box-shadow 0.2s ease, transform 0.2s ease",
              "@media (prefers-reduced-motion: reduce)": {
                transition: "none",
              },
              "&:hover": {
                borderColor: alpha(accent, 0.4),
                bgcolor: alpha(paper, isDark ? 0.55 : 1),
                boxShadow: `0 2px 10px ${alpha(accent, 0.12)}, 0 0 0 1px ${alpha(accent, 0.15)}`,
                transform: "translateY(-1px)",
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
                  width: 36,
                  height: 36,
                  borderRadius: 2,
                  color: "#fff",
                  bgcolor: "#ef4444",
                  boxShadow: `0 2px 8px ${alpha("#ef4444", 0.35)}`,
                  transition: "background-color 0.2s ease, box-shadow 0.2s ease, transform 0.2s ease",
                  "@media (prefers-reduced-motion: reduce)": {
                    transition: "none",
                  },
                  "&:hover": {
                    bgcolor: "#dc2626",
                    boxShadow: `0 4px 14px ${alpha("#ef4444", 0.45)}`,
                    transform: "translateY(-1px)",
                  },
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
                    color: "text.disabled",
                    width: 36,
                    height: 36,
                    borderRadius: 2,
                    border: `1px dashed ${edge(0.18)}`,
                    bgcolor: alpha(paper, isDark ? 0.2 : 0.4),
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
        rowGap={0.75}
        columnGap={1.5}
        sx={{
          px: 1.25,
          py: 0.65,
          borderRadius: 2.5,
          bgcolor: alpha(paper, isDark ? 0.35 : 0.72),
          backdropFilter: "blur(10px)",
          WebkitBackdropFilter: "blur(10px)",
          border: `1px solid ${edge(0.12)}`,
          boxShadow: `
            0 1px 2px ${edge(0.05)},
            0 6px 20px ${alpha(accent, 0.06)},
            inset 0 1px 0 ${edge(0.06)}
          `,
          transition: "box-shadow 0.22s ease, border-color 0.22s ease",
          "@media (prefers-reduced-motion: reduce)": {
            transition: "none",
          },
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
              color: needsWorkspacePath ? "#FF9500" : ink,
              fontWeight: 600,
              maxWidth: { xs: "100%", sm: 240 },
              borderRadius: 2.5,
              px: 1,
              py: 0.35,
              minHeight: 32,
              bgcolor: needsWorkspacePath
                ? alpha("#FF9500", 0.1)
                : isDark
                  ? alpha(def, 0.75)
                  : alpha("#f1f5f9", 0.9),
              border: `1px solid ${
                needsWorkspacePath ? alpha("#FF9500", 0.35) : edge(0.1)
              }`,
              boxShadow: `inset 0 1px 0 ${edge(0.06)}`,
              transition: "background-color 0.2s ease, border-color 0.2s ease, box-shadow 0.2s ease",
              "@media (prefers-reduced-motion: reduce)": {
                transition: "none",
              },
              "&:hover": {
                bgcolor: needsWorkspacePath ? alpha("#FF9500", 0.14) : alpha(accent, 0.06),
                borderColor: needsWorkspacePath ? alpha("#FF9500", 0.5) : alpha(accent, 0.22),
              },
            }}
          >
            <Typography variant="body2" noWrap component="span">
              {pathLabel}
            </Typography>
          </Button>

          {gitInfo?.isGit && !needsWorkspacePath && (
            <Stack direction="row" alignItems="center" spacing={0.5}>
              <Tag sx={{ fontSize: 18, color: "text.secondary" }} />
              <FormControl size="small" sx={{ minWidth: 148 }}>
                <Select
                  value={branchValue || gitInfo.currentBranch}
                  displayEmpty
                  onChange={(e) => {
                    const b = String(e.target.value);
                    setBranchForRoot(rootKey, b);
                  }}
                  sx={{
                    bgcolor: alpha(paper, isDark ? 0.5 : 0.95),
                    borderRadius: 2,
                    fontSize: 13,
                    fontWeight: 600,
                    boxShadow: `0 1px 2px ${edge(0.05)}`,
                    transition: "box-shadow 0.2s ease, border-color 0.2s ease",
                    "& .MuiOutlinedInput-notchedOutline": {
                      borderColor: edge(0.14),
                    },
                    "&:hover .MuiOutlinedInput-notchedOutline": {
                      borderColor: alpha(accent, 0.35),
                    },
                    "&.Mui-focused .MuiOutlinedInput-notchedOutline": {
                      borderColor: alpha(accent, 0.55),
                      boxShadow: `0 0 0 3px ${alpha(accent, 0.15)}`,
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
                sx={{
                  py: 0,
                  color: alpha(accent, 0.55),
                  "&.Mui-checked": { color: accent },
                }}
              />
            }
            label={
              <Typography variant="body2" fontWeight={600} color="text.secondary">
                worktree
              </Typography>
            }
            sx={{
              mr: 0,
              px: 0.75,
              py: 0.25,
              borderRadius: 2,
              border: `1px solid ${edge(0.1)}`,
              bgcolor: isDark ? alpha(def, 0.65) : alpha("#f8fafc", 0.9),
              transition: "background-color 0.2s ease, border-color 0.2s ease",
              "&:hover": {
                bgcolor: alpha(accent, 0.04),
                borderColor: alpha(accent, 0.18),
              },
            }}
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
              fontWeight: 600,
              borderRadius: 2.5,
              minHeight: 32,
              px: 1,
              borderColor: edge(0.14),
              color: ink,
              bgcolor: alpha(paper, isDark ? 0.45 : 0.95),
              boxShadow: `0 1px 2px ${edge(0.05)}, inset 0 1px 0 ${edge(0.06)}`,
              transition: "border-color 0.2s ease, box-shadow 0.2s ease",
              "&:hover": {
                borderColor: alpha(accent, 0.35),
                bgcolor: alpha(paper, isDark ? 0.55 : 1),
                boxShadow: `0 2px 8px ${alpha(accent, 0.1)}`,
              },
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
