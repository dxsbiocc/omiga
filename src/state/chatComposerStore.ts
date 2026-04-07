import { create } from "zustand";
import { persist } from "zustand/middleware";

/** How the agent applies edits (UI only; execution wiring can follow). */
export type AgentComposerMode = "ask" | "auto" | "plan" | "bypass";

interface ChatComposerState {
  agentMode: AgentComposerMode;
  useWorktree: boolean;
  environment: "local" | "remote";
  /** Remembered branch choice per workspace root path */
  selectedBranchByRoot: Record<string, string>;
  setAgentMode: (m: AgentComposerMode) => void;
  setUseWorktree: (v: boolean) => void;
  setEnvironment: (e: "local" | "remote") => void;
  setBranchForRoot: (root: string, branch: string) => void;
}

export const useChatComposerStore = create<ChatComposerState>()(
  persist(
    (set) => ({
      agentMode: "auto",
      useWorktree: false,
      environment: "local",
      selectedBranchByRoot: {},
      setAgentMode: (agentMode) => set({ agentMode }),
      setUseWorktree: (useWorktree) => set({ useWorktree }),
      setEnvironment: (environment) => set({ environment }),
      setBranchForRoot: (root, branch) =>
        set((s) => ({
          selectedBranchByRoot: { ...s.selectedBranchByRoot, [root]: branch },
        })),
    }),
    { name: "omiga-chat-composer" },
  ),
);
