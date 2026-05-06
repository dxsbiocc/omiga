import { describe, expect, it } from "vitest";
import {
  connectorIsProductIntegrated,
  connectorLoginFailureMessage,
} from "./ConnectorsPanel";
import type { ConnectorInfo, ConnectorToolDefinition } from "../../state/connectorStore";

function tool(
  overrides: Partial<ConnectorToolDefinition> = {},
): ConnectorToolDefinition {
  return {
    name: "read_issue",
    description: "Read an issue.",
    readOnly: true,
    requiredScopes: [],
    confirmationRequired: false,
    execution: "native",
    ...overrides,
  };
}

function connector(overrides: Partial<ConnectorInfo> = {}): ConnectorInfo {
  const definition = {
    id: "github",
    name: "GitHub",
    description: "Inspect repositories.",
    category: "coding",
    authType: "oauth" as const,
    envVars: [],
    defaultEnabled: true,
    tools: [tool()],
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

describe("connector product integration state", () => {
  it("treats connectors with first-party login plus native tools as integrated", () => {
    expect(connectorIsProductIntegrated(connector())).toBe(true);
  });

  it("greys out declared-only connectors even when metadata exists", () => {
    expect(
      connectorIsProductIntegrated(
        connector({
          definition: {
            id: "trello",
            name: "Trello",
            description: "Move cards.",
            category: "productivity",
            authType: "apiKey",
            envVars: ["TRELLO_TOKEN"],
            defaultEnabled: true,
            tools: [tool({ execution: "declared", name: "move_card", readOnly: false })],
          },
        }),
      ),
    ).toBe(false);
  });

  it("does not expose token-only native connectors as product-integrated UI", () => {
    expect(
      connectorIsProductIntegrated(
        connector({
          definition: {
            id: "gitlab",
            name: "GitLab",
            description: "Read merge requests.",
            category: "coding",
            authType: "envToken",
            envVars: ["GITLAB_TOKEN"],
            defaultEnabled: true,
            tools: [tool({ execution: "native", name: "read_merge_request" })],
          },
        }),
      ),
    ).toBe(false);
  });

  it("falls back to hosted Slack authorization without surfacing developer config", () => {
    const message = connectorLoginFailureMessage(
      connector({
        definition: {
          id: "slack",
          name: "Slack",
          description: "Read Slack threads.",
          category: "communication",
          authType: "oauth",
          envVars: ["SLACK_BOT_TOKEN"],
          defaultEnabled: true,
          tools: [tool({ name: "read_thread" })],
        },
      }),
      "Slack browser login requires OMIGA_SLACK_OAUTH_CLIENT_ID, OMIGA_SLACK_OAUTH_CLIENT_SECRET, and an HTTPS OMIGA_SLACK_OAUTH_REDIRECT_URI registered in Slack.",
      true,
    );

    expect(message).toContain("已为你打开登录授权页");
    expect(message).toContain("Codex/OpenAI 托管授权页");
    expect(message).toContain("不需要你配置 Client ID 或 Client Secret");
    expect(message).not.toContain("SLACK_BOT_TOKEN");
    expect(message).not.toContain("HTTPS callback bridge");
  });
});
