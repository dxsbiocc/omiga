import { useEffect, useTransition } from "react";
import {
  Box,
  Typography,
  Stack,
  Paper,
  useTheme,
  alpha,
  CircularProgress,
  IconButton,
  Tooltip,
} from "@mui/material";
import SaveRoundedIcon from "@mui/icons-material/SaveRounded";
import CloseRoundedIcon from "@mui/icons-material/CloseRounded";
import { useWorkspaceStore } from "../../state/workspaceStore";
import { FileRenderer } from "./FileRenderer";
import { extToLabel } from "./CodeViewer";

/**
 * Code editor region — shows file content from workspace store (opened from file tree).
 * Supports editing + saving via Monaco Editor.
 */
export function CodeWorkspace() {
  const theme = useTheme();
  const isDark = theme.palette.mode === "dark";
  const toolbarStripe = isDark
    ? alpha(theme.palette.background.default, 0.92)
    : alpha(theme.palette.grey[100], 0.85);
  const statusBarBg = isDark
    ? alpha(theme.palette.common.white, 0.06)
    : alpha(theme.palette.grey[200], 0.45);
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

  // Use transition for closing file to avoid blocking UI during Monaco cleanup
  const [isClosing, startClosing] = useTransition();

  const fileExt = fileName ? (fileName.split(".").pop() ?? "").toLowerCase() : "";
  const languageLabel = fileExt ? extToLabel(fileExt) : "Plain Text";
  const isReadOnly = ["png","jpg","jpeg","gif","webp","svg","bmp","ico","tiff","tif","avif","pdf"].includes(fileExt);
  const canSave = Boolean(filePath && !isReadOnly && isDirty && !isSaving);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (!(event.metaKey || event.ctrlKey) || event.key.toLowerCase() !== "s") return;
      if (!filePath || isReadOnly) return;
      event.preventDefault();
      if (!isDirty || isSaving) return;
      void saveFile();
    };

    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
  }, [filePath, isReadOnly, isDirty, isSaving, saveFile]);

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
          bgcolor: toolbarStripe,
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
              <span>
                <IconButton
                  size="small"
                  aria-label="关闭文件"
                  disabled={isClosing}
                  onClick={() => startClosing(() => clearFile())}
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
              </span>
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
            <Tooltip title={isDirty ? "保存文件 (⌘/Ctrl+S)" : "已保存"}>
              <span>
                <IconButton
                  size="small"
                  aria-label="保存文件"
                  disabled={!canSave}
                  onClick={() => void saveFile()}
                  sx={{
                    width: 30,
                    height: 30,
                    borderRadius: 1.5,
                    color: isDirty ? "warning.main" : "text.secondary",
                    bgcolor: isDirty
                      ? alpha(theme.palette.warning.main, isDark ? 0.14 : 0.08)
                      : "transparent",
                    "&:hover": {
                      bgcolor: alpha(theme.palette.warning.main, isDark ? 0.22 : 0.14),
                    },
                  }}
                >
                  {isSaving ? (
                    <CircularProgress size={15} color="inherit" />
                  ) : (
                    <SaveRoundedIcon sx={{ fontSize: 17 }} />
                  )}
                </IconButton>
              </span>
            </Tooltip>
          )}
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
        ) : (
          <>
            {/* Always render FileRenderer but hide when no file to avoid Monaco re-mount penalty */}
            <Box
              sx={{
                flex: 1,
                minHeight: 0,
                display: filePath ? "flex" : "none",
                flexDirection: "column",
                overflow: "hidden",
              }}
            >
              <FileRenderer
                fileName={fileName ?? ""}
                filePath={filePath ?? ""}
                content={content}
                onChange={setContent}
              />
            </Box>
            {!filePath && (
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
          bgcolor: statusBarBg,
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
