import { memo, useMemo } from "react";
import {
  Box,
  Button,
  Chip,
  Stack,
  Tooltip,
  Typography,
  useTheme,
} from "@mui/material";
import { alpha } from "@mui/material/styles";
import {
  CheckCircleOutline,
  HourglassBottom,
  PauseCircleOutline,
  PlayArrow,
  Science,
} from "@mui/icons-material";

export type ResearchGoalStatus =
  | "active"
  | "paused"
  | "budget_limited"
  | "complete";

export interface ResearchGoalCriteriaAudit {
  criterionId?: string;
  criterion: string;
  covered: boolean;
  evidence: string;
}

export interface ResearchGoalAudit {
  complete: boolean;
  reviewSource?: "llm" | "unknown" | string;
  confidence?: "low" | "medium" | "high" | "unknown" | string;
  finalReportReady?: boolean;
  summary: string;
  criteria: ResearchGoalCriteriaAudit[];
  missingRequirements: string[];
  nextActions: string[];
  limitations?: string[];
  conflictingEvidence?: string[];
  secondOpinion?: ResearchGoalSecondOpinion | null;
}

export interface ResearchGoalSecondOpinion {
  reviewSource?: "llm_second_opinion" | "unknown" | string;
  agreesComplete: boolean;
  confidence?: "low" | "medium" | "high" | "unknown" | string;
  summary: string;
  blockingConcerns: string[];
  requiredNextActions: string[];
}

export interface ResearchGoal {
  goalId: string;
  sessionId: string;
  objective: string;
  status: ResearchGoalStatus;
  successCriteria: string[];
  successCriterionIds?: string[];
  secondOpinionProviderEntry?: string | null;
  autoRunPolicy?: ResearchGoalAutoRunPolicy | null;
  tokenUsage?: ResearchGoalTokenUsage | null;
  maxCycles: number;
  currentCycle: number;
  evidenceRefs: string[];
  artifactRefs: string[];
  notes: string[];
  lastAudit?: ResearchGoalAudit | null;
  createdAt: string;
  updatedAt: string;
  lastRunAt?: string | null;
}

export interface ResearchGoalAutoRunPolicy {
  enabled: boolean;
  cyclesPerRun: number;
  idleDelayMs: number;
  maxElapsedMinutes?: number | null;
  maxTokens?: number | null;
  startedAt?: string | null;
}

export interface ResearchGoalTokenUsage {
  inputTokens: number;
  outputTokens: number;
  totalTokens: number;
}

export interface ResearchGoalCycle {
  cycleId: string;
  goalId: string;
  cycleIndex: number;
  request: string;
  graphId?: string | null;
  researchStatus?: string | null;
  audit: ResearchGoalAudit;
  evidenceRefs: string[];
  artifactRefs: string[];
  tokenUsage?: ResearchGoalCycleTokenUsage | null;
  createdAt: string;
}

export interface ResearchGoalCycleTokenUsage {
  researchSystem: ResearchGoalTokenUsage;
  audit: ResearchGoalTokenUsage;
  total: ResearchGoalTokenUsage;
}

export function researchGoalStatusLabel(status: ResearchGoalStatus): string {
  switch (status) {
    case "active":
      return "进行中";
    case "paused":
      return "已暂停";
    case "budget_limited":
      return "预算用尽";
    case "complete":
      return "已完成";
    default:
      return status;
  }
}

export function nextResearchGoalCommand(goal: ResearchGoal): {
  command: string;
  label: string;
} {
  switch (goal.status) {
    case "paused":
      return { command: "/goal resume", label: "恢复" };
    case "active":
      return { command: "/goal run", label: "推进" };
    case "budget_limited":
    case "complete":
      return { command: "/goal status", label: "查看状态" };
    default:
      return { command: "/goal status", label: "查看状态" };
  }
}

export function researchGoalCanAutoRun(goal: ResearchGoal): boolean {
  return (
    goal.status === "active" &&
    goal.currentCycle < goal.maxCycles &&
    !researchGoalAutoRunTokenBudgetReached(goal)
  );
}

export function buildResearchGoalAutoRunCommand(goal: ResearchGoal): string {
  const remaining = Math.max(1, goal.maxCycles - goal.currentCycle);
  const policyCycles = goal.autoRunPolicy?.cyclesPerRun;
  const cyclesPerRun =
    typeof policyCycles === "number" && Number.isFinite(policyCycles)
      ? Math.max(1, Math.min(Math.floor(policyCycles), 10))
      : 10;
  return `/goal run --cycles ${Math.min(remaining, cyclesPerRun)}`;
}

export function researchGoalAutoRunElapsedBudgetReached(
  goal: ResearchGoal,
  nowMs = Date.now(),
): boolean {
  const policy = goal.autoRunPolicy;
  const minutes = policy?.maxElapsedMinutes;
  const startedAt = policy?.startedAt;
  if (!policy?.enabled || !minutes || !startedAt) return false;
  const startedMs = Date.parse(startedAt);
  if (!Number.isFinite(startedMs)) return false;
  return nowMs - startedMs >= minutes * 60_000;
}

