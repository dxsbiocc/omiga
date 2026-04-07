import { Fragment, useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  Alert,
  Box,
  Button,
  Card,
  CardContent,
  Chip,
  CircularProgress,
  Divider,
  FormControlLabel,
  List,
  ListItem,
  ListItemText,
  Switch,
  Typography,
} from "@mui/material";
import RefreshIcon from "@mui/icons-material/Refresh";
import DeleteOutlineIcon from "@mui/icons-material/DeleteOutline";

type McpToolCatalogEntry = {
  wireName: string;
  description: string;
};

type McpServerCatalogEntry = {
  configKey: string;
  normalizedKey: string;
  enabled: boolean;
  tools: McpToolCatalogEntry[];
};

type SkillSource = "claudeUser" | "omigaUser" | "omigaProject";

const SKILL_SOURCE_LABEL: Record<SkillSource, string> = {
  claudeUser: "Claude ~/.claude",
  omigaUser: "用户 ~/.omiga",
  omigaProject: "项目 .omiga",
};

type SkillCatalogEntry = {
  name: string;
  description: string;
  enabled: boolean;
  source: SkillSource;
  directoryName: string;
  canUninstallOmigaCopy: boolean;
};

type IntegrationsCatalog = {
  mcpServers: McpServerCatalogEntry[];
  skills: SkillCatalogEntry[];
};

type PanelMode = "mcp" | "skills" | "both";

function resolveProjectPath(raw: string): string {
  const t = raw.trim();
  return t.length > 0 ? t : ".";
}

function isSkillSource(s: string): s is SkillSource {
  return s === "claudeUser" || s === "omigaUser" || s === "omigaProject";
}

