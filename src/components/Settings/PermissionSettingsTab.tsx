import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ComponentType,
  type MouseEvent,
} from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  Alert,
  Box,
  Button,
  Card,
  Checkbox,
  Chip,
  CircularProgress,
  Divider,
  InputAdornment,
  Stack,
  TextField,
  Typography,
} from "@mui/material";
import { alpha, useTheme } from "@mui/material/styles";
import type { SvgIconProps } from "@mui/material/SvgIcon";
import {
  Save as SaveIcon,
  Shield as ShieldIcon,
  Terminal as TerminalIcon,
  Article as ArticleIcon,
  EditNote as EditNoteIcon,
  Edit as EditIcon,
  Search as SearchIcon,
  FolderOpen as FolderOpenIcon,
  Language as LanguageIcon,
  TravelExplore as TravelExploreIcon,
  SmartToy as SmartToyIcon,
  AutoAwesome as AutoAwesomeIcon,
  Checklist as ChecklistIcon,
  MenuBook as MenuBookIcon,
  HelpOutline as HelpOutlineIcon,
  ListAlt as ListAltIcon,
  ImportContacts as ImportContactsIcon,
  StopCircle as StopCircleIcon,
  Widgets as WidgetsIcon,
  History as HistoryIcon,
  CheckCircle as CheckCircleIcon,
  Cancel as CancelIcon,
  Refresh as RefreshIcon,
} from "@mui/icons-material";
import {
  PERMISSION_PRESETS,
  buildDenyList,
  parseDenyIntoState,
  type PermissionPreset,
} from "./permissionPresets";
import { isUnsetWorkspacePath } from "../../state/sessionStore";

type PermissionSettingsTabProps = {
  projectPath: string;
};

type AuditDecisionFilter = "all" | "approved" | "denied";

type PermissionAuditEvent = {
  id: string;
  sessionId: string;
  requestId?: string | null;
  projectRoot?: string | null;
  decision: "approved" | "denied" | string;
  toolName: string;
  mode?: string | null;
  reason?: string | null;
  timestamp: string;
};

type PermissionAuditEventsResponse = {
  events: PermissionAuditEvent[];
  totalCount: number;
  approvedCount: number;
  deniedCount: number;
};

const AUDIT_PAGE_SIZE = 20;

const PRESET_ICONS: Record<string, ComponentType<SvgIconProps>> = {
  Bash: TerminalIcon,
  Read: ArticleIcon,
  Write: EditNoteIcon,
  Edit: EditIcon,
  Ripgrep: SearchIcon,
  Grep: SearchIcon,
  Glob: FolderOpenIcon,
  Fetch: LanguageIcon,
  Search: TravelExploreIcon,
  Agent: SmartToyIcon,
  skill: AutoAwesomeIcon,
  TodoWrite: ChecklistIcon,
  NotebookEdit: MenuBookIcon,
  AskUserQuestion: HelpOutlineIcon,
  ListMcpResourcesTool: ListAltIcon,
  ReadMcpResourceTool: ImportContactsIcon,
  TaskStop: StopCircleIcon,
};

function PresetIcon({ rule }: { rule: string }) {
  const Icon = PRESET_ICONS[rule] ?? WidgetsIcon;
  return <Icon sx={{ fontSize: 22 }} aria-hidden />;
}

type PermissionPresetCardProps = {
  preset: PermissionPreset;
  checked: boolean;
  disabled: boolean;
  onToggle: () => void;
};

