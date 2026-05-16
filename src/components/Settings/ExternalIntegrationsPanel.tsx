import { useDeferredValue, useEffect, useMemo, useState } from "react";
import {
  Alert,
  Box,
  Button,
  Chip,
  CircularProgress,
  Divider,
  Paper,
  Stack,
  TextField,
  ToggleButton,
  ToggleButtonGroup,
  Typography,
} from "@mui/material";
import { alpha, useTheme } from "@mui/material/styles";
import {
  LinkRounded,
  RefreshRounded,
  SearchRounded,
  SettingsEthernetRounded,
  SyncRounded,
} from "@mui/icons-material";
import {
  buildConnectorLikeBadges,
  buildMcpServerBadges,
  connectorStatusLabel,
  selectConnectorLikeItems,
  selectExternalIntegrationSummary,
  selectMcpServerRows,
  useExternalIntegrationStore,
  type ExternalConnectorLikeItem,
  type ExternalMcpServerItem,
} from "../../state/externalIntegrationStore";
import { ClaudeCodeImportPanel } from "./ClaudeCodeImportPanel";
import { ConnectorsPanel } from "./ConnectorsPanel";
import { IntegrationsCatalogPanel } from "./IntegrationsCatalogPanel";

const connectorGridSx = {
  display: "grid",
  gridTemplateColumns: { xs: "1fr", xl: "repeat(2, minmax(0, 1fr))" },
  gap: 1,
};

type PanelFilter = "all" | "connectors" | "mcp";

function descriptionText(text: string): string {
  return text.trim() || "No description";
}

function protocolSummary(item: ExternalMcpServerItem): string {
  if (item.protocol === "http") {
    return item.config.url || "Remote HTTP MCP";
  }
  return [item.config.command, ...item.config.args].filter(Boolean).join(" ");
}

function ConnectorCard({ item }: { item: ExternalConnectorLikeItem }) {
  const theme = useTheme();
  const badges = buildConnectorLikeBadges(item);

  return (
    <Paper
      variant="outlined"
      sx={{
        p: 1.5,
        borderRadius: 2,
        bgcolor:
          theme.palette.mode === "dark"
            ? alpha(theme.palette.background.paper, 0.72)
            : theme.palette.background.paper,
      }}
    >
      <Stack spacing={1}>
        <Stack
          direction="row"
          spacing={1}
          alignItems="flex-start"
          justifyContent="space-between"
        >
          <Stack spacing={0.25} sx={{ minWidth: 0 }}>
            <Typography variant="subtitle2">{item.displayName}</Typography>
            <Typography variant="caption" color="text.secondary">
              {descriptionText(item.description)}
            </Typography>
          </Stack>
          <Chip
            size="small"
            icon={<LinkRounded sx={{ fontSize: 16 }} />}
            label={item.kind === "mcp_backed_connector" ? "Unified" : "Connector"}
            color={item.kind === "mcp_backed_connector" ? "info" : "default"}
            variant="outlined"
          />
        </Stack>

        <Stack direction="row" spacing={0.75} useFlexGap flexWrap="wrap">
          {badges.map((badge) => (
            <Chip
              key={`${item.id}-${badge.label}`}
              size="small"
              label={badge.label}
              color={badge.tone}
              variant={badge.tone === "default" ? "outlined" : "filled"}
            />
          ))}
        </Stack>

        <Stack spacing={0.4}>
          <Typography variant="caption" color="text.secondary">
            Status: {connectorStatusLabel(item.status)}
          </Typography>
          <Typography variant="caption" color="text.secondary">
            Auth: {item.authType}
            {item.accountLabel ? ` · ${item.accountLabel}` : ""}
          </Typography>
          <Typography variant="caption" color="text.secondary">
            Tools: {item.nativeToolCount} native, {item.externalToolCount} MCP
          </Typography>
          {item.mcpServers.length > 0 ? (
            <Typography variant="caption" color="text.secondary">
              MCP: {item.mcpServers.map((server) => server.configKey).join(", ")}
            </Typography>
          ) : null}
        </Stack>

        {item.lastError ? (
          <Alert severity="error" sx={{ py: 0 }}>
            {item.lastError}
          </Alert>
        ) : null}
      </Stack>
    </Paper>
  );
}

