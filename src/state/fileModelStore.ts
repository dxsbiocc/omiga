/**
 * File Model Store - Inspired by VSCode/sidex architecture
 * 
 * Key differences from workspaceStore:
 * 1. Models are cached and persist after closing (like VSCode tabs)
 * 2. Direct Monaco model management (bypass React wrapper overhead)
 * 3. Binary file reading for instant TextBuffer creation
 * 4. Editor state is separate from file model state
 */
import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import * as monaco from "monaco-editor";

// File types that must NOT be read as text
const BINARY_EXTS = new Set([
  "png", "jpg", "jpeg", "gif", "webp", "svg", "bmp", "ico", "tiff", "tif", "avif", "pdf",
]);

interface FileModel {
  path: string;
  name: string;
  content: string;
  version: number;
  isDirty: boolean;
  lastModified: number;
  monacoModel?: monaco.editor.ITextModel;
}

interface FileModelState {
  // Model cache: path -> FileModel (persists after closing)
  models: Map<string, FileModel>;
  // Currently open file path
  activePath: string | null;
  // Loading state
  isLoading: boolean;
  error: string | null;

  // Actions
  openFile: (path: string) => Promise<void>;
  closeFile: (path: string) => void;
  closeAllFiles: () => void;
  setActiveFile: (path: string | null) => void;
  updateContent: (path: string, content: string) => void;
  markDirty: (path: string, isDirty: boolean) => void;
  saveFile: (path: string) => Promise<void>;
  getModel: (path: string) => FileModel | undefined;
  getOrCreateMonacoModel: (path: string) => monaco.editor.ITextModel | null;
}

// Helper: Determine language from extension
function getLanguageFromPath(path: string): string {
  const ext = path.split(".").pop()?.toLowerCase() ?? "";
  const langMap: Record<string, string> = {
    py: "python", pyw: "python", rs: "rust", js: "javascript", jsx: "javascript",
    ts: "typescript", tsx: "typescript", go: "go", java: "java", c: "c",
    cpp: "cpp", cc: "cpp", h: "cpp", hpp: "cpp", cs: "csharp", swift: "swift",
    kt: "kotlin", rb: "ruby", php: "php", json: "json", yaml: "yaml", yml: "yaml",
    xml: "xml", html: "html", css: "css", scss: "scss", md: "markdown", sql: "sql",
  };
  return langMap[ext] ?? "plaintext";
}

// Read file as bytes and convert to string (faster than line-by-line)
async function readFileFast(path: string): Promise<string> {
  // Use read_file_bytes from sidex-style backend
  try {
    const bytes: number[] = await invoke("read_file_bytes", { path });
    // Fast Uint8Array -> string conversion
    const decoder = new TextDecoder("utf-8", { fatal: false });
    return decoder.decode(new Uint8Array(bytes));
  } catch {
    // Fallback to regular read_file if read_file_bytes not available
    const result = await invoke<{ content: string }>("read_file", { path, offset: null, limit: null });
    return result.content;
  }
}

export const useFileModelStore = create<FileModelState>((set, get) => ({
  models: new Map(),
  activePath: null,
  isLoading: false,
  error: null,

  openFile: async (path: string) => {
    const { models } = get();
    
    // Check if model already exists (reopening closed file)
    const existing = models.get(path);
    if (existing) {
      set({ activePath: path, error: null });
      return;
    }

    // Check if binary file
    const ext = path.split(".").pop()?.toLowerCase() ?? "";
    if (BINARY_EXTS.has(ext)) {
      const name = path.split(/[/\\]/).pop() ?? path;
      const newModel: FileModel = {
        path,
        name,
        content: "",
        version: 1,
        isDirty: false,
        lastModified: Date.now(),
      };
      const newModels = new Map(models);
      newModels.set(path, newModel);
      set({ models: newModels, activePath: path, isLoading: false, error: null });
      return;
    }

    set({ isLoading: true, error: null });

    try {
      // Fast file read
      const content = await readFileFast(path);
      const name = path.split(/[/\\]/).pop() ?? path;
      
      const newModel: FileModel = {
        path,
        name,
        content,
        version: 1,
        isDirty: false,
        lastModified: Date.now(),
      };

      const newModels = new Map(models);
      newModels.set(path, newModel);
      
      set({ 
        models: newModels, 
        activePath: path, 
        isLoading: false, 
        error: null 
      });
    } catch (e) {
      set({ 
        isLoading: false, 
        error: String(e),
        activePath: null,
      });
    }
  },

  closeFile: (path: string) => {
    const { models, activePath } = get();
    
    // Don't delete model from cache - just deactivate
    // This allows fast reopening like VSCode tabs
    
    if (activePath === path) {
      // Find next file to activate
      const remainingPaths = Array.from(models.keys()).filter(p => p !== path);
      set({ activePath: remainingPaths.length > 0 ? remainingPaths[0] : null });
    }
  },

  closeAllFiles: () => {
    set({ activePath: null });
  },

  setActiveFile: (path: string | null) => {
    set({ activePath: path, error: null });
  },

  updateContent: (path: string, content: string) => {
    const { models } = get();
    const model = models.get(path);
    if (!model) return;

    const isDirty = content !== model.content;
    const newModel = { ...model, content, isDirty };
    
    const newModels = new Map(models);
    newModels.set(path, newModel);
    set({ models: newModels });
  },

  markDirty: (path: string, isDirty: boolean) => {
    const { models } = get();
    const model = models.get(path);
    if (!model) return;

    const newModel = { ...model, isDirty };
    const newModels = new Map(models);
    newModels.set(path, newModel);
    set({ models: newModels });
  },

  saveFile: async (path: string) => {
    const { models } = get();
    const model = models.get(path);
    if (!model || !model.isDirty) return;

    try {
      await invoke("write_file", { path, content: model.content });
      
      const newModel = { ...model, isDirty: false, version: model.version + 1 };
      const newModels = new Map(models);
      newModels.set(path, newModel);
      set({ models: newModels });
    } catch (e) {
      set({ error: String(e) });
    }
  },

  getModel: (path: string) => {
    return get().models.get(path);
  },

  // Create or get Monaco model for direct editor integration
  getOrCreateMonacoModel: (path: string): monaco.editor.ITextModel | null => {
    const { models } = get();
    const model = models.get(path);
    if (!model) return null;

    // Return existing Monaco model if available
    if (model.monacoModel && !model.monacoModel.isDisposed()) {
      return model.monacoModel;
    }

    // Create new Monaco model
    const uri = monaco.Uri.file(path);
    const language = getLanguageFromPath(path);
    
    // Dispose existing model with same URI if exists
    const existing = monaco.editor.getModel(uri);
    if (existing) {
      existing.dispose();
    }

    const monacoModel = monaco.editor.createModel(model.content, language, uri);
    
    // Update model reference
    const newModel = { ...model, monacoModel };
    const newModels = new Map(models);
    newModels.set(path, newModel);
    set({ models: newModels });

    return monacoModel;
  },
}));
