import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import type {
  ConnectorAuthType,
  ConnectorCatalog,
  ConnectorConnectionStatus,
  ConnectorConnectionTestResult,
  ConnectorDefinitionSource,
  ConnectorHealthSummary,
  ConnectorInfo,
} from "./connectorStore";
import { extractErrorMessage } from "../utils/errorMessage";

const EXTERNAL_INTEGRATIONS_CACHE_TTL_MS = 30_000;
const UNIFIED_EXTERNAL_INTEGRATIONS_COMMAND = "get_external_integrations_catalog";

export type ExternalIntegrationKind =
  | "connector"
  | "mcp_backed_connector"
  | "mcp_server"
  | "skill";

export type ExternalIntegrationCatalogSource =
  | "unified_command"
  | "legacy_fallback";

export type ExternalMcpProtocol = "stdio" | "http";
export type ExternalMcpRowStatus =
  | "connected"
  | "configured"
  | "needs_auth"
  | "error"
  | "disabled";
export type ExternalIntegrationSource =
  | ConnectorDefinitionSource
  | "mcp"
  | "mixed";
export type ExternalIntegrationBadgeTone =
  | "default"
  | "success"
  | "warning"
  | "error"
  | "info";

export interface ExternalIntegrationBadge {
  label: string;
  tone: ExternalIntegrationBadgeTone;
}

export interface McpToolCatalogEntry {
  wireName: string;
  description: string;
  connectorId?: string | null;
  connectorName?: string | null;
  connectorDescription?: string | null;
}

export interface McpServerConfigCatalogEntry {
  kind: ExternalMcpProtocol;
  command: string | null;
  args: string[];
  env: Record<string, string>;
  headers: Record<string, string>;
  url: string | null;
  cwd: string | null;
}

export interface McpServerCatalogEntry {
  configKey: string;
  normalizedKey: string;
  enabled: boolean;
  config: McpServerConfigCatalogEntry;
  toolListChecked: boolean;
  oauthAuthenticated: boolean;
  listToolsError: string | null;
  tools: McpToolCatalogEntry[];
}

interface LegacyIntegrationsCatalog {
  mcpServers: McpServerCatalogEntry[];
}

export interface ExternalIntegrationLinkedMcpServer {
  configKey: string;
  normalizedKey: string;
  enabled: boolean;
  oauthAuthenticated: boolean;
  toolCount: number;
  toolListChecked: boolean;
  listToolsError: string | null;
}

export interface ExternalConnectorLikeItem {
  id: string;
  kind: "connector" | "mcp_backed_connector";
  displayName: string;
  description: string;
  category: string;
  enabled: boolean;
  connected: boolean;
  accessible: boolean;
  status: ConnectorConnectionStatus;
  source: ExternalIntegrationSource;
  authType: ConnectorAuthType;
  accountLabel?: string | null;
  authSource?: string | null;
  referencedByPlugins: string[];
  connectionHealth: ConnectorHealthSummary;
  lastConnectionTest?: ConnectorConnectionTestResult | null;
  connector: ConnectorInfo;
  connectorId: string;
  mcpServers: ExternalIntegrationLinkedMcpServer[];
  toolCount: number;
  nativeToolCount: number;
  externalToolCount: number;
  lastError: string | null;
  actionTargets: {
    connectorId: string;
    mcpServerKeys: string[];
  };
}

export interface ExternalMcpServerItem {
  id: string;
  kind: "mcp_server";
  displayName: string;
  description: string;
  category: string;
  enabled: boolean;
  status: ExternalMcpRowStatus;
  protocol: ExternalMcpProtocol;
  oauthAuthenticated: boolean;
  toolListChecked: boolean;
  listToolsError: string | null;
  tools: McpToolCatalogEntry[];
  toolCount: number;
  linkedConnectorIds: string[];
  linkedConnectors: Array<{
    connectorId: string;
    displayName: string;
    status: ConnectorConnectionStatus;
  }>;
  config: McpServerConfigCatalogEntry;
  actionTargets: {
    serverName: string;
    linkedConnectorIds: string[];
  };
}

