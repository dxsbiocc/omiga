import { useState, useEffect, useRef, useMemo } from "react";
import type { CSSProperties } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import {
  Box,
  IconButton,
  Typography,
  Chip,
  Stack,
  Tooltip,
  Fade,
  useTheme,
  Collapse,
  Tabs,
  Tab,
  Alert,
  Button,
  Divider,
  Snackbar,
  CircularProgress,
} from "@mui/material";
import { alpha } from "@mui/material/styles";
import {
  SmartToy,
  Construction,
  CheckCircle,
  ExpandMore,
  ForumOutlined,
  FolderOpen,
  Article,
  Search as SearchIcon,
  Send as SendIcon,
  Terminal as TerminalIcon,
  Link as LinkIcon,
  Checklist as ChecklistIcon,
  TravelExplore as TravelExploreIcon,
  MenuBook as MenuBookIcon,
  Assignment as AssignmentIcon,
  Check as CheckIcon,
} from "@mui/icons-material";
import { Prism as SyntaxHighlighter } from "react-syntax-highlighter";
import {
  oneDark,
  oneLight,
} from "react-syntax-highlighter/dist/esm/styles/prism";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import {
  useSessionStore,
  useActivityStore,
  type Message as StoreMessage,
  isPlaceholderSessionTitle,
  titleFromFirstUserMessage,
  UNUSED_SESSION_LABEL,
  shouldShowNewSessionPlaceholder,
  isUnsetWorkspacePath,
} from "../../state";
import { Terminal } from "../Terminal";
import { ChatComposer } from "./ChatComposer";
import { getChatTokens } from "./chatTokens";
import { AgentSessionStatus } from "./AgentSessionStatus";
import { formatToolDisplayName } from "../../utils/executionSurfaceLabel";

interface ChatProps {
  sessionId: string;
}

interface Message {
  id: string;
  role: "user" | "assistant" | "tool";
  content: string;
  /** From DB: full assistant tool_calls — rebuild trace if tool rows are incomplete */
  toolCallsList?: Array<{ id: string; name: string; arguments: string }>;
  /** Assistant text streamed before the first tool in this round (shown inside tool block, not in final summary). */
  prefaceBeforeTools?: string;
  toolCall?: {
    id?: string;
    name: string;
    status?: "pending" | "running" | "completed" | "error";
    input?: string; // Tool input arguments (JSON)
    output?: string; // Tool execution result
  };
  timestamp?: number;
  roundId?: string;
  roundStatus?: "running" | "partial" | "cancelled" | "completed";
}

/** Persist full transcript (including tool rows) to the session store. */
function chatMessageToStore(m: Message): StoreMessage {
  return {
    id: m.id,
    role: m.role,
    content: m.content,
    prefaceBeforeTools: m.prefaceBeforeTools,
    toolCallsList: m.toolCallsList,
    toolCall: m.toolCall
      ? {
          id: m.toolCall.id ?? `tc-${m.id}`,
          name: m.toolCall.name,
          arguments: m.toolCall.input ?? "",
          output: m.toolCall.output,
          status: m.toolCall.status,
        }
      : undefined,
  };
}

// Backend StreamOutputItem types matching Rust
// Note: Backend uses #[serde(tag = "type", content = "data")] format
interface StreamOutputItem {
  type:
    | "Start"
    | "text"
    | "tool_use"
    | "tool_result"
    | "error"
    | "cancelled"
    | "complete";
  data?: unknown;
}

/** Matches `BackgroundShellCompletePayload` from the Tauri backend */
interface BackgroundShellCompletePayload {
  session_id: string;
  tool_use_id: string;
  task_id: string;
  output_path: string;
  exit_code: number;
  interrupted: boolean;
  description: string;
}

/** Avatar column + row gap; align with pencil-new.pen (36px avatar, ~10px gap). */
/** Chat bubble radius — smaller than pill-style so content stays inside the rounded rect */
const BUBBLE_RADIUS_PX = 10;
/** User message bubble max width (assistant uses full row width) */
const USER_BUBBLE_MAX_CSS = "min(960px, 100%)";
/** Markdown fenced code / blockquote — small px radius (avoid pill look on wide blocks) */
const MD_BLOCK_RADIUS_PX = 1;
/** Inline `code` longer than this: no fill (e.g. protein sequences) */
const INLINE_CODE_LONG_LEN = 80;

const PRISM_CODE_SEL = 'code[class*="language-"]';
const PRISM_PRE_SEL = 'pre[class*="language-"]';

/** Prism oneLight/oneDark set a fill on `code`/`pre`; we only want the outer chat box background. */
function prismStyleTransparentCodeSurface(
  style: Record<string, CSSProperties>,
): Record<string, CSSProperties> {
  return {
    ...style,
    [PRISM_CODE_SEL]: {
      ...(style[PRISM_CODE_SEL] ?? {}),
      background: "transparent",
      backgroundColor: "transparent",
    },
    [PRISM_PRE_SEL]: {
      ...(style[PRISM_PRE_SEL] ?? {}),
      background: "transparent",
      backgroundColor: "transparent",
    },
  };
}

function toolRowIcon(toolName: string) {
  const n = toolName.toLowerCase();
  if (n.includes("ask_user") || n.includes("askuserquestion"))
    return ForumOutlined;
  if (n === "Agent" || n === "Task") return SmartToy;
  if (
    n.includes("send_user_message") ||
    n.includes("sendusermessage") ||
    n.includes("brief")
  )
    return SendIcon;
  if (n.includes("todo_write") || n.includes("todowrite")) return ChecklistIcon;
  if (n.includes("notebook_edit") || n.includes("notebookedit"))
    return MenuBookIcon;
  if (n === "skill" || n === "skilltool") return MenuBookIcon;
  if (n.includes("web_search") || n.includes("websearch"))
    return TravelExploreIcon;
  if (n.includes("web_fetch") || n.includes("fetch")) return LinkIcon;
  if (n.includes("bash") || n.includes("shell")) return TerminalIcon;
  if (n.includes("glob") || n.includes("file")) return FolderOpen;
  if (n.includes("grep")) return SearchIcon;
  if (n.includes("toolsearch")) return SearchIcon;
  if (n.includes("exitplan") || n.includes("enterplan")) return MenuBookIcon;
  if (
    n === "taskcreate" ||
    n === "taskget" ||
    n === "tasklist" ||
    n === "taskupdate"
  )
    return AssignmentIcon;
  if (n.includes("taskstop") || n.includes("taskoutput")) return TerminalIcon;
  if (n.includes("read")) return Article;
  return Construction;
}

/**
 * Skip adding a tool row when the payload has no tool name / id and no usable input.
 * The backend sends `tool_use` twice: first with empty `arguments`, then with full JSON at block end.
 */
function isEmptyToolUsePayload(
  data: { id?: string; name?: string; arguments?: string } | undefined,
): boolean {
  if (!data) return true;
  const name = (data.name ?? "").trim();
  const id = (data.id ?? "").trim();
  const rawArgs = (data.arguments ?? "").trim();
  if (name.length > 0 || id.length > 0) return false;
  if (rawArgs.length === 0) return true;
  if (rawArgs === "{}" || rawArgs === "null") return true;
  return false;
}

/** When bash uses `run_in_background`, show a row in the activity panel. */
function tryParseBashBackground(
  input: string | undefined,
): { label: string } | null {
  if (!input?.trim()) return null;
  try {
    const j = JSON.parse(input) as {
      command?: string;
      run_in_background?: boolean;
      description?: string;
    };
    if (j.run_in_background !== true) return null;
    const label = (
      j.description?.trim() ||
      j.command?.trim() ||
      "后台命令"
    ).slice(0, 160);
    return { label };
  } catch {
    return null;
  }
}

/** Short label for the pencil-style execution step list (tool row). */
function humanizeToolStepTitle(name: string, args?: string): string {
  const n = name.toLowerCase();
  if (n.includes("bash")) {
    const bg = tryParseBashBackground(args);
    if (bg) {
      const t = bg.label;
      return t.length > 42 ? `${t.slice(0, 42)}…` : t;
    }
    try {
      const j = JSON.parse(args ?? "{}") as { command?: string };
      const c = j.command?.trim().slice(0, 48);
      if (c) return c.length >= 48 ? `${c}…` : `运行: ${c}`;
    } catch {
      /* fallthrough */
    }
    return "执行 bash";
  }
  if (n.includes("todo_write") || n.includes("todowrite"))
    return "更新任务清单";
  if (n.includes("web_search")) return "网络搜索";
  if (n.includes("web_fetch") || n.includes("fetch")) return "获取网页";
  if (n.includes("glob")) return "搜索文件";
  if (n.includes("grep")) return "代码搜索";
  if (n.includes("notebook")) return "编辑 Notebook";
  if (n.includes("file_read") || n === "read_file") return "读取文件";
  if (n.includes("file_write") || n.includes("write")) return "写入文件";
  if (n.includes("file_edit") || n.includes("edit")) return "编辑文件";
  if (n === "taskcreate") return "创建任务";
  if (n === "taskget") return "读取任务";
  if (n === "tasklist") return "任务列表";
  if (n === "taskupdate") return "更新任务";
  if (n === "skill" || n === "skilltool") return "加载技能";
  return name || "工具";
}

