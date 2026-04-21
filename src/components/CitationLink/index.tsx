/**
 * CitationLink — inline citation chip with hover metadata tooltip.
 *
 * Renders as a small pill showing the source domain name (e.g. "PubMed", "ScienceDirect").
 * On hover, fetches and displays full metadata (title, authors, journal, year).
 */

import { useState, useCallback, useRef } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  Box,
  Typography,
  Chip,
  CircularProgress,
  Paper,
  Popper,
  Fade,
  alpha,
} from "@mui/material";
import { MenuBook, Language } from "@mui/icons-material";
import { invoke } from "@tauri-apps/api/core";
import { useTheme } from "@mui/material/styles";

// ── Types ─────────────────────────────────────────────────────────────────────

interface CitationMeta {
  url: string;
  title?: string | null;
  description?: string | null;
  authors: string[];
  journal?: string | null;
  year?: number | null;
  doi?: string | null;
  kind: "academic" | "web" | "unknown";
}

type FetchState = "idle" | "loading" | "done" | "error";

// ── Client-side cache ─────────────────────────────────────────────────────────

const metaCache = new Map<string, CitationMeta>();

// ── URL → display label ───────────────────────────────────────────────────────

const SOURCE_MAP: [RegExp, string][] = [
  [/pubmed\.ncbi\.nlm\.nih\.gov|ncbi\.nlm\.nih\.gov\/pubmed/i, "PubMed"],
  [/pmc\.ncbi\.nlm\.nih\.gov/i, "PMC"],
  [/frontiersin\.org/i, "Frontiers"],
  [/sciencedirect\.com/i, "ScienceDirect"],
  [/onlinelibrary\.wiley\.com/i, "Wiley"],
  [/nature\.com/i, "Nature"],
  [/science\.org|sciencemag\.org/i, "Science"],
  [/cell\.com/i, "Cell"],
  [/pnas\.org/i, "PNAS"],
  [/nejm\.org/i, "NEJM"],
  [/thelancet\.com/i, "Lancet"],
  [/bmj\.com/i, "BMJ"],
  [/jama\.jamanetwork\.com|jamanetwork\.com/i, "JAMA"],
  [/arxiv\.org/i, "arXiv"],
  [/biorxiv\.org/i, "bioRxiv"],
  [/medrxiv\.org/i, "medRxiv"],
  [/springer\.com|link\.springer\.com/i, "Springer"],
  [/nih\.gov/i, "NIH"],
  [/cancer\.gov/i, "NCI"],
  [/cancerbiomed\.org/i, "Cancerbiomed"],
  [/amegroups\.com/i, "Amegroups"],
  [/doi\.org/i, "DOI"],
];

function sourceLabel(url: string): string {
  for (const [pattern, label] of SOURCE_MAP) {
    if (pattern.test(url)) return label;
  }
  try {
    const hostname = new URL(url).hostname.replace(/^www\./, "");
    const stem = hostname.split(".")[0];
    return stem.charAt(0).toUpperCase() + stem.slice(1);
  } catch {
    return "Link";
  }
}

function isCitationUrl(url: string): boolean {
  return (
    url.startsWith("http") &&
    (url.includes("doi.org") ||
      url.includes("pubmed") ||
      url.includes("arxiv.org") ||
      url.includes("frontiersin.org") ||
      url.includes("sciencedirect.com") ||
      url.includes("nature.com") ||
      url.includes("wiley.com") ||
      url.includes("springer.com") ||
      url.includes("biorxiv.org") ||
      url.includes("medrxiv.org") ||
      url.includes("pnas.org") ||
      url.includes("cell.com") ||
      url.includes("nejm.org") ||
      url.includes("thelancet.com") ||
      url.includes("nih.gov") ||
      url.includes("cancerbiomed.org") ||
      url.includes("amegroups.com") ||
      url.includes("science.org"))
  );
}

// ── Source icon (colored letter indicator) ────────────────────────────────────

const SOURCE_COLORS: Record<string, string> = {
  PubMed: "#336699",
  PMC: "#336699",
  ScienceDirect: "#FF6600",
  Frontiers: "#E55B2D",
  Nature: "#E43F2B",
  Science: "#1A5276",
  Cell: "#B03A2E",
  arXiv: "#B31B1B",
  bioRxiv: "#B31B1B",
  medRxiv: "#B31B1B",
  Wiley: "#003399",
  NEJM: "#0057A8",
  Lancet: "#001489",
  PNAS: "#003087",
  Springer: "#EF6C00",
  JAMA: "#002366",
  BMJ: "#0074B7",
  NIH: "#3B5998",
};

