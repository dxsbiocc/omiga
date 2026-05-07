export type PermissionArgs = Record<string, unknown> | undefined;

export type ConnectorPermissionIntent = {
  connectorId: string;
  connectorLabel: string;
  operation: string;
  operationLabel: string;
  isWrite: boolean;
  target?: string;
  payloadPreview?: string;
};

function firstString(v: unknown): string | null {
  if (typeof v === "string" && v.trim()) return v;
  return null;
}

function firstNumber(v: unknown): number | null {
  return typeof v === "number" && Number.isFinite(v) ? v : null;
}

function normalizeConnectorToken(value: string): string {
  return value.trim().toLowerCase().replace(/[\s-]+/g, "_");
}

function connectorArgs(args: PermissionArgs): PermissionArgs {
  const maybeTool = firstString(args?.tool);
  const nested = args?.arguments;
  if (
    maybeTool?.toLowerCase() === "connector" &&
    nested &&
    typeof nested === "object" &&
    !Array.isArray(nested)
  ) {
    return nested as Record<string, unknown>;
  }
  return args;
}

function connectorStringField(
  args: PermissionArgs,
  names: string[],
): string | null {
  if (!args) return null;
  for (const name of names) {
    const value = firstString(args[name]);
    if (value) return value.trim();
  }
  return null;
}

function connectorNumberField(
  args: PermissionArgs,
  names: string[],
): number | null {
  if (!args) return null;
  for (const name of names) {
    const value = firstNumber(args[name]);
    if (value !== null) return value;
  }
  return null;
}

const CONNECTOR_LABELS: Record<string, string> = {
  asana: "Asana",
  confluence: "Confluence",
  discord: "Discord",
  github: "GitHub",
  gitlab: "GitLab",
  google_sheets: "Google Sheets",
  jira: "Jira",
  linear: "Linear",
  microsoft_teams: "Microsoft Teams",
  gmail: "Gmail",
  netease_mail: "网易邮箱",
  notion: "Notion",
  outlook: "Outlook",
  qq_mail: "QQ 邮箱",
  sentry: "Sentry",
  slack: "Slack",
  trello: "Trello",
};

function connectorLabel(connectorId: string): string {
  return CONNECTOR_LABELS[connectorId] ?? connectorId.replace(/_/g, " ");
}

function canonicalConnectorOperation(
  connectorId: string,
  operation: string,
): string {
  if (connectorId === "slack") {
    if (operation === "send_message" || operation === "reply") {
      return "post_message";
    }
    if (
      operation === "thread" ||
      operation === "replies" ||
      operation === "conversation_replies"
    ) {
      return "read_thread";
    }
  }
  if (connectorId === "github") {
    if (operation === "issues") return "list_issues";
    if (operation === "get_issue" || operation === "issue") return "read_issue";
    if (
      operation === "list_pulls" ||
      operation === "pull_requests" ||
      operation === "prs"
    ) {
      return "list_pull_requests";
    }
    if (
      operation === "get_pull_request" ||
      operation === "read_pr" ||
      operation === "get_pr" ||
      operation === "pr"
    ) {
      return "read_pull_request";
    }
  }
  if (connectorId === "gitlab") {
    if (operation === "issues") return "list_issues";
    if (operation === "get_issue" || operation === "issue") return "read_issue";
    if (
      operation === "list_mrs" ||
      operation === "merge_requests" ||
      operation === "mrs"
    ) {
      return "list_merge_requests";
    }
    if (
      operation === "get_merge_request" ||
      operation === "read_mr" ||
      operation === "get_mr" ||
      operation === "mr"
    ) {
      return "read_merge_request";
    }
  }
  if (connectorId === "notion") {
    if (
      operation === "search" ||
      operation === "search_page" ||
      operation === "pages"
    ) {
      return "search_pages";
    }
    if (operation === "get_page" || operation === "page") return "read_page";
  }
  if (connectorId === "sentry") {
    if (operation === "issues") return "list_issues";
    if (operation === "get_issue" || operation === "issue") return "read_issue";
  }
  if (
    connectorId === "gmail" ||
    connectorId === "outlook" ||
    connectorId === "qq_mail" ||
    connectorId === "netease_mail"
  ) {
    if (
      operation === "search" ||
      operation === "messages" ||
      operation === "list_messages" ||
      operation === "mail" ||
      operation === "emails"
    ) {
      return "search_messages";
    }
    if (
      operation === "get_message" ||
      operation === "message" ||
      operation === "read_email" ||
      operation === "email"
    ) {
      return "read_message";
    }
    if (
      operation === "send" ||
      operation === "send_email" ||
      operation === "compose"
    ) {
      return "send_message";
    }
  }
  return operation;
}

function connectorOperationLabel(operation: string): string {
  const labels: Record<string, string> = {
    create_issue: "创建 Issue",
    create_task: "创建任务",
    delete_message: "删除消息",
    list_issues: "列出 Issues",
    list_merge_requests: "列出 Merge Requests",
    list_pull_requests: "列出 Pull Requests",
    move_card: "移动卡片",
    post_message: "发送消息",
    publish_changes: "发布变更",
    read_issue: "读取 Issue",
    read_message: "读取邮件",
    read_page: "读取页面",
    read_pull_request: "读取 Pull Request",
    read_thread: "读取会话线程",
    resolve_issue: "解决事件/问题",
    search_messages: "搜索邮件",
    search_pages: "搜索页面",
    send_message: "发送邮件",
    transition_issue: "流转 Issue 状态",
    update_issue_status: "更新 Issue 状态",
    update_task: "更新任务",
    update_values: "更新表格数据",
  };
  if (labels[operation]) return labels[operation];
  return operation
    .split("_")
    .filter(Boolean)
    .map((part) => part[0]?.toUpperCase() + part.slice(1))
    .join(" ");
}

