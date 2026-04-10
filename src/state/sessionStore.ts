import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import { notifyProviderChanged } from "../utils/providerEvents";

const dbg = (...args: unknown[]) =>
  console.debug("[OmigaDebug][sessionStore]", ...args);

const PENDING_PROJECT_PATH_KEY = "omiga.pendingProjectPathSessions";

function readPendingProjectPathIds(): Set<string> {
  if (typeof localStorage === "undefined") return new Set();
  try {
    const raw = localStorage.getItem(PENDING_PROJECT_PATH_KEY);
    if (!raw) return new Set();
    const arr = JSON.parse(raw) as unknown;
    if (!Array.isArray(arr)) return new Set();
    return new Set(arr.filter((x): x is string => typeof x === "string"));
  } catch {
    return new Set();
  }
}

function persistPendingProjectPathIds(ids: Set<string>) {
  if (typeof localStorage === "undefined") return;
  localStorage.setItem(PENDING_PROJECT_PATH_KEY, JSON.stringify([...ids]));
}

/** Legacy auto titles — replaced after the first user message. */
export const PLACEHOLDER_SESSION_TITLE_PREFIX = "New chat ·";

/** Stored name for a fresh session (list shows gray italic until the first message). */
export const UNUSED_SESSION_LABEL = "New session";

/** True when the title is still the empty-session placeholder (first message will rename). */
export function isPlaceholderSessionTitle(name: string | undefined | null): boolean {
  if (!name) return false;
  const t = name.trim();
  if (t === UNUSED_SESSION_LABEL) return true;
  if (t.toLowerCase() === "new session") return true;
  return name.trimStart().startsWith(PLACEHOLDER_SESSION_TITLE_PREFIX);
}

/**
 * Sidebar list: gray italic “New session” until the first user message renames the row.
 * Pass `storeMessageCount` for the current session so local transcript wins over stale `messageCount`.
 */
export function shouldShowNewSessionPlaceholder(
  session: Session,
  opts?: { isCurrentSession?: boolean; storeMessageCount?: number },
): boolean {
  if (!isPlaceholderSessionTitle(session.name)) return false;
  const db = session.messageCount ?? 0;
  const n =
    opts?.isCurrentSession && opts.storeMessageCount != null
      ? Math.max(db, opts.storeMessageCount)
      : db;
  return n === 0;
}

const FIRST_MESSAGE_TITLE_MAX_CHARS = 48;

/** One-line title from the first user message (first non-empty line, collapsed whitespace). */
export function titleFromFirstUserMessage(raw: string): string {
  let firstNonEmpty = "";
  for (const line of raw.split(/\r?\n/)) {
    const t = line.trim();
    if (t) {
      firstNonEmpty = t;
      break;
    }
  }
  const collapsed = firstNonEmpty.replace(/\s+/g, " ");
  if (!collapsed) return "Chat";
  const chars = [...collapsed];
  if (chars.length <= FIRST_MESSAGE_TITLE_MAX_CHARS) return collapsed;
  return `${chars.slice(0, FIRST_MESSAGE_TITLE_MAX_CHARS - 1).join("")}…`;
}

export interface Session {
  id: string;
  name: string;
  projectPath: string;
  workingDirectory?: string;
  createdAt: string;
  updatedAt: string;
  messageCount?: number;
}

/** True when no real workspace folder is set (`"."` and empty mean “pick a folder” in Omiga). */
export function isUnsetWorkspacePath(projectPath: string | undefined): boolean {
  const p = (projectPath ?? "").trim();
  return p === "" || p === ".";
}

export type RoundStatus = "running" | "partial" | "cancelled" | "completed";

