import "../../lib/monacoWorkers";
import {
  useCallback,
  useEffect,
  memo,
  useMemo,
  useRef,
  useState,
  type ReactElement,
  type ReactNode,
} from "react";
import { useNotebookViewerStore } from "../../state";
import Editor from "@monaco-editor/react";
import type { editor } from "monaco-editor";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { useVirtualizer } from "@tanstack/react-virtual";
import { alpha } from "@mui/material/styles";
import {
  Alert,
  Box,
  CircularProgress,
  Divider,
  IconButton,
  Menu,
  MenuItem,
  Select,
  Stack,
  Tooltip,
  Typography,
  useTheme,
} from "@mui/material";
import PlayArrowRoundedIcon from "@mui/icons-material/PlayArrowRounded";
import DeleteOutlineIcon from "@mui/icons-material/DeleteOutline";
import ClearAllIcon from "@mui/icons-material/ClearAll";
import {
  nextCellTargetAfterRun,
  resolveNotebookEditorCommand,
  type NotebookEditorCommand,
  type NotebookEditorKey,
} from "../../lib/notebookEvents";
import {
  NotebookExecutionController,
  type NotebookExecutionControllerHost,
  type NotebookExecutionStatus,
} from "../../lib/notebookExecution";
import { createTauriNotebookRuntimeAdapter } from "../../lib/notebookRuntimeAdapter";
import {
  registerWorkspaceContentProvider,
  useWorkspaceStore,
} from "../../state/workspaceStore";
import {
  NOTEBOOK_EXECUTABLE_KERNEL_OPTIONS,
  OMIGA_NOTEBOOK_PLUGIN,
  createEmptyNotebook,
  createNotebookCell,
  executionLanguageForNotebook,
  getCellSource,
  monacoLanguageForNotebook,
  notebookKernelLanguage,
  notebookKernelName,
  parseNotebookContent,
  renderableNotebookOutput,
  serializeNotebook,
  setCellSource,
  setNotebookCellType,
  setNotebookKernelLanguage,
  type NotebookCell,
  type NotebookDocument,
  type NotebookOutput,
  type NotebookParseResult,
} from "../../lib/notebookPlugin";

interface IpynbViewerProps {
  filePath: string;
  content: string;
  onChange?: (value: string) => void;
}

interface NotebookToolbarButtonProps {
  title: string;
  icon: ReactNode;
  label: string;
  disabled?: boolean;
  onClick?: () => void;
}

function NotebookTooltip({
  title,
  children,
}: {
  title: ReactNode;
  children: ReactElement;
}) {
  const theme = useTheme();
  const isDark = theme.palette.mode === "dark";
  const tooltipBg = isDark
    ? alpha(theme.palette.background.paper, 0.98)
    : theme.palette.background.paper;

  return (
    <Tooltip
      title={title}
      arrow
      componentsProps={{
        tooltip: {
          sx: {
            bgcolor: tooltipBg,
            color: "text.primary",
            border: 1,
            borderColor: "divider",
            boxShadow: theme.shadows[4],
            fontSize: 12,
            fontWeight: 500,
            px: 1,
            py: 0.65,
          },
        },
        arrow: {
          sx: {
            color: tooltipBg,
            "&::before": {
              border: 1,
              borderColor: "divider",
            },
          },
        },
      }}
    >
      {children}
    </Tooltip>
  );
}

function NotebookToolbarButton({
  title,
  icon,
  label,
  disabled = false,
  onClick,
}: NotebookToolbarButtonProps) {
  return (
    <NotebookTooltip title={title}>
      <Box
        component="button"
        type="button"
        disabled={disabled}
        onClick={disabled ? undefined : onClick}
        sx={(theme) => ({
          appearance: "none",
          border: 0,
          bgcolor: "transparent",
          color: disabled ? "text.disabled" : "text.secondary",
          height: 32,
          px: 0.75,
          display: "inline-flex",
          alignItems: "center",
          gap: 0.7,
          flexShrink: 0,
          whiteSpace: "nowrap",
          font: "inherit",
          fontSize: 12,
          fontWeight: 500,
          cursor: disabled ? "not-allowed" : "pointer",
          opacity: disabled ? 0.58 : 1,
          borderRadius: 0.75,
          userSelect: "none",
	          "&:hover": disabled
	            ? {}
	            : {
	                bgcolor: alpha(
	                  theme.palette.primary.main,
	                  theme.palette.mode === "dark" ? 0.18 : 0.08,
	                ),
	                color: "primary.main",
	              },
          "&:focus-visible": {
            outline: `2px solid ${alpha(theme.palette.primary.main, 0.55)}`,
            outlineOffset: -2,
          },
        })}
      >
        <Box
          component="span"
          sx={{
            width: 16,
            minWidth: 16,
            display: "inline-flex",
            alignItems: "center",
            justifyContent: "center",
            lineHeight: 1,
          }}
        >
          {icon}
        </Box>
        <Box component="span" sx={{ display: "inline-block", lineHeight: "32px" }}>
          {label}
        </Box>
      </Box>
    </NotebookTooltip>
  );
}

function NotebookInsertButtons({
  onAddCode,
  onAddMarkdown,
}: {
  onAddCode: () => void;
  onAddMarkdown: () => void;
}) {
  const theme = useTheme();
  const isDark = theme.palette.mode === "dark";
  return (
    <Stack
      direction="row"
      alignItems="center"
      sx={{
        bgcolor: isDark
          ? alpha(theme.palette.background.paper, 0.96)
          : alpha(theme.palette.background.paper, 0.98),
        border: 1,
        borderColor: "divider",
        borderRadius: 1,
        boxShadow: theme.shadows[3],
        overflow: "hidden",
      }}
      onClick={(event) => event.stopPropagation()}
    >
      <Box
        component="button"
        type="button"
        onClick={onAddCode}
        sx={(buttonTheme) => ({
          appearance: "none",
          border: 0,
          bgcolor: "transparent",
          color: "text.primary",
          px: 1.25,
          height: 30,
          font: "inherit",
          fontSize: 12,
          fontWeight: 600,
	          cursor: "pointer",
	          "&:hover": {
	            bgcolor: alpha(
	              buttonTheme.palette.primary.main,
	              buttonTheme.palette.mode === "dark" ? 0.18 : 0.08,
	            ),
	            color: "primary.main",
	          },
        })}
      >
        Add Code
      </Box>
      <Divider orientation="vertical" flexItem />
      <Box
        component="button"
        type="button"
        onClick={onAddMarkdown}
        sx={(buttonTheme) => ({
          appearance: "none",
          border: 0,
          bgcolor: "transparent",
          color: "text.primary",
          px: 1.25,
          height: 30,
          font: "inherit",
          fontSize: 12,
          fontWeight: 600,
	          cursor: "pointer",
	          "&:hover": {
	            bgcolor: alpha(
	              buttonTheme.palette.primary.main,
	              buttonTheme.palette.mode === "dark" ? 0.18 : 0.08,
	            ),
	            color: "primary.main",
	          },
        })}
      >
        Add Markdown
      </Box>
    </Stack>
  );
}

