import { useMemo, useState } from "react";
import {
  Box,
  Chip,
  Collapse,
  IconButton,
  MenuItem,
  Paper,
  Select,
  Stack,
  ToggleButton,
  ToggleButtonGroup,
  Tooltip,
  Typography,
  type ChipProps,
} from "@mui/material";
import { alpha, useTheme } from "@mui/material/styles";
import { KeyboardArrowDownRounded, RefreshRounded } from "@mui/icons-material";
import type { OperatorRunSummary, OperatorSummary } from "../../state/pluginStore";
import {
  operatorDisplayName,
  operatorRunIsCacheHit,
  operatorRunStatusColor,
} from "./PluginsPanel";

type TimelineStatusFilter = "all" | "success" | "failed" | "running" | "cached" | "cancelled";
type TimelineDateRange = "today" | "7d" | "30d" | "all";
type TimelineViewMode = "flat" | "grouped";
type TimelineStatus = Exclude<TimelineStatusFilter, "all"> | "other";
type TimelineRunFields = OperatorRunSummary & {
  alias?: string | null;
  kind?: string | null;
  parentExecutionId?: string | null;
  startedAt?: string | null;
  endedAt?: string | null;
};
type ChainTimelineItem = {
  type: "chain";
  parentExecutionId: string;
  steps: OperatorRunSummary[];
  matchingRunIds: Set<string>;
  latestTimestamp: number;
};
type StandaloneTimelineItem = {
  type: "run";
  run: OperatorRunSummary;
  latestTimestamp: number;
};
type GroupedTimelineItem = ChainTimelineItem | StandaloneTimelineItem;

const statusOptions: Array<{ value: TimelineStatusFilter; label: string }> = [
  { value: "all", label: "All" },
  { value: "success", label: "Success" },
  { value: "failed", label: "Failed" },
  { value: "running", label: "Running" },
  { value: "cached", label: "Cached" },
  { value: "cancelled", label: "Cancelled" },
];

const dateRangeOptions: Array<{ value: TimelineDateRange; label: string }> = [
  { value: "today", label: "Today" },
  { value: "7d", label: "7d" },
  { value: "30d", label: "30d" },
  { value: "all", label: "All" },
];

const viewOptions: Array<{ value: TimelineViewMode; label: string }> = [
  { value: "flat", label: "Flat" },
  { value: "grouped", label: "Grouped" },
];

function runAlias(run: OperatorRunSummary): string {
  const fields = run as TimelineRunFields;
  return fields.alias?.trim() || run.operatorAlias?.trim() || run.operatorId?.trim() || "unknown";
}

function runTimestampText(run: OperatorRunSummary): string | null {
  const fields = run as TimelineRunFields;
  return fields.endedAt?.trim() || fields.startedAt?.trim() || run.updatedAt?.trim() || null;
}

function parseRunTimestamp(text?: string | null): number {
  const trimmed = text?.trim();
  if (!trimmed) return Number.NEGATIVE_INFINITY;
  const timestamp = new Date(trimmed).getTime();
  return Number.isNaN(timestamp) ? Number.NEGATIVE_INFINITY : timestamp;
}

function firstFiniteTimestamp(...timestamps: number[]): number {
  return timestamps.find(Number.isFinite) ?? Number.NEGATIVE_INFINITY;
}

function runTimestamp(run: OperatorRunSummary): number {
  return parseRunTimestamp(runTimestampText(run));
}

function runStartTimestamp(run: OperatorRunSummary): number {
  const fields = run as TimelineRunFields;
  return firstFiniteTimestamp(parseRunTimestamp(fields.startedAt), parseRunTimestamp(run.updatedAt));
}

function runEndTimestamp(run: OperatorRunSummary): number {
  const fields = run as TimelineRunFields;
  return firstFiniteTimestamp(
    parseRunTimestamp(fields.endedAt),
    parseRunTimestamp(run.updatedAt),
    parseRunTimestamp(fields.startedAt),
  );
}

function formatRelativeTime(ms: number): string {
  if (!Number.isFinite(ms)) return "unknown time";
  const diff = Math.max(0, Date.now() - ms);
  if (diff < 60_000) return "just now";
  if (diff < 3_600_000) return `${Math.floor(diff / 60_000)}m ago`;
  if (diff < 86_400_000) return `${Math.floor(diff / 3_600_000)}h ago`;
  return `${Math.floor(diff / 86_400_000)}d ago`;
}

