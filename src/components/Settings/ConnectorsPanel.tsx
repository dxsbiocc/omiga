import { type ChangeEvent, useEffect, useMemo, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  Alert,
  Box,
  Button,
  Card,
  CardContent,
  Paper,
  Chip,
  CircularProgress,
  Dialog,
  DialogActions,
  DialogContent,
  DialogTitle,
  FormControlLabel,
  IconButton,
  MenuItem,
  Stack,
  Switch,
  TextField,
  Tooltip,
  Typography,
} from "@mui/material";
import { alpha, useTheme } from "@mui/material/styles";
import {
  AddRounded,
  CheckCircleRounded,
  CloseRounded,
  CloudOffRounded,
  ContentCopyRounded,
  DeleteRounded,
  DownloadRounded,
  EditRounded,
  LinkRounded,
  OpenInNewRounded,
  RefreshRounded,
  TroubleshootRounded,
  UploadRounded,
} from "@mui/icons-material";
import {
  type ConnectorConnectionStatus,
  type ConnectorConnectionTestResult,
  type ConnectorDefinitionSource,
  type ConnectorAuditEvent,
  type ConnectorAuditOutcome,
  type ConnectorToolDefinition,
  type ConnectorToolExecution,
  type ConnectorHealthSummary,
  type ConnectorInfo,
  type ConnectorLoginPollResult,
  type ConnectorLoginStartResult,
  type ConnectorAuthType,
  type ConnectorDefinition,
  type CustomConnectorRequest,
  useConnectorStore,
} from "../../state/connectorStore";

interface CustomConnectorFormState {
  id: string;
  name: string;
  description: string;
  category: string;
  authType: ConnectorAuthType;
  envVars: string;
  installUrl: string;
  docsUrl: string;
  tools: string;
}

const emptyCustomConnectorForm: CustomConnectorFormState = {
  id: "",
  name: "",
  description: "",
  category: "custom",
  authType: "envToken",
  envVars: "",
  installUrl: "",
  docsUrl: "",
  tools: "",
};

const authTypeOptions: Array<{ value: ConnectorAuthType; label: string }> = [
  { value: "oauth", label: "Browser/software OAuth" },
  { value: "envToken", label: "Advanced credential" },
  { value: "apiKey", label: "External secret credential" },
  { value: "none", label: "No auth" },
  { value: "externalMcp", label: "External MCP/plugin" },
];

const connectorCardGridSx = {
  display: "grid",
  gridTemplateColumns: { xs: "1fr", lg: "repeat(2, minmax(0, 1fr))" },
  gap: 1,
};

type ConnectorStatusFilter = "all" | ConnectorConnectionStatus;
type ConnectorSourceFilter = "all" | ConnectorDefinitionSource;

function statusLabel(status: ConnectorConnectionStatus): string {
  switch (status) {
    case "connected":
      return "Connected";
    case "needs_auth":
      return "Needs auth";
    case "disabled":
      return "Disabled";
    case "metadata_only":
      return "Plugin reference";
    default:
      return status;
  }
}

function statusColor(
  status: ConnectorConnectionStatus,
): "success" | "warning" | "default" {
  switch (status) {
    case "connected":
      return "success";
    case "needs_auth":
      return "warning";
    default:
      return "default";
  }
}

function categoryLabel(value: string): string {
  return value
    .replace(/[-_]+/g, " ")
    .replace(/\b\w/g, (char) => char.toUpperCase());
}

function authHint(connector: ConnectorInfo): string {
  const auth = connector.definition.authType;
  if (connector.definition.id === "github") {
    return "Use GitHub login or GitHub CLI (gh auth login); env tokens are advanced fallbacks";
  }
  if (connector.definition.id === "notion") {
    return "Use Notion login/authorization; env tokens are advanced fallbacks";
  }
  if (connector.definition.id === "slack") {
    return "Use Slack login/authorization; env bot tokens are advanced fallbacks";
  }
  if (auth === "none") return "No authentication required";
  if (auth === "externalMcp")
    return "Declared by plugin; add a matching MCP/tool integration";
  if (connector.definition.envVars.length > 0) {
    return "Use the official connection page or software login; advanced credentials stay outside Omiga config";
  }
  if (auth === "oauth") return "Browser/software authorization";
  if (auth === "apiKey") return "Official authorization or external secret manager";
  return "External authorization required";
}

function testKindLabel(result: ConnectorConnectionTestResult): string {
  return result.checkKind === "native_api"
    ? "Live API check"
    : "Local state check";
}

function formatCheckedAt(value: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return date.toLocaleString();
}

function testDetailChips(result: ConnectorConnectionTestResult): Array<{
  key: string;
  label: string;
  color?: "default" | "warning" | "error";
}> {
  const chips: Array<{
    key: string;
    label: string;
    color?: "default" | "warning" | "error";
  }> = [];
  if (result.httpStatus) {
    chips.push({
      key: "http",
      label: `HTTP ${result.httpStatus}`,
      color: result.httpStatus >= 400 ? "error" : "default",
    });
  }
  if (result.retryable) {
    chips.push({ key: "retryable", label: "Retryable", color: "warning" });
  }
  if (result.errorCode) {
    chips.push({ key: "error", label: result.errorCode, color: "default" });
  }
  return chips;
}

function auditOutcomeLabel(outcome: ConnectorAuditOutcome): string {
  switch (outcome) {
    case "ok":
      return "成功";
    case "blocked":
      return "已拦截";
    case "error":
      return "失败";
    default:
      return outcome;
  }
}

function auditOutcomeColor(
  outcome: ConnectorAuditOutcome,
): "success" | "warning" | "error" | "default" {
  switch (outcome) {
    case "ok":
      return "success";
    case "blocked":
      return "warning";
    case "error":
      return "error";
    default:
      return "default";
  }
}

function connectionHealthColor(
  health?: ConnectorHealthSummary,
): "success" | "warning" | "error" | "default" {
  if (!health || health.totalChecks === 0) return "default";
  if (health.failedChecks === 0) return "success";
  if (health.okChecks > 0) return "warning";
  return "error";
}

function connectionHealthLabel(health?: ConnectorHealthSummary): string {
  if (!health || health.totalChecks === 0) return "No checks yet";
  return `Health ${health.okChecks}/${health.totalChecks} OK`;
}

function connectionHealthDetail(
  health?: ConnectorHealthSummary,
): string | null {
  if (!health || health.totalChecks === 0) return null;
  const parts = [
    `${health.failedChecks} failed`,
    health.retryableFailures > 0
      ? `${health.retryableFailures} retryable`
      : null,
    health.lastFailureAt
      ? `last failure ${formatCheckedAt(health.lastFailureAt)}`
      : null,
    health.lastErrorCode ?? null,
    health.lastHttpStatus ? `HTTP ${health.lastHttpStatus}` : null,
  ].filter(Boolean);
  return parts.length > 0 ? parts.join(" · ") : "All recent checks passed";
}

function sourceLabel(connector: ConnectorInfo): string {
  return sourceValueLabel(connector.source);
}

function sourceValueLabel(source: ConnectorDefinitionSource): string {
  switch (source) {
    case "custom":
      return "Custom";
    case "plugin":
      return "Plugin";
    default:
      return "Built-in";
  }
}

function connectorToForm(connector: ConnectorInfo): CustomConnectorFormState {
  return {
    id: connector.definition.id,
    name: connector.definition.name,
    description: connector.definition.description,
    category: connector.definition.category,
    authType: connector.definition.authType,
    envVars: connector.definition.envVars.join(", "),
    installUrl: connector.definition.installUrl ?? "",
    docsUrl: connector.definition.docsUrl ?? "",
    tools: connector.definition.tools
      .map(
        (tool) =>
          `${tool.name} | ${tool.description} | ${tool.readOnly ? "read" : "write"}`,
      )
      .join("\n"),
  };
}

function splitEnvVars(value: string): string[] {
  return value
    .split(/[,\s]+/)
    .map((item) => item.trim())
    .filter(Boolean);
}

function parseToolLines(value: string): CustomConnectorRequest["tools"] {
  return value
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean)
    .map((line) => {
      const [name = "", description = "", mode = "read"] = line
        .split("|")
        .map((part) => part.trim());
      return {
        name,
        description: description || "Custom connector operation.",
        readOnly: !/^write$/i.test(mode),
        requiredScopes: [],
        confirmationRequired: /^write$/i.test(mode),
        execution: "declared" as const,
      };
    })
    .filter((tool) => tool.name.length > 0);
}

function buildCustomConnectorRequest(
  form: CustomConnectorFormState,
): CustomConnectorRequest {
  return {
    id: form.id.trim(),
    name: form.name.trim(),
    description: form.description.trim(),
    category: form.category.trim() || "custom",
    authType: form.authType,
    envVars: splitEnvVars(form.envVars),
    installUrl: form.installUrl.trim() || null,
    docsUrl: form.docsUrl.trim() || null,
    defaultEnabled: true,
    tools: parseToolLines(form.tools),
  };
}

function connectorMatchesSearch(
  connector: ConnectorInfo,
  query: string,
): boolean {
  const normalizedQuery = query.trim().toLowerCase();
  if (!normalizedQuery) return true;
  const haystack = [
    connector.definition.id,
    connector.definition.name,
    connector.definition.description,
    connector.definition.category,
    connector.source,
    ...connector.definition.envVars,
    ...connector.definition.tools.map(
      (tool) => `${tool.name} ${tool.description}`,
    ),
  ]
    .join(" ")
    .toLowerCase();
  return haystack.includes(normalizedQuery);
}

function envSetupSnippet(connector: ConnectorInfo): string {
  const lines = [
    `# Omiga user-level connector: ${connector.definition.name}`,
    "# Paste real secret values in your shell or secret manager; Omiga stores only env var names.",
  ];
  if (connector.definition.id === "github") {
    lines.push(
      "# Preferred local software login: install GitHub CLI, then run `gh auth login`.",
      "# Omiga will reuse `gh auth token` automatically after you reconnect or refresh connectors.",
    );
  } else if (connector.definition.id === "notion") {
    lines.push(
      "# Preferred user flow: use the Notion login/authorization page opened from Omiga.",
      "# Advanced developer fallback only: run Omiga with a self-owned Notion OAuth integration.",
      "# Redirect URI for self-owned local OAuth: http://127.0.0.1:17654/connectors/notion/callback",
      'export OMIGA_NOTION_OAUTH_CLIENT_ID="<notion-oauth-client-id>"',
      'export OMIGA_NOTION_OAUTH_CLIENT_SECRET="<notion-oauth-client-secret>"',
      "# Optional if 17654 is occupied: export OMIGA_NOTION_OAUTH_CALLBACK_PORT=\"17655\"",
      "# Advanced fallback only: NOTION_TOKEN / NOTION_API_KEY",
    );
  } else if (connector.definition.id === "slack") {
    lines.push(
      "# Preferred user flow: use the Slack login/authorization page opened from Omiga.",
      "# Advanced developer fallback only: run Omiga with a self-owned Slack OAuth app.",
      "# Slack requires the self-owned registered redirect URL to be HTTPS.",
      "# That HTTPS bridge must preserve code/state and redirect to the local callback:",
      "#   http://127.0.0.1:17655/connectors/slack/callback",
      'export OMIGA_SLACK_OAUTH_CLIENT_ID="<slack-client-id>"',
      'export OMIGA_SLACK_OAUTH_CLIENT_SECRET="<slack-client-secret>"',
      'export OMIGA_SLACK_OAUTH_REDIRECT_URI="https://your-domain.example/omiga/slack/callback"',
      "# Optional: export OMIGA_SLACK_OAUTH_SCOPE=\"channels:read,channels:history,chat:write\"",
      "# Optional if 17655 is occupied: export OMIGA_SLACK_OAUTH_LOCAL_CALLBACK_PORT=\"17656\"",
      "# Advanced fallback only: SLACK_BOT_TOKEN",
    );
  }
  for (const envVar of connector.definition.envVars) {
    lines.push(
      `export ${envVar}="<paste-${envVar.toLowerCase().replace(/_/g, "-")}>"`,
    );
  }
  return `${lines.join("\n")}\n`;
}

