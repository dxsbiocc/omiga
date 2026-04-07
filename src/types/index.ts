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