function PermissionPresetCard({
  preset,
  checked,
  disabled,
  onToggle,
}: PermissionPresetCardProps) {
  const theme = useTheme();
  const accent = theme.palette.primary.main;
  const err = theme.palette.error.main;
  const isDark = theme.palette.mode === "dark";

  const handleCardBodyClick = (e: MouseEvent<HTMLElement>) => {
    if (disabled) return;
    const t = e.target as HTMLElement;
    if (t.closest('input[type="checkbox"]') || t.closest("label")) return;
    onToggle();
  };

  return (
    <Card
      variant="outlined"
      sx={{
        position: "relative",
        overflow: "hidden",
        borderRadius: 2.5,
        borderColor: checked
          ? alpha(err, isDark ? 0.45 : 0.35)
          : alpha(theme.palette.divider, isDark ? 0.9 : 1),
        bgcolor: checked
          ? alpha(err, isDark ? 0.08 : 0.06)
          : alpha(theme.palette.background.paper, 1),
        transition:
          "border-color 0.2s ease, box-shadow 0.2s ease, transform 0.2s ease, background-color 0.2s ease",
        "@media (prefers-reduced-motion: reduce)": {
          transition: "border-color 0.2s ease, background-color 0.2s ease",
        },
        "&:hover": disabled
          ? {}
          : {
              borderColor: alpha(accent, 0.45),
              boxShadow: `0 4px 20px ${alpha(accent, isDark ? 0.12 : 0.08)}`,
              transform: "translateY(-1px)",
              "@media (prefers-reduced-motion: reduce)": {
                transform: "none",
              },
            },
        opacity: disabled ? 0.55 : 1,
      }}
    >
      <Box
        role={disabled ? undefined : "button"}
        tabIndex={disabled ? undefined : 0}
        onClick={handleCardBodyClick}
        onKeyDown={(e) => {
          if (disabled) return;
          if (e.key === "Enter" || e.key === " ") {
            e.preventDefault();
            onToggle();
          }
        }}
        sx={{
          p: 1.5,
          cursor: disabled ? "not-allowed" : "pointer",
          outline: "none",
          "&:focus-visible": {
            boxShadow: (t) => `inset 0 0 0 2px ${alpha(t.palette.primary.main, 0.5)}`,
          },
        }}
      >
        <Stack direction="row" spacing={1.25} alignItems="flex-start">
          <Box
            sx={{
              width: 44,
              height: 44,
              borderRadius: 2,
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              flexShrink: 0,
              bgcolor: checked
                ? alpha(err, 0.12)
                : alpha(accent, isDark ? 0.12 : 0.08),
              color: checked ? err : accent,
              transition: "background-color 0.2s ease, color 0.2s ease",
            }}
          >
            <PresetIcon rule={preset.rule} />
          </Box>
          <Box sx={{ flex: 1, minWidth: 0, pt: 0.25 }}>
            <Stack
              direction="row"
              alignItems="center"
              justifyContent="space-between"
              gap={1}
            >
              <Typography
                variant="body2"
                fontWeight={700}
                sx={{
                  lineHeight: 1.35,
                  letterSpacing: "-0.01em",
                  color: "text.primary",
                }}
              >
                {preset.label}
              </Typography>
              <Checkbox
                size="small"
                checked={checked}
                disabled={disabled}
                onChange={onToggle}
                onClick={(e) => e.stopPropagation()}
                onKeyDown={(e) => e.stopPropagation()}
                inputProps={{
                  "aria-label": `${checked ? "取消禁止" : "禁止"} ${preset.label}`,
                }}
                sx={{
                  p: 0.25,
                  color: alpha(err, 0.45),
                  "&.Mui-checked": { color: err },
                }}
              />
            </Stack>
            <Typography
              variant="caption"
              color="text.secondary"
              sx={{ display: "block", mt: 0.5, lineHeight: 1.45 }}
            >
              规则{" "}
              <Box
                component="code"
                sx={{
                  fontFamily: "ui-monospace, SFMono-Regular, Menlo, monospace",
                  fontSize: "0.72rem",
                  px: 0.5,
                  py: 0.1,
                  borderRadius: 0.75,
                  bgcolor: alpha(theme.palette.text.primary, 0.06),
                }}
              >
                {preset.rule}
              </Box>
            </Typography>
            {checked ? (
              <Chip
                size="small"
                label="已禁止"
                sx={{
                  mt: 1,
                  height: 22,
                  fontSize: "0.7rem",
                  fontWeight: 700,
                  bgcolor: alpha(err, 0.12),
                  color: err,
                  border: `1px solid ${alpha(err, 0.25)}`,
                }}
              />
            ) : null}
          </Box>
        </Stack>
      </Box>
    </Card>
  );
}

