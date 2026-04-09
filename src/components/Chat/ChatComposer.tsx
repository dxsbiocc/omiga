import {
  useEffect,
  useState,
  useMemo,
  useRef,
  useCallback,
  createElement,
  type Ref,
  type MutableRefObject,
  type KeyboardEvent,
} from "react";
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
  Collapse,
  Select,
  FormControl,
  FormControlLabel,
  Checkbox,
  Paper,
  Chip,
  Popover,
  List,
  ListItemButton,
  CircularProgress,
  alpha,
  useTheme,
} from "@mui/material";
import {
  Add,
  ExpandMore,
  Mic,
  Square,
  SmartToy,
  ForumOutlined,
  Close,
  ArticleOutlined,
  InsertDriveFile,
  Edit,
  ArrowUpward,
  DeleteOutline,
  HourglassEmpty,
} from "@mui/icons-material";
import type { LucideIcon } from "lucide-react";
import {
  Hand,
  Code as LucideCode,
  AlertTriangle,
  ChevronDown,
  FolderOpen as LucideFolderOpen,
  Laptop,
  Globe2,
  GitBranch,
  File as LucideFile,
  Folder as LucideFolder,
  Plus,
} from "lucide-react";
import {
  useUiStore,
  useChatComposerStore,
  type PermissionMode,
} from "../../state";
import { usePencilPalette } from "../../theme";
import { ProviderSwitcher } from "./ProviderSwitcher";
import type { BackgroundAgentTask } from "./backgroundAgentTypes";
import {
  canSendFollowUpToTask,
  shortBgTaskLabel,
} from "./backgroundAgentTypes";

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

/** Lucide：`stroke="currentColor"`，父级 `color` / `sx` 即可改色 */
const PERMISSION_ICON: Record<PermissionMode, LucideIcon> = {
  ask: Hand,
  auto: LucideCode,
  bypass: AlertTriangle,
};

const PERMISSION_META: Record<PermissionMode, { label: string; hint: string }> =
  {
    ask: {
      label: "每次询问",
      hint: "修改或敏感操作前询问确认。",
    },
    auto: {
      label: "自动处理",
      hint: "自动接受合理的文件编辑。",
    },
    bypass: {
      label: "跳过权限",
      hint: "尽量减少权限提示（谨慎使用）。",
    },
  };

type AvailableAgentRow = { agentType: string; description: string };

function normalizeFsPath(p: string): string {
  return p.replace(/\\/g, "/");
}

/** 文件选择列表：取路径最后一段作为与 FileTree 一致的图标名 */
function filePickerBasename(p: string): string {
  const n = normalizeFsPath(p);
  const parts = n.split("/").filter(Boolean);
  return parts[parts.length - 1] ?? n;
}

/** 与文件管理器列表相同的扩展名图标 + 圆角底（ui-ux-pro-max：一致视觉、清晰层级） */
function ComposerFilePickerRowIcon({
  path,
  isFile,
}: {
  path: string;
  isFile: boolean;
}) {
  const pen = usePencilPalette();
  const theme = useTheme();
  const iconColor = theme.palette.text.secondary;
  const name = filePickerBasename(path);
  return (
    <Box
      sx={{
        display: "inline-flex",
        alignItems: "center",
        justifyContent: "center",
        width: 28,
        height: 28,
        borderRadius: "8px",
        bgcolor: pen.iconChipBg,
        flexShrink: 0,
        border: `1px solid ${pen.borderSubtle}`,
        color: iconColor,
        lineHeight: 0,
        "& svg": { display: "block" },
      }}
      title={name}
    >
      {isFile ? (
        <LucideFile size={18} strokeWidth={2} />
      ) : (
        <LucideFolder size={18} strokeWidth={2} />
      )}
    </Box>
  );
}

function formatBytesShort(n: number): string {
  if (!Number.isFinite(n) || n < 0) return "—";
  if (n < 1024) return `${Math.round(n)} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / (1024 * 1024)).toFixed(1)} MB`;
}

/** Matches `glob_files` / `GlobMatch` from Tauri */
interface GlobMatchRow {
  path: string;
  is_file: boolean;
  size: number;
}

