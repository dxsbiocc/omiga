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
  Snackbar,
  Stack,
  Switch,
  TextField,
  Typography,
} from "@mui/material";
import { alpha, useTheme } from "@mui/material/styles";
import {
  AddRounded,
  ClearRounded,
  CloseRounded,
  ContentCopyRounded,
  DeleteOutlineRounded,
  ExtensionRounded,
  ExpandMoreRounded,
  PlayArrowRounded,
  RefreshRounded,
  SearchRounded,
  TroubleshootRounded,
} from "@mui/icons-material";
import {
  buildPluginDiagnostics,
  buildRetrievalRuntimeDiagnostics,
  flattenMarketplacePlugins,
  summarizeOperatorRunResult,
  type OperatorManifestDiagnostic,
  type OperatorRunCleanupRequest,
  type OperatorRunDetail,
  type OperatorRunVerification,
  type OperatorRunLog,
  type OperatorRunSummary,
  type OperatorSummary,
  type PluginProcessPoolRouteStatus,
  type PluginRetrievalLifecycleState,
  type PluginRetrievalRouteStatus,
  type PluginSummary,
  usePluginStore,
} from "../../state/pluginStore";
import { useChatComposerStore } from "../../state/chatComposerStore";
import { useSessionStore } from "../../state/sessionStore";
import { NotebookViewerSettingsPanel } from "./NotebookSettingsTab";

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

export function displayName(plugin: PluginSummary): string {
  return cleanPluginDisplayName(plugin.interface?.displayName || plugin.name);
}

function cleanPluginDisplayName(value: string): string {
  return value
    .replace(/\s+Retrieval\s+Source$/i, "")
    .replace(/\s+Source$/i, "")
    .replace(/\s+Operator$/i, "")
    .trim() || value;
}

function description(plugin: PluginSummary): string {
  return (
    plugin.interface?.shortDescription ||
    plugin.interface?.longDescription ||
    "Omiga-native plugin bundle."
  );
}

function capabilityChips(plugin: PluginSummary) {
  const caps = plugin.interface?.capabilities ?? [];
  const category = plugin.interface?.category;
  return Array.from(new Set([category, ...caps].filter(Boolean) as string[])).slice(0, 6);
}

function pluginClassificationTerms(plugin: PluginSummary): string[] {
  return [
    plugin.interface?.category,
    ...(plugin.interface?.capabilities ?? []),
    plugin.name,
    plugin.id,
  ]
    .filter((value): value is string => Boolean(value?.trim()))
    .map((value) => value.trim().toLowerCase().replace(/[-_]+/g, " "));
}

function pluginHasTerm(plugin: PluginSummary, terms: string[]): boolean {
  const haystack = pluginClassificationTerms(plugin);
  return haystack.some((value) => terms.some((term) => value === term || value.includes(term)));
}

function isOperatorPlugin(plugin: PluginSummary): boolean {
  return pluginHasTerm(plugin, ["operator", "operators"]);
}

type OperatorPluginIconKind = "r" | "cpp" | "python" | "c" | "shell" | "operator";

type OperatorPluginIconSpec = {
  kind: OperatorPluginIconKind;
  label: string;
  body: string | null;
  color: string | null;
};

const operatorIconifyBodies: Record<Exclude<OperatorPluginIconKind, "operator">, string> = {
  // Iconify Simple Icons data, inlined to avoid adding a runtime icon package.
  r: '<path fill="currentColor" d="M12 2.746c-6.627 0-12 3.599-12 8.037c0 3.897 4.144 7.144 9.64 7.88V16.26c-2.924-.915-4.925-2.755-4.925-4.877c0-3.035 4.084-5.494 9.12-5.494c5.038 0 8.757 1.683 8.757 5.494c0 1.976-.999 3.379-2.662 4.272c.09.066.174.128.258.216c.169.149.25.363.372.544c2.128-1.45 3.44-3.437 3.44-5.631c0-4.44-5.373-8.038-12-8.038m-2.111 4.99v13.516l4.093-.002l-.002-5.291h1.1c.225 0 .321.066.549.25c.272.22.715.982.715.982l2.164 4.063l4.627-.002l-2.864-4.826s-.086-.193-.265-.383a2.2 2.2 0 0 0-.582-.416c-.422-.214-1.149-.434-1.149-.434s3.578-.264 3.578-3.826s-3.744-3.63-3.744-3.63zm4.127 2.93l2.478.002s1.149-.062 1.149 1.127c0 1.165-1.149 1.17-1.149 1.17h-2.478zm1.754 6.119c-.494.049-1.012.079-1.54.088v1.807a17 17 0 0 0 2.37-.473l-.471-.891s-.108-.183-.248-.394c-.039-.054-.08-.098-.111-.137"/>',
  cpp: '<path fill="currentColor" d="M22.394 6c-.167-.29-.398-.543-.652-.69L12.926.22c-.509-.294-1.34-.294-1.848 0L2.26 5.31c-.508.293-.923 1.013-.923 1.6v10.18c0 .294.104.62.271.91s.398.543.652.69l8.816 5.09c.508.293 1.34.293 1.848 0l8.816-5.09c.254-.147.485-.4.652-.69s.27-.616.27-.91V6.91c.003-.294-.1-.62-.268-.91M12 19.11c-3.92 0-7.109-3.19-7.109-7.11s3.19-7.11 7.11-7.11a7.13 7.13 0 0 1 6.156 3.553l-3.076 1.78a3.57 3.57 0 0 0-3.08-1.78A3.56 3.56 0 0 0 8.444 12A3.56 3.56 0 0 0 12 15.555a3.57 3.57 0 0 0 3.08-1.778l3.078 1.78A7.14 7.14 0 0 1 12 19.11m7.11-6.715h-.79v.79h-.79v-.79h-.79v-.79h.79v-.79h.79v.79h.79zm2.962 0h-.79v.79h-.79v-.79h-.79v-.79h.79v-.79h.79v.79h.79z"/>',
  python: '<path fill="currentColor" d="m14.25.18l.9.2l.73.26l.59.3l.45.32l.34.34l.25.34l.16.33l.1.3l.04.26l.02.2l-.01.13V8.5l-.05.63l-.13.55l-.21.46l-.26.38l-.3.31l-.33.25l-.35.19l-.35.14l-.33.1l-.3.07l-.26.04l-.21.02H8.77l-.69.05l-.59.14l-.5.22l-.41.27l-.33.32l-.27.35l-.2.36l-.15.37l-.1.35l-.07.32l-.04.27l-.02.21v3.06H3.17l-.21-.03l-.28-.07l-.32-.12l-.35-.18l-.36-.26l-.36-.36l-.35-.46l-.32-.59l-.28-.73l-.21-.88l-.14-1.05l-.05-1.23l.06-1.22l.16-1.04l.24-.87l.32-.71l.36-.57l.4-.44l.42-.33l.42-.24l.4-.16l.36-.1l.32-.05l.24-.01h.16l.06.01h8.16v-.83H6.18l-.01-2.75l-.02-.37l.05-.34l.11-.31l.17-.28l.25-.26l.31-.23l.38-.2l.44-.18l.51-.15l.58-.12l.64-.1l.71-.06l.77-.04l.84-.02l1.27.05zm-6.3 1.98l-.23.33l-.08.41l.08.41l.23.34l.33.22l.41.09l.41-.09l.33-.22l.23-.34l.08-.41l-.08-.41l-.23-.33l-.33-.22l-.41-.09l-.41.09zm13.09 3.95l.28.06l.32.12l.35.18l.36.27l.36.35l.35.47l.32.59l.28.73l.21.88l.14 1.04l.05 1.23l-.06 1.23l-.16 1.04l-.24.86l-.32.71l-.36.57l-.4.45l-.42.33l-.42.24l-.4.16l-.36.09l-.32.05l-.24.02l-.16-.01h-8.22v.82h5.84l.01 2.76l.02.36l-.05.34l-.11.31l-.17.29l-.25.25l-.31.24l-.38.2l-.44.17l-.51.15l-.58.13l-.64.09l-.71.07l-.77.04l-.84.01l-1.27-.04l-1.07-.14l-.9-.2l-.73-.25l-.59-.3l-.45-.33l-.34-.34l-.25-.34l-.16-.33l-.1-.3l-.04-.25l-.02-.2l.01-.13v-5.34l.05-.64l.13-.54l.21-.46l.26-.38l.3-.32l.33-.24l.35-.2l.35-.14l.33-.1l.3-.06l.26-.04l.21-.02l.13-.01h5.84l.69-.05l.59-.14l.5-.21l.41-.28l.33-.32l.27-.35l.2-.36l.15-.36l.1-.35l.07-.32l.04-.28l.02-.21V6.07h2.09l.14.01zm-6.47 14.25l-.23.33l-.08.41l.08.41l.23.33l.33.23l.41.08l.41-.08l.33-.23l.23-.33l.08-.41l-.08-.41l-.23-.33l-.33-.23l-.41-.08l-.41.08z"/>',
  c: '<path fill="currentColor" d="M16.592 9.196s-.354-3.298-3.627-3.39c-3.274-.09-4.955 2.474-4.955 6.14s1.858 6.597 5.045 6.597c3.184 0 3.538-3.665 3.538-3.665l6.104.365s.36 3.31-2.196 5.836c-2.552 2.524-5.69 2.937-7.876 2.92c-2.19-.016-5.226.035-8.16-2.97c-2.938-3.01-3.436-5.93-3.436-8.8s.556-6.67 4.047-9.55C7.444.72 9.849 0 12.254 0c10.042 0 10.717 9.26 10.717 9.26z"/>',
  shell: '<path fill="currentColor" d="M21.038 4.9L13.461.402a2.86 2.86 0 0 0-2.923.001L2.961 4.9A3.02 3.02 0 0 0 1.5 7.503v8.995c0 1.073.557 2.066 1.462 2.603l7.577 4.497a2.86 2.86 0 0 0 2.922 0l7.577-4.497a3.02 3.02 0 0 0 1.462-2.603V7.503A3.02 3.02 0 0 0 21.038 4.9M15.17 18.946l.013.646c.001.078-.05.167-.111.198l-.383.22c-.061.031-.111-.007-.112-.085l-.007-.635c-.328.136-.66.169-.872.084c-.04-.016-.057-.075-.041-.142l.139-.584a.24.24 0 0 1 .069-.121a.2.2 0 0 1 .036-.026q.033-.017.062-.006c.229.077.521.041.802-.101c.357-.181.596-.545.592-.907c-.003-.328-.181-.465-.613-.468c-.55.001-1.064-.107-1.072-.917c-.007-.667.34-1.361.889-1.8l-.007-.652c-.001-.08.048-.168.111-.2l.37-.236c.061-.031.111.007.112.087l.006.653c.273-.109.511-.138.726-.088c.047.012.067.076.048.151l-.144.578a.26.26 0 0 1-.065.116a.2.2 0 0 1-.038.028a.1.1 0 0 1-.057.009c-.098-.022-.332-.073-.699.113c-.385.195-.52.53-.517.778c.003.297.155.387.681.396c.7.012 1.003.318 1.01 1.023c.007.689-.362 1.433-.928 1.888m3.973-1.087c0 .06-.008.116-.058.145l-1.916 1.164c-.05.029-.09.004-.09-.056v-.494c0-.06.037-.093.087-.122l1.887-1.129c.05-.029.09-.004.09.056zm1.316-11.062l-7.168 4.427c-.894.523-1.553 1.109-1.553 2.187v8.833c0 .645.26 1.063.66 1.184a2.3 2.3 0 0 1-.398.039c-.42 0-.833-.114-1.197-.33L3.226 18.64a2.5 2.5 0 0 1-1.201-2.142V7.503c0-.881.46-1.702 1.201-2.142L10.803.863a2.34 2.34 0 0 1 2.394 0l7.577 4.498a2.48 2.48 0 0 1 1.164 1.732c-.252-.536-.818-.682-1.479-.296"/>',
};

const operatorIconColors: Record<Exclude<OperatorPluginIconKind, "operator">, string> = {
  r: "#276DC3",
  cpp: "#00599C",
  python: "#3776AB",
  c: "#A8B9CC",
  shell: "#4EAA25",
};

const operatorIconLabels: Record<OperatorPluginIconKind, string> = {
  r: "R",
  cpp: "C++",
  python: "Python",
  c: "C",
  shell: "Shell",
  operator: "Operator",
};

function operatorIconKindFromHaystack(haystack: string): OperatorPluginIconKind {
  if (/\bc\+\+\b|\bcpp\b/.test(haystack)) return "cpp";
  if (/\brscript\b|\bbase r\b|\br\b/.test(haystack)) return "r";
  if (/\bpython\b|\bpython3\b|\bpy\b/.test(haystack)) return "python";
  if (/\bseqtk\b|\bc\b/.test(haystack)) return "c";
  if (/\bshell\b|\bsh\b|\bbash\b|\bcontainer\b|\bsmoke\b/.test(haystack)) return "shell";
  return "operator";
}

function buildOperatorIconSpec(kind: OperatorPluginIconKind): OperatorPluginIconSpec {
  return {
    kind,
    label: operatorIconLabels[kind],
    body: kind === "operator" ? null : operatorIconifyBodies[kind],
    color: kind === "operator" ? null : operatorIconColors[kind],
  };
}

export function operatorPluginIconSpec(plugin: PluginSummary): OperatorPluginIconSpec | null {
  if (!isOperatorPlugin(plugin)) return null;
  const haystack = [
    plugin.name,
    plugin.id,
    plugin.interface?.displayName,
    plugin.interface?.shortDescription,
    plugin.interface?.longDescription,
    ...(plugin.interface?.capabilities ?? []),
  ]
    .filter((value): value is string => Boolean(value?.trim()))
    .join(" ")
    .toLowerCase();
  return buildOperatorIconSpec(operatorIconKindFromHaystack(haystack));
}

function isFunctionPlugin(plugin: PluginSummary): boolean {
  return pluginHasTerm(plugin, [
    "function",
    "functions",
    "tool",
    "tools",
    "function plugin",
    "tool plugin",
    "custom tool",
  ]);
}

function isNotebookPlugin(plugin: PluginSummary): boolean {
  return pluginHasTerm(plugin, ["notebook", "jupyter", "ipynb"]);
}

export type PluginCatalogGroupId = "operator" | "tools" | "source" | "other";

export interface PluginCatalogGroup {
  id: PluginCatalogGroupId;
  title: string;
  description: string;
  plugins: PluginSummary[];
}

export interface PluginCatalogSection {
  id: string;
  title: string;
  plugins: PluginSummary[];
}

export function pluginCatalogGroupId(plugin: PluginSummary): PluginCatalogGroupId {
  if (isOperatorPlugin(plugin)) return "operator";
  if (isFunctionPlugin(plugin)) return "tools";
  if (isRetrievalPlugin(plugin)) return "source";
  return "other";
}

function pluginCatalogGroupLabel(group: PluginCatalogGroupId): string {
  switch (group) {
    case "operator":
      return "Operators";
    case "tools":
      return "Tools";
    case "source":
      return "Source";
    default:
      return "Others";
  }
}

