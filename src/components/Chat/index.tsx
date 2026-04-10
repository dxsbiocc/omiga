import { useState, useEffect, useRef, useMemo, useCallback } from "react";
import { flushSync } from "react-dom";
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
  Dialog,
  DialogTitle,
  DialogContent,
  DialogActions,
  TextField,
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
  InsertDriveFile as InsertDriveFileIcon,
  Summarize as SummarizeIcon,
  Replay as ReplayIcon,
  Edit as EditIcon,
  ContentCopy as ContentCopyIcon,
  InfoOutlined as InfoOutlinedIcon,
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
  useChatComposerStore,
  type PermissionMode,
  type Message as StoreMessage,
  isPlaceholderSessionTitle,
  titleFromFirstUserMessage,
  UNUSED_SESSION_LABEL,
  shouldShowNewSessionPlaceholder,
  isUnsetWorkspacePath,
} from "../../state";
import { Terminal } from "../Terminal";
import { ChatComposer } from "./ChatComposer";
import type { AskUserQuestionItem } from "./AskUserQuestionWizard";
import { getChatTokens } from "./chatTokens";
import type { BackgroundAgentTask } from "./backgroundAgentTypes";
import {
  canSendFollowUpToTask,
  shortBgTaskLabel,
} from "./backgroundAgentTypes";
import { BackgroundAgentTranscriptDrawer } from "./BackgroundAgentTranscriptDrawer";
import { AgentSessionStatus } from "./AgentSessionStatus";
import { formatToolDisplayName } from "../../utils/executionSurfaceLabel";
import { parseNextStepSuggestionsFromMarkdown } from "../../utils/parseAssistantNextSteps";

/** SQLite `messages.id` shape — used to pass `retryFromUserMessageId` (not temp `user-…` ids). */
function isPersistedMessageIdForRetry(id: string): boolean {
  return /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(
    id.trim(),
  );
}

interface ChatProps {
  sessionId: string;
}

interface SchedulerPlan {
  planId: string;
  subtasks: Array<{
    id: string;
    description: string;
    agentType: string;
    dependencies: string[];
    critical: boolean;
    estimatedSecs: number;
  }>;
  selectedAgents: string[];
  estimatedDurationSecs: number;
}

interface InitialTodoItem {
  id: string;
  content: string;
  status: "pending" | "in_progress" | "completed";
}

interface Message {
  id: string;
  role: "user" | "assistant" | "tool";
  content: string;
  /** 发送时选择的 Composer Agent，用于用户气泡展示 */
  composerAgentType?: string;
  /** 发送时附加的工作区相对路径（@ 选择） */
  composerAttachedPaths?: string[];
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
  /** 调度系统生成的任务执行计划 */
  schedulerPlan?: SchedulerPlan;
  /** 本轮结束后由后端 LLM 生成的快捷追问（优先于本地启发式建议） */
  followUpSuggestions?: Array<{ label: string; prompt: string }>;
  /** 独立 LLM 生成的可选回合要点摘要（非每轮都输出） */
  turnSummary?: string;
  /** 主对话 LLM 回合结束后的 token 统计（供应商原始 prompt/completion 口径） */
  tokenUsage?: {
    input: number;
    output: number;
    total?: number;
    provider?: string;
  };
  /** Plan mode 初始 todos（与后端 round 对齐，仅存本地） */
  initialTodos?: InitialTodoItem[];
}

/** Build payload text: `@a @b` + optional body (matches composer chips + input). */
function mergeComposerPathsAndBody(paths: string[], body: string): string {
  const pathLine = paths.length > 0 ? paths.map((p) => `@${p}`).join(" ") : "";
  if (pathLine && body) return `${pathLine}\n\n${body}`;
  return pathLine || body;
}

/** One item in the main-session FIFO queue while a previous turn is still streaming. */
interface QueuedMainSend {
  id: string;
  body: string;
  composerAttachedPaths: string[];
  composerAgentType: string;
  permissionMode: PermissionMode;
}

/** User bubble: show body only when paths are shown as chips (content still stores full payload). */
function stripLeadingPathPrefixFromMerged(
  full: string,
  paths: string[],
): string {
  if (!paths.length) return full;
  const pathLine = paths.map((p) => `@${p}`).join(" ");
  const prefix = `${pathLine}\n\n`;
  if (full.startsWith(prefix)) return full.slice(prefix.length);
  if (full === pathLine) return "";
  return full;
}

/** `@a @b` + body 与 `composerAttachedPaths` 是否仍一致 */
function pathsStillMatchMergedContent(
  paths: string[],
  content: string,
): boolean {
  if (paths.length === 0) return true;
  const pathLine = paths.map((p) => `@${p}`).join(" ");
  const prefix = `${pathLine}\n\n`;
  return content.startsWith(prefix) || content.trim() === pathLine;
}

function formatUserMessageTimestamp(ts: number | undefined): string {
  try {
    return new Date(ts ?? Date.now()).toLocaleString(undefined, {
      year: "numeric",
      month: "2-digit",
      day: "2-digit",
      hour: "2-digit",
      minute: "2-digit",
    });
  } catch {
    return "";
  }
}

/** Shown as a user line + sent to the model to continue after the user cancelled a stream. */
const RESUME_AFTER_CANCEL_PROMPT =
  "请从上一轮中断处继续完成回复，衔接已有内容，不要重复已完整输出的段落。";

