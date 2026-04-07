/**
 * useWikiHook — TypeScript hook layer for the Omiga Wiki Agent feature.
 *
 * Implements the "hook" mechanism described in the Karpathy LLM Wiki pattern:
 *   1. PreSend hook: intercepts user messages and queries the wiki for relevant context,
 *      returning an optional system-prompt injection (the transparent hook).
 *   2. Wiki management: ingest, query, lint operations via the wiki-agent sub-agent.
 *
 * This is the TypeScript side of the two-layer hook design:
 *   - Rust backend: injects wiki context into system prompt before every LLM call
 *     (`domain/wiki::query_relevant_context`).
 *   - TypeScript (this file): exposes React state + action helpers for UI and
 *     explicit wiki management operations.
 *
 * Usage:
 *   const wiki = useWikiHook(projectPath);
 *   await wiki.refresh();              // reload status
 *   await wiki.ingest(sourceText);     // ingest new content via wiki-agent
 *   await wiki.query("search term");   // query pages
 *   await wiki.lint();                 // audit wiki health
 */

import { useState, useCallback, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";

// ---------------------------------------------------------------------------
// Types (mirror Rust structs)
// ---------------------------------------------------------------------------

export interface WikiStatus {
  exists: boolean;
  page_count: number;
  index_summary: string | null;
  wiki_dir: string;
  last_log_entry: string | null;
}

export interface WikiPageExcerpt {
  slug: string;
  excerpt: string;
}

export interface WikiQueryResult {
  matched_slugs: string[];
  excerpts: WikiPageExcerpt[];
}

export type WikiOperation = "ingest" | "query" | "lint" | "idle";

export interface WikiHookState {
  status: WikiStatus | null;
  loading: boolean;
  operation: WikiOperation;
  error: string | null;
  lastQueryResult: WikiQueryResult | null;
}

export interface WikiHookActions {
  /** Refresh wiki status from the backend. */
  refresh: () => Promise<void>;
  /** Keyword search over the wiki index (inline, no sub-agent). */
  search: (query: string) => Promise<WikiQueryResult>;
  /** Read a specific wiki page by slug. */
  readPage: (slug: string) => Promise<string | null>;
  /** Write a wiki page (create or overwrite). */
  writePage: (slug: string, content: string) => Promise<void>;
  /** Delete a wiki page. */
  deletePage: (slug: string) => Promise<void>;
  /** Read the wiki index. */
  readIndex: () => Promise<string | null>;
  /** Write the wiki index. */
  writeIndex: (content: string) => Promise<void>;
  /** Read the wiki log. */
  readLog: () => Promise<string | null>;
  /** Append a timestamped entry to log.md. */
  appendLog: (entry: string) => Promise<void>;
  /** Return the absolute path to the wiki directory. */
  getWikiDir: () => Promise<string>;
  /**
   * Launch a wiki-agent sub-agent to ingest `sourceText` into the wiki.
   * Returns the agent's summary response (via the main chat session send_message flow).
   * Note: this function builds the prompt — callers must dispatch it through the
   * normal chat send path to benefit from streaming UI.
   */
  buildIngestPrompt: (sourceText: string, sourceTitle?: string) => string;
  /**
   * Build a query prompt for the wiki-agent to answer a knowledge question.
   */
  buildQueryPrompt: (question: string) => string;
  /**
   * Build a lint prompt for the wiki-agent to audit wiki health.
   */
  buildLintPrompt: () => string;
}

// ---------------------------------------------------------------------------
// Hook
// ---------------------------------------------------------------------------

export function useWikiHook(projectPath: string): WikiHookState & WikiHookActions {
  const [state, setState] = useState<WikiHookState>({
    status: null,
    loading: false,
    operation: "idle",
    error: null,
    lastQueryResult: null,
  });

  const refresh = useCallback(async () => {
    setState((s) => ({ ...s, loading: true, error: null }));
    try {
      const status = await invoke<WikiStatus>("wiki_get_status", { projectPath });
      setState((s) => ({ ...s, status, loading: false }));
    } catch (e) {
      setState((s) => ({ ...s, loading: false, error: String(e) }));
    }
  }, [projectPath]);

  // Auto-load on mount and when projectPath changes
  useEffect(() => {
    if (projectPath) {
      refresh();
    }
  }, [projectPath, refresh]);

  const search = useCallback(
    async (query: string): Promise<WikiQueryResult> => {
      setState((s) => ({ ...s, operation: "query", error: null }));
      try {
        const result = await invoke<WikiQueryResult>("wiki_query", {
          projectPath,
          query,
        });
        setState((s) => ({ ...s, operation: "idle", lastQueryResult: result }));
        return result;
      } catch (e) {
        setState((s) => ({ ...s, operation: "idle", error: String(e) }));
        return { matched_slugs: [], excerpts: [] };
      }
    },
    [projectPath],
  );

  const readPage = useCallback(
    (slug: string) =>
      invoke<string | null>("wiki_read_page", { projectPath, slug }),
    [projectPath],
  );

  const writePage = useCallback(
    (slug: string, content: string) =>
      invoke<void>("wiki_write_page", { req: { project_path: projectPath, slug, content } }),
    [projectPath],
  );

  const deletePage = useCallback(
    (slug: string) => invoke<void>("wiki_delete_page", { projectPath, slug }),
    [projectPath],
  );

  const readIndex = useCallback(
    () => invoke<string | null>("wiki_read_index", { projectPath }),
    [projectPath],
  );

  const writeIndex = useCallback(
    (content: string) => invoke<void>("wiki_write_index", { projectPath, content }),
    [projectPath],
  );

  const readLog = useCallback(
    () => invoke<string | null>("wiki_read_log", { projectPath }),
    [projectPath],
  );

  const appendLog = useCallback(
    (entry: string) => invoke<void>("wiki_append_log", { projectPath, entry }),
    [projectPath],
  );

  const getWikiDir = useCallback(
    () => invoke<string>("wiki_get_dir", { projectPath }),
    [projectPath],
  );

  // ---------------------------------------------------------------------------
  // Prompt builders for wiki-agent operations
  // These build the user-facing prompt that gets dispatched through send_message.
  // The Agent tool (with subagent_type: "wiki-agent") will handle the operation.
  // ---------------------------------------------------------------------------

  const buildIngestPrompt = useCallback(
    (sourceText: string, sourceTitle?: string): string => {
      const title = sourceTitle ? `"${sourceTitle}"` : "the following source material";
      return [
        `Please ingest ${title} into the project wiki using the wiki-agent:`,
        "",
        "```",
        sourceText.slice(0, 8000), // keep under context limits
        sourceText.length > 8000 ? "\n[... truncated ...]" : "",
        "```",
        "",
        "Use the Agent tool with `subagent_type: \"wiki-agent\"` to:",
        "1. Extract key information, entities, and concepts",
        "2. Create or update relevant wiki pages",
        "3. Update index.md with new entries",
        "4. Append an ingest entry to log.md",
        "5. Return a summary of what was ingested",
      ]
        .filter((l) => l !== undefined)
        .join("\n");
    },
    [],
  );

  const buildQueryPrompt = useCallback((question: string): string => {
    return [
      `Please answer the following question using the project wiki (wiki-agent):`,
      "",
      `**Question:** ${question}`,
      "",
      "Use the Agent tool with `subagent_type: \"wiki-agent\"` to:",
      "1. Search the wiki index for relevant pages",
      "2. Read the most relevant pages",
      "3. Synthesize a concise answer with citations",
      "4. Optionally create a new wiki page if the answer is worth persisting",
    ].join("\n");
  }, []);

  const buildLintPrompt = useCallback((): string => {
    return [
      "Please audit the project wiki health using the wiki-agent.",
      "",
      "Use the Agent tool with `subagent_type: \"wiki-agent\"` to:",
      "1. Read index.md and all page files",
      "2. Check for: contradictions, stale claims, orphaned pages, missing cross-references",
      "3. Report issues grouped by severity (critical / warning / info)",
      "4. Suggest new pages or investigations where gaps exist",
      "5. Append a lint entry to log.md with a summary",
    ].join("\n");
  }, []);

  return {
    ...state,
    refresh,
    search,
    readPage,
    writePage,
    deletePage,
    readIndex,
    writeIndex,
    readLog,
    getWikiDir,
    appendLog,
    buildIngestPrompt,
    buildQueryPrompt,
    buildLintPrompt,
  };
}
