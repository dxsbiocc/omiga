export type NotebookCellType = "code" | "markdown" | "raw" | string;

export interface NotebookOutput {
  output_type?: string;
  name?: string;
  text?: string | string[];
  ename?: string;
  evalue?: string;
  traceback?: string | string[];
  data?: Record<string, unknown>;
  metadata?: Record<string, unknown>;
  execution_count?: number | null;
  [key: string]: unknown;
}

export interface NotebookCell {
  id?: string;
  cell_type: NotebookCellType;
  source: string | string[];
  metadata?: Record<string, unknown>;
  outputs?: NotebookOutput[];
  execution_count?: number | null;
  [key: string]: unknown;
}

export interface NotebookDocument {
  cells: NotebookCell[];
  metadata?: Record<string, unknown>;
  nbformat?: number;
  nbformat_minor?: number;
  [key: string]: unknown;
}

export interface ExecuteResult {
  stdout: string;
  stderr: string;
  exit_code: number;
}

export type NotebookParseResult =
  | {
      ok: true;
      nb: NotebookDocument;
      initialized: boolean;
      warnings: string[];
    }
  | {
      ok: false;
      error: string;
    };

export type NotebookRenderableOutput =
  | { kind: "stream"; name: "stdout" | "stderr"; text: string }
  | { kind: "error"; ename: string; evalue: string; traceback: string }
  | { kind: "image"; mime: "image/png" | "image/jpeg" | "image/svg+xml"; src: string }
  | { kind: "html"; html: string }
  | { kind: "json"; text: string }
  | { kind: "markdown"; markdown: string }
  | { kind: "text"; text: string }
  | { kind: "widget"; text: string }
  | { kind: "unknown"; outputType: string };

export const OMIGA_NOTEBOOK_PLUGIN = {
  id: "omiga.notebook.jupyter",
  notebookType: "jupyter-notebook",
  displayName: "Jupyter Notebook",
  filenamePattern: "*.ipynb",
  rendererPolicy:
    "Render by MIME priority in an isolated React/iframe surface; do not execute VS Code extension webviews.",
  controllerPolicy:
    "Execute local Python/R cells through Omiga's Tauri notebook command; kernel discovery is metadata-guided.",
} as const;

export const NOTEBOOK_OUTPUT_MIME_PRIORITY = [
  "application/vnd.jupyter.widget-view+json",
  "image/png",
  "image/jpeg",
  "image/svg+xml",
  "text/html",
  "application/json",
  "text/markdown",
  "text/plain",
] as const;

const DEFAULT_NOTEBOOK_LANGUAGE = "python";

export interface NotebookKernelOption {
  language: "python" | "r";
  label: string;
  kernelName: string;
}

export const NOTEBOOK_EXECUTABLE_KERNEL_OPTIONS: NotebookKernelOption[] = [
  { language: "python", label: "Python 3", kernelName: "python3" },
  { language: "r", label: "R", kernelName: "ir" },
];

export function createEmptyNotebook(language = DEFAULT_NOTEBOOK_LANGUAGE): NotebookDocument {
  const normalizedLanguage = normalizeKernelLanguage(language);
  return {
    cells: [],
    metadata: {
      kernelspec: {
        display_name: kernelDisplayName(normalizedLanguage),
        language: normalizedLanguage,
        name: normalizedLanguage === "r" ? "ir" : "python3",
      },
      language_info: {
        name: normalizedLanguage,
      },
    },
    nbformat: 4,
    nbformat_minor: 5,
  };
}

export function createNotebookCell(
  cellType: "code" | "markdown" | "raw",
  source = "",
): NotebookCell {
  if (cellType === "code") {
    return {
      id: createNotebookCellId(),
      cell_type: "code",
      execution_count: null,
      metadata: {},
      outputs: [],
      source,
    };
  }
  return {
    id: createNotebookCellId(),
    cell_type: cellType,
    metadata: {},
    source,
  };
}

