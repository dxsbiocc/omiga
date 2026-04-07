import { useState, useEffect, useMemo } from "react";
import {
  Box,
  Typography,
  Stack,
  Chip,
  Fade,
} from "@mui/material";
import { alpha } from "@mui/material/styles";
import {
  Terminal,
  CloudQueue,
} from "@mui/icons-material";
import {
  useSessionStore,
  useActivityStore,
  useChatComposerStore,
  type Message,
} from "../../state";
import { formatExecutionElapsedFixed } from "../ExecutionStepPanel";
import { PlanTodoList, type PlanTodoItem } from "./PlanTodoList";
import { ReactStepList } from "./ReactStepList";

interface TodoLine {
  id: string;
  content: string;
  activeForm: string;
  status: string;
}

function parseTodoWriteArgs(raw: string | undefined): TodoLine[] | null {
  if (!raw?.trim()) return null;
  try {
    const j = JSON.parse(raw) as {
      todos?: Array<{
        id?: string;
        content: string;
        activeForm?: string;
        active_form?: string;
        status: string;
      }>;
    };
    if (!j.todos) return [];
    return j.todos.map((t, i) => ({
      id: t.id ?? `todo-${i}`,
      content: t.content,
      activeForm: t.activeForm ?? t.active_form ?? t.content,
      status: String(t.status),
    }));
  } catch {
    return null;
  }
}

function latestTodosFromMessages(messages: Message[]): TodoLine[] {
  for (let i = messages.length - 1; i >= 0; i--) {
    const m = messages[i];
    if (
      m.role === "tool" &&
      m.toolCall?.name === "todo_write" &&
      m.toolCall.arguments
    ) {
      const parsed = parseTodoWriteArgs(m.toolCall.arguments);
      if (parsed !== null) return parsed;
    }
  }
  return [];
}

function todoToPlanItem(t: TodoLine): PlanTodoItem {
  const s = t.status.toLowerCase();
  let status: PlanTodoItem["status"] = "pending";
  if (s.includes("progress")) status = "running";
  else if (s.includes("complete")) status = "completed";
  else if (s.includes("error") || s.includes("fail")) status = "error";
  return {
    id: t.id,
    name: t.content || t.activeForm,
    status,
  };
}

