import type { ReactNode } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, beforeEach, vi } from "vitest";

const PROJECT_PATH = "/tmp/omiga-project";

const storeMock = vi.hoisted(() => ({
  state: {
    catalog: null as { items?: Array<{ kind: string; connected?: boolean; status?: string }> } | null,
    isLoading: false,
    error: null as string | null,
  },
  loadCatalog: vi.fn<(...args: unknown[]) => Promise<null>>(),
  clearError: vi.fn(),
}));

const buttonMock = vi.hoisted(() => ({
  refreshOnClick: null as null | (() => void),
}));

vi.mock("react", async () => {
  const actual = await vi.importActual<typeof import("react")>("react");
  return {
    ...actual,
    useEffect: (effect: () => void | (() => void)) => {
      effect();
    },
  };
});

vi.mock("@mui/material/styles", async () => {
  const React = await vi.importActual<typeof import("react")>("react");
  const theme = {
    palette: {
      mode: "light",
      background: { paper: "#fff" },
      info: { main: "#1976d2", light: "#42a5f5" },
    },
  };

  return {
    alpha: () => "transparent",
    createTheme: () => theme,
    ThemeProvider: ({ children }: { children: ReactNode }) =>
      React.createElement(React.Fragment, null, children),
    useTheme: () => theme,
  };
});

vi.mock("@mui/icons-material", async () => {
  const React = await vi.importActual<typeof import("react")>("react");
  const icon = ({ name }: { name: string }) =>
    React.createElement("span", { "data-icon": name }, name);

  return {
    LinkRounded: () => icon({ name: "LinkRounded" }),
    RefreshRounded: () => icon({ name: "RefreshRounded" }),
    SearchRounded: () => icon({ name: "SearchRounded" }),
    SettingsEthernetRounded: () => icon({ name: "SettingsEthernetRounded" }),
    SyncRounded: () => icon({ name: "SyncRounded" }),
  };
});

vi.mock("@mui/material", async () => {
  const React = await vi.importActual<typeof import("react")>("react");

  function flattenText(node: ReactNode): string {
    if (typeof node === "string" || typeof node === "number") {
      return String(node);
    }
    if (Array.isArray(node)) {
      return node.map(flattenText).join("");
    }
    if (node && typeof node === "object" && "props" in node) {
      return flattenText((node as { props?: { children?: ReactNode } }).props?.children);
    }
    return "";
  }

  function passthrough(tag: string) {
    return ({ children }: { children?: ReactNode }) =>
      React.createElement(tag, null, children);
  }

  return {
    Alert: passthrough("div"),
    Box: passthrough("div"),
    Button: ({
      children,
      onClick,
    }: {
      children?: ReactNode;
      onClick?: () => void;
    }) => {
      if (flattenText(children).includes("Refresh")) {
        buttonMock.refreshOnClick = onClick ?? null;
      }
      return React.createElement("button", { type: "button" }, children);
    },
    Chip: ({ label }: { label?: ReactNode }) =>
      React.createElement("span", null, label),
    CircularProgress: () => React.createElement("span", null, "loading"),
    Divider: () => React.createElement("hr"),
    Paper: passthrough("section"),
    Stack: passthrough("div"),
    TextField: ({ placeholder }: { placeholder?: string }) =>
      React.createElement("input", { placeholder }),
    ToggleButton: passthrough("button"),
    ToggleButtonGroup: passthrough("div"),
    Typography: passthrough("div"),
  };
});

vi.mock("../../state/externalIntegrationStore", () => ({
  useExternalIntegrationStore: () => ({
    catalog: storeMock.state.catalog,
    isLoading: storeMock.state.isLoading,
    error: storeMock.state.error,
    loadCatalog: storeMock.loadCatalog,
    clearError: storeMock.clearError,
  }),
  selectConnectorLikeItems: (catalog: { items?: Array<{ kind: string }> } | null) =>
    catalog?.items?.filter((item) => item.kind !== "mcp_server") ?? [],
  selectMcpServerRows: (catalog: { items?: Array<{ kind: string }> } | null) =>
    catalog?.items?.filter((item) => item.kind === "mcp_server") ?? [],
  selectExternalIntegrationSummary: (
    catalog: {
      items?: Array<{ kind: string; connected?: boolean; status?: string }>;
    } | null,
  ) => {
    const items = catalog?.items ?? [];
    const connectorLikeItems = items.filter((item) => item.kind !== "mcp_server");
    const mcpItems = items.filter((item) => item.kind === "mcp_server");

    return {
      totalItems: items.length,
      connectorLikeCount: connectorLikeItems.length,
      mcpServerCount: mcpItems.length,
      connectedCount: items.filter(
        (item) => item.connected || item.status === "connected",
      ).length,
      issueCount: 0,
    };
  },
  buildConnectorLikeBadges: () => [],
  buildMcpServerBadges: () => [],
  connectorStatusLabel: (status: string) => status,
}));

