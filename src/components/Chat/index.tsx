import { useState, useEffect, useLayoutEffect, useRef, useMemo, useCallback, startTransition, memo, lazy, Suspense } from "react";
import { flushSync } from "react-dom";
import type { Components } from "react-markdown";
import type { Theme } from "@mui/material/styles";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
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
  CircularProgress,
} from "@mui/material";
import { alpha } from "@mui/material/styles";
import {
  SmartToy,
  CheckCircle,
  ExpandMore,
  ForumOutlined,
  FolderOpen,
  Send as SendIcon,
  Check as CheckIcon,
  KeyboardArrowDownRounded,
  Summarize as SummarizeIcon,
  Edit as EditIcon,
  Groups as GroupsIcon,
  RocketLaunch as RocketLaunchIcon,
} from "@mui/icons-material";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import "katex/dist/katex.min.css";
import {
  useSessionStore,
  useActivityStore,
  mergeActiveTodosWithTiming,
  useChatComposerStore,
  type PermissionMode,
  type ComputerUseMode,
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
import { NotificationToast } from "../NotificationToast";
import {
  AssistantTraceItem,
  LiveIntermediateTrace,
} from "./AssistantTraceItem";
import { ChatComposer, type ChatComposerRef } from "./ChatComposer";
import { AssistantMessageBubble } from "./AssistantMessageBubble";
import { ChatMarkdownContent } from "./ChatMarkdownContent";
import { ToolCallCard } from "./ToolCallCard";
import {
  firstRunningToolName,
  getNestedToolPanelOpen,
  summarizeReactFold,
  ToolFoldHeader,
  toolGroupAnyRunning,
  toolGroupFlowComplete,
} from "./ToolFoldSummary";
import { toolTracePrefaceFromText } from "./toolTracePreface";
import { UserMessageBubble } from "./UserMessageBubble";
import type { AskUserQuestionItem } from "./AskUserQuestionWizard";
import { getChatTokens } from "./chatTokens";
import type { BackgroundAgentTask } from "./backgroundAgentTypes";
import type { OmigaDagPayload } from "./DagFlow";
import type { OmigaFlowchartPayload } from "./OmigaFlowchart";
import {
  canSendFollowUpToTask,
  shortBgTaskLabel,
} from "./backgroundAgentTypes";
import { finalizeResearchCommandMessages } from "./researchCommandUtils";
import {
  messageEntranceDelayMs,
  messageRenderItemKey,
  shouldAnimateMessageItem,
} from "./renderItemUtils";
import {
  AUTO_SCROLL_BOTTOM_THRESHOLD_PX,
  isNearScrollBottom,
  shouldShowJumpToLatestButton,
} from "./chatScrollState";
import { selectLiveReActFoldTraceText } from "./liveFoldTrace";
import {
  applyToolResultMessage,
  normalizeAssistantToolCallPrefaces,
  upsertToolUseMessage,
} from "./streamToolMessageUpdates";
import {
  formatProgressiveRenderPerf,
  shouldLogProgressiveRenderPerf,
} from "./renderPerfUtils";
import { settleRunningToolCalls } from "./toolStatusUtils";
import {
  getNestedToolPanelOpenForFold,
  toggleNestedToolPanelOpenForFold,
  type NestedToolPanelOpenByFold,
} from "./toolPanelOpenState";
import {
  shouldShowPostTurnSuggestionsGeneratingPlaceholder,
  shouldStartPostTurnSuggestionsIndicator,
} from "./postTurnSuggestionsState";
import { BackgroundAgentTranscriptDrawer } from "./BackgroundAgentTranscriptDrawer";
import { SessionSwitchSkeleton } from "./SessionSwitchSkeleton";
import { AgentSessionStatus } from "./AgentSessionStatus";
import {
  ResearchGoalStatusPill,
  buildResearchGoalAutoRunCommand,
  researchGoalAutoRunElapsedBudgetReached,
  researchGoalShouldWaitForComposerDraft,
  researchGoalCanAutoRun,
  type ResearchGoal,
  type ResearchGoalCycle,
} from "./ResearchGoalStatusPill";
import {
  ResearchGoalCriteriaDialog,
  type ResearchGoalProviderEntryOption,
  type ResearchGoalProviderTestResult,
  type ResearchGoalSettingsDraft,
} from "./ResearchGoalCriteriaDialog";
import { ResearchGoalAuditDetailsDialog } from "./ResearchGoalAuditDetailsDialog";
import { SshDirectoryTreeDialog } from "./SshDirectoryTreeDialog";
import { ReviewerVerdictList } from "../ReviewerVerdictList";
import { buildPendingExecutionFeedback } from "../../utils/pendingExecutionFeedback";
import { parseNextStepSuggestionsFromMarkdown } from "../../utils/parseAssistantNextSteps";
import { extractSuggestionTooltipMarkdown } from "../../utils/suggestionTooltip";
import {
  aggregateReviewerVerdicts,
  overallReviewerHeadline,
  type BackgroundAgentTaskRow,
  type ReviewerVerdictChip,
} from "../../utils/reviewerVerdict";
import {
  parseGoalCommand,
  parseSkillCommand,
  parseResearchCommand,
  parseWorkflowCommand,
} from "../../utils/workflowCommands";
import {
  formatComposerPathPreview,
  mergeComposerPathsAndBody,
  pathsStillMatchMergedContent,
  splitLeadingPathPrefixFromMerged,
} from "./composerPathMentions";
import {
  buildSchedulerPlanHierarchy,
  schedulerStageLabel,
} from "../../utils/schedulerPlanHierarchy";
import {
  OMIGA_COMPOSER_DISPATCH_EVENT,
  type ComposerDispatchDetail,
} from "../../utils/chatComposerEvents";
import { listenTauriEvent } from "../../utils/tauriEvents";
import { CitationLink } from "../CitationLink";
import { normalizeAgentDisplayName, useAgentStore } from "../../state/agentStore";

import {
  saveStreamSnapshot,
  loadStreamSnapshot,
  snapshotIsActive,
  clearStreamSnapshot,
  registerStreamListener,
  cancelStreamListener,
  cancelAllStreamListeners,
  type SessionStreamSnapshot,
} from "../../state/sessionStreamRegistry";
import {
  activitySnapshotHasRecords,
  buildSessionActivitySnapshot,
  historicalActivityStateFromSnapshot,
  loadLatestActivitySnapshot,
  saveLatestActivitySnapshot,
} from "../../state/sessionActivitySnapshots";

const FencedCodeBlock = lazy(() => import("./FencedCodeBlock"));
const VisualizationRenderer = lazy(() =>
  import("./viz/VisualizationRenderer").then((mod) => ({
    default: mod.VisualizationRenderer,
  })),
);
const DagFlow = lazy(() =>
  import("./DagFlow").then((mod) => ({ default: mod.DagFlow })),
);
const OmigaFlowchart = lazy(() =>
  import("./OmigaFlowchart").then((mod) => ({
    default: mod.OmigaFlowchart,
  })),
);
const MermaidFlow = lazy(() =>
  import("./viz/MermaidFlow").then((mod) => ({
    default: mod.MermaidFlow,
  })),
);
const DotFlow = lazy(() =>
  import("./viz/DotFlow").then((mod) => ({ default: mod.DotFlow })),
);

/** SQLite `messages.id` shape — used to pass `retryFromUserMessageId` (not temp `user-…` ids). */
function isPersistedMessageIdForRetry(id: string): boolean {
  return /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(
    id.trim(),
  );
}

interface ChatProps {
  sessionId: string;
}

const SEARCH_CLIENT_WATCHDOG_MS = 45_000;

function isSearchToolName(name?: string): boolean {
  const n = (name ?? "").toLowerCase();
  return n === "search" || n === "websearch";
}

interface SchedulerPlan {
  planId: string;
  originalRequest?: string;
  entryAgentType?: string;
  executionSupervisorAgentType?: string;
  subtasks: Array<{
    id: string;
    description: string;
    agentType: string;
    supervisorAgentType?: string;
    stage?: string;
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
  /** 发送时显式选择的插件 ID（@ 插件选择） */
  composerSelectedPluginIds?: string[];
  /** From DB: full assistant tool_calls — rebuild trace if tool rows are incomplete */
  toolCallsList?: Array<{ id: string; name: string; arguments: string }>;
  /** Assistant text streamed before a tool in this round (shown inside tool block, not in final summary). */
  prefaceBeforeTools?: string;
  /** Assistant/tool-gap text that belongs inside the ReAct fold, not as the final answer row. */
  intermediate?: boolean;
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

type ToastSeverity = "success" | "info" | "warning" | "error";

interface LearningProposalToast {
  title: string;
  message: string;
  severity: ToastSeverity;
}

interface LearningProposalPromptAction {
  id: "approve_apply" | "snooze" | "dismiss" | string;
  label: string;
  description: string;
}

interface LearningProposalPrompt {
  proposalId: string;
  kind: string;
  title: string;
  message: string;
  actions: LearningProposalPromptAction[];
}

interface LearningProposalActionResult {
  proposalId: string;
  status: string;
  notification: string;
}

interface StreamToolResultPayload {
  tool_use_id?: string;
  name: string;
  input: string;
  output: string;
  is_error: boolean;
}

function objectRecord(value: unknown): Record<string, unknown> | null {
  return value && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;
}

function stringField(
  value: Record<string, unknown> | null,
  key: string,
): string | null {
  const field = value?.[key];
  return typeof field === "string" && field.trim() ? field.trim() : null;
}

function parseJsonObject(raw: string | undefined): Record<string, unknown> | null {
  if (!raw) return null;
  try {
    return objectRecord(JSON.parse(raw));
  } catch {
    return null;
  }
}

function learningProposalToastFromToolResult(
  resultData: StreamToolResultPayload | undefined,
): LearningProposalToast | null {
  if (!resultData || resultData.is_error) return null;
  if (
    resultData.name !== "learning_proposal_decide" &&
    resultData.name !== "learning_proposal_apply"
  ) {
    return null;
  }
  const output = parseJsonObject(resultData.output);
  if (!output) return null;

  const notification = stringField(output, "notification");
  if (notification) {
    return {
      title:
        resultData.name === "learning_proposal_apply"
          ? "学习建议已固化"
          : "学习建议已更新",
      message: notification,
      severity: "success",
    };
  }

  return null;
}

function learningProposalPromptFromToolResult(
  resultData: StreamToolResultPayload | undefined,
): LearningProposalPrompt | null {
  if (!resultData || resultData.is_error || resultData.name !== "learning_proposal_list") {
    return null;
  }
  const output = parseJsonObject(resultData.output);
  const proposals = Array.isArray(output?.proposals) ? output.proposals : [];
  const firstProposal = proposals.map(objectRecord).find(Boolean) ?? null;
  const proposalId = stringField(firstProposal, "id");
  const message =
    stringField(firstProposal, "userMessage") ??
    stringField(firstProposal, "summary");
  if (!proposalId || !message) return null;

  const rawActions = Array.isArray(firstProposal?.actions)
    ? firstProposal.actions
    : [];
  const actions = rawActions
    .map(objectRecord)
    .filter((action): action is Record<string, unknown> => Boolean(action))
    .map((action) => ({
      id: stringField(action, "id") ?? "approve_apply",
      label: stringField(action, "label") ?? "保存",
      description: stringField(action, "description") ?? "",
    }));

  return {
    proposalId,
    kind: stringField(firstProposal, "kind") ?? "proposal",
    title: stringField(firstProposal, "title") ?? "发现可复用建议",
    message,
    actions: actions.length > 0
      ? actions
      : [
          {
            id: "approve_apply",
            label: "保存",
            description: "确认并保存为项目学习记录。",
          },
          {
            id: "snooze",
            label: "稍后",
            description: "暂时不处理。",
          },
          {
            id: "dismiss",
            label: "忽略",
            description: "不保存这条学习建议。",
          },
        ],
  };
}

function assistantMessageHasVisibleText(message: Message): boolean {
  return message.role !== "assistant" || message.content.trim().length > 0;
}

function renderItemHasVisibleContent(item: RenderMsgItem): boolean {
  if (item.kind === "row") {
    return assistantMessageHasVisibleText(item.message);
  }
  return item.fold.some((message) => {
    if (message.role === "assistant") {
      return assistantMessageHasVisibleText(message);
    }
    return message.role === "tool" && Boolean(message.toolCall);
  });
}

/** One item in the main-session FIFO queue while a previous turn is still streaming. */
interface QueuedMainSend {
  id: string;
  body: string;
  composerAttachedPaths: string[];
  composerSelectedPluginIds: string[];
  composerAgentType: string;
  permissionMode: PermissionMode;
  computerUseMode: ComputerUseMode;
  /** 发送入队时的运行环境（与 composer 一致） */
  environment: ExecutionEnvironment;
  /** SSH 服务器名称（仅在 environment === "ssh" 时有效） */
  sshServer: string | null;
  sandboxBackend: SandboxBackend;
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

type ResearchCommandResponse = {
  sessionId: string;
  roundId: string;
  userMessageId: string;
  assistantMessageId: string;
  assistantContent: string;
  goal?: ResearchGoal | null;
  cycle?: ResearchGoalCycle | null;
};

type ResearchGoalStatusResponse = {
  goal: ResearchGoal | null;
};

type SuggestResearchGoalCriteriaResponse = {
  criteria: string[];
};

function researchGoalErrorMessage(error: unknown, fallback: string): string {
  if (typeof error === "string") return error;
  if (error && typeof error === "object") {
    const err = error as Record<string, unknown>;
    if (
      err.type === "Chat" &&
      err.details &&
      typeof err.details === "object"
    ) {
      const details = err.details as Record<string, unknown>;
      if (typeof details.message === "string") return details.message;
    }
    if (typeof err.message === "string") return err.message;
  }
  return fallback;
}

/** Shown as a user line + sent to the model to continue after the user cancelled a stream. */
const RESUME_AFTER_CANCEL_PROMPT =
  "请从上一轮中断处继续完成回复，衔接已有内容，不要重复已完整输出的段落。";
const CANCEL_STREAM_LOCAL_FALLBACK_MS = 1500;
const JUMP_TO_LATEST_CLICK_ANIMATION_MS = 360;

/** Persist full transcript (including tool rows) to the session store. */
function chatMessageToStore(m: Message): StoreMessage {
  return {
    id: m.id,
    role: m.role,
    content: m.content,
    composerAgentType: m.composerAgentType,
    composerAttachedPaths: m.composerAttachedPaths,
    composerSelectedPluginIds: m.composerSelectedPluginIds,
    initialTodos: m.initialTodos,
    followUpSuggestions: m.followUpSuggestions,
    turnSummary: m.turnSummary,
    tokenUsage: m.tokenUsage,
    prefaceBeforeTools: m.prefaceBeforeTools,
    intermediate: m.intermediate,
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

interface ActivityOperationPayload {
  session_id: string;
  operation_id: string;
  label: string;
  status: "running" | "done" | "error";
  detail?: string | null;
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
  if (isSearchToolName(name)) return "网络搜索";
  if (n === "fetch") return "获取网页";
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
    if (isSearchToolName(name)) {
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
            if (assistantMessageHasVisibleText(m)) {
              fold.push(m);
            }
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
        if (assistantMessageHasVisibleText(msg)) {
          out.push({ kind: "row", message: msg, dividerBefore: false });
        }
      }
      continue;
    }

    let lastAssistantAfterTools = -1;
    for (let k = segment.length - 1; k > lastToolIdx; k--) {
      if (
        segment[k].role === "assistant" &&
        !segment[k].intermediate &&
        assistantMessageHasVisibleText(segment[k])
      ) {
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
  const hierarchy = buildSchedulerPlanHierarchy(plan);
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
        transition: "border-color 200ms ease",
        "&:hover": {
          borderColor: alpha(theme.palette.primary.main, 0.3),
        },
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
          transition: "background-color 150ms ease",
          "&:hover": {
            bgcolor: alpha(theme.palette.primary.main, 0.07),
            // Brighten ExpandMore on hover
            "& > svg:last-of-type": {
              color: "primary.main",
              opacity: 1,
            },
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
            opacity: 0.65,
            transform: expanded ? "rotate(180deg)" : "rotate(0deg)",
            transition: "transform 0.2s ease, color 150ms ease, opacity 150ms ease",
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
          {!hierarchy.legacyFlat && (
            <Box
              sx={{
                mb: 0.75,
                px: 0.75,
                py: 0.55,
                borderRadius: 1,
                bgcolor: alpha(theme.palette.background.paper, 0.45),
                border: `1px solid ${alpha(theme.palette.primary.main, 0.12)}`,
              }}
            >
              <Stack direction="row" spacing={0.5} alignItems="center" flexWrap="wrap" useFlexGap>
                <Chip
                  size="small"
                  label={normalizeAgentDisplayName(hierarchy.entryAgentType)}
                  sx={{
                    height: 17,
                    fontSize: 8.5,
                    bgcolor: alpha(theme.palette.primary.main, 0.12),
                    color: theme.palette.primary.main,
                    fontWeight: 600,
                  }}
                />
                <Typography variant="caption" sx={{ color: "text.disabled" }}>
                  →
                </Typography>
                <Chip
                  size="small"
                  label={normalizeAgentDisplayName(hierarchy.executionSupervisorAgentType)}
                  sx={{
                    height: 17,
                    fontSize: 8.5,
                    bgcolor: alpha(theme.palette.success.main, 0.12),
                    color: theme.palette.success.main,
                    fontWeight: 600,
                  }}
                />
                <Typography variant="caption" sx={{ color: "text.disabled" }}>
                  →
                </Typography>
                <Typography variant="caption" sx={{ color: "text.secondary", fontSize: 10 }}>
                  {hierarchy.children.length} 个子 Agent
                </Typography>
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
                  label={
                    hierarchy.legacyFlat
                      ? normalizeAgentDisplayName(task.agentType)
                      : `${schedulerStageLabel(task.stage)} · ${normalizeAgentDisplayName(task.agentType)}`
                  }
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

type ChatTokens = ReturnType<typeof getChatTokens>;
type ReactFoldRenderItemData = Extract<RenderMsgItem, { kind: "react_fold" }>;
type RowRenderItemData = Extract<RenderMsgItem, { kind: "row" }>;

interface ReactFoldRenderItemProps {
  item: ReactFoldRenderItemData;
  expanded: boolean;
  isLastFold: boolean;
  liveIntermediateText: string;
  activityIsStreaming: boolean;
  waitingFirstChunk: boolean;
  pendingAskUserToolUseId: string | null;
  nestedToolPanelOpenForFold: Readonly<Record<string, boolean>>;
  chat: ChatTokens;
  components: Components;
  onToggleGroup: (groupId: string) => void;
  onToggleNestedToolPanel: (
    foldId: string,
    messageId: string,
    tc: NonNullable<Message["toolCall"]>,
  ) => void;
}

const ReactFoldRenderItem = memo(function ReactFoldRenderItem({
  item,
  expanded,
  isLastFold,
  liveIntermediateText,
  activityIsStreaming,
  waitingFirstChunk,
  pendingAskUserToolUseId,
  nestedToolPanelOpenForFold,
  chat,
  components,
  onToggleGroup,
  onToggleNestedToolPanel,
}: ReactFoldRenderItemProps) {
  const { id, fold } = item;
  const toolMsgs = fold.filter((m) => m.role === "tool" && m.toolCall);
  const summary = summarizeReactFold(fold);
  const anyRunning = toolGroupAnyRunning(toolMsgs);
  const showGroupDone = toolGroupFlowComplete(toolMsgs);
  const runningToolName = firstRunningToolName(toolMsgs);
  const runningToolCount = toolMsgs.filter(
    (m) => m.toolCall?.status === "running",
  ).length;

  return (
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
          bgcolor: chat.agentBubbleBg,
          border: `1px solid ${chat.agentBubbleBorder}`,
          px: 1.75,
          py: 1.25,
          fontFamily: chat.font,
          overflow: "visible",
          transition: "border-color 200ms ease",
          "&:hover": {
            borderColor: alpha(chat.accent, 0.28),
          },
        }}
      >
        <ToolFoldHeader
          foldId={id}
          expanded={expanded}
          summary={summary}
          anyRunning={anyRunning}
          runningToolName={runningToolName}
          runningToolCount={runningToolCount}
          showGroupDone={showGroupDone}
          isLastFold={isLastFold}
          activityIsStreaming={activityIsStreaming}
          waitingFirstChunk={waitingFirstChunk}
          chat={chat}
          onToggle={onToggleGroup}
        />

        <Collapse in={expanded}>
          <Box
            sx={{
              mt: 1.25,
              pl: 1.75,
              ml: 0.5,
              borderLeft: `2px solid ${chat.agentBubbleBorder}`,
              display: "flex",
              flexDirection: "column",
              gap: 1.25,
            }}
          >
            {fold.map((message, foldIndex) => {
              if (message.role === "assistant") {
                if (!assistantMessageHasVisibleText(message)) return null;
                return (
                  <AssistantTraceItem
                    key={message.id}
                    content={message.content}
                    intermediate={message.intermediate}
                    chat={chat}
                    components={components}
                  />
                );
              }
              if (message.role !== "tool" || !message.toolCall) return null;
              const tc = message.toolCall;
              const showAskUserPanel = Boolean(
                pendingAskUserToolUseId &&
                  tc.id &&
                  pendingAskUserToolUseId === tc.id,
              );
              const previousAssistantHasText = Boolean(
                fold[foldIndex - 1]?.role === "assistant" &&
                  fold[foldIndex - 1]?.content?.trim(),
              );
              const isAskUserTool = /ask_user|askuserquestion/i.test(tc.name);
              const hasNestedOpenOverride = message.id in nestedToolPanelOpenForFold;
              const nestedOpen =
                showAskUserPanel && isAskUserTool && !hasNestedOpenOverride
                  ? false
                  : getNestedToolPanelOpen(
                      message.id,
                      tc,
                      nestedToolPanelOpenForFold,
                    );

              return (
                <ToolCallCard
                  key={message.id}
                  foldId={id}
                  messageId={message.id}
                  content={message.content}
                  timestamp={message.timestamp}
                  prefaceBeforeTools={message.prefaceBeforeTools}
                  toolCall={tc}
                  previousAssistantHasText={previousAssistantHasText}
                  nestedOpen={nestedOpen}
                  showAskUserPanel={showAskUserPanel}
                  chat={chat}
                  components={components}
                  onToggle={onToggleNestedToolPanel}
                />
              );
            })}

            {liveIntermediateText && (
              <LiveIntermediateTrace
                foldId={id}
                content={liveIntermediateText}
                chat={chat}
                components={components}
              />
            )}

            {showGroupDone && (
              <Stack direction="row" alignItems="center" spacing={1} sx={{ pt: 0.25 }}>
                <CheckCircle sx={{ fontSize: 14, color: chat.doneGreen }} />
                <Typography
                  sx={{
                    fontSize: 12,
                    fontWeight: 600,
                    color: chat.toolIcon,
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
});

interface MessageRowRenderItemProps {
  item: RowRenderItemData;
  nextRowIsUser: boolean;
  isEditingUser: boolean;
  editDraft: string;
  sessionId: string;
  chat: ChatTokens;
  components: Components;
  onRetryMessage: (message: Message) => void;
  onEditMessage: (message: Message) => void;
  onCopyMessage: (message: Message) => void;
  onEditDraftChange: (draft: string) => void;
  onCancelEdit: () => void;
  onSaveEdit: () => void;
  onOpenReviewerTranscript: (taskId: string) => void;
  onExecutePlan: (
    plan: SchedulerPlan,
    mode: "schedule" | "team" | "autopilot",
  ) => void;
  onDispatchWorkflowCommand: (
    mode: "plan" | "schedule" | "team" | "autopilot",
    body: string,
    autoSend?: boolean,
  ) => void;
}

const MessageRowRenderItem = memo(function MessageRowRenderItem({
  item,
  nextRowIsUser,
  isEditingUser,
  editDraft,
  sessionId,
  chat,
  components,
  onRetryMessage,
  onEditMessage,
  onCopyMessage,
  onEditDraftChange,
  onCancelEdit,
  onSaveEdit,
  onOpenReviewerTranscript,
  onExecutePlan,
  onDispatchWorkflowCommand,
}: MessageRowRenderItemProps) {
  const message = item.message;
  const dividerBefore = item.dividerBefore === true;
  const userRowPb =
    message.role === "user" ? (nextRowIsUser ? 1 : 2) : null;
  const userContentParts =
    message.role === "user"
      ? splitLeadingPathPrefixFromMerged(
          message.content,
          message.composerAttachedPaths ?? [],
        )
      : null;
  const userAttachPaths = userContentParts?.paths ?? [];
  const userBubbleDisplayText =
    message.role === "user"
      ? (userContentParts?.body ?? message.content)
      : message.content;

  return (
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
            borderColor: chat.agentBubbleBorder,
            "&::before, &::after": {
              borderColor: chat.agentBubbleBorder,
            },
          }}
        />
      )}
      <Box
        sx={{
          display: "flex",
          justifyContent: message.role === "user" ? "flex-end" : "flex-start",
          width: "100%",
          minWidth: 0,
          maxWidth: "100%",
          pt: 1,
          pb: userRowPb !== null ? userRowPb : 2,
        }}
      >
        {message.role === "user" ? (
          <UserMessageBubble
            content={message.content}
            displayText={userBubbleDisplayText}
            timestamp={message.timestamp}
            composerAgentType={message.composerAgentType}
            attachedPaths={userAttachPaths}
            selectedPluginIds={message.composerSelectedPluginIds ?? []}
            isEditing={isEditingUser}
            editDraft={editDraft}
            chat={chat}
            bubbleRadiusPx={BUBBLE_RADIUS_PX}
            maxWidth={USER_BUBBLE_MAX_CSS}
            onRetry={() => onRetryMessage(message)}
            onEdit={() => onEditMessage(message)}
            onCopy={() => onCopyMessage(message)}
            onEditDraftChange={onEditDraftChange}
            onCancelEdit={onCancelEdit}
            onSaveEdit={onSaveEdit}
          />
        ) : (
          <AssistantMessageBubble
            content={message.content}
            tokenUsage={message.tokenUsage}
            components={components}
            chat={chat}
            bubbleRadiusPx={BUBBLE_RADIUS_PX}
          />
        )}
      </Box>

      {message.schedulerPlan && message.schedulerPlan.subtasks.length > 1 && (
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
            onOpenReviewerTranscript={onOpenReviewerTranscript}
            onExecutePlan={(mode) => {
              if (!message.schedulerPlan) return;
              onExecutePlan(message.schedulerPlan, mode);
            }}
            onRevisePlan={() => {
              const request =
                message.schedulerPlan?.originalRequest?.trim() ||
                (userContentParts?.body ?? message.content)
                  .replace(/^\/plan\s+/iu, "")
                  .trim();
              onDispatchWorkflowCommand(
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
});

function LazyMarkdownBlockFallback({ label = "正在加载内容…" }: { label?: string }) {
  return (
    <Box
      sx={{
        my: 1.25,
        px: 1.25,
        py: 1,
        borderRadius: `${MD_BLOCK_RADIUS_PX}px`,
        border: 1,
        borderColor: "divider",
        color: "text.secondary",
        fontSize: 12,
      }}
    >
      {label}
    </Box>
  );
}

const MARKDOWN_LOCAL_IMAGE_EXT_RE =
  /\.(?:png|jpe?g|gif|webp|svg|bmp|ico|tiff?|avif)(?:[?#].*)?$/i;

function hasUrlScheme(value: string): boolean {
  return /^[a-z][a-z0-9+.-]*:/i.test(value);
}

function isAbsoluteLocalPath(value: string): boolean {
  return value.startsWith("/") || /^[a-zA-Z]:[\\/]/.test(value);
}

function stripFileQueryAndHash(value: string): {
  path: string;
  suffix: string;
} {
  const index = value.search(/[?#]/);
  if (index === -1) return { path: value, suffix: "" };
  return { path: value.slice(0, index), suffix: value.slice(index) };
}

function resolveMarkdownImageSrc(src: string, workspacePath: string): string {
  const raw = src.trim();
  if (!raw) return "";
  if (/^data:image\/[^;]+;base64,/i.test(raw)) return "";

  if (/^(?:https?|blob|asset|tauri):/i.test(raw)) return raw;

  if (/^file:\/\//i.test(raw)) {
    try {
      return convertFileSrc(decodeURIComponent(new URL(raw).pathname));
    } catch {
      return raw;
    }
  }

  if (isAbsoluteLocalPath(raw)) {
    const { path, suffix } = stripFileQueryAndHash(raw);
    return `${convertFileSrc(path)}${suffix}`;
  }

  if (!hasUrlScheme(raw) && workspacePath && MARKDOWN_LOCAL_IMAGE_EXT_RE.test(raw)) {
    const base = workspacePath.replace(/[\\/]+$/, "");
    const relative = raw.replace(/^\.?[\\/]+/, "");
    const { path, suffix } = stripFileQueryAndHash(`${base}/${relative}`);
    return `${convertFileSrc(path)}${suffix}`;
  }

  return raw;
}

function buildMarkdownComponents(
  isAgent: boolean,
  theme: Theme,
  CHAT: ReturnType<typeof getChatTokens>,
  onImageClick: (src: string, alt: string) => void,
  onNodeClick?: (text: string) => void,
  workspacePath = "",
) {
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
        return (
          <Suspense fallback={<LazyMarkdownBlockFallback label="正在加载可视化…" />}>
            <VisualizationRenderer config={config} onNodeClick={onNodeClick} />
          </Suspense>
        );
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
        return (
          <Suspense fallback={<LazyMarkdownBlockFallback label="正在加载 DAG…" />}>
            <DagFlow data={dag} onNodeClick={onNodeClick} />
          </Suspense>
        );
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
          <Suspense fallback={<LazyMarkdownBlockFallback label="正在加载流程图…" />}>
            <OmigaFlowchart
              data={fc}
              isAgent={isAgent}
              onStepClick={onNodeClick ? (text) => onNodeClick(text) : undefined}
            />
          </Suspense>
        );
      }

      // Raw ```mermaid fenced block → React Flow
      if (language === "mermaid") {
        return (
          <Suspense fallback={<LazyMarkdownBlockFallback label="正在加载 Mermaid 图…" />}>
            <MermaidFlow source={blockBody} onNodeClick={onNodeClick} />
          </Suspense>
        );
      }

      // Raw ```dot / ```graphviz fenced block → React Flow
      if (language === "dot" || language === "graphviz") {
        return (
          <Suspense fallback={<LazyMarkdownBlockFallback label="正在加载 Graphviz 图…" />}>
            <DotFlow dot={blockBody} onNodeClick={onNodeClick} />
          </Suspense>
        );
      }

      const lang = language || "text";
      return (
        <Suspense fallback={<LazyMarkdownBlockFallback label={`正在加载 ${lang} 代码块…`} />}>
          <FencedCodeBlock code={blockBody} lang={lang} isAgent={isAgent} chat={CHAT} />
        </Suspense>
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
    const resolvedUrl = resolveMarkdownImageSrc(url, workspacePath);
    if (!resolvedUrl) {
      return (
        <Alert severity="warning" sx={{ my: 1 }}>
          内联图片数据已隐藏。请使用 Markdown 文件路径引用输出图片，例如{" "}
          <Box component="code" sx={{ fontFamily: "monospace" }}>
            ![图](&lt;path/to/image.png&gt;)
          </Box>
          。
        </Alert>
      );
    }
    return (
      <Box
        component="img"
        src={resolvedUrl}
        alt={typeof alt === "string" ? alt : ""}
        onClick={() =>
          onImageClick(resolvedUrl, typeof alt === "string" ? alt : "")
        }
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
          mx: 0,
          pl: 1.5,
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
  const converted = storeMessages.map((msg, index) => ({
    id: msg.id || `${sessionId}-msg-${index}`,
    role: msg.role,
    content: msg.content ?? "",
    composerAgentType: msg.composerAgentType,
    composerAttachedPaths: msg.composerAttachedPaths,
    composerSelectedPluginIds: msg.composerSelectedPluginIds,
    followUpSuggestions: msg.followUpSuggestions,
    turnSummary: msg.turnSummary,
    tokenUsage: msg.tokenUsage,
    prefaceBeforeTools: msg.prefaceBeforeTools,
    intermediate: msg.intermediate,
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
  return normalizeAssistantToolCallPrefaces(converted);
}

export function Chat({ sessionId }: ChatProps) {
  const theme = useTheme();
  const CHAT = useMemo(() => getChatTokens(theme), [theme]);
  const isDev = import.meta.env.DEV;
  const [panelTab, setPanelTab] = useState(0);
  const composerRef = useRef<ChatComposerRef>(null);
  const [messages, setMessages] = useState<Message[]>([]);
  const messagesRef = useRef<Message[]>([]);
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
  const progressivePhase2StartRef = useRef<number | null>(null);
  const progressiveLastAddedHeightRef = useRef<number | null>(null);
  const progressivePhase1LoggedRef = useRef<string | null>(null);
  const progressivePhase2LoggedRef = useRef<string | null>(null);
  const [isStreaming, setIsStreaming] = useState(false);
  /** True while background follow-up suggestions are being generated after `complete` fires */
  const [suggestionsGenerating, setSuggestionsGenerating] = useState(false);
  const [currentResponse, setCurrentResponse] = useState("");
  const [currentFoldIntermediate, setCurrentFoldIntermediate] = useState("");
  const [pendingAssistantHint, setPendingAssistantHint] = useState<string | null>(null);
  const [currentStreamId, setCurrentStreamId] = useState<string | null>(null);
  const [currentRoundId, setCurrentRoundId] = useState<string | null>(null);
  /** After cancel_stream, offer header “断点继续” until a new turn completes or the user sends again. */
  const [awaitingResumeAfterCancel, setAwaitingResumeAfterCancel] =
    useState(false);
  /** Toast when a background bash command completes (`background-shell-complete`). */
  const [bgToast, setBgToast] = useState<string | null>(null);
  /** Human-facing learning proposal notification; JSON stays in tool output. */
  const [learningProposalToast, setLearningProposalToast] =
    useState<LearningProposalToast | null>(null);
  const [learningProposalPrompt, setLearningProposalPrompt] =
    useState<LearningProposalPrompt | null>(null);
  const [learningProposalBusyAction, setLearningProposalBusyAction] =
    useState<string | null>(null);
  const learningProposalSeenRef = useRef<Set<string>>(new Set());
  /** Background Agent tasks (Rust `BackgroundAgentManager`) for teammate-style follow-ups. */
  const [backgroundTasks, setBackgroundTasks] = useState<BackgroundAgentTask[]>(
    [],
  );
  /** Persisted `/goal` state for the current chat session, shown above the composer. */
  const [activeResearchGoal, setActiveResearchGoal] =
    useState<ResearchGoal | null>(null);
  const goalAutoRunEnabled = activeResearchGoal?.autoRunPolicy?.enabled ?? false;
  const [goalCriteriaDialogOpen, setGoalCriteriaDialogOpen] = useState(false);
  const [goalAuditDialogOpen, setGoalAuditDialogOpen] = useState(false);
  const [goalCriteriaSaving, setGoalCriteriaSaving] = useState(false);
  const [goalCriteriaError, setGoalCriteriaError] = useState<string | null>(
    null,
  );
  const [goalProviderEntryOptions, setGoalProviderEntryOptions] = useState<
    ResearchGoalProviderEntryOption[]
  >([]);
  const [goalProviderEntryOptionsLoading, setGoalProviderEntryOptionsLoading] =
    useState(false);
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
  /** Per-tool nested panels inside a fold: fold id → message id → open. Unset → default (open while running). */
  const [nestedToolPanelOpenByFold, setNestedToolPanelOpenByFold] =
    useState<NestedToolPanelOpenByFold>({});
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
  const goalAutoRunLastKeyRef = useRef<string | null>(null);
  const goalAutoRunTimerRef = useRef<number | null>(null);
  /**
   * Set immediately before `handleSend` when draining the main-session FIFO queue.
   * - Avoids stale React `input` in the `handleSend` closure right after `flushSync`.
   * - Forces main-session `send_message` (never `inputTarget: bg:`) — the queue is main-only.
   */
  const mainQueueFlushPayloadRef = useRef<QueuedMainSend | null>(null);

  const queuedMainMessagesForComposer = useMemo(() => {
    void queueRevision;
    return queuedMainSendQueueRef.current.map((item) => {
      const pathLine = formatComposerPathPreview(item.composerAttachedPaths);
      const pluginLine = item.composerSelectedPluginIds
        .map((id) => `#${id}`)
        .join(" ");
      const prefix = [pluginLine, pathLine].filter(Boolean).join(" ");
      const merged =
        prefix && item.body ? `${prefix}\n\n${item.body}` : prefix || item.body;
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
        st.clearComposerSelectedPluginIds();
        for (const id of item.composerSelectedPluginIds) {
          st.addComposerSelectedPluginId(id);
        }
        st.setComposerAgentType(item.composerAgentType);
        st.setPermissionMode(item.permissionMode);
        st.setComputerUseMode(item.computerUseMode);
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
  const messagesContentRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLElement>(null);
  const prepareResearchGoalCommand = useCallback((command: string) => {
    composerRef.current?.setValue(command);
    queueMicrotask(() => inputRef.current?.focus());
  }, []);
  const clearGoalAutoRunTimer = useCallback(() => {
    if (goalAutoRunTimerRef.current !== null) {
      window.clearTimeout(goalAutoRunTimerRef.current);
      goalAutoRunTimerRef.current = null;
    }
  }, []);
  /** Populated by `follow_up_suggestions` stream frame; consumed when attaching the final assistant row */
  const pendingFollowUpSuggestionsRef = useRef<Array<{
    label: string;
    prompt: string;
  }> | null>(null);
  /**
   * Stream id whose completed turn may still emit post-turn metadata.
   * Cleared as soon as the user/queue starts another main turn so stale
   * background metadata events cannot leak into the next run.
   */
  const postTurnMetaStreamIdRef = useRef<string | null>(null);
  /** Stream id currently allowed to show/clear the suggestions-generating placeholder. */
  const postTurnSuggestionStreamIdRef = useRef<string | null>(null);
  const clearPostTurnSuggestionsIndicator = useCallback(() => {
    postTurnSuggestionStreamIdRef.current = null;
    pendingFollowUpSuggestionsRef.current = null;
    setSuggestionsGenerating(false);
  }, []);
  const clearPostTurnMetaState = useCallback(() => {
    postTurnMetaStreamIdRef.current = null;
    clearPostTurnSuggestionsIndicator();
  }, [clearPostTurnSuggestionsIndicator]);
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
  const sessionIdRef = useRef<string | null>(sessionId ?? null);
  const currentStreamIdRef = useRef<string | null>(currentStreamId);
  const currentResponseRef = useRef(currentResponse);
  const currentFoldIntermediateRef = useRef(currentFoldIntermediate);
  const currentRoundIdRef = useRef(currentRoundId);
  const cancelFallbackTimerRef = useRef<number | null>(null);
  const toolWatchdogTimersRef = useRef<Map<string, number>>(new Map());
  /** Buffered text chunks waiting to be batched into React state */
  const pendingTextBufferRef = useRef("");
  /** Buffered provider reasoning/tool-gap chunks shown inside the active ReAct fold. */
  const pendingFoldIntermediateBufferRef = useRef("");
  /** Ordinary text emitted since the last tool call; captured as the next tool's visible preface. */
  const pendingToolTraceTextRef = useRef("");
  const activeRunningToolIdsRef = useRef<Set<string>>(new Set());
  const activeReactFoldIdRef = useRef<string | null>(null);
  /** RAF handle for scheduled text flush */
  const textFlushRafRef = useRef<number | null>(null);
  /** RAF handle for scheduled fold-intermediate flush */
  const foldIntermediateFlushRafRef = useRef<number | null>(null);

  const clearCancelFallbackTimer = useCallback(() => {
    if (cancelFallbackTimerRef.current !== null) {
      window.clearTimeout(cancelFallbackTimerRef.current);
      cancelFallbackTimerRef.current = null;
    }
  }, []);

  // Keep refs in sync with state for access in event listeners
  useEffect(() => {
    activeRunningToolIdsRef.current = new Set(
      messages
        .filter(
          (m) =>
            m.role === "tool" &&
            m.toolCall?.status === "running" &&
            Boolean(m.toolCall.id),
        )
        .map((m) => m.toolCall!.id!),
    );
  }, [messages]);

  useEffect(() => {
    currentResponseRef.current = currentResponse;
  }, [currentResponse]);

  useEffect(() => {
    currentFoldIntermediateRef.current = currentFoldIntermediate;
  }, [currentFoldIntermediate]);

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

  const runResearchGoalCommandWhenIdle = useCallback(
    (command: string): boolean => {
      if (!sessionId || isMainReplyBusy()) return false;
      const st = useChatComposerStore.getState();
      if (
        researchGoalShouldWaitForComposerDraft(
          composerRef.current?.getValue() ?? "",
          st.composerAttachedPaths,
          st.composerSelectedPluginIds,
        )
      ) {
        return false;
      }
      mainQueueFlushPayloadRef.current = {
        id:
          typeof crypto !== "undefined" &&
          typeof crypto.randomUUID === "function"
            ? crypto.randomUUID()
            : `goal-auto-${Date.now()}`,
        body: command,
        composerAttachedPaths: [],
        composerSelectedPluginIds: [],
        composerAgentType: "general-purpose",
        permissionMode: st.permissionMode,
        computerUseMode: "off",
        environment: st.environment,
        sshServer: st.sshServer,
        sandboxBackend: st.sandboxBackend,
      };
      void handleSendRef.current();
      return true;
    },
    [isMainReplyBusy, sessionId],
  );

  useEffect(() => {
    if (!isConnecting) {
      setPendingAssistantHint(null);
    }
  }, [isConnecting]);

  useEffect(() => {
    currentRoundIdRef.current = currentRoundId;
  }, [currentRoundId]);

  useEffect(() => {
    currentStreamIdRef.current = currentStreamId;
  }, [currentStreamId]);

  useEffect(() => {
    sessionIdRef.current = sessionId ?? null;
  }, [sessionId]);

  useEffect(() => {
    return () => {
      clearGoalAutoRunTimer();
      clearCancelFallbackTimer();
      if (textFlushRafRef.current !== null) {
        cancelAnimationFrame(textFlushRafRef.current);
        textFlushRafRef.current = null;
      }
      if (foldIntermediateFlushRafRef.current !== null) {
        cancelAnimationFrame(foldIntermediateFlushRafRef.current);
        foldIntermediateFlushRafRef.current = null;
      }
      if (scrollRafRef.current !== null) {
        cancelAnimationFrame(scrollRafRef.current);
        scrollRafRef.current = null;
      }
    };
  }, [clearCancelFallbackTimer, clearGoalAutoRunTimer]);

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

  const researchGoalProjectPath =
    currentSession?.workingDirectory ?? currentSession?.projectPath ?? ".";

  useEffect(() => {
    messagesRef.current = messages;
  }, [messages]);

  useEffect(() => {
    if (!sessionId) {
      setActiveResearchGoal(null);
      setGoalCriteriaDialogOpen(false);
      setGoalAuditDialogOpen(false);
      setLearningProposalPrompt(null);
      return;
    }

    const requestSessionId = sessionId;
    let cancelled = false;

    void invoke<ResearchGoalStatusResponse>("get_research_goal_status", {
      request: {
        sessionId: requestSessionId,
        projectPath: researchGoalProjectPath,
      },
    })
      .then((response) => {
        if (!cancelled && sessionIdRef.current === requestSessionId) {
          setActiveResearchGoal(response.goal ?? null);
        }
      })
      .catch((error) => {
        console.warn("[Chat] Failed to load /goal status:", error);
        if (!cancelled && sessionIdRef.current === requestSessionId) {
          setActiveResearchGoal(null);
        }
      });

    return () => {
      cancelled = true;
    };
  }, [sessionId, researchGoalProjectPath]);

  useEffect(() => {
    learningProposalSeenRef.current.clear();
    setLearningProposalPrompt(null);
    setLearningProposalBusyAction(null);
  }, [sessionId, researchGoalProjectPath]);

  const handleLearningProposalAction = useCallback(
    async (actionId: string) => {
      const prompt = learningProposalPrompt;
      if (!prompt || learningProposalBusyAction) return;
      const projectRoot =
        currentSession?.workingDirectory ?? currentSession?.projectPath ?? "";
      if (!projectRoot.trim()) {
        setLearningProposalPrompt(null);
        return;
      }
      setLearningProposalBusyAction(actionId);
      try {
        const result = await invoke<LearningProposalActionResult>(
          "learning_proposal_respond",
          {
            projectRoot,
            proposalId: prompt.proposalId,
            action: actionId,
          },
        );
        setLearningProposalPrompt(null);
        setLearningProposalToast({
          title:
            result.status === "applied"
              ? "学习记录已保存"
              : "学习建议已更新",
          message: result.notification,
          severity: result.status === "applied" ? "success" : "info",
        });
      } catch (error) {
        console.warn("[Chat] Failed to respond to learning proposal:", error);
        setLearningProposalToast({
          title: "学习建议处理失败",
          message:
            typeof error === "string"
              ? error
              : "无法更新这条学习建议，请稍后重试。",
          severity: "error",
        });
      } finally {
        setLearningProposalBusyAction(null);
      }
    },
    [
      currentSession?.projectPath,
      currentSession?.workingDirectory,
      learningProposalBusyAction,
      learningProposalPrompt,
    ],
  );

  useEffect(() => {
    goalAutoRunLastKeyRef.current = null;
    clearGoalAutoRunTimer();
  }, [clearGoalAutoRunTimer, sessionId]);

  const loadResearchGoalProviderEntryOptions = useCallback(async () => {
    setGoalProviderEntryOptionsLoading(true);
    try {
      const entries =
        await invoke<ResearchGoalProviderEntryOption[]>("list_provider_configs");
      setGoalProviderEntryOptions((entries || []).filter((entry) => entry.enabled));
    } catch (error) {
      console.warn("[Chat] Failed to load provider entries for /goal:", error);
      setGoalProviderEntryOptions([]);
    } finally {
      setGoalProviderEntryOptionsLoading(false);
    }
  }, []);

  const handleOpenResearchGoalCriteria = useCallback(() => {
    setGoalCriteriaError(null);
    setGoalCriteriaDialogOpen(true);
    void loadResearchGoalProviderEntryOptions();
  }, [loadResearchGoalProviderEntryOptions]);

  const handleOpenResearchGoalAuditDetails = useCallback(() => {
    setGoalAuditDialogOpen(true);
  }, []);

  const updateResearchGoalAutoRunPolicy = useCallback(
    async (goal: ResearchGoal, enabled: boolean) => {
      if (!sessionId) return;
      const policy = goal.autoRunPolicy;
      const response = await invoke<ResearchGoalStatusResponse>(
        "update_research_goal_settings",
        {
          request: {
            sessionId,
            projectPath: researchGoalProjectPath,
            autoRunPolicy: {
              enabled,
              cyclesPerRun: policy?.cyclesPerRun ?? 10,
              idleDelayMs: policy?.idleDelayMs ?? 650,
              maxElapsedMinutes: policy?.maxElapsedMinutes ?? null,
              maxTokens: policy?.maxTokens ?? null,
            },
          },
        },
      );
      setActiveResearchGoal(response.goal ?? null);
    },
    [researchGoalProjectPath, sessionId],
  );

  const handleToggleResearchGoalAutoRun = useCallback(() => {
    const goal = activeResearchGoal;
    if (!goal) return;
    const nextEnabled = !(goal.autoRunPolicy?.enabled ?? false);
    if (nextEnabled && !researchGoalCanAutoRun(goal)) return;
    goalAutoRunLastKeyRef.current = null;
    void updateResearchGoalAutoRunPolicy(goal, nextEnabled).catch((error) => {
      console.warn("[Chat] Failed to update /goal auto-run policy:", error);
      setGoalCriteriaError(
        researchGoalErrorMessage(error, "更新自动续跑策略失败"),
      );
    });
  }, [activeResearchGoal, updateResearchGoalAutoRunPolicy]);

  const handleCloseResearchGoalCriteria = useCallback(() => {
    if (goalCriteriaSaving) return;
    setGoalCriteriaError(null);
    setGoalCriteriaDialogOpen(false);
  }, [goalCriteriaSaving]);

  const handleSaveResearchGoalCriteria = useCallback(
    async (settings: ResearchGoalSettingsDraft) => {
      if (!sessionId || !activeResearchGoal) return;
      setGoalCriteriaSaving(true);
      setGoalCriteriaError(null);
      try {
        const response = await invoke<ResearchGoalStatusResponse>(
          "update_research_goal_settings",
          {
            request: {
              sessionId,
              projectPath: researchGoalProjectPath,
              criteria: settings.criteria,
              maxCycles: settings.maxCycles,
              secondOpinionProviderEntry:
                settings.secondOpinionProviderEntry.trim(),
              autoRunPolicy: settings.autoRunPolicy,
            },
          },
        );
        setActiveResearchGoal(response.goal ?? null);
        setGoalCriteriaDialogOpen(false);
      } catch (error: unknown) {
        setGoalCriteriaError(
          researchGoalErrorMessage(error, "保存科研目标设置失败"),
        );
      } finally {
        setGoalCriteriaSaving(false);
      }
    },
    [activeResearchGoal, researchGoalProjectPath, sessionId],
  );

  const handleSuggestResearchGoalCriteria = useCallback(async () => {
    if (!sessionId || !activeResearchGoal) return [];
    try {
      const response = await invoke<SuggestResearchGoalCriteriaResponse>(
        "suggest_research_goal_criteria",
        {
          request: {
            sessionId,
            projectPath: researchGoalProjectPath,
          },
        },
      );
      return response.criteria;
    } catch (error: unknown) {
      throw new Error(researchGoalErrorMessage(error, "LLM 成功标准生成失败"));
    }
  }, [activeResearchGoal, researchGoalProjectPath, sessionId]);

  const handleTestResearchGoalSecondOpinionProvider = useCallback(
    async (providerEntry: string): Promise<ResearchGoalProviderTestResult> => {
      return invoke<ResearchGoalProviderTestResult>(
        "test_research_goal_second_opinion_provider",
        {
          request: {
            providerEntry,
          },
        },
      );
    },
    [],
  );

  const commitMessagesSnapshot = useCallback(
    (next: Message[]) => {
      messagesRef.current = next;
      setMessages(next);
      replaceStoreMessagesSnapshot(next.map(chatMessageToStore));
    },
    [replaceStoreMessagesSnapshot],
  );

  const clearToolWatchdog = useCallback((toolUseId: string) => {
    const timer = toolWatchdogTimersRef.current.get(toolUseId);
    if (timer !== undefined) {
      window.clearTimeout(timer);
      toolWatchdogTimersRef.current.delete(toolUseId);
    }
  }, []);

  const clearAllToolWatchdogs = useCallback(() => {
    for (const timer of toolWatchdogTimersRef.current.values()) {
      window.clearTimeout(timer);
    }
    toolWatchdogTimersRef.current.clear();
  }, []);

  const markToolRunning = useCallback((toolUseId: string | undefined | null) => {
    const id = toolUseId?.trim();
    if (!id) return;
    activeRunningToolIdsRef.current.add(id);
  }, []);

  const markToolSettled = useCallback((toolUseId: string | undefined | null) => {
    const id = toolUseId?.trim();
    if (!id) return;
    activeRunningToolIdsRef.current.delete(id);
  }, []);

  const clearRunningToolTracking = useCallback(() => {
    activeRunningToolIdsRef.current.clear();
    pendingToolTraceTextRef.current = "";
  }, []);

  const startToolWatchdog = useCallback(
    (toolUseId: string, toolName: string) => {
      if (!toolUseId || !isSearchToolName(toolName)) return;
      clearToolWatchdog(toolUseId);
      const timer = window.setTimeout(() => {
        toolWatchdogTimersRef.current.delete(toolUseId);
        markToolSettled(toolUseId);
        const timeoutSeconds = Math.round(SEARCH_CLIENT_WATCHDOG_MS / 1000);
        const watchdogOutput =
          `search has been running for ${timeoutSeconds}s without a result. ` +
          "Stopped showing it as running to avoid a stuck UI; cancel and retry with a narrower query if the backend does not continue.";
        setMessages((prev) => {
          let didMark = false;
          const next = prev.map((m) => {
            if (
              m.role !== "tool" ||
              m.toolCall?.id !== toolUseId ||
              m.toolCall.status !== "running"
            ) {
              return m;
            }
            didMark = true;
            const name = m.toolCall.name || toolName || "search";
            return {
              ...m,
              content: `\`${name}\` timed out`,
              toolCall: {
                ...m.toolCall,
                name,
                status: "error" as const,
                output: watchdogOutput,
                completedAt: Date.now(),
              },
            };
          });
          if (didMark) {
            replaceStoreMessagesSnapshot(next.map(chatMessageToStore));
            queueMicrotask(() => {
              useActivityStore.getState().onToolResultDone(toolUseId, {
                output: watchdogOutput,
                failed: true,
              });
            });
          }
          return didMark ? next : prev;
        });
      }, SEARCH_CLIENT_WATCHDOG_MS);
      toolWatchdogTimersRef.current.set(toolUseId, timer);
    },
    [clearToolWatchdog, markToolSettled, replaceStoreMessagesSnapshot],
  );

  useEffect(() => clearAllToolWatchdogs, [clearAllToolWatchdogs, sessionId]);

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
    messagesRef.current = converted;
    setMessages(converted);
    // Phase-1 of progressive rendering: immediately show only recent items.
    // Phase-2 (older items) is scheduled after the first paint via the effect below.
    setAllItemsVisible(converted.length <= INSTANT_RENDER_COUNT);
    progressivePhase2StartRef.current = null;
    progressiveLastAddedHeightRef.current = null;
    progressivePhase1LoggedRef.current = null;
    progressivePhase2LoggedRef.current = null;
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
    // ── Session switch: save → restore (or clear) ─────────────────────────
    //
    // Design: stream listeners are NEVER unregistered on session switch.
    // Each listener captures ownerSessionId at setup. While the user is on
    // another session, events update that session's snapshot instead of the
    // visible React state. When the user switches BACK, the snapshot restores
    // the latest accumulated stream state immediately.
    //
    // The snapshot map persists the accumulated response text and activity
    // state so the restored session's UI is correct immediately on return.

    // Capture the session we are ENTERING now.  The return function (cleanup)
    // runs before the next invocation, at which point refs still hold the
    // state for THIS session — used to build the save snapshot.
    const sessionBeingEntered = sessionId ?? null;

    clearCancelFallbackTimer();
    if (textFlushRafRef.current !== null) {
      cancelAnimationFrame(textFlushRafRef.current);
      textFlushRafRef.current = null;
    }
    if (foldIntermediateFlushRafRef.current !== null) {
      cancelAnimationFrame(foldIntermediateFlushRafRef.current);
      foldIntermediateFlushRafRef.current = null;
    }
    clearPostTurnMetaState();
    pendingTurnSummaryRef.current = null;
    pendingTokenUsageRef.current = null;
    clearRunningToolTracking();
    retrySendInFlightRef.current = false;
    sendCancelledDuringRequestRef.current = false;
    queuedMainSendQueueRef.current = [];
    bumpQueueUi();
    setExpandedToolGroups(new Set());
    setNestedToolPanelOpenByFold({});
    setPendingAssistantHint(null);

    // Try to restore the state of the session we're switching TO.
    const snap = sessionBeingEntered
      ? loadStreamSnapshot(sessionBeingEntered)
      : null;
    const latestActivity = sessionBeingEntered
      ? loadLatestActivitySnapshot(sessionBeingEntered)
      : null;
    const restoring = snapshotIsActive(snap);

    if (restoring && snap) {
      // ── Returning to a session that is (or was recently) streaming ────────
      setCurrentStreamId(snap.streamId);
      currentStreamIdRef.current = snap.streamId;
      setCurrentRoundId(snap.roundId);
      currentRoundIdRef.current = snap.roundId;
      setCurrentResponse(snap.response);
      currentResponseRef.current = snap.response;
      setCurrentFoldIntermediate(snap.foldIntermediate);
      currentFoldIntermediateRef.current = snap.foldIntermediate;
      pendingTextBufferRef.current = snap.pendingText;
      pendingFoldIntermediateBufferRef.current = snap.pendingFoldText;
      setIsStreaming(snap.isStreaming);
      isStreamingRef.current = snap.isStreaming;
      setAwaitingResumeAfterCancel(false);
      // Restore the activity panel for this session.
      useActivityStore.setState({
        isConnecting: snap.isConnecting,
        isStreaming: snap.isStreaming,
        waitingFirstChunk: snap.waitingFirstChunk,
        currentToolHint: snap.currentToolHint,
        executionSteps: snap.executionSteps,
        executionStartedAt: snap.executionStartedAt,
        executionEndedAt: snap.executionEndedAt,
        activeTodos: snap.activeTodos,
        backgroundJobs: snap.backgroundJobs,
      });
    } else {
      // ── Fresh / idle session — reset all stream state ─────────────────────
      pendingTextBufferRef.current = "";
      pendingFoldIntermediateBufferRef.current = "";
      pendingToolTraceTextRef.current = "";
      setCurrentStreamId(null);
      currentStreamIdRef.current = null;
      setCurrentRoundId(latestActivity?.roundId ?? null);
      currentRoundIdRef.current = latestActivity?.roundId ?? null;
      setCurrentResponse("");
      currentResponseRef.current = "";
      setCurrentFoldIntermediate("");
      currentFoldIntermediateRef.current = "";
      activeReactFoldIdRef.current = null;
      setIsStreaming(false);
      isStreamingRef.current = false;
      setAwaitingResumeAfterCancel(false);
      if (latestActivity && activitySnapshotHasRecords(latestActivity)) {
        useActivityStore.setState(
          historicalActivityStateFromSnapshot(latestActivity),
        );
      } else {
        useActivityStore.getState().clearAllActivity();
      }
    }

    // ── Save state when we LEAVE this session (return = cleanup) ─────────────
    // This closure captures sessionBeingEntered.  When the next session switch
    // happens, React calls this cleanup BEFORE running the new effect body, so
    // all refs still hold the state for sessionBeingEntered.
    return () => {
      if (!sessionBeingEntered) return;
      const act = useActivityStore.getState();
      saveStreamSnapshot(sessionBeingEntered, {
        streamId: currentStreamIdRef.current,
        roundId: currentRoundIdRef.current,
        response: currentResponseRef.current,
        foldIntermediate: currentFoldIntermediateRef.current,
        pendingText: pendingTextBufferRef.current,
        pendingFoldText: pendingFoldIntermediateBufferRef.current,
        isStreaming: isStreamingRef.current,
        isConnecting: act.isConnecting,
        waitingFirstChunk: act.waitingFirstChunk,
        currentToolHint: act.currentToolHint,
        executionSteps: [...act.executionSteps],
        executionStartedAt: act.executionStartedAt,
        executionEndedAt: act.executionEndedAt,
        activeTodos: act.activeTodos,
        backgroundJobs: [...act.backgroundJobs],
      });
    };
  }, [
    sessionId,
    bumpQueueUi,
    clearCancelFallbackTimer,
    clearPostTurnMetaState,
    clearRunningToolTracking,
  ]);

  useEffect(() => {
    if (!sessionId) return;
    const persist = (act: ReturnType<typeof useActivityStore.getState>) => {
      const snapshot = buildSessionActivitySnapshot(sessionId, {
        roundId: currentRoundIdRef.current,
        executionSteps: act.executionSteps,
        executionStartedAt: act.executionStartedAt,
        executionEndedAt: act.executionEndedAt,
        activeTodos: act.activeTodos,
        backgroundJobs: act.backgroundJobs,
      });
      if (activitySnapshotHasRecords(snapshot)) {
        saveLatestActivitySnapshot(sessionId, snapshot);
      }
    };

    persist(useActivityStore.getState());
    return useActivityStore.subscribe(persist);
  }, [sessionId]);

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
      progressivePhase2StartRef.current = performance.now();
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
    let addedHeight = 0;
    if (el) {
      addedHeight = el.scrollHeight - scrollRestoreRef.current;
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
    progressiveLastAddedHeightRef.current = addedHeight;
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
      const u1 = await listenTauriEvent("background-agent-update", () => {
        void refreshBackgroundTasks();
      });
      const u2 = await listenTauriEvent("background-agent-complete", () => {
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
      const u1 = await listenTauriEvent<{ session_id: string }>("chat-index-start", ({ payload }) => {
        if (payload.session_id !== sessionId) return;
        if (completedTimer) clearTimeout(completedTimer);
        setIndexingStatus("indexing");
      });
      const u2 = await listenTauriEvent<{ session_id: string }>("chat-index-complete", ({ payload }) => {
        if (payload.session_id !== sessionId) return;
        setIndexingStatus("completed");
        completedTimer = setTimeout(() => setIndexingStatus("idle"), 2500);
      });
      const u3 = await listenTauriEvent<{ session_id: string; error: string }>("chat-index-error", ({ payload }) => {
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
      unlisten = await listenTauriEvent<{ sessionId: string; title: string }>(
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

  useEffect(() => {
    clearGoalAutoRunTimer();
    if (!goalAutoRunEnabled) return;

    const goal = activeResearchGoal;
    if (!goal) {
      goalAutoRunLastKeyRef.current = null;
      return;
    }
    if (
      researchGoalAutoRunElapsedBudgetReached(goal) ||
      !researchGoalCanAutoRun(goal)
    ) {
      goalAutoRunLastKeyRef.current = null;
      void updateResearchGoalAutoRunPolicy(goal, false).catch((error) => {
        console.warn("[Chat] Failed to disable /goal auto-run policy:", error);
      });
      return;
    }
    if (
      needsWorkspacePath ||
      followUpTaskId ||
      isConnecting ||
      isStreaming ||
      activityIsStreaming ||
      waitingFirstChunk ||
      isMainReplyBusy() ||
      queuedMainSendQueueRef.current.length > 0
    ) {
      return;
    }

    const command = buildResearchGoalAutoRunCommand(goal);
    const key = `${sessionId}:${goal.goalId}:${goal.currentCycle}:${goal.maxCycles}:${goal.autoRunPolicy?.cyclesPerRun ?? 10}:${command}`;
    if (goalAutoRunLastKeyRef.current === key) return;

    goalAutoRunTimerRef.current = window.setTimeout(() => {
      goalAutoRunTimerRef.current = null;
      if (
        researchGoalAutoRunElapsedBudgetReached(goal) ||
        isMainReplyBusy() ||
        queuedMainSendQueueRef.current.length > 0 ||
        sessionIdRef.current !== sessionId
      ) {
        if (researchGoalAutoRunElapsedBudgetReached(goal)) {
          void updateResearchGoalAutoRunPolicy(goal, false).catch((error) => {
            console.warn("[Chat] Failed to disable /goal auto-run policy:", error);
          });
        }
        return;
      }
      if (runResearchGoalCommandWhenIdle(command)) {
        goalAutoRunLastKeyRef.current = key;
      }
    }, goal.autoRunPolicy?.idleDelayMs ?? 650);

    return clearGoalAutoRunTimer;
  }, [
    activeResearchGoal,
    activityIsStreaming,
    clearGoalAutoRunTimer,
    followUpTaskId,
    goalAutoRunEnabled,
    isConnecting,
    isMainReplyBusy,
    isStreaming,
    needsWorkspacePath,
    queueRevision,
    runResearchGoalCommandWhenIdle,
    sessionId,
    updateResearchGoalAutoRunPolicy,
    waitingFirstChunk,
  ]);

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

  const toggleToolGroupExpand = useCallback((groupId: string) => {
    setExpandedToolGroups((prev) => {
      const next = new Set(prev);
      if (next.has(groupId)) next.delete(groupId);
      else next.add(groupId);
      return next;
    });
  }, []);

  const toggleNestedToolPanel = useCallback((
    foldId: string,
    messageId: string,
    tc: NonNullable<Message["toolCall"]>,
  ) => {
    setNestedToolPanelOpenByFold((prev) => {
      const foldOverrides = getNestedToolPanelOpenForFold(prev, foldId);
      const cur = getNestedToolPanelOpen(messageId, tc, foldOverrides);
      return toggleNestedToolPanelOpenForFold(prev, foldId, messageId, cur);
    });
  }, []);

  // Auto-scroll: only when user is already near the bottom, throttled by RAF
  const shouldAutoScrollRef = useRef(true);
  const scrollRafRef = useRef<number | null>(null);
  const jumpVisibilityRafRef = useRef<number | null>(null);
  const [showJumpToLatest, setShowJumpToLatest] = useState(false);
  const jumpToLatestAnimationTimerRef = useRef<number | null>(null);
  const jumpToLatestClickAnimatingRef = useRef(false);
  const [isJumpToLatestClickAnimating, setIsJumpToLatestClickAnimating] =
    useState(false);

  const clearJumpToLatestTimers = useCallback(() => {
    if (jumpToLatestAnimationTimerRef.current !== null) {
      window.clearTimeout(jumpToLatestAnimationTimerRef.current);
      jumpToLatestAnimationTimerRef.current = null;
    }
  }, []);

  const resetJumpToLatestClickAnimation = useCallback(() => {
    clearJumpToLatestTimers();
    jumpToLatestClickAnimatingRef.current = false;
    setIsJumpToLatestClickAnimating(false);
  }, [clearJumpToLatestTimers]);

  const updateJumpToLatestVisibility = useCallback(() => {
    const el = messagesScrollRef.current;
    if (!el) {
      setShowJumpToLatest(false);
      return true;
    }
    const metrics = {
      scrollTop: el.scrollTop,
      clientHeight: el.clientHeight,
      scrollHeight: el.scrollHeight,
    };
    const nearBottom = isNearScrollBottom(
      metrics,
      AUTO_SCROLL_BOTTOM_THRESHOLD_PX,
    );
    shouldAutoScrollRef.current = nearBottom;
    const shouldShow = shouldShowJumpToLatestButton(
      metrics,
      AUTO_SCROLL_BOTTOM_THRESHOLD_PX,
      messages.length > 0 || currentResponse.trim().length > 0,
    );
    if (jumpToLatestClickAnimatingRef.current && !shouldShow) {
      return nearBottom;
    }
    setShowJumpToLatest((prev) => (prev === shouldShow ? prev : shouldShow));
    return nearBottom;
  }, [currentResponse, messages.length]);

  const scheduleJumpToLatestVisibilityUpdate = useCallback(() => {
    if (jumpVisibilityRafRef.current !== null) return;
    jumpVisibilityRafRef.current = requestAnimationFrame(() => {
      jumpVisibilityRafRef.current = null;
      updateJumpToLatestVisibility();
    });
  }, [updateJumpToLatestVisibility]);

  // Scroll-to-top pagination + auto-scroll bottom detection
  useEffect(() => {
    const el = messagesScrollRef.current;
    if (!el) return;
    const onScroll = () => {
      if (el.scrollTop < 120 && hasMoreMessages && !isLoadingMoreMessages) {
        void loadMoreMessages();
      }
      updateJumpToLatestVisibility();
    };
    el.addEventListener("scroll", onScroll, { passive: true });
    return () => el.removeEventListener("scroll", onScroll);
  }, [
    hasMoreMessages,
    isLoadingMoreMessages,
    loadMoreMessages,
    updateJumpToLatestVisibility,
  ]);

  useEffect(() => {
    resetJumpToLatestClickAnimation();
    shouldAutoScrollRef.current = true;
    setShowJumpToLatest(false);
  }, [resetJumpToLatestClickAnimation, sessionId]);

  useEffect(() => {
    scheduleJumpToLatestVisibilityUpdate();

    if (typeof ResizeObserver === "undefined") return;
    const scrollEl = messagesScrollRef.current;
    if (!scrollEl) return;

    // Re-check when transcript layout changes without a scroll event, e.g.
    // collapsing a ReAct fold can make the transcript non-scrollable.
    const observer = new ResizeObserver(scheduleJumpToLatestVisibilityUpdate);
    observer.observe(scrollEl);

    const contentEl = messagesContentRef.current;
    if (contentEl) observer.observe(contentEl);

    return () => observer.disconnect();
  }, [
    allItemsVisible,
    isSwitchingSession,
    scheduleJumpToLatestVisibilityUpdate,
    sessionId,
  ]);

  const messageRenderItems = useMemo(
    () => groupMessagesForRender(messages),
    [messages],
  );
  const latestUserMessageId = useMemo(() => {
    for (let i = messages.length - 1; i >= 0; i--) {
      if (messages[i].role === "user") return messages[i].id;
    }
    return null;
  }, [messages]);
  const activeReactFoldId = useMemo(() => {
    if (!latestUserMessageId) return null;
    let afterLatestUser = false;
    let foldId: string | null = null;
    // Always computed from the FULL list so live streaming never attaches to a
    // stale ReAct fold from an earlier user turn after retry truncation.
    for (const item of messageRenderItems) {
      if (item.kind === "row" && item.message.role === "user") {
        afterLatestUser = item.message.id === latestUserMessageId;
        foldId = null;
        continue;
      }
      if (afterLatestUser && item.kind === "react_fold") {
        foldId = item.id;
      }
    }
    return foldId;
  }, [latestUserMessageId, messageRenderItems]);
  useEffect(() => {
    activeReactFoldIdRef.current = activeReactFoldId;
  }, [activeReactFoldId]);
  const currentResponseHasVisibleText = currentResponse.trim().length > 0;
  const liveReActFoldTraceText = selectLiveReActFoldTraceText({
    isStreaming,
    activeReactFoldId,
    currentResponse,
    currentFoldIntermediate,
  });
  const shouldRenderLiveTraceInFold = liveReActFoldTraceText.length > 0;

  useEffect(() => {
    if (!shouldRenderLiveTraceInFold || !activeReactFoldId) return;
    setExpandedToolGroups((prev) => {
      if (prev.has(activeReactFoldId)) return prev;
      const next = new Set(prev);
      next.add(activeReactFoldId);
      return next;
    });
  }, [activeReactFoldId, shouldRenderLiveTraceInFold]);

  // Phase-1: show only the most-recent items so the viewport is populated
  // on the very first paint. Phase-2 (allItemsVisible=true) adds all older
  // items without blocking the thread (see progressive rendering effects above).
  const displayedItems = allItemsVisible
    ? messageRenderItems
    : messageRenderItems.slice(-INSTANT_RENDER_COUNT);
  const visibleDisplayedItems = useMemo(
    () => displayedItems.filter(renderItemHasVisibleContent),
    [displayedItems],
  );

  useLayoutEffect(() => {
    const totalItems = messageRenderItems.length;
    if (!shouldLogProgressiveRenderPerf(totalItems, INSTANT_RENDER_COUNT)) {
      return;
    }

    if (!allItemsVisible) {
      const key = `${sessionId}:phase-1:${totalItems}:${displayedItems.length}`;
      if (progressivePhase1LoggedRef.current === key) return;
      progressivePhase1LoggedRef.current = key;
      requestAnimationFrame(() => {
        console.info(
          `%c${formatProgressiveRenderPerf({
            phase: "phase-1",
            sessionId,
            totalItems,
            renderedItems: displayedItems.length,
            instantRenderCount: INSTANT_RENDER_COUNT,
            durationMs: null,
          })}`,
          "color:#8b5cf6;font-weight:bold",
        );
      });
      return;
    }

    if (progressivePhase2StartRef.current == null) return;
    const key = `${sessionId}:phase-2:${totalItems}`;
    if (progressivePhase2LoggedRef.current === key) {
      progressivePhase2StartRef.current = null;
      return;
    }
    progressivePhase2LoggedRef.current = key;
    const durationMs = performance.now() - progressivePhase2StartRef.current;
    progressivePhase2StartRef.current = null;
    console.info(
      `%c${formatProgressiveRenderPerf({
        phase: "phase-2",
        sessionId,
        totalItems,
        renderedItems: displayedItems.length,
        instantRenderCount: INSTANT_RENDER_COUNT,
        durationMs,
        addedHeightPx: progressiveLastAddedHeightRef.current,
      })}`,
      "color:#8b5cf6;font-weight:bold",
    );
    progressiveLastAddedHeightRef.current = null;
  }, [allItemsVisible, displayedItems.length, messageRenderItems.length, sessionId]);

  const scheduleScrollToBottom = useCallback(() => {
    if (scrollRafRef.current !== null) return;
    scrollRafRef.current = requestAnimationFrame(() => {
      scrollRafRef.current = null;
      if (!shouldAutoScrollRef.current) {
        updateJumpToLatestVisibility();
        return;
      }
      messagesEndRef.current?.scrollIntoView({ behavior: "auto" });
      setShowJumpToLatest(false);
      requestAnimationFrame(updateJumpToLatestVisibility);
    });
  }, [updateJumpToLatestVisibility]);

  useEffect(() => {
    return () => {
      clearJumpToLatestTimers();
      if (scrollRafRef.current !== null) {
        cancelAnimationFrame(scrollRafRef.current);
        scrollRafRef.current = null;
      }
      if (jumpVisibilityRafRef.current !== null) {
        cancelAnimationFrame(jumpVisibilityRafRef.current);
        jumpVisibilityRafRef.current = null;
      }
    };
  }, [clearJumpToLatestTimers]);

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
      setShowJumpToLatest(false);
      messagesEndRef.current?.scrollIntoView({ behavior: "smooth" });
    }
  }, [scheduleCompleteSession, sessionId, setScheduleCompleteSession]);

  const handleJumpToLatest = useCallback(() => {
    const scrollToLatest = () => {
      messagesEndRef.current?.scrollIntoView({
        behavior: "auto",
        block: "end",
      });
    };

    const finishJumpToLatestClick = () => {
      jumpToLatestAnimationTimerRef.current = null;
      jumpToLatestClickAnimatingRef.current = false;
      flushSync(() => {
        setIsJumpToLatestClickAnimating(false);
        setShowJumpToLatest(false);
      });
      scrollToLatest();
      requestAnimationFrame(updateJumpToLatestVisibility);
    };

    shouldAutoScrollRef.current = true;
    clearJumpToLatestTimers();
    flushSync(() => {
      setShowJumpToLatest(true);
      setIsJumpToLatestClickAnimating(false);
    });

    if (window.matchMedia?.("(prefers-reduced-motion: reduce)").matches) {
      finishJumpToLatestClick();
      return;
    }

    requestAnimationFrame(() => {
      jumpToLatestClickAnimatingRef.current = true;
      setIsJumpToLatestClickAnimating(true);
    });

    jumpToLatestAnimationTimerRef.current = window.setTimeout(
      finishJumpToLatestClick,
      JUMP_TO_LATEST_CLICK_ANIMATION_MS,
    );
  }, [clearJumpToLatestTimers, updateJumpToLatestVisibility]);

  // Subscribe to the synthesis stream before the first chunk arrives so the
  // leader's reply streams inline instead of requiring a page refresh.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    listenTauriEvent<{ sessionId: string; messageId: string }>(
      "chat-synthesis-start",
      (event) => {
        if (event.payload.sessionId !== sessionId) return;
        const { messageId } = event.payload;
        setCurrentStreamId(messageId);
        currentStreamIdRef.current = messageId;
        setCurrentResponse("");
        currentResponseRef.current = "";
        setCurrentFoldIntermediate("");
        currentFoldIntermediateRef.current = "";
        activeReactFoldIdRef.current = null;
        pendingTextBufferRef.current = "";
        pendingFoldIntermediateBufferRef.current = "";
        pendingToolTraceTextRef.current = "";
        setIsStreaming(false);
        isStreamingRef.current = false;
        useActivityStore.getState().resetExecutionState();
        useActivityStore.getState().beginExecutionRun("等待综合结果");
        useActivityStore.getState().setConnecting(true);
        useActivityStore.getState().setStreaming(false, false);
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
  const showSuggestionsGeneratingPlaceholder =
    shouldShowPostTurnSuggestionsGeneratingPlaceholder({
      suggestionsGenerating,
      showNextStepSuggestions,
      currentStreamId,
      isConnecting,
      isStreaming,
      waitingFirstChunk,
      activityIsStreaming,
    });
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

  useEffect(() => {
    const projectRoot =
      currentSession?.workingDirectory ?? currentSession?.projectPath ?? "";
    if (
      !sessionId ||
      !projectRoot.trim() ||
      isSwitchingSession ||
      isStreaming ||
      isConnecting ||
      waitingFirstChunk ||
      pendingAskUser ||
      learningProposalPrompt
    ) {
      return;
    }

    let cancelled = false;
    const timer = window.setTimeout(() => {
      // Do not refresh/scan ExecutionRecords from UI idle state. Proposal
      // generation is an agent/user-triggered analysis step; the UI only
      // surfaces already-created pending prompts.
      void invoke<LearningProposalPrompt | null>("learning_proposal_next", {
        projectRoot,
        refresh: false,
      })
        .then((prompt) => {
          if (cancelled || !prompt) return;
          if (learningProposalSeenRef.current.has(prompt.proposalId)) return;
          learningProposalSeenRef.current.add(prompt.proposalId);
          setLearningProposalPrompt(prompt);
        })
        .catch((error) => {
          console.warn("[Chat] Failed to load learning proposal prompt:", error);
        });
    }, 900);

    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [
    currentSession?.projectPath,
    currentSession?.workingDirectory,
    isConnecting,
    isStreaming,
    isSwitchingSession,
    learningProposalPrompt,
    pendingAskUser,
    sessionId,
    waitingFirstChunk,
  ]);

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
        const current = messagesRef.current;
        // Skip if content is already the same (render-time reset already applied it)
        if (
          current.length === converted.length &&
          current[current.length - 1]?.id === converted[converted.length - 1]?.id
        ) {
          return;
        }
        messagesRef.current = converted;
        setMessages(converted);
      } else if (!sessionId || storeMessages.length === 0) {
        if (messagesRef.current.length === 0) return;
        messagesRef.current = [];
        setMessages([]);
      }
    } catch (e) {
      console.error(
        "[OmigaDebug][Chat] failed to sync messages from store",
        e,
        { sessionId, storeMessagesLength: storeMessages.length },
      );
      messagesRef.current = [];
      setMessages([]);
    }
  }, [sessionId, storeMessages]);

  // Cancel ALL per-session stream listeners when the component unmounts.
  useEffect(() => {
    return () => {
      cancelAllStreamListeners();
      unlistenRef.current = null;
    };
  }, []);

  // Background bash completion (detached `run_in_background` tasks)
  useEffect(() => {
    if (!sessionId) return;
    let cancelled = false;
    let unlistenBg: (() => void) | undefined;
    let unlistenActivity: (() => void) | undefined;
    (async () => {
      unlistenBg = await listenTauriEvent<BackgroundShellCompletePayload>(
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
      unlistenActivity = await listenTauriEvent<ActivityOperationPayload>(
        "omiga-activity-step",
        (event) => {
          if (cancelled) return;
          const payload = event.payload;
          if (!payload || payload.session_id !== sessionId) return;
          const activity = useActivityStore.getState();
          const detail = payload.detail?.trim();
          if (payload.status === "running") {
            activity.onOperationStart(payload.operation_id, payload.label, {
              summary: detail || payload.label,
            });
          } else {
            activity.onOperationDone(payload.operation_id, payload.label, {
              summary: detail || payload.label,
              output: detail,
              failed: payload.status === "error",
            });
          }
        },
      );
    })();
    return () => {
      cancelled = true;
      unlistenBg?.();
      unlistenActivity?.();
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

  type SnapshotExecutionStep = SessionStreamSnapshot["executionSteps"][number];

  const markSnapshotStepDone = (
    steps: SnapshotExecutionStep[],
    id: string,
    completedAt = Date.now(),
  ): SnapshotExecutionStep[] =>
    steps.map((step) =>
      step.id === id && step.status === "running"
        ? { ...step, status: "done" as const, completedAt }
        : step,
    );

  const ensureSnapshotStreamStarted = (
    snap: SessionStreamSnapshot,
  ): SessionStreamSnapshot => {
    const now = Date.now();
    const nextSteps = markSnapshotStepDone(snap.executionSteps, "connect", now);
    return {
      ...snap,
      isConnecting: false,
      isStreaming: true,
      waitingFirstChunk: true,
      executionSteps: nextSteps.some((step) => step.id === "think")
        ? nextSteps
        : [
            ...nextSteps,
            {
              id: "think",
              title: "推理中",
              status: "running" as const,
              startedAt: now,
            },
          ],
    };
  };

  const markSnapshotFirstOutput = (
    snap: SessionStreamSnapshot,
  ): SessionStreamSnapshot => {
    const now = Date.now();
    const running = snap.executionSteps.find((step) => step.status === "running");
    if (running?.id === "think") {
      return {
        ...snap,
        executionSteps: [
          ...markSnapshotStepDone(snap.executionSteps, "think", now),
          {
            id: "reply",
            title: "解析输出",
            status: "running" as const,
            startedAt: now,
          },
        ],
      };
    }
    if (!running) {
      return {
        ...snap,
        executionSteps: [
          ...snap.executionSteps,
          {
            id: `reply-${now}`,
            title: "解析输出",
            status: "running" as const,
            startedAt: now,
          },
        ],
      };
    }
    return snap;
  };

  const upsertSnapshotToolStep = (
    snap: SessionStreamSnapshot,
    toolUseId: string,
    title: string,
    detail: { summary?: string; input?: string; toolName?: string },
  ): SessionStreamSnapshot => {
    const now = Date.now();
    const stepId = `tool-${toolUseId}`;
    const baseSteps = snap.executionSteps.map((step) => {
      if (
        step.status === "running" &&
        (step.id === "think" || step.id === "reply" || step.id.startsWith("reply-"))
      ) {
        return { ...step, status: "done" as const, completedAt: now };
      }
      return step;
    });
    const existingStep = baseSteps.find((step) => step.id === stepId);
    const nextStep: SnapshotExecutionStep = {
      id: stepId,
      title,
      status: "running",
      startedAt: existingStep?.startedAt ?? now,
      completedAt: undefined,
      summary: detail.summary ?? title,
      ...(detail.input !== undefined ? { input: detail.input } : {}),
      ...(detail.toolName !== undefined ? { toolName: detail.toolName } : {}),
    };
    return {
      ...snap,
      executionSteps: baseSteps.some((step) => step.id === stepId)
        ? baseSteps.map((step) =>
            step.id === stepId ? { ...step, ...nextStep } : step,
          )
        : [...baseSteps, nextStep],
    };
  };

  const settleSnapshotToolStep = (
    snap: SessionStreamSnapshot,
    toolUseId: string,
    detail: { output?: string; failed?: boolean },
  ): SessionStreamSnapshot => {
    const now = Date.now();
    return {
      ...snap,
      executionSteps: markSnapshotStepDone(
        snap.executionSteps,
        `tool-${toolUseId}`,
        now,
      ).map((step) =>
        step.id === `tool-${toolUseId}`
          ? {
              ...step,
              ...(detail.output !== undefined ? { toolOutput: detail.output } : {}),
              ...(detail.failed !== undefined ? { failed: detail.failed } : {}),
            }
          : step,
      ),
    };
  };

  const getBackgroundStreamSnapshot = (
    ownerSessionId: string,
    streamId: string,
    roundId?: string | null,
  ): SessionStreamSnapshot => {
    const existing = loadStreamSnapshot(ownerSessionId);
    const reusable =
      existing && (existing.streamId === streamId || existing.streamId === null)
        ? existing
        : null;
    const now = Date.now();
    return {
      streamId,
      roundId: roundId ?? reusable?.roundId ?? null,
      response: reusable?.response ?? "",
      foldIntermediate: reusable?.foldIntermediate ?? "",
      pendingText: reusable?.pendingText ?? "",
      pendingFoldText: reusable?.pendingFoldText ?? "",
      isStreaming: reusable?.isStreaming ?? false,
      isConnecting: reusable?.isConnecting ?? true,
      waitingFirstChunk: reusable?.waitingFirstChunk ?? false,
      currentToolHint: reusable?.currentToolHint ?? null,
      executionSteps:
        reusable?.executionSteps.length
          ? [...reusable.executionSteps]
          : [
              {
                id: "connect",
                title: "等待响应",
                status: "running" as const,
                startedAt: now,
              },
            ],
      executionStartedAt: reusable?.executionStartedAt ?? now,
      executionEndedAt: reusable?.executionEndedAt ?? null,
      activeTodos: reusable?.activeTodos ?? null,
      backgroundJobs: reusable?.backgroundJobs ? [...reusable.backgroundJobs] : [],
    };
  };

  const updateBackgroundStreamSnapshot = (
    ownerSessionId: string,
    streamId: string,
    payload: StreamOutputItem,
    roundId?: string | null,
  ) => {
    if (
      payload.type === "complete" ||
      payload.type === "error" ||
      payload.type === "cancelled" ||
      payload.type === "turn_summary" ||
      payload.type === "follow_up_suggestions" ||
      payload.type === "suggestions_complete"
    ) {
      clearStreamSnapshot(ownerSessionId);
      return;
    }

    let snap = getBackgroundStreamSnapshot(ownerSessionId, streamId, roundId);

    switch (payload.type) {
      case "Start": {
        snap = ensureSnapshotStreamStarted({
          ...snap,
          response: "",
          foldIntermediate: "",
          pendingText: "",
          pendingFoldText: "",
        });
        break;
      }
      case "text": {
        const text = typeof payload.data === "string" ? payload.data : "";
        if (text) {
          snap = ensureSnapshotStreamStarted(snap);
          snap = markSnapshotFirstOutput({
            ...snap,
            response: snap.response + snap.pendingText + text,
            pendingText: "",
            isConnecting: false,
            isStreaming: true,
            waitingFirstChunk: false,
          });
        }
        break;
      }
      case "thinking": {
        const text = typeof payload.data === "string" ? payload.data : "";
        if (text) {
          snap = ensureSnapshotStreamStarted(snap);
          snap = {
            ...snap,
            foldIntermediate: snap.foldIntermediate + snap.pendingFoldText + text,
            pendingFoldText: "",
            isConnecting: false,
            isStreaming: true,
            waitingFirstChunk: false,
          };
        }
        break;
      }
      case "tool_use": {
        const toolData = payload.data as
          | { id?: string; name?: string; arguments?: string }
          | undefined;
        if (!isEmptyToolUsePayload(toolData)) {
          snap = ensureSnapshotStreamStarted(snap);
          const toolName = toolData?.name ?? "tool";
          const toolUseId = (toolData?.id ?? "").trim();
          snap = {
            ...snap,
            response: snap.response + snap.pendingText,
            foldIntermediate: snap.foldIntermediate + snap.pendingFoldText,
            pendingText: "",
            pendingFoldText: "",
            isConnecting: false,
            isStreaming: true,
            waitingFirstChunk: false,
            currentToolHint: toolName.slice(0, 96),
          };
          if (toolUseId) {
            snap = upsertSnapshotToolStep(
              snap,
              toolUseId,
              humanizeToolStepTitle(toolName, toolData?.arguments),
              {
                summary: executionStepSummary(toolName, toolData?.arguments),
                input: toolData?.arguments,
                toolName,
              },
            );
          }
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
        const toolUseId = resultData?.tool_use_id?.trim();
        snap = ensureSnapshotStreamStarted(snap);
        snap = {
          ...snap,
          response: snap.response + snap.pendingText,
          foldIntermediate: snap.foldIntermediate + snap.pendingFoldText,
          pendingText: "",
          pendingFoldText: "",
          isConnecting: false,
          isStreaming: true,
          waitingFirstChunk: false,
        };
        if (toolUseId) {
          snap = settleSnapshotToolStep(snap, toolUseId, {
            output: resultData?.output,
            failed: Boolean(resultData?.is_error),
          });
        }
        if (resultData?.name === "todo_write" && resultData.input) {
          try {
            const parsed = JSON.parse(resultData.input) as {
              todos?: Array<{
                id?: string;
                content: string;
                activeForm?: string;
                active_form?: string;
                status: string;
              }>;
            };
            if (Array.isArray(parsed.todos)) {
              const nextTodos = parsed.todos.map((todo, index) => ({
                id: todo.id ?? `todo-${index}`,
                content: todo.content,
                activeForm: todo.activeForm ?? todo.active_form ?? todo.content,
                status: String(todo.status),
              }));
              snap = {
                ...snap,
                activeTodos: mergeActiveTodosWithTiming(nextTodos, snap.activeTodos),
              };
            }
          } catch {
            // Ignore malformed todo payloads in background snapshots.
          }
        }
        if (resultData?.name === "bash" && resultData.tool_use_id) {
          const bg = tryParseBashBackground(resultData.input);
          if (bg) {
            const job = {
              id: resultData.tool_use_id,
              toolUseId: resultData.tool_use_id,
              label: bg.label,
              state: "running" as const,
            };
            const idx = snap.backgroundJobs.findIndex((j) => j.id === job.id);
            snap = {
              ...snap,
              backgroundJobs:
                idx >= 0
                  ? snap.backgroundJobs.map((j, i) =>
                      i === idx ? { ...j, ...job } : j,
                    )
                  : [...snap.backgroundJobs, job],
            };
          }
        }
        break;
      }
      case "ask_user_pending": {
        snap = {
          ...snap,
          isConnecting: false,
          isStreaming: true,
          waitingFirstChunk: false,
          currentToolHint: "ask_user_question",
        };
        break;
      }
    }

    saveStreamSnapshot(ownerSessionId, snap);
  };

  // Set up stream listener for a specific stream ID
  const setupStreamListener = async (
    streamId: string,
    ownerSessionIdOverride?: string | null,
    ownerRoundId?: string | null,
  ) => {
    const ownerSessionId = ownerSessionIdOverride ?? sessionIdRef.current;
    // Cancel only the PREVIOUS listener for THIS session (e.g., user sent a
    // second message before the first finished).  Other sessions' listeners
    // are intentionally left alive so parallel tasks are not interrupted.
    if (ownerSessionId) {
      cancelStreamListener(ownerSessionId);
    }

    if (ownerSessionId && sessionIdRef.current !== ownerSessionId) {
      saveStreamSnapshot(
        ownerSessionId,
        getBackgroundStreamSnapshot(ownerSessionId, streamId, ownerRoundId),
      );
    } else {
      // Flush any buffered text before swapping listeners for the visible session.
      if (textFlushRafRef.current !== null) {
        cancelAnimationFrame(textFlushRafRef.current);
        textFlushRafRef.current = null;
      }
      if (foldIntermediateFlushRafRef.current !== null) {
        cancelAnimationFrame(foldIntermediateFlushRafRef.current);
        foldIntermediateFlushRafRef.current = null;
      }
      const buffered = pendingTextBufferRef.current;
      if (buffered) {
        pendingTextBufferRef.current = "";
        const next = currentResponseRef.current + buffered;
        currentResponseRef.current = next;
        setCurrentResponse(next);
      }
      const bufferedFoldIntermediate = pendingFoldIntermediateBufferRef.current;
      if (bufferedFoldIntermediate) {
        pendingFoldIntermediateBufferRef.current = "";
        const next = currentFoldIntermediateRef.current + bufferedFoldIntermediate;
        currentFoldIntermediateRef.current = next;
        setCurrentFoldIntermediate(next);
      }
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
      const next = currentResponseRef.current + text;
      currentResponseRef.current = next;
      setCurrentResponse(next);
    };

    const flushPendingFoldIntermediate = () => {
      if (foldIntermediateFlushRafRef.current !== null) {
        cancelAnimationFrame(foldIntermediateFlushRafRef.current);
        foldIntermediateFlushRafRef.current = null;
      }
      const text = pendingFoldIntermediateBufferRef.current;
      if (!text) return;
      pendingFoldIntermediateBufferRef.current = "";
      const next = currentFoldIntermediateRef.current + text;
      currentFoldIntermediateRef.current = next;
      setCurrentFoldIntermediate(next);
    };

    const scheduleFlush = () => {
      if (textFlushRafRef.current !== null) return;
      textFlushRafRef.current = requestAnimationFrame(() => {
        flushPendingText();
      });
    };

    const scheduleFoldIntermediateFlush = () => {
      if (foldIntermediateFlushRafRef.current !== null) return;
      foldIntermediateFlushRafRef.current = requestAnimationFrame(() => {
        flushPendingFoldIntermediate();
      });
    };

    const unlisten = await listenTauriEvent<StreamOutputItem>(eventName, (event) => {
      if (sessionIdRef.current !== ownerSessionId) {
        // Background session: update its per-session snapshot instead of
        // touching the visible React state for the currently selected session.
        if (ownerSessionId) {
          updateBackgroundStreamSnapshot(
            ownerSessionId,
            streamId,
            event.payload,
            ownerRoundId,
          );
        }
        return;
      }
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

        clearPostTurnMetaState();
        pendingTurnSummaryRef.current = null;
        pendingTokenUsageRef.current = null;
        isStreamingRef.current = true;
        setIsStreaming(true);
        act.setConnecting(false);
        act.setStreaming(true, true);
        segmentStartRef.current = true;
        act.onStreamStart();
        if (clearAssistantDraft) {
          clearRunningToolTracking();
          pendingTextBufferRef.current = "";
          pendingFoldIntermediateBufferRef.current = "";
          pendingToolTraceTextRef.current = "";
          if (textFlushRafRef.current !== null) {
            cancelAnimationFrame(textFlushRafRef.current);
            textFlushRafRef.current = null;
          }
          if (foldIntermediateFlushRafRef.current !== null) {
            cancelAnimationFrame(foldIntermediateFlushRafRef.current);
            foldIntermediateFlushRafRef.current = null;
          }
          setCurrentResponse("");
          currentResponseRef.current = "";
          setCurrentFoldIntermediate("");
          currentFoldIntermediateRef.current = "";
        }
      };

      const isEventForReplacedStream = () =>
        currentStreamIdRef.current !== null &&
        currentStreamIdRef.current !== streamId;

      const hasUnflushedAssistantDraft = () =>
        currentResponseRef.current.length > 0 ||
        pendingTextBufferRef.current.length > 0 ||
        currentFoldIntermediateRef.current.trim().length > 0 ||
        pendingFoldIntermediateBufferRef.current.trim().length > 0;

      /**
       * Complete must be idempotent and stream-scoped. Synthesis/post-turn
       * events can outlive the main turn; if the first `complete` races ahead
       * of listener registration, the next post-turn marker still settles the
       * UI instead of leaving the previous tool (often `bash`) stuck running.
       */
      const finalizeSuccessfulStream = (source: "complete" | "post-turn-meta") => {
        if (isEventForReplacedStream()) return false;

        const act = useActivityStore.getState();
        const hasActiveStreamState =
          currentStreamIdRef.current === streamId ||
          isStreamingRef.current ||
          act.isConnecting ||
          act.isStreaming;
        if (!hasActiveStreamState && !hasUnflushedAssistantDraft()) {
          return false;
        }

        clearCancelFallbackTimer();
        clearAllToolWatchdogs();
        flushPendingText();
        flushPendingFoldIntermediate();
        setAwaitingResumeAfterCancel(false);
        isStreamingRef.current = false;
        setIsStreaming(false);
        setCurrentStreamId(null);
        currentStreamIdRef.current = null;
        setCurrentRoundId(null);
        retrySendInFlightRef.current = false;
        act.finalizeExecutionRun();
        act.clearTransient();
        const roundId = currentRoundIdRef.current;
        if (roundId) {
          updateRoundStatus(roundId, "completed");
        }
        currentRoundIdRef.current = null;

        const finalResponse = currentResponseRef.current;
        const finalFoldIntermediate = currentFoldIntermediateRef.current.trim();
        const followUps = pendingFollowUpSuggestionsRef.current;
        pendingFollowUpSuggestionsRef.current = null;
        const turnSum = pendingTurnSummaryRef.current;
        pendingTurnSummaryRef.current = null;
        const tok = pendingTokenUsageRef.current;
        pendingTokenUsageRef.current = null;
        setMessages((prev) => {
          let next = settleRunningToolCalls(prev);
          if (finalFoldIntermediate && activeReactFoldIdRef.current) {
            next = [
              ...next,
              {
                id: `assistant-intermediate-${Date.now()}`,
                role: "assistant",
                content: finalFoldIntermediate,
                intermediate: true,
                timestamp: Date.now(),
              },
            ];
          }
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
            next = [...next, assistantMsg];
          }
          replaceStoreMessagesSnapshot(next.map(chatMessageToStore));
          messagesRef.current = next;
          return next;
        });
        setCurrentResponse("");
        currentResponseRef.current = "";
        setCurrentFoldIntermediate("");
        currentFoldIntermediateRef.current = "";
        pendingTextBufferRef.current = "";
        pendingFoldIntermediateBufferRef.current = "";
        pendingToolTraceTextRef.current = "";
        clearRunningToolTracking();
        activeReactFoldIdRef.current = null;

        if (ownerSessionId) {
          clearStreamSnapshot(ownerSessionId);
        }
        postTurnMetaStreamIdRef.current = streamId;
        postTurnSuggestionStreamIdRef.current = streamId;

        if (isDev) {
          console.debug("[OmigaDev][AgentComplete]", {
            streamId,
            source,
            final: finalResponse,
          });
        }
        flushQueuedMainSendIfAnyRef.current();
        return true;
      };

      const finalizeIfPostTurnMetaOutranComplete = (
        options: { terminalMeta?: boolean } = {},
      ) => {
        const streamLooksActive =
          currentStreamIdRef.current === streamId ||
          (currentStreamIdRef.current === null &&
            isStreamingRef.current &&
            postTurnMetaStreamIdRef.current !== streamId);
        if (!streamLooksActive) return false;
        // suggestions_complete is emitted only after the visible answer and
        // post-turn metadata finish. If a stale tool row still looks "running",
        // settle it instead of leaving the UI stuck at “解析输出”.
        if (!options.terminalMeta && activeRunningToolIdsRef.current.size > 0) {
          return false;
        }
        return finalizeSuccessfulStream("post-turn-meta");
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
            pendingToolTraceTextRef.current += text;
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
          if (piece) {
            pendingFoldIntermediateBufferRef.current += piece;
            scheduleFoldIntermediateFlush();
            useActivityStore.getState().setStreaming(true, false);
          }
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
          flushPendingFoldIntermediate();
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
          markToolRunning(tuId);
          const newToolId = `tool-${Date.now()}`;
          activeReactFoldIdRef.current =
            activeReactFoldIdRef.current ?? `rf-${newToolId}`;
          const visibleChunk = toolTracePrefaceFromText(
            currentResponseRef.current,
          );
          const foldIntermediateChunk = currentFoldIntermediateRef.current.trim();
          const tracePreface = toolTracePrefaceFromText(
            pendingToolTraceTextRef.current,
          );
          const intermediateChunk = [
            foldIntermediateChunk,
            visibleChunk || tracePreface,
          ]
            .filter(Boolean)
            .join("\n\n");

          const prevMessages = messagesRef.current;
          const existingToolUse =
            Boolean(tuId) &&
            prevMessages.some(
              (m) => m.role === "tool" && m.toolCall?.id === tuId,
            );

          if (intermediateChunk || !existingToolUse) {
            // Clear exactly once after the snapshot is computed. Keeping this
            // outside a functional setState updater is intentional: React may
            // replay updaters in StrictMode/concurrent rendering, and replaying
            // this cleanup was dropping the pre-tool thought on the second pass.
            pendingToolTraceTextRef.current = "";
            currentResponseRef.current = "";
            currentFoldIntermediateRef.current = "";
            queueMicrotask(() => setCurrentResponse(""));
            queueMicrotask(() => setCurrentFoldIntermediate(""));
          }

          const nextMessages = upsertToolUseMessage(prevMessages, {
            toolData,
            newToolId,
            prefaceBeforeTools: intermediateChunk,
            timestamp: Date.now(),
          });
          if (nextMessages !== prevMessages) {
            commitMessagesSnapshot(nextMessages);
          }
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
            startToolWatchdog(tuId, tn);
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
          flushPendingText();
          flushPendingFoldIntermediate();
          const resultPreface = [
            currentFoldIntermediateRef.current.trim(),
            toolTracePrefaceFromText(currentResponseRef.current),
          ]
            .filter(Boolean)
            .join("\n\n");
          if (resultPreface) {
            currentResponseRef.current = "";
            currentFoldIntermediateRef.current = "";
            pendingToolTraceTextRef.current = "";
            queueMicrotask(() => setCurrentResponse(""));
            queueMicrotask(() => setCurrentFoldIntermediate(""));
          }
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
          const prevMessages = messagesRef.current;
          const nextMessages = applyToolResultMessage(prevMessages, {
            resultData,
            matchId: toolResultMatchId,
            completedAt: Date.now(),
            prefaceBeforeTools: resultPreface,
          });
          if (nextMessages !== prevMessages) {
            commitMessagesSnapshot(nextMessages);
          }
          if (toolResultMatchId) {
            markToolSettled(toolResultMatchId);
            clearToolWatchdog(toolResultMatchId);
            useActivityStore.getState().onToolResultDone(toolResultMatchId, {
              output: resultData?.output,
              failed: Boolean(resultData?.is_error),
            });
            setPendingAskUser((p) =>
              p?.toolUseId === toolResultMatchId ? null : p,
            );
          }
          const learningPrompt =
            learningProposalPromptFromToolResult(resultData);
          if (
            learningPrompt &&
            !learningProposalSeenRef.current.has(learningPrompt.proposalId)
          ) {
            learningProposalSeenRef.current.add(learningPrompt.proposalId);
            setLearningProposalToast(null);
            setLearningProposalPrompt(learningPrompt);
          } else if (learningPrompt) {
            setLearningProposalToast(null);
          } else {
            const learningNotice =
              learningProposalToastFromToolResult(resultData);
            if (learningNotice) {
              setLearningProposalToast(learningNotice);
            }
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
          clearCancelFallbackTimer();
          clearAllToolWatchdogs();
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
          clearPostTurnMetaState();
          useActivityStore.getState().finalizeExecutionRun();
          useActivityStore.getState().clearTransient();
          setCurrentResponse("");
          currentResponseRef.current = "";
          setCurrentFoldIntermediate("");
          currentFoldIntermediateRef.current = "";
          pendingToolTraceTextRef.current = "";
          clearRunningToolTracking();
          activeReactFoldIdRef.current = null;
          if (ownerSessionId) clearStreamSnapshot(ownerSessionId);
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
          clearCancelFallbackTimer();
          clearAllToolWatchdogs();
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
          clearPostTurnMetaState();
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
          setCurrentFoldIntermediate("");
          currentFoldIntermediateRef.current = "";
          pendingToolTraceTextRef.current = "";
          clearRunningToolTracking();
          activeReactFoldIdRef.current = null;
          if (ownerSessionId) clearStreamSnapshot(ownerSessionId);
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
          if (isEventForReplacedStream()) break;
          const isLivePostTurnStream = currentStreamIdRef.current === streamId;
          const isFinalizedPostTurnStream =
            postTurnMetaStreamIdRef.current === streamId;
          if (!isLivePostTurnStream && !isFinalizedPostTurnStream) break;
          const raw = payload.data;
          let text: string | null = null;
          if (raw != null && typeof raw === "object" && "text" in raw) {
            const v = (raw as { text?: unknown }).text;
            if (typeof v === "string" && v.trim().length > 0) {
              text = v.trim();
            }
          }
          if (
            activeRunningToolIdsRef.current.size === 0 &&
            isLivePostTurnStream
          ) {
            pendingTurnSummaryRef.current = text;
            finalizeSuccessfulStream("post-turn-meta");
            break;
          }
          if (isFinalizedPostTurnStream) {
            if (text) {
              setMessages((prev) => {
                const lastIdx = prev.length - 1;
                if (lastIdx < 0 || prev[lastIdx].role !== "assistant") return prev;
                const next = [
                  ...prev.slice(0, lastIdx),
                  { ...prev[lastIdx], turnSummary: text },
                ];
                messagesRef.current = next;
                return next;
              });
            }
          }
          break;
        }
        case "follow_up_suggestions": {
          if (isEventForReplacedStream()) break;
          const isLivePostTurnStream = currentStreamIdRef.current === streamId;
          const isFinalizedPostTurnStream =
            postTurnMetaStreamIdRef.current === streamId;
          if (!isLivePostTurnStream && !isFinalizedPostTurnStream) break;
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

          if (
            activeRunningToolIdsRef.current.size === 0 &&
            isLivePostTurnStream
          ) {
            pendingFollowUpSuggestionsRef.current = parsed;
            setSuggestionsGenerating(false);
            finalizeSuccessfulStream("post-turn-meta");
            break;
          }

          // If complete already fired (isStreamingRef is false), patch the last assistant message directly.
          // Do NOT call replaceStoreMessagesSnapshot here — the user may have already sent a new
          // message, and overwriting the store snapshot with fewer messages would cause the
          // storeMessages useEffect to wipe out that new user message.
          // The suggestions are already persisted to the DB by the Rust backend.
          if (isFinalizedPostTurnStream) {
            clearPostTurnSuggestionsIndicator();
            if (parsed.length > 0) {
              setMessages((prev) => {
                const lastIdx = prev.length - 1;
                if (lastIdx < 0 || prev[lastIdx].role !== "assistant") return prev;
                const updated = {
                  ...prev[lastIdx],
                  followUpSuggestions: parsed,
                };
                const next = [...prev.slice(0, lastIdx), updated];
                messagesRef.current = next;
                return next;
              });
            }
          }
          break;
        }
        case "suggestions_generating": {
          if (isEventForReplacedStream()) break;
          finalizeIfPostTurnMetaOutranComplete();
          const act = useActivityStore.getState();
          if (
            shouldStartPostTurnSuggestionsIndicator({
              activePostTurnStreamId: postTurnSuggestionStreamIdRef.current,
              eventStreamId: streamId,
              currentStreamId: currentStreamIdRef.current,
              isConnecting: act.isConnecting,
              isStreaming: isStreamingRef.current,
              waitingFirstChunk: act.waitingFirstChunk,
              activityIsStreaming: act.isStreaming,
              queuedMainSendCount: queuedMainSendQueueRef.current.length,
              flushingQueuedMainSend: mainQueueFlushPayloadRef.current !== null,
            })
          ) {
            setSuggestionsGenerating(true);
          }
          break;
        }
        case "suggestions_complete": {
          if (isEventForReplacedStream()) break;
          finalizeIfPostTurnMetaOutranComplete({ terminalMeta: true });
          if (postTurnSuggestionStreamIdRef.current === streamId) {
            clearPostTurnSuggestionsIndicator();
          }
          break;
        }
        case "token_usage": {
          if (isEventForReplacedStream()) break;
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
          if (
            activeRunningToolIdsRef.current.size === 0 &&
            (currentStreamIdRef.current === streamId ||
              isStreamingRef.current ||
              useActivityStore.getState().isConnecting ||
              useActivityStore.getState().isStreaming)
          ) {
            finalizeSuccessfulStream("post-turn-meta");
          }
          break;
        }
        case "complete": {
          finalizeSuccessfulStream("complete");
          break;
        }
      }
    });

    // Register in the per-session map so session switches don't cancel it,
    // and so it is properly cleaned up when THIS session starts a new stream
    // or the component unmounts.
    if (ownerSessionId) {
      registerStreamListener(ownerSessionId, unlisten);
    }
    if (sessionIdRef.current === ownerSessionId) {
      unlistenRef.current = unlisten;
    }
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
      composerSelectedPluginIds: storePluginIds,
      environment: storeEnv,
      sshServer: storeSsh,
      sandboxBackend: storeSb,
      localVenvType: storeVenvType,
      localVenvName: storeVenvName,
      computerUseMode: storeComputerUseMode,
    } = useChatComposerStore.getState();

    const composerAgentType = flushPayload
      ? flushPayload.composerAgentType
      : storeAgent;
    const permissionMode = flushPayload
      ? flushPayload.permissionMode
      : storePerm;
    const computerUseMode = flushPayload
      ? flushPayload.computerUseMode
      : storeComputerUseMode;
    const environment = flushPayload ? flushPayload.environment : storeEnv;
    const sshServer = flushPayload ? flushPayload.sshServer : storeSsh;
    const sandboxBackend = flushPayload ? flushPayload.sandboxBackend : storeSb;
    const localVenvType = storeVenvType;
    const localVenvName = storeVenvName;
    const composerAttachedPaths = flushPayload
      ? [...flushPayload.composerAttachedPaths]
      : storePaths;
    const composerSelectedPluginIds = flushPayload
      ? [...flushPayload.composerSelectedPluginIds]
      : storePluginIds;

    /** Prefer ref payload after queue flush — `getValue` reads the latest composer state. */
    const trimmed = flushPayload ? flushPayload.body.trim() : (composerRef.current?.getValue() ?? "").trim();
    const researchParsed = parseResearchCommand(trimmed);
    const goalParsed = parseGoalCommand(trimmed);
    const skillParsed = parseSkillCommand(trimmed);

    if (!trimmed && composerAttachedPaths.length === 0) {
      if (flushPayload) restoreFlushToQueue(flushPayload);
      return;
    }
    /** Composer is still in bare `/…`, `$…`, `@…`, or `#…` picker mode — do not send as message */
    if (trimmed && /^\/[^\s]*$/u.test(trimmed) && !researchParsed && !goalParsed) {
      if (flushPayload) restoreFlushToQueue(flushPayload);
      return;
    }
    if (trimmed && /^\$[^\s]*$/u.test(trimmed) && !skillParsed) {
      if (flushPayload) restoreFlushToQueue(flushPayload);
      return;
    }
    if (trimmed && /^@[^\s]*$/u.test(trimmed)) {
      if (flushPayload) restoreFlushToQueue(flushPayload);
      return;
    }
    if (trimmed && /^#[^\s]*$/u.test(trimmed)) {
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
      useChatComposerStore.getState().clearComposerSelectedPluginIds();
      useChatComposerStore.getState().resetTaskComputerUseMode();
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
            selectedPluginIds: composerSelectedPluginIds,
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
        composerSelectedPluginIds: [...composerSelectedPluginIds],
        composerAgentType,
        permissionMode,
        computerUseMode,
        environment,
        sshServer: useChatComposerStore.getState().sshServer,
        sandboxBackend: useChatComposerStore.getState().sandboxBackend,
      });
      bumpQueueUi();
      composerRef.current?.setValue("");
      useChatComposerStore.getState().clearComposerAttachedPaths();
      useChatComposerStore.getState().clearComposerSelectedPluginIds();
      useChatComposerStore.getState().resetTaskComputerUseMode();
      return;
    }

    clearPostTurnMetaState();

    if (researchParsed || goalParsed) {
      const specialCommand = researchParsed
        ? {
            id: "research" as const,
            body: researchParsed.body,
            label: "Research System",
            invokeName: "run_research_command",
            helpSummary: "显示 Research System 帮助",
            errorPrefix: "Failed to execute /research",
          }
        : {
            id: "goal" as const,
            body: goalParsed?.body ?? "",
            label: "Research Goal",
            invokeName: "run_research_goal_command",
            helpSummary: "显示科研目标状态",
            errorPrefix: "Failed to execute /goal",
          };
      const operationId =
        typeof crypto !== "undefined" &&
        typeof crypto.randomUUID === "function"
          ? crypto.randomUUID()
          : `${specialCommand.id}-${Date.now()}`;
      const pendingFeedback = buildPendingExecutionFeedback({
        workflowCommand: specialCommand.id,
        composerAgentType,
      });

      setIndexingStatus("idle");
      setPendingAssistantHint(pendingFeedback.assistantHint);
      setCurrentResponse("");
      currentResponseRef.current = "";
      setCurrentFoldIntermediate("");
      currentFoldIntermediateRef.current = "";
      pendingTextBufferRef.current = "";
      pendingFoldIntermediateBufferRef.current = "";
      pendingToolTraceTextRef.current = "";
      activeReactFoldIdRef.current = null;

      useActivityStore.getState().beginExecutionRun(pendingFeedback.connectLabel);
      useActivityStore.getState().setConnecting(true);
      useActivityStore.getState().setStreaming(false, false);
      useActivityStore.getState().clearActiveTodos();
      useActivityStore.getState().onOperationStart(
        operationId,
        specialCommand.label,
        {
          summary: specialCommand.body || specialCommand.helpSummary,
        },
      );

      const isFirstMessageInSession = storeMessages.length === 0;
      const messageContent = mergeComposerPathsAndBody(
        composerAttachedPaths,
        trimmed,
      );
      const workflowTitleSeed =
        specialCommand.body ||
        trimmed.replace(new RegExp(`^/${specialCommand.id}\\s*`, "iu"), "").trim() ||
        trimmed;
      const bubbleAttachedPaths =
        composerAttachedPaths.length > 0 ? [...composerAttachedPaths] : undefined;
      const bubbleSelectedPluginIds =
        composerSelectedPluginIds.length > 0
          ? [...composerSelectedPluginIds]
          : undefined;
      const userMessage: Message = {
        id: `user-${Date.now()}`,
        role: "user",
        content: messageContent,
        timestamp: Date.now(),
        composerAgentType: undefined,
        composerAttachedPaths: bubbleAttachedPaths,
        composerSelectedPluginIds: bubbleSelectedPluginIds,
      };

      setMessages((prev) => [...prev, userMessage]);
      addMessage({
        role: "user",
        content: messageContent,
        composerAgentType: undefined,
        composerAttachedPaths: bubbleAttachedPaths,
        composerSelectedPluginIds: bubbleSelectedPluginIds,
        id: userMessage.id,
        timestamp: userMessage.timestamp,
      });

      composerRef.current?.setValue("");
      useChatComposerStore.getState().clearComposerAttachedPaths();
      useChatComposerStore.getState().clearComposerSelectedPluginIds();
      useChatComposerStore.getState().resetTaskComputerUseMode();

      const requestSessionId = sessionId;
      try {
        const requestSessionId = sessionId;
        if (
          isFirstMessageInSession &&
          isPlaceholderSessionTitle(currentSession?.name)
        ) {
          const heuristicTitle = titleFromFirstUserMessage(workflowTitleSeed);
          await renameSession(sessionId, heuristicTitle);
        }

        const response = await invoke<ResearchCommandResponse>(
          specialCommand.invokeName,
          {
            request: {
              sessionId: sessionId,
              projectPath:
                currentSession?.workingDirectory ??
                currentSession?.projectPath ??
                ".",
              content: messageContent,
              body: specialCommand.body,
            },
          },
        );

        if (sessionIdRef.current !== requestSessionId) {
          return;
        }
        if (specialCommand.id === "goal") {
          setActiveResearchGoal(response.goal ?? null);
        }

        const persistedUserMessage = {
          ...userMessage,
          id: response.userMessageId,
        };
        const assistantMessage = {
          id: response.assistantMessageId,
          role: "assistant" as const,
          content: response.assistantContent,
          timestamp: Date.now(),
          roundId: response.roundId,
        };
        let nextMessages: Message[] = [];
        setMessages((prev) => {
          nextMessages = finalizeResearchCommandMessages(
            prev,
            userMessage.id,
            persistedUserMessage,
            assistantMessage,
          );
          return nextMessages;
        });
        replaceStoreMessagesSnapshot(nextMessages.map(chatMessageToStore));
        useActivityStore.getState().onOperationDone(
          operationId,
          specialCommand.label,
          {
            summary: `/${specialCommand.id} ${specialCommand.body || "help"}`,
            output: response.assistantContent,
          },
        );
        useActivityStore.getState().finalizeExecutionRun();
        useChatComposerStore.getState().setComposerAgentType("general-purpose");
      } catch (error: unknown) {
        if (sessionIdRef.current !== requestSessionId) {
          return;
        }
        console.error(`${specialCommand.errorPrefix} command:`, error);

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
            }
          } else if (typeof err.message === "string") {
            errorMessage = err.message;
          } else {
            errorMessage = JSON.stringify(error);
          }
        }

        const errorMsg: Message = {
          id: `error-${Date.now()}`,
          role: "assistant",
          content: `${specialCommand.errorPrefix}: ${errorMessage}`,
          timestamp: Date.now(),
        };
        setMessages((prev) => [...prev, errorMsg]);
        replaceStoreMessagesSnapshot(
          [...useSessionStore.getState().storeMessages, errorMsg].map(
            chatMessageToStore,
          ),
        );
        useActivityStore.getState().onOperationDone(
          operationId,
          specialCommand.label,
          {
            summary: `/${specialCommand.id} ${specialCommand.body || "help"}`,
            output: errorMessage,
            failed: true,
          },
        );
        useActivityStore.getState().finalizeExecutionRun();
      } finally {
        useActivityStore.getState().clearTransient();
        setCurrentStreamId(null);
        setCurrentRoundId(null);
        queueMicrotask(() => {
          isStreamingRef.current = false;
          setIsStreaming(false);
          setPendingAskUser(null);
          setAskUserSelections({});
          pendingTokenUsageRef.current = null;
          flushQueuedMainSendIfAnyRef.current();
        });
      }
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
    setCurrentResponse("");
    currentResponseRef.current = "";
    setCurrentFoldIntermediate("");
    currentFoldIntermediateRef.current = "";
    pendingTextBufferRef.current = "";
    pendingFoldIntermediateBufferRef.current = "";
    pendingToolTraceTextRef.current = "";
    clearRunningToolTracking();
    activeReactFoldIdRef.current = null;

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
      parseWorkflowCommand(trimmed)?.body || skillParsed?.args || trimmed;
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
    const bubbleSelectedPluginIds =
      composerSelectedPluginIds.length > 0
        ? [...composerSelectedPluginIds]
        : undefined;

    const userMessage: Message = {
      id: `user-${Date.now()}`,
      role: "user",
      content: messageContent,
      timestamp: Date.now(),
      composerAgentType: bubbleComposerAgent,
      composerAttachedPaths: bubbleAttachedPaths,
      composerSelectedPluginIds: bubbleSelectedPluginIds,
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
      composerSelectedPluginIds: bubbleSelectedPluginIds,
      id: userMessage.id,
      timestamp: userMessage.timestamp,
    });

    composerRef.current?.setValue("");
    useChatComposerStore.getState().clearComposerAttachedPaths();
    useChatComposerStore.getState().clearComposerSelectedPluginIds();
    useChatComposerStore.getState().resetTaskComputerUseMode();

    const requestSessionId = sessionId;
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
          selectedPluginIds: composerSelectedPluginIds,
          computerUseMode,
        },
      });

      if (sessionIdRef.current === requestSessionId) {
        setCurrentRoundId(response.round_id);
        currentRoundIdRef.current = response.round_id;
        setCurrentStreamId(response.message_id);
        currentStreamIdRef.current = response.message_id;
      }

      await setupStreamListener(
        response.message_id,
        requestSessionId,
        response.round_id,
      );

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
        if (sessionIdRef.current === requestSessionId) {
          useActivityStore.getState().clearTransient();
          useActivityStore.getState().resetExecutionState();
          setCurrentStreamId(null);
          setCurrentRoundId(null);
        } else {
          clearStreamSnapshot(requestSessionId);
        }
        return;
      }

      if (sessionIdRef.current !== requestSessionId) {
        return;
      }

      useChatComposerStore.getState().setComposerAgentType("general-purpose");

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

    } catch (error: unknown) {
      if (sessionIdRef.current !== requestSessionId) {
        return;
      }
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
    const parts = splitLeadingPathPrefixFromMerged(
      message.content,
      message.composerAttachedPaths ?? [],
    );
    setUserMessageEdit({
      id: message.id,
      draft: parts.body,
    });
  }, []);

  const copyUserMessageText = useCallback(async (message: Message) => {
    try {
      const { paths, body } = splitLeadingPathPrefixFromMerged(
        message.content,
        message.composerAttachedPaths ?? [],
      );
      const pathLine = formatComposerPathPreview(paths);
      const pluginLine = (message.composerSelectedPluginIds ?? [])
        .map((id) => `#${id}`)
        .join(" ");
      const prefix = [pluginLine, pathLine].filter(Boolean).join(" ");
      await navigator.clipboard.writeText(
        prefix && body ? `${prefix}\n\n${body}` : prefix || body,
      );
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
      const retryContentParts = splitLeadingPathPrefixFromMerged(
        messageContent,
        message.composerAttachedPaths ?? [],
      );
      const rawRetryBody = retryContentParts.body;
      const researchRetry = parseResearchCommand(rawRetryBody);
      const goalRetry = parseGoalCommand(rawRetryBody);
      const composeAgent = message.composerAgentType ?? "general-purpose";
      if (researchRetry || goalRetry) {
        setBgToast(
          researchRetry
            ? "`/research` 暂不支持通过“重试”按钮重放，请重新发送命令。"
            : "`/goal` 暂不支持通过“重试”按钮重放，请重新发送命令。",
        );
        retrySendInFlightRef.current = false;
        return;
      }
      const retryPrepared = rewriteWorkflowBodyForBackend(rawRetryBody);
      const pendingFeedback = buildPendingExecutionFeedback({
        workflowCommand: retryPrepared.workflowCommand,
        composerAgentType: composeAgent,
      });
      const backendRetryContent = mergeComposerPathsAndBody(
        retryContentParts.paths,
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
      clearPostTurnMetaState();
      bumpQueueUi();

      setMessages(truncated);
      replaceStoreMessagesSnapshot(truncated.map(chatMessageToStore));
      setCurrentResponse("");
      currentResponseRef.current = "";
      setCurrentFoldIntermediate("");
      currentFoldIntermediateRef.current = "";
      pendingTextBufferRef.current = "";
      pendingFoldIntermediateBufferRef.current = "";
      pendingToolTraceTextRef.current = "";
      clearRunningToolTracking();
      activeReactFoldIdRef.current = null;
      if (textFlushRafRef.current !== null) {
        cancelAnimationFrame(textFlushRafRef.current);
        textFlushRafRef.current = null;
      }
      if (foldIntermediateFlushRafRef.current !== null) {
        cancelAnimationFrame(foldIntermediateFlushRafRef.current);
        foldIntermediateFlushRafRef.current = null;
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
      const { permissionMode, computerUseMode } = useChatComposerStore.getState();

      const userMessageId = message.id;
      const requestSessionId = sessionId;
      useChatComposerStore.getState().resetTaskComputerUseMode();

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
            selectedPluginIds: message.composerSelectedPluginIds ?? [],
            computerUseMode,
            ...(isPersistedMessageIdForRetry(message.id)
              ? { retryFromUserMessageId: message.id }
              : {}),
          },
        });

        if (sessionIdRef.current === requestSessionId) {
          setCurrentRoundId(response.round_id);
          currentRoundIdRef.current = response.round_id;
          setCurrentStreamId(response.message_id);
          currentStreamIdRef.current = response.message_id;
        }

        await setupStreamListener(
          response.message_id,
          requestSessionId,
          response.round_id,
        );

        if (sessionIdRef.current !== requestSessionId) {
          return;
        }

        useChatComposerStore.getState().setComposerAgentType("general-purpose");

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
      } catch (error: unknown) {
        if (sessionIdRef.current !== requestSessionId) {
          return;
        }
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
      clearPostTurnMetaState,
      clearRunningToolTracking,
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

    const rowContentParts = splitLeadingPathPrefixFromMerged(
      row.content,
      row.composerAttachedPaths ?? [],
    );
    const paths = rowContentParts.paths;
    const keepPaths =
      paths.length > 0
        ? paths
        : pathsStillMatchMergedContent(paths, trimmed)
          ? paths
          : undefined;
    const attached =
      keepPaths && keepPaths.length > 0 ? keepPaths : undefined;
    const content =
      attached && !pathsStillMatchMergedContent(attached, trimmed)
        ? mergeComposerPathsAndBody(attached, trimmed)
        : trimmed;
    const updated: Message = {
      ...row,
      content,
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
    clearPostTurnMetaState();
    flushSync(() => {
      composerRef.current?.setValue(next.body);
      const st = useChatComposerStore.getState();
      st.clearComposerAttachedPaths();
      for (const p of next.composerAttachedPaths) {
        st.addComposerAttachedPath(p);
      }
      st.clearComposerSelectedPluginIds();
      for (const id of next.composerSelectedPluginIds) {
        st.addComposerSelectedPluginId(id);
      }
      st.setComposerAgentType(next.composerAgentType);
      st.setPermissionMode(next.permissionMode);
      st.setComputerUseMode(next.computerUseMode);
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
        clearCancelFallbackTimer();
        cancelFallbackTimerRef.current = window.setTimeout(() => {
          cancelFallbackTimerRef.current = null;
          if (currentStreamIdRef.current !== streamId) return;
          const sid = sessionIdRef.current ?? "";
          cancelStreamListener(sid);
          unlistenRef.current = null;
          if (sid) clearStreamSnapshot(sid);
          setCurrentStreamId(null);
          currentStreamIdRef.current = null;
          setCurrentRoundId(null);
          currentRoundIdRef.current = null;
          setIsStreaming(false);
          isStreamingRef.current = false;
          retrySendInFlightRef.current = false;
          sendCancelledDuringRequestRef.current = false;
          setCurrentResponse("");
          currentResponseRef.current = "";
          setCurrentFoldIntermediate("");
          currentFoldIntermediateRef.current = "";
          activeReactFoldIdRef.current = null;
          pendingTextBufferRef.current = "";
          pendingFoldIntermediateBufferRef.current = "";
          pendingToolTraceTextRef.current = "";
          if (textFlushRafRef.current !== null) {
            cancelAnimationFrame(textFlushRafRef.current);
            textFlushRafRef.current = null;
          }
          if (foldIntermediateFlushRafRef.current !== null) {
            cancelAnimationFrame(foldIntermediateFlushRafRef.current);
            foldIntermediateFlushRafRef.current = null;
          }
          setPendingAssistantHint(null);
          setPendingAskUser(null);
          setAskUserSelections({});
          clearPostTurnMetaState();
          pendingTurnSummaryRef.current = null;
          pendingTokenUsageRef.current = null;
          setAwaitingResumeAfterCancel(true);
          const act = useActivityStore.getState();
          act.clearTransient();
          act.resetExecutionState();
        }, CANCEL_STREAM_LOCAL_FALLBACK_MS);
        // Prefer the backend `cancelled` event; the timer above prevents a stuck UI if it is lost.
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
    {
      const sid = sessionIdRef.current ?? "";
      cancelStreamListener(sid);
      unlistenRef.current = null;
      if (sid) clearStreamSnapshot(sid);
    }
    setCurrentStreamId(null);
    setIsStreaming(false);
    isStreamingRef.current = false;
    setCurrentResponse("");
    currentResponseRef.current = "";
    setCurrentFoldIntermediate("");
    currentFoldIntermediateRef.current = "";
    pendingTextBufferRef.current = "";
    pendingFoldIntermediateBufferRef.current = "";
    pendingToolTraceTextRef.current = "";
    clearRunningToolTracking();
    activeReactFoldIdRef.current = null;
    if (textFlushRafRef.current !== null) {
      cancelAnimationFrame(textFlushRafRef.current);
      textFlushRafRef.current = null;
    }
    if (foldIntermediateFlushRafRef.current !== null) {
      cancelAnimationFrame(foldIntermediateFlushRafRef.current);
      foldIntermediateFlushRafRef.current = null;
    }
    setAwaitingResumeAfterCancel(false);
    act.clearTransient();
    act.resetExecutionState();
  };

  const handleKeyDown = (e: React.KeyboardEvent<HTMLElement>) => {
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
    clearPostTurnMetaState();
    clearQueuedMainSends();
    setFollowUpTaskId(null);
    setBgTranscriptTaskId(null);
    setIndexingStatus("idle");
    composerRef.current?.setValue("");
    setBgToast(null);
    setLearningProposalToast(null);
    setPathRequiredToast(null);
    setCopySuccessToast(false);
    setCurrentRoundId(null);
    clearCancelFallbackTimer();
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

    {
      const sid = sessionIdRef.current ?? "";
      cancelStreamListener(sid);
      unlistenRef.current = null;
      if (sid) clearStreamSnapshot(sid);
    }
    setCurrentStreamId(null);
    setIsStreaming(false);
    isStreamingRef.current = false;
    setCurrentResponse("");
    currentResponseRef.current = "";
    setCurrentFoldIntermediate("");
    currentFoldIntermediateRef.current = "";
    pendingTextBufferRef.current = "";
    pendingFoldIntermediateBufferRef.current = "";
    pendingToolTraceTextRef.current = "";
    clearRunningToolTracking();
    if (textFlushRafRef.current !== null) {
      cancelAnimationFrame(textFlushRafRef.current);
      textFlushRafRef.current = null;
    }
    if (foldIntermediateFlushRafRef.current !== null) {
      cancelAnimationFrame(foldIntermediateFlushRafRef.current);
      foldIntermediateFlushRafRef.current = null;
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
  }, [
    currentStreamId,
    clearQueuedMainSends,
    clearCancelFallbackTimer,
    clearPostTurnMetaState,
    clearRunningToolTracking,
  ]);

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
    clearRunningToolTracking();
    activeReactFoldIdRef.current = null;
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

    const {
      composerAgentType,
      permissionMode,
      computerUseMode,
      environment,
      sshServer,
      sandboxBackend,
    } =
      useChatComposerStore.getState();
    const resumeComputerUseMode =
      computerUseMode === "session" ? computerUseMode : "off";
    const requestSessionId = sessionId;

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
          computerUseMode: resumeComputerUseMode,
        },
      });

      if (sessionIdRef.current === requestSessionId) {
        setCurrentRoundId(response.round_id);
        currentRoundIdRef.current = response.round_id;
        setCurrentStreamId(response.message_id);
        currentStreamIdRef.current = response.message_id;
      }

      await setupStreamListener(
        response.message_id,
        requestSessionId,
        response.round_id,
      );

      if (sessionIdRef.current !== requestSessionId) {
        return;
      }

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
    } catch (error: unknown) {
      if (sessionIdRef.current !== requestSessionId) {
        return;
      }
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

  const markdownImageWorkspacePath =
    currentSession?.workingDirectory ?? currentSession?.projectPath ?? "";
  const agentComponents = useMemo(
    () =>
      buildMarkdownComponents(
        true,
        theme,
        CHAT,
        handleMarkdownImageClick,
        handleNodeClick,
        markdownImageWorkspacePath,
      ),
    [
      theme,
      CHAT,
      handleMarkdownImageClick,
      handleNodeClick,
      markdownImageWorkspacePath,
    ],
  );
  const handleCopyUserMessage = useCallback(
    (message: Message) => {
      void copyUserMessageText(message);
    },
    [copyUserMessageText],
  );
  const handleUserEditDraftChange = useCallback((draft: string) => {
    setUserMessageEdit((cur) => (cur ? { ...cur, draft } : null));
  }, []);
  const handleCancelUserMessageEdit = useCallback(() => {
    setUserMessageEdit(null);
  }, []);
  const restoringOlderItems =
    allItemsVisible && scrollRestoreRef.current !== null;

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
        aria-label="聊天或终端"
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
        <Tab label="聊天" id="omiga-tab-chat" />
        <Tab label="终端" id="omiga-tab-terminal" />
      </Tabs>

      <Box
        aria-hidden={panelTab !== 1}
        sx={{
          flex: panelTab === 1 ? 1 : 0,
          minHeight: 0,
          overflow: "hidden",
          display: panelTab === 1 ? "flex" : "none",
          flexDirection: "column",
        }}
      >
        <Terminal
          embedded
          active={panelTab === 1}
          sessionId={sessionId}
          workspacePath={currentSession?.projectPath ?? null}
        />
      </Box>

      <Box
        aria-hidden={panelTab !== 0}
        sx={{
          flex: panelTab === 0 ? 1 : 0,
          minHeight: 0,
          display: panelTab === 0 ? "flex" : "none",
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
                        transition:
                          "color 0.25s ease, background-color 0.25s ease, transform 0.2s ease, box-shadow 0.25s ease",
                        "@media (prefers-reduced-motion: reduce)": {
                          transition: "none",
                        },
                        "&:hover": {
                          color: theme.palette.primary.main,
                          bgcolor: alpha(theme.palette.primary.main, 0.1),
                          boxShadow: `0 3px 12px ${alpha(theme.palette.primary.main, 0.18)}`,
                          transform: "translateY(-1px)",
                        },
                        "&:active": {
                          transform: "translateY(1px)",
                          boxShadow: "none",
                          transition: "transform 0.1s ease, box-shadow 0.1s ease",
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
                          sx={{
                            transition:
                              "border-color 0.3s ease, box-shadow 0.3s ease, background-color 0.3s ease, transform 0.2s ease",
                            "@media (prefers-reduced-motion: reduce)": {
                              transition: "none",
                            },
                            "& .MuiChip-label": {
                              letterSpacing: "0.01em",
                              transition: "letter-spacing 0.3s ease",
                              "@media (prefers-reduced-motion: reduce)": {
                                transition: "none",
                              },
                            },
                            "&:hover": {
                              borderColor: alpha(theme.palette.primary.main, 0.5),
                              bgcolor: alpha(theme.palette.primary.main, 0.08),
                              boxShadow: `0 3px 14px ${alpha(theme.palette.primary.main, 0.2)}`,
                              transform: "translateY(-1px)",
                              "& .MuiChip-label": { letterSpacing: "0.06em" },
                            },
                            "&:active": {
                              transform: "translateY(1px)",
                              boxShadow: "none",
                              transition: "transform 0.1s ease, box-shadow 0.1s ease",
                            },
                          }}
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
            {/* ── Session switch: skeleton loader ────────────────────────────
                While isSwitchingSession is true (IPC cache miss) we show
                message-shaped shimmer rows instead of a blank area + spinner.
                Fades in immediately on switch, fades out when content arrives. */}
            <Fade in={isSwitchingSession} timeout={100} unmountOnExit>
              <Box sx={{ width: "100%", pt: 0.5 }}>
                <SessionSwitchSkeleton />
              </Box>
            </Fade>

            {/* Pagination: "load older messages" indicator at the top of the list */}
            {!isSwitchingSession && (hasMoreMessages || isLoadingMoreMessages) && (
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

            {/* Empty state — only when genuinely empty (not while skeleton is showing) */}
            {!isSwitchingSession && messages.length === 0 && !currentResponseHasVisibleText && (
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

            {/* ── Content enter animation ────────────────────────────────────
                key changes when:
                  • isSwitchingSession flips false (cache miss load complete)
                  • sessionId changes while not switching (cache hit)
                React remounts this wrapper on key change → @keyframes fires.
                Uses transform+opacity only: zero layout repaints (GPU path).
                prefers-reduced-motion: the animation is gated below. */}
            <Box
              ref={messagesContentRef}
              key={isSwitchingSession ? "skeleton" : (sessionId ?? "none")}
              sx={{
                display: "flex",
                flexDirection: "column",
                gap: 2,
                width: "100%",
                "@media (prefers-reduced-motion: no-preference)": {
                  "@keyframes omigaFadeUp": {
                    from: {
                      opacity: 0,
                      transform: "translateY(10px)",
                    },
                    to: {
                      opacity: 1,
                      transform: "translateY(0)",
                    },
                  },
                  animation: "omigaFadeUp 220ms cubic-bezier(0.16, 1, 0.3, 1) both",
                },
              }}
            >
            {visibleDisplayedItems.map((item, itemIndex) => {
              const itemKey = messageRenderItemKey(item);
              const animateMessageItem = shouldAnimateMessageItem({
                restoringOlderItems,
              });
              const msgStagger = animateMessageItem
                ? messageEntranceDelayMs(itemIndex)
                : 0;
              const nextItem = visibleDisplayedItems[itemIndex + 1];
              const nextRowIsUser =
                nextItem?.kind === "row" && nextItem.message.role === "user";

              return (
                <Box
                  key={itemKey}
                  sx={{
                    width: "100%",
                    ...(animateMessageItem
                      ? {
                          "@media (prefers-reduced-motion: no-preference)": {
                            "@keyframes omigaMsgIn": {
                              from: { opacity: 0, transform: "translateY(5px)" },
                              to: { opacity: 1, transform: "translateY(0)" },
                            },
                            animation: `omigaMsgIn 180ms ${msgStagger}ms ease-out both`,
                          },
                        }
                      : {
                          animation: "none",
                          "@media (prefers-reduced-motion: no-preference)": {
                            animation: "none",
                          },
                        }),
                  }}
                >
                  {item.kind === "react_fold" ? (
                    <ReactFoldRenderItem
                      item={item}
                      expanded={expandedToolGroups.has(item.id)}
                      isLastFold={item.id === activeReactFoldId}
                      liveIntermediateText={
                        item.id === activeReactFoldId
                          ? liveReActFoldTraceText
                          : ""
                      }
                      activityIsStreaming={activityIsStreaming}
                      waitingFirstChunk={waitingFirstChunk}
                      pendingAskUserToolUseId={pendingAskUser?.toolUseId ?? null}
                      nestedToolPanelOpenForFold={getNestedToolPanelOpenForFold(
                        nestedToolPanelOpenByFold,
                        item.id,
                      )}
                      chat={CHAT}
                      components={agentComponents}
                      onToggleGroup={toggleToolGroupExpand}
                      onToggleNestedToolPanel={toggleNestedToolPanel}
                    />
                  ) : (
                    <MessageRowRenderItem
                      item={item}
                      nextRowIsUser={nextRowIsUser}
                      isEditingUser={userMessageEdit?.id === item.message.id}
                      editDraft={
                        userMessageEdit?.id === item.message.id
                          ? userMessageEdit.draft
                          : ""
                      }
                      sessionId={sessionId}
                      chat={CHAT}
                      components={agentComponents}
                      onRetryMessage={requestRetryUserMessage}
                      onEditMessage={openEditUserMessage}
                      onCopyMessage={handleCopyUserMessage}
                      onEditDraftChange={handleUserEditDraftChange}
                      onCancelEdit={handleCancelUserMessageEdit}
                      onSaveEdit={saveUserMessageEdit}
                      onOpenReviewerTranscript={handleOpenBackgroundTranscript}
                      onExecutePlan={executeExistingPlan}
                      onDispatchWorkflowCommand={dispatchWorkflowCommand}
                    />
                  )}
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

            {/* Streaming: visible assistant text is always a normal reply draft.
                Hidden/thinking text is rendered separately inside the active ReAct fold. */}
            {isStreaming && currentResponseHasVisibleText && (
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
                <ChatMarkdownContent
                  content={currentResponse}
                  tone="agent"
                  components={agentComponents}
                  chat={CHAT}
                />
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

            {showSuggestionsGeneratingPlaceholder && (
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

                        // ── Glassmorphism suggestion button ──────────────
                        // Matches the reference design: semi-transparent
                        // primary gradient, inner purple-pink glow overlay
                        // that fades in on hover, plus a damped jitter.
                        const button = (
                          <Button
                            size="small"
                            variant="contained"
                            disableElevation
                            onClick={() => {
                              composerRef.current?.setValue(s.text);
                              queueMicrotask(() => inputRef.current?.focus());
                            }}
                            sx={{
                              position: "relative",
                              overflow: "hidden",
                              textTransform: "none",
                              borderRadius: "10px",
                              maxWidth: "100%",
                              fontSize: 12,
                              fontWeight: 600,
                              py: 0.625,
                              px: 1.5,
                              // Glass base: primary gradient + blur
                              background: `linear-gradient(135deg, ${alpha(CHAT.accent, 0.88)} 0%, ${alpha(CHAT.accent, 0.72)} 100%)`,
                              border: `1px solid ${alpha(CHAT.accent, 0.32)}`,
                              color: theme.palette.primary.contrastText,
                              boxShadow: `0 2px 12px ${alpha(CHAT.accent, 0.26)}, inset 0 1px 0 ${alpha(theme.palette.common.white, 0.15)}`,
                              backdropFilter: "blur(4px)",
                              transition: "background 300ms ease, box-shadow 300ms ease, border-color 300ms ease",
                              // Inner glow: purple→pink, fades in on hover
                              "&::before": {
                                content: '""',
                                position: "absolute",
                                inset: 0,
                                background: `linear-gradient(135deg, ${alpha(CHAT.accent, 0.38)} 0%, ${alpha(CHAT.accent, 0.22)} 100%)`,
                                filter: "blur(10px)",
                                opacity: 0,
                                transition: "opacity 300ms ease",
                                pointerEvents: "none",
                              },
                              "&:hover::before": { opacity: 0.75 },
                              "&:hover": {
                                background: `linear-gradient(135deg, ${alpha(CHAT.accent, 0.94)} 0%, ${alpha(CHAT.accent, 0.82)} 100%)`,
                                boxShadow: `0 4px 20px ${alpha(CHAT.accent, 0.40)}, inset 0 1px 0 ${alpha(theme.palette.common.white, 0.22)}`,
                                borderColor: alpha(CHAT.accent, 0.5),
                              },
                            }}
                          >
                            {/* z-index keeps label above the ::before glow */}
                            <Box
                              component="span"
                              sx={{ position: "relative", zIndex: 1 }}
                            >
                              {s.label}
                            </Box>
                          </Button>
                        );

                        if (!tooltipMarkdown) {
                          return (
                            <Box key={`${idx}-${s.label}`} sx={{ display: "inline-flex" }}>
                              {button}
                            </Box>
                          );
                        }

                        // ── Glassmorphism tooltip ─────────────────────────
                        // Matches the reference: dark-gradient panel, blur,
                        // white/10 border, indigo glow shadow, icon header.
                        return (
                          <Tooltip
                            key={`${idx}-${s.label}`}
                            placement="top"
                            enterDelay={400}
                            arrow
                            componentsProps={{
                              tooltip: {
                                sx: {
                                  bgcolor: "transparent",
                                  p: 0,
                                  maxWidth: 380,
                                  boxShadow: "none",
                                  border: "none",
                                },
                              },
                              arrow: {
                                sx: {
                                  color: alpha(theme.palette.background.paper, 0.97),
                                  "&::before": {
                                    border: `1px solid ${CHAT.agentBubbleBorder}`,
                                  },
                                },
                              },
                            }}
                            title={
                              <Box
                                sx={{
                                  position: "relative",
                                  p: "14px 16px",
                                  background: `linear-gradient(135deg, ${alpha(theme.palette.background.paper, 0.96)} 0%, ${alpha(theme.palette.background.default, 0.94)} 100%)`,
                                  backdropFilter: "blur(12px)",
                                  WebkitBackdropFilter: "blur(12px)",
                                  borderRadius: "16px",
                                  border: `1px solid ${CHAT.agentBubbleBorder}`,
                                  boxShadow: `0 0 30px ${alpha(CHAT.accent, 0.18)}, 0 4px 20px ${alpha(theme.palette.common.black, 0.3)}`,
                                  overflow: "hidden",
                                  maxWidth: 360,
                                }}
                              >
                                {/* Header: icon + title */}
                                <Box
                                  sx={{
                                    display: "flex",
                                    alignItems: "center",
                                    gap: 1.25,
                                    mb: 1.25,
                                  }}
                                >
                                  <Box
                                    sx={{
                                      width: 28,
                                      height: 28,
                                      borderRadius: "50%",
                                      bgcolor: alpha(CHAT.accent, 0.22),
                                      display: "flex",
                                      alignItems: "center",
                                      justifyContent: "center",
                                      flexShrink: 0,
                                    }}
                                  >
                                    <svg
                                      viewBox="0 0 20 20"
                                      fill="currentColor"
                                      style={{
                                        width: 13,
                                        height: 13,
                                        color: CHAT.accent,
                                      }}
                                    >
                                      <path
                                        clipRule="evenodd"
                                        fillRule="evenodd"
                                        d="M18 10a8 8 0 11-16 0 8 8 0 0116 0zm-7-4a1 1 0 11-2 0 1 1 0 012 0zM9 9a1 1 0 000 2v3a1 1 0 001 1h1a1 1 0 100-2v-3a1 1 0 00-1-1H9z"
                                      />
                                    </svg>
                                  </Box>
                                  <Typography
                                    sx={{
                                      fontSize: 12,
                                      fontWeight: 600,
                                      color: CHAT.textPrimary,
                                      lineHeight: 1.3,
                                    }}
                                  >
                                    操作详情
                                  </Typography>
                                </Box>

                                {/* Markdown content */}
                                <Box
                                  sx={{
                                    position: "relative",
                                    zIndex: 1,
                                    "& p": {
                                      m: 0,
                                      lineHeight: 1.5,
                                      fontSize: 12,
                                      color: CHAT.textMuted,
                                    },
                                    "& ul, & ol": {
                                      my: 0.5,
                                      pl: 2,
                                      fontSize: 12,
                                      color: CHAT.textMuted,
                                    },
                                    "& li": { my: 0.25 },
                                    "& code": {
                                      fontSize: 11,
                                      bgcolor: alpha(CHAT.textPrimary, 0.08),
                                      px: 0.5,
                                      py: 0.125,
                                      borderRadius: "4px",
                                      fontFamily: "monospace",
                                    },
                                  }}
                                >
                                  <ReactMarkdown remarkPlugins={[remarkGfm]}>
                                    {tooltipMarkdown}
                                  </ReactMarkdown>
                                </Box>

                                {/* Background glow layer */}
                                <Box
                                  sx={{
                                    position: "absolute",
                                    inset: 0,
                                    borderRadius: "16px",
                                    background: `linear-gradient(135deg, ${alpha(CHAT.accent, 0.08)}, ${alpha(CHAT.accent, 0.04)})`,
                                    filter: "blur(20px)",
                                    opacity: 0.5,
                                    pointerEvents: "none",
                                  }}
                                />
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

            {/* ── Close animated content wrapper ─────────────────────── */}
            </Box>

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
              position: "relative",
            }}
          >
            <Fade
              in={showJumpToLatest}
              timeout={{ enter: 180, exit: 0 }}
              unmountOnExit
            >
              <Box
                sx={{
                  position: "absolute",
                  left: "50%",
                  top: -54,
                  transform: "translateX(-50%)",
                  zIndex: 2,
                }}
              >
                <Tooltip title="跳转到最新消息">
                  <IconButton
                    type="button"
                    aria-label="跳转到最新消息"
                    onClick={handleJumpToLatest}
                    sx={{
                      width: 44,
                      height: 44,
                      p: 0,
                      borderRadius: "50%",
                      border: 0,
                      bgcolor: "transparent",
                      color: "primary.main",
                      overflow: "visible",
                      animation: isJumpToLatestClickAnimating
                        ? `${JUMP_TO_LATEST_CLICK_ANIMATION_MS}ms jumpToLatestBubbleButton cubic-bezier(0.2, 0, 0, 1) both`
                        : "none",
                      transition:
                        "color 180ms ease, transform 180ms ease",
                      "@keyframes jumpToLatestBubbleButton": {
                        "0%": {
                          transform: "translateY(0) scale(1)",
                        },
                        "32%": {
                          transform: "translateY(3px) scale(0.94)",
                        },
                        "64%": {
                          transform: "translateY(-1px) scale(1.03)",
                        },
                        "100%": {
                          transform: "translateY(0) scale(0.98)",
                        },
                      },
                      "@keyframes jumpToLatestBubbleShell": {
                        "0%": {
                          opacity: 1,
                          transform: "scale(1)",
                        },
                        "36%": {
                          opacity: 0.95,
                          transform: "scale(0.88)",
                          borderColor: alpha(theme.palette.primary.main, 0.58),
                          backgroundColor: alpha(theme.palette.primary.main, 0.14),
                        },
                        "72%": {
                          opacity: 0.46,
                          transform: "scale(1.38)",
                          borderColor: alpha(theme.palette.primary.main, 0.36),
                          backgroundColor: alpha(theme.palette.primary.main, 0.04),
                        },
                        "100%": {
                          opacity: 0,
                          transform: "scale(1.76)",
                          borderColor: alpha(theme.palette.primary.main, 0),
                          backgroundColor: alpha(theme.palette.primary.main, 0),
                        },
                      },
                      "@keyframes jumpToLatestBubbleShards": {
                        "0%": {
                          opacity: 0,
                          transform: "scale(0.38)",
                          boxShadow: [
                            `0 0 0 0 ${alpha(theme.palette.primary.main, 0.9)}`,
                            `0 0 0 0 ${alpha(theme.palette.primary.main, 0.72)}`,
                            `0 0 0 0 ${alpha(theme.palette.primary.light, 0.78)}`,
                            `0 0 0 0 ${alpha(theme.palette.primary.main, 0.68)}`,
                            `0 0 0 0 ${alpha(theme.palette.primary.light, 0.7)}`,
                            `0 0 0 0 ${alpha(theme.palette.primary.main, 0.6)}`,
                            `0 0 0 0 ${alpha(theme.palette.primary.light, 0.72)}`,
                            `0 0 0 0 ${alpha(theme.palette.primary.main, 0.66)}`,
                          ].join(", "),
                        },
                        "22%": {
                          opacity: 1,
                          transform: "scale(0.76)",
                          boxShadow: [
                            `0 -8px 0 0 ${alpha(theme.palette.primary.main, 0.9)}`,
                            `7px -5px 0 0 ${alpha(theme.palette.primary.main, 0.72)}`,
                            `8px 3px 0 0 ${alpha(theme.palette.primary.light, 0.78)}`,
                            `3px 8px 0 0 ${alpha(theme.palette.primary.main, 0.68)}`,
                            `-5px 7px 0 0 ${alpha(theme.palette.primary.light, 0.7)}`,
                            `-9px 1px 0 0 ${alpha(theme.palette.primary.main, 0.6)}`,
                            `-6px -6px 0 0 ${alpha(theme.palette.primary.light, 0.72)}`,
                            `2px -10px 0 0 ${alpha(theme.palette.primary.main, 0.66)}`,
                          ].join(", "),
                        },
                        "100%": {
                          opacity: 0,
                          transform: "scale(1.08)",
                          boxShadow: [
                            `0 -23px 0 -1px ${alpha(theme.palette.primary.main, 0)}`,
                            `18px -15px 0 -1px ${alpha(theme.palette.primary.main, 0)}`,
                            `22px 7px 0 -1px ${alpha(theme.palette.primary.light, 0)}`,
                            `7px 24px 0 -1px ${alpha(theme.palette.primary.main, 0)}`,
                            `-15px 20px 0 -1px ${alpha(theme.palette.primary.light, 0)}`,
                            `-24px 4px 0 -1px ${alpha(theme.palette.primary.main, 0)}`,
                            `-18px -17px 0 -1px ${alpha(theme.palette.primary.light, 0)}`,
                            `5px -27px 0 -1px ${alpha(theme.palette.primary.main, 0)}`,
                          ].join(", "),
                        },
                      },
                      "@keyframes jumpToLatestIconBubblePop": {
                        "0%": {
                          transform: "translateY(0) scale(1)",
                          opacity: 1,
                        },
                        "34%": {
                          transform: "translateY(3px) scale(0.86)",
                          opacity: 0.9,
                        },
                        "70%": {
                          transform: "translateY(-1px) scale(0.98)",
                          opacity: 0.48,
                        },
                        "100%": {
                          transform: "translateY(1px) scale(0.68)",
                          opacity: 0,
                        },
                      },
                      "&::before": {
                        content: '""',
                        position: "absolute",
                        inset: 6,
                        borderRadius: "50%",
                        border: `1px solid ${alpha(theme.palette.text.primary, 0.2)}`,
                        bgcolor: alpha(theme.palette.background.paper, 0.96),
                        boxShadow: `0 10px 26px ${alpha(
                          theme.palette.common.black,
                          theme.palette.mode === "dark" ? 0.36 : 0.16,
                        )}`,
                        backdropFilter: "blur(10px)",
                        transform: "scale(1)",
                        animation: isJumpToLatestClickAnimating
                          ? `${JUMP_TO_LATEST_CLICK_ANIMATION_MS}ms jumpToLatestBubbleShell cubic-bezier(0.2, 0, 0, 1) both`
                          : "none",
                        transition:
                          "background-color 180ms ease, border-color 180ms ease, box-shadow 180ms ease, transform 180ms ease",
                      },
                      "&::after": {
                        content: '""',
                        position: "absolute",
                        left: "calc(50% - 2px)",
                        top: "calc(50% - 2px)",
                        width: 4,
                        height: 4,
                        borderRadius: "50%",
                        bgcolor: "primary.main",
                        opacity: 0,
                        pointerEvents: "none",
                        transform: "scale(0.38)",
                        zIndex: 2,
                        animation: isJumpToLatestClickAnimating
                          ? `${JUMP_TO_LATEST_CLICK_ANIMATION_MS}ms jumpToLatestBubbleShards cubic-bezier(0.16, 1, 0.3, 1) both`
                          : "none",
                      },
                      "@media (prefers-reduced-motion: reduce)": {
                        transition: "none",
                        animation: "none",
                        "&::before": {
                          transition: "none",
                          animation: "none",
                        },
                        "&::after": {
                          animation: "none",
                        },
                        "& svg": {
                          animation: "none",
                        },
                      },
                      "&:hover": {
                        color: "primary.main",
                        transform: "translateY(-1px)",
                        "&::before": {
                          bgcolor: theme.palette.background.paper,
                          borderColor: alpha(theme.palette.primary.main, 0.42),
                          boxShadow: `0 12px 30px ${alpha(
                            theme.palette.common.black,
                            theme.palette.mode === "dark" ? 0.42 : 0.2,
                          )}`,
                        },
                      },
                      "&:active": {
                        transform: "translateY(1px)",
                        "&::before": {
                          boxShadow: `0 6px 18px ${alpha(
                            theme.palette.common.black,
                            theme.palette.mode === "dark" ? 0.32 : 0.14,
                          )}`,
                        },
                      },
                      "& svg": {
                        position: "relative",
                        zIndex: 1,
                        color: "primary.main",
                        animation: isJumpToLatestClickAnimating
                          ? `${JUMP_TO_LATEST_CLICK_ANIMATION_MS}ms jumpToLatestIconBubblePop cubic-bezier(0.2, 0, 0, 1) both`
                          : "none",
                      },
                    }}
                  >
                    <KeyboardArrowDownRounded sx={{ fontSize: 28 }} />
                  </IconButton>
                </Tooltip>
              </Box>
            </Fade>
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
              <ResearchGoalStatusPill
                goal={activeResearchGoal}
                onPrepareCommand={prepareResearchGoalCommand}
                onEditCriteria={handleOpenResearchGoalCriteria}
                onOpenAuditDetails={handleOpenResearchGoalAuditDetails}
                autoRunEnabled={goalAutoRunEnabled}
                onToggleAutoRun={handleToggleResearchGoalAutoRun}
                autoRunDisabled={needsWorkspacePath || Boolean(followUpTaskId)}
              />
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

      <ResearchGoalCriteriaDialog
        open={goalCriteriaDialogOpen}
        goal={activeResearchGoal}
        saving={goalCriteriaSaving}
        error={goalCriteriaError}
        providerEntryOptions={goalProviderEntryOptions}
        providerEntryOptionsLoading={goalProviderEntryOptionsLoading}
        onClose={handleCloseResearchGoalCriteria}
        onSave={handleSaveResearchGoalCriteria}
        onSuggestCriteria={handleSuggestResearchGoalCriteria}
        onTestSecondOpinionProvider={handleTestResearchGoalSecondOpinionProvider}
      />

      <ResearchGoalAuditDetailsDialog
        open={goalAuditDialogOpen}
        goal={activeResearchGoal}
        onClose={() => setGoalAuditDialogOpen(false)}
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

      <Dialog
        open={Boolean(learningProposalPrompt)}
        onClose={() => {
          if (learningProposalBusyAction) return;
          setLearningProposalPrompt(null);
        }}
        maxWidth="xs"
        fullWidth
      >
        <DialogTitle>{learningProposalPrompt?.title ?? "学习建议"}</DialogTitle>
        <DialogContent>
          <Alert severity="info" sx={{ mb: 1.5 }}>
            {learningProposalPrompt?.message ?? ""}
          </Alert>
          <Typography variant="body2" color="text.secondary">
            保存后只会写入项目学习记录，用于后续 agent
            自动分析和给出固化建议；不会静默修改 operator、template 或移动产物文件。
          </Typography>
        </DialogContent>
        <DialogActions>
          {(learningProposalPrompt?.actions ?? []).map((action) => (
            <Button
              key={action.id}
              type="button"
              size="small"
              variant={action.id === "approve_apply" ? "contained" : "text"}
              color={
                action.id === "dismiss"
                  ? "inherit"
                  : action.id === "approve_apply"
                    ? "success"
                    : "primary"
              }
              disabled={Boolean(learningProposalBusyAction)}
              onClick={() => void handleLearningProposalAction(action.id)}
              sx={{ minHeight: 30, py: 0.35 }}
            >
              {learningProposalBusyAction === action.id
                ? "处理中…"
                : action.label}
            </Button>
          ))}
        </DialogActions>
      </Dialog>

      <NotificationToast
        open={copySuccessToast}
        autoHideDuration={3000}
        onClose={() => {
          setCopySuccessToast(false);
        }}
        severity="success"
        title="复制成功"
        message="已复制到剪贴板"
        zIndexOffset={1}
      />

      <NotificationToast
        open={Boolean(bgToast)}
        autoHideDuration={7000}
        onClose={() => {
          setBgToast(null);
        }}
        severity="info"
        title="后台任务通知"
        message={bgToast}
      />

      <NotificationToast
        open={Boolean(learningProposalToast)}
        autoHideDuration={9000}
        onClose={() => {
          setLearningProposalToast(null);
        }}
        severity={learningProposalToast?.severity ?? "info"}
        title={learningProposalToast?.title ?? "学习建议"}
        message={learningProposalToast?.message ?? ""}
        zIndexOffset={2}
      />

      <BackgroundAgentTranscriptDrawer
        open={bgTranscriptTaskId !== null}
        onClose={() => setBgTranscriptTaskId(null)}
        sessionId={sessionId}
        taskId={bgTranscriptTaskId}
        taskLabel={bgTranscriptLabel}
      />

      <NotificationToast
        key={pathToastKey}
        open={Boolean(pathRequiredToast)}
        autoHideDuration={5000}
        onClose={() => {
          setPathRequiredToast(null);
        }}
        severity="warning"
        title="需要选择工作目录"
        message={pathRequiredToast}
        zIndexOffset={1}
      />
    </Box>
  );
}