function assignRef<T>(ref: Ref<T> | undefined, value: T | null) {
  if (ref == null) return;
  if (typeof ref === "function") ref(value);
  else (ref as MutableRefObject<T | null>).current = value;
}

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
  /** Stop streaming (toolbar while generating; main session can queue the next message instead). */
  onCancel?: () => void;
  /** Background Agent tasks for this session (teammate follow-up routing). */
  backgroundTasks?: BackgroundAgentTask[];
  /** When set, sends use `inputTarget: bg:<taskId>` instead of the main session. */
  followUpTaskId?: string | null;
  onFollowUpTaskIdChange?: (taskId: string | null) => void;
  /** When true, input stays enabled during main-session streaming (message queue + bg follow-up). */
  allowInputWhileStreaming?: boolean;
  /** Main-session FIFO queue rows (while a turn is streaming). */
  queuedMainMessages?: Array<{
    id: string;
    previewText: string;
    /** Full merged text for tooltip */
    fullText?: string;
  }>;
  /** Clear all messages waiting in the main-session FIFO queue (while streaming). */
  onClearQueuedMessages?: () => void;
  onRemoveQueuedAt?: (index: number) => void;
  onMoveQueuedUp?: (index: number) => void;
  onEditQueuedAt?: (index: number) => void;
  /** Cancel a pending/running background Agent task (Rust `cancel_background_agent_task`). */
  onCancelBackgroundTask?: (taskId: string) => void;
  /** Open sidechain transcript drawer (`load_background_agent_transcript`). */
  onOpenBackgroundTranscript?: (taskId: string) => void;
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
  backgroundTasks = [],
  followUpTaskId = null,
  onFollowUpTaskIdChange,
  allowInputWhileStreaming = false,
  queuedMainMessages = [],
  onClearQueuedMessages,
  onRemoveQueuedAt,
  onMoveQueuedUp,
  onEditQueuedAt,
  onCancelBackgroundTask,
  onOpenBackgroundTranscript,
}: ChatComposerProps) {
  const theme = useTheme();
  const pen = usePencilPalette();
  const accent = theme.palette.primary.main;
  const paper = theme.palette.background.paper;
  const def = theme.palette.background.default;
  const ink = theme.palette.text.primary;
  const mut = theme.palette.text.secondary;
  const warningMain = theme.palette.warning.main;
  const errorMain = theme.palette.error.main;
  const errorDark = theme.palette.error.dark;
  const isDark = theme.palette.mode === "dark";
  /** 工具条 / 底栏主标签：统一字号、字重、行高（与分支 Select 一致 13px / 600） */
  const composerLabelText = {
    fontSize: 13,
    fontWeight: 600,
    lineHeight: 1.25,
  } as const;
  /** Divider 下工具栏：与 IconButton 一致，避免 32 / 36 混用 */
  const COMPOSER_TOOLBAR_CONTROL_PX = 36;
  /** 首行 Agent / 附件 Chip 统一高度 */
  const COMPOSER_INLINE_CHIP_PX = 28;
  /** 与 `--composer-fs` / `--composer-lh` 一致，用于首行与 Chip 垂直对齐 */
  const COMPOSER_FS_PX = 15;
  const COMPOSER_LH = 1.55;
  const COMPOSER_LINE_BOX_PX = COMPOSER_FS_PX * COMPOSER_LH;
  /** 使 textarea 首行与 28px Chip 垂直居中对齐（flex-start 时补偿行高差） */
  const COMPOSER_TEXTAREA_PAD_TOP_WITH_CHIPS =
    (COMPOSER_INLINE_CHIP_PX - COMPOSER_LINE_BOX_PX) / 2;
  /** Hairline border / shadow tint — theme-aware */
  const edge = (a: number) =>
    alpha(isDark ? theme.palette.common.white : theme.palette.common.black, a);
  /** Input card — closer to solid paper so the typing area reads lighter */
  const composerBg = alpha(paper, isDark ? 0.97 : 0.99);
  const setSettingsOpen = useUiStore((s) => s.setSettingsOpen);
  const setSettingsTabIndex = useUiStore((s) => s.setSettingsTabIndex);
  const setRightPanelMode = useUiStore((s) => s.setRightPanelMode);
  const {
    permissionMode,
    setPermissionMode,
    composerAgentType,
    setComposerAgentType,
    composerAttachedPaths,
    addComposerAttachedPath,
    popComposerAttachedPath,
    useWorktree,
    setUseWorktree,
    environment,
    setEnvironment,
    selectedBranchByRoot,
    setBranchForRoot,
  } = useChatComposerStore();

  const [plusAnchor, setPlusAnchor] = useState<null | HTMLElement>(null);
  const [permissionAnchor, setPermissionAnchor] = useState<null | HTMLElement>(
    null,
  );
  const [envAnchor, setEnvAnchor] = useState<null | HTMLElement>(null);
  const [gitInfo, setGitInfo] = useState<GitWorkspaceInfo | null>(null);
  const [availableAgents, setAvailableAgents] = useState<AvailableAgentRow[]>(
    [],
  );
  const [queuedPanelExpanded, setQueuedPanelExpanded] = useState(true);

  useEffect(() => {
    if (queuedMainMessages.length > 0) setQueuedPanelExpanded(true);
  }, [queuedMainMessages.length]);

  useEffect(() => {
    let cancelled = false;
    invoke<AvailableAgentRow[]>("list_available_agents")
      .then((rows) => {
        if (!cancelled) setAvailableAgents(rows);
      })
      .catch(() => {
        if (!cancelled) setAvailableAgents([]);
      });
    return () => {
      cancelled = true;
    };
  }, []);

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

  const selectedAgentDescription = useMemo(() => {
    const row = availableAgents.find((a) => a.agentType === composerAgentType);
    return row?.description ?? "";
  }, [availableAgents, composerAgentType]);

  const textareaRef = useRef<HTMLTextAreaElement | null>(null);

  /** `/`：选择 Agent（整段输入仅为 `/` 或 `/query`） */
  const slashParse = useMemo(() => {
    const t = input;
    if (!/^\/[^\s]*$/u.test(t)) return { active: false as const, query: "" };
    return { active: true as const, query: t.slice(1) };
  }, [input]);

  /** `@`：仅工作区根目录下一层文件/文件夹（整段输入仅为 `@` 或 `@query`） */
  const fileParse = useMemo(() => {
    const t = input;
    if (!/^@[^\s]*$/u.test(t)) return { active: false as const, query: "" };
    return { active: true as const, query: t.slice(1) };
  }, [input]);

  const filteredAtAgents = useMemo(() => {
    if (!slashParse.active) return [];
    const q = slashParse.query.toLowerCase();
    return availableAgents.filter((a) => {
      const id = a.agentType.toLowerCase();
      return !q || id.startsWith(q) || id.includes(q);
    });
  }, [availableAgents, slashParse]);

  const slashFilterKey = useMemo(
    () => filteredAtAgents.map((a) => a.agentType).join("\u0001"),
    [filteredAtAgents],
  );

  const [slashHighlightIndex, setSlashHighlightIndex] = useState(0);
  const slashHighlightIndexRef = useRef(0);
  const slashListRef = useRef<HTMLUListElement>(null);
  /** User clicked outside the / picker; hide until input changes or textarea refocuses. */
  const [slashPickerDismissed, setSlashPickerDismissed] = useState(false);

  const [fileGlobMatches, setFileGlobMatches] = useState<GlobMatchRow[]>([]);
  const [fileGlobLoading, setFileGlobLoading] = useState(false);
  const [fileHighlightIndex, setFileHighlightIndex] = useState(0);
  const fileHighlightIndexRef = useRef(0);
  const fileListRef = useRef<HTMLUListElement>(null);
  const [filePickerDismissed, setFilePickerDismissed] = useState(false);

  useEffect(() => {
    setSlashPickerDismissed(false);
    setFilePickerDismissed(false);
  }, [input]);

  useEffect(() => {
    slashHighlightIndexRef.current = 0;
    setSlashHighlightIndex(0);
  }, [slashFilterKey]);

  useEffect(() => {
    if (!fileParse.active || needsWorkspacePath || !workspacePath.trim()) {
      setFileGlobMatches([]);
      return;
    }
    let cancelled = false;
    setFileGlobLoading(true);
    invoke<{
      entries: Array<{
        name: string;
        path: string;
        is_directory: boolean;
        size?: number | null;
      }>;
    }>("list_directory", { path: workspacePath })
      .then((res) => {
        if (cancelled) return;
        const list: GlobMatchRow[] = (res.entries ?? []).map((e) => ({
          path: e.name,
          is_file: !e.is_directory,
          size: typeof e.size === "number" ? e.size : 0,
        }));
        list.sort((a, b) => {
          if (a.is_file !== b.is_file) return a.is_file ? 1 : -1;
          return normalizeFsPath(a.path).localeCompare(normalizeFsPath(b.path));
        });
        setFileGlobMatches(list);
      })
      .catch(() => {
        if (!cancelled) setFileGlobMatches([]);
      })
      .finally(() => {
        if (!cancelled) setFileGlobLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [fileParse.active, needsWorkspacePath, workspacePath]);

  const filteredFilePaths = useMemo(() => {
    if (!fileParse.active) return [];
    const q = fileParse.query.toLowerCase().trim();
    let rows = fileGlobMatches;
    if (q) {
      rows = rows.filter((m) => {
        const name = normalizeFsPath(m.path).toLowerCase();
        return name.includes(q) || name.startsWith(q);
      });
    }
    return rows.slice(0, 200);
  }, [fileParse, fileGlobMatches]);

  const fileFilterKey = useMemo(
    () =>
      filteredFilePaths
        .map((m) => `${m.is_file ? "f" : "d"}:${m.path}`)
        .join("\u0001"),
    [filteredFilePaths],
  );

  useEffect(() => {
    fileHighlightIndexRef.current = 0;
    setFileHighlightIndex(0);
  }, [fileFilterKey]);

  useEffect(() => {
    if (!slashParse.active || filteredAtAgents.length === 0) return;
    const el = slashListRef.current?.querySelector(
      `[data-slash-index="${slashHighlightIndex}"]`,
    );
    el?.scrollIntoView({ block: "nearest", behavior: "smooth" });
  }, [
    slashHighlightIndex,
    slashParse.active,
    filteredAtAgents.length,
    slashFilterKey,
  ]);

  useEffect(() => {
    if (!fileParse.active || filteredFilePaths.length === 0) return;
    const el = fileListRef.current?.querySelector(
      `[data-file-index="${fileHighlightIndex}"]`,
    );
    el?.scrollIntoView({ block: "nearest", behavior: "smooth" });
  }, [
    fileHighlightIndex,
    fileParse.active,
    filteredFilePaths.length,
    fileFilterKey,
  ]);

  const pickAtAgent = useCallback(
    (agentType: string) => {
      setComposerAgentType(agentType);
      onInputChange("");
    },
    [setComposerAgentType, onInputChange],
  );

  const pickFilePath = useCallback(
    (relPath: string) => {
      const safe = normalizeFsPath(relPath).replace(/^\//u, "");
      if (!safe) return;
      addComposerAttachedPath(safe);
      onInputChange("");
    },
    [addComposerAttachedPath, onInputChange],
  );

  const mergedTextareaRef = useCallback(
    (el: HTMLTextAreaElement | null) => {
      textareaRef.current = el;
      assignRef(inputRef, el);
    },
    [inputRef],
  );

  const handleComposerKeyDown = useCallback(
    (e: KeyboardEvent<HTMLTextAreaElement>) => {
      const ne = e.nativeEvent;
      if (ne.isComposing || ne.keyCode === 229) {
        onKeyDown(e);
        return;
      }
      /* 输入框无内容时退格：先移除末尾附件 Chip，再清除 Agent */
      if (
        (e.key === "Backspace" || e.key === "Delete") &&
        input.trim() === ""
      ) {
        if (composerAttachedPaths.length > 0) {
          e.preventDefault();
          popComposerAttachedPath();
          return;
        }
        if (composerAgentType !== "auto") {
          e.preventDefault();
          setComposerAgentType("auto");
          return;
        }
      }
      if (fileParse.active) {
        if (e.key === "Escape") {
          onInputChange("");
          e.preventDefault();
          return;
        }
        if (filteredFilePaths.length > 0 && !fileGlobLoading) {
          if (e.key === "ArrowDown") {
            e.preventDefault();
            setFileHighlightIndex((i) => {
              const next = (i + 1) % filteredFilePaths.length;
              fileHighlightIndexRef.current = next;
              return next;
            });
            return;
          }
          if (e.key === "ArrowUp") {
            e.preventDefault();
            setFileHighlightIndex((i) => {
              const next =
                (i - 1 + filteredFilePaths.length) % filteredFilePaths.length;
              fileHighlightIndexRef.current = next;
              return next;
            });
            return;
          }
          if (e.key === "Enter" && !e.shiftKey) {
            e.preventDefault();
            const idx = fileHighlightIndexRef.current;
            const row = filteredFilePaths[idx] ?? filteredFilePaths[0];
            if (row) pickFilePath(row.path);
            return;
          }
          if (e.key === "Tab" && !e.shiftKey) {
            e.preventDefault();
            const idx = fileHighlightIndexRef.current;
            const row = filteredFilePaths[idx] ?? filteredFilePaths[0];
            if (row) pickFilePath(row.path);
            return;
          }
        }
      }
      if (slashParse.active) {
        if (e.key === "Escape") {
          onInputChange("");
          e.preventDefault();
          return;
        }
        if (filteredAtAgents.length > 0) {
          if (e.key === "ArrowDown") {
            e.preventDefault();
            setSlashHighlightIndex((i) => {
              const next = (i + 1) % filteredAtAgents.length;
              slashHighlightIndexRef.current = next;
              return next;
            });
            return;
          }
          if (e.key === "ArrowUp") {
            e.preventDefault();
            setSlashHighlightIndex((i) => {
              const next =
                (i - 1 + filteredAtAgents.length) % filteredAtAgents.length;
              slashHighlightIndexRef.current = next;
              return next;
            });
            return;
          }
          if (e.key === "Enter" && !e.shiftKey) {
            e.preventDefault();
            const idx = slashHighlightIndexRef.current;
            const pick = filteredAtAgents[idx] ?? filteredAtAgents[0];
            if (pick) pickAtAgent(pick.agentType);
            return;
          }
          if (e.key === "Tab" && !e.shiftKey) {
            e.preventDefault();
            const idx = slashHighlightIndexRef.current;
            const pick = filteredAtAgents[idx] ?? filteredAtAgents[0];
            if (pick) pickAtAgent(pick.agentType);
            return;
          }
        }
      }
      onKeyDown(e);
    },
    [
      slashParse.active,
      fileParse.active,
      composerAgentType,
      composerAttachedPaths,
      filteredAtAgents,
      filteredFilePaths,
      fileGlobLoading,
      input,
      onInputChange,
      onKeyDown,
      pickAtAgent,
      pickFilePath,
      popComposerAttachedPath,
      setComposerAgentType,
    ],
  );

  const pathLabel = needsWorkspacePath
    ? "选择工作目录"
    : gitInfo?.displayPath
      ? shortRepoLabel(gitInfo.displayPath)
      : shortRepoLabel(workspacePath);

  const placeholder = !sessionId
    ? "Select a session"
    : needsWorkspacePath
      ? "请先选择工作目录后再发送消息…"
      : followUpTaskId
        ? "追加说明将进入该后台 Agent 的下一轮工具循环…"
        : "输入 / 选择 Agent；输入 @ 从当前工作目录选择…";

  /** 允许排队时：连接中 / 流式中均可继续输入；否则与旧行为一致（等待响应或生成时禁用）。 */
  const inputDisabled =
    !sessionId || (!allowInputWhileStreaming && (isConnecting || isStreaming));

  const showSlashPopover =
    slashParse.active &&
    !slashPickerDismissed &&
    !inputDisabled &&
    availableAgents.length > 0;

  const showFilePopover =
    fileParse.active &&
    !filePickerDismissed &&
    !inputDisabled &&
    !needsWorkspacePath &&
    Boolean(workspacePath.trim());

  const showComposerAgentChip =
    composerAgentType !== "general-purpose" && composerAgentType !== "auto";
  const hasInlineComposerChips =
    showComposerAgentChip || composerAttachedPaths.length > 0;

  const showBgRouting =
    Boolean(sessionId) &&
    !needsWorkspacePath &&
    backgroundTasks.length > 0 &&
    typeof onFollowUpTaskIdChange === "function";

  return (
    <Stack spacing={0.75}>
      {queuedMainMessages.length > 0 ? (
        <Box
          sx={{
            position: "relative",
            borderRadius: 3,
            overflow: "hidden",
            /* ui-ux-pro-max：与主输入框 Paper 同层级 — 半透明底、细边框、轻阴影 */
            bgcolor: alpha(paper, isDark ? 0.55 : 0.88),
            backdropFilter: "blur(10px)",
            WebkitBackdropFilter: "blur(10px)",
            border: `1px solid ${pen.borderSubtle}`,
            boxShadow: `
              0 1px 2px ${edge(0.06)},
              0 6px 20px ${alpha(accent, 0.07)},
              inset 0 1px 0 ${edge(0.08)}
            `,
            transition: "box-shadow 0.2s ease, border-color 0.2s ease",
            "@media (prefers-reduced-motion: reduce)": {
              transition: "none",
            },
          }}
        >
          {/* 主色强调条：队列锚点，与 File Manager 行选中态同系 */}
          <Box
            aria-hidden
            sx={{
              position: "absolute",
              left: 0,
              top: 0,
              bottom: 0,
              width: 3,
              background: `linear-gradient(180deg, ${alpha(accent, 0.95)} 0%, ${alpha(accent, 0.35)} 100%)`,
              borderRadius: "0 4px 4px 0",
            }}
          />
          <Stack sx={{ pl: 1.25 }}>
            <Stack
              direction="row"
              alignItems="center"
              spacing={1}
              onClick={() => setQueuedPanelExpanded((e) => !e)}
              sx={{
                px: 1.25,
                py: 1,
                cursor: "pointer",
                userSelect: "none",
                bgcolor: pen.toolbarSurface,
                borderBottom: `1px solid ${pen.borderSubtle}`,
                transition: "background-color 0.15s ease",
                "&:hover": {
                  bgcolor: isDark ? pen.rowHoverDir : pen.rowHover,
                },
              }}
            >
              <ExpandMore
                sx={{
                  fontSize: 22,
                  color: pen.toolbarIconAccent,
                  transform: queuedPanelExpanded
                    ? "rotate(0deg)"
                    : "rotate(-90deg)",
                  transition: theme.transitions.create("transform", {
                    duration: theme.transitions.duration.shorter,
                  }),
                }}
              />
              <HourglassEmpty
                sx={{
                  fontSize: 20,
                  color: accent,
                  opacity: 0.9,
                }}
              />
              <Typography
                variant="caption"
                sx={{
                  fontWeight: 700,
                  letterSpacing: "0.02em",
                  color: pen.textHeader,
                  fontSize: "0.75rem",
                }}
              >
                待发送队列
              </Typography>
              <Chip
                size="small"
                label={queuedMainMessages.length}
                sx={{
                  height: 22,
                  minWidth: 28,
                  fontSize: "0.7rem",
                  fontWeight: 700,
                  bgcolor: alpha(accent, isDark ? 0.18 : 0.12),
                  color: accent,
                  border: `1px solid ${alpha(accent, 0.28)}`,
                  "& .MuiChip-label": { px: 0.75 },
                }}
              />
              <Box sx={{ flex: 1, minWidth: 0 }} />
              {onClearQueuedMessages && queuedMainMessages.length > 1 ? (
                <Tooltip title="清空全部">
                  <Button
                    size="small"
                    variant="text"
                    onClick={(e) => {
                      e.stopPropagation();
                      onClearQueuedMessages();
                    }}
                    sx={{
                      minWidth: 0,
                      px: 1,
                      py: 0.35,
                      fontSize: "0.7rem",
                      fontWeight: 600,
                      color: pen.toolbarIconAccent,
                      borderRadius: 2,
                      "&:hover": {
                        bgcolor: pen.toolbarIconHoverBg,
                      },
                    }}
                  >
                    清空全部
                  </Button>
                </Tooltip>
              ) : null}
            </Stack>
            <Collapse in={queuedPanelExpanded}>
              <Stack
                divider={
                  <Divider flexItem sx={{ borderColor: pen.borderSubtle }} />
                }
              >
                {queuedMainMessages.map((row, index) => (
                  <Stack
                    key={row.id}
                    direction="row"
                    alignItems="center"
                    spacing={1.25}
                    sx={{
                      px: 1.25,
                      py: 1,
                      pr: 0.75,
                      transition: "background-color 0.15s ease",
                      "&:hover": { bgcolor: pen.rowHover },
                    }}
                  >
                    {/* 序号：替代空心圆，层级更清晰 */}
                    <Box
                      sx={{
                        width: 24,
                        height: 24,
                        borderRadius: "50%",
                        flexShrink: 0,
                        display: "flex",
                        alignItems: "center",
                        justifyContent: "center",
                        fontSize: "0.7rem",
                        fontWeight: 800,
                        color: accent,
                        bgcolor: alpha(accent, isDark ? 0.14 : 0.1),
                        border: `1px solid ${alpha(accent, 0.22)}`,
                      }}
                    >
                      {index + 1}
                    </Box>
                    <Typography
                      variant="body2"
                      color="text.primary"
                      title={row.fullText ?? row.previewText}
                      sx={{
                        flex: 1,
                        minWidth: 0,
                        overflow: "hidden",
                        textOverflow: "ellipsis",
                        whiteSpace: "nowrap",
                        fontSize: "0.8125rem",
                        lineHeight: 1.45,
                        fontWeight: 500,
                      }}
                    >
                      {row.previewText}
                    </Typography>
                    {/* 操作区：成组工具条，与 composer 工具栏一致 */}
                    <Stack
                      direction="row"
                      alignItems="center"
                      spacing={0}
                      sx={{
                        flexShrink: 0,
                        borderRadius: 2,
                        bgcolor: alpha(ink, isDark ? 0.08 : 0.04),
                        border: `1px solid ${pen.borderSubtle}`,
                        p: 0.25,
                      }}
                    >
                      {onEditQueuedAt ? (
                        <Tooltip title="编辑并移回输入框">
                          <IconButton
                            size="small"
                            aria-label="编辑排队消息"
                            onClick={(e) => {
                              e.stopPropagation();
                              onEditQueuedAt(index);
                            }}
                            sx={{
                              p: 0.45,
                              color: pen.toolbarIcon,
                              borderRadius: 1.5,
                              "&:hover": {
                                bgcolor: pen.toolbarIconHoverBg,
                                color: pen.toolbarIconAccent,
                              },
                            }}
                          >
                            <Edit sx={{ fontSize: 17 }} />
                          </IconButton>
                        </Tooltip>
                      ) : null}
                      {onMoveQueuedUp ? (
                        <Tooltip title="上移">
                          <span>
                            <IconButton
                              size="small"
                              aria-label="上移"
                              disabled={index === 0}
                              onClick={(e) => {
                                e.stopPropagation();
                                onMoveQueuedUp(index);
                              }}
                              sx={{
                                p: 0.45,
                                color: pen.toolbarIcon,
                                borderRadius: 1.5,
                                "&:hover": {
                                  bgcolor: pen.toolbarIconHoverBg,
                                  color: pen.toolbarIconAccent,
                                },
                                "&.Mui-disabled": {
                                  opacity: 0.35,
                                },
                              }}
                            >
                              <ArrowUpward sx={{ fontSize: 17 }} />
                            </IconButton>
                          </span>
                        </Tooltip>
                      ) : null}
                      {onRemoveQueuedAt ? (
                        <Tooltip title="从队列移除">
                          <IconButton
                            size="small"
                            aria-label="从队列移除"
                            onClick={(e) => {
                              e.stopPropagation();
                              onRemoveQueuedAt(index);
                            }}
                            sx={{
                              p: 0.45,
                              color: pen.toolbarIcon,
                              borderRadius: 1.5,
                              "&:hover": {
                                bgcolor: alpha(errorMain, isDark ? 0.16 : 0.1),
                                color: errorMain,
                              },
                            }}
                          >
                            <DeleteOutline sx={{ fontSize: 17 }} />
                          </IconButton>
                        </Tooltip>
                      ) : null}
                    </Stack>
                  </Stack>
                ))}
              </Stack>
            </Collapse>
          </Stack>
        </Box>
      ) : null}
      {showBgRouting ? (
        <Stack
          direction="row"
          alignItems="center"
          spacing={0.75}
          flexWrap="wrap"
          useFlexGap
        >
          <Typography variant="caption" color="text.secondary" sx={{ mr: 0.5 }}>
            发送到
          </Typography>
          <Chip
            size="small"
            icon={<ForumOutlined sx={{ fontSize: 16 }} />}
            label="主会话"
            color={followUpTaskId ? "default" : "primary"}
            variant={followUpTaskId ? "outlined" : "filled"}
            onClick={() => onFollowUpTaskIdChange?.(null)}
            sx={{ fontWeight: followUpTaskId ? 400 : 600 }}
          />
          {backgroundTasks.map((t) => {
            const ok = canSendFollowUpToTask(t.status);
            const selected = followUpTaskId === t.task_id;
            return (
              <Stack
                key={t.task_id}
                direction="row"
                alignItems="center"
                spacing={0.25}
              >
                <Tooltip
                  title={`${t.agent_type} · ${t.description.slice(0, 200)}${t.description.length > 200 ? "…" : ""}`}
                >
                  <span>
                    <Chip
                      size="small"
                      icon={<SmartToy sx={{ fontSize: 16 }} />}
                      label={`${t.agent_type}: ${shortBgTaskLabel(t, 28)}`}
                      color={selected ? "secondary" : "default"}
                      variant={selected ? "filled" : "outlined"}
                      disabled={!ok}
                      onClick={() => ok && onFollowUpTaskIdChange?.(t.task_id)}
                    />
                  </span>
                </Tooltip>
                {onOpenBackgroundTranscript ? (
                  <Tooltip title="队友记录">
                    <IconButton
                      size="small"
                      aria-label="Background teammate transcript"
                      onClick={(e) => {
                        e.stopPropagation();
                        onOpenBackgroundTranscript(t.task_id);
                      }}
                      sx={{ p: 0.25 }}
                    >
                      <ArticleOutlined sx={{ fontSize: 16 }} />
                    </IconButton>
                  </Tooltip>
                ) : null}
                {ok && onCancelBackgroundTask ? (
                  <Tooltip title="取消后台任务">
                    <IconButton
                      size="small"
                      aria-label="Cancel background task"
                      onClick={(e) => {
                        e.stopPropagation();
                        onCancelBackgroundTask(t.task_id);
                      }}
                      sx={{ p: 0.25 }}
                    >
                      <Close sx={{ fontSize: 16 }} />
                    </IconButton>
                  </Tooltip>
                ) : null}
              </Stack>
            );
          })}
        </Stack>
      ) : null}
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
          transition:
            "box-shadow 0.22s ease, border-color 0.22s ease, transform 0.22s ease",
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
          sx={{
            position: "relative",
            display: "flex",
            flexDirection: "row",
            alignItems: "flex-start",
            gap: 0.75,
            px: 1.75,
            py: 1.15,
            /* 与下方 textarea 首行一致，用于 Agent Chip 与光标垂直对齐 */
            "--composer-fs": `${COMPOSER_FS_PX}px`,
            "--composer-lh": COMPOSER_LH,
            "--composer-chip-h": `${COMPOSER_INLINE_CHIP_PX}px`,
          }}
        >
          {showComposerAgentChip ? (
            <Tooltip
              placement="top"
              enterDelay={250}
              title={
                selectedAgentDescription ? (
                  <Box sx={{ maxWidth: 320 }}>
                    <Typography
                      variant="caption"
                      component="div"
                      fontWeight={700}
                      display="block"
                      sx={{ mb: 0.5 }}
                    >
                      /{composerAgentType}
                    </Typography>
                    <Typography
                      variant="caption"
                      component="div"
                      sx={{ opacity: 0.92, lineHeight: 1.45 }}
                    >
                      {selectedAgentDescription}
                    </Typography>
                  </Box>
                ) : (
                  `/${composerAgentType}`
                )
              }
            >
              <Box
                component="span"
                sx={{
                  display: "inline-flex",
                  alignItems: "center",
                  alignSelf: "flex-start",
                  flexShrink: 0,
                  height: "var(--composer-chip-h)",
                  fontSize: "var(--composer-fs)",
                  lineHeight: "var(--composer-lh)",
                }}
              >
                <Chip
                  size="small"
                  variant="outlined"
                  icon={<SmartToy sx={{ fontSize: 16, color: accent }} />}
                  label={`/${composerAgentType}`}
                  sx={{
                    flexShrink: 0,
                    height: "var(--composer-chip-h)",
                    maxHeight: "var(--composer-chip-h)",
                    fontWeight: 700,
                    bgcolor: alpha(accent, isDark ? 0.16 : 0.1),
                    borderColor: alpha(accent, 0.58),
                    color: ink,
                    maxWidth: { xs: 140, sm: 220 },
                    boxShadow: `0 1px 2px ${edge(0.12)}`,
                    "& .MuiChip-label": {
                      overflow: "hidden",
                      textOverflow: "ellipsis",
                    },
                    "& .MuiChip-icon": {
                      marginTop: 0,
                      marginBottom: 0,
                    },
                  }}
                />
              </Box>
            </Tooltip>
          ) : null}
          {composerAttachedPaths.length > 0 ? (
            <Box
              sx={{
                display: "flex",
                flexWrap: "wrap",
                alignItems: "center",
                alignContent: "flex-start",
                alignSelf: "flex-start",
                gap: 0.25,
                maxWidth: { xs: "100%", sm: 420 },
                minHeight: "var(--composer-chip-h)",
              }}
            >
              {composerAttachedPaths.map((p) => (
                <Tooltip key={p} title={p} placement="top">
                  <Chip
                    size="small"
                    variant="outlined"
                    icon={
                      <InsertDriveFile sx={{ fontSize: 16, color: accent }} />
                    }
                    label={`@${p}`}
                    sx={{
                      flexShrink: 0,
                      height: "var(--composer-chip-h)",
                      maxHeight: "var(--composer-chip-h)",
                      maxWidth: 200,
                      fontWeight: 600,
                      bgcolor: alpha(accent, isDark ? 0.16 : 0.1),
                      borderColor: alpha(accent, 0.58),
                      color: ink,
                      boxShadow: `0 1px 2px ${edge(0.12)}`,
                      "& .MuiChip-label": {
                        overflow: "hidden",
                        textOverflow: "ellipsis",
                      },
                      "& .MuiChip-icon": {
                        marginTop: 0,
                        marginBottom: 0,
                      },
                    }}
                  />
                </Tooltip>
              ))}
            </Box>
          ) : null}
          <Box
            component="textarea"
            ref={mergedTextareaRef}
            value={input}
            onChange={(e) => onInputChange(e.target.value)}
            onFocus={() => {
              if (slashParse.active) setSlashPickerDismissed(false);
              if (fileParse.active) setFilePickerDismissed(false);
            }}
            onKeyDown={handleComposerKeyDown}
            disabled={inputDisabled}
            placeholder={placeholder}
            rows={2}
            aria-label="消息输入"
            aria-autocomplete="list"
            aria-expanded={
              (showSlashPopover && filteredAtAgents.length > 0) ||
              (showFilePopover &&
                (fileGlobLoading || filteredFilePaths.length > 0))
            }
            sx={{
              flex: 1,
              minWidth: 0,
              width: 0,
              boxSizing: "border-box",
              border: "none",
              resize: "none",
              minHeight: 56,
              maxHeight: 280,
              px: 0,
              py: 0,
              paddingTop: hasInlineComposerChips
                ? `${COMPOSER_TEXTAREA_PAD_TOP_WITH_CHIPS}px`
                : 0,
              fontSize: "var(--composer-fs)",
              fontFamily: "inherit",
              lineHeight: "var(--composer-lh)",
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
          <Popover
            open={showSlashPopover}
            anchorEl={textareaRef.current}
            anchorOrigin={{ vertical: "bottom", horizontal: "left" }}
            transformOrigin={{ vertical: "top", horizontal: "left" }}
            disableAutoFocus
            disableEnforceFocus
            onClose={(_, reason) => {
              if (reason === "backdropClick") {
                setSlashPickerDismissed(true);
              }
            }}
            slotProps={{
              paper: {
                sx: {
                  mt: 0.5,
                  maxHeight: 280,
                  width: 320,
                  borderRadius: 2,
                  overflow: "hidden",
                },
              },
            }}
          >
            <List
              ref={slashListRef}
              dense
              sx={{ py: 0, maxHeight: 260, overflow: "auto" }}
            >
              {filteredAtAgents.length === 0 ? (
                <ListItemButton disabled>
                  <ListItemText
                    primary="无匹配 Agent"
                    secondary="继续输入或按 Esc 取消"
                  />
                </ListItemButton>
              ) : (
                filteredAtAgents.map((a, i) => (
                  <Tooltip
                    key={a.agentType}
                    title={a.description}
                    placement="right"
                    enterDelay={200}
                  >
                    <ListItemButton
                      data-slash-index={i}
                      selected={i === slashHighlightIndex}
                      onClick={() => pickAtAgent(a.agentType)}
                    >
                      <ListItemIcon sx={{ minWidth: 36 }}>
                        <SmartToy fontSize="small" />
                      </ListItemIcon>
                      <ListItemText primary={`/${a.agentType}`} />
                    </ListItemButton>
                  </Tooltip>
                ))
              )}
            </List>
          </Popover>
          <Popover
            open={showFilePopover}
            anchorEl={textareaRef.current}
            anchorOrigin={{ vertical: "bottom", horizontal: "left" }}
            transformOrigin={{ vertical: "top", horizontal: "left" }}
            disableAutoFocus
            disableEnforceFocus
            onClose={(_, reason) => {
              if (reason === "backdropClick") {
                setFilePickerDismissed(true);
              }
            }}
            slotProps={{
              paper: {
                sx: {
                  mt: 0.75,
                  maxHeight: 300,
                  width: 380,
                  borderRadius: 2.5,
                  overflow: "hidden",
                  bgcolor: alpha(paper, isDark ? 0.98 : 1),
                  border: `1px solid ${pen.borderSubtle}`,
                  boxShadow: `
                    0 4px 6px -1px ${edge(0.08)},
                    0 12px 28px -4px ${alpha(accent, 0.12)}
                  `,
                  backdropFilter: "blur(12px)",
                  WebkitBackdropFilter: "blur(12px)",
                },
              },
            }}
          >
            <Box
              sx={{
                px: 1,
                py: 0.65,
                borderBottom: `1px solid ${pen.borderSubtle}`,
                bgcolor: alpha(def, isDark ? 0.5 : 0.65),
              }}
            >
              <Typography
                variant="caption"
                sx={{
                  fontWeight: 700,
                  letterSpacing: "0.04em",
                  textTransform: "uppercase",
                  color: pen.textHeader,
                  fontSize: 10,
                }}
              >
                工作区文件
              </Typography>
              <Typography
                variant="caption"
                component="div"
                sx={{
                  mt: 0.15,
                  color: pen.textPath,
                  fontSize: 11,
                  lineHeight: 1.35,
                }}
              >
                根目录下一层 · 与侧栏文件列表相同图标
              </Typography>
            </Box>
            <List
              ref={fileListRef}
              dense
              sx={{
                py: 0,
                px: 0,
                maxHeight: 240,
                overflow: "auto",
                "& .MuiListItemButton-root": {
                  minHeight: 32,
                  py: 0.25,
                  px: 0.75,
                  borderRadius: 1.25,
                  mb: 0,
                  transition: "background-color 0.15s ease",
                  "@media (prefers-reduced-motion: reduce)": {
                    transition: "none",
                  },
                },
                "& .MuiListItemButton-root:hover": {
                  bgcolor: pen.rowHover,
                },
                "& .MuiListItemButton-root.Mui-selected": {
                  bgcolor: pen.rowSelected,
                },
                "& .MuiListItemButton-root.Mui-selected:hover": {
                  bgcolor: pen.rowSelected,
                },
              }}
            >
              {fileGlobLoading ? (
                <ListItemButton
                  disabled
                  sx={{ flexDirection: "column", py: 1.25 }}
                >
                  <CircularProgress
                    size={22}
                    sx={{ color: pen.loadingSpinner, mb: 0.75 }}
                  />
                  <ListItemText
                    primary="正在加载当前目录…"
                    secondary="仅显示工作区根目录下的文件与文件夹"
                    primaryTypographyProps={{
                      sx: { fontWeight: 600, color: pen.textFilename },
                    }}
                    secondaryTypographyProps={{
                      sx: { color: pen.textPath, fontSize: 11 },
                    }}
                  />
                </ListItemButton>
              ) : filteredFilePaths.length === 0 ? (
                <ListItemButton disabled sx={{ py: 1.5, px: 1 }}>
                  <ListItemText
                    primary="当前目录下无匹配项"
                    secondary="继续输入或按 Esc 取消"
                    primaryTypographyProps={{
                      sx: { fontWeight: 600, color: pen.textFilename },
                    }}
                    secondaryTypographyProps={{
                      sx: { color: pen.textPath, fontSize: 12 },
                    }}
                  />
                </ListItemButton>
              ) : (
                filteredFilePaths.map((row, i) => (
                  <ListItemButton
                    key={`${row.is_file ? "f" : "d"}:${row.path}`}
                    data-file-index={i}
                    selected={i === fileHighlightIndex}
                    onClick={() => pickFilePath(row.path)}
                  >
                    <ListItemIcon sx={{ minWidth: 34, mr: 0.25, py: 0 }}>
                      <ComposerFilePickerRowIcon
                        path={row.path}
                        isFile={row.is_file}
                      />
                    </ListItemIcon>
                    <ListItemText
                      primary={normalizeFsPath(row.path)}
                      secondary={
                        row.is_file ? formatBytesShort(row.size) : "文件夹"
                      }
                      sx={{ my: 0.5 }}
                      primaryTypographyProps={{
                        sx: {
                          fontFamily:
                            "ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace",
                          fontSize: 12,
                          fontWeight: 500,
                          color: pen.textFilename,
                          wordBreak: "break-all",
                          lineHeight: 1.35,
                        },
                      }}
                      secondaryTypographyProps={{
                        sx: {
                          fontSize: 11,
                          color: row.is_file ? pen.textSize : pen.textPath,
                          mt: 0.1,
                          lineHeight: 1.25,
                        },
                      }}
                    />
                  </ListItemButton>
                ))
              )}
            </List>
          </Popover>
        </Box>
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
            "--composer-toolbar-h": `${COMPOSER_TOOLBAR_CONTROL_PX}px`,
            background: isDark
              ? `linear-gradient(165deg, ${alpha(paper, 0.48)} 0%, ${alpha(def, 0.94)} 48%, ${alpha(def, 0.72)} 100%)`
              : `linear-gradient(165deg, ${alpha(paper, 0.72)} 0%, ${alpha(def, 0.97)} 48%, ${alpha(paper, 0.65)} 100%)`,
            borderTop: `1px solid ${edge(0.12)}`,
            boxShadow: `inset 0 1px 0 ${edge(0.06)}`,
          }}
        >
          <Tooltip title="更多功能即将推出">
            <IconButton
              size="small"
              aria-label="更多"
              aria-haspopup="menu"
              aria-expanded={Boolean(plusAnchor)}
              onClick={(e) => setPlusAnchor(e.currentTarget)}
              sx={{
                color: mut,
                width: "var(--composer-toolbar-h)",
                height: "var(--composer-toolbar-h)",
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
                  bgcolor: alpha(accent, 0.16),
                  color: accent,
                  borderColor: alpha(accent, 0.32),
                  boxShadow: `0 2px 10px ${alpha(accent, 0.18)}`,
                  transform: "translateY(-1px)",
                },
                "&:hover .MuiSvgIcon-root": {
                  color: accent,
                },
                "& .MuiSvgIcon-root": {
                  color: mut,
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
            slotProps={{ paper: { sx: { minWidth: 220, borderRadius: 2 } } }}
          >
            <MenuItem disabled>
              <ListItemText
                primary="敬请期待"
                secondary="附件等功能将陆续开放"
              />
            </MenuItem>
          </Menu>

          <Box sx={{ flex: 1, minWidth: 8 }} />

          <Button
            size="small"
            variant="text"
            color="inherit"
            onClick={(e) => setPermissionAnchor(e.currentTarget)}
            startIcon={
              <Box
                component="span"
                sx={{
                  display: "inline-flex",
                  alignItems: "center",
                  color: accent,
                  lineHeight: 0,
                  "& svg": { display: "block" },
                }}
              >
                {createElement(PERMISSION_ICON[permissionMode], {
                  size: 18,
                  strokeWidth: 2,
                  color: accent,
                })}
              </Box>
            }
            endIcon={
              <Box
                component="span"
                sx={{
                  display: "inline-flex",
                  alignItems: "center",
                  color: accent,
                  lineHeight: 0,
                  "& svg": { display: "block" },
                }}
              >
                <ChevronDown size={18} strokeWidth={2} color={accent} />
              </Box>
            }
            sx={{
              textTransform: "none",
              color: ink,
              ...composerLabelText,
              borderRadius: 2.5,
              px: 1,
              minHeight: "var(--composer-toolbar-h)",
              height: "var(--composer-toolbar-h)",
              maxWidth: 200,
              border: "1px solid transparent",
              bgcolor: "transparent",
              boxShadow: "none",
              transition:
                "background-color 0.2s ease, border-color 0.2s ease, box-shadow 0.2s ease",
              "@media (prefers-reduced-motion: reduce)": {
                transition: "none",
              },
              "&:hover": {
                bgcolor: alpha(accent, 0.14),
                borderColor: alpha(accent, 0.3),
                boxShadow: "none",
              },
            }}
          >
            <Typography
              variant="body2"
              noWrap
              component="span"
              sx={{ ...composerLabelText, color: "inherit" }}
            >
              {PERMISSION_META[permissionMode].label}
            </Typography>
          </Button>
          <Menu
            anchorEl={permissionAnchor}
            open={Boolean(permissionAnchor)}
            onClose={() => setPermissionAnchor(null)}
            slotProps={{ paper: { sx: { minWidth: 260, borderRadius: 2 } } }}
          >
            <Box sx={{ px: 2, py: 1, borderBottom: 1, borderColor: "divider" }}>
              <Tooltip
                title="工具与编辑的确认策略"
                placement="top"
                enterDelay={200}
              >
                <Typography
                  variant="subtitle2"
                  component="span"
                  sx={{
                    display: "inline-block",
                    cursor: "default",
                    ...composerLabelText,
                    color: ink,
                  }}
                >
                  权限模式
                </Typography>
              </Tooltip>
            </Box>
            {(Object.keys(PERMISSION_META) as PermissionMode[]).map((key) => (
              <Tooltip
                key={key}
                title={PERMISSION_META[key].hint}
                placement="left"
                enterDelay={200}
              >
                <MenuItem
                  selected={permissionMode === key}
                  onClick={() => {
                    setPermissionMode(key);
                    setPermissionAnchor(null);
                  }}
                >
                  <ListItemIcon
                    sx={{
                      minWidth: 40,
                      lineHeight: 0,
                      "& svg": { display: "block" },
                    }}
                  >
                    {createElement(PERMISSION_ICON[key], {
                      size: 20,
                      strokeWidth: 2,
                      color: accent,
                    })}
                  </ListItemIcon>
                  <ListItemText
                    primary={PERMISSION_META[key].label}
                    primaryTypographyProps={{
                      sx: { ...composerLabelText, color: ink },
                    }}
                  />
                </MenuItem>
              </Tooltip>
            ))}
          </Menu>

          <Stack
            direction="row"
            alignItems="center"
            spacing={0.75}
            sx={{ flexShrink: 0 }}
          >
            <ProviderSwitcher
              onOpenSettings={() => {
                setSettingsTabIndex(0);
                setSettingsOpen(true);
                setRightPanelMode("settings");
              }}
              triggerSx={{
                minHeight: "var(--composer-toolbar-h)",
                height: "var(--composer-toolbar-h)",
                maxWidth: { xs: 200, sm: 260 },
                px: 1,
                py: 0,
                borderRadius: 2.5,
                borderColor: edge(0.14),
                color: ink,
                ...composerLabelText,
                bgcolor: alpha(paper, isDark ? 0.45 : 0.88),
                boxShadow: `0 1px 2px ${edge(0.05)}, inset 0 1px 0 ${edge(0.06)}`,
                transition:
                  "border-color 0.2s ease, box-shadow 0.2s ease, transform 0.2s ease, background-color 0.2s ease",
                "@media (prefers-reduced-motion: reduce)": {
                  transition: "none",
                },
                "&:hover": {
                  borderColor: alpha(accent, 0.5),
                  bgcolor: isDark ? alpha(paper, 0.48) : alpha(accent, 0.12),
                  boxShadow: `0 2px 12px ${alpha(accent, 0.2)}, 0 0 0 1px ${alpha(accent, 0.22)}`,
                  transform: "translateY(-1px)",
                },
                "& .MuiChip-root": {
                  maxWidth: 100,
                },
              }}
            />
            {isStreaming && onCancel ? (
              <Tooltip title="停止生成">
                <IconButton
                  size="small"
                  onClick={onCancel}
                  sx={{
                    width: "var(--composer-toolbar-h)",
                    height: "var(--composer-toolbar-h)",
                    borderRadius: 2,
                    color: theme.palette.error.contrastText,
                    bgcolor: errorMain,
                    boxShadow: `0 2px 8px ${alpha(errorMain, 0.35)}`,
                    transition:
                      "background-color 0.2s ease, box-shadow 0.2s ease, transform 0.2s ease",
                    "@media (prefers-reduced-motion: reduce)": {
                      transition: "none",
                    },
                    "& .MuiSvgIcon-root": {
                      color: theme.palette.error.contrastText,
                    },
                    "&:hover": {
                      bgcolor: errorDark,
                      boxShadow: `0 4px 14px ${alpha(errorMain, 0.45)}`,
                      transform: "translateY(-1px)",
                    },
                    "&:focus-visible": {
                      outline: `2px solid ${alpha(errorMain, 0.65)}`,
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
                      color: theme.palette.action.disabled,
                      width: "var(--composer-toolbar-h)",
                      height: "var(--composer-toolbar-h)",
                      borderRadius: 2,
                      border: `1px dashed ${edge(0.18)}`,
                      bgcolor: alpha(paper, isDark ? 0.2 : 0.4),
                      "& .MuiSvgIcon-root": {
                        color: theme.palette.action.disabled,
                      },
                    }}
                  >
                    <Mic fontSize="small" />
                  </IconButton>
                </span>
              </Tooltip>
            )}
          </Stack>
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
          "--composer-footer-h": `${COMPOSER_TOOLBAR_CONTROL_PX}px`,
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
            color="inherit"
            startIcon={
              <Box
                component="span"
                sx={{
                  display: "inline-flex",
                  alignItems: "center",
                  justifyContent: "center",
                  color: needsWorkspacePath ? warningMain : accent,
                  lineHeight: 0,
                  "& svg": { display: "block" },
                }}
              >
                <LucideFolderOpen
                  size={18}
                  strokeWidth={2}
                  color={needsWorkspacePath ? warningMain : accent}
                />
              </Box>
            }
            onClick={onPickWorkspace}
            sx={{
              textTransform: "none",
              color: needsWorkspacePath ? warningMain : ink,
              ...composerLabelText,
              maxWidth: { xs: "100%", sm: 240 },
              borderRadius: 2.5,
              px: 1,
              py: 0,
              minHeight: "var(--composer-footer-h)",
              height: "var(--composer-footer-h)",
              bgcolor: needsWorkspacePath
                ? alpha(warningMain, 0.1)
                : "transparent",
              border: needsWorkspacePath
                ? `1px solid ${alpha(warningMain, 0.35)}`
                : "1px solid transparent",
              boxShadow: "none",
              transition:
                "background-color 0.2s ease, border-color 0.2s ease, box-shadow 0.2s ease",
              "@media (prefers-reduced-motion: reduce)": {
                transition: "none",
              },
              "&:hover": {
                bgcolor: needsWorkspacePath
                  ? alpha(warningMain, 0.22)
                  : alpha(accent, 0.12),
                borderColor: needsWorkspacePath
                  ? alpha(warningMain, 0.55)
                  : alpha(accent, 0.28),
              },
            }}
          >
            <Typography
              variant="body2"
              noWrap
              component="span"
              sx={{ ...composerLabelText, color: "inherit" }}
            >
              {pathLabel}
            </Typography>
          </Button>

          {gitInfo?.isGit && !needsWorkspacePath && (
            <Stack direction="row" alignItems="center" spacing={0.5}>
              <Box
                sx={{
                  display: "inline-flex",
                  color: mut,
                  lineHeight: 0,
                  "& svg": { display: "block" },
                }}
              >
                <GitBranch size={18} strokeWidth={2} />
              </Box>
              <FormControl size="small" sx={{ minWidth: 148 }}>
                <Select
                  value={branchValue || gitInfo.currentBranch}
                  displayEmpty
                  onChange={(e) => {
                    const b = String(e.target.value);
                    setBranchForRoot(rootKey, b);
                  }}
                  sx={{
                    minHeight: "var(--composer-footer-h)",
                    height: "var(--composer-footer-h)",
                    bgcolor: "transparent",
                    color: ink,
                    borderRadius: 2,
                    ...composerLabelText,
                    boxShadow: "none",
                    transition: "box-shadow 0.2s ease, border-color 0.2s ease",
                    "& .MuiSelect-icon": { color: mut },
                    "& .MuiSelect-select": {
                      display: "flex",
                      alignItems: "center",
                      py: 0,
                      minHeight: "var(--composer-footer-h)",
                      boxSizing: "border-box",
                    },
                    "& .MuiOutlinedInput-notchedOutline": {
                      borderColor: edge(0.14),
                    },
                    "&:hover .MuiOutlinedInput-notchedOutline": {
                      borderColor: alpha(accent, 0.48),
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
            <Typography
              variant="body2"
              sx={{
                ...composerLabelText,
                fontWeight: 500,
                color: mut,
              }}
            >
              非 Git 仓库
            </Typography>
          )}
        </Stack>

        <Stack
          direction="row"
          alignItems="center"
          spacing={1}
          flexWrap="wrap"
          sx={{
            flexShrink: 0,
            justifyContent: "flex-end",
            ml: { xs: 0, sm: "auto" },
          }}
        >
          <FormControlLabel
            control={
              <Checkbox
                size="small"
                checked={useWorktree}
                onChange={(_, v) => setUseWorktree(v)}
                sx={{
                  py: 0,
                  color: mut,
                  "&.Mui-checked": { color: accent },
                  "& .MuiSvgIcon-root": { fontSize: 20 },
                }}
              />
            }
            label={
              <Typography
                variant="body2"
                sx={{ ...composerLabelText, color: ink }}
              >
                worktree
              </Typography>
            }
            sx={{
              mr: 0,
              px: 0.5,
              py: 0,
              minHeight: "var(--composer-footer-h)",
              height: "var(--composer-footer-h)",
              borderRadius: 2,
              border: "1px solid transparent",
              bgcolor: "transparent",
              transition: "background-color 0.2s ease, border-color 0.2s ease",
              "& .MuiFormControlLabel-label": {
                ...composerLabelText,
                color: ink,
              },
              "&:hover": {
                bgcolor: alpha(accent, 0.12),
                borderColor: alpha(accent, 0.22),
              },
            }}
          />

          <Button
            size="small"
            variant="text"
            color="inherit"
            startIcon={
              <Box
                component="span"
                sx={{
                  display: "inline-flex",
                  alignItems: "center",
                  color: accent,
                  lineHeight: 0,
                  "& svg": { display: "block" },
                }}
              >
                {environment === "local" ? (
                  <Laptop size={18} strokeWidth={2} color={accent} />
                ) : (
                  <Globe2 size={18} strokeWidth={2} color={accent} />
                )}
              </Box>
            }
            endIcon={
              <Box
                component="span"
                sx={{
                  display: "inline-flex",
                  alignItems: "center",
                  color: accent,
                  lineHeight: 0,
                  "& svg": { display: "block" },
                }}
              >
                <ChevronDown size={18} strokeWidth={2} color={accent} />
              </Box>
            }
            onClick={(e) => setEnvAnchor(e.currentTarget)}
            sx={{
              textTransform: "none",
              color: ink,
              ...composerLabelText,
              borderRadius: 2.5,
              minHeight: "var(--composer-footer-h)",
              height: "var(--composer-footer-h)",
              px: 1,
              py: 0,
              border: "1px solid transparent",
              bgcolor: "transparent",
              boxShadow: "none",
              transition:
                "border-color 0.2s ease, box-shadow 0.2s ease, background-color 0.2s ease",
              "&:hover": {
                borderColor: alpha(accent, 0.32),
                bgcolor: alpha(accent, 0.12),
                boxShadow: "none",
              },
            }}
          >
            <Typography
              component="span"
              sx={{ ...composerLabelText, color: "inherit" }}
            >
              {environment === "local" ? "本地" : "远程"}
            </Typography>
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
              <ListItemIcon
                sx={{
                  minWidth: 40,
                  lineHeight: 0,
                  "& svg": { display: "block" },
                }}
              >
                <Laptop size={20} strokeWidth={2} color={accent} />
              </ListItemIcon>
              <ListItemText
                primary="本地"
                secondary="在本机运行工具与终端"
                primaryTypographyProps={{
                  sx: { ...composerLabelText, color: ink },
                }}
              />
            </MenuItem>
            <Divider />
            <MenuItem disabled>
              <ListItemIcon
                sx={{
                  minWidth: 40,
                  lineHeight: 0,
                  "& svg": { display: "block" },
                }}
              >
                <Plus size={20} strokeWidth={2} color={accent} />
              </ListItemIcon>
              <ListItemText
                primary="添加 SSH 连接"
                secondary="即将推出"
                primaryTypographyProps={{
                  sx: { ...composerLabelText, color: mut },
                }}
              />
            </MenuItem>
            <MenuItem disabled>
              <ListItemText
                primaryTypographyProps={{
                  variant: "caption",
                  color: "text.secondary",
                }}
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
              <ListItemIcon
                sx={{
                  minWidth: 40,
                  lineHeight: 0,
                  "& svg": { display: "block" },
                }}
              >
                <Globe2 size={20} strokeWidth={2} color={accent} />
              </ListItemIcon>
              <ListItemText
                primary="远程"
                secondary="占位：后续对接远程环境"
                primaryTypographyProps={{
                  sx: { ...composerLabelText, color: ink },
                }}
              />
            </MenuItem>
          </Menu>
        </Stack>
      </Stack>
    </Stack>
  );
}
