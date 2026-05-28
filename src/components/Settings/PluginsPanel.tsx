import { useEffect, useMemo, useState, type KeyboardEvent } from "react";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
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
  Portal,
  Snackbar,
  Stack,
  Switch,
  TextField,
  Tooltip,
  Typography,
  type ChipProps,
} from "@mui/material";
import { alpha, useTheme, type Theme } from "@mui/material/styles";
import {
  AddRounded,
  ClearRounded,
  CloseRounded,
  ContentCopyRounded,
  DeleteOutlineRounded,
  DescriptionOutlined,
  ExtensionRounded,
  ExpandMoreRounded,
  PublishedWithChangesRounded,
  RefreshRounded,
  SearchRounded,
  SettingsRounded,
  SyncRounded,
  TroubleshootRounded,
} from "@mui/icons-material";
import {
  buildPluginDiagnostics,
  buildRetrievalRuntimeDiagnostics,
  flattenMarketplacePlugins,
  type EnvironmentCheckResult,
  type OperatorRuntimeResourceProfile,
  type OperatorRunDetail,
  type OperatorRunVerification,
  type OperatorRunLog,
  type OperatorRunSummary,
  type OperatorSummary,
  type MarketplaceRemoteCheckResult,
  type PluginMigrationResult,
  type MarketplaceSourceKind,
  type MarketplaceSourceView,
  type PluginEnvironmentSummary,
  type PluginProcessPoolRouteStatus,
  type PluginRetrievalLifecycleState,
  type PluginRetrievalRouteStatus,
  type PluginRetrievalResourceSummary,
  type PluginSummary,
  type PluginTemplateSummary,
  type RefreshResult,
  usePluginStore,
} from "../../state/pluginStore";
import { useChatComposerStore } from "../../state/chatComposerStore";
import { useSessionStore } from "../../state/sessionStore";
import { ComputerUseSettingsPanel } from "./ComputerUseSettingsTab";
import { NotebookViewerSettingsPanel } from "./NotebookSettingsTab";
import { OperatorRunsTimeline } from "./OperatorRunsTimeline";
import { extractErrorMessage } from "../../utils/errorMessage";

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

export const pluginDetailsDialogSx = {
  "& .MuiDialog-container": {
    alignItems: "flex-start",
  },
  "& .MuiDialog-paper": {
    mt: { xs: 2, sm: 6 },
    mb: { xs: 2, sm: 6 },
    maxHeight: { xs: "calc(100% - 32px)", sm: "calc(100% - 96px)" },
  },
};

export const pluginDetailsTechnicalSectionSx = {
  display: "flex",
  flexDirection: "column",
  gap: 1.25,
  pt: 1.25,
};

export const visualizationRExecuteSkeletonSx = {
  m: 0,
  p: 0.85,
  borderRadius: 1.5,
  color: "text.secondary",
  fontFamily: "ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace",
  fontSize: 11,
  lineHeight: 1.45,
  overflow: "auto",
  overflowWrap: "anywhere",
  wordBreak: "break-word",
  maxHeight: 180,
  whiteSpace: "pre-wrap",
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
    .replace(/\s+Retrieval\s+Resource$/i, "")
    .replace(/\s+Source$/i, "")
    .replace(/\s+Operator$/i, "")
    .replace(/\s+Templates?$/i, "")
    .replace(/\s+Skills?$/i, "")
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
  return Array.from(new Set([category, ...caps].filter(Boolean) as string[]))
    .filter((value) => !isInternalContributionLabel(value))
    .slice(0, 6);
}

function isInternalContributionLabel(value: string): boolean {
  const normalized = value.trim().toLowerCase().replace(/[-_]+/g, " ");
  return [
    "operator",
    "operators",
    "template",
    "templates",
    "skill",
    "skills",
  ].includes(normalized);
}

function pluginClassificationTerms(plugin: PluginSummary): string[] {
  return [
    plugin.interface?.category,
    ...(plugin.interface?.capabilities ?? []),
    plugin.interface?.shortDescription,
    plugin.interface?.longDescription,
    plugin.name,
    plugin.id,
    ...((plugin.operators ?? []).flatMap((operator) => [
      operator.name,
      operator.description,
      ...(operator.tags ?? []),
    ])),
  ]
    .filter((value): value is string => Boolean(value?.trim()))
    .map((value) => value.trim().toLowerCase().replace(/[-_]+/g, " "));
}

function pluginHasTerm(plugin: PluginSummary, terms: string[]): boolean {
  const haystack = pluginClassificationTerms(plugin);
  return haystack.some((value) => terms.some((term) => value === term || value.includes(term)));
}

function normalizedPluginCategory(plugin: PluginSummary): string {
  return plugin.interface?.category?.trim().toLowerCase().replace(/[-_]+/g, " ") ?? "";
}

function pluginNameIdText(plugin: PluginSummary): string {
  return [plugin.name, plugin.id]
    .filter((value): value is string => Boolean(value?.trim()))
    .join(" ")
    .toLowerCase()
    .replace(/[-_]+/g, " ");
}

function pluginMarketplaceTaxonomySegments(plugin: PluginSummary): string[] {
  const rawPath = plugin.sourcePath?.trim() || plugin.installedPath?.trim() || "";
  if (!rawPath) return [];
  const pathSegments = rawPath
    .replace(/\\/g, "/")
    .split("/")
    .map((segment) => segment.trim())
    .filter(Boolean);
  const pluginsIndex = pathSegments.lastIndexOf("plugins");
  if (pluginsIndex < 0 || pluginsIndex >= pathSegments.length - 1) return [];
  return pathSegments.slice(pluginsIndex + 1).map((segment) => segment.toLowerCase().replace(/[_\s]+/g, "-"));
}

function humanizeTaxonomySegment(segment: string): string {
  const normalized = segment.trim().toLowerCase().replace(/[_\s]+/g, "-");
  const acronyms: Record<string, string> = {
    ngs: "NGS",
    qc: "QC",
    hpc: "HPC",
    r: "R",
    rna: "RNA",
    rnaseq: "RNA-seq",
    "rna-seq": "RNA-seq",
    dna: "DNA",
    sv: "SV",
    cnv: "CNV",
  };
  if (acronyms[normalized]) return acronyms[normalized];
  return normalized
    .split("-")
    .filter(Boolean)
    .map((part) => acronyms[part] ?? part.charAt(0).toUpperCase() + part.slice(1))
    .join(" ");
}

function pluginCatalogGroupIdFromTaxonomyRoot(root: string): PluginCatalogGroupId | null {
  switch (root) {
    case "analysis":
    case "analyses":
      return "analysis";
    case "bioinformatics":
    case "omics":
      return "bioinformatics";
    case "visualization":
    case "visualizations":
    case "visualisation":
    case "visualisations":
      return "visualization";
    case "automation":
    case "operator":
    case "operators":
      return "operator";
    case "tool":
    case "tools":
      return "tools";
    case "resource":
    case "resources":
    case "retrieval":
    case "retrievals":
      return "resource";
    default:
      return null;
  }
}

function pluginCatalogGroupIdFromPath(plugin: PluginSummary): PluginCatalogGroupId | null {
  const [root] = pluginMarketplaceTaxonomySegments(plugin);
  return root ? pluginCatalogGroupIdFromTaxonomyRoot(root) : null;
}

function pluginCatalogSectionFromPath(
  groupId: PluginCatalogGroupId,
  plugin: PluginSummary,
): string | null {
  const segments = pluginMarketplaceTaxonomySegments(plugin);
  if (segments.length < 3) return null;
  const rootGroup = pluginCatalogGroupIdFromTaxonomyRoot(segments[0]);
  if (rootGroup !== groupId) return null;
  return segments[1];
}

function pluginHasAnyCategory(plugin: PluginSummary, categories: string[]): boolean {
  const category = normalizedPluginCategory(plugin);
  return categories.some((value) => category === value);
}

function pluginLooksLikeResourceBundle(plugin: PluginSummary): boolean {
  return /(^|\s)(resource|retrieval)(\s|$)/.test(pluginNameIdText(plugin));
}

function isOperatorPlugin(plugin: PluginSummary): boolean {
  return pluginHasTerm(plugin, ["operator", "operators"]);
}

function isTemplatePlugin(plugin: PluginSummary): boolean {
  return pluginHasTerm(plugin, ["template", "templates"]);
}

function isAnalysisPlugin(plugin: PluginSummary): boolean {
  if (isResourcePlugin(plugin) || isVisualizationPlugin(plugin)) return false;
  if (pluginHasAnyCategory(plugin, ["analysis"])) return true;
  return pluginHasTerm(plugin, ["analysis", "statistics", "statistical"]);
}

function isAutomationPlugin(plugin: PluginSummary): boolean {
  return pluginHasTerm(plugin, [
    "automation",
    "computer use",
    "computer observe",
    "computer input",
    "computer accessibility",
    "agent callable",
  ]);
}

function isVisualizationPlugin(plugin: PluginSummary): boolean {
  if (pluginHasAnyCategory(plugin, ["visualization", "visualisation"])) return true;
  if (isResourcePlugin(plugin)) return false;
  return pluginHasTerm(plugin, ["visualization", "visualisation", "figure", "plot"]);
}

function isRVisualizationPlugin(plugin: PluginSummary): boolean {
  const taxonomy = pluginMarketplaceTaxonomySegments(plugin);
  if (taxonomy[0] === "visualization" && taxonomy.includes("r")) return true;
  return pluginHasTerm(plugin, ["r visualization", "rscript", "ggplot", "ggplot2", "base r"]);
}

function resourceCategoryFromTerms(plugin: PluginSummary): string | null {
  if (pluginHasTerm(plugin, ["literature", "pubmed", "semantic scholar", "paper", "papers"])) {
    return "literature";
  }
  if (pluginHasTerm(plugin, ["knowledge", "ensembl", "uniprot", "gene"])) {
    return "knowledge";
  }
  if (pluginHasTerm(plugin, [
    "dataset",
    "datasets",
    "geo",
    "gtex",
    "ena",
    "arrayexpress",
    "biosample",
    "cbioportal",
    "ncbi datasets",
  ])) {
    return "dataset";
  }
  if (pluginHasTerm(plugin, ["resource", "resources", "source", "retrieval", "search", "query", "fetch"])) {
    return "other";
  }
  return null;
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
  if (/\bc\b/.test(haystack)) return "c";
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

function isComputerUsePlugin(plugin: PluginSummary): boolean {
  return pluginHasTerm(plugin, [
    "computer use",
    "computer observe",
    "computer input",
    "computer accessibility",
  ]);
}

function isBioinformaticsPlugin(plugin: PluginSummary): boolean {
  if (isResourcePlugin(plugin) || isVisualizationPlugin(plugin)) return false;
  if (pluginHasAnyCategory(plugin, ["bioinformatics"])) return true;
  return pluginHasTerm(plugin, ["bioinformatics", "omics", "ngs"]);
}

export type PluginCatalogGroupId =
  | "analysis"
  | "bioinformatics"
  | "visualization"
  | "operator"
  | "tools"
  | "resource"
  | "other";

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
  const pathGroup = pluginCatalogGroupIdFromPath(plugin);
  if (pathGroup) return pathGroup;
  // Preserve explicit plugin surface boundaries first. Retrieval bundles can mention
  // genes/variants/assemblies, and visualization templates can mention gene plots;
  // those domain words must not override the plugin's declared contribution type.
  if (isVisualizationPlugin(plugin)) return "visualization";
  if (isResourcePlugin(plugin)) return "resource";
  if (isBioinformaticsPlugin(plugin)) return "bioinformatics";
  if (isAnalysisPlugin(plugin)) return "analysis";
  if (isAutomationPlugin(plugin)) return "operator";
  if (isOperatorPlugin(plugin)) return "operator";
  if (isFunctionPlugin(plugin)) return "tools";
  return "other";
}

function pluginCatalogGroupLabel(group: PluginCatalogGroupId): string {
  switch (group) {
    case "analysis":
      return "Analysis";
    case "bioinformatics":
      return "Bioinformatics";
    case "visualization":
      return "Visualization";
    case "operator":
      return "Automation";
    case "tools":
      return "Tools";
    case "resource":
      return "Resources";
    default:
      return "Others";
  }
}

function pluginCatalogGroupDescription(group: PluginCatalogGroupId): string {
  switch (group) {
    case "analysis":
      return "Analysis plugins that bundle atomic Operator/Template units by domain.";
    case "bioinformatics":
      return "Bioinformatics plugins grouped by marketplace directory taxonomy while preserving atomic units.";
    case "visualization":
      return "Plugins for creating, editing, or reviewing visual outputs.";
    case "operator":
      return "Plugins that add agent-callable automation capabilities.";
    case "tools":
      return "Plugin bundles that expose model-callable functions or custom tool surfaces.";
    case "resource":
      return "Search / Query / Fetch external resource plugins grouped by resource type.";
    default:
      return "Notebook, workflow, and other plugin bundles.";
  }
}

export function groupPluginsByCatalogGroup(plugins: PluginSummary[]): PluginCatalogGroup[] {
  const order: PluginCatalogGroupId[] = [
    "analysis",
    "bioinformatics",
    "visualization",
    "resource",
    "operator",
    "tools",
    "other",
  ];
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
      plugins: grouped.get(id) ?? [],
    }))
    .filter((group) => group.plugins.length > 0);
}

function pluginCatalogSectionId(groupId: PluginCatalogGroupId, plugin: PluginSummary): string {
  if (groupId === "analysis") return `analysis:${primaryAnalysisCategory(plugin)}`;
  if (groupId === "bioinformatics") return `bioinformatics:${primaryBioinformaticsCategory(plugin)}`;
  if (groupId === "visualization") return `visualization:${primaryVisualizationCategory(plugin)}`;
  if (groupId === "operator") return "operator";
  if (groupId === "tools") return "function";
  if (groupId === "resource") return `resource:${primaryResourceCategory(plugin)}`;
  return `category:${plugin.interface?.category?.trim().toLowerCase() || "other"}`;
}

function pluginCatalogSectionLabel(groupId: PluginCatalogGroupId, sectionId: string): string {
  if (groupId === "analysis" && sectionId.startsWith("analysis:")) {
    return analysisCategoryLabel(sectionId.slice("analysis:".length));
  }
  if (groupId === "bioinformatics" && sectionId.startsWith("bioinformatics:")) {
    return bioinformaticsCategoryLabel(sectionId.slice("bioinformatics:".length));
  }
  if (groupId === "visualization" && sectionId.startsWith("visualization:")) {
    return visualizationCategoryLabel(sectionId.slice("visualization:".length));
  }
  if (groupId === "operator") return "Automation plugins";
  if (groupId === "tools") return "Function tools";
  if (groupId === "resource" && sectionId.startsWith("resource:")) {
    return resourceCategoryLabel(sectionId.slice("resource:".length));
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
      plugins: sectionPlugins,
    }))
    .sort((left, right) => {
      const orderDelta =
        pluginCatalogSectionOrder(groupId, left.id) - pluginCatalogSectionOrder(groupId, right.id);
      return orderDelta || left.title.localeCompare(right.title);
    });
}

