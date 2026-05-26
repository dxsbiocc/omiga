import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import { extractErrorMessage } from "../utils/errorMessage";
import type {
  OperatorChainStep,
  Playbook,
  ReplayPlaybookResponse,
} from "./playbookTypes";

export interface ReplayPlaybookArgs {
  playbookId: string;
  projectRoot?: string;
  sessionId?: string;
  executionEnvironment?: string;
  sshServer?: string;
  sandboxBackend?: string;
}

export interface SavePlaybookFromChainArgs {
  playbookId: string;
  title: string;
  steps: OperatorChainStep[];
  expectedOutputKeys: string[];
  chainOk: boolean;
  projectRoot?: string;
  executionEnvironment?: string;
}

export interface PlaybookState {
  playbooks: Playbook[];
  isLoading: boolean;
  error: string | null;
  listPlaybooks: (projectRoot?: string) => Promise<Playbook[]>;
  replayPlaybook: (
    args: ReplayPlaybookArgs,
  ) => Promise<ReplayPlaybookResponse>;
  savePlaybookFromChain: (args: SavePlaybookFromChainArgs) => Promise<Playbook>;
}

export const usePlaybookStore = create<PlaybookState>((set, get) => ({
  playbooks: [],
  isLoading: false,
  error: null,

  listPlaybooks: async (projectRoot?: string) => {
    set({ isLoading: true, error: null });
    try {
      const playbooks = await invoke<Playbook[]>("list_playbooks", {
        projectRoot,
      });
      set({ playbooks, isLoading: false });
      return playbooks;
    } catch (e) {
      const error = extractErrorMessage(e);
      set({ error, isLoading: false });
      throw new Error(error);
    }
  },

  replayPlaybook: async (args: ReplayPlaybookArgs) => {
    try {
      const response = await invoke<ReplayPlaybookResponse>("replay_playbook", {
        ...args,
      });
      try {
        await get().listPlaybooks(args.projectRoot);
      } catch {
        // Refresh is best-effort; replay success should still be returned.
      }
      return response;
    } catch (e) {
      const error = extractErrorMessage(e);
      set({ error });
      throw new Error(error);
    }
  },

  savePlaybookFromChain: async (args: SavePlaybookFromChainArgs) => {
    try {
      const playbook = await invoke<Playbook>("save_playbook_from_chain", {
        ...args,
      });
      try {
        await get().listPlaybooks(args.projectRoot);
      } catch {
        // Refresh is best-effort; save success should still be returned.
      }
      return playbook;
    } catch (e) {
      const error = extractErrorMessage(e);
      set({ error });
      throw new Error(error);
    }
  },
}));
