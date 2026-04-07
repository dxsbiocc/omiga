import {
  useState,
  useEffect,
  useMemo,
  useRef,
  useCallback,
  type KeyboardEvent,
  type ChangeEvent,
} from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import {
  Box,
  Typography,
  IconButton,
  Tooltip,
  alpha,
  useTheme,
} from "@mui/material";
import AddRoundedIcon from "@mui/icons-material/AddRounded";
import DeleteOutlineRoundedIcon from "@mui/icons-material/DeleteOutlineRounded";
import ArrowUpwardRoundedIcon from "@mui/icons-material/ArrowUpwardRounded";
import ArrowDownwardRoundedIcon from "@mui/icons-material/ArrowDownwardRounded";
import UnfoldMoreRoundedIcon from "@mui/icons-material/UnfoldMoreRounded";

// ─── CSV codec ────────────────────────────────────────────────────────────────

function parseCsv(text: string): string[][] {
  const rows: string[][] = [];
  let row: string[] = [];
  let cell = "";
  let inQuotes = false;
  let i = 0;
  while (i < text.length) {
    const ch = text[i];
    const next = text[i + 1];
    if (inQuotes) {
      if (ch === '"' && next === '"') { cell += '"'; i += 2; }
      else if (ch === '"') { inQuotes = false; i++; }
      else { cell += ch; i++; }
    } else {
      if (ch === '"') { inQuotes = true; i++; }
      else if (ch === ",") { row.push(cell); cell = ""; i++; }
      else if (ch === "\r" && next === "\n") { row.push(cell); cell = ""; rows.push(row); row = []; i += 2; }
      else if (ch === "\n" || ch === "\r") { row.push(cell); cell = ""; rows.push(row); row = []; i++; }
      else { cell += ch; i++; }
    }
  }
  row.push(cell);
  if (row.some((c) => c !== "")) rows.push(row);
  return rows;
}

function serializeCsv(headers: string[], rows: string[][]): string {
  const esc = (s: string) =>
    s.includes(",") || s.includes('"') || s.includes("\n")
      ? `"${s.replace(/"/g, '""')}"`
      : s;
  return [headers, ...rows].map((r) => r.map(esc).join(",")).join("\n");
}

// ─── Types ────────────────────────────────────────────────────────────────────

type SortDir = "asc" | "desc";
interface SortConfig { col: number; dir: SortDir }
/** row === -1 means the header row */
interface EditTarget { row: number; col: number }

// ─── Constants ────────────────────────────────────────────────────────────────

const ROW_H = 32;
const GUTTER_W = 48;
const COL_MIN_W = 120;
const MONO = '"JetBrains Mono","Fira Code",ui-monospace,monospace';

// ─── CsvViewer ────────────────────────────────────────────────────────────────

interface CsvViewerProps {
  content: string;
  onChange?: (csv: string) => void;
}

