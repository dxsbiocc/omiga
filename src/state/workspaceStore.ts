import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";

// File types that must NOT be read as text — handled by their own viewers
const BINARY_EXTS = new Set([
  "png", "jpg", "jpeg", "gif", "webp", "svg", "bmp", "ico", "tiff", "tif", "avif",
  "pdf",
]);

interface FileReadResponse {
  content: string;
  total_lines: number;
  has_more: boolean;
}

interface FileWriteResponse {
  bytes_written: number;
  new_hash: string;
}

interface WorkspaceState {
  filePath: string | null;
  fileName: string | null;
  /** Saved content on disk (source of truth for reset). */
  savedContent: string;
  /** Current editor content (may differ from savedContent when dirty). */
  content: string;
  totalLines: number;
  isLoading: boolean;
  isSaving: boolean;
  isDirty: boolean;
  error: string | null;
  saveError: string | null;
  openFile: (path: string) => Promise<void>;
  clearFile: () => void;
  /** Called by the editor whenever content changes. */
  setContent: (value: string) => void;
  saveFile: () => Promise<void>;
}

export const useWorkspaceStore = create<WorkspaceState>((set, get) => ({
  filePath: null,
  fileName: null,
  savedContent: "",
  content: "",
  totalLines: 0,
  isLoading: false,
  isSaving: false,
  isDirty: false,
  error: null,
  saveError: null,

  openFile: async (path: string) => {
    const name = path.split(/[/\\]/).pop() ?? path;
    const ext = (name.split(".").pop() ?? "").toLowerCase();

    // For image files skip text reading entirely — ImageViewer loads them directly
    if (BINARY_EXTS.has(ext)) {
      set({
        filePath: path,
        fileName: name,
        savedContent: "",
        content: "",
        totalLines: 0,
        isLoading: false,
        isDirty: false,
        error: null,
        saveError: null,
      });
      return;
    }

    set({
      isLoading: true,
      error: null,
      filePath: path,
      fileName: name,
      isDirty: false,
      saveError: null,
    });
    try {
      const res = await invoke<FileReadResponse>("read_file", {
        path,
        offset: null,
        limit: null,
      });
      set({
        savedContent: res.content,
        content: res.content,
        totalLines: res.total_lines,
        isLoading: false,
        error: null,
        isDirty: false,
      });
    } catch (e) {
      set({
        error: String(e),
        isLoading: false,
        savedContent: "",
        content: "",
        totalLines: 0,
      });
    }
  },

  clearFile: () =>
    set({
      filePath: null,
      fileName: null,
      savedContent: "",
      content: "",
      totalLines: 0,
      error: null,
      saveError: null,
      isLoading: false,
      isSaving: false,
      isDirty: false,
    }),

  setContent: (value: string) => {
    const { savedContent } = get();
    set({
      content: value,
      isDirty: value !== savedContent,
      saveError: null,
      totalLines: value.split(/\r?\n/u).length,
    });
  },

  saveFile: async () => {
    const { filePath, content } = get();
    if (!filePath) return;
    set({ isSaving: true, saveError: null });
    try {
      await invoke<FileWriteResponse>("write_file", {
        path: filePath,
        content,
        expectedHash: null,
      });
      set({
        savedContent: content,
        isDirty: false,
        isSaving: false,
      });
    } catch (e) {
      set({ saveError: String(e), isSaving: false });
    }
  },
}));