export function researchGoalAutoRunTokenBudgetReached(goal: ResearchGoal): boolean {
  const maxTokens = goal.autoRunPolicy?.maxTokens;
  if (!goal.autoRunPolicy?.enabled || !maxTokens) return false;
  return (goal.tokenUsage?.totalTokens ?? 0) >= maxTokens;
}

export function researchGoalShouldWaitForComposerDraft(
  draft: string,
  attachedPaths: readonly string[] = [],
  selectedPluginIds: readonly string[] = [],
): boolean {
  return (
    draft.trim().length > 0 ||
    attachedPaths.length > 0 ||
    selectedPluginIds.length > 0
  );
}

export function compactResearchGoalObjective(
  objective: string,
  maxChars = 56,
): string {
  const normalized = objective.replace(/\s+/g, " ").trim();
  const chars = [...normalized];
  if (chars.length <= maxChars) return normalized;
  return `${chars.slice(0, Math.max(1, maxChars - 1)).join("")}…`;
}

export function auditSourceLabel(audit: ResearchGoalAudit): string {
  const source =
    audit.secondOpinion && audit.secondOpinion.agreesComplete
      ? "双重 LLM 审计"
      : audit.reviewSource === "llm"
        ? "LLM 审计"
        : "审计";
  return audit.confidence && audit.confidence !== "unknown"
    ? `${source} · 置信度 ${audit.confidence}`
    : source;
}

interface ResearchGoalStatusPillProps {
  goal: ResearchGoal | null;
  onPrepareCommand?: (command: string) => void;
  onEditCriteria?: () => void;
  onOpenAuditDetails?: () => void;
  autoRunEnabled?: boolean;
  onToggleAutoRun?: () => void;
  autoRunDisabled?: boolean;
}

function statusTone(status: ResearchGoalStatus): {
  chipColor: "default" | "primary" | "success" | "warning";
  accent: "primary" | "success" | "warning";
} {
  switch (status) {
    case "complete":
      return { chipColor: "success", accent: "success" };
    case "paused":
    case "budget_limited":
      return { chipColor: "warning", accent: "warning" };
    case "active":
    default:
      return { chipColor: "primary", accent: "primary" };
  }
}

function StatusIcon({ status }: { status: ResearchGoalStatus }) {
  if (status === "complete") return <CheckCircleOutline fontSize="small" />;
  if (status === "paused") return <PauseCircleOutline fontSize="small" />;
  if (status === "budget_limited") return <HourglassBottom fontSize="small" />;
  return <Science fontSize="small" />;
}

