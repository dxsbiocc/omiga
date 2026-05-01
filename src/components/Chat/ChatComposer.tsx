import {
  memo,
  useEffect,
  useLayoutEffect,
  useState,
  useMemo,
  useRef,
  useCallback,
  useImperativeHandle,
  createElement,
  type Ref,
  type MutableRefObject,
  type KeyboardEvent,
  type Dispatch,
  type SetStateAction,
} from "react";
import { invoke } from "@tauri-apps/api/core";
import { confirm } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
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
import { lighten, type Theme } from "@mui/material/styles";
import {
  Add,
  ExpandMore,
  Mic,
  Square,
  SmartToy,
  Route as RouteIcon,
  AutoAwesome,
  ForumOutlined,
  Close,
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
  ChevronRight,
  FolderOpen as LucideFolderOpen,
  Laptop,
  Globe2,
  GitBranch,
  File as LucideFile,
  Folder as LucideFolder,
  Plus,
  Terminal,
  Server,
  Container,
  Cloud,
  Gauge,
  Atom,
  Settings,
} from "lucide-react";
import {
  useUiStore,
  useChatComposerStore,
  type PermissionMode,
  type SandboxBackend,
  type LocalVenvType,
} from "../../state";
import { normalizeAgentDisplayName } from "../../state/agentStore";
import {
  parseResearchCommand,
  WORKFLOW_SLASH_COMMANDS,
  type SlashCommandId,
  type WorkflowSlashCommandDefinition,
} from "../../utils/workflowCommands";
import {
  RSYNC_INSTALL_HELP_URL,
  RSYNC_SSH_WARN_STORAGE_KEY,
} from "../../lib/rsyncSsh";
import {
  buildComposerMentionChildPath,
  filterComposerMentionRows,
  joinWorkspaceMentionDirectory,
  normalizeComposerMentionPath,
  parentComposerMentionDirectory,
  parseComposerFileMentionInput,
  sortComposerMentionRows,
  type ComposerMentionRow,
} from "./composerPathMentions";
import {
  EDITABLE_EMPTY_SENTINEL,
  getEditableInputUpdate,
  normalizeEditableText,
} from "./editableText";

const SANDBOX_BACKENDS: { id: SandboxBackend; label: string }[] = [
  { id: "docker", label: "Docker" },
  { id: "modal", label: "Modal" },
  { id: "daytona", label: "Daytona" },
  { id: "singularity", label: "Singularity" },
];

/** 与各沙箱后端对应的图标（与二级菜单一致） */
const SANDBOX_BACKEND_ICON: Record<SandboxBackend, LucideIcon> = {
  docker: Container,
  modal: Cloud,
  daytona: Gauge,
  singularity: Atom,
};

const SANDBOX_LABEL: Record<SandboxBackend, string> = {
  modal: "Modal",
  daytona: "Daytona",
  docker: "Docker",
  singularity: "Singularity",
};

/** 与 SessionList「Language」二级菜单一致：离开一级行后再关闭子菜单的延迟（ms） */
const ENV_SUBMENU_PARENT_LEAVE_MS = 200;

/** React StrictMode 下 effect 会双跑，避免同页两次 `invoke` + 弹窗 */
let rsyncAvailabilityCheckStarted = false;

/** SSH 服务器配置（与 Rust `SshExecConfig` / 设置页一致：serde 使用 HostName、User、Port） */
interface SshServerConfig {
  host?: string;
  host_name?: string;
  HostName?: string;
  user?: string;
  User?: string;
  port?: number;
  Port?: number;
  enabled?: boolean;
}

type SshServersMap = Record<string, SshServerConfig>;

function sshResolvedHost(cfg: SshServerConfig): string | undefined {
  return cfg.HostName ?? cfg.host_name ?? cfg.host ?? undefined;
}

function sshResolvedUser(cfg: SshServerConfig): string {
  return cfg.User ?? cfg.user ?? "root";
}

function sshResolvedPort(cfg: SshServerConfig): number {
  const p = cfg.Port ?? cfg.port;
  return typeof p === "number" && !Number.isNaN(p) ? p : 22;
}

/** 展示为 user@host 或 user@host:port，供二级菜单与副标题使用 */
function sshConnectionLabel(cfg: SshServerConfig): string | undefined {
  const host = sshResolvedHost(cfg);
  if (!host) return undefined;
  const user = sshResolvedUser(cfg);
  const port = sshResolvedPort(cfg);
  return `${user}@${host}${port !== 22 ? `:${port}` : ""}`;
}
import { usePencilPalette } from "../../theme";
import { ProviderSwitcher } from "./ProviderSwitcher";
import type { BackgroundAgentTask } from "./backgroundAgentTypes";
import {
  canSendFollowUpToTask,
} from "./backgroundAgentTypes";
import { PermissionPromptBar } from "../permissions/PermissionPromptBar";
import {
  AskUserQuestionWizard,
  type AskUserQuestionItem,
} from "./AskUserQuestionWizard";

export type ChatComposerAskUserQuestion = {
  resetKey: string;
  questions: AskUserQuestionItem[];
  selections: Record<string, string>;
  onSelectionsChange: Dispatch<SetStateAction<Record<string, string>>>;
  onSubmit: () => void;
};

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

