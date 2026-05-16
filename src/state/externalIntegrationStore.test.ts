import { beforeEach, describe, expect, it, vi } from "vitest";

const invokeMock = vi.hoisted(() => vi.fn());

vi.mock("@tauri-apps/api/core", () => ({
  invoke: invokeMock,
}));

import {
  buildConnectorLikeBadges,
  buildExternalIntegrationsCatalogFromLegacy,
  buildMcpServerBadges,
  selectConnectorLikeItems,
  selectExternalIntegrationSummary,
  selectMcpServerRows,
  useExternalIntegrationStore,
  type McpServerCatalogEntry,
} from "./externalIntegrationStore";
import type { ConnectorCatalog, ConnectorInfo } from "./connectorStore";

function connector(overrides: Partial<ConnectorInfo> = {}): ConnectorInfo {
  const definition = {
    id: "github",
    name: "GitHub",
    description: "Inspect repositories.",
    category: "code",
    authType: "oauth" as const,
    envVars: [],
    defaultEnabled: true,
    tools: [],
    ...overrides.definition,
  };

  return {
    enabled: true,
    connected: false,
    accessible: false,
    status: "needs_auth",
    accountLabel: null,
    authSource: null,
    connectedAt: null,
    envConfigured: false,
    referencedByPlugins: [],
    source: "built_in",
    lastConnectionTest: null,
    connectionTestHistory: [],
    connectionHealth: {
      totalChecks: 0,
      okChecks: 0,
      failedChecks: 0,
      retryableFailures: 0,
      lastOkAt: null,
      lastFailureAt: null,
      lastErrorCode: null,
      lastHttpStatus: null,
    },
    ...overrides,
    definition,
  };
}

function connectorCatalog(
  connectors: ConnectorInfo[],
  overrides: Partial<ConnectorCatalog> = {},
): ConnectorCatalog {
  return {
    connectors,
    scope: "user",
    configPath: "/tmp/connectors.json",
    notes: [],
    ...overrides,
  };
}

function mcpServer(
  overrides: Partial<McpServerCatalogEntry> = {},
): McpServerCatalogEntry {
  return {
    configKey: "codex_apps",
    normalizedKey: "codex_apps",
    enabled: true,
    config: {
      kind: "http",
      command: null,
      args: [],
      env: {},
      headers: {},
      url: "https://mcp.example.test",
      cwd: null,
    },
    toolListChecked: true,
    oauthAuthenticated: true,
    listToolsError: null,
    tools: [
      {
        wireName: "mcp__codex_apps__read_thread",
        description: "GitHub thread bridge",
      },
    ],
    ...overrides,
  };
}

