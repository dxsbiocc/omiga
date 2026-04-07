import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  Alert,
  Avatar,
  Box,
  Button,
  Card,
  CardContent,
  Chip,
  CircularProgress,
  Collapse,
  Divider,
  FormControlLabel,
  IconButton,
  List,
  ListItem,
  ListItemText,
  Switch,
  Tab,
  Tabs,
  Tooltip,
  Typography,
} from "@mui/material";
import { alpha } from "@mui/material/styles";
import type { Theme } from "@mui/material/styles";
import AddIcon from "@mui/icons-material/Add";
import EditOutlinedIcon from "@mui/icons-material/EditOutlined";
import ExpandMoreIcon from "@mui/icons-material/ExpandMore";
import RefreshIcon from "@mui/icons-material/Refresh";
import DeleteOutlineIcon from "@mui/icons-material/DeleteOutline";
import { SkillPreviewDialog } from "./SkillPreviewDialog";

type McpToolCatalogEntry = {
  wireName: string;
  description: string;
};

type McpServerCatalogEntry = {
  configKey: string;
  normalizedKey: string;
  enabled: boolean;
  listToolsError: string | null;
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
  skillMdPath: string;
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

type SkillFilterTab = "all" | "user" | "project";

function normalizeSkillSource(sk: SkillCatalogEntry): SkillSource {
  return isSkillSource(sk.source) ? sk.source : "omigaProject";
}

function skillMatchesFilter(
  sk: SkillCatalogEntry,
  tab: SkillFilterTab,
): boolean {
  const src = normalizeSkillSource(sk);
  if (tab === "all") return true;
  if (tab === "user") return src === "claudeUser" || src === "omigaUser";
  return src === "omigaProject";
}

function mcpInitialLetter(name: string): string {
  const c = name.trim().charAt(0);
  return c ? c.toUpperCase() : "?";
}

function mcpRowSubtitle(srv: McpServerCatalogEntry): string {
  if (!srv.enabled) return "已禁用";
  if (srv.listToolsError) return "连接失败 · 展开查看详情";
  if (srv.tools.length === 0) return "未发现可用工具";
  return `${srv.tools.length} 个工具已启用`;
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
  const [skillPreview, setSkillPreview] = useState<SkillCatalogEntry | null>(
    null,
  );
  const [skillFilterTab, setSkillFilterTab] = useState<SkillFilterTab>("all");
  const [expandedMcp, setExpandedMcp] = useState<Record<string, boolean>>({});
  const noWorkspace = projectPath.trim().length === 0;
  const load = useCallback(
    async (options?: { ignoreCache?: boolean }) => {
      setLoading(true);
      setMessage(null);
      try {
        const c = await invoke<IntegrationsCatalog>(
          "get_integrations_catalog",
          {
            projectRoot: root,
            ignoreCache: options?.ignoreCache ?? false,
          },
        );
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
    },
    [root],
  );

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
        setMessage({
          kind: "success",
          text: "已保存到 .omiga/integrations.json，新对话将生效。",
        });
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
      const src: SkillSource = isSkillSource(sk.source)
        ? sk.source
        : "omigaProject";
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
          text: `已卸载：${sk.directoryName}`,
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
        <Typography variant="subtitle1" fontWeight={600}>
          当前已加载技能（启用 / 禁用）
        </Typography>
        {!showSkills && (
          <Button
            size="small"
            startIcon={
              loading ? <CircularProgress size={14} /> : <RefreshIcon />
            }
            disabled={loading || saving}
            onClick={() => void load({ ignoreCache: true })}
          >
            刷新
          </Button>
        )}
      </Box>

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
          <Typography
            variant="caption"
            color="text.secondary"
            fontWeight={600}
            letterSpacing="0.04em"
            textTransform="uppercase"
            sx={{ display: "block", mb: 1.5 }}
          >
            已安装的 MCP 服务
          </Typography>
          {catalog.mcpServers.length === 0 ? (
            <Typography variant="body2" color="text.secondary">
              未发现 MCP 配置（请检查 ~/.omiga/mcp.json 或项目
              .omiga/mcp.json，以及应用内置 bundled_mcp.json）。
            </Typography>
          ) : (
            <Box
              sx={(theme) => ({
                borderRadius: 2,
                border: `1px solid ${alpha(theme.palette.divider, theme.palette.mode === "dark" ? 0.9 : 1)}`,
                overflow: "hidden",
                bgcolor:
                  theme.palette.mode === "dark"
                    ? alpha(theme.palette.background.paper, 0.45)
                    : alpha(theme.palette.background.paper, 0.9),
              })}
            >
              {catalog.mcpServers.map((srv, idx) => {
                const expanded = expandedMcp[srv.normalizedKey] ?? false;
                const hasExpand =
                  (srv.tools.length > 0 || Boolean(srv.listToolsError)) &&
                  srv.enabled;
                const statusDot = (theme: Theme) => {
                  if (!srv.enabled) return theme.palette.action.disabled;
                  if (srv.listToolsError) return theme.palette.error.main;
                  return theme.palette.success.main;
                };
                return (
                  <Box key={srv.configKey}>
                    {idx > 0 && <Divider sx={{ opacity: 0.65 }} />}
                    <Box
                      sx={{
                        display: "flex",
                        alignItems: "center",
                        gap: 1.5,
                        px: 2,
                        py: 1.5,
                        minHeight: 64,
                      }}
                    >
                      <Box sx={{ position: "relative", flexShrink: 0 }}>
                        <Avatar
                          variant="rounded"
                          sx={(theme) => ({
                            width: 40,
                            height: 40,
                            fontSize: "1rem",
                            fontWeight: 700,
                            bgcolor: alpha(theme.palette.common.white, 0.08),
                            color: "text.primary",
                            border: `1px solid ${alpha(theme.palette.divider, 0.6)}`,
                          })}
                        >
                          {mcpInitialLetter(srv.configKey)}
                        </Avatar>
                        <Box
                          sx={(theme) => ({
                            position: "absolute",
                            right: -1,
                            bottom: -1,
                            width: 10,
                            height: 10,
                            borderRadius: "50%",
                            bgcolor: statusDot(theme),
                            border: `2px solid ${theme.palette.background.paper}`,
                            boxSizing: "border-box",
                          })}
                        />
                      </Box>
                      <Box sx={{ minWidth: 0, flex: 1 }}>
                        <Typography
                          fontWeight={700}
                          fontSize={15}
                          lineHeight={1.3}
                          noWrap
                          title={srv.configKey}
                          sx={{ color: "text.primary" }}
                        >
                          {srv.configKey}
                        </Typography>
                        <Box
                          sx={{
                            display: "flex",
                            alignItems: "center",
                            gap: 0.5,
                            mt: 0.25,
                          }}
                        >
                          <Typography
                            variant="caption"
                            color="text.secondary"
                            sx={{
                              fontSize: 12,
                              lineHeight: 1.35,
                              overflow: "hidden",
                              textOverflow: "ellipsis",
                              whiteSpace: "nowrap",
                            }}
                            title={srv.normalizedKey}
                          >
                            {mcpRowSubtitle(srv)}
                          </Typography>
                          {hasExpand && (
                            <IconButton
                              size="small"
                              aria-expanded={expanded}
                              aria-label={expanded ? "收起详情" : "展开详情"}
                              onClick={(e) => {
                                e.stopPropagation();
                                setExpandedMcp((p) => ({
                                  ...p,
                                  [srv.normalizedKey]: !expanded,
                                }));
                              }}
                              sx={{
                                p: 0.25,
                                color: "text.secondary",
                                transform: expanded ? "rotate(180deg)" : "none",
                                transition: "transform 0.2s ease",
                              }}
                            >
                              <ExpandMoreIcon sx={{ fontSize: 18 }} />
                            </IconButton>
                          )}
                        </Box>
                      </Box>
                      <Box
                        sx={{
                          display: "flex",
                          alignItems: "center",
                          gap: 0.25,
                          flexShrink: 0,
                        }}
                      >
                        <Tooltip title="编辑配置（即将推出）">
                          <span>
                            <IconButton
                              size="small"
                              disabled
                              sx={{
                                color: "text.disabled",
                                opacity: 0.45,
                              }}
                            >
                              <EditOutlinedIcon sx={{ fontSize: 18 }} />
                            </IconButton>
                          </span>
                        </Tooltip>
                        <Tooltip title="移除服务（即将推出）">
                          <span>
                            <IconButton
                              size="small"
                              disabled
                              sx={{
                                color: "text.disabled",
                                opacity: 0.45,
                              }}
                            >
                              <DeleteOutlineIcon sx={{ fontSize: 18 }} />
                            </IconButton>
                          </span>
                        </Tooltip>
                        <Switch
                          size="small"
                          color="success"
                          checked={srv.enabled}
                          disabled={saving}
                          onChange={(_, v) =>
                            setMcpEnabled(srv.normalizedKey, v)
                          }
                          inputProps={{
                            "aria-label": srv.enabled
                              ? "禁用 MCP 服务"
                              : "启用 MCP 服务",
                          }}
                          sx={{ ml: 0.5 }}
                        />
                      </Box>
                    </Box>
                    <Collapse
                      in={expanded && hasExpand}
                      timeout="auto"
                      unmountOnExit
                    >
                      <Box
                        sx={(theme) => ({
                          px: 2,
                          pb: 1.5,
                          pl: { xs: 2, sm: 9 },
                          borderTop: `1px solid ${alpha(theme.palette.divider, 0.5)}`,
                          bgcolor: alpha(theme.palette.common.black, 0.12),
                        })}
                      >
                        {srv.listToolsError && (
                          <Typography
                            variant="caption"
                            color="error"
                            sx={{
                              display: "block",
                              mb: srv.tools.length > 0 ? 1 : 0,
                              fontFamily:
                                "ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace",
                              whiteSpace: "pre-wrap",
                              wordBreak: "break-word",
                            }}
                          >
                            {srv.listToolsError}
                          </Typography>
                        )}
                        {srv.tools.length > 0 && (
                          <List
                            dense
                            disablePadding
                            sx={{ maxHeight: 220, overflow: "auto" }}
                          >
                            {srv.tools.map((t) => (
                              <ListItem
                                key={t.wireName}
                                sx={{
                                  py: 0.35,
                                  alignItems: "flex-start",
                                  px: 0,
                                }}
                              >
                                <ListItemText
                                  primary={
                                    <Typography
                                      variant="caption"
                                      fontFamily="monospace"
                                      component="span"
                                    >
                                      {t.wireName}
                                    </Typography>
                                  }
                                  secondary={t.description}
                                />
                              </ListItem>
                            ))}
                          </List>
                        )}
                      </Box>
                    </Collapse>
                  </Box>
                );
              })}
              <Divider sx={{ opacity: 0.65 }} />
              <Box
                sx={{
                  display: "flex",
                  alignItems: "center",
                  gap: 1.5,
                  px: 2,
                  py: 1.5,
                  minHeight: 64,
                  cursor: "default",
                }}
              >
                <Avatar
                  variant="rounded"
                  sx={(theme) => ({
                    width: 40,
                    height: 40,
                    bgcolor: alpha(theme.palette.success.main, 0.12),
                    color: "success.main",
                    border: `1px dashed ${alpha(theme.palette.success.main, 0.45)}`,
                  })}
                >
                  <AddIcon sx={{ fontSize: 22 }} />
                </Avatar>
                <Box sx={{ minWidth: 0 }}>
                  <Typography
                    fontWeight={700}
                    fontSize={15}
                    sx={{ color: "text.primary" }}
                  >
                    新建 MCP 服务
                  </Typography>
                  <Typography
                    variant="caption"
                    color="text.secondary"
                    sx={{ fontSize: 12 }}
                  >
                    使用上方「合并 JSON」或从 Claude 全局配置导入
                  </Typography>
                </Box>
              </Box>
            </Box>
          )}
        </Box>
      )}

      {catalog && showSkills && (
        <Box>
          <Typography variant="body2" fontWeight={600} sx={{ mb: 0.5 }}>
            Skills
          </Typography>

          <Box
            sx={{
              display: "flex",
              alignItems: "center",
              gap: 1,
              mb: 2,
              flexWrap: "wrap",
            }}
          >
            <Tabs
              value={skillFilterTab}
              onChange={(_, v) => setSkillFilterTab(v as SkillFilterTab)}
              aria-label="按技能来源筛选"
              sx={{
                flex: 1,
                minWidth: 0,
                minHeight: 40,
                "& .MuiTab-root": {
                  textTransform: "none",
                  fontWeight: 600,
                  fontSize: "0.875rem",
                },
              }}
            >
              <Tab label="全部" value="all" />
              <Tab label="用户级" value="user" />
              <Tab label="项目级" value="project" />
            </Tabs>
            <Button
              size="small"
              startIcon={
                loading ? <CircularProgress size={14} /> : <RefreshIcon />
              }
              disabled={loading || saving}
              onClick={() => void load({ ignoreCache: true })}
              sx={{ flexShrink: 0, alignSelf: "center" }}
            >
              刷新
            </Button>
          </Box>
          {catalog.skills.length === 0 ? (
            <Typography variant="body2" color="text.secondary">
              暂无技能。可经上方从 Claude 目录或任意文件夹导入到 Omiga，或手动放入
              ~/.omiga/skills、项目 .omiga/skills。
            </Typography>
          ) : (
            (() => {
              const visibleSkills = catalog.skills.filter((sk) =>
                skillMatchesFilter(sk, skillFilterTab),
              );
              if (visibleSkills.length === 0) {
                return (
                  <Typography variant="body2" color="text.secondary">
                    当前分类下暂无技能。
                  </Typography>
                );
              }
              return (
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
                  {visibleSkills.map((sk) => {
                    const src = normalizeSkillSource(sk);
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
                        key={sk.skillMdPath}
                        elevation={0}
                        sx={(theme) => ({
                          display: "flex",
                          flexDirection: "column",
                          height: "100%",
                          borderRadius: 3,
                          border: `1px solid ${alpha(
                            theme.palette.divider,
                            theme.palette.mode === "dark" ? 0.55 : 1,
                          )}`,
                          background:
                            theme.palette.mode === "dark"
                              ? alpha(theme.palette.background.paper, 0.85)
                              : theme.palette.background.paper,
                          boxShadow:
                            theme.palette.mode === "dark"
                              ? "0 2px 14px rgba(0,0,0,0.28)"
                              : "0 2px 14px rgba(15, 23, 42, 0.05)",
                          transition:
                            "transform 0.22s ease, box-shadow 0.22s ease",
                          "&:hover": {
                            transform: "translateY(-3px)",
                            boxShadow:
                              theme.palette.mode === "dark"
                                ? "0 14px 32px rgba(0,0,0,0.4)"
                                : "0 14px 36px rgba(15, 23, 42, 0.09)",
                          },
                        })}
                      >
                        <CardContent
                          onClick={() => setSkillPreview(sk)}
                          role="button"
                          tabIndex={0}
                          onKeyDown={(ev) => {
                            if (ev.key === "Enter" || ev.key === " ") {
                              ev.preventDefault();
                              setSkillPreview(sk);
                            }
                          }}
                          sx={(theme) => ({
                            flex: 1,
                            pb: 1.5,
                            pt: 2,
                            px: 2,
                            "&:last-child": { pb: 1.5 },
                            cursor: "pointer",
                            "&:hover": {
                              bgcolor: alpha(
                                theme.palette.text.primary,
                                theme.palette.mode === "dark" ? 0.05 : 0.03,
                              ),
                            },
                            "&:focus-visible": {
                              outline: `2px solid ${alpha(theme.palette.text.primary, 0.35)}`,
                              outlineOffset: 2,
                            },
                          })}
                        >
                          <Box
                            sx={{
                              display: "flex",
                              alignItems: "flex-start",
                              justifyContent: "space-between",
                              gap: 1.25,
                              mb: 1.25,
                            }}
                          >
                            <Typography
                              variant="subtitle1"
                              fontWeight={650}
                              sx={{
                                lineHeight: 1.35,
                                letterSpacing: "-0.02em",
                                wordBreak: "break-word",
                                fontSize: "1.02rem",
                              }}
                            >
                              {sk.name}
                            </Typography>
                            <Chip
                              size="small"
                              label={SKILL_SOURCE_LABEL[src]}
                              variant="outlined"
                              sx={(theme) => ({
                                flexShrink: 0,
                                maxWidth: "52%",
                                height: 24,
                                fontSize: "0.65rem",
                                fontWeight: 600,
                                letterSpacing: "0.06em",
                                textTransform: "uppercase",
                                borderColor: alpha(
                                  theme.palette.text.secondary,
                                  0.35,
                                ),
                                color: "text.secondary",
                                bgcolor: alpha(
                                  theme.palette.text.primary,
                                  0.02,
                                ),
                              })}
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
                              lineHeight: 1.65,
                              fontSize: "0.875rem",
                            }}
                          >
                            {sk.description || "—"}
                          </Typography>
                        </CardContent>
                        <Box
                          onClick={(e) => e.stopPropagation()}
                          onKeyDown={(e) => e.stopPropagation()}
                          sx={(theme) => ({
                            px: 2,
                            py: 1.25,
                            borderTop: `1px solid ${alpha(theme.palette.divider, 0.9)}`,
                            display: "flex",
                            alignItems: "center",
                            justifyContent: "space-between",
                            gap: 1,
                          })}
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
                              卸载
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
              );
            })()
          )}
        </Box>
      )}

      <SkillPreviewDialog
        key={skillPreview?.skillMdPath ?? "closed"}
        open={skillPreview !== null}
        skill={skillPreview}
        onClose={() => setSkillPreview(null)}
      />
    </Box>
  );
}
