import { stringifyUnknown } from "./stringifyUnknown";

export interface BackgroundAgentTaskRow {
  task_id: string;
  agent_type: string;
  description: string;
  status: "Pending" | "Running" | "Completed" | "Failed" | "Cancelled";
  created_at?: number;
  completed_at?: number | null;
  round_id?: string | null;
  plan_id?: string | null;
  result_summary?: unknown;
  error_message?: unknown;
}

export interface ReviewerVerdictChip {
  taskId?: string;
  taskDescription?: string;
  taskStatus?: BackgroundAgentTaskRow["status"];
  createdAt?: number;
  completedAt?: number | null;
  agentType: string;
  severity: string;
  verdict: string;
  summary: string;
  rawText: string;
}

export function isReviewerAgent(agentType: string): boolean {
  return [
    "verification",
    "code-reviewer",
    "security-reviewer",
    "performance-reviewer",
    "quality-reviewer",
    "api-reviewer",
    "critic",
    "test-engineer",
  ].includes(agentType);
}

export function parseReviewerVerdict(
  task: Pick<
    BackgroundAgentTaskRow,
    "task_id" | "description" | "agent_type" | "status" | "created_at" | "completed_at"
  >,
  text: string,
): ReviewerVerdictChip {
  const upper = text.toUpperCase();
  const severity = upper.includes("CRITICAL")
    ? "critical"
    : upper.includes("HIGH")
      ? "high"
      : upper.includes("MEDIUM")
        ? "medium"
        : upper.includes("LOW")
          ? "low"
          : "info";
  const verdict =
    upper.includes("VERDICT: REJECTED") || upper.includes("REJECTED")
      ? "reject"
      : upper.includes("VERDICT: FAIL") || upper.includes("FAIL") || upper.includes("BLOCKER")
        ? "fail"
        : upper.includes("VERDICT: PARTIAL") || upper.includes("PARTIAL")
          ? "partial"
          : upper.includes("VERDICT: PASS") || upper.includes("APPROVED") || upper.includes("PASS")
            ? "pass"
            : "unknown";
  const summary =
    text
      .split("\n")
      .map((line) => line.trim())
      .find((line) => line && !line.startsWith("#") && !line.startsWith("-")) ?? "无摘要";
  return {
    taskId: task.task_id,
    taskDescription: task.description,
    taskStatus: task.status,
    createdAt: task.created_at,
    completedAt: task.completed_at,
    agentType: task.agent_type,
    severity,
    verdict,
    summary: summary.slice(0, 140),
    rawText: text,
  };
}

export function aggregateReviewerVerdicts(tasks: BackgroundAgentTaskRow[]): ReviewerVerdictChip[] {
  return tasks
    .filter((t) => isReviewerAgent(t.agent_type))
    .filter((t) => t.result_summary || t.error_message)
    .map((t) =>
      parseReviewerVerdict(
        t,
        stringifyUnknown(t.result_summary ?? t.error_message ?? ""),
      ),
    );
}

export function reviewerVerdictColor(verdict: string, severity: string): string {
  if (verdict === "reject" || verdict === "fail" || severity === "critical") return "#ef4444";
  if (verdict === "partial" || severity === "high" || severity === "medium") return "#f59e0b";
  if (verdict === "pass") return "#22c55e";
  return "#9ca3af";
}

export function isBlockerVerdict(verdict: ReviewerVerdictChip): boolean {
  return (
    verdict.verdict === "reject" ||
    verdict.verdict === "fail" ||
    verdict.severity === "critical" ||
    verdict.severity === "high"
  );
}

export function latestReviewerVerdicts(
  verdicts: ReviewerVerdictChip[],
): ReviewerVerdictChip[] {
  const latest = new Map<string, ReviewerVerdictChip>();
  for (const verdict of verdicts) {
    const groupKey = reviewerVerdictGroupKey(verdict);
    const existing = latest.get(groupKey);
    if (!existing) {
      latest.set(groupKey, verdict);
      continue;
    }
    const existingTs = existing.completedAt ?? existing.createdAt ?? 0;
    const nextTs = verdict.completedAt ?? verdict.createdAt ?? 0;
    if (nextTs >= existingTs) {
      latest.set(groupKey, verdict);
    }
  }
  return Array.from(latest.values());
}

function reviewerVerdictGroupKey(verdict: ReviewerVerdictChip): string {
  const normalizedTask = (verdict.taskDescription ?? "")
    .trim()
    .toLowerCase()
    .replace(/\s+/g, " ");
  if (normalizedTask.length > 0) {
    return `${verdict.agentType}::${normalizedTask}`;
  }
  return `${verdict.agentType}::${verdict.taskId ?? "unknown-task"}`;
}

export function overallReviewerHeadline(verdicts: ReviewerVerdictChip[]): {
  label: string;
  color: string;
} | null {
  if (verdicts.length === 0) return null;
  if (verdicts.some((v) => v.verdict === "reject" || v.verdict === "fail")) {
    return { label: "Reviewer: FAIL", color: "#ef4444" };
  }
  if (verdicts.some((v) => v.verdict === "partial")) {
    return { label: "Reviewer: PARTIAL", color: "#f59e0b" };
  }
  if (verdicts.some((v) => v.verdict === "pass")) {
    return { label: "Reviewer: PASS", color: "#22c55e" };
  }
  return { label: "Reviewer: INFO", color: "#9ca3af" };
}