export function CsvViewer({ content, onChange }: CsvViewerProps) {
  const theme = useTheme();
  const scrollRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  // Parsed source — only changes when content prop changes
  const parsed = useMemo(() => {
    const all = parseCsv(content);
    if (all.length === 0) return { headers: [] as string[], dataRows: [] as string[][] };
    return { headers: all[0], dataRows: all.slice(1) };
  }, [content]);

  // Mutable copies
  const [headers, setHeaders] = useState<string[]>(() => parsed.headers);
  const [rows, setRows] = useState<string[][]>(() => parsed.dataRows);

  // Re-init when file changes
  useEffect(() => {
    setHeaders(parsed.headers);
    setRows(parsed.dataRows);
    setSortConfig(null);
    setEditTarget(null);
    setEditValue("");
    setHoverRow(null);
  }, [parsed]);

  const [sortConfig, setSortConfig] = useState<SortConfig | null>(null);
  const [editTarget, setEditTarget] = useState<EditTarget | null>(null);
  const [editValue, setEditValue] = useState("");
  const [hoverRow, setHoverRow] = useState<number | null>(null);

  const colCount = headers.length;

  // Notify parent whenever data mutates
  const notify = useCallback(
    (nextHeaders: string[], nextRows: string[][]) => {
      onChange?.(serializeCsv(nextHeaders, nextRows));
    },
    [onChange],
  );

  // ── Sort ────────────────────────────────────────────────────────────────────

  const displayIndices = useMemo(() => {
    const idx = rows.map((_, i) => i);
    if (!sortConfig) return idx;
    return [...idx].sort((a, b) => {
      const va = rows[a]?.[sortConfig.col] ?? "";
      const vb = rows[b]?.[sortConfig.col] ?? "";
      const na = Number(va);
      const nb = Number(vb);
      const cmp =
        va !== "" && vb !== "" && !Number.isNaN(na) && !Number.isNaN(nb)
          ? na - nb
          : va.localeCompare(vb, undefined, { numeric: true });
      return sortConfig.dir === "asc" ? cmp : -cmp;
    });
  }, [rows, sortConfig]);

  const cycleSort = useCallback((col: number) => {
    setSortConfig((prev) => {
      if (!prev || prev.col !== col) return { col, dir: "asc" };
      if (prev.dir === "asc") return { col, dir: "desc" };
      return null;
    });
  }, []);

  // ── Edit lifecycle ───────────────────────────────────────────────────────────

  const startEdit = useCallback((row: number, col: number, current: string) => {
    setEditTarget({ row, col });
    setEditValue(current);
    setTimeout(() => inputRef.current?.select(), 0);
  }, []);

  const commitEdit = useCallback(() => {
    if (!editTarget) return;
    const { row, col } = editTarget;
    if (row === -1) {
      // Header rename
      const next = headers.map((h, i) => (i === col ? editValue : h));
      setHeaders(next);
      notify(next, rows);
    } else {
      // Cell edit
      const next = rows.map((r, ri) =>
        ri === row ? r.map((c, ci) => (ci === col ? editValue : c)) : r,
      );
      setRows(next);
      notify(headers, next);
    }
    setEditTarget(null);
    setEditValue("");
  }, [editTarget, editValue, headers, rows, notify]);

  const cancelEdit = useCallback(() => {
    setEditTarget(null);
    setEditValue("");
  }, []);

  const onKeyDown = useCallback(
    (e: KeyboardEvent<HTMLInputElement>) => {
      if (e.key === "Enter" || e.key === "Tab") { e.preventDefault(); commitEdit(); }
      if (e.key === "Escape") cancelEdit();
    },
    [commitEdit, cancelEdit],
  );

  // ── Row mutations ────────────────────────────────────────────────────────────

  const addRow = useCallback(() => {
    const next = [...rows, Array<string>(colCount).fill("")];
    setRows(next);
    notify(headers, next);
  }, [rows, colCount, headers, notify]);

  const deleteRow = useCallback(
    (originalIdx: number) => {
      const next = rows.filter((_, i) => i !== originalIdx);
      setRows(next);
      notify(headers, next);
      setHoverRow(null);
    },
    [rows, headers, notify],
  );

  // ── Virtualizer ─────────────────────────────────────────────────────────────

  const rowVirtualizer = useVirtualizer({
    count: displayIndices.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => ROW_H,
    overscan: 15,
  });

  const virtualRows = rowVirtualizer.getVirtualItems();
  const totalH = rowVirtualizer.getTotalSize();

  // ── Colours ──────────────────────────────────────────────────────────────────

  const c = {
    headerBg: alpha(theme.palette.grey[900], 0.97),
    headerBorder: theme.palette.divider,
    rowBorder: alpha(theme.palette.divider, 0.35),
    gutterBg: alpha(theme.palette.grey[900], 0.35),
    hoverBg: alpha(theme.palette.primary.main, 0.07),
    editBg: alpha(theme.palette.primary.main, 0.12),
    colBorder: alpha(theme.palette.divider, 0.2),
    numColor: theme.palette.info.light,
    sortActive: theme.palette.primary.light,
  };

  if (headers.length === 0) {
    return (
      <Box sx={{ display: "flex", alignItems: "center", justifyContent: "center", flex: 1 }}>
        <Typography variant="body2" color="text.secondary">CSV 文件为空</Typography>
      </Box>
    );
  }

  // ── Shared cell input ────────────────────────────────────────────────────────

  const cellInput = (
    <input
      ref={inputRef}
      value={editValue}
      autoFocus
      onChange={(e: ChangeEvent<HTMLInputElement>) => setEditValue(e.target.value)}
      onBlur={commitEdit}
      onKeyDown={onKeyDown}
      style={{
        width: "100%",
        height: "100%",
        border: "none",
        outline: "none",
        background: "transparent",
        fontFamily: MONO,
        fontSize: 12,
        color: theme.palette.text.primary,
        padding: "0 12px",
        boxSizing: "border-box",
      }}
    />
  );

  // ── Render ───────────────────────────────────────────────────────────────────

  const minTableW = GUTTER_W + colCount * COL_MIN_W;

  return (
    <Box sx={{ display: "flex", flexDirection: "column", flex: 1, minHeight: 0, overflow: "hidden" }}>
      {/* ── Sticky header ── */}
      <Box
        sx={{
          minWidth: minTableW,
          display: "flex",
          flexShrink: 0,
          borderBottom: `1px solid ${c.headerBorder}`,
          bgcolor: c.headerBg,
          position: "sticky",
          top: 0,
          zIndex: 3,
          overflowX: "hidden",
        }}
      >
        {/* Gutter */}
        <Box sx={{ width: GUTTER_W, flexShrink: 0, borderRight: `1px solid ${c.headerBorder}`, display: "flex", alignItems: "center", justifyContent: "flex-end", pr: 1 }}>
          <Typography sx={{ fontSize: 10, fontWeight: 600, color: "text.disabled", fontFamily: MONO, letterSpacing: "0.04em" }}>#</Typography>
        </Box>
        {headers.map((h, ci) => {
          const isSort = sortConfig?.col === ci;
          const isEditing = editTarget?.row === -1 && editTarget.col === ci;
          return (
            <Box
              key={ci}
              sx={{
                flex: `0 0 ${COL_MIN_W}px`,
                minWidth: COL_MIN_W,
                height: ROW_H,
                display: "flex",
                alignItems: "center",
                borderRight: `1px solid ${alpha(c.headerBorder, 0.4)}`,
                position: "relative",
                cursor: "pointer",
                bgcolor: isEditing ? c.editBg : "transparent",
                "&:hover": { bgcolor: isEditing ? c.editBg : alpha(theme.palette.primary.main, 0.04) },
              }}
              onDoubleClick={() => !isEditing && startEdit(-1, ci, h)}
            >
              {isEditing ? (
                cellInput
              ) : (
                <>
                  <Typography
                    sx={{
                      flex: 1,
                      px: 1.5,
                      fontSize: 11,
                      fontWeight: 600,
                      letterSpacing: "0.04em",
                      textTransform: "uppercase",
                      color: isSort ? c.sortActive : "text.secondary",
                      fontFamily: MONO,
                      overflow: "hidden",
                      textOverflow: "ellipsis",
                      whiteSpace: "nowrap",
                      userSelect: "none",
                    }}
                  >
                    {h || `col_${ci + 1}`}
                  </Typography>
                  <Tooltip title={`排序 ${ci + 1} 列`} placement="top">
                    <IconButton
                      size="small"
                      onClick={() => cycleSort(ci)}
                      sx={{
                        mr: 0.5,
                        p: 0.25,
                        color: isSort ? c.sortActive : "text.disabled",
                        "&:hover": { color: "text.primary" },
                      }}
                    >
                      {isSort && sortConfig!.dir === "asc" ? (
                        <ArrowUpwardRoundedIcon sx={{ fontSize: 13 }} />
                      ) : isSort && sortConfig!.dir === "desc" ? (
                        <ArrowDownwardRoundedIcon sx={{ fontSize: 13 }} />
                      ) : (
                        <UnfoldMoreRoundedIcon sx={{ fontSize: 13 }} />
                      )}
                    </IconButton>
                  </Tooltip>
                </>
              )}
            </Box>
          );
        })}
      </Box>

      {/* ── Virtual body ── */}
      <Box ref={scrollRef} sx={{ flex: 1, overflow: "auto", minHeight: 0 }}>
        <Box sx={{ minWidth: minTableW, position: "relative", height: totalH }}>
          {virtualRows.map((vRow) => {
            const origIdx = displayIndices[vRow.index];
            const row = rows[origIdx] ?? [];
            const isHover = hoverRow === origIdx;

            return (
              <Box
                key={vRow.key}
                sx={{
                  position: "absolute",
                  top: vRow.start,
                  left: 0,
                  right: 0,
                  height: ROW_H,
                  display: "flex",
                  bgcolor: isHover ? c.hoverBg : "transparent",
                  borderBottom: `1px solid ${c.rowBorder}`,
                  "&:hover": { bgcolor: c.hoverBg },
                }}
                onMouseEnter={() => setHoverRow(origIdx)}
                onMouseLeave={() => setHoverRow(null)}
              >
                {/* Gutter: row number + delete button */}
                <Box
                  sx={{
                    width: GUTTER_W,
                    flexShrink: 0,
                    borderRight: `1px solid ${c.rowBorder}`,
                    bgcolor: c.gutterBg,
                    display: "flex",
                    alignItems: "center",
                    justifyContent: "flex-end",
                    pr: 0.5,
                    position: "relative",
                  }}
                >
                  {isHover ? (
                    <Tooltip title="删除行" placement="left">
                      <IconButton
                        size="small"
                        onClick={() => deleteRow(origIdx)}
                        sx={{ p: 0.3, color: "error.main", "&:hover": { bgcolor: alpha(theme.palette.error.main, 0.1) } }}
                      >
                        <DeleteOutlineRoundedIcon sx={{ fontSize: 14 }} />
                      </IconButton>
                    </Tooltip>
                  ) : (
                    <Typography sx={{ fontSize: 11, color: "text.disabled", fontFamily: MONO, fontVariantNumeric: "tabular-nums", pr: 0.5 }}>
                      {origIdx + 1}
                    </Typography>
                  )}
                </Box>

                {/* Cells */}
                {Array.from({ length: colCount }, (_, ci) => {
                  const val = row[ci] ?? "";
                  const isNum = val !== "" && !Number.isNaN(Number(val));
                  const isEditing = editTarget?.row === origIdx && editTarget.col === ci;

                  return (
                    <Box
                      key={ci}
                      sx={{
                        flex: `0 0 ${COL_MIN_W}px`,
                        minWidth: COL_MIN_W,
                        height: "100%",
                        display: "flex",
                        alignItems: "center",
                        borderRight: `1px solid ${c.colBorder}`,
                        cursor: "text",
                        bgcolor: isEditing ? c.editBg : "transparent",
                        outline: isEditing ? `1.5px solid ${theme.palette.primary.main}` : "none",
                        outlineOffset: -1,
                      }}
                      onClick={() => !isEditing && startEdit(origIdx, ci, val)}
                    >
                      {isEditing ? (
                        cellInput
                      ) : (
                        <Typography
                          component="span"
                          title={val}
                          sx={{
                            px: 1.5,
                            fontSize: 12,
                            fontFamily: MONO,
                            color: isNum ? c.numColor : "text.primary",
                            fontVariantNumeric: isNum ? "tabular-nums" : undefined,
                            overflow: "hidden",
                            textOverflow: "ellipsis",
                            whiteSpace: "nowrap",
                            width: "100%",
                          }}
                        >
                          {val}
                        </Typography>
                      )}
                    </Box>
                  );
                })}
              </Box>
            );
          })}
        </Box>

        {/* ── Add row ── */}
        <Box
          sx={{
            minWidth: minTableW,
            height: ROW_H,
            display: "flex",
            alignItems: "center",
            pl: `${GUTTER_W + 8}px`,
            borderTop: `1px solid ${c.rowBorder}`,
            cursor: "pointer",
            color: "text.disabled",
            "&:hover": { bgcolor: alpha(theme.palette.primary.main, 0.04), color: "text.secondary" },
          }}
          onClick={addRow}
        >
          <AddRoundedIcon sx={{ fontSize: 14, mr: 0.75 }} />
          <Typography sx={{ fontSize: 12, fontFamily: MONO, userSelect: "none" }}>
            添加行
          </Typography>
        </Box>
      </Box>
    </Box>
  );
}
