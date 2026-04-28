import { Box, Button, Chip, Collapse, IconButton, Stack, Typography } from "@mui/material";
import { alpha } from "@mui/material/styles";
import { ExpandMore, WarningAmber } from "@mui/icons-material";
import { compactLabel } from "../../utils/compactLabel";
import {
  orchestrationPhaseLabel,
  parseEventTime,
  stringifyTracePayload,
  type OrchestrationEventDto,
  type TimelineEvent,
} from "./orchestrationProjection";
import { formatRelativeTimelineTime } from "./OrchestrationTimelineList";

type TraceFailureDiagnosticItem = {
  id: string;
  taskId?: string;
  traceEventId?: string;
  agentLabel: string;
  title: string;
  detail?: string;
  summary: string;
  at?: number;
  source: "task" | "reviewer";
};

interface OrchestrationTraceListProps {
  scopedEvents: OrchestrationEventDto[];
  filteredEvents: OrchestrationEventDto[];
  timelineEvents: TimelineEvent[];
  failureDiagnostics: TraceFailureDiagnosticItem[];
  traceModes: string[];
  traceEventTypes: string[];
  traceModeFilter: string;
  traceEventTypeFilter: string;
  expandedTraceEventId: string | null;
  copiedTraceEventId: string | null;
  onTraceModeFilterChange: (filter: string) => void;
  onTraceEventTypeFilterChange: (filter: string) => void;
  onToggleTraceEvent: (eventId: string) => void;
  onTimelineEvent: (event: TimelineEvent) => void;
  onOpenTaskRecord: (taskId: string, label: string) => void;
  onCopyTracePayload: (event: OrchestrationEventDto) => void;
  onBackToFailures: () => void;
  now?: number;
}

export function findMatchingTimelineEvent(
  event: OrchestrationEventDto,
  timelineEvents: TimelineEvent[],
): TimelineEvent | undefined {
  return timelineEvents.find(
    (item) =>
      item.id === event.id ||
      (event.task_id &&
        item.action &&
        "taskId" in item.action &&
        item.action.taskId === event.task_id),
  );
}

export function findRelatedFailure(
  event: OrchestrationEventDto,
  failureDiagnostics: TraceFailureDiagnosticItem[],
): TraceFailureDiagnosticItem | undefined {
  return failureDiagnostics.find(
    (item) => item.traceEventId === event.id || (event.task_id && item.taskId === event.task_id),
  );
}

export function taskRecordLabel(failure: TraceFailureDiagnosticItem): string {
  return `${failure.agentLabel}: ${failure.detail ?? failure.summary}`;
}

function traceActionButtonLabel(action: TimelineEvent["action"]): string {
  if (!action) return "定位";
  if (action.type === "task" || action.type === "reviewer") return "任务";
  if (action.type === "trace") return "展开";
  if (action.type === "plan") return "计划";
  return "状态";
}