export function parseNotebookContent(content: string): NotebookParseResult {
  if (content.trim().length === 0) {
    return {
      ok: true,
      nb: createEmptyNotebook(),
      initialized: true,
      warnings: [],
    };
  }

  let raw: unknown;
  try {
    raw = JSON.parse(content);
  } catch {
    return {
      ok: false,
      error: "无法解析 JSON",
    };
  }

  if (!isRecord(raw) || !Array.isArray(raw.cells)) {
    return {
      ok: false,
      error: "无效的 .ipynb：缺少 cells 数组",
    };
  }

  const warnings: string[] = [];
  const nb = normalizeNotebook(raw as NotebookDocument, warnings);
  return { ok: true, nb, initialized: false, warnings };
}

export function serializeNotebook(nb: NotebookDocument): string {
  return `${JSON.stringify(normalizeNotebook(nb), null, 2)}\n`;
}

export function cloneNotebook(nb: NotebookDocument): NotebookDocument {
  return JSON.parse(JSON.stringify(nb)) as NotebookDocument;
}

export function getCellSource(cell: NotebookCell): string {
  return textFromMultilineValue(cell.source);
}

export function setCellSource(cell: NotebookCell, text: string): void {
  cell.source = text;
}

export function setNotebookCellType(
  cell: NotebookCell,
  cellType: "code" | "markdown" | "raw",
): void {
  cell.cell_type = cellType;
  cell.source = getCellSource(cell);
  cell.metadata = isRecord(cell.metadata) ? cell.metadata : {};
  if (cellType === "code") {
    cell.outputs = Array.isArray(cell.outputs) ? cell.outputs : [];
    cell.execution_count =
      typeof cell.execution_count === "number" ? cell.execution_count : null;
    return;
  }
  delete cell.outputs;
  delete cell.execution_count;
}

export function notebookKernelLanguage(nb: NotebookDocument): string {
  const metadata = asRecord(nb.metadata);
  const languageInfo = asRecord(metadata?.language_info);
  const kernelSpec = asRecord(metadata?.kernelspec);
  const fromLanguageInfo = asTrimmedString(languageInfo?.name);
  if (fromLanguageInfo) return normalizeKernelLanguage(fromLanguageInfo);
  const fromKernelLanguage = asTrimmedString(kernelSpec?.language);
  if (fromKernelLanguage) return normalizeKernelLanguage(fromKernelLanguage);
  const firstCodeCellLanguage = nb.cells
    .filter((cell) => cell.cell_type === "code")
    .map((cell) => asTrimmedString(asRecord(cell.metadata)?.language))
    .find(Boolean);
  return normalizeKernelLanguage(firstCodeCellLanguage ?? DEFAULT_NOTEBOOK_LANGUAGE);
}

export function notebookKernelName(nb: NotebookDocument): string {
  const kernelSpec = asRecord(asRecord(nb.metadata)?.kernelspec);
  return (
    asTrimmedString(kernelSpec?.display_name) ??
    asTrimmedString(kernelSpec?.name) ??
    notebookKernelLanguage(nb)
  );
}

export function monacoLanguageForNotebook(lang: string): string {
  const normalized = normalizeKernelLanguage(lang);
  switch (normalized) {
    case "r":
      return "r";
    case "julia":
      return "julia";
    case "javascript":
      return "javascript";
    case "typescript":
      return "typescript";
    case "bash":
    case "shell":
      return "shell";
    case "powershell":
      return "powershell";
    case "sql":
      return "sql";
    case "csharp":
      return "csharp";
    case "fsharp":
      return "fsharp";
    default:
      return "python";
  }
}

export function executionLanguageForNotebook(lang: string): "python" | "r" {
  return normalizeKernelLanguage(lang) === "r" ? "r" : "python";
}

export function setNotebookKernelLanguage(nb: NotebookDocument, language: string): void {
  const normalizedLanguage = normalizeKernelLanguage(language);
  const metadata = isRecord(nb.metadata) ? { ...nb.metadata } : {};
  const kernelspec = isRecord(metadata.kernelspec) ? { ...metadata.kernelspec } : {};
  const languageInfo = isRecord(metadata.language_info) ? { ...metadata.language_info } : {};
  nb.metadata = {
    ...metadata,
    kernelspec: {
      ...kernelspec,
      display_name: kernelDisplayName(normalizedLanguage),
      language: normalizedLanguage,
      name: kernelNameForLanguage(normalizedLanguage),
    },
    language_info: {
      ...languageInfo,
      name: normalizedLanguage,
    },
  };
}

