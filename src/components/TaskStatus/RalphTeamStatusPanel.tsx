/**
 * Ralph / Team Status Panel
 *
 * Polls the backend for active Ralph sessions and displays their phase,
 * iteration progress, todo completion, and stuck-error warnings.
 * Also surfaces Team mode parallel workers from the activity store's
 * backgroundJobs list (jobs with agent-type-style labels).
 */

import { useState, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  Box,
  Typography,
  Stack,
  Chip,
  LinearProgress,
  Tooltip,
  IconButton,
  Collapse,
  Fade,
} from "@mui/material";
import { alpha } from "@mui/material/styles";
import {
  Loop,
  Warning,
  CheckCircleOutline,
  ExpandMore,
  ExpandLess,
  DeleteOutline,
  Groups,
} from "@mui/icons-material";
import { useActivityStore } from "../../state/activityStore";

// ─── Types ──────────────────────────────────────────────────────────────────

interface BlackboardEntry {
  subtask_id: string;
  agent_type: string;
  key: string;
  value: string;
  posted_at: string;
}

interface BlackboardDto {
  session_id: string;
  entries: BlackboardEntry[];
  updated_at: string;
}

interface TeamSessionInfo {
  session_id: string;
  goal: string;
  phase: string;
  subtask_count: number;
  completed_count: number;
  failed_count: number;
  running_count: number;
  started_at: string;
  updated_at: string;
}

interface RalphSessionInfo {
  session_id: string;
  goal: string;
  phase: string;
  iteration: number;
  consecutive_errors: number;
  todos_completed: string[];
  todos_pending: string[];
  started_at: string;
  updated_at: string;
}

// ─── Phase display helpers ───────────────────────────────────────────────────

const PHASE_LABELS: Record<string, string> = {
  planning: "规划中",
  env_check: "环境检查",
  executing: "执行中",
  quality_check: "质量检查",
  verifying: "架构师验证",
  complete: "已完成",
};

const PHASE_COLORS: Record<string, string> = {
  planning: "#6366f1",
  env_check: "#0ea5e9",
  executing: "#22c55e",
  quality_check: "#f59e0b",
  verifying: "#a855f7",
  complete: "#10b981",
};

function phaseLabel(phase: string): string {
  return PHASE_LABELS[phase] ?? phase;
}

function phaseColor(phase: string): string {
  return PHASE_COLORS[phase] ?? "#9ca3af";
}

// ─── Single Ralph session card ───────────────────────────────────────────────

interface RalphCardProps {
  session: RalphSessionInfo;
  projectRoot: string;
  onCleared: () => void;
}

