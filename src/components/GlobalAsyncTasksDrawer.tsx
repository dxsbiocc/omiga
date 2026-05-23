import { useEffect, useMemo, useState } from "react";
import {
  Badge,
  Box,
  Button,
  Chip,
  type ChipProps,
  Divider,
  Drawer,
  Fab,
  IconButton,
  Stack,
  Tooltip,
  Typography,
  alpha,
  useTheme,
} from "@mui/material";
import { CircleStop, Clock3, ListTodo, X } from "lucide-react";
import { usePluginStore } from "../state/pluginStore";

function formatElapsed(startedAt: number | null | undefined, now: number): string {
  if (startedAt == null || !Number.isFinite(startedAt)) return "00:00";
  const seconds = Math.max(0, Math.floor((now - startedAt) / 1000));
  const minutes = Math.floor(seconds / 60);
  const remainder = seconds % 60;
  return `${String(minutes).padStart(2, "0")}:${String(remainder).padStart(2, "0")}`;
}

function queueStatusLabel(status: { state: string; jobId?: string }): string {
  return `${status.state}${status.jobId ? ` · job ${status.jobId}` : ""}`;
}

function queueStatusColor(state: string): ChipProps["color"] {
  const normalized = state.trim().toUpperCase();
  if (normalized === "R" || normalized === "RUNNING") return "success";
  if (normalized === "PD" || normalized === "PENDING" || normalized === "CF") return "warning";
  if (normalized === "CG" || normalized === "COMPLETING") return "info";
  if (["F", "FAILED", "CA", "CANCELLED", "TO", "TIMEOUT"].includes(normalized)) {
    return "error";
  }
  return "default";
}

export function GlobalAsyncTasksDrawer() {
  const theme = useTheme();
  const [open, setOpen] = useState(false);
  const [now, setNow] = useState(() => Date.now());
  const [cancellingTaskIds, setCancellingTaskIds] = useState<Record<string, boolean>>({});
  const activeOperatorTasks = usePluginStore((state) => state.activeOperatorTasks);
  const activeOperatorTaskStartedAt = usePluginStore(
    (state) => state.activeOperatorTaskStartedAt,
  );
  const activeOperatorTaskStatus = usePluginStore(
    (state) => state.activeOperatorTaskStatus,
  );
  const cancelOperatorTask = usePluginStore((state) => state.cancelOperatorTask);

  const taskRows = useMemo(
    () =>
      Object.entries(activeOperatorTasks)
        .sort(([leftAlias], [rightAlias]) => leftAlias.localeCompare(rightAlias))
        .map(([alias, taskId]) => ({
          alias,
          taskId,
          startedAt: activeOperatorTaskStartedAt[alias],
          status: activeOperatorTaskStatus[alias],
        })),
    [activeOperatorTasks, activeOperatorTaskStartedAt, activeOperatorTaskStatus],
  );
  const taskCount = Object.keys(activeOperatorTasks).length;

  useEffect(() => {
    if (taskCount === 0) {
      setOpen(false);
      return undefined;
    }
    setNow(Date.now());
    const intervalId = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(intervalId);
  }, [taskCount]);

  const handleCancel = async (taskId: string) => {
    setCancellingTaskIds((current) => ({ ...current, [taskId]: true }));
    try {
      await cancelOperatorTask(taskId);
    } finally {
      setCancellingTaskIds((current) => {
        const next = { ...current };
        delete next[taskId];
        return next;
      });
    }
  };

  if (taskCount === 0) return null;

  const edge = alpha(
    theme.palette.mode === "dark"
      ? theme.palette.common.white
      : theme.palette.common.black,
    0.1,
  );

  return (
    <>
      <Tooltip title="Async operator tasks">
        <Badge
          badgeContent={taskCount}
          color="error"
          sx={{
            position: "fixed",
            right: { xs: 16, sm: 24 },
            bottom: { xs: 16, sm: 24 },
            zIndex: theme.zIndex.drawer - 1,
          }}
        >
          <Fab
            color="primary"
            size="medium"
            aria-label="Open async operator tasks"
            onClick={() => setOpen(true)}
            sx={{ boxShadow: theme.shadows[8] }}
          >
            <ListTodo size={22} aria-hidden />
          </Fab>
        </Badge>
      </Tooltip>

      <Drawer
        anchor="right"
        open={open}
        onClose={() => setOpen(false)}
        PaperProps={{
          sx: {
            width: { xs: "100%", sm: 400 },
            maxWidth: "100vw",
            bgcolor: alpha(theme.palette.background.paper, 0.98),
            borderLeft: `1px solid ${edge}`,
          },
        }}
      >
        <Stack
          direction="row"
          alignItems="center"
          justifyContent="space-between"
          sx={{ px: 2, py: 1.5 }}
        >
          <Box sx={{ minWidth: 0 }}>
            <Typography variant="subtitle1" fontWeight={800}>
              Async operator tasks
            </Typography>
            <Typography variant="body2" color="text.secondary">
              {taskCount} active
            </Typography>
          </Box>
          <IconButton
            size="small"
            aria-label="Close async operator tasks"
            onClick={() => setOpen(false)}
          >
            <X size={18} aria-hidden />
          </IconButton>
        </Stack>
        <Divider />

        <Stack spacing={1.25} sx={{ p: 2, overflow: "auto" }}>
          {taskRows.map(({ alias, taskId, startedAt, status }) => (
            <Box
              key={`${alias}:${taskId}`}
              sx={{
                p: 1.25,
                borderRadius: 1,
                border: `1px solid ${edge}`,
                bgcolor: alpha(theme.palette.background.default, 0.58),
              }}
            >
              <Stack spacing={1}>
                <Stack
                  direction="row"
                  alignItems="flex-start"
                  justifyContent="space-between"
                  gap={1}
                >
                  <Box sx={{ minWidth: 0 }}>
                    <Typography
                      variant="subtitle1"
                      fontWeight={850}
                      sx={{ wordBreak: "break-word" }}
                    >
                      {alias}
                    </Typography>
                    <Typography variant="caption" color="text.secondary" sx={{ wordBreak: "break-all" }}>
                      {taskId}
                    </Typography>
                  </Box>
                  <Stack direction="row" alignItems="center" gap={0.5} sx={{ flexShrink: 0 }}>
                    <Clock3 size={15} aria-hidden />
                    <Typography variant="body2" fontWeight={700}>
                      {formatElapsed(startedAt, now)}
                    </Typography>
                  </Stack>
                </Stack>

                <Stack direction="row" gap={0.75} flexWrap="wrap" alignItems="center">
                  {status ? (
                    <Chip
                      size="small"
                      color={queueStatusColor(status.state)}
                      variant="outlined"
                      label={queueStatusLabel(status)}
                      title={`${status.scheduler} queue state`}
                      sx={{ fontWeight: 700 }}
                    />
                  ) : null}
                  <Button
                    size="small"
                    variant="outlined"
                    color="warning"
                    startIcon={<CircleStop size={16} aria-hidden />}
                    disabled={Boolean(cancellingTaskIds[taskId])}
                    onClick={() => void handleCancel(taskId)}
                    sx={{ textTransform: "none", borderRadius: 1, ml: "auto" }}
                  >
                    Cancel
                  </Button>
                </Stack>
              </Stack>
            </Box>
          ))}
        </Stack>
      </Drawer>
    </>
  );
}
