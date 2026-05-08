import { useState, useEffect, useRef, useCallback } from "react";
import { useWheelZoom } from "../../lib/useWheelZoom";
import { invoke } from "@tauri-apps/api/core";
import {
  Box,
  CircularProgress,
  Typography,
  IconButton,
  Slider,
  Tooltip,
  Stack,
  alpha,
  useTheme,
} from "@mui/material";
import ZoomInRoundedIcon from "@mui/icons-material/ZoomInRounded";
import ZoomOutRoundedIcon from "@mui/icons-material/ZoomOutRounded";
import FitScreenRoundedIcon from "@mui/icons-material/FitScreenRounded";
import CropFreeRoundedIcon from "@mui/icons-material/CropFreeRounded";
import { getLocalWorkspaceSessionId } from "../../utils/sshWorkspace";

interface ImageReadResponse {
  data: string;
  mime_type: string;
}

type FitMode = "fit" | "actual";

const ZOOM_MIN = 10;
const ZOOM_MAX = 800;
const ZOOM_STEP = 10;

interface ImageViewerProps {
  filePath: string;
}

export function ImageViewer({ filePath }: ImageViewerProps) {
  const theme = useTheme();
  const canvasRef = useRef<HTMLDivElement>(null);

  const [src, setSrc] = useState<string | null>(null);
  const [isLoading, setIsLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [zoom, setZoom] = useState(100);
  const [fit, setFit] = useState<FitMode>("fit");
  const [naturalSize, setNaturalSize] = useState<{ w: number; h: number } | null>(null);

  // ── Load image ──────────────────────────────────────────────────────────────

  useEffect(() => {
    let cancelled = false;
    setIsLoading(true);
    setError(null);
    setSrc(null);
    setNaturalSize(null);
    setZoom(100);
    setFit("fit");

    const sessionId = getLocalWorkspaceSessionId();
    if (!sessionId) {
      setError("请先选择本地工作区后再读取图片");
      setIsLoading(false);
      return () => { cancelled = true; };
    }

    invoke<ImageReadResponse>("read_image_base64", { path: filePath, sessionId })
      .then((res) => {
        if (cancelled) return;
        setSrc(`data:${res.mime_type};base64,${res.data}`);
        setIsLoading(false);
      })
      .catch((e) => {
        if (cancelled) return;
        setError(String(e));
        setIsLoading(false);
      });

    return () => { cancelled = true; };
  }, [filePath]);

  // ── Pinch / Ctrl+scroll zoom ────────────────────────────────────────────────

  const onZoomStart = useCallback(() => setFit("actual"), []);
  useWheelZoom(canvasRef, setZoom, { min: ZOOM_MIN, max: ZOOM_MAX, onZoomStart });

  // ── Controls ────────────────────────────────────────────────────────────────

  const handleZoomIn = useCallback(() => {
    setFit("actual");
    setZoom((z) => Math.min(ZOOM_MAX, z + ZOOM_STEP));
  }, []);

  const handleZoomOut = useCallback(() => {
    setFit("actual");
    setZoom((z) => Math.max(ZOOM_MIN, z - ZOOM_STEP));
  }, []);

  const handleFitToggle = useCallback(() => {
    setFit((prev) => {
      if (prev === "fit") { setZoom(100); return "actual"; }
      return "fit";
    });
  }, []);

  // ── States ──────────────────────────────────────────────────────────────────

  if (isLoading) {
    return (
      <Stack alignItems="center" justifyContent="center" sx={{ flex: 1 }} gap={1.5}>
        <CircularProgress size={28} />
        <Typography variant="body2" color="text.secondary">正在加载图片…</Typography>
      </Stack>
    );
  }

  if (error) {
    return (
      <Stack alignItems="center" justifyContent="center" sx={{ flex: 1, px: 3 }}>
        <Typography variant="body2" color="error" textAlign="center">{error}</Typography>
      </Stack>
    );
  }

  // ── Render ──────────────────────────────────────────────────────────────────

  const imgSx = fit === "fit"
    ? { maxWidth: "100%", maxHeight: "100%", objectFit: "contain" as const }
    : { width: `${zoom}%`, height: "auto", minWidth: `${zoom}%` };

  return (
    <Box sx={{ display: "flex", flexDirection: "column", flex: 1, minHeight: 0, overflow: "hidden" }}>
      {/* ── Toolbar ── */}
      <Stack
        direction="row"
        alignItems="center"
        gap={0.5}
        sx={{
          px: 1.5,
          py: 0.5,
          flexShrink: 0,
          borderBottom: `1px solid ${theme.palette.divider}`,
          bgcolor: alpha(theme.palette.grey[900], 0.4),
        }}
      >
        <Tooltip title="缩小">
          <IconButton size="small" onClick={handleZoomOut} sx={{ p: 0.5 }}>
            <ZoomOutRoundedIcon sx={{ fontSize: 18 }} />
          </IconButton>
        </Tooltip>

        <Slider
          size="small"
          min={ZOOM_MIN}
          max={ZOOM_MAX}
          step={ZOOM_STEP}
          value={zoom}
          onChange={(_, v) => { setFit("actual"); setZoom(v as number); }}
          sx={{ width: 100, mx: 0.5 }}
        />

        <Tooltip title="放大">
          <IconButton size="small" onClick={handleZoomIn} sx={{ p: 0.5 }}>
            <ZoomInRoundedIcon sx={{ fontSize: 18 }} />
          </IconButton>
        </Tooltip>

        <Typography
          variant="caption"
          sx={{
            minWidth: 44,
            textAlign: "center",
            fontVariantNumeric: "tabular-nums",
            color: "text.secondary",
            fontSize: 11,
          }}
        >
          {zoom}%
        </Typography>

        <Box sx={{ flex: 1 }} />

        <Tooltip title={fit === "fit" ? "实际大小 (100%)" : "适合窗口"}>
          <IconButton size="small" onClick={handleFitToggle} sx={{ p: 0.5 }}>
            {fit === "fit"
              ? <CropFreeRoundedIcon sx={{ fontSize: 18 }} />
              : <FitScreenRoundedIcon sx={{ fontSize: 18 }} />}
          </IconButton>
        </Tooltip>

        {naturalSize && (
          <Typography variant="caption" color="text.disabled" sx={{ fontSize: 11, ml: 0.5 }}>
            {naturalSize.w} × {naturalSize.h}
          </Typography>
        )}
      </Stack>

      {/* ── Canvas — overflow:auto enables panning when image is larger than view ── */}
      <Box
        ref={canvasRef}
        sx={{
          flex: 1,
          minHeight: 0,
          overflow: "auto",
          display: "flex",
          alignItems: fit === "fit" ? "center" : "flex-start",
          justifyContent: fit === "fit" ? "center" : "flex-start",
          backgroundImage: `
            linear-gradient(45deg, ${alpha(theme.palette.grey[700], 0.25)} 25%, transparent 25%),
            linear-gradient(-45deg, ${alpha(theme.palette.grey[700], 0.25)} 25%, transparent 25%),
            linear-gradient(45deg, transparent 75%, ${alpha(theme.palette.grey[700], 0.25)} 75%),
            linear-gradient(-45deg, transparent 75%, ${alpha(theme.palette.grey[700], 0.25)} 75%)
          `,
          backgroundSize: "16px 16px",
          backgroundPosition: "0 0, 0 8px, 8px -8px, -8px 0",
          cursor: fit === "actual" ? "grab" : "default",
          p: fit === "fit" ? 2 : 1,
        }}
      >
        {src && (
          <Box
            component="img"
            src={src}
            alt="preview"
            draggable={false}
            onLoad={(e) => {
              const img = e.currentTarget as HTMLImageElement;
              setNaturalSize({ w: img.naturalWidth, h: img.naturalHeight });
            }}
            sx={{
              display: "block",
              flexShrink: 0,
              imageRendering: zoom > 200 ? "pixelated" : "auto",
              boxShadow: theme.shadows[4],
              ...imgSx,
            }}
          />
        )}
      </Box>
    </Box>
  );
}