export function TaskStatus() {
  const agentMode = useChatComposerStore((s) => s.agentMode);
  const storeMessages = useSessionStore((s) => s.storeMessages);
  const executionSteps = useActivityStore((s) => s.executionSteps);
  const executionStartedAt = useActivityStore((s) => s.executionStartedAt);
  const executionEndedAt = useActivityStore((s) => s.executionEndedAt);
  const backgroundJobs = useActivityStore((s) => s.backgroundJobs);
  const isConnecting = useActivityStore((s) => s.isConnecting);
  const isStreaming = useActivityStore((s) => s.isStreaming);
  const waitingFirstChunk = useActivityStore((s) => s.waitingFirstChunk);
  const currentToolHint = useActivityStore((s) => s.currentToolHint);

  const [elapsedTick, setElapsedTick] = useState(0);
  const runActive =
    agentMode !== "plan" &&
    executionSteps.length > 0 &&
    executionEndedAt == null;
  useEffect(() => {
    if (!runActive) return;
    const id = window.setInterval(() => setElapsedTick((n) => n + 1), 1000);
    return () => window.clearInterval(id);
  }, [runActive]);

  const elapsedLabel = useMemo(
    () =>
      formatExecutionElapsedFixed(
        executionStartedAt,
        executionEndedAt,
        elapsedTick,
      ),
    [executionStartedAt, executionEndedAt, elapsedTick],
  );

  const todoItems = useMemo(() => {
    const todos = latestTodosFromMessages(storeMessages);
    return todos.map(todoToPlanItem);
  }, [storeMessages]);

  const hasExecution = executionSteps.length > 0;
  const hasBackground = backgroundJobs.length > 0;

  const showPlanSection = agentMode === "plan";
  const showReactSection = agentMode !== "plan" && hasExecution;

  const surfaceContext = useMemo(
    () => ({
      isConnecting,
      isStreaming,
      waitingFirstChunk,
      toolHintFallback: currentToolHint,
    }),
    [isConnecting, isStreaming, waitingFirstChunk, currentToolHint],
  );

  return (
    <Box sx={{ height: "100%", display: "flex", flexDirection: "column", minHeight: 0 }}>
      {showPlanSection && (
        <Box sx={{ flexShrink: 0, borderBottom: 1, borderColor: "divider" }}>
          <Stack
            direction="row"
            alignItems="center"
            justifyContent="space-between"
            spacing={1}
            sx={{ px: 1.5, pt: 1.25, pb: 0.5 }}
          >
            <Stack direction="row" alignItems="center" spacing={0.75}>
              <Typography
                variant="body2"
                fontWeight={700}
                sx={{ fontSize: 12, letterSpacing: "0.02em" }}
              >
                任务区
              </Typography>
              <Chip
                size="small"
                label="计划模式"
                sx={{ height: 20, fontSize: 10, fontWeight: 600 }}
              />
            </Stack>
          </Stack>
          <Box sx={{ px: 1.5, pb: 1.25 }}>
            <PlanTodoList items={todoItems} />
          </Box>
        </Box>
      )}

      {showReactSection && (
        <Box sx={{ flexShrink: 0, borderBottom: 1, borderColor: "divider" }}>
          <Stack
            direction="row"
            alignItems="center"
            justifyContent="space-between"
            spacing={1}
            sx={{ px: 1.5, pt: 1.25, pb: 0.5 }}
          >
            <Stack direction="row" alignItems="center" spacing={0.75}>
              <Typography
                variant="body2"
                fontWeight={700}
                sx={{ fontSize: 12, letterSpacing: "0.02em" }}
              >
                任务区
              </Typography>
              <Chip
                size="small"
                label="ReAct"
                sx={{ height: 20, fontSize: 10, fontWeight: 600 }}
              />
            </Stack>
          </Stack>
          <Box sx={{ px: 1.5, pb: 1.25 }}>
            <ReactStepList
              steps={executionSteps}
              elapsedLabel={elapsedLabel}
              surfaceContext={surfaceContext}
            />
          </Box>
        </Box>
      )}

      {hasBackground && (
        <Box
          sx={{
            flex: 1,
            overflow: "auto",
            display: "flex",
            flexDirection: "column",
            minHeight: 0,
          }}
        >
          <Box sx={{ px: 1.5, py: 1.5, flex: 1 }}>
            <Stack direction="row" alignItems="center" spacing={0.75} sx={{ mb: 1 }}>
              <CloudQueue fontSize="small" sx={{ color: "#6366f1" }} />
              <Typography variant="body2" fontWeight={600}>
                后台任务
              </Typography>
              <Chip size="small" label={backgroundJobs.length} sx={{ height: 20, fontSize: 10 }} />
            </Stack>
            <Stack spacing={1}>
              {backgroundJobs.map((job) => (
                <Fade key={job.id} in timeout={200}>
                  <Box
                    sx={{
                      display: "flex",
                      alignItems: "flex-start",
                      gap: 1,
                      p: 1,
                      borderRadius: 1.5,
                      bgcolor: alpha("#6366f1", 0.04),
                      border: 1,
                      borderColor: alpha("#6366f1", 0.12),
                    }}
                  >
                    <Terminal
                      fontSize="small"
                      sx={{ color: "#6366f1", mt: 0.15, flexShrink: 0 }}
                    />
                    <Box sx={{ minWidth: 0, flex: 1 }}>
                      <Typography variant="body2" sx={{ fontSize: 12, lineHeight: 1.35 }}>
                        {job.label}
                      </Typography>
                      <Stack direction="row" alignItems="center" spacing={0.5} sx={{ mt: 0.5 }}>
                        {job.state === "running" && (
                          <Chip
                            size="small"
                            label="运行中"
                            sx={{ height: 20, fontSize: 10 }}
                          />
                        )}
                        {job.state === "done" && (
                          <Chip
                            size="small"
                            label="已完成"
                            color="success"
                            variant="outlined"
                            sx={{ height: 20, fontSize: 10 }}
                          />
                        )}
                        {(job.state === "error" || job.state === "interrupted") && (
                          <Chip
                            size="small"
                            label={job.state === "interrupted" ? "已中断" : "失败"}
                            color="warning"
                            variant="outlined"
                            sx={{ height: 20, fontSize: 10 }}
                          />
                        )}
                        {job.exitCode != null && job.state !== "running" && (
                          <Typography variant="caption" color="text.secondary">
                            exit {job.exitCode}
                          </Typography>
                        )}
                      </Stack>
                    </Box>
                  </Box>
                </Fade>
              ))}
            </Stack>
          </Box>
        </Box>
      )}
    </Box>
  );
}