function RalphCard({ session, projectRoot, onCleared }: RalphCardProps) {
  const [expanded, setExpanded] = useState(false);
  const [clearing, setClearing] = useState(false);

  const totalTodos =
    session.todos_completed.length + session.todos_pending.length;
  const completedCount = session.todos_completed.length;
  const progress = totalTodos > 0 ? (completedCount / totalTodos) * 100 : 0;
  const isStuck = session.consecutive_errors >= 3;
  const color = isStuck ? "#ef4444" : phaseColor(session.phase);

  const handleClear = async () => {
    setClearing(true);
    try {
      await invoke("clear_ralph_session", {
        projectRoot,
        sessionId: session.session_id,
      });
      onCleared();
    } catch {
      // ignore — session may already be gone
    } finally {
      setClearing(false);
    }
  };

  return (
    <Fade in timeout={250}>
      <Box
        sx={{
          border: 1,
          borderColor: isStuck ? alpha("#ef4444", 0.35) : alpha(color, 0.2),
          borderRadius: 1.5,
          bgcolor: isStuck ? alpha("#ef4444", 0.04) : alpha(color, 0.03),
          overflow: "hidden",
        }}
      >
        {/* Header row */}
        <Stack
          direction="row"
          alignItems="flex-start"
          spacing={0.75}
          sx={{ px: 1.25, pt: 1, pb: 0.75 }}
        >
          <Loop
            fontSize="small"
            sx={{
              color,
              mt: 0.1,
              flexShrink: 0,
              animation:
                session.phase === "executing" && !isStuck
                  ? "spin 2s linear infinite"
                  : "none",
              "@keyframes spin": {
                from: { transform: "rotate(0deg)" },
                to: { transform: "rotate(360deg)" },
              },
            }}
          />
          <Box sx={{ flex: 1, minWidth: 0 }}>
            <Stack direction="row" alignItems="center" spacing={0.5}>
              <Chip
                label={phaseLabel(session.phase)}
                size="small"
                sx={{
                  height: 17,
                  fontSize: 10,
                  bgcolor: alpha(color, 0.12),
                  color,
                  fontWeight: 600,
                }}
              />
              <Typography variant="caption" color="text.disabled" sx={{ fontSize: 10 }}>
                第 {session.iteration} 轮
              </Typography>
              {isStuck && (
                <Tooltip title={`同一错误连续出现 ${session.consecutive_errors} 次`}>
                  <Warning sx={{ fontSize: 14, color: "#ef4444" }} />
                </Tooltip>
              )}
            </Stack>
            <Typography
              variant="body2"
              sx={{
                fontSize: 12,
                mt: 0.25,
                lineHeight: 1.35,
                overflow: "hidden",
                textOverflow: "ellipsis",
                display: "-webkit-box",
                WebkitLineClamp: 2,
                WebkitBoxOrient: "vertical",
              }}
              title={session.goal}
            >
              {session.goal}
            </Typography>
          </Box>
          <Stack direction="row" alignItems="center" spacing={0}>
            <Tooltip title={expanded ? "收起" : "展开待办"}>
              <IconButton
                size="small"
                onClick={() => setExpanded((v) => !v)}
                sx={{ p: 0.25 }}
              >
                {expanded ? (
                  <ExpandLess fontSize="small" />
                ) : (
                  <ExpandMore fontSize="small" />
                )}
              </IconButton>
            </Tooltip>
            <Tooltip title="清除状态文件">
              <IconButton
                size="small"
                onClick={handleClear}
                disabled={clearing}
                sx={{ p: 0.25, color: "text.disabled" }}
              >
                <DeleteOutline fontSize="small" />
              </IconButton>
            </Tooltip>
          </Stack>
        </Stack>

        {/* Progress bar */}
        {totalTodos > 0 && (
          <Box sx={{ px: 1.25, pb: 0.75 }}>
            <Stack direction="row" alignItems="center" spacing={0.75} sx={{ mb: 0.4 }}>
              <LinearProgress
                variant="determinate"
                value={progress}
                sx={{
                  flex: 1,
                  height: 4,
                  borderRadius: 2,
                  bgcolor: alpha(color, 0.12),
                  "& .MuiLinearProgress-bar": { bgcolor: color, borderRadius: 2 },
                }}
              />
              <Typography variant="caption" color="text.disabled" sx={{ fontSize: 10, flexShrink: 0 }}>
                {completedCount}/{totalTodos}
              </Typography>
            </Stack>
          </Box>
        )}

        {/* Stuck error banner */}
        {isStuck && (
          <Box
            sx={{
              mx: 1.25,
              mb: 0.75,
              p: 0.75,
              borderRadius: 1,
              bgcolor: alpha("#ef4444", 0.08),
              border: 1,
              borderColor: alpha("#ef4444", 0.2),
            }}
          >
            <Typography variant="caption" sx={{ fontSize: 10, color: "#ef4444", fontWeight: 600 }}>
              ⚠ 任务卡住 — 同一错误出现 {session.consecutive_errors} 次，需人工介入
            </Typography>
          </Box>
        )}

        {/* Expandable todo list */}
        <Collapse in={expanded}>
          <Box sx={{ px: 1.25, pb: 1 }}>
            {session.todos_completed.length > 0 && (
              <Stack spacing={0.25} sx={{ mb: 0.5 }}>
                {session.todos_completed.map((t, i) => (
                  <Stack key={i} direction="row" alignItems="flex-start" spacing={0.5}>
                    <CheckCircleOutline
                      sx={{ fontSize: 12, color: "#22c55e", mt: 0.1, flexShrink: 0 }}
                    />
                    <Typography
                      variant="caption"
                      sx={{ fontSize: 10, color: "text.secondary", lineHeight: 1.4 }}
                    >
                      {t}
                    </Typography>
                  </Stack>
                ))}
              </Stack>
            )}
            {session.todos_pending.map((t, i) => (
              <Stack key={i} direction="row" alignItems="flex-start" spacing={0.5}>
                <Box
                  sx={{
                    width: 12,
                    height: 12,
                    borderRadius: "50%",
                    border: "1.5px solid",
                    borderColor: alpha(color, 0.4),
                    mt: 0.1,
                    flexShrink: 0,
                  }}
                />
                <Typography
                  variant="caption"
                  sx={{ fontSize: 10, color: "text.disabled", lineHeight: 1.4 }}
                >
                  {t}
                </Typography>
              </Stack>
            ))}
          </Box>
        </Collapse>
      </Box>
    </Fade>
  );
}

