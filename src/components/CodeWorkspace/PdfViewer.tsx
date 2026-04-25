import "../../lib/pdfWorker";
import { useState, useEffect, useRef, useCallback } from "react";
import { useWheelZoom } from "../../lib/useWheelZoom";
import { invoke } from "@tauri-apps/api/core";
import { Document, Page } from "react-pdf";
import "react-pdf/dist/Page/AnnotationLayer.css";
import "react-pdf/dist/Page/TextLayer.css";
import {
  Box,
  CircularProgress,
  Typography,
  IconButton,
  Slider,
  Tooltip,
  Stack,
  TextField,
  alpha,
  useTheme,
} from "@mui/material";
import NavigateBeforeRoundedIcon from "@mui/icons-material/NavigateBeforeRounded";
import NavigateNextRoundedIcon from "@mui/icons-material/NavigateNextRounded";
import ZoomInRoundedIcon from "@mui/icons-material/ZoomInRounded";
import ZoomOutRoundedIcon from "@mui/icons-material/ZoomOutRounded";
import FitScreenRoundedIcon from "@mui/icons-material/FitScreenRounded";

interface ImageReadResponse {
  data: string;
  mime_type: string;
}

const ZOOM_MIN = 25;
const ZOOM_MAX = 400;
const ZOOM_STEP = 25;

interface PdfViewerProps {
  filePath: string;
}