export function PermissionSettingsTab({ projectPath }: PermissionSettingsTabProps) {
  const theme = useTheme();
  const accent = theme.palette.primary.main;
  const isDark = theme.palette.mode === "dark";

  const [presetChecked, setPresetChecked] = useState<Record<string, boolean>>({});
  const [customBlock, setCustomBlock] = useState("");
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [auditLoading, setAuditLoading] = useState(false);
  const [auditEvents, setAuditEvents] = useState<PermissionAuditEvent[]>([]);
  const [auditTotalCount, setAuditTotalCount] = useState(0);
  const [auditApprovedCount, setAuditApprovedCount] = useState(0);
  const [auditDeniedCount, setAuditDeniedCount] = useState(0);
  const [auditDecisionFilter, setAuditDecisionFilter] =
    useState<AuditDecisionFilter>("all");
  const [auditToolFilter, setAuditToolFilter] = useState("");
  const [auditToolQuery, setAuditToolQuery] = useState("");
  const [auditPage, setAuditPage] = useState(1);
  const [message, setMessage] = useState<{
    type: "success" | "error";
    text: string;
  } | null>(null);
  const auditRequestSeq = useRef(0);
  const previousAuditQueryKey = useRef<string | null>(null);
  const auditHasMore = auditPage * AUDIT_PAGE_SIZE < auditTotalCount;

  const blockedCount = useMemo(
    () => Object.values(presetChecked).filter(Boolean).length,
    [presetChecked],
  );
  const auditPageSummary = useMemo(
    () =>
      auditEvents.reduce(
        (summary, event) => {
          if (event.decision === "approved") {
            summary.approved += 1;
          } else if (event.decision === "denied") {
            summary.denied += 1;
          } else {
            summary.other += 1;
          }
          return summary;
        },
        { approved: 0, denied: 0, other: 0 },
      ),
    [auditEvents],
  );
  const hasActiveAuditFilters =
    auditDecisionFilter !== "all" || auditToolQuery.length > 0;
  const auditFilterSummary = useMemo(() => {
    const parts: string[] = [];
    if (auditDecisionFilter === "approved") {
      parts.push("仅看批准");
    } else if (auditDecisionFilter === "denied") {
      parts.push("仅看拒绝");
    } else {
      parts.push("全部决策");
    }
    if (auditToolQuery.length > 0) {
      parts.push(`工具包含“${auditToolQuery}”`);
    }
    return parts.join(" / ");
  }, [auditDecisionFilter, auditToolQuery]);
  const auditPageSummaryText = useMemo(() => {
    const parts = [
      `本页 ${auditEvents.length} 条`,
      `批准 ${auditPageSummary.approved}`,
      `拒绝 ${auditPageSummary.denied}`,
    ];
    if (auditPageSummary.other > 0) {
      parts.push(`其他 ${auditPageSummary.other}`);
    }
    return parts.join(" / ");
  }, [auditEvents.length, auditPageSummary]);
  const auditFacetSummaryText = useMemo(() => {
    return `总计 ${auditTotalCount} 条 / 批准 ${auditApprovedCount} / 拒绝 ${auditDeniedCount}`;
  }, [auditApprovedCount, auditDeniedCount, auditTotalCount]);

  const load = useCallback(async () => {
    if (isUnsetWorkspacePath(projectPath)) {
      setPresetChecked({});
      setCustomBlock("");
      return;
    }
    setLoading(true);
    setMessage(null);
    try {
      const deny = await invoke<string[]>("get_omiga_permission_denies", {
        projectRoot: projectPath,
      });
      const { presetChecked: pc, customBlock: cb } = parseDenyIntoState(deny);
      setPresetChecked(pc);
      setCustomBlock(cb);
    } catch (e) {
      setMessage({
        type: "error",
        text: `加载失败: ${e instanceof Error ? e.message : String(e)}`,
      });
    } finally {
      setLoading(false);
    }
  }, [projectPath]);

  const loadAuditEvents = useCallback(async (page: number) => {
    if (isUnsetWorkspacePath(projectPath)) {
      setAuditEvents([]);
      setAuditTotalCount(0);
      setAuditApprovedCount(0);
      setAuditDeniedCount(0);
      return;
    }
    const requestSeq = ++auditRequestSeq.current;
    setAuditLoading(true);
    try {
      const response = await invoke<PermissionAuditEventsResponse>("permission_get_audit_events", {
        limit: AUDIT_PAGE_SIZE,
        offset: (page - 1) * AUDIT_PAGE_SIZE,
        projectRoot: projectPath,
        decision: auditDecisionFilter === "all" ? undefined : auditDecisionFilter,
        toolQuery: auditToolQuery || undefined,
      });
      if (requestSeq !== auditRequestSeq.current) {
        return;
      }
      setAuditEvents(response.events);
      setAuditTotalCount(response.totalCount);
      setAuditApprovedCount(response.approvedCount);
      setAuditDeniedCount(response.deniedCount);
    } catch (e) {
      setMessage({
        type: "error",
        text: `加载权限审计失败: ${e instanceof Error ? e.message : String(e)}`,
      });
    } finally {
      if (requestSeq === auditRequestSeq.current) {
        setAuditLoading(false);
      }
    }
  }, [auditDecisionFilter, auditToolQuery, projectPath]);

  useEffect(() => {
    const timeoutId = window.setTimeout(() => {
      setAuditToolQuery(auditToolFilter.trim());
    }, 250);
    return () => window.clearTimeout(timeoutId);
  }, [auditToolFilter]);

  useEffect(() => {
    void load();
  }, [load]);

  useEffect(() => {
    const queryKey = `${projectPath}::${auditDecisionFilter}::${auditToolQuery}`;
    if (previousAuditQueryKey.current !== queryKey) {
      previousAuditQueryKey.current = queryKey;
      if (auditPage !== 1) {
        setAuditPage(1);
        return;
      }
    }
    void loadAuditEvents(auditPage);
  }, [auditDecisionFilter, auditPage, auditToolQuery, loadAuditEvents, projectPath]);

  const togglePreset = (rule: string) => {
    setPresetChecked((prev) => ({ ...prev, [rule]: !prev[rule] }));
  };

  const handleSave = async () => {
    if (isUnsetWorkspacePath(projectPath)) {
      setMessage({ type: "error", text: "请先在会话中选择工作区文件夹后再保存权限。" });
      return;
    }
    setSaving(true);
    setMessage(null);
    try {
      const deny = buildDenyList(presetChecked, customBlock);
      await invoke("save_omiga_permission_denies", {
        projectRoot: projectPath,
        deny,
      });
      setMessage({
        type: "success",
        text: "已保存到 .omiga/permissions.json，并与 ~/.claude、.claude 中的规则合并生效。",
      });
    } catch (e) {
      setMessage({
        type: "error",
        text: `保存失败: ${e instanceof Error ? e.message : String(e)}`,
      });
    } finally {
      setSaving(false);
    }
  };

  const unset = isUnsetWorkspacePath(projectPath);

  return (
    <Box sx={{ mt: 0.5 }}>
      <Box
        sx={{
          position: "relative",
          overflow: "hidden",
          borderRadius: 3,
          p: 2.5,
          mb: 3,
          background: isDark
            ? `linear-gradient(135deg, ${alpha(accent, 0.18)} 0%, ${alpha(theme.palette.secondary.main, 0.12)} 55%, ${alpha(theme.palette.background.paper, 0.4)} 100%)`
            : `linear-gradient(135deg, ${alpha(accent, 0.1)} 0%, ${alpha(theme.palette.secondary.main, 0.08)} 50%, ${alpha("#fff", 0.95)} 100%)`,
          border: `1px solid ${alpha(accent, isDark ? 0.25 : 0.2)}`,
          boxShadow: `0 8px 32px ${alpha(accent, isDark ? 0.12 : 0.06)}`,
        }}
      >
        <Stack direction="row" alignItems="flex-start" spacing={1.5}>
          <Box
            sx={{
              width: 48,
              height: 48,
              borderRadius: 2,
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              bgcolor: alpha(accent, isDark ? 0.22 : 0.15),
              color: accent,
              flexShrink: 0,
            }}
          >
            <ShieldIcon sx={{ fontSize: 28 }} />
          </Box>
          <Box sx={{ flex: 1, minWidth: 0 }}>
            <Typography
              variant="h6"
              sx={{
                fontWeight: 800,
                letterSpacing: "-0.02em",
                mb: 0.75,
                fontSize: { xs: "1.1rem", sm: "1.2rem" },
              }}
            >
              工具权限（Denylist）
            </Typography>
            <Typography variant="body2" color="text.secondary" sx={{ lineHeight: 1.65 }}>
              勾选下方能力即<strong>禁止</strong> AI 调用对应工具。规则写入当前工作区{" "}
              <Box
                component="span"
                sx={{
                  fontFamily: "ui-monospace, monospace",
                  fontSize: "0.8rem",
                  px: 0.6,
                  py: 0.15,
                  borderRadius: 1,
                  bgcolor: alpha(theme.palette.text.primary, 0.06),
                }}
              >
                .omiga/permissions.json
              </Box>
              ，并与 Claude Code 的{" "}
              <Box
                component="span"
                sx={{
                  fontFamily: "ui-monospace, monospace",
                  fontSize: "0.8rem",
                  px: 0.6,
                  py: 0.15,
                  borderRadius: 1,
                  bgcolor: alpha(theme.palette.text.primary, 0.06),
                }}
              >
                permissions.deny
              </Box>{" "}
              合并生效。
            </Typography>
            {!unset && (
              <Stack direction="row" alignItems="center" spacing={1} sx={{ mt: 1.5 }} flexWrap="wrap" useFlexGap>
                <Chip
                  size="small"
                  label={`预设已禁 ${blockedCount} 项`}
                  sx={{
                    fontWeight: 700,
                    bgcolor: alpha(accent, 0.12),
                    color: accent,
                    border: `1px solid ${alpha(accent, 0.2)}`,
                  }}
                />
              </Stack>
            )}
          </Box>
        </Stack>
      </Box>

      {unset && (
        <Alert severity="warning" sx={{ mb: 2, borderRadius: 2 }}>
          当前会话未选择工作区目录，无法读写权限文件。请在聊天侧选择项目文件夹。
        </Alert>
      )}

      {loading ? (
        <Box sx={{ display: "flex", justifyContent: "center", py: 6 }}>
          <CircularProgress size={36} sx={{ color: accent }} />
        </Box>
      ) : (
        <>
          <Stack
            direction={{ xs: "column", sm: "row" }}
            alignItems={{ xs: "stretch", sm: "center" }}
            justifyContent="space-between"
            spacing={1.5}
            sx={{ mb: 2 }}
          >
            <Stack direction="row" alignItems="center" spacing={1}>
              <HistoryIcon sx={{ color: "text.secondary", fontSize: 22 }} />
              <Box>
                <Typography variant="overline" sx={{ display: "block", letterSpacing: 0, fontWeight: 800, color: "text.secondary", lineHeight: 1.2 }}>
                  权限审计
                </Typography>
                <Typography variant="caption" color="text.secondary">
                  最近的批准和拒绝会持久保存，重启后仍可查看。
                </Typography>
              </Box>
            </Stack>
            <Button
              size="small"
              variant="outlined"
              startIcon={auditLoading ? <CircularProgress size={14} /> : <RefreshIcon />}
              onClick={() => void loadAuditEvents(auditPage)}
              disabled={auditLoading}
              sx={{ alignSelf: { xs: "flex-start", sm: "center" }, textTransform: "none", fontWeight: 700 }}
            >
              刷新
            </Button>
          </Stack>

          <Stack
            direction={{ xs: "column", md: "row" }}
            spacing={1.5}
            alignItems={{ xs: "stretch", md: "center" }}
            sx={{ mb: 1.5 }}
          >
            <Stack direction="row" spacing={1} flexWrap="wrap" useFlexGap>
              <Chip
                label="全部"
                color={auditDecisionFilter === "all" ? "primary" : "default"}
                variant={auditDecisionFilter === "all" ? "filled" : "outlined"}
                onClick={() => setAuditDecisionFilter("all")}
                sx={{ fontWeight: 700 }}
              />
              <Chip
                label="批准"
                color={auditDecisionFilter === "approved" ? "success" : "default"}
                variant={auditDecisionFilter === "approved" ? "filled" : "outlined"}
                onClick={() => setAuditDecisionFilter("approved")}
                sx={{ fontWeight: 700 }}
              />
              <Chip
                label="拒绝"
                color={auditDecisionFilter === "denied" ? "error" : "default"}
                variant={auditDecisionFilter === "denied" ? "filled" : "outlined"}
                onClick={() => setAuditDecisionFilter("denied")}
                sx={{ fontWeight: 700 }}
              />
            </Stack>
            <TextField
              size="small"
              value={auditToolFilter}
              onChange={(event) => setAuditToolFilter(event.target.value)}
              placeholder="按工具名过滤，例如 Bash 或 mcp__server"
              inputProps={{ "aria-label": "按工具名过滤审计记录" }}
              sx={{ minWidth: { xs: "100%", md: 280 }, maxWidth: { md: 360 } }}
              InputProps={{
                startAdornment: (
                  <InputAdornment position="start">
                    <SearchIcon sx={{ fontSize: 18, color: "text.secondary" }} />
                  </InputAdornment>
                ),
              }}
            />
          </Stack>

          <Stack
            direction={{ xs: "column", md: "row" }}
            spacing={1}
            alignItems={{ xs: "flex-start", md: "center" }}
            justifyContent="space-between"
            sx={{ mb: 1.25 }}
          >
            <Typography variant="caption" color="text.secondary">
              {`第 ${auditPage} 页 / 当前筛选：${auditFilterSummary}${
                auditLoading ? " / 加载中…" : ""
              }${!auditHasMore && auditTotalCount > 0 ? " / 无更多记录" : ""}`}
            </Typography>
            <Typography variant="caption" color="text.secondary">
              {`${auditFacetSummaryText} / ${auditPageSummaryText}`}
            </Typography>
          </Stack>

          <Box
            sx={{
              border: `1px solid ${alpha(theme.palette.divider, 0.9)}`,
              borderRadius: 2.5,
              overflow: "hidden",
              mb: 3,
              bgcolor: alpha(theme.palette.background.paper, 0.82),
            }}
          >
            {auditLoading && auditEvents.length === 0 ? (
              <Stack
                alignItems="center"
                spacing={1}
                sx={{ px: 2, py: 4, color: "text.secondary" }}
              >
                <CircularProgress size={24} />
                <Typography variant="body2" color="text.secondary">
                  正在加载最近的权限审计记录…
                </Typography>
              </Stack>
            ) : auditEvents.length === 0 ? (
              <Stack spacing={0.75} sx={{ p: 2 }}>
                <Typography variant="body2" fontWeight={700}>
                  {hasActiveAuditFilters
                    ? "当前筛选下没有匹配的记录。"
                    : auditPage > 1
                      ? "当前页没有更多权限审计记录。"
                      : "暂无权限审计记录。"}
                </Typography>
                <Typography variant="caption" color="text.secondary">
                  {hasActiveAuditFilters
                    ? "试试切回“全部”，或清空工具名过滤关键字。"
                    : auditPage > 1
                      ? "可以返回上一页，或刷新后重试。"
                      : "批准或拒绝工具后，这里会显示最近记录。"}
                </Typography>
                {hasActiveAuditFilters ? (
                  <Box>
                    <Button
                      size="small"
                      onClick={() => {
                        setAuditDecisionFilter("all");
                        setAuditToolFilter("");
                      }}
                      sx={{ mt: 0.5, textTransform: "none", fontWeight: 700 }}
                    >
                      清除筛选
                    </Button>
                  </Box>
                ) : null}
              </Stack>
            ) : (
              <Stack divider={<Divider />}>
                {auditEvents.map((event) => {
                  const approved = event.decision === "approved";
                  return (
                    <Stack
                      key={event.id}
                      direction={{ xs: "column", sm: "row" }}
                      alignItems={{ xs: "flex-start", sm: "center" }}
                      spacing={1.25}
                      sx={{ px: 1.5, py: 1.2 }}
                    >
                      {approved ? (
                        <CheckCircleIcon color="success" sx={{ fontSize: 20 }} />
                      ) : (
                        <CancelIcon color="error" sx={{ fontSize: 20 }} />
                      )}
                      <Box sx={{ flex: 1, minWidth: 0 }}>
                        <Stack direction="row" alignItems="center" spacing={1} flexWrap="wrap" useFlexGap>
                          <Typography variant="body2" fontWeight={800}>
                            {approved ? "已批准" : "已拒绝"}
                          </Typography>
                          <Box component="code" sx={{ fontSize: "0.76rem", px: 0.6, py: 0.15, borderRadius: 0.75, bgcolor: alpha(theme.palette.text.primary, 0.06) }}>
                            {event.toolName}
                          </Box>
                          {event.mode ? (
                            <Chip size="small" label={event.mode} sx={{ height: 22, fontSize: "0.68rem" }} />
                          ) : null}
                        </Stack>
                        {event.reason ? (
                          <Typography variant="caption" color="text.secondary" sx={{ display: "block", mt: 0.25 }}>
                            {event.reason}
                          </Typography>
                        ) : null}
                      </Box>
                      <Typography variant="caption" color="text.secondary" sx={{ whiteSpace: "nowrap" }}>
                        {new Date(event.timestamp).toLocaleString()}
                      </Typography>
                    </Stack>
                  );
                })}
              </Stack>
            )}
            <Divider />
            <Stack
              direction="row"
              alignItems="center"
              justifyContent="space-between"
              spacing={1}
              sx={{ px: 1.5, py: 1.2 }}
            >
              <Button
                size="small"
                variant="text"
                onClick={() => setAuditPage((page) => Math.max(1, page - 1))}
                disabled={auditLoading || auditPage === 1}
                sx={{ textTransform: "none", fontWeight: 700 }}
              >
                上一页
              </Button>
              <Typography variant="caption" color="text.secondary">
                {`第 ${auditPage} 页 · 共 ${auditTotalCount} 条${
                  !auditHasMore && auditTotalCount > 0 ? " · 无更多记录" : ""
                }`}
              </Typography>
              <Button
                size="small"
                variant="text"
                onClick={() => setAuditPage((page) => page + 1)}
                disabled={auditLoading || !auditHasMore}
                sx={{ textTransform: "none", fontWeight: 700 }}
              >
                下一页
              </Button>
            </Stack>
          </Box>

          <Typography
            variant="overline"
            sx={{
              display: "block",
              letterSpacing: "0.12em",
              fontWeight: 800,
              color: "text.secondary",
              mb: 1.5,
            }}
          >
            常用工具
          </Typography>
          <Box
            sx={{
              display: "grid",
              gridTemplateColumns: {
                xs: "1fr",
                sm: "repeat(2, 1fr)",
                md: "repeat(2, 1fr)",
                lg: "repeat(3, 1fr)",
              },
              gap: 1.5,
            }}
          >
            {PERMISSION_PRESETS.map((p: PermissionPreset) => (
              <PermissionPresetCard
                key={p.rule}
                preset={p}
                checked={Boolean(presetChecked[p.rule])}
                disabled={unset}
                onToggle={() => togglePreset(p.rule)}
              />
            ))}
          </Box>

          <Divider sx={{ my: 3, borderColor: alpha(theme.palette.divider, 0.9) }} />

          <Typography
            variant="overline"
            sx={{
              display: "block",
              letterSpacing: "0.12em",
              fontWeight: 800,
              color: "text.secondary",
              mb: 1,
            }}
          >
            自定义规则
          </Typography>
          <Typography variant="body2" color="text.secondary" sx={{ mb: 1.5, lineHeight: 1.6 }}>
            每行一条。例如禁用整个 MCP 服务：<code>mcp__server-name</code>，或通配{" "}
            <code>mcp__server__*</code>
          </Typography>
          <TextField
            fullWidth
            multiline
            minRows={5}
            value={customBlock}
            onChange={(e) => setCustomBlock(e.target.value)}
            disabled={unset}
            placeholder={"mcp__user-Figma\nBash(rm:*)"}
            sx={{
              mb: 2.5,
              "& .MuiOutlinedInput-root": {
                fontFamily: "ui-monospace, SFMono-Regular, Menlo, monospace",
                fontSize: "0.85rem",
                lineHeight: 1.55,
                borderRadius: 2.5,
                bgcolor: alpha(theme.palette.text.primary, isDark ? 0.04 : 0.03),
                transition: "box-shadow 0.2s ease, border-color 0.2s ease",
                "&:hover": {
                  bgcolor: alpha(theme.palette.text.primary, isDark ? 0.06 : 0.04),
                },
                "&.Mui-focused": {
                  boxShadow: `0 0 0 3px ${alpha(accent, 0.2)}`,
                },
              },
            }}
          />

          <Button
            variant="contained"
            size="large"
            disableElevation
            startIcon={
              saving ? (
                <CircularProgress size={18} color="inherit" />
              ) : (
                <SaveIcon sx={{ fontSize: 20 }} />
              )
            }
            onClick={() => void handleSave()}
            disabled={unset || saving}
            sx={{
              textTransform: "none",
              fontWeight: 800,
              letterSpacing: "0.02em",
              px: 3,
              py: 1.25,
              borderRadius: 2.5,
              boxShadow: `0 8px 24px ${alpha(accent, 0.35)}`,
              background: `linear-gradient(135deg, ${accent} 0%, ${theme.palette.primary.dark ?? accent} 100%)`,
              "&:hover": {
                boxShadow: `0 12px 28px ${alpha(accent, 0.45)}`,
                filter: "brightness(1.05)",
              },
              "&:disabled": {
                background: alpha(theme.palette.action.disabled, 0.15),
                boxShadow: "none",
                filter: "none",
              },
            }}
          >
            {saving ? "保存中…" : "保存权限"}
          </Button>
        </>
      )}

      {message && (
        <Alert severity={message.type} sx={{ mt: 2.5, borderRadius: 2 }}>
          {message.text}
        </Alert>
      )}
    </Box>
  );
}