describe("useExternalIntegrationStore", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    useExternalIntegrationStore.setState({
      catalog: null,
      isLoading: false,
      error: null,
      loadedAt: null,
      loadedProjectRoot: null,
    });
  });

  it("falls back to legacy connector and MCP commands when the unified command is unavailable", async () => {
    const github = connector({
      connected: true,
      accessible: true,
      status: "connected",
      authSource: "codex_apps",
      definition: {
        id: "github",
        name: "GitHub",
        tools: [
          {
            name: "read_thread",
            description: "Thread access",
            readOnly: true,
            requiredScopes: [],
            confirmationRequired: false,
            execution: "external_mcp",
          },
        ],
      },
    });

    invokeMock
      .mockRejectedValueOnce("unknown command get_external_integrations_catalog")
      .mockResolvedValueOnce(connectorCatalog([github]))
      .mockResolvedValueOnce({
        mcpServers: [mcpServer()],
      });

    const result = await useExternalIntegrationStore
      .getState()
      .loadCatalog("/project", { force: true, probeTools: true });

    expect(invokeMock).toHaveBeenNthCalledWith(
      1,
      "get_external_integrations_catalog",
      {
        projectRoot: "/project",
        ignoreCache: false,
        probeTools: true,
      },
    );
    expect(invokeMock).toHaveBeenNthCalledWith(2, "list_omiga_connectors");
    expect(invokeMock).toHaveBeenNthCalledWith(3, "get_integrations_catalog", {
      projectRoot: "/project",
      ignoreCache: false,
      probeTools: true,
    });
    expect(result?.source).toBe("legacy_fallback");

    const connectorItems = selectConnectorLikeItems(result ?? null);
    expect(connectorItems).toHaveLength(1);
    expect(connectorItems[0]).toMatchObject({
      kind: "mcp_backed_connector",
      connectorId: "github",
      actionTargets: {
        connectorId: "github",
        mcpServerKeys: ["codex_apps"],
      },
    });

    const mcpRows = selectMcpServerRows(result ?? null);
    expect(mcpRows).toHaveLength(1);
    expect(mcpRows[0].linkedConnectorIds).toEqual(["github"]);
  });

  it("does not fall back when the unified command returns a runtime error", async () => {
    invokeMock.mockRejectedValueOnce("timeout probing MCP");

    const result = await useExternalIntegrationStore
      .getState()
      .loadCatalog("/project", { force: true, probeTools: true });

    expect(result).toBeNull();
    expect(invokeMock).toHaveBeenCalledTimes(1);
    expect(invokeMock).toHaveBeenCalledWith(
      "get_external_integrations_catalog",
      {
        projectRoot: "/project",
        ignoreCache: false,
        probeTools: true,
      },
    );
    expect(useExternalIntegrationStore.getState()).toMatchObject({
      error: "timeout probing MCP",
      isLoading: false,
    });
  });

  it("normalizes the unified command payload into connector and MCP rows", async () => {
    const github = connector({
      connected: true,
      accessible: true,
      status: "connected",
      authSource: "codex_apps",
      definition: {
        id: "github",
        name: "GitHub",
      },
    });
    const codexApps = mcpServer();

    invokeMock.mockResolvedValueOnce({
      items: [
        {
          id: "connector:github",
          kind: "mcp_backed_connector",
          displayName: "GitHub",
          category: "code",
          enabled: true,
          connected: true,
          authenticated: true,
          accessible: true,
          connectorId: "github",
          normalizedMcpKey: "codex_apps",
          mcpServerKeys: ["codex_apps"],
          authSource: "codex_apps",
          accountLabel: "octocat",
          lastError: null,
          connector: github,
          mcpServers: [codexApps],
          skill: null,
        },
        {
          id: "mcp:filesystem",
          kind: "mcp_server",
          displayName: "filesystem",
          category: "mcp_server",
          enabled: true,
          connected: true,
          authenticated: false,
          accessible: true,
          connectorId: null,
          normalizedMcpKey: "filesystem",
          mcpServerKeys: ["filesystem"],
          authSource: null,
          accountLabel: null,
          lastError: null,
          connector: null,
          mcpServers: [
            mcpServer({
              configKey: "filesystem",
              normalizedKey: "filesystem",
              config: {
                kind: "stdio",
                command: "node",
                args: ["server.js"],
                env: {},
                headers: {},
                url: null,
                cwd: null,
              },
              oauthAuthenticated: false,
            }),
          ],
          skill: null,
        },
      ],
    });

    const result = await useExternalIntegrationStore
      .getState()
      .loadCatalog("/project", { force: true });

    expect(invokeMock).toHaveBeenCalledWith(
      "get_external_integrations_catalog",
      {
        projectRoot: "/project",
        ignoreCache: false,
        probeTools: false,
      },
    );
    expect(result?.source).toBe("unified_command");
    expect(selectConnectorLikeItems(result ?? null)[0]).toMatchObject({
      id: "github",
      kind: "mcp_backed_connector",
      connectorId: "github",
      accountLabel: "octocat",
      externalToolCount: 1,
    });
    expect(selectMcpServerRows(result ?? null)[0]).toMatchObject({
      id: "mcp:filesystem",
      displayName: "filesystem",
      protocol: "stdio",
    });
  });

  it("reuses a fresh catalog for the same project root", async () => {
    const catalog = buildExternalIntegrationsCatalogFromLegacy({
      connectors: [connector()],
      mcpServers: [mcpServer()],
    });

    useExternalIntegrationStore.setState({
      catalog,
      loadedAt: Date.now(),
      loadedProjectRoot: "/project",
      error: "stale",
    });

    const result = await useExternalIntegrationStore
      .getState()
      .loadCatalog("/project");

    expect(invokeMock).not.toHaveBeenCalled();
    expect(result).toBe(catalog);
    expect(useExternalIntegrationStore.getState().error).toBeNull();
  });
});

