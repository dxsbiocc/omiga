import { useEffect, useMemo, useState } from "react";
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
  Paper,
  Stack,
  Switch,
  Typography,
} from "@mui/material";
import {
  CloudDownloadRounded,
  DeleteOutlineRounded,
  ExtensionRounded,
  ExpandMoreRounded,
  RefreshRounded,
} from "@mui/icons-material";
import {
  flattenMarketplacePlugins,
  type PluginMarketplaceEntry,
  type PluginSummary,
  usePluginStore,
} from "../../state/pluginStore";

const pluginListSx = {
  maxHeight: { xs: 420, md: 560 },
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

function displayName(plugin: PluginSummary): string {
  return plugin.interface?.displayName || plugin.name;
}

function description(plugin: PluginSummary): string {
  return (
    plugin.interface?.shortDescription ||
    plugin.interface?.longDescription ||
    "Omiga-native plugin bundle."
  );
}

function marketplaceLabel(marketplace: PluginMarketplaceEntry): string {
  return marketplace.interface?.displayName || marketplace.name;
}

function capabilityChips(plugin: PluginSummary) {
  const caps = plugin.interface?.capabilities ?? [];
  const category = plugin.interface?.category;
  return Array.from(new Set([category, ...caps].filter(Boolean) as string[])).slice(0, 6);
}

function PluginCard({
  plugin,
  installedView,
  busy,
  onInstall,
  onUninstall,
  onToggle,
}: {
  plugin: PluginSummary;
  installedView?: boolean;
  busy: boolean;
  onInstall: (plugin: PluginSummary) => void;
  onUninstall: (plugin: PluginSummary) => void;
  onToggle: (plugin: PluginSummary, enabled: boolean) => void;
}) {
  const chips = capabilityChips(plugin);
  const installable = plugin.installPolicy !== "NOT_AVAILABLE";
  return (
    <Paper variant="outlined" sx={{ p: 2, borderRadius: 2 }}>
      <Stack spacing={1.25}>
        <Stack direction={{ xs: "column", sm: "row" }} spacing={1.5} alignItems={{ xs: "stretch", sm: "flex-start" }}>
          <ExtensionRounded color={plugin.enabled ? "primary" : "disabled"} sx={{ mt: 0.25, display: { xs: "none", sm: "block" } }} />
          <Box sx={{ flex: 1, minWidth: 0 }}>
            <Stack direction="row" gap={0.75} alignItems="center" flexWrap="wrap">
              <Typography variant="subtitle2" fontWeight={700}>
                {displayName(plugin)}
              </Typography>
              <Chip
                size="small"
                label={plugin.installed ? (plugin.enabled ? "Enabled" : "Disabled") : "Available"}
                color={plugin.installed ? (plugin.enabled ? "success" : "default") : "primary"}
                variant={plugin.installed && plugin.enabled ? "filled" : "outlined"}
              />
              {plugin.authPolicy === "ON_INSTALL" && (
                <Chip size="small" label="Auth on install" variant="outlined" />
              )}
            </Stack>
            <Typography variant="caption" color="text.secondary" display="block" noWrap title={plugin.id}>
              {plugin.id}
            </Typography>
          </Box>
          {plugin.installed ? (
            <Stack direction="row" spacing={1} alignItems="center" justifyContent="flex-end">
              <Switch
                size="small"
                checked={plugin.enabled}
                disabled={busy}
                onChange={(event) => onToggle(plugin, event.target.checked)}
              />
              <Button
                size="small"
                color="error"
                variant="text"
                startIcon={<DeleteOutlineRounded />}
                disabled={busy}
                onClick={() => onUninstall(plugin)}
                sx={{ textTransform: "none", borderRadius: 1.5 }}
              >
                Uninstall
              </Button>
            </Stack>
          ) : (
            <Button
              size="small"
              variant="contained"
              disableElevation
              startIcon={busy ? <CircularProgress size={16} color="inherit" /> : <CloudDownloadRounded />}
              disabled={busy || !installable}
              onClick={() => onInstall(plugin)}
              sx={{ textTransform: "none", borderRadius: 1.5 }}
            >
              {installable ? "Install plugin" : "Unavailable"}
            </Button>
          )}
        </Stack>

        <Typography variant="body2" color="text.secondary">
          {description(plugin)}
        </Typography>

        {chips.length > 0 && (
          <Stack direction="row" gap={0.75} flexWrap="wrap">
            {chips.map((chip) => (
              <Chip key={chip} size="small" variant="outlined" label={chip} />
            ))}
          </Stack>
        )}

        {plugin.interface?.defaultPrompt?.length ? (
          <Stack spacing={0.5}>
            <Typography variant="caption" color="text.secondary" fontWeight={700}>
              Starter prompts
            </Typography>
            {plugin.interface.defaultPrompt.map((prompt) => (
              <Typography key={prompt} variant="caption" color="text.secondary">
                • {prompt}
              </Typography>
            ))}
          </Stack>
        ) : null}

        <Divider />
        <Typography variant="caption" color="text.secondary" sx={{ wordBreak: "break-all" }}>
          {installedView && plugin.installedPath ? plugin.installedPath : plugin.sourcePath}
        </Typography>
      </Stack>
    </Paper>
  );
}

export function PluginsPanel({ projectPath }: { projectPath: string }) {
  const { marketplaces, isLoading, isMutating, error, loadPlugins, installPlugin, uninstallPlugin, setPluginEnabled } = usePluginStore();
  const [message, setMessage] = useState<string | null>(null);
  const projectRoot = projectPath.trim() || undefined;

  useEffect(() => {
    void loadPlugins(projectRoot);
  }, [loadPlugins, projectRoot]);

  const allPlugins = useMemo(() => flattenMarketplacePlugins(marketplaces), [marketplaces]);
  const installedPlugins = useMemo(
    () => allPlugins.filter((plugin) => plugin.installed),
    [allPlugins],
  );
  const availablePlugins = useMemo(
    () => allPlugins.filter((plugin) => !plugin.installed),
    [allPlugins],
  );
  const availableMarketplaces = useMemo(() => {
    const seen = new Set<string>();
    return marketplaces
      .map((marketplace) => ({
        marketplace,
        plugins: marketplace.plugins.filter((plugin) => {
          if (plugin.installed || seen.has(plugin.id)) return false;
          seen.add(plugin.id);
          return true;
        }),
      }))
      .filter(({ plugins }) => plugins.length > 0);
  }, [marketplaces]);

  const handleInstall = async (plugin: PluginSummary) => {
    setMessage(null);
    try {
      await installPlugin(plugin, projectRoot);
      setMessage(`Installed ${displayName(plugin)}`);
    } catch {
      // Store exposes the error banner.
    }
  };

  const handleUninstall = async (plugin: PluginSummary) => {
    if (!window.confirm(`Uninstall ${displayName(plugin)}?`)) return;
    setMessage(null);
    try {
      await uninstallPlugin(plugin.id, projectRoot);
      setMessage(`Uninstalled ${displayName(plugin)}`);
    } catch {
      // Store exposes the error banner.
    }
  };

  const handleToggle = async (plugin: PluginSummary, enabled: boolean) => {
    setMessage(null);
    try {
      await setPluginEnabled(plugin.id, enabled, projectRoot);
      setMessage(`${enabled ? "Enabled" : "Disabled"} ${displayName(plugin)}`);
    } catch {
      // Store exposes the error banner.
    }
  };

  return (
    <Stack spacing={2}>
      <Alert severity="info" sx={{ borderRadius: 2 }}>
        Omiga plugins are native capability bundles: Skills, MCP server configs, app connector references, and UI metadata.
        They do not run VS Code extension code or require a VS Code Extension Host.
      </Alert>

      {(error || message) && (
        <Alert severity={error ? "error" : "success"} sx={{ borderRadius: 2 }}>
          {error || message}
        </Alert>
      )}

      <Paper variant="outlined" sx={{ p: 2, borderRadius: 2 }}>
        <Stack direction={{ xs: "column", sm: "row" }} spacing={1.5} alignItems={{ xs: "stretch", sm: "center" }}>
          <Button
            variant="outlined"
            startIcon={isLoading ? <CircularProgress size={16} /> : <RefreshRounded />}
            disabled={isLoading || isMutating}
            onClick={() => void loadPlugins(projectRoot)}
            sx={{ textTransform: "none", borderRadius: 1.5 }}
          >
            Refresh
          </Button>
          <Box sx={{ flex: 1 }} />
          <Typography variant="caption" color="text.secondary">
            {installedPlugins.length} installed · {availablePlugins.length} available · {marketplaces.length} marketplaces
          </Typography>
        </Stack>
      </Paper>

      <Accordion defaultExpanded disableGutters elevation={0} sx={accordionSx}>
        <AccordionSummary expandIcon={<ExpandMoreRounded />} sx={{ px: 2, minHeight: 56, "& .MuiAccordionSummary-content": { my: 1.25 } }}>
          <Stack direction="row" spacing={1} alignItems="center" flexWrap="wrap">
            <Typography variant="subtitle2" fontWeight={700}>Recommended plugins</Typography>
            <Chip size="small" variant="outlined" label={`${availablePlugins.length} available`} />
          </Stack>
        </AccordionSummary>
        <AccordionDetails sx={{ px: 2, pt: 0, pb: 2 }}>
          <Box sx={pluginListSx} role="region" aria-label="Recommended Omiga plugins list">
            {marketplaces.length === 0 || allPlugins.length === 0 ? (
              <Paper variant="outlined" sx={{ p: 3, borderRadius: 2, textAlign: "center" }}>
                <ExtensionRounded sx={{ color: "text.secondary", mb: 1 }} />
                <Typography variant="body2" color="text.secondary">
                  No plugin marketplace found yet. Add one at ~/.omiga/plugins/marketplace.json or project .omiga/plugins/marketplace.json.
                </Typography>
              </Paper>
            ) : availableMarketplaces.length === 0 ? (
              <Paper variant="outlined" sx={{ p: 3, borderRadius: 2, textAlign: "center" }}>
                <ExtensionRounded sx={{ color: "text.secondary", mb: 1 }} />
                <Typography variant="body2" color="text.secondary">
                  All available plugins are installed.
                </Typography>
              </Paper>
            ) : (
              availableMarketplaces.map(({ marketplace, plugins }) => (
                <Stack key={marketplace.path} spacing={1.25}>
                  <Typography variant="caption" color="text.secondary" fontWeight={700}>
                    {marketplaceLabel(marketplace)}
                  </Typography>
                  {plugins.map((plugin) => (
                    <PluginCard
                      key={plugin.id}
                      plugin={plugin}
                      busy={isMutating}
                      onInstall={(p) => void handleInstall(p)}
                      onUninstall={(p) => void handleUninstall(p)}
                      onToggle={(p, enabled) => void handleToggle(p, enabled)}
                    />
                  ))}
                </Stack>
              ))
            )}
          </Box>
        </AccordionDetails>
      </Accordion>

      <Accordion defaultExpanded disableGutters elevation={0} sx={accordionSx}>
        <AccordionSummary expandIcon={<ExpandMoreRounded />} sx={{ px: 2, minHeight: 56, "& .MuiAccordionSummary-content": { my: 1.25 } }}>
          <Stack direction="row" spacing={1} alignItems="center" flexWrap="wrap">
            <Typography variant="subtitle2" fontWeight={700}>Installed Omiga plugins</Typography>
            <Chip size="small" variant="outlined" label={`${installedPlugins.length} installed`} />
          </Stack>
        </AccordionSummary>
        <AccordionDetails sx={{ px: 2, pt: 0, pb: 2 }}>
          <Box sx={pluginListSx} role="region" aria-label="Installed Omiga plugins list">
            {installedPlugins.length === 0 ? (
              <Paper variant="outlined" sx={{ p: 3, borderRadius: 2, textAlign: "center" }}>
                <ExtensionRounded sx={{ color: "text.secondary", mb: 1 }} />
                <Typography variant="body2" color="text.secondary">
                  No Omiga plugins installed yet. Install a recommended plugin to add native skills, MCP servers, or app connectors.
                </Typography>
              </Paper>
            ) : (
              installedPlugins.map((plugin) => (
                <PluginCard
                  key={plugin.id}
                  plugin={plugin}
                  installedView
                  busy={isMutating}
                  onInstall={(p) => void handleInstall(p)}
                  onUninstall={(p) => void handleUninstall(p)}
                  onToggle={(p, enabled) => void handleToggle(p, enabled)}
                />
              ))
            )}
          </Box>
        </AccordionDetails>
      </Accordion>
    </Stack>
  );
}
