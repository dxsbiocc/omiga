import { useMemo, useState } from "react";
import {
  Button,
  Chip,
  CircularProgress,
  MenuItem,
  Paper,
  Stack,
  TextField,
  ToggleButton,
  ToggleButtonGroup,
  Typography,
  type ChipProps,
} from "@mui/material";
import { alpha, useTheme } from "@mui/material/styles";
import { RefreshRounded } from "@mui/icons-material";
import type { OperatorRunSummary, OperatorSummary } from "../../state/pluginStore";
import {
  operatorDisplayName,
  operatorRunIsCacheHit,
  operatorRunStatusColor,
} from "./PluginsPanel";

type TimelineStatusFilter = "all" | "success" | "failed" | "running" | "cached" | "cancelled";
type TimelineDateRange = "today" | "7d" | "30d" | "all";
type TimelineStatus = Exclude<TimelineStatusFilter, "all"> | "other";
type TimelineRunFields = OperatorRunSummary & {
  alias?: string | null;
  kind?: string | null;
  startedAt?: string | null;
};

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

function runAlias(run: OperatorRunSummary): string {
  const fields = run as TimelineRunFields;
  return fields.alias?.trim() || run.operatorAlias?.trim() || run.operatorId?.trim() || "unknown";
}

function runTimestampText(run: OperatorRunSummary): string | null {
  const fields = run as TimelineRunFields;
  return fields.startedAt?.trim() || run.updatedAt?.trim() || null;
}

function runTimestamp(run: OperatorRunSummary): number {
  const text = runTimestampText(run);
  if (!text) return Number.NEGATIVE_INFINITY;
  const timestamp = new Date(text).getTime();
  return Number.isNaN(timestamp) ? Number.NEGATIVE_INFINITY : timestamp;
}

function formatRelativeTime(ms: number): string {
  if (!Number.isFinite(ms)) return "unknown time";
  const diff = Math.max(0, Date.now() - ms);
  if (diff < 60_000) return "just now";
  if (diff < 3_600_000) return `${Math.floor(diff / 60_000)}m ago`;
  if (diff < 86_400_000) return `${Math.floor(diff / 3_600_000)}h ago`;
  return `${Math.floor(diff / 86_400_000)}d ago`;
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

function runKindLabel(run: OperatorRunSummary): "smoke" | "chain" | "normal" {
  const fields = run as TimelineRunFields;
  const kind = (run.runKind || fields.kind || "").trim().toLowerCase();
  if (kind === "smoke" || run.smokeTestId?.trim()) return "smoke";
  if (kind === "chain") return "chain";
  return "normal";
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

  const aliases = useMemo(
    () => Array.from(new Set(runs.map(runAlias))).sort((left, right) => left.localeCompare(right)),
    [runs],
  );
  const operatorsByAlias = useMemo(() => operatorByAlias(operators), [operators]);
  const filteredRuns = useMemo(() => {
    const now = Date.now();
    const start = timelineDateStart(dateRange, now);
    return [...runs]
      .filter((run) => {
        const timestamp = runTimestamp(run);
        if (start !== null && (!Number.isFinite(timestamp) || timestamp < start)) return false;
        if (aliasFilter !== "all" && runAlias(run) !== aliasFilter) return false;
        return statusMatches(run, statusFilter);
      })
      .sort((left, right) => {
        const byTime = runTimestamp(right) - runTimestamp(left);
        return byTime !== 0 ? byTime : right.runId.localeCompare(left.runId);
      });
  }, [aliasFilter, dateRange, runs, statusFilter]);

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
          <Stack direction={{ xs: "column", lg: "row" }} gap={1} alignItems={{ xs: "stretch", lg: "center" }}>
            <ToggleButtonGroup
              exclusive
              size="small"
              value={statusFilter}
              onChange={(_, value: TimelineStatusFilter | null) => {
                if (value) setStatusFilter(value);
              }}
              sx={{ flexWrap: "wrap" }}
            >
              {statusOptions.map((option) => (
                <ToggleButton key={option.value} value={option.value} sx={{ textTransform: "none" }}>
                  {option.label}
                </ToggleButton>
              ))}
            </ToggleButtonGroup>

            <TextField
              select
              size="small"
              label="Alias"
              value={aliasFilter}
              onChange={(event) => setAliasFilter(event.target.value)}
              sx={{ minWidth: { xs: "100%", lg: 190 } }}
            >
              <MenuItem value="all">All</MenuItem>
              {aliases.map((alias) => (
                <MenuItem key={alias} value={alias}>
                  {alias}
                </MenuItem>
              ))}
            </TextField>

            <ToggleButtonGroup
              exclusive
              size="small"
              value={dateRange}
              onChange={(_, value: TimelineDateRange | null) => {
                if (value) setDateRange(value);
              }}
            >
              {dateRangeOptions.map((option) => (
                <ToggleButton key={option.value} value={option.value} sx={{ textTransform: "none" }}>
                  {option.label}
                </ToggleButton>
              ))}
            </ToggleButtonGroup>

            <Button
              size="small"
              variant="outlined"
              startIcon={busy ? <CircularProgress size={16} /> : <RefreshRounded />}
              disabled={busy}
              onClick={onRefresh}
              sx={{ ml: { lg: "auto" }, textTransform: "none", borderRadius: 1.5, minHeight: 36 }}
            >
              Refresh
            </Button>
          </Stack>
        </Paper>

        <Stack spacing={0.75} sx={{ p: 1.25 }}>
          {filteredRuns.length === 0 ? (
            <Paper variant="outlined" sx={{ p: 2, borderRadius: 1.5, textAlign: "center", bgcolor: "action.hover" }}>
              <Typography variant="body2" color="text.secondary">
                No matching operator runs.
              </Typography>
            </Paper>
          ) : (
            filteredRuns.map((run) => {
              const alias = runAlias(run);
              const operator = operatorsByAlias.get(alias);
              const status = timelineStatus(run);
              const timestamp = runTimestamp(run);
              const kind = runKindLabel(run);
              return (
                <Paper
                  key={run.runId}
                  component="button"
                  type="button"
                  variant="outlined"
                  onClick={() => onOpen(run)}
                  sx={{
                    width: "100%",
                    p: 1.1,
                    borderRadius: 1.5,
                    appearance: "none",
                    bgcolor: "background.paper",
                    color: "text.primary",
                    cursor: "pointer",
                    font: "inherit",
                    textAlign: "left",
                    transition: theme.transitions.create(["background-color", "border-color"], {
                      duration: theme.transitions.duration.shortest,
                    }),
                    "&:hover": {
                      bgcolor: "action.hover",
                      borderColor: "text.secondary",
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
                        {formatRelativeTime(timestamp)} · {run.runDir}
                      </Typography>
                    </Stack>
                    <Stack direction="row" gap={0.75} alignItems="center" flexWrap="wrap">
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
                </Paper>
              );
            })
          )}
        </Stack>
      </Stack>
    </Paper>
  );
}