describe("external integration selectors", () => {
  it("builds connector and MCP selectors from legacy payloads", () => {
    const github = connector({
      connected: true,
      accessible: true,
      status: "connected",
      authSource: "codex_apps",
      accountLabel: "octocat",
      definition: {
        id: "github",
        name: "GitHub",
        description: "Inspect repositories.",
        tools: [
          {
            name: "list_pull_requests",
            description: "List pull requests",
            readOnly: true,
            requiredScopes: [],
            confirmationRequired: false,
            execution: "external_mcp",
          },
        ],
      },
    });
    const brokenServer = mcpServer({
      configKey: "github_remote",
      normalizedKey: "github_remote",
      oauthAuthenticated: false,
      listToolsError: "401 unauthorized",
      tools: [],
    });
    const codexApps = mcpServer();

    const catalog = buildExternalIntegrationsCatalogFromLegacy({
      connectors: [github],
      mcpServers: [codexApps, brokenServer],
      notes: ["legacy bridge active"],
    });

    const connectorItems = selectConnectorLikeItems(catalog, {
      search: "git",
    });
    expect(connectorItems).toHaveLength(1);
    expect(buildConnectorLikeBadges(connectorItems[0]).map((badge) => badge.label)).toContain(
      "1 MCP",
    );

    const mcpRows = selectMcpServerRows(catalog);
    expect(mcpRows).toHaveLength(2);
    expect(buildMcpServerBadges(mcpRows[1]).map((badge) => badge.label)).toContain(
      "Error",
    );

    expect(selectExternalIntegrationSummary(catalog)).toMatchObject({
      totalItems: 3,
      connectorLikeCount: 1,
      mcpServerCount: 2,
      connectedCount: 2,
      issueCount: 1,
    });
  });

  it("does not over-link codex apps fallback rows to unrelated external MCP connectors", () => {
    const github = connector({
      connected: true,
      accessible: true,
      status: "connected",
      authSource: "codex_apps",
      definition: {
        id: "github",
        name: "GitHub",
        authType: "externalMcp",
        tools: [
          {
            name: "read_pull_request",
            description: "Read pull requests",
            readOnly: true,
            requiredScopes: [],
            confirmationRequired: false,
            execution: "external_mcp",
          },
        ],
      },
    });
    const slack = connector({
      connected: true,
      accessible: true,
      status: "connected",
      authSource: "codex_apps",
      definition: {
        id: "slack",
        name: "Slack",
        authType: "externalMcp",
        tools: [
          {
            name: "post_message",
            description: "Post Slack messages",
            readOnly: false,
            requiredScopes: [],
            confirmationRequired: true,
            execution: "external_mcp",
          },
        ],
      },
    });
    const codexApps = mcpServer({
      tools: [
        {
          wireName: "mcp__codex_apps__read_pull_request",
          description: "Read GitHub pull requests",
          connectorId: "github",
          connectorName: "GitHub",
        },
      ],
    });

    const catalog = buildExternalIntegrationsCatalogFromLegacy({
      connectors: [github, slack],
      mcpServers: [codexApps],
    });

    const connectorItems = selectConnectorLikeItems(catalog);
    expect(connectorItems).toHaveLength(2);
    expect(
      connectorItems.find((item) => item.connectorId === "github"),
    ).toMatchObject({
      kind: "mcp_backed_connector",
      mcpServers: [{ configKey: "codex_apps" }],
      externalToolCount: 1,
    });
    expect(
      connectorItems.find((item) => item.connectorId === "slack"),
    ).toMatchObject({
      kind: "connector",
      mcpServers: [],
      externalToolCount: 0,
    });
    expect(selectMcpServerRows(catalog)[0].linkedConnectorIds).toEqual([
      "github",
    ]);
  });
});