export type ExternalIntegrationItem =
  | ExternalConnectorLikeItem
  | ExternalMcpServerItem;

export interface ExternalIntegrationsCatalog {
  items: ExternalIntegrationItem[];
  notes: string[];
  source: ExternalIntegrationCatalogSource;
  scope?: string | null;
  configPath?: string | null;
}

export interface LoadExternalIntegrationsOptions {
  force?: boolean;
  background?: boolean;
  probeTools?: boolean;
  ignoreCache?: boolean;
}

interface ExternalIntegrationState {
  catalog: ExternalIntegrationsCatalog | null;
  isLoading: boolean;
  error: string | null;
  loadedAt: number | null;
  loadedProjectRoot: string | null;
  loadCatalog: (
    projectRoot: string,
    options?: LoadExternalIntegrationsOptions,
  ) => Promise<ExternalIntegrationsCatalog | null>;
  clearError: () => void;
}

type LegacyExternalCatalogPayload = {
  connectors?: ConnectorInfo[];
  mcpServers?: McpServerCatalogEntry[];
  notes?: string[];
  scope?: string | null;
  configPath?: string | null;
};

type UnifiedExternalCatalogPayload = {
  items: UnifiedExternalIntegrationEntry[];
};

type UnifiedExternalIntegrationEntry = {
  id: string;
  kind: ExternalIntegrationKind;
  displayName: string;
  category?: string | null;
  enabled: boolean;
  connected: boolean;
  authenticated: boolean;
  accessible: boolean;
  connectorId?: string | null;
  normalizedMcpKey?: string | null;
  mcpServerKeys?: string[];
  authSource?: string | null;
  accountLabel?: string | null;
  lastError?: string | null;
  connector?: ConnectorInfo | null;
  mcpServers?: McpServerCatalogEntry[];
};

function normalizeProjectRoot(projectRoot: string): string {
  return projectRoot.trim() || ".";
}

function normalizeSearchValue(value: unknown): string {
  return String(value ?? "")
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "");
}

function isMissingUnifiedCatalogCommand(error: unknown): boolean {
  const message = extractErrorMessage(error).toLowerCase();
  return (
    message.includes("get_external_integrations_catalog") &&
    (message.includes("unknown command") ||
      message.includes("not found") ||
      message.includes("missing") ||
      message.includes("does not exist"))
  );
}

function toolTextHaystack(tool: McpToolCatalogEntry): string[] {
  return [
    tool.wireName,
    tool.description,
    tool.connectorId,
    tool.connectorName,
    tool.connectorDescription,
  ].map(normalizeSearchValue);
}

function connectorMatchesMcpServer(
  connector: ConnectorInfo,
  server: McpServerCatalogEntry,
): boolean {
  const connectorId = normalizeSearchValue(connector.definition.id);
  const connectorName = normalizeSearchValue(connector.definition.name);
  const declaredExternalTerms = connector.definition.tools
    .filter((tool) => tool.execution === "external_mcp")
    .flatMap((tool) => [tool.name, tool.description])
    .map(normalizeSearchValue)
    .filter(Boolean);
  const toolTerms = server.tools.flatMap(toolTextHaystack);

  if (toolTerms.some((term) => term.includes(connectorId))) {
    return true;
  }
  if (toolTerms.some((term) => term.includes(connectorName))) {
    return true;
  }

  return declaredExternalTerms.some((term) =>
    toolTerms.some((toolTerm) => toolTerm.includes(term)),
  );
}

function connectorLastError(
  connector: ConnectorInfo,
  linkedServers: McpServerCatalogEntry[],
): string | null {
  const lastTest = connector.lastConnectionTest;
  if (lastTest && !lastTest.ok) {
    return lastTest.message;
  }
  return (
    linkedServers.find((server) => Boolean(server.listToolsError))
      ?.listToolsError ?? null
  );
}

