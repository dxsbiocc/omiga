import { useCallback, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useNotebookViewerStore, useUiStore } from "../../state";
import SettingsIcon from "@mui/icons-material/Settings";
import Editor from "@monaco-editor/react";
import type { editor } from "monaco-editor";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { useVirtualizer } from "@tanstack/react-virtual";
import { alpha } from "@mui/material/styles";
import {
  Box,
  Button,
  Chip,
  CircularProgress,
  Divider,
  IconButton,
  Stack,
  Tooltip,
  Typography,
  useTheme,
} from "@mui/material";
import PlayArrowRoundedIcon from "@mui/icons-material/PlayArrowRounded";
import PlayCircleOutlineIcon from "@mui/icons-material/PlayCircleOutline";
import DeleteOutlineIcon from "@mui/icons-material/DeleteOutline";
import AddIcon from "@mui/icons-material/Add";
import ClearAllIcon from "@mui/icons-material/ClearAll";
import VerticalAlignTopIcon from "@mui/icons-material/VerticalAlignTop";
import VerticalAlignBottomIcon from "@mui/icons-material/VerticalAlignBottom";

interface IpynbViewerProps {
  filePath: string;
  content: string;
  onChange?: (value: string) => void;
}

interface NbCell {
  cell_type: string;
  source: string | string[];
  metadata?: Record<string, unknown>;
  outputs?: unknown[];
  execution_count?: number | null;
}

interface NotebookDoc {
  cells: NbCell[];
  metadata?: Record<string, unknown>;
  nbformat?: number;
  nbformat_minor?: number;
}

function getCellSource(cell: NbCell): string {
  const s = cell.source;
  if (typeof s === "string") return s;
  if (Array.isArray(s)) return s.join("");
  return "";
}

function setCellSource(cell: NbCell, text: string) {
  cell.source = text;
}

function serializeNotebook(nb: NotebookDoc): string {
  return `${JSON.stringify(nb, null, 2)}\n`;
}

interface ExecuteResult {
  stdout: string;
  stderr: string;
  exit_code: number;
}

function kernelLanguage(nb: NotebookDoc): string {
  const ks = nb.metadata?.kernelspec as Record<string, unknown> | undefined;
  const lang = ks?.language;
  if (typeof lang === "string") return lang.trim().toLowerCase();
  return "python";
}

function monacoLanguageForKernel(lang: string): string {
  if (lang === "r" || lang === "ir") return "r";
  return "python";
}

function nextGlobalExecutionCount(nb: NotebookDoc): number {
  let m = 0;
  for (const c of nb.cells) {
    if (c.cell_type === "code" && typeof c.execution_count === "number" && !Number.isNaN(c.execution_count)) {
      m = Math.max(m, c.execution_count);
    }
  }
  return m + 1;
}

function buildOutputsFromRun(res: ExecuteResult): unknown[] {
  const outs: unknown[] = [];
  if (res.stdout) {
    outs.push({ output_type: "stream", name: "stdout", text: res.stdout });
  }
  if (res.stderr) {
    outs.push({ output_type: "stream", name: "stderr", text: res.stderr });
  }
  if (res.exit_code !== 0) {
    outs.push({
      output_type: "error",
      ename: "ExitCode",
      evalue: `Process exited with code ${res.exit_code}`,
      traceback: [],
    });
  }
  return outs;
}

function streamTextFromOutput(o: Record<string, unknown>): string {
  const t = o.text;
  if (typeof t === "string") return t;
  if (Array.isArray(t)) return t.join("");
  return "";
}

function textPlainFromData(data: Record<string, unknown>): string {
  const v = data["text/plain"];
  if (typeof v === "string") return v;
  if (Array.isArray(v)) return v.join("");
  return "";
}

function textHtmlFromData(data: Record<string, unknown>): string {
  const v = data["text/html"];
  if (typeof v === "string") return v;
  if (Array.isArray(v)) return v.join("");
  return "";
}

function jsonPrettyFromData(data: Record<string, unknown>): string | null {
  const j = data["application/json"];
  if (j === undefined) return null;
  if (typeof j === "string") {
    try {
      return JSON.stringify(JSON.parse(j), null, 2);
    } catch {
      return j;
    }
  }
  try {
    return JSON.stringify(j, null, 2);
  } catch {
    return String(j);
  }
}

