import { useEffect, useMemo, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import {
  Alert,
  Box,
  Button,
  Chip,
  CircularProgress,
  Divider,
  FormControl,
  InputLabel,
  MenuItem,
  Paper,
  Select,
  Stack,
  Typography,
} from "@mui/material";
import {
  DeleteOutlineRounded,
  ExtensionRounded,
  RefreshRounded,
  UploadFileRounded,
} from "@mui/icons-material";
import {
  DEFAULT_ICON_THEME_ID,
  contributionSummary,
  getCustomEditorContributions,
  getNotebookContributions,
} from "../../utils/vscodeExtensions";
import { useExtensionStore } from "../../state/extensionStore";

function contributionChips(summary: ReturnType<typeof contributionSummary>) {
  return [
    { label: `${summary.iconThemes} icon themes`, show: summary.iconThemes > 0 },
    { label: `${summary.languages} languages`, show: summary.languages > 0 },
    { label: `${summary.customEditors} renderers`, show: summary.customEditors > 0 },
    { label: `${summary.notebooks} notebooks`, show: summary.notebooks > 0 },
  ].filter((chip) => chip.show);
}

export function VscodeExtensionsPanel() {
  const {
    extensionsDir,
    installedExtensions,
    iconThemes,
    activeIconThemeId,
    isLoading,
    isInstalling,
    error,
    loadExtensions,
    installVsix,
    uninstallExtension,
    setActiveIconTheme,
  } = useExtensionStore();
  const [message, setMessage] = useState<string | null>(null);

  useEffect(() => {
    void loadExtensions();
  }, [loadExtensions]);

  const customEditorCount = useMemo(
    () => getCustomEditorContributions(installedExtensions).length,
    [installedExtensions],
  );
  const notebookCount = useMemo(
    () => getNotebookContributions(installedExtensions).length,
    [installedExtensions],
  );

  const handleInstallVsix = async () => {
    setMessage(null);
    const selected = await open({
      multiple: false,
      directory: false,
      filters: [{ name: "VS Code Extension", extensions: ["vsix"] }],
    });
    if (typeof selected !== "string") return;
    const installed = await installVsix(selected);
    setMessage(`Installed ${installed.displayName || installed.id}`);
  };

  return (
    <Stack spacing={2}>
      <Alert severity="info" sx={{ borderRadius: 2 }}>
        支持安装 VS Code <code>.vsix</code> 包并读取标准{" "}
        <code>package.json</code> 贡献点：图标主题、语言关联、notebook/custom
        editor 文件匹配。图标主题可立即用于文件树；custom editor
        插件会被识别并在文件区显示匹配状态，完整 VS Code Webview/Extension Host
        UI 运行时将作为后续增强。
      </Alert>

      {(error || message) && (
        <Alert severity={error ? "error" : "success"} sx={{ borderRadius: 2 }}>
          {error || message}
        </Alert>
      )}

      <Paper variant="outlined" sx={{ p: 2, borderRadius: 2 }}>
        <Stack direction={{ xs: "column", sm: "row" }} spacing={1.5} alignItems={{ xs: "stretch", sm: "center" }}>
          <Button
            variant="contained"
            disableElevation
            startIcon={isInstalling ? <CircularProgress size={16} color="inherit" /> : <UploadFileRounded />}
            disabled={isInstalling}
            onClick={() => void handleInstallVsix()}
            sx={{ textTransform: "none", borderRadius: 1.5 }}
          >
            Install VSIX
          </Button>
          <Button
            variant="outlined"
            startIcon={isLoading ? <CircularProgress size={16} /> : <RefreshRounded />}
            disabled={isLoading}
            onClick={() => void loadExtensions()}
            sx={{ textTransform: "none", borderRadius: 1.5 }}
          >
            Refresh
          </Button>
          <Box sx={{ flex: 1 }} />
          <Typography variant="caption" color="text.secondary" sx={{ wordBreak: "break-all" }}>
            {extensionsDir ? `Extensions: ${extensionsDir}` : "Extensions directory not loaded"}
          </Typography>
        </Stack>
      </Paper>

      <Paper variant="outlined" sx={{ p: 2, borderRadius: 2 }}>
        <Stack spacing={1.5}>
          <Typography variant="subtitle2" fontWeight={700}>
            File icon theme
          </Typography>
          <FormControl size="small" fullWidth>
            <InputLabel id="vscode-icon-theme-label">Active icon theme</InputLabel>
            <Select
              labelId="vscode-icon-theme-label"
              label="Active icon theme"
              value={activeIconThemeId}
              onChange={(event) => void setActiveIconTheme(event.target.value)}
            >
              <MenuItem value={DEFAULT_ICON_THEME_ID}>Omiga Material Icons (built-in)</MenuItem>
              {iconThemes.map((theme) => (
                <MenuItem key={`${theme.extensionId}:${theme.id}`} value={theme.id}>
                  {theme.label} — {theme.extensionName}
                </MenuItem>
              ))}
            </Select>
          </FormControl>
          <Typography variant="caption" color="text.secondary">
            Installed icon themes: {iconThemes.length}. Custom renderers detected:{" "}
            {customEditorCount}. Notebook contributions: {notebookCount}.
          </Typography>
        </Stack>
      </Paper>

      <Stack spacing={1.5}>
        <Typography variant="subtitle2" fontWeight={700}>
          Installed VS Code extensions
        </Typography>
        {installedExtensions.length === 0 ? (
          <Paper variant="outlined" sx={{ p: 3, borderRadius: 2, textAlign: "center" }}>
            <ExtensionRounded sx={{ color: "text.secondary", mb: 1 }} />
            <Typography variant="body2" color="text.secondary">
              No VS Code extensions installed yet. Install a .vsix package to
              enable compatible static contribution points.
            </Typography>
          </Paper>
        ) : (
          installedExtensions.map((extension) => {
            const summary = contributionSummary(extension);
            const chips = contributionChips(summary);
            return (
              <Paper key={extension.id} variant="outlined" sx={{ p: 2, borderRadius: 2 }}>
                <Stack spacing={1.25}>
                  <Stack direction="row" spacing={1.5} alignItems="flex-start">
                    <ExtensionRounded color="primary" sx={{ mt: 0.25 }} />
                    <Box sx={{ minWidth: 0, flex: 1 }}>
                      <Typography variant="subtitle2" fontWeight={700} noWrap title={extension.displayName}>
                        {extension.displayName || extension.name}
                      </Typography>
                      <Typography variant="caption" color="text.secondary" display="block" noWrap title={extension.id}>
                        {extension.id} · v{extension.version}
                      </Typography>
                    </Box>
                    <Button
                      size="small"
                      color="error"
                      variant="text"
                      startIcon={<DeleteOutlineRounded />}
                      disabled={isLoading}
                      onClick={() => void uninstallExtension(extension.id)}
                      sx={{ textTransform: "none", borderRadius: 1.5 }}
                    >
                      Uninstall
                    </Button>
                  </Stack>

                  {extension.description && (
                    <Typography variant="body2" color="text.secondary">
                      {extension.description}
                    </Typography>
                  )}

                  {chips.length > 0 && (
                    <Stack direction="row" gap={0.75} flexWrap="wrap">
                      {chips.map((chip) => (
                        <Chip key={chip.label} size="small" label={chip.label} variant="outlined" />
                      ))}
                    </Stack>
                  )}

                  <Divider />
                  <Typography variant="caption" color="text.secondary" sx={{ wordBreak: "break-all" }}>
                    {extension.path}
                  </Typography>
                </Stack>
              </Paper>
            );
          })
        )}
      </Stack>
    </Stack>
  );
}
