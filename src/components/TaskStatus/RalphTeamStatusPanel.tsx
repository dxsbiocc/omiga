/**
 * Ralph / Team Status Panel
 *
 * Polls the backend for active Ralph sessions and displays their phase,
 * iteration progress, todo completion, and stuck-error warnings.
 * Also surfaces Team mode parallel workers from the activity store's
 * backgroundJobs list (jobs with agent-type-style labels).
 */

import { useState, useEffect, useCallback, useMemo } from "react";
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
import {
  aggregateReviewerVerdicts,
  reviewerVerdictColor,
  type BackgroundAgentTaskRow,
  type ReviewerVerdictChip,
} from "../../utils/reviewerVerdict";
import { normalizeAgentDisplayName } from "../../state/agentStore";
import { compactLabel, isLabelCompacted } from "../../utils/compactLabel";
import { MarkdownText, MarkdownTextViewer } from "../MarkdownText";
import { RightDetailDrawer } from "../RightDetailDrawer";
import { filterPersistentSessionsBySessionId } from "./persistentSessionScope";

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

type PersistentDetail = {
  title: string;
  subtitle?: string;
  body: string;
  chips?: string[];
};

function PersistentDetailDrawer({
  detail,
  onClose,
}: {
  detail: PersistentDetail | null;
  onClose: () => void;
}) {
  return (
    <RightDetailDrawer
      open={Boolean(detail)}
      onClose={onClose}
      title="阶段详情"
      subtitle={detail?.title ?? "详情"}
      width={480}
      titleWeight={700}
      titleAlign="flex-start"
    >
      <Box
        sx={{
          mb: 2,
          p: 1.5,
          borderRadius: 2,
          bgcolor: (theme) => alpha(theme.palette.secondary.main, 0.06),
          border: 1,
          borderColor: (theme) =>
            alpha(
              theme.palette.mode === "dark"
                ? theme.palette.common.white
                : theme.palette.common.black,
              0.08,
            ),
        }}
      >
        <Typography variant="caption" color="secondary" fontWeight={700}>
          黑板结果
        </Typography>
        {detail?.subtitle ? (
          <Typography
            variant="body2"
            color="text.secondary"
            sx={{ whiteSpace: "pre-wrap", wordBreak: "break-word", mt: 0.5 }}
          >
            {detail.subtitle}
          </Typography>
        ) : null}
        {detail?.chips && detail.chips.length > 0 ? (
          <Stack direction="row" spacing={0.5} flexWrap="wrap" useFlexGap sx={{ mt: 1 }}>
            {detail.chips.map((chip) => (
              <Chip key={chip} label={chip} size="small" sx={{ height: 20, fontSize: 10 }} />
            ))}
          </Stack>
        ) : null}
      </Box>

      <Box
        sx={{
          p: 1.5,
          borderRadius: 2,
          bgcolor: (theme) => alpha(theme.palette.warning.main, 0.06),
          border: 1,
          borderColor: (theme) =>
            alpha(
              theme.palette.mode === "dark"
                ? theme.palette.common.white
                : theme.palette.common.black,
              0.08,
            ),
        }}
      >
        <Typography variant="caption" color="warning.dark" fontWeight={700}>
          完整内容
        </Typography>
        <Box sx={{ mt: 0.75 }}>
          <MarkdownTextViewer>{detail?.body ?? ""}</MarkdownTextViewer>
        </Box>
      </Box>
    </RightDetailDrawer>
  );
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

interface AutopilotSessionInfo {
  session_id: string;
  goal: string;
  phase: string;
  qa_cycles: number;
  max_qa_cycles: number;
  todos_completed: string[];
  todos_pending: string[];
  started_at: string;
  updated_at: string;
}

interface ModeLaneInfo {
  session_id: string;
  mode: string;
  lane_id: string;
  preferred_agent_type?: string | null;
  supplemental_agent_types: string[];
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

function ReviewerSummary({
  verdicts,
}: {
  verdicts: ReviewerVerdictChip[];
}) {
  const [selectedVerdict, setSelectedVerdict] = useState<ReviewerVerdictChip | null>(null);
  if (verdicts.length === 0) return null;
  const headline = verdicts.some((v) => v.verdict === "reject" || v.verdict === "fail")
    ? "存在阻断性 reviewer 结论"
    : verdicts.some((v) => v.verdict === "partial")
      ? "存在部分通过 / 风险提示"
      : "reviewer 已完成";

  return (
    <>
    <Stack spacing={0.5} sx={{ mt: 0.75 }}>
      <Typography variant="caption" color="text.secondary" sx={{ fontSize: 10 }}>
        Reviewer 摘要：{headline}
      </Typography>
      {verdicts.map((v) => {
        const color = reviewerVerdictColor(v.verdict, v.severity);
        return (
          <Box
            key={`${v.agentType}-${v.summary}`}
            role="button"
            tabIndex={0}
            onClick={() => setSelectedVerdict(v)}
            onKeyDown={(event) => {
              if (event.key === "Enter" || event.key === " ") {
                event.preventDefault();
                setSelectedVerdict(v);
              }
            }}
            sx={{
              p: 0.75,
              borderRadius: 1,
              border: 1,
              borderColor: alpha(color, 0.18),
              bgcolor: alpha(color, 0.05),
              cursor: "pointer",
              transition: "background-color 0.15s ease, border-color 0.15s ease",
              "&:hover": {
                bgcolor: alpha(color, 0.1),
                borderColor: alpha(color, 0.3),
              },
              "&:focus-visible": {
                outline: `2px solid ${alpha(color, 0.45)}`,
                outlineOffset: 2,
              },
            }}
          >
            <Stack direction="row" spacing={0.5} alignItems="center" flexWrap="wrap" useFlexGap sx={{ mb: 0.25 }}>
              <Chip label={normalizeAgentDisplayName(v.agentType)} size="small" sx={{ height: 16, fontSize: 9 }} />
              <Chip label={v.severity} size="small" sx={{ height: 16, fontSize: 9, bgcolor: alpha(color, 0.12), color }} />
              <Chip label={v.verdict} size="small" variant="outlined" sx={{ height: 16, fontSize: 9, borderColor: alpha(color, 0.3), color }} />
            </Stack>
            <Typography variant="caption" sx={{ fontSize: 10, color: "text.secondary" }}>
              {v.summary}
            </Typography>
            <Typography variant="caption" sx={{ display: "block", mt: 0.35, fontSize: 9, color, fontWeight: 700 }}>
              详情
            </Typography>
          </Box>
        );
      })}
    </Stack>
    <RightDetailDrawer
      open={selectedVerdict !== null}
      onClose={() => setSelectedVerdict(null)}
      title="Reviewer 结论"
      subtitle={
        selectedVerdict
          ? `${normalizeAgentDisplayName(selectedVerdict.agentType)} · ${selectedVerdict.verdict.toUpperCase()}`
          : undefined
      }
      width={500}
      titleWeight={700}
      titleAlign="flex-start"
    >
      {selectedVerdict && (() => {
        const color = reviewerVerdictColor(selectedVerdict.verdict, selectedVerdict.severity);
        return (
          <Stack spacing={1.5}>
            <Box
              sx={{
                p: 1.25,
                borderRadius: 2,
                border: 1,
                borderColor: alpha(color, 0.22),
                bgcolor: alpha(color, 0.06),
              }}
            >
              <Stack direction="row" spacing={0.5} flexWrap="wrap" useFlexGap sx={{ mb: 0.75 }}>
                <Chip label={normalizeAgentDisplayName(selectedVerdict.agentType)} size="small" sx={{ height: 20, fontSize: 10 }} />
                <Chip label={selectedVerdict.verdict.toUpperCase()} size="small" sx={{ height: 20, fontSize: 10, bgcolor: alpha(color, 0.14), color, fontWeight: 700 }} />
                <Chip label={selectedVerdict.severity.toUpperCase()} size="small" variant="outlined" sx={{ height: 20, fontSize: 10, borderColor: alpha(color, 0.32), color }} />
              </Stack>
              {selectedVerdict.taskDescription && (
                <Typography variant="body2" color="text.secondary" sx={{ whiteSpace: "pre-wrap", wordBreak: "break-word" }}>
                  {selectedVerdict.taskDescription}
                </Typography>
              )}
              <Typography variant="body2" sx={{ mt: 1, color, fontWeight: 700, whiteSpace: "pre-wrap", wordBreak: "break-word" }}>
                {selectedVerdict.summary}
              </Typography>
            </Box>
            <Box
              sx={{
                p: 1.25,
                borderRadius: 2,
                border: 1,
                borderColor: alpha(color, 0.12),
                bgcolor: alpha(color, 0.04),
              }}
            >
              <Typography variant="caption" color="text.secondary" fontWeight={700}>
                原始 reviewer 输出
              </Typography>
              <Box sx={{ mt: 0.75 }}>
                <MarkdownTextViewer>{selectedVerdict.rawText || selectedVerdict.summary}</MarkdownTextViewer>
              </Box>
            </Box>
          </Stack>
        );
      })()}
    </RightDetailDrawer>
    </>
  );
}

function LaneSummary({
  lane,
  color,
}: {
  lane: ModeLaneInfo | null;
  color: string;
}) {
  if (!lane) return null;
  const laneIdShort = compactLabel(lane.lane_id, 18);
  const laneIdCompacted = isLabelCompacted(lane.lane_id, laneIdShort);
  const preferredRole = lane.preferred_agent_type
    ? normalizeAgentDisplayName(lane.preferred_agent_type)
    : null;
  const preferredRoleShort = preferredRole ? compactLabel(preferredRole, 16) : null;
  const preferredRoleCompacted =
    preferredRole != null &&
    preferredRoleShort != null &&
    isLabelCompacted(preferredRole, preferredRoleShort);
  const supplementalDisplay = lane.supplemental_agent_types
    .map(normalizeAgentDisplayName)
    .map((name) => compactLabel(name, 14))
    .join(" / ");
  const supplementalRaw = lane.supplemental_agent_types
    .map(normalizeAgentDisplayName)
    .join(" / ");
  const supplementalCompacted = isLabelCompacted(
    supplementalRaw,
    supplementalDisplay,
  );

  const laneChip = (
    <Chip
      label={laneIdShort}
      size="small"
      sx={{
        height: 16,
        fontSize: 9,
        bgcolor: alpha(color, 0.1),
        color,
        fontWeight: 600,
      }}
    />
  );

  return (
    <Stack spacing={0.5} sx={{ mt: 0.75 }}>
      <Stack direction="row" spacing={0.5} alignItems="center" flexWrap="wrap" useFlexGap>
        {laneIdCompacted ? (
          <Tooltip title={lane.lane_id}>
            <Box>{laneChip}</Box>
          </Tooltip>
        ) : (
          laneChip
        )}
        {preferredRole && preferredRoleShort && (
          preferredRoleCompacted ? (
            <Tooltip title={`主角色: ${preferredRole}`}>
              <Box>
                <Chip
                  label={`主角色: ${preferredRoleShort}`}
                  size="small"
                  variant="outlined"
                  sx={{ height: 16, fontSize: 9 }}
                />
              </Box>
            </Tooltip>
          ) : (
            <Chip
              label={`主角色: ${preferredRoleShort}`}
              size="small"
              variant="outlined"
              sx={{ height: 16, fontSize: 9 }}
            />
          )
        )}
      </Stack>
      {lane.supplemental_agent_types.length > 0 && (
        supplementalCompacted ? (
          <Tooltip title={supplementalRaw}>
            <Typography variant="caption" color="text.secondary" sx={{ fontSize: 10 }}>
              辅助角色：{supplementalDisplay}
            </Typography>
          </Tooltip>
        ) : (
          <Typography variant="caption" color="text.secondary" sx={{ fontSize: 10 }}>
            辅助角色：{supplementalDisplay}
          </Typography>
        )
      )}
    </Stack>
  );
}

function ClickableTodoRow({
  text,
  done,
  color,
  onOpen,
}: {
  text: string;
  done: boolean;
  color: string;
  onOpen: () => void;
}) {
  const mutedColor = done ? "text.secondary" : "text.disabled";
  return (
    <Box
      role="button"
      tabIndex={0}
      onClick={onOpen}
      onKeyDown={(event) => {
        if (event.key === "Enter" || event.key === " ") {
          event.preventDefault();
          onOpen();
        }
      }}
      sx={{
        display: "flex",
        alignItems: "flex-start",
        gap: 0.5,
        p: 0.45,
        borderRadius: 1,
        cursor: "pointer",
        transition: "background-color 0.15s ease",
        "&:hover": {
          bgcolor: alpha(color, 0.07),
        },
        "&:focus-visible": {
          outline: `2px solid ${alpha(color, 0.4)}`,
          outlineOffset: 1,
        },
      }}
    >
      {done ? (
        <CheckCircleOutline
          sx={{ fontSize: 12, color: "#22c55e", mt: 0.15, flexShrink: 0 }}
        />
      ) : (
        <Box
          sx={{
            width: 12,
            height: 12,
            borderRadius: "50%",
            border: "1.5px solid",
            borderColor: alpha(color, 0.4),
            mt: 0.2,
            flexShrink: 0,
          }}
        />
      )}
      <Box sx={{ flex: 1, minWidth: 0 }}>
        <MarkdownText compact color={mutedColor}>
          {text}
        </MarkdownText>
      </Box>
      <Typography
        variant="caption"
        sx={{ fontSize: 9, color, fontWeight: 700, flexShrink: 0, mt: 0.05 }}
      >
        详情
      </Typography>
    </Box>
  );
}

// ─── Single Ralph session card ───────────────────────────────────────────────

interface RalphCardProps {
  session: RalphSessionInfo;
  lane: ModeLaneInfo | null;
  projectRoot: string;
  onCleared: () => void;
}

function RalphCard({ session, lane, projectRoot, onCleared }: RalphCardProps) {
  const [expanded, setExpanded] = useState(false);
  const [clearing, setClearing] = useState(false);
  const [reviewerVerdicts, setReviewerVerdicts] = useState<ReviewerVerdictChip[]>([]);
  const [detail, setDetail] = useState<PersistentDetail | null>(null);

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

  useEffect(() => {
    let cancelled = false;
    const load = async () => {
      try {
        const tasks = await invoke<BackgroundAgentTaskRow[]>("list_session_background_tasks", {
          sessionId: session.session_id,
        });
        if (cancelled) return;
        const verdicts = (tasks ?? [])
          .filter((t) => t.result_summary || t.error_message);
        setReviewerVerdicts(aggregateReviewerVerdicts(verdicts));
      } catch {
        if (!cancelled) setReviewerVerdicts([]);
      }
    };
    void load();
    const id = window.setInterval(() => void load(), 4000);
    return () => {
      cancelled = true;
      window.clearInterval(id);
    };
  }, [session.session_id]);

  const openTodoDetail = (args: { text: string; done: boolean; index: number }) => {
    setDetail({
      title: `${args.done ? "已完成" : "待处理"}待办 #${args.index + 1}`,
      subtitle: `Ralph · ${phaseLabel(session.phase)} · 第 ${session.iteration} 轮`,
      body: args.text,
      chips: [
        args.done ? "已完成" : "待处理",
        phaseLabel(session.phase),
        `${completedCount}/${totalTodos || 0}`,
      ],
    });
  };

  return (
    <>
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
            <LaneSummary lane={lane} color={color} />
            <ReviewerSummary verdicts={reviewerVerdicts} />
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
                  <ClickableTodoRow
                    key={i}
                    text={t}
                    done
                    color={color}
                    onOpen={() => openTodoDetail({ text: t, done: true, index: i })}
                  />
                ))}
              </Stack>
            )}
            {session.todos_pending.map((t, i) => (
              <ClickableTodoRow
                key={i}
                text={t}
                done={false}
                color={color}
                onOpen={() => openTodoDetail({ text: t, done: false, index: i })}
              />
            ))}
          </Box>
        </Collapse>
      </Box>
    </Fade>
    <PersistentDetailDrawer detail={detail} onClose={() => setDetail(null)} />
    </>
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
          const shortLabel = compactLabel(job.label, 30);
          const labelCompacted = isLabelCompacted(job.label, shortLabel);
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
              {labelCompacted ? (
                <Tooltip title={job.label}>
                  <Typography
                    variant="caption"
                    sx={{ fontSize: 10, color: "text.secondary", flex: 1, minWidth: 0 }}
                    noWrap
                  >
                    {shortLabel}
                  </Typography>
                </Tooltip>
              ) : (
                <Typography
                  variant="caption"
                  sx={{ fontSize: 10, color: "text.secondary", flex: 1, minWidth: 0 }}
                  noWrap
                >
                  {shortLabel}
                </Typography>
              )}
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
  lane: ModeLaneInfo | null;
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

