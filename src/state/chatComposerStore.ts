import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";

/** 主会话工具/编辑确认方式（与 Agent 类型独立）。 */
export type PermissionMode = "ask" | "auto" | "bypass";

/** Computer Use 显式开启范围：默认关闭，task 发送后自动回到 off。 */
export type ComputerUseMode = "off" | "task" | "session";

/** User-facing sandbox backends with real local integrations. */
export type SandboxBackend = "docker" | "singularity";

/** 本地虚拟环境类型 */
export type LocalVenvType = "none" | "conda" | "venv" | "pyenv";

/** 执行环境类型 */
export type ExecutionEnvironment = "local" | "ssh" | "sandbox";

export function executionWorkspaceScopeKey(
  environment: ExecutionEnvironment,
  sshServer: string | null,
  sandboxBackend: SandboxBackend,
): string {
  if (environment === "ssh") {
    return `ssh:${(sshServer ?? "").trim()}`;
  }
  if (environment === "sandbox") {
    return `sandbox:${sandboxBackend}`;
  }
  return "local";
}

export function shouldResetWorkspaceForExecutionScopeChange(
  previousScopeKey: string,
  nextScopeKey: string,
): boolean {
  return previousScopeKey !== nextScopeKey;
}

export interface SessionConfigResponse {
  active_provider_entry_name: string | null;
  permission_mode: string;
  composer_agent_type: string;
  execution_environment: string;
  ssh_server: string | null;
  sandbox_backend: string;
  local_venv_type: string;
  local_venv_name: string;
  use_worktree: boolean;
  runtime_constraints?: unknown;
}

export const DEFAULT_SESSION_CONFIG: SessionConfigResponse = {
  active_provider_entry_name: null,
  permission_mode: "auto",
  composer_agent_type: "auto",
  execution_environment: "local",
  ssh_server: null,
  sandbox_backend: "docker",
  local_venv_type: "none",
  local_venv_name: "",
  use_worktree: false,
};

function asString(v: unknown, fallback: string): string {
  return typeof v === "string" ? v : fallback;
}

function asNullableString(v: unknown): string | null {
  return typeof v === "string" ? v : null;
}

function normalizeSandboxBackendValue(v: unknown): SandboxBackend {
  const value = typeof v === "string" ? v.trim().toLowerCase() : "";
  return value === "singularity" ? "singularity" : "docker";
}

export function normalizeSessionConfig(
  cfg?: Partial<SessionConfigResponse> | null,
): SessionConfigResponse {
  return {
    active_provider_entry_name: asNullableString(cfg?.active_provider_entry_name),
    permission_mode: asString(
      cfg?.permission_mode,
      DEFAULT_SESSION_CONFIG.permission_mode,
    ),
    composer_agent_type: asString(
      cfg?.composer_agent_type,
      DEFAULT_SESSION_CONFIG.composer_agent_type,
    ),
    execution_environment: asString(
      cfg?.execution_environment,
      DEFAULT_SESSION_CONFIG.execution_environment,
    ),
    ssh_server: asNullableString(cfg?.ssh_server),
    sandbox_backend: normalizeSandboxBackendValue(cfg?.sandbox_backend),
    local_venv_type: asString(
      cfg?.local_venv_type,
      DEFAULT_SESSION_CONFIG.local_venv_type,
    ),
    local_venv_name: asString(
      cfg?.local_venv_name,
      DEFAULT_SESSION_CONFIG.local_venv_name,
    ),
    // Worktree mode is intentionally hidden for the focused research UI.
    // Keep the response field for backward compatibility, but never re-enable it.
    use_worktree: false,
    runtime_constraints: cfg?.runtime_constraints,
  };
}