function notebookEditorHeight(source: string, minHeight: number, maxHeight: number): number {
  const lines = Math.max(1, source.split(/\r\n|\r|\n/).length);
  return Math.min(maxHeight, Math.max(minHeight, lines * 20 + 14));
}

function cellLanguageLabel(cell: NotebookCell, kernelLang: string): string {
  if (cell.cell_type === "markdown") return "Markdown";
  if (cell.cell_type === "raw") return "Raw";
  return monacoLanguageForNotebook(kernelLang).replace(/^\w/, (char) => char.toUpperCase());
}

function OutputBlock({ output }: { output: NotebookOutput }) {
  const theme = useTheme();
  const htmlSandboxAllowScripts = useNotebookViewerStore((s) => s.htmlSandboxAllowScripts);
  const renderable = renderableNotebookOutput(output);
  if (renderable.kind === "stream") {
    const isErr = renderable.name === "stderr";
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
        {renderable.text}
      </Box>
    );
  }
  if (renderable.kind === "error") {
    return (
      <Box sx={{ mt: 0.5 }}>
        <Typography variant="caption" color="error" component="div" sx={{ fontWeight: 600 }}>
          {renderable.ename}: {renderable.evalue}
        </Typography>
        {renderable.traceback ? (
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
            {renderable.traceback}
          </Box>
        ) : null}
      </Box>
    );
  }
  if (renderable.kind === "image") {
    return (
      <Box sx={{ mt: 0.5, maxWidth: "100%" }}>
        <Box
          component="img"
          src={renderable.src}
          alt=""
          sx={{ maxWidth: "100%", height: "auto", borderRadius: 0.5 }}
        />
      </Box>
    );
  }
  if (renderable.kind === "html") {
    const iframeSandbox = htmlSandboxAllowScripts
      ? "allow-scripts allow-same-origin allow-downloads"
      : "allow-downloads allow-same-origin";
    return (
      <Box sx={{ mt: 0.5, width: "100%", maxWidth: "100%" }}>
        <Box
          component="iframe"
          title="HTML output"
          sandbox={iframeSandbox}
          srcDoc={renderable.html}
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
  if (renderable.kind === "json" || renderable.kind === "text") {
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
          maxHeight: renderable.kind === "json" ? 360 : undefined,
          overflow: renderable.kind === "json" ? "auto" : undefined,
        }}
      >
        {renderable.text}
      </Box>
    );
  }
  if (renderable.kind === "markdown") {
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
        <ReactMarkdown remarkPlugins={[remarkGfm]}>{renderable.markdown}</ReactMarkdown>
      </Box>
    );
  }
  if (renderable.kind === "widget") {
    return (
      <Alert severity="info" variant="outlined" sx={{ mt: 0.5, py: 0.5 }}>
        <Typography variant="caption">{renderable.text}</Typography>
      </Alert>
    );
  }
  return (
    <Typography variant="caption" color="text.secondary" sx={{ mt: 0.5 }}>
      [{renderable.outputType || "display_data — 无可识别 MIME"}]
    </Typography>
  );
}

interface NotebookCellBodyProps {
  index: number;
  cell: NotebookCell;
  cellSignature: string;
  kernelLang: string;
  isActive: boolean;
  isRunning: boolean;
  runningAll: boolean;
  setActiveCellIndex: (index: number) => void;
  updateCellSource: (index: number, text: string) => void;
  updateCellType: (index: number, type: "code" | "markdown" | "raw") => void;
  insertCell: (index: number, type: "code" | "markdown", position: "before" | "after") => void;
  runCell: (index: number) => Promise<boolean>;
  clearOneOutput: (index: number) => void;
  deleteCell: (index: number) => void;
  attachCellEditorKeys: (
    index: number,
    editorInst: editor.IStandaloneCodeEditor,
    monaco: typeof import("monaco-editor"),
  ) => () => void;
}

function notebookCellOutputSignature(cell: NotebookCell): string {
  if (!Array.isArray(cell.outputs) || cell.outputs.length === 0) return "";
  try {
    return JSON.stringify(cell.outputs);
  } catch {
    return String(cell.outputs.length);
  }
}

function notebookCellRenderSignature(cell: NotebookCell): string {
  return [
    cell.id ?? "",
    cell.cell_type,
    getCellSource(cell),
    cell.execution_count ?? "",
    notebookCellOutputSignature(cell),
  ].join("\u001f");
}

function cloneNotebookForCellEdit(
  nb: NotebookDocument,
  index: number,
): { nb: NotebookDocument; cell: NotebookCell } | null {
  const originalCell = nb.cells[index];
  if (!originalCell) return null;
  const cells = nb.cells.slice();
  const cell = { ...originalCell };
  cells[index] = cell;
  return {
    nb: {
      ...nb,
      cells,
    },
    cell,
  };
}

function notebookEditorKeyFromMonaco(
  keyCode: number,
  keyCodes: typeof import("monaco-editor").KeyCode,
): NotebookEditorKey {
  if (keyCode === keyCodes.Enter) return "Enter";
  if (keyCode === keyCodes.UpArrow) return "ArrowUp";
  if (keyCode === keyCodes.DownArrow) return "ArrowDown";
  return "Other";
}