function invokeLanguage(lang: string): string {
  return lang === "r" || lang === "ir" ? "r" : "python";
}

function findNextCodeIndex(cells: NbCell[], from: number): number {
  for (let i = from + 1; i < cells.length; i++) {
    if (cells[i].cell_type === "code") return i;
  }
  return -1;
}

function OutputBlock({ output }: { output: Record<string, unknown> }) {
  const theme = useTheme();
  const htmlSandboxAllowScripts = useNotebookViewerStore((s) => s.htmlSandboxAllowScripts);
  const ot = output.output_type;
  if (ot === "stream") {
    const name = output.name === "stderr" ? "stderr" : "stdout";
    const isErr = name === "stderr";
    return (
      <Box
        component="pre"
        sx={{
          m: 0,
          mt: 0.5,
          p: 1,
          bgcolor: isErr ? alpha(theme.palette.error.main, 0.12) : "action.hover",
          color: isErr ? "error.main" : "text.primary",
          borderRadius: 0.5,
          fontSize: 11,
          whiteSpace: "pre-wrap",
          wordBreak: "break-word",
          fontFamily: "JetBrains Mono, Monaco, Consolas, monospace",
          border: 1,
          borderColor: isErr ? "error.dark" : "divider",
        }}
      >
        {streamTextFromOutput(output)}
      </Box>
    );
  }
  if (ot === "error") {
    const ename = String(output.ename ?? "Error");
    const evalue = String(output.evalue ?? "");
    const tb = output.traceback;
    const tbStr = Array.isArray(tb) ? tb.join("\n") : typeof tb === "string" ? tb : "";
    return (
      <Box sx={{ mt: 0.5 }}>
        <Typography variant="caption" color="error" component="div" sx={{ fontWeight: 600 }}>
          {ename}: {evalue}
        </Typography>
        {tbStr ? (
          <Box
            component="pre"
            sx={{
              m: 0,
              mt: 0.5,
              p: 1,
              fontSize: 10,
              bgcolor: alpha(theme.palette.error.main, 0.08),
              borderRadius: 0.5,
              overflow: "auto",
              maxHeight: 200,
              whiteSpace: "pre-wrap",
            }}
          >
            {tbStr}
          </Box>
        ) : null}
      </Box>
    );
  }
  if (ot === "display_data" || ot === "execute_result") {
    const data = output.data as Record<string, unknown> | undefined;
    if (!data) {
      return (
        <Typography variant="caption" color="text.secondary">
          [空 display_data]
        </Typography>
      );
    }

    const png = data["image/png"];
    if (typeof png === "string" && png.length > 0) {
      return (
        <Box sx={{ mt: 0.5, maxWidth: "100%" }}>
          <Box
            component="img"
            src={`data:image/png;base64,${png}`}
            alt=""
            sx={{ maxWidth: "100%", height: "auto", borderRadius: 0.5 }}
          />
        </Box>
      );
    }

    const jpeg = data["image/jpeg"];
    if (typeof jpeg === "string" && jpeg.length > 0) {
      return (
        <Box sx={{ mt: 0.5, maxWidth: "100%" }}>
          <Box
            component="img"
            src={`data:image/jpeg;base64,${jpeg}`}
            alt=""
            sx={{ maxWidth: "100%", height: "auto", borderRadius: 0.5 }}
          />
        </Box>
      );
    }

    const svgRaw = data["image/svg+xml"];
    if (typeof svgRaw === "string" && svgRaw.length > 0) {
      const t = svgRaw.trim();
      if (t.startsWith("<")) {
        return (
          <Box
            sx={{ mt: 0.5, maxWidth: "100%", "& svg": { maxWidth: "100%", height: "auto" } }}
            dangerouslySetInnerHTML={{ __html: svgRaw }}
          />
        );
      }
      return (
        <Box sx={{ mt: 0.5, maxWidth: "100%" }}>
          <Box
            component="img"
            src={`data:image/svg+xml;base64,${svgRaw}`}
            alt=""
            sx={{ maxWidth: "100%", height: "auto", borderRadius: 0.5 }}
          />
        </Box>
      );
    }

    const html = textHtmlFromData(data);
    if (html.length > 0) {
      const iframeSandbox = htmlSandboxAllowScripts
        ? "allow-scripts allow-same-origin allow-downloads"
        : "allow-downloads allow-same-origin";
      return (
        <Box sx={{ mt: 0.5, width: "100%", maxWidth: "100%" }}>
          <Box
            component="iframe"
            title="HTML output"
            sandbox={iframeSandbox}
            srcDoc={html}
            sx={{
              width: "100%",
              minHeight: 120,
              maxHeight: 480,
              border: 1,
              borderColor: "divider",
              borderRadius: 0.5,
              bgcolor: "background.default",
            }}
          />
        </Box>
      );
    }

    const jsonStr = jsonPrettyFromData(data);
    if (jsonStr) {
      return (
        <Box
          component="pre"
          sx={{
            m: 0,
            mt: 0.5,
            p: 1,
            bgcolor: "action.hover",
            borderRadius: 0.5,
            fontSize: 11,
            whiteSpace: "pre-wrap",
            wordBreak: "break-word",
            fontFamily: "JetBrains Mono, Monaco, Consolas, monospace",
            maxHeight: 360,
            overflow: "auto",
          }}
        >
          {jsonStr}
        </Box>
      );
    }

    const md = data["text/markdown"];
    if (typeof md === "string" && md.length > 0) {
      return (
        <Box
          className="ipynb-md-preview"
          sx={{
            mt: 0.5,
            p: 1,
            bgcolor: "action.hover",
            borderRadius: 0.5,
            fontSize: 12,
            maxHeight: 400,
            overflow: "auto",
            "& pre": { overflow: "auto", p: 1, bgcolor: "background.paper", borderRadius: 1 },
          }}
        >
          <ReactMarkdown remarkPlugins={[remarkGfm]}>{md}</ReactMarkdown>
        </Box>
      );
    }

    const plain = textPlainFromData(data);
    if (plain) {
      return (
        <Box
          component="pre"
          sx={{
            m: 0,
            mt: 0.5,
            p: 1,
            bgcolor: "action.hover",
            borderRadius: 0.5,
            fontSize: 11,
            whiteSpace: "pre-wrap",
            wordBreak: "break-word",
            fontFamily: "JetBrains Mono, Monaco, Consolas, monospace",
          }}
        >
          {plain}
        </Box>
      );
    }

    return (
      <Typography variant="caption" color="text.secondary" sx={{ mt: 0.5 }}>
        [display_data — 无可识别 MIME]
      </Typography>
    );
  }
  return (
    <Typography variant="caption" color="text.secondary" sx={{ mt: 0.5 }}>
      [{String(ot)}]
    </Typography>
  );
}