/** One-line title for collapsed accordion (sentence / path / command hint). */
function executionStepSummary(name: string, args?: string): string {
  const n = (name || "").toLowerCase();
  if (!args?.trim()) return humanizeToolStepTitle(name, args);
  try {
    const j = JSON.parse(args) as Record<string, unknown>;
    if (n.includes("bash")) {
      const desc = (j.description as string | undefined)?.trim();
      if (desc) return desc.length > 120 ? `${desc.slice(0, 120)}…` : desc;
      const cmd = (j.command as string | undefined)?.trim() ?? "";
      if (cmd) {
        const line = cmd.split("\n")[0].trim();
        return line.length > 100 ? `${line.slice(0, 100)}…` : line;
      }
    }
    if (n.includes("file_read") || n === "read_file") {
      const p = String(j.path ?? j.target_file ?? j.file_path ?? "");
      if (p.trim())
        return `Read file: ${p.split("/").filter(Boolean).slice(-3).join("/")}`;
    }
    if (
      n.includes("file_write") ||
      (n.includes("write") && n.includes("file"))
    ) {
      const p = String(j.path ?? j.file_path ?? "");
      if (p.trim())
        return `Write file: ${p.split("/").filter(Boolean).slice(-2).join("/")}`;
    }
    if (n.includes("grep")) {
      const pat = String(j.pattern ?? "");
      const path = String(j.path ?? "");
      if (pat || path) return `Search: ${pat || path}`.slice(0, 100);
    }
    if (n.includes("glob")) {
      const pat = String(j.glob_pattern ?? j.pattern ?? "");
      if (pat) return `Find files: ${pat}`;
    }
    if (n.includes("web_search")) {
      const q = String(j.query ?? "");
      if (q) return `Search web: ${q.slice(0, 80)}`;
    }
  } catch {
    /* fallthrough */
  }
  return humanizeToolStepTitle(name, args);
}

/**
 * One ReAct trace per user turn: intermediate assistant text + all tool calls live in a single
 * collapsible. Only the last assistant message after the last tool gets a horizontal divider above it.
 */
type RenderMsgItem =
  | { kind: "row"; message: Message; dividerBefore?: boolean }
  | { kind: "react_fold"; id: string; fold: Message[] };

function groupMessagesForRender(messages: Message[]): RenderMsgItem[] {
  const out: RenderMsgItem[] = [];
  let i = 0;
  while (i < messages.length) {
    const m = messages[i];
    if (m.role === "user") {
      out.push({ kind: "row", message: m });
      i++;
      continue;
    }
    const segStart = i;
    let j = i;
    while (j < messages.length && messages[j].role !== "user") j++;
    const segment = messages.slice(segStart, j);
    i = j;

    let lastToolIdx = -1;
    for (let k = 0; k < segment.length; k++) {
      if (segment[k].role === "tool" && segment[k].toolCall) lastToolIdx = k;
    }

    if (lastToolIdx < 0) {
      const hasPersistedToolPlan = segment.some(
        (m) =>
          m.role === "assistant" &&
          m.toolCallsList &&
          m.toolCallsList.length > 0,
      );
      if (hasPersistedToolPlan) {
        const fold: Message[] = [];
        const consumedToolIdx = new Set<number>();
        for (let si = 0; si < segment.length; si++) {
          const m = segment[si];
          if (m.role === "assistant") {
            fold.push(m);
            const list = m.toolCallsList;
            if (list?.length) {
              for (const tc of list) {
                const rawIdx = segment.findIndex(
                  (x, i) =>
                    i !== si &&
                    x.role === "tool" &&
                    x.toolCall?.id === tc.id &&
                    !consumedToolIdx.has(i),
                );
                let output = "";
                if (rawIdx >= 0) {
                  consumedToolIdx.add(rawIdx);
                  const raw = segment[rawIdx];
                  output = (
                    raw.toolCall?.output ??
                    raw.content ??
                    ""
                  ).trimEnd();
                }
                fold.push({
                  id: `${m.id}-syn-${tc.id}`,
                  role: "tool",
                  content: output,
                  toolCall: {
                    id: tc.id,
                    name: tc.name,
                    status: "completed",
                    input: tc.arguments,
                    output: output || undefined,
                  },
                });
              }
            }
          } else if (m.role === "tool" && !consumedToolIdx.has(si)) {
            fold.push(m);
          }
        }
        if (fold.length > 0) {
          out.push({ kind: "react_fold", id: `rf-${fold[0].id}`, fold });
          continue;
        }
      }
      for (const msg of segment) {
        out.push({ kind: "row", message: msg, dividerBefore: false });
      }
      continue;
    }

    let lastAssistantAfterTools = -1;
    for (let k = segment.length - 1; k > lastToolIdx; k--) {
      if (segment[k].role === "assistant") {
        lastAssistantAfterTools = k;
        break;
      }
    }

    if (lastAssistantAfterTools >= 0) {
      const fold = segment.slice(0, lastAssistantAfterTools);
      if (fold.length > 0) {
        out.push({ kind: "react_fold", id: `rf-${fold[0].id}`, fold });
      }
      out.push({
        kind: "row",
        message: segment[lastAssistantAfterTools],
        dividerBefore: true,
      });
      for (let k = lastAssistantAfterTools + 1; k < segment.length; k++) {
        out.push({ kind: "row", message: segment[k], dividerBefore: false });
      }
    } else {
      out.push({
        kind: "react_fold",
        id: `rf-${segment[0].id}`,
        fold: segment,
      });
    }
  }
  return out;
}

/** Pencil collapseRow summary: "Ran 2 commands, viewed a file" */
function summarizeToolGroup(messages: Message[]): string {
  if (messages.length === 0) return "";
  if (messages.length === 1) {
    return messages[0].toolCall?.name ?? "tool";
  }
  const names = messages.map((m) => m.toolCall?.name ?? "tool");
  const bashCount = names.filter((n) => n === "bash").length;
  const fileOps = names.filter(
    (n) =>
      n.includes("glob") ||
      n.includes("file_read") ||
      n.includes("read_file") ||
      n.includes("file_write") ||
      n.includes("file_edit") ||
      n.includes("web_fetch") ||
      n.includes("web_search") ||
      n.includes("todo_write") ||
      n.includes("notebook_edit") ||
      n === "file_read",
  ).length;
  const parts: string[] = [];
  if (bashCount > 0) {
    parts.push(`${bashCount} command${bashCount > 1 ? "s" : ""}`);
  }
  if (fileOps > 0) {
    parts.push(fileOps === 1 ? "viewed a file" : `viewed ${fileOps} files`);
  }
  const accounted = bashCount + fileOps;
  if (accounted < messages.length) {
    parts.push(`${messages.length - accounted} more`);
  }
  if (parts.length === 0) {
    return `Ran ${messages.length} tools`;
  }
  return `Ran ${parts.join(", ")}`;
}

function summarizeReactFold(fold: Message[]): string {
  const tools = fold.filter((m) => m.role === "tool" && m.toolCall);
  const thinking = fold.filter((m) => m.role === "assistant").length;
  if (tools.length === 0) {
    return thinking > 0 ? "Reasoning" : "Trace";
  }
  const toolSummary = summarizeToolGroup(tools);
  return thinking > 0 ? `Reasoning · ${toolSummary}` : toolSummary;
}

function toolGroupAnyRunning(messages: Message[]): boolean {
  return messages.some((m) => m.toolCall?.status === "running");
}

/** Name of a tool call still running (prefer the latest in the fold). */
function firstRunningToolName(messages: Message[]): string | null {
  for (let i = messages.length - 1; i >= 0; i--) {
    const m = messages[i];
    if (
      m.role === "tool" &&
      m.toolCall?.status === "running" &&
      m.toolCall.name?.trim()
    ) {
      return m.toolCall.name.trim();
    }
  }
  return null;
}

/**
 * Outer tool fold is "complete" when nothing is still running. Per-tool `error`
 * from `is_error` does not fail the whole fold — nested rows still show Error.
 */
function toolGroupFlowComplete(messages: Message[]): boolean {
  if (messages.length === 0) return false;
  return !toolGroupAnyRunning(messages);
}