const NotebookCellBody = memo(function NotebookCellBody({
  index,
  cell,
  cellSignature: _cellSignature,
  kernelLang,
  isActive,
  isRunning,
  runningAll,
  setActiveCellIndex,
  updateCellSource,
  updateCellType,
  insertCell,
  runCell,
  clearOneOutput,
  deleteCell,
  attachCellEditorKeys,
}: NotebookCellBodyProps) {
  const theme = useTheme();
  const isMd = cell.cell_type === "markdown";
  const isCode = cell.cell_type === "code";
  const selectedCellType =
    cell.cell_type === "code" || cell.cell_type === "markdown" || cell.cell_type === "raw"
      ? cell.cell_type
      : "raw";
  const source = getCellSource(cell);
  const editorTheme = theme.palette.mode === "dark" ? "vs-dark" : "vs";
  const isDark = theme.palette.mode === "dark";
  const notebookBg = isDark ? "#1e1e1e" : theme.palette.background.default;
  const editorBg = isDark ? "#252526" : theme.palette.background.paper;
  const cellBorder = isActive ? theme.palette.primary.main : "transparent";
  const disabled = isRunning || runningAll;
  const languageLabel = cellLanguageLabel(cell, kernelLang);
  const cellTypeLabel =
    selectedCellType === "markdown" ? "Md" : selectedCellType === "raw" ? "Raw" : "Code";
  const editorHeight = notebookEditorHeight(source, isCode ? 44 : isMd ? 72 : 54, isCode ? 220 : 180);
  const cellActionHoverBg = alpha(
    theme.palette.primary.main,
    isDark ? 0.18 : 0.08,
  );
  const cellActionButtonSx = {
    width: 28,
    height: 28,
    borderRadius: 0,
    color: "text.secondary",
    "&:hover": {
      color: "primary.main",
      bgcolor: cellActionHoverBg,
    },
    "&.Mui-disabled": {
      color: "text.disabled",
    },
  };
  const [moreAnchorEl, setMoreAnchorEl] = useState<HTMLElement | null>(null);
  const closeMoreMenu = () => setMoreAnchorEl(null);

  return (
    <Box
      onClick={() => setActiveCellIndex(index)}
      sx={{
        position: "relative",
        display: "grid",
        gridTemplateColumns: "46px minmax(0, 1fr)",
        columnGap: 0.5,
        bgcolor: notebookBg,
        pt: 0.5,
        pb: 0.5,
        "&:hover .ipynb-cell-actions, &:focus-within .ipynb-cell-actions": {
          opacity: 1,
          pointerEvents: "auto",
        },
        "&:hover .ipynb-insert-below, &:focus-within .ipynb-insert-below": {
          opacity: 1,
          pointerEvents: "auto",
        },
      }}
    >
      <Stack
        alignItems="center"
        gap={0.5}
        sx={{
          pt: 0.75,
          color: "text.secondary",
          fontFamily: "JetBrains Mono, Monaco, Consolas, monospace",
        }}
      >
        {isCode ? (
          <NotebookTooltip title="运行此单元（Shift+Enter 运行并跳到下一格；Ctrl/Cmd+Enter 仅运行）">
            <span>
              <IconButton
                size="small"
                disabled={disabled}
                onClick={(event) => {
                  event.stopPropagation();
                  void runCell(index);
                }}
                aria-label="run notebook cell"
                sx={{
                  width: 28,
                  height: 28,
                  color: isActive ? "primary.main" : "text.secondary",
                  "&:hover": {
                    color: "primary.main",
                    bgcolor: alpha(theme.palette.primary.main, 0.08),
                  },
                }}
              >
                {isRunning ? (
	                  <CircularProgress size={16} />
	                ) : (
                  <PlayArrowRoundedIcon sx={{ fontSize: 20 }} />
                )}
              </IconButton>
            </span>
          </NotebookTooltip>
        ) : (
          <Box sx={{ width: 28, height: 28 }} />
        )}
        <Typography variant="caption" sx={{ fontSize: 11, lineHeight: 1, color: "text.secondary" }}>
          {isCode ? `[${cell.execution_count ?? " "}]` : ""}
        </Typography>
      </Stack>

      <Box sx={{ minWidth: 0, pr: 1.25 }}>
        <Box
          sx={{
            position: "relative",
            border: 1,
            borderColor: cellBorder,
            bgcolor: editorBg,
            boxShadow: isActive
              ? `inset 3px 0 0 ${theme.palette.primary.main}`
              : `inset 3px 0 0 transparent`,
            transition: "border-color 140ms ease, box-shadow 140ms ease",
            "&:hover": {
              borderColor: isActive
                ? theme.palette.primary.main
                : alpha(theme.palette.primary.main, 0.55),
            },
          }}
        >
          <Stack
            className="ipynb-cell-actions"
            direction="row"
            alignItems="center"
            spacing={0.25}
            sx={{
              position: "absolute",
              top: -1,
              right: 10,
              zIndex: 2,
              opacity: isActive ? 1 : 0,
              pointerEvents: isActive ? "auto" : "none",
              height: 28,
              bgcolor: isDark
                ? alpha(theme.palette.background.paper, 0.94)
                : alpha(theme.palette.background.paper, 0.98),
              border: 1,
              borderColor: isDark
                ? alpha(theme.palette.common.white, 0.18)
                : alpha(theme.palette.common.black, 0.14),
              boxShadow: isDark
                ? `0 8px 22px ${alpha(theme.palette.common.black, 0.28)}`
                : `0 8px 20px ${alpha(theme.palette.common.black, 0.1)}`,
              backdropFilter: "blur(10px)",
              WebkitBackdropFilter: "blur(10px)",
              overflow: "hidden",
            }}
            onClick={(event) => event.stopPropagation()}
          >
            <NotebookTooltip title="更改单元格类型">
              <Select
                size="small"
                value={selectedCellType}
                onChange={(event) => {
                  updateCellType(index, event.target.value as "code" | "markdown" | "raw");
                }}
                renderValue={() => cellTypeLabel}
                aria-label="更改单元格类型"
                sx={{
                  height: 28,
                  minWidth: 62,
                  flexShrink: 0,
                  borderRadius: 0,
                  fontSize: 12,
                  fontWeight: 600,
                  color: "text.secondary",
                  "& .MuiSelect-select": {
                    py: 0,
                    pl: 0.75,
                    pr: "20px !important",
                    display: "flex",
                    alignItems: "center",
                    height: 28,
                    lineHeight: "28px",
                  },
                  "& .MuiSvgIcon-root": {
                    right: 2,
                    fontSize: 16,
                    color: "text.secondary",
                  },
                  "& fieldset": { borderColor: "transparent" },
                  "&:hover": {
                    bgcolor: cellActionHoverBg,
                    color: "primary.main",
                  },
                  "&:hover fieldset": { borderColor: "transparent" },
                }}
              >
                <MenuItem value="code">Code</MenuItem>
                <MenuItem value="markdown">Markdown</MenuItem>
                <MenuItem value="raw">Raw</MenuItem>
              </Select>
            </NotebookTooltip>
            <NotebookTooltip title={languageLabel}>
              <IconButton
                size="small"
                aria-label="more notebook cell actions"
                onClick={(event) => setMoreAnchorEl(event.currentTarget)}
                sx={cellActionButtonSx}
              >
                <Box
                  component="span"
                  sx={{
                    width: "100%",
                    height: "100%",
                    display: "inline-flex",
                    alignItems: "center",
                    justifyContent: "center",
                    fontSize: 18,
                    lineHeight: 1,
                    transform: "translateY(-1px)",
                  }}
                >
                  …
                </Box>
              </IconButton>
            </NotebookTooltip>
            <NotebookTooltip title="删除单元">
              <IconButton
                size="small"
                color="error"
                onClick={() => deleteCell(index)}
                aria-label="delete cell"
                sx={{
                  ...cellActionButtonSx,
                  color: "error.main",
                  "&:hover": {
                    color: "error.main",
                    bgcolor: alpha(theme.palette.error.main, isDark ? 0.2 : 0.08),
                  },
                }}
              >
                <DeleteOutlineIcon sx={{ fontSize: 16 }} />
              </IconButton>
            </NotebookTooltip>
          </Stack>
          <Menu
            anchorEl={moreAnchorEl}
            open={Boolean(moreAnchorEl)}
            onClose={closeMoreMenu}
            anchorOrigin={{ vertical: "bottom", horizontal: "right" }}
            transformOrigin={{ vertical: "top", horizontal: "right" }}
            MenuListProps={{ dense: true }}
          >
            <MenuItem
              selected={selectedCellType === "code"}
              onClick={() => {
                updateCellType(index, "code");
                closeMoreMenu();
              }}
            >
              Change Cell to Code
            </MenuItem>
            <MenuItem
              selected={selectedCellType === "markdown"}
              onClick={() => {
                updateCellType(index, "markdown");
                closeMoreMenu();
              }}
            >
              Change Cell to Markdown
            </MenuItem>
            <MenuItem
              selected={selectedCellType === "raw"}
              onClick={() => {
                updateCellType(index, "raw");
                closeMoreMenu();
              }}
            >
              Change Cell to Raw
            </MenuItem>
            {isCode && (
              <MenuItem
                onClick={() => {
                  clearOneOutput(index);
                  closeMoreMenu();
                }}
              >
                Clear Outputs
              </MenuItem>
            )}
            <MenuItem
              onClick={() => {
                deleteCell(index);
                closeMoreMenu();
              }}
              sx={{ color: "error.main" }}
            >
              Delete Cell
            </MenuItem>
          </Menu>

          {isMd && (
            <Box sx={{ p: 1.25, pb: 0 }}>
              <Box
                className="ipynb-md-preview"
                sx={{
                  color: "text.primary",
                  fontSize: 13,
                  minHeight: 28,
                  "& > :first-of-type": { mt: 0 },
                  "& > :last-child": { mb: 0 },
                  "& pre": { overflow: "auto", p: 1, bgcolor: "action.hover", borderRadius: 1 },
                  "& code": { fontFamily: "JetBrains Mono, monospace", fontSize: 12 },
                }}
              >
                <ReactMarkdown remarkPlugins={[remarkGfm]}>{source || " "}</ReactMarkdown>
              </Box>
              <Divider sx={{ my: 1, opacity: 0.6 }} />
            </Box>
          )}

          <Editor
            height={`${editorHeight}px`}
            language={
              isCode
                ? monacoLanguageForNotebook(kernelLang)
                : isMd
                  ? "markdown"
                  : "plaintext"
            }
            theme={editorTheme}
            value={source}
            onChange={(v) => updateCellSource(index, v ?? "")}
            onMount={(ed, monaco) => {
              const focusSubscription = ed.onDidFocusEditorWidget(() => setActiveCellIndex(index));
              const cleanupKeys = attachCellEditorKeys(index, ed, monaco);
              return () => {
                focusSubscription.dispose();
                cleanupKeys();
              };
            }}
            options={{
              minimap: { enabled: false },
              fontSize: 13,
              lineHeight: 20,
              padding: { top: 8, bottom: 8 },
              scrollBeyondLastLine: false,
              wordWrap: isCode ? "off" : "on",
              automaticLayout: true,
              glyphMargin: false,
              folding: false,
              lineNumbers: isCode ? "on" : "off",
              renderLineHighlight: "none",
              overviewRulerBorder: false,
              hideCursorInOverviewRuler: true,
            }}
          />
        </Box>

        {isCode && cell.outputs && cell.outputs.length > 0 && (
          <Box
            sx={{
              px: 1.5,
              py: 1,
              borderLeft: 1,
              borderRight: 1,
              borderBottom: 1,
              borderColor: "divider",
              bgcolor: isDark ? "#1e1e1e" : "background.paper",
            }}
          >
            {cell.outputs.map((out, oi) => (
              <OutputBlock key={oi} output={out} />
            ))}
          </Box>
        )}
      </Box>
      <Stack
        className="ipynb-insert-below"
        alignItems="center"
        sx={{
          gridColumn: "2 / 3",
          mt: "3px",
          height: 30,
          opacity: 0,
          pointerEvents: "none",
          transition: "opacity 120ms ease",
        }}
      >
        <NotebookInsertButtons
          onAddCode={() => insertCell(index, "code", "after")}
          onAddMarkdown={() => insertCell(index, "markdown", "after")}
        />
      </Stack>
    </Box>
  );
}, (prev, next) =>
  prev.index === next.index &&
  prev.cellSignature === next.cellSignature &&
  prev.kernelLang === next.kernelLang &&
  prev.isActive === next.isActive &&
  prev.isRunning === next.isRunning &&
  prev.runningAll === next.runningAll &&
  prev.setActiveCellIndex === next.setActiveCellIndex &&
  prev.updateCellSource === next.updateCellSource &&
  prev.updateCellType === next.updateCellType &&
  prev.insertCell === next.insertCell &&
  prev.runCell === next.runCell &&
  prev.clearOneOutput === next.clearOneOutput &&
  prev.deleteCell === next.deleteCell &&
  prev.attachCellEditorKeys === next.attachCellEditorKeys
);

