import { beforeEach, describe, expect, it, vi } from "vitest";

const invokeMock = vi.hoisted(() => vi.fn());

vi.mock("@tauri-apps/api/core", () => ({
  invoke: invokeMock,
}));

import {
  useConnectorStore,
  type ConnectorCatalog,
  type ConnectorInfo,
} from "./connectorStore";

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

function catalog(overrides: Partial<ConnectorCatalog> = {}): ConnectorCatalog {
  return {
    connectors: [connector()],
    scope: "user",
    configPath: "/tmp/connectors.json",
    notes: [],
    ...overrides,
  };
}

describe("useConnectorStore catalog loading", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    useConnectorStore.setState({
      catalog: null,
      auditEvents: [],
      isLoading: false,
      isMutating: false,
      testingConnectorIds: {},
      testResults: {},
      error: null,
      loadedAt: null,
    });
  });

  it("loads connector definitions without eagerly loading audit events", async () => {
    const loadedCatalog = catalog();
    invokeMock.mockResolvedValue(loadedCatalog);

    await useConnectorStore.getState().loadConnectors({ force: true });

    expect(invokeMock).toHaveBeenCalledTimes(1);
    expect(invokeMock).toHaveBeenCalledWith("list_omiga_connectors");
    expect(useConnectorStore.getState().catalog).toBe(loadedCatalog);
    expect(useConnectorStore.getState().auditEvents).toEqual([]);
  });

  it("reuses a recently loaded catalog on quick tab switches", async () => {
    const loadedCatalog = catalog();
    useConnectorStore.setState({
      catalog: loadedCatalog,
      loadedAt: Date.now(),
      error: "stale error",
    });

    await useConnectorStore.getState().loadConnectors();

    expect(invokeMock).not.toHaveBeenCalled();
    expect(useConnectorStore.getState().catalog).toBe(loadedCatalog);
    expect(useConnectorStore.getState().error).toBeNull();
  });
});