function SourceBadge({ label }: { label: string }) {
  const color = SOURCE_COLORS[label];
  if (!color) return <Language sx={{ fontSize: 12 }} />;
  return (
    <Box
      sx={{
        width: 14,
        height: 14,
        borderRadius: "3px",
        bgcolor: color,
        display: "inline-flex",
        alignItems: "center",
        justifyContent: "center",
        flexShrink: 0,
      }}
    >
      <Typography sx={{ fontSize: 9, color: "#fff", fontWeight: 700, lineHeight: 1 }}>
        {label.charAt(0)}
      </Typography>
    </Box>
  );
}

// ── Tooltip card ──────────────────────────────────────────────────────────────

function CitationTooltipCard({ meta, label }: { meta: CitationMeta; label: string }) {
  const authorLine =
    meta.authors.length > 0
      ? meta.authors.slice(0, 3).join(", ") +
        (meta.authors.length > 3 ? ` et al.` : "")
      : null;

  const sourceLine = meta.journal ?? label;

  return (
    <Paper
      elevation={6}
      sx={{
        p: 1.5,
        maxWidth: 340,
        borderRadius: 2,
        pointerEvents: "none",
      }}
    >
      {/* Title */}
      {meta.title ? (
        <Typography
          sx={{ fontSize: 13, fontWeight: 500, lineHeight: 1.45, mb: 0.75 }}
        >
          {meta.title}
        </Typography>
      ) : (
        <Typography
          variant="caption"
          color="text.secondary"
          sx={{ wordBreak: "break-all", display: "block", mb: 0.5 }}
        >
          {meta.url}
        </Typography>
      )}

      {/* Authors */}
      {authorLine && (
        <Typography
          variant="caption"
          color="text.secondary"
          sx={{ display: "block", mb: 0.5, lineHeight: 1.4 }}
        >
          {authorLine}
          {meta.year ? ` · ${meta.year}` : ""}
        </Typography>
      )}

      {/* Source row */}
      <Box sx={{ display: "flex", alignItems: "center", gap: 0.5, mt: 0.25 }}>
        <SourceBadge label={label} />
        <Typography
          variant="caption"
          color="text.secondary"
          sx={{ fontWeight: 500, fontSize: 11 }}
        >
          {sourceLine}
        </Typography>
        {meta.kind === "academic" && (
          <MenuBook sx={{ fontSize: 11, color: "text.disabled", ml: "auto" }} />
        )}
      </Box>

      {/* Abstract snippet */}
      {meta.description && (
        <Typography
          variant="caption"
          color="text.secondary"
          sx={{ display: "block", mt: 0.75, lineHeight: 1.5, borderTop: "1px solid", borderColor: "divider", pt: 0.75 }}
        >
          {meta.description}
        </Typography>
      )}
    </Paper>
  );
}

// ── Main component ────────────────────────────────────────────────────────────

interface CitationLinkProps {
  href?: string;
  children?: React.ReactNode;
  accentColor?: string;
}

