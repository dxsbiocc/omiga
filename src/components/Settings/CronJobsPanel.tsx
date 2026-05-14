import { useCallback, useEffect, useState } from "react";
import {
  Box,
  Button,
  CircularProgress,
  Dialog,
  DialogActions,
  DialogContent,
  DialogTitle,
  IconButton,
  Paper,
  Stack,
  Table,
  TableBody,
  TableCell,
  TableContainer,
  TableHead,
  TableRow,
  TextField,
  Tooltip,
  Typography,
} from "@mui/material";
import { alpha, useTheme } from "@mui/material/styles";
import {
  AddRounded,
  DeleteOutlineRounded,
  ScheduleRounded,
} from "@mui/icons-material";
import { invoke } from "@tauri-apps/api/core";

interface CronJobSummary {
  id: string;
  schedule: string;
  task: string;
  sessionId: string | null;
  createdAt: string;
}

function formatDate(iso: string): string {
  try {
    return new Date(iso).toLocaleString(undefined, {
      year: "numeric",
      month: "short",
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit",
    });
  } catch {
    return iso;
  }
}

const SCHEDULE_HELPER = [
  "每天早8点: 0 8 * * *",
  "每周一早9点: 0 9 * * 1",
  "每小时: 0 * * * *",
  "每15分钟: */15 * * * *",
  "每月1号: 0 9 1 * *",
].join("  |  ");

function validateSchedule(value: string): string | null {
  const parts = value.trim().split(/\s+/);
  if (parts.length < 5) {
    return "Cron expression must have at least 5 space-separated fields (min hour dom month dow)";
  }
  return null;
}

interface CreateCronJobDialogProps {
  open: boolean;
  onClose: () => void;
  onCreated: (job: CronJobSummary) => void;
}

function CreateCronJobDialog({
  open,
  onClose,
  onCreated,
}: CreateCronJobDialogProps) {
  const [schedule, setSchedule] = useState("");
  const [task, setTask] = useState("");
  const [scheduleError, setScheduleError] = useState<string | null>(null);
  const [submitError, setSubmitError] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);

  const handleClose = useCallback(() => {
    setSchedule("");
    setTask("");
    setScheduleError(null);
    setSubmitError(null);
    setCreating(false);
    onClose();
  }, [onClose]);

  const handleScheduleChange = useCallback(
    (value: string) => {
      setSchedule(value);
      setScheduleError(value.trim() ? validateSchedule(value) : null);
    },
    [],
  );

  const handleCreate = useCallback(async () => {
    const schErr = validateSchedule(schedule);
    if (schErr) {
      setScheduleError(schErr);
      return;
    }
    if (!task.trim()) {
      setSubmitError("Task description must not be empty.");
      return;
    }

    setCreating(true);
    setSubmitError(null);
    try {
      const created = await invoke<CronJobSummary>("create_cron_job", {
        schedule: schedule.trim(),
        task: task.trim(),
      });
      onCreated(created);
      handleClose();
    } catch (err) {
      setSubmitError(String(err));
    } finally {
      setCreating(false);
    }
  }, [schedule, task, onCreated, handleClose]);

  return (
    <Dialog open={open} onClose={handleClose} maxWidth="sm" fullWidth>
      <DialogTitle>New Scheduled Job</DialogTitle>
      <DialogContent>
        <Stack spacing={2} sx={{ mt: 1 }}>
          <TextField
            label="Schedule (cron expression)"
            value={schedule}
            onChange={(e) => handleScheduleChange(e.target.value)}
            error={Boolean(scheduleError)}
            helperText={
              scheduleError ??
              `Examples — ${SCHEDULE_HELPER}`
            }
            placeholder="0 8 * * *"
            fullWidth
            size="small"
            autoFocus
          />
          <TextField
            label="Task description"
            value={task}
            onChange={(e) => setTask(e.target.value)}
            placeholder="描述 AI 应该执行什么任务，例如：检查项目的 TODO 列表并生成日报"
            multiline
            rows={3}
            fullWidth
            size="small"
          />
          {submitError && (
            <Typography variant="body2" color="error">
              {submitError}
            </Typography>
          )}
        </Stack>
      </DialogContent>
      <DialogActions sx={{ px: 3, pb: 2 }}>
        <Button onClick={handleClose} disabled={creating}>
          Cancel
        </Button>
        <Button
          variant="contained"
          onClick={() => void handleCreate()}
          disabled={creating || !schedule.trim() || !task.trim()}
          startIcon={
            creating ? <CircularProgress size={14} color="inherit" /> : undefined
          }
        >
          Create
        </Button>
      </DialogActions>
    </Dialog>
  );
}

