import { useCallback, useEffect, useState } from "react";
import {
  Box,
  CircularProgress,
  IconButton,
  Paper,
  Stack,
  Table,
  TableBody,
  TableCell,
  TableContainer,
  TableHead,
  TableRow,
  Tooltip,
  Typography,
} from "@mui/material";
import { alpha, useTheme } from "@mui/material/styles";
import { DeleteOutlineRounded, ScheduleRounded } from "@mui/icons-material";
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

export function CronJobsPanel() {
  const theme = useTheme();
  const [jobs, setJobs] = useState<CronJobSummary[]>([]);
  const [loading, setLoading] = useState(true);
  const [deletingId, setDeletingId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

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

  return (
    <Box>
      <Stack direction="row" alignItems="center" spacing={1} sx={{ mb: 2 }}>
        <ScheduleRounded fontSize="small" color="action" />
        <Typography variant="subtitle2" fontWeight={600}>
          Scheduled Jobs
        </Typography>
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
    </Box>
  );
}
