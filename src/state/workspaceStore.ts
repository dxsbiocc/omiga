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
    const { filePath, savedContent } = get();
    
    // If already open, just activate without reloading
    if (filePath === path && savedContent !== "") {
      return;
    }
    
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
      savedContent: "",
      content: "",
      totalLines: 0,
      isDirty: false,
      saveError: null,
    });
    
    try {
      // Use chunked reading for faster initial display of large files
      // First chunk: read first 500 lines to display quickly
      const firstChunk = await invoke<FileReadResponse>("read_file", {
        path,
        offset: 0,
        limit: 500,
      });
      
      // Show first chunk immediately
      set({
        savedContent: firstChunk.content,
        content: firstChunk.content,
        totalLines: firstChunk.total_lines,
        isLoading: false,
        error: null,
        isDirty: false,
      });
      
      // If there's more content, load the rest in background
      if (firstChunk.has_more) {
        const remaining = await invoke<FileReadResponse>("read_file", {
          path,
          offset: 500,
          limit: 10000, // Large enough to get rest
        });
        const fullContent = firstChunk.content + "\n" + remaining.content;
        set({
          savedContent: fullContent,
          content: fullContent,
          totalLines: remaining.total_lines,
        });
      }
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