// ─── Team workers strip (from backgroundJobs in activity store) ──────────────

function TeamWorkersStrip() {
  const backgroundJobs = useActivityStore((s) => s.backgroundJobs);
  const teamJobs = backgroundJobs.filter(
    (j) => j.label.startsWith("executor") || j.label.startsWith("worker") || j.label.startsWith("subtask"),
  );

  if (teamJobs.length === 0) return null;

  const runningCount = teamJobs.filter((j) => j.state === "running").length;
  const doneCount = teamJobs.filter((j) => j.state === "done").length;
  const errorCount = teamJobs.filter((j) => j.state === "error").length;

  return (
    <Box
      sx={{
        border: 1,
        borderColor: alpha("#8b5cf6", 0.2),
        borderRadius: 1.5,
        bgcolor: alpha("#8b5cf6", 0.03),
        px: 1.25,
        py: 0.75,
      }}
    >
      <Stack direction="row" alignItems="center" spacing={0.75} sx={{ mb: 0.5 }}>
        <Groups fontSize="small" sx={{ color: "#8b5cf6" }} />
        <Typography variant="body2" sx={{ fontSize: 11, fontWeight: 600, color: "#8b5cf6" }}>
          Team 并行工作者
        </Typography>
        {runningCount > 0 && (
          <Chip
            label={`${runningCount} 运行中`}
            size="small"
            sx={{ height: 16, fontSize: 9, bgcolor: alpha("#8b5cf6", 0.12), color: "#8b5cf6" }}
          />
        )}
        {doneCount > 0 && (
          <Chip
            label={`${doneCount} 完成`}
            size="small"
            sx={{ height: 16, fontSize: 9, bgcolor: alpha("#22c55e", 0.12), color: "#22c55e" }}
          />
        )}
        {errorCount > 0 && (
          <Chip
            label={`${errorCount} 失败`}
            size="small"
            sx={{ height: 16, fontSize: 9, bgcolor: alpha("#ef4444", 0.12), color: "#ef4444" }}
          />
        )}
      </Stack>
      <Stack spacing={0.4}>
        {teamJobs.map((job) => {
          const stateColor =
            job.state === "running"
              ? "#8b5cf6"
              : job.state === "done"
                ? "#22c55e"
                : job.state === "error"
                  ? "#ef4444"
                  : "#9ca3af";
          return (
            <Stack key={job.id} direction="row" alignItems="center" spacing={0.5}>
              <Box
                sx={{
                  width: 6,
                  height: 6,
                  borderRadius: "50%",
                  bgcolor: stateColor,
                  flexShrink: 0,
                  ...(job.state === "running"
                    ? { animation: "pulse 1.5s ease-in-out infinite" }
                    : {}),
                  "@keyframes pulse": {
                    "0%, 100%": { opacity: 1 },
                    "50%": { opacity: 0.4 },
                  },
                }}
              />
              <Typography
                variant="caption"
                sx={{ fontSize: 10, color: "text.secondary", flex: 1, minWidth: 0 }}
                noWrap
              >
                {job.label}
              </Typography>
            </Stack>
          );
        })}
      </Stack>
    </Box>
  );
}

// ─── Single Team session card ────────────────────────────────────────────────

interface TeamCardProps {
  session: TeamSessionInfo;
  projectRoot: string;
  onCleared: () => void;
}

const TEAM_PHASE_LABELS: Record<string, string> = {
  decomposing: "分解中",
  executing: "执行中",
  aggregating: "聚合中",
  complete: "已完成",
  failed: "失败",
};

