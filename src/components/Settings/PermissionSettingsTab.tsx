import {
  useCallback,
  useEffect,
  useMemo,
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
  const [message, setMessage] = useState<{
    type: "success" | "error";
    text: string;
  } | null>(null);

  const blockedCount = useMemo(
    () => Object.values(presetChecked).filter(Boolean).length,
    [presetChecked],
  );

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

  useEffect(() => {
    void load();
  }, [load]);

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