const ESTIMATE_CELL_H = 320;
const NOTEBOOK_DRAFT_EMIT_DELAY_MS = 140;
type NotebookCommitMode = "immediate" | "deferred";

export function IpynbViewer({ filePath, content, onChange }: IpynbViewerProps) {
  const theme = useTheme();
  const isDark = theme.palette.mode === "dark";
  const [runningIdx, setRunningIdx] = useState<number | null>(null);
  const [runningAll, setRunningAll] = useState(false);
  const [runError, setRunError] = useState<string | null>(null);
  const [activeCellIndex, setActiveCellIndex] = useState<number | null>(0);

  const virtualizeCells = useNotebookViewerStore((s) => s.virtualizeCells);
  const enableNotebookShortcuts = useNotebookViewerStore((s) => s.enableNotebookShortcuts);
  const enablePythonShellMagicHint = useNotebookViewerStore((s) => s.enablePythonShellMagic);

  const isEmptyNotebookFile = content.trim().length === 0;
  const autoInitializedEmptyFileRef = useRef<string | null>(null);

  useEffect(() => {
    if (!isEmptyNotebookFile) {
      if (autoInitializedEmptyFileRef.current === filePath) {
        autoInitializedEmptyFileRef.current = null;
      }
      return;
    }
    if (!onChange || autoInitializedEmptyFileRef.current === filePath) return;
    autoInitializedEmptyFileRef.current = filePath;
    onChange(serializeNotebook(createEmptyNotebook()));
  }, [filePath, isEmptyNotebookFile, onChange]);

  const [parsed, setParsed] = useState<NotebookParseResult>(() =>
    parseNotebookContent(content),
  );
  const latestContentPropRef = useRef(content);
  const lastEmittedContentRef = useRef<string | null>(null);

  useEffect(() => {
    if (latestContentPropRef.current === content) return;
    latestContentPropRef.current = content;
    if (lastEmittedContentRef.current === content) return;
    setParsed(parseNotebookContent(content));
  }, [content]);

  const kernelLang = parsed.ok ? notebookKernelLanguage(parsed.nb) : "python";
  const langArg = executionLanguageForNotebook(kernelLang);

  const nbRef = useRef(parsed.ok ? parsed.nb : null);
  if (parsed.ok) nbRef.current = parsed.nb;
  useEffect(
    () =>
      registerWorkspaceContentProvider(filePath, () => {
        const nb = nbRef.current;
        return nb ? serializeNotebook(nb) : content;
      }),
    [content, filePath],
  );
  const runningIdxRef = useRef(runningIdx);
  const runningAllRef = useRef(runningAll);
  runningIdxRef.current = runningIdx;
  runningAllRef.current = runningAll;

  const pendingEmitNbRef = useRef<NotebookDocument | null>(null);
  const emitTimerRef = useRef<number | null>(null);

  const emitNotebookContent = useCallback(
    (nb: NotebookDocument) => {
      const serialized = serializeNotebook(nb);
      lastEmittedContentRef.current = serialized;
      latestContentPropRef.current = serialized;
      onChange?.(serialized);
    },
    [onChange],
  );

  const flushPendingNotebookEmit = useCallback(() => {
    if (emitTimerRef.current !== null) {
      window.clearTimeout(emitTimerRef.current);
      emitTimerRef.current = null;
    }
    const pending = pendingEmitNbRef.current;
    pendingEmitNbRef.current = null;
    if (pending) emitNotebookContent(pending);
  }, [emitNotebookContent]);

  const scheduleNotebookEmit = useCallback(
    (nb: NotebookDocument) => {
      pendingEmitNbRef.current = nb;
      useWorkspaceStore.getState().markContentDirty(filePath);
      if (emitTimerRef.current !== null) {
        window.clearTimeout(emitTimerRef.current);
      }
      emitTimerRef.current = window.setTimeout(() => {
        flushPendingNotebookEmit();
      }, NOTEBOOK_DRAFT_EMIT_DELAY_MS);
    },
    [filePath, flushPendingNotebookEmit],
  );

  useEffect(
    () => () => {
      if (emitTimerRef.current !== null) {
        window.clearTimeout(emitTimerRef.current);
        emitTimerRef.current = null;
      }
      pendingEmitNbRef.current = null;
    },
    [],
  );

  const pushNotebook = useCallback(
    (nb: NotebookDocument, mode: NotebookCommitMode = "immediate") => {
      nbRef.current = nb;
      setParsed({ ok: true, nb, initialized: false, warnings: [] });
      if (mode === "deferred") {
        scheduleNotebookEmit(nb);
        return;
      }
      if (emitTimerRef.current !== null) {
        window.clearTimeout(emitTimerRef.current);
        emitTimerRef.current = null;
      }
      pendingEmitNbRef.current = null;
      emitNotebookContent(nb);
    },
    [emitNotebookContent, scheduleNotebookEmit],
  );

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
      const current = nbRef.current;
      if (!current) return;
      const edit = cloneNotebookForCellEdit(current, index);
      if (!edit) return;
      setCellSource(edit.cell, text);
      pushNotebook(edit.nb, "deferred");
    },
    [pushNotebook],
  );

  const focusNotebookCell = useCallback(
    (index: number, placement: "start" | "end" = "start") => {
      const cells = nbRef.current?.cells;
      if (!cells || cells.length === 0) return;
      const target = Math.max(0, Math.min(index, cells.length - 1));
      setActiveCellIndex(target);
      virtualizerRef.current?.scrollToIndex(target, { align: "auto" });

      let attempts = 0;
      const tryFocus = () => {
        virtualizerRef.current?.scrollToIndex(target, { align: "auto" });
        const editorInst = codeEditorRefs.current.get(target);
        if (editorInst) {
          const model = editorInst.getModel();
          editorInst.focus();
          if (model) {
            const lineNumber = placement === "end" ? model.getLineCount() : 1;
            const column = placement === "end" ? model.getLineMaxColumn(lineNumber) : 1;
            const position = { lineNumber, column };
            editorInst.setPosition(position);
            editorInst.revealPositionInCenterIfOutsideViewport(position);
          }
          return;
        }
        attempts += 1;
        if (attempts <= 12) {
          window.setTimeout(tryFocus, attempts <= 2 ? 0 : 25);
        }
      };

      window.setTimeout(tryFocus, 0);
    },
    [],
  );

  const updateCellType = useCallback(
    (index: number, type: "code" | "markdown" | "raw") => {
      const current = nbRef.current;
      if (!current) return;
      const edit = cloneNotebookForCellEdit(current, index);
      if (!edit) return;
      setNotebookCellType(edit.cell, type);
      setActiveCellIndex(index);
      pushNotebook(edit.nb);
    },
    [pushNotebook],
  );

  const runtimeAdapter = useMemo(() => createTauriNotebookRuntimeAdapter(), []);

  const notebookExecutionOptions = useCallback(
    () => ({
      notebookPath: filePath,
      language: langArg,
      shellMagic: useNotebookViewerStore.getState().enablePythonShellMagic,
    }),
    [filePath, langArg],
  );

  const updateExecutionStatus = useCallback((status: NotebookExecutionStatus) => {
    runningIdxRef.current = status.runningCellIndex;
    runningAllRef.current = status.runningAll;
    setRunningIdx(status.runningCellIndex);
    setRunningAll(status.runningAll);
    setRunError(status.error);
  }, []);

  const executionHostRef = useRef<NotebookExecutionControllerHost | null>(null);
  executionHostRef.current = {
    getNotebook: () => nbRef.current,
    getOptions: notebookExecutionOptions,
    execute: runtimeAdapter.executeCell,
    commit: pushNotebook,
    onStatus: updateExecutionStatus,
    formatError: (error) => String(error),
  };

  const executionControllerRef = useRef<NotebookExecutionController | null>(null);
  if (!executionControllerRef.current) {
    executionControllerRef.current = new NotebookExecutionController({
      getNotebook: () => executionHostRef.current?.getNotebook() ?? null,
      getOptions: () => {
        const host = executionHostRef.current;
        if (!host) return { notebookPath: filePath, language: langArg, shellMagic: true };
        return host.getOptions();
      },
      execute: (request) => {
        const host = executionHostRef.current;
        if (!host) return Promise.reject(new Error("Notebook execution host not ready"));
        return host.execute(request);
      },
      commit: (nb) => executionHostRef.current?.commit(nb),
      onStatus: (status) => executionHostRef.current?.onStatus(status),
      formatError: (error) => executionHostRef.current?.formatError?.(error) ?? String(error),
    });
  }

  const runCell = useCallback(async (index: number) => {
    const controller = executionControllerRef.current;
    if (!controller) return false;
    return controller.runCell(index);
  }, []);

  const runCellRef = useRef(runCell);
  runCellRef.current = runCell;

  const runAll = useCallback(async () => {
    const controller = executionControllerRef.current;
    if (!controller) return false;
    return controller.runAll();
  }, []);

  const clearAllOutputs = useCallback(() => {
    const current = nbRef.current;
    if (!current) return;
    let changed = false;
    const cells = current.cells.map((c) => {
      if (c.cell_type === "code") {
        const hasOutputs = Array.isArray(c.outputs) && c.outputs.length > 0;
        const hasExecutionCount = c.execution_count !== null && c.execution_count !== undefined;
        if (hasOutputs || hasExecutionCount) {
          changed = true;
          return { ...c, outputs: [], execution_count: null };
        }
      }
      return c;
    });
    if (changed) pushNotebook({ ...current, cells });
  }, [pushNotebook]);

  const clearOneOutput = useCallback(
    (index: number) => {
      const current = nbRef.current;
      if (!current) return;
      const edit = cloneNotebookForCellEdit(current, index);
      if (!edit || edit.cell.cell_type !== "code") return;
      edit.cell.outputs = [];
      edit.cell.execution_count = null;
      pushNotebook(edit.nb);
    },
    [pushNotebook],
  );

  const deleteCell = useCallback(
    (index: number) => {
      const current = nbRef.current;
      if (!current) return;
      const cells = current.cells.slice();
      cells.splice(index, 1);
      const nb = { ...current, cells };
      setActiveCellIndex((current) => {
        if (nb.cells.length === 0) return null;
        if (current === null) return nb.cells.length > 0 ? 0 : null;
        if (current === index) return Math.min(index, Math.max(0, nb.cells.length - 1));
        if (current > index) return current - 1;
        return current;
      });
      pushNotebook(nb);
    },
    [pushNotebook],
  );

  const insertCell = useCallback(
    (index: number, type: "code" | "markdown", position: "before" | "after") => {
      const current = nbRef.current;
      if (!current) return;
      const cells = current.cells.slice();
      const newCell: NotebookCell = createNotebookCell(type);
      const at = position === "before" ? index : index + 1;
      cells.splice(at, 0, newCell);
      const nb = { ...current, cells };
      setActiveCellIndex(at);
      pushNotebook(nb);
    },
    [pushNotebook],
  );

  const addCellAtEnd = useCallback(
    (type: "code" | "markdown") => {
      const current = nbRef.current;
      if (!current) return;
      const newCell: NotebookCell = createNotebookCell(type);
      const cells = [...current.cells, newCell];
      const nb = { ...current, cells };
      setActiveCellIndex(nb.cells.length - 1);
      pushNotebook(nb);
    },
    [pushNotebook],
  );

  const updateKernelLanguage = useCallback(
    (language: string) => {
      const current = nbRef.current;
      if (!current) return;
      const nb = { ...current, cells: current.cells };
      setNotebookKernelLanguage(nb, language);
      pushNotebook(nb);
    },
    [pushNotebook],
  );

  const executeNotebookEditorCommand = useCallback(
    async (command: NotebookEditorCommand) => {
      switch (command.type) {
        case "none":
        case "blocked":
          return;
        case "run-cell":
          await runCellRef.current(command.cellIndex);
          return;
        case "focus-relative-cell":
          focusNotebookCell(command.fromIndex + command.offset, command.placement);
          return;
        case "run-and-focus-next": {
          const current = nbRef.current;
          if (!current) return;
          if (current.cells[command.cellIndex]?.cell_type === "code") {
            await runCellRef.current(command.cellIndex);
          }

          const latest = nbRef.current;
          if (!latest) return;
          const target = nextCellTargetAfterRun(command.cellIndex, latest.cells.length);
          if (!target.createCodeCell) {
            focusNotebookCell(target.targetIndex, "start");
            return;
          }

          const cells = latest.cells.slice();
          cells.splice(target.targetIndex, 0, createNotebookCell("code"));
          pushNotebook({ ...latest, cells });
          focusNotebookCell(target.targetIndex, "start");
        }
      }
    },
    [focusNotebookCell, pushNotebook],
  );

  const attachCellEditorKeys = useCallback(
    (index: number, editorInst: editor.IStandaloneCodeEditor, monaco: typeof import("monaco-editor")) => {
      codeEditorRefs.current.set(index, editorInst);
      const sub = editorInst.onKeyDown((e) => {
        const model = editorInst.getModel();
        const position = editorInst.getPosition();
        const cells = nbRef.current?.cells ?? [];
        const cell = cells[index];
        const command = resolveNotebookEditorCommand(
          {
            key: notebookEditorKeyFromMonaco(e.keyCode, monaco.KeyCode),
            shiftKey: e.shiftKey,
            ctrlKey: e.ctrlKey,
            metaKey: e.metaKey,
            altKey: e.altKey,
          },
          {
            shortcutsEnabled: useNotebookViewerStore.getState().enableNotebookShortcuts,
            isBusy: runningIdxRef.current !== null || runningAllRef.current,
            cellIndex: index,
            cellCount: cells.length,
            cellType: cell?.cell_type ?? "raw",
            cursorLineNumber: position?.lineNumber ?? 1,
            lineCount: model?.getLineCount() ?? 1,
          },
        );
        if (!command.consume) return;
        e.preventDefault();
        e.stopPropagation();
        void executeNotebookEditorCommand(command);
      });
      return () => {
        sub.dispose();
        codeEditorRefs.current.delete(index);
      };
    },
    [executeNotebookEditorCommand],
  );

  if (!parsed.ok) {
    return (
      <Box sx={{ p: 2 }}>
        <Alert severity="error" sx={{ borderRadius: 2 }}>
          <Typography variant="body2" fontWeight={600}>
            {parsed.error}
          </Typography>
          <Typography variant="caption" color="text.secondary">
            {OMIGA_NOTEBOOK_PLUGIN.displayName} 插件需要有效 .ipynb JSON。空
            notebook 会自动初始化；若仍看到此错误说明文件内容不是合法 JSON。
          </Typography>
        </Alert>
      </Box>
    );
  }

  const { nb } = parsed;
  const kernelName = notebookKernelName(nb);
  const selectedKernelLanguage = kernelLang === "r" ? "r" : "python";
  const selectedKernelLabel =
    NOTEBOOK_EXECUTABLE_KERNEL_OPTIONS.find((option) => option.language === selectedKernelLanguage)
      ?.label ?? kernelName;
  const isBusy = runningIdx !== null || runningAll;
  const activeNotebookCell =
    activeCellIndex !== null ? nb.cells[activeCellIndex] : nb.cells[0];
  const activeCellType =
    activeNotebookCell?.cell_type === "code" ||
    activeNotebookCell?.cell_type === "markdown" ||
    activeNotebookCell?.cell_type === "raw"
      ? activeNotebookCell.cell_type
      : "raw";
  const shortcutSummary = [
    enableNotebookShortcuts ? "Shift+Enter / Ctrl+Enter" : "快捷键已关闭",
    enablePythonShellMagicHint ? "支持 Python ! shell" : "Python ! shell 已关闭",
  ].join(" · ");

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
        bgcolor: isDark ? "#1e1e1e" : "background.default",
      }}
    >
      <Stack
        direction="row"
        alignItems="center"
        gap={0}
        sx={{
          minHeight: 34,
          px: 0.75,
          borderBottom: 1,
          borderColor: isDark ? alpha(theme.palette.common.white, 0.12) : "divider",
          bgcolor: isDark ? "#252526" : alpha(theme.palette.background.paper, 0.98),
          color: "text.secondary",
          overflowX: "auto",
          overflowY: "hidden",
          whiteSpace: "nowrap",
        }}
      >
        <NotebookToolbarButton
          title="从上到下依次运行所有代码单元（In [1]… 顺序编号）"
          disabled={isBusy}
          onClick={() => void runAll()}
          icon={
            runningAll ? (
              <CircularProgress size={13} color="inherit" />
            ) : (
              <PlayArrowRoundedIcon sx={{ fontSize: 16 }} />
            )
          }
          label="Run All"
        />
        <NotebookToolbarButton
          title="清空所有代码单元的输出"
          disabled={isBusy}
          onClick={clearAllOutputs}
          icon={<ClearAllIcon sx={{ fontSize: 16 }} />}
          label="Clear Outputs"
        />
        <NotebookToolbarButton
          title="重启内核（本地执行器暂未保持长驻 kernel，会在后续版本接入）"
          disabled
          icon={<Typography component="span" sx={{ fontSize: 16, lineHeight: 1 }}>↻</Typography>}
          label="Restart"
        />
        <NotebookToolbarButton
          title="中断当前运行（长驻 kernel 接入后启用）"
          disabled
          icon={<Typography component="span" sx={{ fontSize: 12, lineHeight: 1 }}>■</Typography>}
          label="Interrupt"
        />
        <NotebookToolbarButton
          title="变量视图将在 Notebook runtime 接入后启用"
          disabled
          icon={<Typography component="span" sx={{ fontSize: 14, lineHeight: 1 }}>▦</Typography>}
          label="Variables"
        />
        <NotebookTooltip title="更改当前选中单元格类型">
          <Select
            size="small"
            value={activeCellType}
            disabled={!activeNotebookCell || isBusy}
            onChange={(event) => {
              const index = activeCellIndex ?? 0;
              updateCellType(index, event.target.value as "code" | "markdown" | "raw");
            }}
            renderValue={(value) =>
              value === "markdown" ? "Markdown" : value === "raw" ? "Raw" : "Code"
            }
            aria-label="更改当前单元格类型"
            sx={{
              mx: 0.25,
              minWidth: 104,
              flexShrink: 0,
              height: 28,
              fontSize: 12,
              color: "text.secondary",
              bgcolor: alpha(theme.palette.background.paper, isDark ? 0.35 : 0.72),
              "& .MuiSelect-select": { py: 0.25, pl: 1 },
              "& fieldset": { borderColor: "transparent" },
              "&:hover fieldset": { borderColor: "divider" },
            }}
          >
            <MenuItem value="code">Code</MenuItem>
            <MenuItem value="markdown">Markdown</MenuItem>
            <MenuItem value="raw">Raw</MenuItem>
          </Select>
        </NotebookTooltip>
        <NotebookTooltip title={`${OMIGA_NOTEBOOK_PLUGIN.displayName} · ${shortcutSummary}`}>
          <IconButton
            size="small"
            aria-label="notebook details"
            sx={{ width: 28, height: 28, ml: 0.25, flexShrink: 0 }}
          >
            <Typography component="span" sx={{ fontSize: 18, lineHeight: 1 }}>
              ⋯
            </Typography>
          </IconButton>
        </NotebookTooltip>
        <Divider orientation="vertical" flexItem sx={{ mx: 0.5, flexShrink: 0 }} />
        <Box sx={{ flex: 1, minWidth: 16 }} />
        <NotebookTooltip title={`Kernel: ${kernelName}`}>
          <Select
            size="small"
            value={selectedKernelLanguage}
            onChange={(event) => updateKernelLanguage(event.target.value)}
            renderValue={() => selectedKernelLabel}
            aria-label="选择 notebook kernel"
            sx={{
              minWidth: 170,
              flexShrink: 0,
              height: 28,
              fontSize: 12,
              color: "text.secondary",
              bgcolor: alpha(theme.palette.background.paper, isDark ? 0.35 : 0.72),
              "& .MuiSelect-select": { py: 0.35, pl: 1 },
              "& fieldset": { borderColor: "transparent" },
              "&:hover fieldset": { borderColor: "divider" },
            }}
          >
            {NOTEBOOK_EXECUTABLE_KERNEL_OPTIONS.map((option) => (
              <MenuItem key={option.language} value={option.language}>
                {option.label}
              </MenuItem>
            ))}
          </Select>
        </NotebookTooltip>
      </Stack>

      <Box
        ref={scrollParentRef}
        sx={{
          flex: 1,
          minHeight: 0,
          overflow: "auto",
          py: 0.5,
          bgcolor: isDark ? "#1e1e1e" : "background.default",
        }}
      >
        {runError && (
          <Typography color="error" variant="caption" sx={{ display: "block", mb: 1 }}>
            {runError}
          </Typography>
        )}
        {nb.cells.length === 0 ? (
          <Stack
            alignItems="center"
            justifyContent="flex-start"
            sx={{ pt: 2.5, minHeight: 140 }}
          >
            <NotebookInsertButtons
              onAddCode={() => addCellAtEnd("code")}
              onAddMarkdown={() => addCellAtEnd("markdown")}
            />
          </Stack>
        ) : virtualizeCells ? (
          <Box sx={{ position: "relative", width: "100%", height: totalVirtH }}>
            {virtualItems.map((virtualRow) => {
              const index = virtualRow.index;
              const c = nb.cells[index];
              const cellId = c.id ?? `cell-${index}`;
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
                    pb: 0,
                  }}
                >
                  <NotebookCellBody
                    index={index}
                    cell={c}
                    cellSignature={notebookCellRenderSignature(c)}
                    kernelLang={kernelLang}
                    isActive={activeCellIndex === index}
                    isRunning={runningIdx === index}
                    runningAll={runningAll}
                    setActiveCellIndex={setActiveCellIndex}
                    updateCellSource={updateCellSource}
                    updateCellType={updateCellType}
                    insertCell={insertCell}
                    runCell={runCell}
                    clearOneOutput={clearOneOutput}
                    deleteCell={deleteCell}
                    attachCellEditorKeys={attachCellEditorKeys}
                  />
                </Box>
              );
            })}
          </Box>
        ) : (
          <Stack spacing={0}>
            {nb.cells.map((_, index) => {
              const c = nb.cells[index];
              const cellId = c.id ?? `cell-${index}`;
              return (
                <Box key={cellId}>
                  <NotebookCellBody
                    index={index}
                    cell={c}
                    cellSignature={notebookCellRenderSignature(c)}
                    kernelLang={kernelLang}
                    isActive={activeCellIndex === index}
                    isRunning={runningIdx === index}
                    runningAll={runningAll}
                    setActiveCellIndex={setActiveCellIndex}
                    updateCellSource={updateCellSource}
                    updateCellType={updateCellType}
                    insertCell={insertCell}
                    runCell={runCell}
                    clearOneOutput={clearOneOutput}
                    deleteCell={deleteCell}
                    attachCellEditorKeys={attachCellEditorKeys}
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