function pluginCatalogGroupDescription(group: PluginCatalogGroupId): string {
  switch (group) {
    case "operator":
      return "Plugin bundles that contribute operator manifests and agent-callable operator tools.";
    case "tools":
      return "Plugin bundles that expose model-callable functions or custom tool surfaces.";
    case "source":
      return "Search / Query / Fetch data-source plugins grouped by source type.";
    default:
      return "Notebook, workflow, and other plugin bundles.";
  }
}

export function groupPluginsByCatalogGroup(plugins: PluginSummary[]): PluginCatalogGroup[] {
  const order: PluginCatalogGroupId[] = ["operator", "tools", "source", "other"];
  const grouped = new Map<PluginCatalogGroupId, PluginSummary[]>();
  for (const plugin of plugins) {
    const group = pluginCatalogGroupId(plugin);
    grouped.set(group, [...(grouped.get(group) ?? []), plugin]);
  }
  return order
    .map((id) => ({
      id,
      title: pluginCatalogGroupLabel(id),
      description: pluginCatalogGroupDescription(id),
      plugins: (grouped.get(id) ?? []).sort((left, right) =>
        displayName(left).localeCompare(displayName(right)),
      ),
    }))
    .filter((group) => group.plugins.length > 0);
}

function pluginCatalogSectionId(groupId: PluginCatalogGroupId, plugin: PluginSummary): string {
  if (groupId === "operator") return "operator";
  if (groupId === "tools") return "function";
  if (groupId === "source") return `source:${primaryRetrievalCategory(plugin)}`;
  return `category:${plugin.interface?.category?.trim().toLowerCase() || "other"}`;
}

function pluginCatalogSectionLabel(groupId: PluginCatalogGroupId, sectionId: string): string {
  if (groupId === "operator") return "Operator plugins";
  if (groupId === "tools") return "Function tools";
  if (groupId === "source" && sectionId.startsWith("source:")) {
    return retrievalCategoryLabel(sectionId.slice("source:".length));
  }
  const category = sectionId.slice("category:".length);
  return category === "other" ? "Other plugins" : capabilityLabel(category);
}

