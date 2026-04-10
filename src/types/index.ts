export interface Session {
  id: string;
  name: string;
  project_path: string;
  messages: Message[];
  created_at: string;
  updated_at: string;
}

export interface Message {
  id: string;
  role: "user" | "assistant" | "tool";
  content: string;
  isStreaming?: boolean;
}

export interface Tool {
  name: string;
  description: string;
  parameters: Record<string, unknown>;
}

export interface GrepMatch {
  file: string;
  line: number;
  column: number;
  content: string;
}

export interface GlobMatch {
  path: string;
  is_file: boolean;
  size: number;
}

export interface FileEntry {
  path: string;
  name: string;
  is_file: boolean;
  size?: number;
  modified?: string;
}

export interface StreamOutputItem {
  type: string;
  data?: string | Record<string, unknown>;
}

export interface ApiError {
  message: string;
  code?: string;
}

// Unified Memory System types
export interface MemoryConfig {
  root_dir: string;
  wiki_dir: string;
  implicit_dir: string;
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

export interface MemoryPaths {
  root: string;
  wiki: string;
  implicit: string;
  permanent_wiki: string;
}

export interface UnifiedMemoryStatus {
  exists: boolean;
  version: string;
  needs_migration: boolean;
  explicit: ExplicitMemoryStatus;
  implicit: ImplicitMemoryStatus;
  paths: MemoryPaths;
}

export interface MemoryQueryResult {
  title: string;
  path: string;
  breadcrumb: string[];
  excerpt: string;
  score: number;
  match_type: string;
}