function McpRow({ item }: { item: ExternalMcpServerItem }) {
  const theme = useTheme();
  const badges = buildMcpServerBadges(item);

  return (
    <Paper
      variant="outlined"
      sx={{
        p: 1.5,
        borderRadius: 2,
        bgcolor:
          theme.palette.mode === "dark"
            ? alpha(theme.palette.background.paper, 0.72)
            : theme.palette.background.paper,
      }}
    >
      <Stack spacing={1}>
        <Stack
          direction="row"
          spacing={1}
          alignItems="flex-start"
          justifyContent="space-between"
        >
          <Stack spacing={0.25} sx={{ minWidth: 0 }}>
            <Typography variant="subtitle2">{item.displayName}</Typography>
            <Typography
              variant="caption"
              color="text.secondary"
              sx={{
                fontFamily:
                  "ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace",
                overflowWrap: "anywhere",
              }}
            >
              {protocolSummary(item) || descriptionText(item.description)}
            </Typography>
          </Stack>
          <Chip
            size="small"
            icon={<SettingsEthernetRounded sx={{ fontSize: 16 }} />}
            label="MCP"
            variant="outlined"
          />
        </Stack>

        <Stack direction="row" spacing={0.75} useFlexGap flexWrap="wrap">
          {badges.map((badge) => (
            <Chip
              key={`${item.id}-${badge.label}`}
              size="small"
              label={badge.label}
              color={badge.tone}
              variant={badge.tone === "default" ? "outlined" : "filled"}
            />
          ))}
        </Stack>

        {item.linkedConnectors.length > 0 ? (
          <Typography variant="caption" color="text.secondary">
            Linked connectors:{" "}
            {item.linkedConnectors.map((connector) => connector.displayName).join(", ")}
          </Typography>
        ) : (
          <Typography variant="caption" color="text.secondary">
            Standalone MCP server row.
          </Typography>
        )}

        {item.listToolsError ? (
          <Alert severity="error" sx={{ py: 0 }}>
            {item.listToolsError}
          </Alert>
        ) : null}
      </Stack>
    </Paper>
  );
}