export function nextGlobalExecutionCount(nb: NotebookDocument): number {
  let max = 0;
  for (const cell of nb.cells) {
    if (
      cell.cell_type === "code" &&
      typeof cell.execution_count === "number" &&
      !Number.isNaN(cell.execution_count)
    ) {
      max = Math.max(max, cell.execution_count);
    }
  }
  return max + 1;
}

export function buildOutputsFromRun(result: ExecuteResult): NotebookOutput[] {
  const outputs: NotebookOutput[] = [];
  if (result.stdout) {
    outputs.push({ output_type: "stream", name: "stdout", text: result.stdout });
  }
  if (result.stderr) {
    outputs.push({ output_type: "stream", name: "stderr", text: result.stderr });
  }
  if (result.exit_code !== 0) {
    outputs.push({
      output_type: "error",
      ename: "ExitCode",
      evalue: `Process exited with code ${result.exit_code}`,
      traceback: [],
    });
  }
  return outputs;
}

export function findNextCodeIndex(cells: NotebookCell[], from: number): number {
  for (let index = from + 1; index < cells.length; index += 1) {
    if (cells[index].cell_type === "code") return index;
  }
  return -1;
}

export function buildNotebookExecutionPrelude(
  cells: NotebookCell[],
  beforeIndex: number,
): string {
  return cells
    .slice(0, Math.max(0, beforeIndex))
    .filter((cell) => cell.cell_type === "code")
    .map((cell) => getCellSource(cell).trimEnd())
    .filter((source) => source.trim().length > 0)
    .join("\n\n");
}

export function renderableNotebookOutput(output: NotebookOutput): NotebookRenderableOutput {
  const outputType = String(output.output_type ?? "");
  if (outputType === "stream") {
    const name = output.name === "stderr" ? "stderr" : "stdout";
    return { kind: "stream", name, text: textFromMultilineValue(output.text) };
  }

  if (outputType === "error") {
    return {
      kind: "error",
      ename: String(output.ename ?? "Error"),
      evalue: String(output.evalue ?? ""),
      traceback: textFromMultilineValue(output.traceback),
    };
  }

  if (outputType === "display_data" || outputType === "execute_result") {
    return renderableMimeOutput(asRecord(output.data));
  }

  return { kind: "unknown", outputType };
}

function renderableMimeOutput(data?: Record<string, unknown>): NotebookRenderableOutput {
  if (!data) return { kind: "unknown", outputType: "display_data" };
  for (const mime of NOTEBOOK_OUTPUT_MIME_PRIORITY) {
    if (!(mime in data)) continue;
    const value = data[mime];
    switch (mime) {
      case "application/vnd.jupyter.widget-view+json":
        return {
          kind: "widget",
          text: "Widget output requires a Jupyter widget renderer; Omiga keeps the notebook data but does not execute widget webviews.",
        };
      case "image/png":
      case "image/jpeg":
        if (typeof value === "string" && value.length > 0) {
          return { kind: "image", mime, src: `data:${mime};base64,${value}` };
        }
        break;
      case "image/svg+xml": {
        const svg = textFromMultilineValue(value).trim();
        if (svg.length > 0) {
          const src = svg.startsWith("<")
            ? `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`
            : `data:image/svg+xml;base64,${svg}`;
          return { kind: "image", mime, src };
        }
        break;
      }
      case "text/html": {
        const html = textFromMultilineValue(value);
        if (html.length > 0) return { kind: "html", html };
        break;
      }
      case "application/json": {
        const text = prettyJson(value);
        if (text) return { kind: "json", text };
        break;
      }
      case "text/markdown": {
        const markdown = textFromMultilineValue(value);
        if (markdown.length > 0) return { kind: "markdown", markdown };
        break;
      }
      case "text/plain": {
        const text = textFromMultilineValue(value);
        if (text.length > 0) return { kind: "text", text };
        break;
      }
    }
  }
  return { kind: "unknown", outputType: "display_data" };
}