function pluginCatalogSectionOrder(groupId: PluginCatalogGroupId, sectionId: string): number {
  if (groupId === "analysis" && sectionId.startsWith("analysis:")) {
    switch (sectionId.slice("analysis:".length)) {
      case "workflow":
        return 0;
      case "statistics":
        return 10;
      default:
        return 40;
    }
  }
  if (groupId === "bioinformatics" && sectionId.startsWith("bioinformatics:")) {
    switch (sectionId.slice("bioinformatics:".length)) {
      case "ngs":
        return 0;
      case "genomics":
        return 10;
      case "transcriptomics":
        return 20;
      case "proteomics":
        return 30;
      case "metabolomics":
        return 40;
      default:
        return 80;
    }
  }
  if (groupId === "visualization" && sectionId.startsWith("visualization:")) {
    switch (sectionId.slice("visualization:".length)) {
      case "r":
        return 0;
      case "python":
        return 10;
      default:
        return 40;
    }
  }
  if (groupId !== "resource" || !sectionId.startsWith("resource:")) return 50;
  switch (sectionId.slice("resource:".length)) {
    case "provider":
      return 0;
    case "dataset":
      return 10;
    case "knowledge":
      return 20;
    case "literature":
      return 30;
    default:
      return 40;
  }
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
  return `${capabilityLabel(status.category)}:${status.resourceId} · idle ${formatDuration(status.remainingMs)}`;
}