vi.mock("./ConnectorsPanel", () => ({
  ConnectorsPanel: ({ projectPath }: { projectPath: string }) => (
    <div data-testid="connectors-panel">Mock ConnectorsPanel: {projectPath}</div>
  ),
}));

vi.mock("./IntegrationsCatalogPanel", () => ({
  IntegrationsCatalogPanel: ({
    projectPath,
    mode,
  }: {
    projectPath: string;
    mode?: string;
  }) => (
    <div data-testid="integrations-catalog-panel">
      Mock IntegrationsCatalogPanel: {projectPath} [{mode ?? "default"}]
    </div>
  ),
}));

vi.mock("./ClaudeCodeImportPanel", () => ({
  ClaudeCodeImportPanel: ({
    projectPath,
    mode,
  }: {
    projectPath: string;
    mode?: string;
  }) => (
    <div data-testid="claude-code-import-panel">
      Mock ClaudeCodeImportPanel: {projectPath} [{mode ?? "default"}]
    </div>
  ),
}));

import { ExternalIntegrationsPanel } from "./ExternalIntegrationsPanel";

function renderPanel(initialView: "connectors" | "mcp"): string {
  return renderToStaticMarkup(
    <ExternalIntegrationsPanel projectPath={PROJECT_PATH} initialView={initialView} />,
  );
}

describe("ExternalIntegrationsPanel initial view", () => {
  beforeEach(() => {
    storeMock.state.catalog = null;
    storeMock.state.isLoading = false;
    storeMock.state.error = null;
    storeMock.loadCatalog.mockReset().mockResolvedValue(null);
    storeMock.clearError.mockReset();
    buttonMock.refreshOnClick = null;
  });

  it('renders the connectors-first view and loads the catalog without background probing', () => {
    const html = renderPanel("connectors");

    expect(html).toContain("Services");
    expect(html).toContain("Account and service links");
    expect(html).toContain(`Mock ConnectorsPanel: ${PROJECT_PATH}`);
    expect(html).not.toContain("MCP Servers");
    expect(html).not.toContain("Mock IntegrationsCatalogPanel");
    expect(html).not.toContain("Mock ClaudeCodeImportPanel");
    expect(html).not.toContain("导入已有 MCP JSON");
    expect(storeMock.loadCatalog).toHaveBeenCalledTimes(1);
    expect(storeMock.loadCatalog).toHaveBeenCalledWith(PROJECT_PATH, {
      background: false,
      probeTools: false,
    });
  });

  it("renders the MCP-first view and refreshes with force plus tool probing", () => {
    const html = renderPanel("mcp");

    expect(html).toContain("MCP Servers");
    expect(html).toContain(`Mock IntegrationsCatalogPanel: ${PROJECT_PATH} [mcp]`);
    expect(html).toContain(`Mock ClaudeCodeImportPanel: ${PROJECT_PATH} [mcp]`);
    expect(html).toContain("导入已有 MCP JSON");
    expect(html).not.toContain("Account and service links");
    expect(html).not.toContain("Mock ConnectorsPanel");
    expect(buttonMock.refreshOnClick).toBeTypeOf("function");

    storeMock.loadCatalog.mockClear();
    buttonMock.refreshOnClick?.();

    expect(storeMock.loadCatalog).toHaveBeenCalledTimes(1);
    expect(storeMock.loadCatalog).toHaveBeenCalledWith(PROJECT_PATH, {
      force: true,
      probeTools: true,
    });
  });
});