function TeamCard({ session, projectRoot, onCleared }: TeamCardProps) {
  const [clearing, setClearing] = useState(false);
  const [expanded, setExpanded] = useState(false);
  const [board, setBoard] = useState<BlackboardDto | null>(null);

  // Poll blackboard while session is active
  useEffect(() => {
    if (session.phase === "complete" || session.phase === "failed") return;
    const fetch = async () => {
      try {
        const b = await invoke<BlackboardDto | null>("get_blackboard", {
          projectRoot,
          sessionId: session.session_id,
        });
        setBoard(b);
      } catch { /* silent */ }
    };
    void fetch();
    const id = window.setInterval(() => void fetch(), 4000);
    return () => window.clearInterval(id);
  }, [projectRoot, session.session_id, session.phase]);

  const progress =
    session.subtask_count > 0
      ? (session.completed_count / session.subtask_count) * 100
      : 0;
  const color =
    session.phase === "failed"
      ? "#ef4444"
      : session.phase === "complete"
        ? "#10b981"
        : "#8b5cf6";

  const handleClear = async () => {
    setClearing(true);
    try {
      await invoke("clear_team_session", {
        projectRoot,
        sessionId: session.session_id,
      });
      await invoke("clear_blackboard", { projectRoot, sessionId: session.session_id });
      onCleared();
    } catch {
      /* ignore */
    } finally {
      setClearing(false);
    }
  };

  return (
    <Fade in timeout={250}>
      <Box
        sx={{
          border: 1,
          borderColor: alpha(color, 0.2),
          borderRadius: 1.5,
          bgcolor: alpha(color, 0.03),
          overflow: "hidden",
        }}
      >
        <Stack
          direction="row"
          alignItems="flex-start"
          spacing={0.75}
          sx={{ px: 1.25, pt: 1, pb: 0.75 }}
        >
          <Groups fontSize="small" sx={{ color, mt: 0.1, flexShrink: 0 }} />
          <Box sx={{ flex: 1, minWidth: 0 }}>
            <Stack direction="row" alignItems="center" spacing={0.5}>
              <Chip
                label={TEAM_PHASE_LABELS[session.phase] ?? session.phase}
                size="small"
                sx={{
                  height: 17,
                  fontSize: 10,
                  bgcolor: alpha(color, 0.12),
                  color,
                  fontWeight: 600,
                }}
              />
              <Typography variant="caption" color="text.disabled" sx={{ fontSize: 10 }}>
                {session.completed_count}/{session.subtask_count} 子任务
              </Typography>
              {session.running_count > 0 && (
                <Chip
                  label={`${session.running_count} 运行中`}
                  size="small"
                  sx={{ height: 14, fontSize: 9, bgcolor: alpha("#8b5cf6", 0.1), color: "#8b5cf6" }}
                />
              )}
            </Stack>
            <Typography
              variant="body2"
              sx={{
                fontSize: 12,
                mt: 0.25,
                lineHeight: 1.35,
                overflow: "hidden",
                textOverflow: "ellipsis",
                display: "-webkit-box",
                WebkitLineClamp: 2,
                WebkitBoxOrient: "vertical",
              }}
              title={session.goal}
            >
              {session.goal}
            </Typography>
          </Box>
          <Stack direction="row" alignItems="center" spacing={0}>
            {board && board.entries.length > 0 && (
              <Tooltip title={expanded ? "收起结果" : "展开黑板结果"}>
                <IconButton size="small" onClick={() => setExpanded((v) => !v)} sx={{ p: 0.25 }}>
                  {expanded ? <ExpandLess fontSize="small" /> : <ExpandMore fontSize="small" />}
                </IconButton>
              </Tooltip>
            )}
            <Tooltip title="清除状态文件">
              <IconButton
                size="small"
                onClick={handleClear}
                disabled={clearing}
                sx={{ p: 0.25, color: "text.disabled" }}
              >
                <DeleteOutline fontSize="small" />
              </IconButton>
            </Tooltip>
          </Stack>
        </Stack>

        {session.subtask_count > 0 && (
          <Box sx={{ px: 1.25, pb: 0.75 }}>
            <Stack direction="row" alignItems="center" spacing={0.75} sx={{ mb: 0.4 }}>
              <LinearProgress
                variant="determinate"
                value={progress}
                sx={{
                  flex: 1,
                  height: 4,
                  borderRadius: 2,
                  bgcolor: alpha(color, 0.12),
                  "& .MuiLinearProgress-bar": { bgcolor: color, borderRadius: 2 },
                }}
              />
              <Typography variant="caption" color="text.disabled" sx={{ fontSize: 10, flexShrink: 0 }}>
                {session.completed_count}/{session.subtask_count}
              </Typography>
            </Stack>
          </Box>
        )}

        {/* Blackboard entries — streaming partial results */}
        <Collapse in={expanded && !!board && board.entries.length > 0}>
          <Box sx={{ px: 1.25, pb: 1 }}>
            <Typography variant="caption" sx={{ fontSize: 9, color: "text.disabled", textTransform: "uppercase", letterSpacing: 0.5 }}>
              黑板 · 已完成结果
            </Typography>
            <Stack spacing={0.5} sx={{ mt: 0.4 }}>
              {(board?.entries ?? []).map((entry, i) => (
                <Box
                  key={i}
                  sx={{
                    p: 0.75,
                    borderRadius: 1,
                    bgcolor: alpha(color, 0.05),
                    border: 1,
                    borderColor: alpha(color, 0.1),
                  }}
                >
                  <Stack direction="row" alignItems="center" spacing={0.5} sx={{ mb: 0.25 }}>
                    <CheckCircleOutline sx={{ fontSize: 10, color: "#22c55e", flexShrink: 0 }} />
                    <Typography variant="caption" sx={{ fontSize: 9, color, fontWeight: 600 }}>
                      [{entry.subtask_id}] {entry.agent_type}
                    </Typography>
                  </Stack>
                  <Typography
                    variant="caption"
                    sx={{
                      fontSize: 10,
                      color: "text.secondary",
                      display: "-webkit-box",
                      WebkitLineClamp: 3,
                      WebkitBoxOrient: "vertical",
                      overflow: "hidden",
                      lineHeight: 1.4,
                    }}
                  >
                    {entry.value}
                  </Typography>
                </Box>
              ))}
            </Stack>
          </Box>
        </Collapse>
      </Box>
    </Fade>
  );
}

