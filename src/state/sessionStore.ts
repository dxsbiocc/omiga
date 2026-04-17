import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import { notifyProviderChanged } from "../utils/providerEvents";
import {
  useChatComposerStore,
  type SessionConfigResponse,
} from "./chatComposerStore";

const dbg = (...args: unknown[]) =>
  console.debug("[OmigaDebug][sessionStore]", ...args);

// ── Session message cache ────────────────────────────────────────────────────
// Module-level (outside Zustand) so cache ops never trigger React re-renders.
// On a cache hit, setCurrentSession shows content instantly (~0ms) and fires
// a silent background IPC refresh to pick up any new messages.
//
// Root cause for slow IPC: Tauri WKWebView (macOS) routes invoke() through
// NSURLProtocol; while the WebView is rendering, calls queue up and can take
// 200-800ms regardless of payload size. Caching bypasses this entirely.

const MSG_CACHE_TTL_MS = 10 * 60 * 1_000; // 10 min — keep frequently-used sessions warm longer
const MSG_CACHE_MAX_SESSIONS = 40;

interface CachedSession {
  session: Session;
  messages: Message[];
  hasMoreMessages: boolean;
  activeProviderEntryName: string | null;
  sessionConfig: SessionConfigResponse;
  cachedAt: number; // Date.now()
}

const _msgCache = new Map<string, CachedSession>();

function msgCacheGet(sessionId: string): CachedSession | null {
  const entry = _msgCache.get(sessionId);
  if (!entry) return null;
  if (Date.now() - entry.cachedAt > MSG_CACHE_TTL_MS) {
    _msgCache.delete(sessionId);
    return null;
  }
  return entry;
}

function msgCacheSet(sessionId: string, data: Omit<CachedSession, "cachedAt">) {
  if (_msgCache.size >= MSG_CACHE_MAX_SESSIONS && !_msgCache.has(sessionId)) {
    // Evict the oldest entry (Map preserves insertion order)
    const oldest = _msgCache.keys().next().value;
    if (oldest !== undefined) _msgCache.delete(oldest);
  }
  _msgCache.set(sessionId, { ...data, cachedAt: Date.now() });
}

function msgCacheInvalidate(sessionId: string) {
  _msgCache.delete(sessionId);
}

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
export function isPlaceholderSessionTitle(
  name: string | undefined | null,
): boolean {
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
  project_path: string;
  message_count: number;
  updated_at: string;
}

interface RawMessage {
  role: "user" | "assistant" | "tool";
  /** SQLite `messages.id` when present */
  id?: string;
  content?: string;
  output?: string;
  tool_calls?: Array<{ id: string; name: string; arguments: string }>;
  tool_call_id?: string;
  token_usage?: {
    input: number;
    output: number;
    total?: number;
    provider?: string;
  };
  /** Follow-up suggestions from backend (LLM-generated) */
  follow_up_suggestions?: Array<{ label: string; prompt: string }>;
}

interface SessionData {
  id: string;
  name: string;
  messages: RawMessage[];
  project_path: string;
  created_at: string;
  updated_at: string;
  active_provider_entry_name: string | null;
  has_more_messages: boolean;
  session_config: SessionConfigResponse;
}