export function retrievalStatusDiagnostic(status: PluginRetrievalRouteStatus): {
  title: string;
  detail: string;
  lastError: string | null;
} {
  const title = status.route || `${status.category}.${status.resourceId}`;
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
    title: status.route || `${status.category}.${status.resourceId}`,
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

function isResourcePlugin(plugin: PluginSummary): boolean {
  const explicitResourceCategory = pluginHasAnyCategory(plugin, [
    "retrieval",
    "resource",
    "resources",
    "data source",
    "data sources",
    "literature",
    "dataset",
    "datasets",
    "knowledge",
    "provider",
  ]);
  const declaresRetrievalSurface = pluginHasTerm(plugin, ["retrieval", "search", "query", "fetch"]);
  return (
    Boolean(plugin.retrieval?.resources.length) ||
    explicitResourceCategory ||
    pluginLooksLikeResourceBundle(plugin) ||
    (declaresRetrievalSurface && resourceCategoryFromTerms(plugin) !== null)
  );
}

export type PluginCatalogFilter =
  | "all"
  | "available"
  | "installed"
  | "enabled"
  | "analysis"
  | "bioinformatics"
  | "visualization"
  | "operators"
  | "tools"
  | "resources"
  | "general";

const pluginCatalogFilterOptions: Array<{ value: PluginCatalogFilter; label: string }> = [
  { value: "all", label: "All" },
  { value: "available", label: "Available" },
  { value: "installed", label: "Installed" },
  { value: "enabled", label: "Enabled" },
  { value: "analysis", label: "Analysis" },
  { value: "bioinformatics", label: "Bioinformatics" },
  { value: "visualization", label: "Visualization" },
  { value: "operators", label: "Automation" },
  { value: "tools", label: "Tools" },
  { value: "resources", label: "Resources" },
  { value: "general", label: "Others" },
];

function pluginSearchText(plugin: PluginSummary): string {
  const retrievalText = (plugin.retrieval?.resources ?? [])
    .flatMap((resource) => [
      resource.id,
      resource.category,
      resource.label,
      resource.description,
      ...resource.subcategories,
      ...resource.capabilities,
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
  const environmentText = (plugin.environments ?? [])
    .flatMap((environment) => [
      environment.id,
      environment.canonicalId,
      environment.name,
      environment.description,
      environment.runtimeType,
      environment.runtimeFile,
      environment.runtimeFileKind,
      environment.availabilityStatus,
      environment.availabilityManager,
    ])
    .join(" ");
  return [
    plugin.id,
    plugin.name,
    plugin.marketplaceName,
    plugin.sourcePath,
    plugin.installedPath,
    interfaceText,
    retrievalText,
    environmentText,
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
    case "analysis":
      return pluginCatalogGroupId(plugin) === "analysis";
    case "bioinformatics":
      return pluginCatalogGroupId(plugin) === "bioinformatics";
    case "visualization":
      return pluginCatalogGroupId(plugin) === "visualization";
    case "operators":
      return pluginCatalogGroupId(plugin) === "operator";
    case "tools":
      return pluginCatalogGroupId(plugin) === "tools";
    case "resources":
      return pluginCatalogGroupId(plugin) === "resource";
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

function primaryResourceCategory(plugin: PluginSummary): string {
  const pathCategory = pluginCatalogSectionFromPath("resource", plugin);
  if (pathCategory) return pathCategory;
  const categories = Array.from(
    new Set((plugin.retrieval?.resources ?? []).map((resource) => resource.category).filter(Boolean)),
  );
  if (categories.length > 1) return "provider";
  return categories[0] || resourceCategoryFromTerms(plugin) || "other";
}

function resourceCategoryLabel(category: string): string {
  switch (category) {
    case "provider":
      return "Provider resources";
    case "dataset":
      return "Dataset resources";
    case "literature":
      return "Literature resources";
    case "knowledge":
      return "Knowledge resources";
    default:
      return `${capabilityLabel(category)} resources`;
  }
}

function primaryAnalysisCategory(plugin: PluginSummary): string {
  const pathCategory = pluginCatalogSectionFromPath("analysis", plugin);
  if (pathCategory) return pathCategory;
  if (pluginHasTerm(plugin, ["statistical", "statistics", "stats"])) return "statistics";
  if (pluginHasTerm(plugin, ["workflow", "pipeline"])) return "workflow";
  return "general";
}

function analysisCategoryLabel(category: string): string {
  switch (category) {
    case "statistics":
      return "Statistical analysis";
    case "workflow":
      return "Analysis workflows";
    default:
      return category === "general" ? "General analysis" : humanizeTaxonomySegment(category);
  }
}

function primaryBioinformaticsCategory(plugin: PluginSummary): string {
  const pathCategory = pluginCatalogSectionFromPath("bioinformatics", plugin);
  if (pathCategory) return pathCategory;
  const declaredCapabilities = (plugin.interface?.capabilities ?? [])
    .map((capability) => capability.trim().toLowerCase().replace(/[-_\s]+/g, "-"))
    .filter(Boolean);
  const taxonomyCapability = declaredCapabilities.find((capability) =>
    ["ngs", "genomics", "transcriptomics", "proteomics", "metabolomics"].includes(capability),
  );
  if (taxonomyCapability) return taxonomyCapability;
  return "other";
}

function bioinformaticsCategoryLabel(category: string): string {
  switch (category) {
    case "ngs":
      return "NGS";
    case "transcriptomics":
      return "Transcriptomics";
    case "genomics":
      return "Genomics";
    case "proteomics":
      return "Proteomics";
    case "metabolomics":
      return "Metabolomics";
    default:
      return humanizeTaxonomySegment(category);
  }
}

function primaryVisualizationCategory(plugin: PluginSummary): string {
  const pathCategory = pluginCatalogSectionFromPath("visualization", plugin);
  if (pathCategory) return pathCategory;
  if (isRVisualizationPlugin(plugin) || pluginHasTerm(plugin, ["rscript", "ggplot", "ggplot2"])) {
    return "r";
  }
  if (pluginHasTerm(plugin, ["python", "matplotlib", "seaborn", "plotly"])) {
    return "python";
  }
  return "template";
}

function visualizationCategoryLabel(category: string): string {
  switch (category) {
    case "r":
      return "R visualization";
    case "python":
      return "Python visualization";
    default:
      return category === "template" ? "Visualization templates" : humanizeTaxonomySegment(category);
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
      routeCount: plugin.retrieval?.resources.length ?? 0,
      issueCount: 0,
      pooledCount: 0,
      lastError: null,
    };
  }
  if (!plugin.enabled) {
    return {
      state: "disabled",
      label: "Disabled",
      routeCount: plugin.retrieval?.resources.length ?? 0,
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
      routeCount: retrievalStatuses.length || (plugin.retrieval?.resources.length ?? 0),
      issueCount: issueStatuses.length,
      pooledCount: processPoolStatuses.length,
      lastError,
    };
  }
  if (issueStatuses.length > 0) {
    return {
      state: "degraded",
      label: "Needs attention",
      routeCount: retrievalStatuses.length || (plugin.retrieval?.resources.length ?? 0),
      issueCount: issueStatuses.length,
      pooledCount: processPoolStatuses.length,
      lastError,
    };
  }
  if (retrievalStatuses.length === 0 && (plugin.retrieval?.resources.length ?? 0) > 0) {
    return {
      state: "idle",
      label: "No calls yet",
      routeCount: plugin.retrieval?.resources.length ?? 0,
      issueCount: 0,
      pooledCount: processPoolStatuses.length,
      lastError: null,
    };
  }
  return {
    state: "healthy",
    label: "Healthy",
    routeCount: retrievalStatuses.length || (plugin.retrieval?.resources.length ?? 0),
    issueCount: 0,
    pooledCount: processPoolStatuses.length,
    lastError: null,
  };
}

type PluginRuntimeSummary = ReturnType<typeof pluginRuntimeSummary>;

export interface VisualizationRTemplateGroup {
  id: string;
  title: string;
  count: number;
  items: string[];
  templates: VisualizationRTemplateSummary[];
}

export interface VisualizationRTemplateSummary {
  id: string;
  name: string;
  description?: string | null;
  execute?: unknown;
}

export interface VisualizationRCompletionOverview {
  totalTemplates: number;
  supportedGroups: VisualizationRTemplateGroup[];
  quickStarts: VisualizationRTemplateSummary[];
  outputs: string[];
  runtime: string;
  workflow: Array<{
    title: string;
    detail: string;
  }>;
  pending: string[];
}

const defaultVisualizationRTemplateGroups: VisualizationRTemplateGroup[] = [];

function visualizationRTemplateGroups(
  templates?: PluginTemplateSummary | null,
): VisualizationRTemplateGroup[] {
  if (!templates?.groups?.length) return defaultVisualizationRTemplateGroups;
  return templates.groups.map((group) => {
    const groupTemplates = group.templates.map((template) => {
      const summary: VisualizationRTemplateSummary = {
        id: template.id,
        name: template.name,
        description: template.description,
      };
      if (template.execute !== undefined && template.execute !== null) {
        summary.execute = template.execute;
      }
      return summary;
    });
    return {
      id: group.id,
      title: group.title,
      count: group.count,
      items: groupTemplates.map((template) => template.name),
      templates: groupTemplates,
    };
  });
}

function selectVisualizationRQuickStarts(
  groups: VisualizationRTemplateGroup[],
): VisualizationRTemplateSummary[] {
  return groups.flatMap((group) => group.templates).slice(0, 3);
}

export function visualizationRTemplatePrompt(template: VisualizationRTemplateSummary): string {
  return `Use Template \`${template.id}\` (${template.name}) to generate an editable figure from my CSV/TSV data. First confirm the required columns, then call template_execute with my data file and suitable params.`;
}

export function visualizationRTemplateToolCall(template: VisualizationRTemplateSummary): string {
  const fallback = {
    tool: "template_execute",
    arguments: {
      id: template.id,
      inputs: {
        table: "path/to/data.tsv",
      },
      params: {},
      resources: {},
    },
  };
  return JSON.stringify(
    template.execute ?? fallback,
    null,
    2,
  );
}

export function visualizationRCompletionOverview(
  templates?: PluginTemplateSummary | null,
): VisualizationRCompletionOverview {
  const supportedGroups = visualizationRTemplateGroups(templates);
  return {
    totalTemplates: templates?.count ?? supportedGroups.reduce((count, group) => count + group.count, 0),
    supportedGroups,
    quickStarts: selectVisualizationRQuickStarts(supportedGroups),
    outputs: ["PNG", "PDF", "editable R script"],
    runtime: "Rscript + ggplot2; table-driven CSV/TSV inputs",
    workflow: [
      {
        title: "1. Prepare table",
        detail: "Use CSV/TSV data matching the selected template columns.",
      },
      {
        title: "2. Generate figure",
        detail: "Choose an exact viz_* Template ID, then ask the agent to call template_execute.",
      },
      {
        title: "3. Refine source",
        detail: "Edit or promote the generated R script for reusable style preferences.",
      },
    ],
    pending: [],
  };
}

export function shouldShowPluginRuntimeSummaryCard(
  plugin: PluginSummary,
  runtimeSummary: PluginRuntimeSummary,
  declaredRetrievalResources: unknown[] = [],
  processPoolStatuses: unknown[] = [],
): boolean {
  const hasActionableRuntimeState =
    runtimeSummary.state !== "healthy"
    || runtimeSummary.issueCount > 0
    || Boolean(runtimeSummary.lastError)
    || processPoolStatuses.length > 0;
  return hasActionableRuntimeState || (declaredRetrievalResources.length > 0 && !plugin.enabled);
}

export function pluginCardSubtitle(plugin: PluginSummary): string {
  const resources = plugin.retrieval?.resources ?? [];
  if (resources.length === 1) {
    return resources[0].label || `${capabilityLabel(resources[0].category)} resource`;
  }
  if (resources.length > 1) {
    const categories = Array.from(new Set(resources.map((resource) => resource.category).filter(Boolean)));
    if (categories.length > 1) {
      return `${resources.length} routes: ${previewList(resources.map((resource) => resource.label || resource.id), 4)}`;
    }
    const category = capabilityLabel(resources[0].category);
    return `${resources.length} ${category} routes`;
  }
  return description(plugin);
}

export interface PluginContentOverviewItem {
  id: "visualization" | "library" | "automation" | "routes" | "tools" | "prompt" | "bundle";
  title: string;
  detail: string;
  meta: string;
}

function previewList(values: string[], limit = 3): string {
  const cleaned = values
    .map((value) => value.trim())
    .filter(Boolean);
  if (cleaned.length === 0) return "";
  const visible = cleaned.slice(0, limit).join(", ");
  const hidden = cleaned.length - limit;
  return hidden > 0 ? `${visible} · +${hidden} more` : visible;
}

export function pluginContentOverview(
  plugin: PluginSummary,
  operators: OperatorSummary[] = [],
): PluginContentOverviewItem[] {
  const items: PluginContentOverviewItem[] = [];
  const resources = plugin.retrieval?.resources ?? [];
  const primaryPrompt = plugin.interface?.defaultPrompt?.[0]?.trim();

  if (isVisualizationPlugin(plugin)) {
    items.push({
      id: "visualization",
      title: "Visualization",
      detail: "Create editable figures and publication-style plots from human-editable R artifacts.",
      meta: "Figures",
    });
  } else if (isTemplatePlugin(plugin)) {
    items.push({
      id: "library",
      title: "Reusable library",
      detail: "Provides reusable workflows for generated artifacts.",
      meta: "Library",
    });
  }

  if (operators.length > 0) {
    const operationCount = operatorPluginOperationCount(operators);
    items.push({
      id: "automation",
      title: "Operator programs",
      detail: previewList(operators.map(operatorDisplayName)) || "Plugin-declared Operator programs agents can call.",
      meta: `${operationCount} op${operationCount === 1 ? "" : "s"}`,
    });
  }

  if (resources.length > 0) {
    items.push({
      id: "routes",
      title: "Search / Query / Fetch routes",
      detail:
        previewList(
          resources.map((resource) => resource.label || `${capabilityLabel(resource.category)} ${resource.id}`),
        ) || "Plugin-defined retrieval routes.",
      meta: `${resources.length}`,
    });
  }

  if (isFunctionPlugin(plugin)) {
    items.push({
      id: "tools",
      title: "Tool surface",
      detail: "Model-callable functions or custom tool integrations declared by this plugin.",
      meta: "Tools",
    });
  }

  if (primaryPrompt && items.length === 0) {
    items.push({
      id: "prompt",
      title: "Suggested use",
      detail: primaryPrompt,
      meta: "Prompt",
    });
  }

  if (items.length === 0) {
    items.push({
      id: "bundle",
      title: "Plugin bundle",
      detail: "Contributes workflows, automation, metadata, or connector references. Technical files stay hidden unless needed for troubleshooting.",
      meta: "Bundle",
    });
  }

  return items;
}

export function operatorDisplayName(operator: OperatorSummary): string {
  return operator.name?.trim() || operator.id;
}

type OperatorOperationSummary = NonNullable<OperatorSummary["operations"]>[number];

function operatorOperationDisplayName(operation: OperatorOperationSummary): string {
  return operation.name?.trim() || operation.id;
}

function operatorOperationCount(operator: OperatorSummary): number {
  return Math.max(operator.operations?.length ?? 0, 1);
}

function operatorExposedOperationCount(operator: OperatorSummary): number {
  const operations = operator.operations ?? [];
  if (operations.length === 0) return operator.exposed ? 1 : 0;
  return operations.filter((operation) => operation.exposed !== false && operator.exposed).length;
}

function operatorPluginOperationCount(operators: OperatorSummary[]): number {
  return operators.reduce((total, operator) => total + operatorOperationCount(operator), 0);
}

function operatorPluginExposedOperationCount(operators: OperatorSummary[]): number {
  return operators.reduce((total, operator) => total + operatorExposedOperationCount(operator), 0);
}

function operationTaxonomyLabel(operation: OperatorOperationSummary): string {
  return (
    operation.stage?.trim()
    || operation.group?.trim()
    || operation.category?.trim()
    || "Operations"
  );
}

function operatorOperationGroups(operator: OperatorSummary): Array<{
  key: string;
  label: string;
  operations: OperatorOperationSummary[];
}> {
  const grouped = new Map<string, { key: string; label: string; operations: OperatorOperationSummary[] }>();
  for (const operation of operator.operations ?? []) {
    const label = operationTaxonomyLabel(operation);
    const key = label.trim().toLowerCase().replace(/[^a-z0-9/-]+/g, "-") || "operations";
    const current = grouped.get(key) ?? { key, label, operations: [] };
    current.operations.push(operation);
    grouped.set(key, current);
  }
  return Array.from(grouped.values()).sort((left, right) => left.label.localeCompare(right.label));
}

function templateDisplayName(template: PluginTemplateSummary["groups"][number]["templates"][number]): string {
  return template.name?.trim() || template.id;
}

function templateEnabled(template: PluginTemplateSummary["groups"][number]["templates"][number]): boolean {
  return template.exposed !== false;
}

function retrievalResourceEnabled(source: PluginRetrievalResourceSummary): boolean {
  return source.exposed !== false;
}

function retrievalResourceDisplayName(source: PluginRetrievalResourceSummary): string {
  return source.label?.trim() || capabilityLabel(source.id);
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

function stringRecord(value: unknown): Record<string, unknown> | null {
  return value && typeof value === "object" && !Array.isArray(value)
    ? value as Record<string, unknown>
    : null;
}

function numberField(value: unknown): number | null {
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

function stringFieldValue(value: unknown): string | null {
  return typeof value === "string" && value.trim().length > 0 ? value.trim() : null;
}

function stringArrayField(value: unknown): string[] {
  return Array.isArray(value)
    ? value.filter((item): item is string => typeof item === "string" && item.trim().length > 0)
    : [];
}

export function operatorResourceProfile(operator: OperatorSummary): OperatorRuntimeResourceProfile | null {
  const runtime = stringRecord(operator.runtime);
  const profile = stringRecord(runtime?.resourceProfile);
  if (!profile) return null;
  return {
    tier: stringFieldValue(profile.tier),
    localPolicy: stringFieldValue(profile.localPolicy),
    minCpu: numberField(profile.minCpu),
    recommendedCpu: numberField(profile.recommendedCpu),
    minMemoryGb: numberField(profile.minMemoryGb),
    recommendedMemoryGb: numberField(profile.recommendedMemoryGb),
    diskGb: numberField(profile.diskGb),
    notes: stringArrayField(profile.notes),
  };
}

function normalizedResourceTier(profile: OperatorRuntimeResourceProfile | null): string {
  return profile?.tier?.trim().toLowerCase().replace(/_/g, "-") ?? "";
}

function normalizedLocalPolicy(profile: OperatorRuntimeResourceProfile | null): string {
  return profile?.localPolicy?.trim().toLowerCase().replace(/_/g, "-") ?? "";
}

export function operatorResourceProfileLabel(operator: OperatorSummary): string | null {
  const tier = normalizedResourceTier(operatorResourceProfile(operator));
  if (!tier || tier === "local-ok") return null;
  if (tier === "hpc-required") return "HPC required";
  if (tier === "hpc-recommended" || tier === "server-recommended") return "HPC recommended";
  if (tier === "local-warn") return "Local warning";
  if (tier === "heavy") return "Heavy";
  return capabilityLabel(tier);
}

function operatorResourceProfileColor(operator: OperatorSummary): ChipProps["color"] {
  const tier = normalizedResourceTier(operatorResourceProfile(operator));
  if (tier === "hpc-required") return "error";
  if (tier === "heavy" || tier === "hpc-recommended" || tier === "server-recommended" || tier === "local-warn") {
    return "warning";
  }
  return "default";
}

export function operatorResourceProfileSummary(operator: OperatorSummary): string | null {
  const profile = operatorResourceProfile(operator);
  const label = operatorResourceProfileLabel(operator);
  if (!profile || !label) return null;
  const parts = [label];
  if (profile.recommendedCpu) parts.push(`${profile.recommendedCpu} CPU recommended`);
  if (profile.recommendedMemoryGb) parts.push(`${profile.recommendedMemoryGb} GB RAM recommended`);
  if (profile.diskGb) parts.push(`${profile.diskGb} GB disk`);
  const notes = profile.notes?.slice(0, 2).join(" ");
  return notes ? `${parts.join(" · ")}. ${notes}` : parts.join(" · ");
}

export function operatorShouldWarnBeforeLocalRun(operator: OperatorSummary): boolean {
  const profile = operatorResourceProfile(operator);
  const policy = normalizedLocalPolicy(profile);
  const tier = normalizedResourceTier(profile);
  return (
    policy === "warn" ||
    policy === "block" ||
    tier === "heavy" ||
    tier === "local-warn" ||
    tier === "hpc-recommended" ||
    tier === "server-recommended" ||
    tier === "hpc-required"
  );
}

function operatorResourceProfileChip(operator: OperatorSummary) {
  const label = operatorResourceProfileLabel(operator);
  if (!label) return null;
  return (
    <Chip
      size="small"
      color={operatorResourceProfileColor(operator)}
      variant="outlined"
      label={`⚡ ${label}`}
    />
  );
}

type PluginUnitKind = "operator" | "template";

function pluginUnitKindColor(kind: PluginUnitKind): NonNullable<ChipProps["color"]> {
  return kind === "operator" ? "primary" : "secondary";
}

function pluginUnitKindChipSx(kind: PluginUnitKind): ChipProps["sx"] {
  return (theme: Theme) => {
    const tone = kind === "operator" ? theme.palette.primary.main : theme.palette.secondary.main;
    return {
      bgcolor: alpha(tone, theme.palette.mode === "dark" ? 0.14 : 0.07),
      borderColor: alpha(tone, theme.palette.mode === "dark" ? 0.62 : 0.38),
      color: tone,
      fontWeight: 800,
    };
  };
}

function operatorResourceProfilePriority(operator: OperatorSummary): number {
  const tier = normalizedResourceTier(operatorResourceProfile(operator));
  if (tier === "hpc-required") return 4;
  if (tier === "hpc-recommended" || tier === "server-recommended") return 3;
  if (tier === "heavy" || tier === "local-warn") return 2;
  return 0;
}

function pluginRepresentativeResourceOperator(operators: OperatorSummary[]): OperatorSummary | null {
  return operators
    .filter((operator) => operatorResourceProfileLabel(operator))
    .sort((left, right) =>
      operatorResourceProfilePriority(right) - operatorResourceProfilePriority(left)
      || operatorDisplayName(left).localeCompare(operatorDisplayName(right)),
    )[0] ?? null;
}

function pluginResourceProfileChip(operators: OperatorSummary[], compact = false) {
  const representative = pluginRepresentativeResourceOperator(operators);
  if (!representative) return null;
  const label = operatorResourceProfileLabel(representative);
  if (!label) return null;
  const profiledCount = operators.filter((operator) => operatorResourceProfileLabel(operator)).length;
  const tier = normalizedResourceTier(operatorResourceProfile(representative));
  const compactLabel =
    tier === "hpc-required"
      ? "HPC required"
      : tier === "hpc-recommended" || tier === "server-recommended"
        ? "HPC"
        : label;
  const title = [
    `${profiledCount} resource-marked operator${profiledCount === 1 ? "" : "s"}.`,
    operatorResourceProfileSummary(representative) ?? label,
  ].join(" ");
  return (
    <Tooltip title={title}>
      <Chip
        size="small"
        color={operatorResourceProfileColor(representative)}
        variant="outlined"
        label={`⚡ ${compact ? compactLabel : label}${!compact && profiledCount > 1 ? ` · ${profiledCount}` : ""}`}
      />
    </Tooltip>
  );
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

function runtimeStringValue(runtime: unknown, key: string): string | null {
  if (!runtime || typeof runtime !== "object" || Array.isArray(runtime)) return null;
  const value = (runtime as Record<string, unknown>)[key];
  return typeof value === "string" && value.trim().length > 0 ? value.trim() : null;
}

export function operatorEnvironmentRef(operator: OperatorSummary): string | null {
  return (
    runtimeStringValue(operator.runtime, "envRef")
    ?? runtimeStringValue(operator.runtime, "environmentRef")
    ?? runtimeStringValue(operator.runtime, "environment")
  );
}

export function pluginEnvironmentDisplayName(environment: PluginEnvironmentSummary): string {
  return environment.name?.trim() || environment.id;
}

export function pluginEnvironmentStatusColor(
  status: string,
): "success" | "warning" | "error" | "info" | "default" {
  switch (status.trim().toLowerCase()) {
    case "available":
    case "ready":
    case "ok":
      return "success";
    case "missing":
    case "not_found":
    case "not-found":
    case "unavailable":
      return "warning";
    case "failed":
    case "error":
    case "invalid":
      return "error";
    case "not_run":
    case "not-run":
    case "pending":
      return "info";
    default:
      return "default";
  }
}

function pathBasename(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).pop() || path;
}

export function pluginEnvironmentRuntimeFileLabel(environment: PluginEnvironmentSummary): string {
  const kind = environment.runtimeFileKind?.trim() || `${environment.runtimeType || "runtime"} file`;
  if (!environment.runtimeFile?.trim()) return `${kind}: not declared`;
  const filename = pathBasename(environment.runtimeFile);
  const kindAlternatives = kind.split("|").map((item) => item.trim()).filter(Boolean);
  if (kind === filename || kindAlternatives.includes(filename)) return filename;
  return `${kind}: ${filename}`;
}

function pluginEnvironmentRuntimeKey(environment: PluginEnvironmentSummary): string {
  return environment.runtimeType.trim().toLowerCase() || "system";
}

function pluginSyncNeedsAction(plugin: PluginSummary): boolean {
  return Boolean(
    plugin.installed &&
    plugin.sync &&
    !["upToDate"].includes(plugin.sync.state),
  );
}

function pluginSyncButtonLabel(plugin: PluginSummary): string {
  const state = plugin.sync?.state;
  if (state === "conflictRisk" || state === "localModified") return "Review sync";
  if (state === "unknown") return "Track sync";
  return "Sync";
}

function pluginSyncTooltip(plugin: PluginSummary): string {
  const label = pluginSyncButtonLabel(plugin);
  const message = plugin.sync?.message?.trim();
  return message ? `${label}: ${message}` : label;
}

function pluginCanForceSync(plugin: PluginSummary): boolean {
  return Boolean(plugin.installed && plugin.sync && plugin.sync.state !== "upToDate");
}

function pluginSyncChipColor(state?: string): "default" | "info" | "warning" | "error" | "success" {
  switch (state) {
    case "upToDate":
      return "success";
    case "updateAvailable":
      return "info";
    case "localModified":
      return "warning";
    case "conflictRisk":
      return "error";
    case "unknown":
      return "warning";
    default:
      return "default";
  }
}

function remoteMarketplaceCheckMessage(results: MarketplaceRemoteCheckResult[]): string {
  if (results.length === 0) return "No remote marketplace is configured yet.";
  const errors = results.filter((result) => result.state === "error");
  const updates = results.filter((result) => result.state === "updateAvailable");
  if (errors.length > 0) {
    return `${errors.length} remote marketplace check${errors.length === 1 ? "" : "s"} failed.`;
  }
  if (updates.length > 0) {
    const changed = updates.reduce((total, result) => total + result.changedPlugins.length, 0);
    return `${updates.length} remote marketplace update${updates.length === 1 ? "" : "s"} available${changed > 0 ? ` · ${changed} plugin${changed === 1 ? "" : "s"} changed` : ""}.`;
  }
  return "All remote marketplaces are up to date.";
}

function marketplaceSourceLabel(source: { label?: string | null; location: string }): string {
  const label = source.label?.trim();
  return label || source.location;
}

export function marketplaceSourceRefreshMessage(result: RefreshResult): string {
  if (!result.ok) return result.message;
  const details = [
    result.marketplaceName?.trim() ? result.marketplaceName.trim() : null,
    typeof result.pluginCount === "number"
      ? `${result.pluginCount} plugin${result.pluginCount === 1 ? "" : "s"}`
      : null,
  ].filter(Boolean);
  return details.length > 0
    ? `${result.message} · ${details.join(" · ")}`
    : result.message;
}

function pluginMigrationCount(
  count: number,
  singular: string,
  plural: string,
): string {
  return `${count} ${count === 1 ? singular : plural}`;
}

export function pluginMigrationSummary(result: PluginMigrationResult): string {
  return [
    result.configRewritten ? "config rewritten" : "config unchanged",
    pluginMigrationCount(
      result.legacyCacheEntriesMigrated,
      "legacy cache entry migrated",
      "legacy cache entries migrated",
    ),
    pluginMigrationCount(
      result.builtinRootsRefreshed,
      "built-in root refreshed",
      "built-in roots refreshed",
    ),
  ].join(" · ");
}

export function remoteMarketplaceChangedPluginNames(
  results: MarketplaceRemoteCheckResult[],
): Set<string> {
  const names = new Set<string>();
  for (const result of results) {
    if (result.state !== "updateAvailable") continue;
    for (const pluginName of result.changedPlugins) {
      const normalized = pluginName.trim();
      if (normalized.length > 0) names.add(normalized);
    }
  }
  return names;
}

export function remoteMarketplaceCheckSignature(
  results: MarketplaceRemoteCheckResult[],
): string {
  return results
    .map((result) =>
      [
        result.name,
        result.path,
        result.state,
        result.localDigest ?? "",
        result.remoteDigest ?? "",
        [...result.changedPlugins].sort((left, right) => left.localeCompare(right)).join(","),
      ].join("|")
    )
    .sort((left, right) => left.localeCompare(right))
    .join("||");
}

export function pluginHasRemoteMarketplaceUpdate(
  plugin: PluginSummary,
  changedPluginNames: Set<string>,
): boolean {
  return (
    changedPluginNames.has(plugin.name) ||
    changedPluginNames.has(plugin.id) ||
    changedPluginNames.has(plugin.id.split("@")[0])
  );
}

function pluginEnvironmentByRef(
  environments: PluginEnvironmentSummary[],
): Map<string, PluginEnvironmentSummary> {
  const byRef = new Map<string, PluginEnvironmentSummary>();
  for (const environment of environments) {
    byRef.set(environment.id, environment);
    byRef.set(environment.canonicalId, environment);
  }
  return byRef;
}

type PluginEnvironmentCheckState = {
  loading: boolean;
  result?: EnvironmentCheckResult | null;
  error?: string | null;
};

function pluginEnvironmentKey(environment: PluginEnvironmentSummary): string {
  return environment.canonicalId || environment.id;
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

type OperatorSlurmDiagnostic = {
  category?: string | null;
  state?: string | null;
  exitCode?: string | null;
  suggestedAction?: string | null;
};

type OperatorRunSummaryWithSlurmDiagnostic = OperatorRunSummary & {
  slurmDiagnostic?: unknown;
};

function operatorSlurmDiagnosticFromUnknown(value: unknown): OperatorSlurmDiagnostic | null {
  const diagnostic = stringRecord(value);
  if (!diagnostic) return null;
  const category = stringFieldValue(diagnostic.category);
  const state = stringFieldValue(diagnostic.state);
  const exitCode = stringFieldValue(diagnostic.exitCode);
  const suggestedAction = stringFieldValue(diagnostic.suggestedAction);
  if (!category && !state && !exitCode && !suggestedAction) return null;
  return { category, state, exitCode, suggestedAction };
}

function operatorRunSlurmDiagnostic(
  run: OperatorRunSummary,
  detail?: OperatorRunDetail | null,
): OperatorSlurmDiagnostic | null {
  const runDiagnostic = operatorSlurmDiagnosticFromUnknown(
    (run as OperatorRunSummaryWithSlurmDiagnostic).slurmDiagnostic,
  );
  if (runDiagnostic) return runDiagnostic;

  const document = stringRecord(detail?.document);
  const error = stringRecord(document?.error);
  return operatorSlurmDiagnosticFromUnknown(error?.slurmDiagnostic);
}

function formatSlurmDiagnosticCategory(category?: string | null): string {
  switch (category) {
    case "oom":
      return "OOM";
    case "timeout":
      return "Timeout";
    case "cancelled":
      return "Cancelled";
    case "failedExit":
      return "Failed exit";
    case "other":
      return "Other";
    default:
      return category || "SLURM";
  }
}

function formatSlurmDiagnosticSummary(diagnostic: OperatorSlurmDiagnostic): string {
  return [
    formatSlurmDiagnosticCategory(diagnostic.category),
    diagnostic.state,
    diagnostic.exitCode ? `exit ${diagnostic.exitCode}` : null,
  ].filter(Boolean).join(" - ");
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
        slurmDiagnostic: operatorRunSlurmDiagnostic(run),
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

function PluginCard({
  plugin,
  retrievalStatuses = [],
  operators = [],
  remoteUpdateAvailable = false,
  busy,
  onInstall,
  onToggle,
  onOpenDetails,
}: {
  plugin: PluginSummary;
  retrievalStatuses?: PluginRetrievalRouteStatus[];
  processPoolStatuses?: PluginProcessPoolRouteStatus[];
  operators?: OperatorSummary[];
  remoteUpdateAvailable?: boolean;
  busy: boolean;
  onInstall: (plugin: PluginSummary) => void;
  onToggle: (plugin: PluginSummary, enabled: boolean) => void;
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
  const exposedOperatorCount = operatorPluginExposedOperationCount(operators);
  const operationCount = operatorPluginOperationCount(operators);
  const operatorExposureLabel = operators.length > 0
    ? plugin.installed && plugin.enabled
      ? `${exposedOperatorCount}/${operationCount} operations exposed`
      : `${operationCount} operations available`
    : null;
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
        <Stack direction="row" gap={0.75} alignItems="center" sx={{ minWidth: 0 }}>
          <Typography
            variant="subtitle2"
            fontWeight={800}
            noWrap
            title={displayName(plugin)}
            sx={{ minWidth: 0 }}
          >
            {displayName(plugin)}
          </Typography>
          {remoteUpdateAvailable && (
            <Tooltip title="Remote marketplace has an update for this plugin">
              <Chip
                size="small"
                color="warning"
                variant="outlined"
                label="Update"
                sx={{ height: 22, flexShrink: 0 }}
              />
            </Tooltip>
          )}
          {pluginResourceProfileChip(operators, true)}
          {operatorExposureLabel && (
            <Chip
              size="small"
              variant="outlined"
              label={operatorExposureLabel}
              sx={{ height: 22, flexShrink: 0 }}
            />
          )}
        </Stack>
        <Typography variant="body2" color="text.secondary" noWrap title={subtitle} sx={{ mt: 0.15 }}>
          {subtitle}
        </Typography>
      </Box>

      {plugin.installed ? (
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
  remoteChangedPluginNames,
  busy,
  busyPluginIds,
  onInstall,
  onToggle,
  onOpenDetails,
}: {
  plugins: PluginSummary[];
  retrievalStatusesByPlugin: Map<string, PluginRetrievalRouteStatus[]>;
  processPoolStatusesByPlugin: Map<string, PluginProcessPoolRouteStatus[]>;
  operatorsByPlugin: Map<string, OperatorSummary[]>;
  remoteChangedPluginNames: Set<string>;
  busy: boolean;
  busyPluginIds: Set<string>;
  onInstall: (plugin: PluginSummary) => void;
  onToggle: (plugin: PluginSummary, enabled: boolean) => void;
  onOpenDetails: (plugin: PluginSummary) => void;
}) {
  const groups = groupPluginsByCatalogGroup(plugins);

  return (
    <>
      {groups.map((group) => {
        const sections = groupPluginsByCatalogSection(group.id, group.plugins);
        return (
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
                {sections.map((section) => (
                  <Box key={section.id}>
                    <Typography variant="subtitle2" color="text.secondary" sx={{ mb: 1 }}>
                      {section.title}
                    </Typography>
                    <Box sx={pluginCardGridSx}>
                      {section.plugins.map((plugin) => {
                        const pluginOperators =
                          operatorsByPlugin.get(plugin.id)
                          ?? operatorsByPlugin.get(plugin.name)
                          ?? plugin.operators
                          ?? [];
                        return (
                          <PluginCard
                            key={plugin.id}
                            plugin={plugin}
                            retrievalStatuses={retrievalStatusesByPlugin.get(plugin.id)}
                            processPoolStatuses={processPoolStatusesByPlugin.get(plugin.id)}
                            operators={pluginOperators}
                            remoteUpdateAvailable={pluginHasRemoteMarketplaceUpdate(plugin, remoteChangedPluginNames)}
                            busy={busy || busyPluginIds.has(plugin.id)}
                            onInstall={onInstall}
                            onToggle={onToggle}
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
        );
      })}
    </>
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

function VisualizationRDetailOverview({ templates }: { templates?: PluginTemplateSummary | null }) {
  const theme = useTheme();
  const overview = visualizationRCompletionOverview(templates);
  const [copiedTemplateId, setCopiedTemplateId] = useState<string | null>(null);
  const copyQuickStart = (template: VisualizationRTemplateSummary) => {
    const text = [
      visualizationRTemplatePrompt(template),
      "",
      "Tool-call skeleton:",
      visualizationRTemplateToolCall(template),
    ].join("\n");
    void globalThis.navigator?.clipboard?.writeText(text);
    setCopiedTemplateId(template.id);
    window.setTimeout(() => setCopiedTemplateId((current) => (current === template.id ? null : current)), 1500);
  };

  return (
    <Stack spacing={1.25}>
      <Stack
        direction={{ xs: "column", sm: "row" }}
        spacing={1}
        alignItems={{ xs: "flex-start", sm: "center" }}
        justifyContent="space-between"
      >
        <Box sx={{ minWidth: 0 }}>
          <Typography variant="subtitle1" fontWeight={850}>
            Supported figure types
          </Typography>
          <Typography variant="body2" color="text.secondary" sx={{ mt: 0.35, lineHeight: 1.45 }}>
            Current bundle ships {overview.totalTemplates} table-driven R/ggplot2 templates. Runs export {overview.outputs.join(", ")} for publication-style editing and handoff.
          </Typography>
        </Box>
        <Chip size="small" color="success" variant="outlined" label={`${overview.totalTemplates} templates ready`} />
      </Stack>

      <Box
        sx={{
          display: "grid",
          gridTemplateColumns: { xs: "1fr", md: "repeat(2, minmax(0, 1fr))" },
          gap: 1,
        }}
      >
        {overview.supportedGroups.map((group) => (
          <Paper
            key={group.id}
            variant="outlined"
            sx={{
              p: 1.25,
              borderRadius: 2.25,
              bgcolor: alpha(theme.palette.background.default, theme.palette.mode === "dark" ? 0.36 : 0.55),
            }}
          >
            <Stack direction="row" gap={0.75} alignItems="center" flexWrap="wrap">
              <Typography variant="subtitle2" fontWeight={850}>
                {group.title}
              </Typography>
              <Chip size="small" variant="outlined" label={`${group.count}`} />
            </Stack>
            <Typography variant="body2" color="text.secondary" sx={{ mt: 0.65, lineHeight: 1.45 }}>
              {group.items.join(", ")}
            </Typography>
            <Stack direction="row" gap={0.6} flexWrap="wrap" sx={{ mt: 0.9 }}>
              {group.templates.map((template) => (
                <Chip
                  key={template.id}
                  size="small"
                  variant="outlined"
                  label={`${template.name} · ${template.id}`}
                  sx={{
                    maxWidth: "100%",
                    "& .MuiChip-label": {
                      overflow: "hidden",
                      textOverflow: "ellipsis",
                    },
                  }}
                />
              ))}
            </Stack>
          </Paper>
        ))}
      </Box>

      <Paper
        variant="outlined"
        sx={{
          p: 1.25,
          borderRadius: 2.25,
          bgcolor: alpha(theme.palette.success.main, theme.palette.mode === "dark" ? 0.11 : 0.045),
        }}
      >
        <Stack spacing={1}>
          <Stack direction="row" gap={0.75} alignItems="center" flexWrap="wrap">
            <Typography variant="subtitle2" fontWeight={850}>
              How to use
            </Typography>
            <Chip size="small" color="success" variant="outlined" label={overview.outputs.join(" + ")} />
          </Stack>
          <Box
            sx={{
              display: "grid",
              gridTemplateColumns: { xs: "1fr", md: "repeat(3, minmax(0, 1fr))" },
              gap: 1,
            }}
          >
            {overview.workflow.map((step) => (
              <Box key={step.title} sx={{ minWidth: 0 }}>
                <Typography variant="caption" fontWeight={850} color="text.primary">
                  {step.title}
                </Typography>
                <Typography variant="caption" color="text.secondary" sx={{ display: "block", mt: 0.25, lineHeight: 1.45 }}>
                  {step.detail}
                </Typography>
              </Box>
            ))}
          </Box>
        </Stack>
      </Paper>

      <Paper
        variant="outlined"
        sx={{
          p: 1.25,
          borderRadius: 2.25,
          bgcolor: alpha(theme.palette.info.main, theme.palette.mode === "dark" ? 0.12 : 0.045),
        }}
      >
        <Stack spacing={1}>
          <Stack direction="row" gap={0.75} alignItems="center" flexWrap="wrap">
            <Typography variant="subtitle2" fontWeight={850}>
              Execution shortcuts
            </Typography>
            <Chip size="small" color="info" variant="outlined" label="template_execute" />
          </Stack>
          <Typography variant="caption" color="text.secondary" sx={{ lineHeight: 1.45 }}>
            Pick an exact Template ID from the chips above, or copy one of these starter prompts into chat. The agent can inspect details with <Box component="span" sx={{ fontFamily: "ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace" }}>unit_describe</Box> before running.
          </Typography>
          <Box
            sx={{
              display: "grid",
              gridTemplateColumns: { xs: "1fr", md: "repeat(3, minmax(0, 1fr))" },
              gap: 1,
            }}
          >
            {overview.quickStarts.map((template) => (
              <Paper
                key={template.id}
                variant="outlined"
                sx={{ p: 1, borderRadius: 2, bgcolor: "background.paper", minWidth: 0 }}
              >
                <Stack spacing={0.75}>
                  <Box sx={{ minWidth: 0 }}>
                    <Typography variant="caption" fontWeight={850} color="text.primary">
                      {template.name}
                    </Typography>
                    <Typography
                      variant="caption"
                      color="text.secondary"
                      sx={{
                        display: "block",
                        fontFamily: "ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace",
                        overflowWrap: "anywhere",
                      }}
                    >
                      {template.id}
                    </Typography>
                  </Box>
                  <Typography variant="caption" color="text.secondary" sx={{ lineHeight: 1.45 }}>
                    {visualizationRTemplatePrompt(template)}
                  </Typography>
                  <Box
                    component="pre"
                    sx={{
                      ...visualizationRExecuteSkeletonSx,
                      bgcolor: alpha(theme.palette.text.primary, theme.palette.mode === "dark" ? 0.12 : 0.06),
                    }}
                  >
                    {visualizationRTemplateToolCall(template)}
                  </Box>
                  <Button
                    size="small"
                    variant="outlined"
                    startIcon={<ContentCopyRounded />}
                    onClick={() => copyQuickStart(template)}
                    sx={{ textTransform: "none", borderRadius: 1.5, alignSelf: "flex-start" }}
                  >
                    {copiedTemplateId === template.id ? "Copied" : "Copy prompt"}
                  </Button>
                </Stack>
              </Paper>
            ))}
          </Box>
        </Stack>
      </Paper>

      {overview.pending.length > 0 ? (
        <Paper
          variant="outlined"
          sx={{
            p: 1.25,
            borderRadius: 2.25,
            bgcolor: alpha(theme.palette.warning.main, theme.palette.mode === "dark" ? 0.12 : 0.05),
          }}
        >
          <Stack spacing={0.8}>
            <Stack direction="row" gap={0.75} alignItems="center" flexWrap="wrap">
              <Typography variant="subtitle2" fontWeight={850}>
                Planned / not implemented yet
              </Typography>
              <Chip size="small" color="warning" variant="outlined" label="roadmap" />
            </Stack>
            <Stack direction="row" gap={0.75} flexWrap="wrap">
              {overview.pending.map((item) => (
                <Chip key={item} size="small" variant="outlined" label={item} />
              ))}
            </Stack>
            <Typography variant="caption" color="text.secondary">
              Runtime scope: {overview.runtime}. First-phase templates intentionally avoid auto-installing R packages.
            </Typography>
          </Stack>
        </Paper>
      ) : null}
    </Stack>
  );
}

function PluginUnitControls({
  plugin,
  operators,
  busy,
  onTemplateToggle,
  onRetrievalResourceToggle,
}: {
  plugin: PluginSummary;
  operators: OperatorSummary[];
  busy: boolean;
  onTemplateToggle: (plugin: PluginSummary, templateId: string, enabled: boolean) => void;
  onRetrievalResourceToggle: (
    plugin: PluginSummary,
    category: string,
    resourceId: string,
    enabled: boolean,
  ) => void;
}) {
  const retrievalResources = plugin.retrieval?.resources ?? [];
  const templates = plugin.templates?.groups.flatMap((group) =>
    group.templates.map((template) => ({
      ...template,
      groupTitle: group.title,
    })),
  ) ?? [];
  const environmentsByRef = pluginEnvironmentByRef(plugin.environments ?? []);
  const operationCount = operatorPluginOperationCount(operators);
  const exposedOperationCount = plugin.enabled ? operatorPluginExposedOperationCount(operators) : 0;
  const totalUnits = operators.length + templates.length + retrievalResources.length;
  if (totalUnits === 0) return null;

  const disabled = busy || !plugin.installed || !plugin.enabled;
  return (
    <Stack spacing={1.25}>
      <Stack direction="row" gap={0.75} alignItems="center" flexWrap="wrap">
        <Typography variant="subtitle1" fontWeight={850}>
          Included units
        </Typography>
        {operators.length > 0 && (
          <Chip
            size="small"
            color={pluginUnitKindColor("operator")}
            variant="outlined"
            sx={pluginUnitKindChipSx("operator")}
            label={`${operators.filter((operator) => operator.exposed).length}/${operators.length} Operator program${operators.length === 1 ? "" : "s"}`}
          />
        )}
        {operators.length > 0 && (
          <Chip
            size="small"
            color={pluginUnitKindColor("operator")}
            variant="outlined"
            sx={pluginUnitKindChipSx("operator")}
            label={`${exposedOperationCount}/${operationCount} operations exposed`}
          />
        )}
        {templates.length > 0 && (
          <Chip
            size="small"
            color={pluginUnitKindColor("template")}
            variant="outlined"
            sx={pluginUnitKindChipSx("template")}
            label={`${templates.filter(templateEnabled).length}/${templates.length} templates on`}
          />
        )}
        {retrievalResources.length > 0 && (
          <Chip
            size="small"
            variant="outlined"
            label={`${retrievalResources.filter(retrievalResourceEnabled).length}/${retrievalResources.length} routes on`}
          />
        )}
      </Stack>
      {operators.length > 0 && (
        <Typography variant="caption" color="text.secondary" sx={{ lineHeight: 1.45 }}>
          Operator programs are exposed automatically while this plugin is enabled. Use the plugin switch to turn the whole bundle on or off; no per-tool registration is required.{" "}
          Choose subcommands as <code>operator_execute.operation</code>; operation categories come from the plugin manifest.
        </Typography>
      )}
      <Paper variant="outlined" sx={{ borderRadius: 2.5, overflow: "hidden" }}>
        <Stack divider={<Box sx={{ height: 1, bgcolor: "divider" }} />}>
          {retrievalResources.map((resource) => {
            const checked = retrievalResourceEnabled(resource);
            const resourceName = retrievalResourceDisplayName(resource);
            return (
              <Stack
                key={`retrieval:${resource.category}:${resource.id}`}
                direction="row"
                spacing={1.25}
                alignItems="center"
                sx={{ p: 1.15 }}
              >
                <Box
                  sx={{
                    width: 32,
                    height: 32,
                    borderRadius: 1.75,
                    display: "inline-flex",
                    alignItems: "center",
                    justifyContent: "center",
                    bgcolor: "action.hover",
                    color: "text.secondary",
                    flexShrink: 0,
                  }}
                >
                  <SearchRounded fontSize="small" />
                </Box>
                <Box sx={{ flex: 1, minWidth: 0 }}>
                  <Stack direction="row" gap={0.75} alignItems="center" flexWrap="wrap">
                    <Typography variant="subtitle2" fontWeight={800}>
                      {resourceName}
                    </Typography>
                    <Chip size="small" variant="outlined" label="Route" />
                    <Chip size="small" variant="outlined" label={capabilityLabel(resource.category)} />
                  </Stack>
                  <Typography variant="body2" color="text.secondary" sx={{ mt: 0.25, lineHeight: 1.45 }}>
                    {resource.description?.trim() || `Retrieval route ID: ${resource.category}.${resource.id}`}
                  </Typography>
                </Box>
                <Switch
                  size="small"
                  checked={checked}
                  disabled={disabled}
                  onChange={(event) =>
                    onRetrievalResourceToggle(
                      plugin,
                      resource.category,
                      resource.id,
                      event.target.checked,
                    )
                  }
                  inputProps={{ "aria-label": `${checked ? "Disable" : "Enable"} ${resourceName} route` }}
                />
              </Stack>
            );
          })}
          {operators.map((operator) => {
            const exposed = plugin.enabled && operator.exposed;
            const alias = operatorPrimaryAlias(operator);
            const envRef = operatorEnvironmentRef(operator);
            const environment = envRef ? environmentsByRef.get(envRef) : null;
            const operationGroups = operatorOperationGroups(operator);
            const opCount = operatorOperationCount(operator);
            return (
              <Stack
                key={`operator:${operator.sourcePlugin}:${operator.id}:${operator.version}`}
                direction="row"
                spacing={1.25}
                alignItems="center"
                sx={{ p: 1.15 }}
              >
                <Box
                  sx={{
                    width: 32,
                    height: 32,
                    borderRadius: 1.75,
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
                <Box sx={{ flex: 1, minWidth: 0 }}>
                  <Stack direction="row" gap={0.75} alignItems="center" flexWrap="wrap">
                    <Typography variant="subtitle2" fontWeight={800}>
                      {operatorDisplayName(operator)}
                    </Typography>
                    <Chip
                      size="small"
                      color={pluginUnitKindColor("operator")}
                      variant="outlined"
                      sx={pluginUnitKindChipSx("operator")}
                      label="Operator program"
                    />
                    <Chip size="small" variant="outlined" label={`${opCount} operation${opCount === 1 ? "" : "s"}`} />
                    {operatorResourceProfileChip(operator)}
                    {envRef && (
                      <Chip
                        size="small"
                        variant="outlined"
                        color={environment ? pluginEnvironmentStatusColor(environment.availabilityStatus) : "default"}
                        label={`env: ${environment ? pluginEnvironmentDisplayName(environment) : envRef}`}
                      />
                    )}
                    <Chip
                      size="small"
                      color={exposed ? "success" : "default"}
                      variant={exposed ? "filled" : "outlined"}
                      label={
                        exposed
                          ? "Exposed by plugin"
                          : plugin.enabled
                            ? "Not exposed"
                            : "Plugin disabled"
                      }
                    />
                    {exposed && <Chip size="small" color="success" variant="outlined" label={operatorToolName(alias)} />}
                  </Stack>
                  <Typography variant="body2" color="text.secondary" sx={{ mt: 0.25, lineHeight: 1.45 }}>
                    {operator.description?.trim() || `Atomic operator ID: ${operator.id}`}
                  </Typography>
                  {operationGroups.length > 0 && (
                    <Stack spacing={0.5} sx={{ mt: 0.75 }}>
                      {operationGroups.map((group) => (
                        <Box key={`${operator.id}:${group.key}`}>
                          <Typography variant="caption" color="text.secondary" fontWeight={800}>
                            {group.label}
                          </Typography>
                          <Stack direction="row" gap={0.5} flexWrap="wrap" sx={{ mt: 0.35 }}>
                            {group.operations.map((operation) => (
                              <Chip
                                key={`${operator.id}:${operation.id}`}
                                size="small"
                                variant="outlined"
                                label={operatorOperationDisplayName(operation)}
                              />
                            ))}
                          </Stack>
                        </Box>
                      ))}
                    </Stack>
                  )}
                </Box>
              </Stack>
            );
          })}
          {templates.map((template) => {
            const checked = templateEnabled(template);
            return (
              <Stack
                key={`template:${template.id}`}
                direction="row"
                spacing={1.25}
                alignItems="center"
                sx={{ p: 1.15 }}
              >
                <Box
                  sx={{
                    width: 32,
                    height: 32,
                    borderRadius: 1.75,
                    display: "inline-flex",
                    alignItems: "center",
                    justifyContent: "center",
                    bgcolor: "action.hover",
                    color: "text.secondary",
                    flexShrink: 0,
                  }}
                >
                  <DescriptionOutlined fontSize="small" />
                </Box>
                <Box sx={{ flex: 1, minWidth: 0 }}>
                  <Stack direction="row" gap={0.75} alignItems="center" flexWrap="wrap">
                    <Typography variant="subtitle2" fontWeight={800}>
                      {templateDisplayName(template)}
                    </Typography>
                    <Chip
                      size="small"
                      color={pluginUnitKindColor("template")}
                      variant="outlined"
                      sx={pluginUnitKindChipSx("template")}
                      label="Template"
                    />
                    <Chip size="small" variant="outlined" label={template.groupTitle} />
                  </Stack>
                  <Typography variant="body2" color="text.secondary" sx={{ mt: 0.25, lineHeight: 1.45 }}>
                    {template.description?.trim() || `Template ID: ${template.id}`}
                  </Typography>
                </Box>
                <Switch
                  size="small"
                  checked={checked}
                  disabled={disabled}
                  onChange={(event) => onTemplateToggle(plugin, template.id, event.target.checked)}
                  inputProps={{ "aria-label": `${checked ? "Disable" : "Enable"} ${templateDisplayName(template)} template` }}
                />
              </Stack>
            );
          })}
        </Stack>
      </Paper>
      {disabled && (
        <Typography variant="caption" color="text.secondary">
          Install and enable the plugin to change individual units.
        </Typography>
      )}
    </Stack>
  );
}

function PluginEnvironmentOverview({
  plugin,
  environments,
  busy,
  checkStates,
  onConfigure,
  onTest,
  onEnvironmentToggle,
}: {
  plugin: PluginSummary;
  environments?: PluginEnvironmentSummary[];
  busy: boolean;
  checkStates: Record<string, PluginEnvironmentCheckState | undefined>;
  onConfigure: (plugin: PluginSummary, environment: PluginEnvironmentSummary) => void;
  onTest: (plugin: PluginSummary, environment: PluginEnvironmentSummary) => void;
  onEnvironmentToggle: (plugin: PluginSummary, environment: PluginEnvironmentSummary, enabled: boolean) => void;
}) {
  const theme = useTheme();
  const visible = (environments ?? []).filter((environment) => environment.id.trim().length > 0);
  if (visible.length === 0) return null;
  const availableCount = visible.filter(
    (environment) => pluginEnvironmentStatusColor(environment.availabilityStatus) === "success",
  ).length;
  return (
    <Stack spacing={1.25}>
      <Stack direction="row" gap={0.75} alignItems="center" flexWrap="wrap">
        <Typography variant="subtitle1" fontWeight={850}>
          Runtime environments
        </Typography>
        <Chip size="small" variant="outlined" label={`${visible.length} profiles`} />
        <Chip
          size="small"
          color={availableCount === visible.length ? "success" : availableCount > 0 ? "warning" : "default"}
          variant="outlined"
          label={`${availableCount}/${visible.length} detected`}
        />
      </Stack>
      <Paper variant="outlined" sx={{ borderRadius: 2.5, overflow: "hidden" }}>
        <Stack divider={<Box sx={{ height: 1, bgcolor: "divider" }} />}>
          {visible.map((environment) => {
            const color = pluginEnvironmentStatusColor(environment.availabilityStatus);
            const runtimeKey = pluginEnvironmentRuntimeKey(environment);
            const checkState = checkStates[pluginEnvironmentKey(environment)];
            const checkColor = checkState?.result
              ? pluginEnvironmentStatusColor(checkState.result.status)
              : checkState?.error
                ? "error"
                : null;
            const configureDisabled = busy || !plugin.installed || !plugin.installedPath;
            const environmentEnabled = environment.exposed !== false;
            const switchDisabled = busy || !plugin.installed || !plugin.enabled;
            const configureEnvLabel = "Configure env";
            const testEnvLabel = "Test env";
            return (
              <Stack
                key={`${environment.canonicalId}:${environment.manifestPath}`}
                direction="row"
                spacing={1.25}
                alignItems="flex-start"
                sx={{ p: 1.15 }}
              >
                <Box
                  sx={{
                    width: 32,
                    height: 32,
                    borderRadius: 1.75,
                    display: "inline-flex",
                    alignItems: "center",
                    justifyContent: "center",
                    bgcolor: alpha(theme.palette.text.primary, theme.palette.mode === "dark" ? 0.12 : 0.06),
                    color: "text.secondary",
                    flexShrink: 0,
                  }}
                >
                  <TroubleshootRounded fontSize="small" />
                </Box>
                <Box sx={{ flex: 1, minWidth: 0 }}>
                  <Stack direction="row" gap={0.75} alignItems="center" flexWrap="wrap">
                    <Typography variant="subtitle2" fontWeight={800}>
                      {pluginEnvironmentDisplayName(environment)}
                    </Typography>
                    <Chip size="small" variant="outlined" label={runtimeKey} />
                    <Chip
                      size="small"
                      color={color}
                      variant={color === "success" ? "filled" : "outlined"}
                      label={environment.availabilityManager || environment.availabilityStatus}
                    />
                    <Chip size="small" variant="outlined" label={pluginEnvironmentRuntimeFileLabel(environment)} />
                  </Stack>
                  <Typography variant="body2" color="text.secondary" sx={{ mt: 0.35, lineHeight: 1.45 }}>
                    {environment.description?.trim() || `Environment profile ID: ${environment.id}`}
                  </Typography>
                  <Typography variant="caption" color="text.secondary" sx={{ display: "block", mt: 0.5, wordBreak: "break-word" }}>
                    {environment.availabilityMessage}
                  </Typography>
                  {environment.installHint?.trim() && color !== "success" && (
                    <Typography variant="caption" color="text.secondary" sx={{ display: "block", mt: 0.35, wordBreak: "break-word" }}>
                      Install hint: {environment.installHint}
                    </Typography>
                  )}
                  {environment.checkCommand.length > 0 && (
                    <Typography
                      variant="caption"
                      color="text.secondary"
                      sx={{
                        display: "block",
                        mt: 0.35,
                        fontFamily: "ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace",
                        wordBreak: "break-word",
                      }}
                    >
                      Check: {environment.checkCommand.join(" ")}
                    </Typography>
                  )}
                  {checkState?.result && (
                    <Typography variant="caption" color="text.secondary" sx={{ display: "block", mt: 0.35, wordBreak: "break-word" }}>
                      Last test: {checkState.result.status}
                      {typeof checkState.result.exitCode === "number" ? ` · exit ${checkState.result.exitCode}` : ""}
                      {checkState.result.error ? ` · ${checkState.result.error}` : ""}
                    </Typography>
                  )}
                  {checkState?.error && (
                    <Typography variant="caption" color="error" sx={{ display: "block", mt: 0.35, wordBreak: "break-word" }}>
                      Test failed: {checkState.error}
                    </Typography>
                  )}
                </Box>
                <Stack direction="row" spacing={0.5} alignItems="center" sx={{ flexShrink: 0 }}>
                  {checkColor && (
                    <Chip
                      size="small"
                      color={checkColor}
                      variant={checkColor === "success" ? "filled" : "outlined"}
                      label={checkState?.result?.status || "error"}
                    />
                  )}
                  <Tooltip
                    title={
                      configureDisabled
                        ? "Install the plugin first; environment edits are only allowed in the user plugin copy."
                        : "Reveal the user-copy environment file for editing."
                    }
                  >
                    <span>
                      <IconButton
                        size="small"
                        disabled={configureDisabled}
                        onClick={() => onConfigure(plugin, environment)}
                        aria-label={`${configureEnvLabel}: ${pluginEnvironmentDisplayName(environment)}`}
                        sx={{ border: 1, borderColor: "divider", borderRadius: 1.5 }}
                      >
                        <SettingsRounded fontSize="small" />
                      </IconButton>
                    </span>
                  </Tooltip>
                  <Tooltip title="Prepare/reuse the isolated runtime when needed, then run the profile check command.">
                    <span>
                      <IconButton
                        size="small"
                        disabled={busy || checkState?.loading}
                        onClick={() => onTest(plugin, environment)}
                        aria-label={`${testEnvLabel}: ${pluginEnvironmentDisplayName(environment)}`}
                        sx={{ border: 1, borderColor: "divider", borderRadius: 1.5 }}
                      >
                        {checkState?.loading ? <CircularProgress size={16} /> : <TroubleshootRounded fontSize="small" />}
                      </IconButton>
                    </span>
                  </Tooltip>
                  <Switch
                    size="small"
                    checked={environmentEnabled}
                    disabled={switchDisabled}
                    onChange={(event) => onEnvironmentToggle(plugin, environment, event.target.checked)}
                    inputProps={{
                      "aria-label": `${environmentEnabled ? "Disable" : "Enable"} ${pluginEnvironmentDisplayName(environment)} environment`,
                    }}
                  />
                </Stack>
              </Stack>
            );
          })}
        </Stack>
      </Paper>
    </Stack>
  );
}

function PluginDetailsDialog({
  plugin,
  open,
  retrievalStatuses = [],
  processPoolStatuses = [],
  operators = [],
  remoteUpdateAvailable = false,
  busy,
  onClose,
  onInstall,
  onUninstall,
  onSync,
  onForceSync,
  onToggle,
  onTemplateToggle,
  onRetrievalResourceToggle,
  onConfigureEnvironment,
  onTestEnvironment,
  onEnvironmentToggle,
  environmentCheckStates,
  onCopyDiagnostics,
  projectPath,
}: {
  plugin: PluginSummary | null;
  open: boolean;
  projectPath: string;
  retrievalStatuses?: PluginRetrievalRouteStatus[];
  processPoolStatuses?: PluginProcessPoolRouteStatus[];
  operators?: OperatorSummary[];
  remoteUpdateAvailable?: boolean;
  busy: boolean;
  onClose: () => void;
  onInstall: (plugin: PluginSummary) => void;
  onUninstall: (plugin: PluginSummary) => void;
  onSync: (plugin: PluginSummary) => void;
  onForceSync: (plugin: PluginSummary) => void;
  onToggle: (plugin: PluginSummary, enabled: boolean) => void;
  onTemplateToggle: (plugin: PluginSummary, templateId: string, enabled: boolean) => void;
  onRetrievalResourceToggle: (
    plugin: PluginSummary,
    category: string,
    resourceId: string,
    enabled: boolean,
  ) => void;
  onConfigureEnvironment: (plugin: PluginSummary, environment: PluginEnvironmentSummary) => void;
  onTestEnvironment: (plugin: PluginSummary, environment: PluginEnvironmentSummary) => void;
  onEnvironmentToggle: (
    plugin: PluginSummary,
    environment: PluginEnvironmentSummary,
    enabled: boolean,
  ) => void;
  environmentCheckStates: Record<string, PluginEnvironmentCheckState | undefined>;
  onCopyDiagnostics: (
    plugin: PluginSummary,
    retrievalStatuses: PluginRetrievalRouteStatus[],
    processPoolStatuses: PluginProcessPoolRouteStatus[],
  ) => void;
}) {
  const theme = useTheme();
  if (!plugin) return null;

  const chips = capabilityChips(plugin).slice(0, 2);
  const declaredRetrievalResources = plugin.retrieval?.resources ?? [];
  const installable = plugin.installPolicy !== "NOT_AVAILABLE";
  const isNotebookHelper = isNotebookPlugin(plugin);
  const isComputerUse = isComputerUsePlugin(plugin);
  const hasVisualizationCompletionOverview = isRVisualizationPlugin(plugin);
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
    runtimeSummary.issueCount > 0 || processPoolStatuses.length > 0 || Boolean(runtimeSummary.lastError);
  const showRuntimeSummaryCard = shouldShowPluginRuntimeSummaryCard(
    plugin,
    runtimeSummary,
    declaredRetrievalResources,
    processPoolStatuses,
  );
  const action = plugin.installed ? (
    <Stack direction="row" spacing={0.75} alignItems="center" flexWrap="wrap" justifyContent="flex-end">
      {pluginSyncNeedsAction(plugin) && (
        <Tooltip title={pluginSyncTooltip(plugin)} arrow>
          <span>
            <IconButton
              aria-label={`${pluginSyncButtonLabel(plugin)} ${displayName(plugin)}`}
              size="small"
              color={plugin.sync?.state === "conflictRisk" ? "warning" : "primary"}
              disabled={busy}
              onClick={() => onSync(plugin)}
              sx={{
                border: 1,
                borderColor: "divider",
                bgcolor: "background.paper",
                "&:hover": { bgcolor: "action.hover" },
              }}
            >
              {busy ? <CircularProgress size={18} color="inherit" /> : <SyncRounded fontSize="small" />}
            </IconButton>
          </span>
        </Tooltip>
      )}
      <Tooltip title={`Uninstall ${displayName(plugin)}`} arrow>
        <span>
          <IconButton
            aria-label={`Uninstall ${displayName(plugin)}`}
            size="small"
            color="error"
            disabled={busy}
            onClick={() => onUninstall(plugin)}
          >
            <DeleteOutlineRounded fontSize="small" />
          </IconButton>
        </span>
      </Tooltip>
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
    <Dialog
      open={open}
      onClose={onClose}
      fullWidth
      maxWidth="md"
      scroll="paper"
      aria-labelledby="plugin-details-title"
      sx={pluginDetailsDialogSx}
    >
      <DialogTitle id="plugin-details-title" sx={{ px: 3, py: 2, pr: plugin.installed ? 25 : 7 }}>
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
        {plugin.installed && (
          <Stack
            direction="row"
            spacing={0.75}
            alignItems="center"
            sx={{
              position: "absolute",
              right: 54,
              top: 9,
              minHeight: 38,
              px: 1,
              borderRadius: 2,
              bgcolor: alpha(
                plugin.enabled ? theme.palette.success.main : theme.palette.text.primary,
                theme.palette.mode === "dark" ? 0.14 : 0.07,
              ),
              border: 1,
              borderColor: plugin.enabled ? alpha(theme.palette.success.main, 0.32) : "divider",
            }}
          >
            <Typography variant="caption" color={plugin.enabled ? "success.main" : "text.secondary"} fontWeight={850}>
              {plugin.enabled ? "Enabled" : "Disabled"}
            </Typography>
            <Switch
              size="small"
              checked={plugin.enabled}
              disabled={busy}
              onChange={(event) => onToggle(plugin, event.target.checked)}
              inputProps={{ "aria-label": `${plugin.enabled ? "Disable" : "Enable"} ${displayName(plugin)}` }}
            />
          </Stack>
        )}
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
              <Stack direction="row" gap={0.9} alignItems="center" flexWrap="wrap">
                <Typography variant="h5" fontWeight={850} sx={{ lineHeight: 1.15 }}>
                  {displayName(plugin)}
                </Typography>
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
              </Stack>
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
                {pluginSyncNeedsAction(plugin) && plugin.sync && (
                  <Chip
                    size="small"
                    color={pluginSyncChipColor(plugin.sync.state)}
                    variant={plugin.sync.state === "conflictRisk" ? "filled" : "outlined"}
                    label={plugin.sync.label}
                    title={plugin.sync.message}
                  />
                )}
                {remoteUpdateAvailable && (
                  <Chip
                    size="small"
                    color="warning"
                    variant="outlined"
                    label="Remote update"
                    title="Remote marketplace has an update for this plugin"
                  />
                )}
                {declaredRetrievalResources.length > 0 && (
                  <Chip
                    size="small"
                    variant="outlined"
                    label={`${declaredRetrievalResources.length} route${declaredRetrievalResources.length === 1 ? "" : "s"}`}
                  />
                )}
                {(plugin.environments?.length ?? 0) > 0 && (
                  <Chip
                    size="small"
                    variant="outlined"
                    label={`${plugin.environments?.length ?? 0} env${plugin.environments?.length === 1 ? "" : "s"}`}
                  />
                )}
                {pluginResourceProfileChip(operators)}
                {chips.map((chip) => (
                  <Chip key={chip} size="small" variant="outlined" label={capabilityLabel(chip)} />
                ))}
              </Stack>
            </Box>

            <Box sx={{ flexShrink: 0, alignSelf: { xs: "flex-start", md: "center" } }}>
              {action}
            </Box>
          </Stack>

          {pluginSyncNeedsAction(plugin) && plugin.sync && (
            <Alert
              severity={plugin.sync.state === "conflictRisk" ? "warning" : "info"}
              sx={{ borderRadius: 2 }}
              action={
                <Stack direction="row" spacing={0.5} alignItems="center">
                  <Tooltip title={pluginSyncTooltip(plugin)} arrow>
                    <span>
                      <IconButton
                        aria-label={`${pluginSyncButtonLabel(plugin)} ${displayName(plugin)}`}
                        color="inherit"
                        size="small"
                        disabled={busy}
                        onClick={() => onSync(plugin)}
                      >
                        {busy ? <CircularProgress size={18} color="inherit" /> : <SyncRounded fontSize="small" />}
                      </IconButton>
                    </span>
                  </Tooltip>
                  {pluginCanForceSync(plugin) && (
                    <Tooltip title="Force overwrite local plugin edits from marketplace" arrow>
                      <span>
                        <IconButton
                          aria-label={`Force overwrite ${displayName(plugin)} from marketplace`}
                          color="warning"
                          size="small"
                          disabled={busy}
                          onClick={() => onForceSync(plugin)}
                        >
                          <PublishedWithChangesRounded fontSize="small" />
                        </IconButton>
                      </span>
                    </Tooltip>
                  )}
                </Stack>
              }
            >
              <Typography variant="body2" fontWeight={800}>
                {plugin.sync.label}
              </Typography>
              <Typography variant="body2">
                {plugin.sync.message} {plugin.sync.changedCount} upstream change{plugin.sync.changedCount === 1 ? "" : "s"},
                {" "}{plugin.sync.localModifiedCount} local edit{plugin.sync.localModifiedCount === 1 ? "" : "s"},
                {" "}{plugin.sync.conflictCount} conflict{plugin.sync.conflictCount === 1 ? "" : "s"}.
              </Typography>
            </Alert>
          )}

          {plugin.changelog?.entries?.length ? (
            <Accordion disableGutters elevation={0} sx={accordionSx}>
              <AccordionSummary expandIcon={<ExpandMoreRounded />} sx={accordionSummarySx}>
                <Stack direction="row" gap={1} alignItems="center" flexWrap="wrap">
                  <Typography variant="subtitle2" fontWeight={800}>
                    Changelog
                  </Typography>
                  {plugin.changelog.latestVersion && (
                    <Chip
                      size="small"
                      variant="outlined"
                      label={`v${plugin.changelog.latestVersion}`}
                    />
                  )}
                  <Chip
                    size="small"
                    variant="outlined"
                    label={`${plugin.changelog.entries.length} entr${plugin.changelog.entries.length === 1 ? "y" : "ies"}`}
                  />
                </Stack>
              </AccordionSummary>
              <AccordionDetails sx={{ px: 2, pt: 0.5, pb: 2 }}>
                <Stack spacing={1.25}>
                  {plugin.changelog.entries.slice(0, 5).map((entry, index) => (
                    <Paper
                      key={`${entry.title}:${index}`}
                      variant="outlined"
                      sx={{ p: 1.25, borderRadius: 2, bgcolor: "action.hover" }}
                    >
                      <Stack spacing={0.5}>
                        <Stack direction="row" gap={0.75} alignItems="center" flexWrap="wrap">
                          <Typography variant="body2" fontWeight={850}>
                            {entry.title}
                          </Typography>
                          {entry.version && <Chip size="small" variant="outlined" label={`v${entry.version}`} />}
                          {entry.date && <Chip size="small" variant="outlined" label={entry.date} />}
                        </Stack>
                        {entry.body && (
                          <Typography
                            variant="caption"
                            color="text.secondary"
                            sx={{ whiteSpace: "pre-line" }}
                          >
                            {entry.body}
                          </Typography>
                        )}
                      </Stack>
                    </Paper>
                  ))}
                </Stack>
              </AccordionDetails>
            </Accordion>
          ) : null}

          {showRuntimeSummaryCard && (
            <Paper
              variant="outlined"
              sx={{
                p: 1.5,
                borderRadius: 2.5,
                bgcolor: alpha(theme.palette.background.default, theme.palette.mode === "dark" ? 0.42 : 0.72),
              }}
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
                {declaredRetrievalResources.length > 0 && (
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
          )}

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

          <PluginUnitControls
            plugin={plugin}
            operators={operators}
            busy={busy}
            onTemplateToggle={onTemplateToggle}
            onRetrievalResourceToggle={onRetrievalResourceToggle}
          />

          <PluginEnvironmentOverview
            plugin={plugin}
            environments={plugin.environments}
            busy={busy}
            checkStates={environmentCheckStates}
            onConfigure={onConfigureEnvironment}
            onTest={onTestEnvironment}
            onEnvironmentToggle={onEnvironmentToggle}
          />

          {isComputerUse && (
            <Stack spacing={1.25}>
              <Typography variant="subtitle1" fontWeight={850}>
                Safety, permissions, and audit
              </Typography>
              <ComputerUseSettingsPanel
                projectPath={projectPath}
                showIntro={false}
                showPluginButton={false}
              />
            </Stack>
          )}

          {hasVisualizationCompletionOverview ? (
            <VisualizationRDetailOverview templates={plugin.templates} />
          ) : (
            <Stack spacing={1.25}>
              <Typography variant="subtitle1" fontWeight={850}>
                Capabilities
              </Typography>
              <Paper variant="outlined" sx={{ borderRadius: 2.5, overflow: "hidden" }}>
                <Stack divider={<Box sx={{ height: 1, bgcolor: "divider" }} />}>
                  {pluginContentOverview(plugin, operators).map((item) => (
                    <Stack key={item.id} direction="row" spacing={1.25} alignItems="center" sx={{ p: 1.25 }}>
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
                      <Box sx={{ flex: 1, minWidth: 0 }}>
                        <Stack direction="row" gap={0.75} alignItems="center" flexWrap="wrap">
                          <Typography variant="subtitle2" fontWeight={800}>
                            {item.title}
                          </Typography>
                          <Chip size="small" variant="outlined" label={item.meta} />
                        </Stack>
                        <Typography variant="body2" color="text.secondary" sx={{ mt: 0.25, lineHeight: 1.45 }}>
                          {item.detail}
                        </Typography>
                      </Box>
                    </Stack>
                  ))}
                </Stack>
              </Paper>
            </Stack>
          )}

          <Box sx={pluginDetailsTechnicalSectionSx}>
            <Accordion disableGutters elevation={0} sx={accordionSx}>
              <AccordionSummary expandIcon={<ExpandMoreRounded />} sx={compactAccordionSummarySx}>
                <Stack direction="row" gap={0.75} alignItems="center" flexWrap="wrap">
                  <Typography variant="subtitle1" fontWeight={850}>
                    Developer & troubleshooting
                  </Typography>
                  <Chip size="small" variant="outlined" label="paths" />
                </Stack>
              </AccordionSummary>
              <AccordionDetails sx={{ px: 1.5, pt: 0, pb: 1.5 }}>
                <Stack spacing={1.15}>
                  <Typography variant="body2" color="text.secondary">
                    Source code, manifests, schemas, and examples are not previewed here. Developers can inspect the plugin path directly when needed.
                  </Typography>
                  <Paper variant="outlined" sx={{ p: 1.15, borderRadius: 2, bgcolor: "background.paper" }}>
                    <Stack spacing={0.65}>
                      <OperatorMetaLine label="Plugin ID" value={plugin.id} />
                      <OperatorMetaLine label="Source path" value={plugin.sourcePath || "not available"} />
                      {plugin.installedPath?.trim() && (
                        <OperatorMetaLine label="Installed" value={plugin.installedPath} />
                      )}
                      <OperatorMetaLine label="Marketplace" value={plugin.marketplaceName} />
                    </Stack>
                  </Paper>
                  <Button
                    size="small"
                    variant="outlined"
                    startIcon={<ContentCopyRounded />}
                    disabled={busy}
                    onClick={() => onCopyDiagnostics(plugin, retrievalStatuses, processPoolStatuses)}
                    sx={{ textTransform: "none", borderRadius: 1.5, alignSelf: "flex-start" }}
                  >
                    Copy diagnostics
                  </Button>
                </Stack>
              </AccordionDetails>
            </Accordion>

            {hasRuntimeDetails && (
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
                              key={`${status.category}:${status.resourceId}`}
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
                  {declaredRetrievalResources.length > 0 && retrievalStatuses.length === 0 && (
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
                          <Box key={`${status.category}:${status.resourceId}:${status.pluginRoot}`} sx={{ p: 1, borderRadius: 1.5, bgcolor: alpha(theme.palette.info.main, theme.palette.mode === "dark" ? 0.12 : 0.05) }}>
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

          </Box>

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
  const slurmDiagnostic = operatorRunSlurmDiagnostic(run, detail);
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
          aria-label="Close tool run details"
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
                Reused a previous tool result
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
                {slurmDiagnostic && (
                  <Box sx={{ p: 0.75, borderRadius: 1, bgcolor: "background.paper", border: 1, borderColor: "divider" }}>
                    <Typography variant="caption" fontWeight={850} sx={{ display: "block", wordBreak: "break-word" }}>
                      SLURM diagnostic: {formatSlurmDiagnosticSummary(slurmDiagnostic)}
                    </Typography>
                    <Typography variant="caption" color="text.secondary" sx={{ display: "block", wordBreak: "break-word" }}>
                      {slurmDiagnostic.suggestedAction ? `Suggested action: ${slurmDiagnostic.suggestedAction}` : "No automatic re-run suggestion."}
                    </Typography>
                  </Box>
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

export function PluginsPanel({ projectPath }: { projectPath: string }) {
  const {
    marketplaceSourceViews,
    marketplaces,
    operators,
    operatorRuns,
    retrievalStatuses,
    processPoolStatuses,
    remoteMarketplaceChecks,
    builtinMarketplaceStatus,
    isLoading,
    isMutating,
    bootstrapInProgress,
    error,
    ensureBuiltinMarketplace,
    loadPlugins,
    loadOperatorRuns,
    readOperatorRun,
    readOperatorRunLog,
    verifyOperatorRun,
    clearProcessPool,
    migratePluginState,
    installPlugin,
    syncPlugin,
    checkRemoteMarketplaces,
    addMarketplaceSource,
    removeMarketplaceSource,
    setMarketplaceSourceEnabled,
    refreshMarketplaceSource,
    uninstallPlugin,
    setPluginEnabled,
    setTemplateEnabled,
    setRetrievalResourceEnabled,
    setEnvironmentEnabled,
    checkPluginEnvironment,
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
  const [marketplaceSourceKind, setMarketplaceSourceKind] = useState<MarketplaceSourceKind>("local");
  const [marketplaceSourceLocation, setMarketplaceSourceLocation] = useState("");
  const [marketplaceSourceLabelInput, setMarketplaceSourceLabelInput] = useState("");
  const [marketplaceSourceFormError, setMarketplaceSourceFormError] = useState<string | null>(null);
  const [marketplaceSourceActionKey, setMarketplaceSourceActionKey] = useState<string | null>(null);
  const [marketplaceSourceRefreshResults, setMarketplaceSourceRefreshResults] = useState<
    Record<string, RefreshResult | undefined>
  >({});
  const [pluginMigrationResult, setPluginMigrationResult] = useState<PluginMigrationResult | null>(null);
  const [pluginMigrationRunning, setPluginMigrationRunning] = useState(false);
  const [dismissedFeedbackKey, setDismissedFeedbackKey] = useState<string | null>(null);
  const [installingPluginId, setInstallingPluginId] = useState<string | null>(null);
  const [checkingRemoteMarketplaces, setCheckingRemoteMarketplaces] = useState(false);
  const [environmentCheckStates, setEnvironmentCheckStates] = useState<Record<string, PluginEnvironmentCheckState | undefined>>({});
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
  const remoteMarketplaceCount = useMemo(
    () => marketplaces.filter((marketplace) => marketplace.remote?.url?.trim()).length,
    [marketplaces],
  );
  const remoteChangedPluginNames = useMemo(
    () => remoteMarketplaceChangedPluginNames(remoteMarketplaceChecks),
    [remoteMarketplaceChecks],
  );
  const remoteChangedPluginCount = remoteChangedPluginNames.size;
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
    ? operatorsByPlugin.get(detailPlugin.id)
      ?? operatorsByPlugin.get(detailPlugin.name)
      ?? detailPlugin.operators
      ?? []
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
  const marketplaceSourceMutationDisabled = isMutating || isLoading || bootstrapInProgress;

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
      await loadPlugins(projectRoot, operatorSurface);
      setMessage(`Installed ${displayName(plugin)}`);
    } catch {
      // Store exposes the error banner.
    } finally {
      setInstallingPluginId((current) => (current === plugin.id ? null : current));
    }
  };

  const handleSyncPlugin = async (plugin: PluginSummary) => {
    setMessage(null);
    setInstallingPluginId(plugin.id);
    try {
      const result = await syncPlugin(plugin, projectRoot);
      await loadPlugins(projectRoot, operatorSurface);
      const changed = result.updated.length + result.added.length + result.removed.length;
      setMessage(
        result.conflicts.length > 0
          ? `Synced ${displayName(plugin)} with ${result.conflicts.length} conflict${result.conflicts.length === 1 ? "" : "s"} kept local`
          : changed > 0
            ? `Synced ${displayName(plugin)} (${changed} file${changed === 1 ? "" : "s"})`
            : `${displayName(plugin)} is up to date`,
      );
    } catch {
      // Store exposes the error banner.
    } finally {
      setInstallingPluginId((current) => (current === plugin.id ? null : current));
    }
  };

  const handleForceSyncPlugin = async (plugin: PluginSummary) => {
    if (!window.confirm(`Force overwrite ${displayName(plugin)} from the marketplace source? Local plugin edits will be replaced.`)) return;
    setMessage(null);
    setInstallingPluginId(plugin.id);
    try {
      const result = await syncPlugin(plugin, projectRoot, { force: true });
      await loadPlugins(projectRoot, operatorSurface);
      const changed = result.updated.length + result.added.length + result.removed.length;
      setMessage(
        `Force synced ${displayName(plugin)}${changed > 0 ? ` (${changed} file${changed === 1 ? "" : "s"})` : ""}`,
      );
    } catch {
      // Store exposes the error banner.
    } finally {
      setInstallingPluginId((current) => (current === plugin.id ? null : current));
    }
  };

  const handleCheckRemoteMarketplaces = async () => {
    setMessage(null);
    setCheckingRemoteMarketplaces(true);
    const previousSignature = remoteMarketplaceCheckSignature(remoteMarketplaceChecks);
    try {
      const results = await checkRemoteMarketplaces(projectRoot);
      const nextSignature = remoteMarketplaceCheckSignature(results);
      if (remoteMarketplaceChecks.length === 0 || nextSignature !== previousSignature) {
        setMessage(remoteMarketplaceCheckMessage(results));
      }
    } catch {
      // Store exposes the error banner.
    } finally {
      setCheckingRemoteMarketplaces(false);
    }
  };

  const handleAddMarketplaceSource = async () => {
    setMessage(null);
    setMarketplaceSourceFormError(null);
    const location = marketplaceSourceLocation.trim();
    const label = marketplaceSourceLabelInput.trim();
    if (!location) {
      setMarketplaceSourceFormError(
        marketplaceSourceKind === "local"
          ? "Enter a local marketplace path."
          : "Enter an HTTPS Git URL.",
      );
      return;
    }
    setMarketplaceSourceActionKey("add");
    try {
      const source = await addMarketplaceSource(
        marketplaceSourceKind,
        location,
        label || undefined,
        projectRoot,
      );
      setMarketplaceSourceLocation("");
      setMarketplaceSourceLabelInput("");
      if (source.kind === "remote") {
        const result = await refreshMarketplaceSource(source.id, projectRoot);
        setMarketplaceSourceRefreshResults((current) => ({
          ...current,
          [source.id]: { ...result },
        }));
        if (result.ok) {
          setMessage(marketplaceSourceRefreshMessage(result));
        }
      } else {
        setMessage("Added local marketplace source");
      }
    } catch (err) {
      setMarketplaceSourceFormError(extractErrorMessage(err));
    } finally {
      setMarketplaceSourceActionKey((current) => (current === "add" ? null : current));
    }
  };

  const handleRefreshMarketplaceSource = async (source: MarketplaceSourceView) => {
    if (!source.removable || source.kind !== "remote") return;
    setMessage(null);
    setMarketplaceSourceFormError(null);
    setMarketplaceSourceActionKey(`refresh:${source.id}`);
    try {
      const result = await refreshMarketplaceSource(source.id, projectRoot);
      setMarketplaceSourceRefreshResults((current) => ({
        ...current,
        [source.id]: { ...result },
      }));
      if (result.ok) setMessage(marketplaceSourceRefreshMessage(result));
    } catch (err) {
      const result: RefreshResult = {
        id: source.id,
        ok: false,
        message: extractErrorMessage(err),
      };
      setMarketplaceSourceRefreshResults((current) => ({
        ...current,
        [source.id]: result,
      }));
    } finally {
      setMarketplaceSourceActionKey((current) =>
        current === `refresh:${source.id}` ? null : current,
      );
    }
  };

  const handleRefreshBuiltinMarketplace = async () => {
    setMessage(null);
    setMarketplaceSourceFormError(null);
    const status = await ensureBuiltinMarketplace(projectRoot);
    if (status.ok) setMessage(status.message);
  };

  const handleMigratePluginState = async () => {
    setMessage(null);
    setPluginMigrationResult(null);
    setPluginMigrationRunning(true);
    try {
      const result = await migratePluginState(projectRoot);
      setPluginMigrationResult(result);
      setMessage(`Plugin migration completed · ${pluginMigrationSummary(result)}`);
    } catch {
      // Store exposes the error banner.
    } finally {
      setPluginMigrationRunning(false);
    }
  };

  const handleToggleMarketplaceSource = async (
    source: MarketplaceSourceView,
    enabled: boolean,
  ) => {
    if (!source.removable) return;
    setMessage(null);
    setMarketplaceSourceFormError(null);
    setMarketplaceSourceActionKey(`toggle:${source.id}`);
    try {
      await setMarketplaceSourceEnabled(source.id, enabled, projectRoot);
      setMessage(`${enabled ? "Enabled" : "Disabled"} ${marketplaceSourceLabel(source)}`);
    } catch {
      // Store exposes the error banner.
    } finally {
      setMarketplaceSourceActionKey((current) =>
        current === `toggle:${source.id}` ? null : current,
      );
    }
  };

  const handleRemoveMarketplaceSource = async (source: MarketplaceSourceView) => {
    if (!source.removable) return;
    setMessage(null);
    setMarketplaceSourceFormError(null);
    setMarketplaceSourceActionKey(`remove:${source.id}`);
    try {
      await removeMarketplaceSource(source.id, projectRoot);
      setMarketplaceSourceRefreshResults((current) => {
        const next = { ...current };
        delete next[source.id];
        return next;
      });
      setMessage(`Removed ${marketplaceSourceLabel(source)}`);
    } catch {
      // Store exposes the error banner.
    } finally {
      setMarketplaceSourceActionKey((current) =>
        current === `remove:${source.id}` ? null : current,
      );
    }
  };

  const handleConfigureEnvironment = async (
    plugin: PluginSummary,
    environment: PluginEnvironmentSummary,
  ) => {
    if (!plugin.installed) {
      setMessage("Install the plugin first; environment edits happen only in the user plugin copy.");
      return;
    }
    const target = environment.runtimeFile?.trim() || environment.manifestPath;
    try {
      await revealItemInDir(target);
      setMessage(`Opened environment file for ${pluginEnvironmentDisplayName(environment)}`);
    } catch (err) {
      setMessage(null);
      usePluginStore.setState({ error: extractErrorMessage(err) });
    }
  };

  const handleTestEnvironment = async (
    plugin: PluginSummary,
    environment: PluginEnvironmentSummary,
  ) => {
    const key = pluginEnvironmentKey(environment);
    setMessage(null);
    setEnvironmentCheckStates((current) => ({
      ...current,
      [key]: { loading: true, result: current[key]?.result ?? null, error: null },
    }));
    try {
      const result = await checkPluginEnvironment(plugin, environment.id, projectRoot);
      setEnvironmentCheckStates((current) => ({
        ...current,
        [key]: { loading: false, result: result.check, error: null },
      }));
      setMessage(`${pluginEnvironmentDisplayName(environment)} environment test: ${result.check.status}`);
    } catch (err) {
      const error = extractErrorMessage(err);
      setEnvironmentCheckStates((current) => ({
        ...current,
        [key]: { loading: false, result: current[key]?.result ?? null, error },
      }));
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

  const handleTemplateToggle = async (
    plugin: PluginSummary,
    templateId: string,
    enabled: boolean,
  ) => {
    setMessage(null);
    try {
      await setTemplateEnabled(plugin.id, templateId, enabled, projectRoot);
      setMessage(`${enabled ? "Enabled" : "Disabled"} template ${templateId}`);
    } catch {
      // Store exposes the error banner.
    }
  };

  const handleRetrievalResourceToggle = async (
    plugin: PluginSummary,
    category: string,
    resourceId: string,
    enabled: boolean,
  ) => {
    setMessage(null);
    try {
      await setRetrievalResourceEnabled(plugin.id, category, resourceId, enabled, projectRoot);
      setMessage(`${enabled ? "Enabled" : "Disabled"} route ${category}.${resourceId}`);
    } catch {
      // Store exposes the error banner.
    }
  };

  const handleEnvironmentToggle = async (
    plugin: PluginSummary,
    environment: PluginEnvironmentSummary,
    enabled: boolean,
  ) => {
    setMessage(null);
    try {
      await setEnvironmentEnabled(plugin.id, environment.id, enabled, projectRoot);
      setMessage(
        `${enabled ? "Enabled" : "Disabled"} environment ${pluginEnvironmentDisplayName(environment)}`,
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

  const handleRefreshOperatorRuns = async () => {
    setMessage(null);
    try {
      await loadOperatorRuns(projectRoot, operatorSurface);
      setMessage("Refreshed tool runs");
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
                  Install visualization, automation, tools, and external resource capabilities. Details and diagnostics stay one click away.
                </Typography>
              </Box>
            </Stack>
            <Stack direction="row" spacing={1} alignItems="center" flexWrap="wrap" sx={{ alignSelf: { xs: "flex-start", md: "center" } }}>
              <Tooltip title={remoteMarketplaceCount > 0 ? "Check configured remote marketplace manifests" : "No remote marketplace configured"}>
                <span>
                  <Button
                    variant="outlined"
                    startIcon={checkingRemoteMarketplaces ? <CircularProgress size={16} /> : <SyncRounded />}
                    disabled={isLoading || isMutating || checkingRemoteMarketplaces || remoteMarketplaceCount === 0}
                    onClick={() => void handleCheckRemoteMarketplaces()}
                    sx={{ textTransform: "none", borderRadius: 2, minHeight: 40 }}
                  >
                    Check updates
                  </Button>
                </span>
              </Tooltip>
              <Button
                variant="outlined"
                startIcon={isLoading ? <CircularProgress size={16} /> : <RefreshRounded />}
                disabled={isLoading || isMutating}
                onClick={() => void loadPlugins(projectRoot, operatorSurface)}
                sx={{ textTransform: "none", borderRadius: 2, minHeight: 40 }}
              >
                Refresh
              </Button>
            </Stack>
          </Stack>
          <Stack direction="row" spacing={1.5} flexWrap="wrap" useFlexGap alignItems="center">
            {[
              { label: "Enabled", value: enabledPlugins.length },
              { label: "Installable", value: availablePlugins.length },
              { label: "Exposed", value: exposedOperators.length },
              { label: "Runs", value: operatorRuns.length },
              { label: "Issues", value: quarantinedRouteCount + degradedRouteCount },
              { label: "Pooled", value: processPoolStatuses.length },
              { label: "Remote", value: remoteMarketplaceCount },
              { label: "Updates", value: remoteChangedPluginCount },
            ].map(({ label, value }) => {
              if (label === "Updates" && value > 0) {
                return (
                  <Chip
                    key={label}
                    size="small"
                    color="warning"
                    variant="outlined"
                    label={`${value} update${value === 1 ? "" : "s"}`}
                  />
                );
              }
              return (
                <Box key={label} sx={{ display: "inline-flex", alignItems: "baseline", gap: 0.5 }}>
                  <Typography variant="subtitle2" fontWeight={850}>
                    {value}
                  </Typography>
                  <Typography variant="caption" color="text.secondary" fontWeight={700}>
                    {label}
                  </Typography>
                </Box>
              );
            })}
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
              placeholder="Search plugins, resources, routes..."
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
        projectPath={projectPath}
        retrievalStatuses={detailPlugin ? retrievalStatusesByPlugin.get(detailPlugin.id) : undefined}
        processPoolStatuses={detailPlugin ? processPoolStatusesByPlugin.get(detailPlugin.id) : undefined}
        operators={detailPluginOperators}
        remoteUpdateAvailable={
          detailPlugin ? pluginHasRemoteMarketplaceUpdate(detailPlugin, remoteChangedPluginNames) : false
        }
        busy={isMutating || (detailPlugin ? busyPluginIds.has(detailPlugin.id) : false)}
        onClose={() => setDetailPluginId(null)}
        onInstall={(plugin) => void handleInstall(plugin)}
        onUninstall={(plugin) => void handleUninstall(plugin)}
        onSync={(plugin) => void handleSyncPlugin(plugin)}
        onForceSync={(plugin) => void handleForceSyncPlugin(plugin)}
        onToggle={(plugin, enabled) => void handleToggle(plugin, enabled)}
        onTemplateToggle={(plugin, templateId, enabled) => void handleTemplateToggle(plugin, templateId, enabled)}
        onRetrievalResourceToggle={(plugin, category, resourceId, enabled) =>
          void handleRetrievalResourceToggle(plugin, category, resourceId, enabled)
        }
        onConfigureEnvironment={(plugin, environment) => void handleConfigureEnvironment(plugin, environment)}
        onTestEnvironment={(plugin, environment) => void handleTestEnvironment(plugin, environment)}
        onEnvironmentToggle={(plugin, environment, enabled) =>
          void handleEnvironmentToggle(plugin, environment, enabled)
        }
        environmentCheckStates={environmentCheckStates}
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

      <OperatorRunsTimeline
        runs={operatorRuns}
        operators={operators}
        onOpen={(run) => void handleOpenOperatorRun(run)}
        onRefresh={() => void handleRefreshOperatorRuns()}
        busy={isMutating}
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
                      key={`${status.pluginId}:${status.category}:${status.resourceId}`}
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
                              key={`${status.category}:${status.resourceId}:${status.pluginRoot}`}
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

      <Accordion disableGutters elevation={0} sx={nestedAccordionSx}>
        <AccordionSummary expandIcon={<ExpandMoreRounded />} sx={nestedAccordionSummarySx}>
          <Stack direction="row" spacing={1} alignItems="center" flexWrap="wrap">
            <Typography variant="subtitle2" fontWeight={700}>Marketplace Sources</Typography>
            <Chip size="small" variant="outlined" label={`${marketplaceSourceViews.length} source${marketplaceSourceViews.length === 1 ? "" : "s"}`} />
          </Stack>
        </AccordionSummary>
        <AccordionDetails sx={{ px: 1.5, pt: 0.75, pb: 1.5 }}>
          <Stack spacing={1.25} useFlexGap>
            <Alert
              severity={pluginMigrationResult?.warnings.length ? "warning" : "info"}
              sx={{ borderRadius: 1.5 }}
            >
              <Stack spacing={0.75}>
                <Stack
                  direction={{ xs: "column", sm: "row" }}
                  spacing={1}
                  alignItems={{ xs: "stretch", sm: "center" }}
                  justifyContent="space-between"
                >
                  <Typography variant="body2">
                    If you upgraded from an older version or plugins look inconsistent, run migration to refresh config, cache, and built-in roots.
                  </Typography>
                  <Button
                    size="small"
                    variant="outlined"
                    startIcon={
                      pluginMigrationRunning ? (
                        <CircularProgress size={14} />
                      ) : (
                        <PublishedWithChangesRounded />
                      )
                    }
                    disabled={isLoading || isMutating || bootstrapInProgress || pluginMigrationRunning}
                    onClick={() => void handleMigratePluginState()}
                    sx={{ textTransform: "none", borderRadius: 1.5, alignSelf: { xs: "flex-start", sm: "center" } }}
                  >
                    Run migration
                  </Button>
                </Stack>
                {pluginMigrationResult && (
                  <Box>
                    <Typography variant="body2" fontWeight={700}>
                      Last migration: {pluginMigrationSummary(pluginMigrationResult)}
                    </Typography>
                    {pluginMigrationResult.warnings.length > 0 && (
                      <Stack component="ul" spacing={0.25} sx={{ mt: 0.5, mb: 0, pl: 2 }}>
                        {pluginMigrationResult.warnings.map((warning) => (
                          <Typography
                            key={warning}
                            component="li"
                            variant="caption"
                            color="text.secondary"
                          >
                            {warning}
                          </Typography>
                        ))}
                      </Stack>
                    )}
                  </Box>
                )}
              </Stack>
            </Alert>
            {bootstrapInProgress && (
              <Stack direction="row" spacing={1} alignItems="center">
                <CircularProgress size={16} />
                <Typography variant="caption" color="text.secondary">
                  Refreshing built-in marketplace
                </Typography>
              </Stack>
            )}
            {builtinMarketplaceStatus && !builtinMarketplaceStatus.ok && (
              <Alert severity="warning" sx={{ borderRadius: 1.5 }}>
                <Stack
                  direction={{ xs: "column", sm: "row" }}
                  spacing={1}
                  alignItems={{ xs: "stretch", sm: "center" }}
                  justifyContent="space-between"
                >
                  <Typography variant="body2">
                    {builtinMarketplaceStatus.message}
                  </Typography>
                  <Button
                    size="small"
                    variant="outlined"
                    startIcon={bootstrapInProgress ? <CircularProgress size={14} /> : <RefreshRounded />}
                    disabled={isLoading || bootstrapInProgress}
                    onClick={() => void handleRefreshBuiltinMarketplace()}
                    sx={{ textTransform: "none", borderRadius: 1.5, alignSelf: { xs: "flex-start", sm: "center" } }}
                  >
                    Retry
                  </Button>
                </Stack>
              </Alert>
            )}
            {marketplaceSourceViews.length === 0 ? (
              <Box sx={{ p: 1.5, borderRadius: 2, textAlign: "center", bgcolor: "background.paper" }}>
                <Typography variant="body2" color="text.secondary">
                  No marketplace sources configured.
                </Typography>
              </Box>
            ) : (
              <Stack spacing={1} useFlexGap>
                {marketplaceSourceViews.map((source) => {
                  const label = marketplaceSourceLabel(source);
                  const sourceLabel = source.label?.trim();
                  const refreshResult = marketplaceSourceRefreshResults[source.id];
                  const kindLabel =
                    source.kind === "builtin"
                      ? "Built-in"
                      : source.kind === "remote"
                        ? "Remote"
                        : "Local";
                  const kindColor: ChipProps["color"] =
                    source.kind === "builtin"
                      ? "success"
                      : source.kind === "remote"
                        ? "info"
                        : "default";
                  return (
                    <Paper
                      key={source.id}
                      variant="outlined"
                      sx={{ p: 1.25, borderRadius: 2, bgcolor: "background.paper" }}
                    >
                      <Stack spacing={1}>
                        <Stack
                          direction={{ xs: "column", md: "row" }}
                          spacing={1}
                          alignItems={{ xs: "stretch", md: "center" }}
                        >
                          <Stack direction="row" spacing={1} alignItems="center" sx={{ minWidth: 0, flex: 1 }}>
                            <Chip
                              size="small"
                              color={kindColor}
                              variant="outlined"
                              label={kindLabel}
                            />
                            <Box sx={{ minWidth: 0 }}>
                              {sourceLabel ? (
                                <Typography variant="body2" fontWeight={800} noWrap title={sourceLabel}>
                                  {sourceLabel}
                                </Typography>
                              ) : null}
                              <Typography
                                variant={sourceLabel ? "caption" : "body2"}
                                color={sourceLabel ? "text.secondary" : "text.primary"}
                                sx={{ display: "block", wordBreak: "break-all" }}
                              >
                                {source.location}
                              </Typography>
                            </Box>
                          </Stack>
                          <Stack direction="row" spacing={0.75} alignItems="center" justifyContent="flex-end">
                            {source.removable ? (
                              <>
                                <Switch
                                  checked={source.enabled}
                                  disabled={marketplaceSourceMutationDisabled}
                                  onChange={(event) =>
                                    void handleToggleMarketplaceSource(source, event.target.checked)
                                  }
                                  inputProps={{
                                    "aria-label": `${source.enabled ? "Disable" : "Enable"} marketplace source ${label}`,
                                  }}
                                />
                                {source.kind === "remote" && (
                                  <Button
                                    size="small"
                                    variant="outlined"
                                    startIcon={
                                      marketplaceSourceActionKey === `refresh:${source.id}`
                                        ? <CircularProgress size={16} />
                                        : <RefreshRounded />
                                    }
                                    disabled={marketplaceSourceMutationDisabled}
                                    onClick={() => void handleRefreshMarketplaceSource(source)}
                                    aria-label={`Refresh marketplace source ${label}`}
                                    sx={{ textTransform: "none", borderRadius: 1.5 }}
                                  >
                                    Refresh
                                  </Button>
                                )}
                                <IconButton
                                  size="small"
                                  color="error"
                                  disabled={marketplaceSourceMutationDisabled}
                                  onClick={() => void handleRemoveMarketplaceSource(source)}
                                  aria-label={`Remove marketplace source ${label}`}
                                >
                                  {marketplaceSourceActionKey === `remove:${source.id}` ? (
                                    <CircularProgress size={16} />
                                  ) : (
                                    <DeleteOutlineRounded fontSize="small" />
                                  )}
                                </IconButton>
                              </>
                            ) : (
                              <>
                                {source.kind === "builtin" && (
                                  <Button
                                    size="small"
                                    variant="outlined"
                                    startIcon={bootstrapInProgress ? <CircularProgress size={16} /> : <RefreshRounded />}
                                    disabled={isLoading || bootstrapInProgress}
                                    onClick={() => void handleRefreshBuiltinMarketplace()}
                                    aria-label={`Refresh marketplace source ${label}`}
                                    sx={{ textTransform: "none", borderRadius: 1.5 }}
                                  >
                                    Refresh
                                  </Button>
                                )}
                                <Typography
                                  variant="caption"
                                  color="text.secondary"
                                  sx={{ whiteSpace: "nowrap" }}
                                >
                                  Always enabled
                                </Typography>
                              </>
                            )}
                          </Stack>
                        </Stack>
                        {refreshResult && (
                          <Alert severity={refreshResult.ok ? "success" : "error"} sx={{ borderRadius: 1.5 }}>
                            {marketplaceSourceRefreshMessage(refreshResult)}
                          </Alert>
                        )}
                      </Stack>
                    </Paper>
                  );
                })}
              </Stack>
            )}

            <Box sx={{ p: 1.25, borderRadius: 2, bgcolor: "background.paper" }}>
              <Stack spacing={1} useFlexGap>
                <Stack
                  direction={{ xs: "column", md: "row" }}
                  spacing={1}
                  alignItems={{ xs: "stretch", md: "flex-start" }}
                >
                  <TextField
                    select
                    size="small"
                    label="Source type"
                    value={marketplaceSourceKind}
                    onChange={(event) => setMarketplaceSourceKind(event.target.value as MarketplaceSourceKind)}
                    inputProps={{ "aria-label": "Marketplace source kind" }}
                    sx={{ minWidth: { xs: "100%", md: 150 } }}
                  >
                    <MenuItem value="local">Local</MenuItem>
                    <MenuItem value="remote">Remote</MenuItem>
                  </TextField>
                  <TextField
                    size="small"
                    label={marketplaceSourceKind === "local" ? "Local path" : "Remote Git URL"}
                    value={marketplaceSourceLocation}
                    onChange={(event) => setMarketplaceSourceLocation(event.target.value)}
                    error={Boolean(marketplaceSourceFormError)}
                    inputProps={{ "aria-label": "Marketplace source location" }}
                    sx={{ flex: 1, minWidth: { xs: "100%", md: 260 } }}
                  />
                  <TextField
                    size="small"
                    label="Label"
                    value={marketplaceSourceLabelInput}
                    onChange={(event) => setMarketplaceSourceLabelInput(event.target.value)}
                    inputProps={{ "aria-label": "Marketplace source label" }}
                    sx={{ minWidth: { xs: "100%", md: 190 } }}
                  />
                  <Button
                    size="small"
                    variant="contained"
                    startIcon={marketplaceSourceActionKey === "add" ? <CircularProgress size={16} /> : <AddRounded />}
                    disabled={marketplaceSourceMutationDisabled || marketplaceSourceActionKey === "add"}
                    onClick={() => void handleAddMarketplaceSource()}
                    aria-label="Add marketplace source"
                    sx={{
                      textTransform: "none",
                      borderRadius: 1.5,
                      minHeight: 40,
                      flexShrink: 0,
                      whiteSpace: "nowrap",
                      alignSelf: { xs: "stretch", md: "flex-start" },
                    }}
                  >
                    Add
                  </Button>
                </Stack>
                {marketplaceSourceFormError && (
                  <Alert severity="error" sx={{ borderRadius: 1.5 }}>
                    {marketplaceSourceFormError}
                  </Alert>
                )}
              </Stack>
            </Box>
          </Stack>
        </AccordionDetails>
      </Accordion>

      {marketplaces.length === 0 || allPlugins.length === 0 ? (
        <Paper variant="outlined" sx={{ p: 3, borderRadius: 2, textAlign: "center" }}>
          <ExtensionRounded sx={{ color: "text.secondary", mb: 1 }} />
          <Typography variant="body2" color="text.secondary">
            No plugin marketplace found yet. Clone or refresh the omiga-plugins repository used as the marketplace source.
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
          remoteChangedPluginNames={remoteChangedPluginNames}
          busy={isMutating}
          busyPluginIds={busyPluginIds}
          onInstall={(plugin) => void handleInstall(plugin)}
          onToggle={(plugin, enabled) => void handleToggle(plugin, enabled)}
          onOpenDetails={(selectedPlugin) => setDetailPluginId(selectedPlugin.id)}
        />
      )}

    </Stack>
    <Portal>
      <Snackbar
        key={feedbackKey ?? "plugin-feedback"}
        open={feedbackOpen}
        autoHideDuration={error ? null : 4200}
        onClose={handleFeedbackClose}
        anchorOrigin={{ vertical: "bottom", horizontal: "center" }}
        // Rendered through Portal and lifted above Dialog + Backdrop.
        sx={{ zIndex: (t) => t.zIndex.tooltip + 1 }}
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
    </Portal>
    </>
  );
}