function TeamCard({ session, lane, projectRoot, onCleared }: TeamCardProps) {
  const [clearing, setClearing] = useState(false);
  const [expanded, setExpanded] = useState(false);
  const [board, setBoard] = useState<BlackboardDto | null>(null);
  const [reviewerVerdicts, setReviewerVerdicts] = useState<ReviewerVerdictChip[]>([]);
  const [detail, setDetail] = useState<PersistentDetail | null>(null);

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

  useEffect(() => {
    let cancelled = false;
    const load = async () => {
      try {
        const tasks = await invoke<BackgroundAgentTaskRow[]>("list_session_background_tasks", {
          sessionId: session.session_id,
        });
        if (cancelled) return;
        const verdicts = (tasks ?? [])
          .filter((t) => t.result_summary || t.error_message);
        setReviewerVerdicts(aggregateReviewerVerdicts(verdicts));
      } catch {
        if (!cancelled) setReviewerVerdicts([]);
      }
    };
    void load();
    const id = window.setInterval(() => void load(), 4000);
    return () => {
      cancelled = true;
      window.clearInterval(id);
    };
  }, [session.session_id]);

  return (
    <>
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
            <LaneSummary lane={lane} color={color} />
            <ReviewerSummary verdicts={reviewerVerdicts} />
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
                  role="button"
                  tabIndex={0}
                  onClick={() =>
                    setDetail({
                      title: `[${entry.subtask_id}] ${normalizeAgentDisplayName(entry.agent_type)}`,
                      subtitle: `黑板结果 · ${entry.key} · ${entry.posted_at}`,
                      body: entry.value,
                      chips: [
                        TEAM_PHASE_LABELS[session.phase] ?? session.phase,
                        `${session.completed_count}/${session.subtask_count} 子任务`,
                      ],
                    })
                  }
                  onKeyDown={(event) => {
                    if (event.key === "Enter" || event.key === " ") {
                      event.preventDefault();
                      setDetail({
                        title: `[${entry.subtask_id}] ${normalizeAgentDisplayName(entry.agent_type)}`,
                        subtitle: `黑板结果 · ${entry.key} · ${entry.posted_at}`,
                        body: entry.value,
                        chips: [
                          TEAM_PHASE_LABELS[session.phase] ?? session.phase,
                          `${session.completed_count}/${session.subtask_count} 子任务`,
                        ],
                      });
                    }
                  }}
                  sx={{
                    p: 0.75,
                    borderRadius: 1,
                    bgcolor: alpha(color, 0.05),
                    border: 1,
                    borderColor: alpha(color, 0.1),
                    cursor: "pointer",
                    transition: "border-color 0.15s ease, background-color 0.15s ease",
                    "&:hover": {
                      bgcolor: alpha(color, 0.09),
                      borderColor: alpha(color, 0.28),
                    },
                    "&:focus-visible": {
                      outline: `2px solid ${alpha(color, 0.45)}`,
                      outlineOffset: 2,
                    },
                  }}
                >
                  <Stack direction="row" alignItems="center" spacing={0.5} sx={{ mb: 0.25 }}>
                    <CheckCircleOutline sx={{ fontSize: 10, color: "#22c55e", flexShrink: 0 }} />
                    <Typography variant="caption" sx={{ fontSize: 9, color, fontWeight: 600, flex: 1, minWidth: 0 }}>
                      [{entry.subtask_id}] {normalizeAgentDisplayName(entry.agent_type)}
                    </Typography>
                    <Typography variant="caption" sx={{ fontSize: 9, color, fontWeight: 600, flexShrink: 0 }}>
                      详情
                    </Typography>
                  </Stack>
                  <MarkdownText compact>{entry.value}</MarkdownText>
                </Box>
              ))}
            </Stack>
          </Box>
        </Collapse>
        </Box>
      </Fade>
      <PersistentDetailDrawer detail={detail} onClose={() => setDetail(null)} />
    </>
  );
}

