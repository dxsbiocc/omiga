import { useEffect, useState } from "react";
import {
  Alert,
  Box,
  Button,
  Chip,
  CircularProgress,
  Paper,
  Snackbar,
  Stack,
  Typography,
  type AlertColor,
  type ChipProps,
} from "@mui/material";
import { alpha, useTheme } from "@mui/material/styles";
import { PlayArrowRounded, RefreshRounded } from "@mui/icons-material";
import {
  type Playbook,
  type PlaybookStatus,
  type ReplayPlaybookResponse,
} from "../../state/playbookTypes";
import { usePlaybookStore } from "../../state/playbookStore";

type ReplayNotice = {
  severity: AlertColor;
  message: string;
};

function playbookStatusColor(status: PlaybookStatus): ChipProps["color"] {
  switch (status) {
    case "active":
      return "success";
    case "stale":
      return "warning";
    case "quarantined":
      return "error";
    default:
      return "default";
  }
}

function formatVerifiedAt(value?: string | null): string {
  if (!value) return "Never verified";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return date.toLocaleString();
}

function replayNotice(response: ReplayPlaybookResponse): ReplayNotice {
  switch (response.outcome) {
    case "replayed":
      return {
        severity: response.verified ? "success" : "warning",
        message: `Playbook replayed; verified: ${response.verified ? "yes" : "no"}.`,
      };
    case "invalidated":
      return {
        severity: "warning",
        message: "Playbook invalidated by version or environment drift.",
      };
    case "notFound":
      return {
        severity: "error",
        message: "Playbook was not found.",
      };
    case "inactive":
      return {
        severity: "warning",
        message: "Playbook is inactive.",
      };
    default:
      return {
        severity: "info",
        message: "Replay completed.",
      };
  }
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function PlaybookCard({
  playbook,
  replaying,
  replayDisabled,
  onReplay,
}: {
  playbook: Playbook;
  replaying: boolean;
  replayDisabled: boolean;
  onReplay: (playbook: Playbook) => void;
}) {
  const status = playbook.health.status;
  const healthLabel = `${playbook.health.successCount}/${playbook.health.hitCount} successful`;

  return (
    <Paper
      variant="outlined"
      sx={{
        p: { xs: 1.5, md: 2 },
        borderRadius: 2,
        bgcolor: "background.paper",
      }}
    >
      <Stack
        direction={{ xs: "column", md: "row" }}
        spacing={1.5}
        alignItems={{ xs: "stretch", md: "center" }}
      >
        <Stack spacing={0.75} sx={{ flex: 1, minWidth: 0 }}>
          <Stack direction="row" spacing={0.75} alignItems="center" flexWrap="wrap" useFlexGap>
            <Typography variant="subtitle2" fontWeight={800} sx={{ minWidth: 0 }}>
              {playbook.title}
            </Typography>
            <Chip size="small" color={playbookStatusColor(status)} label={status} />
            <Chip size="small" variant="outlined" label={playbook.kind} />
          </Stack>
          <Stack direction="row" spacing={1} flexWrap="wrap" useFlexGap>
            <Typography
              variant="caption"
              color="text.secondary"
              title={playbook.canonicalId}
              sx={{ wordBreak: "break-word" }}
            >
              canonical: {playbook.canonicalId}
            </Typography>
            <Typography variant="caption" color="text.secondary">
              version: {playbook.operatorVersion}
            </Typography>
          </Stack>
          <Stack direction="row" spacing={1} flexWrap="wrap" useFlexGap>
            <Typography variant="caption" color="text.secondary">
              health: {healthLabel}
            </Typography>
            <Typography variant="caption" color="text.secondary">
              last verified: {formatVerifiedAt(playbook.health.lastVerifiedAt)}
            </Typography>
          </Stack>
        </Stack>
        <Button
          variant="outlined"
          size="small"
          startIcon={replaying ? <CircularProgress size={14} /> : <PlayArrowRounded />}
          disabled={replayDisabled}
          onClick={() => onReplay(playbook)}
          sx={{ textTransform: "none", borderRadius: 1.5, alignSelf: { xs: "flex-start", md: "center" } }}
        >
          Replay
        </Button>
      </Stack>
    </Paper>
  );
}

export function PlaybooksPanel({ projectPath }: { projectPath?: string }) {
  const theme = useTheme();
  const {
    playbooks,
    isLoading,
    error,
    listPlaybooks,
    replayPlaybook,
  } = usePlaybookStore();
  const projectRoot = projectPath?.trim() || undefined;
  const [replayingId, setReplayingId] = useState<string | null>(null);
  const [notice, setNotice] = useState<ReplayNotice | null>(null);

  useEffect(() => {
    void listPlaybooks(projectRoot).catch((err: unknown) => {
      setNotice({ severity: "error", message: errorMessage(err) });
    });
  }, [listPlaybooks, projectRoot]);

  const handleRefresh = () => {
    setNotice(null);
    void listPlaybooks(projectRoot).catch((err: unknown) => {
      setNotice({ severity: "error", message: errorMessage(err) });
    });
  };

  const handleReplay = async (playbook: Playbook) => {
    setNotice(null);
    setReplayingId(playbook.playbookId);
    try {
      const response = await replayPlaybook({
        playbookId: playbook.playbookId,
        projectRoot,
      });
      setNotice(replayNotice(response));
    } catch (err) {
      setNotice({ severity: "error", message: errorMessage(err) });
    } finally {
      setReplayingId(null);
    }
  };

  return (
    <>
      <Stack spacing={2} useFlexGap>
        <Paper
          variant="outlined"
          sx={{
            p: { xs: 2, md: 2.5 },
            borderRadius: 3,
            bgcolor: alpha(theme.palette.primary.main, theme.palette.mode === "dark" ? 0.12 : 0.05),
            borderColor: alpha(theme.palette.primary.main, theme.palette.mode === "dark" ? 0.32 : 0.16),
          }}
        >
          <Stack
            direction={{ xs: "column", sm: "row" }}
            spacing={1.5}
            alignItems={{ xs: "stretch", sm: "center" }}
          >
            <Box sx={{ flex: 1, minWidth: 0 }}>
              <Typography variant="h6" fontWeight={800}>
                Playbooks
              </Typography>
              <Typography variant="body2" color="text.secondary">
                Review distilled playbooks and replay them against the current project.
              </Typography>
            </Box>
            <Button
              variant="outlined"
              startIcon={isLoading ? <CircularProgress size={16} /> : <RefreshRounded />}
              disabled={isLoading}
              onClick={handleRefresh}
              sx={{ textTransform: "none", borderRadius: 2, minHeight: 40 }}
            >
              Refresh
            </Button>
          </Stack>
        </Paper>

        {error && (
          <Alert severity="error" sx={{ borderRadius: 2 }}>
            {error}
          </Alert>
        )}

        {isLoading ? (
          <Box sx={{ display: "flex", justifyContent: "center", py: 4 }}>
            <CircularProgress size={24} />
          </Box>
        ) : playbooks.length === 0 ? (
          <Paper variant="outlined" sx={{ p: 3, borderRadius: 2, textAlign: "center" }}>
            <Typography variant="body2" color="text.secondary">
              No playbooks yet
            </Typography>
          </Paper>
        ) : (
          <Stack spacing={1.25} useFlexGap>
            {playbooks.map((playbook) => (
              <PlaybookCard
                key={playbook.playbookId}
                playbook={playbook}
                replaying={replayingId === playbook.playbookId}
                replayDisabled={replayingId !== null}
                onReplay={(selected) => {
                  void handleReplay(selected);
                }}
              />
            ))}
          </Stack>
        )}
      </Stack>

      <Snackbar
        open={Boolean(notice)}
        autoHideDuration={4200}
        onClose={(_event, reason) => {
          if (reason !== "clickaway") setNotice(null);
        }}
        anchorOrigin={{ vertical: "bottom", horizontal: "center" }}
      >
        <Alert
          severity={notice?.severity ?? "info"}
          variant="filled"
          onClose={() => setNotice(null)}
          sx={{ borderRadius: 2, boxShadow: 4 }}
        >
          {notice?.message}
        </Alert>
      </Snackbar>
    </>
  );
}