function isConnectorAuthType(value: unknown): value is ConnectorAuthType {
  return (
    value === "none" ||
    value === "envToken" ||
    value === "oauth" ||
    value === "apiKey" ||
    value === "externalMcp"
  );
}

function toStringArray(value: unknown): string[] {
  return Array.isArray(value)
    ? value.map((item) => String(item).trim()).filter(Boolean)
    : [];
}

function toConnectorTools(value: unknown): CustomConnectorRequest["tools"] {
  if (!Array.isArray(value)) return [];
  return value
    .map((tool) => {
      const item = tool as Partial<ConnectorDefinition["tools"][number]>;
      return {
        name: String(item.name ?? "").trim(),
        description: String(item.description ?? "").trim(),
        readOnly: item.readOnly !== false,
        requiredScopes: Array.isArray(item.requiredScopes)
          ? item.requiredScopes
              .map((scope) => String(scope).trim())
              .filter(Boolean)
          : [],
        confirmationRequired:
          item.confirmationRequired === true || item.readOnly === false,
        execution: "declared" as ConnectorToolExecution,
      };
    })
    .filter((tool) => tool.name.length > 0);
}

function parseCustomConnectorImport(raw: string): CustomConnectorRequest[] {
  const parsed = JSON.parse(raw) as unknown;
  const connectors = Array.isArray(parsed)
    ? parsed
    : typeof parsed === "object" &&
        parsed !== null &&
        Array.isArray((parsed as { connectors?: unknown }).connectors)
      ? (parsed as { connectors: unknown[] }).connectors
      : null;

  if (!connectors) {
    throw new Error(
      "Import JSON must be an array or an object with a connectors array.",
    );
  }

  return connectors.map((value) => {
    const item = value as Partial<ConnectorDefinition>;
    return {
      id: String(item.id ?? "").trim(),
      name: String(item.name ?? "").trim(),
      description: String(item.description ?? "").trim(),
      category: String(item.category ?? "custom").trim(),
      authType: isConnectorAuthType(item.authType) ? item.authType : "envToken",
      envVars: toStringArray(item.envVars),
      installUrl: typeof item.installUrl === "string" ? item.installUrl : null,
      docsUrl: typeof item.docsUrl === "string" ? item.docsUrl : null,
      defaultEnabled: item.defaultEnabled !== false,
      tools: toConnectorTools(item.tools),
    };
  });
}

function CustomConnectorDialog({
  open,
  form,
  busy,
  editing,
  onChange,
  onClose,
  onSave,
}: {
  open: boolean;
  form: CustomConnectorFormState;
  busy: boolean;
  editing: boolean;
  onChange: (form: CustomConnectorFormState) => void;
  onClose: () => void;
  onSave: () => void;
}) {
  const setField =
    <K extends keyof CustomConnectorFormState>(field: K) =>
    (event: ChangeEvent<HTMLInputElement>) => {
      onChange({ ...form, [field]: event.target.value });
    };

  return (
    <Dialog
      open={open}
      onClose={busy ? undefined : onClose}
      maxWidth="md"
      fullWidth
    >
      <DialogTitle>
        {editing ? "Edit custom connector" : "Add custom connector"}
      </DialogTitle>
      <DialogContent>
        <Stack spacing={2} sx={{ pt: 1 }}>
          <Alert severity="info">
            Custom connectors are user-level metadata only. Omiga stores names,
            docs, env var names, and declared tool hints here; never paste
            secret values into this form.
          </Alert>
          <Stack direction={{ xs: "column", sm: "row" }} spacing={2}>
            <TextField
              label="Connector ID"
              value={form.id}
              onChange={setField("id")}
              disabled={editing || busy}
              required
              fullWidth
              helperText="Lowercase id; spaces become underscores. Built-in ids cannot be replaced."
            />
            <TextField
              label="Name"
              value={form.name}
              onChange={setField("name")}
              disabled={busy}
              required
              fullWidth
            />
          </Stack>
          <TextField
            label="Description"
            value={form.description}
            onChange={setField("description")}
            disabled={busy}
            required
            multiline
            minRows={2}
            fullWidth
          />
          <Stack direction={{ xs: "column", sm: "row" }} spacing={2}>
            <TextField
              label="Category"
              value={form.category}
              onChange={setField("category")}
              disabled={busy}
              fullWidth
            />
            <TextField
              label="Auth type"
              value={form.authType}
              onChange={setField("authType")}
              disabled={busy}
              select
              fullWidth
            >
              {authTypeOptions.map((option) => (
                <MenuItem key={option.value} value={option.value}>
                  {option.label}
                </MenuItem>
              ))}
            </TextField>
          </Stack>
          <TextField
            label="Environment variable names"
            value={form.envVars}
            onChange={setField("envVars")}
            disabled={busy || form.authType === "externalMcp"}
            fullWidth
            helperText="Comma or space separated, e.g. INTERNAL_DOCS_TOKEN. Secret values stay outside Omiga."
          />
          <Stack direction={{ xs: "column", sm: "row" }} spacing={2}>
            <TextField
              label="Connect docs URL"
              value={form.installUrl}
              onChange={setField("installUrl")}
              disabled={busy}
              fullWidth
            />
            <TextField
              label="API docs URL"
              value={form.docsUrl}
              onChange={setField("docsUrl")}
              disabled={busy}
              fullWidth
            />
          </Stack>
          <TextField
            label="Declared tools"
            value={form.tools}
            onChange={setField("tools")}
            disabled={busy}
            multiline
            minRows={3}
            fullWidth
            helperText="One per line: tool_name | description | read/write. These are hints until a native/MCP executor exists."
          />
        </Stack>
      </DialogContent>
      <DialogActions>
        <Button onClick={onClose} disabled={busy}>
          Cancel
        </Button>
        <Button
          variant="contained"
          onClick={onSave}
          disabled={
            busy ||
            !form.id.trim() ||
            !form.name.trim() ||
            !form.description.trim()
          }
        >
          {busy ? "Saving…" : "Save connector"}
        </Button>
      </DialogActions>
    </Dialog>
  );
}

function CustomConnectorImportDialog({
  open,
  value,
  replaceExisting,
  busy,
  onChange,
  onReplaceExistingChange,
  onClose,
  onImport,
}: {
  open: boolean;
  value: string;
  replaceExisting: boolean;
  busy: boolean;
  onChange: (value: string) => void;
  onReplaceExistingChange: (value: boolean) => void;
  onClose: () => void;
  onImport: () => void;
}) {
  return (
    <Dialog
      open={open}
      onClose={busy ? undefined : onClose}
      maxWidth="md"
      fullWidth
    >
      <DialogTitle>Import custom connectors</DialogTitle>
      <DialogContent>
        <Stack spacing={2} sx={{ pt: 1 }}>
          <Alert severity="warning">
            Import only connector metadata that you trust. Secret values are not
            expected here and should stay in environment variables or a
            dedicated secret manager.
          </Alert>
          <TextField
            label="Connector JSON"
            value={value}
            onChange={(event) => onChange(event.target.value)}
            disabled={busy}
            multiline
            minRows={12}
            fullWidth
            helperText="Paste either the exported object with a connectors array, or a raw array of connector definitions."
          />
          <FormControlLabel
            control={
              <Switch
                checked={replaceExisting}
                onChange={(event) =>
                  onReplaceExistingChange(event.target.checked)
                }
                disabled={busy}
              />
            }
            label="Replace existing custom connectors"
          />
        </Stack>
      </DialogContent>
      <DialogActions>
        <Button onClick={onClose} disabled={busy}>
          Cancel
        </Button>
        <Button
          variant="contained"
          onClick={onImport}
          disabled={busy || !value.trim()}
          startIcon={<UploadRounded />}
        >
          {busy ? "Importing…" : "Import"}
        </Button>
      </DialogActions>
    </Dialog>
  );
}