function summarizeConnectorSource(
  connector: ConnectorInfo,
  linkedServers: McpServerCatalogEntry[],
): ExternalIntegrationSource {
  if (linkedServers.length === 0) {
    return connector.source;
  }
  return connector.source === "plugin" ? "mixed" : "mcp";
}

function toolCountSummary(
  connector: ConnectorInfo,
  linkedServers: McpServerCatalogEntry[],
): {
  nativeToolCount: number;
  externalToolCount: number;
  toolCount: number;
} {
  const nativeToolCount = connector.definition.tools.filter(
    (tool) => tool.execution !== "external_mcp",
  ).length;
  const externalToolCount = new Set(
    linkedServers.flatMap((server) => server.tools.map((tool) => tool.wireName)),
  ).size;
  return {
    nativeToolCount,
    externalToolCount,
    toolCount: nativeToolCount + externalToolCount,
  };
}

function buildMcpRowStatus(server: McpServerCatalogEntry): ExternalMcpRowStatus {
  if (!server.enabled) return "disabled";
  if (server.listToolsError) return "error";
  if (server.oauthAuthenticated) return "connected";
  if (server.toolListChecked && server.tools.length > 0) return "connected";
  if (server.config.kind === "http" && !server.oauthAuthenticated) {
    return server.toolListChecked ? "needs_auth" : "configured";
  }
  return "configured";
}

function describeMcpServer(server: McpServerCatalogEntry): string {
  if (server.config.kind === "http") {
    return server.config.url || "Remote MCP server";
  }
  if (server.config.command) {
    return [server.config.command, ...server.config.args].join(" ");
  }
  return "Local MCP server";
}

export function buildExternalIntegrationsCatalogFromLegacy(
  payload: LegacyExternalCatalogPayload,
): ExternalIntegrationsCatalog {
  const connectors = payload.connectors ?? [];
  const mcpServers = payload.mcpServers ?? [];
  const linkedConnectorIdsByServer = new Map<string, string[]>();

  const connectorItems: ExternalConnectorLikeItem[] = connectors.map(
    (connector) => {
      const linkedServers = mcpServers.filter((server) =>
        connectorMatchesMcpServer(connector, server),
      );
      const counts = toolCountSummary(connector, linkedServers);

      for (const server of linkedServers) {
        const existing = linkedConnectorIdsByServer.get(server.normalizedKey) ?? [];
        linkedConnectorIdsByServer.set(server.normalizedKey, [
          ...existing,
          connector.definition.id,
        ]);
      }

      return {
        id: connector.definition.id,
        kind: linkedServers.length > 0 ? "mcp_backed_connector" : "connector",
        displayName: connector.definition.name,
        description: connector.definition.description,
        category: connector.definition.category || "other",
        enabled: connector.enabled,
        connected: connector.connected,
        accessible: connector.accessible,
        status: connector.status,
        source: summarizeConnectorSource(connector, linkedServers),
        authType: connector.definition.authType,
        accountLabel: connector.accountLabel,
        authSource: connector.authSource,
        referencedByPlugins: connector.referencedByPlugins,
        connectionHealth: connector.connectionHealth,
        lastConnectionTest: connector.lastConnectionTest,
        connector,
        connectorId: connector.definition.id,
        mcpServers: linkedServers.map((server) => ({
          configKey: server.configKey,
          normalizedKey: server.normalizedKey,
          enabled: server.enabled,
          oauthAuthenticated: server.oauthAuthenticated,
          toolCount: server.tools.length,
          toolListChecked: server.toolListChecked,
          listToolsError: server.listToolsError,
        })),
        toolCount: counts.toolCount,
        nativeToolCount: counts.nativeToolCount,
        externalToolCount: counts.externalToolCount,
        lastError: connectorLastError(connector, linkedServers),
        actionTargets: {
          connectorId: connector.definition.id,
          mcpServerKeys: linkedServers.map((server) => server.configKey),
        },
      };
    },
  );

  const connectorLookup = new Map(
    connectorItems.map((item) => [item.connectorId, item] as const),
  );

  const mcpItems: ExternalMcpServerItem[] = mcpServers.map((server) => {
    const linkedConnectorIds = linkedConnectorIdsByServer.get(server.normalizedKey) ?? [];
    const linkedConnectors = linkedConnectorIds
      .map((connectorId) => connectorLookup.get(connectorId))
      .filter((item): item is ExternalConnectorLikeItem => Boolean(item))
      .map((item) => ({
        connectorId: item.connectorId,
        displayName: item.displayName,
        status: item.status,
      }));

    return {
      id: `mcp:${server.normalizedKey}`,
      kind: "mcp_server",
      displayName: server.configKey,
      description: describeMcpServer(server),
      category: "mcp",
      enabled: server.enabled,
      status: buildMcpRowStatus(server),
      protocol: server.config.kind,
      oauthAuthenticated: server.oauthAuthenticated,
      toolListChecked: server.toolListChecked,
      listToolsError: server.listToolsError,
      tools: server.tools,
      toolCount: server.tools.length,
      linkedConnectorIds,
      linkedConnectors,
      config: server.config,
      actionTargets: {
        serverName: server.configKey,
        linkedConnectorIds,
      },
    };
  });

  return {
    items: [...connectorItems, ...mcpItems],
    notes: payload.notes ?? [],
    source: "legacy_fallback",
    scope: payload.scope ?? null,
    configPath: payload.configPath ?? null,
  };
}

