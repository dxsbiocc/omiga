/** Matches `domain::agents::background::BackgroundAgentStatus` (serde default variant names). */
export type BackgroundAgentStatus =
  | "Pending"
  | "Running"
  | "Completed"
  | "Failed"
  | "Cancelled";

/** Matches `BackgroundAgentTask` from Rust (`serde` snake_case fields). */
export interface BackgroundAgentTask {
  task_id: string;
  agent_type: string;
  description: string;
  status: BackgroundAgentStatus;
  created_at: number;
  started_at?: number | null;
  completed_at?: number | null;
  result_summary?: string | null;
  error_message?: string | null;
  output_path?: string | null;
  session_id: string;
  message_id: string;
  round_id?: string | null;
  plan_id?: string | null;
}

export function shortBgTaskLabel(t: BackgroundAgentTask, maxLen = 36): string {
  const head = t.description.trim() || t.agent_type;
  return head.length > maxLen ? `${head.slice(0, maxLen - 1)}…` : head;
}

export function canSendFollowUpToTask(status: BackgroundAgentStatus): boolean {
  return status === "Pending" || status === "Running";
}

/** Matches Rust `domain::session::Message` JSON (`#[serde(tag = "role", rename_all = "lowercase")]`). */
export type BgSidechainMessage =
  | { role: "user"; content: string }
  | {
      role: "assistant";
      content: string;
      tool_calls?: Array<{
        id: string;
        name: string;
        arguments: string;
      }> | null;
    }
  | { role: "tool"; tool_call_id: string; output: string };
