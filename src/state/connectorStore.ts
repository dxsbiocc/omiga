import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import { extractErrorMessage } from "../utils/errorMessage";

const CONNECTOR_TEST_HISTORY_LIMIT = 20;
const CONNECTOR_CATALOG_CACHE_TTL_MS = 30_000;

export type ConnectorAuthType =
  | "none"
  | "envToken"
  | "oauth"
  | "apiKey"
  | "externalMcp";
export type ConnectorConnectionStatus =
  | "connected"
  | "needs_auth"
  | "disabled"
  | "metadata_only";
export type ConnectorDefinitionSource = "built_in" | "custom" | "plugin";
export type ConnectorToolExecution = "native" | "declared" | "external_mcp";
export type ConnectorAuditAccess = "read" | "write";
export type ConnectorAuditOutcome = "ok" | "error" | "blocked";

export interface ConnectorToolDefinition {
  name: string;
  description: string;
  readOnly: boolean;
  requiredScopes: string[];
  confirmationRequired: boolean;
  execution: ConnectorToolExecution;
}

export interface ConnectorDefinition {
  id: string;
  name: string;
  description: string;
  category: string;
  authType: ConnectorAuthType;
  envVars: string[];
  installUrl?: string | null;
  docsUrl?: string | null;
  defaultEnabled: boolean;
  tools: ConnectorToolDefinition[];
}

export interface ConnectorInfo {
  definition: ConnectorDefinition;
  enabled: boolean;
  connected: boolean;
  accessible: boolean;
  status: ConnectorConnectionStatus;
  accountLabel?: string | null;
  authSource?: string | null;
  connectedAt?: string | null;
  envConfigured: boolean;
  referencedByPlugins: string[];
  source: ConnectorDefinitionSource;
  lastConnectionTest?: ConnectorConnectionTestResult | null;
  connectionTestHistory: ConnectorConnectionTestResult[];
  connectionHealth: ConnectorHealthSummary;
}

export interface ConnectorCatalog {
  connectors: ConnectorInfo[];
  scope: string;
  configPath: string;
  notes: string[];
}

export type ConnectorConnectionTestKind = "local_state" | "native_api";

export interface ConnectorConnectionTestResult {
  connectorId: string;
  connectorName: string;
  ok: boolean;
  status: ConnectorConnectionStatus;
  checkKind: ConnectorConnectionTestKind;
  message: string;
  checkedAt: string;
  accountLabel?: string | null;
  authSource?: string | null;
  httpStatus?: number | null;
  retryable: boolean;
  errorCode?: string | null;
  details?: string | null;
}

export interface ConnectorHealthSummary {
  totalChecks: number;
  okChecks: number;
  failedChecks: number;
  retryableFailures: number;
  lastOkAt?: string | null;
  lastFailureAt?: string | null;
  lastErrorCode?: string | null;
  lastHttpStatus?: number | null;
}

export interface ConnectorAuditEvent {
  id: string;
  connectorId: string;
  operation: string;
  access: ConnectorAuditAccess;
  confirmationRequired: boolean;
  confirmed: boolean;
  target?: string | null;
  sessionId?: string | null;
  projectRoot?: string | null;
  outcome: ConnectorAuditOutcome;
  errorCode?: string | null;
  message?: string | null;
  createdAt: string;
}

export interface ConnectorLoginStartResult {
  connectorId: string;
  connectorName: string;
  provider: string;
  loginSessionId: string;
  verificationUri: string;
  verificationUriComplete?: string | null;
  userCode: string;
  expiresIn: number;
  intervalSecs: number;
  expiresAt: string;
  message: string;
}

export type ConnectorLoginPollStatus =
  | "pending"
  | "slow_down"
  | "complete"
  | "expired"
  | "denied"
  | "error";

export interface ConnectorLoginPollResult {
  connectorId: string;
  provider: string;
  status: ConnectorLoginPollStatus;
  message: string;
  intervalSecs: number;
  connector?: ConnectorInfo | null;
}

export interface MailConnectorCredentialRequest {
  connectorId: string;
  emailAddress: string;
  authorizationCode: string;
}

