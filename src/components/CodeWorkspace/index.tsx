import { useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  Box,
  Typography,
  Button,
  Stack,
  Paper,
  useTheme,
  alpha,
  CircularProgress,
  IconButton,
  Tooltip,
} from "@mui/material";
import PlayArrowRoundedIcon from "@mui/icons-material/PlayArrowRounded";
import SaveRoundedIcon from "@mui/icons-material/SaveRounded";
import CloseRoundedIcon from "@mui/icons-material/CloseRounded";
import { useWorkspaceStore } from "../../state/workspaceStore";
import { FileRenderer } from "./FileRenderer";
import { extToLabel } from "./CodeViewer";

/**
 * Code editor region — shows file content from workspace store (opened from file tree).
 * Supports editing + saving via Monaco Editor.
 */
interface DocRenderResult {
  stdout: string;
  stderr: string;
  exit_code: number;
}

export function CodeWorkspace() {
  const theme = useTheme();
  const {
    filePath,
    fileName,
    content,
    totalLines,
    isLoading,
    isSaving,
    isDirty,
    error,
    saveError,
    clearFile,
    setContent,
    saveFile,
  } = useWorkspaceStore();

  const [renderOut, setRenderOut] = useState<DocRenderResult | null>(null);
  const [renderErr, setRenderErr] = useState<string | null>(null);
  const [renderLoading, setRenderLoading] = useState(false);

  const fileExt = fileName ? (fileName.split(".").pop() ?? "").toLowerCase() : "";
  const languageLabel = fileExt ? extToLabel(fileExt) : "Plain Text";
  const isReadOnly = ["png","jpg","jpeg","gif","webp","svg","bmp","ico","tiff","tif","avif","pdf"].includes(fileExt);
  const isRmd = fileExt === "rmd";
  const isQmd = fileExt === "qmd";
  const isRenderableDoc = isRmd || isQmd;

  const handleRunDoc = useCallback(async () => {
    if (!filePath || !isRenderableDoc || renderLoading) return;
    setRenderErr(null);
    setRenderOut(null);
    setRenderLoading(true);
    try {
      if (isDirty) {
        await saveFile();
        const saveErr = useWorkspaceStore.getState().saveError;
        if (saveErr) {
          setRenderErr(`请先保存文件: ${saveErr}`);
          return;
        }
      }
      const res = await invoke<DocRenderResult>(
        isRmd ? "render_rmarkdown" : "render_quarto",
        { path: filePath },
      );
      setRenderOut(res);
      if (res.exit_code !== 0) {
        setRenderErr(
          `${isRmd ? "R" : "Quarto"} 进程退出码 ${res.exit_code}`,
        );
      }
    } catch (e) {
      setRenderErr(String(e));
    } finally {
      setRenderLoading(false);
    }
  }, [filePath, isRmd, isQmd, isDirty, renderLoading, saveFile]);

  return (
    <Paper
      elevation={0}
      square
      sx={{
        flex: 1,
        minHeight: 0,
        height: "100%",
        minWidth: 0,
        display: "flex",
        flexDirection: "column",
        overflow: "hidden",
        bgcolor: "background.paper",
        borderBottom: 1,
        borderColor: "divider",
      }}
    >
      {/* ── Toolbar ── */}
      <Stack
        direction="row"
        alignItems="stretch"
        justifyContent="space-between"
        sx={{
          minHeight: 40,
          px: 1,
          bgcolor: alpha(theme.palette.grey[100], 0.85),
          borderBottom: 1,
          borderColor: "divider",
        }}
      >
        <Stack
          direction="row"
          alignItems="center"
          sx={{ px: 1, minWidth: 0, flex: 1 }}
        >
          {/* Dirty indicator dot */}
          {isDirty && (
            <Box
              sx={{
                width: 7,
                height: 7,
                borderRadius: "50%",
                bgcolor: "warning.main",
                flexShrink: 0,
                mr: 0.75,
              }}
            />
          )}
          <Typography
            variant="body2"
            fontWeight={600}
            noWrap
            title={filePath ?? undefined}
            sx={{ minWidth: 0 }}
          >
            {fileName ?? "代码"}
          </Typography>
          {filePath && (
            <Tooltip title="关闭文件">
              <IconButton
                size="small"
                aria-label="关闭文件"
                onClick={() => clearFile()}
                sx={{
                  flexShrink: 0,
                  ml: 0.25,
                  color: "text.secondary",
                  "&:hover": {
                    color: "text.primary",
                    bgcolor: alpha(theme.palette.error.main, 0.08),
                  },
                }}
              >
                <CloseRoundedIcon sx={{ fontSize: 18 }} />
              </IconButton>
            </Tooltip>
          )}
          {!filePath && (
            <Typography
              variant="caption"
              color="text.secondary"
              sx={{ ml: 1 }}
              noWrap
            >
              点击右侧工具目录中的文件查看源码
            </Typography>
          )}
        </Stack>

        <Stack direction="row" alignItems="center" gap={0.75} sx={{ pr: 1 }}>
          {/* Save button — only for editable files */}
          {filePath && !isReadOnly && (
            <Tooltip title={isDirty ? "保存文件 (⌘S)" : "已保存"}>
              <span>
                <Button
                  size="small"
                  variant={isDirty ? "outlined" : "text"}
                  color={isDirty ? "warning" : "inherit"}
                  disableElevation
                  disabled={!isDirty || isSaving}
                  onClick={() => void saveFile()}
                  startIcon={
                    isSaving ? (
                      <CircularProgress size={13} color="inherit" />
                    ) : (
                      <SaveRoundedIcon sx={{ fontSize: 16 }} />
                    )
                  }
                  sx={{
                    textTransform: "none",
                    minHeight: 30,
                    px: 1.25,
                    borderRadius: 1.5,
                    fontSize: 12,
                    fontWeight: 600,
                  }}
                >
                  {isSaving ? "保存中…" : "保存"}
                </Button>
              </span>
            </Tooltip>
          )}

          <Tooltip
            title={
              isRmd
                ? "调用 Rscript 执行 rmarkdown::render（需本机已安装 R 与 rmarkdown 包）"
                : isQmd
                  ? "quarto CLI 需达到 OMIGA_MIN_QUARTO_VERSION（默认 1.3.0）；未安装或低于该版本时回退到 R 的 quarto::quarto_render"
                  : "当前仅支持对 .Rmd / .qmd 执行渲染"
            }
          >
            <span>
              <Button
                size="small"
                variant="contained"
                disableElevation
                disabled={!filePath || !isRenderableDoc || renderLoading}
                onClick={() => void handleRunDoc()}
                startIcon={
                  renderLoading ? (
                    <CircularProgress size={14} color="inherit" />
                  ) : (
                    <PlayArrowRoundedIcon sx={{ fontSize: 18 }} />
                  )
                }
                sx={{
                  textTransform: "none",
                  minHeight: 30,
                  px: 1.5,
                  borderRadius: 1.5,
                  fontSize: 13,
                  fontWeight: 600,
                }}
              >
                {isRmd ? "渲染 Rmd" : isQmd ? "渲染 Quarto" : "Run"}
              </Button>
            </span>
          </Tooltip>
        </Stack>
      </Stack>

      {/* ── Body ── */}
      <Box
        role="region"
        aria-label="Code editor"
        sx={{
          flex: 1,
          minHeight: 0,
          display: "flex",
          flexDirection: "column",
          overflow: "hidden",
        }}
      >
        {isLoading ? (
          <Stack
            alignItems="center"
            justifyContent="center"
            sx={{ flex: 1, py: 4 }}
          >
            <CircularProgress size={28} sx={{ mb: 1 }} />
            <Typography variant="body2" color="text.secondary">
              正在读取…
            </Typography>
          </Stack>
        ) : error ? (
          <Stack
            alignItems="center"
            justifyContent="center"
            sx={{ flex: 1, px: 2, py: 3 }}
          >
            <Typography variant="body2" color="error" textAlign="center">
              {error}
            </Typography>
          </Stack>
        ) : !filePath ? (
          <Stack
            alignItems="center"
            justifyContent="center"
            sx={{ flex: 1, px: 2, py: 3 }}
          >
            <Typography variant="body2" color="text.secondary" textAlign="center">
              未打开文件
              <br />
              请从右侧「工具目录」点击文件查看源码
            </Typography>
          </Stack>
        ) : (
          <>
            <FileRenderer
              fileName={fileName!}
              filePath={filePath!}
              content={content}
              onChange={setContent}
            />
            {isRenderableDoc && (renderOut || renderErr || renderLoading) && (
              <Box
                sx={{
                  flexShrink: 0,
                  maxHeight: 200,
                  overflow: "auto",
                  borderTop: 1,
                  borderColor: "divider",
                  px: 1.5,
                  py: 1,
                  bgcolor: alpha(theme.palette.grey[100], 0.6),
                  fontFamily: "JetBrains Mono, Monaco, Consolas, monospace",
                  fontSize: 11,
                  lineHeight: 1.5,
                }}
              >
                <Typography variant="caption" fontWeight={700} color="text.secondary" display="block" sx={{ mb: 0.5 }}>
                  {isQmd ? "Quarto 渲染输出" : "R Markdown 渲染输出"}
                </Typography>
                {renderLoading && (
                  <Typography variant="caption" color="text.secondary">
                    {isQmd ? "正在渲染 Quarto 文档…" : "正在调用 Rscript …"}
                  </Typography>
                )}
                {renderErr && (
                  <Typography variant="caption" color="error" display="block" sx={{ whiteSpace: "pre-wrap" }}>
                    {renderErr}
                  </Typography>
                )}
                {renderOut?.stdout ? (
                  <Box component="pre" sx={{ m: 0, whiteSpace: "pre-wrap", wordBreak: "break-word" }}>
                    {renderOut.stdout}
                  </Box>
                ) : null}
                {renderOut?.stderr ? (
                  <Box
                    component="pre"
                    sx={{
                      m: 0,
                      mt: renderOut.stdout ? 1 : 0,
                      whiteSpace: "pre-wrap",
                      wordBreak: "break-word",
                      color: "error.main",
                    }}
                  >
                    {renderOut.stderr}
                  </Box>
                ) : null}
              </Box>
            )}
          </>
        )}
      </Box>

      {/* ── Status bar ── */}
      <Stack
        direction="row"
        alignItems="center"
        gap={1.5}
        sx={{
          height: 24,
          px: 2,
          flexShrink: 0,
          bgcolor: alpha(theme.palette.grey[200], 0.45),
          borderTop: 1,
          borderColor: "divider",
        }}
      >
        <Typography variant="caption" color="text.secondary" sx={{ fontSize: 11 }}>
          {fileName
            ? `${languageLabel} · UTF-8 · ${totalLines} lines`
            : "未打开文件"}
        </Typography>
        {saveError && (
          <Typography variant="caption" color="error" sx={{ fontSize: 11 }} noWrap>
            保存失败: {saveError}
          </Typography>
        )}
      </Stack>
    </Paper>
  );
}
