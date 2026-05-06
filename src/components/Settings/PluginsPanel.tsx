import { useEffect, useMemo, useState, type KeyboardEvent } from "react";
import {
  Accordion,
  AccordionDetails,
  AccordionSummary,
  Alert,
  Box,
  Button,
  Chip,
  CircularProgress,
  Dialog,
  DialogContent,
  DialogTitle,
  IconButton,
  InputAdornment,
  MenuItem,
  Paper,
  Stack,
  Switch,
  TextField,
  Typography,
} from "@mui/material";
import { alpha, useTheme } from "@mui/material/styles";
import {
  AddRounded,
  CheckRounded,
  ClearRounded,
  CloseRounded,
  ContentCopyRounded,
  DeleteOutlineRounded,
  ExtensionRounded,
  ExpandMoreRounded,
  RefreshRounded,
  SearchRounded,
} from "@mui/icons-material";
import {
  buildPluginDiagnostics,
  buildRetrievalRuntimeDiagnostics,
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

const pluginCardGridSx = {
  display: "grid",
  gridTemplateColumns: { xs: "1fr", lg: "repeat(2, minmax(0, 1fr))" },
  gap: 1.5,
};

const accordionSx = {
  border: 1,
  borderColor: "divider",
  borderRadius: 2,
  overflow: "hidden",
  m: 0,
  "&:before": { display: "none" },
  "&.Mui-expanded": { m: 0 },
};

const nestedAccordionSx = {
  border: 0,
  borderRadius: 2,
  overflow: "hidden",
  bgcolor: "action.hover",
  m: 0,
  "&:before": { display: "none" },
  "&.Mui-expanded": { m: 0 },
};

const accordionSummarySx = {
  px: 2,
  minHeight: 56,
  "&.Mui-expanded": { minHeight: 56 },
  "& .MuiAccordionSummary-content": { my: 1.25 },
  "& .MuiAccordionSummary-content.Mui-expanded": { my: 1.25 },
};

const nestedAccordionSummarySx = {
  px: 1.5,
  minHeight: 48,
  "&.Mui-expanded": { minHeight: 48 },
  "& .MuiAccordionSummary-content": { my: 1 },
  "& .MuiAccordionSummary-content.Mui-expanded": { my: 1 },
};

const compactAccordionSummarySx = {
  px: 1.5,
  minHeight: 52,
  "&.Mui-expanded": { minHeight: 52 },
  "& .MuiAccordionSummary-content": { my: 1 },
  "& .MuiAccordionSummary-content.Mui-expanded": { my: 1 },
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

function processPoolStatusLabel(status: PluginProcessPoolRouteStatus): string {
  return `${capabilityLabel(status.category)}:${status.sourceId} · idle ${formatDuration(status.remainingMs)}`;
}

export function retrievalStatusDiagnostic(status: PluginRetrievalRouteStatus): {
  title: string;
  detail: string;
  lastError: string | null;
} {
  const title = status.route || `${status.category}.${status.sourceId}`;
  const lastError = status.lastError?.trim() || null;
  if (status.quarantined) {
    return {
      title,
      detail: `Quarantined for ${formatDuration(status.remainingMs)} after ${status.consecutiveFailures} consecutive failure${status.consecutiveFailures === 1 ? "" : "s"}.`,
      lastError,
    };
  }
  if (status.state === "degraded") {
    return {
      title,
      detail: `${status.consecutiveFailures} recent failure${status.consecutiveFailures === 1 ? "" : "s"} recorded; another failure may quarantine this route.`,
      lastError,
    };
  }
  return {
    title,
    detail: "Healthy. No recent plugin failures recorded for this route.",
    lastError,
  };
}

export function processPoolStatusDiagnostic(status: PluginProcessPoolRouteStatus): {
  title: string;
  detail: string;
  pluginRoot: string;
} {
  return {
    title: status.route || `${status.category}.${status.sourceId}`,
    detail: `Warm child process will idle for ${formatDuration(status.remainingMs)} before shutdown.`,
    pluginRoot: status.pluginRoot,
  };
}

export function unknownRetrievalRuntimePluginIds(
  plugins: PluginSummary[],
  retrievalStatuses: PluginRetrievalRouteStatus[],
  processPoolStatuses: PluginProcessPoolRouteStatus[],
): string[] {
  const knownPluginIds = new Set(plugins.map((plugin) => plugin.id));
  const runtimePluginIds = new Set<string>();
  for (const status of retrievalStatuses) runtimePluginIds.add(status.pluginId);
  for (const status of processPoolStatuses) runtimePluginIds.add(status.pluginId);
  return Array.from(runtimePluginIds)
    .filter((pluginId) => !knownPluginIds.has(pluginId))
    .sort((left, right) => left.localeCompare(right));
}

function isRetrievalPlugin(plugin: PluginSummary): boolean {
  return Boolean(plugin.retrieval?.sources.length);
}

export type PluginCatalogFilter =
  | "all"
  | "available"
  | "installed"
  | "enabled"
  | "data-sources"
  | "general";

const pluginCatalogFilterOptions: Array<{ value: PluginCatalogFilter; label: string }> = [
  { value: "all", label: "All" },
  { value: "available", label: "Available" },
  { value: "installed", label: "Installed" },
  { value: "enabled", label: "Enabled" },
  { value: "data-sources", label: "Data sources" },
  { value: "general", label: "General" },
];

function pluginSearchText(plugin: PluginSummary): string {
  const retrievalText = (plugin.retrieval?.sources ?? [])
    .flatMap((source) => [
      source.id,
      source.category,
      source.label,
      source.description,
      ...source.subcategories,
      ...source.capabilities,
    ])
    .join(" ");
  const interfaceText = plugin.interface
    ? [
        plugin.interface.displayName,
        plugin.interface.shortDescription,
        plugin.interface.longDescription,
        plugin.interface.developerName,
        plugin.interface.category,
        ...plugin.interface.capabilities,
        ...plugin.interface.defaultPrompt,
      ].join(" ")
    : "";
  return [
    plugin.id,
    plugin.name,
    plugin.marketplaceName,
    plugin.sourcePath,
    plugin.installedPath,
    interfaceText,
    retrievalText,
  ]
    .filter(Boolean)
    .join(" ")
    .toLowerCase();
}

function pluginMatchesCatalogFilter(
  plugin: PluginSummary,
  filter: PluginCatalogFilter,
): boolean {
  switch (filter) {
    case "available":
      return !plugin.installed;
    case "installed":
      return plugin.installed;
    case "enabled":
      return plugin.installed && plugin.enabled;
    case "data-sources":
      return isRetrievalPlugin(plugin);
    case "general":
      return !isRetrievalPlugin(plugin);
    default:
      return true;
  }
}

export function filterPluginsForCatalog(
  plugins: PluginSummary[],
  query: string,
  filter: PluginCatalogFilter,
): PluginSummary[] {
  const tokens = query
    .trim()
    .toLowerCase()
    .split(/\s+/)
    .filter(Boolean);

  return plugins.filter((plugin) => {
    if (!pluginMatchesCatalogFilter(plugin, filter)) return false;
    if (tokens.length === 0) return true;
    const haystack = pluginSearchText(plugin);
    return tokens.every((token) => haystack.includes(token));
  });
}

function primaryRetrievalCategory(plugin: PluginSummary): string {
  return plugin.retrieval?.sources[0]?.category || "other";
}

function retrievalCategoryLabel(category: string): string {
  switch (category) {
    case "dataset":
      return "Dataset sources";
    case "literature":
      return "Literature sources";
    case "knowledge":
      return "Knowledge sources";
    default:
      return `${capabilityLabel(category)} sources`;
  }
}

export function pluginRuntimeSummary(
  plugin: PluginSummary,
  retrievalStatuses: PluginRetrievalRouteStatus[] = [],
  processPoolStatuses: PluginProcessPoolRouteStatus[] = [],
): {
  state: PluginRetrievalLifecycleState | "not-installed" | "disabled" | "idle";
  label: string;
  routeCount: number;
  issueCount: number;
  pooledCount: number;
  lastError: string | null;
} {
  if (!plugin.installed) {
    return {
      state: "not-installed",
      label: "Not installed",
      routeCount: plugin.retrieval?.sources.length ?? 0,
      issueCount: 0,
      pooledCount: 0,
      lastError: null,
    };
  }
  if (!plugin.enabled) {
    return {
      state: "disabled",
      label: "Disabled",
      routeCount: plugin.retrieval?.sources.length ?? 0,
      issueCount: 0,
      pooledCount: processPoolStatuses.length,
      lastError: null,
    };
  }

  const issueStatuses = retrievalStatuses.filter(
    (status) => status.state !== "healthy" || status.quarantined || Boolean(status.lastError?.trim()),
  );
  const lastError =
    retrievalStatuses
      .map((status) => status.lastError?.trim())
      .find((value): value is string => Boolean(value)) ?? null;
  if (issueStatuses.some((status) => status.quarantined || status.state === "quarantined")) {
    return {
      state: "quarantined",
      label: "Quarantined",
      routeCount: retrievalStatuses.length || (plugin.retrieval?.sources.length ?? 0),
      issueCount: issueStatuses.length,
      pooledCount: processPoolStatuses.length,
      lastError,
    };
  }
  if (issueStatuses.length > 0) {
    return {
      state: "degraded",
      label: "Needs attention",
      routeCount: retrievalStatuses.length || (plugin.retrieval?.sources.length ?? 0),
      issueCount: issueStatuses.length,
      pooledCount: processPoolStatuses.length,
      lastError,
    };
  }
  if (retrievalStatuses.length === 0 && (plugin.retrieval?.sources.length ?? 0) > 0) {
    return {
      state: "idle",
      label: "No calls yet",
      routeCount: plugin.retrieval?.sources.length ?? 0,
      issueCount: 0,
      pooledCount: processPoolStatuses.length,
      lastError: null,
    };
  }
  return {
    state: "healthy",
    label: "Healthy",
    routeCount: retrievalStatuses.length || (plugin.retrieval?.sources.length ?? 0),
    issueCount: 0,
    pooledCount: processPoolStatuses.length,
    lastError: null,
  };
}

export function pluginCardSubtitle(plugin: PluginSummary): string {
  const sources = plugin.retrieval?.sources ?? [];
  if (sources.length === 1) {
    return sources[0].label || `${capabilityLabel(sources[0].category)} source`;
  }
  if (sources.length > 1) {
    const category = capabilityLabel(sources[0].category);
    return `${sources.length} ${category} routes`;
  }
  return description(plugin);
}

function groupRetrievalPlugins(plugins: PluginSummary[]) {
  const order = ["dataset", "literature", "knowledge"];
  const grouped = new Map<string, PluginSummary[]>();
  for (const plugin of plugins.filter(isRetrievalPlugin)) {
    const category = primaryRetrievalCategory(plugin);
    grouped.set(category, [...(grouped.get(category) ?? []), plugin]);
  }
  return Array.from(grouped.entries())
    .sort(([left], [right]) => {
      const leftIndex = order.indexOf(left);
      const rightIndex = order.indexOf(right);
      if (leftIndex !== -1 || rightIndex !== -1) {
        return (leftIndex === -1 ? Number.MAX_SAFE_INTEGER : leftIndex) -
          (rightIndex === -1 ? Number.MAX_SAFE_INTEGER : rightIndex);
      }
      return left.localeCompare(right);
    })
    .map(([category, groupPlugins]) => ({
      category,
      plugins: groupPlugins.sort((left, right) => displayName(left).localeCompare(displayName(right))),
    }));
}

function PluginCard({
  plugin,
  retrievalStatuses = [],
  busy,
  onInstall,
  onOpenDetails,
}: {
  plugin: PluginSummary;
  retrievalStatuses?: PluginRetrievalRouteStatus[];
  processPoolStatuses?: PluginProcessPoolRouteStatus[];
  busy: boolean;
  onInstall: (plugin: PluginSummary) => void;
  onOpenDetails: (plugin: PluginSummary) => void;
}) {
  const installable = plugin.installPolicy !== "NOT_AVAILABLE";
  const theme = useTheme();
  const isActive = plugin.installed && plugin.enabled;
  const tone = isActive ? theme.palette.success.main : theme.palette.text.secondary;
  const hasRuntimeIssue = retrievalStatuses.some(
    (status) => status.quarantined || status.state === "degraded",
  );
  const subtitle = pluginCardSubtitle(plugin);

  const openDetails = () => onOpenDetails(plugin);
  const handleKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key !== "Enter" && event.key !== " ") return;
    event.preventDefault();
    openDetails();
  };

  return (
    <Paper
      variant="outlined"
      role="button"
      tabIndex={0}
      aria-label={`Open ${displayName(plugin)} plugin details`}
      onClick={openDetails}
      onKeyDown={handleKeyDown}
      sx={{
        px: 1.25,
        py: 1.15,
        minHeight: 72,
        borderRadius: 2.5,
        cursor: "pointer",
        display: "flex",
        alignItems: "center",
        gap: 1.25,
        bgcolor: "background.paper",
        borderColor: hasRuntimeIssue
          ? alpha(theme.palette.warning.main, 0.36)
          : "transparent",
        boxShadow: "none",
        transition: "background-color 160ms ease, box-shadow 160ms ease, transform 160ms ease",
        "@media (prefers-reduced-motion: reduce)": {
          transition: "none",
        },
        "&:hover": {
          bgcolor: "action.hover",
          boxShadow: `0 8px 22px ${alpha(theme.palette.common.black, theme.palette.mode === "dark" ? 0.24 : 0.07)}`,
          transform: "translateY(-1px)",
        },
        "&:focus-visible": {
          outline: `2px solid ${alpha(theme.palette.primary.main, 0.7)}`,
          outlineOffset: 2,
        },
      }}
    >
      <Box
        sx={{
          width: 38,
          height: 38,
          borderRadius: 2,
          display: "inline-flex",
          alignItems: "center",
          justifyContent: "center",
          color: tone,
          bgcolor: alpha(tone, theme.palette.mode === "dark" ? 0.18 : 0.09),
          border: `1px solid ${alpha(tone, theme.palette.mode === "dark" ? 0.22 : 0.12)}`,
          flexShrink: 0,
        }}
      >
        <ExtensionRounded fontSize="small" />
      </Box>

      <Box sx={{ minWidth: 0, flex: 1 }}>
        <Typography variant="subtitle2" fontWeight={800} noWrap title={displayName(plugin)}>
          {displayName(plugin)}
        </Typography>
        <Typography variant="body2" color="text.secondary" noWrap title={subtitle} sx={{ mt: 0.15 }}>
          {subtitle}
        </Typography>
      </Box>

      {plugin.installed ? (
        <Box
          aria-label={`${displayName(plugin)} is ${plugin.enabled ? "enabled" : "disabled"}`}
          title={plugin.enabled ? "Enabled" : "Installed but disabled"}
          sx={{
            width: 32,
            height: 32,
            borderRadius: "50%",
            display: "inline-flex",
            alignItems: "center",
            justifyContent: "center",
            flexShrink: 0,
            color: plugin.enabled ? "success.main" : "text.disabled",
          }}
        >
          <CheckRounded fontSize="small" />
        </Box>
      ) : (
        <IconButton
          aria-label={installable ? `Install ${displayName(plugin)}` : `${displayName(plugin)} unavailable`}
          size="small"
          disabled={busy || !installable}
          onClick={(event) => {
            event.stopPropagation();
            onInstall(plugin);
          }}
          onKeyDown={(event) => event.stopPropagation()}
          sx={{
            width: 34,
            height: 34,
            flexShrink: 0,
            bgcolor: alpha(theme.palette.text.primary, theme.palette.mode === "dark" ? 0.12 : 0.06),
            "&:hover": {
              bgcolor: alpha(theme.palette.primary.main, theme.palette.mode === "dark" ? 0.22 : 0.1),
            },
          }}
        >
          {busy ? <CircularProgress size={16} /> : <AddRounded fontSize="small" />}
        </IconButton>
      )}
    </Paper>
  );
}