export function IntegrationsCatalogPanel({
  projectPath,
  mode,
}: {
  projectPath: string;
  mode: PanelMode;
}) {
  const root = resolveProjectPath(projectPath);
  const [catalog, setCatalog] = useState<IntegrationsCatalog | null>(null);
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [message, setMessage] = useState<{
    kind: "success" | "error";
    text: string;
  } | null>(null);
  const [removingKey, setRemovingKey] = useState<string | null>(null);
  const noWorkspace = projectPath.trim().length === 0;
  const load = useCallback(async () => {
    setLoading(true);
    setMessage(null);
    try {
      const c = await invoke<IntegrationsCatalog>("get_integrations_catalog", {
        projectRoot: root,
      });
      setCatalog(c);
    } catch (e) {
      setCatalog(null);
      setMessage({
        kind: "error",
        text: e instanceof Error ? e.message : String(e),
      });
    } finally {
      setLoading(false);
    }
  }, [root]);

  useEffect(() => {
    void load();
  }, [load]);

  const persist = useCallback(
    async (next: IntegrationsCatalog) => {
      setSaving(true);
      setMessage(null);
      try {
        const disabledMcpServers = next.mcpServers
          .filter((s) => !s.enabled)
          .map((s) => s.normalizedKey);
        const disabledSkills = next.skills
          .filter((s) => !s.enabled)
          .map((s) => s.name);
        await invoke("save_integrations_state", {
          projectRoot: root,
          disabledMcpServers,
          disabledSkills,
        });
        setMessage({ kind: "success", text: "已保存到 .omiga/integrations.json，新对话将生效。" });
        setCatalog(next);
      } catch (e) {
        setMessage({
          kind: "error",
          text: e instanceof Error ? e.message : String(e),
        });
      } finally {
        setSaving(false);
      }
    },
    [root],
  );

  const setMcpEnabled = (normalizedKey: string, enabled: boolean) => {
    if (!catalog) return;
    const mcpServers = catalog.mcpServers.map((s) =>
      s.normalizedKey === normalizedKey ? { ...s, enabled } : s,
    );
    void persist({ ...catalog, mcpServers });
  };

  const setSkillEnabled = (name: string, enabled: boolean) => {
    if (!catalog) return;
    const skills = catalog.skills.map((s) =>
      s.name === name ? { ...s, enabled } : s,
    );
    void persist({ ...catalog, skills });
  };

  const uninstallOmigaSkillCopy = useCallback(
    async (sk: SkillCatalogEntry) => {
      if (!sk.canUninstallOmigaCopy || !sk.directoryName) return;
      const src: SkillSource = isSkillSource(sk.source) ? sk.source : "omigaProject";
      if (src === "omigaProject" && noWorkspace) return;
      const target = src === "omigaUser" ? "userOmiga" : "projectOmiga";
      const rk = `${target}:${sk.directoryName}`;
      if (
        !window.confirm(
          `确定删除 Omiga 目录下的技能副本「${sk.directoryName}」？\n（不会删除 ~/.claude/skills 中的文件）`,
        )
      ) {
        return;
      }
      setMessage(null);
      setRemovingKey(rk);
      try {
        await invoke("remove_omiga_imported_skill", {
          projectRoot: root,
          directoryName: sk.directoryName,
          target,
        });
        setMessage({
          kind: "success",
          text: `已卸载副本：${sk.directoryName}`,
        });
        await load();
      } catch (e) {
        setMessage({
          kind: "error",
          text: e instanceof Error ? e.message : String(e),
        });
      } finally {
        setRemovingKey(null);
      }
    },
    [root, load, noWorkspace],
  );

  const showMcp = mode === "mcp" || mode === "both";
  const showSkills = mode === "skills" || mode === "both";

  return (
    <Box sx={{ mt: 2 }}>
      <Box
        sx={{
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
          gap: 1,
          mb: 1,
        }}
      >
        <Typography variant="subtitle2" fontWeight={600}>
          当前已加载项（启用 / 禁用）
        </Typography>
        <Button
          size="small"
          startIcon={loading ? <CircularProgress size={14} /> : <RefreshIcon />}
          disabled={loading || saving}
          onClick={() => void load()}
        >
          刷新
        </Button>
      </Box>
      <Typography variant="caption" color="text.secondary" display="block" sx={{ mb: 1 }}>
        禁用后将从模型工具列表与技能说明中移除；MCP 的 resources 与动态工具调用也会被拦截。配置保存在{" "}
        <Typography component="span" fontFamily="monospace" fontSize="0.85em">
          .omiga/integrations.json
        </Typography>
        。
      </Typography>

      {message && (
        <Alert
          severity={message.kind === "success" ? "success" : "error"}
          sx={{ mb: 2, borderRadius: 2 }}
          onClose={() => setMessage(null)}
        >
          {message.text}
        </Alert>
      )}

      {loading && !catalog && (
        <Box sx={{ py: 2, display: "flex", justifyContent: "center" }}>
          <CircularProgress size={28} />
        </Box>
      )}

      {catalog && showMcp && (
        <Box sx={{ mb: showSkills ? 3 : 0 }}>
          <Typography variant="body2" fontWeight={600} sx={{ mb: 1 }}>
            MCP 服务
          </Typography>
          {catalog.mcpServers.length === 0 ? (
            <Typography variant="body2" color="text.secondary">
              未发现 MCP 配置（请检查 ~/.omiga/mcp.json 或项目 .omiga/mcp.json，以及应用内置 bundled_mcp.json）。
            </Typography>
          ) : (
            <Fragment>
            {catalog.mcpServers.map((srv) => (
              <Box
                key={srv.configKey}
                sx={{
                  border: 1,
                  borderColor: "divider",
                  borderRadius: 1,
                  mb: 1,
                  overflow: "hidden",
                }}
              >
                <Box
                  sx={{
                    display: "flex",
                    alignItems: "center",
                    justifyContent: "space-between",
                    gap: 1,
                    px: 1.5,
                    py: 1,
                    bgcolor: "action.hover",
                  }}
                >
                  <Box sx={{ minWidth: 0 }}>
                    <Typography fontWeight={600} noWrap title={srv.configKey}>
                      {srv.configKey}
                    </Typography>
                    <Typography variant="caption" color="text.secondary" display="block" noWrap>
                      {srv.normalizedKey} · {srv.tools.length} tools
                    </Typography>
                  </Box>
                  <FormControlLabel
                    control={
                      <Switch
                        size="small"
                        checked={srv.enabled}
                        disabled={saving}
                        onChange={(_, v) => setMcpEnabled(srv.normalizedKey, v)}
                      />
                    }
                    label={srv.enabled ? "启用" : "禁用"}
                  />
                </Box>
                <Divider />
                <List dense disablePadding sx={{ maxHeight: 220, overflow: "auto" }}>
                  {srv.tools.map((t) => (
                    <ListItem key={t.wireName} sx={{ py: 0.35, alignItems: "flex-start" }}>
                      <ListItemText
                        primary={
                          <Typography variant="caption" fontFamily="monospace" component="span">
                            {t.wireName}
                          </Typography>
                        }
                        secondary={t.description}
                      />
                    </ListItem>
                  ))}
                </List>
              </Box>
            ))}
            </Fragment>
          )}
        </Box>
      )}

      {catalog && showSkills && (
        <Box>
          <Typography variant="body2" fontWeight={600} sx={{ mb: 0.5 }}>
            Skills
          </Typography>
          <Typography variant="caption" color="text.secondary" display="block" sx={{ mb: 1.5 }}>
            标签为「用户 ~/.omiga」或「项目 .omiga」的卡片可「卸载副本」（仅删除对应目录，不影响 ~/.claude/skills）。
          </Typography>
          {catalog.skills.length === 0 ? (
            <Typography variant="body2" color="text.secondary">
              未发现技能（请将技能放在 ~/.omiga/skills 或项目 .omiga/skills；~/.claude/skills 需在上方打开「在会话中加载」开关，或使用从文件夹复制）。
            </Typography>
          ) : (
            <Box
              sx={{
                display: "grid",
                gridTemplateColumns: {
                  xs: "1fr",
                  sm: "repeat(2, minmax(0, 1fr))",
                  md: "repeat(3, minmax(0, 1fr))",
                },
                gap: 1.5,
              }}
            >
              {catalog.skills.map((sk) => {
                const src: SkillSource = isSkillSource(sk.source) ? sk.source : "omigaProject";
                const chipColor =
                  src === "omigaUser" ? "primary" : src === "claudeUser" ? "secondary" : "default";
                const chipVariant = src === "omigaProject" ? "outlined" : "filled";
                const showUninstall =
                  sk.canUninstallOmigaCopy &&
                  sk.directoryName &&
                  (src !== "omigaProject" || !noWorkspace);
                const rk =
                  src === "omigaUser"
                    ? `userOmiga:${sk.directoryName}`
                    : `projectOmiga:${sk.directoryName}`;
                const busyRm = removingKey === rk;
                return (
                  <Card
                    key={sk.name}
                    variant="outlined"
                    sx={{
                      display: "flex",
                      flexDirection: "column",
                      height: "100%",
                      borderRadius: 2,
                      transition: "box-shadow 0.2s ease",
                      "&:hover": { boxShadow: 2 },
                    }}
                  >
                    <CardContent sx={{ flex: 1, pb: 1, "&:last-child": { pb: 1 } }}>
                      <Box
                        sx={{
                          display: "flex",
                          alignItems: "flex-start",
                          justifyContent: "space-between",
                          gap: 1,
                          mb: 1,
                        }}
                      >
                        <Typography
                          variant="subtitle2"
                          fontWeight={700}
                          sx={{
                            lineHeight: 1.3,
                            wordBreak: "break-word",
                          }}
                        >
                          {sk.name}
                        </Typography>
                        <Chip
                          size="small"
                          label={SKILL_SOURCE_LABEL[src]}
                          color={chipColor}
                          variant={chipVariant}
                          sx={{ flexShrink: 0, maxWidth: "50%" }}
                        />
                      </Box>
                      <Typography
                        variant="body2"
                        color="text.secondary"
                        sx={{
                          display: "-webkit-box",
                          WebkitLineClamp: 4,
                          WebkitBoxOrient: "vertical",
                          overflow: "hidden",
                          minHeight: "4.5em",
                        }}
                      >
                        {sk.description || "—"}
                      </Typography>
                    </CardContent>
                    <Box
                      sx={{
                        px: 2,
                        py: 1,
                        borderTop: 1,
                        borderColor: "divider",
                        display: "flex",
                        alignItems: "center",
                        justifyContent: "space-between",
                        gap: 1,
                        bgcolor: "action.hover",
                      }}
                    >
                      {showUninstall ? (
                        <Button
                          size="small"
                          color="error"
                          variant="text"
                          disabled={saving || busyRm}
                          startIcon={
                            busyRm ? (
                              <CircularProgress size={14} />
                            ) : (
                              <DeleteOutlineIcon fontSize="small" />
                            )
                          }
                          onClick={() => void uninstallOmigaSkillCopy(sk)}
                          sx={{ flexShrink: 0 }}
                        >
                          卸载副本
                        </Button>
                      ) : (
                        <Box sx={{ minWidth: 0 }} />
                      )}
                      <FormControlLabel
                        sx={{ m: 0, flexShrink: 0 }}
                        control={
                          <Switch
                            size="small"
                            checked={sk.enabled}
                            disabled={saving}
                            onChange={(_, v) => setSkillEnabled(sk.name, v)}
                          />
                        }
                        label={sk.enabled ? "启用" : "禁用"}
                      />
                    </Box>
                  </Card>
                );
              })}
            </Box>
          )}
        </Box>
      )}
    </Box>
  );
}