interface ChatComposerState {
  permissionMode: PermissionMode;
  computerUseMode: ComputerUseMode;
  /** 注册表中的 Agent id，如 Explore、Plan、general-purpose */
  composerAgentType: string;
  /** `@` 选择器选中的工作区相对路径（仅内存，不持久化） */
  composerAttachedPaths: string[];
  /** `@` 选择器选中的 Omiga 插件 ID（仅本轮内存态，不持久化） */
  composerSelectedPluginIds: string[];
  /** 执行环境：本地、SSH、沙箱 */
  environment: ExecutionEnvironment;
  /** SSH 服务器名称；仅在 `environment === "ssh"` 时生效。 */
  sshServer: string | null;
  /** 沙箱执行后端；仅在 `environment === "sandbox"` 时生效。 */
  sandboxBackend: SandboxBackend;
  /** 本地虚拟环境类型；仅在 `environment === "local"` 时生效。 */
  localVenvType: LocalVenvType;
  /** 虚拟环境名称/路径：conda env 名、venv 目录路径、pyenv 版本号。 */
  localVenvName: string;
  /** Remembered branch choice per workspace root path */
  selectedBranchByRoot: Record<string, string>;
  /** Currently active session ID used for lazy save */
  activeSessionId: string | null;
  setPermissionMode: (m: PermissionMode) => void;
  setComputerUseMode: (m: ComputerUseMode) => void;
  resetTaskComputerUseMode: () => void;
  setComposerAgentType: (t: string) => void;
  addComposerAttachedPath: (relativePath: string) => void;
  removeComposerAttachedPath: (relativePath: string) => void;
  popComposerAttachedPath: () => void;
  clearComposerAttachedPaths: () => void;
  addComposerSelectedPluginId: (pluginId: string) => void;
  removeComposerSelectedPluginId: (pluginId: string) => void;
  popComposerSelectedPluginId: () => void;
  clearComposerSelectedPluginIds: () => void;
  setEnvironment: (e: ExecutionEnvironment) => void;
  setSshServer: (name: string | null) => void;
  setSandboxBackend: (b: SandboxBackend) => void;
  setLocalVenv: (type: LocalVenvType, name: string) => void;
  setBranchForRoot: (root: string, branch: string) => void;
  /** Initialize composer state for a specific session (called on session switch). */
  initForSession: (
    sessionId: string,
    cfg?: Partial<SessionConfigResponse> | null,
  ) => void;
  /** Reset to defaults when no session is active. */
  resetToDefaults: () => void;
}

function defaults() {
  return {
    permissionMode: "auto" as PermissionMode,
    computerUseMode: "off" as ComputerUseMode,
    composerAgentType: "auto",
    composerAttachedPaths: [] as string[],
    composerSelectedPluginIds: [] as string[],
    environment: "local" as ExecutionEnvironment,
    sshServer: null as string | null,
    sandboxBackend: "docker" as SandboxBackend,
    localVenvType: "none" as LocalVenvType,
    localVenvName: "",
  };
}

async function saveSessionConfig(
  sessionId: string,
  state: Omit<ChatComposerState, keyof {
    setPermissionMode: unknown;
    setComputerUseMode: unknown;
    resetTaskComputerUseMode: unknown;
    setComposerAgentType: unknown;
    addComposerAttachedPath: unknown;
    removeComposerAttachedPath: unknown;
    popComposerAttachedPath: unknown;
    clearComposerAttachedPaths: unknown;
    addComposerSelectedPluginId: unknown;
    removeComposerSelectedPluginId: unknown;
    popComposerSelectedPluginId: unknown;
    clearComposerSelectedPluginIds: unknown;
    setEnvironment: unknown;
    setSshServer: unknown;
    setSandboxBackend: unknown;
    setLocalVenv: unknown;
    setBranchForRoot: unknown;
    initForSession: unknown;
    resetToDefaults: unknown;
    activeSessionId: unknown;
    selectedBranchByRoot: unknown;
    composerAttachedPaths: unknown;
    composerSelectedPluginIds: unknown;
  }>,
) {
  try {
    await invoke("save_session_config_command", {
      sessionId,
      config: {
        active_provider_entry_name: null,
        permission_mode: state.permissionMode,
        composer_agent_type: state.composerAgentType,
        execution_environment: state.environment,
        ssh_server: state.sshServer,
        sandbox_backend: state.sandboxBackend,
        local_venv_type: state.localVenvType,
        local_venv_name: state.localVenvName,
        use_worktree: false,
      },
    });
  } catch (e) {
    console.error("[OmigaDebug] Failed to save session config:", e);
  }
}

async function syncSessionPermissionStance(
  sessionId: string,
  permissionMode: PermissionMode,
) {
  try {
    await invoke("permission_set_session_stance", {
      sessionId,
      stance: permissionMode,
    });
  } catch (e) {
    console.error("[OmigaDebug] Failed to sync permission stance:", e);
  }
}