function ConnectorCard({
  connector,
  busy,
  testResult,
  onEnable,
  onOpenDetails,
}: {
  connector: ConnectorInfo;
  busy: boolean;
  testResult?: ConnectorConnectionTestResult;
  onEnable: (connector: ConnectorInfo, enabled: boolean) => void;
  onOpenDetails: (connector: ConnectorInfo) => void;
}) {
  const theme = useTheme();
  const isProductIntegrated = connectorIsProductIntegrated(connector);
  const isReady = connector.accessible;
  const needsAttention =
    isProductIntegrated &&
    connector.enabled &&
    (!connector.accessible || testResult?.ok === false);
  const tone = !isProductIntegrated
    ? theme.palette.text.disabled
    : isReady
      ? theme.palette.success.main
      : needsAttention
        ? theme.palette.warning.main
        : theme.palette.text.secondary;
  const subtitle = connector.definition.description;

  const openDetails = () => onOpenDetails(connector);

  return (
    <Paper
      variant="outlined"
      role="button"
      tabIndex={0}
      aria-label={`Open ${connector.definition.name} connector details`}
      aria-disabled={!isProductIntegrated}
      onClick={openDetails}
      onKeyDown={(event) => {
        if (event.key !== "Enter" && event.key !== " ") return;
        event.preventDefault();
        openDetails();
      }}
      sx={{
        px: 1.25,
        py: 1.15,
        minHeight: 72,
        borderRadius: 2.5,
        cursor: "pointer",
        display: "flex",
        alignItems: "center",
        gap: 1.25,
        bgcolor: !isProductIntegrated
          ? alpha(
              theme.palette.text.disabled,
              theme.palette.mode === "dark" ? 0.1 : 0.06,
            )
          : "background.paper",
        borderColor: needsAttention
          ? alpha(theme.palette.warning.main, 0.36)
          : !isProductIntegrated
            ? alpha(theme.palette.text.disabled, 0.22)
            : "transparent",
        boxShadow: "none",
        opacity: !isProductIntegrated ? 0.68 : 1,
        transition:
          "background-color 160ms ease, box-shadow 160ms ease, transform 160ms ease",
        "@media (prefers-reduced-motion: reduce)": {
          transition: "none",
        },
        "&:hover": {
          bgcolor: !isProductIntegrated
            ? alpha(
                theme.palette.text.disabled,
                theme.palette.mode === "dark" ? 0.12 : 0.08,
              )
            : "action.hover",
          boxShadow: !isProductIntegrated
            ? "none"
            : `0 8px 22px ${alpha(theme.palette.common.black, theme.palette.mode === "dark" ? 0.24 : 0.07)}`,
          transform: !isProductIntegrated ? "none" : "translateY(-1px)",
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
        {isReady ? (
          <CheckCircleRounded fontSize="small" />
        ) : !isProductIntegrated ? (
          <CloudOffRounded fontSize="small" />
        ) : (
          <LinkRounded fontSize="small" />
        )}
      </Box>

      <Box sx={{ minWidth: 0, flex: 1 }}>
        <Stack
          direction="row"
          spacing={0.75}
          alignItems="center"
          sx={{ minWidth: 0 }}
        >
          <Typography
            variant="subtitle2"
            fontWeight={800}
            noWrap
            title={connector.definition.name}
          >
            {connector.definition.name}
          </Typography>
          {connector.source !== "built_in" && (
            <Chip
              size="small"
              variant="outlined"
              label={sourceLabel(connector)}
              sx={{ height: 20, flexShrink: 0 }}
            />
          )}
          {!isProductIntegrated && (
            <Chip
              size="small"
              variant="outlined"
              label="未接入"
              sx={{
                height: 20,
                flexShrink: 0,
                color: "text.disabled",
                borderColor: "divider",
                bgcolor: "action.disabledBackground",
              }}
            />
          )}
        </Stack>
        <Typography
          variant="body2"
          color="text.secondary"
          noWrap
          title={subtitle}
          sx={{ mt: 0.15 }}
        >
          {subtitle}
        </Typography>
      </Box>

      {!isProductIntegrated ? (
        <Box
          aria-label={`${connector.definition.name} is not integrated yet`}
          title="未接入真实连接方式"
          sx={{
            width: 32,
            height: 32,
            borderRadius: "50%",
            display: "inline-flex",
            alignItems: "center",
            justifyContent: "center",
            flexShrink: 0,
            color: "text.disabled",
            bgcolor: "action.disabledBackground",
          }}
        >
          <CloudOffRounded fontSize="small" />
        </Box>
      ) : connector.enabled ? (
        <Box
          aria-label={`${connector.definition.name} is ${statusLabel(connector.status)}`}
          title={statusLabel(connector.status)}
          sx={{
            width: 32,
            height: 32,
            borderRadius: "50%",
            display: "inline-flex",
            alignItems: "center",
            justifyContent: "center",
            flexShrink: 0,
            color: isReady
              ? "success.main"
              : needsAttention
                ? "warning.main"
                : "text.disabled",
          }}
        >
          {isReady ? (
            <CheckCircleRounded fontSize="small" />
          ) : (
            <LinkRounded fontSize="small" />
          )}
        </Box>
      ) : (
        <IconButton
          aria-label={`Enable ${connector.definition.name}`}
          size="small"
          disabled={busy || !isProductIntegrated}
          onClick={(event) => {
            event.stopPropagation();
            onEnable(connector, true);
          }}
          onKeyDown={(event) => event.stopPropagation()}
          sx={{
            width: 34,
            height: 34,
            flexShrink: 0,
            bgcolor: alpha(
              theme.palette.text.primary,
              theme.palette.mode === "dark" ? 0.12 : 0.06,
            ),
            "&:hover": {
              bgcolor: alpha(
                theme.palette.primary.main,
                theme.palette.mode === "dark" ? 0.22 : 0.1,
              ),
            },
          }}
        >
          {busy ? <CircularProgress size={16} /> : <AddRounded fontSize="small" />}
        </IconButton>
      )}
    </Paper>
  );
}

function connectorInitials(connector: ConnectorInfo): string {
  return connector.definition.name
    .split(/\s+/)
    .filter(Boolean)
    .slice(0, 2)
    .map((part) => part[0]?.toUpperCase() ?? "")
    .join("") || connector.definition.name.slice(0, 2).toUpperCase();
}

function connectorDeveloperLabel(connector: ConnectorInfo): string {
  if (connector.source === "custom") return "由你添加";
  if (connector.source === "plugin") return "由插件提供";
  return "由 Omiga 开发";
}

function connectorCapabilitiesLabel(connector: ConnectorInfo): string {
  const tools = connector.definition.tools;
  if (tools.length === 0) return "Metadata only";
  const readOnly = tools.every((tool) => tool.readOnly);
  const hasWrites = tools.some((tool) => !tool.readOnly);
  if (readOnly) return "Read";
  if (hasWrites && tools.some((tool) => tool.readOnly)) return "Read, Write";
  return "Write";
}

function nativeToolCount(connector: ConnectorInfo): number {
  return connector.definition.tools.filter((tool) => tool.execution === "native")
    .length;
}

function connectorHasProductConnectionFlow(connector: ConnectorInfo): boolean {
  return connectorSupportsLogin(connector);
}

export function connectorIsProductIntegrated(connector: ConnectorInfo): boolean {
  return connectorHasProductConnectionFlow(connector) && nativeToolCount(connector) > 0;
}

function connectorRuntimeLabel(connector: ConnectorInfo): string {
  if (!connectorIsProductIntegrated(connector)) {
    return "暂未接入";
  }
  const nativeCount = nativeToolCount(connector);
  if (nativeCount > 0) {
    return `${nativeCount} 个原生可执行工具`;
  }
  if (connector.definition.tools.length > 0) {
    return "声明能力，等待 MCP/插件/native 执行器";
  }
  return "仅元数据";
}

function toolExecutionLabel(execution?: ConnectorToolExecution): string {
  switch (execution) {
    case "native":
      return "原生可用";
    case "external_mcp":
      return "外部 MCP";
    default:
      return "声明能力";
  }
}

function toolExecutionColor(
  execution?: ConnectorToolExecution,
): "success" | "info" | "default" {
  switch (execution) {
    case "native":
      return "success";
    case "external_mcp":
      return "info";
    default:
      return "default";
  }
}

function toolAccessLabel(tool: ConnectorToolDefinition): string {
  if (tool.readOnly) return "Read";
  return tool.confirmationRequired ? "Write · 需确认" : "Write";
}

function connectorAuthLabel(connector: ConnectorInfo): string {
  if (connector.definition.id === "github") {
    return "GitHub 登录 / GitHub CLI / 高级凭证";
  }
  if (connector.definition.id === "notion") {
    return "Notion 浏览器登录 / 高级凭证";
  }
  if (connector.definition.id === "slack") {
    return "Slack 浏览器登录 / 高级凭证";
  }
  switch (connector.definition.authType) {
    case "none":
      return "无需认证";
    case "oauth":
      return connector.definition.envVars.length > 0
        ? "浏览器 OAuth / 高级凭证"
        : "浏览器 OAuth";
    case "apiKey":
      return "官方授权 / 高级凭证";
    case "externalMcp":
      return "外部 MCP / 插件";
    default:
      return connector.definition.envVars.length > 0
        ? "官方授权 / 高级凭证"
        : "外部授权";
  }
}

function authSourceLabel(source?: string | null): string | null {
  switch (source) {
    case "environment":
      return "环境变量";
    case "oauth_device":
      return "Omiga OAuth";
    case "oauth_browser":
      return "浏览器 OAuth";
    case "github_cli":
      return "GitHub CLI";
    case "codex_apps":
    case "mcp_app":
      return "Codex/OpenAI Apps";
    case "manual":
      return "旧版本地状态";
    default:
      return source ? source.replace(/[_-]+/g, " ") : null;
  }
}

function connectorStatusText(connector: ConnectorInfo): string {
  if (connector.accessible) return "已连接";
  if (!connector.enabled) return "未添加";
  if (connector.status === "metadata_only") return "仅元数据";
  return "需要认证";
}

function connectorSupportsLogin(connector: ConnectorInfo): boolean {
  return (
    connector.definition.id === "github" ||
    connector.definition.id === "notion" ||
    connector.definition.id === "slack"
  );
}

function connectorConnectionUrl(connector: ConnectorInfo): string | null {
  return connectorSupportsLogin(connector)
    ? (connectorHostedInstallUrl(connector) ??
        connector.definition.installUrl ??
        connector.definition.docsUrl ??
        null)
    : null;
}

function connectorSetupUrl(connector: ConnectorInfo): string | null {
  return (
    connectorHostedInstallUrl(connector) ??
    connector.definition.installUrl ??
    connector.definition.docsUrl ??
    null
  );
}

function connectorSetupFallbackLabel(connector: ConnectorInfo): string {
  if (connector.definition.id === "slack") return "打开 Slack 登录授权页";
  if (connector.definition.id === "notion") return "打开 Notion 登录授权页";
  if (connector.definition.id === "github") return "打开 GitHub 登录授权页";
  return `打开 ${connector.definition.name} 登录授权页`;
}

function connectorNameSlug(name: string): string {
  const normalized = name
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
  return normalized || "app";
}

function connectorHostedInstallUrl(connector: ConnectorInfo): string | null {
  if (!connectorSupportsLogin(connector)) return null;
  const slug = connectorNameSlug(connector.definition.name);
  return `https://chatgpt.com/apps/${slug}/${connector.definition.id}`;
}

function connectorHasMissingProductOAuthConfig(
  connector: ConnectorInfo,
  errorMessage: string,
): boolean {
  const normalized = errorMessage.toLowerCase();
  if (connector.definition.id === "slack") {
    return (
      normalized.includes("omiga_slack_oauth") ||
      normalized.includes("slack oauth requires") ||
      normalized.includes("slack browser login requires")
    );
  }
  if (connector.definition.id === "notion") {
    return (
      normalized.includes("omiga_notion_oauth") ||
      normalized.includes("notion browser login requires")
    );
  }
  if (connector.definition.id === "github") {
    return (
      normalized.includes("omiga_github_oauth") ||
      normalized.includes("github oauth login requires")
    );
  }
  return false;
}

export function connectorLoginFailureMessage(
  connector: ConnectorInfo,
  errorMessage: string,
  openedHostedPage = false,
): string {
  const action = openedHostedPage ? "已为你打开登录授权页；" : "";
  if (connectorHasMissingProductOAuthConfig(connector, errorMessage)) {
    return `${action}${connector.definition.name} 将通过 Codex/OpenAI 托管授权页完成登录，不需要你配置 Client ID 或 Client Secret。请在浏览器中完成账号登录和授权，回到 Omiga 后点击“检测连接”刷新状态。`;
  }
  return errorMessage || `${connector.definition.name} 登录启动失败。`;
}

function connectorConnectionCtaLabel(connector: ConnectorInfo): string {
  if (!connectorIsProductIntegrated(connector)) return "暂未接入";
  if (connector.accessible) return "已连接";
  if (connectorSupportsLogin(connector)) return `连接 ${connector.definition.name}`;
  return "暂未接入";
}

function connectorLoginButtonLabel(connector: ConnectorInfo): string {
  if (connector.definition.id === "github") return "使用 GitHub 登录连接";
  if (connector.definition.id === "notion") return "使用 Notion 登录连接";
  if (connector.definition.id === "slack") return "使用 Slack 登录连接";
  return `连接 ${connector.definition.name}`;
}

function connectorLoginWaitingText(connector: ConnectorInfo): string {
  if (connector.definition.id === "github") {
    return "如果你已经安装 GitHub CLI，也可以在终端运行 gh auth login，Omiga 会复用该登录态。环境变量只作为高级/开发者备选。";
  }
  if (connector.definition.id === "notion") {
    return "请在 Notion 登录授权页选择工作区并批准访问；Omiga 不要求普通用户配置 Client ID/Secret，环境变量只作为高级/开发者备选。";
  }
  if (connector.definition.id === "slack") {
    return "请在 Slack 登录授权页选择工作区并批准安装；Omiga 不要求普通用户配置 Client ID/Secret，环境变量只作为高级/开发者备选。";
  }
  return "环境变量/API key 只作为高级/开发者备选，不是默认连接体验。";
}

function ConnectorInstallDialog({
  connector,
  open,
  busy,
  testing,
  onClose,
  onEnable,
  onTest,
  onStartLogin,
  onPollLogin,
}: {
  connector: ConnectorInfo;
  open: boolean;
  busy: boolean;
  testing: boolean;
  onClose: () => void;
  onEnable: (connector: ConnectorInfo, enabled: boolean) => void;
  onTest: (connector: ConnectorInfo) => void;
  onStartLogin: (connector: ConnectorInfo) => Promise<ConnectorLoginStartResult>;
  onPollLogin: (sessionId: string) => Promise<ConnectorLoginPollResult>;
}) {
  const theme = useTheme();
  const [loginStart, setLoginStart] = useState<ConnectorLoginStartResult | null>(
    null,
  );
  const [loginMessage, setLoginMessage] = useState<string | null>(null);
  const [loginError, setLoginError] = useState<string | null>(null);
  const [isStartingLogin, setIsStartingLogin] = useState(false);
  const [isPollingLogin, setIsPollingLogin] = useState(false);
  const installing = busy || testing || isStartingLogin || isPollingLogin;
  const supportsLogin = connectorSupportsLogin(connector);
  const isProductIntegrated = connectorIsProductIntegrated(connector);
  const setupUrl = connectorSetupUrl(connector);
  const hostedAppLogin = loginStart?.provider === "hosted_app";

  useEffect(() => {
    if (!open) {
      setLoginStart(null);
      setLoginMessage(null);
      setLoginError(null);
      setIsStartingLogin(false);
      setIsPollingLogin(false);
    }
  }, [open, connector.definition.id]);

  useEffect(() => {
    if (!open || !loginStart || loginStart.provider === "hosted_app") {
      return undefined;
    }
    let cancelled = false;
    let timer: number | undefined;

    const poll = async () => {
      if (cancelled) return;
      setIsPollingLogin(true);
      try {
        const result = await onPollLogin(loginStart.loginSessionId);
        if (cancelled) return;
        setLoginMessage(result.message);
        if (result.status === "complete") {
          setIsPollingLogin(false);
          onClose();
          return;
        }
        if (
          result.status === "pending" ||
          result.status === "slow_down"
        ) {
          timer = window.setTimeout(
            poll,
            Math.max(result.intervalSecs, 1) * 1000,
          );
          return;
        }
        setIsPollingLogin(false);
        setLoginError(result.message);
      } catch (error) {
        if (cancelled) return;
        setIsPollingLogin(false);
        setLoginError(
          error instanceof Error ? error.message : "Connector login failed.",
        );
      }
    };

    timer = window.setTimeout(
      poll,
      Math.max(loginStart.intervalSecs, 1) * 1000,
    );
    return () => {
      cancelled = true;
      if (timer !== undefined) window.clearTimeout(timer);
    };
  }, [loginStart, onClose, onPollLogin, open]);

  const handleInstall = async () => {
    setLoginError(null);
    setLoginMessage(null);
    if (!isProductIntegrated) {
      setLoginError(
        `${connector.definition.name} 还没有接入真实登录和可执行工具，暂不可启用。`,
      );
      return;
    }
    if (!supportsLogin) {
      onEnable(connector, true);
      const targetUrl = connectorConnectionUrl(connector);
      if (targetUrl) {
        setLoginMessage(
          `已打开 ${connector.definition.name} 的官方连接页面。完成授权/安装后回到 Omiga 点击“检测连接”。`,
        );
        void openUrl(targetUrl);
        return;
      }
      setLoginError(
        `${connector.definition.name} 还没有浏览器或本地软件连接方式。需要先实现该服务的 OAuth / MCP / native connector。`,
      );
      return;
    }

    setIsStartingLogin(true);
    try {
      const result = await onStartLogin(connector);
      setLoginStart(result);
      setLoginMessage(result.message);
      if (result.provider !== "github_cli") {
        void openUrl(result.verificationUriComplete ?? result.verificationUri);
      }
    } catch (error) {
      const errorMessage =
        error instanceof Error ? error.message : "Connector login failed.";
      const loginSetupUrl = connectorSetupUrl(connector);
      const shouldOpenSetupPage =
        Boolean(loginSetupUrl) &&
        connectorHasMissingProductOAuthConfig(connector, errorMessage);
      setLoginError(
        connectorLoginFailureMessage(
          connector,
          errorMessage,
          shouldOpenSetupPage,
        ),
      );
      if (loginSetupUrl && shouldOpenSetupPage) {
        void openUrl(loginSetupUrl).catch((openError) => {
          setLoginError(
            `${connectorLoginFailureMessage(connector, errorMessage, false)} 另外，打开登录授权页失败：${
              openError instanceof Error ? openError.message : String(openError)
            }`,
          );
        });
      }
    } finally {
      setIsStartingLogin(false);
    }
  };

  return (
    <Dialog
      open={open}
      onClose={installing ? undefined : onClose}
      fullWidth
      maxWidth="sm"
      PaperProps={{
        sx: {
          position: "relative",
          display: "flex",
          flexDirection: "column",
          borderRadius: 4,
          overflow: "hidden",
          maxHeight: { xs: "calc(100dvh - 32px)", sm: "min(92dvh, 780px)" },
          boxShadow: `0 24px 80px ${alpha(theme.palette.common.black, theme.palette.mode === "dark" ? 0.58 : 0.18)}`,
        },
      }}
    >
      <IconButton
        aria-label="关闭连接窗口"
        disabled={installing}
        onClick={onClose}
        sx={{
          position: "absolute",
          right: 18,
          top: 18,
          width: 40,
          height: 40,
          borderRadius: 1.5,
          border: 1,
          borderColor: "divider",
          bgcolor: "background.paper",
          zIndex: 2,
        }}
      >
        <CloseRounded />
      </IconButton>

      <DialogContent
        sx={{
          p: 0,
          overflowY: "auto",
          scrollbarGutter: "stable",
        }}
      >
        <Box
          sx={{
            px: { xs: 3, sm: 4.5 },
            pt: { xs: 4, sm: 4.5 },
            pb: { xs: 2.5, sm: 3 },
          }}
        >
          <Stack spacing={{ xs: 2.5, sm: 3 }} alignItems="center">
          <Stack direction="row" spacing={2} alignItems="center">
            <Box
              sx={{
                width: { xs: 60, sm: 68 },
                height: { xs: 60, sm: 68 },
                borderRadius: 2.5,
                display: "grid",
                placeItems: "center",
                bgcolor: "grey.950",
                color: "common.white",
                fontWeight: 900,
                fontSize: 18,
                boxShadow: `0 14px 34px ${alpha(theme.palette.common.black, 0.2)}`,
              }}
            >
              O
            </Box>
            <Typography
              aria-hidden="true"
              color="text.disabled"
              sx={{ letterSpacing: 4, fontWeight: 900, fontSize: 26 }}
            >
              •••
            </Typography>
            <Box
              sx={{
                width: { xs: 60, sm: 68 },
                height: { xs: 60, sm: 68 },
                borderRadius: 2.5,
                display: "grid",
                placeItems: "center",
                border: 1,
                borderColor: "divider",
                bgcolor: "background.paper",
                color: "text.primary",
                fontWeight: 900,
                fontSize: 18,
              }}
            >
              {connectorInitials(connector)}
            </Box>
          </Stack>

          <Stack spacing={0.5} alignItems="center" textAlign="center">
            <Typography variant="h5" fontWeight={900}>
              连接 {connector.definition.name}
            </Typography>
            <Typography variant="body2" color="text.secondary">
              {connectorDeveloperLabel(connector)}
            </Typography>
          </Stack>

          <Paper
            variant="outlined"
            sx={{
              width: "100%",
              borderRadius: 3,
              overflow: "hidden",
              bgcolor: alpha(theme.palette.background.paper, 0.86),
            }}
          >
            {[
              {
                title: "参考记忆和对话",
                body: `允许 Omiga 在使用 ${connector.definition.name} 时参考当前对话上下文，以生成更相关的工具调用。`,
                action: (
                  <Switch
                    size="small"
                    disabled
                    checked={false}
                    inputProps={{ "aria-label": "参考记忆和对话" }}
                  />
                ),
              },
              {
                title: "始终遵守权限",
                body: "Omiga 只会使用你明确配置的连接器凭证；你可以随时停用或断开连接。",
              },
              {
                title: "一切由你掌控",
                body: supportsLogin
                  ? `${connector.definition.name} 授权通过官方浏览器/本机软件完成；connectors/config.json 只保存账号标签和连接状态，密钥保存在系统安全存储或提供方本地登录态中。`
                  : "Omiga 不会要求你把 token 粘到界面里。优先跳转到官方页面或软件完成授权；高级凭证只应由外部 secret manager/环境提供。",
              },
              {
                title: "连接器可能会引入风险",
                body: "第三方服务可能返回不可信内容；执行写操作前仍需要明确确认。",
              },
            ].map((item, index, items) => (
              <Stack
                key={item.title}
                direction="row"
                spacing={2}
                alignItems="center"
                sx={{
                  px: 2.25,
                  py: 1.45,
                  borderBottom: index === items.length - 1 ? 0 : 1,
                  borderColor: "divider",
                }}
              >
                <Box sx={{ minWidth: 0, flex: 1 }}>
                  <Typography variant="subtitle2" fontWeight={900}>
                    {item.title}
                  </Typography>
                  <Typography
                    variant="body2"
                    color="text.secondary"
                    sx={{ mt: 0.25 }}
                  >
                    {item.body}
                  </Typography>
                </Box>
                {item.action}
              </Stack>
            ))}
          </Paper>

          {loginStart && (
            <Alert severity="info" sx={{ width: "100%", borderRadius: 2 }}>
              <Stack spacing={1}>
                <Typography variant="body2" fontWeight={800}>
                  {loginStart.provider === "github_cli"
                    ? "已启动 GitHub CLI 登录"
                    : loginStart.provider === "hosted_app"
                      ? "已打开 Codex/OpenAI 托管授权页"
                    : loginStart.provider === "notion_oauth"
                      ? "已打开 Notion 官方授权页"
                      : loginStart.provider === "slack_oauth"
                        ? "已打开 Slack 官方授权页"
                      : `在 GitHub 打开授权页并输入代码：${loginStart.userCode}`}
                </Typography>
                <Typography variant="body2">
                  {loginMessage ?? loginStart.message}
                </Typography>
                {loginStart.provider === "github_cli" ? (
                  <Typography variant="caption" color="text.secondary">
                    如果没有弹出终端，请手动运行 gh auth login，然后回到 Omiga 等待检测。
                  </Typography>
                ) : loginStart.provider === "hosted_app" ? (
                  <Stack direction="row" spacing={1} flexWrap="wrap" useFlexGap>
                    <Button
                      size="small"
                      variant="outlined"
                      onClick={() => void openUrl(loginStart.verificationUri)}
                    >
                      重新打开 {loginStart.connectorName} 登录授权页
                    </Button>
                    <Button
                      size="small"
                      variant="text"
                      disabled={testing || busy}
                      startIcon={
                        testing ? (
                          <CircularProgress size={14} color="inherit" />
                        ) : (
                          <RefreshRounded />
                        )
                      }
                      onClick={() => onTest(connector)}
                    >
                      检测连接
                    </Button>
                    <Typography
                      variant="caption"
                      color="text.secondary"
                      sx={{ alignSelf: "center" }}
                    >
                      授权完成后回到 Omiga 点击“检测连接”刷新；普通用户不需要配置
                      Client ID/Secret。
                    </Typography>
                  </Stack>
                ) : loginStart.provider === "notion_oauth" ||
                  loginStart.provider === "slack_oauth" ? (
                  <Stack direction="row" spacing={1} flexWrap="wrap" useFlexGap>
                    <Button
                      size="small"
                      variant="outlined"
                      onClick={() => void openUrl(loginStart.verificationUri)}
                    >
                      重新打开 {loginStart.connectorName} 授权页
                    </Button>
                    <Typography
                      variant="caption"
                      color="text.secondary"
                      sx={{ alignSelf: "center" }}
                    >
                      授权完成后浏览器会回跳本机，Omiga 自动检测完成。
                    </Typography>
                  </Stack>
                ) : (
                  <Stack direction="row" spacing={1} flexWrap="wrap" useFlexGap>
                    <Button
                      size="small"
                      variant="outlined"
                      onClick={() => void openUrl(loginStart.verificationUri)}
                    >
                      打开 GitHub 授权页
                    </Button>
                    <Button
                      size="small"
                      variant="text"
                      startIcon={<ContentCopyRounded />}
                      onClick={() =>
                        void navigator.clipboard.writeText(loginStart.userCode)
                      }
                    >
                      复制代码
                    </Button>
                  </Stack>
                )}
              </Stack>
            </Alert>
          )}

          {loginError && (
            <Alert severity="warning" sx={{ width: "100%", borderRadius: 2 }}>
              <Stack spacing={1}>
                <Typography variant="body2">{loginError}</Typography>
                <Typography variant="caption" color="text.secondary">
                  {supportsLogin
                    ? connectorLoginWaitingText(connector)
                    : "环境变量/API key 只作为高级/开发者备选，不是默认连接体验。"}
                </Typography>
                {setupUrl && (
                  <Button
                    size="small"
                    variant="outlined"
                    startIcon={<OpenInNewRounded />}
                    onClick={() => void openUrl(setupUrl)}
                    sx={{ alignSelf: "flex-start", borderRadius: 999 }}
                  >
                    {connectorSetupFallbackLabel(connector)}
                  </Button>
                )}
              </Stack>
            </Alert>
          )}

          {loginMessage && !loginStart && !loginError && (
            <Alert severity="info" sx={{ width: "100%", borderRadius: 2 }}>
              {loginMessage}
            </Alert>
          )}

          </Stack>
        </Box>
      </DialogContent>
      <DialogActions
        sx={{
          px: { xs: 3, sm: 4.5 },
          py: { xs: 2, sm: 2.5 },
          borderTop: 1,
          borderColor: "divider",
          bgcolor: "background.paper",
        }}
      >
        <Button
          fullWidth
          size="large"
          variant="contained"
          disabled={installing || (Boolean(loginStart) && !hostedAppLogin) || !isProductIntegrated}
          onClick={() => void handleInstall()}
          startIcon={
            installing ? <CircularProgress size={18} color="inherit" /> : undefined
          }
          sx={{
            borderRadius: 999,
            minHeight: 56,
            bgcolor: isProductIntegrated ? "text.primary" : "action.disabledBackground",
            color: isProductIntegrated ? "background.paper" : "text.disabled",
            fontWeight: 900,
            fontSize: 16,
            "&:hover": {
              bgcolor: isProductIntegrated
                ? "text.secondary"
                : "action.disabledBackground",
            },
          }}
        >
          {installing
            ? `正在连接 ${connector.definition.name}`
            : !isProductIntegrated
              ? `暂未接入 ${connector.definition.name}`
              : hostedAppLogin
                ? `重新打开 ${connector.definition.name} 登录授权页`
              : supportsLogin
                ? connectorLoginButtonLabel(connector)
                : `等待接入 ${connector.definition.name}`}
        </Button>
      </DialogActions>
    </Dialog>
  );
}

function ConnectorDetailsDialog({
  connector,
  open,
  busy,
  testing,
  testResult,
  auditEvents,
  onClose,
  onEnable,
  onDisconnect,
  onTest,
  onEdit,
  onDelete,
  onCopyEnv,
  onStartLogin,
  onPollLogin,
}: {
  connector: ConnectorInfo | null;
  open: boolean;
  busy: boolean;
  testing: boolean;
  testResult?: ConnectorConnectionTestResult;
  auditEvents: ConnectorAuditEvent[];
  onClose: () => void;
  onEnable: (connector: ConnectorInfo, enabled: boolean) => void;
  onDisconnect: (connector: ConnectorInfo) => void;
  onTest: (connector: ConnectorInfo) => void;
  onEdit: (connector: ConnectorInfo) => void;
  onDelete: (connector: ConnectorInfo) => void;
  onCopyEnv: (connector: ConnectorInfo) => void;
  onStartLogin: (
    connector: ConnectorInfo,
  ) => Promise<ConnectorLoginStartResult>;
  onPollLogin: (sessionId: string) => Promise<ConnectorLoginPollResult>;
}) {
  const theme = useTheme();
  const [installDialogOpen, setInstallDialogOpen] = useState(false);

  useEffect(() => {
    if (!open) setInstallDialogOpen(false);
  }, [open, connector?.definition.id]);

  if (!connector) return null;

  const metadataOnly = connector.status === "metadata_only";
  const isProductIntegrated = connectorIsProductIntegrated(connector);
  const toolCount = connector.definition.tools.length;
  const connectionTestHistory = connector.connectionTestHistory ?? [];
  const previousTestResults = connectionTestHistory
    .filter(
      (item) =>
        !testResult ||
        item.connectorId !== testResult.connectorId ||
        item.checkedAt !== testResult.checkedAt,
    )
    .slice(0, 3);
  const healthDetail = connectionHealthDetail(connector.connectionHealth);
  const connectionUrl = connectorConnectionUrl(connector);
  const infoRows = [
    ["类别", `${sourceLabel(connector)}, ${categoryLabel(connector.definition.category)}`],
    ["功能", connectorCapabilitiesLabel(connector)],
    ["执行", connectorRuntimeLabel(connector)],
    ["开发者", connectorDeveloperLabel(connector).replace(/^由\s*/, "")],
    ["认证", connectorAuthLabel(connector)],
    ["状态", connectorStatusText(connector)],
    ["存储", "用户级，密钥不写入配置文件"],
  ];
  const primaryActionLabel = connectorConnectionCtaLabel(connector);
  const primaryActionDisabled =
    busy ||
    testing ||
    !isProductIntegrated ||
    connector.accessible ||
    metadataOnly ||
    (!connectorSupportsLogin(connector) && !connectionUrl);

  return (
    <>
      <Dialog
        open={open}
        onClose={onClose}
        fullWidth
        maxWidth="lg"
        aria-labelledby="connector-details-title"
        PaperProps={{
          sx: {
            borderRadius: 3,
            overflow: "hidden",
            maxHeight: "92vh",
          },
        }}
      >
        <DialogContent
          sx={{
            p: 0,
            bgcolor:
              theme.palette.mode === "dark"
                ? alpha(theme.palette.common.black, 0.22)
                : "background.default",
          }}
        >
          <Box
            sx={{
              maxWidth: 920,
              mx: "auto",
              px: { xs: 2.5, md: 4 },
              py: { xs: 3, md: 4 },
            }}
          >
            <Stack spacing={{ xs: 3, md: 3.5 }}>
              <Stack
                direction="row"
                spacing={1.25}
                alignItems="center"
                sx={{ color: "text.secondary" }}
              >
                <Typography variant="body2" fontWeight={700}>
                  连接器
                </Typography>
                <Typography aria-hidden="true">›</Typography>
                <Typography
                  id="connector-details-title"
                  variant="body2"
                  color="text.primary"
                  fontWeight={900}
                  sx={{ flex: 1 }}
                >
                  {connector.definition.name}
                </Typography>
                <Tooltip title="复制连接器 ID">
                  <IconButton
                    size="small"
                    aria-label="复制连接器 ID"
                    onClick={() =>
                      void navigator.clipboard.writeText(connector.definition.id)
                    }
                  >
                    <LinkRounded fontSize="small" />
                  </IconButton>
                </Tooltip>
                <Button
                  variant="contained"
                  disabled={primaryActionDisabled}
                  onClick={() => setInstallDialogOpen(true)}
                  startIcon={
                    busy || testing ? (
                      <CircularProgress size={16} color="inherit" />
                    ) : undefined
                  }
                  sx={{
                    borderRadius: 999,
                    bgcolor: isProductIntegrated
                      ? "text.primary"
                      : "action.disabledBackground",
                    color: isProductIntegrated
                      ? "background.paper"
                      : "text.disabled",
                    fontWeight: 900,
                    px: 2.25,
                    "&:hover": {
                      bgcolor: isProductIntegrated
                        ? "text.secondary"
                        : "action.disabledBackground",
                    },
                  }}
                >
                  {busy || testing ? "正在连接" : primaryActionLabel}
                </Button>
                <IconButton aria-label="关闭连接器详情" onClick={onClose}>
                  <CloseRounded />
                </IconButton>
              </Stack>

              <Stack spacing={2.5}>
                <Box
                  sx={{
                    width: 64,
                    height: 64,
                    borderRadius: 2.5,
                    display: "grid",
                    placeItems: "center",
                    border: 1,
                    borderColor: "divider",
                    bgcolor: isProductIntegrated
                      ? "background.paper"
                      : "action.disabledBackground",
                    color: isProductIntegrated ? "text.primary" : "text.disabled",
                    fontWeight: 900,
                    fontSize: 18,
                    boxShadow: `0 12px 30px ${alpha(theme.palette.common.black, theme.palette.mode === "dark" ? 0.28 : 0.07)}`,
                  }}
                >
                  {connectorInitials(connector)}
                </Box>
                <Box>
                  <Typography variant="h4" fontWeight={950} letterSpacing={-0.4}>
                    {connector.definition.name}
                  </Typography>
                  <Typography variant="h6" color="text.secondary" fontWeight={500} sx={{ mt: 0.75 }}>
                    {connector.definition.description}
                  </Typography>
                </Box>
              </Stack>

              {!isProductIntegrated && (
                <Alert severity="info" sx={{ borderRadius: 2 }}>
                  {connector.definition.name} 暂未接入真实软件登录和可执行工具，
                  当前仅展示灰色元数据，不能启用或连接。后续接入 OAuth / 本地软件登录 /
                  native 工具后会自动变为可用。
                </Alert>
              )}

              <Box
                sx={{
                  borderRadius: 4,
                  overflow: "hidden",
                  minHeight: { xs: 180, md: 210 },
                  display: "grid",
                  placeItems: "center",
                  px: 3,
                  background:
                    !isProductIntegrated
                      ? `linear-gradient(135deg, ${alpha(theme.palette.text.disabled, 0.18)}, ${alpha(theme.palette.text.disabled, 0.08)})`
                      : theme.palette.mode === "dark"
                      ? `linear-gradient(135deg, ${alpha(theme.palette.success.dark, 0.18)}, ${alpha(theme.palette.warning.dark, 0.12)}), radial-gradient(circle at 20% 20%, ${alpha(theme.palette.common.white, 0.12)}, transparent 28%)`
                      : `linear-gradient(135deg, ${alpha(theme.palette.primary.light, 0.38)}, ${alpha(theme.palette.secondary.light, 0.28)}), radial-gradient(circle at 18% 22%, ${alpha(theme.palette.common.white, 0.8)}, transparent 30%)`,
                }}
              >
                <Paper
                  variant="outlined"
                  sx={{
                    maxWidth: 620,
                    px: 2,
                    py: 1.5,
                    borderRadius: 3,
                    bgcolor: alpha(theme.palette.background.paper, theme.palette.mode === "dark" ? 0.76 : 0.72),
                    backdropFilter: "blur(18px)",
                  }}
                >
                  <Stack direction="row" spacing={1.25} alignItems="center">
                    <Box
                      sx={{
                        width: 28,
                        height: 28,
                        borderRadius: 1,
                        display: "grid",
                        placeItems: "center",
                        bgcolor: "background.paper",
                        border: 1,
                        borderColor: "divider",
                        fontSize: 12,
                        fontWeight: 900,
                      }}
                    >
                      {connectorInitials(connector)}
                    </Box>
                    <Typography variant="body1" fontWeight={800}>
                      {connector.definition.name}
                    </Typography>
                    <Typography variant="body1" color="text.secondary">
                      {connector.definition.description}
                    </Typography>
                  </Stack>
                </Paper>
              </Box>

              <Typography variant="body1" sx={{ lineHeight: 1.65 }}>
                使用 {connector.definition.name} 访问外部服务、读取必要上下文并执行已声明的工具能力。
                连接器优先走用户级凭证，必要时可回退到环境变量或外部 MCP。
              </Typography>

              <Stack spacing={2}>
                <Typography variant="h6" fontWeight={900}>
                  包含内容
                </Typography>
                <Paper variant="outlined" sx={{ borderRadius: 3, overflow: "hidden", bgcolor: "background.paper" }}>
                  <Stack
                    direction="row"
                    spacing={2}
                    alignItems="center"
                    sx={{ px: 2, py: 1.7 }}
                  >
                    <Box
                      sx={{
                        width: 44,
                        height: 44,
                        borderRadius: "50%",
                        display: "grid",
                        placeItems: "center",
                        border: 1,
                        borderColor: "divider",
                        color: "text.secondary",
                        flexShrink: 0,
                      }}
                    >
                      <LinkRounded fontSize="small" />
                    </Box>
                    <Box sx={{ minWidth: 0, flex: 1 }}>
                      <Stack direction="row" spacing={1} alignItems="center" flexWrap="wrap">
                        <Typography variant="subtitle1" fontWeight={900}>
                          {connector.definition.name}
                        </Typography>
                        <Typography variant="body2" color="text.secondary">
                          应用
                        </Typography>
                      </Stack>
                      <Typography variant="body2" color="text.secondary" noWrap>
                        {authHint(connector)}
                      </Typography>
                    </Box>
                    {connector.enabled && <CheckCircleRounded color="success" fontSize="small" />}
                  </Stack>
                  {connector.definition.tools.map((tool) => (
                    <Stack
                      key={tool.name}
                      direction="row"
                      spacing={2}
                      alignItems="center"
                      sx={{ px: 2, py: 1.7, borderTop: 1, borderColor: "divider" }}
                    >
                      <Box
                        sx={{
                          width: 44,
                          height: 44,
                          borderRadius: "50%",
                          display: "grid",
                          placeItems: "center",
                          border: 1,
                          borderColor: "divider",
                          color: "text.secondary",
                          flexShrink: 0,
                        }}
                      >
                        <TroubleshootRounded fontSize="small" />
                      </Box>
                      <Box sx={{ minWidth: 0, flex: 1 }}>
                        <Stack direction="row" spacing={1} alignItems="center" flexWrap="wrap">
                          <Typography variant="subtitle1" fontWeight={900}>
                            {tool.name}
                          </Typography>
                          <Chip
                            size="small"
                            color={toolExecutionColor(tool.execution)}
                            variant="outlined"
                            label={toolExecutionLabel(tool.execution)}
                          />
                          <Chip
                            size="small"
                            variant="outlined"
                            color={tool.readOnly ? "default" : "warning"}
                            label={toolAccessLabel(tool)}
                          />
                        </Stack>
                        <Typography variant="body2" color="text.secondary" noWrap>
                          {tool.description}
                        </Typography>
                        {tool.requiredScopes.length > 0 && (
                          <Stack
                            direction="row"
                            spacing={0.75}
                            flexWrap="wrap"
                            useFlexGap
                            sx={{ mt: 0.75 }}
                          >
                            <Typography
                              variant="caption"
                              color="text.secondary"
                              sx={{ alignSelf: "center", fontWeight: 700 }}
                            >
                              权限
                            </Typography>
                            {tool.requiredScopes.map((scope) => (
                              <Chip
                                key={`${tool.name}-${scope}`}
                                size="small"
                                variant="outlined"
                                label={scope}
                                sx={{ height: 22 }}
                              />
                            ))}
                          </Stack>
                        )}
                      </Box>
                    </Stack>
                  ))}
                  {toolCount === 0 && (
                    <Box sx={{ px: 2, py: 1.7, borderTop: 1, borderColor: "divider" }}>
                      <Typography variant="body2" color="text.secondary">
                        暂未声明原生工具。可通过自定义连接器或 MCP 补充能力。
                      </Typography>
                    </Box>
                  )}
                </Paper>
              </Stack>

              <Stack spacing={2}>
                <Typography variant="h6" fontWeight={900}>
                  信息
                </Typography>
                <Paper variant="outlined" sx={{ borderRadius: 3, overflow: "hidden", bgcolor: "background.paper" }}>
                  {infoRows.map(([label, value], index) => (
                    <Stack
                      key={label}
                      direction={{ xs: "column", sm: "row" }}
                      spacing={1}
                      sx={{
                        px: 2,
                        py: 1.7,
                        borderTop: index === 0 ? 0 : 1,
                        borderColor: "divider",
                      }}
                    >
                      <Typography variant="body2" color="text.secondary" sx={{ width: 220, flexShrink: 0 }}>
                        {label}
                      </Typography>
                      <Typography variant="body2" fontWeight={700} sx={{ minWidth: 0, wordBreak: "break-word" }}>
                        {value}
                      </Typography>
                    </Stack>
                  ))}
                  {(connector.definition.installUrl || connector.definition.docsUrl) && (
                    <Stack
                      direction={{ xs: "column", sm: "row" }}
                      spacing={1}
                      sx={{ px: 2, py: 1.7, borderTop: 1, borderColor: "divider" }}
                    >
                      <Typography variant="body2" color="text.secondary" sx={{ width: 220, flexShrink: 0 }}>
                        链接
                      </Typography>
                      <Stack direction="row" spacing={1} flexWrap="wrap" useFlexGap>
                        {connector.definition.installUrl && (
                          <Button
                            size="small"
                            variant="text"
                            endIcon={<OpenInNewRounded />}
                            onClick={() => void openUrl(connector.definition.installUrl!)}
                          >
                            连接文档
                          </Button>
                        )}
                        {connector.definition.docsUrl && (
                          <Button
                            size="small"
                            variant="text"
                            endIcon={<OpenInNewRounded />}
                            onClick={() => void openUrl(connector.definition.docsUrl!)}
                          >
                            API 文档
                          </Button>
                        )}
                      </Stack>
                    </Stack>
                  )}
                </Paper>
              </Stack>

              <Stack spacing={2}>
                <Typography variant="h6" fontWeight={900}>
                  最近操作
                </Typography>
                <Paper variant="outlined" sx={{ borderRadius: 3, overflow: "hidden", bgcolor: "background.paper" }}>
                  {auditEvents.length > 0 ? (
                    auditEvents.slice(0, 5).map((event, index) => (
                      <Stack
                        key={event.id}
                        direction={{ xs: "column", md: "row" }}
                        spacing={1.5}
                        alignItems={{ xs: "flex-start", md: "center" }}
                        sx={{
                          px: 2,
                          py: 1.6,
                          borderTop: index === 0 ? 0 : 1,
                          borderColor: "divider",
                        }}
                      >
                        <Box sx={{ minWidth: 0, flex: 1 }}>
                          <Stack direction="row" spacing={1} alignItems="center" flexWrap="wrap" useFlexGap>
                            <Typography variant="subtitle2" fontWeight={900}>
                              {event.operation}
                            </Typography>
                            <Chip
                              size="small"
                              variant="outlined"
                              color={event.access === "write" ? "warning" : "default"}
                              label={event.access === "write" ? "Write" : "Read"}
                            />
                            <Chip
                              size="small"
                              variant="outlined"
                              color={auditOutcomeColor(event.outcome)}
                              label={auditOutcomeLabel(event.outcome)}
                            />
                            {event.confirmationRequired && (
                              <Chip
                                size="small"
                                variant="outlined"
                                label={event.confirmed ? "已确认" : "未确认"}
                              />
                            )}
                          </Stack>
                          <Typography variant="body2" color="text.secondary" sx={{ mt: 0.4 }}>
                            {event.target ? `目标：${event.target}` : "未记录目标"} ·{" "}
                            {formatCheckedAt(event.createdAt)}
                          </Typography>
                          {event.message && (
                            <Typography
                              variant="caption"
                              color="text.secondary"
                              sx={{ mt: 0.35, display: "block" }}
                            >
                              {event.message}
                            </Typography>
                          )}
                        </Box>
                        {event.sessionId && (
                          <Chip
                            size="small"
                            variant="outlined"
                            label={`session ${event.sessionId}`}
                            sx={{ maxWidth: 220 }}
                          />
                        )}
                      </Stack>
                    ))
                  ) : (
                    <Box sx={{ px: 2, py: 1.7 }}>
                      <Typography variant="body2" color="text.secondary">
                        暂无 connector 工具调用记录。读取或写入外部服务后会在这里显示审计事件。
                      </Typography>
                    </Box>
                  )}
                </Paper>
              </Stack>

              <Stack spacing={2}>
                <Stack
                  direction={{ xs: "column", sm: "row" }}
                  spacing={1}
                  alignItems={{ xs: "flex-start", sm: "center" }}
                >
                  <Typography variant="h6" fontWeight={900} sx={{ flex: 1 }}>
                    连接状态
                  </Typography>
                  <Chip
                    size="small"
                    color={statusColor(connector.status)}
                    label={connectorStatusText(connector)}
                  />
                </Stack>
                <Paper
                  variant="outlined"
                  sx={{
                    borderRadius: 3,
                    overflow: "hidden",
                    bgcolor:
                      theme.palette.mode === "dark"
                        ? alpha(theme.palette.background.paper, 0.72)
                        : "background.paper",
                    borderColor:
                      !connector.accessible && connector.enabled
                        ? alpha(theme.palette.warning.main, 0.28)
                        : "divider",
                  }}
                >
                  <Box
                    sx={{
                      px: 2,
                      py: 1.75,
                      borderBottom: 1,
                      borderColor: "divider",
                      bgcolor:
                        !connector.accessible && connector.enabled
                          ? alpha(
                              theme.palette.warning.main,
                              theme.palette.mode === "dark" ? 0.08 : 0.05,
                            )
                          : alpha(
                              theme.palette.success.main,
                              connector.accessible ? 0.06 : 0,
                            ),
                    }}
                  >
                    <Stack
                      direction={{ xs: "column", md: "row" }}
                      spacing={1.5}
                      alignItems={{ xs: "stretch", md: "center" }}
                    >
                      <Stack spacing={0.75} sx={{ minWidth: 0, flex: 1 }}>
                        <Stack
                          direction="row"
                          spacing={1}
                          alignItems="center"
                          flexWrap="wrap"
                          useFlexGap
                        >
                          <Typography variant="subtitle1" fontWeight={900}>
                            {connector.accessible
                              ? "连接已可用"
                              : connectorSupportsLogin(connector)
                                ? "需要登录授权"
                                : "等待真实接入"}
                          </Typography>
                          {(connector.connectionHealth?.totalChecks ?? 0) > 0 && (
                            <Chip
                              size="small"
                              color={connectionHealthColor(
                                connector.connectionHealth,
                              )}
                              variant="outlined"
                              label={connectionHealthLabel(
                                connector.connectionHealth,
                              )}
                            />
                          )}
                          {connector.accountLabel && (
                            <Chip
                              size="small"
                              variant="outlined"
                              label={`账号：${connector.accountLabel}`}
                            />
                          )}
                          {authSourceLabel(connector.authSource) && (
                            <Chip
                              size="small"
                              variant="outlined"
                              label={`来源：${authSourceLabel(connector.authSource)}`}
                            />
                          )}
                        </Stack>
                        <Typography variant="body2" color="text.secondary">
                          {connector.accessible
                            ? "Omiga 可以使用该连接器调用已声明的工具能力。"
                            : connectorSupportsLogin(connector)
                              ? connector.definition.id === "github"
                                ? "点击“连接 GitHub”通过浏览器授权；也可先在终端运行 gh auth login，Omiga 会自动复用 GitHub CLI 登录态，或使用环境变量作为备用凭证。"
                                : "点击连接按钮打开登录授权页；普通用户不需要配置 Client ID/Secret，授权完成后回到 Omiga 检测连接状态。"
                              : "该连接器还没有浏览器、软件或 native/MCP 接入方式，不能仅靠手动标记变成可用。"}
                        </Typography>
                        {connector.connectedAt && (
                          <Typography variant="caption" color="text.secondary">
                            连接于 {formatCheckedAt(connector.connectedAt)}
                          </Typography>
                        )}
                      </Stack>

                      <Stack
                        direction="row"
                        spacing={1}
                        flexWrap="wrap"
                        useFlexGap
                        justifyContent={{ xs: "flex-start", md: "flex-end" }}
                      >
                        {!connector.accessible && connectorSupportsLogin(connector) && (
                          <Button
                            size="small"
                            variant="contained"
                            startIcon={<LinkRounded />}
                            disabled={busy || testing || metadataOnly}
                            onClick={() => setInstallDialogOpen(true)}
                            sx={{ borderRadius: 999, fontWeight: 800 }}
                          >
                            连接 {connector.definition.name}
                          </Button>
                        )}
                        <Button
                          size="small"
                          variant="outlined"
                          startIcon={
                            testing ? (
                              <CircularProgress size={14} />
                            ) : (
                              <TroubleshootRounded />
                            )
                          }
                          disabled={busy || testing}
                          onClick={() => onTest(connector)}
                          sx={{ borderRadius: 999 }}
                        >
                          检测连接
                        </Button>
                        {connector.definition.envVars.length > 0 && (
                          <Button
                            size="small"
                            variant="text"
                            startIcon={<ContentCopyRounded />}
                            onClick={() => onCopyEnv(connector)}
                            sx={{ borderRadius: 999 }}
                          >
                            {connector.definition.id === "github"
                              ? "复制 gh/env 设置"
                              : connector.definition.id === "notion"
                                ? "复制 OAuth/env 设置"
                                : connector.definition.id === "slack"
                                  ? "复制 OAuth/env 设置"
                              : "高级凭证"}
                          </Button>
                        )}
                      </Stack>
                    </Stack>
                  </Box>

                  <Stack spacing={1.25} sx={{ p: 2 }}>
                    {testResult ? (
                      <Paper
                        variant="outlined"
                        sx={{
                          borderRadius: 2.5,
                          p: 1.5,
                          bgcolor: alpha(
                            testResult.ok
                              ? theme.palette.success.main
                              : theme.palette.warning.main,
                            theme.palette.mode === "dark" ? 0.08 : 0.045,
                          ),
                          borderColor: alpha(
                            testResult.ok
                              ? theme.palette.success.main
                              : theme.palette.warning.main,
                            0.32,
                          ),
                        }}
                      >
                        <Stack direction="row" spacing={1.25} alignItems="flex-start">
                          <Box
                            sx={{
                              width: 32,
                              height: 32,
                              borderRadius: "50%",
                              display: "grid",
                              placeItems: "center",
                              color: testResult.ok
                                ? "success.main"
                                : "warning.main",
                              flexShrink: 0,
                            }}
                          >
                            {testResult.ok ? (
                              <CheckCircleRounded fontSize="small" />
                            ) : (
                              <TroubleshootRounded fontSize="small" />
                            )}
                          </Box>
                          <Box sx={{ minWidth: 0, flex: 1 }}>
                            <Typography variant="body2" fontWeight={900}>
                              {testKindLabel(testResult)} ·{" "}
                              {testResult.ok ? "连接正常" : "需要处理"}
                            </Typography>
                            <Typography variant="body2" color="text.secondary">
                              {testResult.message}
                            </Typography>
                            <Stack
                              direction="row"
                              spacing={0.75}
                              flexWrap="wrap"
                              useFlexGap
                              sx={{ mt: 0.75 }}
                            >
                              <Chip
                                size="small"
                                variant="outlined"
                                label={`检测于 ${formatCheckedAt(testResult.checkedAt)}`}
                              />
                              {testDetailChips(testResult).map((chip) => (
                                <Chip
                                  key={chip.key}
                                  size="small"
                                  color={chip.color ?? "default"}
                                  variant="outlined"
                                  label={chip.label}
                                />
                              ))}
                            </Stack>
                            {testResult.details && (
                              <Typography
                                variant="caption"
                                color="text.secondary"
                                sx={{ mt: 0.75, display: "block" }}
                              >
                                {testResult.details}
                              </Typography>
                            )}
                          </Box>
                        </Stack>
                      </Paper>
                    ) : (
                      <Typography variant="body2" color="text.secondary">
                        还没有连接测试结果。完成官方授权或本机软件登录后，可以运行一次检测确认可用性。
                      </Typography>
                    )}

                    {healthDetail && (
                      <Typography variant="body2" color="text.secondary">
                        Recent health: {healthDetail}
                      </Typography>
                    )}
                    {previousTestResults.length > 0 && (
                      <Stack direction="row" spacing={0.75} flexWrap="wrap" useFlexGap>
                        {previousTestResults.map((item) => (
                          <Chip
                            key={`${item.connectorId}-${item.checkedAt}`}
                            size="small"
                            color={item.ok ? "success" : "warning"}
                            variant="outlined"
                            label={`${item.ok ? "OK" : (item.errorCode ?? "Failed")} · ${formatCheckedAt(item.checkedAt)}`}
                          />
                        ))}
                      </Stack>
                    )}

                    <Stack direction="row" spacing={1} flexWrap="wrap" useFlexGap>
                      {connector.accessible ? (
                        <Button
                          size="small"
                          variant="outlined"
                          color="inherit"
                          startIcon={<CloudOffRounded />}
                          disabled={busy || metadataOnly}
                          onClick={() => onDisconnect(connector)}
                        >
                          断开连接
                        </Button>
                      ) : null}
                      {connector.source === "custom" && (
                        <>
                          <Button
                            size="small"
                            variant="text"
                            startIcon={<EditRounded />}
                            disabled={busy}
                            onClick={() => onEdit(connector)}
                          >
                            编辑
                          </Button>
                          <Button
                            size="small"
                            variant="text"
                            color="error"
                            startIcon={<DeleteRounded />}
                            disabled={busy}
                            onClick={() => onDelete(connector)}
                          >
                            删除
                          </Button>
                        </>
                      )}
                    </Stack>
                  </Stack>
                </Paper>
              </Stack>
            </Stack>
          </Box>
        </DialogContent>
      </Dialog>

      <ConnectorInstallDialog
        connector={connector}
        open={installDialogOpen}
        busy={busy}
        testing={testing}
        onClose={() => setInstallDialogOpen(false)}
        onEnable={onEnable}
        onTest={onTest}
        onStartLogin={onStartLogin}
        onPollLogin={onPollLogin}
      />
    </>
  );
}

export function ConnectorsPanel({
  projectPath,
}: {
  projectPath: string;
}) {
  const {
    catalog,
    auditEvents,
    isLoading,
    isMutating,
    testingConnectorIds,
    testResults,
    error,
    loadConnectors,
    loadConnectorAuditEvents,
    setConnectorEnabled,
    disconnectConnector,
    testConnectorConnection,
    startConnectorLogin,
    pollConnectorLogin,
    upsertCustomConnector,
    deleteCustomConnector,
    exportCustomConnectors,
    importCustomConnectors,
  } = useConnectorStore();
  const [customDialogOpen, setCustomDialogOpen] = useState(false);
  const [editingCustomConnectorId, setEditingCustomConnectorId] = useState<
    string | null
  >(null);
  const [customForm, setCustomForm] = useState<CustomConnectorFormState>(
    emptyCustomConnectorForm,
  );
  const [importDialogOpen, setImportDialogOpen] = useState(false);
  const [importText, setImportText] = useState("");
  const [replaceExistingImport, setReplaceExistingImport] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [searchQuery, setSearchQuery] = useState("");
  const [statusFilter, setStatusFilter] =
    useState<ConnectorStatusFilter>("all");
  const [sourceFilter, setSourceFilter] =
    useState<ConnectorSourceFilter>("all");
  const [categoryFilter, setCategoryFilter] = useState("all");
  const [onlyAccessible, setOnlyAccessible] = useState(false);
  const [detailConnectorId, setDetailConnectorId] = useState<string | null>(
    null,
  );

  useEffect(() => {
    void loadConnectors();
  }, [loadConnectors]);

  useEffect(() => {
    if (!detailConnectorId) return;
    void loadConnectorAuditEvents(detailConnectorId);
  }, [detailConnectorId, loadConnectorAuditEvents]);

  const categories = useMemo(() => {
    return Array.from(
      new Set(
        (catalog?.connectors ?? []).map(
          (connector) => connector.definition.category || "other",
        ),
      ),
    ).sort((left, right) => left.localeCompare(right));
  }, [catalog]);

  const connectorsById = useMemo(() => {
    const byId = new Map<string, ConnectorInfo>();
    for (const connector of catalog?.connectors ?? []) {
      byId.set(connector.definition.id, connector);
    }
    return byId;
  }, [catalog]);

  const detailConnector = detailConnectorId
    ? (connectorsById.get(detailConnectorId) ?? null)
    : null;
  const detailAuditEvents = useMemo(
    () =>
      detailConnectorId
        ? auditEvents.filter((event) => event.connectorId === detailConnectorId)
        : [],
    [auditEvents, detailConnectorId],
  );

  const connectorTestResult = (
    connector: ConnectorInfo,
  ): ConnectorConnectionTestResult | undefined =>
    testResults[connector.definition.id] ??
    connector.lastConnectionTest ??
    undefined;

  const filteredConnectors = useMemo(() => {
    return (catalog?.connectors ?? []).filter((connector) => {
      if (!connectorMatchesSearch(connector, searchQuery)) return false;
      if (statusFilter !== "all" && connector.status !== statusFilter)
        return false;
      if (sourceFilter !== "all" && connector.source !== sourceFilter)
        return false;
      if (
        categoryFilter !== "all" &&
        connector.definition.category !== categoryFilter
      ) {
        return false;
      }
      if (
        onlyAccessible &&
        (!connector.accessible || !connectorIsProductIntegrated(connector))
      ) {
        return false;
      }
      return true;
    });
  }, [
    catalog,
    categoryFilter,
    onlyAccessible,
    searchQuery,
    sourceFilter,
    statusFilter,
  ]);

  const grouped = useMemo(() => {
    const groups = new Map<string, ConnectorInfo[]>();
    for (const connector of filteredConnectors) {
      const category = connector.definition.category || "other";
      groups.set(category, [...(groups.get(category) ?? []), connector]);
    }
    return Array.from(groups.entries()).sort(([left], [right]) =>
      left.localeCompare(right),
    );
  }, [filteredConnectors]);

  const openAddCustomConnector = () => {
    setEditingCustomConnectorId(null);
    setCustomForm(emptyCustomConnectorForm);
    setCustomDialogOpen(true);
  };

  const openEditCustomConnector = (connector: ConnectorInfo) => {
    setDetailConnectorId(null);
    setEditingCustomConnectorId(connector.definition.id);
    setCustomForm(connectorToForm(connector));
    setCustomDialogOpen(true);
  };

  const closeCustomConnectorDialog = () => {
    if (isMutating) return;
    setCustomDialogOpen(false);
    setEditingCustomConnectorId(null);
    setCustomForm(emptyCustomConnectorForm);
  };

  const saveCustomConnector = () => {
    void upsertCustomConnector(buildCustomConnectorRequest(customForm)).then(
      () => {
        closeCustomConnectorDialog();
      },
    );
  };

  const handleDeleteCustomConnector = (connector: ConnectorInfo) => {
    const confirmed = window.confirm(
      `Delete custom connector "${connector.definition.name}" from user-level settings? This does not remove any external account or secret.`,
    );
    if (!confirmed) return;
    if (detailConnectorId === connector.definition.id) {
      setDetailConnectorId(null);
    }
    void deleteCustomConnector(connector.definition.id);
  };

  const handleCopyEnvSetup = (connector: ConnectorInfo) => {
    void navigator.clipboard.writeText(envSetupSnippet(connector)).then(() => {
      setNotice(`Copied advanced credential setup for ${connector.definition.name}.`);
    });
  };

  const handleExportCustomConnectors = () => {
    void exportCustomConnectors().then((payload) => {
      void navigator.clipboard
        .writeText(JSON.stringify(payload, null, 2))
        .then(() =>
          setNotice(
            `Copied ${payload.connectors.length} custom connector${
              payload.connectors.length === 1 ? "" : "s"
            } to clipboard.`,
          ),
        );
    });
  };

  const openImportCustomConnectors = () => {
    setImportText("");
    setReplaceExistingImport(false);
    setImportDialogOpen(true);
  };

  const closeImportCustomConnectors = () => {
    if (isMutating) return;
    setImportDialogOpen(false);
    setImportText("");
    setReplaceExistingImport(false);
  };

  const handleImportCustomConnectors = () => {
    let connectors: CustomConnectorRequest[];
    try {
      connectors = parseCustomConnectorImport(importText);
    } catch (error) {
      setNotice(null);
      window.alert(
        error instanceof Error
          ? error.message
          : "Invalid connector import JSON.",
      );
      return;
    }
    void importCustomConnectors(connectors, replaceExistingImport).then(() => {
      setNotice(
        `Imported ${connectors.length} custom connector${connectors.length === 1 ? "" : "s"}.`,
      );
      closeImportCustomConnectors();
    });
  };

  return (
    <Box sx={{ mt: 2 }}>
      <Stack spacing={2}>
        <Alert severity="info" sx={{ borderRadius: 2 }}>
          Connectors are user-level account/service links shared across projects
          on this machine. They model external access separately from Plugins
          and MCP. Omiga stores enablement and account labels only; product
          flows should connect through the provider's browser/software login.
          Environment/API-key credentials are advanced fallbacks for local
          development or external secret managers. A connector becomes
          actionable only when matching MCP/native tools are available.
        </Alert>

        <Stack direction="row" spacing={1} alignItems="center">
          <Typography variant="h6" fontWeight={700} sx={{ flex: 1 }}>
            Connectors
          </Typography>
          <Chip
            size="small"
            variant="outlined"
            label={`Scope: ${catalog?.scope ?? "user"}`}
          />
          {catalog?.configPath && (
            <Tooltip title={catalog.configPath}>
              <IconButton
                size="small"
                onClick={() =>
                  void navigator.clipboard.writeText(catalog.configPath)
                }
              >
                <ContentCopyRounded fontSize="small" />
              </IconButton>
            </Tooltip>
          )}
          <Button
            size="small"
            variant="contained"
            startIcon={<AddRounded />}
            disabled={isMutating}
            onClick={openAddCustomConnector}
          >
            Add custom
          </Button>
          <Button
            size="small"
            variant="outlined"
            startIcon={<UploadRounded />}
            disabled={isMutating}
            onClick={openImportCustomConnectors}
          >
            Import
          </Button>
          <Button
            size="small"
            variant="outlined"
            startIcon={<DownloadRounded />}
            disabled={isMutating}
            onClick={handleExportCustomConnectors}
          >
            Export custom
          </Button>
          <Button
            size="small"
            variant="outlined"
            startIcon={
              isLoading ? <CircularProgress size={14} /> : <RefreshRounded />
            }
            disabled={isLoading}
            onClick={() => void loadConnectors()}
          >
            Refresh
          </Button>
        </Stack>

        <Card variant="outlined" sx={{ borderRadius: 2 }}>
          <CardContent>
            <Stack spacing={1.5}>
              <Stack direction={{ xs: "column", md: "row" }} spacing={1.5}>
                <TextField
                  size="small"
                  label="Search connectors"
                  value={searchQuery}
                  onChange={(event) => setSearchQuery(event.target.value)}
                  fullWidth
                />
                <TextField
                  size="small"
                  label="Category"
                  value={categoryFilter}
                  onChange={(event) => setCategoryFilter(event.target.value)}
                  select
                  sx={{ minWidth: 180 }}
                >
                  <MenuItem value="all">All categories</MenuItem>
                  {categories.map((category) => (
                    <MenuItem key={category} value={category}>
                      {categoryLabel(category)}
                    </MenuItem>
                  ))}
                </TextField>
                <TextField
                  size="small"
                  label="Status"
                  value={statusFilter}
                  onChange={(event) =>
                    setStatusFilter(event.target.value as ConnectorStatusFilter)
                  }
                  select
                  sx={{ minWidth: 170 }}
                >
                  <MenuItem value="all">All statuses</MenuItem>
                  {(
                    [
                      "connected",
                      "needs_auth",
                      "disabled",
                      "metadata_only",
                    ] as const
                  ).map((status) => (
                    <MenuItem key={status} value={status}>
                      {statusLabel(status)}
                    </MenuItem>
                  ))}
                </TextField>
                <TextField
                  size="small"
                  label="Source"
                  value={sourceFilter}
                  onChange={(event) =>
                    setSourceFilter(event.target.value as ConnectorSourceFilter)
                  }
                  select
                  sx={{ minWidth: 150 }}
                >
                  <MenuItem value="all">All sources</MenuItem>
                  {(["built_in", "custom", "plugin"] as const).map((source) => (
                    <MenuItem key={source} value={source}>
                      {sourceValueLabel(source)}
                    </MenuItem>
                  ))}
                </TextField>
              </Stack>
              <Stack
                direction="row"
                spacing={1}
                alignItems="center"
                flexWrap="wrap"
              >
                <FormControlLabel
                  control={
                    <Switch
                      checked={onlyAccessible}
                      onChange={(event) =>
                        setOnlyAccessible(event.target.checked)
                      }
                    />
                  }
                  label="Only actionable"
                />
                <Chip
                  size="small"
                  variant="outlined"
                  label={`${filteredConnectors.length} / ${catalog?.connectors.length ?? 0} shown`}
                />
              </Stack>
            </Stack>
          </CardContent>
        </Card>

        {catalog?.notes?.length ? (
          <Alert severity="success" sx={{ borderRadius: 2 }}>
            <Stack spacing={0.5}>
              {catalog.notes.map((note) => (
                <Typography key={note} variant="body2">
                  {note}
                </Typography>
              ))}
            </Stack>
          </Alert>
        ) : null}

        {notice && (
          <Alert severity="success" onClose={() => setNotice(null)}>
            {notice}
          </Alert>
        )}

        {error && <Alert severity="error">{error}</Alert>}

        {isLoading && !catalog ? (
          <Box sx={{ py: 4, textAlign: "center" }}>
            <CircularProgress size={24} />
          </Box>
        ) : grouped.length === 0 ? (
          <Alert severity="warning">
            {catalog?.connectors.length
              ? "No connectors match the current filters."
              : "No connector definitions found."}
          </Alert>
        ) : (
          grouped.map(([category, connectors]) => (
            <Box key={category}>
              <Typography
                variant="subtitle2"
                color="text.secondary"
                sx={{ mb: 1 }}
              >
                {categoryLabel(category)}
              </Typography>
              <Box sx={connectorCardGridSx}>
                {connectors.map((connector) => (
                  <ConnectorCard
                    key={connector.definition.id}
                    connector={connector}
                    busy={isMutating}
                    testResult={connectorTestResult(connector)}
                    onEnable={(item, enabled) =>
                      void setConnectorEnabled(item.definition.id, enabled)
                    }
                    onOpenDetails={(item) =>
                      setDetailConnectorId(item.definition.id)
                    }
                  />
                ))}
              </Box>
            </Box>
          ))
        )}
      </Stack>
      <ConnectorDetailsDialog
        connector={detailConnector}
        open={Boolean(detailConnector)}
        busy={isMutating}
        testing={Boolean(
          detailConnector
            ? testingConnectorIds[detailConnector.definition.id]
            : false,
        )}
        testResult={
          detailConnector ? connectorTestResult(detailConnector) : undefined
        }
        auditEvents={detailAuditEvents}
        onClose={() => setDetailConnectorId(null)}
        onEnable={(item, enabled) =>
          void setConnectorEnabled(item.definition.id, enabled)
        }
        onDisconnect={(item) => void disconnectConnector(item.definition.id)}
        onTest={(item) =>
          void testConnectorConnection(item.definition.id, projectPath)
        }
        onEdit={openEditCustomConnector}
        onDelete={handleDeleteCustomConnector}
        onCopyEnv={handleCopyEnvSetup}
        onStartLogin={(item) => startConnectorLogin(item.definition.id)}
        onPollLogin={pollConnectorLogin}
      />
      <CustomConnectorDialog
        open={customDialogOpen}
        form={customForm}
        busy={isMutating}
        editing={Boolean(editingCustomConnectorId)}
        onChange={setCustomForm}
        onClose={closeCustomConnectorDialog}
        onSave={saveCustomConnector}
      />
      <CustomConnectorImportDialog
        open={importDialogOpen}
        value={importText}
        replaceExisting={replaceExistingImport}
        busy={isMutating}
        onChange={setImportText}
        onReplaceExistingChange={setReplaceExistingImport}
        onClose={closeImportCustomConnectors}
        onImport={handleImportCustomConnectors}
      />
    </Box>
  );
}