interface NotebookCellBodyProps {
  index: number;
  nb: NotebookDoc;
  kernelLang: string;
  runningIdx: number | null;
  runningAll: boolean;
  updateCellSource: (index: number, text: string) => void;
  insertCell: (index: number, type: "code" | "markdown", position: "before" | "after") => void;
  runCell: (index: number) => void;
  clearOneOutput: (index: number) => void;
  deleteCell: (index: number) => void;
  attachCodeEditorKeys: (
    index: number,
    editorInst: editor.IStandaloneCodeEditor,
    monaco: typeof import("monaco-editor"),
  ) => () => void;
}

function NotebookCellBody({
  index,
  nb,
  kernelLang,
  runningIdx,
  runningAll,
  updateCellSource,
  insertCell,
  runCell,
  clearOneOutput,
  deleteCell,
  attachCodeEditorKeys,
}: NotebookCellBodyProps) {
  const theme = useTheme();
  const cell = nb.cells[index];
  const isMd = cell.cell_type === "markdown";
  const isCode = cell.cell_type === "code";
  const isRaw = cell.cell_type === "raw";
  const source = getCellSource(cell);
  return (
    <Box
      sx={{
        border: 1,
        borderColor: "divider",
        borderRadius: 1,
        overflow: "hidden",
        bgcolor: "background.paper",
      }}
    >
      <Stack
        direction="row"
        alignItems="center"
        justifyContent="space-between"
        flexWrap="wrap"
        gap={0.5}
        sx={{
          px: 1,
          py: 0.5,
          bgcolor: "action.hover",
          borderBottom: 1,
          borderColor: "divider",
        }}
      >
        <Typography variant="caption" fontWeight={600} color="text.secondary">
          {isCode ? "Code" : isMd ? "Markdown" : isRaw ? "Raw" : cell.cell_type} · #{index}
          {isCode && cell.execution_count != null && (
            <span> · In [{cell.execution_count}]</span>
          )}
        </Typography>
        <Stack direction="row" alignItems="center" spacing={0.25}>
          <Tooltip title="在上方插入代码单元">
            <IconButton size="small" onClick={() => insertCell(index, "code", "before")} aria-label="insert code before">
              <VerticalAlignTopIcon fontSize="small" />
            </IconButton>
          </Tooltip>
          <Tooltip title="在下方插入 Markdown 单元">
            <IconButton size="small" onClick={() => insertCell(index, "markdown", "after")} aria-label="insert md after">
              <VerticalAlignBottomIcon fontSize="small" />
            </IconButton>
          </Tooltip>
          {isCode && (
            <>
              <Tooltip title="运行此单元（Shift+Enter 运行并下一格）">
                <span>
                  <Button
                    size="small"
                    variant="outlined"
                    disabled={runningIdx !== null || runningAll}
                    onClick={() => void runCell(index)}
                    startIcon={
                      runningIdx === index ? (
                        <CircularProgress size={14} />
                      ) : (
                        <PlayArrowRoundedIcon sx={{ fontSize: 18 }} />
                      )
                    }
                    sx={{ textTransform: "none", fontSize: 11, minHeight: 28 }}
                  >
                    运行
                  </Button>
                </span>
              </Tooltip>
              <Tooltip title="清除此单元输出">
                <IconButton size="small" onClick={() => clearOneOutput(index)} aria-label="clear output">
                  <ClearAllIcon fontSize="small" />
                </IconButton>
              </Tooltip>
            </>
          )}
          <Tooltip title="删除单元">
            <IconButton size="small" color="error" onClick={() => deleteCell(index)} aria-label="delete cell">
              <DeleteOutlineIcon fontSize="small" />
            </IconButton>
          </Tooltip>
        </Stack>
      </Stack>

      <Box sx={{ p: isMd ? 1.5 : 0 }}>
        {isMd && (
          <Box sx={{ mb: 1 }}>
            <Typography variant="caption" color="text.secondary" sx={{ mb: 0.5, display: "block" }}>
              预览
            </Typography>
            <Box
              className="ipynb-md-preview"
              sx={{
                fontSize: 13,
                "& pre": { overflow: "auto", p: 1, bgcolor: "action.hover", borderRadius: 1 },
                "& code": { fontFamily: "JetBrains Mono, monospace", fontSize: 12 },
              }}
            >
              <ReactMarkdown remarkPlugins={[remarkGfm]}>{source || " "}</ReactMarkdown>
            </Box>
          </Box>
        )}
        {isMd && (
          <Box sx={{ minHeight: 120 }}>
            <Typography variant="caption" color="text.secondary" sx={{ mb: 0.5, display: "block" }}>
              编辑
            </Typography>
            <Editor
              height="140px"
              language="markdown"
              theme={theme.palette.mode === "dark" ? "vs-dark" : "vs"}
              value={source}
              onChange={(v) => updateCellSource(index, v ?? "")}
              options={{
                minimap: { enabled: false },
                fontSize: 12,
                scrollBeyondLastLine: false,
                wordWrap: "on",
                automaticLayout: true,
              }}
            />
          </Box>
        )}
        {isCode && (
          <Editor
            height="180px"
            language={monacoLanguageForKernel(kernelLang)}
            theme={theme.palette.mode === "dark" ? "vs-dark" : "vs"}
            value={source}
            onChange={(v) => updateCellSource(index, v ?? "")}
            onMount={(ed, monaco) => {
              const cleanup = attachCodeEditorKeys(index, ed, monaco);
              return cleanup;
            }}
            options={{
              minimap: { enabled: false },
              fontSize: 12,
              scrollBeyondLastLine: false,
              wordWrap: "off",
              automaticLayout: true,
            }}
          />
        )}
        {isRaw && (
          <Box sx={{ p: 1.5 }}>
            <Typography variant="caption" color="text.secondary" sx={{ mb: 1, display: "block" }}>
              Raw（纯文本）
            </Typography>
            <Editor
              height="100px"
              language="plaintext"
              theme={theme.palette.mode === "dark" ? "vs-dark" : "vs"}
              value={source}
              onChange={(v) => updateCellSource(index, v ?? "")}
              options={{
                minimap: { enabled: false },
                fontSize: 12,
                automaticLayout: true,
              }}
            />
          </Box>
        )}
        {!isMd && !isCode && !isRaw && (
          <Editor
            height="120px"
            language="plaintext"
            theme={theme.palette.mode === "dark" ? "vs-dark" : "vs"}
            value={source}
            onChange={(v) => updateCellSource(index, v ?? "")}
            options={{
              minimap: { enabled: false },
              fontSize: 12,
              automaticLayout: true,
            }}
          />
        )}
      </Box>

      {isCode && cell.outputs && cell.outputs.length > 0 && (
        <Box sx={{ px: 1, pb: 1, pt: 0 }}>
          <Typography variant="caption" color="text.secondary" sx={{ mb: 0.5, display: "block" }}>
            输出
          </Typography>
          {cell.outputs.map((out, oi) => (
            <OutputBlock key={oi} output={out as Record<string, unknown>} />
          ))}
        </Box>
      )}
    </Box>
  );
}