export function groupPluginsByCatalogSection(
  groupId: PluginCatalogGroupId,
  plugins: PluginSummary[],
): PluginCatalogSection[] {
  const grouped = new Map<string, PluginSummary[]>();
  for (const plugin of plugins) {
    const sectionId = pluginCatalogSectionId(groupId, plugin);
    grouped.set(sectionId, [...(grouped.get(sectionId) ?? []), plugin]);
  }
  return Array.from(grouped.entries())
    .map(([sectionId, sectionPlugins]) => ({
      id: sectionId,
      title: pluginCatalogSectionLabel(groupId, sectionId),
      plugins: sectionPlugins.sort((left, right) => displayName(left).localeCompare(displayName(right))),
    }))
    .sort((left, right) => left.title.localeCompare(right.title));
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
  | "operators"
  | "tools"
  | "data-sources"
  | "general";

const pluginCatalogFilterOptions: Array<{ value: PluginCatalogFilter; label: string }> = [
  { value: "all", label: "All" },
  { value: "available", label: "Available" },
  { value: "installed", label: "Installed" },
  { value: "enabled", label: "Enabled" },
  { value: "operators", label: "Operators" },
  { value: "tools", label: "Tools" },
  { value: "data-sources", label: "Source" },
  { value: "general", label: "Others" },
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
    case "operators":
      return pluginCatalogGroupId(plugin) === "operator";
    case "tools":
      return pluginCatalogGroupId(plugin) === "tools";
    case "data-sources":
      return pluginCatalogGroupId(plugin) === "source";
    case "general":
      return pluginCatalogGroupId(plugin) === "other";
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

export function operatorDisplayName(operator: OperatorSummary): string {
  return operator.name?.trim() || operator.id;
}

export function operatorPrimaryAlias(operator: OperatorSummary): string {
  return operator.enabledAliases.find((alias) => alias.trim().length > 0) || operator.id;
}

export function operatorToolName(alias: string): string {
  return `operator__${alias}`;
}

export function operatorSupportsSmokeRun(operator: OperatorSummary): boolean {
  return (operator.smokeTests?.length ?? 0) > 0;
}

export function operatorSmokeTestForRun(
  operator: OperatorSummary,
  smokeTestId?: string | null,
) {
  const smokeTests = operator.smokeTests ?? [];
  const requested = smokeTestId?.trim();
  if (requested) {
    const match = smokeTests.find((test) => test.id === requested);
    if (match) return match;
  }
  return smokeTests.find((test) => test.id.trim().length > 0) ?? null;
}

export function operatorPrimarySmokeTest(operator: OperatorSummary) {
  return operatorSmokeTestForRun(operator);
}

export function operatorSmokeRunArguments(
  operator: OperatorSummary,
  smokeTestId?: string | null,
) {
  return operatorSmokeTestForRun(operator, smokeTestId)?.arguments ?? {
    inputs: {},
    params: {},
    resources: {},
  };
}

export function operatorSmokeRunLabel(
  operator: OperatorSummary,
  smokeTestId?: string | null,
): string {
  const smokeTest = operatorSmokeTestForRun(operator, smokeTestId);
  return smokeTest?.name?.trim() || smokeTest?.id || "Smoke test";
}

export function operatorSmokeTestSummary(
  operator: OperatorSummary,
  smokeTestId?: string | null,
): string | null {
  const smokeTests = operator.smokeTests ?? [];
  if (smokeTests.length === 0) return null;
  const primary = operatorSmokeTestForRun(operator, smokeTestId);
  const label = operatorSmokeRunLabel(operator, smokeTestId);
  const extra = smokeTests.length > 1 ? ` · +${smokeTests.length - 1} more` : "";
  const description = primary?.description?.trim();
  return description ? `${label}: ${description}${extra}` : `${label}${extra}`;
}

function previewValue(value: unknown): string {
  if (value == null) return "null";
  if (typeof value === "string") return value;
  if (typeof value === "number" || typeof value === "boolean") return String(value);
  const raw = JSON.stringify(value);
  if (!raw) return String(value);
  return raw.length > 80 ? `${raw.slice(0, 77)}…` : raw;
}

function runtimeAxisValues(runtime: unknown, axis: string): string[] {
  if (!runtime || typeof runtime !== "object" || Array.isArray(runtime)) return [];
  const value = (runtime as Record<string, unknown>)[axis];
  if (typeof value === "string" && value.trim()) return [value.trim()];
  if (Array.isArray(value)) {
    return value.filter((item): item is string => typeof item === "string" && item.trim().length > 0);
  }
  if (value && typeof value === "object" && !Array.isArray(value)) {
    const supported = (value as Record<string, unknown>).supported;
    if (typeof supported === "string" && supported.trim()) return [supported.trim()];
    if (Array.isArray(supported)) {
      return supported.filter((item): item is string => typeof item === "string" && item.trim().length > 0);
    }
  }
  return [];
}

export function operatorTemplateScript(operator: OperatorSummary): string | null {
  const argv = operator.execution?.argv ?? [];
  return (
    argv.find((token) => /(^|\/)bin\/[^/]+$/.test(token.trim())) ||
    argv.find((token) => /\.(r|R|py|sh|bash|pl|rb|jl|cpp|c)(\s|$)/.test(token.trim())) ||
    argv[0] ||
    null
  );
}

export function operatorImplementationIconSpec(operator: OperatorSummary): OperatorPluginIconSpec {
  const haystack = [
    operator.id,
    operator.name,
    operator.description,
    ...(operator.tags ?? []),
    ...(operator.execution?.argv ?? []),
  ]
    .filter((value): value is string => Boolean(value?.trim()))
    .join(" ")
    .toLowerCase();
  return buildOperatorIconSpec(operatorIconKindFromHaystack(haystack));
}

export function operatorRuntimeSummary(operator: OperatorSummary): string {
  const runtime = operator.runtime;
  const placement = runtimeAxisValues(runtime, "placement");
  const container = runtimeAxisValues(runtime, "container");
  const scheduler = runtimeAxisValues(runtime, "scheduler");
  const parts = [
    placement.length > 0 ? `placement: ${placement.join(", ")}` : null,
    container.length > 0 ? `container: ${container.join(", ")}` : null,
    scheduler.length > 0 ? `scheduler: ${scheduler.join(", ")}` : null,
  ].filter((part): part is string => Boolean(part));
  return parts.join(" · ") || "runtime: user environment";
}

export function operatorSchemaStats(operator: OperatorSummary): {
  inputs: number;
  requiredInputs: number;
  params: number;
  requiredParams: number;
  outputs: number;
  resources: number;
} {
  const inputs = Object.values(operator.interface?.inputs ?? {});
  const params = Object.values(operator.interface?.params ?? {});
  return {
    inputs: inputs.length,
    requiredInputs: inputs.filter((field) => field.required).length,
    params: params.length,
    requiredParams: params.filter((field) => field.required).length,
    outputs: Object.keys(operator.interface?.outputs ?? {}).length,
    resources: Object.values(operator.resources ?? {}).filter((resource) => resource.exposed !== false).length,
  };
}

function operatorExecutionCommand(operator: OperatorSummary): string {
  const argv = operator.execution?.argv ?? [];
  if (argv.length === 0) return "No execution argv declared";
  const command = argv.join(" ");
  return command.length > 180 ? `${command.slice(0, 177)}…` : command;
}

function operatorFieldEntries(
  fields: Record<string, { kind?: string | null; required?: boolean; default?: unknown; description?: string | null }> | undefined,
): Array<[string, { kind?: string | null; required?: boolean; default?: unknown; description?: string | null }]> {
  return Object.entries(fields ?? {}).sort(([left], [right]) => left.localeCompare(right));
}

function operatorResourceEntries(operator: OperatorSummary) {
  return Object.entries(operator.resources ?? {})
    .filter(([, resource]) => resource.exposed !== false)
    .sort(([left], [right]) => left.localeCompare(right));
}

function operatorCatalogKey(operator: OperatorSummary): string {
  return `${operator.id}:${operator.version}:${operator.sourcePlugin}:${operator.manifestPath}`;
}

export function operatorRunStatusColor(
  status: string,
): "success" | "warning" | "error" | "info" | "default" {
  switch (status.trim().toLowerCase()) {
    case "succeeded":
    case "success":
      return "success";
    case "failed":
    case "error":
      return "error";
    case "running":
    case "created":
    case "collecting_outputs":
    case "exporting_results":
      return "info";
    case "cancelled":
    case "timeout":
    case "timed_out":
      return "warning";
    default:
      return "default";
  }
}

export function operatorRunTitle(run: OperatorRunSummary): string {
  const alias = run.operatorAlias?.trim();
  if (alias) return operatorToolName(alias);
  return run.operatorId?.trim() || run.runId;
}

export interface OperatorRunStats {
  total: number;
  succeeded: number;
  failed: number;
  running: number;
  warning: number;
  other: number;
  cacheHits: number;
  cacheMisses: number;
  smokeTotal: number;
  smokeSucceeded: number;
  smokeFailed: number;
  regularTotal: number;
  latestRun: OperatorRunSummary | null;
  latestSmokeRun: OperatorRunSummary | null;
  latestRegularRun: OperatorRunSummary | null;
}

export function operatorRunIsSmoke(run: OperatorRunSummary): boolean {
  return run.runKind?.trim().toLowerCase() === "smoke" || Boolean(run.smokeTestId?.trim());
}

export function operatorRunIsCacheHit(run: OperatorRunSummary): boolean {
  return run.cacheHit === true;
}

function operatorRunCacheState(
  run: OperatorRunSummary,
  detail?: OperatorRunDetail | null,
): {
  key: string | null;
  hit: boolean | null;
  sourceRunId: string | null;
  sourceRunDir: string | null;
} {
  const cache = detail?.document?.cache;
  const cacheObject = cache && typeof cache === "object" && !Array.isArray(cache)
    ? cache as Record<string, unknown>
    : {};
  const text = (value: unknown): string | null =>
    typeof value === "string" && value.trim() ? value : null;
  const hit = typeof cacheObject.hit === "boolean" ? cacheObject.hit : run.cacheHit ?? null;
  return {
    key: text(cacheObject.key) ?? run.cacheKey ?? null,
    hit,
    sourceRunId: text(cacheObject.sourceRunId) ?? run.cacheSourceRunId ?? null,
    sourceRunDir: text(cacheObject.sourceRunDir) ?? run.cacheSourceRunDir ?? null,
  };
}

function operatorRunExportDir(
  run: OperatorRunSummary,
  detail?: OperatorRunDetail | null,
): string | null {
  const value = detail?.document?.exportDir;
  return typeof value === "string" && value.trim() ? value : run.exportDir ?? null;
}

export function operatorStructuredOutputEntries(
  detail?: OperatorRunDetail | null,
): Array<[string, unknown]> {
  const outputs = detail?.document?.structuredOutputs;
  if (!outputs || typeof outputs !== "object" || Array.isArray(outputs)) return [];
  return Object.entries(outputs as Record<string, unknown>);
}

function formatStructuredOutputPreview(value: unknown): string {
  if (typeof value === "string") return value;
  if (typeof value === "number" || typeof value === "boolean") return String(value);
  if (value == null) return "null";
  const raw = JSON.stringify(value);
  if (!raw) return String(value);
  return raw.length > 240 ? `${raw.slice(0, 237)}…` : raw;
}

export function operatorRunDiagnosisSummary(run: OperatorRunSummary): string | null {
  return (
    run.errorMessage?.trim() ||
    run.stderrTail?.trim() ||
    run.suggestedAction?.trim() ||
    null
  );
}

export function operatorRunDiagnosticsPayload(
  run: OperatorRunSummary,
  operator?: OperatorSummary | null,
): string {
  return JSON.stringify(
    {
      operator: operator
        ? {
            id: operator.id,
            version: operator.version,
            sourcePlugin: operator.sourcePlugin,
            aliases: operator.enabledAliases,
            toolNames: operator.enabledAliases.map(operatorToolName),
            manifestPath: operator.manifestPath,
          }
        : {
            alias: run.operatorAlias,
            id: run.operatorId,
            version: run.operatorVersion,
            sourcePlugin: run.sourcePlugin,
          },
      run: {
        runId: run.runId,
        status: run.status,
        location: run.location,
        runKind: run.runKind,
        smokeTestId: run.smokeTestId,
        smokeTestName: run.smokeTestName,
        runDir: run.runDir,
        provenancePath: run.provenancePath,
        exportDir: run.exportDir,
        outputCount: run.outputCount,
        updatedAt: run.updatedAt,
      },
      cache: {
        hit: run.cacheHit,
        key: run.cacheKey,
        sourceRunId: run.cacheSourceRunId,
        sourceRunDir: run.cacheSourceRunDir,
      },
      error: {
        kind: run.errorKind,
        message: run.errorMessage,
        retryable: run.retryable,
        suggestedAction: run.suggestedAction,
        stdoutTail: run.stdoutTail,
        stderrTail: run.stderrTail,
      },
    },
    null,
    2,
  );
}

export function operatorRunBelongsToOperator(
  operator: OperatorSummary,
  run: OperatorRunSummary,
): boolean {
  const runOperatorId = run.operatorId?.trim();
  const runAlias = run.operatorAlias?.trim();
  const aliases = new Set([
    operator.id,
    ...operator.enabledAliases.map((alias) => alias.trim()).filter(Boolean),
  ]);
  if (runOperatorId) {
    if (runOperatorId !== operator.id) return false;
  } else if (runAlias && !aliases.has(runAlias)) {
    return false;
  } else if (!runAlias) {
    return false;
  }
  if (run.sourcePlugin?.trim() && run.sourcePlugin !== operator.sourcePlugin) return false;
  if (run.operatorVersion?.trim() && run.operatorVersion !== operator.version) return false;
  return true;
}

export function operatorRunsForOperator(
  operator: OperatorSummary,
  runs: OperatorRunSummary[],
): OperatorRunSummary[] {
  return runs.filter((run) => operatorRunBelongsToOperator(operator, run));
}

export function operatorRunStats(
  operator: OperatorSummary,
  runs: OperatorRunSummary[],
): OperatorRunStats {
  const operatorRuns = operatorRunsForOperator(operator, runs);
  const stats: OperatorRunStats = {
    total: operatorRuns.length,
    succeeded: 0,
    failed: 0,
    running: 0,
    warning: 0,
    other: 0,
    cacheHits: 0,
    cacheMisses: 0,
    smokeTotal: 0,
    smokeSucceeded: 0,
    smokeFailed: 0,
    regularTotal: 0,
    latestRun: operatorRuns[0] ?? null,
    latestSmokeRun: null,
    latestRegularRun: null,
  };
  let latestTime = Number.NEGATIVE_INFINITY;
  let latestSmokeTime = Number.NEGATIVE_INFINITY;
  let latestRegularTime = Number.NEGATIVE_INFINITY;
  for (const run of operatorRuns) {
    const color = operatorRunStatusColor(run.status);
    if (color === "success") stats.succeeded += 1;
    else if (color === "error") stats.failed += 1;
    else if (color === "info") stats.running += 1;
    else if (color === "warning") stats.warning += 1;
    else stats.other += 1;

    if (run.cacheHit === true) stats.cacheHits += 1;
    else if (run.cacheHit === false) stats.cacheMisses += 1;

    const isSmoke = operatorRunIsSmoke(run);
    if (isSmoke) {
      stats.smokeTotal += 1;
      if (color === "success") stats.smokeSucceeded += 1;
      if (color === "error") stats.smokeFailed += 1;
    } else {
      stats.regularTotal += 1;
    }
    const timestamp = run.updatedAt ? new Date(run.updatedAt).getTime() : Number.NaN;
    const sortValue = Number.isNaN(timestamp) ? Number.NEGATIVE_INFINITY : timestamp;
    if (!stats.latestRun || sortValue > latestTime) {
      stats.latestRun = run;
      latestTime = sortValue;
    }
    if (isSmoke && (!stats.latestSmokeRun || sortValue > latestSmokeTime)) {
      stats.latestSmokeRun = run;
      latestSmokeTime = sortValue;
    }
    if (!isSmoke && (!stats.latestRegularRun || sortValue > latestRegularTime)) {
      stats.latestRegularRun = run;
      latestRegularTime = sortValue;
    }
  }
  return stats;
}

function formatOperatorRunTimestamp(updatedAt?: string | null): string | null {
  if (!updatedAt?.trim()) return null;
  const date = new Date(updatedAt);
  if (Number.isNaN(date.getTime())) return updatedAt;
  return date.toLocaleString();
}

function formatBytes(bytes?: number | null): string {
  if (!Number.isFinite(bytes ?? Number.NaN) || (bytes ?? 0) <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let value = bytes ?? 0;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  const precision = value >= 10 || unit === 0 ? 0 : 1;
  return `${value.toFixed(precision)} ${units[unit]}`;
}

function PluginCard({
  plugin,
  retrievalStatuses = [],
  operators = [],
  busy,
  onInstall,
  onToggle,
  onOperatorRegistrationChange,
  onOpenDetails,
}: {
  plugin: PluginSummary;
  retrievalStatuses?: PluginRetrievalRouteStatus[];
  processPoolStatuses?: PluginProcessPoolRouteStatus[];
  operators?: OperatorSummary[];
  busy: boolean;
  onInstall: (plugin: PluginSummary) => void;
  onToggle: (plugin: PluginSummary, enabled: boolean) => void;
  onOperatorRegistrationChange: (operators: OperatorSummary[], enabled: boolean) => void;
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
  const registeredOperatorCount = operators.filter((operator) => operator.exposed).length;
  const hasOperatorRegistrationControl = plugin.installed && operators.length > 0;
  const operatorRegistrationChecked = registeredOperatorCount > 0;
  const operatorRegistrationLabel = operators.length === 1
    ? operatorRegistrationChecked ? "Registered" : "Not registered"
    : `${registeredOperatorCount}/${operators.length} registered`;
  const operatorIcon = operatorPluginIconSpec(plugin);
  const iconTone = operatorIcon?.color ?? tone;

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
          color: iconTone,
          bgcolor: alpha(iconTone, theme.palette.mode === "dark" ? 0.18 : 0.09),
          border: `1px solid ${alpha(iconTone, theme.palette.mode === "dark" ? 0.22 : 0.12)}`,
          flexShrink: 0,
        }}
      >
        {operatorIcon?.body ? (
          <Box
            component="svg"
            viewBox="0 0 24 24"
            aria-label={`${operatorIcon.label} operator`}
            sx={{ width: 23, height: 23, display: "block" }}
            dangerouslySetInnerHTML={{ __html: operatorIcon.body }}
          />
        ) : (
          <ExtensionRounded fontSize="small" />
        )}
      </Box>

      <Box sx={{ minWidth: 0, flex: 1 }}>
        <Typography variant="subtitle2" fontWeight={800} noWrap title={displayName(plugin)}>
          {displayName(plugin)}
        </Typography>
        <Typography variant="body2" color="text.secondary" noWrap title={subtitle} sx={{ mt: 0.15 }}>
          {subtitle}
        </Typography>
      </Box>

      {hasOperatorRegistrationControl ? (
        <Stack
          direction="row"
          spacing={0.75}
          alignItems="center"
          onClick={(event) => event.stopPropagation()}
          onKeyDown={(event) => event.stopPropagation()}
          sx={{ flexShrink: 0 }}
        >
          <Typography variant="caption" color="text.secondary" fontWeight={750}>
            {operatorRegistrationLabel}
          </Typography>
          <Switch
            size="small"
            checked={operatorRegistrationChecked}
            disabled={busy || !plugin.enabled}
            onChange={(event) => onOperatorRegistrationChange(operators, event.target.checked)}
            inputProps={{ "aria-label": `${operatorRegistrationChecked ? "Unregister" : "Register"} ${displayName(plugin)} operators` }}
          />
        </Stack>
      ) : plugin.installed ? (
        <Box
          aria-label={`${displayName(plugin)} is ${plugin.enabled ? "enabled" : "disabled"}`}
          title={plugin.enabled ? "Enabled" : "Installed but disabled"}
          onClick={(event) => event.stopPropagation()}
          onKeyDown={(event) => event.stopPropagation()}
          sx={{
            flexShrink: 0,
          }}
        >
          <Switch
            size="small"
            checked={plugin.enabled}
            disabled={busy}
            onChange={(event) => onToggle(plugin, event.target.checked)}
            inputProps={{ "aria-label": `${plugin.enabled ? "Disable" : "Enable"} ${displayName(plugin)}` }}
          />
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

function PluginCatalogGroupList({
  plugins,
  retrievalStatusesByPlugin,
  processPoolStatusesByPlugin,
  operatorsByPlugin,
  busy,
  busyPluginIds,
  onInstall,
  onToggle,
  onOperatorRegistrationChange,
  onOpenDetails,
}: {
  plugins: PluginSummary[];
  retrievalStatusesByPlugin: Map<string, PluginRetrievalRouteStatus[]>;
  processPoolStatusesByPlugin: Map<string, PluginProcessPoolRouteStatus[]>;
  operatorsByPlugin: Map<string, OperatorSummary[]>;
  busy: boolean;
  busyPluginIds: Set<string>;
  onInstall: (plugin: PluginSummary) => void;
  onToggle: (plugin: PluginSummary, enabled: boolean) => void;
  onOperatorRegistrationChange: (operators: OperatorSummary[], enabled: boolean) => void;
  onOpenDetails: (plugin: PluginSummary) => void;
}) {
  const groups = groupPluginsByCatalogGroup(plugins);

  return (
    <>
      {groups.map((group) => (
        <Accordion
          key={group.id}
          disableGutters
          elevation={0}
          defaultExpanded={groups.length === 1 || group.id !== "other"}
          sx={nestedAccordionSx}
        >
          <AccordionSummary expandIcon={<ExpandMoreRounded />} sx={nestedAccordionSummarySx}>
            <Stack direction="row" spacing={1} alignItems="center" flexWrap="wrap">
              <Typography variant="subtitle2" fontWeight={800}>
                {group.title}
              </Typography>
              <Chip size="small" variant="outlined" label={`${group.plugins.length} plugins`} />
            </Stack>
          </AccordionSummary>
          <AccordionDetails sx={{ px: 1.5, pt: 0.75, pb: 1.5 }}>
            <Stack spacing={1.5} useFlexGap>
              <Typography variant="caption" color="text.secondary">
                {group.description}
              </Typography>
              {groupPluginsByCatalogSection(group.id, group.plugins).map((section) => (
                <Box key={section.id}>
                  <Typography variant="subtitle2" color="text.secondary" sx={{ mb: 1 }}>
                    {section.title}
                  </Typography>
                  <Box sx={pluginCardGridSx}>
                    {section.plugins.map((plugin) => {
                      const pluginOperators = operatorsByPlugin.get(plugin.id) ?? operatorsByPlugin.get(plugin.name);
                      return (
                        <PluginCard
                          key={plugin.id}
                          plugin={plugin}
                          retrievalStatuses={retrievalStatusesByPlugin.get(plugin.id)}
                          processPoolStatuses={processPoolStatusesByPlugin.get(plugin.id)}
                          operators={pluginOperators}
                          busy={busy || busyPluginIds.has(plugin.id)}
                          onInstall={onInstall}
                          onToggle={onToggle}
                          onOperatorRegistrationChange={onOperatorRegistrationChange}
                          onOpenDetails={onOpenDetails}
                        />
                      );
                    })}
                  </Box>
                </Box>
              ))}
            </Stack>
          </AccordionDetails>
        </Accordion>
      ))}
    </>
  );
}

function OperatorMetricCard({ label, value }: { label: string; value: string | number }) {
  return (
    <Box
      sx={{
        px: 1.15,
        py: 0.85,
        minWidth: 0,
        borderRadius: 1.75,
        bgcolor: "background.paper",
        border: 1,
        borderColor: "divider",
      }}
    >
      <Typography variant="subtitle2" fontWeight={900} sx={{ lineHeight: 1.1 }}>
        {value}
      </Typography>
      <Typography variant="caption" color="text.secondary" fontWeight={750}>
        {label}
      </Typography>
    </Box>
  );
}

function OperatorMetaLine({ label, value }: { label: string; value: string }) {
  return (
    <Stack direction={{ xs: "column", sm: "row" }} spacing={0.75} sx={{ minWidth: 0 }}>
      <Typography
        variant="caption"
        color="text.secondary"
        fontWeight={850}
        sx={{ minWidth: 92, flexShrink: 0 }}
      >
        {label}
      </Typography>
      <Typography
        variant="caption"
        sx={{
          minWidth: 0,
          color: "text.secondary",
          fontFamily: "ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace",
          overflowWrap: "anywhere",
          lineHeight: 1.55,
        }}
      >
        {value}
      </Typography>
    </Stack>
  );
}

type OperatorSchemaEntry = [string, { kind?: string | null; required?: boolean; default?: unknown; description?: string | null }];

function OperatorSchemaAccordion({
  title,
  entries,
  count,
  requiredCount,
  defaultExpanded = false,
  emptyLabel = "None declared",
}: {
  title: string;
  entries: OperatorSchemaEntry[];
  count: number;
  requiredCount?: number;
  defaultExpanded?: boolean;
  emptyLabel?: string;
}) {
  return (
    <Accordion
      disableGutters
      elevation={0}
      defaultExpanded={defaultExpanded}
      sx={{
        border: 1,
        borderColor: "divider",
        borderRadius: 2,
        overflow: "hidden",
        bgcolor: "background.paper",
        "&:before": { display: "none" },
        "&.Mui-expanded": { m: 0 },
      }}
    >
      <AccordionSummary
        expandIcon={<ExpandMoreRounded />}
        sx={{
          px: 1.1,
          minHeight: 44,
          "&.Mui-expanded": { minHeight: 44 },
          "& .MuiAccordionSummary-content": { my: 0.8 },
          "& .MuiAccordionSummary-content.Mui-expanded": { my: 0.8 },
        }}
      >
        <Stack direction="row" gap={0.75} alignItems="center" flexWrap="wrap">
          <Typography variant="caption" color="text.secondary" fontWeight={900} sx={{ textTransform: "uppercase", letterSpacing: 0.4 }}>
            {title}
          </Typography>
          <Chip size="small" variant="outlined" label={`${count} item${count === 1 ? "" : "s"}`} sx={{ height: 22 }} />
          {requiredCount != null && requiredCount > 0 && (
            <Chip size="small" color="warning" variant="outlined" label={`${requiredCount} required`} sx={{ height: 22 }} />
          )}
        </Stack>
      </AccordionSummary>
      <AccordionDetails sx={{ px: 1.1, pt: 0, pb: 1.1 }}>
        <Stack
          spacing={0.6}
          sx={{
            maxHeight: 172,
            overflowY: "auto",
            pr: 0.4,
          }}
        >
          {entries.length > 0 ? entries.map(([name, field]) => (
            <Box
              key={name}
              sx={{
                display: "grid",
                gridTemplateColumns: "minmax(0, 1fr) auto",
                gap: 0.75,
                alignItems: "center",
                px: 0.75,
                py: 0.6,
                borderRadius: 1.4,
                bgcolor: "action.hover",
              }}
            >
              <Box sx={{ minWidth: 0 }}>
                <Typography
                  variant="caption"
                  fontWeight={850}
                  sx={{
                    display: "block",
                    fontFamily: "ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace",
                    overflow: "hidden",
                    textOverflow: "ellipsis",
                    whiteSpace: "nowrap",
                  }}
                  title={name}
                >
                  {name}
                </Typography>
                {field.description?.trim() ? (
                  <Typography
                    variant="caption"
                    color="text.secondary"
                    sx={{ display: "block", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}
                    title={field.description}
                  >
                    {field.description}
                  </Typography>
                ) : null}
              </Box>
              <Stack direction="row" spacing={0.45} alignItems="center" sx={{ justifySelf: "end" }}>
                {field.kind && <Chip size="small" variant="outlined" label={field.kind} sx={{ height: 22 }} />}
                {field.required && <Chip size="small" color="warning" variant="outlined" label="required" sx={{ height: 22 }} />}
                {field.default !== undefined && field.default !== null && (
                  <Chip size="small" variant="outlined" label={`=${previewValue(field.default)}`} sx={{ height: 22 }} />
                )}
              </Stack>
            </Box>
          )) : (
            <Typography variant="caption" color="text.secondary" sx={{ py: 0.75 }}>
              {emptyLabel}
            </Typography>
          )}
        </Stack>
      </AccordionDetails>
    </Accordion>
  );
}

function OperatorResourceAccordion({
  entries,
  defaultExpanded = false,
}: {
  entries: ReturnType<typeof operatorResourceEntries>;
  defaultExpanded?: boolean;
}) {
  return (
    <Accordion
      disableGutters
      elevation={0}
      defaultExpanded={defaultExpanded}
      sx={{
        border: 1,
        borderColor: "divider",
        borderRadius: 2,
        overflow: "hidden",
        bgcolor: "background.paper",
        "&:before": { display: "none" },
        "&.Mui-expanded": { m: 0 },
      }}
    >
      <AccordionSummary
        expandIcon={<ExpandMoreRounded />}
        sx={{
          px: 1.1,
          minHeight: 44,
          "&.Mui-expanded": { minHeight: 44 },
          "& .MuiAccordionSummary-content": { my: 0.8 },
          "& .MuiAccordionSummary-content.Mui-expanded": { my: 0.8 },
        }}
      >
        <Stack direction="row" gap={0.75} alignItems="center" flexWrap="wrap">
          <Typography variant="caption" color="text.secondary" fontWeight={900} sx={{ textTransform: "uppercase", letterSpacing: 0.4 }}>
            Resources
          </Typography>
          <Chip size="small" variant="outlined" label={`${entries.length} item${entries.length === 1 ? "" : "s"}`} sx={{ height: 22 }} />
        </Stack>
      </AccordionSummary>
      <AccordionDetails sx={{ px: 1.1, pt: 0, pb: 1.1 }}>
        <Stack spacing={0.6} sx={{ maxHeight: 172, overflowY: "auto", pr: 0.4 }}>
          {entries.length > 0 ? entries.map(([name, resource]) => (
            <Box
              key={name}
              sx={{
                display: "flex",
                justifyContent: "space-between",
                gap: 1,
                alignItems: "center",
                px: 0.75,
                py: 0.6,
                borderRadius: 1.4,
                bgcolor: "action.hover",
              }}
            >
              <Typography variant="caption" fontWeight={850} sx={{ fontFamily: "ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace" }}>
                {name}
              </Typography>
              <Stack direction="row" spacing={0.45} flexWrap="wrap" justifyContent="flex-end">
                {resource.default != null && <Chip size="small" variant="outlined" label={`default ${previewValue(resource.default)}`} sx={{ height: 22 }} />}
                {resource.min != null && <Chip size="small" variant="outlined" label={`min ${previewValue(resource.min)}`} sx={{ height: 22 }} />}
                {resource.max != null && <Chip size="small" variant="outlined" label={`max ${previewValue(resource.max)}`} sx={{ height: 22 }} />}
              </Stack>
            </Box>
          )) : (
            <Typography variant="caption" color="text.secondary" sx={{ py: 0.75 }}>
              No exposed resources.
            </Typography>
          )}
        </Stack>
      </AccordionDetails>
    </Accordion>
  );
}

function OperatorBundleContentList({ operators }: { operators: OperatorSummary[] }) {
  const theme = useTheme();
  const sorted = [...operators].sort((left, right) =>
    operatorDisplayName(left).localeCompare(operatorDisplayName(right)),
  );
  return (
    <Stack spacing={1.25} sx={{ p: 1.25 }}>
      {sorted.map((operator) => {
        const stats = operatorSchemaStats(operator);
        const script = operatorTemplateScript(operator);
        const inputEntries = operatorFieldEntries(operator.interface?.inputs);
        const paramEntries = operatorFieldEntries(operator.interface?.params);
        const outputEntries = operatorFieldEntries(operator.interface?.outputs);
        const resourceEntries = operatorResourceEntries(operator);
        const icon = operatorImplementationIconSpec(operator);
        const iconTone = icon.color ?? theme.palette.text.secondary;
        return (
          <Paper
            key={operatorCatalogKey(operator)}
            variant="outlined"
            sx={{
              p: 1.5,
              borderRadius: 2.75,
              overflow: "hidden",
              bgcolor: alpha(theme.palette.background.paper, theme.palette.mode === "dark" ? 0.52 : 0.9),
              borderColor: alpha(iconTone, theme.palette.mode === "dark" ? 0.28 : 0.18),
              background: `linear-gradient(135deg, ${alpha(iconTone, theme.palette.mode === "dark" ? 0.15 : 0.09)}, transparent 42%)`,
            }}
          >
            <Stack spacing={1.35}>
              <Stack direction={{ xs: "column", sm: "row" }} spacing={1.35} alignItems={{ xs: "stretch", sm: "flex-start" }}>
                <Box
                  sx={{
                    width: 44,
                    height: 44,
                    borderRadius: 2.2,
                    display: "inline-flex",
                    alignItems: "center",
                    justifyContent: "center",
                    bgcolor: alpha(iconTone, theme.palette.mode === "dark" ? 0.18 : 0.1),
                    color: iconTone,
                    border: `1px solid ${alpha(iconTone, theme.palette.mode === "dark" ? 0.28 : 0.18)}`,
                    flexShrink: 0,
                  }}
                >
                  {icon.body ? (
                    <Box
                      component="svg"
                      viewBox="0 0 24 24"
                      aria-label={`${icon.label} operator`}
                      sx={{ width: 27, height: 27, display: "block" }}
                      dangerouslySetInnerHTML={{ __html: icon.body }}
                    />
                  ) : (
                    <ExtensionRounded fontSize="small" />
                  )}
                </Box>
                <Box sx={{ flex: 1, minWidth: 0 }}>
                  <Stack direction="row" gap={0.75} alignItems="center" flexWrap="wrap">
                    <Typography variant="subtitle1" fontWeight={900} sx={{ lineHeight: 1.2 }}>
                      {operatorDisplayName(operator)}
                    </Typography>
                    <Chip size="small" variant="outlined" label={operator.id} sx={{ maxWidth: 360 }} />
                    <Chip size="small" variant="outlined" label={`v${operator.version}`} />
                    <Chip
                      size="small"
                      color={operator.exposed ? "success" : "default"}
                      variant={operator.exposed ? "filled" : "outlined"}
                      label={operator.exposed ? "Exposed" : "Not exposed"}
                    />
                  </Stack>
                  <Typography variant="body2" color="text.secondary" sx={{ mt: 0.45, lineHeight: 1.5, wordBreak: "break-word" }}>
                    {operator.description?.trim() || "Plugin-defined operator callable by agents as a tool."}
                  </Typography>
                </Box>
              </Stack>

              <Box
                sx={{
                  display: "grid",
                  gridTemplateColumns: { xs: "repeat(2, minmax(0, 1fr))", md: "repeat(5, minmax(0, 1fr))" },
                  gap: 0.75,
                }}
              >
                <OperatorMetricCard label="inputs" value={`${stats.inputs}/${stats.requiredInputs}`} />
                <OperatorMetricCard label="params" value={stats.params} />
                <OperatorMetricCard label="outputs" value={stats.outputs} />
                <OperatorMetricCard label="resources" value={stats.resources} />
                <OperatorMetricCard label="smoke" value={operator.smokeTests?.length ?? 0} />
              </Box>

              <Paper
                variant="outlined"
                sx={{
                  p: 1.15,
                  borderRadius: 2,
                  bgcolor: alpha(theme.palette.common.black, theme.palette.mode === "dark" ? 0.12 : 0.03),
                  borderColor: alpha(theme.palette.divider, 0.8),
                }}
              >
                <Stack spacing={0.65}>
                  <OperatorMetaLine label="Script" value={script || "not declared"} />
                  <OperatorMetaLine label="Environment" value={operatorRuntimeSummary(operator)} />
                  <OperatorMetaLine label="Command" value={operatorExecutionCommand(operator)} />
                  <OperatorMetaLine label="Manifest" value={operator.manifestPath} />
                </Stack>
              </Paper>

              <Box
                sx={{
                  display: "grid",
                  gridTemplateColumns: "1fr",
                  gap: 1,
                }}
              >
                <OperatorSchemaAccordion
                  title="Inputs"
                  entries={inputEntries}
                  count={stats.inputs}
                  requiredCount={stats.requiredInputs}
                  defaultExpanded
                />
                <OperatorSchemaAccordion
                  title="Params"
                  entries={paramEntries}
                  count={stats.params}
                  requiredCount={stats.requiredParams}
                />
                <OperatorSchemaAccordion
                  title="Outputs"
                  entries={outputEntries}
                  count={stats.outputs}
                  requiredCount={outputEntries.filter(([, field]) => field.required).length}
                />
                <OperatorResourceAccordion entries={resourceEntries} />
              </Box>
            </Stack>
          </Paper>
        );
      })}
    </Stack>
  );
}

function PluginDetailsDialog({
  plugin,
  open,
  retrievalStatuses = [],
  processPoolStatuses = [],
  operators = [],
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
  operators?: OperatorSummary[];
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
  const hasOperatorContent = operators.length > 0;
  const installable = plugin.installPolicy !== "NOT_AVAILABLE";
  const isNotebookHelper = isNotebookPlugin(plugin);
  const primaryPrompt = plugin.interface?.defaultPrompt?.[0] ?? null;
  const runtimeSummary = pluginRuntimeSummary(
    plugin,
    retrievalStatuses,
    processPoolStatuses,
  );
  const operatorIcon = operatorPluginIconSpec(plugin);
  const detailIconTone = operatorIcon?.color
    ?? (plugin.installed && plugin.enabled ? theme.palette.success.main : theme.palette.text.secondary);
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
                color: detailIconTone,
                bgcolor: alpha(detailIconTone, theme.palette.mode === "dark" ? 0.16 : 0.07),
                border: `1px solid ${alpha(detailIconTone, theme.palette.mode === "dark" ? 0.2 : 0.11)}`,
                flexShrink: 0,
              }}
            >
              {operatorIcon?.body ? (
                <Box
                  component="svg"
                  viewBox="0 0 24 24"
                  aria-label={`${operatorIcon.label} operator`}
                  sx={{ width: 34, height: 34, display: "block" }}
                  dangerouslySetInnerHTML={{ __html: operatorIcon.body }}
                />
              ) : (
                <ExtensionRounded sx={{ fontSize: 34 }} />
              )}
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

          {isNotebookHelper && (
            <Stack spacing={1.25}>
              <Typography variant="subtitle1" fontWeight={850}>
                Notebook viewer settings
              </Typography>
              <Paper variant="outlined" sx={{ p: 1.5, borderRadius: 2.5 }}>
                <NotebookViewerSettingsPanel showIntro={false} />
              </Paper>
            </Stack>
          )}

          <Stack spacing={1.25}>
            <Typography variant="subtitle1" fontWeight={850}>
              {declaredRetrievalSources.length > 0 ? "Routes" : hasOperatorContent ? "Operators" : "Included content"}
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
                ) : hasOperatorContent ? (
                  <OperatorBundleContentList operators={operators} />
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

function OperatorRunDetailsDialog({
  run,
  detail,
  log,
  verification,
  loading,
  logLoading,
  verifying,
  error,
  onClose,
  onLoadLog,
  onVerify,
  onCopy,
}: {
  run: OperatorRunSummary | null;
  detail: OperatorRunDetail | null;
  log: OperatorRunLog | null;
  verification: OperatorRunVerification | null;
  loading: boolean;
  logLoading: "stdout" | "stderr" | null;
  verifying: boolean;
  error: string | null;
  onClose: () => void;
  onLoadLog: (logName: "stdout" | "stderr") => void;
  onVerify: () => void;
  onCopy: (text: string, successMessage: string) => void;
}) {
  if (!run) return null;
  const detailJson = detail ? JSON.stringify(detail.document, null, 2) : "";
  const structuredOutputEntries = operatorStructuredOutputEntries(detail);
  const structuredOutputsJson = structuredOutputEntries.length > 0
    ? JSON.stringify(Object.fromEntries(structuredOutputEntries), null, 2)
    : "";
  const cacheState = operatorRunCacheState(run, detail);
  const exportDir = operatorRunExportDir(run, detail);
  return (
    <Dialog open={Boolean(run)} onClose={onClose} fullWidth maxWidth="md" aria-labelledby="operator-run-details-title">
      <DialogTitle id="operator-run-details-title" sx={{ px: 3, py: 2, pr: 7 }}>
        <Stack direction="row" spacing={1} alignItems="center" flexWrap="wrap">
          <Typography variant="body2" color="text.secondary">
            Operator run
          </Typography>
          <Typography variant="body2" color="text.secondary">
            ›
          </Typography>
          <Typography variant="body2" fontWeight={850} sx={{ wordBreak: "break-all" }}>
            {run.runId}
          </Typography>
        </Stack>
        <IconButton
          aria-label="Close operator run details"
          onClick={onClose}
          sx={{ position: "absolute", right: 12, top: 10 }}
        >
          <CloseRounded />
        </IconButton>
      </DialogTitle>
      <DialogContent sx={{ px: 3, pt: 2, pb: 3 }}>
        <Stack spacing={1.5} useFlexGap>
          <Stack direction="row" gap={0.75} alignItems="center" flexWrap="wrap">
            <Typography variant="subtitle1" fontWeight={850}>
              {operatorRunTitle(run)}
            </Typography>
            <Chip
              size="small"
              color={operatorRunStatusColor(run.status)}
              variant={operatorRunStatusColor(run.status) === "default" ? "outlined" : "filled"}
              label={run.status}
            />
            <Chip size="small" variant="outlined" label={run.location} />
            {operatorRunIsSmoke(run) && (
              <Chip
                size="small"
                color="info"
                variant="outlined"
                label={run.smokeTestName || run.smokeTestId || "smoke"}
              />
            )}
            {run.outputCount > 0 && (
              <Chip size="small" color="success" variant="outlined" label={`${run.outputCount} output${run.outputCount === 1 ? "" : "s"}`} />
            )}
            {(run.structuredOutputCount ?? 0) > 0 && (
              <Chip size="small" color="info" variant="outlined" label={`${run.structuredOutputCount} structured`} />
            )}
            {cacheState.hit === true && (
              <Chip size="small" color="success" variant="outlined" label="cache hit" />
            )}
            {cacheState.hit === false && (
              <Chip size="small" variant="outlined" label="cache miss" />
            )}
          </Stack>
          <Typography variant="caption" color="text.secondary" sx={{ wordBreak: "break-all" }}>
            Run dir: {detail?.runDir || run.runDir}
          </Typography>
          {exportDir && (
            <Typography variant="caption" color="text.secondary" sx={{ wordBreak: "break-all" }}>
              Exported results: {exportDir}
            </Typography>
          )}
          {detail?.sourcePath && (
            <Typography variant="caption" color="text.secondary" sx={{ wordBreak: "break-all" }}>
              Source: {detail.sourcePath}
            </Typography>
          )}
          {cacheState.hit === true && (
            <Alert severity="info" sx={{ borderRadius: 2 }}>
              <Stack spacing={0.5}>
                <Typography variant="body2" fontWeight={850}>
                  Reused a previous operator result
                </Typography>
                <Typography variant="caption" sx={{ wordBreak: "break-all" }}>
                  Source run: {cacheState.sourceRunId || "unknown"}
                </Typography>
                {cacheState.sourceRunDir && (
                  <Typography variant="caption" sx={{ wordBreak: "break-all" }}>
                    Source dir: {cacheState.sourceRunDir}
                  </Typography>
                )}
                {cacheState.key && (
                  <Typography variant="caption" color="text.secondary" sx={{ wordBreak: "break-all" }}>
                    Cache key: {cacheState.key}
                  </Typography>
                )}
              </Stack>
            </Alert>
          )}
          {loading && (
            <Stack direction="row" spacing={1} alignItems="center">
              <CircularProgress size={16} />
              <Typography variant="body2" color="text.secondary">
                Loading run detail…
              </Typography>
            </Stack>
          )}
          {error && <Alert severity="warning" sx={{ borderRadius: 2 }}>{error}</Alert>}
          {structuredOutputEntries.length > 0 && (
            <Paper variant="outlined" sx={{ borderRadius: 2, overflow: "hidden" }}>
              <Stack direction="row" spacing={1} alignItems="center" justifyContent="space-between" sx={{ px: 1.25, py: 0.75, bgcolor: "action.hover" }}>
                <Typography variant="caption" fontWeight={850}>
                  Structured outputs
                </Typography>
                <Button
                  size="small"
                  variant="text"
                  startIcon={<ContentCopyRounded />}
                  onClick={() => onCopy(structuredOutputsJson, `Copied ${run.runId} structured outputs`)}
                  sx={{ textTransform: "none", borderRadius: 1.5 }}
                >
                  Copy
                </Button>
              </Stack>
              <Stack spacing={0.75} sx={{ p: 1.25 }}>
                {structuredOutputEntries.map(([name, value]) => (
                  <Box key={name} sx={{ p: 0.85, borderRadius: 1.5, bgcolor: "action.hover" }}>
                    <Typography variant="caption" fontWeight={850} sx={{ display: "block", wordBreak: "break-all" }}>
                      {name}
                    </Typography>
                    <Typography variant="caption" color="text.secondary" sx={{ display: "block", wordBreak: "break-word", whiteSpace: "pre-wrap" }}>
                      {formatStructuredOutputPreview(value)}
                    </Typography>
                  </Box>
                ))}
              </Stack>
            </Paper>
          )}
          {operatorRunStatusColor(run.status) === "error" && (
            <Alert severity="error" sx={{ borderRadius: 2 }}>
              <Stack spacing={0.75}>
                <Typography variant="body2" fontWeight={850}>
                  Run failed
                </Typography>
                {run.errorMessage && (
                  <Typography variant="caption" sx={{ wordBreak: "break-word" }}>
                    {run.errorMessage}
                  </Typography>
                )}
                {run.suggestedAction && (
                  <Typography variant="caption" sx={{ wordBreak: "break-word" }}>
                    Suggested action: {run.suggestedAction}
                  </Typography>
                )}
                {run.stderrTail && (
                  <Box
                    component="pre"
                    sx={{
                      m: 0,
                      p: 0.75,
                      maxHeight: 120,
                      overflow: "auto",
                      borderRadius: 1,
                      bgcolor: "background.paper",
                      fontSize: 12,
                      whiteSpace: "pre-wrap",
                      wordBreak: "break-word",
                    }}
                  >
                    {run.stderrTail}
                  </Box>
                )}
                <Button
                  size="small"
                  variant="outlined"
                  startIcon={<ContentCopyRounded />}
                  onClick={() => onCopy(operatorRunDiagnosticsPayload(run), `Copied ${run.runId} diagnostics`)}
                  sx={{ alignSelf: "flex-start", textTransform: "none", borderRadius: 1.5 }}
                >
                  Copy diagnosis
                </Button>
              </Stack>
            </Alert>
          )}
          {detail && (
            <Paper variant="outlined" sx={{ borderRadius: 2, overflow: "hidden" }}>
              <Stack direction="row" spacing={1} alignItems="center" justifyContent="space-between" sx={{ px: 1.25, py: 0.75, bgcolor: "action.hover" }}>
                <Typography variant="caption" fontWeight={850}>
                  provenance/status JSON
                </Typography>
                <Button
                  size="small"
                  variant="text"
                  startIcon={<ContentCopyRounded />}
                  onClick={() => onCopy(detailJson, `Copied ${run.runId} detail`)}
                  sx={{ textTransform: "none", borderRadius: 1.5 }}
                >
                  Copy
                </Button>
              </Stack>
              <Box
                component="pre"
                sx={{
                  m: 0,
                  p: 1.25,
                  maxHeight: 280,
                  overflow: "auto",
                  fontSize: 12,
                  lineHeight: 1.5,
                  whiteSpace: "pre-wrap",
                  wordBreak: "break-word",
                }}
              >
                {detailJson}
              </Box>
            </Paper>
          )}
          <Stack direction="row" gap={1} flexWrap="wrap">
            <Button
              size="small"
              variant="contained"
              disableElevation
              disabled={verifying}
              startIcon={verifying ? <CircularProgress size={14} color="inherit" /> : <TroubleshootRounded />}
              onClick={onVerify}
              sx={{ textTransform: "none", borderRadius: 1.5 }}
            >
              Verify artifacts/logs
            </Button>
            {(["stdout", "stderr"] as const).map((logName) => (
              <Button
                key={logName}
                size="small"
                variant="outlined"
                disabled={Boolean(logLoading)}
                startIcon={logLoading === logName ? <CircularProgress size={14} /> : <TroubleshootRounded />}
                onClick={() => onLoadLog(logName)}
                sx={{ textTransform: "none", borderRadius: 1.5 }}
              >
                Load {logName}
              </Button>
            ))}
          </Stack>
          {verification && (
            <Paper variant="outlined" sx={{ borderRadius: 2, overflow: "hidden" }}>
              <Stack direction="row" spacing={1} alignItems="center" justifyContent="space-between" sx={{ px: 1.25, py: 0.75, bgcolor: "action.hover" }}>
                <Stack direction="row" gap={0.75} alignItems="center" flexWrap="wrap">
                  <Typography variant="caption" fontWeight={850}>
                    Verification
                  </Typography>
                  <Chip
                    size="small"
                    color={verification.ok ? "success" : "error"}
                    variant={verification.ok ? "filled" : "outlined"}
                    label={verification.ok ? "ok" : "issues"}
                  />
                  <Chip size="small" variant="outlined" label={verification.location} />
                </Stack>
              </Stack>
              <Stack spacing={0.75} sx={{ p: 1.25 }}>
                {verification.checks.map((check) => (
                  <Box key={`${check.name}:${check.path ?? ""}:${check.message}`} sx={{ p: 0.75, borderRadius: 1.5, bgcolor: "background.paper", border: 1, borderColor: "divider" }}>
                    <Stack direction="row" gap={0.75} alignItems="center" flexWrap="wrap">
                      <Chip
                        size="small"
                        color={check.ok ? "success" : check.severity === "warning" ? "warning" : "error"}
                        variant={check.ok ? "outlined" : "filled"}
                        label={check.ok ? "ok" : check.severity}
                      />
                      <Typography variant="caption" fontWeight={850} sx={{ wordBreak: "break-all" }}>
                        {check.name}
                      </Typography>
                    </Stack>
                    <Typography variant="caption" color="text.secondary" sx={{ display: "block", mt: 0.35, wordBreak: "break-word" }}>
                      {check.message}
                    </Typography>
                    {check.path && (
                      <Typography variant="caption" color="text.secondary" sx={{ display: "block", mt: 0.35, wordBreak: "break-all" }}>
                        {check.path}
                      </Typography>
                    )}
                  </Box>
                ))}
              </Stack>
            </Paper>
          )}
          {log && (
            <Paper variant="outlined" sx={{ borderRadius: 2, overflow: "hidden" }}>
              <Stack direction="row" spacing={1} alignItems="center" justifyContent="space-between" sx={{ px: 1.25, py: 0.75, bgcolor: "action.hover" }}>
                <Stack direction="row" gap={0.75} alignItems="center" flexWrap="wrap">
                  <Typography variant="caption" fontWeight={850}>
                    {log.logName}
                  </Typography>
                  <Chip size="small" variant="outlined" label={log.location} />
                </Stack>
                <Button
                  size="small"
                  variant="text"
                  startIcon={<ContentCopyRounded />}
                  onClick={() => onCopy(log.content, `Copied ${run.runId} ${log.logName}`)}
                  sx={{ textTransform: "none", borderRadius: 1.5 }}
                >
                  Copy
                </Button>
              </Stack>
              <Typography variant="caption" color="text.secondary" sx={{ display: "block", px: 1.25, pt: 0.75, wordBreak: "break-all" }}>
                {log.path}
              </Typography>
              <Box
                component="pre"
                sx={{
                  m: 0,
                  p: 1.25,
                  maxHeight: 260,
                  overflow: "auto",
                  fontSize: 12,
                  lineHeight: 1.5,
                  whiteSpace: "pre-wrap",
                  wordBreak: "break-word",
                }}
              >
                {log.content || "(empty log)"}
              </Box>
            </Paper>
          )}
        </Stack>
      </DialogContent>
    </Dialog>
  );
}

function OperatorDetailsDialog({
  operator,
  runs,
  busy,
  onClose,
  onOpenRun,
  onCleanupRuns,
  onCopy,
}: {
  operator: OperatorSummary | null;
  runs: OperatorRunSummary[];
  busy: boolean;
  onClose: () => void;
  onOpenRun: (run: OperatorRunSummary) => void;
  onCleanupRuns: (operator: OperatorSummary) => void;
  onCopy: (text: string, successMessage: string) => void;
}) {
  if (!operator) return null;
  const title = operatorDisplayName(operator);
  const aliases = operator.enabledAliases.filter((value) => value.trim().length > 0);
  const smokeTests = operator.smokeTests ?? [];
  const operatorRuns = operatorRunsForOperator(operator, runs);
  const stats = operatorRunStats(operator, runs);
  const latestRun = stats.latestRun;
  const latestFailedRun = operatorRuns.find((run) => operatorRunStatusColor(run.status) === "error") ?? null;
  return (
    <Dialog open={Boolean(operator)} onClose={onClose} fullWidth maxWidth="md" aria-labelledby="operator-details-title">
      <DialogTitle id="operator-details-title" sx={{ px: 3, py: 2, pr: 7 }}>
        <Stack direction="row" spacing={1} alignItems="center" flexWrap="wrap">
          <Typography variant="body2" color="text.secondary">
            Operator
          </Typography>
          <Typography variant="body2" color="text.secondary">
            ›
          </Typography>
          <Typography variant="body2" fontWeight={850} sx={{ wordBreak: "break-all" }}>
            {title}
          </Typography>
        </Stack>
        <IconButton
          aria-label="Close operator details"
          onClick={onClose}
          sx={{ position: "absolute", right: 12, top: 10 }}
        >
          <CloseRounded />
        </IconButton>
      </DialogTitle>
      <DialogContent sx={{ px: 3, pt: 2, pb: 3 }}>
        <Stack spacing={1.5} useFlexGap>
          <Stack direction="row" gap={0.75} alignItems="center" flexWrap="wrap">
            <Typography variant="subtitle1" fontWeight={850}>
              {title}
            </Typography>
            <Chip size="small" variant="outlined" label={operator.id} />
            <Chip size="small" variant="outlined" label={`v${operator.version}`} />
            <Chip
              size="small"
              color={operator.exposed ? "success" : "default"}
              variant={operator.exposed ? "filled" : "outlined"}
              label={operator.exposed ? "Exposed" : "Not exposed"}
            />
          </Stack>
          <Typography variant="body2" color="text.secondary">
            {operator.description?.trim() || "Plugin-defined operator callable by agents as a tool."}
          </Typography>
          <Stack spacing={0.5}>
            <Typography variant="caption" color="text.secondary" sx={{ wordBreak: "break-all" }}>
              Source plugin: {operator.sourcePlugin}
            </Typography>
            <Typography variant="caption" color="text.secondary" sx={{ wordBreak: "break-all" }}>
              Manifest: {operator.manifestPath}
            </Typography>
          </Stack>

          <Paper variant="outlined" sx={{ p: 1.25, borderRadius: 2 }}>
            <Stack spacing={1}>
              <Typography variant="caption" fontWeight={850}>
                Run status
              </Typography>
              <Stack direction="row" gap={0.75} flexWrap="wrap">
                <Chip size="small" variant="outlined" label={`${stats.total} calls`} />
                <Chip size="small" color="success" variant="outlined" label={`${stats.succeeded} succeeded`} />
                <Chip size="small" color={stats.failed > 0 ? "error" : "default"} variant="outlined" label={`${stats.failed} failed`} />
                <Chip size="small" variant="outlined" label={`${stats.regularTotal} regular`} />
                <Chip
                  size="small"
                  color={stats.smokeFailed > 0 ? "error" : stats.smokeSucceeded > 0 ? "success" : "default"}
                  variant="outlined"
                  label={`${stats.smokeTotal} smoke`}
                />
                {stats.running > 0 && <Chip size="small" color="info" variant="outlined" label={`${stats.running} running`} />}
                {stats.warning > 0 && <Chip size="small" color="warning" variant="outlined" label={`${stats.warning} warning`} />}
                {stats.other > 0 && <Chip size="small" variant="outlined" label={`${stats.other} other`} />}
                {stats.cacheHits > 0 && (
                  <Chip size="small" color="success" variant="outlined" label={`${stats.cacheHits} cache hits`} />
                )}
                {stats.cacheMisses > 0 && (
                  <Chip size="small" variant="outlined" label={`${stats.cacheMisses} cache misses`} />
                )}
                {latestRun && (
                  <Chip
                    size="small"
                    color={operatorRunStatusColor(latestRun.status)}
                    variant={operatorRunStatusColor(latestRun.status) === "default" ? "outlined" : "filled"}
                    label={`latest ${latestRun.status}`}
                  />
                )}
              </Stack>
              {latestRun ? (
                <Typography variant="caption" color="text.secondary" sx={{ wordBreak: "break-all" }}>
                  Latest: {formatOperatorRunTimestamp(latestRun.updatedAt) ?? "unknown time"} · {latestRun.runId} · {latestRun.runDir}
                </Typography>
              ) : (
                <Typography variant="caption" color="text.secondary">
                  No runs recorded for this operator on the current execution surface.
                </Typography>
              )}
              {stats.latestSmokeRun && (
                <Typography variant="caption" color="text.secondary" sx={{ wordBreak: "break-all" }}>
                  Latest smoke: {stats.latestSmokeRun.smokeTestName || stats.latestSmokeRun.smokeTestId || "smoke"} · {stats.latestSmokeRun.status} · {stats.latestSmokeRun.runId}
                </Typography>
              )}
              {stats.latestRegularRun && (
                <Typography variant="caption" color="text.secondary" sx={{ wordBreak: "break-all" }}>
                  Latest regular: {stats.latestRegularRun.status} · {stats.latestRegularRun.runId}
                </Typography>
              )}
              <Button
                size="small"
                variant="outlined"
                color="warning"
                startIcon={<DeleteOutlineRounded />}
                disabled={busy || operatorRuns.length === 0}
                onClick={() => onCleanupRuns(operator)}
                sx={{ alignSelf: "flex-start", textTransform: "none", borderRadius: 1.5 }}
              >
                Clean this operator's old/cache runs
              </Button>
            </Stack>
          </Paper>

          {latestFailedRun && (
            <Alert severity="error" sx={{ borderRadius: 2 }}>
              <Stack spacing={0.75}>
                <Stack direction="row" gap={0.75} alignItems="center" flexWrap="wrap">
                  <Typography variant="body2" fontWeight={850}>
                    Latest failure
                  </Typography>
                  <Chip size="small" variant="outlined" label={latestFailedRun.runId} />
                  {latestFailedRun.errorKind && (
                    <Chip size="small" variant="outlined" label={latestFailedRun.errorKind} />
                  )}
                  {latestFailedRun.retryable != null && (
                    <Chip
                      size="small"
                      color={latestFailedRun.retryable ? "warning" : "default"}
                      variant="outlined"
                      label={latestFailedRun.retryable ? "retryable" : "not retryable"}
                    />
                  )}
                </Stack>
                {latestFailedRun.errorMessage && (
                  <Typography variant="caption" sx={{ wordBreak: "break-word" }}>
                    {latestFailedRun.errorMessage}
                  </Typography>
                )}
                {latestFailedRun.suggestedAction && (
                  <Typography variant="caption" sx={{ wordBreak: "break-word" }}>
                    Suggested action: {latestFailedRun.suggestedAction}
                  </Typography>
                )}
                {latestFailedRun.stderrTail && (
                  <Box
                    component="pre"
                    sx={{
                      m: 0,
                      p: 0.75,
                      maxHeight: 120,
                      overflow: "auto",
                      borderRadius: 1,
                      bgcolor: "background.paper",
                      fontSize: 12,
                      whiteSpace: "pre-wrap",
                      wordBreak: "break-word",
                    }}
                  >
                    {latestFailedRun.stderrTail}
                  </Box>
                )}
                <Stack direction="row" gap={0.75} flexWrap="wrap">
                  <Button
                    size="small"
                    variant="outlined"
                    startIcon={<ContentCopyRounded />}
                    onClick={() => onCopy(operatorRunDiagnosticsPayload(latestFailedRun, operator), `Copied ${latestFailedRun.runId} diagnostics`)}
                    sx={{ textTransform: "none", borderRadius: 1.5 }}
                  >
                    Copy diagnosis
                  </Button>
                  <Button
                    size="small"
                    variant="outlined"
                    startIcon={<TroubleshootRounded />}
                    onClick={() => {
                      onClose();
                      onOpenRun(latestFailedRun);
                    }}
                    sx={{ textTransform: "none", borderRadius: 1.5 }}
                  >
                    Open failed run
                  </Button>
                </Stack>
              </Stack>
            </Alert>
          )}

          <Paper variant="outlined" sx={{ p: 1.25, borderRadius: 2 }}>
            <Stack spacing={1}>
              <Typography variant="caption" fontWeight={850}>
                Tool aliases and smoke tests
              </Typography>
              <Stack direction="row" gap={0.75} flexWrap="wrap">
                {(aliases.length > 0 ? aliases : [operatorPrimaryAlias(operator)]).map((alias) => (
                  <Chip key={alias} size="small" variant="outlined" label={operatorToolName(alias)} />
                ))}
                <Chip size="small" variant="outlined" label={`${smokeTests.length} smoke ${smokeTests.length === 1 ? "test" : "tests"}`} />
              </Stack>
              {smokeTests.length > 0 && (
                <Stack spacing={0.75}>
                  {smokeTests.map((smokeTest) => (
                    <Box key={smokeTest.id}>
                      <Typography variant="caption" fontWeight={800} sx={{ display: "block" }}>
                        {smokeTest.name?.trim() || smokeTest.id}
                      </Typography>
                      {smokeTest.description?.trim() && (
                        <Typography variant="caption" color="text.secondary" sx={{ display: "block", wordBreak: "break-word" }}>
                          {smokeTest.description}
                        </Typography>
                      )}
                    </Box>
                  ))}
                </Stack>
              )}
            </Stack>
          </Paper>

          <Paper variant="outlined" sx={{ p: 1.25, borderRadius: 2 }}>
            <Stack spacing={1}>
              <Typography variant="caption" fontWeight={850}>
                Recent operator runs
              </Typography>
              {operatorRuns.length === 0 ? (
                <Typography variant="caption" color="text.secondary">
                  No matching runs yet.
                </Typography>
              ) : (
                <Stack spacing={0.75}>
                  {operatorRuns.slice(0, 8).map((run) => (
                    <Box key={run.runId} sx={{ p: 1, borderRadius: 1.5, border: 1, borderColor: "divider" }}>
                      <Stack spacing={0.5}>
                        <Stack direction="row" gap={0.75} alignItems="center" flexWrap="wrap">
                          <Typography variant="body2" fontWeight={850} sx={{ wordBreak: "break-all" }}>
                            {run.runId}
                          </Typography>
                          <Chip
                            size="small"
                            color={operatorRunStatusColor(run.status)}
                            variant={operatorRunStatusColor(run.status) === "default" ? "outlined" : "filled"}
                            label={run.status}
                          />
                          <Chip size="small" variant="outlined" label={run.location} />
                          {operatorRunIsSmoke(run) && (
                            <Chip
                              size="small"
                              color="info"
                              variant="outlined"
                              label={run.smokeTestName || run.smokeTestId || "smoke"}
                            />
                          )}
                          {run.outputCount > 0 && (
                            <Chip size="small" color="success" variant="outlined" label={`${run.outputCount} output${run.outputCount === 1 ? "" : "s"}`} />
                          )}
                          {(run.structuredOutputCount ?? 0) > 0 && (
                            <Chip size="small" color="info" variant="outlined" label={`${run.structuredOutputCount} structured`} />
                          )}
                          {operatorRunIsCacheHit(run) && (
                            <Chip size="small" color="success" variant="outlined" label="cache hit" />
                          )}
                        </Stack>
                        {run.errorMessage && (
                          <Typography variant="caption" color="error.main" sx={{ wordBreak: "break-word" }}>
                            {run.errorMessage}
                          </Typography>
                        )}
                        {run.suggestedAction && operatorRunStatusColor(run.status) === "error" && (
                          <Typography variant="caption" color="text.secondary" sx={{ wordBreak: "break-word" }}>
                            Suggested action: {run.suggestedAction}
                          </Typography>
                        )}
                        {run.stderrTail && operatorRunStatusColor(run.status) === "error" && (
                          <Box
                            component="pre"
                            sx={{
                              m: 0,
                              p: 0.75,
                              maxHeight: 90,
                              overflow: "auto",
                              borderRadius: 1,
                              bgcolor: "action.hover",
                              fontSize: 12,
                              whiteSpace: "pre-wrap",
                              wordBreak: "break-word",
                            }}
                          >
                            {run.stderrTail}
                          </Box>
                        )}
                        <Typography variant="caption" color="text.secondary" sx={{ wordBreak: "break-all" }}>
                          {(formatOperatorRunTimestamp(run.updatedAt) ?? "unknown time")} · {run.runDir}
                        </Typography>
                        {operatorRunIsCacheHit(run) && (
                          <Typography variant="caption" color="text.secondary" sx={{ wordBreak: "break-all" }}>
                            Reused source run {run.cacheSourceRunId || "unknown"}{run.cacheSourceRunDir ? ` · ${run.cacheSourceRunDir}` : ""}
                          </Typography>
                        )}
                        <Stack direction="row" gap={0.75} flexWrap="wrap">
                          <Button
                            size="small"
                            variant="text"
                            startIcon={<TroubleshootRounded />}
                            onClick={() => {
                              onClose();
                              onOpenRun(run);
                            }}
                            sx={{ alignSelf: "flex-start", textTransform: "none", borderRadius: 1.5 }}
                          >
                            View run detail
                          </Button>
                          {operatorRunStatusColor(run.status) === "error" && (
                            <Button
                              size="small"
                              variant="text"
                              startIcon={<ContentCopyRounded />}
                              onClick={() => onCopy(operatorRunDiagnosticsPayload(run, operator), `Copied ${run.runId} diagnostics`)}
                              sx={{ alignSelf: "flex-start", textTransform: "none", borderRadius: 1.5 }}
                            >
                              Copy diagnosis
                            </Button>
                          )}
                        </Stack>
                      </Stack>
                    </Box>
                  ))}
                </Stack>
              )}
            </Stack>
          </Paper>
        </Stack>
      </DialogContent>
    </Dialog>
  );
}

function OperatorCatalogSection({
  operators,
  diagnostics,
  runs,
  registryPath,
  busy,
  onToggle,
  onSmokeRun,
  onRefreshRuns,
  onCleanupRuns,
  onOpenRun,
  onCopy,
}: {
  operators: OperatorSummary[];
  diagnostics: OperatorManifestDiagnostic[];
  runs: OperatorRunSummary[];
  registryPath: string | null;
  busy: boolean;
  onToggle: (operator: OperatorSummary, enabled: boolean) => void;
  onSmokeRun: (operator: OperatorSummary, smokeTestId?: string | null) => void;
  onRefreshRuns: () => void;
  onCleanupRuns: (operator?: OperatorSummary) => void;
  onOpenRun: (run: OperatorRunSummary) => void;
  onCopy: (text: string, successMessage: string) => void;
}) {
  const theme = useTheme();
  const [selectedSmokeTests, setSelectedSmokeTests] = useState<Record<string, string>>({});
  const [detailOperator, setDetailOperator] = useState<OperatorSummary | null>(null);
  const sortedOperators = [...operators].sort((left, right) =>
    left.id
      .localeCompare(right.id)
      || left.sourcePlugin.localeCompare(right.sourcePlugin)
      || left.version.localeCompare(right.version),
  );
  const exposedCount = operators.filter((operator) => operator.exposed).length;
  const unavailableCount = operators.filter((operator) => operator.unavailableReason).length;
  const failedRunCount = runs.filter((run) => operatorRunStatusColor(run.status) === "error").length;
  const cacheHitCount = runs.filter(operatorRunIsCacheHit).length;
  const diagnosticIssueCount = diagnostics.filter((diagnostic) => diagnostic.severity !== "info").length;

  return (
    <>
      <OperatorDetailsDialog
        operator={detailOperator}
        runs={runs}
        busy={busy}
        onClose={() => setDetailOperator(null)}
        onOpenRun={onOpenRun}
        onCleanupRuns={onCleanupRuns}
        onCopy={onCopy}
      />
      <Accordion disableGutters elevation={0} sx={accordionSx}>
      <AccordionSummary expandIcon={<ExpandMoreRounded />} sx={accordionSummarySx}>
        <Stack direction="row" spacing={1} alignItems="center" flexWrap="wrap">
          <Typography variant="subtitle2" fontWeight={700}>Operators</Typography>
          <Chip size="small" variant="outlined" label={`${exposedCount} exposed`} />
          <Chip size="small" variant="outlined" label={`${operators.length} discovered`} />
          {diagnosticIssueCount > 0 && (
            <Chip size="small" color="warning" variant="filled" label={`${diagnosticIssueCount} manifest issues`} />
          )}
          {runs.length > 0 && (
            <Chip size="small" variant="outlined" label={`${runs.length} runs`} />
          )}
          {cacheHitCount > 0 && (
            <Chip size="small" color="success" variant="outlined" label={`${cacheHitCount} cache hits`} />
          )}
          {unavailableCount > 0 && (
            <Chip size="small" color="warning" variant="filled" label={`${unavailableCount} unavailable`} />
          )}
          {failedRunCount > 0 && (
            <Chip size="small" color="error" variant="filled" label={`${failedRunCount} failed runs`} />
          )}
        </Stack>
      </AccordionSummary>
      <AccordionDetails sx={{ px: 2, pt: 0.75, pb: 2 }}>
        <Stack spacing={1.25} useFlexGap>
          <Typography variant="body2" color="text.secondary">
            Operators are plugin-defined tools that agents can call directly after exposure. Runtime follows the current session environment; the registry stays local.
          </Typography>
          {registryPath && (
            <Typography variant="caption" color="text.secondary" sx={{ wordBreak: "break-all" }}>
              Registry: {registryPath}
            </Typography>
          )}
          {diagnosticIssueCount > 0 && (
            <Alert severity="warning" sx={{ borderRadius: 2 }}>
              <Stack spacing={0.75}>
                <Typography variant="body2" fontWeight={800}>
                  Some operator manifests failed static validation.
                </Typography>
                {diagnostics.slice(0, 4).map((diagnostic) => (
                  <Box key={`${diagnostic.sourcePlugin}:${diagnostic.manifestPath}:${diagnostic.message}`}>
                    <Typography variant="caption" sx={{ display: "block", wordBreak: "break-all" }}>
                      {diagnostic.sourcePlugin} · {diagnostic.manifestPath}
                    </Typography>
                    <Typography variant="caption" color="text.secondary" sx={{ display: "block", wordBreak: "break-word" }}>
                      {diagnostic.message}
                    </Typography>
                  </Box>
                ))}
                {diagnostics.length > 4 && (
                  <Typography variant="caption" color="text.secondary">
                    Showing first 4 issues.
                  </Typography>
                )}
              </Stack>
            </Alert>
          )}
          <Paper
            variant="outlined"
            sx={{
              p: 1.25,
              borderRadius: 2.5,
              bgcolor: alpha(theme.palette.background.default, theme.palette.mode === "dark" ? 0.36 : 0.58),
            }}
          >
            <Stack spacing={1} useFlexGap>
              <Stack
                direction={{ xs: "column", sm: "row" }}
                spacing={1}
                alignItems={{ xs: "stretch", sm: "center" }}
                justifyContent="space-between"
              >
                <Stack direction="row" spacing={1} alignItems="center" flexWrap="wrap">
                  <Typography variant="subtitle2" fontWeight={850}>
                    Recent runs
                  </Typography>
                  <Chip size="small" variant="outlined" label={`${runs.length} recorded`} />
                </Stack>
                <Stack direction="row" gap={0.75} flexWrap="wrap">
                  <Button
                    size="small"
                    variant="outlined"
                    startIcon={<RefreshRounded />}
                    disabled={busy}
                    onClick={onRefreshRuns}
                    sx={{ textTransform: "none", borderRadius: 1.5, alignSelf: { xs: "flex-start", sm: "center" } }}
                  >
                    Refresh runs
                  </Button>
                  <Button
                    size="small"
                    variant="outlined"
                    color="warning"
                    startIcon={<DeleteOutlineRounded />}
                    disabled={busy || runs.length === 0}
                    onClick={() => onCleanupRuns()}
                    sx={{ textTransform: "none", borderRadius: 1.5, alignSelf: { xs: "flex-start", sm: "center" } }}
                  >
                    Clean old/cache runs
                  </Button>
                </Stack>
              </Stack>
              {runs.length === 0 ? (
                <Typography variant="body2" color="text.secondary">
                  No operator run records yet. SSH/sandbox operator artifacts stay on the selected remote execution environment and are referenced from the tool result.
                </Typography>
              ) : (
                <Stack spacing={0.75} useFlexGap>
                  {runs.slice(0, 5).map((run) => {
                    const timestamp = formatOperatorRunTimestamp(run.updatedAt);
                    return (
                      <Box
                        key={run.runId}
                        sx={{
                          p: 1,
                          borderRadius: 1.5,
                          bgcolor: "background.paper",
                          border: 1,
                          borderColor:
                            operatorRunStatusColor(run.status) === "error"
                              ? alpha(theme.palette.error.main, 0.32)
                              : "divider",
                        }}
                      >
                        <Stack spacing={0.65}>
                          <Stack direction="row" gap={0.75} alignItems="center" flexWrap="wrap">
                            <Typography variant="body2" fontWeight={850} sx={{ wordBreak: "break-all" }}>
                              {operatorRunTitle(run)}
                            </Typography>
                            <Chip
                              size="small"
                              color={operatorRunStatusColor(run.status)}
                              variant={operatorRunStatusColor(run.status) === "default" ? "outlined" : "filled"}
                              label={run.status}
                            />
                            <Chip size="small" variant="outlined" label={run.runId} />
                            <Chip size="small" variant="outlined" label={run.location} />
                            {operatorRunIsSmoke(run) && (
                              <Chip
                                size="small"
                                color="info"
                                variant="outlined"
                                label={run.smokeTestName || run.smokeTestId || "smoke"}
                              />
                            )}
                            {run.outputCount > 0 && (
                              <Chip size="small" color="success" variant="outlined" label={`${run.outputCount} output${run.outputCount === 1 ? "" : "s"}`} />
                            )}
                            {(run.structuredOutputCount ?? 0) > 0 && (
                              <Chip size="small" color="info" variant="outlined" label={`${run.structuredOutputCount} structured`} />
                            )}
                            {operatorRunIsCacheHit(run) && (
                              <Chip size="small" color="success" variant="outlined" label="cache hit" />
                            )}
                            {run.sourcePlugin && (
                              <Chip size="small" variant="outlined" label={run.sourcePlugin} />
                            )}
                          </Stack>
                          {run.errorMessage && (
                            <Typography variant="caption" color="error.main" sx={{ wordBreak: "break-word" }}>
                              {run.errorMessage}
                            </Typography>
                          )}
                          <Typography variant="caption" color="text.secondary" sx={{ wordBreak: "break-all" }}>
                            {timestamp ? `${timestamp} · ` : ""}{run.runDir}
                          </Typography>
                          {operatorRunIsCacheHit(run) && (
                            <Typography variant="caption" color="text.secondary" sx={{ wordBreak: "break-all" }}>
                              Reused source run {run.cacheSourceRunId || "unknown"}{run.cacheSourceRunDir ? ` · ${run.cacheSourceRunDir}` : ""}
                            </Typography>
                          )}
                          <Button
                            size="small"
                            variant="text"
                            startIcon={<TroubleshootRounded />}
                            disabled={busy}
                            onClick={() => onOpenRun(run)}
                            sx={{ alignSelf: "flex-start", textTransform: "none", borderRadius: 1.5 }}
                          >
                            View run detail
                          </Button>
                        </Stack>
                      </Box>
                    );
                  })}
                  {runs.length > 5 && (
                    <Typography variant="caption" color="text.secondary">
                      Showing latest 5 runs.
                    </Typography>
                  )}
                </Stack>
              )}
            </Stack>
          </Paper>
          {operators.length === 0 ? (
            <Paper variant="outlined" sx={{ p: 3, borderRadius: 2, textAlign: "center" }}>
              <ExtensionRounded sx={{ color: "text.secondary", mb: 1 }} />
              <Typography variant="body2" color="text.secondary">
                No operators discovered from enabled plugins yet.
              </Typography>
            </Paper>
          ) : (
            <Box sx={pluginCardGridSx} role="region" aria-label="Plugin operator list">
              {sortedOperators.map((operator) => {
                const alias = operatorPrimaryAlias(operator);
                const aliases = operator.enabledAliases.filter((value) => value.trim().length > 0);
                const title = operatorDisplayName(operator);
                const tone = operator.exposed ? theme.palette.success.main : theme.palette.text.secondary;
                const supportsSmokeRun = operatorSupportsSmokeRun(operator);
                const operatorKey = operatorCatalogKey(operator);
                const smokeCount = operator.smokeTests?.length ?? 0;
                const smokeTests = operator.smokeTests ?? [];
                const selectedSmokeTestId = selectedSmokeTests[operatorKey] ?? smokeTests[0]?.id ?? "";
                const smokeLabel = operatorSmokeRunLabel(operator, selectedSmokeTestId);
                const smokeSummary = operatorSmokeTestSummary(operator, selectedSmokeTestId);
                const latestFailedRun = operatorRunsForOperator(operator, runs)
                  .find((run) => operatorRunStatusColor(run.status) === "error") ?? null;
                const latestFailureSummary = latestFailedRun ? operatorRunDiagnosisSummary(latestFailedRun) : null;
                return (
                  <Paper
                    key={operatorKey}
                    variant="outlined"
                    role="button"
                    tabIndex={0}
                    onClick={() => setDetailOperator(operator)}
                    onKeyDown={(event) => {
                      if (event.key === "Enter" || event.key === " ") {
                        event.preventDefault();
                        setDetailOperator(operator);
                      }
                    }}
                    sx={{
                      p: 1.25,
                      borderRadius: 2.5,
                      bgcolor: "background.paper",
                      borderColor: operator.exposed
                        ? alpha(theme.palette.success.main, 0.28)
                        : "divider",
                      cursor: "pointer",
                      transition: "border-color 120ms ease, box-shadow 120ms ease",
                      "&:hover": {
                        borderColor: alpha(theme.palette.primary.main, 0.5),
                        boxShadow: `0 0 0 1px ${alpha(theme.palette.primary.main, 0.12)}`,
                      },
                    }}
                  >
                    <Stack spacing={1.1}>
                      <Stack direction="row" spacing={1.25} alignItems="center">
                        <Box
                          sx={{
                            width: 36,
                            height: 36,
                            borderRadius: 2,
                            display: "inline-flex",
                            alignItems: "center",
                            justifyContent: "center",
                            color: tone,
                            bgcolor: alpha(tone, theme.palette.mode === "dark" ? 0.18 : 0.08),
                            border: `1px solid ${alpha(tone, theme.palette.mode === "dark" ? 0.22 : 0.12)}`,
                            flexShrink: 0,
                          }}
                        >
                          <ExtensionRounded fontSize="small" />
                        </Box>
                        <Box sx={{ minWidth: 0, flex: 1 }}>
                          <Typography variant="subtitle2" fontWeight={850} noWrap title={title}>
                            {title}
                          </Typography>
                          <Typography variant="caption" color="text.secondary" noWrap title={operator.id}>
                            {operator.id} · v{operator.version}
                          </Typography>
                        </Box>
                        <Switch
                          size="small"
                          checked={operator.exposed}
                          disabled={busy}
                          onClick={(event) => event.stopPropagation()}
                          onChange={(event) => onToggle(operator, event.target.checked)}
                          inputProps={{ "aria-label": `${operator.exposed ? "Disable" : "Expose"} operator ${operator.id}` }}
                        />
                      </Stack>

                      <Typography
                        variant="body2"
                        color="text.secondary"
                        sx={{ minHeight: 20 }}
                      >
                        {operator.description?.trim() || "Plugin-defined operator callable by agents as a tool."}
                      </Typography>

                      <Stack direction="row" gap={0.75} flexWrap="wrap">
                        <Chip
                          size="small"
                          color={operator.exposed ? "success" : "default"}
                          variant={operator.exposed ? "filled" : "outlined"}
                          label={operator.exposed ? "Exposed" : "Not exposed"}
                        />
                        {operator.exposed ? (
                          aliases.map((enabledAlias) => (
                            <Chip
                              key={enabledAlias}
                              size="small"
                              variant="outlined"
                              label={operatorToolName(enabledAlias)}
                            />
                          ))
                        ) : (
                          <Chip size="small" variant="outlined" label={`alias ${alias}`} />
                        )}
                        <Chip size="small" variant="outlined" label={operator.sourcePlugin} />
                        {smokeCount > 0 && (
                          <Chip
                            size="small"
                            variant="outlined"
                            label={`${smokeCount} smoke ${smokeCount === 1 ? "test" : "tests"}`}
                          />
                        )}
                      </Stack>

                      {(() => {
                        const stats = operatorRunStats(operator, runs);
                        const latestRun = stats.latestRun;
                        return (
                          <Stack direction="row" gap={0.75} flexWrap="wrap">
                            <Chip size="small" variant="outlined" label={`${stats.total} calls`} />
                            <Chip size="small" color="success" variant="outlined" label={`${stats.succeeded} succeeded`} />
                            <Chip size="small" color={stats.failed > 0 ? "error" : "default"} variant="outlined" label={`${stats.failed} failed`} />
                            <Chip
                              size="small"
                              color={stats.smokeFailed > 0 ? "error" : stats.smokeSucceeded > 0 ? "success" : "default"}
                              variant="outlined"
                              label={`${stats.smokeTotal} smoke`}
                            />
                            {stats.cacheHits > 0 && (
                              <Chip size="small" color="success" variant="outlined" label={`${stats.cacheHits} cache hits`} />
                            )}
                            {latestRun && (
                              <Chip
                                size="small"
                                color={operatorRunStatusColor(latestRun.status)}
                                variant={operatorRunStatusColor(latestRun.status) === "default" ? "outlined" : "filled"}
                                label={`latest ${latestRun.status}`}
                              />
                            )}
                            {stats.latestSmokeRun && (
                              <Chip
                                size="small"
                                color={operatorRunStatusColor(stats.latestSmokeRun.status)}
                                variant="outlined"
                                label={`latest smoke ${stats.latestSmokeRun.status}`}
                              />
                            )}
                          </Stack>
                        );
                      })()}

                      {latestFailedRun && latestFailureSummary && (
                        <Alert severity="error" sx={{ py: 0.5, borderRadius: 1.5 }}>
                          <Typography variant="caption" sx={{ wordBreak: "break-word" }}>
                            Latest failure: {latestFailureSummary}
                          </Typography>
                        </Alert>
                      )}

                      {smokeSummary && (
                        <Typography variant="caption" color="text.secondary" sx={{ wordBreak: "break-word" }}>
                          {smokeSummary}
                        </Typography>
                      )}

                      {supportsSmokeRun && (
                        <Stack direction={{ xs: "column", sm: "row" }} gap={0.75} alignItems={{ xs: "stretch", sm: "center" }}>
                          {smokeTests.length > 1 && (
                            <TextField
                              select
                              size="small"
                              label="Smoke test"
                              value={selectedSmokeTestId}
                              onClick={(event) => event.stopPropagation()}
                              onKeyDown={(event) => event.stopPropagation()}
                              onChange={(event) =>
                                setSelectedSmokeTests((current) => ({
                                  ...current,
                                  [operatorKey]: event.target.value,
                                }))
                              }
                              disabled={busy}
                              sx={{ minWidth: 220 }}
                            >
                              {smokeTests.map((smokeTest) => (
                                <MenuItem key={smokeTest.id} value={smokeTest.id}>
                                  {smokeTest.name?.trim() || smokeTest.id}
                                </MenuItem>
                              ))}
                            </TextField>
                          )}
                          <Button
                            size="small"
                            variant="outlined"
                            startIcon={<PlayArrowRounded />}
                            disabled={busy || !operator.exposed}
                            onClick={(event) => {
                              event.stopPropagation();
                              onSmokeRun(operator, selectedSmokeTestId);
                            }}
                            sx={{ alignSelf: { xs: "flex-start", sm: "center" }, textTransform: "none", borderRadius: 1.5 }}
                          >
                            {operator.exposed ? `Run ${smokeLabel}` : "Expose to run smoke test"}
                          </Button>
                        </Stack>
                      )}

                      {operator.unavailableReason && (
                        <Typography variant="caption" color="warning.main" sx={{ wordBreak: "break-word" }}>
                          {operator.unavailableReason}
                        </Typography>
                      )}
                    </Stack>
                  </Paper>
                );
              })}
            </Box>
          )}
        </Stack>
      </AccordionDetails>
      </Accordion>
    </>
  );
}

export function PluginsPanel({ projectPath }: { projectPath: string }) {
  const {
    marketplaces,
    operators,
    operatorDiagnostics,
    operatorRegistryPath,
    operatorRuns,
    retrievalStatuses,
    processPoolStatuses,
    isLoading,
    isMutating,
    error,
    loadPlugins,
    loadOperatorRuns,
    readOperatorRun,
    readOperatorRunLog,
    verifyOperatorRun,
    cleanupOperatorRuns,
    clearProcessPool,
    installPlugin,
    uninstallPlugin,
    setPluginEnabled,
    setOperatorEnabled,
    runOperator,
  } = usePluginStore();
  const [message, setMessage] = useState<string | null>(null);
  const [detailPluginId, setDetailPluginId] = useState<string | null>(null);
  const [detailRun, setDetailRun] = useState<OperatorRunSummary | null>(null);
  const [operatorRunDetail, setOperatorRunDetail] = useState<OperatorRunDetail | null>(null);
  const [operatorRunLog, setOperatorRunLog] = useState<OperatorRunLog | null>(null);
  const [operatorRunVerification, setOperatorRunVerification] = useState<OperatorRunVerification | null>(null);
  const [operatorRunDetailLoading, setOperatorRunDetailLoading] = useState(false);
  const [operatorRunLogLoading, setOperatorRunLogLoading] = useState<"stdout" | "stderr" | null>(null);
  const [operatorRunVerifying, setOperatorRunVerifying] = useState(false);
  const [operatorRunDetailError, setOperatorRunDetailError] = useState<string | null>(null);
  const [pluginSearch, setPluginSearch] = useState("");
  const [pluginFilter, setPluginFilter] = useState<PluginCatalogFilter>("all");
  const [dismissedFeedbackKey, setDismissedFeedbackKey] = useState<string | null>(null);
  const [installingPluginId, setInstallingPluginId] = useState<string | null>(null);
  const projectRoot = projectPath.trim() || undefined;
  const theme = useTheme();
  const sessionId = useSessionStore((state) => state.currentSession?.id ?? null);
  const executionEnvironment = useChatComposerStore((state) => state.environment);
  const sshServer = useChatComposerStore((state) => state.sshServer);
  const sandboxBackend = useChatComposerStore((state) => state.sandboxBackend);
  const operatorSurface = useMemo(
    () => ({
      sessionId,
      executionEnvironment,
      sshServer,
      sandboxBackend,
    }),
    [executionEnvironment, sandboxBackend, sessionId, sshServer],
  );

  useEffect(() => {
    void loadPlugins(projectRoot, operatorSurface);
  }, [loadPlugins, operatorSurface, projectRoot]);

  const allPlugins = useMemo(() => flattenMarketplacePlugins(marketplaces), [marketplaces]);
  const exposedOperators = useMemo(
    () => operators.filter((operator) => operator.exposed),
    [operators],
  );
  const operatorsByPlugin = useMemo(() => {
    const grouped = new Map<string, OperatorSummary[]>();
    for (const operator of operators) {
      const current = grouped.get(operator.sourcePlugin) ?? [];
      current.push(operator);
      grouped.set(operator.sourcePlugin, current);
    }
    for (const [pluginId, pluginOperators] of grouped) {
      grouped.set(
        pluginId,
        [...pluginOperators].sort((left, right) =>
          operatorDisplayName(left).localeCompare(operatorDisplayName(right)),
        ),
      );
    }
    return grouped;
  }, [operators]);
  const pluginsById = useMemo(() => {
    const byId = new Map<string, PluginSummary>();
    for (const plugin of allPlugins) {
      byId.set(plugin.id, plugin);
    }
    return byId;
  }, [allPlugins]);
  const detailPlugin = detailPluginId ? pluginsById.get(detailPluginId) ?? null : null;
  const detailPluginOperators = detailPlugin
    ? operatorsByPlugin.get(detailPlugin.id) ?? operatorsByPlugin.get(detailPlugin.name) ?? []
    : [];
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
  const hasRuntimeDiagnosticsIssue =
    runtimeAttentionStatuses.length > 0 || unknownRuntimePluginIds.length > 0;
  const processPoolStatusesByPlugin = useMemo(() => {
    const grouped = new Map<string, PluginProcessPoolRouteStatus[]>();
    for (const status of processPoolStatuses) {
      const current = grouped.get(status.pluginId) ?? [];
      current.push(status);
      grouped.set(status.pluginId, current);
    }
    return grouped;
  }, [processPoolStatuses]);
  const hasPluginCatalogFilters = pluginSearch.trim().length > 0 || pluginFilter !== "all";
  const busyPluginIds = useMemo(
    () => new Set(installingPluginId ? [installingPluginId] : []),
    [installingPluginId],
  );
  const feedbackText = error || message;
  const feedbackSeverity = error ? "error" : "success";
  const feedbackKey = feedbackText ? `${feedbackSeverity}:${feedbackText}` : null;
  const feedbackOpen = Boolean(feedbackText && feedbackKey !== dismissedFeedbackKey);

  useEffect(() => {
    setDismissedFeedbackKey(null);
  }, [feedbackKey]);

  const handleFeedbackClose = (_event?: unknown, reason?: string) => {
    if (reason === "clickaway") return;
    if (feedbackKey) setDismissedFeedbackKey(feedbackKey);
    if (!error) setMessage(null);
  };

  const handleInstall = async (plugin: PluginSummary) => {
    setMessage(null);
    setInstallingPluginId(plugin.id);
    try {
      await installPlugin(plugin, projectRoot);
      setMessage(`Installed ${displayName(plugin)}`);
    } catch {
      // Store exposes the error banner.
    } finally {
      setInstallingPluginId((current) => (current === plugin.id ? null : current));
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

  const handleOperatorToggle = async (operator: OperatorSummary, enabled: boolean) => {
    const alias = operatorPrimaryAlias(operator);
    setMessage(null);
    try {
      await setOperatorEnabled(
        {
          alias,
          operatorId: operator.id,
          sourcePlugin: operator.sourcePlugin,
          version: operator.version,
          enabled,
        },
        projectRoot,
        operatorSurface,
      );
      const toolName = operatorToolName(alias);
      setMessage(
        enabled
          ? `Exposed ${operatorDisplayName(operator)} as ${toolName}`
          : `Disabled ${toolName}`,
      );
    } catch {
      // Store exposes the error banner.
    }
  };

  const handleOperatorRegistrationChange = async (
    targetOperators: OperatorSummary[],
    enabled: boolean,
  ) => {
    if (targetOperators.length === 0) return;
    setMessage(null);
    try {
      for (const operator of targetOperators) {
        const alias = operatorPrimaryAlias(operator);
        await setOperatorEnabled(
          {
            alias,
            operatorId: operator.id,
            sourcePlugin: operator.sourcePlugin,
            version: operator.version,
            enabled,
          },
          projectRoot,
          operatorSurface,
        );
      }
      setMessage(
        `${enabled ? "Registered" : "Unregistered"} ${targetOperators.length} operator${targetOperators.length === 1 ? "" : "s"}`,
      );
    } catch {
      // Store exposes the error banner.
    }
  };

  const openOperatorRunDetail = async (
    run: OperatorRunSummary,
    options: { autoVerify?: boolean } = {},
  ): Promise<OperatorRunVerification | null> => {
    setMessage(null);
    setDetailRun(run);
    setOperatorRunDetail(null);
    setOperatorRunLog(null);
    setOperatorRunVerification(null);
    setOperatorRunDetailError(null);
    setOperatorRunDetailLoading(true);
    let loaded = false;
    try {
      const detail = await readOperatorRun(run.runId, projectRoot, operatorSurface);
      setOperatorRunDetail(detail);
      loaded = true;
    } catch (err) {
      setOperatorRunDetailError(err instanceof Error ? err.message : String(err));
    } finally {
      setOperatorRunDetailLoading(false);
    }
    if (options.autoVerify && loaded) {
      setOperatorRunVerifying(true);
      try {
        const verification = await verifyOperatorRun(run.runId, projectRoot, operatorSurface);
        setOperatorRunVerification(verification);
        return verification;
      } catch (err) {
        setOperatorRunDetailError(err instanceof Error ? err.message : String(err));
      } finally {
        setOperatorRunVerifying(false);
      }
    }
    return null;
  };

  const handleOperatorSmokeRun = async (
    operator: OperatorSummary,
    smokeTestId?: string | null,
  ) => {
    const alias = operatorPrimaryAlias(operator);
    const smokeTest = operatorSmokeTestForRun(operator, smokeTestId);
    setMessage(null);
    try {
      const response = await runOperator(
        alias,
        operatorSmokeRunArguments(operator, smokeTestId),
        projectRoot,
        operatorSurface,
        {
          kind: "smoke",
          smokeTestId: smokeTest?.id ?? smokeTestId ?? null,
          smokeTestName: smokeTest?.name ?? null,
        },
      );
      const runDir = response.result.runDir;
      const smokeLabel = operatorSmokeRunLabel(operator, smokeTestId);
      setMessage(
        `${smokeLabel} succeeded for ${operatorToolName(alias)}${typeof runDir === "string" ? ` · ${runDir}` : ""}`,
      );
      const summary = summarizeOperatorRunResult(response.result);
      if (summary) {
        const verification = await openOperatorRunDetail(summary, { autoVerify: true });
        const verifyStatus = verification
          ? verification.ok ? "verified" : "verification reported issues"
          : "opened run detail";
        setMessage(
          `${smokeLabel} succeeded and ${verifyStatus} for ${operatorToolName(alias)}${typeof runDir === "string" ? ` · ${runDir}` : ""}`,
        );
      }
    } catch {
      // Store exposes the error banner.
    }
  };

  const handleRefreshOperatorRuns = async () => {
    setMessage(null);
    try {
      await loadOperatorRuns(projectRoot, operatorSurface);
      setMessage("Refreshed operator runs");
    } catch {
      // Store exposes the error banner.
    }
  };

  const operatorCleanupRequest = (operator?: OperatorSummary): OperatorRunCleanupRequest => ({
    dryRun: true,
    keepLatest: 25,
    maxAgeDays: 30,
    includeCacheHits: true,
    includeFailed: true,
    includeSucceeded: true,
    limit: 500,
    operatorAlias: operator ? operatorPrimaryAlias(operator) : null,
    operatorId: operator?.id ?? null,
    operatorVersion: operator?.version ?? null,
    sourcePlugin: operator?.sourcePlugin ?? null,
  });

  const handleCleanupOperatorRuns = async (operator?: OperatorSummary) => {
    setMessage(null);
    const request = operatorCleanupRequest(operator);
    const scopeLabel = operator
      ? ` for ${operatorDisplayName(operator)}`
      : "";
    try {
      const preview = await cleanupOperatorRuns(request, projectRoot, operatorSurface);
      if (preview.matchedCount === 0) {
        setMessage(
          `No cleanup candidates${scopeLabel} in ${preview.runsRoot}; latest 25 matching runs are preserved.`,
        );
        return;
      }
      const candidateLines = preview.candidates
        .slice(0, 8)
        .map((candidate) => `• ${candidate.runId} (${candidate.status}, ${candidate.reason})`)
        .join("\n");
      const remaining = preview.candidates.length > 8
        ? `\n… and ${preview.candidates.length - 8} more`
        : "";
      const confirmed = window.confirm(
        `Delete ${preview.matchedCount} operator run director${preview.matchedCount === 1 ? "y" : "ies"}${scopeLabel} from the current ${preview.location} workspace?\n\n` +
        `Runs root: ${preview.runsRoot}\n` +
        `Estimated space: ${formatBytes(preview.estimatedBytes)}\n\n` +
        `${candidateLines}${remaining}\n\n` +
        "This only affects the active session workspace and cannot be undone.",
      );
      if (!confirmed) {
        setMessage("Operator run cleanup cancelled");
        return;
      }
      const result = await cleanupOperatorRuns(
        { ...request, dryRun: false },
        projectRoot,
        operatorSurface,
      );
      setMessage(
        `Deleted ${result.deletedCount} operator run director${result.deletedCount === 1 ? "y" : "ies"}${scopeLabel}${result.skippedCount > 0 ? ` · ${result.skippedCount} skipped` : ""} · ${formatBytes(result.estimatedBytes)} estimated`,
      );
    } catch {
      // Store exposes the error banner.
    }
  };

  const handleOpenOperatorRun = async (run: OperatorRunSummary) => {
    await openOperatorRunDetail(run);
  };

  const handleLoadOperatorRunLog = async (logName: "stdout" | "stderr") => {
    if (!detailRun) return;
    setOperatorRunLogLoading(logName);
    setOperatorRunDetailError(null);
    try {
      const log = await readOperatorRunLog(detailRun.runId, logName, projectRoot, operatorSurface);
      setOperatorRunLog(log);
    } catch (err) {
      setOperatorRunDetailError(err instanceof Error ? err.message : String(err));
    } finally {
      setOperatorRunLogLoading(null);
    }
  };

  const handleVerifyOperatorRun = async () => {
    if (!detailRun) return;
    setOperatorRunVerifying(true);
    setOperatorRunDetailError(null);
    try {
      const verification = await verifyOperatorRun(detailRun.runId, projectRoot, operatorSurface);
      setOperatorRunVerification(verification);
    } catch (err) {
      setOperatorRunDetailError(err instanceof Error ? err.message : String(err));
    } finally {
      setOperatorRunVerifying(false);
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
    <>
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
              onClick={() => void loadPlugins(projectRoot, operatorSurface)}
              sx={{ textTransform: "none", borderRadius: 2, minHeight: 40, alignSelf: { xs: "flex-start", md: "center" } }}
            >
              Refresh
            </Button>
          </Stack>
          <Stack direction="row" spacing={1.5} flexWrap="wrap" useFlexGap alignItems="center">
            {[
              ["Enabled", enabledPlugins.length],
              ["Installable", availablePlugins.length],
              ["Registered", exposedOperators.length],
              ["Runs", operatorRuns.length],
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

      <PluginDetailsDialog
        plugin={detailPlugin}
        open={Boolean(detailPlugin)}
        retrievalStatuses={detailPlugin ? retrievalStatusesByPlugin.get(detailPlugin.id) : undefined}
        processPoolStatuses={detailPlugin ? processPoolStatusesByPlugin.get(detailPlugin.id) : undefined}
        operators={detailPluginOperators}
        busy={isMutating || (detailPlugin ? busyPluginIds.has(detailPlugin.id) : false)}
        onClose={() => setDetailPluginId(null)}
        onInstall={(plugin) => void handleInstall(plugin)}
        onUninstall={(plugin) => void handleUninstall(plugin)}
        onToggle={(plugin, enabled) => void handleToggle(plugin, enabled)}
        onCopyDiagnostics={handleCopyDiagnostics}
      />

      <OperatorRunDetailsDialog
        run={detailRun}
        detail={operatorRunDetail}
        log={operatorRunLog}
        verification={operatorRunVerification}
        loading={operatorRunDetailLoading}
        logLoading={operatorRunLogLoading}
        verifying={operatorRunVerifying}
        error={operatorRunDetailError}
        onClose={() => {
          setDetailRun(null);
          setOperatorRunDetail(null);
          setOperatorRunLog(null);
          setOperatorRunVerification(null);
          setOperatorRunDetailError(null);
        }}
        onLoadLog={(logName) => void handleLoadOperatorRunLog(logName)}
        onVerify={() => void handleVerifyOperatorRun()}
        onCopy={(text, successMessage) => void copyToClipboard(text, successMessage)}
      />

      {hasRuntimeDiagnosticsIssue && (
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
            {runtimeAttentionStatuses.length === 0 &&
            processPoolStatuses.length === 0 &&
            unknownRuntimePluginIds.length === 0 ? (
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
      )}

      <OperatorCatalogSection
        operators={operators}
        diagnostics={operatorDiagnostics}
        runs={operatorRuns}
        registryPath={operatorRegistryPath}
        busy={isMutating}
        onToggle={(operator, enabled) => void handleOperatorToggle(operator, enabled)}
        onSmokeRun={(operator, smokeTestId) => void handleOperatorSmokeRun(operator, smokeTestId)}
        onRefreshRuns={() => void handleRefreshOperatorRuns()}
        onCleanupRuns={(operator) => void handleCleanupOperatorRuns(operator)}
        onOpenRun={(run) => void handleOpenOperatorRun(run)}
        onCopy={(text, successMessage) => void copyToClipboard(text, successMessage)}
      />

      {marketplaces.length === 0 || allPlugins.length === 0 ? (
        <Paper variant="outlined" sx={{ p: 3, borderRadius: 2, textAlign: "center" }}>
          <ExtensionRounded sx={{ color: "text.secondary", mb: 1 }} />
          <Typography variant="body2" color="text.secondary">
            No plugin marketplace found yet. Add one at ~/.omiga/plugins/marketplace.json or project .omiga/plugins/marketplace.json.
          </Typography>
        </Paper>
      ) : filteredCatalogPlugins.length === 0 ? (
        <Paper variant="outlined" sx={{ p: 3, borderRadius: 2, textAlign: "center" }}>
          <SearchRounded sx={{ color: "text.secondary", mb: 1 }} />
          <Typography variant="body2" color="text.secondary">
            No plugins match the current search or filter.
          </Typography>
        </Paper>
      ) : (
        <PluginCatalogGroupList
          plugins={filteredCatalogPlugins}
          retrievalStatusesByPlugin={retrievalStatusesByPlugin}
          processPoolStatusesByPlugin={processPoolStatusesByPlugin}
          operatorsByPlugin={operatorsByPlugin}
          busy={isMutating}
          busyPluginIds={busyPluginIds}
          onInstall={(plugin) => void handleInstall(plugin)}
          onToggle={(plugin, enabled) => void handleToggle(plugin, enabled)}
          onOperatorRegistrationChange={(targetOperators, enabled) =>
            void handleOperatorRegistrationChange(targetOperators, enabled)
          }
          onOpenDetails={(selectedPlugin) => setDetailPluginId(selectedPlugin.id)}
        />
      )}
    </Stack>
    <Snackbar
      key={feedbackKey ?? "plugin-feedback"}
      open={feedbackOpen}
      autoHideDuration={error ? null : 4200}
      onClose={handleFeedbackClose}
      anchorOrigin={{ vertical: "bottom", horizontal: "center" }}
    >
      <Alert
        severity={feedbackSeverity}
        variant="filled"
        onClose={() => handleFeedbackClose()}
        sx={{ borderRadius: 2, boxShadow: 4 }}
      >
        {feedbackText}
      </Alert>
    </Snackbar>
    </>
  );
}
