import { create } from "zustand";
import { persist } from "zustand/middleware";

/** 主会话工具/编辑确认方式（与 Agent 类型独立）。 */
export type PermissionMode = "ask" | "auto" | "bypass";

/** 与 `omiga/src-tauri/src/execution/types.rs` `EnvironmentType` 对齐（不含 Local）。 */
export type SandboxBackend = "modal" | "daytona" | "docker" | "singularity";

/** 执行环境类型 */
export type ExecutionEnvironment = "local" | "ssh" | "sandbox";

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
  /** Remembered branch choice per workspace root path */
  selectedBranchByRoot: Record<string, string>;
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
  setBranchForRoot: (root: string, branch: string) => void;
}

export const useChatComposerStore = create<ChatComposerState>()(
  persist(
    (set) => ({
      permissionMode: "auto",
      composerAgentType: "auto",
      composerAttachedPaths: [],
      useWorktree: false,
      environment: "local",
      sshServer: null,
      sandboxBackend: "docker",
      selectedBranchByRoot: {},
      setPermissionMode: (permissionMode) => set({ permissionMode }),
      setComposerAgentType: (composerAgentType) => set({ composerAgentType }),
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
      setUseWorktree: (useWorktree) => set({ useWorktree }),
      setEnvironment: (environment) => set({ environment }),
      setSshServer: (sshServer) => set({ sshServer }),
      setSandboxBackend: (sandboxBackend) => set({ sandboxBackend }),
      setBranchForRoot: (root, branch) =>
        set((s) => ({
          selectedBranchByRoot: { ...s.selectedBranchByRoot, [root]: branch },
        })),
    }),
    {
      name: "omiga-chat-composer",
      version: 5,
      /** 草稿附件路径不写入 localStorage */
      partialize: (s) => ({
        permissionMode: s.permissionMode,
        composerAgentType: s.composerAgentType,
        useWorktree: s.useWorktree,
        environment: s.environment,
        sshServer: s.sshServer,
        sandboxBackend: s.sandboxBackend,
        selectedBranchByRoot: s.selectedBranchByRoot,
      }),
      migrate: (persisted, version) => {
        if (version >= 5) return persisted as unknown as ChatComposerState;
        if (version >= 4) {
          const p = persisted as Record<string, unknown>;
          // 版本 4 -> 5: 将 environment "remote" 迁移到新的 "ssh" | "sandbox" 格式
          const oldEnv = p.environment as string | undefined;
          const oldSandboxBackend = p.sandboxBackend as string | undefined;
          let newEnvironment: ExecutionEnvironment = "local";
          let newSshServer: string | null = null;
          let newSandboxBackend: SandboxBackend = "docker";
          if (oldEnv === "remote") {
            // 旧版 "remote" 根据 sandboxBackend 决定新类型
            if (oldSandboxBackend === "ssh") {
              newEnvironment = "ssh";
              newSshServer = null;
            } else {
              newEnvironment = "sandbox";
              // 过滤掉 "ssh"，默认使用 "docker"
              newSandboxBackend = (oldSandboxBackend as SandboxBackend) ?? "docker";
            }
          } else if (oldEnv === "local") {
            newEnvironment = "local";
          }
          return {
            ...p,
            environment: newEnvironment,
            sshServer: newSshServer,
            sandboxBackend: newSandboxBackend,
          } as unknown as ChatComposerState;
        }
        if (version >= 3) {
          const p = persisted as Record<string, unknown>;
          return {
            ...p,
            sandboxBackend: "ssh" as SandboxBackend,
          } as unknown as ChatComposerState;
        }
        if (version >= 2) {
          const prev = persisted as Record<string, unknown>;
          const env =
            prev.environment === "remote" ? "remote" : ("local" as const);
          return {
            ...prev,
            composerAttachedPaths: [] as string[],
            environment: env,
          } as unknown as ChatComposerState;
        }
        const p = persisted as Record<string, unknown> | null;
        if (!p || typeof p !== "object") {
          return {
            permissionMode: "auto" as PermissionMode,
            composerAgentType: "general-purpose",
            composerAttachedPaths: [],
            useWorktree: false,
            environment: "local" as const,
            sshServer: null,
            sandboxBackend: "docker" as SandboxBackend,
            selectedBranchByRoot: {},
          } as unknown as ChatComposerState;
        }
        const oldMode = p.agentMode as string | undefined;
        let permissionMode: PermissionMode = "auto";
        let composerAgentType = "auto";
        if (oldMode === "plan") {
          composerAgentType = "Plan";
          permissionMode = "auto";
        } else if (
          oldMode === "ask" ||
          oldMode === "auto" ||
          oldMode === "bypass"
        ) {
          permissionMode = oldMode;
        }
        const { agentMode: _drop, ...rest } = p;
        return {
          ...rest,
          permissionMode,
          composerAgentType,
          composerAttachedPaths: [],
          sshServer: null,
          sandboxBackend: "docker" as SandboxBackend,
        } as unknown as ChatComposerState;
      },
    },
  ),
);
