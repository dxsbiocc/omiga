import { useMemo } from "react";
import {
  Box,
  Stack,
  Typography,
  CircularProgress,
} from "@mui/material";
import { CheckCircle, Error as ErrorIcon, RadioButtonUnchecked } from "@mui/icons-material";
import { alpha } from "@mui/material/styles";
import type { ExecutionStep } from "../../state/activityStore";
import { formatToolDisplayName } from "../../utils/executionSurfaceLabel";
import { compactLabel } from "../../utils/compactLabel";

export interface ToolStep {
  toolName: string;
  displayName: string;
  status: "running" | "done" | "error" | "pending";
  summary?: string;
  startedAt?: number;
  durationMs?: number;
}

const TOOL_DISPLAY_NAMES: Record<string, string> = {
  bash: "Running command",
  file_write: "Writing file",
  file_edit: "Editing file",
  file_read: "Reading file",
  read_file: "Reading file",
  write_file: "Writing file",
  search: "Searching web",
  fetch: "Fetching page",
  recall: "Searching memory",
  agent: "Running sub-agent",
  task_create: "Creating task",
  task_stop: "Stopping task",
  task_get: "Getting task",
  task_list: "Listing tasks",
  todo_write: "Updating checklist",
  glob: "Searching files",
  ripgrep: "Searching code",
  grep: "Searching code",
  notebook_edit: "Editing notebook",
};

function resolveDisplayName(toolName: string): string {
  const key = toolName.toLowerCase().replace(/[^a-z0-9_]/g, "_");
  if (TOOL_DISPLAY_NAMES[key]) return TOOL_DISPLAY_NAMES[key];
  for (const [k, v] of Object.entries(TOOL_DISPLAY_NAMES)) {
    if (key.includes(k)) return v;
  }
  return formatToolDisplayName(toolName);
}

function formatDuration(ms: number): string {
  if (ms < 1000) return `${ms}ms`;
  return `${(ms / 1000).toFixed(1)}s`;
}

interface StepDotProps {
  status: ToolStep["status"];
}

function StepDot({ status }: StepDotProps) {
  if (status === "running") {
    return <CircularProgress size={12} thickness={5} sx={{ color: "#eab308", flexShrink: 0 }} />;
  }
  if (status === "done") {
    return <CheckCircle sx={{ fontSize: 14, color: "success.main", flexShrink: 0 }} />;
  }
  if (status === "error") {
    return <ErrorIcon sx={{ fontSize: 14, color: "error.main", flexShrink: 0 }} />;
  }
  return <RadioButtonUnchecked sx={{ fontSize: 14, color: "action.disabled", flexShrink: 0 }} />;
}

interface TaskProgressStepsProps {
  steps: ToolStep[];
  /** When true, show a collapsed summary line instead of full step list */
  collapsed?: boolean;
  totalDurationMs?: number;
}

export function TaskProgressSteps({ steps, collapsed, totalDurationMs }: TaskProgressStepsProps) {
  if (steps.length === 0) return null;

  if (collapsed) {
    const errorCount = steps.filter((s) => s.status === "error").length;
    const doneCount = steps.filter((s) => s.status === "done" || s.status === "error").length;
    const durationLabel = totalDurationMs != null ? ` in ${formatDuration(totalDurationMs)}` : "";
    return (
      <Box sx={{ display: "flex", alignItems: "center", gap: 0.5, py: 0.5 }}>
        <CheckCircle sx={{ fontSize: 13, color: errorCount > 0 ? "warning.main" : "success.main" }} />
        <Typography variant="caption" color="text.secondary" sx={{ fontSize: 11 }}>
          {errorCount > 0
            ? `${doneCount} steps completed (${errorCount} errors)${durationLabel}`
            : `${doneCount} steps completed${durationLabel}`}
        </Typography>
      </Box>
    );
  }

  return (
    <Stack spacing={0.25}>
      {steps.map((step) => (
        <Box
          key={`${step.toolName}-${step.startedAt ?? 0}`}
          sx={{
            display: "flex",
            alignItems: "center",
            gap: 0.75,
            py: 0.3,
            px: 0.5,
            borderRadius: 1,
            bgcolor:
              step.status === "running"
                ? alpha("#eab308", 0.06)
                : step.status === "error"
                  ? alpha("#ef4444", 0.05)
                  : "transparent",
          }}
        >
          <StepDot status={step.status} />

          <Box sx={{ flex: 1, minWidth: 0 }}>
            <Typography
              variant="body2"
              sx={{
                fontSize: 11.5,
                lineHeight: 1.3,
                fontWeight: step.status === "running" ? 600 : 400,
                color:
                  step.status === "error"
                    ? "error.main"
                    : step.status === "running"
                      ? "#ca8a04"
                      : "text.primary",
                overflow: "hidden",
                textOverflow: "ellipsis",
                whiteSpace: "nowrap",
              }}
            >
              {step.displayName}
            </Typography>
            {step.summary && step.status !== "running" && (
              <Typography
                variant="caption"
                color="text.secondary"
                sx={{
                  fontSize: 10,
                  lineHeight: 1.3,
                  display: "block",
                  overflow: "hidden",
                  textOverflow: "ellipsis",
                  whiteSpace: "nowrap",
                }}
                title={step.summary}
              >
                {compactLabel(step.summary, 60)}
              </Typography>
            )}
          </Box>

          <Box sx={{ flexShrink: 0, ml: 0.5 }}>
            {step.status === "running" ? (
              <Typography
                variant="caption"
                sx={{ fontSize: 10, color: "#ca8a04", fontWeight: 600 }}
              >
                …
              </Typography>
            ) : step.durationMs != null ? (
              <Typography variant="caption" color="text.disabled" sx={{ fontSize: 10 }}>
                {formatDuration(step.durationMs)}
              </Typography>
            ) : null}
          </Box>
        </Box>
      ))}
    </Stack>
  );
}

/**
 * Converts raw ExecutionStep array from activityStore into ToolStep[]
 * suitable for TaskProgressSteps. Only includes tool-prefixed steps (not
 * connect/think/reply pseudo-steps).
 */
export function useToolSteps(
  executionSteps: ExecutionStep[],
  executionStartedAt: number | null,
  isStreaming: boolean,
  executionEndedAt?: number | null,
): { steps: ToolStep[]; totalDurationMs: number | undefined } {
  return useMemo(() => {
    const toolSteps = executionSteps.filter((s) => s.id.startsWith("tool-"));

    const steps: ToolStep[] = toolSteps.map((s) => {
      // Only use toolName (the actual tool identifier); fall back to empty string
      // rather than s.title which may contain UI-facing translated text.
      const rawName = s.toolName ?? "";
      const status: ToolStep["status"] = s.failed
        ? "error"
        : s.status === "running"
          ? "running"
          : "done";
      return {
        toolName: rawName,
        displayName: rawName ? resolveDisplayName(rawName) : "Tool",
        status,
        summary: s.toolOutput ?? s.summary,
      };
    });

    const allDone = toolSteps.length > 0 && toolSteps.every((s) => s.status !== "running");
    // Use the store's executionEndedAt if available; fall back to current time only
    // when still streaming (so we don't produce a stale snapshot from memo cache).
    const totalDurationMs =
      !isStreaming && allDone && executionStartedAt != null
        ? (executionEndedAt ?? Date.now()) - executionStartedAt
        : undefined;

    return { steps, totalDurationMs };
  }, [executionSteps, executionStartedAt, isStreaming, executionEndedAt]);
}
