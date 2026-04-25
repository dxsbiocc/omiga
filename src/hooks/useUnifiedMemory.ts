/**
 * useUnifiedMemory — Unified hook for the Memory system (explicit + implicit)
 *
 * Provides access to both:
 * - Explicit memory (wiki): User-curated knowledge
 * - Implicit memory (pageindex): Auto-indexed project files
 */

import { useState, useCallback, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export interface MemoryConfig {
  root_dir: string;
  wiki_dir: string;
  implicit_dir: string;
  /** Absolute path where raw original files are kept. Defaults to ~/.omiga/memory/raw. */
  raw_dir: string;
  /** `user_home` (default, ~/.omiga/memory/projects/...) or `project_relative` */
  memory_mode: string;
  auto_build_index: boolean;
  index_extensions: string[];
  exclude_dirs: string[];
  max_file_size: number;
}

export interface ExplicitMemoryStatus {
  enabled: boolean;
  document_count: number;
}

export interface ImplicitMemoryStatus {
  enabled: boolean;
  document_count: number;
  section_count: number;
  total_bytes: number;
  last_build_time: number | null;
}

export interface PermanentProfileStatus {
  enabled: boolean;
  item_count: number;
  injected_char_count: number;
}

export interface SessionWorkingMemoryStatus {
  enabled: boolean;
  dirty: boolean;
  item_count: number;
  last_refreshed_at: string | null;
}

export interface LongTermStatus {
  project_entry_count: number;
  global_entry_count: number;
}

export interface KnowledgeBaseStatus {
  project_page_count: number;
  global_page_count: number;
}

export interface MemoryPaths {
  root: string;
  wiki: string;
  implicit: string;
  /** ~/.omiga/memory/permanent/wiki */
  permanent_wiki: string;
  long_term: string;
  permanent_long_term: string;
  /** ~/.omiga/memory/raw (configurable) */
  raw: string;
}

export interface UnifiedMemoryStatus {
  exists: boolean;
  version: string;
  needs_migration: boolean;
  explicit: ExplicitMemoryStatus;
  implicit: ImplicitMemoryStatus;
  permanent_profile: PermanentProfileStatus;
  working_memory: SessionWorkingMemoryStatus;
  long_term: LongTermStatus;
  knowledge_base: KnowledgeBaseStatus;
  paths: MemoryPaths;
}

export interface SetMemoryConfigRequest {
  project_path: string;
  root_dir?: string;
  wiki_dir?: string;
  implicit_dir?: string;
  /** Absolute path for raw file storage. Pass empty string to reset to default. */
  raw_dir?: string;
  memory_mode?: "user_home" | "project_relative";
  auto_build_index?: boolean;
  index_extensions?: string[];
  exclude_dirs?: string[];
  max_file_size?: number;
}

export interface QueryResultItem {
  title: string;
  path: string;
  breadcrumb: string[];
  excerpt: string;
  score: number;
  match_type: string;
  source_type: string;
}

export interface QueryResponse {
  results: QueryResultItem[];
  query: string;
  total_matches: number;
}

export type MemoryTab = "overview" | "knowledge" | "implicit" | "config";

export interface ImportToWikiResult {
  success: boolean;
  imported_count: number;
  skipped_count: number;
  errors: string[];
  created_pages: string[];
}

export type MemoryLevel = "project" | "user";

export interface ImportToWikiOptions {
  include_content?: boolean;
  tags?: string[];
  memory_level?: MemoryLevel;
}

// ---------------------------------------------------------------------------
// Hook
// ---------------------------------------------------------------------------

export function useUnifiedMemory(projectPath: string) {
  const [status, setStatus] = useState<UnifiedMemoryStatus | null>(null);
  const [config, setConfig] = useState<MemoryConfig | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [activeTab, setActiveTab] = useState<MemoryTab>("overview");
  const [searchQuery, setSearchQuery] = useState("");
  const [searchResults, setSearchResults] = useState<QueryResponse | null>(null);
  const [building, setBuilding] = useState(false);
  const [importing, setImporting] = useState(false);
  const [importResult, setImportResult] = useState<ImportToWikiResult | null>(null);
  const [supportedExtensions, setSupportedExtensions] = useState<string[]>([]);

  // Load status and config
  const refresh = useCallback(async () => {
    if (!projectPath) return;
    
    setLoading(true);
    setError(null);
    try {
      const [statusData, configData] = await Promise.all([
        invoke<UnifiedMemoryStatus>("memory_get_unified_status", { projectPath }),
        invoke<MemoryConfig>("memory_get_config", { projectPath }),
      ]);
      setStatus(statusData);
      setConfig(configData);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, [projectPath]);

  useEffect(() => {
    refresh();
  }, [refresh]);

  // Update config
  const updateConfig = useCallback(async (updates: Partial<SetMemoryConfigRequest>) => {
    if (!projectPath) return;
    
    setLoading(true);
    setError(null);
    try {
      const newConfig = await invoke<MemoryConfig>("memory_set_config", {
        req: {
          project_path: projectPath,
          ...updates,
        },
      });
      setConfig(newConfig);
      await refresh();
      return true;
    } catch (e) {
      setError(String(e));
      return false;
    } finally {
      setLoading(false);
    }
  }, [projectPath, refresh]);

  // Build implicit index
  const buildIndex = useCallback(async () => {
    if (!projectPath) return;
    
    setBuilding(true);
    setError(null);
    try {
      await invoke("memory_build_index", {
        req: {
          project_path: projectPath,
        },
      });
      await refresh();
    } catch (e) {
      setError(String(e));
    } finally {
      setBuilding(false);
    }
  }, [projectPath, refresh]);

  // Update implicit index (incremental)
  const updateIndex = useCallback(async () => {
    if (!projectPath) return;
    
    setBuilding(true);
    setError(null);
    try {
      await invoke("memory_update_index", { projectPath });
      await refresh();
    } catch (e) {
      setError(String(e));
    } finally {
      setBuilding(false);
    }
  }, [projectPath, refresh]);

  // Search implicit index
  const search = useCallback(async (query: string, limit?: number) => {
    if (!projectPath || !query.trim()) return;
    
    setLoading(true);
    setError(null);
    try {
      const results = await invoke<QueryResponse>("memory_query", {
        req: {
          project_path: projectPath,
          query: query.trim(),
          limit,
        },
      });
      setSearchResults(results);
      return results;
    } catch (e) {
      setError(String(e));
      return null;
    } finally {
      setLoading(false);
    }
  }, [projectPath]);

  // Clear implicit index
  const clearIndex = useCallback(async () => {
    if (!projectPath) return;
    if (!confirm("确定要清除隐性记忆索引吗？此操作不可恢复。")) return;
    
    setLoading(true);
    setError(null);
    try {
      await invoke("memory_clear_index", { projectPath });
      await refresh();
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, [projectPath, refresh]);

  // Run migration
  const migrate = useCallback(async () => {
    if (!projectPath) return;
    
    setLoading(true);
    setError(null);
    try {
      const migrated = await invoke<boolean>("memory_migrate", { projectPath });
      if (migrated) {
        await refresh();
      }
      return migrated;
    } catch (e) {
      setError(String(e));
      return false;
    } finally {
      setLoading(false);
    }
  }, [projectPath, refresh]);

  // Check if path is valid
  const isValidPath = useCallback((path: string): boolean => {
    if (!path.trim()) return false;
    if (path.includes("..")) return false;
    if (path === "/" || path === "\\") return false;
    return true;
  }, []);

  // Import file/directory/text to wiki
  const importToWiki = useCallback(async (
    sourceType: "file" | "directory" | "text",
    sourcePath?: string,
    textTitle?: string,
    textContent?: string,
    options?: ImportToWikiOptions
  ) => {
    if (!projectPath) return null;
    
    setImporting(true);
    setError(null);
    try {
      const result = await invoke<ImportToWikiResult>("memory_import_to_wiki", {
        req: {
          project_path: projectPath,
          source_type: sourceType,
          source_path: sourcePath,
          text_title: textTitle,
          text_content: textContent,
          include_content: options?.include_content ?? true,
          tags: options?.tags,
          memory_level: options?.memory_level ?? "project",
        },
      });
      setImportResult(result);
      if (result.success) {
        await refresh();
      }
      return result;
    } catch (e) {
      setError(String(e));
      return null;
    }
  }, [projectPath, refresh]);

  // Load supported import extensions
  const loadSupportedExtensions = useCallback(async () => {
    try {
      const extensions = await invoke<string[]>("memory_get_import_extensions");
      setSupportedExtensions(extensions);
    } catch (e) {
      console.error("Failed to load extensions:", e);
    }
  }, []);

  // Load extensions on mount
  useEffect(() => {
    loadSupportedExtensions();
  }, [loadSupportedExtensions]);

  return {
    // State
    status,
    config,
    loading,
    error,
    activeTab,
    setActiveTab,
    searchQuery,
    setSearchQuery,
    searchResults,
    building,
    importing,
    importResult,
    supportedExtensions,
    
    // Actions
    refresh,
    updateConfig,
    buildIndex,
    updateIndex,
    search,
    clearIndex,
    migrate,
    isValidPath,
    importToWiki,
    loadSupportedExtensions,
  };
}