function formatDuration(ms: number): string {
  if (!Number.isFinite(ms)) return "duration unavailable";
  const seconds = Math.max(0, Math.round(ms / 1000));
  if (seconds < 60) return `${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  const remainingSeconds = seconds % 60;
  if (minutes < 60) return remainingSeconds ? `${minutes}m ${remainingSeconds}s` : `${minutes}m`;
  const hours = Math.floor(minutes / 60);
  const remainingMinutes = minutes % 60;
  if (hours < 24) return remainingMinutes ? `${hours}h ${remainingMinutes}m` : `${hours}h`;
  const days = Math.floor(hours / 24);
  const remainingHours = hours % 24;
  return remainingHours ? `${days}d ${remainingHours}h` : `${days}d`;
}

function timelineDateStart(range: TimelineDateRange, now: number): number | null {
  if (range === "all") return null;
  if (range === "today") {
    const today = new Date(now);
    today.setHours(0, 0, 0, 0);
    return today.getTime();
  }
  return now - (range === "7d" ? 7 : 30) * 86_400_000;
}

function normalizedRunKind(run: OperatorRunSummary): string {
  const fields = run as TimelineRunFields;
  return (run.runKind || fields.kind || "").trim().toLowerCase();
}

function runKindLabel(run: OperatorRunSummary): "smoke" | "chain" | "normal" {
  const kind = normalizedRunKind(run);
  if (kind === "smoke" || run.smokeTestId?.trim()) return "smoke";
  if (kind === "chain") return "chain";
  return "normal";
}

function runParentExecutionId(run: OperatorRunSummary): string | null {
  return (run as TimelineRunFields).parentExecutionId?.trim() || null;
}

function isCancelledRun(run: OperatorRunSummary): boolean {
  const status = run.status.trim().toLowerCase();
  return status === "cancelled" || status === "canceled";
}

function isCachedRun(run: OperatorRunSummary): boolean {
  const status = run.status.trim().toLowerCase();
  return operatorRunIsCacheHit(run) || status === "cached" || status === "cache_hit";
}

function timelineStatus(run: OperatorRunSummary): {
  key: TimelineStatus;
  label: string;
  color: ChipProps["color"];
} {
  if (isCachedRun(run)) {
    return { key: "cached", label: "cached", color: "secondary" };
  }
  if (isCancelledRun(run)) {
    return { key: "cancelled", label: "cancelled", color: operatorRunStatusColor(run.status) };
  }

  const color = operatorRunStatusColor(run.status);
  if (color === "success") return { key: "success", label: "success", color };
  if (color === "error") return { key: "failed", label: "failed", color };
  if (color === "info") return { key: "running", label: "running", color };

  return { key: "other", label: run.status || "unknown", color };
}

function statusMatches(run: OperatorRunSummary, filter: TimelineStatusFilter): boolean {
  if (filter === "all") return true;
  return timelineStatus(run).key === filter;
}

function operatorByAlias(operators: OperatorSummary[]): Map<string, OperatorSummary> {
  const byAlias = new Map<string, OperatorSummary>();
  for (const operator of operators) {
    byAlias.set(operator.id, operator);
    for (const alias of operator.enabledAliases) {
      const trimmed = alias.trim();
      if (trimmed) byAlias.set(trimmed, operator);
    }
  }
  return byAlias;
}

function runMatchesTimelineFilters(
  run: OperatorRunSummary,
  statusFilter: TimelineStatusFilter,
  aliasFilter: string,
  dateStart: number | null,
): boolean {
  const timestamp = runTimestamp(run);
  if (dateStart !== null && (!Number.isFinite(timestamp) || timestamp < dateStart)) return false;
  if (aliasFilter !== "all" && runAlias(run) !== aliasFilter) return false;
  return statusMatches(run, statusFilter);
}

function compareRunsNewest(left: OperatorRunSummary, right: OperatorRunSummary): number {
  const leftTimestamp = runTimestamp(left);
  const rightTimestamp = runTimestamp(right);
  if (Number.isFinite(leftTimestamp) && Number.isFinite(rightTimestamp) && leftTimestamp !== rightTimestamp) {
    return rightTimestamp - leftTimestamp;
  }
  if (Number.isFinite(leftTimestamp) !== Number.isFinite(rightTimestamp)) {
    return Number.isFinite(leftTimestamp) ? -1 : 1;
  }
  return right.runId.localeCompare(left.runId);
}

function compareRunsOldest(left: OperatorRunSummary, right: OperatorRunSummary): number {
  const leftTimestamp = runStartTimestamp(left);
  const rightTimestamp = runStartTimestamp(right);
  if (Number.isFinite(leftTimestamp) && Number.isFinite(rightTimestamp) && leftTimestamp !== rightTimestamp) {
    return leftTimestamp - rightTimestamp;
  }
  if (Number.isFinite(leftTimestamp) !== Number.isFinite(rightTimestamp)) {
    return Number.isFinite(leftTimestamp) ? -1 : 1;
  }
  return left.runId.localeCompare(right.runId);
}

function chainLatestTimestamp(steps: OperatorRunSummary[]): number {
  const timestamps = steps.map(runTimestamp).filter(Number.isFinite);
  return timestamps.length > 0 ? Math.max(...timestamps) : Number.NEGATIVE_INFINITY;
}

function chainDurationText(steps: OperatorRunSummary[]): string {
  const starts = steps.map(runStartTimestamp).filter(Number.isFinite);
  const ends = steps.map(runEndTimestamp).filter(Number.isFinite);
  if (starts.length === 0 || ends.length === 0) return "duration unavailable";
  return formatDuration(Math.max(...ends) - Math.min(...starts));
}

function isSuccessfulStep(run: OperatorRunSummary): boolean {
  const status = timelineStatus(run).key;
  return status === "success" || status === "cached";
}

function aggregateChainStatus(steps: OperatorRunSummary[]): {
  label: string;
  color: ChipProps["color"];
} {
  if (steps.length === 0) return { label: "unknown", color: "default" };
  if (steps.every(isSuccessfulStep)) return { label: "succeeded", color: "success" };

  const firstStatus = timelineStatus(steps[0]).key;
  const laterSteps = steps.slice(1);
  const laterProgressed = laterSteps.some((step) => isSuccessfulStep(step) || timelineStatus(step).key === "running");
  if (firstStatus === "failed" && !laterProgressed) return { label: "failed", color: "error" };
  if (steps.some((step) => timelineStatus(step).key === "failed")) return { label: "partial", color: "warning" };
  if (steps.some((step) => timelineStatus(step).key === "running")) return { label: "running", color: "info" };
  if (steps.some((step) => timelineStatus(step).key === "cancelled")) return { label: "partial", color: "warning" };
  return { label: "partial", color: "warning" };
}

function groupedItemTimestamp(item: GroupedTimelineItem): number {
  return item.latestTimestamp;
}

function groupedItemId(item: GroupedTimelineItem): string {
  return item.type === "chain" ? item.parentExecutionId : item.run.runId;
}

function compareGroupedTimelineItems(left: GroupedTimelineItem, right: GroupedTimelineItem): number {
  const leftTimestamp = groupedItemTimestamp(left);
  const rightTimestamp = groupedItemTimestamp(right);
  if (Number.isFinite(leftTimestamp) && Number.isFinite(rightTimestamp) && leftTimestamp !== rightTimestamp) {
    return rightTimestamp - leftTimestamp;
  }
  if (Number.isFinite(leftTimestamp) !== Number.isFinite(rightTimestamp)) {
    return Number.isFinite(leftTimestamp) ? -1 : 1;
  }
  return groupedItemId(right).localeCompare(groupedItemId(left));
}

function TimelineRunRow({
  run,
  operator,
  onOpen,
  nested = false,
  dimmed = false,
  highlighted = false,
}: {
  run: OperatorRunSummary;
  operator?: OperatorSummary;
  onOpen: (run: OperatorRunSummary) => void;
  nested?: boolean;
  dimmed?: boolean;
  highlighted?: boolean;
}) {
  const theme = useTheme();
  const alias = runAlias(run);
  const status = timelineStatus(run);
  const timestamp = runTimestamp(run);
  const kind = runKindLabel(run);

  return (
    <Paper
      component="button"
      type="button"
      variant="outlined"
      onClick={() => onOpen(run)}
      sx={{
        width: "100%",
        p: nested ? 0.9 : 1.1,
        ml: nested ? { xs: 1, md: 3 } : 0,
        borderRadius: 1.5,
        borderLeft: nested ? 3 : undefined,
        borderLeftColor: nested ? "divider" : undefined,
        appearance: "none",
        bgcolor: highlighted ? alpha(theme.palette.primary.main, theme.palette.mode === "dark" ? 0.16 : 0.07) : "background.paper",
        color: "text.primary",
        cursor: "pointer",
        font: "inherit",
        opacity: dimmed ? 0.48 : 1,
        textAlign: "left",
        transition: theme.transitions.create(["background-color", "border-color", "opacity"], {
          duration: theme.transitions.duration.shortest,
        }),
        "&:hover": {
          bgcolor: highlighted ? alpha(theme.palette.primary.main, theme.palette.mode === "dark" ? 0.2 : 0.11) : "action.hover",
          borderColor: "text.secondary",
          opacity: 1,
        },
        "&:focus-visible": {
          outline: `2px solid ${theme.palette.primary.main}`,
          outlineOffset: 2,
        },
      }}
    >
      <Stack direction={{ xs: "column", md: "row" }} gap={1} alignItems={{ xs: "flex-start", md: "center" }}>
        <Chip
          size="small"
          color={status.color}
          variant={status.color === "default" ? "outlined" : "filled"}
          label={status.label}
          sx={{ minWidth: 76, textTransform: "capitalize" }}
        />
        <Stack spacing={0.35} sx={{ minWidth: 0, flex: 1 }}>
          <Stack direction="row" gap={0.75} alignItems="center" flexWrap="wrap">
            <Typography variant="body2" fontWeight={850} sx={{ minWidth: 0, wordBreak: "break-word" }}>
              {operator ? operatorDisplayName(operator) : alias}
            </Typography>
            <Chip
              size="small"
              variant="outlined"
              label={alias}
              sx={{
                maxWidth: "100%",
                fontFamily: "ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace",
                "& .MuiChip-label": { overflow: "hidden", textOverflow: "ellipsis" },
              }}
            />
          </Stack>
          <Typography variant="caption" color="text.secondary" sx={{ wordBreak: "break-word" }}>
            {run.runDir}
          </Typography>
        </Stack>
        <Stack
          spacing={0.4}
          alignItems={{ xs: "flex-start", md: "flex-end" }}
          sx={{ ml: { md: "auto" }, minWidth: { md: nested ? 148 : 180 } }}
        >
          <Typography variant="caption" color="text.secondary" sx={{ textAlign: { xs: "left", md: "right" } }}>
            {formatRelativeTime(timestamp)}
          </Typography>
          <Stack direction="row" gap={0.75} alignItems="center" justifyContent={{ xs: "flex-start", md: "flex-end" }} flexWrap="wrap">
            <Chip size="small" color={kind === "normal" ? "default" : "info"} variant="outlined" label={kind} />
            <Chip
              size="small"
              variant="outlined"
              label={run.runId.slice(-8)}
              sx={{
                fontFamily: "ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace",
              }}
            />
          </Stack>
        </Stack>
      </Stack>
    </Paper>
  );
}

function ChainRunCard({
  item,
  operatorsByAlias,
  expanded,
  filtersActive,
  onToggle,
  onOpen,
}: {
  item: ChainTimelineItem;
  operatorsByAlias: Map<string, OperatorSummary>;
  expanded: boolean;
  filtersActive: boolean;
  onToggle: () => void;
  onOpen: (run: OperatorRunSummary) => void;
}) {
  const theme = useTheme();
  const status = aggregateChainStatus(item.steps);
  const parentLabel = item.parentExecutionId.length > 12 ? item.parentExecutionId.slice(-12) : item.parentExecutionId;

  return (
    <Paper variant="outlined" sx={{ borderRadius: 1.5, overflow: "hidden", bgcolor: "background.paper" }}>
      <Box
        component="button"
        type="button"
        aria-expanded={expanded}
        onClick={onToggle}
        sx={{
          width: "100%",
          border: 0,
          p: 1.1,
          appearance: "none",
          bgcolor: "transparent",
          color: "text.primary",
          cursor: "pointer",
          font: "inherit",
          textAlign: "left",
          transition: theme.transitions.create(["background-color"], {
            duration: theme.transitions.duration.shortest,
          }),
          "&:hover": { bgcolor: "action.hover" },
          "&:focus-visible": {
            outline: `2px solid ${theme.palette.primary.main}`,
            outlineOffset: -2,
          },
        }}
      >
        <Stack direction={{ xs: "column", md: "row" }} gap={1} alignItems={{ xs: "flex-start", md: "center" }}>
          <Stack direction="row" gap={0.75} alignItems="center" flexWrap="wrap" sx={{ minWidth: 0, flex: 1 }}>
            <KeyboardArrowDownRounded
              fontSize="small"
              sx={{
                color: "text.secondary",
                transform: expanded ? "rotate(0deg)" : "rotate(-90deg)",
                transition: theme.transitions.create("transform", {
                  duration: theme.transitions.duration.shortest,
                }),
              }}
            />
            <Chip
              size="small"
              color={status.color}
              variant={status.color === "default" ? "outlined" : "filled"}
              label={status.label}
              sx={{ minWidth: 86, textTransform: "capitalize" }}
            />
            <Typography variant="body2" fontWeight={850} sx={{ minWidth: 0, wordBreak: "break-word" }}>
              Chain run · {item.steps.length} steps
            </Typography>
            <Chip
              size="small"
              variant="outlined"
              label={parentLabel}
              sx={{
                fontFamily: "ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace",
              }}
            />
          </Stack>
          <Stack alignItems={{ xs: "flex-start", md: "flex-end" }} sx={{ ml: { md: "auto" }, minWidth: { md: 180 } }}>
            <Typography variant="body2" fontWeight={750} sx={{ textAlign: { xs: "left", md: "right" } }}>
              {chainDurationText(item.steps)}
            </Typography>
            <Typography variant="caption" color="text.secondary" sx={{ textAlign: { xs: "left", md: "right" } }}>
              {formatRelativeTime(item.latestTimestamp)}
            </Typography>
          </Stack>
        </Stack>
      </Box>
      <Collapse in={expanded} timeout="auto" unmountOnExit>
        <Stack spacing={0.6} sx={{ px: 1, pb: 1, pt: 0.25 }}>
          {item.steps.map((step) => {
            const matching = item.matchingRunIds.has(step.runId);
            const alias = runAlias(step);
            return (
              <TimelineRunRow
                key={step.runId}
                run={step}
                operator={operatorsByAlias.get(alias)}
                onOpen={onOpen}
                nested
                dimmed={!matching}
                highlighted={filtersActive && matching}
              />
            );
          })}
        </Stack>
      </Collapse>
    </Paper>
  );
}

export function OperatorRunsTimeline({
  runs,
  operators,
  onOpen,
  onRefresh,
  busy,
}: {
  runs: OperatorRunSummary[];
  operators: OperatorSummary[];
  onOpen: (run: OperatorRunSummary) => void;
  onRefresh: () => void;
  busy: boolean;
}) {
  const theme = useTheme();
  const [statusFilter, setStatusFilter] = useState<TimelineStatusFilter>("all");
  const [aliasFilter, setAliasFilter] = useState("all");
  const [dateRange, setDateRange] = useState<TimelineDateRange>("all");
  const [viewMode, setViewMode] = useState<TimelineViewMode>("grouped");
  const [expandedChainIds, setExpandedChainIds] = useState<Set<string>>(() => new Set());

  const aliases = useMemo(
    () => Array.from(new Set(runs.map(runAlias))).sort((left, right) => left.localeCompare(right)),
    [runs],
  );
  const operatorsByAlias = useMemo(() => operatorByAlias(operators), [operators]);
  const filteredRuns = useMemo(() => {
    const now = Date.now();
    const start = timelineDateStart(dateRange, now);
    return [...runs]
      .filter((run) => runMatchesTimelineFilters(run, statusFilter, aliasFilter, start))
      .sort(compareRunsNewest);
  }, [aliasFilter, dateRange, runs, statusFilter]);
  const groupedTimelineItems = useMemo(() => {
    const now = Date.now();
    const start = timelineDateStart(dateRange, now);
    const chainGroups = new Map<string, OperatorRunSummary[]>();
    const standalone: OperatorRunSummary[] = [];

    for (const run of runs) {
      const parentExecutionId = runParentExecutionId(run);
      if (normalizedRunKind(run) === "chain" && parentExecutionId) {
        const group = chainGroups.get(parentExecutionId) ?? [];
        group.push(run);
        chainGroups.set(parentExecutionId, group);
      } else {
        standalone.push(run);
      }
    }

    const items: GroupedTimelineItem[] = [];
    for (const [parentExecutionId, groupRuns] of chainGroups) {
      const steps = [...groupRuns].sort(compareRunsOldest);
      const matchingRunIds = new Set(
        steps
          .filter((run) => runMatchesTimelineFilters(run, statusFilter, aliasFilter, start))
          .map((run) => run.runId),
      );
      if (matchingRunIds.size > 0) {
        items.push({
          type: "chain",
          parentExecutionId,
          steps,
          matchingRunIds,
          latestTimestamp: chainLatestTimestamp(steps),
        });
      }
    }

    for (const run of standalone) {
      if (runMatchesTimelineFilters(run, statusFilter, aliasFilter, start)) {
        items.push({ type: "run", run, latestTimestamp: runTimestamp(run) });
      }
    }

    return items.sort(compareGroupedTimelineItems);
  }, [aliasFilter, dateRange, runs, statusFilter]);
  const filtersActive = statusFilter !== "all" || aliasFilter !== "all" || dateRange !== "all";
  const visibleItemCount = viewMode === "grouped" ? groupedTimelineItems.length : filteredRuns.length;

  const toggleChainExpanded = (parentExecutionId: string) => {
    setExpandedChainIds((previous) => {
      const next = new Set(previous);
      if (next.has(parentExecutionId)) {
        next.delete(parentExecutionId);
      } else {
        next.add(parentExecutionId);
      }
      return next;
    });
  };

  return (
    <Paper variant="outlined" sx={{ borderRadius: 2 }}>
      <Stack spacing={0}>
        <Stack spacing={0.5} sx={{ px: 2, pt: 2, pb: 1.25 }}>
          <Stack direction="row" gap={1} alignItems="center" justifyContent="space-between" flexWrap="wrap">
            <Typography variant="subtitle2" fontWeight={800}>
              Runs timeline
            </Typography>
            <Chip size="small" variant="outlined" label={`${filteredRuns.length} of ${runs.length}`} />
          </Stack>
        </Stack>

        <Paper
          square
          elevation={0}
          sx={{
            position: "sticky",
            top: 0,
            zIndex: 1,
            borderTop: 1,
            borderBottom: 1,
            borderColor: "divider",
            bgcolor: alpha(theme.palette.background.paper, theme.palette.mode === "dark" ? 0.92 : 0.96),
            backdropFilter: "blur(10px)",
            px: 2,
            py: 1.25,
          }}
        >
          <Stack direction="row" spacing={1.5} useFlexGap flexWrap="wrap" alignItems="center" sx={{ minWidth: 0 }}>
            <Stack
              direction="row"
              spacing={0.75}
              alignItems="center"
              sx={{ minWidth: 0, maxWidth: "100%", overflowX: "auto", pb: 0.25 }}
            >
              <Typography variant="caption" color="text.secondary" fontWeight={700} sx={{ whiteSpace: "nowrap" }}>
                Status:
              </Typography>
              <ToggleButtonGroup
                exclusive
                size="small"
                value={statusFilter}
                onChange={(_, value: TimelineStatusFilter | null) => {
                  if (value) setStatusFilter(value);
                }}
                sx={{ flexWrap: "nowrap", minWidth: "max-content" }}
              >
                {statusOptions.map((option) => (
                  <ToggleButton key={option.value} value={option.value} sx={{ textTransform: "none", whiteSpace: "nowrap" }}>
                    {option.label}
                  </ToggleButton>
                ))}
              </ToggleButtonGroup>
            </Stack>

            <Stack direction="row" spacing={0.75} alignItems="center" sx={{ minWidth: 0, maxWidth: "100%" }}>
              <Typography variant="caption" color="text.secondary" fontWeight={700} sx={{ whiteSpace: "nowrap" }}>
                Alias:
              </Typography>
              <Select
                size="small"
                value={aliasFilter}
                onChange={(event) => setAliasFilter(event.target.value)}
                sx={{
                  minWidth: 140,
                  maxWidth: 200,
                  "& .MuiSelect-select": {
                    overflow: "hidden",
                    textOverflow: "ellipsis",
                    whiteSpace: "nowrap",
                  },
                }}
              >
                <MenuItem value="all">All</MenuItem>
                {aliases.map((alias) => (
                  <MenuItem key={alias} value={alias}>
                    {alias}
                  </MenuItem>
                ))}
              </Select>
            </Stack>

            <Stack
              direction="row"
              spacing={0.75}
              alignItems="center"
              sx={{ minWidth: 0, maxWidth: "100%", overflowX: "auto", pb: 0.25 }}
            >
              <Typography variant="caption" color="text.secondary" fontWeight={700} sx={{ whiteSpace: "nowrap" }}>
                Date:
              </Typography>
              <ToggleButtonGroup
                exclusive
                size="small"
                value={dateRange}
                onChange={(_, value: TimelineDateRange | null) => {
                  if (value) setDateRange(value);
                }}
                sx={{ flexWrap: "nowrap", minWidth: "max-content" }}
              >
                {dateRangeOptions.map((option) => (
                  <ToggleButton key={option.value} value={option.value} sx={{ textTransform: "none", whiteSpace: "nowrap" }}>
                    {option.label}
                  </ToggleButton>
                ))}
              </ToggleButtonGroup>
            </Stack>

            <Stack direction="row" spacing={0.75} alignItems="center" sx={{ minWidth: 0 }}>
              <Typography variant="caption" color="text.secondary" fontWeight={700} sx={{ whiteSpace: "nowrap" }}>
                View:
              </Typography>
              <ToggleButtonGroup
                exclusive
                size="small"
                value={viewMode}
                onChange={(_, value: TimelineViewMode | null) => {
                  if (value) setViewMode(value);
                }}
              >
                {viewOptions.map((option) => (
                  <ToggleButton key={option.value} value={option.value} sx={{ textTransform: "none", whiteSpace: "nowrap" }}>
                    {option.label}
                  </ToggleButton>
                ))}
              </ToggleButtonGroup>
            </Stack>

            <Tooltip title="Refresh runs">
              <span>
                <IconButton
                  size="small"
                  aria-label="Refresh runs"
                  aria-busy={busy ? true : undefined}
                  disabled={busy}
                  onClick={onRefresh}
                  sx={{ border: 1, borderColor: "divider", borderRadius: 1.25 }}
                >
                  <RefreshRounded fontSize="small" />
                </IconButton>
              </span>
            </Tooltip>
          </Stack>
        </Paper>

        <Stack spacing={0.75} sx={{ p: 1.25 }}>
          {visibleItemCount === 0 ? (
            <Paper variant="outlined" sx={{ p: 2, borderRadius: 1.5, textAlign: "center", bgcolor: "action.hover" }}>
              <Typography variant="body2" color="text.secondary">
                No matching operator runs.
              </Typography>
            </Paper>
          ) : viewMode === "grouped" ? (
            groupedTimelineItems.map((item) => {
              if (item.type === "chain") {
                return (
                  <ChainRunCard
                    key={`chain:${item.parentExecutionId}`}
                    item={item}
                    operatorsByAlias={operatorsByAlias}
                    expanded={expandedChainIds.has(item.parentExecutionId)}
                    filtersActive={filtersActive}
                    onToggle={() => toggleChainExpanded(item.parentExecutionId)}
                    onOpen={onOpen}
                  />
                );
              }
              const alias = runAlias(item.run);
              return (
                <TimelineRunRow
                  key={item.run.runId}
                  run={item.run}
                  operator={operatorsByAlias.get(alias)}
                  onOpen={onOpen}
                />
              );
            })
          ) : (
            filteredRuns.map((run) => {
              const alias = runAlias(run);
              return (
                <TimelineRunRow
                  key={run.runId}
                  run={run}
                  operator={operatorsByAlias.get(alias)}
                  onOpen={onOpen}
                />
              );
            })
          )}
        </Stack>
      </Stack>
    </Paper>
  );
}