interface SendMessageRequest {
  content: string;
  session_id?: string;
  project_path?: string;
  session_name?: string;
  use_tools: boolean;
  /** `leader` (default) | `bg:<task_id>` to queue follow-up for a background Agent task */
  inputTarget?: string;
  /** `local` | `ssh` | `sandbox` — chat composer execution surface */
  executionEnvironment?: "local" | "ssh" | "sandbox";
  /** Selected SSH server name; used when executionEnvironment === "ssh" */
  sshServer?: string | null;
  /** `modal` | `daytona` | `docker` | `singularity` — composer sandbox backend */
  sandboxBackend?: string;
  /** `"none"` | `"conda"` | `"venv"` | `"pyenv"` — local virtual env type */
  localVenvType?: string;
  /** Conda env name, venv directory path, or pyenv version string */
  localVenvName?: string;
  /** Specialist agent id from list_available_agents (e.g. Explore, Plan) */
  composerAgentType?: string;
  /** `ask` | `auto` | `bypass` — user-facing permission stance for this turn */
  permissionMode?: string;
  /** DB user row id — truncate after this row and reuse instead of inserting a duplicate user message */
  retryFromUserMessageId?: string;
  /** Session's stored provider entry name — passed through for lazy LLM config restoration. */
  activeProviderEntryName?: string | null;
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
  /** True while a session switch is in-flight (loadSession pending).
   *  Chat uses this to overlay a loading indicator instead of going blank. */
  isSwitchingSession: boolean;
  /** True when the current session has older messages not yet loaded (pagination). */
  hasMoreMessages: boolean;
  /** True while `loadMoreMessages` is fetching older messages. */
  isLoadingMoreMessages: boolean;
  storeMessages: Message[];
  activeRounds: Map<string, RoundStatus>;
  /** Provider entry name for the current session (from DB).  Used by ProviderSwitcher
   *  to show the correct chip without an extra round-trip after session switch. */
  activeProviderEntryName: string | null;
  /** Sessions created with placeholder path — show “pick folder” until set */
  pendingProjectPathSessions: Set<string>;
  createSession: (name: string, projectPath: string) => Promise<void>;
  createSessionQuick: () => Promise<void>;
  updateSessionProjectPath: (
    sessionId: string,
    projectPath: string,
  ) => Promise<void>;
  loadSessions: () => Promise<void>;
  loadSession: (sessionId: string, opts?: { silent?: boolean }) => Promise<void>;
  /** Prepend older messages for the current session (scroll-to-top pagination). */
  loadMoreMessages: () => Promise<void>;
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

/** Convert raw DB message rows to store Messages. Extracted so both loadSession
 *  and loadMoreMessages can share the same conversion logic. */
function convertRawMessages(
  rawMessages: RawMessage[],
  sessionId: string,
): Message[] {
  // Build tool_call_id → {name, arguments} map from assistant rows in O(N).
  const toolMetaById = new Map<string, { name: string; arguments: string }>();
  for (const m of rawMessages) {
    if (m.role !== "assistant" || !m.tool_calls?.length) continue;
    for (const tc of m.tool_calls) {
      toolMetaById.set(tc.id, { name: tc.name, arguments: tc.arguments });
    }
  }

  return rawMessages.map((m, index) => {
    const content =
      m.role === "tool" && m.output != null ? m.output : m.content || "";

    let toolCallsList: Message["toolCallsList"];
    let toolCall: Message["toolCall"];

    if (m.tool_calls?.length) {
      toolCallsList = m.tool_calls.map((tc) => ({
        id: tc.id,
        name: tc.name,
        arguments: tc.arguments,
      }));
      const tc = m.tool_calls[0];
      toolCall = { id: tc.id, name: tc.name, arguments: tc.arguments };
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

    const followUpSuggestions: Message["followUpSuggestions"] =
      m.role === "assistant" && m.follow_up_suggestions?.length
        ? m.follow_up_suggestions
        : undefined;

    return sanitizeMessageForPersistence({
      id: m.id ?? `${sessionId}-msg-${index}`,
      role: m.role,
      content,
      toolCallsList,
      toolCall,
      ...(tokenUsage ? { tokenUsage } : {}),
      ...(followUpSuggestions ? { followUpSuggestions } : {}),
    });
  });
}

export const useSessionStore = create<SessionState>((set, get) => ({
  sessions: [],
  currentSession: null,
  messages: [],
  storeMessages: [],
  isLoading: false,
  isSwitchingSession: false,
  hasMoreMessages: false,
  isLoadingMoreMessages: false,
  activeRounds: new Map(),
  activeProviderEntryName: null,
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
      useChatComposerStore.getState().initForSession(
        sessionData.id,
        sessionData.session_config,
      );
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
        projectPath: s.project_path || ".",
        workingDirectory: s.project_path || ".",
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

  loadSession: async (sessionId: string, opts?: { clearSwitching?: boolean; silent?: boolean }) => {
    const perfStart = performance.now();
    dbg("loadSession:start", { sessionId, silent: opts?.silent });
    // T1: IPC call starts
    if (!opts?.silent) performance.mark("sw:ipc-start");
    try {
      const sessionData = await invoke<SessionData>("load_session", {
        sessionId,
      });
      // T2: IPC call done
      const ipcMs = Math.round(performance.now() - perfStart);
      if (!opts?.silent) {
        performance.mark("sw:ipc-done");
        try { performance.measure("sw: IPC round-trip", "sw:ipc-start", "sw:ipc-done"); } catch { /**/ }
      }

      const currentSessionId = get().currentSession?.id ?? null;
      if (!opts?.silent && currentSessionId !== sessionId) {
        dbg("loadSession:stale-foreground-skip", {
          sessionId,
          currentSessionId,
        });
        return;
      }

      const messages = convertRawMessages(
        sessionData.messages ?? [],
        sessionId,
      );
      const session: Session = {
        id: sessionData.id,
        name: sessionData.name,
        projectPath: sessionData.project_path,
        workingDirectory: sessionData.project_path,
        createdAt: sessionData.created_at,
        updatedAt: sessionData.updated_at,
        messageCount: messages.length,
      };

      const newActiveProvider = sessionData.active_provider_entry_name ?? null;
      // Write to cache so subsequent switches to this session are instant.
      msgCacheSet(sessionId, {
        session,
        messages,
        hasMoreMessages: sessionData.has_more_messages,
        activeProviderEntryName: newActiveProvider,
        sessionConfig: sessionData.session_config,
      });

      if (opts?.silent) {
        // Silent background refresh: only update if this session is still current
        // and the loaded data is different (new messages arrived).
        const cur = get().currentSession;
        if (cur?.id === sessionId) {
          set((state) => {
            const exists = state.sessions.some((s) => s.id === session.id);
            const sessions = exists
              ? state.sessions.map((s) => s.id === session.id ? { ...s, ...session } : s)
              : [session, ...state.sessions];
            return {
              messages,
              storeMessages: messages,
              hasMoreMessages: sessionData.has_more_messages,
              activeProviderEntryName: newActiveProvider,
              currentSession: session,
              sessions,
            };
          });
          notifyProviderChanged();
          // Sync composer config only while this session is still active.
          useChatComposerStore
            .getState()
            .initForSession(sessionId, sessionData.session_config);
        }
        dbg("loadSession:silent-refresh-ok", { sessionId, msgs: messages.length });
        return;
      }

      // T3: Zustand state set (React reconciliation starts after this)
      const jsConvertMs = Math.round(performance.now() - perfStart) - ipcMs;
      if (!opts?.silent) {
        performance.mark("sw:state-set");
        try { performance.measure("sw: JS conversion", "sw:ipc-done", "sw:state-set"); } catch { /**/ }
      }
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
          hasMoreMessages: sessionData.has_more_messages,
          activeProviderEntryName: newActiveProvider,
          sessions,
        };
      });
      // Log store-side breakdown (React render time logged separately in Chat component)
      const clickAt = (window as unknown as { __swClickAt?: number }).__swClickAt;
      const t0ToStore = clickAt != null ? Math.round(performance.now() - clickAt) : "?";
      console.info(
        `%c[SwPerf] ${sessionId.slice(0, 8)} | click→store: ${t0ToStore}ms | IPC: ${ipcMs}ms | JS-conv: ${jsConvertMs}ms | msgs: ${messages.length}`,
        "color:#6c9ef8;font-weight:bold",
      );

      // Sync composer config so the latest per-session settings are applied.
      useChatComposerStore.getState().initForSession(sessionId, sessionData.session_config);

      // Always notify so ProviderSwitcher refreshes its "active" chip for the new session.
      notifyProviderChanged();

      // Fire-and-forget: pre-warm LLM config / integrations / permission / MCP caches
      // so the first send_message in this session doesn't pay cold-cache penalties.
      // Inspired by codex's startup prewarm pattern.
      void invoke("prewarm_session", {
        projectPath: session.projectPath,
        activeProviderEntryName: newActiveProvider,
      }).catch(() => {
        // Prewarm is best-effort — silently ignore errors.
      });

      const duration = Math.round(performance.now() - perfStart);
      dbg("loadSession:ok", {
        sessionId,
        messageCount: messages.length,
        duration,
      });
    } catch (error) {
      console.error("[OmigaDebug] loadSession failed", error);
      const fallback = get().sessions.find((s) => s.id === sessionId) ?? null;
      dbg("loadSession:error", {
        sessionId,
        fallbackName: fallback?.name ?? null,
      });
      if (get().currentSession?.id === sessionId) {
        set({
          isLoading: false,
          isSwitchingSession: false,
          currentSession: fallback,
          messages: [],
          storeMessages: [],
          hasMoreMessages: false,
        });
      }
    }
  },

  loadMoreMessages: async () => {
    const { currentSession, messages, hasMoreMessages, isLoadingMoreMessages } =
      get();
    if (!currentSession || !hasMoreMessages || isLoadingMoreMessages) return;
    const oldestId = messages[0]?.id;
    if (!oldestId) return;
    set({ isLoadingMoreMessages: true });
    try {
      const raw = await invoke<RawMessage[]>("load_more_messages", {
        sessionId: currentSession.id,
        beforeId: oldestId,
      });
      if (!raw.length) {
        set({ hasMoreMessages: false, isLoadingMoreMessages: false });
        return;
      }
      const older = convertRawMessages(raw, currentSession.id);
      set((state) => ({
        messages: [...older, ...state.messages],
        storeMessages: [...older, ...state.storeMessages],
        // Backend page size is 30 — fewer results means we've reached the beginning.
        hasMoreMessages: raw.length >= 30,
        isLoadingMoreMessages: false,
      }));
    } catch (e) {
      console.error("[OmigaDebug] loadMoreMessages failed", e);
      set({ isLoadingMoreMessages: false });
    }
  },

  deleteSession: async (sessionId: string) => {
    msgCacheInvalidate(sessionId);
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
    const t0 = performance.now();
    dbg("setCurrentSession", { sessionId });
    if (!sessionId) {
      set({
        currentSession: null,
        messages: [],
        storeMessages: [],
        isSwitchingSession: false,
      });
      useChatComposerStore.getState().resetToDefaults();
      return;
    }

    // ── Cache hit: show content instantly, refresh in background ─────────────
    const cached = msgCacheGet(sessionId);
    if (cached) {
      set({
        currentSession: cached.session,
        messages: cached.messages,
        storeMessages: cached.messages,
        hasMoreMessages: cached.hasMoreMessages,
        activeProviderEntryName: cached.activeProviderEntryName,
        isSwitchingSession: false,
      });
      useChatComposerStore.getState().initForSession(sessionId, cached.sessionConfig);
      notifyProviderChanged();
      const hitMs = Math.round(performance.now() - t0);
      console.info(
        `%c[SwPerf] ${sessionId.slice(0, 8)} | CACHE HIT ~${hitMs}ms | msgs: ${cached.messages.length} — background refresh…`,
        "color:#4caf50;font-weight:bold",
      );
      // Silent background refresh: picks up any messages that arrived since cache
      void get().loadSession(sessionId, { silent: true }).catch(() => {});
      dbg("setCurrentSession:cache-hit", { sessionId });
      return;
    }

    // ── Cache miss: instant UI update, load messages in background ───────────
    // We already have session metadata from list_sessions, so we can show the
    // header + sidebar selection immediately instead of waiting for the DB round-trip.
    // The chat area simply goes blank briefly while messages load (~200-800 ms on macOS IPC).
    const session = get().sessions.find((s) => s.id === sessionId);
    const now = new Date().toISOString();
    const fallbackSession: Session = {
      id: sessionId,
      name: session?.name || "Loading…",
      projectPath: session?.projectPath || ".",
      workingDirectory: session?.workingDirectory || ".",
      createdAt: session?.createdAt || now,
      updatedAt: session?.updatedAt || now,
    };
    // Use the real metadata when available so the header never flashes "Loading…"
    const immediateSession = session ? { ...session, id: sessionId } : fallbackSession;
    set({
      currentSession: immediateSession,
      messages: [],
      storeMessages: [],
      isSwitchingSession: false,
    });
    notifyProviderChanged();

    // Load per-session config immediately so composer settings don't flash stale values.
    void invoke<SessionConfigResponse>("get_session_config", { sessionId })
      .then((cfg) => {
        if (get().currentSession?.id !== sessionId) return;
        useChatComposerStore.getState().initForSession(sessionId, cfg);
      })
      .catch(() => {
        if (get().currentSession?.id !== sessionId) return;
        useChatComposerStore.getState().initForSession(sessionId, {
          active_provider_entry_name: null,
          permission_mode: "auto",
          composer_agent_type: "auto",
          execution_environment: "local",
          ssh_server: null,
          sandbox_backend: "docker",
          local_venv_type: "none",
          local_venv_name: "",
          use_worktree: false,
        });
      });

    void get()
      .loadSession(sessionId)
      .catch((error) => {
        console.error("[OmigaDebug] loadSession from setCurrentSession failed", error);
      })
      .finally(() => {
        const missMs = Math.round(performance.now() - t0);
        console.info(
          `%c[SwPerf] ${sessionId.slice(0, 8)} | CACHE MISS total: ${missMs}ms`,
          "color:#ff9800;font-weight:bold",
        );
        dbg("setCurrentSession:done", { sessionId, missMs });
      });
  },

  addMessage: (message) => {
    const newMessage: Message = sanitizeMessageForPersistence({
      ...message,
      id: message.id ?? `msg-${Date.now()}`,
    });
    set((state) => {
      const messages = [...state.messages, newMessage];
      // Keep cache in sync so switching away and back shows the latest messages.
      if (state.currentSession) {
        const cached = _msgCache.get(state.currentSession.id);
        if (cached) {
          _msgCache.set(state.currentSession.id, {
            ...cached,
            messages,
            cachedAt: Date.now(),
          });
        }
      }
      return { messages, storeMessages: messages };
    });
  },

  replaceStoreMessagesSnapshot: (messages) => {
    const cleaned = messages.map(sanitizeMessageForPersistence);
    set((state) => {
      if (state.currentSession) {
        const cached = _msgCache.get(state.currentSession.id);
        if (cached) {
          _msgCache.set(state.currentSession.id, {
            ...cached,
            messages: cleaned,
            cachedAt: Date.now(),
          });
        }
      }
      return { storeMessages: cleaned, messages: cleaned };
    });
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