function buildExternalIntegrationsCatalogFromUnified(
  payload: UnifiedExternalCatalogPayload,
): ExternalIntegrationsCatalog {
  const unifiedItems = payload.items;
  const connectorEntries = unifiedItems.filter(
    (item) =>
      (item.kind === "connector" || item.kind === "mcp_backed_connector") &&
      Boolean(item.connector),
  );
  const linkedConnectorIdsByServer = new Map<string, string[]>();

  const connectorItems: ExternalConnectorLikeItem[] = connectorEntries.map(
    (item) => {
      const connector = item.connector as ConnectorInfo;
      const linkedServers = item.mcpServers ?? [];
      const counts = toolCountSummary(connector, linkedServers);
      const connectorId = item.connectorId ?? connector.definition.id;
      const mcpServerKeys =
        item.mcpServerKeys ?? linkedServers.map((server) => server.configKey);

      for (const serverKey of mcpServerKeys) {
        const existing = linkedConnectorIdsByServer.get(serverKey) ?? [];
        linkedConnectorIdsByServer.set(serverKey, [...existing, connectorId]);
      }

      return {
        id: connector.definition.id,
        kind:
          item.kind === "mcp_backed_connector"
            ? "mcp_backed_connector"
            : "connector",
        displayName: item.displayName || connector.definition.name,
        description: connector.definition.description,
        category: item.category ?? connector.definition.category ?? "other",
        enabled: item.enabled,
        connected: item.connected,
        accessible: item.accessible,
        status: connector.status,
        source: summarizeConnectorSource(connector, linkedServers),
        authType: connector.definition.authType,
        accountLabel: item.accountLabel ?? connector.accountLabel,
        authSource: item.authSource ?? connector.authSource,
        referencedByPlugins: connector.referencedByPlugins,
        connectionHealth: connector.connectionHealth,
        lastConnectionTest: connector.lastConnectionTest,
        connector,
        connectorId,
        mcpServers: linkedServers.map((server) => ({
          configKey: server.configKey,
          normalizedKey: server.normalizedKey,
          enabled: server.enabled,
          oauthAuthenticated: server.oauthAuthenticated,
          toolCount: server.tools.length,
          toolListChecked: server.toolListChecked,
          listToolsError: server.listToolsError,
        })),
        toolCount: counts.toolCount,
        nativeToolCount: counts.nativeToolCount,
        externalToolCount: counts.externalToolCount,
        lastError: item.lastError ?? connectorLastError(connector, linkedServers),
        actionTargets: {
          connectorId,
          mcpServerKeys,
        },
      };
    },
  );

  const connectorLookup = new Map(
    connectorItems.map((item) => [item.connectorId, item] as const),
  );

  const mcpItems: ExternalMcpServerItem[] = unifiedItems
    .filter((item) => item.kind === "mcp_server")
    .flatMap((item) => {
      const server = item.mcpServers?.[0];
      if (!server) return [];
      const linkedConnectorIds =
        linkedConnectorIdsByServer.get(server.configKey) ?? [];
      const linkedConnectors = linkedConnectorIds
        .map((connectorId) => connectorLookup.get(connectorId))
        .filter((connector): connector is ExternalConnectorLikeItem =>
          Boolean(connector),
        )
        .map((connector) => ({
          connectorId: connector.connectorId,
          displayName: connector.displayName,
          status: connector.status,
        }));

      return [
        {
          id: item.id,
          kind: "mcp_server" as const,
          displayName: item.displayName || server.configKey,
          description: describeMcpServer(server),
          category: item.category ?? "mcp",
          enabled: item.enabled,
          status: buildMcpRowStatus(server),
          protocol: server.config.kind,
          oauthAuthenticated: server.oauthAuthenticated,
          toolListChecked: server.toolListChecked,
          listToolsError: item.lastError ?? server.listToolsError,
          tools: server.tools,
          toolCount: server.tools.length,
          linkedConnectorIds,
          linkedConnectors,
          config: server.config,
          actionTargets: {
            serverName: server.configKey,
            linkedConnectorIds,
          },
        },
      ];
    });

  return {
    items: [...connectorItems, ...mcpItems],
    notes: [],
    source: "unified_command",
    scope: null,
    configPath: null,
  };
}

