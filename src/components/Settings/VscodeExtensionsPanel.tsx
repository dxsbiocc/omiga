import { useEffect, useMemo, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import {
  Accordion,
  AccordionDetails,
  AccordionSummary,
  Alert,
  Box,
  Button,
  Chip,
  CircularProgress,
  Divider,
  FormControl,
  InputLabel,
  Link,
  MenuItem,
  Paper,
  Select,
  Stack,
  Typography,
} from "@mui/material";
import {
  CloudDownloadRounded,
  DeleteOutlineRounded,
  ExtensionRounded,
  ExpandMoreRounded,
  OpenInNewRounded,
  RefreshRounded,
  UploadFileRounded,
} from "@mui/icons-material";
import {
  DEFAULT_ICON_THEME_ID,
  contributionSummary,
  getCustomEditorContributions,
  getNotebookContributions,
  getNotebookRuntimeContributions,
  isExtensionInstalled,
} from "../../utils/vscodeExtensions";
import { useExtensionStore } from "../../state/extensionStore";

function contributionChips(summary: ReturnType<typeof contributionSummary>) {
  return [
    { label: `${summary.iconThemes} icon themes`, show: summary.iconThemes > 0 },
    { label: `${summary.languages} languages`, show: summary.languages > 0 },
    { label: `${summary.customEditors} renderers`, show: summary.customEditors > 0 },
    { label: `${summary.notebooks} notebooks`, show: summary.notebooks > 0 },
    {
      label: `${summary.notebookRenderers} notebook renderers`,
      show: summary.notebookRenderers > 0,
    },
    {
      label: `${summary.notebookPreloads} notebook preloads`,
      show: summary.notebookPreloads > 0,
    },
  ].filter((chip) => chip.show);
}

const extensionListSx = {
  maxHeight: { xs: 420, md: 520 },
  overflowY: "auto",
  overscrollBehavior: "contain",
  pr: 0.5,
  display: "flex",
  flexDirection: "column",
  gap: 1.5,
  scrollbarGutter: "stable",
};

const accordionSx = {
  border: 1,
  borderColor: "divider",
  borderRadius: 2,
  overflow: "hidden",
  "&:before": { display: "none" },
  "&.Mui-expanded": { my: 0 },
};

export function VscodeExtensionsPanel() {
  const {
    extensionsDir,
    installedExtensions,
    recommendedExtensions,
    iconThemes,
    activeIconThemeId,
    isLoading,
    isInstalling,
    error,
    loadExtensions,
    installVsix,
    installRecommendedExtension,
    uninstallExtension,
    setActiveIconTheme,
  } = useExtensionStore();
  const [message, setMessage] = useState<string | null>(null);
  const [installingRecommendedId, setInstallingRecommendedId] = useState<string | null>(null);

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
  const notebookRuntimeCount = useMemo(
    () => getNotebookRuntimeContributions(installedExtensions).length,
    [installedExtensions],
  );
  const usableRecommendedCount = useMemo(
    () => recommendedExtensions.filter((extension) => extension.installableNow).length,
    [recommendedExtensions],
  );
  const sortedRecommendedExtensions = useMemo(
    () =>
      [...recommendedExtensions].sort((a, b) => {
        if (a.installableNow !== b.installableNow) return a.installableNow ? -1 : 1;
        return a.displayName.localeCompare(b.displayName);
      }),
    [recommendedExtensions],
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

  const handleInstallRecommended = async (extensionId: string) => {
    setMessage(null);
    setInstallingRecommendedId(extensionId);
    try {
      const installed = await installRecommendedExtension(extensionId);
      setMessage(`Installed ${installed.displayName || installed.id}`);
    } catch {
      // Store already exposes the error banner.
    } finally {
      setInstallingRecommendedId(null);
    }
  };

  return (
    <Stack spacing={2}>
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

      {recommendedExtensions.length > 0 && (
        <Accordion defaultExpanded disableGutters elevation={0} sx={accordionSx}>
          <AccordionSummary
            expandIcon={<ExpandMoreRounded />}
            aria-controls="recommended-plugins-content"
            id="recommended-plugins-header"
            sx={{ px: 2, minHeight: 56, "& .MuiAccordionSummary-content": { my: 1.25 } }}
          >
            <Stack direction="row" spacing={1} alignItems="center" flexWrap="wrap">
              <Typography variant="subtitle2" fontWeight={700}>
                Recommended plugins
              </Typography>
              <Chip size="small" variant="outlined" label={`${recommendedExtensions.length} plugins`} />
              <Chip size="small" color="success" variant="outlined" label={`${usableRecommendedCount} useful now`} />
            </Stack>
          </AccordionSummary>
          <AccordionDetails sx={{ px: 2, pt: 0, pb: 2 }}>
            <Box sx={extensionListSx} role="region" aria-label="Recommended plugins list">
              {sortedRecommendedExtensions.map((extension) => {
                const installed = isExtensionInstalled(installedExtensions, extension.id);
                const installing = installingRecommendedId === extension.id;
                const blockedUntilExtensionHost = !extension.installableNow && !installed;
                return (
                  <Paper key={extension.id} variant="outlined" sx={{ p: 2, borderRadius: 2 }}>
                    <Stack spacing={1.25}>
                      <Stack direction={{ xs: "column", sm: "row" }} spacing={1.5} alignItems={{ xs: "stretch", sm: "flex-start" }}>
                        <ExtensionRounded color="primary" sx={{ mt: 0.25, display: { xs: "none", sm: "block" } }} />
                        <Box sx={{ minWidth: 0, flex: 1 }}>
                          <Stack direction="row" gap={0.75} alignItems="center" flexWrap="wrap">
                            <Typography variant="subtitle2" fontWeight={700}>
                              {extension.displayName}
                            </Typography>
                            <Chip
                              size="small"
                              label={
                                installed
                                  ? "Installed"
                                  : extension.installableNow
                                    ? "Useful now"
                                    : "Needs Extension Host"
                              }
                              color={
                                installed
                                  ? "success"
                                  : extension.installableNow
                                    ? "primary"
                                    : "warning"
                              }
                              variant={installed ? "filled" : "outlined"}
                            />
                          </Stack>
                          <Typography variant="caption" color="text.secondary" display="block">
                            {extension.id}
                          </Typography>
                        </Box>
                        <Button
                          size="small"
                          variant={installed ? "outlined" : "contained"}
                          disableElevation
                          startIcon={installing ? <CircularProgress size={16} color="inherit" /> : <CloudDownloadRounded />}
                          disabled={installed || isInstalling || blockedUntilExtensionHost}
                          onClick={() => void handleInstallRecommended(extension.id)}
                          sx={{ textTransform: "none", borderRadius: 1.5 }}
                        >
                          {installed
                            ? "Installed"
                            : blockedUntilExtensionHost
                              ? "Needs Extension Host"
                              : installing
                                ? "Installing…"
                                : "Install plugin"}
                        </Button>
                      </Stack>

                      <Typography variant="body2" color="text.secondary">
                        {extension.description}
                      </Typography>
                      <Alert
                        severity={extension.installableNow ? "info" : "warning"}
                        variant="outlined"
                        sx={{ py: 0.5, borderRadius: 1.5 }}
                      >
                        <Typography variant="caption" color="text.secondary">
                          {extension.supportNote}
                        </Typography>
                      </Alert>
                      <Typography variant="caption" color="text.secondary">
                        Omiga 会下载安装官方 VSIX，并读取标准
                        <code> package.json </code>贡献点；当前不会运行 VS Code
                        extension host。
                      </Typography>

                      <Stack direction="row" gap={1.25} flexWrap="wrap">
                        <Link
                          href={extension.repositoryUrl}
                          target="_blank"
                          rel="noopener noreferrer"
                          underline="hover"
                          variant="caption"
                          sx={{ display: "inline-flex", alignItems: "center", gap: 0.25 }}
                        >
                          GitHub <OpenInNewRounded sx={{ fontSize: 14 }} />
                        </Link>
                        <Link
                          href={extension.marketplaceUrl}
                          target="_blank"
                          rel="noopener noreferrer"
                          underline="hover"
                          variant="caption"
                          sx={{ display: "inline-flex", alignItems: "center", gap: 0.25 }}
                        >
                          Marketplace <OpenInNewRounded sx={{ fontSize: 14 }} />
                        </Link>
                      </Stack>
                    </Stack>
                  </Paper>
                );
              })}
            </Box>
          </AccordionDetails>
        </Accordion>
      )}

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
            {customEditorCount}. Notebook file associations: {notebookCount}. Notebook
            renderer/preload metadata: {notebookRuntimeCount}.
          </Typography>
        </Stack>
      </Paper>

      <Accordion defaultExpanded disableGutters elevation={0} sx={accordionSx}>
        <AccordionSummary
          expandIcon={<ExpandMoreRounded />}
          aria-controls="installed-extensions-content"
          id="installed-extensions-header"
          sx={{ px: 2, minHeight: 56, "& .MuiAccordionSummary-content": { my: 1.25 } }}
        >
          <Stack direction="row" spacing={1} alignItems="center" flexWrap="wrap">
            <Typography variant="subtitle2" fontWeight={700}>
              Installed VS Code extensions
            </Typography>
            <Chip size="small" variant="outlined" label={`${installedExtensions.length} installed`} />
          </Stack>
        </AccordionSummary>
        <AccordionDetails sx={{ px: 2, pt: 0, pb: 2 }}>
          <Box sx={extensionListSx} role="region" aria-label="Installed VS Code extensions list">
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
          </Box>
        </AccordionDetails>
      </Accordion>
    </Stack>
  );
}