// ─── Single Autopilot session card ──────────────────────────────────────────

const AUTOPILOT_PHASE_LABELS: Record<string, string> = {
  intake: "接收中",
  interview: "澄清中",
  expansion: "问题展开",
  design: "分析设计",
  plan: "分析计划",
  implementation: "分析执行",
  qa: "论证中",
  validation: "审查中",
  complete: "已完成",
};

function AutopilotCard({
  session,
  lane,
  projectRoot,
  onCleared,
}: {
  session: AutopilotSessionInfo;
  lane: ModeLaneInfo | null;
  projectRoot: string;
  onCleared: () => void;
}) {
  const [expanded, setExpanded] = useState(false);
  const [clearing, setClearing] = useState(false);
  const [reviewerVerdicts, setReviewerVerdicts] = useState<ReviewerVerdictChip[]>([]);
  const [detail, setDetail] = useState<PersistentDetail | null>(null);
  const totalTodos = session.todos_completed.length + session.todos_pending.length;
  const progress = totalTodos > 0 ? (session.todos_completed.length / totalTodos) * 100 : 0;
  const color = session.phase === "complete" ? "#10b981" : "#2563eb";

  const handleClear = async () => {
    setClearing(true);
    try {
      await invoke("clear_autopilot_session", {
        projectRoot,
        sessionId: session.session_id,
      });
      onCleared();
    } catch {
      /* ignore */
    } finally {
      setClearing(false);
    }
  };

  useEffect(() => {
    let cancelled = false;
    const load = async () => {
      try {
        const tasks = await invoke<BackgroundAgentTaskRow[]>("list_session_background_tasks", {
          sessionId: session.session_id,
        });
        if (cancelled) return;
        const verdicts = (tasks ?? [])
          .filter((t) => t.result_summary || t.error_message);
        setReviewerVerdicts(aggregateReviewerVerdicts(verdicts));
      } catch {
        if (!cancelled) setReviewerVerdicts([]);
      }
    };
    void load();
    const id = window.setInterval(() => void load(), 4000);
    return () => {
      cancelled = true;
      window.clearInterval(id);
    };
  }, [session.session_id]);

  const openTodoDetail = (args: { text: string; done: boolean; index: number }) => {
    setDetail({
      title: `${args.done ? "已完成" : "待处理"}阶段 #${args.index + 1}`,
      subtitle: `Autopilot · ${AUTOPILOT_PHASE_LABELS[session.phase] ?? session.phase} · QA ${session.qa_cycles}/${session.max_qa_cycles}`,
      body: args.text,
      chips: [
        args.done ? "已完成" : "待处理",
        AUTOPILOT_PHASE_LABELS[session.phase] ?? session.phase,
        `${session.todos_completed.length}/${totalTodos || 0}`,
      ],
    });
  };

  return (
    <>
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
        <Stack direction="row" alignItems="flex-start" spacing={0.75} sx={{ px: 1.25, pt: 1, pb: 0.75 }}>
          <Loop fontSize="small" sx={{ color, mt: 0.1, flexShrink: 0 }} />
          <Box sx={{ flex: 1, minWidth: 0 }}>
            <Stack direction="row" alignItems="center" spacing={0.5}>
              <Chip
                label={AUTOPILOT_PHASE_LABELS[session.phase] ?? session.phase}
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
                论证 {session.qa_cycles}/{session.max_qa_cycles}
              </Typography>
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
            <LaneSummary lane={lane} color={color} />
            <ReviewerSummary verdicts={reviewerVerdicts} />
          </Box>
          <Stack direction="row" alignItems="center" spacing={0}>
            {totalTodos > 0 && (
              <Tooltip title={expanded ? "收起阶段" : "展开阶段"}>
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
        {totalTodos > 0 && (
          <Box sx={{ px: 1.25, pb: 0.75 }}>
            <Stack direction="row" alignItems="center" spacing={0.75}>
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
                {session.todos_completed.length}/{totalTodos}
              </Typography>
            </Stack>
          </Box>
        )}
        <Collapse in={expanded && totalTodos > 0}>
          <Box sx={{ px: 1.25, pb: 1 }}>
            {session.todos_completed.length > 0 && (
              <Stack spacing={0.25} sx={{ mb: 0.5 }}>
                {session.todos_completed.map((t, i) => (
                  <ClickableTodoRow
                    key={i}
                    text={t}
                    done
                    color={color}
                    onOpen={() => openTodoDetail({ text: t, done: true, index: i })}
                  />
                ))}
              </Stack>
            )}
            {session.todos_pending.map((t, i) => (
              <ClickableTodoRow
                key={i}
                text={t}
                done={false}
                color={color}
                onOpen={() => openTodoDetail({ text: t, done: false, index: i })}
              />
            ))}
          </Box>
        </Collapse>
      </Box>
    </Fade>
    <PersistentDetailDrawer detail={detail} onClose={() => setDetail(null)} />
    </>
  );
}

// ─── Main panel ──────────────────────────────────────────────────────────────

interface RalphTeamStatusPanelProps {
  /** Project root path — used to scope Ralph session queries */
  projectRoot?: string;
  /** When set, only show persistent tasks for this session. */
  sessionId?: string | null;
  /** Render inside another card/tab without extra outer divider chrome. */
  embedded?: boolean;
}

export function RalphTeamStatusPanel({
  projectRoot,
  sessionId,
  embedded = false,
}: RalphTeamStatusPanelProps) {
  const [ralphSessions, setRalphSessions] = useState<RalphSessionInfo[]>([]);
  const [autopilotSessions, setAutopilotSessions] = useState<AutopilotSessionInfo[]>([]);
  const [teamSessions, setTeamSessions] = useState<TeamSessionInfo[]>([]);
  const [modeLanes, setModeLanes] = useState<ModeLaneInfo[]>([]);
  const backgroundJobs = useActivityStore((s) => s.backgroundJobs);

  const fetchSessions = useCallback(async () => {
    if (!projectRoot) return;
    try {
      const [ralph, autopilot, team, lanes] = await Promise.all([
        invoke<RalphSessionInfo[]>("list_ralph_sessions", { projectRoot }),
        invoke<AutopilotSessionInfo[]>("list_autopilot_sessions", { projectRoot }),
        invoke<TeamSessionInfo[]>("list_team_sessions", { projectRoot }),
        invoke<ModeLaneInfo[]>("list_active_mode_lanes", { projectRoot }),
      ]);
      setRalphSessions(ralph ?? []);
      setAutopilotSessions(autopilot ?? []);
      setTeamSessions(team ?? []);
      setModeLanes(lanes ?? []);
    } catch {
      // Backend not ready or no state dir — silent
    }
  }, [projectRoot]);

  // Poll every 4 seconds while there are active sessions or background jobs running
  useEffect(() => {
    void fetchSessions();
    const hasRunningJobs = backgroundJobs.some((j) => j.state === "running");
    const totalSessions = ralphSessions.length + autopilotSessions.length + teamSessions.length;
    const interval = totalSessions > 0 || hasRunningJobs ? 4000 : 15000;
    const id = window.setInterval(() => void fetchSessions(), interval);
    return () => window.clearInterval(id);
  }, [fetchSessions, ralphSessions.length, autopilotSessions.length, teamSessions.length, backgroundJobs]);

  const laneFor = useCallback(
    (mode: string, sessionId: string) =>
      modeLanes.find((lane) => lane.mode === mode && lane.session_id === sessionId) ?? null,
    [modeLanes],
  );

  const visibleRalphSessions = useMemo(
    () => filterPersistentSessionsBySessionId(ralphSessions, sessionId),
    [ralphSessions, sessionId],
  );
  const visibleAutopilotSessions = useMemo(
    () => filterPersistentSessionsBySessionId(autopilotSessions, sessionId),
    [autopilotSessions, sessionId],
  );
  const visibleTeamSessions = useMemo(
    () => filterPersistentSessionsBySessionId(teamSessions, sessionId),
    [sessionId, teamSessions],
  );

  const teamJobs = backgroundJobs.filter(
    (j) =>
      j.label.startsWith("executor") ||
      j.label.startsWith("worker") ||
      j.label.startsWith("subtask"),
  );

  const hasAnything =
    visibleRalphSessions.length > 0 ||
    visibleAutopilotSessions.length > 0 ||
    visibleTeamSessions.length > 0 ||
    teamJobs.length > 0;
  if (!hasAnything) return null;

  return (
    <Box
      sx={{
        flexShrink: 0,
        borderTop: embedded ? 0 : 1,
        borderColor: "divider",
        px: embedded ? 0 : 1.25,
        py: embedded ? 0 : 1,
      }}
    >
      {/* Section header */}
      <Stack direction="row" alignItems="center" spacing={0.75} sx={{ mb: 0.75 }}>
        <Loop fontSize="small" sx={{ color: "#6366f1", fontSize: 14 }} />
        <Typography variant="body2" fontWeight={600} sx={{ fontSize: 11 }}>
          持久任务
        </Typography>
        {visibleRalphSessions.length > 0 && (
          <Chip
            label={`Ralph ×${visibleRalphSessions.length}`}
            size="small"
            sx={{ height: 16, fontSize: 9 }}
          />
        )}
        {visibleAutopilotSessions.length > 0 && (
          <Chip
            label={`Autopilot ×${visibleAutopilotSessions.length}`}
            size="small"
            sx={{ height: 16, fontSize: 9, bgcolor: alpha("#2563eb", 0.1), color: "#2563eb" }}
          />
        )}
        {visibleTeamSessions.length > 0 && (
          <Chip
            label={`Team ×${visibleTeamSessions.length}`}
            size="small"
            sx={{ height: 16, fontSize: 9, bgcolor: alpha("#8b5cf6", 0.1), color: "#8b5cf6" }}
          />
        )}
      </Stack>

      <Stack spacing={0.75}>
        {/* Ralph sessions */}
        {visibleRalphSessions.map((s) => (
          <RalphCard
            key={s.session_id}
            session={s}
            lane={laneFor("ralph", s.session_id)}
            projectRoot={projectRoot ?? ""}
            onCleared={() => void fetchSessions()}
          />
        ))}

        {/* Autopilot sessions */}
        {visibleAutopilotSessions.map((s) => (
          <AutopilotCard
            key={s.session_id}
            session={s}
            lane={laneFor("autopilot", s.session_id)}
            projectRoot={projectRoot ?? ""}
            onCleared={() => void fetchSessions()}
          />
        ))}

        {/* Team sessions from backend state */}
        {visibleTeamSessions.map((s) => (
          <TeamCard
            key={s.session_id}
            session={s}
            lane={laneFor("team", s.session_id)}
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
