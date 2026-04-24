import { useState, useEffect, useLayoutEffect, useRef, useMemo, useCallback, startTransition } from "react";
import { flushSync } from "react-dom";
import type { CSSProperties } from "react";
import type { Components } from "react-markdown";
import type { Theme } from "@mui/material/styles";
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
  Groups as GroupsIcon,
  RocketLaunch as RocketLaunchIcon,
} from "@mui/icons-material";
import { Prism as SyntaxHighlighter } from "react-syntax-highlighter";
import {
  oneDark,
  oneLight,
} from "react-syntax-highlighter/dist/esm/styles/prism";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import remarkMath from "remark-math";
import rehypeKatex from "rehype-katex";
import "katex/dist/katex.min.css";
import {
  useSessionStore,
  useActivityStore,
  useChatComposerStore,
  type PermissionMode,
  type SandboxBackend,
  type ExecutionEnvironment,
  type Message as StoreMessage,
  isPlaceholderSessionTitle,
  titleFromFirstUserMessage,
  shouldShowNewSessionPlaceholder,
  isUnsetWorkspacePath,
} from "../../state";
import { Terminal } from "../Terminal";
import { OmigaLogo } from "../OmigaLogo";
import { ChatComposer, type ChatComposerRef } from "./ChatComposer";
import type { AskUserQuestionItem } from "./AskUserQuestionWizard";
import { getChatTokens } from "./chatTokens";
import type { BackgroundAgentTask } from "./backgroundAgentTypes";
import { VisualizationRenderer } from "./viz/VisualizationRenderer";
import { DagFlow, type OmigaDagPayload } from "./DagFlow";
import { OmigaFlowchart, type OmigaFlowchartPayload } from "./OmigaFlowchart";
import { MermaidFlow } from "./viz/MermaidFlow";
import { DotFlow } from "./viz/DotFlow";
import {
  canSendFollowUpToTask,
  shortBgTaskLabel,
} from "./backgroundAgentTypes";
import { BackgroundAgentTranscriptDrawer } from "./BackgroundAgentTranscriptDrawer";
import { AgentSessionStatus } from "./AgentSessionStatus";
import { SshDirectoryTreeDialog } from "./SshDirectoryTreeDialog";
import { ReviewerVerdictList } from "../ReviewerVerdictList";
import { formatToolDisplayName } from "../../utils/executionSurfaceLabel";
import { buildPendingExecutionFeedback } from "../../utils/pendingExecutionFeedback";
import { parseNextStepSuggestionsFromMarkdown } from "../../utils/parseAssistantNextSteps";
import { extractSuggestionTooltipMarkdown } from "../../utils/suggestionTooltip";
import {
  aggregateReviewerVerdicts,
  overallReviewerHeadline,
  type BackgroundAgentTaskRow,
  type ReviewerVerdictChip,
} from "../../utils/reviewerVerdict";
import { parseWorkflowCommand } from "../../utils/workflowCommands";
import {
  OMIGA_COMPOSER_DISPATCH_EVENT,
  type ComposerDispatchDetail,
} from "../../utils/chatComposerEvents";
import { CitationLink } from "../CitationLink";
import { normalizeAgentDisplayName, useAgentStore } from "../../state/agentStore";

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
  originalRequest?: string;
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
  reviewerAgents?: string[];
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
    completedAt?: number; // Unix ms when tool finished (for duration display)
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
  /** 发送入队时的运行环境（与 composer 一致） */
  environment: ExecutionEnvironment;
  /** SSH 服务器名称（仅在 environment === "ssh" 时有效） */
  sshServer: string | null;
  sandboxBackend: SandboxBackend;
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

