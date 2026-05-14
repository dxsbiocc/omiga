import { useEffect, useState } from "react";
import { Box, Chip, Stack, Typography } from "@mui/material";
import {
  fetchSessionArtifacts,
  type ArtifactEntry,
} from "../../state/sessionArtifacts";

interface SessionArtifactsPanelProps {
  sessionId: string;
  /** Refresh trigger — increment to force a re-fetch */
  refreshKey?: number;
}

function shortenPath(p: string, max = 60): string {
  if (p.length <= max) return p;
  return "..." + p.slice(-(max - 3));
}

function relativeTime(ts: string): string {
  const diff = Date.now() - new Date(ts).getTime();
  const secs = Math.floor(diff / 1000);
  if (secs < 60) return `${secs}s ago`;
  const mins = Math.floor(secs / 60);
  if (mins < 60) return `${mins}m ago`;
  return new Date(ts).toLocaleTimeString();
}

export function SessionArtifactsPanel({
  sessionId,
  refreshKey,
}: SessionArtifactsPanelProps) {
  const [artifacts, setArtifacts] = useState<ArtifactEntry[]>([]);

  useEffect(() => {
    if (!sessionId) return;
    fetchSessionArtifacts(sessionId)
      .then(setArtifacts)
      .catch(() => {});
  }, [sessionId, refreshKey]);

  if (artifacts.length === 0) return null;

  return (
    <Box sx={{ mt: 1.5 }}>
      <Typography
        variant="caption"
        color="text.secondary"
        sx={{ fontSize: 10, textTransform: "uppercase", letterSpacing: 0.5, mb: 0.5, display: "block" }}
      >
        Files changed
      </Typography>
      <Stack spacing={0.4}>
        {artifacts.map((a) => (
          <Box
            key={a.path}
            sx={{ display: "flex", alignItems: "center", gap: 0.75, minWidth: 0 }}
          >
            <Chip
              label={a.operation === "write" ? "NEW" : "EDIT"}
              size="small"
              color={a.operation === "write" ? "primary" : "default"}
              sx={{ fontSize: 9, height: 16, "& .MuiChip-label": { px: 0.6 }, flexShrink: 0 }}
            />
            <Typography
              variant="caption"
              sx={{
                fontFamily: "monospace",
                fontSize: 10.5,
                flex: 1,
                minWidth: 0,
                overflow: "hidden",
                textOverflow: "ellipsis",
                whiteSpace: "nowrap",
                color: "text.primary",
              }}
              title={a.path}
            >
              {shortenPath(a.path)}
            </Typography>
            <Typography
              variant="caption"
              color="text.disabled"
              sx={{ fontSize: 10, flexShrink: 0 }}
            >
              {relativeTime(a.ts)}
            </Typography>
          </Box>
        ))}
      </Stack>
    </Box>
  );
}