/** Prefer toolCall.output; avoid using short status-only message.content as "Output". */
function toolDisplayOutputText(
  message: Message,
  tc: NonNullable<Message["toolCall"]>,
): string {
  const fromTc = tc.output?.trim();
  if (fromTc) return fromTc;
  if (message.role !== "tool" || !message.content?.trim()) return "";
  const c = message.content.trim();
  if (/^`[^`]+`\s+(completed|failed)$/i.test(c)) return "";
  return c;
}

/** Stable key for nested expand state inside a react_fold. */
function toolNestedPanelKey(foldId: string, messageId: string): string {
  return `${foldId}::${messageId}`;
}

/** Read `description` from tool JSON (bash, file_read, etc.). */
function parseToolDescriptionFromInput(
  input: string | undefined,
): string | null {
  if (!input?.trim()) return null;
  try {
    const j = JSON.parse(input) as Record<string, unknown>;
    const d = j.description;
    if (typeof d === "string" && d.trim()) return d.trim();
  } catch {
    /* not JSON */
  }
  return null;
}

/** Title when `description` is absent: tool name or a short hint from JSON. */
function toolTitleFallbackFromInput(
  input: string | undefined,
  toolName: string,
): string {
  if (!input?.trim()) return toolName;
  try {
    const j = JSON.parse(input) as Record<string, unknown>;
    const n = toolName.toLowerCase();
    if (n === "bash" || n.includes("bash")) {
      const cmd = j.command;
      if (typeof cmd === "string" && cmd.trim()) {
        const line = cmd.trim().split(/\r?\n/u)[0] ?? "";
        if (line) return line.length > 80 ? `${line.slice(0, 80)}…` : line;
      }
    }
    const keys = Object.keys(j).filter((k) => k !== "description");
    if (keys.length) return `${toolName} · ${keys.slice(0, 2).join(", ")}`;
  } catch {
    /* */
  }
  const t = input.trim();
  return t.length > 72 ? `${t.slice(0, 72)}…` : t;
}

/** Display title for a tool row: `description` from arguments JSON, else fallback. */
function toolCallPanelTitle(
  input: string | undefined,
  toolName: string,
): string {
  return (
    parseToolDescriptionFromInput(input) ??
    toolTitleFallbackFromInput(input, toolName)
  );
}

function getNestedToolPanelOpen(
  key: string,
  tc: NonNullable<Message["toolCall"]>,
  overrides: Record<string, boolean>,
): boolean {
  if (key in overrides) return overrides[key];
  return tc.status === "running";
}

export function Chat({ sessionId }: ChatProps) {
  const theme = useTheme();
  const CHAT = useMemo(() => getChatTokens(theme), [theme]);
  const isDev = import.meta.env.DEV;
  const [panelTab, setPanelTab] = useState(0);
  const [input, setInput] = useState("");
  const [messages, setMessages] = useState<Message[]>([]);
  const [isStreaming, setIsStreaming] = useState(false);
  const [currentResponse, setCurrentResponse] = useState("");
  const [currentStreamId, setCurrentStreamId] = useState<string | null>(null);
  const [currentRoundId, setCurrentRoundId] = useState<string | null>(null);
  /** Toast when a background bash command completes (`background-shell-complete`). */
  const [bgToast, setBgToast] = useState<string | null>(null);
  /** 未选工作目录时的提示（Snackbar，5s 自动消失）；key 用于重复触发时重置计时 */
  const [pathToastKey, setPathToastKey] = useState(0);
  const [pathRequiredToast, setPathRequiredToast] = useState<string | null>(
    null,
  );
  const showPathRequiredWarning = () => {
    setPathToastKey((k) => k + 1);
    setPathRequiredToast("请先选择工作目录后再发送消息。");
  };
  /** Expanded tool-trace panels (default: none → collapsed, matching pencil-new.pen). */
  const [expandedToolGroups, setExpandedToolGroups] = useState<Set<string>>(
    new Set(),
  );
  /** Per-tool nested panels inside a fold: key → open. Unset → default (open while running). */
  const [nestedToolPanelOpen, setNestedToolPanelOpen] = useState<
    Record<string, boolean>
  >({});
  const isConnecting = useActivityStore((s) => s.isConnecting);
  const activityIsStreaming = useActivityStore((s) => s.isStreaming);
  const waitingFirstChunk = useActivityStore((s) => s.waitingFirstChunk);
  const currentToolHint = useActivityStore((s) => s.currentToolHint);
  const executionSteps = useActivityStore((s) => s.executionSteps);
  /** First text chunk of each “segment” (after Start or after tool_result). */
  const segmentStartRef = useRef(true);

  /** Implicit memory indexing status for the last completed turn */
  const [indexingStatus, setIndexingStatus] = useState<
    "idle" | "indexing" | "completed" | "error"
  >("idle");

  const messagesEndRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);
  const unlistenRef = useRef<(() => void) | null>(null);
  const currentResponseRef = useRef(currentResponse);
  const currentRoundIdRef = useRef(currentRoundId);

  // Keep refs in sync with state for access in event listeners
  useEffect(() => {
    currentResponseRef.current = currentResponse;
  }, [currentResponse]);

  useEffect(() => {
    currentRoundIdRef.current = currentRoundId;
  }, [currentRoundId]);

  const {
    storeMessages,
    currentSession,
    addMessage,
    replaceStoreMessagesSnapshot,
    updateRoundStatus,
    updateSessionProjectPath,
    renameSession,
  } = useSessionStore();

  const showNewSessionPlaceholder =
    currentSession != null &&
    shouldShowNewSessionPlaceholder(currentSession, {
      isCurrentSession: true,
      storeMessageCount: storeMessages.length,
    });

  useEffect(() => {
    useActivityStore.getState().clearTransient();
    useActivityStore.getState().resetExecutionState();
    useActivityStore.getState().clearBackgroundJobs();
  }, [sessionId]);

  const needsWorkspacePath =
    Boolean(sessionId) &&
    currentSession != null &&
    isUnsetWorkspacePath(
      currentSession.workingDirectory ?? currentSession.projectPath,
    );

  const handlePickProjectFolder = async () => {
    if (!sessionId) return;
    try {
      const selected = await open({
        directory: true,
        multiple: false,
        title: "选择工作目录",
      });
      if (selected == null) return;
      const path = Array.isArray(selected) ? selected[0] : selected;
      if (!path) return;
      await updateSessionProjectPath(sessionId, path);
    } catch (e) {
      console.error("[Chat] folder dialog failed", e);
    }
  };

  const toggleToolGroupExpand = (groupId: string) => {
    setExpandedToolGroups((prev) => {
      const next = new Set(prev);
      if (next.has(groupId)) next.delete(groupId);
      else next.add(groupId);
      return next;
    });
  };

  const toggleNestedToolPanel = (
    foldId: string,
    messageId: string,
    tc: NonNullable<Message["toolCall"]>,
  ) => {
    const key = toolNestedPanelKey(foldId, messageId);
    setNestedToolPanelOpen((prev) => {
      const cur = getNestedToolPanelOpen(key, tc, prev);
      return { ...prev, [key]: !cur };
    });
  };

  // Scroll to bottom when messages change
  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: "auto" });
  }, [messages, currentResponse, isConnecting, waitingFirstChunk]);

  const messageRenderItems = useMemo(
    () => groupMessagesForRender(messages),
    [messages],
  );
  const lastReactFoldId = useMemo(() => {
    for (let i = messageRenderItems.length - 1; i >= 0; i--) {
      const it = messageRenderItems[i];
      if (it.kind === "react_fold") return it.id;
    }
    return null;
  }, [messageRenderItems]);

  // Load messages from store when session changes
  useEffect(() => {
    try {
      if (sessionId && storeMessages.length > 0) {
        const convertedMessages: Message[] = storeMessages.map(
          (msg, index) => ({
            id: `${sessionId}-msg-${index}`,
            role: msg.role,
            content: msg.content ?? "",
            prefaceBeforeTools: msg.prefaceBeforeTools,
            toolCallsList: msg.toolCallsList,
            timestamp: Date.now() - (storeMessages.length - index) * 1000,
            toolCall: msg.toolCall
              ? {
                  id: msg.toolCall.id,
                  name: msg.toolCall.name,
                  status: msg.toolCall.status ?? ("completed" as const),
                  input: msg.toolCall.arguments ?? "",
                  output:
                    msg.toolCall.output ??
                    (msg.role === "tool" ? (msg.content ?? "") : undefined),
                }
              : undefined,
          }),
        );
        setMessages(convertedMessages);
      } else if (!sessionId) {
        setMessages([]);
      } else if (sessionId && storeMessages.length === 0) {
        setMessages([]);
      }
    } catch (e) {
      console.error(
        "[OmigaDebug][Chat] failed to sync messages from store",
        e,
        {
          sessionId,
          storeMessagesLength: storeMessages.length,
        },
      );
      setMessages([]);
    }
  }, [sessionId, storeMessages]);

  // Clean up listener on unmount
  useEffect(() => {
    return () => {
      if (unlistenRef.current) {
        unlistenRef.current();
        unlistenRef.current = null;
      }
    };
  }, []);

  // Background bash completion (detached `run_in_background` tasks)
  useEffect(() => {
    if (!sessionId) return;
    let cancelled = false;
    let unlistenBg: (() => void) | undefined;
    (async () => {
      unlistenBg = await listen<BackgroundShellCompletePayload>(
        "background-shell-complete",
        (event) => {
          if (cancelled) return;
          const p = event.payload;
          if (p.session_id !== sessionId) return;
          const summary = p.interrupted
            ? `Background command interrupted. Output file: ${p.output_path}`
            : `Background command finished (exit ${p.exit_code}). Output file: ${p.output_path}`;
          const toastText = p.description
            ? `${p.description} — ${summary}`
            : summary;
          setMessages((prev) => {
            const idx = prev.findIndex(
              (m) => m.role === "tool" && m.toolCall?.id === p.tool_use_id,
            );
            if (idx < 0) {
              return prev;
            }
            const row = prev[idx];
            if (!row.toolCall) return prev;
            const prevOut = (row.toolCall.output ?? "").trimEnd();
            const append = prevOut ? `\n\n---\n${summary}` : summary;
            const next = [...prev];
            next[idx] = {
              ...row,
              toolCall: {
                ...row.toolCall,
                output: `${prevOut}${append}`,
              },
            };
            replaceStoreMessagesSnapshot(next.map(chatMessageToStore));
            return next;
          });
          setBgToast(toastText);
          useActivityStore.getState().upsertBackgroundJob({
            id: p.tool_use_id,
            toolUseId: p.tool_use_id,
            label: p.description?.trim() || `后台任务 (exit ${p.exit_code})`,
            state: p.interrupted
              ? "interrupted"
              : p.exit_code === 0
                ? "done"
                : "error",
            exitCode: p.exit_code,
          });
        },
      );
    })();
    return () => {
      cancelled = true;
      unlistenBg?.();
    };
  }, [sessionId, replaceStoreMessagesSnapshot]);

  // Wiki dispatch: window event "wikiSendMessage" → fill input then auto-send.
  // Used by WikiSettingsTab (inside Settings modal) to dispatch agent prompts to the chat.
  const wikiPendingSendRef = useRef<string | null>(null);
  useEffect(() => {
    const handler = (e: Event) => {
      const content = (e as CustomEvent<{ content: string }>).detail?.content;
      if (!content?.trim()) return;
      wikiPendingSendRef.current = content.trim();
      setInput(content.trim());
    };
    window.addEventListener("wikiSendMessage", handler);
    return () => window.removeEventListener("wikiSendMessage", handler);
  }, []);

  // Trigger send once input has been updated by wikiPendingSendRef
  useEffect(() => {
    if (wikiPendingSendRef.current && input === wikiPendingSendRef.current) {
      wikiPendingSendRef.current = null;
      handleSend();
    }
  });

  // Set up stream listener for a specific stream ID
  const setupStreamListener = async (streamId: string) => {
    // Clean up previous listener
    if (unlistenRef.current) {
      unlistenRef.current();
    }

    const eventName = `chat-stream-${streamId}`;

    const unlisten = await listen<StreamOutputItem>(eventName, (event) => {
      const payload = event.payload;

      switch (payload.type) {
        case "Start": {
          setIsStreaming(true);
          useActivityStore.getState().setConnecting(false);
          useActivityStore.getState().setStreaming(true, true);
          segmentStartRef.current = true;
          useActivityStore.getState().onStreamStart();
          setCurrentResponse("");
          if (isDev) {
            console.debug("[OmigaDev][AgentStream]", {
              streamId,
              type: payload.type,
            });
          }
          break;
        }
        case "text": {
          const text = typeof payload.data === "string" ? payload.data : "";
          if (text) {
            if (segmentStartRef.current) {
              segmentStartRef.current = false;
              useActivityStore.getState().onFirstTextChunk();
            }
            const firstChunk = currentResponseRef.current.length === 0;
            setCurrentResponse((prev) => prev + text);
            if (firstChunk) {
              useActivityStore.getState().setStreaming(true, false);
            }
          }
          if (isDev && text) {
            console.debug("[OmigaDev][AgentChunk]", { streamId, chunk: text });
          }
          break;
        }
        case "tool_use": {
          const toolData = payload.data as
            | { id?: string; name?: string; arguments?: string }
            | undefined;
          if (isEmptyToolUsePayload(toolData)) {
            if (isDev) {
              console.debug("[OmigaDev][AgentToolUse skipped empty]", {
                streamId,
                toolData,
              });
            }
            break;
          }
          useActivityStore.getState().setStreaming(true, false);
          useActivityStore
            .getState()
            .setCurrentToolHint((toolData?.name ?? "tool").slice(0, 96));
          const tuId = (toolData?.id ?? "").trim();
          setMessages((prev) => {
            if (tuId) {
              const ex = prev.findIndex(
                (m) => m.role === "tool" && m.toolCall?.id === tuId,
              );
              if (ex >= 0) {
                const cur = prev[ex];
                if (!cur.toolCall) return prev;
                const next = [...prev];
                next[ex] = {
                  ...cur,
                  content: `\`${toolData?.name || cur.toolCall.name}\``,
                  toolCall: {
                    ...cur.toolCall,
                    id: tuId,
                    name: toolData?.name || cur.toolCall.name,
                    input:
                      toolData?.arguments !== undefined
                        ? toolData.arguments
                        : cur.toolCall.input,
                    status: cur.toolCall.status ?? "running",
                  },
                };
                return next;
              }
            }
            const thinkingChunk = currentResponseRef.current.trim();
            queueMicrotask(() => setCurrentResponse(""));
            const newToolId = `tool-${Date.now()}`;
            const toolMsg: Message = {
              id: newToolId,
              role: "tool",
              content: `\`${toolData?.name || "tool"}\``,
              toolCall: {
                id: tuId || undefined,
                name: toolData?.name || "tool",
                status: "running",
                input: toolData?.arguments,
              },
              timestamp: Date.now(),
            };
            const next = [...prev];
            if (thinkingChunk) {
              next.push({
                id: `assistant-seg-${newToolId}`,
                role: "assistant",
                content: thinkingChunk,
                timestamp: Date.now(),
              });
            }
            next.push(toolMsg);
            return next;
          });
          if (isDev) {
            console.debug("[OmigaDev][AgentToolUse]", {
              streamId,
              tool: toolData,
            });
          }
          if (tuId) {
            const tn = toolData?.name ?? "tool";
            const rawArgs = toolData?.arguments;
            useActivityStore
              .getState()
              .onToolUseStart(tuId, humanizeToolStepTitle(tn, rawArgs), {
                summary: executionStepSummary(tn, rawArgs),
                input: rawArgs,
                toolName: tn,
              });
          }
          break;
        }
        case "tool_result": {
          const resultData = payload.data as
            | {
                tool_use_id?: string;
                name: string;
                input: string;
                output: string;
                is_error: boolean;
              }
            | undefined;
          const toolResultMatchId =
            resultData &&
            typeof resultData.tool_use_id === "string" &&
            resultData.tool_use_id.trim() !== ""
              ? resultData.tool_use_id.trim()
              : null;
          setMessages((prev) => {
            const matchId = toolResultMatchId;
            let idx = -1;
            if (matchId) {
              for (let i = prev.length - 1; i >= 0; i--) {
                const m = prev[i];
                if (
                  m.role === "tool" &&
                  m.toolCall &&
                  m.toolCall.id === matchId
                ) {
                  idx = i;
                  break;
                }
              }
            }
            if (idx < 0) {
              for (let i = prev.length - 1; i >= 0; i--) {
                const m = prev[i];
                if (
                  m.role === "tool" &&
                  m.toolCall &&
                  m.toolCall.status === "running"
                ) {
                  idx = i;
                  break;
                }
              }
            }
            if (idx < 0) return prev;
            const lastMsg = prev[idx];
            if (!lastMsg.toolCall) return prev;
            const updated = [...prev];
            const nextInput =
              resultData != null &&
              typeof resultData.input === "string" &&
              resultData.input.trim() !== ""
                ? resultData.input
                : lastMsg.toolCall.input;
            updated[idx] = {
              ...lastMsg,
              content: `\`${resultData?.name || lastMsg.toolCall.name}\` ${resultData?.is_error ? "failed" : "completed"}`,
              toolCall: {
                ...lastMsg.toolCall,
                name: resultData?.name || lastMsg.toolCall.name,
                status: resultData?.is_error ? "error" : "completed",
                input: nextInput,
                output: resultData?.output,
              },
            };
            return updated;
          });
          if (toolResultMatchId) {
            useActivityStore.getState().onToolResultDone(toolResultMatchId, {
              output: resultData?.output,
              failed: Boolean(resultData?.is_error),
            });
          }
          segmentStartRef.current = true;
          if (resultData?.name === "bash" && resultData.tool_use_id) {
            const bg = tryParseBashBackground(resultData.input);
            if (bg) {
              useActivityStore.getState().upsertBackgroundJob({
                id: resultData.tool_use_id,
                toolUseId: resultData.tool_use_id,
                label: bg.label,
                state: "running",
              });
            }
          }
          if (isDev) {
            console.debug("[OmigaDev][AgentToolResult]", {
              streamId,
              result: resultData,
            });
          }
          break;
        }
        case "error": {
          const errorData = payload.data as
            | { message: string; code?: string }
            | undefined;
          setIsStreaming(false);
          useActivityStore.getState().finalizeExecutionRun();
          useActivityStore.getState().clearTransient();
          setCurrentResponse("");
          const errorMsg: Message = {
            id: `error-${Date.now()}`,
            role: "assistant",
            content: `Error: ${errorData?.message || "Unknown error occurred"}`,
            timestamp: Date.now(),
          };
          setMessages((prev) => {
            const next = [...prev, errorMsg];
            replaceStoreMessagesSnapshot(next.map(chatMessageToStore));
            return next;
          });
          if (isDev) {
            console.debug("[OmigaDev][AgentError]", {
              streamId,
              error: errorData,
            });
          }
          break;
        }
        case "cancelled": {
          setIsStreaming(false);
          useActivityStore.getState().finalizeExecutionRun();
          useActivityStore.getState().clearTransient();
          // Mark round as cancelled - use ref to get latest round ID
          const roundId = currentRoundIdRef.current;
          if (roundId) {
            updateRoundStatus(roundId, "cancelled");
          }
          // Add cancelled message - use ref to get latest response
          const partialResponse = currentResponseRef.current;
          const cancelledMsg: Message = {
            id: `cancelled-${Date.now()}`,
            role: "assistant",
            content: partialResponse
              ? partialResponse + "\n\n[Cancelled by user]"
              : "[Cancelled by user]",
            timestamp: Date.now(),
          };
          setMessages((prev) => {
            const next = [...prev, cancelledMsg];
            replaceStoreMessagesSnapshot(next.map(chatMessageToStore));
            return next;
          });
          setCurrentResponse("");
          if (isDev) {
            console.debug("[OmigaDev][AgentCancelled]", {
              streamId,
              partial: partialResponse,
            });
          }
          break;
        }
        case "complete": {
          setIsStreaming(false);
          useActivityStore.getState().finalizeExecutionRun();
          useActivityStore.getState().clearTransient();
          // Mark round as completed - use ref to get latest round ID
          const roundId = currentRoundIdRef.current;
          if (roundId) {
            updateRoundStatus(roundId, "completed");
          }
          // Use ref to get the latest accumulated response
          const finalResponse = currentResponseRef.current;
          setMessages((prev) => {
            let next = prev;
            if (finalResponse) {
              const assistantMsg: Message = {
                id: `assistant-${Date.now()}`,
                role: "assistant",
                content: finalResponse,
                timestamp: Date.now(),
              };
              next = [...prev, assistantMsg];
            }
            // Full transcript (user + tools + assistant) so useEffect does not drop tool rows
            replaceStoreMessagesSnapshot(next.map(chatMessageToStore));
            return next;
          });
          setCurrentResponse("");

          // Trigger implicit memory indexing status display
          setIndexingStatus("indexing");
          // Auto-hide after 3 seconds
          setTimeout(() => {
            setIndexingStatus((prev) =>
              prev === "indexing" ? "completed" : prev,
            );
            // Clear completed status after another 2 seconds
            setTimeout(() => {
              setIndexingStatus((prev) =>
                prev === "completed" ? "idle" : prev,
              );
            }, 2000);
          }, 3000);

          if (isDev) {
            console.debug("[OmigaDev][AgentComplete]", {
              streamId,
              final: finalResponse,
            });
          }
          break;
        }
      }
    });

    unlistenRef.current = unlisten;
  };

  const handleSend = async () => {
    if (!input.trim() || isStreaming || isConnecting || !sessionId) return;
    if (needsWorkspacePath) {
      showPathRequiredWarning();
      return;
    }

    // Reset indexing status on new message
    setIndexingStatus("idle");

    useActivityStore.getState().beginExecutionRun();
    useActivityStore.getState().setConnecting(true);
    useActivityStore.getState().setStreaming(false, false);

    const isFirstMessageInSession = storeMessages.length === 0;

    const userMessage: Message = {
      id: `user-${Date.now()}`,
      role: "user",
      content: input.trim(),
      timestamp: Date.now(),
    };

    // Add to local state
    setMessages((prev) => [...prev, userMessage]);

    // Add to store for persistence
    addMessage({
      role: "user",
      content: input.trim(),
    });

    const messageContent = input.trim();
    setInput("");

    try {
      if (
        isFirstMessageInSession &&
        isPlaceholderSessionTitle(currentSession?.name)
      ) {
        await renameSession(
          sessionId,
          titleFromFirstUserMessage(messageContent),
        );
      }

      // Call backend with new request structure
      const response = await invoke<{
        message_id: string;
        session_id: string;
        round_id: string;
      }>("send_message", {
        request: {
          content: messageContent,
          session_id: sessionId,
          project_path: currentSession?.projectPath,
          session_name: currentSession?.name,
          use_tools: true,
        },
      });

      // Track round_id for status updates
      setCurrentRoundId(response.round_id);

      // Set up listener for this specific stream
      setCurrentStreamId(response.message_id);
      await setupStreamListener(response.message_id);
    } catch (error: unknown) {
      console.error("Failed to send message:", error);
      useActivityStore.getState().clearTransient();
      useActivityStore.getState().resetExecutionState();

      // OmigaError has: { type: "Chat", details: { kind: "StreamError", message: "..." } }
      let errorMessage = "Unknown error";
      if (typeof error === "string") {
        errorMessage = error;
      } else if (error && typeof error === "object") {
        const err = error as Record<string, unknown>;

        // Handle OmigaError structure: { type, details: { kind, message } }
        if (
          err.type === "Chat" &&
          err.details &&
          typeof err.details === "object"
        ) {
          const details = err.details as Record<string, unknown>;
          // ChatError: { kind, message }
          if (details.kind === "ApiKeyMissing") {
            errorMessage =
              "API key not configured. Please set your LLM API key in settings.";
          } else if (typeof details.message === "string") {
            errorMessage = details.message;
          } else if (typeof details.kind === "string") {
            errorMessage = details.kind;
          }
        } else if (err.type === "Config" && typeof err.details === "string") {
          errorMessage = err.details;
        } else if (
          err.type === "Api" &&
          err.details &&
          typeof err.details === "object"
        ) {
          const details = err.details as Record<string, unknown>;
          errorMessage =
            typeof details.message === "string"
              ? details.message
              : typeof details.kind === "string"
                ? details.kind
                : String(err.details);
        } else if (typeof err.message === "string") {
          errorMessage = err.message;
        } else {
          errorMessage = JSON.stringify(error);
        }
      } else {
        errorMessage = String(error);
      }

      console.log("Extracted error message:", errorMessage);

      const errorMsg: Message = {
        id: `error-${Date.now()}`,
        role: "assistant",
        content: `Failed to send message: ${errorMessage}`,
        timestamp: Date.now(),
      };
      setMessages((prev) => [...prev, errorMsg]);
    }
  };

  const handleKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key !== "Enter" || e.shiftKey) return;

    // IME（中文/日文等）：Enter 用于确认候选词，不应发送消息
    const ne = e.nativeEvent;
    if (ne.isComposing || ne.keyCode === 229) return;

    e.preventDefault();
    if (needsWorkspacePath) {
      if (input.trim()) {
        showPathRequiredWarning();
      }
      return;
    }
    handleSend();
  };

  const handleCancel = async () => {
    if (!currentStreamId) return;

    try {
      await invoke("cancel_stream", { messageId: currentStreamId });
    } catch (error) {
      console.error("Failed to cancel stream:", error);
    }
  };

  const renderMessageContent = (
    content: string,
    tone: "default" | "agent" = "default",
  ) => {
    const isAgent = tone === "agent";
    const prismStyleRaw = theme.palette.mode === "dark" ? oneDark : oneLight;
    const prismStyleFenced = prismStyleTransparentCodeSurface(
      prismStyleRaw as Record<string, CSSProperties>,
    );
    // Handle empty or undefined content
    if (!content || content.trim() === "") {
      return (
        <Typography
          variant="body1"
          color="text.secondary"
          sx={{ fontStyle: "italic" }}
        >
          (Empty response)
        </Typography>
      );
    }

    return (
      <Box
        sx={{
          fontFamily: CHAT.font,
          minWidth: 0,
          maxWidth: "100%",
          overflowX: "hidden",
          overflowWrap: "anywhere",
          wordBreak: "break-word",
          ...(isAgent ? { "& :first-of-type": { mt: 0 } } : {}),
        }}
      >
        <ReactMarkdown
          remarkPlugins={[remarkGfm]}
          components={{
            code({
              className,
              children,
            }: {
              className?: string;
              children?: React.ReactNode;
            }) {
              const match = /language-(\w+)/.exec(className || "");
              const language = match ? match[1] : "";
              const isInline = !className?.includes("language-");

              if (!isInline && language) {
                const blockBody = String(children).replace(/\n$/, "");
                const lang = language || "text";
                const fencedScrollStyle = {
                  margin: 0,
                  borderRadius: 0,
                  background: "transparent",
                  whiteSpace: "pre" as const,
                  wordBreak: "normal" as const,
                  overflowWrap: "normal" as const,
                  minWidth: "min-content",
                };
                if (isAgent) {
                  return (
                    <Box sx={{ my: 1.25 }}>
                      <Typography
                        sx={{
                          fontSize: 10,
                          color: CHAT.labelMuted,
                          fontWeight: 400,
                          mb: 0.5,
                          display: "block",
                        }}
                      >
                        {lang}
                      </Typography>
                      <Box
                        sx={{
                          borderRadius: `${MD_BLOCK_RADIUS_PX}px`,
                          border: `1px solid ${CHAT.agentBubbleBorder}`,
                          bgcolor: CHAT.codeBg,
                          maxHeight: 320,
                          maxWidth: "100%",
                          overflow: "auto",
                          [`& ${PRISM_PRE_SEL}, & ${PRISM_CODE_SEL}`]: {
                            background: "transparent !important",
                            backgroundColor: "transparent !important",
                          },
                        }}
                      >
                        <SyntaxHighlighter
                          style={prismStyleFenced}
                          language={lang}
                          PreTag="div"
                          customStyle={{
                            ...fencedScrollStyle,
                            padding: "8px 12px",
                            fontSize: 11,
                            lineHeight: 1.45,
                          }}
                        >
                          {blockBody}
                        </SyntaxHighlighter>
                      </Box>
                    </Box>
                  );
                }
                return (
                  <Box
                    component="div"
                    sx={{
                      my: 1.5,
                      borderRadius: `${MD_BLOCK_RADIUS_PX}px`,
                      overflow: "hidden",
                      border: 1,
                      borderColor: alpha(theme.palette.divider, 0.5),
                    }}
                  >
                    <Box
                      sx={{
                        px: 2,
                        py: 0.5,
                        bgcolor: alpha(theme.palette.background.paper, 0.5),
                        borderBottom: 1,
                        borderColor: alpha(theme.palette.divider, 0.3),
                      }}
                    >
                      <Typography variant="caption" color="text.secondary">
                        {lang}
                      </Typography>
                    </Box>
                    <Box
                      sx={{
                        bgcolor: CHAT.codeBg,
                        maxHeight: 360,
                        maxWidth: "100%",
                        overflow: "auto",
                        [`& ${PRISM_PRE_SEL}, & ${PRISM_CODE_SEL}`]: {
                          background: "transparent !important",
                          backgroundColor: "transparent !important",
                        },
                      }}
                    >
                      <SyntaxHighlighter
                        style={prismStyleFenced}
                        language={lang}
                        PreTag="div"
                        customStyle={{
                          ...fencedScrollStyle,
                          padding: "12px 16px",
                          fontSize: "0.8125rem",
                          lineHeight: 1.6,
                        }}
                      >
                        {blockBody}
                      </SyntaxHighlighter>
                    </Box>
                  </Box>
                );
              }
              const inlineRaw = String(children);
              const longInline = inlineRaw.length > INLINE_CODE_LONG_LEN;
              return (
                <Box
                  component="code"
                  className={className}
                  sx={{
                    fontFamily: "Menlo, Monaco, Consolas, monospace",
                    fontSize: isAgent ? 12 : "0.875em",
                    lineHeight: 1.45,
                    background: "transparent",
                    ...(longInline
                      ? {
                          padding: 0,
                          borderRadius: 0,
                          boxDecorationBreak: "unset",
                          WebkitBoxDecorationBreak: "unset",
                          color: isAgent ? CHAT.textPrimary : "inherit",
                        }
                      : {
                          padding: isAgent ? "2px 5px" : "0.1em 0.35em",
                          borderRadius: `${MD_BLOCK_RADIUS_PX}px`,
                          boxDecorationBreak: "clone",
                          WebkitBoxDecorationBreak: "clone",
                          color: isAgent ? CHAT.textPrimary : "inherit",
                        }),
                    display: "inline-block",
                    maxWidth: "100%",
                    verticalAlign: "baseline",
                    boxSizing: "border-box",
                    whiteSpace: "pre-wrap",
                    wordBreak: "break-all",
                    overflowWrap: "anywhere",
                  }}
                >
                  {children}
                </Box>
              );
            },
            p({ children }) {
              return (
                <Typography
                  variant="body1"
                  sx={{
                    my: 1,
                    lineHeight: isAgent ? 1.45 : 1.7,
                    fontSize: isAgent ? 13 : undefined,
                    color: isAgent ? CHAT.textPrimary : undefined,
                    maxWidth: "100%",
                    // 无空格超长串（如蛋白序列）也可在容器内折行
                    overflowWrap: "anywhere",
                    wordBreak: "break-word",
                  }}
                >
                  {children}
                </Typography>
              );
            },
            a({ href, children }) {
              return (
                <Typography
                  component="a"
                  href={href ?? "#"}
                  sx={{
                    color: isAgent ? CHAT.accent : "primary.main",
                    overflowWrap: "anywhere",
                    wordBreak: "break-all",
                  }}
                >
                  {children}
                </Typography>
              );
            },
            table({ children }) {
              return (
                <Box
                  component="table"
                  sx={{
                    width: "100%",
                    maxWidth: "100%",
                    tableLayout: "fixed",
                    borderCollapse: "collapse",
                    my: 1,
                    fontSize: isAgent ? 12 : undefined,
                    "& th, & td": {
                      border: `1px solid ${alpha(theme.palette.divider, 0.6)}`,
                      px: 0.75,
                      py: 0.5,
                      wordBreak: "break-word",
                      overflowWrap: "anywhere",
                      verticalAlign: "top",
                    },
                  }}
                >
                  {children}
                </Box>
              );
            },
            img({ src, alt }) {
              return (
                <Box
                  component="img"
                  src={src}
                  alt={alt ?? ""}
                  sx={{
                    display: "block",
                    maxWidth: "100%",
                    height: "auto",
                    borderRadius: 1,
                    my: 1,
                  }}
                />
              );
            },
            ul({ children }) {
              return (
                <Box
                  component="ul"
                  sx={{
                    my: 1,
                    // 为默认 outside 序号/圆点留出左侧空间，避免被气泡 overflow 裁切
                    pl: 3.5,
                    ...(isAgent
                      ? { color: CHAT.textPrimary, fontSize: 13 }
                      : {}),
                  }}
                >
                  {children}
                </Box>
              );
            },
            ol({ children }) {
              return (
                <Box
                  component="ol"
                  sx={{
                    my: 1,
                    pl: 3.5,
                    ...(isAgent
                      ? { color: CHAT.textPrimary, fontSize: 13 }
                      : {}),
                  }}
                >
                  {children}
                </Box>
              );
            },
            li({ children }) {
              return (
                <Box
                  component="li"
                  sx={{
                    display: "list-item",
                    my: 0.5,
                    maxWidth: "100%",
                  }}
                >
                  <Typography
                    variant="body1"
                    sx={{
                      maxWidth: "100%",
                      overflowWrap: "anywhere",
                      wordBreak: "break-word",
                      ...(isAgent ? { fontSize: 13, lineHeight: 1.45 } : {}),
                    }}
                  >
                    {children}
                  </Typography>
                </Box>
              );
            },
            h1({ children }) {
              return (
                <Typography
                  variant="h4"
                  sx={{
                    mt: 3,
                    mb: 1,
                    fontWeight: 600,
                    ...(isAgent
                      ? { color: CHAT.textPrimary, fontSize: 15 }
                      : {}),
                  }}
                >
                  {children}
                </Typography>
              );
            },
            h2({ children }) {
              return (
                <Typography
                  variant="h5"
                  sx={{
                    mt: 2.5,
                    mb: 1,
                    fontWeight: 600,
                    ...(isAgent
                      ? { color: CHAT.textPrimary, fontSize: 15 }
                      : {}),
                  }}
                >
                  {children}
                </Typography>
              );
            },
            h3({ children }) {
              return (
                <Typography
                  variant="h6"
                  sx={{
                    mt: 2,
                    mb: 1,
                    fontWeight: 600,
                    ...(isAgent
                      ? { color: CHAT.textPrimary, fontSize: 15 }
                      : {}),
                  }}
                >
                  {children}
                </Typography>
              );
            },
            blockquote({ children }) {
              return (
                <Box
                  component="blockquote"
                  sx={{
                    my: 1.5,
                    pl: 2,
                    borderLeft: 3,
                    borderColor: isAgent
                      ? CHAT.agentBubbleBorder
                      : "primary.main",
                    bgcolor: isAgent
                      ? "transparent"
                      : alpha(theme.palette.primary.main, 0.05),
                    py: 1,
                    borderRadius: `0 ${MD_BLOCK_RADIUS_PX}px ${MD_BLOCK_RADIUS_PX}px 0`,
                    ...(isAgent ? { color: CHAT.textMuted, fontSize: 13 } : {}),
                  }}
                >
                  {children}
                </Box>
              );
            },
          }}
        >
          {content}
        </ReactMarkdown>
      </Box>
    );
  };

  return (
    <Box
      sx={{
        display: "flex",
        flexDirection: "column",
        height: "100%",
        minHeight: 0,
      }}
    >
      <Tabs
        value={panelTab}
        onChange={(_, v) => setPanelTab(v)}
        aria-label="Chat or terminal"
        sx={{
          flexShrink: 0,
          minHeight: 40,
          borderBottom: 1,
          borderColor: "divider",
          bgcolor: alpha(theme.palette.background.paper, 0.85),
          "& .MuiTab-root": {
            minHeight: 40,
            textTransform: "none",
            fontWeight: 600,
            fontSize: 13,
          },
          "& .MuiTabs-indicator": { height: 2 },
        }}
      >
        <Tab label="Chat" id="omiga-tab-chat" />
        <Tab label="Terminal" id="omiga-tab-terminal" />
      </Tabs>

      {panelTab === 1 ? (
        <Box
          sx={{
            flex: 1,
            minHeight: 0,
            overflow: "hidden",
            display: "flex",
            flexDirection: "column",
          }}
        >
          <Terminal embedded />
        </Box>
      ) : (
        <Box
          sx={{
            flex: 1,
            minHeight: 0,
            display: "flex",
            flexDirection: "column",
          }}
        >
          {/* Chat Header */}
          {currentSession && (
            <Box
              sx={{
                px: 3,
                py: 1.5,
                borderBottom: 1,
                borderColor: "divider",
                bgcolor: alpha(theme.palette.background.paper, 0.6),
                position: "relative",
              }}
            >
              <Stack
                direction="row"
                alignItems="center"
                justifyContent="space-between"
                spacing={1}
              >
                <Stack
                  direction="row"
                  alignItems="center"
                  spacing={0.75}
                  minWidth={0}
                >
                  <Tooltip title="Conversations">
                    <IconButton
                      size="small"
                      aria-label="Open conversations list"
                      onClick={() => {
                        document
                          .getElementById("omiga-session-panel")
                          ?.scrollIntoView({
                            behavior: "auto",
                            block: "nearest",
                          });
                      }}
                      sx={{
                        color: "text.secondary",
                        "&:hover": {
                          bgcolor: alpha(theme.palette.primary.main, 0.08),
                        },
                      }}
                    >
                      <ForumOutlined fontSize="small" />
                    </IconButton>
                  </Tooltip>
                  <Stack direction="column" spacing={0.25} minWidth={0}>
                    <Stack
                      direction="row"
                      alignItems="center"
                      spacing={0.75}
                      minWidth={0}
                    >
                      <Typography
                        variant="subtitle1"
                        fontWeight={600}
                        noWrap
                        sx={
                          showNewSessionPlaceholder
                            ? {
                                color: "text.secondary",
                                fontStyle: "italic",
                                fontWeight: 500,
                              }
                            : undefined
                        }
                      >
                        {showNewSessionPlaceholder
                          ? UNUSED_SESSION_LABEL
                          : currentSession.name}
                      </Typography>
                      {messages.length > 0 && (
                        <Chip
                          size="small"
                          label={`${messages.length} messages`}
                          variant="outlined"
                          color="default"
                        />
                      )}
                    </Stack>
                  </Stack>
                </Stack>
                <AgentSessionStatus
                  executionSteps={executionSteps}
                  isConnecting={isConnecting}
                  isStreaming={activityIsStreaming}
                  waitingFirstChunk={waitingFirstChunk}
                  toolHintFallback={currentToolHint}
                />
              </Stack>
            </Box>
          )}

          {/* Messages Area */}
          <Box
            sx={{
              flex: 1,
              minWidth: 0,
              overflowY: "auto",
              overflowX: "hidden",
              p: 3,
              display: "flex",
              flexDirection: "column",
              gap: 2,
            }}
          >
            {messages.length === 0 && !currentResponse && (
              <Box
                sx={{
                  display: "flex",
                  flexDirection: "column",
                  alignItems: "center",
                  justifyContent: "center",
                  height: "100%",
                  color: "text.secondary",
                }}
              >
                <SmartToy sx={{ fontSize: 48, mb: 2, opacity: 0.5 }} />
                <Typography variant="h6" gutterBottom>
                  Welcome to Omiga
                </Typography>
                <Typography variant="body2">
                  {sessionId
                    ? "Send a message to start the conversation"
                    : "Select or create a session to begin"}
                </Typography>
              </Box>
            )}

            {messageRenderItems.map((item) => {
              if (item.kind === "react_fold") {
                const { id, fold } = item;
                const toolMsgs = fold.filter(
                  (m) => m.role === "tool" && m.toolCall,
                );
                const summary = summarizeReactFold(fold);
                const anyRunning = toolGroupAnyRunning(toolMsgs);
                const showGroupDone = toolGroupFlowComplete(toolMsgs);
                const runningToolName = firstRunningToolName(toolMsgs);
                const isLastFold = id === lastReactFoldId;

                return (
                  <Fade in key={id} timeout={300}>
                    <Box
                      sx={{
                        width: "100%",
                        minWidth: 0,
                        maxWidth: "100%",
                      }}
                    >
                      <Box
                        sx={{
                          width: "100%",
                          minWidth: 0,
                          maxWidth: "100%",
                          borderRadius: `${BUBBLE_RADIUS_PX}px`,
                          bgcolor: CHAT.agentBubbleBg,
                          border: `1px solid ${CHAT.agentBubbleBorder}`,
                          px: 1.75,
                          py: 1.25,
                          fontFamily: CHAT.font,
                          overflow: "hidden",
                        }}
                      >
                        <Stack
                          direction="row"
                          alignItems="center"
                          spacing={1}
                          onClick={() => toggleToolGroupExpand(id)}
                          sx={{
                            cursor: "pointer",
                            userSelect: "none",
                            minWidth: 0,
                          }}
                        >
                          <ExpandMore
                            sx={{
                              fontSize: 14,
                              color: CHAT.toolIcon,
                              transform: expandedToolGroups.has(id)
                                ? "rotate(0deg)"
                                : "rotate(-90deg)",
                              transition: "transform 0.15s",
                            }}
                          />
                          <Typography
                            sx={{
                              fontSize: 12,
                              color: CHAT.textMuted,
                              flex: 1,
                              minWidth: 0,
                              overflowWrap: "anywhere",
                              wordBreak: "break-word",
                            }}
                          >
                            {summary}
                            {anyRunning && runningToolName
                              ? ` · ${formatToolDisplayName(runningToolName)}`
                              : ""}
                            {!anyRunning &&
                              isLastFold &&
                              activityIsStreaming &&
                              ` · ${waitingFirstChunk ? "推理中" : "解析输出"}`}
                          </Typography>
                          {anyRunning && (
                            <Chip
                              size="small"
                              label={
                                runningToolName
                                  ? formatToolDisplayName(runningToolName)
                                  : "运行中"
                              }
                              sx={{ height: 22, fontSize: 11 }}
                            />
                          )}
                          {showGroupDone && (
                            <Chip
                              size="small"
                              icon={<CheckCircle fontSize="small" />}
                              label="Done"
                              color="success"
                              variant="outlined"
                              sx={{ height: 22, fontSize: 11 }}
                            />
                          )}
                        </Stack>

                        <Collapse in={expandedToolGroups.has(id)}>
                          <Box
                            sx={{
                              mt: 1.25,
                              pl: 1.75,
                              ml: 0.5,
                              borderLeft: `2px solid ${CHAT.agentBubbleBorder}`,
                              display: "flex",
                              flexDirection: "column",
                              gap: 1.25,
                            }}
                          >
                            {fold.map((message) => {
                              if (message.role === "assistant") {
                                if (!message.content?.trim()) return null;
                                return (
                                  <Box key={message.id} sx={{ pb: 0.25 }}>
                                    <Box
                                      sx={{
                                        fontSize: 12,
                                        color: CHAT.textMuted,
                                        lineHeight: 1.45,
                                        "& p": { m: 0 },
                                      }}
                                    >
                                      {renderMessageContent(
                                        message.content,
                                        "agent",
                                      )}
                                    </Box>
                                  </Box>
                                );
                              }
                              if (message.role !== "tool" || !message.toolCall)
                                return null;
                              const tc = message.toolCall;
                              const StepIcon = toolRowIcon(tc.name);
                              const displayOutput = toolDisplayOutputText(
                                message,
                                tc,
                              );
                              const hasInput = Boolean(
                                tc.input && tc.input.trim(),
                              );
                              const hasOutput = Boolean(displayOutput);
                              const isBash =
                                tc.name === "bash" ||
                                tc.name.toLowerCase().includes("bash");
                              const commandSectionLabel = isBash
                                ? "Command"
                                : tc.name;
                              const nestedKey = toolNestedPanelKey(
                                id,
                                message.id,
                              );
                              const panelTitle = toolCallPanelTitle(
                                tc.input,
                                tc.name,
                              );
                              const nestedOpen = getNestedToolPanelOpen(
                                nestedKey,
                                tc,
                                nestedToolPanelOpen,
                              );

                              return (
                                <Box key={message.id}>
                                  {message.prefaceBeforeTools ? (
                                    <Typography
                                      sx={{
                                        fontSize: 12,
                                        color: CHAT.textMuted,
                                        lineHeight: 1.45,
                                        whiteSpace: "pre-wrap",
                                        wordBreak: "break-word",
                                        pb: 0.75,
                                      }}
                                    >
                                      {message.prefaceBeforeTools}
                                    </Typography>
                                  ) : null}

                                  <Box
                                    sx={{
                                      borderRadius: "10px",
                                      border: `1px solid ${CHAT.agentBubbleBorder}`,
                                      bgcolor: CHAT.toolCallCardBg,
                                      overflow: "hidden",
                                    }}
                                  >
                                    <Stack
                                      direction="row"
                                      alignItems="center"
                                      spacing={1}
                                      onClick={() =>
                                        toggleNestedToolPanel(
                                          id,
                                          message.id,
                                          tc,
                                        )
                                      }
                                      sx={{
                                        cursor: "pointer",
                                        userSelect: "none",
                                        px: 1.25,
                                        py: 0.85,
                                        "&:hover": {
                                          bgcolor: "action.hover",
                                        },
                                      }}
                                    >
                                      <ExpandMore
                                        sx={{
                                          fontSize: 18,
                                          color: CHAT.toolIcon,
                                          flexShrink: 0,
                                          transform: nestedOpen
                                            ? "rotate(0deg)"
                                            : "rotate(-90deg)",
                                          transition: "transform 0.15s",
                                        }}
                                      />
                                      <StepIcon
                                        sx={{
                                          fontSize: 16,
                                          color: CHAT.toolIcon,
                                          flexShrink: 0,
                                        }}
                                      />
                                      <Typography
                                        sx={{
                                          fontSize: 12,
                                          fontWeight: 600,
                                          color: CHAT.textPrimary,
                                          flex: 1,
                                          lineHeight: 1.35,
                                          wordBreak: "break-word",
                                        }}
                                      >
                                        {panelTitle}
                                      </Typography>
                                      {tc.status === "running" && (
                                        <Chip
                                          size="small"
                                          label="Running"
                                          sx={{
                                            height: 22,
                                            fontSize: 11,
                                            flexShrink: 0,
                                          }}
                                        />
                                      )}
                                      {tc.status === "error" && (
                                        <Chip
                                          size="small"
                                          label="Error"
                                          color="error"
                                          variant="outlined"
                                          sx={{
                                            height: 22,
                                            fontSize: 11,
                                            flexShrink: 0,
                                          }}
                                        />
                                      )}
                                    </Stack>

                                    <Collapse in={nestedOpen}>
                                      <Box
                                        sx={{
                                          px: 1.25,
                                          pb: 1.25,
                                          pt: 0,
                                          borderTop: `1px solid ${alpha(CHAT.agentBubbleBorder, 0.85)}`,
                                        }}
                                      >
                                        <Typography
                                          sx={{
                                            fontSize: 10,
                                            color: CHAT.labelMuted,
                                            mb: 0.75,
                                            fontWeight: 500,
                                          }}
                                        >
                                          {tc.name}
                                        </Typography>

                                        {(hasInput || hasOutput) && (
                                          <Stack
                                            direction="column"
                                            spacing={1}
                                            sx={{ width: "100%" }}
                                          >
                                            {hasInput && (
                                              <Box>
                                                <Typography
                                                  sx={{
                                                    fontSize: 10,
                                                    color: CHAT.labelMuted,
                                                    mb: 0.5,
                                                    fontWeight: 400,
                                                  }}
                                                >
                                                  {commandSectionLabel}
                                                </Typography>
                                                <Box
                                                  sx={{
                                                    borderRadius: "6px",
                                                    border: `1px solid ${CHAT.agentBubbleBorder}`,
                                                    bgcolor: "transparent",
                                                    p: 1,
                                                    maxHeight: 200,
                                                    maxWidth: "100%",
                                                    overflowY: "auto",
                                                    overflowX: "hidden",
                                                  }}
                                                >
                                                  <Typography
                                                    component="pre"
                                                    sx={{
                                                      m: 0,
                                                      fontFamily:
                                                        "Menlo, Monaco, Consolas, monospace",
                                                      fontSize: 11,
                                                      lineHeight: 1.45,
                                                      color: CHAT.textPrimary,
                                                      whiteSpace: "pre-wrap",
                                                      wordBreak: "break-word",
                                                    }}
                                                  >
                                                    {tc.input}
                                                  </Typography>
                                                </Box>
                                              </Box>
                                            )}
                                            {hasOutput && (
                                              <Box>
                                                <Typography
                                                  sx={{
                                                    fontSize: 10,
                                                    color: CHAT.labelMuted,
                                                    mb: 0.5,
                                                    fontWeight: 400,
                                                  }}
                                                >
                                                  Output
                                                </Typography>
                                                <Box
                                                  sx={{
                                                    borderRadius: "6px",
                                                    border: `1px solid ${CHAT.agentBubbleBorder}`,
                                                    bgcolor: CHAT.outputBg,
                                                    p: 1,
                                                    maxHeight: 320,
                                                    maxWidth: "100%",
                                                    overflowY: "auto",
                                                    overflowX: "hidden",
                                                  }}
                                                >
                                                  <Typography
                                                    component="pre"
                                                    sx={{
                                                      m: 0,
                                                      fontFamily:
                                                        "Inter, system-ui, sans-serif",
                                                      fontSize: 10,
                                                      lineHeight: 1.35,
                                                      color: CHAT.textMuted,
                                                      whiteSpace: "pre-wrap",
                                                      wordBreak: "break-word",
                                                    }}
                                                  >
                                                    {displayOutput}
                                                  </Typography>
                                                </Box>
                                              </Box>
                                            )}
                                          </Stack>
                                        )}

                                        {!hasInput && !hasOutput && (
                                          <Typography
                                            sx={{
                                              fontSize: 12,
                                              color: CHAT.textMuted,
                                              fontStyle: "italic",
                                            }}
                                          >
                                            No command or output yet.
                                          </Typography>
                                        )}
                                      </Box>
                                    </Collapse>
                                  </Box>
                                </Box>
                              );
                            })}

                            {showGroupDone && (
                              <Stack
                                direction="row"
                                alignItems="center"
                                spacing={1}
                                sx={{ pt: 0.25 }}
                              >
                                <CheckCircle
                                  sx={{ fontSize: 14, color: CHAT.doneGreen }}
                                />
                                <Typography
                                  sx={{
                                    fontSize: 12,
                                    fontWeight: 600,
                                    color: CHAT.toolIcon,
                                  }}
                                >
                                  Done
                                </Typography>
                              </Stack>
                            )}
                          </Box>
                        </Collapse>
                      </Box>
                    </Box>
                  </Fade>
                );
              }

              const message = item.message;
              const dividerBefore = item.dividerBefore === true;
              return (
                <Fade in key={message.id} timeout={300}>
                  <Box
                    sx={{
                      display: "flex",
                      flexDirection: "column",
                      width: "100%",
                      maxWidth: "100%",
                      gap: dividerBefore ? 1.5 : 0,
                    }}
                  >
                    {dividerBefore && (
                      <Divider
                        sx={{
                          borderColor: CHAT.agentBubbleBorder,
                          "&::before, &::after": {
                            borderColor: CHAT.agentBubbleBorder,
                          },
                        }}
                      />
                    )}
                    <Box
                      sx={{
                        display: "flex",
                        justifyContent:
                          message.role === "user" ? "flex-end" : "flex-start",
                        width: "100%",
                        minWidth: 0,
                        maxWidth: "100%",
                      }}
                    >
                      {message.role === "user" ? (
                        <Box
                          sx={{
                            minWidth: 0,
                            width: "fit-content",
                            maxWidth: USER_BUBBLE_MAX_CSS,
                            px: 1.75,
                            py: 1.25,
                            borderRadius: `${BUBBLE_RADIUS_PX}px`,
                            border: `1px solid ${CHAT.userBubbleBorder}`,
                            background: CHAT.userGrad,
                            color: CHAT.userBubbleText,
                            fontFamily: CHAT.font,
                            overflow: "hidden",
                          }}
                        >
                          <Typography
                            sx={{
                              fontSize: 13,
                              lineHeight: 1.45,
                              whiteSpace: "pre-wrap",
                              wordBreak: "break-word",
                              overflowWrap: "anywhere",
                            }}
                          >
                            {message.content}
                          </Typography>
                        </Box>
                      ) : (
                        <Box
                          sx={{
                            width: "100%",
                            minWidth: 0,
                            maxWidth: "100%",
                            px: 1.75,
                            py: 1.25,
                            borderRadius: `${BUBBLE_RADIUS_PX}px`,
                            bgcolor: CHAT.agentBubbleBg,
                            border: `1px solid ${CHAT.agentBubbleBorder}`,
                            fontFamily: CHAT.font,
                            overflow: "hidden",
                          }}
                        >
                          {renderMessageContent(message.content, "agent")}
                        </Box>
                      )}
                    </Box>
                  </Box>
                </Fade>
              );
            })}

            {/* Streaming: final summary text only — divider appears on the persisted assistant row after complete */}
            {isStreaming && currentResponse && (
              <Fade in timeout={200}>
                <Box
                  sx={{
                    width: "100%",
                    minWidth: 0,
                    maxWidth: "100%",
                    px: 1.75,
                    py: 1.25,
                    borderRadius: `${BUBBLE_RADIUS_PX}px`,
                    bgcolor: CHAT.agentBubbleBg,
                    border: `1px solid ${CHAT.agentBubbleBorder}`,
                    fontFamily: CHAT.font,
                    overflow: "hidden",
                  }}
                >
                  {renderMessageContent(currentResponse, "agent")}
                  <Box
                    component="span"
                    sx={{
                      display: "inline-block",
                      width: 8,
                      height: 16,
                      bgcolor: CHAT.accent,
                      ml: 0.5,
                      animation: "pulse 1s ease-in-out infinite",
                      "@keyframes pulse": {
                        "0%, 100%": { opacity: 1 },
                        "50%": { opacity: 0.3 },
                      },
                    }}
                  />
                </Box>
              </Fade>
            )}

            {/* Implicit Memory Indexing Status */}
            {indexingStatus !== "idle" && !isStreaming && (
              <Fade in timeout={300}>
                <Box
                  sx={{
                    width: "100%",
                    minWidth: 0,
                    maxWidth: "100%",
                    px: 1.5,
                    py: 0.75,
                    borderRadius: `${BUBBLE_RADIUS_PX}px`,
                    bgcolor:
                      indexingStatus === "error"
                        ? alpha(theme.palette.error.main, 0.06)
                        : alpha(theme.palette.success.main, 0.04),
                    border: `1px solid ${
                      indexingStatus === "error"
                        ? alpha(theme.palette.error.main, 0.2)
                        : alpha(theme.palette.success.main, 0.15)
                    }`,
                    display: "flex",
                    alignItems: "center",
                    gap: 1,
                  }}
                >
                  {indexingStatus === "indexing" ? (
                    <CircularProgress size={14} thickness={4} />
                  ) : (
                    <CheckIcon
                      sx={{
                        fontSize: 16,
                        color: "success.main",
                      }}
                    />
                  )}
                  <Typography
                    variant="caption"
                    sx={{
                      color:
                        indexingStatus === "error"
                          ? "error.main"
                          : "text.secondary",
                      fontSize: "0.75rem",
                    }}
                  >
                    {indexingStatus === "indexing"
                      ? "正在更新隐性记忆索引..."
                      : indexingStatus === "completed"
                        ? "隐性记忆索引已更新"
                        : "隐性记忆索引更新失败"}
                  </Typography>
                </Box>
              </Fade>
            )}

            <div ref={messagesEndRef} />
          </Box>

          {/* Input Area - Matching pencil design */}
          <Box
            sx={{
              p: 1.5,
              borderTop: 1,
              borderColor: "divider",
              bgcolor: "background.paper",
            }}
          >
            <Collapse in={needsWorkspacePath}>
              <Alert
                severity="info"
                icon={<FolderOpen fontSize="inherit" />}
                sx={{ mb: 1.5, borderRadius: 2 }}
                action={
                  <Button
                    color="inherit"
                    size="small"
                    variant="outlined"
                    onClick={handlePickProjectFolder}
                    sx={{ whiteSpace: "nowrap", fontWeight: 600 }}
                  >
                    选择文件夹
                  </Button>
                }
              >
                请为此对话选择工作目录（代码与工具将相对于该路径）。选择后会自动保存并隐藏此提示。
              </Alert>
            </Collapse>
            <Box
              sx={{
                width: "100%",
                maxWidth: USER_BUBBLE_MAX_CSS,
                mx: "auto",
              }}
            >
              <ChatComposer
                sessionId={sessionId}
                workspacePath={
                  currentSession?.workingDirectory ??
                  currentSession?.projectPath ??
                  ""
                }
                needsWorkspacePath={needsWorkspacePath}
                onPickWorkspace={handlePickProjectFolder}
                input={input}
                onInputChange={setInput}
                onKeyDown={handleKeyDown}
                inputRef={inputRef}
                isStreaming={isStreaming}
                isConnecting={isConnecting}
                onCancel={handleCancel}
              />
            </Box>
          </Box>
        </Box>
      )}

      <Snackbar
        open={Boolean(bgToast)}
        autoHideDuration={7000}
        onClose={(_, reason) => {
          if (reason === "clickaway") return;
          setBgToast(null);
        }}
        anchorOrigin={{ vertical: "bottom", horizontal: "center" }}
      >
        <Alert
          onClose={() => setBgToast(null)}
          severity="info"
          variant="filled"
          sx={{ width: "100%", maxWidth: 560 }}
        >
          {bgToast}
        </Alert>
      </Snackbar>

      <Snackbar
        key={pathToastKey}
        open={Boolean(pathRequiredToast)}
        autoHideDuration={5000}
        onClose={(_, reason) => {
          if (reason === "clickaway") return;
          setPathRequiredToast(null);
        }}
        anchorOrigin={{ vertical: "bottom", horizontal: "center" }}
        sx={{ zIndex: (t) => t.zIndex.snackbar + 1 }}
      >
        <Alert
          onClose={() => setPathRequiredToast(null)}
          severity="warning"
          variant="filled"
          sx={{ width: "100%", maxWidth: 560 }}
        >
          {pathRequiredToast}
        </Alert>
      </Snackbar>
    </Box>
  );
}