function isExternalIntegrationCatalog(
  value: unknown,
): value is ExternalIntegrationsCatalog {
  if (!value || typeof value !== "object") return false;
  const candidate = value as Record<string, unknown>;
  return Array.isArray(candidate.items);
}

function isUnifiedCatalogPayload(
  value: unknown,
): value is UnifiedExternalCatalogPayload {
  if (!value || typeof value !== "object") return false;
  const candidate = value as Record<string, unknown>;
  return (
    Array.isArray(candidate.items) &&
    typeof candidate.source !== "string" &&
    candidate.items.some((item) => {
      if (!item || typeof item !== "object") return false;
      const record = item as Record<string, unknown>;
      return (
        typeof record.kind === "string" &&
        ("connector" in record ||
          "mcpServers" in record ||
          "mcpServerKeys" in record)
      );
    })
  );
}

function isLegacyCatalogPayload(
  value: unknown,
): value is LegacyExternalCatalogPayload {
  if (!value || typeof value !== "object") return false;
  const candidate = value as Record<string, unknown>;
  return (
    Array.isArray(candidate.connectors) || Array.isArray(candidate.mcpServers)
  );
}

function normalizeExternalCatalogPayload(
  payload: unknown,
): ExternalIntegrationsCatalog {
  if (isUnifiedCatalogPayload(payload)) {
    return buildExternalIntegrationsCatalogFromUnified(payload);
  }
  if (isExternalIntegrationCatalog(payload)) {
    return {
      ...payload,
      source: payload.source ?? "unified_command",
    };
  }
  if (isLegacyCatalogPayload(payload)) {
    const catalog = buildExternalIntegrationsCatalogFromLegacy(payload);
    return {
      ...catalog,
      source: "unified_command",
    };
  }
  throw new Error("External integrations catalog returned an unsupported shape.");
}

