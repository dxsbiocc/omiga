import { Box, Stack, Typography } from "@mui/material";
import { alpha } from "@mui/material/styles";
import type { TimelineEvent } from "./orchestrationProjection";

export function formatRelativeTimelineTime(ts: number, now = Date.now()): string {
  const deltaSec = Math.max(0, Math.round((now - ts) / 1000));
  if (deltaSec < 60) return `${deltaSec}s 前`;
  if (deltaSec < 3600) return `${Math.round(deltaSec / 60)}m 前`;
  if (deltaSec < 86400) return `${Math.round(deltaSec / 3600)}h 前`;
  return `${Math.round(deltaSec / 86400)}d 前`;
}

interface OrchestrationTimelineListProps {
  events: TimelineEvent[];
  onEventClick?: (event: TimelineEvent) => void;
  now?: number;
  emptyText?: string;
}

export function OrchestrationTimelineList({
  events,
  onEventClick,
  now,
  emptyText = "当前会话暂无时间线事件。开始执行或恢复任务后，这里会记录关键编排节点。",
}: OrchestrationTimelineListProps) {
  return (
    <Box
      sx={{
        mt: 0.25,
        pt: 0.75,
        borderTop: 1,
        borderColor: alpha("#6366f1", 0.12),
      }}
    >
      {events.length === 0 ? (
        <Typography
          variant="caption"
          color="text.secondary"
          sx={{ display: "block", fontSize: 10, lineHeight: 1.6 }}
        >
          {emptyText}
        </Typography>
      ) : (
        <>
          <Typography
            variant="caption"
            color="text.secondary"
            sx={{ display: "block", mb: 0.75, fontSize: 10 }}
          >
            编排事件时间线
          </Typography>
          <Stack spacing={0.75}>
            {events.map((event) => {
              const color =
                event.tone === "error"
                  ? "#ef4444"
                  : event.tone === "warning"
                    ? "#f59e0b"
                    : event.tone === "success"
                      ? "#22c55e"
                      : "#6366f1";
              const clickable = Boolean(event.action && onEventClick);
              return (
                <Stack
                  key={event.id}
                  direction="row"
                  spacing={0.75}
                  alignItems="flex-start"
                  onClick={clickable ? () => onEventClick?.(event) : undefined}
                  sx={{
                    cursor: clickable ? "pointer" : "default",
                    borderRadius: 1,
                    px: 0.5,
                    py: 0.25,
                    transition: "background-color 0.15s ease",
                    "&:hover": clickable
                      ? {
                          bgcolor: alpha(color, 0.06),
                        }
                      : undefined,
                  }}
                >
                  <Box
                    sx={{
                      width: 8,
                      height: 8,
                      borderRadius: "50%",
                      bgcolor: color,
                      mt: 0.4,
                      flexShrink: 0,
                    }}
                  />
                  <Box sx={{ minWidth: 0, flex: 1 }}>
                    <Stack
                      direction="row"
                      spacing={0.75}
                      alignItems="center"
                      justifyContent="space-between"
                    >
                      <Typography
                        variant="caption"
                        sx={{ fontSize: 10.5, fontWeight: 600 }}
                      >
                        {event.label}
                      </Typography>
                      <Typography
                        variant="caption"
                        color="text.secondary"
                        sx={{ fontSize: 9, flexShrink: 0 }}
                      >
                        {formatRelativeTimelineTime(event.at, now)}
                      </Typography>
                    </Stack>
                    {event.detail && (
                      <Typography
                        variant="caption"
                        color="text.secondary"
                        sx={{
                          display: "block",
                          fontSize: 9.5,
                          lineHeight: 1.45,
                          mt: 0.15,
                        }}
                      >
                        {event.detail}
                      </Typography>
                    )}
                    {clickable && (
                      <Typography
                        variant="caption"
                        sx={{
                          display: "block",
                          mt: 0.2,
                          fontSize: 9,
                          color,
                          fontWeight: 600,
                        }}
                      >
                        打开关联证据
                      </Typography>
                    )}
                  </Box>
                </Stack>
              );
            })}
          </Stack>
        </>
      )}
    </Box>
  );
}
