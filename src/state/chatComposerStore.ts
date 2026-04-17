import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";

/** 主会话工具/编辑确认方式（与 Agent 类型独立）。 */
export type PermissionMode = "ask" | "auto" | "bypass";

/** 与 `omiga/src-tauri/src/execution/types.rs` `EnvironmentType` 对齐（不含 Local）。 */
export type SandboxBackend = "modal" | "daytona" | "docker" | "singularity";

/** 本地虚拟环境类型 */
export type LocalVenvType = "none" | "conda" | "venv" | "pyenv";

/** 执行环境类型 */
export type ExecutionEnvironment = "local" | "ssh" | "sandbox";

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

interface ChatComposerState {
  permissionMode: PermissionMode;
  /** 注册表中的 Agent id，如 Explore、Plan、general-purpose */
  composerAgentType: string;
  /** `@` 选择器选中的工作区相对路径（仅内存，不持久化） */
  composerAttachedPaths: string[];
  useWorktree: boolean;
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
  setComposerAgentType: (t: string) => void;
  addComposerAttachedPath: (relativePath: string) => void;
  removeComposerAttachedPath: (relativePath: string) => void;
  popComposerAttachedPath: () => void;
  clearComposerAttachedPaths: () => void;
  setUseWorktree: (v: boolean) => void;
  setEnvironment: (e: ExecutionEnvironment) => void;
  setSshServer: (name: string | null) => void;
  setSandboxBackend: (b: SandboxBackend) => void;
  setLocalVenv: (type: LocalVenvType, name: string) => void;
  setBranchForRoot: (root: string, branch: string) => void;
  /** Initialize composer state for a specific session (called on session switch). */
  initForSession: (sessionId: string, cfg: SessionConfigResponse) => void;
  /** Reset to defaults when no session is active. */
  resetToDefaults: () => void;
}

function defaults() {
  return {
    permissionMode: "auto" as PermissionMode,
    composerAgentType: "auto",
    composerAttachedPaths: [] as string[],
    useWorktree: false,
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
    setComposerAgentType: unknown;
    addComposerAttachedPath: unknown;
    removeComposerAttachedPath: unknown;
    popComposerAttachedPath: unknown;
    clearComposerAttachedPaths: unknown;
    setUseWorktree: unknown;
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
        use_worktree: state.useWorktree,
      },
    });
  } catch (e) {
    console.error("[OmigaDebug] Failed to save session config:", e);
  }
}

export const useChatComposerStore = create<ChatComposerState>((set, get) => ({
  ...defaults(),
  selectedBranchByRoot: {},
  activeSessionId: null,

  setPermissionMode: (permissionMode) => {
    set({ permissionMode });
    const { activeSessionId } = get();
    if (activeSessionId) saveSessionConfig(activeSessionId, get());
  },
  setComposerAgentType: (composerAgentType) => {
    set({ composerAgentType });
    const { activeSessionId } = get();
    if (activeSessionId) saveSessionConfig(activeSessionId, get());
  },
  addComposerAttachedPath: (relativePath) =>
    set((s) => {
      const t = relativePath.trim().replace(/\\/g, "/");
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
  setUseWorktree: (useWorktree) => {
    set({ useWorktree });
    const { activeSessionId } = get();
    if (activeSessionId) saveSessionConfig(activeSessionId, get());
  },
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
    set({
      activeSessionId: sessionId,
      permissionMode: (cfg.permission_mode as PermissionMode) ?? "auto",
      composerAgentType: cfg.composer_agent_type || "auto",
      environment: (cfg.execution_environment as ExecutionEnvironment) ?? "local",
      sshServer: cfg.ssh_server ?? null,
      sandboxBackend: (cfg.sandbox_backend as SandboxBackend) ?? "docker",
      localVenvType: (cfg.local_venv_type as LocalVenvType) ?? "none",
      localVenvName: cfg.local_venv_name ?? "",
      useWorktree: cfg.use_worktree ?? false,
      // Keep composerAttachedPaths empty on switch
      composerAttachedPaths: [],
    });
  },

  resetToDefaults: () => {
    set({ ...defaults(), activeSessionId: null, composerAttachedPaths: [] });
  },
}));