export function PdfViewer({ filePath }: PdfViewerProps) {
  const theme = useTheme();
  const canvasRef = useRef<HTMLDivElement>(null);

  const [fileData, setFileData] = useState<{ data: Uint8Array } | null>(null);
  const [isLoading, setIsLoading] = useState(true);
  const [loadError, setLoadError] = useState<string | null>(null);

  const [numPages, setNumPages] = useState<number>(0);
  const [page, setPage] = useState(1);
  const [pageInput, setPageInput] = useState("1");
  const [zoom, setZoom] = useState(100);
  const [pageLoading, setPageLoading] = useState(false);

  // ── Load PDF as base64 ──────────────────────────────────────────────────────

  useEffect(() => {
    let cancelled = false;
    setIsLoading(true);
    setLoadError(null);
    setFileData(null);
    setNumPages(0);
    setPage(1);
    setPageInput("1");
    setZoom(100);

    invoke<ImageReadResponse>("read_image_base64", { path: filePath })
      .then((res) => {
        if (cancelled) return;
        // pdf.js requires raw binary bytes (Uint8Array), NOT a base64 string.
        // atob() decodes base64 → binary string, then we copy each char code
        // into a Uint8Array so pdf.js can parse the actual PDF structure.
        const binary = atob(res.data);
        const bytes = new Uint8Array(binary.length);
        for (let i = 0; i < binary.length; i++) {
          bytes[i] = binary.charCodeAt(i);
        }
        setFileData({ data: bytes });
        setIsLoading(false);
      })
      .catch((e) => {
        if (cancelled) return;
        setLoadError(String(e));
        setIsLoading(false);
      });

    return () => { cancelled = true; };
  }, [filePath]);

  // ── Pinch / Ctrl+scroll zoom ────────────────────────────────────────────────

  useWheelZoom(canvasRef, setZoom, { min: ZOOM_MIN, max: ZOOM_MAX });

  // ── Page navigation ─────────────────────────────────────────────────────────

  const goTo = useCallback((n: number) => {
    const clamped = Math.max(1, Math.min(numPages, n));
    setPage(clamped);
    setPageInput(String(clamped));
  }, [numPages]);

  const handlePageInputBlur = () => {
    const n = parseInt(pageInput, 10);
    if (Number.isNaN(n)) { setPageInput(String(page)); return; }
    goTo(n);
  };

  const handlePageInputKey = (e: React.KeyboardEvent) => {
    if (e.key === "Enter") handlePageInputBlur();
    if (e.key === "Escape") setPageInput(String(page));
  };

  // ── Render ──────────────────────────────────────────────────────────────────

  if (isLoading) {
    return (
      <Stack alignItems="center" justifyContent="center" sx={{ flex: 1 }} gap={1.5}>
        <CircularProgress size={28} />
        <Typography variant="body2" color="text.secondary">正在加载 PDF…</Typography>
      </Stack>
    );
  }

  if (loadError) {
    return (
      <Stack alignItems="center" justifyContent="center" sx={{ flex: 1, px: 3 }}>
        <Typography variant="body2" color="error" textAlign="center">{loadError}</Typography>
      </Stack>
    );
  }

  // Width of the rendered page (pixels), scaled by zoom
  const pageWidth = Math.round(680 * (zoom / 100));

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
        {/* Page navigation */}
        <Tooltip title="上一页">
          <span>
            <IconButton size="small" onClick={() => goTo(page - 1)} disabled={page <= 1} sx={{ p: 0.5 }}>
              <NavigateBeforeRoundedIcon sx={{ fontSize: 18 }} />
            </IconButton>
          </span>
        </Tooltip>

        <Stack direction="row" alignItems="center" gap={0.5}>
          <TextField
            size="small"
            value={pageInput}
            onChange={(e) => setPageInput(e.target.value)}
            onBlur={handlePageInputBlur}
            onKeyDown={handlePageInputKey}
            inputProps={{
              style: {
                width: 36,
                padding: "2px 6px",
                textAlign: "center",
                fontSize: 12,
                fontVariantNumeric: "tabular-nums",
              },
            }}
            sx={{
              "& .MuiOutlinedInput-root": { borderRadius: 1 },
              "& .MuiOutlinedInput-notchedOutline": { borderColor: alpha(theme.palette.divider, 0.6) },
            }}
          />
          <Typography variant="caption" color="text.secondary" sx={{ fontSize: 11, whiteSpace: "nowrap" }}>
            / {numPages}
          </Typography>
        </Stack>

        <Tooltip title="下一页">
          <span>
            <IconButton size="small" onClick={() => goTo(page + 1)} disabled={page >= numPages} sx={{ p: 0.5 }}>
              <NavigateNextRoundedIcon sx={{ fontSize: 18 }} />
            </IconButton>
          </span>
        </Tooltip>

        <Box sx={{ flex: 1 }} />

        {/* Zoom */}
        <Tooltip title="缩小">
          <IconButton size="small" onClick={() => setZoom((z) => Math.max(ZOOM_MIN, z - ZOOM_STEP))} sx={{ p: 0.5 }}>
            <ZoomOutRoundedIcon sx={{ fontSize: 18 }} />
          </IconButton>
        </Tooltip>

        <Slider
          size="small"
          min={ZOOM_MIN}
          max={ZOOM_MAX}
          step={ZOOM_STEP}
          value={zoom}
          onChange={(_, v) => setZoom(v as number)}
          sx={{ width: 90, mx: 0.5 }}
        />

        <Tooltip title="放大">
          <IconButton size="small" onClick={() => setZoom((z) => Math.min(ZOOM_MAX, z + ZOOM_STEP))} sx={{ p: 0.5 }}>
            <ZoomInRoundedIcon sx={{ fontSize: 18 }} />
          </IconButton>
        </Tooltip>

        <Typography
          variant="caption"
          sx={{ minWidth: 40, textAlign: "center", color: "text.secondary", fontSize: 11, fontVariantNumeric: "tabular-nums" }}
        >
          {zoom}%
        </Typography>

        <Tooltip title="适合宽度">
          <IconButton size="small" onClick={() => setZoom(100)} sx={{ p: 0.5 }}>
            <FitScreenRoundedIcon sx={{ fontSize: 18 }} />
          </IconButton>
        </Tooltip>
      </Stack>

      {/* ── PDF canvas ── */}
      <Box
        ref={canvasRef}
        sx={{
          flex: 1,
          minHeight: 0,
          overflow: "auto",
          display: "flex",
          flexDirection: "column",
          alignItems: "center",
          bgcolor: alpha(theme.palette.grey[800], 0.35),
          py: 2,
          gap: 1.5,
          position: "relative",
        }}
      >
        {pageLoading && (
          <Box sx={{ position: "absolute", top: 12, right: 12, zIndex: 1 }}>
            <CircularProgress size={16} />
          </Box>
        )}

        <Document
          file={fileData}
          onLoadSuccess={({ numPages: n }) => setNumPages(n)}
          onLoadError={(e) => setLoadError(e.message)}
          loading={
            <Stack alignItems="center" justifyContent="center" sx={{ height: 200 }} gap={1.5}>
              <CircularProgress size={24} />
              <Typography variant="caption" color="text.secondary">解析文档…</Typography>
            </Stack>
          }
          error={
            <Typography variant="body2" color="error" sx={{ p: 3 }}>
              无法解析 PDF 文件
            </Typography>
          }
        >
          <Box
            sx={{
              boxShadow: theme.shadows[6],
              bgcolor: "#fff",
              lineHeight: 0,
            }}
          >
            <Page
              pageNumber={page}
              width={pageWidth}
              onRenderSuccess={() => setPageLoading(false)}
              onRenderError={() => setPageLoading(false)}
              loading={
                <Box
                  sx={{
                    width: pageWidth,
                    height: Math.round(pageWidth * 1.414), // A4 ratio
                    display: "flex",
                    alignItems: "center",
                    justifyContent: "center",
                    bgcolor: "#fff",
                  }}
                >
                  <CircularProgress size={24} sx={{ color: "grey.400" }} />
                </Box>
              }
            />
          </Box>
        </Document>
      </Box>

      {/* ── Status bar ── */}
      <Stack
        direction="row"
        alignItems="center"
        sx={{
          height: 24,
          px: 2,
          flexShrink: 0,
          bgcolor: alpha(theme.palette.grey[200], 0.45),
          borderTop: `1px solid ${theme.palette.divider}`,
        }}
      >
        <Typography variant="caption" color="text.secondary" sx={{ fontSize: 11 }}>
          {numPages > 0 ? `PDF · 第 ${page} / ${numPages} 页 · Ctrl+滚轮缩放` : "PDF"}
        </Typography>
      </Stack>
    </Box>
  );
}