async function loadLegacyExternalIntegrations(
  projectRoot: string,
  options?: Pick<LoadExternalIntegrationsOptions, "ignoreCache" | "probeTools">,
): Promise<ExternalIntegrationsCatalog> {
  const [connectorCatalog, integrationsCatalog] = await Promise.all([
    invoke<ConnectorCatalog>("list_omiga_connectors"),
    invoke<LegacyIntegrationsCatalog>("get_integrations_catalog", {
      projectRoot,
      ignoreCache: options?.ignoreCache ?? false,
      probeTools: options?.probeTools ?? false,
    }),
  ]);

  return buildExternalIntegrationsCatalogFromLegacy({
    connectors: connectorCatalog.connectors,
    scope: connectorCatalog.scope,
    configPath: connectorCatalog.configPath,
    notes: connectorCatalog.notes,
    mcpServers: integrationsCatalog.mcpServers,
  });
}

async function loadExternalIntegrationsCatalog(
  projectRoot: string,
  options?: Pick<LoadExternalIntegrationsOptions, "ignoreCache" | "probeTools">,
): Promise<ExternalIntegrationsCatalog> {
  try {
    const payload = await invoke<unknown>(UNIFIED_EXTERNAL_INTEGRATIONS_COMMAND, {
      projectRoot,
      ignoreCache: options?.ignoreCache ?? false,
      probeTools: options?.probeTools ?? false,
    });
    return normalizeExternalCatalogPayload(payload);
  } catch (error) {
    if (!isMissingUnifiedCatalogCommand(error)) {
      throw error;
    }
    return loadLegacyExternalIntegrations(projectRoot, options);
  }
}

export function isConnectorLikeItem(
  item: ExternalIntegrationItem,
): item is ExternalConnectorLikeItem {
  return item.kind === "connector" || item.kind === "mcp_backed_connector";
}

export function isMcpServerItem(
  item: ExternalIntegrationItem,
): item is ExternalMcpServerItem {
  return item.kind === "mcp_server";
}

function matchesSearch(item: ExternalIntegrationItem, search: string): boolean {
  if (!search) return true;
  const needle = normalizeSearchValue(search);
  if (!needle) return true;
  const haystack = [
    item.displayName,
    item.description,
    item.category,
    ...(isConnectorLikeItem(item)
      ? [
          item.connectorId,
          item.connector.definition.name,
          item.connector.definition.description,
          item.authSource ?? "",
          item.accountLabel ?? "",
          ...item.mcpServers.map((server) => server.configKey),
        ]
      : [
          item.displayName,
          item.description,
          item.protocol,
          ...item.linkedConnectors.map((connector) => connector.displayName),
          ...item.tools.map((tool) => tool.wireName),
        ]),
  ]
    .map(normalizeSearchValue)
    .filter(Boolean);

  return haystack.some((value) => value.includes(needle));
}

export function selectConnectorLikeItems(
  catalog: ExternalIntegrationsCatalog | null,
  options: { search?: string } = {},
): ExternalConnectorLikeItem[] {
  if (!catalog) return [];
  return catalog.items
    .filter(isConnectorLikeItem)
    .filter((item) => matchesSearch(item, options.search ?? ""))
    .sort((left, right) => left.displayName.localeCompare(right.displayName));
}

export function selectMcpServerRows(
  catalog: ExternalIntegrationsCatalog | null,
  options: { search?: string } = {},
): ExternalMcpServerItem[] {
  if (!catalog) return [];
  return catalog.items
    .filter(isMcpServerItem)
    .filter((item) => matchesSearch(item, options.search ?? ""))
    .sort((left, right) => left.displayName.localeCompare(right.displayName));
}

export function selectExternalIntegrationSummary(
  catalog: ExternalIntegrationsCatalog | null,
): {
  totalItems: number;
  connectorLikeCount: number;
  mcpServerCount: number;
  connectedCount: number;
  issueCount: number;
} {
  const connectorItems = selectConnectorLikeItems(catalog);
  const mcpItems = selectMcpServerRows(catalog);
  const connectedCount =
    connectorItems.filter((item) => item.connected).length +
    mcpItems.filter((item) => item.status === "connected").length;
  const issueCount =
    connectorItems.filter(
      (item) =>
        item.status === "needs_auth" ||
        item.lastError ||
        (item.enabled && !item.accessible),
    ).length +
    mcpItems.filter(
      (item) => item.status === "error" || item.status === "needs_auth",
    ).length;

  return {
    totalItems: catalog?.items.length ?? 0,
    connectorLikeCount: connectorItems.length,
    mcpServerCount: mcpItems.length,
    connectedCount,
    issueCount,
  };
}