export function ExternalIntegrationsPanel({
  projectPath,
  initialView,
}: {
  projectPath: string;
  initialView?: string;
}) {
  const theme = useTheme();
  const { catalog, isLoading, error, loadCatalog, clearError } =
    useExternalIntegrationStore();
  const [searchQuery, setSearchQuery] = useState("");
  const [filter, setFilter] = useState<PanelFilter>(() => {
    if (initialView === "connectors") return "connectors";
    if (initialView === "mcp") return "mcp";
    return "all";
  });
  const deferredSearchQuery = useDeferredValue(searchQuery);
  const hasCatalog = Boolean(catalog);

  useEffect(() => {
    void loadCatalog(projectPath, { background: hasCatalog, probeTools: false });
  }, [hasCatalog, loadCatalog, projectPath]);

  const connectorItems = useMemo(
    () => selectConnectorLikeItems(catalog, { search: deferredSearchQuery }),
    [catalog, deferredSearchQuery],
  );
  const mcpItems = useMemo(
    () => selectMcpServerRows(catalog, { search: deferredSearchQuery }),
    [catalog, deferredSearchQuery],
  );
  const summary = useMemo(
    () => selectExternalIntegrationSummary(catalog),
    [catalog],
  );

  const showConnectors = filter !== "mcp";
  const showMcp = filter !== "connectors";

  return (
    <Stack spacing={1.5}>
      <Paper
        variant="outlined"
        sx={{
          p: 1.5,
          borderRadius: 2,
          background:
            theme.palette.mode === "dark"
              ? `linear-gradient(135deg, ${alpha(theme.palette.info.main, 0.14)} 0%, ${alpha(theme.palette.background.paper, 0.72)} 100%)`
              : `linear-gradient(135deg, ${alpha(theme.palette.info.light, 0.18)} 0%, ${theme.palette.background.paper} 100%)`,
        }}
      >
        <Stack spacing={1.25}>
          <Stack
            direction={{ xs: "column", md: "row" }}
            spacing={1}
            justifyContent="space-between"
            alignItems={{ xs: "stretch", md: "center" }}
          >
            <Box>
              <Typography variant="subtitle1">External Integrations</Typography>
              <Typography variant="body2" color="text.secondary">
                Services and MCP servers in this workspace.
              </Typography>
            </Box>
            <Stack direction="row" spacing={1}>
              <Button
                variant="outlined"
                size="small"
                startIcon={<RefreshRounded />}
                onClick={() =>
                  void loadCatalog(projectPath, {
                    force: true,
                    probeTools: true,
                  })
                }
              >
                Refresh
              </Button>
            </Stack>
          </Stack>

          <Stack direction="row" spacing={0.75} useFlexGap flexWrap="wrap">
            <Chip size="small" label={`${summary.connectorLikeCount} connectors`} />
            <Chip size="small" label={`${summary.mcpServerCount} MCP`} />
            <Chip
              size="small"
              color="success"
              label={`${summary.connectedCount} connected`}
            />
            <Chip
              size="small"
              color={summary.issueCount > 0 ? "warning" : "default"}
              label={`${summary.issueCount} issues`}
            />
            {catalog ? (
              <Chip
                size="small"
                variant="outlined"
                icon={<SyncRounded sx={{ fontSize: 16 }} />}
                label={
                  catalog.source === "unified_command"
                    ? "Unified command"
                    : "Legacy fallback"
                }
              />
            ) : null}
          </Stack>

          <Stack
            direction={{ xs: "column", md: "row" }}
            spacing={1}
            alignItems={{ xs: "stretch", md: "center" }}
          >
            <TextField
              size="small"
              fullWidth
              placeholder="Search connectors, MCP servers, tools"
              value={searchQuery}
              onChange={(event) => setSearchQuery(event.target.value)}
              InputProps={{
                startAdornment: <SearchRounded sx={{ mr: 1, fontSize: 18 }} />,
              }}
            />
            <ToggleButtonGroup
              exclusive
              size="small"
              color="primary"
              value={filter}
              onChange={(_, value: PanelFilter | null) => {
                if (value) setFilter(value);
              }}
            >
              <ToggleButton value="all">All</ToggleButton>
              <ToggleButton value="connectors">Connectors</ToggleButton>
              <ToggleButton value="mcp">MCP</ToggleButton>
            </ToggleButtonGroup>
          </Stack>
        </Stack>
      </Paper>

      {error ? (
        <Alert severity="error" onClose={clearError}>
          {error}
        </Alert>
      ) : null}

      {catalog?.notes.length ? (
        <Alert severity="info">
          {catalog.notes.join(" ")}
        </Alert>
      ) : null}

      {isLoading && !catalog ? (
        <Paper
          variant="outlined"
          sx={{ p: 3, borderRadius: 2, display: "grid", placeItems: "center" }}
        >
          <Stack spacing={1} alignItems="center">
            <CircularProgress size={24} />
            <Typography variant="body2" color="text.secondary">
              Loading external integrations…
            </Typography>
          </Stack>
        </Paper>
      ) : null}

      {!isLoading && catalog && summary.totalItems === 0 ? (
        <Paper variant="outlined" sx={{ p: 2, borderRadius: 2 }}>
          <Typography variant="body2" color="text.secondary">
            No external integrations are currently visible in this workspace.
          </Typography>
        </Paper>
      ) : null}

      {showConnectors ? (
        <Stack spacing={1}>
          <Stack
            direction="row"
            alignItems="center"
            justifyContent="space-between"
          >
            <Typography variant="subtitle2">Services</Typography>
            <Typography variant="caption" color="text.secondary">
              Account and service links
            </Typography>
          </Stack>
          {connectorItems.length > 0 ? (
            <Box sx={connectorGridSx}>
              {connectorItems.map((item) => (
                <ConnectorCard key={item.id} item={item} />
              ))}
            </Box>
          ) : (
            <Paper variant="outlined" sx={{ p: 2, borderRadius: 2 }}>
              <Typography variant="body2" color="text.secondary">
                No connector-like items match the current filter.
              </Typography>
            </Paper>
          )}
        </Stack>
      ) : null}

      {showConnectors && showMcp ? <Divider /> : null}

      {showMcp ? (
        <Stack spacing={1}>
          <Stack
            direction="row"
            alignItems="center"
            justifyContent="space-between"
          >
            <Typography variant="subtitle2">MCP Servers</Typography>
            <Typography variant="caption" color="text.secondary">
              Protocol servers
            </Typography>
          </Stack>
          {mcpItems.length > 0 ? (
            <Stack spacing={1}>
              {mcpItems.map((item) => (
                <McpRow key={item.id} item={item} />
              ))}
            </Stack>
          ) : (
            <Paper variant="outlined" sx={{ p: 2, borderRadius: 2 }}>
              <Typography variant="body2" color="text.secondary">
                No MCP servers match the current filter.
              </Typography>
            </Paper>
          )}
        </Stack>
      ) : null}

      <Divider />
      <Stack spacing={1}>
        <Typography variant="subtitle2">Actions</Typography>
      </Stack>
      {showConnectors ? (
        <Box>
          <ConnectorsPanel projectPath={projectPath} />
        </Box>
      ) : null}
      {showConnectors && showMcp ? <Divider /> : null}
      {showMcp ? (
        <Box>
          <IntegrationsCatalogPanel projectPath={projectPath} mode="mcp" />
          <Box sx={{ mt: 3 }}>
            <Typography variant="subtitle1" fontWeight={650}>
              导入已有 MCP JSON
            </Typography>
            <Typography
              variant="body2"
              color="text.secondary"
              sx={{ mt: 0.5 }}
            >
              如果已经有符合 mcpServers 格式的 JSON 文件，可以合并到当前项目
              .omiga/mcp.json。
            </Typography>
            <ClaudeCodeImportPanel projectPath={projectPath} mode="mcp" />
          </Box>
        </Box>
      ) : null}
    </Stack>
  );
}
