import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import { getLocalWorkspaceSessionId, getWorkspaceFileContext } from "../utils/sshWorkspace";
import { extractErrorMessage } from "../utils/errorMessage";
import { countTextLines } from "../utils/textMetrics";

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

interface WorkspaceContentProvider {
  filePath: string;
  getContent: () => string;
}

let activeContentProvider: WorkspaceContentProvider | null = null;

export function registerWorkspaceContentProvider(
  filePath: string,
  getContent: () => string,
): () => void {
  const provider = { filePath, getContent };
  activeContentProvider = provider;
  return () => {
    if (activeContentProvider === provider) {
      activeContentProvider = null;
    }
  };
}

function getLatestWorkspaceContent(filePath: string, fallback: string): string {
  if (activeContentProvider?.filePath === filePath) {
    return activeContentProvider.getContent();
  }
  return fallback;
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
  /** Mark dirty when an editor owns a local draft that has not been serialized yet. */
  markContentDirty: (filePath: string) => void;
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
      const ctx = getWorkspaceFileContext();
      const sessionId = ctx.mode === "local" ? getLocalWorkspaceSessionId() : null;
      if (ctx.mode === "local" && !sessionId) {
        throw new Error("请先选择本地工作区后再读取文件");
      }
      const readFirst = () => {
        if (ctx.mode === "ssh")
          return invoke<FileReadResponse>("ssh_read_file", { sshProfileName: ctx.profile, path, offset: 0, limit: 500 });
        if (ctx.mode === "sandbox")
          return invoke<FileReadResponse>("sandbox_read_file", { sessionId: ctx.sessionId, sandboxBackend: ctx.backend, path, offset: 0, limit: 500 });
        return invoke<FileReadResponse>("read_file", { path, offset: 0, limit: 500, sessionId });
      };
      const readRest = () => {
        if (ctx.mode === "ssh")
          return invoke<FileReadResponse>("ssh_read_file", { sshProfileName: ctx.profile, path, offset: 500, limit: 10000 });
        if (ctx.mode === "sandbox")
          return invoke<FileReadResponse>("sandbox_read_file", { sessionId: ctx.sessionId, sandboxBackend: ctx.backend, path, offset: 500, limit: 10000 });
        return invoke<FileReadResponse>("read_file", { path, offset: 500, limit: 10000, sessionId });
      };

      // Use chunked reading for faster initial display of large files
      // First chunk: read first 500 lines to display quickly
      const firstChunk = await readFirst();
      
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
        const remaining = await readRest();
        const fullContent = firstChunk.content + "\n" + remaining.content;
        set({
          savedContent: fullContent,
          content: fullContent,
          totalLines: remaining.total_lines,
        });
      }
    } catch (e) {
      set({
        error: extractErrorMessage(e),
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
    const { savedContent, content, isDirty, saveError } = get();
    const nextDirty = value !== savedContent;
    if (content === value && isDirty === nextDirty && saveError === null) return;
    set({
      content: value,
      isDirty: nextDirty,
      saveError: null,
      totalLines: countTextLines(value),
    });
  },

  markContentDirty: (path: string) => {
    const { filePath, isDirty, saveError } = get();
    if (filePath !== path) return;
    if (isDirty && saveError === null) return;
    set({ isDirty: true, saveError: null });
  },

  saveFile: async () => {
    const { filePath, content: storeContent } = get();
    if (!filePath) return;
    const content = getLatestWorkspaceContent(filePath, storeContent);
    if (content !== storeContent) {
      set({
        content,
        isDirty: true,
        saveError: null,
        totalLines: countTextLines(content),
      });
    }
    set({ isSaving: true, saveError: null });
    try {
      const ctx = getWorkspaceFileContext();
      if (ctx.mode === "ssh") {
        await invoke<FileWriteResponse>("ssh_write_file", { sshProfileName: ctx.profile, path: filePath, content, expectedHash: null });
      } else if (ctx.mode === "sandbox") {
        await invoke<FileWriteResponse>("sandbox_write_file", { sessionId: ctx.sessionId, sandboxBackend: ctx.backend, path: filePath, content, expectedHash: null });
      } else {
        const sessionId = getLocalWorkspaceSessionId();
        if (!sessionId) {
          throw new Error("请先选择本地工作区后再保存文件");
        }
        await invoke<FileWriteResponse>("write_file", { path: filePath, content, expectedHash: null, sessionId });
      }
      set({
        savedContent: content,
        isDirty: false,
        isSaving: false,
      });
    } catch (e) {
      set({ saveError: extractErrorMessage(e), isSaving: false });
    }
  },
}));