function compactSuggestionLabel(label: string, prompt: string): string {
  const source = (label || prompt).replace(/\r?\n/g, " ").trim();
  const cleaned = source
    .replace(/^[-*#>\s]+/u, "")
    .replace(/^\d+[.、)\]]\s*/u, "")
    .replace(/\*\*/g, "")
    .replace(/`/g, "")
    .replace(/\[(.*?)\]\((.*?)\)/g, "$1")
    .replace(/\s+/g, " ")
    .trim();
  const heading = cleaned.split(/[：:]/u)[0]?.trim() || cleaned;
  const chars = [...heading];
  if (chars.length <= 14) return heading;
  return `${chars.slice(0, 13).join("")}…`;
}

function rewriteWorkflowBodyForBackend(body: string): {
  content: string;
  workflowCommand?: "plan" | "schedule" | "team" | "autopilot";
} {
  const parsed = parseWorkflowCommand(body);
  if (!parsed) {
    return { content: body };
  }
  const taskBody = parsed.body;
  switch (parsed.command) {
    case "plan":
      return {
        content: taskBody ? `plan this ${taskBody}` : "plan this",
        workflowCommand: "plan",
      };
    case "team":
      return {
        content: taskBody ? `team ${taskBody}` : "team",
        workflowCommand: "team",
      };
    case "autopilot":
      return {
        content: taskBody ? `autopilot ${taskBody}` : "autopilot",
        workflowCommand: "autopilot",
      };
    case "schedule":
      return {
        content: taskBody || body,
        workflowCommand: "schedule",
      };
    default:
      return { content: body };
  }
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
    initialTodos: m.initialTodos,
    followUpSuggestions: m.followUpSuggestions,
    turnSummary: m.turnSummary,
    tokenUsage: m.tokenUsage,
    prefaceBeforeTools: m.prefaceBeforeTools,
    toolCallsList: m.toolCallsList,
    ...(m.timestamp !== undefined ? { timestamp: m.timestamp } : {}),
    toolCall: m.toolCall
      ? {
          id: m.toolCall.id ?? `tc-${m.id}`,
          name: m.toolCall.name,
          arguments: m.toolCall.input ?? "",
          output: m.toolCall.output,
          status: m.toolCall.status,
          ...(m.toolCall.completedAt !== undefined
            ? { completedAt: m.toolCall.completedAt }
            : {}),
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
    | "suggestions_generating"
    | "suggestions_complete"
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
  if (n === "recall") return SearchIcon;
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
  if (n === "recall") return "检索知识库";
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
        const toolRowByCallId = new Map<
          string,
          { idx: number; msg: Message }
        >();
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

/** Display title for a tool row: `description` from arguments JSON, else tool name. */
function toolCallPanelTitle(
  input: string | undefined,
  toolName: string,
): string {
  return parseToolDescriptionFromInput(input) ?? toolName;
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
function SchedulerPlanDisplay({
  plan,
  sessionId,
  onOpenReviewerTranscript,
  onExecutePlan,
  onRevisePlan,
}: {
  plan: SchedulerPlan;
  sessionId?: string;
  onOpenReviewerTranscript?: (taskId: string) => void;
  onExecutePlan?: (mode: "schedule" | "team" | "autopilot") => void;
  onRevisePlan?: () => void;
}) {
  const theme = useTheme();
  const [expanded, setExpanded] = useState(false);
  const [reviewerHeadline, setReviewerHeadline] = useState<{
    label: string;
    color: string;
  } | null>(null);
  const [reviewerVerdicts, setReviewerVerdicts] = useState<ReviewerVerdictChip[]>([]);
  const [planTasks, setPlanTasks] = useState<BackgroundAgentTaskRow[]>([]);

  useEffect(() => {
    if (!sessionId) {
      setReviewerHeadline(null);
      setReviewerVerdicts([]);
      setPlanTasks([]);
      return;
    }
    let cancelled = false;
    invoke<BackgroundAgentTaskRow[]>("list_session_background_tasks", {
      sessionId,
    })
      .then((rows) => {
        if (cancelled) return;
        const scopedRows = (rows ?? []).filter(
          (row) => row.plan_id && row.plan_id === plan.planId,
        );
        setPlanTasks(scopedRows);
        const verdicts = aggregateReviewerVerdicts(scopedRows);
        setReviewerVerdicts(verdicts);
        setReviewerHeadline(overallReviewerHeadline(verdicts));
      })
      .catch(() => {
        if (!cancelled) {
          setReviewerHeadline(null);
          setReviewerVerdicts([]);
          setPlanTasks([]);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [plan.planId, sessionId]);

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

  const runningCount = planTasks.filter(
    (task) => task.status === "Running" || task.status === "Pending",
  ).length;
  const completedCount = planTasks.filter(
    (task) => task.status === "Completed",
  ).length;
  const failedCount = planTasks.filter(
    (task) => task.status === "Failed" || task.status === "Cancelled",
  ).length;
  const executionFeed = useMemo(() => {
    const sorted = [...planTasks].sort((a, b) => {
      const aTs = a.completed_at ?? a.created_at ?? 0;
      const bTs = b.completed_at ?? b.created_at ?? 0;
      return bTs - aTs;
    });
    return sorted.slice(0, 4).map((task) => ({
      id: task.task_id,
      label:
        task.status === "Running" || task.status === "Pending"
          ? "进行中"
          : task.status === "Completed"
            ? "已完成"
            : "异常",
      color:
        task.status === "Running" || task.status === "Pending"
          ? "#0ea5e9"
          : task.status === "Completed"
            ? "#22c55e"
            : "#ef4444",
      text: `${normalizeAgentDisplayName(task.agent_type)} · ${task.description}`,
    }));
  }, [planTasks]);

  return (
    <Box
      sx={{
        borderRadius: 1.25,
        border: `1px solid ${alpha(theme.palette.primary.main, 0.14)}`,
        bgcolor: alpha(theme.palette.primary.main, 0.03),
        overflow: "hidden",
      }}
    >
      {/* 头部 */}
      <Box
        onClick={() => setExpanded(!expanded)}
        sx={{
          px: 1.25,
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
            已生成调度计划
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
          {runningCount > 0 ? (
            <Typography
              variant="caption"
              sx={{ color: "#0ea5e9", fontWeight: 600 }}
            >
              正在执行 {runningCount} 项
            </Typography>
          ) : completedCount > 0 ? (
            <Typography
              variant="caption"
              sx={{ color: "#22c55e", fontWeight: 600 }}
            >
              已完成 {completedCount} 项
            </Typography>
          ) : null}
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
        <Box sx={{ px: 1.25, pb: 1.1 }}>
          <Stack direction="row" spacing={0.5} flexWrap="wrap" useFlexGap sx={{ mb: 0.75 }}>
            {runningCount > 0 && (
              <Chip
                size="small"
                label={`${runningCount} 运行中`}
                sx={{
                  height: 18,
                  fontSize: 9,
                  bgcolor: alpha("#0ea5e9", 0.1),
                  color: "#0ea5e9",
                }}
              />
            )}
            {completedCount > 0 && (
              <Chip
                size="small"
                label={`${completedCount} 已完成`}
                sx={{
                  height: 18,
                  fontSize: 9,
                  bgcolor: alpha("#22c55e", 0.1),
                  color: "#22c55e",
                }}
              />
            )}
            {failedCount > 0 && (
              <Chip
                size="small"
                label={`${failedCount} 异常`}
                sx={{
                  height: 18,
                  fontSize: 9,
                  bgcolor: alpha("#ef4444", 0.1),
                  color: "#ef4444",
                }}
              />
            )}
            {reviewerHeadline && (
              <Chip
                size="small"
                label={reviewerHeadline.label}
                sx={{
                  height: 18,
                  fontSize: 9,
                  bgcolor: alpha(reviewerHeadline.color, 0.12),
                  color: reviewerHeadline.color,
                }}
              />
            )}
          </Stack>
          {runningCount + completedCount + failedCount > 0 ? (
            <Typography variant="caption" sx={{ color: "text.secondary", display: "block", mb: 0.5 }}>
              编排详情已收敛到右侧任务 / 编排区；这里只保留轻量摘要。
            </Typography>
          ) : (
            <Typography variant="caption" sx={{ color: "text.secondary", display: "block", mb: 0.5 }}>
              这是可审阅计划，尚未执行。确认方向后可从下方选择执行方式。
            </Typography>
          )}
          {executionFeed.length > 0 && (
            <Box sx={{ mb: 0.75 }}>
              <Typography
                variant="caption"
                sx={{ color: "text.secondary", display: "block", mb: 0.35 }}
              >
                执行动态：
              </Typography>
              <Stack spacing={0.35}>
                {executionFeed.map((item) => (
                  <Typography
                    key={item.id}
                    variant="caption"
                    sx={{
                      display: "block",
                      color: "text.secondary",
                      lineHeight: 1.35,
                    }}
                  >
                    <Box
                      component="span"
                      sx={{ color: item.color, fontWeight: 700, mr: 0.75 }}
                    >
                      {item.label}
                    </Box>
                    {item.text}
                  </Typography>
                ))}
              </Stack>
            </Box>
          )}
          <Stack spacing={0.5}>
            {plan.subtasks.slice(0, 3).map((task, index) => (
              <Box
                key={task.id}
                sx={{
                  display: "flex",
                  alignItems: "center",
                  gap: 0.75,
                  px: 0.75,
                  py: 0.5,
                  borderRadius: 1,
                  bgcolor: alpha(theme.palette.background.paper, 0.45),
                }}
              >
                <Typography
                  variant="caption"
                  sx={{
                    width: 14,
                    textAlign: "center",
                    color: "text.secondary",
                    fontWeight: 700,
                    flexShrink: 0,
                  }}
                >
                  {index + 1}
                </Typography>
                <Typography
                  variant="caption"
                  sx={{
                    flex: 1,
                    minWidth: 0,
                    fontWeight: 500,
                    whiteSpace: "nowrap",
                    overflow: "hidden",
                    textOverflow: "ellipsis",
                  }}
                >
                  {task.description}
                </Typography>
                <Chip
                  size="small"
                  label={normalizeAgentDisplayName(task.agentType)}
                  sx={{
                    height: 16,
                    fontSize: 8.5,
                    bgcolor: alpha(getAgentColor(task.agentType), 0.1),
                    color: getAgentColor(task.agentType),
                    flexShrink: 0,
                  }}
                />
              </Box>
            ))}
          </Stack>
          {onExecutePlan && runningCount + completedCount + failedCount === 0 && (
            <Stack
              direction="row"
              spacing={0.75}
              flexWrap="wrap"
              useFlexGap
              sx={{ mt: 1 }}
            >
              <Button
                size="small"
                variant="contained"
                startIcon={<SendIcon sx={{ fontSize: 14 }} />}
                onClick={(event) => {
                  event.stopPropagation();
                  onExecutePlan("schedule");
                }}
                sx={{ fontSize: 11, py: 0.35 }}
              >
                执行分析
              </Button>
              <Button
                size="small"
                variant="outlined"
                startIcon={<GroupsIcon sx={{ fontSize: 14 }} />}
                onClick={(event) => {
                  event.stopPropagation();
                  onExecutePlan("team");
                }}
                sx={{ fontSize: 11, py: 0.35 }}
              >
                协作分析
              </Button>
              <Button
                size="small"
                variant="outlined"
                startIcon={<RocketLaunchIcon sx={{ fontSize: 14 }} />}
                onClick={(event) => {
                  event.stopPropagation();
                  onExecutePlan("autopilot");
                }}
                sx={{ fontSize: 11, py: 0.35 }}
              >
                全流程分析
              </Button>
              {onRevisePlan && (
                <Button
                  size="small"
                  variant="text"
                  startIcon={<EditIcon sx={{ fontSize: 14 }} />}
                  onClick={(event) => {
                    event.stopPropagation();
                    onRevisePlan();
                  }}
                  sx={{ fontSize: 11, py: 0.35 }}
                >
                  修改计划
                </Button>
              )}
            </Stack>
          )}
          {reviewerVerdicts.length > 0 && (
            <Box sx={{ mt: 0.75 }}>
              <ReviewerVerdictList
                verdicts={reviewerVerdicts}
                title="Reviewer 摘要："
                onSelectVerdict={(verdict) => {
                  if (!verdict.taskId) return;
                  onOpenReviewerTranscript?.(verdict.taskId);
                }}
              />
            </Box>
          )}
        </Box>
      </Collapse>
    </Box>
  );
}

/**
 * Repair common GFM table breakage caused by LLMs placing newlines inside
 * table cells or embedding next-row markers mid-line.
 *
 * Two patterns handled:
 * A) Continuation lines: after a table separator row, a non-pipe line is cell
 *    continuation — join it into the previous row's last cell.
 * B) Embedded row markers: " | | " or " || " mid-line while in table context
 *    signals a new row — split the line there.
 */
function fixBrokenGfmTables(md: string): string {
  const lines = md.split("\n");
  const out: string[] = [];
  let afterSeparator = false; // true once we've seen a |---|---| row

  const isTableRow = (s: string) => s.trimStart().startsWith("|");
  const isSeparatorRow = (s: string) => /^\s*\|[\s\-:|]+\|/.test(s);

  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    const trimmed = line.trim();

    if (isSeparatorRow(trimmed)) {
      afterSeparator = true;
      // Flush any pending non-pipe continuation first (shouldn't happen, but guard)
      out.push(line);
      continue;
    }

    if (!trimmed) {
      afterSeparator = false;
      out.push(line);
      continue;
    }

    if (isTableRow(trimmed)) {
      afterSeparator = afterSeparator || false; // keep existing state
      out.push(line);
      continue;
    }

    // Non-pipe line while we're inside a table
    if (afterSeparator) {
      // Pattern B: line contains embedded row markers " | | " or " || "
      const embeddedRow = / \| \| | \|\| /;
      if (embeddedRow.test(line)) {
        const parts = line.split(/ \| \| | \|\| /);
        // First part is continuation of the previous row's last cell
        if (parts[0].trim() && out.length > 0 && isTableRow(out[out.length - 1])) {
          const prev = out[out.length - 1].trimEnd();
          out[out.length - 1] = prev.endsWith("|")
            ? prev.slice(0, -1).trimEnd() + " " + parts[0].trim() + " |"
            : prev + " " + parts[0].trim();
        } else if (parts[0].trim()) {
          out.push(parts[0].trim());
        }
        // Remaining parts become new table rows
        for (let p = 1; p < parts.length; p++) {
          if (parts[p].trim()) {
            const rowContent = parts[p].trim();
            out.push(rowContent.startsWith("|") ? rowContent : "| " + rowContent);
          }
        }
        continue;
      }

      // Pattern A: plain continuation line — merge into previous row's last cell
      if (out.length > 0 && isTableRow(out[out.length - 1])) {
        const prev = out[out.length - 1].trimEnd();
        out[out.length - 1] = prev.endsWith("|")
          ? prev.slice(0, -1).trimEnd() + " " + trimmed + " |"
          : prev + " " + trimmed;
        continue;
      }

      // Can't repair — exit table context and emit as-is
      afterSeparator = false;
    }

    out.push(line);
  }

  return out.join("\n");
}

/** How close to the bottom (px) before auto-scroll kicks in */
const AUTO_SCROLL_BOTTOM_THRESHOLD_PX = 100;

function buildMarkdownComponents(
  isAgent: boolean,
  theme: Theme,
  CHAT: ReturnType<typeof getChatTokens>,
  onImageClick: (src: string, alt: string) => void,
  onNodeClick?: (text: string) => void,
) {
  const prismStyleRaw = theme.palette.mode === "dark" ? oneDark : oneLight;
  const prismStyleFenced = prismStyleTransparentCodeSurface(
    prismStyleRaw as Record<string, CSSProperties>,
  );
  return {
  code({
    className,
    children,
  }: {
    className?: string;
    children?: React.ReactNode;
  }) {
    const match = /language-(.+)/.exec(className || "");
    const language = match ? match[1].trim() : "";
    const isInline = !className?.includes("language-");

    if (!isInline && language) {
      const blockBody = String(children).replace(/\n$/, "");

      if (language === "visualization") {
        let config: { type: string } | null = null;
        try {
          config = JSON.parse(blockBody);
        } catch {
          return (
            <Alert severity="error" sx={{ my: 1 }}>
              无效的可视化配置
            </Alert>
          );
        }
        if (!config) return null;
        return <VisualizationRenderer config={config} onNodeClick={onNodeClick} />;
      }

      if (language === "omiga-dag") {
        let dag: OmigaDagPayload | null = null;
        try {
          dag = JSON.parse(blockBody) as OmigaDagPayload;
        } catch {
          return (
            <Alert severity="error" sx={{ my: 1 }}>
              无效的 DAG 配置
            </Alert>
          );
        }
        if (!dag?.nodes?.length) return null;
        return <DagFlow data={dag} onNodeClick={onNodeClick} />;
      }

      if (language === "omiga-flowchart") {
        let fc: OmigaFlowchartPayload | null = null;
        try {
          fc = JSON.parse(blockBody) as OmigaFlowchartPayload;
        } catch {
          return (
            <Alert severity="error" sx={{ my: 1 }}>
              无效的流程图配置
            </Alert>
          );
        }
        if (!fc?.stages?.length) return null;
        return (
          <OmigaFlowchart
            data={fc}
            isAgent={isAgent}
            onStepClick={onNodeClick ? (text) => onNodeClick(text) : undefined}
          />
        );
      }

      // Raw ```mermaid fenced block → React Flow
      if (language === "mermaid") {
        return <MermaidFlow source={blockBody} onNodeClick={onNodeClick} />;
      }

      // Raw ```dot / ```graphviz fenced block → React Flow
      if (language === "dot" || language === "graphviz") {
        return <DotFlow dot={blockBody} onNodeClick={onNodeClick} />;
      }

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
      <CitationLink href={href} accentColor={isAgent ? CHAT.accent : undefined}>
        {children}
      </CitationLink>
    );
  },
  table({ children }) {
    const isDark = theme.palette.mode === "dark";
    const tableBorder = isDark
      ? alpha(theme.palette.common.white, 0.12)
      : alpha(theme.palette.common.black, 0.12);
    const theadBg = isDark
      ? alpha(theme.palette.common.white, 0.07)
      : alpha(theme.palette.common.black, 0.04);
    const tbodyRowHover = isDark
      ? alpha(theme.palette.common.white, 0.04)
      : alpha(theme.palette.common.black, 0.025);
    const wrapperBg = isDark
      ? alpha(theme.palette.common.white, 0.03)
      : theme.palette.background.paper;

    return (
      <Box
        sx={{
          overflowX: "auto",
          overflowY: "visible",
          my: 1.5,
          borderRadius: 1,
          border: `1px solid ${tableBorder}`,
          bgcolor: wrapperBg,
          "& + *": { mt: 1 },
        }}
      >
        <Box
          component="table"
          sx={{
            minWidth: "100%",
            tableLayout: "auto",
            borderCollapse: "collapse",
            fontSize: isAgent ? 12 : 13,
            color: theme.palette.text.primary,
            "& thead tr": {
              bgcolor: theadBg,
            },
            "& th": {
              border: `1px solid ${tableBorder}`,
              px: 1.5,
              py: 0.75,
              fontWeight: 600,
              whiteSpace: "nowrap",
              verticalAlign: "middle",
              textAlign: "left",
              color: theme.palette.text.primary,
            },
            "& td": {
              border: `1px solid ${tableBorder}`,
              px: 1.5,
              py: 0.75,
              verticalAlign: "top",
              wordBreak: "break-word",
              overflowWrap: "anywhere",
              minWidth: 80,
              color: theme.palette.text.primary,
            },
            "& tbody tr:hover": {
              bgcolor: tbodyRowHover,
            },
          }}
        >
          {children}
        </Box>
      </Box>
    );
  },
  img({ src, alt }) {
    const url = typeof src === "string" ? src : "";
    return (
      <Box
        component="img"
        src={url}
        alt={typeof alt === "string" ? alt : ""}
        onClick={() => onImageClick(url, typeof alt === "string" ? alt : "")}
        sx={{
          display: "block",
          maxWidth: "100%",
          height: "auto",
          borderRadius: 1,
          my: 1,
          cursor: "pointer",
          transition: "opacity 0.2s",
          "&:hover": { opacity: 0.92 },
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

  } as Components;
}

/** Pure helper — converts store messages to local Message[] for rendering. */
function convertStoreMessages(storeMessages: StoreMessage[], sessionId: string): Message[] {
  return storeMessages.map((msg, index) => ({
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
    // Use stored DB timestamp; fall back to an estimated sequence offset if missing.
    timestamp: msg.timestamp ?? (Date.now() - (storeMessages.length - index) * 1000),
    toolCall: msg.toolCall
      ? {
          id: msg.toolCall.id,
          name: msg.toolCall.name,
          status: msg.toolCall.status ?? ("completed" as const),
          input: msg.toolCall.arguments ?? "",
          output:
            msg.toolCall.output ??
            (msg.role === "tool" ? (msg.content ?? "") : undefined),
          completedAt: msg.toolCall.completedAt,
        }
      : undefined,
  }));
}

export function Chat({ sessionId }: ChatProps) {
  const theme = useTheme();
  const CHAT = useMemo(() => getChatTokens(theme), [theme]);
  const isDev = import.meta.env.DEV;
  const [panelTab, setPanelTab] = useState(0);
  const composerRef = useRef<ChatComposerRef>(null);
  const [messages, setMessages] = useState<Message[]>([]);
  // Track which session we last initialized messages for, so we can reset
  // synchronously during render instead of waiting for a useEffect commit.
  // React allows calling setState during render when guarded by a different
  // state value — it abandons the current render and restarts immediately.
  // This eliminates the "render with stale messages → effect → render with new
  // messages" double-render cycle that was the primary source of switch latency.
  const [prevSessionIdForMsg, setPrevSessionIdForMsg] = useState<string>(sessionId ?? "");

  // ── Progressive rendering ─────────────────────────────────────────────────
  // For long sessions, render only the most-recent INSTANT_RENDER_COUNT items
  // first so the user sees content on the first paint. Older items are added
  // in a low-priority background render (startTransition + rAF) that does not
  // block the main thread. Phase-2 restores the scroll position so the
  // viewport stays anchored to the same latest messages.
  const INSTANT_RENDER_COUNT = 30;
  const [allItemsVisible, setAllItemsVisible] = useState(true);
  /** Saved scrollHeight of the scroll container just before phase-2 renders.
   *  Used to compute how much height was added above the current view. */
  const scrollRestoreRef = useRef<number | null>(null);
  const [isStreaming, setIsStreaming] = useState(false);
  /** True while background follow-up suggestions are being generated after `complete` fires */
  const [suggestionsGenerating, setSuggestionsGenerating] = useState(false);
  const [currentResponse, setCurrentResponse] = useState("");
  const [pendingAssistantHint, setPendingAssistantHint] = useState<string | null>(null);
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
  /** SSH 执行环境：树形选择远程绝对路径作为工作区（非本机文件夹选择器） */
  const [sshWorkspaceDialogOpen, setSshWorkspaceDialogOpen] = useState(false);
  /** 用户气泡「复制」成功提示 */
  const [copySuccessToast, setCopySuccessToast] = useState(false);
  /** True when session was just created on-demand; triggers a deferred send once sessionId updates. */
  const [pendingFirstSend, setPendingFirstSend] = useState(false);
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
  /** User pressed stop while `send_message` is still in flight (no `message_id` / listener yet). */
  const sendCancelledDuringRequestRef = useRef(false);
  /** FIFO: main-session messages enqueued while a turn is still streaming (flush one per stream end). */
  const queuedMainSendQueueRef = useRef<QueuedMainSend[]>([]);
  /** Bumps when the in-memory queue mutates so the composer list re-renders. */
  const [queueRevision, setQueueRevision] = useState(0);
  const bumpQueueUi = useCallback(() => setQueueRevision((r) => r + 1), []);
  const handleSendRef = useRef<() => Promise<void>>(async () => {});
  const retryUserMessageRef = useRef<(message: Message) => Promise<void>>(async () => {});
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
        composerRef.current?.setValue(item.body);
        const st = useChatComposerStore.getState();
        st.clearComposerAttachedPaths();
        for (const p of item.composerAttachedPaths) {
          st.addComposerAttachedPath(p);
        }
        st.setComposerAgentType(item.composerAgentType);
        st.setPermissionMode(item.permissionMode);
        st.setEnvironment(item.environment);
        st.setSshServer(item.sshServer);
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
  /** Buffered text chunks waiting to be batched into React state */
  const pendingTextBufferRef = useRef("");
  /** RAF handle for scheduled text flush */
  const textFlushRafRef = useRef<number | null>(null);

  // Keep refs in sync with state for access in event listeners
  useEffect(() => {
    currentResponseRef.current = currentResponse;
  }, [currentResponse]);

  const isMainReplyBusy = useCallback(() => {
    const act = useActivityStore.getState();
    return (
      act.isConnecting ||
      act.isStreaming ||
      act.waitingFirstChunk ||
      isStreamingRef.current
    );
  }, []);

  const clearStaleRetryBusyFlag = useCallback(() => {
    if (!isMainReplyBusy()) {
      retrySendInFlightRef.current = false;
    }
  }, [isMainReplyBusy]);

  useEffect(() => {
    if (!isConnecting) {
      setPendingAssistantHint(null);
    }
  }, [isConnecting]);

  useEffect(() => {
    currentRoundIdRef.current = currentRoundId;
  }, [currentRoundId]);

  useEffect(() => {
    return () => {
      if (textFlushRafRef.current !== null) {
        cancelAnimationFrame(textFlushRafRef.current);
        textFlushRafRef.current = null;
      }
      if (scrollRafRef.current !== null) {
        cancelAnimationFrame(scrollRafRef.current);
        scrollRafRef.current = null;
      }
    };
  }, []);

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
    activeProviderEntryName,
  } = useSessionStore();

  // ── Synchronous messages reset on session change ───────────────────────────
  // When sessionId changes, reset local messages during the CURRENT render
  // instead of waiting for a useEffect → setMessages → second render cycle.
  // React re-renders immediately when setState is called during render and the
  // guard state differs — no extra commit, no blank flash between sessions.
  if (prevSessionIdForMsg !== (sessionId ?? "")) {
    setPrevSessionIdForMsg(sessionId ?? "");
    const converted =
      sessionId && storeMessages.length > 0
        ? convertStoreMessages(storeMessages, sessionId)
        : [];
    setMessages(converted);
    // Phase-1 of progressive rendering: immediately show only recent items.
    // Phase-2 (older items) is scheduled after the first paint via the effect below.
    setAllItemsVisible(converted.length <= INSTANT_RENDER_COUNT);
  }

  /** After `renameSession`, React's `currentSession` can still be stale until re-render — use for `send_message.session_name`. */
  const getSessionNameForRequest = useCallback(() => {
    const s = useSessionStore.getState().currentSession;
    if (s?.id === sessionId) return s.name;
    return currentSession?.name;
  }, [sessionId, currentSession?.name]);

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
    // Reset per-session UI state so previous session's expanded panels don't
    // bleed into the newly selected session.
    setExpandedToolGroups(new Set());
    setNestedToolPanelOpen({});
  }, [sessionId, bumpQueueUi]);

  // ── Progressive rendering: phase-2 scheduler ─────────────────────────────
  // When allItemsVisible is false (long session, phase 1 rendered), schedule
  // a low-priority update that renders all older items after the first paint.
  // The effect fires only when allItemsVisible is false, so it doesn't run
  // during streaming (where allItemsVisible stays true).
  useEffect(() => {
    if (allItemsVisible) return; // Phase 2 already done or small session
    // Save scroll container's total height before adding older items.
    // Used in useLayoutEffect below to restore the viewport position.
    const el = messagesScrollRef.current;
    scrollRestoreRef.current = el ? el.scrollHeight : 0;
    // Wait for the first paint to show phase-1 items, then add older items
    // via startTransition so React can yield to user input between chunks.
    const rafId = requestAnimationFrame(() => {
      startTransition(() => setAllItemsVisible(true));
    });
    return () => cancelAnimationFrame(rafId);
  }, [allItemsVisible]);

  // ── Progressive rendering: scroll restoration after phase-2 ──────────────
  // Fires synchronously after React commits the full item list. Computes how
  // much height was added above the current view and adjusts scrollTop so the
  // user's viewport stays anchored to the same latest messages.
  useLayoutEffect(() => {
    if (!allItemsVisible || scrollRestoreRef.current === null) return;
    const el = messagesScrollRef.current;
    if (el) {
      const addedHeight = el.scrollHeight - scrollRestoreRef.current;
      if (addedHeight > 0) {
        if (shouldAutoScrollRef.current) {
          // User was at the bottom: scroll to the new bottom (same visual position)
          el.scrollTop = el.scrollHeight - el.clientHeight;
        } else {
          // User scrolled up: shift scrollTop by the added height to preserve their view
          el.scrollTop += addedHeight;
        }
      }
    }
    scrollRestoreRef.current = null;
  }, [allItemsVisible]);

  // ── Session-switch render timing ──────────────────────────────────────────
  // useLayoutEffect fires synchronously after React commits the DOM — closest
  // point we can measure "render done" without a real paint observer.
  // We then schedule a rAF to capture the first frame painted after the update.
  const isSwitchingSessionRef = useRef(false);
  useLayoutEffect(() => {
    const nowSwitching = isSwitchingSession;
    // Detect the transition false (overlay gone, messages visible)
    if (!nowSwitching && isSwitchingSessionRef.current) {
      isSwitchingSessionRef.current = false;
      // T4: React committed DOM — measure layout time from state-set
      performance.mark("sw:layout");
      try { performance.measure("sw: React layout", "sw:state-set", "sw:layout"); } catch { /**/ }
      const clickAt = (window as unknown as { __swClickAt?: number }).__swClickAt;
      const layoutMs = clickAt != null ? Math.round(performance.now() - clickAt) : 0;
      // T5: schedule rAF to get first-paint time
      requestAnimationFrame(() => {
        performance.mark("sw:paint");
        try { performance.measure("sw: rAF (paint)", "sw:layout", "sw:paint"); } catch { /**/ }
        const paintMs = clickAt != null ? Math.round(performance.now() - clickAt) : 0;
        console.info(
          `%c[SwPerf] click→layout: ${layoutMs}ms | click→paint: ${paintMs}ms | rAF delta: ${paintMs - layoutMs}ms`,
          "color:#f0a500;font-weight:bold",
        );
        // Clean up marks so next switch starts fresh
        ["sw:click","sw:ipc-start","sw:ipc-done","sw:state-set","sw:layout","sw:paint"].forEach(
          (m) => { try { performance.clearMarks(m); } catch { /**/ } },
        );
        (window as unknown as { __swClickAt?: number }).__swClickAt = undefined;
      });
    } else if (nowSwitching) {
      isSwitchingSessionRef.current = true;
    }
  }, [isSwitchingSession]);

  // Deferred send: when a session was just auto-created from handleSend and
  // the prop sessionId has now updated to a real ID, fire the send immediately.
  useEffect(() => {
    if (pendingFirstSend && sessionId) {
      setPendingFirstSend(false);
      void handleSendRef.current();
    }
  }, [pendingFirstSend, sessionId]);

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

  const dispatchWorkflowCommand = useCallback(
    (
      mode: "plan" | "schedule" | "team" | "autopilot",
      body: string,
      autoSend = true,
    ) => {
      const trimmedBody = body.trim();
      if (!trimmedBody) return;
      window.dispatchEvent(
        new CustomEvent<ComposerDispatchDetail>(OMIGA_COMPOSER_DISPATCH_EVENT, {
          detail: {
            content: `/${mode} ${trimmedBody}`,
            autoSend,
          },
        }),
      );
    },
    [],
  );

  const executeExistingPlan = useCallback(
    async (plan: SchedulerPlan, mode: "schedule" | "team" | "autopilot") => {
      const request =
        plan.originalRequest?.trim() ||
        plan.subtasks.map((task) => task.description).join("\n").trim();
      if (!request) return;

      if (mode === "autopilot") {
        dispatchWorkflowCommand("autopilot", request);
        return;
      }

      const projectRoot =
        currentSession?.workingDirectory ?? currentSession?.projectPath;
      if (!sessionId || !projectRoot || isUnsetWorkspacePath(projectRoot)) {
        setBgToast("请先选择有效工作目录，再执行计划。");
        return;
      }

      setBgToast(mode === "team" ? "正在按团队模式执行当前计划…" : "正在执行当前计划…");
      try {
        await invoke("run_existing_agent_plan", {
          request: {
            plan,
            projectRoot,
            sessionId,
            modeHint: mode,
            strategy: mode === "team" ? "Team" : "Phased",
          },
        });
        await refreshBackgroundTasks();
      } catch (error) {
        console.error("[Chat] run_existing_agent_plan failed:", error);
        setBgToast("直接执行当前计划失败，已回退为重新发送命令。");
        dispatchWorkflowCommand(mode, request);
      }
    },
    [
      currentSession?.projectPath,
      currentSession?.workingDirectory,
      dispatchWorkflowCommand,
      refreshBackgroundTasks,
      sessionId,
    ],
  );

  const bgTranscriptLabel = useMemo(() => {
    if (!bgTranscriptTaskId) return undefined;
    const t = backgroundTasks.find((x) => x.task_id === bgTranscriptTaskId);
    if (!t) return undefined;
    return `${normalizeAgentDisplayName(t.agent_type)}: ${shortBgTaskLabel(t, 72)}`;
  }, [bgTranscriptTaskId, backgroundTasks]);

  useEffect(() => {
    setFollowUpTaskId(null);
    setBgTranscriptTaskId(null);
    void refreshBackgroundTasks();
  }, [sessionId, refreshBackgroundTasks]);

  useEffect(() => {
    if (!followUpTaskId) return;
    const selectedTask = backgroundTasks.find(
      (task) => task.task_id === followUpTaskId,
    );
    if (!selectedTask || !canSendFollowUpToTask(selectedTask.status)) {
      setFollowUpTaskId(null);
    }
  }, [backgroundTasks, followUpTaskId]);

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

  // Listen for implicit memory indexing events emitted by index_chat_to_implicit_memory.
  // These are global Tauri events filtered by session_id.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let completedTimer: ReturnType<typeof setTimeout> | null = null;
    const setup = async () => {
      const u1 = await listen<{ session_id: string }>("chat-index-start", ({ payload }) => {
        if (payload.session_id !== sessionId) return;
        if (completedTimer) clearTimeout(completedTimer);
        setIndexingStatus("indexing");
      });
      const u2 = await listen<{ session_id: string }>("chat-index-complete", ({ payload }) => {
        if (payload.session_id !== sessionId) return;
        setIndexingStatus("completed");
        completedTimer = setTimeout(() => setIndexingStatus("idle"), 2500);
      });
      const u3 = await listen<{ session_id: string; error: string }>("chat-index-error", ({ payload }) => {
        if (payload.session_id !== sessionId) return;
        setIndexingStatus("error");
        completedTimer = setTimeout(() => setIndexingStatus("idle"), 3500);
      });
      unlisten = () => { u1(); u2(); u3(); };
    };
    void setup();
    return () => {
      unlisten?.();
      if (completedTimer) clearTimeout(completedTimer);
    };
  }, [sessionId]);

  // Listen for background title updates emitted by spawn_session_title_async.
  // The backend already persisted the rename; we only need to refresh local state.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    const setup = async () => {
      unlisten = await listen<{ sessionId: string; title: string }>(
        "session-title-updated",
        ({ payload }) => {
          const st = useSessionStore.getState();
          // Only apply if this session is still active and name hasn't been changed
          // manually by the user since the heuristic rename.
          if (st.currentSession?.id !== payload.sessionId) return;
          if (payload.title === st.currentSession.name) return;
          useSessionStore.setState((prev) => ({
            sessions: prev.sessions.map((s) =>
              s.id === payload.sessionId ? { ...s, name: payload.title } : s,
            ),
            currentSession:
              prev.currentSession?.id === payload.sessionId
                ? { ...prev.currentSession, name: payload.title }
                : prev.currentSession,
          }));
        },
      );
    };
    void setup();
    return () => {
      unlisten?.();
    };
  }, []);

  const needsWorkspacePath =
    Boolean(sessionId) &&
    currentSession != null &&
    isUnsetWorkspacePath(
      currentSession.workingDirectory ?? currentSession.projectPath,
    );

  const handlePickProjectFolder = async () => {
    if (!sessionId) return;
    const { environment, sshServer } = useChatComposerStore.getState();
    if (environment === "ssh") {
      if (!sshServer?.trim()) {
        setPathToastKey((k) => k + 1);
        setPathRequiredToast("请先在执行环境中选择 SSH 服务器。");
        return;
      }
      setSshWorkspaceDialogOpen(true);
      return;
    }
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

  const handleSshWorkspaceConfirm = async (path: string) => {
    if (!sessionId) return;
    if (!path.startsWith("/")) {
      setPathToastKey((k) => k + 1);
      setPathRequiredToast(
        "远程工作区须为绝对路径（以 / 开头），例如 /home/ubuntu/project",
      );
      return;
    }
    try {
      await updateSessionProjectPath(sessionId, path);
      setSshWorkspaceDialogOpen(false);
    } catch (e) {
      console.error("[Chat] ssh workspace path failed", e);
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

  // Auto-scroll: only when user is already near the bottom, throttled by RAF
  const shouldAutoScrollRef = useRef(true);
  const scrollRafRef = useRef<number | null>(null);

  // Scroll-to-top pagination + auto-scroll bottom detection
  useEffect(() => {
    const el = messagesScrollRef.current;
    if (!el) return;
    const onScroll = () => {
      if (el.scrollTop < 120 && hasMoreMessages && !isLoadingMoreMessages) {
        void loadMoreMessages();
      }
      const isNearBottom =
        el.scrollTop + el.clientHeight >= el.scrollHeight - AUTO_SCROLL_BOTTOM_THRESHOLD_PX;
      shouldAutoScrollRef.current = isNearBottom;
    };
    el.addEventListener("scroll", onScroll, { passive: true });
    return () => el.removeEventListener("scroll", onScroll);
  }, [hasMoreMessages, isLoadingMoreMessages, loadMoreMessages]);

  const messageRenderItems = useMemo(
    () => groupMessagesForRender(messages),
    [messages],
  );
  const lastReactFoldId = useMemo(() => {
    // Always computed from the FULL list so "isLastFold" identifies the real
    // last tool-group even when only a subset is rendered in phase 1.
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

  // Phase-1: show only the most-recent items so the viewport is populated
  // on the very first paint. Phase-2 (allItemsVisible=true) adds all older
  // items without blocking the thread (see progressive rendering effects above).
  const displayedItems = allItemsVisible
    ? messageRenderItems
    : messageRenderItems.slice(-INSTANT_RENDER_COUNT);

  const scheduleScrollToBottom = useCallback(() => {
    if (scrollRafRef.current !== null) return;
    scrollRafRef.current = requestAnimationFrame(() => {
      scrollRafRef.current = null;
      if (!shouldAutoScrollRef.current) return;
      messagesEndRef.current?.scrollIntoView({ behavior: "auto" });
    });
  }, []);

  useEffect(() => {
    scheduleScrollToBottom();
  }, [
    messages,
    currentResponse,
    isConnecting,
    waitingFirstChunk,
    isStreaming,
    scheduleScrollToBottom,
  ]);

  // When an agent orchestration completes, force-scroll to bottom so the
  // synthesized reply is immediately visible regardless of scroll position.
  const scheduleCompleteSession = useAgentStore((s) => s.scheduleCompleteSession);
  const setScheduleCompleteSession = useAgentStore((s) => s.setScheduleCompleteSession);
  useEffect(() => {
    if (scheduleCompleteSession === sessionId) {
      setScheduleCompleteSession(null);
      shouldAutoScrollRef.current = true;
      messagesEndRef.current?.scrollIntoView({ behavior: "smooth" });
    }
  }, [scheduleCompleteSession, sessionId, setScheduleCompleteSession]);

  // Subscribe to the synthesis stream before the first chunk arrives so the
  // leader's reply streams inline instead of requiring a page refresh.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    listen<{ sessionId: string; messageId: string }>(
      "chat-synthesis-start",
      (event) => {
        if (event.payload.sessionId !== sessionId) return;
        const { messageId } = event.payload;
        setCurrentStreamId(messageId);
        setCurrentResponse("");
        useActivityStore.getState().resetExecutionState();
        useActivityStore.getState().setConnecting(true);
        void setupStreamListener(messageId);
      },
    ).then((fn) => {
      unlisten = fn;
    });
    return () => unlisten?.();
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sessionId]);

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

  /** Image lightbox for markdown images */
  const [imageLightbox, setImageLightbox] = useState<{
    open: boolean;
    src: string;
    alt: string;
  }>({ open: false, src: "", alt: "" });

  const composerSuggestionBundle = useMemo(() => {
    const last = messages[messages.length - 1];
    if (
      last?.role === "assistant" &&
      last.followUpSuggestions &&
      last.followUpSuggestions.length > 0
    ) {
      return {
        rows: last.followUpSuggestions.map((s) => ({
          label: compactSuggestionLabel(s.label, s.prompt),
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

  // Sync messages from store when storeMessages change (e.g. streaming new messages).
  // On session switch the render-time guard above already resets messages synchronously,
  // so this effect only fires for same-session updates (incoming stream chunks, etc.).
  // We compare length + last message id to avoid re-rendering when already in sync.
  useEffect(() => {
    try {
      if (sessionId && storeMessages.length > 0) {
        const converted = convertStoreMessages(storeMessages, sessionId);
        setMessages((prev) => {
          // Skip if content is already the same (render-time reset already applied it)
          if (
            prev.length === converted.length &&
            prev[prev.length - 1]?.id === converted[converted.length - 1]?.id
          ) {
            return prev;
          }
          return converted;
        });
      } else if (!sessionId || storeMessages.length === 0) {
        setMessages((prev) => (prev.length === 0 ? prev : []));
      }
    } catch (e) {
      console.error(
        "[OmigaDebug][Chat] failed to sync messages from store",
        e,
        { sessionId, storeMessagesLength: storeMessages.length },
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
  useEffect(() => {
    const handler = (e: Event) => {
      const content = (e as CustomEvent<{ content: string }>).detail?.content;
      if (!content?.trim()) return;
      composerRef.current?.setValue(content.trim());
      queueMicrotask(() => handleSendRef.current());
    };
    window.addEventListener("wikiSendMessage", handler);
    return () => window.removeEventListener("wikiSendMessage", handler);
  }, []);

  useEffect(() => {
    const handler = (e: Event) => {
      const detail = (e as CustomEvent<ComposerDispatchDetail>).detail;
      const content = detail?.content?.trim();
      if (!content) return;
      composerRef.current?.setValue(content);
      queueMicrotask(() => {
        composerRef.current?.focus();
        if (detail?.autoSend) {
          void handleSendRef.current();
        }
      });
    };
    window.addEventListener(OMIGA_COMPOSER_DISPATCH_EVENT, handler);
    return () =>
      window.removeEventListener(OMIGA_COMPOSER_DISPATCH_EVENT, handler);
  }, []);

  // Set up stream listener for a specific stream ID
  const setupStreamListener = async (streamId: string) => {
    // Clean up previous listener
    if (unlistenRef.current) {
      unlistenRef.current();
    }
    // Flush any buffered text before swapping listeners
    if (textFlushRafRef.current !== null) {
      cancelAnimationFrame(textFlushRafRef.current);
      textFlushRafRef.current = null;
    }
    const buffered = pendingTextBufferRef.current;
    if (buffered) {
      pendingTextBufferRef.current = "";
      setCurrentResponse((prev) => {
        const next = prev + buffered;
        currentResponseRef.current = next;
        return next;
      });
    }

    const eventName = `chat-stream-${streamId}`;

    const flushPendingText = () => {
      if (textFlushRafRef.current !== null) {
        cancelAnimationFrame(textFlushRafRef.current);
        textFlushRafRef.current = null;
      }
      const text = pendingTextBufferRef.current;
      if (!text) return;
      pendingTextBufferRef.current = "";
      setCurrentResponse((prev) => {
        const next = prev + text;
        currentResponseRef.current = next;
        return next;
      });
    };

    const scheduleFlush = () => {
      if (textFlushRafRef.current !== null) return;
      textFlushRafRef.current = requestAnimationFrame(() => {
        flushPendingText();
      });
    };

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
        setSuggestionsGenerating(false);
        act.setConnecting(false);
        act.setStreaming(true, true);
        segmentStartRef.current = true;
        act.onStreamStart();
        if (clearAssistantDraft) {
          pendingTextBufferRef.current = "";
          if (textFlushRafRef.current !== null) {
            cancelAnimationFrame(textFlushRafRef.current);
            textFlushRafRef.current = null;
          }
          setCurrentResponse("");
          currentResponseRef.current = "";
        }
      };

      switch (payload.type) {
        case "Start": {
          flushPendingText();
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
            const isFirstChunk =
              currentResponseRef.current.length === 0 &&
              pendingTextBufferRef.current.length === 0;
            pendingTextBufferRef.current += text;
            scheduleFlush();
            if (isFirstChunk) {
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
          flushPendingText();
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
            // Clear immediately so parallel tool_use events don't capture the same thinking text twice.
            currentResponseRef.current = "";
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
                completedAt: Date.now(),
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
          // Real-time todo sync: push todos to activityStore on every todo_write result
          // so TaskStatus can display them during streaming (before storeMessages syncs).
          if (resultData?.name === "todo_write" && resultData.input) {
            try {
              const j = JSON.parse(resultData.input) as {
                todos?: Array<{
                  id?: string;
                  content: string;
                  activeForm?: string;
                  active_form?: string;
                  status: string;
                }>;
              };
              if (Array.isArray(j?.todos)) {
                useActivityStore.getState().setActiveTodos(
                  j.todos.map((t, i) => ({
                    id: t.id ?? `todo-${i}`,
                    content: t.content,
                    activeForm: t.activeForm ?? t.active_form ?? t.content,
                    status: String(t.status),
                  })),
                );
              }
            } catch {
              // silently ignore malformed todo_write args
            }
          }
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
          flushPendingText();
          const errorData = payload.data as
            | { message: string; code?: string }
            | undefined;
          setAwaitingResumeAfterCancel(false);
          isStreamingRef.current = false;
          setIsStreaming(false);
          setCurrentStreamId(null);
          setCurrentRoundId(null);
          retrySendInFlightRef.current = false;
          setPendingAskUser(null);
          setAskUserSelections({});
          pendingTokenUsageRef.current = null;
          useActivityStore.getState().finalizeExecutionRun();
          useActivityStore.getState().clearTransient();
          setCurrentResponse("");
          currentResponseRef.current = "";
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
          flushPendingText();
          setAwaitingResumeAfterCancel(true);
          isStreamingRef.current = false;
          setIsStreaming(false);
          setCurrentStreamId(null);
          setCurrentRoundId(null);
          retrySendInFlightRef.current = false;
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
          currentResponseRef.current = "";
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
          const parsed = rows
            .map((r) => ({
              label: typeof r.label === "string" ? r.label.trim() : "",
              prompt: typeof r.prompt === "string" ? r.prompt.trim() : "",
            }))
            .filter((r) => r.label.length > 0 && r.prompt.length > 0)
            .slice(0, 5);

          // If complete already fired (isStreamingRef is false), patch the last assistant message directly.
          // Do NOT call replaceStoreMessagesSnapshot here — the user may have already sent a new
          // message, and overwriting the store snapshot with fewer messages would cause the
          // storeMessages useEffect to wipe out that new user message.
          // The suggestions are already persisted to the DB by the Rust backend.
          if (!isStreamingRef.current) {
            setSuggestionsGenerating(false);
            if (parsed.length > 0) {
              setMessages((prev) => {
                const lastIdx = prev.length - 1;
                if (lastIdx < 0 || prev[lastIdx].role !== "assistant") return prev;
                const updated = {
                  ...prev[lastIdx],
                  followUpSuggestions: parsed,
                };
                return [...prev.slice(0, lastIdx), updated];
              });
            }
          } else {
            pendingFollowUpSuggestionsRef.current = parsed;
          }
          break;
        }
        case "suggestions_generating": {
          setSuggestionsGenerating(true);
          break;
        }
        case "suggestions_complete": {
          setSuggestionsGenerating(false);
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
          flushPendingText();
          setAwaitingResumeAfterCancel(false);
          isStreamingRef.current = false;
          setIsStreaming(false);
          setCurrentStreamId(null);
          setCurrentRoundId(null);
          retrySendInFlightRef.current = false;
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
          currentResponseRef.current = "";

          // Memory indexing status is driven by chat-index-* Tauri events below.

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
    if (!sessionId) {
      // No session yet — create one on-demand. The deferred-send useEffect
      // will fire handleSend again once the prop sessionId becomes non-empty.
      try {
        await useSessionStore.getState().createSessionQuick();
        setPendingFirstSend(true);
      } catch (e) {
        console.error("[Chat] createSessionQuick on-demand failed", e);
      }
      return;
    }
    sendCancelledDuringRequestRef.current = false;
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
      environment: storeEnv,
      sshServer: storeSsh,
      sandboxBackend: storeSb,
      localVenvType: storeVenvType,
      localVenvName: storeVenvName,
    } = useChatComposerStore.getState();

    const composerAgentType = flushPayload
      ? flushPayload.composerAgentType
      : storeAgent;
    const permissionMode = flushPayload
      ? flushPayload.permissionMode
      : storePerm;
    const environment = flushPayload ? flushPayload.environment : storeEnv;
    const sshServer = flushPayload ? flushPayload.sshServer : storeSsh;
    const sandboxBackend = flushPayload ? flushPayload.sandboxBackend : storeSb;
    const localVenvType = storeVenvType;
    const localVenvName = storeVenvName;
    const composerAttachedPaths = flushPayload
      ? [...flushPayload.composerAttachedPaths]
      : storePaths;

    /** Prefer ref payload after queue flush — `getValue` reads the latest composer state. */
    const trimmed = flushPayload ? flushPayload.body.trim() : (composerRef.current?.getValue() ?? "").trim();

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
      composerRef.current?.setValue("");
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
            session_name: getSessionNameForRequest(),
            use_tools: true,
            inputTarget: `bg:${followUpTaskId}`,
            executionEnvironment: useChatComposerStore.getState().environment,
            sshServer: useChatComposerStore.getState().sshServer,
            sandboxBackend: useChatComposerStore.getState().sandboxBackend,
            localVenvType: useChatComposerStore.getState().localVenvType,
            localVenvName: useChatComposerStore.getState().localVenvName,
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
        environment,
        sshServer: useChatComposerStore.getState().sshServer,
        sandboxBackend: useChatComposerStore.getState().sandboxBackend,
      });
      bumpQueueUi();
      composerRef.current?.setValue("");
      useChatComposerStore.getState().clearComposerAttachedPaths();
      return;
    }

    const workflowPrepared = rewriteWorkflowBodyForBackend(trimmed);
    const pendingFeedback = buildPendingExecutionFeedback({
      workflowCommand: workflowPrepared.workflowCommand,
      composerAgentType,
    });

    // Reset indexing status on new message
    setIndexingStatus("idle");
    setPendingAssistantHint(pendingFeedback.assistantHint);

    useActivityStore.getState().beginExecutionRun(pendingFeedback.connectLabel);
    useActivityStore.getState().setConnecting(true);
    useActivityStore.getState().setStreaming(false, false);
    useActivityStore.getState().clearActiveTodos();

    const isFirstMessageInSession = storeMessages.length === 0;

    const messageContent = mergeComposerPathsAndBody(
      composerAttachedPaths,
      trimmed,
    );
    const workflowTitleSeed =
      parseWorkflowCommand(trimmed)?.body || trimmed;
    const backendMessageContent = mergeComposerPathsAndBody(
      composerAttachedPaths,
      workflowPrepared.content,
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
      timestamp: userMessage.timestamp,
    });

    composerRef.current?.setValue("");
    useChatComposerStore.getState().clearComposerAttachedPaths();

    try {
      if (
        isFirstMessageInSession &&
        isPlaceholderSessionTitle(currentSession?.name)
      ) {
        const heuristicTitle = titleFromFirstUserMessage(workflowTitleSeed);
        await renameSession(sessionId, heuristicTitle);
        // Fire-and-forget: backend spawns an independent task that calls the LLM
        // after a short delay, persists the result, and emits "session-title-updated".
        void invoke("spawn_session_title_async", {
          sessionId,
          userMessage: workflowTitleSeed,
        });
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
          routingContent: backendMessageContent,
          session_id: sessionId,
          project_path: currentSession?.projectPath,
          session_name: getSessionNameForRequest(),
          use_tools: true,
          composerAgentType,
          workflowCommand: workflowPrepared.workflowCommand,
          permissionMode,
          executionEnvironment: environment,
          sshServer,
          sandboxBackend,
          localVenvType,
          localVenvName,
          activeProviderEntryName,
        },
      });

      if (sendCancelledDuringRequestRef.current) {
        sendCancelledDuringRequestRef.current = false;
        try {
          await invoke("cancel_stream", { messageId: response.message_id });
        } catch (e) {
          console.error(
            "[Chat] cancel_stream after user stopped during send_message",
            e,
          );
        }
        useActivityStore.getState().clearTransient();
        useActivityStore.getState().resetExecutionState();
        setCurrentStreamId(null);
        setCurrentRoundId(null);
        return;
      }

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
      sendCancelledDuringRequestRef.current = false;
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
        setIsStreaming(false);
        setPendingAskUser(null);
        setAskUserSelections({});
        pendingTokenUsageRef.current = null;
        flushQueuedMainSendIfAnyRef.current();
      });
    }
  };

  handleSendRef.current = handleSend;

  const openEditUserMessage = useCallback((message: Message) => {
    if (message.role !== "user") return;
    setUserMessageEdit({ id: message.id, draft: message.content });
  }, []);

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
      clearStaleRetryBusyFlag();
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
      const rawRetryBody = stripLeadingPathPrefixFromMerged(
        messageContent,
        message.composerAttachedPaths ?? [],
      );
      const composeAgent = message.composerAgentType ?? "general-purpose";
      const retryPrepared = rewriteWorkflowBodyForBackend(rawRetryBody);
      const pendingFeedback = buildPendingExecutionFeedback({
        workflowCommand: retryPrepared.workflowCommand,
        composerAgentType: composeAgent,
      });
      const backendRetryContent = mergeComposerPathsAndBody(
        message.composerAttachedPaths ?? [],
        retryPrepared.content,
      );

      const truncated = messages
        .slice(0, idx + 1)
        .map((m, i) =>
          i === idx && m.role === "user"
            ? { ...message, schedulerPlan: undefined, initialTodos: undefined }
            : m,
        );

      retrySendInFlightRef.current = true;
      queuedMainSendQueueRef.current = [];
      mainQueueFlushPayloadRef.current = null;
      bumpQueueUi();

      setMessages(truncated);
      replaceStoreMessagesSnapshot(truncated.map(chatMessageToStore));
      setCurrentResponse("");
      currentResponseRef.current = "";
      pendingTextBufferRef.current = "";
      if (textFlushRafRef.current !== null) {
        cancelAnimationFrame(textFlushRafRef.current);
        textFlushRafRef.current = null;
      }
      setAwaitingResumeAfterCancel(false);

      setIndexingStatus("idle");
      setPendingAssistantHint(pendingFeedback.assistantHint);
      useActivityStore
        .getState()
        .beginExecutionRun(pendingFeedback.connectLabel);
      useActivityStore.getState().setConnecting(true);
      useActivityStore.getState().setStreaming(false, false);

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
            routingContent: backendRetryContent,
            session_id: sessionId,
            project_path: currentSession?.projectPath,
            session_name: getSessionNameForRequest(),
            use_tools: true,
            composerAgentType: composeAgent,
            workflowCommand: retryPrepared.workflowCommand,
            permissionMode,
            executionEnvironment: useChatComposerStore.getState().environment,
            sshServer: useChatComposerStore.getState().sshServer,
            sandboxBackend: useChatComposerStore.getState().sandboxBackend,
            localVenvType: useChatComposerStore.getState().localVenvType,
            localVenvName: useChatComposerStore.getState().localVenvName,
            activeProviderEntryName,
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
          setIsStreaming(false);
          setPendingAskUser(null);
          setAskUserSelections({});
          pendingTokenUsageRef.current = null;
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
      getSessionNameForRequest,
      replaceStoreMessagesSnapshot,
      bumpQueueUi,
      clearStaleRetryBusyFlag,
    ],
  );

  retryUserMessageRef.current = retryUserMessage;

  const saveUserMessageEdit = useCallback(() => {
    if (!userMessageEdit) return;
    const { id, draft } = userMessageEdit;
    const trimmed = draft.trim();
    if (!trimmed) return;

    const idx = messages.findIndex((m) => m.id === id);
    if (idx < 0) {
      setUserMessageEdit(null);
      return;
    }
    const row = messages[idx];
    if (row.role !== "user") {
      setUserMessageEdit(null);
      return;
    }

    const paths = row.composerAttachedPaths ?? [];
    const keepPaths = pathsStillMatchMergedContent(paths, trimmed)
      ? paths
      : undefined;
    const attached = keepPaths && keepPaths.length > 0 ? keepPaths : undefined;
    const updated: Message = {
      ...row,
      content: trimmed,
      composerAttachedPaths: attached,
      timestamp: Date.now(),
      schedulerPlan: undefined,
      initialTodos: undefined,
    };

    setUserMessageEdit(null);
    void retryUserMessageRef.current(updated);
  }, [messages, userMessageEdit]);

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
      clearStaleRetryBusyFlag();
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
    [sessionId, needsWorkspacePath, followUpTaskId, messages, clearStaleRetryBusyFlag],
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
      composerRef.current?.setValue(next.body);
      const st = useChatComposerStore.getState();
      st.clearComposerAttachedPaths();
      for (const p of next.composerAttachedPaths) {
        st.addComposerAttachedPath(p);
      }
      st.setComposerAgentType(next.composerAgentType);
      st.setPermissionMode(next.permissionMode);
      st.setEnvironment(next.environment);
      st.setSshServer(next.sshServer);
      st.setSandboxBackend(next.sandboxBackend);
    });
    void handleSendRef.current();
  };

  /**
   * 取消当前 Agent 任务（类终端 ESC）：`cancel_stream` 触发 round_cancel（含 bash 等子进程）、
   * 清空主会话排队、取消进行中的后台跟进任务；无 message_id 时做本地兜底清理。
   * 唯一入口：输入区工具栏按钮；顶部状态条不再重复提供停止控件。
   */
  const handleCancelStream = async () => {
    clearQueuedMainSends();

    const fid = followUpTaskId;
    if (fid) {
      try {
        await handleCancelBackgroundTask(fid);
      } catch (e) {
        console.error("[Chat] cancel background task with stream cancel:", e);
      }
    }

    const streamId = currentStreamId;
    if (streamId) {
      try {
        await invoke("cancel_stream", { messageId: streamId });
        // On success, wait for the stream `cancelled` event to clean up state
        return;
      } catch (error) {
        console.error("Failed to cancel stream:", error);
        // Fall through to local cleanup so the UI doesn’t stay stuck
      }
    }

    const act = useActivityStore.getState();
    const busy =
      act.isConnecting ||
      act.isStreaming ||
      act.waitingFirstChunk ||
      isStreamingRef.current;
    if (!busy) return;

    sendCancelledDuringRequestRef.current = true;
    if (unlistenRef.current) {
      unlistenRef.current();
      unlistenRef.current = null;
    }
    setCurrentStreamId(null);
    setIsStreaming(false);
    isStreamingRef.current = false;
    setCurrentResponse("");
    currentResponseRef.current = "";
    pendingTextBufferRef.current = "";
    if (textFlushRafRef.current !== null) {
      cancelAnimationFrame(textFlushRafRef.current);
      textFlushRafRef.current = null;
    }
    setAwaitingResumeAfterCancel(false);
    act.clearTransient();
    act.resetExecutionState();
  };

  const handleKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === "Escape") {
      const act = useActivityStore.getState();
      const agentBusy =
        isConnecting ||
        isStreaming ||
        waitingFirstChunk ||
        act.isConnecting ||
        act.isStreaming ||
        act.waitingFirstChunk ||
        isStreamingRef.current ||
        Boolean(currentStreamId);
      if (agentBusy) {
        e.preventDefault();
        void handleCancelStream();
      }
      return;
    }

    if (e.key !== "Enter" || e.shiftKey) return;

    // IME（中文/日文等）：Enter 用于确认候选词，不应发送消息
    const ne = e.nativeEvent;
    if (ne.isComposing || ne.keyCode === 229) return;

    e.preventDefault();
    if (needsWorkspacePath) {
      if ((composerRef.current?.getValue() ?? "").trim()) {
        showPathRequiredWarning();
      }
      return;
    }
    handleSend();
  };

  /**
   * 退出当前对话：取消进行中的流、清理监听与队列，并清空当前会话选择（回到侧栏选会话）。
   * 用于确认类对话框中的「取消」等需要结束会话的操作。
   */
  const handleExitConversation = useCallback(async () => {
    sendCancelledDuringRequestRef.current = false;
    retrySendInFlightRef.current = false;
    setRetryConfirmForMessage(null);
    setUserMessageEdit(null);
    setPendingAskUser(null);
    setAskUserSelections({});
    clearQueuedMainSends();
    setFollowUpTaskId(null);
    setBgTranscriptTaskId(null);
    setIndexingStatus("idle");
    composerRef.current?.setValue("");
    setBgToast(null);
    setPathRequiredToast(null);
    setCopySuccessToast(false);
    setCurrentRoundId(null);
    useChatComposerStore.getState().clearComposerAttachedPaths();
    useChatComposerStore.getState().setComposerAgentType("general-purpose");

    const streamId = currentStreamId;
    if (streamId) {
      try {
        await invoke("cancel_stream", { messageId: streamId });
      } catch (e) {
        console.error(
          "[Chat] cancel_stream before exit conversation failed:",
          e,
        );
      }
    }

    if (unlistenRef.current) {
      unlistenRef.current();
      unlistenRef.current = null;
    }
    setCurrentStreamId(null);
    setIsStreaming(false);
    isStreamingRef.current = false;
    setCurrentResponse("");
    currentResponseRef.current = "";
    pendingTextBufferRef.current = "";
    if (textFlushRafRef.current !== null) {
      cancelAnimationFrame(textFlushRafRef.current);
      textFlushRafRef.current = null;
    }
    setAwaitingResumeAfterCancel(false);
    const act = useActivityStore.getState();
    act.clearTransient();
    act.resetExecutionState();
    act.clearBackgroundJobs();

    try {
      await useSessionStore.getState().setCurrentSession(null);
    } catch (e) {
      console.error("[Chat] setCurrentSession(null) failed:", e);
    }
  }, [currentStreamId, clearQueuedMainSends]);

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

    setPendingAssistantHint(null);
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

    const { composerAgentType, permissionMode, environment, sshServer, sandboxBackend } =
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
          session_name: getSessionNameForRequest(),
          use_tools: true,
          composerAgentType,
          permissionMode,
          executionEnvironment: environment,
          sshServer,
          sandboxBackend,
          localVenvType: useChatComposerStore.getState().localVenvType,
          localVenvName: useChatComposerStore.getState().localVenvName,
          activeProviderEntryName,
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

  const handleMarkdownImageClick = useCallback((src: string, alt: string) => {
    setImageLightbox({ open: true, src, alt });
  }, []);

  const handleNodeClick = useCallback((text: string) => {
    composerRef.current?.appendValue(text);
    queueMicrotask(() => composerRef.current?.focus());
  }, []);

  const agentComponents = useMemo(
    () => buildMarkdownComponents(true, theme, CHAT, handleMarkdownImageClick, handleNodeClick),
    [theme, CHAT, handleMarkdownImageClick, handleNodeClick],
  );
  const defaultComponents = useMemo(
    () => buildMarkdownComponents(false, theme, CHAT, handleMarkdownImageClick, handleNodeClick),
    [theme, CHAT, handleMarkdownImageClick, handleNodeClick],
  );

  const renderMessageContent = useCallback((
    content: string,
    tone: "default" | "agent" = "default",
  ) => {
    const isAgent = tone === "agent";
    const components = isAgent ? agentComponents : defaultComponents;
    if (!content || content.trim() === "") {
      return (
        <Typography variant="body1" color="text.secondary" sx={{ fontStyle: "italic" }}>
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
          remarkPlugins={[remarkGfm, remarkMath]}
          rehypePlugins={[rehypeKatex]}
          components={components}
        >
          {fixBrokenGfmTables(content.replace(/<br\s*\/?>/gi, "\n"))}
        </ReactMarkdown>
      </Box>
    );
  }, [agentComponents, defaultComponents, CHAT]);


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
                      {showNewSessionPlaceholder ? null : (
                        <Typography variant="subtitle1" fontWeight={600} noWrap>
                          {currentSession.name}
                        </Typography>
                      )}
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
                  showResume={awaitingResumeAfterCancel && !followUpTaskId}
                  onResume={handleResumeAfterCancel}
                  backgroundTaskCount={
                    backgroundTasks.filter(
                      (task) =>
                        task.status === "Running" || task.status === "Pending",
                    ).length
                  }
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
                <OmigaLogo size={64} style={{ marginBottom: 16, opacity: 0.85 }} />
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

            {displayedItems.map((item, itemIndex) => {
              const itemKey =
                item.kind === "react_fold" ? item.id : item.message.id;
              return (
                <Box key={itemKey} sx={{ width: "100%" }}>
                  {(() => {
                    if (item.kind === "react_fold") {
                const { id, fold } = item;
                const toolMsgs = fold.filter(
                  (m) => m.role === "tool" && m.toolCall,
                );
                const summary = summarizeReactFold(fold);
                const anyRunning = toolGroupAnyRunning(toolMsgs);
                const showGroupDone = toolGroupFlowComplete(toolMsgs);
                const runningToolName = firstRunningToolName(toolMsgs);
                const runningToolCount = toolMsgs.filter(m => m.toolCall?.status === "running").length;
                const isLastFold = id === lastReactFoldId;

                return (
                  <Box
                    key={id}
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
                            {anyRunning && runningToolCount > 1
                              ? ` · ${runningToolCount} 并行`
                              : anyRunning && runningToolName
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
                                runningToolCount > 1
                                  ? `${runningToolCount} 并行运行中`
                                  : runningToolName
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
                                      {tc.completedAt != null &&
                                        message.timestamp != null && (
                                          <Typography
                                            sx={{
                                              fontSize: 10,
                                              color: CHAT.labelMuted,
                                              flexShrink: 0,
                                              fontVariantNumeric: "tabular-nums",
                                            }}
                                          >
                                            {tc.completedAt - message.timestamp >= 1000
                                              ? `${((tc.completedAt - message.timestamp) / 1000).toFixed(1)}s`
                                              : `${tc.completedAt - message.timestamp}ms`}
                                          </Typography>
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
                                        {panelTitle !== tc.name && (
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
                                        )}

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
                                                  {isBash ? commandSectionLabel : "Input"}
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
                );
              }

              const message = item.message;
              const dividerBefore = item.dividerBefore === true;
              const nextItem = displayedItems[itemIndex + 1];
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
                <Box
                  key={message.id}
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
                                      保存后将截断后续消息并重新分析。可按 Esc
                                      取消，或使用 Ctrl/⌘ + Enter 保存并重发。
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
                          <SchedulerPlanDisplay
                            plan={message.schedulerPlan}
                            sessionId={sessionId}
                            onOpenReviewerTranscript={(taskId) =>
                              setBgTranscriptTaskId(taskId)
                            }
                            onExecutePlan={(mode) => {
                              if (!message.schedulerPlan) return;
                              void executeExistingPlan(message.schedulerPlan, mode);
                            }}
                            onRevisePlan={() => {
                              const request =
                                message.schedulerPlan?.originalRequest?.trim() ||
                                stripLeadingPathPrefixFromMerged(
                                  message.content,
                                  message.composerAttachedPaths ?? [],
                                ).replace(/^\/plan\s+/iu, "").trim();
                              dispatchWorkflowCommand(
                                "plan",
                                `${request}\n\n修改要求：`,
                                false,
                              );
                            }}
                          />
                        </Box>
                      )}
                  </Box>
              );
            })()}
                </Box>
              );
            })}

            {isConnecting && pendingAssistantHint && (
              <Box
                sx={{
                  width: "100%",
                  minWidth: 0,
                  maxWidth: "100%",
                  px: 1.75,
                  py: 1.5,
                  borderRadius: `${BUBBLE_RADIUS_PX}px`,
                  bgcolor: alpha(CHAT.agentBubbleBg, 0.75),
                  border: `1px dashed ${alpha(CHAT.agentBubbleBorder, 0.9)}`,
                  fontFamily: CHAT.font,
                  color: "text.secondary",
                }}
              >
                <Typography variant="body2" sx={{ fontSize: 14, lineHeight: 1.7 }}>
                  {pendingAssistantHint}
                </Typography>
              </Box>
            )}

            {/* Streaming: final summary text only — divider appears on the persisted assistant row after complete */}
            {isStreaming && currentResponse && (
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

            {suggestionsGenerating && !showNextStepSuggestions && (
              <Fade in timeout={200}>
                <Box sx={{ width: "100%", pt: 0.5, pb: 0.5 }}>
                  <Typography
                    variant="caption"
                    sx={{
                      color: "text.disabled",
                      fontStyle: "italic",
                      display: "flex",
                      alignItems: "center",
                      gap: 0.5,
                    }}
                  >
                    正在生成下一步建议…
                  </Typography>
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
                      (() => {
                        const tooltipMarkdown = extractSuggestionTooltipMarkdown(
                          s.text,
                          s.label,
                        );
                        const button = (
                          <Button
                            size="small"
                            variant="outlined"
                            color="primary"
                            onClick={() => {
                              composerRef.current?.setValue(s.text);
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
                        );

                        if (!tooltipMarkdown) {
                          return (
                            <Box key={`${idx}-${s.label}`} sx={{ display: "inline-flex" }}>
                              {button}
                            </Box>
                          );
                        }

                        return (
                          <Tooltip
                            key={`${idx}-${s.label}`}
                            placement="top"
                            enterDelay={400}
                            title={
                              <Box
                                sx={{
                                  maxWidth: 360,
                                  "& p": { m: 0, lineHeight: 1.45 },
                                  "& ul, & ol": { my: 0.5, pl: 2 },
                                  "& li": { my: 0.25 },
                                }}
                              >
                                <ReactMarkdown remarkPlugins={[remarkGfm]}>
                                  {tooltipMarkdown}
                                </ReactMarkdown>
                              </Box>
                            }
                          >
                            {button}
                          </Tooltip>
                        );
                      })()
                    ))}
                  </Stack>
                </Box>
              </Fade>
            )}

            <div ref={messagesEndRef} />
          </Box>

          {/* Image Lightbox */}
          <Dialog
            open={imageLightbox.open}
            onClose={() => setImageLightbox((s) => ({ ...s, open: false }))}
            maxWidth="xl"
            PaperProps={{
              sx: {
                bgcolor: "transparent",
                boxShadow: "none",
                m: 2,
                maxWidth: "calc(100% - 32px)",
                maxHeight: "calc(100% - 32px)",
                overflow: "hidden",
              },
            }}
          >
            <Box
              component="img"
              src={imageLightbox.src}
              alt={imageLightbox.alt}
              onClick={() => setImageLightbox((s) => ({ ...s, open: false }))}
              sx={{
                maxWidth: "100%",
                maxHeight: "calc(100vh - 32px)",
                objectFit: "contain",
                borderRadius: 1,
                cursor: "pointer",
                display: "block",
                mx: "auto",
              }}
            />
          </Dialog>

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
                sx={{
                  mb: 1.5,
                  borderRadius: 2,
                  alignItems: "center",
                  "& .MuiAlert-message": {
                    display: "flex",
                    alignItems: "center",
                    py: 0.25,
                  },
                  "& .MuiAlert-action": {
                    alignItems: "center",
                    alignSelf: "center",
                    pt: 0,
                    pb: 0,
                  },
                }}
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
                请为此对话选择工作目录（代码与工具将相对于该路径），选择后会自动保存并隐藏此提示。
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
                composerRef={composerRef}
                onKeyDown={handleKeyDown}
                inputRef={inputRef}
                isStreaming={isStreaming}
                isConnecting={isConnecting}
                waitingFirstChunk={waitingFirstChunk}
                onCancel={handleCancelStream}
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
                onCloseBackgroundTranscript={() => setBgTranscriptTaskId(null)}
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

      <SshDirectoryTreeDialog
        open={sshWorkspaceDialogOpen}
        onClose={() => setSshWorkspaceDialogOpen(false)}
        sshProfileName={useChatComposerStore.getState().sshServer ?? ""}
        defaultPath={
          (
            currentSession?.workingDirectory ??
            currentSession?.projectPath ??
            ""
          ).trim() || undefined
        }
        onConfirm={handleSshWorkspaceConfirm}
      />

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
          <Typography
            variant="caption"
            color="text.secondary"
            sx={{ display: "block", mt: 1.5 }}
          >
            「返回」关闭此窗口；「取消」退出当前对话（结束本会话）；「确认重试」执行重试。
          </Typography>
        </DialogContent>
        <DialogActions>
          <Button
            type="button"
            variant="outlined"
            color="inherit"
            onClick={() => setRetryConfirmForMessage(null)}
          >
            返回
          </Button>
          <Button
            type="button"
            color="error"
            onClick={() => void handleExitConversation()}
          >
            取消
          </Button>
          <Button
            type="button"
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
