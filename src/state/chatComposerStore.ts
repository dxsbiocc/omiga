import { create } from "zustand";
import { persist } from "zustand/middleware";

/** 主会话工具/编辑确认方式（与 Agent 类型独立）。 */
export type PermissionMode = "ask" | "auto" | "bypass";

interface ChatComposerState {
  permissionMode: PermissionMode;
  /** 注册表中的 Agent id，如 Explore、Plan、general-purpose */
  composerAgentType: string;
  /** `@` 选择器选中的工作区相对路径（仅内存，不持久化） */
  composerAttachedPaths: string[];
  useWorktree: boolean;
  environment: "local" | "remote";
  /** Remembered branch choice per workspace root path */
  selectedBranchByRoot: Record<string, string>;
  setPermissionMode: (m: PermissionMode) => void;
  setComposerAgentType: (t: string) => void;
  addComposerAttachedPath: (relativePath: string) => void;
  removeComposerAttachedPath: (relativePath: string) => void;
  popComposerAttachedPath: () => void;
  clearComposerAttachedPaths: () => void;
  setUseWorktree: (v: boolean) => void;
  setEnvironment: (e: "local" | "remote") => void;
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
      setBranchForRoot: (root, branch) =>
        set((s) => ({
          selectedBranchByRoot: { ...s.selectedBranchByRoot, [root]: branch },
        })),
    }),
    {
      name: "omiga-chat-composer",
      version: 3,
      /** 草稿附件路径不写入 localStorage */
      partialize: (s) => ({
        permissionMode: s.permissionMode,
        composerAgentType: s.composerAgentType,
        useWorktree: s.useWorktree,
        environment: s.environment,
        selectedBranchByRoot: s.selectedBranchByRoot,
      }),
      migrate: (persisted, version) => {
        if (version >= 3) return persisted as unknown as ChatComposerState;
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
        } as unknown as ChatComposerState;
      },
    },
  ),
);
