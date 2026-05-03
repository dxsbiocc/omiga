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
import { alpha, useTheme } from "@mui/material/styles";
import {
  CloudDownloadRounded,
  DeleteOutlineRounded,
  ExtensionRounded,
  ExpandMoreRounded,
  RefreshRounded,
} from "@mui/icons-material";
import {
  flattenMarketplacePlugins,
  type PluginProcessPoolRouteStatus,
  type PluginMarketplaceEntry,
  type PluginRetrievalLifecycleState,
  type PluginRetrievalRouteStatus,
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

const capabilityLabel = (value: string): string =>
  value
    .replace(/[-_]+/g, " ")
    .replace(/\b\w/g, (char) => char.toUpperCase());

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

function retrievalStateColor(
  state: PluginRetrievalLifecycleState,
): "success" | "warning" | "error" | "default" {
  switch (state) {
    case "healthy":
      return "success";
    case "degraded":
      return "warning";
    case "quarantined":
      return "error";
    default:
      return "default";
  }
}

function formatDuration(ms: number): string {
  if (ms <= 0) return "0s";
  const seconds = Math.max(1, Math.ceil(ms / 1000));
  if (seconds < 60) return `${seconds}s`;
  const minutes = Math.ceil(seconds / 60);
  return `${minutes}m`;
}

function retrievalStatusLabel(status: PluginRetrievalRouteStatus): string {
  const route = `${capabilityLabel(status.category)}:${status.sourceId}`;
  if (status.state === "quarantined") {
    return `${route} · Quarantined ${formatDuration(status.remainingMs)}`;
  }
  if (status.state === "degraded") {
    return `${route} · ${status.consecutiveFailures} failures`;
  }
  return `${route} · Healthy`;
}

function processPoolStatusLabel(status: PluginProcessPoolRouteStatus): string {
  return `${capabilityLabel(status.category)}:${status.sourceId} · idle ${formatDuration(status.remainingMs)}`;
}

function PluginCard({
  plugin,
  retrievalStatuses = [],
  installedView,
  busy,
  onInstall,
  onUninstall,
  onToggle,
}: {
  plugin: PluginSummary;
  retrievalStatuses?: PluginRetrievalRouteStatus[];
  installedView?: boolean;
  busy: boolean;
  onInstall: (plugin: PluginSummary) => void;
  onUninstall: (plugin: PluginSummary) => void;
  onToggle: (plugin: PluginSummary, enabled: boolean) => void;
}) {
  const chips = capabilityChips(plugin);
  const installable = plugin.installPolicy !== "NOT_AVAILABLE";
  const theme = useTheme();
  const isActive = plugin.installed && plugin.enabled;
  const tone = isActive ? theme.palette.primary.main : theme.palette.text.secondary;
  const surface = alpha(tone, theme.palette.mode === "dark" ? 0.16 : 0.08);
  const border = alpha(tone, theme.palette.mode === "dark" ? 0.42 : 0.24);
  return (
    <Paper
      variant="outlined"
      sx={{
        p: 2,
        borderRadius: 2.5,
        borderColor: isActive ? border : "divider",
        bgcolor: isActive ? surface : "background.paper",
        transition: "border-color 160ms ease, box-shadow 160ms ease, transform 160ms ease",
        "@media (prefers-reduced-motion: reduce)": {
          transition: "none",
        },
        "&:hover": {
          borderColor: border,
          boxShadow: `0 10px 28px ${alpha(tone, theme.palette.mode === "dark" ? 0.16 : 0.12)}`,
          transform: "translateY(-1px)",
        },
      }}
    >
      <Stack spacing={1.4}>
        <Stack
          direction={{ xs: "column", sm: "row" }}
          spacing={1.5}
          alignItems={{ xs: "stretch", sm: "flex-start" }}
        >
          <Box
            sx={{
              width: 40,
              height: 40,
              borderRadius: 2,
              display: { xs: "none", sm: "inline-flex" },
              alignItems: "center",
              justifyContent: "center",
              color: tone,
              bgcolor: alpha(tone, theme.palette.mode === "dark" ? 0.22 : 0.11),
              border: `1px solid ${alpha(tone, 0.24)}`,
              flexShrink: 0,
            }}
          >
            <ExtensionRounded fontSize="small" />
          </Box>
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
            {plugin.installed && plugin.enabled ? (
              <Typography variant="caption" color="primary" display="block" sx={{ mt: 0.35, fontWeight: 600 }}>
                Type @plugin: in chat to target this plugin for one turn.
              </Typography>
            ) : null}
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
              <Chip key={chip} size="small" variant="outlined" label={capabilityLabel(chip)} />
            ))}
          </Stack>
        )}

        {retrievalStatuses.length > 0 && (
          <Stack spacing={0.6}>
            <Typography variant="caption" color="text.secondary" fontWeight={700}>
              Retrieval routes
            </Typography>
            <Stack direction="row" gap={0.75} flexWrap="wrap">
              {retrievalStatuses.map((status) => (
                <Chip
                  key={`${status.category}:${status.sourceId}`}
                  size="small"
                  color={retrievalStateColor(status.state)}
                  variant={status.state === "healthy" ? "outlined" : "filled"}
                  label={retrievalStatusLabel(status)}
                  title={status.lastError || undefined}
                />
              ))}
            </Stack>
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
  const {
    marketplaces,
    retrievalStatuses,
    processPoolStatuses,
    isLoading,
    isMutating,
    error,
    loadPlugins,
    clearProcessPool,
    installPlugin,
    uninstallPlugin,
    setPluginEnabled,
  } = usePluginStore();
  const [message, setMessage] = useState<string | null>(null);
  const projectRoot = projectPath.trim() || undefined;
  const theme = useTheme();

  useEffect(() => {
    void loadPlugins(projectRoot);
  }, [loadPlugins, projectRoot]);

  const allPlugins = useMemo(() => flattenMarketplacePlugins(marketplaces), [marketplaces]);
  const pluginsById = useMemo(() => {
    const byId = new Map<string, PluginSummary>();
    for (const plugin of allPlugins) {
      byId.set(plugin.id, plugin);
    }
    return byId;
  }, [allPlugins]);
  const installedPlugins = useMemo(
    () => allPlugins.filter((plugin) => plugin.installed),
    [allPlugins],
  );
  const activePlugins = useMemo(
    () => installedPlugins.filter((plugin) => plugin.enabled),
    [installedPlugins],
  );
  const availablePlugins = useMemo(
    () => allPlugins.filter((plugin) => !plugin.installed),
    [allPlugins],
  );
  const retrievalStatusesByPlugin = useMemo(() => {
    const grouped = new Map<string, PluginRetrievalRouteStatus[]>();
    for (const status of retrievalStatuses) {
      const current = grouped.get(status.pluginId) ?? [];
      current.push(status);
      grouped.set(status.pluginId, current);
    }
    return grouped;
  }, [retrievalStatuses]);
  const quarantinedRouteCount = useMemo(
    () => retrievalStatuses.filter((status) => status.quarantined).length,
    [retrievalStatuses],
  );
  const processPoolStatusesByPlugin = useMemo(() => {
    const grouped = new Map<string, PluginProcessPoolRouteStatus[]>();
    for (const status of processPoolStatuses) {
      const current = grouped.get(status.pluginId) ?? [];
      current.push(status);
      grouped.set(status.pluginId, current);
    }
    return grouped;
  }, [processPoolStatuses]);
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

  const handleClearProcessPool = async () => {
    setMessage(null);
    try {
      const cleared = await clearProcessPool(projectRoot);
      setMessage(`Cleared ${cleared} pooled plugin process${cleared === 1 ? "" : "es"}`);
    } catch {
      // Store exposes the error banner.
    }
  };

  return (
    <Stack spacing={2}>
      <Paper
        variant="outlined"
        sx={{
          p: { xs: 2, md: 2.5 },
          borderRadius: 3,
          overflow: "hidden",
          position: "relative",
          bgcolor: alpha(theme.palette.primary.main, theme.palette.mode === "dark" ? 0.12 : 0.05),
          borderColor: alpha(theme.palette.primary.main, theme.palette.mode === "dark" ? 0.32 : 0.16),
          "&:before": {
            content: '""',
            position: "absolute",
            inset: 0,
            pointerEvents: "none",
            background: `radial-gradient(circle at top right, ${alpha(theme.palette.primary.main, 0.16)}, transparent 42%)`,
          },
        }}
      >
        <Stack spacing={2} sx={{ position: "relative" }}>
          <Stack direction={{ xs: "column", md: "row" }} spacing={2} alignItems={{ xs: "stretch", md: "center" }}>
            <Stack direction="row" spacing={1.25} alignItems="flex-start" sx={{ flex: 1, minWidth: 0 }}>
              <Box
                sx={{
                  width: 44,
                  height: 44,
                  borderRadius: 2.5,
                  display: "inline-flex",
                  alignItems: "center",
                  justifyContent: "center",
                  color: "primary.main",
                  bgcolor: alpha(theme.palette.primary.main, theme.palette.mode === "dark" ? 0.22 : 0.12),
                  border: `1px solid ${alpha(theme.palette.primary.main, 0.28)}`,
                  flexShrink: 0,
                }}
              >
                <ExtensionRounded />
              </Box>
              <Box sx={{ minWidth: 0 }}>
                <Typography variant="h6" fontWeight={800} sx={{ lineHeight: 1.2 }}>
                  Omiga native plugins
                </Typography>
                <Typography variant="body2" color="text.secondary" sx={{ mt: 0.5, maxWidth: 880 }}>
                  Plugins add Skills, MCP server configs, app connector references, and composer metadata.
                  They do not run VS Code extension code or require a VS Code Extension Host.
                </Typography>
              </Box>
            </Stack>
            <Button
              variant="outlined"
              startIcon={isLoading ? <CircularProgress size={16} /> : <RefreshRounded />}
              disabled={isLoading || isMutating}
              onClick={() => void loadPlugins(projectRoot)}
              sx={{ textTransform: "none", borderRadius: 2, minHeight: 40, alignSelf: { xs: "flex-start", md: "center" } }}
            >
              Refresh
            </Button>
          </Stack>
          <Stack direction="row" spacing={1} flexWrap="wrap" useFlexGap>
            {[
              ["Available", activePlugins.length],
              ["Installed", installedPlugins.length],
              ["Installable", availablePlugins.length],
              ["Retrieval routes", retrievalStatuses.length],
              ["Quarantined", quarantinedRouteCount],
              ["Pooled processes", processPoolStatuses.length],
              ["Marketplaces", marketplaces.length],
            ].map(([label, value]) => (
              <Chip
                key={label}
                label={`${value} ${label}`}
                variant="outlined"
                sx={{
                  height: 28,
                  bgcolor: alpha(theme.palette.background.paper, theme.palette.mode === "dark" ? 0.45 : 0.75),
                  borderColor: alpha(theme.palette.primary.main, 0.2),
                  fontWeight: 700,
                }}
              />
            ))}
          </Stack>
        </Stack>
      </Paper>

      {(error || message) && (
        <Alert severity={error ? "error" : "success"} sx={{ borderRadius: 2 }}>
          {error || message}
        </Alert>
      )}

      <Alert severity="info" sx={{ borderRadius: 2 }}>
        Install and enable a plugin, then type <strong>@plugin:</strong> in the chat composer to target a plugin,
        or <strong>@</strong> to browse plugins and workspace files together.
      </Alert>

      <Accordion disableGutters elevation={0} sx={accordionSx}>
        <AccordionSummary expandIcon={<ExpandMoreRounded />} sx={{ px: 2, minHeight: 56, "& .MuiAccordionSummary-content": { my: 1.25 } }}>
          <Stack direction="row" spacing={1} alignItems="center" flexWrap="wrap">
            <Typography variant="subtitle2" fontWeight={700}>Retrieval plugin process pool</Typography>
            <Chip size="small" variant="outlined" label={`${processPoolStatuses.length} pooled`} />
          </Stack>
        </AccordionSummary>
        <AccordionDetails sx={{ px: 2, pt: 0, pb: 2 }}>
          <Stack spacing={1.5}>
            <Stack direction={{ xs: "column", sm: "row" }} spacing={1.5} alignItems={{ xs: "stretch", sm: "center" }}>
              <Typography variant="body2" color="text.secondary" sx={{ flex: 1 }}>
                Successful local retrieval plugin calls can keep a child process warm until its idle TTL expires.
                Cancelled, timed out, or failed plugin calls are discarded instead of returning to this pool.
              </Typography>
              <Button
                size="small"
                color="warning"
                variant="outlined"
                startIcon={isMutating ? <CircularProgress size={16} /> : <DeleteOutlineRounded />}
                disabled={isMutating || processPoolStatuses.length === 0}
                onClick={() => void handleClearProcessPool()}
                sx={{ textTransform: "none", borderRadius: 1.5, alignSelf: { xs: "flex-start", sm: "center" } }}
              >
                Clear pool
              </Button>
            </Stack>
            {processPoolStatuses.length === 0 ? (
              <Paper variant="outlined" sx={{ p: 2.5, borderRadius: 2, textAlign: "center" }}>
                <Typography variant="body2" color="text.secondary">
                  No retrieval plugin child process is currently pooled.
                </Typography>
              </Paper>
            ) : (
              Array.from(processPoolStatusesByPlugin.entries())
                .sort(([left], [right]) => left.localeCompare(right))
                .map(([pluginId, statuses]) => {
                  const plugin = pluginsById.get(pluginId);
                  return (
                    <Paper key={pluginId} variant="outlined" sx={{ p: 1.5, borderRadius: 2 }}>
                      <Stack spacing={1}>
                        <Stack direction="row" spacing={1} alignItems="center" flexWrap="wrap">
                          <Typography variant="subtitle2" fontWeight={700}>
                            {plugin ? displayName(plugin) : pluginId}
                          </Typography>
                          <Chip size="small" variant="outlined" label={`${statuses.length} active`} />
                        </Stack>
                        <Stack direction="row" gap={0.75} flexWrap="wrap">
                          {statuses.map((status) => (
                            <Chip
                              key={`${status.category}:${status.sourceId}:${status.pluginRoot}`}
                              size="small"
                              color="info"
                              variant="outlined"
                              label={processPoolStatusLabel(status)}
                              title={`${status.route}\n${status.pluginRoot}`}
                            />
                          ))}
                        </Stack>
                      </Stack>
                    </Paper>
                  );
                })
            )}
          </Stack>
        </AccordionDetails>
      </Accordion>

      <Accordion defaultExpanded disableGutters elevation={0} sx={accordionSx}>
        <AccordionSummary expandIcon={<ExpandMoreRounded />} sx={{ px: 2, minHeight: 56, "& .MuiAccordionSummary-content": { my: 1.25 } }}>
          <Stack direction="row" spacing={1} alignItems="center" flexWrap="wrap">
            <Typography variant="subtitle2" fontWeight={700}>Available Omiga plugins</Typography>
            <Chip size="small" variant="outlined" label={`${activePlugins.length} available`} />
          </Stack>
        </AccordionSummary>
        <AccordionDetails sx={{ px: 2, pt: 0, pb: 2 }}>
          <Box sx={pluginListSx} role="region" aria-label="Available Omiga plugins list">
            {activePlugins.length === 0 ? (
              <Paper variant="outlined" sx={{ p: 3, borderRadius: 2, textAlign: "center" }}>
                <ExtensionRounded sx={{ color: "text.secondary", mb: 1 }} />
                <Typography variant="body2" color="text.secondary">
                  No plugins are currently available to the app. Install and enable a plugin to make its capabilities usable in chat.
                </Typography>
              </Paper>
            ) : (
              activePlugins.map((plugin) => (
                <PluginCard
                  key={plugin.id}
                  plugin={plugin}
                  retrievalStatuses={retrievalStatusesByPlugin.get(plugin.id)}
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

      <Accordion defaultExpanded disableGutters elevation={0} sx={accordionSx}>
        <AccordionSummary expandIcon={<ExpandMoreRounded />} sx={{ px: 2, minHeight: 56, "& .MuiAccordionSummary-content": { my: 1.25 } }}>
          <Stack direction="row" spacing={1} alignItems="center" flexWrap="wrap">
            <Typography variant="subtitle2" fontWeight={700}>Recommended plugins</Typography>
            <Chip size="small" variant="outlined" label={`${availablePlugins.length} installable`} />
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
                      retrievalStatuses={retrievalStatusesByPlugin.get(plugin.id)}
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
                  retrievalStatuses={retrievalStatusesByPlugin.get(plugin.id)}
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