export const useChatComposerStore = create<ChatComposerState>((set, get) => ({
  ...defaults(),
  selectedBranchByRoot: {},
  activeSessionId: null,

  setPermissionMode: (permissionMode) => {
    set({ permissionMode });
    const { activeSessionId } = get();
    if (activeSessionId) {
      void saveSessionConfig(activeSessionId, get());
      void syncSessionPermissionStance(activeSessionId, permissionMode);
    }
  },
  setComputerUseMode: (computerUseMode) => {
    set({ computerUseMode });
  },
  resetTaskComputerUseMode: () => {
    if (get().computerUseMode === "task") set({ computerUseMode: "off" });
  },
  setComposerAgentType: (composerAgentType) => {
    set({ composerAgentType });
    const { activeSessionId } = get();
    if (activeSessionId) saveSessionConfig(activeSessionId, get());
  },
  addComposerAttachedPath: (relativePath) =>
    set((s) => {
      const t = relativePath
        .trim()
        .replace(/^@(?:file:)?/u, "")
        .replace(/\\/g, "/")
        .replace(/\/{2,}/g, "/")
        .replace(/(.+)\/+$/u, "$1");
      if (!t || s.composerAttachedPaths.includes(t)) return s;
      return {
        composerAttachedPaths: [...s.composerAttachedPaths, t],
      };
    }),
  removeComposerAttachedPath: (relativePath) =>
    set((s) => ({
      composerAttachedPaths: s.composerAttachedPaths.filter(
        (p) => p !== relativePath,
      ),
    })),
  popComposerAttachedPath: () =>
    set((s) => ({
      composerAttachedPaths: s.composerAttachedPaths.slice(0, -1),
    })),
  clearComposerAttachedPaths: () => set({ composerAttachedPaths: [] }),
  addComposerSelectedPluginId: (pluginId) =>
    set((s) => {
      const t = pluginId.trim();
      if (!t || s.composerSelectedPluginIds.includes(t)) return s;
      return {
        composerSelectedPluginIds: [...s.composerSelectedPluginIds, t],
      };
    }),
  removeComposerSelectedPluginId: (pluginId) =>
    set((s) => ({
      composerSelectedPluginIds: s.composerSelectedPluginIds.filter(
        (id) => id !== pluginId,
      ),
    })),
  popComposerSelectedPluginId: () =>
    set((s) => ({
      composerSelectedPluginIds: s.composerSelectedPluginIds.slice(0, -1),
    })),
  clearComposerSelectedPluginIds: () => set({ composerSelectedPluginIds: [] }),
  setEnvironment: (environment) => {
    set({ environment });
    const { activeSessionId } = get();
    if (activeSessionId) saveSessionConfig(activeSessionId, get());
  },
  setSshServer: (sshServer) => {
    set({ sshServer });
    const { activeSessionId } = get();
    if (activeSessionId) saveSessionConfig(activeSessionId, get());
  },
  setSandboxBackend: (sandboxBackend) => {
    set({ sandboxBackend });
    const { activeSessionId } = get();
    if (activeSessionId) saveSessionConfig(activeSessionId, get());
  },
  setLocalVenv: (localVenvType, localVenvName) => {
    set({ localVenvType, localVenvName });
    const { activeSessionId } = get();
    if (activeSessionId) saveSessionConfig(activeSessionId, get());
  },
  setBranchForRoot: (root, branch) =>
    set((s) => ({
      selectedBranchByRoot: { ...s.selectedBranchByRoot, [root]: branch },
    })),

  initForSession: (sessionId, cfg) => {
    const normalized = normalizeSessionConfig(cfg);
    set({
      activeSessionId: sessionId,
      permissionMode: normalized.permission_mode as PermissionMode,
      computerUseMode: "off",
      composerAgentType: normalized.composer_agent_type || "auto",
      environment: normalized.execution_environment as ExecutionEnvironment,
      sshServer: normalized.ssh_server,
      sandboxBackend: normalized.sandbox_backend as SandboxBackend,
      localVenvType: normalized.local_venv_type as LocalVenvType,
      localVenvName: normalized.local_venv_name,
      // Keep one-turn picker selections empty on switch
      composerAttachedPaths: [],
      composerSelectedPluginIds: [],
    });
  },

  resetToDefaults: () => {
    set({
      ...defaults(),
      activeSessionId: null,
      computerUseMode: "off",
      composerAttachedPaths: [],
      composerSelectedPluginIds: [],
    });
  },
}));