export function CronJobsPanel() {
  const theme = useTheme();
  const [jobs, setJobs] = useState<CronJobSummary[]>([]);
  const [loading, setLoading] = useState(true);
  const [deletingId, setDeletingId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [dialogOpen, setDialogOpen] = useState(false);

  const loadJobs = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const result = await invoke<CronJobSummary[]>("list_cron_jobs");
      setJobs(result);
    } catch (err) {
      setError(String(err));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void loadJobs();
  }, [loadJobs]);

  const handleDelete = useCallback(
    async (id: string) => {
      setDeletingId(id);
      try {
        await invoke<boolean>("delete_cron_job", { id });
        setJobs((prev) => prev.filter((j) => j.id !== id));
      } catch (err) {
        setError(String(err));
      } finally {
        setDeletingId(null);
      }
    },
    [],
  );

  const handleCreated = useCallback((job: CronJobSummary) => {
    setJobs((prev) => [job, ...prev]);
  }, []);

  return (
    <Box>
      <Stack direction="row" alignItems="center" spacing={1} sx={{ mb: 2 }}>
        <ScheduleRounded fontSize="small" color="action" />
        <Typography variant="subtitle2" fontWeight={600} sx={{ flex: 1 }}>
          Scheduled Jobs
        </Typography>
        <Button
          variant="contained"
          size="small"
          startIcon={<AddRounded />}
          onClick={() => setDialogOpen(true)}
        >
          New Job
        </Button>
      </Stack>

      <Typography variant="body2" color="text.secondary" sx={{ mb: 2 }}>
        Cron jobs created by the AI assistant. Delete any job to stop it from running.
      </Typography>

      {error && (
        <Typography variant="body2" color="error" sx={{ mb: 2 }}>
          {error}
        </Typography>
      )}

      {loading ? (
        <Box display="flex" justifyContent="center" py={4}>
          <CircularProgress size={24} />
        </Box>
      ) : jobs.length === 0 ? (
        <Paper
          variant="outlined"
          sx={{
            p: 4,
            textAlign: "center",
            borderRadius: 2,
            bgcolor: alpha(theme.palette.action.hover, 0.5),
          }}
        >
          <ScheduleRounded
            sx={{ fontSize: 36, color: "text.disabled", mb: 1 }}
          />
          <Typography variant="body2" color="text.secondary">
            No scheduled jobs
          </Typography>
          <Typography variant="caption" color="text.disabled">
            Jobs created by the AI assistant will appear here.
          </Typography>
        </Paper>
      ) : (
        <TableContainer
          component={Paper}
          variant="outlined"
          sx={{ borderRadius: 2 }}
        >
          <Table size="small">
            <TableHead>
              <TableRow
                sx={{
                  bgcolor: alpha(theme.palette.action.hover, 0.5),
                }}
              >
                <TableCell sx={{ fontWeight: 600, width: "20%" }}>
                  Schedule
                </TableCell>
                <TableCell sx={{ fontWeight: 600 }}>Task</TableCell>
                <TableCell sx={{ fontWeight: 600, width: "22%" }}>
                  Created
                </TableCell>
                <TableCell sx={{ width: 48 }} />
              </TableRow>
            </TableHead>
            <TableBody>
              {jobs.map((job) => (
                <TableRow
                  key={job.id}
                  hover
                  sx={{
                    "&:last-child td": { border: 0 },
                  }}
                >
                  <TableCell>
                    <Typography
                      variant="body2"
                      fontFamily="monospace"
                      fontSize="0.75rem"
                    >
                      {job.schedule}
                    </Typography>
                  </TableCell>
                  <TableCell>
                    <Typography variant="body2">{job.task}</Typography>
                    {job.sessionId && (
                      <Typography
                        variant="caption"
                        color="text.disabled"
                        display="block"
                      >
                        Session: {job.sessionId.slice(0, 8)}…
                      </Typography>
                    )}
                  </TableCell>
                  <TableCell>
                    <Typography variant="body2" color="text.secondary">
                      {formatDate(job.createdAt)}
                    </Typography>
                  </TableCell>
                  <TableCell align="right">
                    <Tooltip title="Delete job">
                      <span>
                        <IconButton
                          size="small"
                          color="error"
                          disabled={deletingId === job.id}
                          onClick={() => void handleDelete(job.id)}
                        >
                          {deletingId === job.id ? (
                            <CircularProgress size={16} color="inherit" />
                          ) : (
                            <DeleteOutlineRounded fontSize="small" />
                          )}
                        </IconButton>
                      </span>
                    </Tooltip>
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </TableContainer>
      )}

      <CreateCronJobDialog
        open={dialogOpen}
        onClose={() => setDialogOpen(false)}
        onCreated={handleCreated}
      />
    </Box>
  );
}