/** Persist full transcript (including tool rows) to the session store. */
function chatMessageToStore(m: Message): StoreMessage {
  return {
    id: m.id,
    role: m.role,
    content: m.content,
    composerAgentType: m.composerAgentType,
    composerAttachedPaths: m.composerAttachedPaths,
    followUpSuggestions: m.followUpSuggestions,
    turnSummary: m.turnSummary,
    tokenUsage: m.tokenUsage,
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
    | "thinking"
    | "tool_use"
    | "tool_result"
    | "ask_user_pending"
    | "error"
    | "cancelled"
    | "turn_summary"
    | "follow_up_suggestions"
    | "token_usage"
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
  if (n.includes("ripgrep") || n.includes("grep")) return SearchIcon;
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
  if (n.includes("ripgrep") || n.includes("grep")) return "代码搜索";
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
    if (n.includes("ripgrep") || n.includes("grep")) {
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
        // Build O(1) lookup: toolCallId → {index, message} for all tool rows in segment.
        // Previously used segment.findIndex() inside a nested loop — O(N²).
        const toolRowByCallId = new Map<string, { idx: number; msg: Message }>();
        for (let si = 0; si < segment.length; si++) {
          const m = segment[si];
          if (m.role === "tool" && m.toolCall?.id) {
            // Only record first occurrence; duplicates stay unconsumed.
            if (!toolRowByCallId.has(m.toolCall.id)) {
              toolRowByCallId.set(m.toolCall.id, { idx: si, msg: m });
            }
          }
        }

        const fold: Message[] = [];
        const consumedToolIdx = new Set<number>();
        for (let si = 0; si < segment.length; si++) {
          const m = segment[si];
          if (m.role === "assistant") {
            fold.push(m);
            const list = m.toolCallsList;
            if (list?.length) {
              for (const tc of list) {
                const entry = toolRowByCallId.get(tc.id);
                let output = "";
                if (entry && !consumedToolIdx.has(entry.idx)) {
                  consumedToolIdx.add(entry.idx);
                  output = (
                    entry.msg.toolCall?.output ??
                    entry.msg.content ??
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

/** 调度计划显示组件 */
function SchedulerPlanDisplay({ plan }: { plan: SchedulerPlan }) {
  const theme = useTheme();
  const [expanded, setExpanded] = useState(false);

  // 获取并行执行组
  const getParallelGroups = () => {
    const groups: string[][] = [];
    const completed = new Set<string>();
    const remaining = plan.subtasks.map((t) => t.id);

    while (remaining.length > 0) {
      const currentGroup: string[] = [];
      const stillRemaining: string[] = [];

      for (const taskId of remaining) {
        const task = plan.subtasks.find((t) => t.id === taskId);
        if (task) {
          const depsSatisfied = task.dependencies.every((dep) =>
            completed.has(dep),
          );
          if (depsSatisfied) {
            currentGroup.push(taskId);
          } else {
            stillRemaining.push(taskId);
          }
        }
      }

      if (currentGroup.length === 0 && stillRemaining.length > 0) {
        currentGroup.push(stillRemaining.shift()!);
      }

      currentGroup.forEach((id) => completed.add(id));
      groups.push(currentGroup);
      remaining.length = 0;
      remaining.push(...stillRemaining);
    }

    return groups;
  };

  const groups = getParallelGroups();

  // Agent 颜色映射
  const getAgentColor = (agentType: string) => {
    const colors: Record<string, string> = {
      Explore: theme.palette.info.main,
      Plan: theme.palette.warning.main,
      verification: theme.palette.success.main,
      "general-purpose": theme.palette.primary.main,
    };
    return colors[agentType] || theme.palette.grey[500];
  };

  return (
    <Box
      sx={{
        borderRadius: 1.5,
        border: `1px solid ${alpha(theme.palette.primary.main, 0.2)}`,
        bgcolor: alpha(theme.palette.primary.main, 0.03),
        overflow: "hidden",
      }}
    >
      {/* 头部 */}
      <Box
        onClick={() => setExpanded(!expanded)}
        sx={{
          px: 1.5,
          py: 0.75,
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
          cursor: "pointer",
          "&:hover": {
            bgcolor: alpha(theme.palette.primary.main, 0.05),
          },
        }}
      >
        <Box sx={{ display: "flex", alignItems: "center", gap: 1 }}>
          <SmartToy sx={{ fontSize: 14, color: "primary.main" }} />
          <Typography
            variant="caption"
            sx={{ fontWeight: 600, color: "primary.main" }}
          >
            智能调度计划
          </Typography>
          <Chip
            size="small"
            label={`${plan.subtasks.length} 个子任务`}
            sx={{
              height: 18,
              fontSize: 10,
              bgcolor: alpha(theme.palette.primary.main, 0.1),
              color: "primary.main",
            }}
          />
        </Box>
        <ExpandMore
          sx={{
            fontSize: 16,
            color: "text.secondary",
            transform: expanded ? "rotate(180deg)" : "rotate(0deg)",
            transition: "transform 0.2s",
          }}
        />
      </Box>

      {/* 展开内容 */}
      <Collapse in={expanded}>
        <Box sx={{ px: 1.5, pb: 1.5 }}>
          <Typography
            variant="caption"
            sx={{ color: "text.secondary", mb: 1, display: "block" }}
          >
            预估执行时间: ~{Math.round(plan.estimatedDurationSecs / 60)} 分钟
          </Typography>

          {groups.map((group, groupIdx) => (
            <Box key={groupIdx} sx={{ mb: 1 }}>
              {groups.length > 1 && (
                <Typography
                  variant="caption"
                  sx={{
                    fontSize: 10,
                    color: "text.secondary",
                    textTransform: "uppercase",
                    letterSpacing: 0.5,
                  }}
                >
                  阶段 {groupIdx + 1}
                </Typography>
              )}
              <Box
                sx={{
                  display: "flex",
                  flexDirection: "column",
                  gap: 0.5,
                  mt: 0.5,
                }}
              >
                {group.map((taskId) => {
                  const task = plan.subtasks.find((t) => t.id === taskId);
                  if (!task) return null;

                  const globalIndex =
                    plan.subtasks.findIndex((t) => t.id === taskId) + 1;

                  return (
                    <Box
                      key={task.id}
                      sx={{
                        display: "flex",
                        alignItems: "center",
                        gap: 1,
                        py: 0.5,
                        px: 1,
                        borderRadius: 1,
                        bgcolor: "background.paper",
                        border: `1px solid ${alpha(theme.palette.divider, 0.5)}`,
                      }}
                    >
                      <Typography
                        variant="caption"
                        sx={{
                          width: 16,
                          height: 16,
                          borderRadius: "50%",
                          bgcolor: alpha(getAgentColor(task.agentType), 0.1),
                          color: getAgentColor(task.agentType),
                          display: "flex",
                          alignItems: "center",
                          justifyContent: "center",
                          fontSize: 10,
                          fontWeight: 600,
                          flexShrink: 0,
                        }}
                      >
                        {globalIndex}
                      </Typography>
                      <Box sx={{ flex: 1, minWidth: 0 }}>
                        <Typography
                          variant="caption"
                          sx={{ display: "block", fontWeight: 500 }}
                        >
                          {task.description}
                        </Typography>
                        {task.dependencies.length > 0 && (
                          <Typography
                            variant="caption"
                            sx={{ fontSize: 10, color: "text.secondary" }}
                          >
                            依赖: {task.dependencies.join(", ")}
                          </Typography>
                        )}
                      </Box>
                      <Chip
                        size="small"
                        label={task.agentType}
                        sx={{
                          height: 18,
                          fontSize: 9,
                          bgcolor: alpha(getAgentColor(task.agentType), 0.1),
                          color: getAgentColor(task.agentType),
                          fontWeight: 500,
                          flexShrink: 0,
                        }}
                      />
                      {task.critical && (
                        <Tooltip title="关键任务">
                          <Box
                            sx={{
                              width: 6,
                              height: 6,
                              borderRadius: "50%",
                              bgcolor: "warning.main",
                              flexShrink: 0,
                            }}
                          />
                        </Tooltip>
                      )}
                    </Box>
                  );
                })}
              </Box>
            </Box>
          ))}
        </Box>
      </Collapse>
    </Box>
  );
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
  /** After cancel_stream, offer header “断点继续” until a new turn completes or the user sends again. */
  const [awaitingResumeAfterCancel, setAwaitingResumeAfterCancel] =
    useState(false);
  /** Toast when a background bash command completes (`background-shell-complete`). */
  const [bgToast, setBgToast] = useState<string | null>(null);
  /** Background Agent tasks (Rust `BackgroundAgentManager`) for teammate-style follow-ups. */
  const [backgroundTasks, setBackgroundTasks] = useState<BackgroundAgentTask[]>(
    [],
  );
  /** When set, `send_message` uses `inputTarget: bg:<id>` (main transcript unchanged). */
  const [followUpTaskId, setFollowUpTaskId] = useState<string | null>(null);
  /** 就地编辑用户气泡：id + 草稿全文（与 `message.content` 同形） */
  const [userMessageEdit, setUserMessageEdit] = useState<{
    id: string;
    draft: string;
  } | null>(null);
  /** 重试前确认弹窗：说明节点后记录将删除，确认后截断并 send_message */
  const [retryConfirmForMessage, setRetryConfirmForMessage] =
    useState<Message | null>(null);
  /** Sidechain transcript drawer (`load_background_agent_transcript`). */
  const [bgTranscriptTaskId, setBgTranscriptTaskId] = useState<string | null>(
    null,
  );
  /** 未选工作目录时的提示（Snackbar，5s 自动消失）；key 用于重复触发时重置计时 */
  const [pathToastKey, setPathToastKey] = useState(0);
  const [pathRequiredToast, setPathRequiredToast] = useState<string | null>(
    null,
  );
  /** 用户气泡「复制」成功提示 */
  const [copySuccessToast, setCopySuccessToast] = useState(false);
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

  /** Mirrors `isStreaming` for handlers (stream listener) where React state is stale. */
  const isStreamingRef = useRef(false);
  useEffect(() => {
    isStreamingRef.current = isStreaming;
  }, [isStreaming]);

  /** Prevents overlapping `retryUserMessage` runs (React `isConnecting` can lag one frame behind the store). */
  const retrySendInFlightRef = useRef(false);
  /** FIFO: main-session messages enqueued while a turn is still streaming (flush one per stream end). */
  const queuedMainSendQueueRef = useRef<QueuedMainSend[]>([]);
  /** Bumps when the in-memory queue mutates so the composer list re-renders. */
  const [queueRevision, setQueueRevision] = useState(0);
  const bumpQueueUi = useCallback(() => setQueueRevision((r) => r + 1), []);
  const handleSendRef = useRef<() => Promise<void>>(async () => {});
  const flushQueuedMainSendIfAnyRef = useRef<() => void>(() => {});
  /**
   * Set immediately before `handleSend` when draining the main-session FIFO queue.
   * - Avoids stale React `input` in the `handleSend` closure right after `flushSync`.
   * - Forces main-session `send_message` (never `inputTarget: bg:`) — the queue is main-only.
   */
  const mainQueueFlushPayloadRef = useRef<QueuedMainSend | null>(null);

  const queuedMainMessagesForComposer = useMemo(() => {
    void queueRevision;
    return queuedMainSendQueueRef.current.map((item) => {
      const merged = mergeComposerPathsAndBody(
        item.composerAttachedPaths,
        item.body,
      );
      const previewText =
        merged.length > 200 ? `${merged.slice(0, 200)}…` : merged;
      return { id: item.id, previewText, fullText: merged };
    });
  }, [queueRevision]);

  const clearQueuedMainSends = useCallback(() => {
    if (queuedMainSendQueueRef.current.length === 0) return;
    queuedMainSendQueueRef.current = [];
    bumpQueueUi();
  }, [bumpQueueUi]);

  const removeQueuedAt = useCallback(
    (index: number) => {
      const q = queuedMainSendQueueRef.current;
      if (index < 0 || index >= q.length) return;
      q.splice(index, 1);
      bumpQueueUi();
    },
    [bumpQueueUi],
  );

  const moveQueuedUp = useCallback(
    (index: number) => {
      const q = queuedMainSendQueueRef.current;
      if (index <= 0 || index >= q.length) return;
      const t = q[index - 1];
      q[index - 1] = q[index];
      q[index] = t;
      bumpQueueUi();
    },
    [bumpQueueUi],
  );

  const editQueuedAt = useCallback(
    (index: number) => {
      const q = queuedMainSendQueueRef.current;
      const item = q[index];
      if (!item) return;
      q.splice(index, 1);
      bumpQueueUi();
      flushSync(() => {
        setInput(item.body);
        const st = useChatComposerStore.getState();
        st.clearComposerAttachedPaths();
        for (const p of item.composerAttachedPaths) {
          st.addComposerAttachedPath(p);
        }
        st.setComposerAgentType(item.composerAgentType);
        st.setPermissionMode(item.permissionMode);
      });
      queueMicrotask(() => inputRef.current?.focus());
    },
    [bumpQueueUi],
  );

  /** Implicit memory indexing status for the last completed turn */
  const [indexingStatus, setIndexingStatus] = useState<
    "idle" | "indexing" | "completed" | "error"
  >("idle");

  const messagesEndRef = useRef<HTMLDivElement>(null);
  const messagesScrollRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);
  /** Populated by `follow_up_suggestions` stream frame; consumed when attaching the final assistant row */
  const pendingFollowUpSuggestionsRef = useRef<Array<{
    label: string;
    prompt: string;
  }> | null>(null);
  /** Populated by `turn_summary` stream frame; consumed when attaching the final assistant row */
  const pendingTurnSummaryRef = useRef<string | null>(null);
  /** Populated by `token_usage` stream frame; consumed when attaching the final assistant row */
  const pendingTokenUsageRef = useRef<{
    input: number;
    output: number;
    total: number;
    provider: string;
  } | null>(null);
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
    isSwitchingSession,
    hasMoreMessages,
    isLoadingMoreMessages,
    loadMoreMessages,
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
    setAwaitingResumeAfterCancel(false);
    queuedMainSendQueueRef.current = [];
    bumpQueueUi();
  }, [sessionId, bumpQueueUi]);

  const refreshBackgroundTasks = useCallback(async () => {
    if (!sessionId) {
      setBackgroundTasks([]);
      return;
    }
    try {
      const tasks = await invoke<BackgroundAgentTask[]>(
        "list_session_background_tasks",
        { sessionId },
      );
      setBackgroundTasks(tasks);
      setFollowUpTaskId((prev) => {
        if (!prev) return null;
        const t = tasks.find((x) => x.task_id === prev);
        if (!t || !canSendFollowUpToTask(t.status)) return null;
        return prev;
      });
    } catch {
      setBackgroundTasks([]);
    }
  }, [sessionId]);

  const handleCancelBackgroundTask = useCallback(
    async (taskId: string) => {
      if (!sessionId) return;
      try {
        await invoke<BackgroundAgentTask>("cancel_background_agent_task", {
          sessionId,
          taskId,
        });
        await refreshBackgroundTasks();
        setFollowUpTaskId((prev) => (prev === taskId ? null : prev));
        setBgTranscriptTaskId((prev) => (prev === taskId ? null : prev));
      } catch (e) {
        console.error("Failed to cancel background task:", e);
      }
    },
    [sessionId, refreshBackgroundTasks],
  );

  const handleOpenBackgroundTranscript = useCallback((taskId: string) => {
    setBgTranscriptTaskId(taskId);
  }, []);

  const bgTranscriptLabel = useMemo(() => {
    if (!bgTranscriptTaskId) return undefined;
    const t = backgroundTasks.find((x) => x.task_id === bgTranscriptTaskId);
    if (!t) return undefined;
    return `${t.agent_type}: ${shortBgTaskLabel(t, 72)}`;
  }, [bgTranscriptTaskId, backgroundTasks]);

  useEffect(() => {
    setFollowUpTaskId(null);
    setBgTranscriptTaskId(null);
    void refreshBackgroundTasks();
  }, [sessionId, refreshBackgroundTasks]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    const setup = async () => {
      const u1 = await listen("background-agent-update", () => {
        void refreshBackgroundTasks();
      });
      const u2 = await listen("background-agent-complete", () => {
        void refreshBackgroundTasks();
      });
      unlisten = () => {
        u1();
        u2();
      };
    };
    void setup();
    return () => {
      unlisten?.();
    };
  }, [refreshBackgroundTasks]);

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

  // Scroll to bottom when messages change (include isStreaming so the thread settles after a turn completes)
  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: "auto" });
  }, [messages, currentResponse, isConnecting, waitingFirstChunk, isStreaming]);

  // Scroll-to-top pagination: load older messages when user scrolls near the top.
  useEffect(() => {
    const el = messagesScrollRef.current;
    if (!el) return;
    const onScroll = () => {
      if (el.scrollTop < 120 && hasMoreMessages && !isLoadingMoreMessages) {
        void loadMoreMessages();
      }
    };
    el.addEventListener("scroll", onScroll, { passive: true });
    return () => el.removeEventListener("scroll", onScroll);
  }, [hasMoreMessages, isLoadingMoreMessages, loadMoreMessages]);

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
  const lastReactFoldIdRef = useRef<string | null>(null);
  useEffect(() => {
    lastReactFoldIdRef.current = lastReactFoldId;
  }, [lastReactFoldId]);

  /** Blocked `ask_user_question` — show inline picker until user submits */
  const [pendingAskUser, setPendingAskUser] = useState<{
    toolUseId: string;
    sessionId: string;
    messageId: string;
    questions: AskUserQuestionItem[];
  } | null>(null);
  const [askUserSelections, setAskUserSelections] = useState<
    Record<string, string>
  >({});

  const composerSuggestionBundle = useMemo(() => {
    const last = messages[messages.length - 1];
    if (
      last?.role === "assistant" &&
      last.followUpSuggestions &&
      last.followUpSuggestions.length > 0
    ) {
      return {
        rows: last.followUpSuggestions.map((s) => ({
          label: s.label,
          text: s.prompt,
        })),
        source: "llm" as const,
      };
    }
    if (last?.role === "assistant" && last.content) {
      const fromMd = parseNextStepSuggestionsFromMarkdown(last.content);
      if (fromMd.length > 0) {
        return { rows: fromMd, source: "markdown" as const };
      }
    }
    return { rows: [], source: "none" as const };
  }, [messages]);
  const composerSuggestions = composerSuggestionBundle.rows;
  const suggestionSource = composerSuggestionBundle.source;
  const stickyTurnSummary = useMemo(() => {
    const last = messages[messages.length - 1];
    if (last?.role === "assistant" && last.turnSummary?.trim()) {
      return last.turnSummary.trim();
    }
    return null;
  }, [messages]);
  const showNextStepSuggestions =
    Boolean(sessionId) &&
    !isStreaming &&
    !isConnecting &&
    !waitingFirstChunk &&
    !pendingAskUser &&
    !awaitingResumeAfterCancel &&
    composerSuggestions.length > 0;
  const showTurnSummaryCard =
    Boolean(sessionId) &&
    Boolean(stickyTurnSummary) &&
    !isStreaming &&
    !isConnecting &&
    !waitingFirstChunk &&
    !pendingAskUser &&
    !awaitingResumeAfterCancel;

  useEffect(() => {
    setPendingAskUser(null);
    setAskUserSelections({});
  }, [sessionId]);

  const submitPendingAskUser = useCallback(async () => {
    if (!pendingAskUser) return;
    const answers: Record<string, string> = {};
    for (const q of pendingAskUser.questions) {
      const qt = q.question.trim();
      const v = (askUserSelections[qt] ?? "").trim();
      if (!v) {
        return;
      }
      answers[qt] = v;
    }
    try {
      await invoke("submit_ask_user_answer", {
        sessionId: pendingAskUser.sessionId,
        messageId: pendingAskUser.messageId,
        toolUseId: pendingAskUser.toolUseId,
        answers,
      });
    } catch (e) {
      console.error("[Chat] submit_ask_user_answer failed", e);
    }
  }, [pendingAskUser, askUserSelections]);

  // Load messages from store when session changes
  useEffect(() => {
    try {
      if (sessionId && storeMessages.length > 0) {
        const convertedMessages: Message[] = storeMessages.map(
          (msg, index) => ({
            id: msg.id || `${sessionId}-msg-${index}`,
            role: msg.role,
            content: msg.content ?? "",
            composerAgentType: msg.composerAgentType,
            composerAttachedPaths: msg.composerAttachedPaths,
            followUpSuggestions: msg.followUpSuggestions,
            turnSummary: msg.turnSummary,
            tokenUsage: msg.tokenUsage,
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

      /**
       * If `Start` was never applied (serde/wire mismatch, or first chunk races),
       * `isConnecting` stays true and the status strip stays on「等待响应」forever.
       * Bootstrap the same state as `case "Start"` on the first real stream event.
       */
      const ensureChatStreamStarted = (clearAssistantDraft: boolean) => {
        const act = useActivityStore.getState();
        const needBootstrap =
          act.isConnecting ||
          act.executionSteps.some(
            (s) => s.id === "connect" && s.status === "running",
          );
        if (!needBootstrap) return;

        pendingFollowUpSuggestionsRef.current = null;
        pendingTurnSummaryRef.current = null;
        pendingTokenUsageRef.current = null;
        isStreamingRef.current = true;
        setIsStreaming(true);
        act.setConnecting(false);
        act.setStreaming(true, true);
        segmentStartRef.current = true;
        act.onStreamStart();
        if (clearAssistantDraft) {
          setCurrentResponse("");
        }
      };

      switch (payload.type) {
        case "Start": {
          ensureChatStreamStarted(true);
          if (isDev) {
            console.debug("[OmigaDev][AgentStream]", {
              streamId,
              type: payload.type,
            });
          }
          break;
        }
        case "text": {
          ensureChatStreamStarted(false);
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
        case "thinking": {
          ensureChatStreamStarted(false);
          const piece = typeof payload.data === "string" ? payload.data : "";
          if (isDev && piece) {
            console.debug("[OmigaDev][AgentThinking]", {
              streamId,
              len: piece.length,
            });
          }
          break;
        }
        case "tool_use": {
          ensureChatStreamStarted(false);
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
        case "ask_user_pending": {
          const raw = payload.data as
            | {
                session_id?: string;
                message_id?: string;
                tool_use_id?: string;
                questions?: AskUserQuestionItem[];
              }
            | undefined;
          const sid = raw?.session_id?.trim();
          const mid = raw?.message_id?.trim();
          const tuid = raw?.tool_use_id?.trim();
          const qs = raw?.questions;
          if (!sid || !mid || !tuid || !Array.isArray(qs) || qs.length === 0) {
            break;
          }
          if (lastReactFoldIdRef.current) {
            setExpandedToolGroups((prev) => {
              const next = new Set(prev);
              next.add(lastReactFoldIdRef.current!);
              return next;
            });
          }
          setAskUserSelections({});
          setPendingAskUser({
            toolUseId: tuid,
            sessionId: sid,
            messageId: mid,
            questions: qs,
          });
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
            setPendingAskUser((p) =>
              p?.toolUseId === toolResultMatchId ? null : p,
            );
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
          setAwaitingResumeAfterCancel(false);
          isStreamingRef.current = false;
          setIsStreaming(false);
          setPendingAskUser(null);
          setAskUserSelections({});
          pendingTokenUsageRef.current = null;
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
          flushQueuedMainSendIfAnyRef.current();
          break;
        }
        case "cancelled": {
          setAwaitingResumeAfterCancel(true);
          isStreamingRef.current = false;
          setIsStreaming(false);
          setPendingAskUser(null);
          setAskUserSelections({});
          pendingTokenUsageRef.current = null;
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
          flushQueuedMainSendIfAnyRef.current();
          break;
        }
        case "turn_summary": {
          const raw = payload.data;
          let text: string | null = null;
          if (raw != null && typeof raw === "object" && "text" in raw) {
            const v = (raw as { text?: unknown }).text;
            if (typeof v === "string" && v.trim().length > 0) {
              text = v.trim();
            }
          }
          pendingTurnSummaryRef.current = text;
          break;
        }
        case "follow_up_suggestions": {
          const raw = payload.data;
          const rows = Array.isArray(raw)
            ? (raw as Array<{ label?: unknown; prompt?: unknown }>)
            : [];
          pendingFollowUpSuggestionsRef.current = rows
            .map((r) => ({
              label: typeof r.label === "string" ? r.label.trim() : "",
              prompt: typeof r.prompt === "string" ? r.prompt.trim() : "",
            }))
            .filter((r) => r.label.length > 0 && r.prompt.length > 0)
            .slice(0, 5);
          break;
        }
        case "token_usage": {
          const raw = payload.data as
            | {
                prompt_tokens?: unknown;
                completion_tokens?: unknown;
                total_tokens?: unknown;
                provider?: unknown;
              }
            | undefined;
          const pi = raw?.prompt_tokens;
          const co = raw?.completion_tokens;
          const tot = raw?.total_tokens;
          const prov = raw?.provider;
          if (typeof pi === "number" && typeof co === "number") {
            pendingTokenUsageRef.current = {
              input: pi,
              output: co,
              total: typeof tot === "number" ? tot : pi + co,
              provider: typeof prov === "string" ? prov : "",
            };
          } else {
            pendingTokenUsageRef.current = null;
          }
          break;
        }
        case "complete": {
          setAwaitingResumeAfterCancel(false);
          isStreamingRef.current = false;
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
          const followUps = pendingFollowUpSuggestionsRef.current;
          pendingFollowUpSuggestionsRef.current = null;
          const turnSum = pendingTurnSummaryRef.current;
          pendingTurnSummaryRef.current = null;
          const tok = pendingTokenUsageRef.current;
          pendingTokenUsageRef.current = null;
          setMessages((prev) => {
            let next = prev;
            if (finalResponse) {
              const assistantMsg: Message = {
                id: `assistant-${Date.now()}`,
                role: "assistant",
                content: finalResponse,
                timestamp: Date.now(),
                ...(followUps && followUps.length > 0
                  ? { followUpSuggestions: followUps }
                  : {}),
                ...(turnSum ? { turnSummary: turnSum } : {}),
                ...(tok
                  ? {
                      tokenUsage: {
                        input: tok.input,
                        output: tok.output,
                        total: tok.total,
                        ...(tok.provider ? { provider: tok.provider } : {}),
                      },
                    }
                  : {}),
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
          flushQueuedMainSendIfAnyRef.current();
          break;
        }
      }
    });

    unlistenRef.current = unlisten;
  };

  const handleSend = async () => {
    if (!sessionId) return;
    setAwaitingResumeAfterCancel(false);

    const flushPayload = mainQueueFlushPayloadRef.current;
    if (flushPayload) {
      mainQueueFlushPayloadRef.current = null;
    }
    const restoreFlushToQueue = (p: QueuedMainSend) => {
      queuedMainSendQueueRef.current.unshift(p);
      bumpQueueUi();
    };

    const {
      composerAgentType: storeAgent,
      permissionMode: storePerm,
      composerAttachedPaths: storePaths,
    } = useChatComposerStore.getState();

    const composerAgentType = flushPayload
      ? flushPayload.composerAgentType
      : storeAgent;
    const permissionMode = flushPayload
      ? flushPayload.permissionMode
      : storePerm;
    const composerAttachedPaths = flushPayload
      ? [...flushPayload.composerAttachedPaths]
      : storePaths;

    /** Prefer ref payload after queue flush — `input` in closure can still be stale even after `flushSync`. */
    const trimmed = flushPayload ? flushPayload.body.trim() : input.trim();

    if (!trimmed && composerAttachedPaths.length === 0) {
      if (flushPayload) restoreFlushToQueue(flushPayload);
      return;
    }
    /** Composer is still in bare `/…` or `@…` picker mode — do not send as message */
    if (trimmed && /^\/[^\s]*$/u.test(trimmed)) {
      if (flushPayload) restoreFlushToQueue(flushPayload);
      return;
    }
    if (trimmed && /^@[^\s]*$/u.test(trimmed)) {
      if (flushPayload) restoreFlushToQueue(flushPayload);
      return;
    }
    if (needsWorkspacePath) {
      if (flushPayload) restoreFlushToQueue(flushPayload);
      showPathRequiredWarning();
      return;
    }

    /** Queued main sends must stay on the main transcript; ignore teammate routing for this send. */
    const isFollowUp = Boolean(followUpTaskId) && !flushPayload;

    if (isFollowUp) {
      const messageContent = mergeComposerPathsAndBody(
        composerAttachedPaths,
        trimmed,
      );
      setInput("");
      useChatComposerStore.getState().clearComposerAttachedPaths();
      try {
        const response = await invoke<{
          message_id: string;
          session_id: string;
          round_id: string;
          input_kind?: string;
        }>("send_message", {
          request: {
            content: messageContent,
            session_id: sessionId,
            project_path: currentSession?.projectPath,
            session_name: currentSession?.name,
            use_tools: true,
            inputTarget: `bg:${followUpTaskId}`,
          },
        });
        if (response.input_kind === "background_followup_queued") {
          setBgToast("已加入后台 Agent 队列，将在下一轮工具循环中处理。");
        }
        void refreshBackgroundTasks();
      } catch (error: unknown) {
        console.error("Failed to queue background follow-up:", error);
        let errorMessage = "无法发送跟进";
        if (typeof error === "string") {
          errorMessage = error;
        } else if (error && typeof error === "object") {
          const err = error as Record<string, unknown>;
          if (
            err.type === "Chat" &&
            err.details &&
            typeof err.details === "object"
          ) {
            const details = err.details as Record<string, unknown>;
            if (typeof details.message === "string") {
              errorMessage = details.message;
            }
          }
        }
        setBgToast(errorMessage);
      }
      return;
    }

    // FIFO enqueue while a turn is in flight (连接中/流式中); one item flushed per stream end.
    if (isConnecting || isStreamingRef.current) {
      queuedMainSendQueueRef.current.push({
        id:
          typeof crypto !== "undefined" &&
          typeof crypto.randomUUID === "function"
            ? crypto.randomUUID()
            : `q-${Date.now()}-${Math.random().toString(36).slice(2, 11)}`,
        body: trimmed,
        composerAttachedPaths: [...composerAttachedPaths],
        composerAgentType,
        permissionMode,
      });
      bumpQueueUi();
      setInput("");
      useChatComposerStore.getState().clearComposerAttachedPaths();
      return;
    }

    // Reset indexing status on new message
    setIndexingStatus("idle");

    useActivityStore.getState().beginExecutionRun();
    useActivityStore.getState().setConnecting(true);
    useActivityStore.getState().setStreaming(false, false);

    const isFirstMessageInSession = storeMessages.length === 0;

    const messageContent = mergeComposerPathsAndBody(
      composerAttachedPaths,
      trimmed,
    );
    // "general-purpose" 和 "auto" 都不作为显式 Agent 类型传递
    // "auto" 会触发后端的自动调度模式
    const bubbleComposerAgent =
      composerAgentType !== "general-purpose" && composerAgentType !== "auto"
        ? composerAgentType
        : undefined;
    const bubbleAttachedPaths =
      composerAttachedPaths.length > 0 ? [...composerAttachedPaths] : undefined;

    const userMessage: Message = {
      id: `user-${Date.now()}`,
      role: "user",
      content: messageContent,
      timestamp: Date.now(),
      composerAgentType: bubbleComposerAgent,
      composerAttachedPaths: bubbleAttachedPaths,
    };

    // 存储用户消息以便后续更新 schedulerPlan
    const userMessageId = userMessage.id;

    // Add to local state
    setMessages((prev) => [...prev, userMessage]);

    // Add to store for persistence
    addMessage({
      role: "user",
      content: messageContent,
      composerAgentType: bubbleComposerAgent,
      composerAttachedPaths: bubbleAttachedPaths,
      id: userMessage.id,
    });

    setInput("");
    useChatComposerStore.getState().clearComposerAttachedPaths();

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
        user_message_id?: string;
        scheduler_plan?: SchedulerPlan;
        initial_todos?: InitialTodoItem[];
      }>("send_message", {
        request: {
          content: messageContent,
          session_id: sessionId,
          project_path: currentSession?.projectPath,
          session_name: currentSession?.name,
          use_tools: true,
          composerAgentType,
          permissionMode,
        },
      });

      useChatComposerStore.getState().setComposerAgentType("general-purpose");

      // Track round_id for status updates
      setCurrentRoundId(response.round_id);

      // 如果有调度计划，更新用户消息
      if (
        response.scheduler_plan &&
        response.scheduler_plan.subtasks.length > 1
      ) {
        setMessages((prev) =>
          prev.map((msg) =>
            msg.id === userMessageId
              ? { ...msg, schedulerPlan: response.scheduler_plan }
              : msg,
          ),
        );
      }

      // 如果有初始 todos（Plan mode），添加到消息中
      if (response.initial_todos && response.initial_todos.length > 0) {
        setMessages((prev) =>
          prev.map((msg) =>
            msg.id === userMessageId
              ? { ...msg, initialTodos: response.initial_todos }
              : msg,
          ),
        );
      }

      if (response.user_message_id) {
        setMessages((prev) =>
          prev.map((m) =>
            m.id === userMessageId
              ? { ...m, id: response.user_message_id! }
              : m,
          ),
        );
        const sm = useSessionStore.getState().storeMessages;
        replaceStoreMessagesSnapshot(
          sm.map((m) =>
            m.id === userMessageId
              ? { ...m, id: response.user_message_id! }
              : m,
          ),
        );
      }

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
      queueMicrotask(() => {
        isStreamingRef.current = false;
        flushQueuedMainSendIfAnyRef.current();
      });
    }
  };

  handleSendRef.current = handleSend;

  const openEditUserMessage = useCallback((message: Message) => {
    if (message.role !== "user") return;
    setUserMessageEdit({ id: message.id, draft: message.content });
  }, []);

  const saveUserMessageEdit = useCallback(() => {
    if (!userMessageEdit) return;
    const { id, draft } = userMessageEdit;
    const trimmed = draft.trim();
    if (!trimmed) return;

    setMessages((prev) => {
      const idx = prev.findIndex((m) => m.id === id);
      if (idx < 0) return prev;
      const row = prev[idx];
      if (row.role !== "user") return prev;
      const paths = row.composerAttachedPaths ?? [];
      const keepPaths = pathsStillMatchMergedContent(paths, trimmed)
        ? paths
        : undefined;
      const attached =
        keepPaths && keepPaths.length > 0 ? keepPaths : undefined;
      const updated: Message = {
        ...row,
        content: trimmed,
        composerAttachedPaths: attached,
        timestamp: Date.now(),
        schedulerPlan: undefined,
        initialTodos: undefined,
      };
      const next = [...prev.slice(0, idx), updated];
      replaceStoreMessagesSnapshot(next.map(chatMessageToStore));
      return next;
    });
    setUserMessageEdit(null);
  }, [userMessageEdit, replaceStoreMessagesSnapshot]);

  const copyUserMessageText = useCallback(async (message: Message) => {
    try {
      await navigator.clipboard.writeText(message.content);
      setCopySuccessToast(true);
    } catch (e) {
      console.error("[Chat] clipboard copy failed", e);
      setBgToast("复制失败，请检查剪贴板权限");
    }
  }, []);

  const retryUserMessage = useCallback(
    async (message: Message) => {
      if (!sessionId || message.role !== "user") return;
      if (needsWorkspacePath) {
        showPathRequiredWarning();
        return;
      }
      if (followUpTaskId) {
        setBgToast("请在主会话中重试（当前为后台跟进模式）");
        return;
      }
      const actNow = useActivityStore.getState();
      if (
        actNow.isConnecting ||
        isStreamingRef.current ||
        retrySendInFlightRef.current
      ) {
        setBgToast("请等待当前回复结束后再重试");
        return;
      }

      const idx = messages.findIndex((m) => m.id === message.id);
      if (idx < 0) return;

      const messageContent = message.content.trim();
      if (!messageContent) {
        setBgToast("消息为空，无法重试");
        return;
      }

      const truncated = messages
        .slice(0, idx + 1)
        .map((m, i) =>
          i === idx && m.role === "user"
            ? { ...m, schedulerPlan: undefined, initialTodos: undefined }
            : m,
        );

      retrySendInFlightRef.current = true;
      queuedMainSendQueueRef.current = [];
      mainQueueFlushPayloadRef.current = null;
      bumpQueueUi();

      setMessages(truncated);
      replaceStoreMessagesSnapshot(truncated.map(chatMessageToStore));
      setCurrentResponse("");
      setAwaitingResumeAfterCancel(false);

      setIndexingStatus("idle");
      useActivityStore.getState().beginExecutionRun();
      useActivityStore.getState().setConnecting(true);
      useActivityStore.getState().setStreaming(false, false);

      const composeAgent = message.composerAgentType ?? "general-purpose";
      flushSync(() => {
        useChatComposerStore.getState().setComposerAgentType(composeAgent);
      });
      const { permissionMode } = useChatComposerStore.getState();

      const userMessageId = message.id;

      try {
        const response = await invoke<{
          message_id: string;
          session_id: string;
          round_id: string;
          user_message_id?: string;
          scheduler_plan?: SchedulerPlan;
          initial_todos?: InitialTodoItem[];
        }>("send_message", {
          request: {
            content: messageContent,
            session_id: sessionId,
            project_path: currentSession?.projectPath,
            session_name: currentSession?.name,
            use_tools: true,
            composerAgentType: composeAgent,
            permissionMode,
            ...(isPersistedMessageIdForRetry(message.id)
              ? { retryFromUserMessageId: message.id }
              : {}),
          },
        });

        useChatComposerStore.getState().setComposerAgentType("general-purpose");

        setCurrentRoundId(response.round_id);

        if (
          response.scheduler_plan &&
          response.scheduler_plan.subtasks.length > 1
        ) {
          setMessages((prev) =>
            prev.map((msg) =>
              msg.id === userMessageId
                ? { ...msg, schedulerPlan: response.scheduler_plan }
                : msg,
            ),
          );
        }

        if (response.initial_todos && response.initial_todos.length > 0) {
          setMessages((prev) =>
            prev.map((msg) =>
              msg.id === userMessageId
                ? { ...msg, initialTodos: response.initial_todos }
                : msg,
            ),
          );
        }

        if (response.user_message_id) {
          setMessages((prev) =>
            prev.map((m) =>
              m.id === userMessageId
                ? { ...m, id: response.user_message_id! }
                : m,
            ),
          );
          const sm = useSessionStore.getState().storeMessages;
          replaceStoreMessagesSnapshot(
            sm.map((m) =>
              m.id === userMessageId
                ? { ...m, id: response.user_message_id! }
                : m,
            ),
          );
        }

        setCurrentStreamId(response.message_id);
        await setupStreamListener(response.message_id);
      } catch (error: unknown) {
        console.error("Failed to retry message:", error);
        useActivityStore.getState().clearTransient();
        useActivityStore.getState().resetExecutionState();

        let errorMessage = "Unknown error";
        if (typeof error === "string") {
          errorMessage = error;
        } else if (error && typeof error === "object") {
          const err = error as Record<string, unknown>;
          if (
            err.type === "Chat" &&
            err.details &&
            typeof err.details === "object"
          ) {
            const details = err.details as Record<string, unknown>;
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

        const errorMsg: Message = {
          id: `error-${Date.now()}`,
          role: "assistant",
          content: `Failed to send message: ${errorMessage}`,
          timestamp: Date.now(),
        };
        setMessages((prev) => [...prev, errorMsg]);
        queueMicrotask(() => {
          isStreamingRef.current = false;
          flushQueuedMainSendIfAnyRef.current();
        });
      } finally {
        retrySendInFlightRef.current = false;
      }
    },
    [
      sessionId,
      messages,
      needsWorkspacePath,
      followUpTaskId,
      currentSession,
      replaceStoreMessagesSnapshot,
      bumpQueueUi,
    ],
  );

  const requestRetryUserMessage = useCallback(
    (message: Message) => {
      if (!sessionId || message.role !== "user") return;
      if (needsWorkspacePath) {
        showPathRequiredWarning();
        return;
      }
      if (followUpTaskId) {
        setBgToast("请在主会话中重试（当前为后台跟进模式）");
        return;
      }
      if (
        useActivityStore.getState().isConnecting ||
        isStreamingRef.current ||
        retrySendInFlightRef.current
      ) {
        setBgToast("请等待当前回复结束后再重试");
        return;
      }
      const idx = messages.findIndex((m) => m.id === message.id);
      if (idx < 0) return;
      if (!message.content.trim()) {
        setBgToast("消息为空，无法重试");
        return;
      }
      setRetryConfirmForMessage(message);
    },
    [sessionId, needsWorkspacePath, followUpTaskId, messages],
  );

  const confirmRetryUserMessage = useCallback(() => {
    setRetryConfirmForMessage((cur) => {
      if (cur) void retryUserMessage(cur);
      return null;
    });
  }, [retryUserMessage]);

  flushQueuedMainSendIfAnyRef.current = () => {
    const q = queuedMainSendQueueRef.current;
    if (q.length === 0) return;
    const next = q.shift();
    if (!next) return;
    bumpQueueUi();
    mainQueueFlushPayloadRef.current = next;
    flushSync(() => {
      setInput(next.body);
      const st = useChatComposerStore.getState();
      st.clearComposerAttachedPaths();
      for (const p of next.composerAttachedPaths) {
        st.addComposerAttachedPath(p);
      }
      st.setComposerAgentType(next.composerAgentType);
      st.setPermissionMode(next.permissionMode);
    });
    void handleSendRef.current();
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

  const handleResumeAfterCancel = async () => {
    if (
      !sessionId ||
      isConnecting ||
      isStreaming ||
      needsWorkspacePath ||
      followUpTaskId
    ) {
      return;
    }

    setAwaitingResumeAfterCancel(false);
    setIndexingStatus("idle");

    useActivityStore.getState().beginExecutionRun();
    useActivityStore.getState().setConnecting(true);
    useActivityStore.getState().setStreaming(false, false);

    const resumeLocalUserId = `user-${Date.now()}`;

    setMessages((prev) => {
      const stripped = prev.filter((m) => !m.id.startsWith("cancelled-"));
      const userMessage: Message = {
        id: resumeLocalUserId,
        role: "user",
        content: RESUME_AFTER_CANCEL_PROMPT,
        timestamp: Date.now(),
      };
      const next = [...stripped, userMessage];
      replaceStoreMessagesSnapshot(next.map(chatMessageToStore));
      return next;
    });

    const { composerAgentType, permissionMode } =
      useChatComposerStore.getState();

    try {
      const response = await invoke<{
        message_id: string;
        session_id: string;
        round_id: string;
        user_message_id?: string;
        scheduler_plan?: SchedulerPlan;
      }>("send_message", {
        request: {
          content: RESUME_AFTER_CANCEL_PROMPT,
          session_id: sessionId,
          project_path: currentSession?.projectPath,
          session_name: currentSession?.name,
          use_tools: true,
          composerAgentType,
          permissionMode,
        },
      });

      useChatComposerStore.getState().setComposerAgentType("general-purpose");

      if (response.user_message_id) {
        setMessages((prev) =>
          prev.map((m) =>
            m.id === resumeLocalUserId
              ? { ...m, id: response.user_message_id! }
              : m,
          ),
        );
        const sm = useSessionStore.getState().storeMessages;
        replaceStoreMessagesSnapshot(
          sm.map((m) =>
            m.id === resumeLocalUserId
              ? { ...m, id: response.user_message_id! }
              : m,
          ),
        );
      }

      setCurrentRoundId(response.round_id);
      setCurrentStreamId(response.message_id);
      await setupStreamListener(response.message_id);
    } catch (error: unknown) {
      console.error("Failed to resume after cancel:", error);
      setAwaitingResumeAfterCancel(true);
      useActivityStore.getState().clearTransient();
      useActivityStore.getState().resetExecutionState();

      let errorMessage = "Unknown error";
      if (typeof error === "string") {
        errorMessage = error;
      } else if (error && typeof error === "object") {
        const err = error as Record<string, unknown>;
        if (
          err.type === "Chat" &&
          err.details &&
          typeof err.details === "object"
        ) {
          const details = err.details as Record<string, unknown>;
          if (typeof details.message === "string") {
            errorMessage = details.message;
          } else if (typeof details.kind === "string") {
            errorMessage = details.kind;
          }
        } else if (typeof err.message === "string") {
          errorMessage = err.message;
        }
      }

      const errorMsg: Message = {
        id: `error-${Date.now()}`,
        role: "assistant",
        content: `Failed to resume: ${errorMessage}`,
        timestamp: Date.now(),
      };
      setMessages((prev) => {
        const next = [...prev, errorMsg];
        replaceStoreMessagesSnapshot(next.map(chatMessageToStore));
        return next;
      });
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
                  canCancel={
                    !followUpTaskId &&
                    (isConnecting || isStreaming || waitingFirstChunk)
                  }
                  onCancel={handleCancel}
                  showResume={awaitingResumeAfterCancel && !followUpTaskId}
                  onResume={handleResumeAfterCancel}
                />
              </Stack>
            </Box>
          )}

          {/* Messages Area */}
          <Box
            ref={messagesScrollRef}
            sx={{
              flex: 1,
              minWidth: 0,
              overflowY: "auto",
              overflowX: "hidden",
              p: 3,
              display: "flex",
              flexDirection: "column",
              /* 统一：仅由 gap 控制气泡之间的间距（用户消息不再额外 pb 撑高） */
              gap: 2,
              position: "relative",
            }}
          >
            {/* Session-switch loading overlay: keeps previous messages visible
                instead of going blank. Fades in/out smoothly. */}
            <Fade in={isSwitchingSession} timeout={120} unmountOnExit>
              <Box
                sx={{
                  position: "absolute",
                  inset: 0,
                  zIndex: 10,
                  display: "flex",
                  alignItems: "center",
                  justifyContent: "center",
                  bgcolor: (t) => alpha(t.palette.background.default, 0.6),
                  backdropFilter: "blur(2px)",
                  pointerEvents: "none",
                }}
              >
                <CircularProgress size={28} thickness={3} />
              </Box>
            </Fade>

            {/* Pagination: "load older messages" indicator at the top of the list */}
            {(hasMoreMessages || isLoadingMoreMessages) && (
              <Box sx={{ display: "flex", justifyContent: "center", py: 1 }}>
                {isLoadingMoreMessages ? (
                  <CircularProgress size={20} thickness={3} />
                ) : (
                  <Typography
                    variant="caption"
                    sx={{ color: "text.disabled", userSelect: "none" }}
                  >
                    向上滚动加载更多历史消息
                  </Typography>
                )}
              </Box>
            )}
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

            {messageRenderItems.map((item, itemIndex) => {
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
                          /* 勿用 overflow:hidden：流式输出时圆角裁剪会导致新文本被边框/圆角遮住，重绘后才撑开 */
                          overflow: "visible",
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
                              color="primary"
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
                              const showAskUserPanel = Boolean(
                                pendingAskUser &&
                                tc.id &&
                                pendingAskUser.toolUseId === tc.id,
                              );
                              const nestedOpen =
                                getNestedToolPanelOpen(
                                  nestedKey,
                                  tc,
                                  nestedToolPanelOpen,
                                ) || showAskUserPanel;

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
                                          label={
                                            showAskUserPanel
                                              ? "等待你的回答"
                                              : "Running"
                                          }
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


                                        {!hasInput &&
                                          !hasOutput &&
                                          !showAskUserPanel && (
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
              const nextItem = messageRenderItems[itemIndex + 1];
              const nextRowIsUser =
                nextItem?.kind === "row" && nextItem.message.role === "user";
              const userRowPb =
                message.role === "user" ? (nextRowIsUser ? 1 : 2) : null;
              const userAttachPaths = message.composerAttachedPaths ?? [];
              const userBubbleDisplayText =
                message.role === "user" && userAttachPaths.length > 0
                  ? stripLeadingPathPrefixFromMerged(
                      message.content,
                      userAttachPaths,
                    )
                  : message.content;
              const isEditingUser =
                message.role === "user" && userMessageEdit?.id === message.id;
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
                        pt: 1,
                        pb: userRowPb !== null ? userRowPb : 2,
                      }}
                    >
                      {message.role === "user" ? (
                        <Box
                          className="user-msg-wrap"
                          sx={{
                            position: "relative",
                            display: "flex",
                            flexDirection: "column",
                            alignItems: isEditingUser ? "stretch" : "flex-end",
                            minWidth: 0,
                            width: "100%",
                            maxWidth: "100%",
                            alignSelf: "stretch",
                            pb: 1,
                            "&:hover .user-msg-hover-actions": {
                              opacity: 1,
                              pointerEvents: "auto",
                            },
                          }}
                        >
                          <Box
                            sx={{
                              position: "relative",
                              display: "flex",
                              flexDirection: "column",
                              alignItems: isEditingUser
                                ? "stretch"
                                : "flex-end",
                              width: "100%",
                              minWidth: 0,
                            }}
                          >
                            <Box
                              sx={{
                                minWidth: 0,
                                width: isEditingUser ? "100%" : "fit-content",
                                maxWidth: isEditingUser
                                  ? "100%"
                                  : USER_BUBBLE_MAX_CSS,
                                px: isEditingUser ? 2 : 1.75,
                                py: isEditingUser ? 2 : 1.25,
                                borderRadius: `${BUBBLE_RADIUS_PX}px`,
                                border: `1px solid ${
                                  isEditingUser
                                    ? CHAT.agentBubbleBorder
                                    : CHAT.userBubbleBorder
                                }`,
                                background: isEditingUser
                                  ? theme.palette.background.paper
                                  : CHAT.userGrad,
                                color: isEditingUser
                                  ? CHAT.textPrimary
                                  : CHAT.userBubbleText,
                                fontFamily: CHAT.font,
                                overflow: "hidden",
                                display: "flex",
                                flexDirection: isEditingUser ? "column" : "row",
                                flexWrap: isEditingUser ? "nowrap" : "wrap",
                                alignItems: isEditingUser
                                  ? "stretch"
                                  : "center",
                                alignContent: isEditingUser
                                  ? "stretch"
                                  : "center",
                                gap: 0.25,
                                boxShadow: isEditingUser
                                  ? theme.palette.mode === "dark"
                                    ? `0 1px 4px ${alpha(theme.palette.common.black, 0.45)}`
                                    : `0 1px 3px ${alpha(theme.palette.common.black, 0.08)}`
                                  : undefined,
                              }}
                            >
                              {message.composerAgentType ||
                              userAttachPaths.length > 0 ? (
                                <Box
                                  sx={{
                                    display: "flex",
                                    flexDirection: "row",
                                    flexWrap: "wrap",
                                    alignItems: "center",
                                    alignContent: "center",
                                    gap: 0.25,
                                    flexShrink: 0,
                                  }}
                                >
                                  {message.composerAgentType ? (
                                    <Chip
                                      size="small"
                                      variant="outlined"
                                      icon={
                                        <SmartToy
                                          sx={{ fontSize: 14, opacity: 0.9 }}
                                        />
                                      }
                                      label={`/${message.composerAgentType}`}
                                      sx={{
                                        flexShrink: 0,
                                        maxWidth: "min(100%, 220px)",
                                        height: 22,
                                        fontSize: 11,
                                        fontWeight: 600,
                                        bgcolor: CHAT.userChipBg,
                                        borderColor: CHAT.userChipBorder,
                                        color: CHAT.userBubbleText,
                                        boxShadow: `0 1px 2px ${alpha(CHAT.userBubbleText, 0.12)}`,
                                        "& .MuiChip-icon": {
                                          color: CHAT.accent,
                                          marginLeft: "6px",
                                        },
                                        "& .MuiChip-label": {
                                          px: 0.5,
                                          overflow: "hidden",
                                          textOverflow: "ellipsis",
                                        },
                                      }}
                                    />
                                  ) : null}
                                  {userAttachPaths.map((p) => (
                                    <Tooltip key={p} title={p} placement="top">
                                      <Chip
                                        size="small"
                                        variant="outlined"
                                        icon={
                                          <InsertDriveFileIcon
                                            sx={{ fontSize: 14, opacity: 0.9 }}
                                          />
                                        }
                                        label={`@${p}`}
                                        sx={{
                                          flexShrink: 0,
                                          maxWidth: "min(100%, 220px)",
                                          height: 22,
                                          fontSize: 11,
                                          fontWeight: 600,
                                          bgcolor: CHAT.userChipBg,
                                          borderColor: CHAT.userChipBorder,
                                          color: CHAT.userBubbleText,
                                          boxShadow: `0 1px 2px ${alpha(CHAT.userBubbleText, 0.12)}`,
                                          "& .MuiChip-icon": {
                                            color: CHAT.accent,
                                            marginLeft: "6px",
                                          },
                                          "& .MuiChip-label": {
                                            px: 0.5,
                                            overflow: "hidden",
                                            textOverflow: "ellipsis",
                                          },
                                        }}
                                      />
                                    </Tooltip>
                                  ))}
                                </Box>
                              ) : null}
                              {isEditingUser ? (
                                <>
                                  <TextField
                                    autoFocus
                                    multiline
                                    fullWidth
                                    minRows={4}
                                    maxRows={24}
                                    value={userMessageEdit?.draft ?? ""}
                                    onChange={(e) =>
                                      setUserMessageEdit((cur) =>
                                        cur
                                          ? { ...cur, draft: e.target.value }
                                          : null,
                                      )
                                    }
                                    onKeyDown={(e) => {
                                      if (e.key === "Escape") {
                                        e.preventDefault();
                                        setUserMessageEdit(null);
                                      }
                                      if (
                                        (e.metaKey || e.ctrlKey) &&
                                        e.key === "Enter"
                                      ) {
                                        e.preventDefault();
                                        saveUserMessageEdit();
                                      }
                                    }}
                                    variant="outlined"
                                    placeholder="编辑消息内容…"
                                    sx={{
                                      flex: "1 1 auto",
                                      minWidth: 0,
                                      width: "100%",
                                      mt: 0.25,
                                      "& .MuiOutlinedInput-root": {
                                        fontSize: 13,
                                        lineHeight: 1.45,
                                        bgcolor: CHAT.codeBg,
                                        color: CHAT.textPrimary,
                                        alignItems: "flex-start",
                                        borderRadius: `${BUBBLE_RADIUS_PX}px`,
                                      },
                                      "& .MuiOutlinedInput-notchedOutline": {
                                        borderColor: alpha(
                                          CHAT.agentBubbleBorder,
                                          0.9,
                                        ),
                                      },
                                      "& .MuiInputBase-input": {
                                        whiteSpace: "pre-wrap",
                                        wordBreak: "break-word",
                                        overflowWrap: "anywhere",
                                        px: 1,
                                      },
                                    }}
                                  />
                                  <Stack
                                    direction="row"
                                    alignItems="flex-start"
                                    spacing={1}
                                    sx={{ mt: 1.5 }}
                                  >
                                    <InfoOutlinedIcon
                                      sx={{
                                        fontSize: 18,
                                        color: CHAT.textMuted,
                                        flexShrink: 0,
                                        mt: 0.15,
                                      }}
                                    />
                                    <Typography
                                      variant="caption"
                                      sx={{
                                        color: CHAT.textMuted,
                                        lineHeight: 1.45,
                                      }}
                                    >
                                      保存后将更新本条消息。可按 Esc
                                      取消，或使用 Ctrl/⌘ + Enter 保存。
                                    </Typography>
                                  </Stack>
                                  <Stack
                                    direction="row"
                                    justifyContent="flex-end"
                                    spacing={1}
                                    sx={{ mt: 1, flexShrink: 0 }}
                                  >
                                    <Button
                                      size="small"
                                      variant="outlined"
                                      color="inherit"
                                      onClick={() => setUserMessageEdit(null)}
                                    >
                                      取消
                                    </Button>
                                    <Button
                                      size="small"
                                      variant="contained"
                                      onClick={() => saveUserMessageEdit()}
                                    >
                                      保存
                                    </Button>
                                  </Stack>
                                </>
                              ) : userBubbleDisplayText ? (
                                <Typography
                                  component="span"
                                  sx={{
                                    fontSize: 13,
                                    lineHeight: 1.45,
                                    whiteSpace: "pre-wrap",
                                    wordBreak: "break-word",
                                    overflowWrap: "anywhere",
                                    flex: "1 1 0",
                                    minWidth: 0,
                                    textAlign: "left",
                                  }}
                                >
                                  {userBubbleDisplayText}
                                </Typography>
                              ) : null}
                            </Box>
                            <Stack
                              className="user-msg-hover-actions"
                              direction="row"
                              alignItems="center"
                              justifyContent="flex-end"
                              flexWrap="nowrap"
                              sx={{
                                position: "absolute",
                                left: 0,
                                right: 0,
                                top: "100%",
                                mt: 0.5,
                                width: "100%",
                                maxWidth: "100%",
                                boxSizing: "border-box",
                                px: 0.25,
                                py: 0,
                                gap: 0.5,
                                opacity: isEditingUser ? 1 : 0,
                                pointerEvents: isEditingUser ? "auto" : "none",
                                transition: "opacity 0.15s ease",
                                zIndex: 2,
                                minWidth: 0,
                                overflowX: "auto",
                                overflowY: "hidden",
                                scrollbarWidth: "thin",
                              }}
                            >
                              <Typography
                                component="span"
                                sx={{
                                  fontSize: 11,
                                  lineHeight: 1.2,
                                  color: CHAT.textMuted,
                                  whiteSpace: "nowrap",
                                  flexShrink: 0,
                                  userSelect: "none",
                                }}
                              >
                                {formatUserMessageTimestamp(message.timestamp)}
                              </Typography>
                              {!isEditingUser ? (
                                <Tooltip title="重试">
                                  <IconButton
                                    size="small"
                                    aria-label="重试"
                                    onClick={(e) => {
                                      e.stopPropagation();
                                      requestRetryUserMessage(message);
                                    }}
                                    sx={{
                                      p: 0.35,
                                      color: CHAT.toolIcon,
                                      "&:hover": {
                                        color: CHAT.accent,
                                        bgcolor: alpha(CHAT.accent, 0.1),
                                      },
                                    }}
                                  >
                                    <ReplayIcon sx={{ fontSize: 17 }} />
                                  </IconButton>
                                </Tooltip>
                              ) : null}
                              {!isEditingUser ? (
                                <Tooltip title="编辑">
                                  <IconButton
                                    size="small"
                                    aria-label="编辑"
                                    onClick={(e) => {
                                      e.stopPropagation();
                                      openEditUserMessage(message);
                                    }}
                                    sx={{
                                      p: 0.35,
                                      color: CHAT.toolIcon,
                                      "&:hover": {
                                        color: CHAT.accent,
                                        bgcolor: alpha(CHAT.accent, 0.1),
                                      },
                                    }}
                                  >
                                    <EditIcon sx={{ fontSize: 17 }} />
                                  </IconButton>
                                </Tooltip>
                              ) : null}
                              <Tooltip title="复制">
                                <IconButton
                                  size="small"
                                  aria-label="复制"
                                  onClick={(e) => {
                                    e.stopPropagation();
                                    void copyUserMessageText(message);
                                  }}
                                  sx={{
                                    p: 0.35,
                                    color: CHAT.toolIcon,
                                    "&:hover": {
                                      color: CHAT.accent,
                                      bgcolor: alpha(CHAT.accent, 0.1),
                                    },
                                  }}
                                >
                                  <ContentCopyIcon sx={{ fontSize: 17 }} />
                                </IconButton>
                              </Tooltip>
                            </Stack>
                          </Box>
                        </Box>
                      ) : (
                        <Box
                          sx={{
                            position: "relative",
                            width: "100%",
                            minWidth: 0,
                            maxWidth: "100%",
                            px: 1.75,
                            py: 1.25,
                            pb: message.tokenUsage ? 2.25 : 1.25,
                            borderRadius: `${BUBBLE_RADIUS_PX}px`,
                            bgcolor: CHAT.agentBubbleBg,
                            border: `1px solid ${CHAT.agentBubbleBorder}`,
                            fontFamily: CHAT.font,
                            overflow: "visible",
                          }}
                        >
                          {renderMessageContent(message.content, "agent")}
                          {message.tokenUsage ? (
                            <Typography
                              component="div"
                              sx={{
                                position: "absolute",
                                right: 10,
                                bottom: 6,
                                fontSize: 10,
                                lineHeight: 1.2,
                                color: alpha(CHAT.toolIcon, 0.85),
                                userSelect: "none",
                                pointerEvents: "none",
                                textAlign: "right",
                                maxWidth: "calc(100% - 20px)",
                              }}
                            >
                              输入 {message.tokenUsage.input.toLocaleString()} ·
                              输出 {message.tokenUsage.output.toLocaleString()}
                              {message.tokenUsage.total != null &&
                              message.tokenUsage.total !==
                                message.tokenUsage.input +
                                  message.tokenUsage.output
                                ? ` · Σ ${message.tokenUsage.total.toLocaleString()}`
                                : ""}
                              {message.tokenUsage.provider
                                ? ` · ${message.tokenUsage.provider}`
                                : ""}
                            </Typography>
                          ) : null}
                        </Box>
                      )}
                    </Box>

                    {/* 调度计划显示 */}
                    {message.schedulerPlan &&
                      message.schedulerPlan.subtasks.length > 1 && (
                        <Box
                          sx={{
                            width: "100%",
                            maxWidth: USER_BUBBLE_MAX_CSS,
                            alignSelf: "flex-end",
                            mt: 0.5,
                          }}
                        >
                          <SchedulerPlanDisplay plan={message.schedulerPlan} />
                        </Box>
                      )}
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
                    py: 1.5,
                    borderRadius: `${BUBBLE_RADIUS_PX}px`,
                    bgcolor: CHAT.agentBubbleBg,
                    border: `1px solid ${CHAT.agentBubbleBorder}`,
                    fontFamily: CHAT.font,
                    overflow: "visible",
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

            {showTurnSummaryCard && stickyTurnSummary && (
              <Fade in timeout={200}>
                <Box
                  sx={{
                    width: "100%",
                    maxWidth: "100%",
                    pt: 0.5,
                    px: 0.25,
                  }}
                >
                  <Stack
                    direction="row"
                    alignItems="flex-start"
                    gap={1}
                    sx={{
                      width: "100%",
                      p: 1.25,
                      borderRadius: `${BUBBLE_RADIUS_PX}px`,
                      bgcolor: alpha(theme.palette.primary.main, 0.06),
                      border: `1px solid ${alpha(theme.palette.primary.main, 0.12)}`,
                    }}
                  >
                    <SummarizeIcon
                      sx={{
                        fontSize: 18,
                        color: "primary.main",
                        mt: 0.15,
                        flexShrink: 0,
                      }}
                    />
                    <Box sx={{ minWidth: 0, flex: 1 }}>
                      <Stack
                        direction="row"
                        alignItems="center"
                        gap={0.75}
                        sx={{ mb: 0.5 }}
                      >
                        <Typography
                          variant="caption"
                          sx={{
                            color: "text.secondary",
                            fontWeight: 700,
                            letterSpacing: 0.02,
                          }}
                        >
                          本轮要点
                        </Typography>
                        <Chip
                          label="独立 LLM"
                          size="small"
                          sx={{
                            height: 20,
                            fontSize: "0.65rem",
                            fontWeight: 600,
                            "& .MuiChip-label": { px: 0.75 },
                          }}
                        />
                      </Stack>
                      <Typography
                        variant="body2"
                        sx={{
                          color: "text.primary",
                          lineHeight: 1.55,
                          whiteSpace: "pre-wrap",
                          wordBreak: "break-word",
                        }}
                      >
                        {stickyTurnSummary}
                      </Typography>
                    </Box>
                  </Stack>
                </Box>
              </Fade>
            )}

            {showNextStepSuggestions && (
              <Fade in timeout={200}>
                <Box
                  sx={{
                    width: "100%",
                    maxWidth: "100%",
                    pt: 0.5,
                  }}
                >
                  <Typography
                    variant="caption"
                    sx={{
                      display: "block",
                      mb: 0.25,
                      color: "text.secondary",
                      fontWeight: 600,
                      letterSpacing: 0.02,
                    }}
                  >
                    下一步建议
                  </Typography>
                  <Typography
                    variant="caption"
                    sx={{
                      display: "block",
                      mb: 1,
                      color: "text.disabled",
                      fontWeight: 500,
                    }}
                  >
                    {suggestionSource === "llm"
                      ? "由独立模型根据上文生成，点击填入输入框"
                      : suggestionSource === "markdown"
                        ? "由助手正文「下一步建议」小节解析，点击填入输入框"
                        : ""}
                  </Typography>
                  <Stack
                    direction="row"
                    useFlexGap
                    flexWrap="wrap"
                    gap={1}
                    sx={{ width: "100%" }}
                  >
                    {composerSuggestions.map((s, idx) => (
                      <Tooltip
                        key={`${idx}-${s.label}`}
                        title={s.text}
                        placement="top"
                        enterDelay={400}
                      >
                        <Button
                          size="small"
                          variant="outlined"
                          color="primary"
                          onClick={() => {
                            setInput(s.text);
                            queueMicrotask(() => inputRef.current?.focus());
                          }}
                          sx={{
                            textTransform: "none",
                            borderRadius: `${BUBBLE_RADIUS_PX}px`,
                            maxWidth: "100%",
                            fontSize: 12,
                            fontWeight: 600,
                            py: 0.5,
                          }}
                        >
                          {s.label}
                        </Button>
                      </Tooltip>
                    ))}
                  </Stack>
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
                backgroundTasks={backgroundTasks}
                followUpTaskId={followUpTaskId}
                onFollowUpTaskIdChange={setFollowUpTaskId}
                allowInputWhileStreaming
                queuedMainMessages={queuedMainMessagesForComposer}
                onClearQueuedMessages={clearQueuedMainSends}
                onRemoveQueuedAt={removeQueuedAt}
                onMoveQueuedUp={moveQueuedUp}
                onEditQueuedAt={editQueuedAt}
                onCancelBackgroundTask={handleCancelBackgroundTask}
                onOpenBackgroundTranscript={handleOpenBackgroundTranscript}
                askUserQuestion={
                  pendingAskUser
                    ? {
                        resetKey: pendingAskUser.toolUseId,
                        questions: pendingAskUser.questions,
                        selections: askUserSelections,
                        onSelectionsChange: setAskUserSelections,
                        onSubmit: () => void submitPendingAskUser(),
                      }
                    : null
                }
              />
            </Box>
          </Box>
        </Box>
      )}

      <Dialog
        open={retryConfirmForMessage !== null}
        onClose={() => setRetryConfirmForMessage(null)}
        maxWidth="sm"
        fullWidth
      >
        <DialogTitle>确认重试</DialogTitle>
        <DialogContent>
          <Alert severity="warning" sx={{ mb: 0 }}>
            以本条用户消息为节点：该节点之后的全部聊天记录（含助手回复、工具结果等）将被删除，且无法恢复；确认后仅保留该条及之前内容，并基于该条重新发起请求。
          </Alert>
        </DialogContent>
        <DialogActions>
          <Button onClick={() => setRetryConfirmForMessage(null)}>取消</Button>
          <Button
            variant="contained"
            color="warning"
            onClick={confirmRetryUserMessage}
          >
            确认重试
          </Button>
        </DialogActions>
      </Dialog>

      <Snackbar
        open={copySuccessToast}
        autoHideDuration={3000}
        onClose={(_, reason) => {
          if (reason === "clickaway") return;
          setCopySuccessToast(false);
        }}
        anchorOrigin={{ vertical: "bottom", horizontal: "center" }}
        sx={{ zIndex: (t) => t.zIndex.snackbar + 1 }}
      >
        <Alert
          onClose={() => setCopySuccessToast(false)}
          severity="success"
          variant="filled"
          sx={{ width: "100%", maxWidth: 560 }}
        >
          已复制到剪贴板
        </Alert>
      </Snackbar>

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

      <BackgroundAgentTranscriptDrawer
        open={bgTranscriptTaskId !== null}
        onClose={() => setBgTranscriptTaskId(null)}
        sessionId={sessionId}
        taskId={bgTranscriptTaskId}
        taskLabel={bgTranscriptLabel}
      />

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