function PluginDetailsDialog({
  plugin,
  open,
  retrievalStatuses = [],
  processPoolStatuses = [],
  busy,
  onClose,
  onInstall,
  onUninstall,
  onToggle,
  onCopyDiagnostics,
}: {
  plugin: PluginSummary | null;
  open: boolean;
  retrievalStatuses?: PluginRetrievalRouteStatus[];
  processPoolStatuses?: PluginProcessPoolRouteStatus[];
  busy: boolean;
  onClose: () => void;
  onInstall: (plugin: PluginSummary) => void;
  onUninstall: (plugin: PluginSummary) => void;
  onToggle: (plugin: PluginSummary, enabled: boolean) => void;
  onCopyDiagnostics: (
    plugin: PluginSummary,
    retrievalStatuses: PluginRetrievalRouteStatus[],
    processPoolStatuses: PluginProcessPoolRouteStatus[],
  ) => void;
}) {
  const theme = useTheme();
  if (!plugin) return null;

  const chips = capabilityChips(plugin).slice(0, 2);
  const declaredRetrievalSources = plugin.retrieval?.sources ?? [];
  const installable = plugin.installPolicy !== "NOT_AVAILABLE";
  const primaryPrompt = plugin.interface?.defaultPrompt?.[0] ?? null;
  const runtimeSummary = pluginRuntimeSummary(
    plugin,
    retrievalStatuses,
    processPoolStatuses,
  );
  const hasRuntimeDetails =
    retrievalStatuses.length > 0 || processPoolStatuses.length > 0 || runtimeSummary.lastError;
  const action = plugin.installed ? (
    <Stack direction="row" spacing={1} alignItems="center" flexWrap="wrap" justifyContent="flex-end">
      <Stack direction="row" spacing={1} alignItems="center">
        <Typography variant="body2" color="text.secondary">
          Enabled
        </Typography>
        <Switch
          size="small"
          checked={plugin.enabled}
          disabled={busy}
          onChange={(event) => onToggle(plugin, event.target.checked)}
          inputProps={{ "aria-label": `Enable ${displayName(plugin)}` }}
        />
      </Stack>
      <Button
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
      variant="contained"
      disableElevation
      startIcon={busy ? <CircularProgress size={16} color="inherit" /> : <AddRounded />}
      disabled={busy || !installable}
      onClick={() => onInstall(plugin)}
      sx={{ textTransform: "none", borderRadius: 2, whiteSpace: "nowrap" }}
    >
      {installable ? "Add to Omiga" : "Unavailable"}
    </Button>
  );

  return (
    <Dialog open={open} onClose={onClose} fullWidth maxWidth="md" aria-labelledby="plugin-details-title">
      <DialogTitle id="plugin-details-title" sx={{ px: 3, py: 2, pr: 7 }}>
        <Stack direction="row" spacing={1} alignItems="center" flexWrap="wrap">
          <Typography variant="body2" color="text.secondary">
            Plugins
          </Typography>
          <Typography variant="body2" color="text.secondary">
            ›
          </Typography>
          <Typography variant="body2" fontWeight={800}>
            {displayName(plugin)}
          </Typography>
        </Stack>
        <IconButton
          aria-label="Close plugin details"
          onClick={onClose}
          sx={{ position: "absolute", right: 12, top: 10 }}
        >
          <CloseRounded />
        </IconButton>
      </DialogTitle>

      <DialogContent sx={{ px: 3, pt: 2, pb: 3 }}>
        <Stack spacing={2.25}>
          <Stack direction={{ xs: "column", md: "row" }} spacing={2} alignItems={{ xs: "stretch", md: "flex-start" }}>
            <Box
              sx={{
                width: 56,
                height: 56,
                borderRadius: 2.5,
                display: "inline-flex",
                alignItems: "center",
                justifyContent: "center",
                color: plugin.installed && plugin.enabled ? "success.main" : "text.secondary",
                bgcolor: alpha(
                  plugin.installed && plugin.enabled ? theme.palette.success.main : theme.palette.text.primary,
                  theme.palette.mode === "dark" ? 0.16 : 0.07,
                ),
                border: `1px solid ${alpha(theme.palette.text.primary, theme.palette.mode === "dark" ? 0.18 : 0.1)}`,
                flexShrink: 0,
              }}
            >
              <ExtensionRounded sx={{ fontSize: 34 }} />
            </Box>

            <Box sx={{ flex: 1, minWidth: 0 }}>
              <Typography variant="h5" fontWeight={850} sx={{ lineHeight: 1.15 }}>
                {displayName(plugin)}
              </Typography>
              <Typography variant="body1" color="text.secondary" sx={{ mt: 0.6, lineHeight: 1.45 }}>
                {description(plugin)}
              </Typography>
              <Stack direction="row" gap={0.75} flexWrap="wrap" sx={{ mt: 1.25 }}>
                <Chip
                  size="small"
                  label={plugin.installed ? (plugin.enabled ? "Enabled" : "Installed") : "Available"}
                  color={plugin.installed ? (plugin.enabled ? "success" : "default") : "primary"}
                  variant={plugin.installed && plugin.enabled ? "filled" : "outlined"}
                />
                {declaredRetrievalSources.length > 0 && (
                  <Chip
                    size="small"
                    variant="outlined"
                    label={`${declaredRetrievalSources.length} route${declaredRetrievalSources.length === 1 ? "" : "s"}`}
                  />
                )}
                {chips.map((chip) => (
                  <Chip key={chip} size="small" variant="outlined" label={capabilityLabel(chip)} />
                ))}
              </Stack>
            </Box>

            <Box sx={{ flexShrink: 0, alignSelf: { xs: "flex-start", md: "center" } }}>
              {action}
            </Box>
          </Stack>

          <Paper
            variant="outlined"
            sx={{
              p: 1.5,
              borderRadius: 2.5,
              bgcolor: alpha(theme.palette.background.default, theme.palette.mode === "dark" ? 0.42 : 0.72),
            }}
          >
            <Stack
              direction={{ xs: "column", md: "row" }}
              spacing={1.25}
              alignItems={{ xs: "stretch", md: "center" }}
              justifyContent="space-between"
            >
              <Stack direction="row" gap={1} flexWrap="wrap" alignItems="center">
                <Chip
                  size="small"
                  color={
                    runtimeSummary.state === "healthy"
                      ? "success"
                      : runtimeSummary.state === "degraded"
                        ? "warning"
                        : runtimeSummary.state === "quarantined"
                          ? "error"
                          : "default"
                  }
                  variant={runtimeSummary.state === "healthy" ? "filled" : "outlined"}
                  label={runtimeSummary.label}
                />
                {declaredRetrievalSources.length > 0 && (
                  <Chip
                    size="small"
                    variant="outlined"
                    label={`${runtimeSummary.routeCount} route${runtimeSummary.routeCount === 1 ? "" : "s"}`}
                  />
                )}
                {runtimeSummary.issueCount > 0 && (
                  <Chip
                    size="small"
                    color="warning"
                    variant="filled"
                    label={`${runtimeSummary.issueCount} issue${runtimeSummary.issueCount === 1 ? "" : "s"}`}
                  />
                )}
                {runtimeSummary.pooledCount > 0 && (
                  <Chip
                    size="small"
                    color="info"
                    variant="outlined"
                    label={`${runtimeSummary.pooledCount} pooled`}
                  />
                )}
              </Stack>
              <Button
                size="small"
                variant="outlined"
                startIcon={<ContentCopyRounded />}
                disabled={busy}
                onClick={() => onCopyDiagnostics(plugin, retrievalStatuses, processPoolStatuses)}
                sx={{ textTransform: "none", borderRadius: 1.5, alignSelf: { xs: "flex-start", md: "center" } }}
              >
                Copy diagnostics
              </Button>
            </Stack>
            {runtimeSummary.lastError && (
              <Typography
                variant="caption"
                color="text.secondary"
                sx={{ display: "block", mt: 1, wordBreak: "break-word" }}
              >
                Last error: {runtimeSummary.lastError}
              </Typography>
            )}
          </Paper>

          {primaryPrompt && (
            <Paper
              elevation={0}
              sx={{
                p: 1.5,
                borderRadius: 2.5,
                overflow: "hidden",
                bgcolor: alpha(theme.palette.primary.main, theme.palette.mode === "dark" ? 0.18 : 0.08),
                background: `linear-gradient(135deg, ${alpha(theme.palette.primary.main, theme.palette.mode === "dark" ? 0.26 : 0.16)}, ${alpha(theme.palette.secondary.main, theme.palette.mode === "dark" ? 0.18 : 0.08)})`,
              }}
            >
              <Typography variant="caption" color="text.secondary" fontWeight={800}>
                Try in chat
              </Typography>
              <Typography variant="body2" sx={{ mt: 0.5, wordBreak: "break-word" }}>
                <Box component="span" sx={{ color: "primary.main", fontWeight: 850, mr: 0.75 }}>
                  {displayName(plugin)}
                </Box>
                {primaryPrompt}
              </Typography>
            </Paper>
          )}

          <Stack spacing={1.25}>
            <Typography variant="subtitle1" fontWeight={850}>
              {declaredRetrievalSources.length > 0 ? "Routes" : "Included content"}
            </Typography>
            <Paper variant="outlined" sx={{ borderRadius: 2.5, overflow: "hidden" }}>
              <Stack divider={<Box sx={{ height: 1, bgcolor: "divider" }} />}>
                {declaredRetrievalSources.length > 0 ? (
                  declaredRetrievalSources.map((source) => (
                    <Stack key={`${source.category}:${source.id}`} direction="row" spacing={1.25} alignItems="center" sx={{ p: 1.25 }}>
                      <Box
                        sx={{
                          width: 32,
                          height: 32,
                          borderRadius: "50%",
                          display: "inline-flex",
                          alignItems: "center",
                          justifyContent: "center",
                          bgcolor: alpha(theme.palette.warning.main, theme.palette.mode === "dark" ? 0.16 : 0.08),
                          color: "warning.main",
                          flexShrink: 0,
                        }}
                      >
                        <ExtensionRounded fontSize="small" />
                      </Box>
                      <Box sx={{ flex: 1, minWidth: 0 }}>
                        <Stack direction="row" gap={0.75} alignItems="center" flexWrap="wrap">
                          <Typography variant="subtitle2" fontWeight={800}>
                            {source.label || source.id}
                          </Typography>
                          <Chip size="small" variant="outlined" label={`source=${source.id}`} />
                          {source.capabilities.slice(0, 3).map((capability) => (
                            <Chip key={capability} size="small" variant="outlined" label={capabilityLabel(capability)} />
                          ))}
                        </Stack>
                        <Typography variant="caption" color="text.secondary" sx={{ display: "block", mt: 0.25 }}>
                          {capabilityLabel(source.category)}
                          {source.replacesBuiltin ? " · replaces built-in route" : ""}
                        </Typography>
                      </Box>
                    </Stack>
                  ))
                ) : (
                  <Stack direction="row" spacing={1.4} alignItems="center" sx={{ p: 1.5 }}>
                    <Box
                      sx={{
                        width: 34,
                        height: 34,
                        borderRadius: "50%",
                        display: "inline-flex",
                        alignItems: "center",
                        justifyContent: "center",
                        bgcolor: "action.hover",
                        color: "text.secondary",
                        flexShrink: 0,
                      }}
                    >
                      <ExtensionRounded fontSize="small" />
                    </Box>
                    <Box sx={{ minWidth: 0 }}>
                      <Typography variant="subtitle2" fontWeight={800}>
                        Plugin bundle
                      </Typography>
                      <Typography variant="body2" color="text.secondary">
                        Skills, workflows, metadata, or connector references declared by this plugin.
                      </Typography>
                    </Box>
                  </Stack>
                )}
              </Stack>
            </Paper>
          </Stack>

          {(declaredRetrievalSources.length > 0 || hasRuntimeDetails) && (
            <Accordion disableGutters elevation={0} sx={accordionSx}>
              <AccordionSummary expandIcon={<ExpandMoreRounded />} sx={compactAccordionSummarySx}>
                <Stack direction="row" gap={0.75} alignItems="center" flexWrap="wrap">
                  <Typography variant="subtitle1" fontWeight={850}>
                    Route diagnostics
                  </Typography>
                  {retrievalStatuses.length > 0 && <Chip size="small" variant="outlined" label={`${retrievalStatuses.length} route status`} />}
                  {processPoolStatuses.length > 0 && <Chip size="small" color="info" variant="outlined" label={`${processPoolStatuses.length} pooled`} />}
                </Stack>
              </AccordionSummary>
              <AccordionDetails sx={{ px: 1.5, pt: 0, pb: 1.5 }}>
                <Stack spacing={1.1}>
                  {retrievalStatuses.length > 0 && (
                    <Stack spacing={0.85}>
                      {retrievalStatuses.map((status) => {
                        const diagnostic = retrievalStatusDiagnostic(status);
                        return (
                          <Box
                            key={`${status.category}:${status.sourceId}`}
                            sx={{
                              p: 1,
                              borderRadius: 1.5,
                              bgcolor:
                                status.state === "healthy"
                                  ? "action.hover"
                                  : alpha(
                                      status.state === "quarantined"
                                        ? theme.palette.error.main
                                        : theme.palette.warning.main,
                                      theme.palette.mode === "dark" ? 0.13 : 0.06,
                                    ),
                            }}
                          >
                            <Stack spacing={0.6}>
                              <Stack direction="row" gap={0.75} alignItems="center" flexWrap="wrap">
                                <Typography variant="body2" fontWeight={800} sx={{ wordBreak: "break-all" }}>
                                  {diagnostic.title}
                                </Typography>
                                <Chip
                                  size="small"
                                  color={retrievalStateColor(status.state)}
                                  variant={status.state === "healthy" ? "outlined" : "filled"}
                                  label={status.state}
                                />
                                {status.consecutiveFailures > 0 && (
                                  <Chip size="small" color="warning" variant="outlined" label={`${status.consecutiveFailures} failures`} />
                                )}
                              </Stack>
                              <Typography variant="caption" color="text.secondary">
                                {diagnostic.detail}
                              </Typography>
                              {diagnostic.lastError && (
                                <Typography variant="caption" color="text.secondary" sx={{ wordBreak: "break-word" }}>
                                  Last error: {diagnostic.lastError}
                                </Typography>
                              )}
                            </Stack>
                          </Box>
                        );
                      })}
                    </Stack>
                  )}
                  {declaredRetrievalSources.length > 0 && retrievalStatuses.length === 0 && (
                    <Alert severity={plugin.installed ? "info" : "warning"} sx={{ borderRadius: 1.5 }}>
                      {plugin.installed
                        ? "No live route status yet. Enable this plugin route and run a Search / Query / Fetch call to populate diagnostics."
                        : "Install this plugin before runtime route diagnostics are available."}
                    </Alert>
                  )}
                  {processPoolStatuses.length > 0 && (
                    <Stack spacing={0.85}>
                      {processPoolStatuses.map((status) => {
                        const diagnostic = processPoolStatusDiagnostic(status);
                        return (
                          <Box key={`${status.category}:${status.sourceId}:${status.pluginRoot}`} sx={{ p: 1, borderRadius: 1.5, bgcolor: alpha(theme.palette.info.main, theme.palette.mode === "dark" ? 0.12 : 0.05) }}>
                            <Stack spacing={0.5}>
                              <Stack direction="row" gap={0.75} alignItems="center" flexWrap="wrap">
                                <Typography variant="body2" fontWeight={800} sx={{ wordBreak: "break-all" }}>
                                  {diagnostic.title}
                                </Typography>
                                <Chip size="small" color="info" variant="outlined" label="Pooled process" />
                              </Stack>
                              <Typography variant="caption" color="text.secondary">
                                {diagnostic.detail}
                              </Typography>
                              <Typography variant="caption" color="text.secondary" sx={{ wordBreak: "break-all" }}>
                                Plugin root: {diagnostic.pluginRoot}
                              </Typography>
                            </Stack>
                          </Box>
                        );
                      })}
                    </Stack>
                  )}
                </Stack>
              </AccordionDetails>
            </Accordion>
          )}

        </Stack>
      </DialogContent>
    </Dialog>
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
  const [detailPluginId, setDetailPluginId] = useState<string | null>(null);
  const [pluginSearch, setPluginSearch] = useState("");
  const [pluginFilter, setPluginFilter] = useState<PluginCatalogFilter>("all");
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
  const detailPlugin = detailPluginId ? pluginsById.get(detailPluginId) ?? null : null;
  const installedPlugins = useMemo(
    () => allPlugins.filter((plugin) => plugin.installed),
    [allPlugins],
  );
  const enabledPlugins = useMemo(
    () => installedPlugins.filter((plugin) => plugin.enabled),
    [installedPlugins],
  );
  const availablePlugins = useMemo(
    () => allPlugins.filter((plugin) => !plugin.installed),
    [allPlugins],
  );
  const filteredCatalogPlugins = useMemo(
    () => filterPluginsForCatalog(allPlugins, pluginSearch, pluginFilter),
    [allPlugins, pluginFilter, pluginSearch],
  );
  const filteredInstalledPlugins = useMemo(
    () => filterPluginsForCatalog(installedPlugins, pluginSearch, pluginFilter),
    [installedPlugins, pluginFilter, pluginSearch],
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
  const degradedRouteCount = useMemo(
    () => retrievalStatuses.filter((status) => status.state === "degraded").length,
    [retrievalStatuses],
  );
  const unknownRuntimePluginIds = useMemo(
    () =>
      unknownRetrievalRuntimePluginIds(
        allPlugins,
        retrievalStatuses,
        processPoolStatuses,
      ),
    [allPlugins, processPoolStatuses, retrievalStatuses],
  );
  const runtimeAttentionStatuses = useMemo(
    () =>
      retrievalStatuses.filter(
        (status) => status.state !== "healthy" || Boolean(status.lastError?.trim()),
      ),
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
  const filteredAvailableMarketplaces = useMemo(
    () =>
      availableMarketplaces
        .map(({ marketplace, plugins }) => ({
          marketplace,
          plugins: filterPluginsForCatalog(plugins, pluginSearch, pluginFilter),
        }))
        .filter(({ plugins }) => plugins.length > 0),
    [availableMarketplaces, pluginFilter, pluginSearch],
  );
  const filteredAvailablePluginCount = useMemo(
    () =>
      filteredAvailableMarketplaces.reduce(
        (count, { plugins }) => count + plugins.length,
        0,
      ),
    [filteredAvailableMarketplaces],
  );
  const hasPluginCatalogFilters = pluginSearch.trim().length > 0 || pluginFilter !== "all";

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

  const copyToClipboard = async (text: string, successMessage: string) => {
    setMessage(null);
    try {
      await navigator.clipboard.writeText(text);
      setMessage(successMessage);
    } catch {
      setMessage("Clipboard copy failed. Select the text manually and copy it.");
    }
  };

  const handleCopyDiagnostics = (
    plugin: PluginSummary,
    pluginRetrievalStatuses: PluginRetrievalRouteStatus[],
    pluginProcessPoolStatuses: PluginProcessPoolRouteStatus[],
  ) => {
    void copyToClipboard(
      buildPluginDiagnostics(
        plugin,
        pluginRetrievalStatuses,
        pluginProcessPoolStatuses,
      ),
      `Copied route diagnostics for ${displayName(plugin)}`,
    );
  };

  const handleCopyRuntimeDiagnostics = () => {
    void copyToClipboard(
      buildRetrievalRuntimeDiagnostics(
        allPlugins,
        retrievalStatuses,
        processPoolStatuses,
      ),
      "Copied retrieval runtime diagnostics",
    );
  };

  return (
    <Stack spacing={2.5} useFlexGap>
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
                  Plugins
                </Typography>
                <Typography variant="body2" color="text.secondary" sx={{ mt: 0.5, maxWidth: 880 }}>
                  Install local tools and data-source routes. Details and diagnostics stay one click away.
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
          <Stack direction="row" spacing={1.5} flexWrap="wrap" useFlexGap alignItems="center">
            {[
              ["Enabled", enabledPlugins.length],
              ["Installable", availablePlugins.length],
              ["Issues", quarantinedRouteCount + degradedRouteCount],
              ["Pooled", processPoolStatuses.length],
            ].map(([label, value]) => (
              <Box key={label} sx={{ display: "inline-flex", alignItems: "baseline", gap: 0.5 }}>
                <Typography variant="subtitle2" fontWeight={850}>
                  {value}
                </Typography>
                <Typography variant="caption" color="text.secondary" fontWeight={700}>
                  {label}
                </Typography>
              </Box>
            ))}
            {quarantinedRouteCount > 0 && (
              <Chip size="small" color="error" variant="filled" label={`${quarantinedRouteCount} quarantined`} />
            )}
            {unknownRuntimePluginIds.length > 0 && (
              <Chip size="small" color="warning" variant="filled" label={`${unknownRuntimePluginIds.length} stale refs`} />
            )}
          </Stack>
          <Stack direction={{ xs: "column", md: "row" }} spacing={1.25} alignItems={{ xs: "stretch", md: "center" }}>
            <TextField
              value={pluginSearch}
              onChange={(event) => setPluginSearch(event.target.value)}
              placeholder="Search plugins, data sources, routes..."
              size="small"
              fullWidth
              inputProps={{ "aria-label": "Search Omiga plugins" }}
              InputProps={{
                startAdornment: (
                  <InputAdornment position="start">
                    <SearchRounded fontSize="small" />
                  </InputAdornment>
                ),
                endAdornment: pluginSearch ? (
                  <InputAdornment position="end">
                    <IconButton
                      aria-label="Clear plugin search"
                      edge="end"
                      size="small"
                      onClick={() => setPluginSearch("")}
                    >
                      <ClearRounded fontSize="small" />
                    </IconButton>
                  </InputAdornment>
                ) : undefined,
              }}
              sx={{
                flex: 1,
                "& .MuiOutlinedInput-root": {
                  borderRadius: 2,
                  bgcolor: alpha(theme.palette.background.paper, theme.palette.mode === "dark" ? 0.55 : 0.82),
                },
              }}
            />
            <TextField
              select
              size="small"
              value={pluginFilter}
              onChange={(event) => setPluginFilter(event.target.value as PluginCatalogFilter)}
              inputProps={{ "aria-label": "Filter Omiga plugins" }}
              sx={{
                minWidth: { xs: "100%", md: 180 },
                "& .MuiOutlinedInput-root": {
                  borderRadius: 2,
                  bgcolor: alpha(theme.palette.background.paper, theme.palette.mode === "dark" ? 0.55 : 0.82),
                  fontWeight: 700,
                },
              }}
            >
              {pluginCatalogFilterOptions.map((option) => (
                <MenuItem key={option.value} value={option.value}>
                  {option.label}
                </MenuItem>
              ))}
            </TextField>
          </Stack>
          <Typography variant="caption" color="text.secondary">
            Showing {filteredCatalogPlugins.length} of {allPlugins.length}
            {hasPluginCatalogFilters ? " · filtered" : ""}.
          </Typography>
        </Stack>
      </Paper>

      {(error || message) && (
        <Alert severity={error ? "error" : "success"} sx={{ borderRadius: 2 }}>
          {error || message}
        </Alert>
      )}

      <PluginDetailsDialog
        plugin={detailPlugin}
        open={Boolean(detailPlugin)}
        retrievalStatuses={detailPlugin ? retrievalStatusesByPlugin.get(detailPlugin.id) : undefined}
        processPoolStatuses={detailPlugin ? processPoolStatusesByPlugin.get(detailPlugin.id) : undefined}
        busy={isMutating}
        onClose={() => setDetailPluginId(null)}
        onInstall={(plugin) => void handleInstall(plugin)}
        onUninstall={(plugin) => void handleUninstall(plugin)}
        onToggle={(plugin, enabled) => void handleToggle(plugin, enabled)}
        onCopyDiagnostics={handleCopyDiagnostics}
      />

      <Accordion disableGutters elevation={0} sx={accordionSx}>
        <AccordionSummary expandIcon={<ExpandMoreRounded />} sx={accordionSummarySx}>
          <Stack direction="row" spacing={1} alignItems="center" flexWrap="wrap">
            <Typography variant="subtitle2" fontWeight={700}>Runtime diagnostics</Typography>
            <Chip size="small" variant="outlined" label={`${retrievalStatuses.length} routes`} />
            <Chip size="small" variant="outlined" label={`${processPoolStatuses.length} pooled`} />
            {quarantinedRouteCount > 0 && (
              <Chip size="small" color="error" variant="filled" label={`${quarantinedRouteCount} quarantined`} />
            )}
            {degradedRouteCount > 0 && (
              <Chip size="small" color="warning" variant="filled" label={`${degradedRouteCount} degraded`} />
            )}
            {unknownRuntimePluginIds.length > 0 && (
              <Chip size="small" color="warning" variant="filled" label={`${unknownRuntimePluginIds.length} stale refs`} />
            )}
          </Stack>
        </AccordionSummary>
        <AccordionDetails sx={{ px: 2, pt: 0.75, pb: 2 }}>
          <Stack spacing={1.25} useFlexGap>
            <Stack direction="row" gap={1} flexWrap="wrap" justifyContent="flex-end">
              <Button
                size="small"
                variant="outlined"
                startIcon={<ContentCopyRounded />}
                onClick={handleCopyRuntimeDiagnostics}
                sx={{ textTransform: "none", borderRadius: 1.5 }}
              >
                Copy diagnostics
              </Button>
              <Button
                size="small"
                color="warning"
                variant="outlined"
                startIcon={isMutating ? <CircularProgress size={16} /> : <DeleteOutlineRounded />}
                disabled={isMutating || processPoolStatuses.length === 0}
                onClick={() => void handleClearProcessPool()}
                sx={{ textTransform: "none", borderRadius: 1.5 }}
              >
                Clear pool
              </Button>
            </Stack>
            {unknownRuntimePluginIds.length > 0 && (
              <Alert severity="warning" sx={{ borderRadius: 2 }}>
                <Stack spacing={1}>
                  <Typography variant="body2" fontWeight={700}>
                    Runtime diagnostics reference plugins that are not in the current catalog.
                  </Typography>
                  <Typography variant="body2">
                    Refresh plugins, clear pooled processes, and check old plugin or MCP config if these IDs keep coming back.
                  </Typography>
                  <Stack direction="row" gap={0.75} flexWrap="wrap">
                    {unknownRuntimePluginIds.map((pluginId) => (
                      <Chip
                        key={pluginId}
                        size="small"
                        color="warning"
                        variant="outlined"
                        label={pluginId}
                        sx={{ maxWidth: "100%", "& .MuiChip-label": { overflow: "hidden", textOverflow: "ellipsis" } }}
                      />
                    ))}
                  </Stack>
                </Stack>
              </Alert>
            )}
            {runtimeAttentionStatuses.length === 0 && processPoolStatuses.length === 0 ? (
              <Box sx={{ p: 1.5, borderRadius: 2, textAlign: "center", bgcolor: "action.hover" }}>
                <Typography variant="body2" color="text.secondary">
                  All routes healthy. No pooled child processes.
                </Typography>
              </Box>
            ) : null}
            {runtimeAttentionStatuses.length > 0 && (
              <Stack spacing={1}>
                <Typography variant="caption" color="text.secondary" fontWeight={800}>
                  Routes needing attention
                </Typography>
                {runtimeAttentionStatuses.map((status) => {
                  const diagnostic = retrievalStatusDiagnostic(status);
                  const plugin = pluginsById.get(status.pluginId);
                  return (
                    <Paper
                      key={`${status.pluginId}:${status.category}:${status.sourceId}`}
                      variant="outlined"
                      sx={{
                        p: 1,
                        borderRadius: 1.5,
                        bgcolor: alpha(
                          status.state === "quarantined"
                            ? theme.palette.error.main
                            : theme.palette.warning.main,
                          theme.palette.mode === "dark" ? 0.12 : 0.05,
                        ),
                        borderColor: alpha(
                          status.state === "quarantined"
                            ? theme.palette.error.main
                            : theme.palette.warning.main,
                          0.28,
                        ),
                      }}
                    >
                      <Stack spacing={0.65}>
                        <Stack direction="row" gap={0.75} alignItems="center" flexWrap="wrap">
                          <Typography variant="body2" fontWeight={800}>
                            {plugin ? displayName(plugin) : status.pluginId}
                          </Typography>
                          <Chip
                            size="small"
                            color={retrievalStateColor(status.state)}
                            variant="filled"
                            label={status.state}
                          />
                          <Chip size="small" variant="outlined" label={diagnostic.title} />
                        </Stack>
                        <Typography variant="caption" color="text.secondary">
                          {diagnostic.detail}
                        </Typography>
                        {diagnostic.lastError && (
                          <Typography variant="caption" color="text.secondary" sx={{ wordBreak: "break-word" }}>
                            Last error: {diagnostic.lastError}
                          </Typography>
                        )}
                      </Stack>
                    </Paper>
                  );
                })}
              </Stack>
            )}
            {processPoolStatuses.length > 0 && (
              Array.from(processPoolStatusesByPlugin.entries())
                .sort(([left], [right]) => left.localeCompare(right))
                .map(([pluginId, statuses]) => {
                  const plugin = pluginsById.get(pluginId);
                  return (
                    <Box key={pluginId} sx={{ p: 1.25, borderRadius: 2, bgcolor: "action.hover" }}>
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
                    </Box>
                  );
                })
            )}
          </Stack>
        </AccordionDetails>
      </Accordion>

      <Accordion defaultExpanded disableGutters elevation={0} sx={accordionSx}>
        <AccordionSummary expandIcon={<ExpandMoreRounded />} sx={accordionSummarySx}>
          <Stack direction="row" spacing={1} alignItems="center" flexWrap="wrap">
            <Typography variant="subtitle2" fontWeight={700}>Available plugins</Typography>
            <Chip size="small" variant="outlined" label={`${filteredAvailablePluginCount} installable`} />
            {hasPluginCatalogFilters && (
              <Chip size="small" variant="outlined" label={`${availablePlugins.length} total`} />
            )}
          </Stack>
        </AccordionSummary>
        <AccordionDetails sx={{ px: 2, pt: 0.75, pb: 2 }}>
          <Box sx={pluginListSx} role="region" aria-label="Installable Omiga plugins list">
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
            ) : filteredAvailableMarketplaces.length === 0 ? (
              <Paper variant="outlined" sx={{ p: 3, borderRadius: 2, textAlign: "center" }}>
                <SearchRounded sx={{ color: "text.secondary", mb: 1 }} />
                <Typography variant="body2" color="text.secondary">
                  No installable plugins match the current search or filter.
                </Typography>
              </Paper>
            ) : (
              filteredAvailableMarketplaces.map(({ marketplace, plugins }) => {
                const retrievalGroups = groupRetrievalPlugins(plugins);
                const otherPlugins = plugins.filter((plugin) => !isRetrievalPlugin(plugin));
                return (
                  <Stack key={marketplace.path} spacing={1.5} useFlexGap>
                    <Stack direction="row" spacing={1} alignItems="center" flexWrap="wrap" useFlexGap>
                      <Typography variant="caption" color="text.secondary" fontWeight={800}>
                        {marketplaceLabel(marketplace)}
                      </Typography>
                      <Chip
                        size="small"
                        variant="outlined"
                        label={`${plugins.filter(isRetrievalPlugin).length} data-source plugins`}
                      />
                      {otherPlugins.length > 0 && (
                        <Chip size="small" variant="outlined" label={`${otherPlugins.length} general tools`} />
                      )}
                    </Stack>

                    {retrievalGroups.map(({ category, plugins: groupPlugins }) => (
                      <Accordion
                        key={category}
                        disableGutters
                        elevation={0}
                        defaultExpanded={retrievalGroups.length === 1}
                        sx={nestedAccordionSx}
                      >
                        <AccordionSummary
                          expandIcon={<ExpandMoreRounded />}
                          sx={nestedAccordionSummarySx}
                        >
                          <Stack direction="row" spacing={1} alignItems="center" flexWrap="wrap">
                            <Typography variant="subtitle2" fontWeight={800}>
                              {retrievalCategoryLabel(category)}
                            </Typography>
                            <Chip size="small" variant="outlined" label={`${groupPlugins.length} plugins`} />
                            <Typography variant="caption" color="text.secondary">
                              Search / Query / Fetch routes
                            </Typography>
                          </Stack>
                        </AccordionSummary>
                        <AccordionDetails sx={{ px: 1.5, pt: 0.75, pb: 1.5 }}>
                          <Stack spacing={1.25} useFlexGap>
                            <Typography variant="caption" color="text.secondary">
                              Install one source at a time; each plugin owns only its listed route.
                            </Typography>
                            <Box sx={pluginCardGridSx}>
                              {groupPlugins.map((plugin) => (
                                <PluginCard
                                  key={plugin.id}
                                  plugin={plugin}
                                  retrievalStatuses={retrievalStatusesByPlugin.get(plugin.id)}
                                  processPoolStatuses={processPoolStatusesByPlugin.get(plugin.id)}
                                  busy={isMutating}
                                  onInstall={(p) => void handleInstall(p)}
                                  onOpenDetails={(selectedPlugin) => setDetailPluginId(selectedPlugin.id)}
                                />
                              ))}
                            </Box>
                          </Stack>
                        </AccordionDetails>
                      </Accordion>
                    ))}

                    {otherPlugins.length > 0 && (
                      <Accordion disableGutters elevation={0} defaultExpanded={retrievalGroups.length === 0} sx={nestedAccordionSx}>
                        <AccordionSummary
                          expandIcon={<ExpandMoreRounded />}
                          sx={nestedAccordionSummarySx}
                        >
                          <Stack direction="row" spacing={1} alignItems="center" flexWrap="wrap">
                            <Typography variant="subtitle2" fontWeight={800}>
                              General plugins
                            </Typography>
                            <Chip size="small" variant="outlined" label={`${otherPlugins.length} plugins`} />
                            <Typography variant="caption" color="text.secondary">
                              Skills, notebook helpers, workflows
                            </Typography>
                          </Stack>
                        </AccordionSummary>
                        <AccordionDetails sx={{ px: 1.5, pt: 0.75, pb: 1.5 }}>
                          <Stack spacing={1.25} useFlexGap>
                            <Typography variant="caption" color="text.secondary">
                              Non-retrieval capabilities such as notebook helpers or workflow tools.
                            </Typography>
                            <Box sx={pluginCardGridSx}>
                              {otherPlugins.map((plugin) => (
                                <PluginCard
                                  key={plugin.id}
                                  plugin={plugin}
                                  retrievalStatuses={retrievalStatusesByPlugin.get(plugin.id)}
                                  processPoolStatuses={processPoolStatusesByPlugin.get(plugin.id)}
                                  busy={isMutating}
                                  onInstall={(p) => void handleInstall(p)}
                                  onOpenDetails={(selectedPlugin) => setDetailPluginId(selectedPlugin.id)}
                                />
                              ))}
                            </Box>
                          </Stack>
                        </AccordionDetails>
                      </Accordion>
                    )}
                  </Stack>
                );
              })
            )}
          </Box>
        </AccordionDetails>
      </Accordion>

      <Accordion defaultExpanded disableGutters elevation={0} sx={accordionSx}>
        <AccordionSummary expandIcon={<ExpandMoreRounded />} sx={accordionSummarySx}>
          <Stack direction="row" spacing={1} alignItems="center" flexWrap="wrap">
            <Typography variant="subtitle2" fontWeight={700}>Installed plugins</Typography>
            <Chip size="small" variant="outlined" label={`${filteredInstalledPlugins.length} installed`} />
            {hasPluginCatalogFilters && (
              <Chip size="small" variant="outlined" label={`${installedPlugins.length} total`} />
            )}
          </Stack>
        </AccordionSummary>
        <AccordionDetails sx={{ px: 2, pt: 0.75, pb: 2 }}>
          <Box sx={pluginListSx} role="region" aria-label="Installed Omiga plugins list">
            {installedPlugins.length === 0 ? (
              <Paper variant="outlined" sx={{ p: 3, borderRadius: 2, textAlign: "center" }}>
                <ExtensionRounded sx={{ color: "text.secondary", mb: 1 }} />
                <Typography variant="body2" color="text.secondary">
                  No Omiga plugins installed yet. Install a recommended plugin to add native skills, MCP servers, or app connectors.
                </Typography>
              </Paper>
            ) : filteredInstalledPlugins.length === 0 ? (
              <Paper variant="outlined" sx={{ p: 3, borderRadius: 2, textAlign: "center" }}>
                <SearchRounded sx={{ color: "text.secondary", mb: 1 }} />
                <Typography variant="body2" color="text.secondary">
                  No installed plugins match the current search or filter.
                </Typography>
              </Paper>
            ) : (
              <Box sx={pluginCardGridSx}>
                {filteredInstalledPlugins.map((plugin) => (
                  <PluginCard
                    key={plugin.id}
                    plugin={plugin}
                    retrievalStatuses={retrievalStatusesByPlugin.get(plugin.id)}
                    processPoolStatuses={processPoolStatusesByPlugin.get(plugin.id)}
                    busy={isMutating}
                    onInstall={(p) => void handleInstall(p)}
                    onOpenDetails={(selectedPlugin) => setDetailPluginId(selectedPlugin.id)}
                  />
                ))}
              </Box>
            )}
          </Box>
        </AccordionDetails>
      </Accordion>
    </Stack>
  );
}