export function OrchestrationTraceList({
  scopedEvents,
  filteredEvents,
  timelineEvents,
  failureDiagnostics,
  traceModes,
  traceEventTypes,
  traceModeFilter,
  traceEventTypeFilter,
  expandedTraceEventId,
  copiedTraceEventId,
  onTraceModeFilterChange,
  onTraceEventTypeFilterChange,
  onToggleTraceEvent,
  onTimelineEvent,
  onOpenTaskRecord,
  onCopyTracePayload,
  onBackToFailures,
  now,
}: OrchestrationTraceListProps) {
  return (
    <Box
      sx={{
        mt: 0.25,
        pt: 0.75,
        borderTop: 1,
        borderColor: alpha("#6366f1", 0.12),
      }}
    >
      <Typography
        variant="caption"
        color="text.secondary"
        sx={{ display: "block", mb: 0.75, fontSize: 10 }}
      >
        Trace 原始事件
      </Typography>

      <Stack spacing={0.75}>
        <Stack direction="row" spacing={0.5} flexWrap="wrap" useFlexGap>
          <Chip
            size="small"
            label={traceModeFilter === "all" ? `全部模式 ${scopedEvents.length}` : traceModeFilter}
            color={traceModeFilter === "all" ? "primary" : "default"}
            variant={traceModeFilter === "all" ? "filled" : "outlined"}
            onClick={() => onTraceModeFilterChange("all")}
            sx={{ height: 18, fontSize: 9, cursor: "pointer" }}
          />
          {traceModes.map((mode) => (
            <Chip
              key={mode}
              size="small"
              label={mode}
              color={traceModeFilter === mode ? "primary" : "default"}
              variant={traceModeFilter === mode ? "filled" : "outlined"}
              onClick={() => onTraceModeFilterChange(mode)}
              sx={{ height: 18, fontSize: 9, cursor: "pointer" }}
            />
          ))}
        </Stack>

        <Stack direction="row" spacing={0.5} flexWrap="wrap" useFlexGap>
          <Chip
            size="small"
            label={
              traceEventTypeFilter === "all"
                ? `全部事件 ${filteredEvents.length}`
                : traceEventTypeFilter
            }
            color={traceEventTypeFilter === "all" ? "secondary" : "default"}
            variant={traceEventTypeFilter === "all" ? "filled" : "outlined"}
            onClick={() => onTraceEventTypeFilterChange("all")}
            sx={{ height: 18, fontSize: 9, cursor: "pointer" }}
          />
          {traceEventTypes.slice(0, 10).map((eventType) => (
            <Chip
              key={eventType}
              size="small"
              label={eventType}
              color={traceEventTypeFilter === eventType ? "secondary" : "default"}
              variant={traceEventTypeFilter === eventType ? "filled" : "outlined"}
              onClick={() => onTraceEventTypeFilterChange(eventType)}
              sx={{ height: 18, fontSize: 9, cursor: "pointer" }}
            />
          ))}
        </Stack>

        {filteredEvents.length === 0 ? (
          <Typography variant="caption" color="text.secondary">
            当前筛选条件下暂无 trace 事件。
          </Typography>
        ) : (
          <Stack spacing={0.5}>
            {filteredEvents.slice(0, 12).map((event) => {
              const ts = parseEventTime(event.created_at) ?? Date.now();
              const isExpanded = expandedTraceEventId === event.id;
              const payloadText = stringifyTracePayload(event.payload);
              const relatedFailure = findRelatedFailure(event, failureDiagnostics);
              const matchingTimeline = findMatchingTimelineEvent(event, timelineEvents);
              return (
                <Box
                  key={event.id}
                  sx={{
                    border: 1,
                    borderColor: alpha("#6366f1", 0.12),
                    borderRadius: 1.25,
                    bgcolor: alpha("#6366f1", 0.03),
                    overflow: "hidden",
                    transition: "border-color 200ms ease",
                    "&:hover": {
                      borderColor: alpha("#6366f1", 0.28),
                    },
                  }}
                >
                  <Stack
                    direction="row"
                    alignItems="center"
                    spacing={0.75}
                    sx={{ px: 1, py: 0.75 }}
                  >
                    <Box sx={{ minWidth: 0, flex: 1 }}>
                      <Stack
                        direction="row"
                        spacing={0.5}
                        alignItems="center"
                        flexWrap="wrap"
                        useFlexGap
                        sx={{ mb: 0.35 }}
                      >
                        {event.mode && (
                          <Chip size="small" label={event.mode} sx={{ height: 16, fontSize: 8.5 }} />
                        )}
                        <Chip
                          size="small"
                          label={event.event_type}
                          color="primary"
                          variant="outlined"
                          sx={{ height: 16, fontSize: 8.5 }}
                        />
                        {event.phase && (
                          <Chip
                            size="small"
                            label={orchestrationPhaseLabel(event.phase)}
                            sx={{ height: 16, fontSize: 8.5 }}
                          />
                        )}
                        {relatedFailure && (
                          <Chip
                            size="small"
                            icon={<WarningAmber sx={{ fontSize: 10 }} />}
                            label="关联失败"
                            sx={{
                              height: 16,
                              fontSize: 8.5,
                              bgcolor: alpha("#ef4444", 0.1),
                              color: "#ef4444",
                            }}
                          />
                        )}
                      </Stack>
                      <Typography
                        variant="caption"
                        sx={{ display: "block", fontSize: 10, fontWeight: 600 }}
                      >
                        {matchingTimeline?.label ?? event.event_type}
                      </Typography>
                      <Typography
                        variant="caption"
                        color="text.secondary"
                        sx={{ display: "block", fontSize: 9 }}
                      >
                        {formatRelativeTimelineTime(ts, now)}
                        {matchingTimeline?.detail ? ` · ${matchingTimeline.detail}` : ""}
                      </Typography>
                      {relatedFailure && (
                        <Typography
                          variant="caption"
                          sx={{
                            display: "block",
                            mt: 0.2,
                            fontSize: 9,
                            lineHeight: 1.35,
                            color: "#ef4444",
                          }}
                        >
                          {compactLabel(relatedFailure.summary, 96)}
                        </Typography>
                      )}
                    </Box>

                    {matchingTimeline?.action && (
                      <Button
                        size="small"
                        variant="outlined"
                        onClick={() => onTimelineEvent(matchingTimeline)}
                        sx={{ fontSize: 10, py: 0.2, minWidth: 0 }}
                      >
                        {traceActionButtonLabel(matchingTimeline.action)}
                      </Button>
                    )}
                    {relatedFailure?.taskId && (
                      <Button
                        size="small"
                        color="error"
                        variant="outlined"
                        onClick={() => onOpenTaskRecord(relatedFailure.taskId!, taskRecordLabel(relatedFailure))}
                        sx={{ fontSize: 10, py: 0.2, minWidth: 0 }}
                      >
                        记录
                      </Button>
                    )}
                    <IconButton
                      size="small"
                      aria-label="展开 trace payload"
                      onClick={() => onToggleTraceEvent(event.id)}
                      sx={{
                        color: "text.secondary",
                        opacity: 0.6,
                        transform: isExpanded ? "rotate(180deg)" : "rotate(0deg)",
                        transition: "transform 0.2s ease, color 150ms ease, opacity 150ms ease",
                        "&:hover": {
                          color: "#6366f1",
                          opacity: 1,
                          bgcolor: alpha("#6366f1", 0.08),
                        },
                      }}
                    >
                      <ExpandMore sx={{ fontSize: 16 }} />
                    </IconButton>
                  </Stack>

                  <Collapse in={isExpanded}>
                    <Box
                      sx={{
                        px: 1,
                        pb: 1,
                        borderTop: 1,
                        borderColor: alpha("#6366f1", 0.08),
                        bgcolor: alpha("#6366f1", 0.02),
                      }}
                    >
                      <Typography
                        variant="caption"
                        color="text.secondary"
                        sx={{ display: "block", mb: 0.5, mt: 0.75, fontSize: 9 }}
                      >
                        原始 payload
                      </Typography>
                      <Stack direction="row" spacing={0.5} flexWrap="wrap" useFlexGap sx={{ mb: 0.75 }}>
                        <Button
                          size="small"
                          variant="outlined"
                          onClick={() => onCopyTracePayload(event)}
                          sx={{ minWidth: 0, fontSize: 10, py: 0.15 }}
                        >
                          {copiedTraceEventId === event.id ? "已复制" : "复制 payload"}
                        </Button>
                        {relatedFailure && (
                          <Button
                            size="small"
                            color="error"
                            variant="outlined"
                            onClick={onBackToFailures}
                            sx={{ minWidth: 0, fontSize: 10, py: 0.15 }}
                          >
                            回到失败诊断
                          </Button>
                        )}
                        {relatedFailure?.taskId && (
                          <Button
                            size="small"
                            color="error"
                            variant="text"
                            onClick={() => onOpenTaskRecord(relatedFailure.taskId!, taskRecordLabel(relatedFailure))}
                            sx={{ minWidth: 0, fontSize: 10, py: 0.15 }}
                          >
                            打开队友记录
                          </Button>
                        )}
                      </Stack>
                      <Box
                        component="pre"
                        sx={{
                          m: 0,
                          p: 0.75,
                          borderRadius: 1,
                          bgcolor: alpha("#0f172a", 0.06),
                          fontSize: 10,
                          lineHeight: 1.45,
                          whiteSpace: "pre-wrap",
                          wordBreak: "break-word",
                          overflowX: "auto",
                        }}
                      >
                        {payloadText}
                      </Box>
                    </Box>
                  </Collapse>
                </Box>
              );
            })}
          </Stack>
        )}
      </Stack>
    </Box>
  );
}