// ─── Main panel ──────────────────────────────────────────────────────────────

interface RalphTeamStatusPanelProps {
  /** Project root path — used to scope Ralph session queries */
  projectRoot?: string;
}

export function RalphTeamStatusPanel({ projectRoot }: RalphTeamStatusPanelProps) {
  const [ralphSessions, setRalphSessions] = useState<RalphSessionInfo[]>([]);
  const [teamSessions, setTeamSessions] = useState<TeamSessionInfo[]>([]);
  const backgroundJobs = useActivityStore((s) => s.backgroundJobs);

  const fetchSessions = useCallback(async () => {
    if (!projectRoot) return;
    try {
      const [ralph, team] = await Promise.all([
        invoke<RalphSessionInfo[]>("list_ralph_sessions", { projectRoot }),
        invoke<TeamSessionInfo[]>("list_team_sessions", { projectRoot }),
      ]);
      setRalphSessions(ralph ?? []);
      setTeamSessions(team ?? []);
    } catch {
      // Backend not ready or no state dir — silent
    }
  }, [projectRoot]);

  // Poll every 4 seconds while there are active sessions or background jobs running
  useEffect(() => {
    void fetchSessions();
    const hasRunningJobs = backgroundJobs.some((j) => j.state === "running");
    const totalSessions = ralphSessions.length + teamSessions.length;
    const interval = totalSessions > 0 || hasRunningJobs ? 4000 : 15000;
    const id = window.setInterval(() => void fetchSessions(), interval);
    return () => window.clearInterval(id);
  }, [fetchSessions, ralphSessions.length, teamSessions.length, backgroundJobs]);

  const teamJobs = backgroundJobs.filter(
    (j) =>
      j.label.startsWith("executor") ||
      j.label.startsWith("worker") ||
      j.label.startsWith("subtask"),
  );

  const hasAnything = ralphSessions.length > 0 || teamSessions.length > 0 || teamJobs.length > 0;
  if (!hasAnything) return null;

  return (
    <Box
      sx={{
        flexShrink: 0,
        borderTop: 1,
        borderColor: "divider",
        px: 1.25,
        py: 1,
      }}
    >
      {/* Section header */}
      <Stack direction="row" alignItems="center" spacing={0.75} sx={{ mb: 0.75 }}>
        <Loop fontSize="small" sx={{ color: "#6366f1", fontSize: 14 }} />
        <Typography variant="body2" fontWeight={600} sx={{ fontSize: 11 }}>
          持久任务
        </Typography>
        {ralphSessions.length > 0 && (
          <Chip
            label={`Ralph ×${ralphSessions.length}`}
            size="small"
            sx={{ height: 16, fontSize: 9 }}
          />
        )}
        {teamSessions.length > 0 && (
          <Chip
            label={`Team ×${teamSessions.length}`}
            size="small"
            sx={{ height: 16, fontSize: 9, bgcolor: alpha("#8b5cf6", 0.1), color: "#8b5cf6" }}
          />
        )}
      </Stack>

      <Stack spacing={0.75}>
        {/* Ralph sessions */}
        {ralphSessions.map((s) => (
          <RalphCard
            key={s.session_id}
            session={s}
            projectRoot={projectRoot ?? ""}
            onCleared={() => void fetchSessions()}
          />
        ))}

        {/* Team sessions from backend state */}
        {teamSessions.map((s) => (
          <TeamCard
            key={s.session_id}
            session={s}
            projectRoot={projectRoot ?? ""}
            onCleared={() => void fetchSessions()}
          />
        ))}

        {/* Team workers (live background jobs) */}
        <TeamWorkersStrip />
      </Stack>
    </Box>
  );
}