export interface CustomConnectorRequest {
  id: string;
  name: string;
  description: string;
  category: string;
  authType: ConnectorAuthType;
  envVars: string[];
  installUrl?: string | null;
  docsUrl?: string | null;
  defaultEnabled: boolean;
  tools: ConnectorToolDefinition[];
}

export interface CustomConnectorExport {
  version: number;
  scope: string;
  connectors: ConnectorDefinition[];
}

export interface LoadConnectorsOptions {
  force?: boolean;
  background?: boolean;
}

interface ConnectorState {
  catalog: ConnectorCatalog | null;
  auditEvents: ConnectorAuditEvent[];
  isLoading: boolean;
  isMutating: boolean;
  testingConnectorIds: Record<string, boolean>;
  testResults: Record<string, ConnectorConnectionTestResult>;
  error: string | null;
  loadedAt: number | null;
  loadConnectors: (options?: LoadConnectorsOptions) => Promise<void>;
  loadConnectorAuditEvents: (connectorId?: string | null) => Promise<void>;
  setConnectorEnabled: (connectorId: string, enabled: boolean) => Promise<void>;
  connectConnector: (
    connectorId: string,
    options?: { accountLabel?: string; authSource?: string },
  ) => Promise<void>;
  saveMailConnectorCredentials: (
    request: MailConnectorCredentialRequest,
  ) => Promise<ConnectorInfo>;
  disconnectConnector: (connectorId: string) => Promise<void>;
  testConnectorConnection: (
    connectorId: string,
    projectRoot?: string | null,
  ) => Promise<ConnectorConnectionTestResult>;
  startConnectorLogin: (
    connectorId: string,
  ) => Promise<ConnectorLoginStartResult>;
  pollConnectorLogin: (
    loginSessionId: string,
  ) => Promise<ConnectorLoginPollResult>;
  upsertCustomConnector: (request: CustomConnectorRequest) => Promise<void>;
  deleteCustomConnector: (connectorId: string) => Promise<void>;
  exportCustomConnectors: () => Promise<CustomConnectorExport>;
  importCustomConnectors: (
    connectors: CustomConnectorRequest[],
    replaceExisting: boolean,
  ) => Promise<void>;
}

function replaceConnector(
  catalog: ConnectorCatalog | null,
  connector: ConnectorInfo,
): ConnectorCatalog | null {
  if (!catalog) return catalog;
  return {
    ...catalog,
    connectors: catalog.connectors.map((item) =>
      item.definition.id === connector.definition.id ? connector : item,
    ),
  };
}

function removeTestResult(
  results: Record<string, ConnectorConnectionTestResult>,
  connectorId: string,
): Record<string, ConnectorConnectionTestResult> {
  const next = { ...results };
  delete next[connectorId];
  return next;
}

function collectLastTestResults(
  catalog: ConnectorCatalog,
): Record<string, ConnectorConnectionTestResult> {
  return catalog.connectors.reduce<
    Record<string, ConnectorConnectionTestResult>
  >((results, connector) => {
    const latest =
      connector.lastConnectionTest ??
      connector.connectionTestHistory?.[0] ??
      null;
    if (latest) {
      results[connector.definition.id] = latest;
    }
    return results;
  }, {});
}

function summarizeConnectionHistory(
  history: ConnectorConnectionTestResult[],
): ConnectorHealthSummary {
  const okChecks = history.filter((result) => result.ok).length;
  const failedChecks = history.length - okChecks;
  const retryableFailures = history.filter(
    (result) => !result.ok && result.retryable,
  ).length;
  const lastOk = history.find((result) => result.ok);
  const lastFailure = history.find((result) => !result.ok);

  return {
    totalChecks: history.length,
    okChecks,
    failedChecks,
    retryableFailures,
    lastOkAt: lastOk?.checkedAt ?? null,
    lastFailureAt: lastFailure?.checkedAt ?? null,
    lastErrorCode: lastFailure?.errorCode ?? null,
    lastHttpStatus: lastFailure?.httpStatus ?? null,
  };
}