export const ResearchGoalStatusPill = memo(function ResearchGoalStatusPill({
  goal,
  onPrepareCommand,
  onEditCriteria,
  onOpenAuditDetails,
  autoRunEnabled = false,
  onToggleAutoRun,
  autoRunDisabled = false,
}: ResearchGoalStatusPillProps) {
  const theme = useTheme();
  const action = useMemo(() => (goal ? nextResearchGoalCommand(goal) : null), [goal]);

  if (!goal || !action) return null;

  const tone = statusTone(goal.status);
  const accent = theme.palette[tone.accent].main;
  const audit = goal.lastAudit;
  const tooltip = (
    <Box sx={{ maxWidth: 420 }}>
      <Typography variant="subtitle2" sx={{ mb: 0.5 }}>
        {goal.objective}
      </Typography>
      <Typography variant="caption" component="div">
        轮次：{goal.currentCycle}/{goal.maxCycles} · 证据 {goal.evidenceRefs.length} · 产物{" "}
        {goal.artifactRefs.length}
      </Typography>
      {(goal.tokenUsage?.totalTokens ?? 0) > 0 && (
        <Typography variant="caption" component="div" sx={{ mt: 0.5 }}>
          Token：{goal.tokenUsage?.totalTokens ?? 0}
        </Typography>
      )}
      {goal.successCriteria.length > 0 && (
        <Typography variant="caption" component="div" sx={{ mt: 0.5 }}>
          成功标准：{goal.successCriteria.slice(0, 3).join("；")}
        </Typography>
      )}
      {goal.secondOpinionProviderEntry && (
        <Typography variant="caption" component="div" sx={{ mt: 0.5 }}>
          二审模型：{goal.secondOpinionProviderEntry}
        </Typography>
      )}
      {goal.autoRunPolicy?.enabled && (
        <Typography variant="caption" component="div" sx={{ mt: 0.5 }}>
          自动续跑：每次 {goal.autoRunPolicy.cyclesPerRun} 轮，空闲{" "}
          {goal.autoRunPolicy.idleDelayMs}ms
          {goal.autoRunPolicy.maxElapsedMinutes
            ? `，最长 ${goal.autoRunPolicy.maxElapsedMinutes} 分钟`
            : ""}
          {goal.autoRunPolicy.maxTokens
            ? `，最多 ${goal.autoRunPolicy.maxTokens} tokens`
            : ""}
        </Typography>
      )}
      {audit && (
        <Box sx={{ mt: 1 }}>
          <Typography variant="caption" component="div">
            最近{auditSourceLabel(audit)}：{audit.summary}
          </Typography>
          {audit.missingRequirements.length > 0 && (
            <Typography variant="caption" component="div" sx={{ mt: 0.5 }}>
              缺口：{audit.missingRequirements.slice(0, 3).join("；")}
            </Typography>
          )}
          {audit.nextActions.length > 0 && (
            <Typography variant="caption" component="div" sx={{ mt: 0.5 }}>
              下一步：{audit.nextActions.slice(0, 2).join("；")}
            </Typography>
          )}
        </Box>
      )}
    </Box>
  );

  return (
    <Box
      aria-label="科研目标状态"
      sx={{
        mb: 1,
        px: 1.25,
        py: 0.875,
        borderRadius: 2,
        border: `1px solid ${alpha(accent, 0.28)}`,
        bgcolor: alpha(accent, theme.palette.mode === "dark" ? 0.12 : 0.08),
        boxShadow: `0 8px 24px ${alpha(accent, theme.palette.mode === "dark" ? 0.14 : 0.10)}`,
      }}
    >
      <Stack
        direction={{ xs: "column", sm: "row" }}
        spacing={1}
        alignItems={{ xs: "stretch", sm: "center" }}
        justifyContent="space-between"
      >
        <Tooltip title={tooltip} arrow placement="top">
          <Stack direction="row" spacing={1} alignItems="center" minWidth={0}>
            <Chip
              icon={<StatusIcon status={goal.status} />}
              size="small"
              color={tone.chipColor}
              variant="outlined"
              label={`${researchGoalStatusLabel(goal.status)} · ${goal.currentCycle}/${goal.maxCycles}`}
              sx={{ flexShrink: 0, fontWeight: 700 }}
            />
            <Typography
              variant="body2"
              noWrap
              sx={{
                minWidth: 0,
                color: "text.primary",
                fontWeight: 600,
              }}
            >
              {compactResearchGoalObjective(goal.objective)}
            </Typography>
            {audit?.reviewSource === "llm" && (
              <Chip
                size="small"
                variant="outlined"
                label={auditSourceLabel(audit)}
                sx={{ flexShrink: 0, height: 22, fontWeight: 700 }}
              />
            )}
            {goal.secondOpinionProviderEntry && (
              <Chip
                size="small"
                variant="outlined"
                label={`二审 ${goal.secondOpinionProviderEntry}`}
                sx={{ flexShrink: 0, height: 22, fontWeight: 700 }}
              />
            )}
            {goal.autoRunPolicy?.enabled && (
              <Chip
                size="small"
                variant="outlined"
                color="warning"
                label={`自动 ${goal.autoRunPolicy.cyclesPerRun}轮`}
                sx={{ flexShrink: 0, height: 22, fontWeight: 700 }}
              />
            )}
          </Stack>
        </Tooltip>
        <Stack direction="row" spacing={0.75} justifyContent="flex-end">
          <Button
            type="button"
            size="small"
            variant="contained"
            disableElevation
            startIcon={<PlayArrow fontSize="small" />}
            onClick={() => onPrepareCommand?.(action.command)}
            sx={{ whiteSpace: "nowrap", textTransform: "none", fontWeight: 700 }}
          >
            {action.label}
          </Button>
          {onToggleAutoRun && (
            <Button
              type="button"
              size="small"
              variant={autoRunEnabled ? "contained" : "outlined"}
              color={autoRunEnabled ? "warning" : "primary"}
              disableElevation
              disabled={
                autoRunDisabled || (!autoRunEnabled && !researchGoalCanAutoRun(goal))
              }
              onClick={onToggleAutoRun}
              sx={{ whiteSpace: "nowrap", textTransform: "none", fontWeight: 700 }}
            >
              {autoRunEnabled ? "停止自动" : "自动续跑"}
            </Button>
          )}
          {onEditCriteria && (
            <Button
              type="button"
              size="small"
              variant="outlined"
              onClick={onEditCriteria}
              sx={{ whiteSpace: "nowrap", textTransform: "none", fontWeight: 700 }}
            >
              设置
            </Button>
          )}
          {audit && onOpenAuditDetails && (
            <Button
              type="button"
              size="small"
              variant="outlined"
              onClick={onOpenAuditDetails}
              sx={{ whiteSpace: "nowrap", textTransform: "none", fontWeight: 700 }}
            >
              审计详情
            </Button>
          )}
          <Button
            type="button"
            size="small"
            variant="text"
            onClick={() => onPrepareCommand?.("/goal status")}
            sx={{ whiteSpace: "nowrap", textTransform: "none", fontWeight: 700 }}
          >
            状态
          </Button>
        </Stack>
      </Stack>
    </Box>
  );
});