function normalizeNotebook(
  input: NotebookDocument,
  warnings: string[] = [],
): NotebookDocument {
  const metadata = isRecord(input.metadata) ? input.metadata : {};
  const nbformat = typeof input.nbformat === "number" ? input.nbformat : 4;
  const nbformatMinor =
    typeof input.nbformat_minor === "number" ? input.nbformat_minor : 5;
  const cells = Array.isArray(input.cells)
    ? input.cells.map((cell, index) => normalizeNotebookCell(cell, index, warnings))
    : [];

  return {
    ...input,
    cells,
    metadata,
    nbformat,
    nbformat_minor: nbformatMinor,
  };
}

function normalizeNotebookCell(
  input: unknown,
  index: number,
  warnings: string[],
): NotebookCell {
  const raw = isRecord(input) ? input : {};
  if (!isRecord(input)) {
    warnings.push(`Cell ${index} was not an object and was converted to a raw cell.`);
  }
  const rawType = asTrimmedString(raw.cell_type) ?? "raw";
  const cellType: NotebookCellType =
    rawType === "code" || rawType === "markdown" || rawType === "raw"
      ? rawType
      : "raw";
  if (rawType !== cellType) {
    warnings.push(`Cell ${index} has unsupported type "${rawType}" and is shown as raw.`);
  }
  const metadata = isRecord(raw.metadata) ? raw.metadata : {};
  const source = textFromMultilineValue(raw.source);
  const id =
    asTrimmedString(raw.id) ??
    asTrimmedString(metadata.id) ??
    createNotebookCellId(index);

  if (cellType === "code") {
    return {
      ...raw,
      id,
      cell_type: "code",
      metadata,
      source,
      outputs: Array.isArray(raw.outputs) ? (raw.outputs as NotebookOutput[]) : [],
      execution_count:
        typeof raw.execution_count === "number" ? raw.execution_count : null,
    };
  }

  const { outputs: _outputs, execution_count: _executionCount, ...rest } = raw;
  return {
    ...rest,
    id,
    cell_type: cellType,
    metadata,
    source,
  };
}

function normalizeKernelLanguage(language: string): string {
  const normalized = language.trim().toLowerCase();
  switch (normalized) {
    case "ir":
      return "r";
    case "py":
    case "ipython":
    case "python3":
      return "python";
    case "js":
    case "node":
    case "nodejs":
      return "javascript";
    case "ts":
      return "typescript";
    case "sh":
    case "zsh":
    case "fish":
      return "shell";
    case "ps1":
      return "powershell";
    case "c#":
      return "csharp";
    case "f#":
      return "fsharp";
    default:
      return normalized || DEFAULT_NOTEBOOK_LANGUAGE;
  }
}

function kernelDisplayName(language: string): string {
  switch (language) {
    case "r":
      return "R";
    case "julia":
      return "Julia";
    default:
      return "Python 3";
  }
}

function kernelNameForLanguage(language: string): string {
  switch (language) {
    case "r":
      return "ir";
    case "python":
      return "python3";
    default:
      return language || DEFAULT_NOTEBOOK_LANGUAGE;
  }
}

function createNotebookCellId(index?: number): string {
  const random = globalThis.crypto?.randomUUID?.();
  if (random) return random.slice(0, 8);
  const suffix = index === undefined ? Math.random().toString(36).slice(2, 10) : `${index}`;
  return `omiga-${suffix}`;
}

function textFromMultilineValue(value: unknown): string {
  if (typeof value === "string") return value;
  if (Array.isArray(value)) return value.map((item) => String(item)).join("");
  return "";
}

function prettyJson(value: unknown): string | null {
  if (value === undefined) return null;
  if (typeof value === "string") {
    try {
      return JSON.stringify(JSON.parse(value), null, 2);
    } catch {
      return value;
    }
  }
  try {
    return JSON.stringify(value, null, 2);
  } catch {
    return String(value);
  }
}

function asRecord(value: unknown): Record<string, unknown> | undefined {
  return isRecord(value) ? value : undefined;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return Boolean(value) && typeof value === "object" && !Array.isArray(value);
}

function asTrimmedString(value: unknown): string | undefined {
  return typeof value === "string" && value.trim().length > 0
    ? value.trim()
    : undefined;
}