function replaceConnectorLastConnectionTest(
  catalog: ConnectorCatalog | null,
  connectorId: string,
  result: ConnectorConnectionTestResult,
): ConnectorCatalog | null {
  if (!catalog) return catalog;
  return {
    ...catalog,
    connectors: catalog.connectors.map((connector) =>
      connector.definition.id === connectorId
        ? (() => {
            const previousHistory = connector.connectionTestHistory ?? [];
            const connectionTestHistory = [
              result,
              ...previousHistory.filter(
                (item) =>
                  item.connectorId !== result.connectorId ||
                  item.checkedAt !== result.checkedAt,
              ),
            ].slice(0, CONNECTOR_TEST_HISTORY_LIMIT);
            return {
              ...connector,
              lastConnectionTest: result,
              connectionTestHistory,
              connectionHealth: summarizeConnectionHistory(
                connectionTestHistory,
              ),
            };
          })()
        : connector,
    ),
  };
}

export const useConnectorStore = create<ConnectorState>((set, get) => ({
  catalog: null,
  auditEvents: [],
  isLoading: false,
  isMutating: false,
  testingConnectorIds: {},
  testResults: {},
  error: null,
  loadedAt: null,

  loadConnectors: async (options = {}) => {
    const { force = false, background = false } = options;
    const state = get();
    const now = Date.now();
    if (
      !force &&
      (state.isLoading ||
        (state.catalog &&
          state.loadedAt &&
          now - state.loadedAt < CONNECTOR_CATALOG_CACHE_TTL_MS))
    ) {
      if (state.error) set({ error: null });
      return;
    }

    if (background) {
      set({ error: null });
    } else {
      set({ isLoading: true, error: null });
    }
    try {
      const catalog = await invoke<ConnectorCatalog>("list_omiga_connectors");
      set({
        catalog,
        testResults: collectLastTestResults(catalog),
        isLoading: false,
        loadedAt: Date.now(),
      });
    } catch (e) {
      set({ isLoading: false, error: extractErrorMessage(e) });
    }
  },

  loadConnectorAuditEvents: async (connectorId) => {
    set({ error: null });
    try {
      const auditEvents = await invoke<ConnectorAuditEvent[]>(
        "list_omiga_connector_audit_events",
        {
          connectorId: connectorId ?? null,
          limit: 100,
        },
      );
      set((state) => ({
        auditEvents: connectorId
          ? [
              ...auditEvents,
              ...state.auditEvents.filter(
                (event) => event.connectorId !== connectorId,
              ),
            ]
          : auditEvents,
      }));
    } catch (e) {
      const error = extractErrorMessage(e);
      set({ error });
      throw new Error(error);
    }
  },

  setConnectorEnabled: async (connectorId, enabled) => {
    set({ isMutating: true, error: null });
    try {
      const connector = await invoke<ConnectorInfo>(
        "set_omiga_connector_enabled",
        {
          connectorId,
          enabled,
        },
      );
      set((state) => ({
        catalog: replaceConnector(state.catalog, connector),
        testResults: removeTestResult(state.testResults, connectorId),
        isMutating: false,
      }));
    } catch (e) {
      const error = extractErrorMessage(e);
      set({ isMutating: false, error });
      throw new Error(error);
    }
  },

  connectConnector: async (connectorId, options) => {
    set({ isMutating: true, error: null });
    try {
      const connector = await invoke<ConnectorInfo>("connect_omiga_connector", {
        request: {
          connectorId,
          accountLabel: options?.accountLabel,
          authSource: options?.authSource ?? "manual",
        },
      });
      set((state) => ({
        catalog: replaceConnector(state.catalog, connector),
        testResults: removeTestResult(state.testResults, connectorId),
        isMutating: false,
      }));
    } catch (e) {
      const error = extractErrorMessage(e);
      set({ isMutating: false, error });
      throw new Error(error);
    }
  },

  saveMailConnectorCredentials: async (request) => {
    set({ isMutating: true, error: null });
    try {
      const connector = await invoke<ConnectorInfo>(
        "save_omiga_mail_connector_credentials",
        {
          request,
        },
      );
      set((state) => ({
        catalog: replaceConnector(state.catalog, connector),
        testResults: removeTestResult(state.testResults, request.connectorId),
        isMutating: false,
      }));
      return connector;
    } catch (e) {
      const error = extractErrorMessage(e);
      set({ isMutating: false, error });
      throw new Error(error);
    }
  },

  disconnectConnector: async (connectorId) => {
    set({ isMutating: true, error: null });
    try {
      const connector = await invoke<ConnectorInfo>(
        "disconnect_omiga_connector",
        {
          connectorId,
        },
      );
      set((state) => ({
        catalog: replaceConnector(state.catalog, connector),
        testResults: removeTestResult(state.testResults, connectorId),
        isMutating: false,
      }));
    } catch (e) {
      const error = extractErrorMessage(e);
      set({ isMutating: false, error });
      throw new Error(error);
    }
  },

  testConnectorConnection: async (connectorId, projectRoot) => {
    set((state) => ({
      testingConnectorIds: {
        ...state.testingConnectorIds,
        [connectorId]: true,
      },
      error: null,
    }));
    try {
      const result = await invoke<ConnectorConnectionTestResult>(
        "test_omiga_connector_connection",
        { connectorId, projectRoot: projectRoot ?? null },
      );
      set((state) => ({
        testingConnectorIds: {
          ...state.testingConnectorIds,
          [connectorId]: false,
        },
        catalog: replaceConnectorLastConnectionTest(
          state.catalog,
          connectorId,
          result,
        ),
        testResults: { ...state.testResults, [connectorId]: result },
      }));
      return result;
    } catch (e) {
      const error = extractErrorMessage(e);
      set((state) => ({
        testingConnectorIds: {
          ...state.testingConnectorIds,
          [connectorId]: false,
        },
        error,
      }));
      throw new Error(error);
    }
  },

  startConnectorLogin: async (connectorId) => {
    set({ isMutating: true, error: null });
    try {
      const result = await invoke<ConnectorLoginStartResult>(
        "start_omiga_connector_login",
        { connectorId },
      );
      set({ isMutating: false });
      return result;
    } catch (e) {
      const error = extractErrorMessage(e);
      set({ isMutating: false });
      throw new Error(error);
    }
  },

  pollConnectorLogin: async (loginSessionId) => {
    set({ error: null });
    try {
      const result = await invoke<ConnectorLoginPollResult>(
        "poll_omiga_connector_login",
        { loginSessionId },
      );
      set((state) => ({
        catalog: result.connector
          ? replaceConnector(state.catalog, result.connector)
          : state.catalog,
        testResults: result.connector
          ? removeTestResult(state.testResults, result.connector.definition.id)
          : state.testResults,
      }));
      return result;
    } catch (e) {
      const error = extractErrorMessage(e);
      throw new Error(error);
    }
  },

  upsertCustomConnector: async (request) => {
    set({ isMutating: true, error: null });
    try {
      const catalog = await invoke<ConnectorCatalog>(
        "upsert_omiga_custom_connector",
        {
          request,
        },
      );
      set(() => ({
        catalog,
        testResults: collectLastTestResults(catalog),
        isMutating: false,
        loadedAt: Date.now(),
      }));
    } catch (e) {
      const error = extractErrorMessage(e);
      set({ isMutating: false, error });
      throw new Error(error);
    }
  },

  deleteCustomConnector: async (connectorId) => {
    set({ isMutating: true, error: null });
    try {
      const catalog = await invoke<ConnectorCatalog>(
        "delete_omiga_custom_connector",
        {
          connectorId,
        },
      );
      set(() => ({
        catalog,
        testResults: collectLastTestResults(catalog),
        isMutating: false,
        loadedAt: Date.now(),
      }));
    } catch (e) {
      const error = extractErrorMessage(e);
      set({ isMutating: false, error });
      throw new Error(error);
    }
  },

  exportCustomConnectors: async () => {
    set({ error: null });
    try {
      return await invoke<CustomConnectorExport>(
        "export_omiga_custom_connectors",
      );
    } catch (e) {
      const error = extractErrorMessage(e);
      set({ error });
      throw new Error(error);
    }
  },

  importCustomConnectors: async (connectors, replaceExisting) => {
    set({ isMutating: true, error: null });
    try {
      const catalog = await invoke<ConnectorCatalog>(
        "import_omiga_custom_connectors",
        {
          request: {
            connectors,
            replaceExisting,
          },
        },
      );
      set({
        catalog,
        testResults: collectLastTestResults(catalog),
        isMutating: false,
        loadedAt: Date.now(),
      });
    } catch (e) {
      const error = extractErrorMessage(e);
      set({ isMutating: false, error });
      throw new Error(error);
    }
  },
}));