export interface SchedulerPlan {
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

export interface Message {
  id: string;
  role: "user" | "assistant" | "tool";
  content: string;
  /** Composer 选择的 Agent（非 general-purpose）；用于聊天气泡展示，重载会话时若后端未存则可能缺失 */
  composerAgentType?: string;
  /** @ 选择的相对路径，仅本地会话快照 */
  composerAttachedPaths?: string[];
  /** 调度系统生成的任务执行计划 */
  schedulerPlan?: SchedulerPlan;
  /** Full tool_calls from DB on assistant rows — used to rebuild trace when tool rows are missing or unnamed */
  toolCallsList?: Array<{ id: string; name: string; arguments: string }>;
  /** Assistant thinking merged into the ReAct fold (optional round-trip for local snapshot) */
  prefaceBeforeTools?: string;
  toolCall?: {
    id: string;
    name: string;
    arguments: string;
    /** Tool stdout/stderr result (tool role messages also keep plain text in `content`) */
    output?: string;
    status?: "pending" | "running" | "completed" | "error";
  };
  roundId?: string;
  roundStatus?: RoundStatus;
  /** 后端在回合结束时生成的快捷追问（仅存本地快照） */
  followUpSuggestions?: Array<{ label: string; prompt: string }>;
  /** 回合结束后由独立 LLM 生成的可选要点摘要（仅存本地快照） */
  turnSummary?: string;
  /** 主对话 token 统计（prompt / completion，来自供应商 API） */
  tokenUsage?: {
    input: number;
    output: number;
    total?: number;
    provider?: string;
  };
}

/** Strip trailing `[stderr]` (and preceding newlines) from merged shell / task output before persistence. */
function stripTrailingStderrMarker(
  text: string | undefined,
): string | undefined {
  if (text == null) return text;
  let s = text;
  let prev: string;
  do {
    prev = s;
    s = s.replace(/(?:\r?\n)*\[stderr\]\s*$/u, "");
  } while (s !== prev);
  return s;
}

function sanitizeMessageForPersistence(m: Message): Message {
  const ts = m.turnSummary?.trim();
  return {
    ...m,
    ...(ts ? { turnSummary: ts } : { turnSummary: undefined }),
    content: stripTrailingStderrMarker(m.content) ?? "",
    prefaceBeforeTools: stripTrailingStderrMarker(m.prefaceBeforeTools),
    toolCallsList: m.toolCallsList?.map((tc) => ({
      ...tc,
      arguments: stripTrailingStderrMarker(tc.arguments) ?? "",
    })),
    toolCall: m.toolCall
      ? {
          ...m.toolCall,
          arguments: stripTrailingStderrMarker(m.toolCall.arguments) ?? "",
          output: stripTrailingStderrMarker(m.toolCall.output),
        }
      : undefined,
  };
}

interface SessionSummary {
  id: string;
  name: string;
  message_count: number;
  updated_at: string;
}

interface SessionData {
  id: string;
  name: string;
  messages: Array<{
    role: "user" | "assistant" | "tool";
    /** SQLite `messages.id` when present */
    id?: string;
    content?: string; // User and Assistant have content
    output?: string; // Tool messages have output instead of content
    tool_calls?: Array<{
      id: string;
      name: string;
      arguments: string;
    }>;
    tool_call_id?: string;
    /** Persisted on assistant rows — matches Rust `MessageTokenUsage` */
    token_usage?: {
      input: number;
      output: number;
      total?: number;
      provider?: string;
    };
  }>;
  project_path: string;
  created_at: string;
  updated_at: string;
}

interface SendMessageRequest {
  content: string;
  session_id?: string;
  project_path?: string;
  session_name?: string;
  use_tools: boolean;
  /** `leader` (default) | `bg:<task_id>` to queue follow-up for a background Agent task */
  inputTarget?: string;
  /** DB user row id — truncate after this row and reuse instead of inserting a duplicate user message */
  retryFromUserMessageId?: string;
}

interface MessageResponse {
  message_id: string;
  session_id: string;
  round_id: string;
  /** Persisted SQLite user message row id for this turn */
  user_message_id?: string;
  /** Present when `inputTarget` routed away from the main session */
  input_kind?: string;
}

interface SessionState {
  sessions: Session[];
  currentSession: Session | null;
  messages: Message[];
  isLoading: boolean;
  storeMessages: Message[];
  activeRounds: Map<string, RoundStatus>;
  /** Sessions created with placeholder path — show “pick folder” until set */
  pendingProjectPathSessions: Set<string>;
  createSession: (name: string, projectPath: string) => Promise<void>;
  createSessionQuick: () => Promise<void>;
  updateSessionProjectPath: (
    sessionId: string,
    projectPath: string,
  ) => Promise<void>;
  loadSessions: () => Promise<void>;
  loadSession: (sessionId: string) => Promise<void>;
  deleteSession: (sessionId: string) => Promise<void>;
  renameSession: (sessionId: string, name: string) => Promise<void>;
  setCurrentSession: (sessionId: string | null) => Promise<void>;
  addMessage: (message: Omit<Message, "id"> & { id?: string }) => void;
  /** Replace transcript (user + tools + assistant) so tool rows are not dropped when a turn completes. */
  replaceStoreMessagesSnapshot: (messages: Message[]) => void;
  clearMessages: () => void;
  sendMessage: (request: SendMessageRequest) => Promise<MessageResponse | null>;
  cancelStream: (messageId: string) => Promise<void>;
  updateRoundStatus: (roundId: string, status: RoundStatus) => void;
}

export const useSessionStore = create<SessionState>((set, get) => ({
  sessions: [],
  currentSession: null,
  messages: [],
  storeMessages: [],
  isLoading: false,
  activeRounds: new Map(),
  pendingProjectPathSessions: readPendingProjectPathIds(),

  createSession: async (name, projectPath) => {
    try {
      const sessionData = await invoke<SessionData>("create_session", {
        name,
        projectPath,
      });

      const newSession: Session = {
        id: sessionData.id,
        name: sessionData.name,
        projectPath: sessionData.project_path,
        workingDirectory: projectPath,
        createdAt: sessionData.created_at,
        updatedAt: sessionData.updated_at,
        messageCount: 0,
      };

      set((state) => {
        let pending = state.pendingProjectPathSessions;
        if (projectPath === "." || projectPath === "") {
          pending = new Set(state.pendingProjectPathSessions);
          pending.add(newSession.id);
          persistPendingProjectPathIds(pending);
        }
        return {
          sessions: [newSession, ...state.sessions],
          currentSession: newSession,
          messages: [],
          storeMessages: [],
          pendingProjectPathSessions: pending,
        };
      });
    } catch (error) {
      console.error("Failed to create session:", error);
      throw error;
    }
  },

  createSessionQuick: async () => {
    const { sessions, currentSession, storeMessages } = get();
    const reusable = sessions.find((s) => {
      const isCur = currentSession?.id === s.id;
      return shouldShowNewSessionPlaceholder(s, {
        isCurrentSession: isCur,
        storeMessageCount: isCur ? storeMessages.length : undefined,
      });
    });
    if (reusable) {
      await get().setCurrentSession(reusable.id);
      return;
    }
    await get().createSession(UNUSED_SESSION_LABEL, ".");
  },

  updateSessionProjectPath: async (sessionId, projectPath) => {
    await invoke("update_session_project_path", { sessionId, projectPath });
    set((state) => {
      const pending = new Set(state.pendingProjectPathSessions);
      pending.delete(sessionId);
      persistPendingProjectPathIds(pending);
      const patch = (s: Session) =>
        s.id === sessionId
          ? { ...s, projectPath, workingDirectory: projectPath }
          : s;
      return {
        sessions: state.sessions.map(patch),
        currentSession:
          state.currentSession?.id === sessionId
            ? {
                ...state.currentSession,
                projectPath,
                workingDirectory: projectPath,
              }
            : state.currentSession,
        pendingProjectPathSessions: pending,
      };
    });
  },

  loadSessions: async () => {
    set({ isLoading: true });
    try {
      const sessionsData = await invoke<SessionSummary[]>("list_sessions");

      const sessions: Session[] = sessionsData.map((s) => ({
        id: s.id,
        name: s.name,
        projectPath: ".",
        workingDirectory: ".",
        createdAt: s.updated_at,
        updatedAt: s.updated_at,
        messageCount: s.message_count,
      }));

      set({ sessions, isLoading: false });
    } catch (error) {
      console.error("Failed to load sessions:", error);
      set({ isLoading: false });
    }
  },

  loadSession: async (sessionId: string) => {
    // Do not toggle global `isLoading` here — it hides the entire SessionList skeleton
    // and feels like a blank UI while switching chats.
    dbg("loadSession:start", { sessionId });
    try {
      const sessionData = await invoke<SessionData>("load_session", {
        sessionId,
      });

      const rawMessages = sessionData.messages ?? [];

      /** Map tool_use_id -> name/arguments from assistant rows so persisted `tool` rows can show real tool names after reload */
      const toolMetaById = new Map<
        string,
        { name: string; arguments: string }
      >();
      for (const m of rawMessages) {
        if (m.role !== "assistant" || !m.tool_calls?.length) continue;
        for (const tc of m.tool_calls) {
          toolMetaById.set(tc.id, {
            name: tc.name,
            arguments: tc.arguments,
          });
        }
      }

      const session: Session = {
        id: sessionData.id,
        name: sessionData.name,
        projectPath: sessionData.project_path,
        workingDirectory: sessionData.project_path,
        createdAt: sessionData.created_at,
        updatedAt: sessionData.updated_at,
        messageCount: rawMessages.length,
      };

      // Convert messages - handle different field names for different roles
      const messages: Message[] = rawMessages.map((m, index) => {
        // Tool messages have `output` field instead of `content`
        const content =
          m.role === "tool" && m.output !== undefined && m.output !== null
            ? m.output
            : m.content || "";

        let toolCallsList: Message["toolCallsList"] = undefined;
        let toolCall = undefined;

        if (m.tool_calls && m.tool_calls.length > 0) {
          toolCallsList = m.tool_calls.map((tc) => ({
            id: tc.id,
            name: tc.name,
            arguments: tc.arguments,
          }));
          const tc = m.tool_calls[0];
          toolCall = {
            id: tc.id,
            name: tc.name,
            arguments: tc.arguments,
          };
        } else if (m.role === "tool") {
          const tid = (m.tool_call_id ?? "").trim();
          const meta = tid ? toolMetaById.get(tid) : undefined;
          toolCall = {
            id: tid || `tool-row-${index}`,
            name: meta?.name ?? "tool",
            arguments: meta?.arguments ?? "",
            output: m.output ?? content,
          };
        }

        const tokenUsage: Message["tokenUsage"] =
          m.role === "assistant" && m.token_usage
            ? {
                input: m.token_usage.input,
                output: m.token_usage.output,
                total: m.token_usage.total,
                provider: m.token_usage.provider,
              }
            : undefined;

        return sanitizeMessageForPersistence({
          id: m.id ?? `${sessionId}-msg-${index}`,
          role: m.role,
          content,
          toolCallsList,
          toolCall,
          ...(tokenUsage ? { tokenUsage } : {}),
        });
      });

      set((state) => {
        const exists = state.sessions.some((s) => s.id === session.id);
        const sessions = exists
          ? state.sessions.map((s) =>
              s.id === session.id ? { ...s, ...session } : s,
            )
          : [session, ...state.sessions];
        return {
          currentSession: session,
          messages,
          storeMessages: messages,
          isLoading: false,
          sessions,
        };
      });
      dbg("loadSession:ok", { sessionId, messageCount: messages.length });
      notifyProviderChanged();
    } catch (error) {
      console.error("[OmigaDebug] loadSession failed", error);
      const fallback = get().sessions.find((s) => s.id === sessionId) ?? null;
      dbg("loadSession:error", {
        sessionId,
        fallbackName: fallback?.name ?? null,
      });
      set({
        isLoading: false,
        currentSession: fallback,
        messages: [],
        storeMessages: [],
      });
    }
  },

  deleteSession: async (sessionId: string) => {
    try {
      await invoke("delete_session", { sessionId });

      set((state) => {
        const newSessions = state.sessions.filter((s) => s.id !== sessionId);
        const newCurrentSession =
          state.currentSession?.id === sessionId ? null : state.currentSession;
        const pending = new Set(state.pendingProjectPathSessions);
        pending.delete(sessionId);
        persistPendingProjectPathIds(pending);

        return {
          sessions: newSessions,
          currentSession: newCurrentSession,
          messages: newCurrentSession ? state.messages : [],
          storeMessages: newCurrentSession ? state.storeMessages : [],
          pendingProjectPathSessions: pending,
        };
      });
    } catch (error) {
      console.error("Failed to delete session:", error);
    }
  },

  renameSession: async (sessionId: string, name: string) => {
    try {
      await invoke("rename_session", { sessionId, name });

      set((state) => ({
        sessions: state.sessions.map((s) =>
          s.id === sessionId ? { ...s, name } : s,
        ),
        currentSession:
          state.currentSession?.id === sessionId
            ? { ...state.currentSession, name }
            : state.currentSession,
      }));
    } catch (error) {
      console.error("Failed to rename session:", error);
    }
  },

  setCurrentSession: async (sessionId) => {
    dbg("setCurrentSession", { sessionId });
    if (!sessionId) {
      set({ currentSession: null, messages: [], storeMessages: [] });
      return;
    }
    const session = get().sessions.find((s) => s.id === sessionId);
    const now = new Date().toISOString();
    const placeholder: Session = {
      id: sessionId,
      name: "Loading…",
      projectPath: ".",
      workingDirectory: ".",
      createdAt: now,
      updatedAt: now,
    };
    // Clear stale messages immediately so Chat never shows the previous session's history
    set({
      currentSession: session ?? placeholder,
      messages: [],
      storeMessages: [],
    });
    await get().loadSession(sessionId);
    dbg("setCurrentSession:done", { sessionId });
  },

  addMessage: (message) => {
    const newMessage: Message = sanitizeMessageForPersistence({
      ...message,
      id: message.id ?? `msg-${Date.now()}`,
    });
    set((state) => ({
      messages: [...state.messages, newMessage],
      storeMessages: [...state.storeMessages, newMessage],
    }));
  },

  replaceStoreMessagesSnapshot: (messages) => {
    const cleaned = messages.map(sanitizeMessageForPersistence);
    set({ storeMessages: cleaned, messages: cleaned });
  },

  clearMessages: () => {
    set({ messages: [], storeMessages: [] });
  },

  sendMessage: async (request: SendMessageRequest) => {
    const response = await invoke<MessageResponse>("send_message", {
      request,
    });

    // Main-session rounds only (not queued follow-ups to background agents)
    if (response.input_kind !== "background_followup_queued") {
      const { activeRounds } = get();
      activeRounds.set(response.round_id, "running");
      set({ activeRounds });
    }

    return response;
  },

  cancelStream: async (messageId: string) => {
    try {
      await invoke("cancel_stream", { messageId });
      // Caller updates round status via updateRoundStatus when needed.
    } catch (error) {
      console.error("Failed to cancel stream:", error);
    }
  },

  updateRoundStatus: (roundId: string, status: RoundStatus) => {
    const { activeRounds } = get();
    activeRounds.set(roundId, status);
    set({ activeRounds });

    // Also update any message with this roundId
    set((state) => ({
      messages: state.messages.map((msg) =>
        msg.roundId === roundId ? { ...msg, roundStatus: status } : msg,
      ),
    }));
  },
}));