export function CitationLink({ href, children, accentColor }: CitationLinkProps) {
  const theme = useTheme();
  const url = href ?? "#";
  const anchorRef = useRef<HTMLSpanElement>(null);
  const hoverTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const [open, setOpen] = useState(false);
  const [hovered, setHovered] = useState(false);
  const [fetchState, setFetchState] = useState<FetchState>("idle");
  const [meta, setMeta] = useState<CitationMeta | null>(null);

  const label = sourceLabel(url);
  const isCitation = isCitationUrl(url);

  const fetchMeta = useCallback(async () => {
    if (!url.startsWith("http")) return;
    if (metaCache.has(url)) {
      setMeta(metaCache.get(url)!);
      setFetchState("done");
      return;
    }
    setFetchState("loading");
    try {
      const result = await invoke<CitationMeta>("fetch_citation_metadata", { url });
      metaCache.set(url, result);
      setMeta(result);
      setFetchState("done");
    } catch {
      setFetchState("error");
    }
  }, [url]);

  const handleMouseEnter = useCallback(() => {
    setHovered(true);
    hoverTimer.current = setTimeout(() => {
      setOpen(true);
      if (fetchState === "idle") fetchMeta();
    }, 250);
  }, [fetchMeta, fetchState]);

  const handleMouseLeave = useCallback(() => {
    setHovered(false);
    if (hoverTimer.current) clearTimeout(hoverTimer.current);
    setOpen(false);
  }, []);

  const handleClick = useCallback(
    (e: React.MouseEvent) => {
      e.preventDefault();
      if (url.startsWith("http")) void openUrl(url);
    },
    [url],
  );

  // Non-citation URLs: render as regular styled link
  if (!isCitation) {
    return (
      <Box
        component="a"
        href={url}
        target="_blank"
        rel="noopener noreferrer"
        onClick={handleClick}
        sx={{
          color: accentColor ?? "primary.main",
          textDecoration: "underline",
          textDecorationStyle: "solid",
          textUnderlineOffset: 2,
          cursor: "pointer",
          "&:hover": { opacity: 0.8 },
        }}
      >
        {children}
      </Box>
    );
  }

  const chipBg =
    theme.palette.mode === "dark"
      ? alpha(theme.palette.common.white, 0.08)
      : alpha(theme.palette.common.black, 0.06);
  const chipBgHover =
    theme.palette.mode === "dark"
      ? alpha(theme.palette.common.white, 0.14)
      : alpha(theme.palette.common.black, 0.1);
  const chipBorder =
    theme.palette.mode === "dark"
      ? alpha(theme.palette.common.white, 0.14)
      : alpha(theme.palette.common.black, 0.12);

  return (
    <>
      <Box
        component="span"
        ref={anchorRef}
        sx={{ display: "inline-flex", alignItems: "center", verticalAlign: "middle", mx: 0.25 }}
      >
        <Chip
          component="a"
          href={url}
          target="_blank"
          rel="noopener noreferrer"
          onClick={handleClick}
          onMouseEnter={handleMouseEnter}
          onMouseLeave={handleMouseLeave}
          label={
            <Box
              sx={{ display: "inline-flex", alignItems: "center", gap: 0.3, lineHeight: 1 }}
            >
              <Typography
                sx={{
                  fontSize: 11,
                  fontWeight: 450,
                  maxWidth: 120,
                  overflow: "hidden",
                  textOverflow: "ellipsis",
                  whiteSpace: "nowrap",
                  lineHeight: 1,
                }}
              >
                {label}
              </Typography>
              {hovered && (
                <Typography sx={{ fontSize: 10, lineHeight: 1, opacity: 0.7 }}>↗</Typography>
              )}
            </Box>
          }
          size="small"
          sx={{
            height: 20,
            cursor: "pointer",
            bgcolor: chipBg,
            border: `1px solid ${chipBorder}`,
            borderRadius: "4px",
            color: "text.primary",
            transition: "background 0.15s",
            "& .MuiChip-label": { px: "6px", py: 0 },
            "&:hover": { bgcolor: chipBgHover },
          }}
        />
      </Box>

      <Popper
        open={open && url !== "#"}
        anchorEl={anchorRef.current}
        placement="top-start"
        transition
        modifiers={[
          { name: "offset", options: { offset: [0, 6] } },
          {
            name: "flip",
            options: { fallbackPlacements: ["bottom-start", "top-end", "bottom-end"] },
          },
          { name: "preventOverflow", options: { boundary: "clippingParents", padding: 8 } },
        ]}
        sx={{ zIndex: 2000, pointerEvents: "none" }}
      >
        {({ TransitionProps }) => (
          <Fade {...TransitionProps} timeout={150}>
            <Box>
              {fetchState === "loading" && (
                <Paper
                  elevation={4}
                  sx={{ p: 1.5, display: "flex", alignItems: "center", gap: 1, borderRadius: 2 }}
                >
                  <CircularProgress size={12} />
                  <Typography variant="caption" color="text.secondary">
                    加载引用信息…
                  </Typography>
                </Paper>
              )}
              {fetchState === "done" && meta && (
                <CitationTooltipCard meta={meta} label={label} />
              )}
              {fetchState === "error" && (
                <Paper elevation={3} sx={{ p: 1.25, maxWidth: 280, borderRadius: 2 }}>
                  <Box sx={{ display: "flex", alignItems: "center", gap: 0.5, mb: 0.5 }}>
                    <SourceBadge label={label} />
                    <Typography variant="caption" sx={{ fontWeight: 500 }}>
                      {label}
                    </Typography>
                  </Box>
                  <Typography
                    variant="caption"
                    color="text.secondary"
                    sx={{ wordBreak: "break-all" }}
                  >
                    {url}
                  </Typography>
                </Paper>
              )}
            </Box>
          </Fade>
        )}
      </Popper>
    </>
  );
}