const ESTIMATE_CELL_H = 320;

export function IpynbViewer({ filePath, content, onChange }: IpynbViewerProps) {
  const [runningIdx, setRunningIdx] = useState<number | null>(null);
  const [runningAll, setRunningAll] = useState(false);
  const [runError, setRunError] = useState<string | null>(null);

  const virtualizeCells = useNotebookViewerStore((s) => s.virtualizeCells);
  const enableNotebookShortcuts = useNotebookViewerStore((s) => s.enableNotebookShortcuts);
  const enablePythonShellMagicHint = useNotebookViewerStore((s) => s.enablePythonShellMagic);
  const setSettingsTabIndex = useUiStore((s) => s.setSettingsTabIndex);
  const setSettingsOpen = useUiStore((s) => s.setSettingsOpen);
  const setRightPanelMode = useUiStore((s) => s.setRightPanelMode);

  const parsed = useMemo(() => {
    try {
      const nb = JSON.parse(content) as NotebookDoc;
      if (!nb.cells || !Array.isArray(nb.cells)) {
        return { ok: false as const, error: "无效的 .ipynb：缺少 cells 数组" };
      }
      return { ok: true as const, nb };
    } catch {
      return { ok: false as const, error: "无法解析 JSON" };
    }
  }, [content]);

  const pushNotebook = useCallback(
    (nb: NotebookDoc) => {
      onChange?.(serializeNotebook(nb));
    },
    [onChange],
  );

  const cloneNb = useCallback((nb: NotebookDoc) => JSON.parse(JSON.stringify(nb)) as NotebookDoc, []);

  const kernelLang = parsed.ok ? kernelLanguage(parsed.nb) : "python";
  const langArg = invokeLanguage(kernelLang);

  const nbRef = useRef(parsed.ok ? parsed.nb : null);
  if (parsed.ok) nbRef.current = parsed.nb;

  const scrollParentRef = useRef<HTMLDivElement | null>(null);
  const codeEditorRefs = useRef<Map<number, editor.IStandaloneCodeEditor>>(new Map());
  const virtualizerRef = useRef<ReturnType<typeof useVirtualizer<HTMLDivElement, Element>> | null>(null);

  const rowVirtualizer = useVirtualizer({
    count: parsed.ok && virtualizeCells ? parsed.nb.cells.length : 0,
    getScrollElement: () => scrollParentRef.current,
    estimateSize: () => ESTIMATE_CELL_H,
    overscan: 4,
    measureElement:
      typeof window !== "undefined" && typeof document !== "undefined"
        ? (el) => el.getBoundingClientRect().height
        : undefined,
  });
  virtualizerRef.current = rowVirtualizer;

  const updateCellSource = useCallback(
    (index: number, text: string) => {
      if (!parsed.ok) return;
      const nb = cloneNb(parsed.nb);
      const cell = nb.cells[index];
      if (!cell) return;
      setCellSource(cell, text);
      pushNotebook(nb);
    },
    [parsed, cloneNb, pushNotebook],
  );

  const runCell = useCallback(
    async (index: number) => {
      if (!parsed.ok) return;
      const cell = parsed.nb.cells[index];
      if (!cell || cell.cell_type !== "code") return;
      setRunError(null);
      setRunningIdx(index);
      try {
        const nb = cloneNb(parsed.nb);
        const c = nb.cells[index];
        if (!c || c.cell_type !== "code") return;
        const res = await invoke<ExecuteResult>("execute_ipynb_cell", {
          notebookPath: filePath,
          cellIndex: index,
          source: getCellSource(c),
          language: langArg,
          shellMagic: useNotebookViewerStore.getState().enablePythonShellMagic,
        });
        c.outputs = buildOutputsFromRun(res);
        c.execution_count = nextGlobalExecutionCount(nb);
        pushNotebook(nb);
      } catch (e) {
        setRunError(String(e));
      } finally {
        setRunningIdx(null);
      }
    },
    [parsed, cloneNb, pushNotebook, filePath, langArg],
  );

  const runCellRef = useRef(runCell);
  runCellRef.current = runCell;

  const runAll = useCallback(async () => {
    if (!parsed.ok) return;
    setRunError(null);
    setRunningAll(true);
    try {
      const nb = cloneNb(parsed.nb);
      let seq = 1;
      for (let i = 0; i < nb.cells.length; i++) {
        if (nb.cells[i].cell_type !== "code") continue;
        setRunningIdx(i);
        const c = nb.cells[i];
        const res = await invoke<ExecuteResult>("execute_ipynb_cell", {
          notebookPath: filePath,
          cellIndex: i,
          source: getCellSource(c),
          language: langArg,
          shellMagic: useNotebookViewerStore.getState().enablePythonShellMagic,
        });
        c.outputs = buildOutputsFromRun(res);
        c.execution_count = seq;
        seq += 1;
        pushNotebook(cloneNb(nb));
      }
    } catch (e) {
      setRunError(String(e));
    } finally {
      setRunningIdx(null);
      setRunningAll(false);
    }
  }, [parsed, cloneNb, pushNotebook, filePath, langArg]);

  const clearAllOutputs = useCallback(() => {
    if (!parsed.ok) return;
    const nb = cloneNb(parsed.nb);
    for (const c of nb.cells) {
      if (c.cell_type === "code") {
        c.outputs = [];
        c.execution_count = null;
      }
    }
    pushNotebook(nb);
  }, [parsed, cloneNb, pushNotebook]);

  const clearOneOutput = useCallback(
    (index: number) => {
      if (!parsed.ok) return;
      const nb = cloneNb(parsed.nb);
      const c = nb.cells[index];
      if (c?.cell_type === "code") {
        c.outputs = [];
        c.execution_count = null;
      }
      pushNotebook(nb);
    },
    [parsed, cloneNb, pushNotebook],
  );

  const deleteCell = useCallback(
    (index: number) => {
      if (!parsed.ok) return;
      const nb = cloneNb(parsed.nb);
      nb.cells.splice(index, 1);
      pushNotebook(nb);
    },
    [parsed, cloneNb, pushNotebook],
  );

  const insertCell = useCallback(
    (index: number, type: "code" | "markdown", position: "before" | "after") => {
      if (!parsed.ok) return;
      const nb = cloneNb(parsed.nb);
      const newCell: NbCell =
        type === "code"
          ? {
              cell_type: "code",
              execution_count: null,
              metadata: {},
              outputs: [],
              source: "",
            }
          : {
              cell_type: "markdown",
              metadata: {},
              source: "",
            };
      const at = position === "before" ? index : index + 1;
      nb.cells.splice(at, 0, newCell);
      pushNotebook(nb);
    },
    [parsed, cloneNb, pushNotebook],
  );

  const addCellAtEnd = useCallback(
    (type: "code" | "markdown") => {
      if (!parsed.ok) return;
      const nb = cloneNb(parsed.nb);
      const newCell: NbCell =
        type === "code"
          ? {
              cell_type: "code",
              execution_count: null,
              metadata: {},
              outputs: [],
              source: "",
            }
          : {
              cell_type: "markdown",
              metadata: {},
              source: "",
            };
      nb.cells.push(newCell);
      pushNotebook(nb);
    },
    [parsed, cloneNb, pushNotebook],
  );

  const runningIdxRef = useRef(runningIdx);
  const runningAllRef = useRef(runningAll);
  runningIdxRef.current = runningIdx;
  runningAllRef.current = runningAll;

  const attachCodeEditorKeys = useCallback(
    (index: number, editorInst: editor.IStandaloneCodeEditor, monaco: typeof import("monaco-editor")) => {
      codeEditorRefs.current.set(index, editorInst);
      const runOnly = () => {
        if (runningIdxRef.current !== null || runningAllRef.current) return;
        void runCellRef.current(index);
      };
      const runAndNext = () => {
        if (runningIdxRef.current !== null || runningAllRef.current) return;
        void runCellRef.current(index).then(() => {
          const cells = nbRef.current?.cells;
          if (!cells) return;
          const next = findNextCodeIndex(cells, index);
          if (next < 0) return;
          virtualizerRef.current?.scrollToIndex(next, { align: "start" });
          window.setTimeout(() => {
            codeEditorRefs.current.get(next)?.focus();
          }, 80);
        });
      };
      const sub = editorInst.onKeyDown((e) => {
        if (!useNotebookViewerStore.getState().enableNotebookShortcuts) return;
        if (e.keyCode !== monaco.KeyCode.Enter) return;
        if (e.shiftKey) {
          e.preventDefault();
          e.stopPropagation();
          runAndNext();
          return;
        }
        if (e.ctrlKey || e.metaKey) {
          e.preventDefault();
          e.stopPropagation();
          runOnly();
        }
      });
      return () => {
        sub.dispose();
        codeEditorRefs.current.delete(index);
      };
    },
    [],
  );

  if (!parsed.ok) {
    return (
      <Box sx={{ p: 2 }}>
        <Typography color="error" variant="body2">
          {parsed.error}
        </Typography>
      </Box>
    );
  }

  const { nb } = parsed;
  const ks = nb.metadata?.kernelspec as Record<string, unknown> | undefined;
  const kernelName = typeof ks?.name === "string" ? ks.name : kernelLang;

  const virtualItems = rowVirtualizer.getVirtualItems();
  const totalVirtH = rowVirtualizer.getTotalSize();

  return (
    <Box
      sx={{
        flex: 1,
        minHeight: 0,
        overflow: "hidden",
        display: "flex",
        flexDirection: "column",
      }}
    >
      <Stack
        direction="row"
        alignItems="center"
        flexWrap="wrap"
        gap={0.75}
        sx={{
          px: 1.5,
          py: 1,
          borderBottom: 1,
          borderColor: "divider",
          bgcolor: "action.hover",
        }}
      >
        <Chip size="small" label={`Kernel: ${kernelName}`} variant="outlined" />
        <Chip size="small" label={`Lang: ${kernelLang}`} variant="outlined" />
        <Typography variant="caption" color="text.secondary" sx={{ display: { xs: "none", md: "block" } }}>
          {enableNotebookShortcuts ? "Shift+Enter / Ctrl+Enter · " : "快捷键已关 · "}
          {enablePythonShellMagicHint ? "Python「!」shell · " : "「!」魔法已关 · "}
          Settings → Notebook
        </Typography>
        <Divider orientation="vertical" flexItem sx={{ mx: 0.5 }} />
        <Tooltip title="从上到下依次运行所有代码单元（In [1]… 顺序编号）">
          <span>
            <Button
              size="small"
              variant="contained"
              disabled={runningIdx !== null || runningAll}
              onClick={() => void runAll()}
              startIcon={runningAll ? <CircularProgress size={14} color="inherit" /> : <PlayCircleOutlineIcon />}
              sx={{ textTransform: "none", fontSize: 12 }}
            >
              全部运行
            </Button>
          </span>
        </Tooltip>
        <Tooltip title="清空所有代码单元的输出">
          <Button
            size="small"
            variant="outlined"
            disabled={runningIdx !== null || runningAll}
            onClick={clearAllOutputs}
            startIcon={<ClearAllIcon />}
            sx={{ textTransform: "none", fontSize: 12 }}
          >
            清空输出
          </Button>
        </Tooltip>
        <Divider orientation="vertical" flexItem sx={{ mx: 0.5 }} />
        <Button
          size="small"
          variant="outlined"
          onClick={() => addCellAtEnd("code")}
          startIcon={<AddIcon />}
          sx={{ textTransform: "none", fontSize: 12 }}
        >
          代码单元
        </Button>
        <Button
          size="small"
          variant="outlined"
          onClick={() => addCellAtEnd("markdown")}
          startIcon={<AddIcon />}
          sx={{ textTransform: "none", fontSize: 12 }}
        >
          Markdown
        </Button>
        <Tooltip title="打开 Settings → Notebook，调整虚拟滚动、HTML 沙箱、快捷键与 ! shell">
          <Button
            size="small"
            variant="text"
            onClick={() => {
              setSettingsTabIndex(7);
              setSettingsOpen(true);
              setRightPanelMode("settings");
            }}
            startIcon={<SettingsIcon />}
            sx={{ textTransform: "none", fontSize: 12 }}
          >
            Notebook 设置
          </Button>
        </Tooltip>
      </Stack>

      <Box ref={scrollParentRef} sx={{ flex: 1, minHeight: 0, overflow: "auto", py: 1, px: 1.5 }}>
        {runError && (
          <Typography color="error" variant="caption" sx={{ display: "block", mb: 1 }}>
            {runError}
          </Typography>
        )}
        {virtualizeCells ? (
          <Box sx={{ position: "relative", width: "100%", height: totalVirtH }}>
            {virtualItems.map((virtualRow) => {
              const index = virtualRow.index;
              const c = nb.cells[index];
              const cellId =
                typeof c.metadata?.id === "string" ? c.metadata.id : `cell-${index}`;
              return (
                <Box
                  key={cellId}
                  data-index={virtualRow.index}
                  ref={rowVirtualizer.measureElement}
                  sx={{
                    position: "absolute",
                    top: 0,
                    left: 0,
                    width: "100%",
                    transform: `translateY(${virtualRow.start}px)`,
                    pb: 2,
                  }}
                >
                  <NotebookCellBody
                    index={index}
                    nb={nb}
                    kernelLang={kernelLang}
                    runningIdx={runningIdx}
                    runningAll={runningAll}
                    updateCellSource={updateCellSource}
                    insertCell={insertCell}
                    runCell={runCell}
                    clearOneOutput={clearOneOutput}
                    deleteCell={deleteCell}
                    attachCodeEditorKeys={attachCodeEditorKeys}
                  />
                </Box>
              );
            })}
          </Box>
        ) : (
          <Stack spacing={2}>
            {nb.cells.map((_, index) => {
              const c = nb.cells[index];
              const cellId =
                typeof c.metadata?.id === "string" ? c.metadata.id : `cell-${index}`;
              return (
                <Box key={cellId}>
                  <NotebookCellBody
                    index={index}
                    nb={nb}
                    kernelLang={kernelLang}
                    runningIdx={runningIdx}
                    runningAll={runningAll}
                    updateCellSource={updateCellSource}
                    insertCell={insertCell}
                    runCell={runCell}
                    clearOneOutput={clearOneOutput}
                    deleteCell={deleteCell}
                    attachCodeEditorKeys={attachCodeEditorKeys}
                  />
                </Box>
              );
            })}
          </Stack>
        )}
      </Box>
    </Box>
  );
}