function isConnectorWriteOperation(
  connectorId: string,
  operation: string,
): boolean {
  if (
    [
      "create_",
      "delete_",
      "post_",
      "publish_",
      "resolve_",
      "send_",
      "transition_",
      "update_",
    ].some((prefix) => operation.startsWith(prefix))
  ) {
    return true;
  }
  return (
    (connectorId === "slack" && operation === "post_message") ||
    (connectorId === "discord" && operation === "post_message") ||
    (connectorId === "microsoft_teams" && operation === "post_message") ||
    (connectorId === "linear" && operation === "update_issue_status") ||
    (connectorId === "jira" && operation === "transition_issue") ||
    (connectorId === "sentry" && operation === "resolve_issue") ||
    (connectorId === "google_sheets" && operation === "update_values") ||
    (connectorId === "asana" && operation === "update_task") ||
    (connectorId === "trello" && operation === "move_card")
  );
}

function connectorTarget(
  connectorId: string,
  operation: string,
  args: PermissionArgs,
): string | null {
  if (!args) return null;
  if (connectorId === "github" || connectorId === "gitlab") {
    const repo = connectorStringField(args, ["repo", "repository"]);
    if (!repo) return null;
    const number = connectorNumberField(args, [
      "number",
      "issue",
      "issue_number",
      "pr",
    ]);
    return number ? `${repo}#${number}` : repo;
  }
  if (connectorId === "linear") {
    return (
      connectorStringField(args, ["id", "key", "identifier"]) ??
      connectorNumberField(args, ["number"])?.toString() ??
      null
    );
  }
  if (connectorId === "notion") {
    const id = connectorStringField(args, ["id", "page_id"]);
    if (id) return id;
    const query = connectorStringField(args, ["query", "search", "term"]);
    return query ? `search:${query}` : null;
  }
  if (connectorId === "sentry") {
    const repo = connectorStringField(args, ["repo"]);
    if (repo) return repo;
    const org = connectorStringField(args, ["org", "organization"]);
    const project = connectorStringField(args, ["project", "project_slug"]);
    if (org && project) return `${org}/${project}`;
    return connectorStringField(args, ["id", "issue_id"]);
  }
  if (connectorId === "slack") {
    const channel = connectorStringField(args, ["channel", "repo"]);
    if (!channel) return null;
    const threadTs = connectorStringField(args, [
      "thread_ts",
      "threadTs",
      "thread",
      "id",
    ]);
    if (
      threadTs &&
      (operation === "post_message" || operation === "read_thread")
    ) {
      return `${channel} thread ${threadTs}`;
    }
    return channel;
  }
  if (
    connectorId === "gmail" ||
    connectorId === "outlook" ||
    connectorId === "qq_mail" ||
    connectorId === "netease_mail"
  ) {
    const messageId = connectorStringField(args, [
      "id",
      "message_id",
      "messageId",
      "thread_id",
      "threadId",
    ]);
    if (messageId) return messageId;
    const recipient = connectorStringField(args, ["to", "recipient", "email"]);
    if (recipient) return recipient;
    const query = connectorStringField(args, ["query", "search", "subject"]);
    if (query) return `search:${query}`;
    return connectorStringField(args, ["folder", "mailbox"]);
  }
  return connectorStringField(args, ["id", "target", "channel", "repo", "key"]);
}

function connectorPayloadPreview(args: PermissionArgs): string | null {
  return connectorStringField(args, [
    "subject",
    "text",
    "message",
    "body",
    "content",
    "title",
    "status",
  ]);
}

export function inferConnectorPermissionIntent(
  toolNameRaw: string,
  argsRaw: PermissionArgs,
): ConnectorPermissionIntent | null {
  const toolName = (toolNameRaw || "").trim().toLowerCase();
  const args = connectorArgs(argsRaw);
  const rawNestedTool = firstString(argsRaw?.tool)?.trim().toLowerCase();
  const toolMatches = toolName === "connector" || rawNestedTool === "connector";
  if (!toolMatches || !args) return null;

  const connectorIdRaw = connectorStringField(args, [
    "connector",
    "connectorId",
  ]);
  const operationRaw = connectorStringField(args, ["operation", "tool"]);
  if (!connectorIdRaw || !operationRaw) return null;

  const connectorId = normalizeConnectorToken(connectorIdRaw);
  const operation = canonicalConnectorOperation(
    connectorId,
    normalizeConnectorToken(operationRaw),
  );

  return {
    connectorId,
    connectorLabel: connectorLabel(connectorId),
    operation,
    operationLabel: connectorOperationLabel(operation),
    isWrite: isConnectorWriteOperation(connectorId, operation),
    target: connectorTarget(connectorId, operation, args) ?? undefined,
    payloadPreview: connectorPayloadPreview(args) ?? undefined,
  };
}