export function connectorStatusLabel(status: ConnectorConnectionStatus): string {
  switch (status) {
    case "connected":
      return "Connected";
    case "needs_auth":
      return "Needs auth";
    case "disabled":
      return "Disabled";
    case "metadata_only":
      return "Metadata only";
    default:
      return status;
  }
}

export function mcpStatusLabel(status: ExternalMcpRowStatus): string {
  switch (status) {
    case "connected":
      return "Connected";
    case "configured":
      return "Configured";
    case "needs_auth":
      return "Needs auth";
    case "error":
      return "Error";
    case "disabled":
      return "Disabled";
    default:
      return status;
  }
}

export function buildConnectorLikeBadges(
  item: ExternalConnectorLikeItem,
): ExternalIntegrationBadge[] {
  const badges: ExternalIntegrationBadge[] = [
    {
      label: connectorStatusLabel(item.status),
      tone:
        item.status === "connected"
          ? "success"
          : item.status === "needs_auth"
            ? "warning"
            : "default",
    },
    {
      label: item.enabled ? "Enabled" : "Disabled",
      tone: item.enabled ? "info" : "default",
    },
    {
      label: `${item.toolCount} tools`,
      tone: "default",
    },
  ];

  if (item.kind === "mcp_backed_connector") {
    badges.push({
      label: `${item.mcpServers.length} MCP`,
      tone: "info",
    });
  }
  if (item.lastError) {
    badges.push({ label: "Issue", tone: "error" });
  }

  return badges;
}

export function buildMcpServerBadges(
  item: ExternalMcpServerItem,
): ExternalIntegrationBadge[] {
  const badges: ExternalIntegrationBadge[] = [
    {
      label: item.protocol.toUpperCase(),
      tone: "default",
    },
    {
      label: mcpStatusLabel(item.status),
      tone:
        item.status === "connected"
          ? "success"
          : item.status === "needs_auth"
            ? "warning"
            : item.status === "error"
              ? "error"
              : "default",
    },
    {
      label: `${item.toolCount} tools`,
      tone: "default",
    },
  ];

  if (item.linkedConnectorIds.length > 0) {
    badges.push({
      label: `${item.linkedConnectorIds.length} linked`,
      tone: "info",
    });
  }

  return badges;
}

export const useExternalIntegrationStore = create<ExternalIntegrationState>(
  (set, get) => ({
    catalog: null,
    isLoading: false,
    error: null,
    loadedAt: null,
    loadedProjectRoot: null,

    loadCatalog: async (projectRoot, options = {}) => {
      const root = normalizeProjectRoot(projectRoot);
      const { force = false, background = false } = options;
      const state = get();
      const now = Date.now();

      if (
        !force &&
        state.loadedProjectRoot === root &&
        (state.isLoading ||
          (state.catalog &&
            state.loadedAt &&
            now - state.loadedAt < EXTERNAL_INTEGRATIONS_CACHE_TTL_MS))
      ) {
        if (state.error) {
          set({ error: null });
        }
        return state.catalog;
      }

      if (background) {
        set({ error: null });
      } else {
        set({ isLoading: true, error: null });
      }

      try {
        const catalog = await loadExternalIntegrationsCatalog(root, options);
        set({
          catalog,
          isLoading: false,
          error: null,
          loadedAt: Date.now(),
          loadedProjectRoot: root,
        });
        return catalog;
      } catch (error) {
        set({
          isLoading: false,
          error: extractErrorMessage(error),
        });
        return null;
      }
    },

    clearError: () => set({ error: null }),
  }),
);