/** 解析 hex 相对亮度（0–1），非 hex 时返回 0.5 避免误判 */
function hexRelativeLuminance(color: string): number {
  if (typeof color !== "string" || !color.startsWith("#")) return 0.5;
  const h = color.replace("#", "");
  if (h.length !== 6 || !/^[0-9a-fA-F]{6}$/.test(h)) return 0.5;
  const r = parseInt(h.slice(0, 2), 16) / 255;
  const g = parseInt(h.slice(2, 4), 16) / 255;
  const b = parseInt(h.slice(4, 6), 16) / 255;
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

/**
 * 「每次询问」用 info 语义色；部分 accent 预设里 info.main 接近黑色，暗色模式下与底栏对比不足。
 * 暗色模式目标亮度更高（minLum），必要时多级 lighten，保证图标与文字足够醒目。
 */
function askPermissionAccent(theme: Theme): string {
  const { info, mode } = theme.palette;
  /** 暗色下希望更接近「亮蓝/亮青」观感，略抬高阈值 */
  const minLum = 0.58;
  if (mode !== "dark") return info.main;
  if (hexRelativeLuminance(info.main) >= minLum) return info.main;
  if (hexRelativeLuminance(info.light) >= minLum) return info.light;

  let next = lighten(info.light, 0.38);
  if (hexRelativeLuminance(next) >= minLum) return next;

  next = lighten(info.main, 0.82);
  if (hexRelativeLuminance(next) >= minLum) return next;

  return lighten(next, 0.22);
}

/** 权限等级语义色：保守询问 / 默认自动 / 高风险跳过 */
function permissionModeAccent(theme: Theme, mode: PermissionMode): string {
  const p = theme.palette;
  switch (mode) {
    case "ask":
      return askPermissionAccent(theme);
    case "auto":
      return p.primary.main;
    case "bypass":
      return p.warning.main;
    default:
      return p.primary.main;
  }
}

type AvailableAgentRow = { agentType: string; description: string; background: boolean };
type AvailableSkillRow = {
  name: string;
  description: string;
  source: "claudeUser" | "omigaUser" | "omigaProject" | "omigaPlugin";
  tags: string[];
};
type SlashPickerOption =
  | { kind: "command"; command: WorkflowSlashCommandDefinition }
  | { kind: "agent"; agent: AvailableAgentRow }
  | { kind: "skill"; skill: AvailableSkillRow };

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
  const theme = useTheme();
  const iconColor = isFile
    ? theme.palette.info.main
    : theme.palette.warning.main;
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
        bgcolor: alpha(iconColor, theme.palette.mode === "dark" ? 0.16 : 0.1),
        flexShrink: 0,
        border: `1px solid ${alpha(iconColor, theme.palette.mode === "dark" ? 0.52 : 0.34)}`,
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

function assignRef<T>(ref: Ref<T> | undefined, value: T | null) {
  if (ref == null) return;
  if (typeof ref === "function") ref(value);
  else (ref as MutableRefObject<T | null>).current = value;
}

function ensureEditableTextNode(el: HTMLElement): Text {
  const doc = el.ownerDocument;
  const first = el.firstChild;
  if (first?.nodeType === Node.TEXT_NODE) {
    return first as Text;
  }
  const textNode = doc.createTextNode(el.textContent ?? "");
  el.textContent = "";
  el.appendChild(textNode);
  return textNode;
}

function setEditableDomText(el: HTMLElement, value: string, focused: boolean) {
  el.textContent = value || (focused ? EDITABLE_EMPTY_SENTINEL : "");
}

function setEditableTextSelection(
  el: HTMLElement,
  start: number,
  end = start,
) {
  const doc = el.ownerDocument;
  const selection = doc.defaultView?.getSelection();
  if (!selection) return;
  if (normalizeEditableText(el.textContent ?? "") === "") {
    setEditableDomText(el, "", true);
  }
  const textNode = ensureEditableTextNode(el);
  const raw = textNode.textContent ?? "";
  const isSentinel = raw === EDITABLE_EMPTY_SENTINEL;
  const len = isSentinel ? 0 : raw.length;
  const toDomOffset = (value: number) =>
    isSentinel ? 1 : Math.max(0, Math.min(value, len));
  const range = doc.createRange();
  range.setStart(textNode, toDomOffset(start));
  range.setEnd(textNode, toDomOffset(end));
  selection.removeAllRanges();
  selection.addRange(range);
}

function insertTextAtEditableSelection(el: HTMLElement, text: string): string {
  if (normalizeEditableText(el.textContent ?? "") === "") {
    el.textContent = text;
    setEditableTextSelection(el, text.length);
    return text;
  }
  const doc = el.ownerDocument;
  const selection = doc.defaultView?.getSelection();
  if (!selection || selection.rangeCount === 0) {
    const next = `${normalizeEditableText(el.textContent ?? "")}${text}`;
    el.textContent = next;
    setEditableTextSelection(el, next.length);
    return next;
  }
  const range = selection.getRangeAt(0);
  if (!el.contains(range.commonAncestorContainer)) {
    const next = `${normalizeEditableText(el.textContent ?? "")}${text}`;
    el.textContent = next;
    setEditableTextSelection(el, next.length);
    return next;
  }
  range.deleteContents();
  const node = doc.createTextNode(text);
  range.insertNode(node);
  range.setStartAfter(node);
  range.collapse(true);
  selection.removeAllRanges();
  selection.addRange(range);
  const next = normalizeEditableText(el.textContent ?? "");
  if (next !== el.textContent) {
    const caret = next.length;
    el.textContent = next;
    setEditableTextSelection(el, caret);
  }
  return next;
}

export interface ChatComposerRef {
  getValue: () => string;
  setValue: (value: string) => void;
  /** Append text to the current input, with a newline separator when input is non-empty. */
  appendValue: (text: string) => void;
  focus: () => void;
}

export interface ChatComposerProps {
  sessionId: string | null;
  /** Absolute workspace path when set */
  workspacePath: string;
  needsWorkspacePath: boolean;
  onPickWorkspace: () => void;
  /** Optional controlled initial value; use composerRef for programmatic updates. */
  input?: string;
  onInputChange?: (v: string) => void;
  onKeyDown: (e: React.KeyboardEvent<HTMLElement>) => void;
  inputRef?: React.Ref<HTMLElement>;
  composerRef?: React.Ref<ChatComposerRef>;
  isStreaming: boolean;
  isConnecting: boolean;
  /** True while waiting for first model chunk (show cancel with connecting/streaming). */
  waitingFirstChunk?: boolean;
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
  /** Close sidechain transcript drawer (used when switching back to main session). */
  onCloseBackgroundTranscript?: () => void;
  /** Blocked `ask_user_question` — wizard above permission bar, same band as permission prompt. */
  askUserQuestion?: ChatComposerAskUserQuestion | null;
}

export const ChatComposer = memo(function ChatComposer({
  sessionId,
  workspacePath,
  needsWorkspacePath,
  onPickWorkspace,
  input: initialInput,
  onInputChange,
  onKeyDown,
  inputRef,
  composerRef,
  isStreaming,
  isConnecting,
  waitingFirstChunk = false,
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
  onCloseBackgroundTranscript,
  askUserQuestion = null,
}: ChatComposerProps) {
  const [input, setInput] = useState(initialInput ?? "");
  const [selectedSlashCommandId, setSelectedSlashCommandId] =
    useState<SlashCommandId | null>(null);
  const [selectedSkillCommandName, setSelectedSkillCommandName] =
    useState<string | null>(null);
  const inputValueRef = useRef(input);
  inputValueRef.current = input;
  const editableRef = useRef<HTMLSpanElement | null>(null);
  const editableCompositionRef = useRef(false);
  const normalizeCommandValue = useCallback(
    (
      rawValue: string,
    ): { commandId: SlashCommandId | null; body: string } => {
      const trimmed = rawValue.trim();
      const research = parseResearchCommand(trimmed);
      if (research) {
        return { commandId: "research", body: research.body };
      }
      const workflow = WORKFLOW_SLASH_COMMANDS.find((command) => {
        const label = command.label;
        return trimmed === label || trimmed.startsWith(`${label} `);
      });
      if (workflow) {
        const body =
          trimmed === workflow.label
            ? ""
            : trimmed.slice(workflow.label.length).trimStart();
        return { commandId: workflow.id, body };
      }
      return { commandId: null, body: rawValue };
    },
    [],
  );
  const setInputValue = useCallback(
    (v: string) => {
      const normalized = normalizeCommandValue(v);
      setSelectedSlashCommandId(normalized.commandId);
      setSelectedSkillCommandName(null);
      setInput(normalized.body);
      const outgoing = normalized.commandId
        ? normalized.body.trim().length > 0
          ? `/${normalized.commandId} ${normalized.body}`
          : `/${normalized.commandId}`
        : normalized.body;
      onInputChange?.(outgoing);
    },
    [normalizeCommandValue, onInputChange],
  );
  const commitEditableInput = useCallback(
    (nextValue: string) => {
      if (selectedSlashCommandId) {
        setInput(nextValue);
        onInputChange?.(
          nextValue.trim().length > 0
            ? `/${selectedSlashCommandId} ${nextValue}`
            : `/${selectedSlashCommandId}`,
        );
        return;
      }
      if (selectedSkillCommandName) {
        setInput(nextValue);
        onInputChange?.(
          nextValue.trim().length > 0
            ? `$${selectedSkillCommandName} ${nextValue}`
            : `$${selectedSkillCommandName}`,
        );
        return;
      }
      setInputValue(nextValue);
    },
    [
      onInputChange,
      selectedSkillCommandName,
      selectedSlashCommandId,
      setInputValue,
    ],
  );
  const focusEditableEnd = useCallback(() => {
    const el = editableRef.current;
    if (!el) return;
    el.focus();
    setEditableTextSelection(el, normalizeEditableText(el.textContent ?? "").length);
  }, []);
  useImperativeHandle(
    composerRef,
    () => ({
      getValue: () =>
        selectedSlashCommandId
          ? inputValueRef.current.trim().length > 0
            ? `/${selectedSlashCommandId} ${inputValueRef.current}`
            : `/${selectedSlashCommandId}`
          : selectedSkillCommandName
            ? inputValueRef.current.trim().length > 0
              ? `$${selectedSkillCommandName} ${inputValueRef.current}`
              : `$${selectedSkillCommandName}`
          : inputValueRef.current,
      setValue: (v: string) => setInputValue(v),
      appendValue: (text: string) => {
        const cur = inputValueRef.current;
        setInputValue(cur ? `${cur}\n${text}` : text);
      },
      focus: () => focusEditableEnd(),
    }),
    [
      selectedSkillCommandName,
      selectedSlashCommandId,
      setInputValue,
      focusEditableEnd,
    ],
  );
  const theme = useTheme();
  const pen = usePencilPalette();
  const accent = theme.palette.primary.main;
  const commandTone = theme.palette.success.main;
  const skillTone = theme.palette.secondary.main;
  const fileTone = theme.palette.info.main;
  const agentTone = accent;
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
  const COMPOSER_LH = 1.72;
  /** 主会话一轮进行中：连接中 / 首包前等待 / 流式输出 — 与输入区「取消任务」按钮一致 */
  const agentTurnActive =
    isStreaming || isConnecting || waitingFirstChunk;
  /** Hairline border / shadow tint — theme-aware */
  const edge = (a: number) =>
    alpha(isDark ? theme.palette.common.white : theme.palette.common.black, a);
  const semanticChipSurface = (tone: string) => ({
    bgcolor: alpha(tone, isDark ? 0.2 : 0.11),
    borderColor: alpha(tone, isDark ? 0.62 : 0.45),
    color: tone,
    boxShadow: `0 1px 2px ${alpha(tone, isDark ? 0.2 : 0.14)}`,
    "& .MuiChip-label": {
      overflow: "hidden",
      textOverflow: "ellipsis",
      color: tone,
    },
    "& .MuiChip-icon": {
      color: tone,
      marginTop: 0,
      marginBottom: 0,
    },
  } as const);
  /** Input card — closer to solid paper so the typing area reads lighter */
  const composerBg = alpha(paper, isDark ? 0.97 : 0.99);
  const setSettingsOpen = useUiStore((s) => s.setSettingsOpen);
  const setSettingsTabIndex = useUiStore((s) => s.setSettingsTabIndex);
  const setSettingsExecutionSubTab = useUiStore(
    (s) => s.setSettingsExecutionSubTab,
  );
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
    sshServer,
    setSshServer,
    sandboxBackend,
    setSandboxBackend,
    localVenvType,
    localVenvName,
    setLocalVenv,
    selectedBranchByRoot,
    setBranchForRoot,
  } = useChatComposerStore();

  const permissionAccent = permissionModeAccent(theme, permissionMode);

  const [plusAnchor, setPlusAnchor] = useState<null | HTMLElement>(null);
  const [permissionAnchor, setPermissionAnchor] = useState<null | HTMLElement>(
    null,
  );
  const [envAnchor, setEnvAnchor] = useState<null | HTMLElement>(null);
  const [sandboxMenuAnchor, setSandboxMenuAnchor] =
    useState<null | HTMLElement>(null);
  const [sshMenuAnchor, setSshMenuAnchor] = useState<null | HTMLElement>(null);
  const [venvMenuAnchor, setVenvMenuAnchor] = useState<null | HTMLElement>(
    null,
  );
  const [sshServers, setSshServers] = useState<SshServersMap>({});
  const [sshServersLoading, setSshServersLoading] = useState(false);
  const [localVenvs, setLocalVenvs] = useState<
    { kind: string; label: string; name: string }[]
  >([]);
  const [localVenvsLoading, setLocalVenvsLoading] = useState(false);

  // 沙箱 / SSH 二级菜单：与 SessionList「Language」相同（定时器 + 嵌套 Menu + pointerEvents），避免 Popover 与一级 Menu 模态层事件死循环
  const sandboxSubmenuLeaveTimerRef = useRef<ReturnType<
    typeof setTimeout
  > | null>(null);
  const clearSandboxSubmenuLeaveTimer = useCallback(() => {
    if (sandboxSubmenuLeaveTimerRef.current) {
      clearTimeout(sandboxSubmenuLeaveTimerRef.current);
      sandboxSubmenuLeaveTimerRef.current = null;
    }
  }, []);
  const openSandboxSub = useCallback(
    (el: HTMLElement | null) => {
      if (!el) return;
      clearSandboxSubmenuLeaveTimer();
      setSandboxMenuAnchor((prev) => (prev === el ? prev : el));
    },
    [clearSandboxSubmenuLeaveTimer],
  );
  const scheduleCloseSandboxSub = useCallback(() => {
    clearSandboxSubmenuLeaveTimer();
    sandboxSubmenuLeaveTimerRef.current = setTimeout(() => {
      sandboxSubmenuLeaveTimerRef.current = null;
      setSandboxMenuAnchor(null);
    }, ENV_SUBMENU_PARENT_LEAVE_MS);
  }, [clearSandboxSubmenuLeaveTimer]);

  const sshSubmenuLeaveTimerRef = useRef<ReturnType<typeof setTimeout> | null>(
    null,
  );
  const clearSshSubmenuLeaveTimer = useCallback(() => {
    if (sshSubmenuLeaveTimerRef.current) {
      clearTimeout(sshSubmenuLeaveTimerRef.current);
      sshSubmenuLeaveTimerRef.current = null;
    }
  }, []);
  const openSshSub = useCallback(
    (el: HTMLElement | null) => {
      if (!el) return;
      clearSshSubmenuLeaveTimer();
      setSshMenuAnchor((prev) => (prev === el ? prev : el));
    },
    [clearSshSubmenuLeaveTimer],
  );
  const scheduleCloseSshSub = useCallback(() => {
    clearSshSubmenuLeaveTimer();
    sshSubmenuLeaveTimerRef.current = setTimeout(() => {
      sshSubmenuLeaveTimerRef.current = null;
      setSshMenuAnchor(null);
    }, ENV_SUBMENU_PARENT_LEAVE_MS);
  }, [clearSshSubmenuLeaveTimer]);

  const venvSubmenuLeaveTimerRef = useRef<ReturnType<typeof setTimeout> | null>(
    null,
  );
  const clearVenvSubmenuLeaveTimer = useCallback(() => {
    if (venvSubmenuLeaveTimerRef.current) {
      clearTimeout(venvSubmenuLeaveTimerRef.current);
      venvSubmenuLeaveTimerRef.current = null;
    }
  }, []);
  const openVenvSub = useCallback(
    (el: HTMLElement | null) => {
      if (!el) return;
      clearVenvSubmenuLeaveTimer();
      setVenvMenuAnchor((prev) => (prev === el ? prev : el));
    },
    [clearVenvSubmenuLeaveTimer],
  );
  const scheduleCloseVenvSub = useCallback(() => {
    clearVenvSubmenuLeaveTimer();
    venvSubmenuLeaveTimerRef.current = setTimeout(() => {
      venvSubmenuLeaveTimerRef.current = null;
      setVenvMenuAnchor(null);
    }, ENV_SUBMENU_PARENT_LEAVE_MS);
  }, [clearVenvSubmenuLeaveTimer]);

  const sshFetchSeq = useRef(0);

  const loadSshServers = useCallback((opts?: { showLoading?: boolean }) => {
    const showLoading = opts?.showLoading !== false;
    const seq = ++sshFetchSeq.current;
    if (showLoading) setSshServersLoading(true);
    invoke<SshServersMap>("get_ssh_configs")
      .then((configs) => {
        if (seq !== sshFetchSeq.current) return;
        // 与 Execution 设置页一致：展示 get_ssh_configs 全部条目（含 ~/.ssh/config 合并结果）
        setSshServers(configs ?? {});
      })
      .catch(() => {
        if (seq !== sshFetchSeq.current) return;
        setSshServers({});
      })
      .finally(() => {
        if (seq !== sshFetchSeq.current) return;
        setSshServersLoading(false);
      });
  }, []);

  const venvFetchSeq = useRef(0);

  const loadLocalVenvs = useCallback((projectRoot: string) => {
    const seq = ++venvFetchSeq.current;
    setLocalVenvsLoading(true);
    invoke<{ kind: string; label: string; name: string }[]>("list_local_venvs", {
      projectRoot,
    })
      .then((items) => {
        if (seq !== venvFetchSeq.current) return;
        setLocalVenvs(items ?? []);
      })
      .catch(() => {
        if (seq !== venvFetchSeq.current) return;
        setLocalVenvs([]);
      })
      .finally(() => {
        if (seq !== venvFetchSeq.current) return;
        setLocalVenvsLoading(false);
      });
  }, []);

  /** Settings → Execution（侧栏 index 9），内层 Tab：0 Modal / 1 Daytona / 2 SSH */
  const openExecutionSettings = useCallback(
    (executionSubTab: 0 | 1 | 2) => {
      setSettingsTabIndex(9);
      setSettingsExecutionSubTab(executionSubTab);
      setSettingsOpen(true);
      setRightPanelMode("settings");
    },
    [
      setSettingsTabIndex,
      setSettingsExecutionSubTab,
      setSettingsOpen,
      setRightPanelMode,
    ],
  );

  // 预取 SSH 列表，避免首次 hover 长时间「加载中」
  useEffect(() => {
    loadSshServers({ showLoading: true });
  }, [loadSshServers]);

  // 打开执行环境菜单时静默刷新（不闪 loading），与设置里新增的配置同步
  useEffect(() => {
    if (!envAnchor) return;
    loadSshServers({ showLoading: false });
  }, [envAnchor, loadSshServers]);

  // 首次使用 SSH 环境时检测 rsync；缺失则弹窗（确认打开安装说明 / 取消），见 `is_rsync_available`
  useEffect(() => {
    if (environment !== "ssh") return;
    if (rsyncAvailabilityCheckStarted) return;
    rsyncAvailabilityCheckStarted = true;
    let cancelled = false;
    void (async () => {
      try {
        const ok = await invoke<boolean>("is_rsync_available");
        if (cancelled || ok) return;
        if (
          typeof localStorage !== "undefined" &&
          localStorage.getItem(RSYNC_SSH_WARN_STORAGE_KEY) === "1"
        ) {
          return;
        }
        const openDocs = await confirm(
          "SSH 远程环境的「文件同步」（技能、credentials、缓存等）依赖本机已安装 rsync。\n\n未检测到 rsync：将不会同步上述文件，远端命令仍可执行。\n\n是否在浏览器中打开 rsync 安装说明？\n（点「取消」关闭提示；安装完成后可继续使用 SSH，同步将自动生效。）",
          {
            title: "需要安装 rsync",
            kind: "warning",
            okLabel: "查看安装说明",
            cancelLabel: "取消",
          },
        );
        if (typeof localStorage !== "undefined") {
          localStorage.setItem(RSYNC_SSH_WARN_STORAGE_KEY, "1");
        }
        if (!cancelled && openDocs) {
          await openUrl(RSYNC_INSTALL_HELP_URL);
        }
      } catch {
        /* 非 Tauri 或对话框不可用时忽略 */
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [environment]);

  const [gitInfo, setGitInfo] = useState<GitWorkspaceInfo | null>(null);
  const [availableAgents, setAvailableAgents] = useState<AvailableAgentRow[]>(
    [],
  );
  const [availableSkills, setAvailableSkills] = useState<AvailableSkillRow[]>(
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
    let cancelled = false;
    invoke<AvailableSkillRow[]>("list_available_skills", {
      projectRoot: workspacePath || ".",
    })
      .then((rows) => {
        if (!cancelled) setAvailableSkills(rows ?? []);
      })
      .catch(() => {
        if (!cancelled) setAvailableSkills([]);
      });
    return () => {
      cancelled = true;
    };
  }, [workspacePath]);

  useEffect(() => {
    if (!workspacePath || needsWorkspacePath) {
      setGitInfo(null);
      return;
    }
    if (environment === "ssh") {
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
  }, [workspacePath, needsWorkspacePath, environment]);

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

  const composerInputAnchorRef = useRef<HTMLDivElement | null>(null);
  const [composerPopoverWidth, setComposerPopoverWidth] = useState<number | null>(
    null,
  );

  useLayoutEffect(() => {
    const el = composerInputAnchorRef.current;
    if (!el) return;
    const updateWidth = () => {
      const next = Math.round(el.getBoundingClientRect().width);
      setComposerPopoverWidth(next > 0 ? next : null);
    };
    updateWidth();
    const resizeObserver =
      typeof ResizeObserver !== "undefined"
        ? new ResizeObserver(updateWidth)
        : null;
    resizeObserver?.observe(el);
    if (typeof window !== "undefined") {
      window.addEventListener("resize", updateWidth);
    }
    return () => {
      resizeObserver?.disconnect();
      if (typeof window !== "undefined") {
        window.removeEventListener("resize", updateWidth);
      }
    };
  }, []);

  /** `/`：工作流命令或 Agent 选择（整段输入仅为 `/` 或 `/query`） */
  const slashParse = useMemo(() => {
    if (selectedSlashCommandId || selectedSkillCommandName) {
      return { active: false as const, query: "" };
    }
    const t = input;
    if (!/^\/[^\s]*$/u.test(t)) return { active: false as const, query: "" };
    return { active: true as const, query: t.slice(1) };
  }, [input, selectedSkillCommandName, selectedSlashCommandId]);

  /** `$`：Skill 选择（整段输入仅为 `$` 或 `$query`） */
  const skillParse = useMemo(() => {
    if (selectedSlashCommandId || selectedSkillCommandName) {
      return { active: false as const, query: "" };
    }
    const t = input;
    if (!/^\$[^\s]*$/u.test(t)) return { active: false as const, query: "" };
    return { active: true as const, query: t.slice(1) };
  }, [input, selectedSkillCommandName, selectedSlashCommandId]);

  /** `@`：工作区相对路径选择（支持 `@dir/child` 逐级选择） */
  const fileParse = useMemo(() => {
    return parseComposerFileMentionInput(input);
  }, [input]);

  // Agents selectable via slash-picker: exclude defaults and background-only agents.
  const selectableAgents = useMemo(
    () =>
      availableAgents.filter(
        (a) =>
          a.agentType !== "auto" &&
          a.agentType !== "general-purpose" &&
          !a.background,
      ),
    [availableAgents],
  );

  const filteredAtAgents = useMemo(() => {
    if (!slashParse.active) return [];
    const q = slashParse.query.toLowerCase();
    return selectableAgents.filter((a) => {
      const id = a.agentType.toLowerCase();
      const displayName = normalizeAgentDisplayName(a.agentType).toLowerCase();
      return !q || id.startsWith(q) || id.includes(q) || displayName.includes(q);
    });
  }, [selectableAgents, slashParse]);

  const filteredWorkflowCommands = useMemo(() => {
    if (!slashParse.active) return [];
    const q = slashParse.query.toLowerCase();
    return WORKFLOW_SLASH_COMMANDS.filter((command) => {
      const id = command.id.toLowerCase();
      const label = command.label.toLowerCase();
      const desc = command.description.toLowerCase();
      return !q || id.startsWith(q) || label.includes(q) || desc.includes(q);
    });
  }, [slashParse]);

  const filteredSkillCommands = useMemo(() => {
    if (!skillParse.active) return [];
    const q = skillParse.query.toLowerCase();
    return availableSkills.filter((skill) => {
      const name = skill.name.toLowerCase();
      const desc = skill.description.toLowerCase();
      const tags = (skill.tags ?? []).join(" ").toLowerCase();
      return !q || name.includes(q) || desc.includes(q) || tags.includes(q);
    });
  }, [availableSkills, skillParse]);

  const filteredSlashOptions = useMemo<SlashPickerOption[]>(
    () =>
      skillParse.active
        ? filteredSkillCommands.map((skill) => ({
            kind: "skill" as const,
            skill,
          }))
        : [
            ...filteredWorkflowCommands.map((command) => ({
              kind: "command" as const,
              command,
            })),
            ...filteredAtAgents.map((agent) => ({
              kind: "agent" as const,
              agent,
            })),
          ],
    [
      filteredAtAgents,
      filteredSkillCommands,
      filteredWorkflowCommands,
      skillParse.active,
    ],
  );

  const explicitSlashCommandId = useMemo(() => {
    return selectedSlashCommandId;
  }, [selectedSlashCommandId]);
  const explicitSlashCommandBody = explicitSlashCommandId ? input : null;

  const slashFilterKey = useMemo(
    () =>
      filteredSlashOptions
        .map((item) =>
          item.kind === "command"
            ? `cmd:${item.command.id}`
            : item.kind === "agent"
              ? `agent:${item.agent.agentType}`
              : `skill:${item.skill.name}`,
        )
        .join("\u0001"),
    [filteredSlashOptions],
  );

  const [slashHighlightIndex, setSlashHighlightIndex] = useState(-1);
  const slashHighlightIndexRef = useRef(-1);
  const slashListRef = useRef<HTMLUListElement>(null);
  /** User clicked outside the / picker; hide until input changes or textarea refocuses. */
  const [slashPickerDismissed, setSlashPickerDismissed] = useState(false);

  const highlightedSlashCommand = useMemo(() => {
    if (explicitSlashCommandId) {
      return (
        WORKFLOW_SLASH_COMMANDS.find(
          (command) => command.id === explicitSlashCommandId,
        ) ?? null
      );
    }
    return null;
  }, [
    explicitSlashCommandId,
  ]);

  const highlightedSkillCommand = useMemo(() => {
    if (selectedSkillCommandName) {
      return (
        availableSkills.find(
          (skill) =>
            skill.name.toLowerCase() === selectedSkillCommandName.toLowerCase(),
        ) ?? {
          name: selectedSkillCommandName,
          description: "直接使用该 Skill",
          source: "omigaProject" as const,
          tags: [],
        }
      );
    }
    return null;
  }, [
    availableSkills,
    selectedSkillCommandName,
  ]);

  const [fileGlobMatches, setFileGlobMatches] = useState<
    ComposerMentionRow[]
  >([]);
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
    slashHighlightIndexRef.current = -1;
    setSlashHighlightIndex(-1);
  }, [slashFilterKey]);

  useEffect(() => {
    if (!fileParse.active || needsWorkspacePath || !workspacePath.trim()) {
      setFileGlobMatches([]);
      return;
    }
    const useSsh = environment === "ssh" && Boolean(sshServer?.trim());
    const useSandbox = environment === "sandbox" && Boolean(sandboxBackend?.trim());
    if (environment === "ssh" && !useSsh) {
      setFileGlobMatches([]);
      return;
    }
    if (environment === "sandbox" && (!useSandbox || !sessionId)) {
      setFileGlobMatches([]);
      return;
    }
    if (environment === "local" && !sessionId) {
      setFileGlobMatches([]);
      return;
    }
    let cancelled = false;
    setFileGlobLoading(true);
    const directoryPath = joinWorkspaceMentionDirectory(
      workspacePath,
      fileParse.directory,
    );
    const listPromise = useSsh
      ? invoke<{
          entries: Array<{
            name: string;
            path: string;
            is_directory: boolean;
            size?: number | null;
          }>;
        }>("ssh_list_directory", {
          sshProfileName: sshServer!.trim(),
          path: directoryPath,
        })
      : useSandbox
        ? invoke<{
            entries: Array<{
              name: string;
              path: string;
              is_directory: boolean;
              size?: number | null;
            }>;
          }>("sandbox_list_directory", {
            sessionId,
            sandboxBackend: sandboxBackend!.trim(),
            path: directoryPath,
          })
        : invoke<{
            entries: Array<{
              name: string;
              path: string;
              is_directory: boolean;
              size?: number | null;
            }>;
          }>("list_directory", { path: directoryPath, sessionId });
    listPromise
      .then((res) => {
        if (cancelled) return;
        const list: ComposerMentionRow[] = sortComposerMentionRows(
          (res.entries ?? []).map((e) => ({
            path: buildComposerMentionChildPath(fileParse.directory, e.name),
            is_file: !e.is_directory,
            size: typeof e.size === "number" ? e.size : 0,
          })),
        );
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
  }, [
    fileParse.active,
    needsWorkspacePath,
    workspacePath,
    fileParse.directory,
    sessionId,
    environment,
    sshServer,
    sandboxBackend,
  ]);

  const filteredFilePaths = useMemo(() => {
    if (!fileParse.active) return [];
    return filterComposerMentionRows(fileGlobMatches, fileParse.filter);
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
    if (
      !(slashParse.active || skillParse.active) ||
      filteredSlashOptions.length === 0 ||
      slashHighlightIndex < 0
    ) {
      return;
    }
    const el = slashListRef.current?.querySelector(
      `[data-slash-index="${slashHighlightIndex}"]`,
    );
    el?.scrollIntoView({ block: "nearest", behavior: "smooth" });
  }, [
    slashHighlightIndex,
    slashParse.active,
    skillParse.active,
    filteredSlashOptions.length,
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
      setInputValue("");
      queueMicrotask(() => focusEditableEnd());
    },
    [focusEditableEnd, setComposerAgentType, setInputValue],
  );

  const pickWorkflowCommand = useCallback(
    (commandId: WorkflowSlashCommandDefinition["id"]) => {
      setSelectedSlashCommandId(commandId);
      setSelectedSkillCommandName(null);
      setComposerAgentType("auto");
      setInput("");
      onInputChange?.(`/${commandId}`);
      queueMicrotask(() => focusEditableEnd());
    },
    [focusEditableEnd, onInputChange, setComposerAgentType],
  );

  const pickSkillCommand = useCallback(
    (skillName: string) => {
      setSelectedSlashCommandId(null);
      setSelectedSkillCommandName(skillName);
      setComposerAgentType("auto");
      setInput("");
      onInputChange?.(`$${skillName}`);
      queueMicrotask(() => focusEditableEnd());
    },
    [focusEditableEnd, onInputChange, setComposerAgentType],
  );

  const pickFilePath = useCallback(
    (relPath: string) => {
      const safe = normalizeComposerMentionPath(relPath);
      if (!safe) return;
      addComposerAttachedPath(safe);
      setInputValue("");
      queueMicrotask(() => focusEditableEnd());
    },
    [addComposerAttachedPath, focusEditableEnd, setInputValue],
  );

  const enterFileDirectory = useCallback(
    (relPath: string) => {
      const safe = normalizeComposerMentionPath(relPath);
      if (!safe) return;
      setInputValue(`@${safe}/`);
      queueMicrotask(() => focusEditableEnd());
    },
    [focusEditableEnd, setInputValue],
  );

  const goUpFileDirectory = useCallback(() => {
    if (!fileParse.active) return;
    const parent = parentComposerMentionDirectory(fileParse.directory);
    setInputValue(parent ? `@${parent}/` : "@");
    queueMicrotask(() => focusEditableEnd());
  }, [fileParse.active, fileParse.directory, focusEditableEnd, setInputValue]);

  const mergedEditableRef = useCallback(
    (el: HTMLSpanElement | null) => {
      editableRef.current = el;
      assignRef(inputRef, el);
    },
    [inputRef],
  );

  useLayoutEffect(() => {
    const el = editableRef.current;
    if (!el) {
      return;
    }
    if (editableCompositionRef.current) {
      return;
    }
    const isFocused = el.ownerDocument.activeElement === el;
    if (normalizeEditableText(el.textContent ?? "") === input) {
      if (input === "" && (el.textContent ?? "") !== "") {
        setEditableDomText(el, "", isFocused);
      }
      return;
    }
    setEditableDomText(el, input, isFocused);
  }, [input]);

  const handleComposerKeyDown = useCallback(
    (e: KeyboardEvent<HTMLElement>) => {
      const ne = e.nativeEvent;
      if (ne.isComposing || ne.keyCode === 229) {
        onKeyDown(e);
        return;
      }
      /* Home / End：光标到全文首/尾（保留 Ctrl/Cmd/Alt 给系统或浏览器默认） */
      if (
        (e.key === "Home" || e.key === "End") &&
        !e.ctrlKey &&
        !e.metaKey &&
        !e.altKey
      ) {
        e.preventDefault();
        const el = editableRef.current ?? e.currentTarget;
        const len = el.textContent?.length ?? 0;
        const selection = el.ownerDocument.defaultView?.getSelection();
        const range =
          selection && selection.rangeCount > 0 ? selection.getRangeAt(0) : null;
        const a =
          range && el.contains(range.startContainer) ? range.startOffset : 0;
        const b = range && el.contains(range.endContainer) ? range.endOffset : a;
        if (e.key === "Home") {
          if (e.shiftKey) {
            setEditableTextSelection(el, 0, Math.max(a, b));
          } else {
            setEditableTextSelection(el, 0);
          }
        } else if (e.shiftKey) {
          setEditableTextSelection(el, Math.min(a, b), len);
        } else {
          setEditableTextSelection(el, len);
        }
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
      if (
        explicitSlashCommandId &&
        (e.key === "Backspace" || e.key === "Delete") &&
        !explicitSlashCommandBody
      ) {
        e.preventDefault();
        setSelectedSlashCommandId(null);
        setInputValue("");
        return;
      }
      if (
        selectedSkillCommandName &&
        (e.key === "Backspace" || e.key === "Delete") &&
        input.trim() === ""
      ) {
        e.preventDefault();
        setSelectedSkillCommandName(null);
        setInputValue("");
        return;
      }
      if (
        e.key === "Enter" &&
        e.shiftKey &&
        !fileParse.active &&
        !slashParse.active &&
        !skillParse.active
      ) {
        const el = editableRef.current;
        if (el) {
          e.preventDefault();
          const nextValue = insertTextAtEditableSelection(el, "\n");
          commitEditableInput(nextValue);
          return;
        }
      }
      if (fileParse.active) {
        if (e.key === "Escape") {
          setInputValue("");
          e.preventDefault();
          return;
        }
        if (filteredFilePaths.length > 0 && !fileGlobLoading) {
          const highlightedFileRow =
            filteredFilePaths[fileHighlightIndexRef.current] ??
            filteredFilePaths[0];
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
          if (e.key === "ArrowRight" && highlightedFileRow?.is_file === false) {
            e.preventDefault();
            enterFileDirectory(highlightedFileRow.path);
            return;
          }
          if (e.key === "ArrowLeft" && fileParse.directory) {
            e.preventDefault();
            goUpFileDirectory();
            return;
          }
          if (e.key === "Enter" && !e.shiftKey) {
            e.preventDefault();
            if (
              highlightedFileRow?.is_file === false &&
              !e.metaKey &&
              !e.ctrlKey
            ) {
              enterFileDirectory(highlightedFileRow.path);
            } else if (highlightedFileRow) {
              pickFilePath(highlightedFileRow.path);
            }
            return;
          }
          if (e.key === "Tab" && !e.shiftKey) {
            e.preventDefault();
            if (highlightedFileRow?.is_file === false) {
              enterFileDirectory(highlightedFileRow.path);
            } else if (highlightedFileRow) {
              pickFilePath(highlightedFileRow.path);
            }
            return;
          }
        }
      }
      if (slashParse.active || skillParse.active) {
        if (e.key === "Escape") {
          setInputValue("");
          e.preventDefault();
          return;
        }
        if (filteredSlashOptions.length > 0) {
          if (e.key === "ArrowDown") {
            e.preventDefault();
            setSlashHighlightIndex((i) => {
              const next = i < 0 ? 0 : (i + 1) % filteredSlashOptions.length;
              slashHighlightIndexRef.current = next;
              return next;
            });
            return;
          }
          if (e.key === "ArrowUp") {
            e.preventDefault();
            setSlashHighlightIndex((i) => {
              const next =
                i < 0
                  ? filteredSlashOptions.length - 1
                  : (i - 1 + filteredSlashOptions.length) %
                    filteredSlashOptions.length;
              slashHighlightIndexRef.current = next;
              return next;
            });
            return;
          }
          if (e.key === "Enter" && !e.shiftKey) {
            e.preventDefault();
            const idx = slashHighlightIndexRef.current;
            const pick = filteredSlashOptions[idx] ?? filteredSlashOptions[0];
            if (pick?.kind === "command") pickWorkflowCommand(pick.command.id);
            if (pick?.kind === "agent") pickAtAgent(pick.agent.agentType);
            if (pick?.kind === "skill") pickSkillCommand(pick.skill.name);
            return;
          }
          if (e.key === "Tab" && !e.shiftKey) {
            e.preventDefault();
            const idx = slashHighlightIndexRef.current;
            const pick = filteredSlashOptions[idx] ?? filteredSlashOptions[0];
            if (pick?.kind === "command") pickWorkflowCommand(pick.command.id);
            if (pick?.kind === "agent") pickAtAgent(pick.agent.agentType);
            if (pick?.kind === "skill") pickSkillCommand(pick.skill.name);
            return;
          }
        }
      }
      onKeyDown(e);
    },
    [
      slashParse.active,
      skillParse.active,
      fileParse.active,
      fileParse.directory,
      explicitSlashCommandBody,
      explicitSlashCommandId,
      selectedSkillCommandName,
      composerAgentType,
      composerAttachedPaths,
      filteredSlashOptions,
      filteredFilePaths,
      fileGlobLoading,
      input,
      setInputValue,
      commitEditableInput,
      onKeyDown,
      pickAtAgent,
      pickWorkflowCommand,
      pickSkillCommand,
      pickFilePath,
      enterFileDirectory,
      goUpFileDirectory,
      popComposerAttachedPath,
      setComposerAgentType,
    ],
  );

  const pathLabel = needsWorkspacePath
    ? "选择工作目录"
    : gitInfo?.displayPath
      ? shortRepoLabel(gitInfo.displayPath)
      : shortRepoLabel(workspacePath);

  const askUserBlocksInput = Boolean(askUserQuestion);

  const placeholder = askUserBlocksInput
    ? "请先完成上方的选择题…"
    : needsWorkspacePath
      ? "请先选择工作目录后再发送消息…"
      : followUpTaskId
        ? "追加说明将进入该后台 Agent 的下一轮工具循环…"
        : "输入 / 选择工作流命令或 Agent；输入 $ 选择 Skill；输入 @ 从当前工作目录选择…";

  /** 允许排队时：连接中 / 流式中均可继续输入；否则与旧行为一致（等待响应或生成时禁用）。 */
  const inputDisabled =
    (!allowInputWhileStreaming && (isConnecting || isStreaming)) ||
    askUserBlocksInput;

  const showSlashPopover =
    (slashParse.active || skillParse.active) &&
    !slashPickerDismissed &&
    !inputDisabled &&
    (skillParse.active ||
      availableAgents.length > 0 ||
      WORKFLOW_SLASH_COMMANDS.length > 0);

  const showFilePopover =
    fileParse.active &&
    !filePickerDismissed &&
    !inputDisabled &&
    !needsWorkspacePath &&
    Boolean(workspacePath.trim());

  const filePickerDirectoryLabel =
    fileParse.active && fileParse.directory ? `@${fileParse.directory}/` : "@/";

  const isBackgroundAgent = useMemo(
    () => availableAgents.find((a) => a.agentType === composerAgentType)?.background ?? false,
    [availableAgents, composerAgentType],
  );
  const showComposerAgentChip =
    composerAgentType !== "general-purpose" &&
    composerAgentType !== "auto" &&
    !isBackgroundAgent &&
    !highlightedSlashCommand &&
    !highlightedSkillCommand;
  const hasInlineComposerChips =
    Boolean(highlightedSlashCommand) ||
    Boolean(highlightedSkillCommand) ||
    showComposerAgentChip ||
    composerAttachedPaths.length > 0;

  const showBgRouting =
    Boolean(sessionId) &&
    !needsWorkspacePath &&
    backgroundTasks.some((task) => canSendFollowUpToTask(task.status)) &&
    typeof onFollowUpTaskIdChange === "function";

  const followUpTargets = useMemo(() => {
    const list = backgroundTasks.filter((task) => canSendFollowUpToTask(task.status));
    const totalByRole = new Map<string, number>();
    for (const task of list) {
      const role = normalizeAgentDisplayName(task.agent_type);
      totalByRole.set(role, (totalByRole.get(role) ?? 0) + 1);
    }
    const seenByRole = new Map<string, number>();
    return list.map((task) => {
      const role = normalizeAgentDisplayName(task.agent_type);
      const idx = (seenByRole.get(role) ?? 0) + 1;
      seenByRole.set(role, idx);
      const total = totalByRole.get(role) ?? 1;
      return {
        task,
        chipLabel: total > 1 ? `${role} #${idx}` : role,
        tooltip: `${role} · ${task.description.slice(0, 200)}${
          task.description.length > 200 ? "…" : ""
        }`,
      };
    });
  }, [backgroundTasks]);

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
            onClick={() => {
              onFollowUpTaskIdChange?.(null);
              onCloseBackgroundTranscript?.();
            }}
            sx={{ fontWeight: followUpTaskId ? 400 : 600 }}
          />
          {followUpTargets.map(({ task: t, chipLabel, tooltip }) => {
            const ok = canSendFollowUpToTask(t.status);
            const selected = followUpTaskId === t.task_id;
            return (
              <Stack
                key={t.task_id}
                direction="row"
                alignItems="center"
                spacing={0.25}
              >
                <Tooltip title={tooltip}>
                  <span>
                    <Chip
                      size="small"
                      icon={<SmartToy sx={{ fontSize: 16 }} />}
                      label={chipLabel}
                      color={selected ? "secondary" : "default"}
                      variant={selected ? "filled" : "outlined"}
                      disabled={!ok}
                      onClick={() => {
                        if (!ok) return;
                        onFollowUpTaskIdChange?.(t.task_id);
                        onOpenBackgroundTranscript?.(t.task_id);
                      }}
                      sx={{
                        maxWidth: 156,
                        "& .MuiChip-label": {
                          overflow: "hidden",
                          textOverflow: "ellipsis",
                        },
                      }}
                    />
                  </span>
                </Tooltip>
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
        {askUserQuestion ? (
          <AskUserQuestionWizard
            variant="composer"
            resetKey={askUserQuestion.resetKey}
            questions={askUserQuestion.questions}
            selections={askUserQuestion.selections}
            onSelectionsChange={askUserQuestion.onSelectionsChange}
            onSubmit={askUserQuestion.onSubmit}
          />
        ) : null}
        <PermissionPromptBar />
        <Box
          ref={composerInputAnchorRef}
          onMouseDown={(event) => {
            if (event.target !== event.currentTarget || inputDisabled) return;
            event.preventDefault();
            focusEditableEnd();
          }}
          sx={{
            position: "relative",
            display: "block",
            minHeight: 56,
            maxHeight: 280,
            overflowY: "auto",
            boxSizing: "content-box",
            cursor: inputDisabled ? "not-allowed" : "text",
            px: 1.75,
            py: 1.15,
            /* 与输入首行一致，用于 Agent Chip 与光标垂直对齐 */
            "--composer-fs": `${COMPOSER_FS_PX}px`,
            "--composer-lh": COMPOSER_LH,
            "--composer-chip-h": `${COMPOSER_INLINE_CHIP_PX}px`,
            fontSize: "var(--composer-fs)",
            lineHeight: "var(--composer-lh)",
          }}
          >
            {hasInlineComposerChips ? (
              <Box
                component="span"
                sx={{
                  verticalAlign: "middle",
                  display: "inline-flex",
                  alignItems: "center",
                  gap: 0.75,
                  flexWrap: "wrap",
                  maxWidth: "100%",
                  mr: 0.9,
                  rowGap: 0.45,
                  pointerEvents: "auto",
                }}
              >
            {highlightedSlashCommand ? (
            <Tooltip
              placement="top"
              enterDelay={180}
              title={highlightedSlashCommand.description}
            >
              <Box
                component="span"
                sx={{
                  display: "inline-flex",
                  alignItems: "center",
                  alignSelf: "center",
                  flexShrink: 0,
                  height: "var(--composer-chip-h)",
                  fontSize: "var(--composer-fs)",
                  lineHeight: "var(--composer-lh)",
                }}
              >
                <Chip
                  className="composer-command-chip"
                  size="small"
                  variant="outlined"
                  icon={<RouteIcon sx={{ fontSize: 16 }} />}
                  label={highlightedSlashCommand.label}
                  sx={{
                    ...semanticChipSurface(commandTone),
                    flexShrink: 0,
                    height: "var(--composer-chip-h)",
                    maxHeight: "var(--composer-chip-h)",
                    fontWeight: 700,
                    maxWidth: { xs: 160, sm: 220 },
                  }}
                />
              </Box>
            </Tooltip>
          ) : null}
          {highlightedSkillCommand ? (
            <Tooltip
              placement="top"
              enterDelay={180}
              title={
                highlightedSkillCommand.description ||
                "直接使用该 Skill"
              }
            >
              <Box
                component="span"
                sx={{
                  display: "inline-flex",
                  alignItems: "center",
                  alignSelf: "center",
                  flexShrink: 0,
                  height: "var(--composer-chip-h)",
                  fontSize: "var(--composer-fs)",
                  lineHeight: "var(--composer-lh)",
                }}
              >
                <Chip
                  className="composer-skill-chip"
                  size="small"
                  variant="outlined"
                  icon={<AutoAwesome sx={{ fontSize: 16 }} />}
                  label={`$${highlightedSkillCommand.name}`}
                  sx={{
                    ...semanticChipSurface(skillTone),
                    flexShrink: 0,
                    height: "var(--composer-chip-h)",
                    maxHeight: "var(--composer-chip-h)",
                    fontWeight: 700,
                    maxWidth: { xs: 170, sm: 240 },
                  }}
                />
              </Box>
            </Tooltip>
          ) : null}
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
                      {normalizeAgentDisplayName(composerAgentType)}
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
                  normalizeAgentDisplayName(composerAgentType)
                )
              }
            >
              <Box
                component="span"
                sx={{
                  display: "inline-flex",
                  alignItems: "center",
                  alignSelf: "center",
                  flexShrink: 0,
                  height: "var(--composer-chip-h)",
                  fontSize: "var(--composer-fs)",
                  lineHeight: "var(--composer-lh)",
                }}
              >
                <Chip
                  className="composer-agent-chip"
                  size="small"
                  variant="outlined"
                  icon={<SmartToy sx={{ fontSize: 16 }} />}
                  label={normalizeAgentDisplayName(composerAgentType)}
                  sx={{
                    ...semanticChipSurface(agentTone),
                    flexShrink: 0,
                    height: "var(--composer-chip-h)",
                    maxHeight: "var(--composer-chip-h)",
                    fontWeight: 700,
                    maxWidth: { xs: 140, sm: 220 },
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
                alignContent: "center",
                alignSelf: "center",
                gap: 0.25,
                maxWidth: { xs: "100%", sm: 420 },
                minHeight: "var(--composer-chip-h)",
              }}
            >
              {composerAttachedPaths.map((p) => (
                <Tooltip key={p} title={p} placement="top">
                  <Chip
                    className="composer-file-chip"
                    size="small"
                    variant="outlined"
                    icon={
                      <InsertDriveFile sx={{ fontSize: 16 }} />
                    }
                    label={`@${p}`}
                    sx={{
                      ...semanticChipSurface(fileTone),
                      flexShrink: 0,
                      height: "var(--composer-chip-h)",
                      maxHeight: "var(--composer-chip-h)",
                      maxWidth: 200,
                      fontWeight: 600,
                    }}
                  />
                </Tooltip>
                ))}
              </Box>
            ) : null}
              </Box>
            ) : null}
          <Box
            component="span"
            ref={mergedEditableRef}
            contentEditable={!inputDisabled}
            suppressContentEditableWarning
            role="textbox"
            aria-label="消息输入"
            aria-multiline="true"
            aria-autocomplete="list"
            aria-disabled={inputDisabled || undefined}
            aria-expanded={
              (showSlashPopover && filteredSlashOptions.length > 0) ||
              (showFilePopover &&
                (fileGlobLoading || filteredFilePaths.length > 0))
            }
            data-placeholder={!hasInlineComposerChips ? placeholder : ""}
            data-empty={input === "" ? "true" : undefined}
            onMouseDown={(e) => {
              if (
                inputDisabled ||
                normalizeEditableText(e.currentTarget.textContent ?? "") !== ""
              ) {
                return;
              }
              e.preventDefault();
              focusEditableEnd();
            }}
            onInput={(e) => {
              const rawValue = e.currentTarget.textContent ?? "";
              const nativeEvent = e.nativeEvent as InputEvent;
              const update = getEditableInputUpdate(
                rawValue,
                editableCompositionRef.current ||
                  nativeEvent.isComposing === true,
              );
              if (update.shouldNormalizeDom) {
                setEditableDomText(e.currentTarget, update.nextValue, true);
                setEditableTextSelection(e.currentTarget, update.nextValue.length);
              }
              if (update.shouldCommit) {
                commitEditableInput(update.nextValue);
              }
            }}
            onCompositionStart={() => {
              editableCompositionRef.current = true;
            }}
            onCompositionEnd={(e) => {
              editableCompositionRef.current = false;
              const rawValue = e.currentTarget.textContent ?? "";
              const update = getEditableInputUpdate(rawValue, false);
              if (update.shouldNormalizeDom) {
                setEditableDomText(e.currentTarget, update.nextValue, true);
                setEditableTextSelection(e.currentTarget, update.nextValue.length);
              }
              commitEditableInput(update.nextValue);
            }}
            onPaste={(e) => {
              e.preventDefault();
              const text = e.clipboardData.getData("text/plain");
              const el = editableRef.current;
              if (!el) return;
              const nextValue = insertTextAtEditableSelection(el, text);
              commitEditableInput(nextValue);
            }}
            onFocus={() => {
              if (slashParse.active || skillParse.active) {
                setSlashPickerDismissed(false);
              }
              if (fileParse.active) setFilePickerDismissed(false);
              if (
                normalizeEditableText(editableRef.current?.textContent ?? "") ===
                ""
              ) {
                queueMicrotask(() => {
                  const el = editableRef.current;
                  if (el && el.ownerDocument.activeElement === el) {
                    focusEditableEnd();
                  }
                });
              }
            }}
            onBlur={(e) => {
              if (
                normalizeEditableText(e.currentTarget.textContent ?? "") === ""
              ) {
                setEditableDomText(e.currentTarget, "", false);
              }
            }}
            onKeyDown={handleComposerKeyDown}
            sx={{
              display: input === "" ? "inline-block" : "inline",
              verticalAlign: "middle",
              minWidth:
                input === ""
                  ? hasInlineComposerChips
                    ? "1ch"
                    : "100%"
                  : hasInlineComposerChips
                    ? 2
                    : "100%",
              boxSizing: "border-box",
              border: "none",
              outline: "none",
              whiteSpace: "pre-wrap",
              overflowWrap: "anywhere",
              wordBreak: "break-word",
              fontSize: "var(--composer-fs)",
              fontFamily: "inherit",
              lineHeight: "var(--composer-lh)",
              letterSpacing: "-0.01em",
              color: inputDisabled ? alpha(ink, 0.38) : ink,
              caretColor: accent,
              cursor: inputDisabled ? "not-allowed" : "text",
              transition: "color 0.15s ease",
              "&[data-empty='true']:not(:focus)::before": {
                content: "attr(data-placeholder)",
                color: alpha(mut, 0.65),
                pointerEvents: "none",
              },
            }}
          />
          <Popover
            open={showSlashPopover}
            anchorEl={composerInputAnchorRef.current}
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
                  width: composerPopoverWidth ?? 320,
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
              {filteredSlashOptions.length === 0 ? (
                <ListItemButton disabled>
                  <ListItemText
                    primary={
                      skillParse.active ? "无匹配 Skill" : "无匹配命令或 Agent"
                    }
                    secondary={
                      skillParse.active
                        ? "输入 $skill 参数，或按 Esc 取消"
                        : "继续输入或按 Esc 取消"
                    }
                  />
                </ListItemButton>
              ) : (
                filteredSlashOptions.map((item, i) => (
                  <Tooltip
                    key={
                      item.kind === "command"
                        ? `cmd-${item.command.id}`
                        : item.kind === "agent"
                          ? `agent-${item.agent.agentType}`
                          : `skill-${item.skill.name}`
                    }
                    title={
                      item.kind === "command"
                        ? item.command.description
                        : item.kind === "agent"
                          ? item.agent.description
                          : item.skill.description
                    }
                    placement="right"
                    enterDelay={200}
                  >
                    <ListItemButton
                      data-slash-index={i}
                      selected={i === slashHighlightIndex}
                      onClick={() => {
                        if (item.kind === "command") {
                          pickWorkflowCommand(item.command.id);
                        } else if (item.kind === "agent") {
                          pickAtAgent(item.agent.agentType);
                        } else {
                          pickSkillCommand(item.skill.name);
                        }
                      }}
                    >
                      <ListItemIcon
                        sx={{
                          minWidth: 36,
                          color:
                            item.kind === "command"
                              ? commandTone
                              : item.kind === "agent"
                                ? agentTone
                                : skillTone,
                        }}
                      >
                        {item.kind === "command" ? (
                          <RouteIcon fontSize="small" />
                        ) : item.kind === "agent" ? (
                          <SmartToy fontSize="small" />
                        ) : (
                          <AutoAwesome fontSize="small" />
                        )}
                      </ListItemIcon>
                      <ListItemText
                        sx={{ minWidth: 0 }}
                        primary={
                          item.kind === "command"
                            ? item.command.label
                            : item.kind === "agent"
                              ? normalizeAgentDisplayName(item.agent.agentType)
                              : `$${item.skill.name}`
                        }
                        secondary={
                          item.kind === "command"
                            ? item.command.description
                            : item.kind === "agent"
                              ? "设置当前输入框角色"
                              : item.skill.description || "直接使用该 Skill"
                        }
                        primaryTypographyProps={{
                          sx: {
                            color:
                              item.kind === "command"
                                ? commandTone
                                : item.kind === "agent"
                                  ? agentTone
                                  : skillTone,
                            fontWeight: 700,
                          },
                        }}
                        secondaryTypographyProps={{
                          sx: {
                            color: alpha(
                              item.kind === "command"
                                ? commandTone
                                : item.kind === "agent"
                                  ? agentTone
                                  : skillTone,
                              0.76,
                            ),
                            ...(item.kind === "skill"
                              ? {
                                  display: "block",
                                  overflow: "hidden",
                                  textOverflow: "ellipsis",
                                  whiteSpace: "nowrap",
                                }
                              : null),
                          },
                        }}
                      />
                    </ListItemButton>
                  </Tooltip>
                ))
              )}
            </List>
          </Popover>
          <Popover
            open={showFilePopover}
            anchorEl={composerInputAnchorRef.current}
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
                  width: composerPopoverWidth ?? 380,
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
              <Stack direction="row" alignItems="center" spacing={0.75}>
                <Typography
                  variant="caption"
                  sx={{
                    fontWeight: 700,
                    letterSpacing: "0.04em",
                    textTransform: "uppercase",
                    color: pen.textHeader,
                    fontSize: 10,
                    flex: 1,
                    minWidth: 0,
                  }}
                >
                  工作区文件
                </Typography>
                {fileParse.directory ? (
                  <Button
                    size="small"
                    variant="text"
                    onClick={goUpFileDirectory}
                    sx={{
                      minWidth: 0,
                      px: 0.75,
                      py: 0,
                      fontSize: 11,
                      color: pen.textPath,
                    }}
                  >
                    上一级
                  </Button>
                ) : null}
              </Stack>
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
                当前 {filePickerDirectoryLabel} · Enter 进入文件夹 · 选择文件会注入精确路径
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
                    secondary={filePickerDirectoryLabel}
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
                    secondary={`${filePickerDirectoryLabel} · 继续输入或按 Esc 取消`}
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
                    alignItems="center"
                    onClick={() => {
                      if (row.is_file) {
                        pickFilePath(row.path);
                      } else {
                        enterFileDirectory(row.path);
                      }
                    }}
                  >
                    <ListItemIcon
                      sx={{
                        minWidth: 34,
                        mr: 0.25,
                        py: 0,
                        alignSelf: "center",
                      }}
                    >
                      <ComposerFilePickerRowIcon
                        path={row.path}
                        isFile={row.is_file}
                      />
                    </ListItemIcon>
                    <ListItemText
                      primary={normalizeFsPath(row.path)}
                      secondary={
                        row.is_file
                          ? formatBytesShort(row.size)
                          : "文件夹 · 点击进入"
                      }
                      sx={{ m: 0 }}
                      primaryTypographyProps={{
                        sx: {
                          fontFamily:
                            "ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace",
                          fontSize: 12,
                          fontWeight: 500,
                          color: row.is_file ? fileTone : pen.textFilename,
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
                    {row.is_file ? null : (
                      <Stack
                        direction="row"
                        alignItems="center"
                        spacing={0.5}
                        sx={{ ml: 0.75, flexShrink: 0 }}
                      >
                        <Button
                          size="small"
                          variant="outlined"
                          onClick={(event) => {
                            event.stopPropagation();
                            pickFilePath(row.path);
                          }}
                          sx={{
                            minWidth: 0,
                            px: 0.75,
                            py: 0,
                            fontSize: 11,
                            lineHeight: 1.6,
                          }}
                        >
                          选择
                        </Button>
                        <ChevronRight
                          size={16}
                          strokeWidth={2}
                          color={pen.textPath}
                        />
                      </Stack>
                    )}
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
                  bgcolor: alpha(accent, 0.3),
                  color: accent,
                  borderColor: alpha(accent, 0.26),
                  boxShadow: `0 2px 10px ${alpha(accent, 0.12)}`,
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
                  color: permissionAccent,
                  lineHeight: 0,
                  "& svg": { display: "block" },
                }}
              >
                {createElement(PERMISSION_ICON[permissionMode], {
                  size: 18,
                  strokeWidth: 2,
                  color: permissionAccent,
                })}
              </Box>
            }
            endIcon={
              <Box
                component="span"
                sx={{
                  display: "inline-flex",
                  alignItems: "center",
                  color: permissionAccent,
                  lineHeight: 0,
                  "& svg": { display: "block" },
                }}
              >
                <ChevronDown
                  size={18}
                  strokeWidth={2}
                  color={permissionAccent}
                />
              </Box>
            }
            sx={{
              textTransform: "none",
              color: permissionAccent,
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
                "background-color 0.2s ease, border-color 0.2s ease, box-shadow 0.2s ease, color 0.2s ease",
              "@media (prefers-reduced-motion: reduce)": {
                transition: "none",
              },
              "&:hover": {
                bgcolor: alpha(permissionAccent, 0.12),
                borderColor: alpha(permissionAccent, 0.28),
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
            {(Object.keys(PERMISSION_META) as PermissionMode[]).map((key) => {
              const rowAccent = permissionModeAccent(theme, key);
              return (
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
                    sx={{
                      "&.Mui-selected": {
                        bgcolor: alpha(rowAccent, isDark ? 0.18 : 0.12),
                        "&:hover": {
                          bgcolor: alpha(rowAccent, isDark ? 0.26 : 0.16),
                        },
                      },
                    }}
                  >
                    <ListItemIcon
                      sx={{
                        minWidth: 40,
                        lineHeight: 0,
                        color: rowAccent,
                        "& svg": { display: "block" },
                      }}
                    >
                      {createElement(PERMISSION_ICON[key], {
                        size: 20,
                        strokeWidth: 2,
                        color: rowAccent,
                      })}
                    </ListItemIcon>
                    <ListItemText
                      primary={PERMISSION_META[key].label}
                      primaryTypographyProps={{
                        sx: { ...composerLabelText, color: rowAccent },
                      }}
                    />
                  </MenuItem>
                </Tooltip>
              );
            })}
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
                  borderColor: alpha(accent, 0.42),
                  bgcolor: isDark ? alpha(paper, 0.4) : alpha(accent, 0.3),
                  boxShadow: `0 2px 12px ${alpha(accent, 0.14)}, 0 0 0 1px ${alpha(accent, 0.16)}`,
                  transform: "translateY(-1px)",
                },
                "& .MuiChip-root": {
                  maxWidth: 100,
                },
              }}
            />
            {agentTurnActive && onCancel ? (
              <Tooltip title="取消任务">
                <IconButton
                  size="small"
                  onClick={onCancel}
                  aria-label="取消任务"
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
          {/* 执行环境选择器 */}
          <Button
            size="small"
            variant="text"
            color="inherit"
            onClick={(e) => setEnvAnchor(e.currentTarget)}
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
                  <Laptop size={16} strokeWidth={2} color={accent} />
                ) : environment === "ssh" ? (
                  <Terminal size={16} strokeWidth={2} color={accent} />
                ) : (
                  createElement(SANDBOX_BACKEND_ICON[sandboxBackend], {
                    size: 16,
                    strokeWidth: 2,
                    color: accent,
                  })
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
                <ChevronDown size={16} strokeWidth={2} color={accent} />
              </Box>
            }
            sx={{
              textTransform: "none",
              color: accent,
              ...composerLabelText,
              borderRadius: 2.5,
              px: 1,
              py: 0,
              minHeight: "var(--composer-footer-h)",
              height: "var(--composer-footer-h)",
              border: "1px solid transparent",
              bgcolor: "transparent",
              boxShadow: "none",
              gap: 0.5,
              transition:
                "background-color 0.2s ease, border-color 0.2s ease, box-shadow 0.2s ease",
              "@media (prefers-reduced-motion: reduce)": {
                transition: "none",
              },
              "&:hover": {
                bgcolor: alpha(accent, 0.12),
                borderColor: alpha(accent, 0.28),
                boxShadow: "none",
              },
              "& .MuiButton-startIcon": { marginRight: 0 },
              "& .MuiButton-endIcon": { marginLeft: 0 },
            }}
          >
            <Typography
              component="span"
              noWrap
              sx={{ ...composerLabelText, color: "inherit" }}
            >
              {environment === "local"
                ? localVenvType !== "none" && localVenvName
                  ? `本地·${localVenvName}`
                  : "本地"
                : environment === "ssh"
                  ? sshServer
                    ? `SSH·${sshServer}`
                    : "SSH"
                  : `沙箱·${SANDBOX_LABEL[sandboxBackend]}`}
            </Typography>
          </Button>

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
              display: "inline-flex",
              alignItems: "center",
              justifyContent: "flex-start",
              maxWidth: { xs: "100%", sm: 240 },
              borderRadius: 2.5,
              px: 1,
              py: 0,
              minHeight: "var(--composer-footer-h)",
              height: "var(--composer-footer-h)",
              "& .MuiButton-startIcon": {
                marginRight: 1,
                marginLeft: 0,
                display: "inline-flex",
                alignItems: "center",
                alignSelf: "center",
              },
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
                  ? alpha(warningMain, 0.3)
                  : alpha(accent, 0.3),
                borderColor: needsWorkspacePath
                  ? alpha(warningMain, 0.48)
                  : alpha(accent, 0.22),
              },
            }}
          >
            <Typography
              variant="body2"
              noWrap
              component="span"
              sx={{
                ...composerLabelText,
                color: "inherit",
                display: "inline-flex",
                alignItems: "center",
                lineHeight: 1.25,
              }}
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
                bgcolor: alpha(accent, 0.3),
                borderColor: alpha(accent, 0.24),
              },
            }}
          />

          <Menu
            anchorEl={envAnchor}
            open={Boolean(envAnchor)}
            onClose={() => {
              clearSshSubmenuLeaveTimer();
              clearSandboxSubmenuLeaveTimer();
              setEnvAnchor(null);
              setSandboxMenuAnchor(null);
              setSshMenuAnchor(null);
            }}
            slotProps={{
              paper: {
                sx: {
                  minWidth: 220,
                  overflow: "visible",
                },
              },
            }}
          >
            {/* 本地环境 */}
            <MenuItem
              selected={environment === "local"}
              onMouseEnter={(e) => {
                loadLocalVenvs(workspacePath);
                openVenvSub(e.currentTarget);
              }}
              onMouseLeave={scheduleCloseVenvSub}
              onClick={() => {
                setEnvironment("local");
                setLocalVenv("none", "");
                setEnvAnchor(null);
                setSshMenuAnchor(null);
                setSandboxMenuAnchor(null);
                setVenvMenuAnchor(null);
              }}
              sx={{
                pr: 0.75,
                display: "flex",
                alignItems: "center",
                justifyContent: "space-between",
                gap: 0.5,
              }}
            >
              <Stack direction="row" alignItems="center" sx={{ minWidth: 0 }}>
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
                  secondary={
                    localVenvType !== "none"
                      ? `${localVenvType}: ${localVenvName}`
                      : "在本机运行工具与终端"
                  }
                  primaryTypographyProps={{
                    sx: { ...composerLabelText, color: ink },
                  }}
                  secondaryTypographyProps={{
                    sx: { fontSize: 11, color: mut },
                  }}
                />
              </Stack>
              <ChevronRight size={18} strokeWidth={2} color={accent} />
            </MenuItem>

            <Divider />

            {/* SSH - 带二级菜单显示可用服务器 */}
            <MenuItem
              selected={environment === "ssh"}
              onMouseEnter={(e) => openSshSub(e.currentTarget)}
              onMouseLeave={scheduleCloseSshSub}
              onClick={() => {
                // 如果没有配置 SSH 服务器，保持菜单打开
                if (Object.keys(sshServers).length === 0) return;
                // 选择第一个可用的服务器
                const firstServer = Object.keys(sshServers)[0];
                if (firstServer) {
                  setSshServer(firstServer);
                  setEnvironment("ssh");
                  setEnvAnchor(null);
                  setSshMenuAnchor(null);
                }
              }}
              sx={{
                pr: 0.75,
                display: "flex",
                alignItems: "center",
                justifyContent: "space-between",
                gap: 0.5,
              }}
            >
              <Stack direction="row" alignItems="center" sx={{ minWidth: 0 }}>
                <ListItemIcon
                  sx={{
                    minWidth: 40,
                    lineHeight: 0,
                    "& svg": { display: "block" },
                  }}
                >
                  <Terminal size={20} strokeWidth={2} color={accent} />
                </ListItemIcon>
                <ListItemText
                  primary="SSH"
                  secondary={
                    Object.keys(sshServers).length > 0
                      ? `${Object.keys(sshServers).length} 个可用服务器`
                      : "点击配置 SSH 连接"
                  }
                  primaryTypographyProps={{
                    sx: { ...composerLabelText, color: ink },
                  }}
                  secondaryTypographyProps={{
                    sx: { fontSize: 11, color: mut },
                  }}
                />
              </Stack>
              <ChevronRight size={18} strokeWidth={2} color={accent} />
            </MenuItem>

            {/* 沙箱 - 带二级菜单显示可用后端 */}
            <MenuItem
              selected={environment === "sandbox"}
              onMouseEnter={(e) => openSandboxSub(e.currentTarget)}
              onMouseLeave={scheduleCloseSandboxSub}
              sx={{
                pr: 0.75,
                display: "flex",
                alignItems: "center",
                justifyContent: "space-between",
                gap: 0.5,
              }}
            >
              <Stack direction="row" alignItems="center" sx={{ minWidth: 0 }}>
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
                  primary="沙箱"
                  secondary="远程容器化执行环境"
                  primaryTypographyProps={{
                    sx: { ...composerLabelText, color: ink },
                  }}
                  secondaryTypographyProps={{
                    sx: { fontSize: 11, color: mut },
                  }}
                />
              </Stack>
              <ChevronRight size={18} strokeWidth={2} color={accent} />
            </MenuItem>
          </Menu>

          {/* SSH 二级菜单 — 与 SessionList Language 相同：嵌套 Menu + pointerEvents，避免 Popover/Modal 抢事件 */}
          <Menu
            anchorEl={sshMenuAnchor}
            open={Boolean(sshMenuAnchor)}
            onClose={() => setSshMenuAnchor(null)}
            anchorOrigin={{ vertical: "top", horizontal: "right" }}
            transformOrigin={{ vertical: "top", horizontal: "left" }}
            disableAutoFocus
            sx={{
              pointerEvents: "none",
              zIndex: (t) => t.zIndex.modal + 2,
            }}
            slotProps={{
              paper: {
                sx: {
                  pointerEvents: "auto",
                  minWidth: 200,
                  maxWidth: 280,
                  mt: 0.5,
                  ml: -1.25,
                  boxShadow: (th) => th.shadows[8],
                },
              },
            }}
            MenuListProps={{
              dense: true,
              sx: { py: 0.5 },
              onMouseEnter: clearSshSubmenuLeaveTimer,
              onMouseLeave: () => setSshMenuAnchor(null),
            }}
          >
            {sshServersLoading ? (
              <MenuItem disabled sx={{ py: 1 }}>
                <ListItemIcon sx={{ minWidth: 36 }}>
                  <CircularProgress size={16} sx={{ color: mut }} />
                </ListItemIcon>
                <ListItemText
                  primary="加载中..."
                  primaryTypographyProps={{ sx: { fontSize: 13, color: mut } }}
                />
              </MenuItem>
            ) : Object.keys(sshServers).length === 0 ? (
              <>
                <MenuItem disabled>
                  <ListItemText
                    primary="未配置 SSH 服务器"
                    secondary="请在设置中添加 SSH 配置"
                    primaryTypographyProps={{
                      sx: { fontSize: 13, color: mut },
                    }}
                    secondaryTypographyProps={{ sx: { fontSize: 11 } }}
                  />
                </MenuItem>
                <Divider sx={{ my: 0.5 }} />
                <MenuItem
                  onClick={() => {
                    openExecutionSettings(2);
                    setSshMenuAnchor(null);
                    setEnvAnchor(null);
                  }}
                >
                  <ListItemIcon sx={{ minWidth: 36 }}>
                    <Plus size={18} strokeWidth={2} color={accent} />
                  </ListItemIcon>
                  <ListItemText
                    primary="添加 SSH 配置"
                    primaryTypographyProps={{
                      sx: { fontSize: 13, color: ink, fontWeight: 500 },
                    }}
                  />
                </MenuItem>
              </>
            ) : (
              <>
                {Object.entries(sshServers).map(([name, cfg]) => (
                  <MenuItem
                    key={name}
                    selected={environment === "ssh" && sshServer === name}
                    onClick={() => {
                      setSshServer(name);
                      setEnvironment("ssh");
                      setSshMenuAnchor(null);
                      setEnvAnchor(null);
                    }}
                    sx={{ px: 1.5, py: 0.75 }}
                  >
                    <ListItemIcon sx={{ minWidth: 32 }}>
                      <Server size={16} strokeWidth={2} color={accent} />
                    </ListItemIcon>
                    <ListItemText
                      primary={name}
                      secondary={sshConnectionLabel(cfg)}
                      primaryTypographyProps={{
                        sx: { fontSize: 13, fontWeight: 500, color: ink },
                      }}
                      secondaryTypographyProps={{
                        sx: { fontSize: 11, color: mut },
                      }}
                    />
                  </MenuItem>
                ))}
                <Divider sx={{ my: 0.5 }} />
                <MenuItem
                  onClick={() => {
                    openExecutionSettings(2);
                    setSshMenuAnchor(null);
                    setEnvAnchor(null);
                  }}
                >
                  <ListItemIcon sx={{ minWidth: 36 }}>
                    <Plus size={18} strokeWidth={2} color={mut} />
                  </ListItemIcon>
                  <ListItemText
                    primary="管理 SSH 配置"
                    primaryTypographyProps={{
                      sx: { fontSize: 13, color: mut },
                    }}
                  />
                </MenuItem>
              </>
            )}
          </Menu>

          {/* 沙箱二级菜单 — 同上 */}
          <Menu
            anchorEl={sandboxMenuAnchor}
            open={Boolean(sandboxMenuAnchor)}
            onClose={() => setSandboxMenuAnchor(null)}
            anchorOrigin={{ vertical: "top", horizontal: "right" }}
            transformOrigin={{ vertical: "top", horizontal: "left" }}
            disableAutoFocus
            sx={{
              pointerEvents: "none",
              zIndex: (t) => t.zIndex.modal + 2,
            }}
            slotProps={{
              paper: {
                sx: {
                  pointerEvents: "auto",
                  minWidth: 160,
                  mt: 0.5,
                  ml: -1.25,
                  boxShadow: (th) => th.shadows[8],
                },
              },
            }}
            MenuListProps={{
              dense: true,
              sx: { py: 0.5 },
              onMouseEnter: clearSandboxSubmenuLeaveTimer,
              onMouseLeave: () => setSandboxMenuAnchor(null),
            }}
          >
            {SANDBOX_BACKENDS.map((b) => {
              const BackendIcon = SANDBOX_BACKEND_ICON[b.id];
              return (
                <MenuItem
                  key={b.id}
                  selected={
                    environment === "sandbox" && sandboxBackend === b.id
                  }
                  onClick={() => {
                    setSandboxBackend(b.id);
                    setEnvironment("sandbox");
                    setSandboxMenuAnchor(null);
                    setEnvAnchor(null);
                  }}
                  sx={{ px: 1.5, py: 0.75 }}
                >
                  <ListItemIcon sx={{ minWidth: 32 }}>
                    <BackendIcon size={16} strokeWidth={2} color={accent} />
                  </ListItemIcon>
                  <ListItemText
                    primary={b.label}
                    primaryTypographyProps={{
                      sx: { fontSize: 13, fontWeight: 500, color: ink },
                    }}
                  />
                </MenuItem>
              );
            })}
            <Divider sx={{ my: 0.5 }} />
            <MenuItem
              onClick={() => {
                openExecutionSettings(0);
                setSandboxMenuAnchor(null);
                setEnvAnchor(null);
              }}
              sx={{ px: 1.5, py: 0.75 }}
            >
              <ListItemIcon sx={{ minWidth: 36 }}>
                <Settings size={18} strokeWidth={2} color={mut} />
              </ListItemIcon>
              <ListItemText
                primary="配置沙箱后端"
                primaryTypographyProps={{
                  sx: { fontSize: 13, color: mut, fontWeight: 500 },
                }}
              />
            </MenuItem>
          </Menu>

          {/* 本地虚拟环境二级菜单 */}
          <Menu
            anchorEl={venvMenuAnchor}
            open={Boolean(venvMenuAnchor)}
            onClose={() => setVenvMenuAnchor(null)}
            anchorOrigin={{ vertical: "top", horizontal: "right" }}
            transformOrigin={{ vertical: "top", horizontal: "left" }}
            disableAutoFocus
            sx={{
              pointerEvents: "none",
              zIndex: (t) => t.zIndex.modal + 2,
            }}
            slotProps={{
              paper: {
                sx: {
                  pointerEvents: "auto",
                  minWidth: 200,
                  maxWidth: 300,
                  mt: 0.5,
                  ml: -1.25,
                  boxShadow: (th) => th.shadows[8],
                },
              },
            }}
            MenuListProps={{
              dense: true,
              sx: { py: 0.5 },
              onMouseEnter: clearVenvSubmenuLeaveTimer,
              onMouseLeave: () => setVenvMenuAnchor(null),
            }}
          >
            {/* 无虚拟环境选项 */}
            <MenuItem
              selected={environment === "local" && localVenvType === "none"}
              onClick={() => {
                setEnvironment("local");
                setLocalVenv("none", "");
                setVenvMenuAnchor(null);
                setEnvAnchor(null);
              }}
              sx={{ px: 1.5, py: 0.75 }}
            >
              <ListItemIcon sx={{ minWidth: 32 }}>
                <Laptop size={16} strokeWidth={2} color={accent} />
              </ListItemIcon>
              <ListItemText
                primary="无虚拟环境"
                primaryTypographyProps={{
                  sx: { fontSize: 13, fontWeight: 500, color: ink },
                }}
              />
            </MenuItem>
            {localVenvsLoading ? (
              <MenuItem disabled sx={{ py: 1 }}>
                <ListItemIcon sx={{ minWidth: 36 }}>
                  <CircularProgress size={16} sx={{ color: mut }} />
                </ListItemIcon>
                <ListItemText
                  primary="检测中..."
                  primaryTypographyProps={{ sx: { fontSize: 13, color: mut } }}
                />
              </MenuItem>
            ) : localVenvs.length === 0 ? (
              <MenuItem disabled>
                <ListItemText
                  primary="未检测到虚拟环境"
                  secondary="支持 conda、venv、pyenv"
                  primaryTypographyProps={{
                    sx: { fontSize: 13, color: mut },
                  }}
                  secondaryTypographyProps={{ sx: { fontSize: 11 } }}
                />
              </MenuItem>
            ) : (
              <>
                <Divider sx={{ my: 0.5 }} />
                {localVenvs.map((v) => (
                  <MenuItem
                    key={`${v.kind}:${v.name}`}
                    selected={
                      environment === "local" &&
                      localVenvType === (v.kind as LocalVenvType) &&
                      localVenvName === v.name
                    }
                    onClick={() => {
                      setEnvironment("local");
                      setLocalVenv(v.kind as LocalVenvType, v.name);
                      setVenvMenuAnchor(null);
                      setEnvAnchor(null);
                    }}
                    sx={{ px: 1.5, py: 0.75 }}
                  >
                    <ListItemIcon sx={{ minWidth: 32 }}>
                      <Atom size={16} strokeWidth={2} color={accent} />
                    </ListItemIcon>
                    <ListItemText
                      primary={v.label}
                      primaryTypographyProps={{
                        sx: { fontSize: 13, fontWeight: 500, color: ink },
                      }}
                    />
                  </MenuItem>
                ))}
              </>
            )}
          </Menu>
        </Stack>
      </Stack>
    </Stack>
  );
});
