import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import {
  Alert,
  Box,
  Button,
  CircularProgress,
  FormControlLabel,
  Switch,
  Typography,
} from "@mui/material";
import UploadFileIcon from "@mui/icons-material/UploadFile";
import FolderOpenIcon from "@mui/icons-material/FolderOpen";
import ContentCopyIcon from "@mui/icons-material/ContentCopy";
import CloudDownloadIcon from "@mui/icons-material/CloudDownload";

type ClaudeDefaultPaths = {
  claudeConfigHome: string;
  defaultUserSkillsDir: string;
  envClaudeConfigDirSet: boolean;
  globalClaudeConfig: string;
  globalClaudeConfigExists: boolean;
};

type ImportMcpResult = {
  wrotePath: string;
  serverCount: number;
};

type ImportSkillsResult = {
  destSkillsRoot: string;
  importedSkillDirs: string[];
};

type SkillsImportTarget = "userOmiga" | "projectOmiga";

type PanelMode = "mcp" | "skills" | "both";

type BusyKey =
  | "mcp"
  | "mcpGlobal"
  | "skillsFolderUser"
  | "skillsFolderProject"
  | "skillsClaudeUser"
  | "skillsClaudeProject";

/** SQLite `settings` key — must match `skills::SETTING_KEY_LOAD_CLAUDE_USER_SKILLS` (Rust). */
const SETTING_LOAD_CLAUDE_USER_SKILLS = "loadClaudeUserSkills";

function resolveProjectPath(raw: string): string {
  const t = raw.trim();
  return t.length > 0 ? t : ".";
}

export function ClaudeCodeImportPanel({
  projectPath,
  mode,
}: {
  projectPath: string;
  mode: PanelMode;
}) {
  const root = resolveProjectPath(projectPath);
  const noWorkspace = projectPath.trim().length === 0;
  const [defaults, setDefaults] = useState<ClaudeDefaultPaths | null>(null);
  const [defaultsError, setDefaultsError] = useState<string | null>(null);
  const [busy, setBusy] = useState<BusyKey | null>(null);
  const [message, setMessage] = useState<{
    kind: "success" | "error";
    text: string;
  } | null>(null);
  const [loadClaudeUserSkillsAtRuntime, setLoadClaudeUserSkillsAtRuntime] =
    useState(false);
  const [loadClaudeSettingReady, setLoadClaudeSettingReady] = useState(false);

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const d = await invoke<ClaudeDefaultPaths>("get_claude_default_paths");
        if (!cancelled) {
          setDefaults(d);
          setDefaultsError(null);
        }
      } catch (e) {
        if (!cancelled) {
          setDefaults(null);
          setDefaultsError(e instanceof Error ? e.message : String(e));
        }
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const v = await invoke<string | null>("get_setting", {
          key: SETTING_LOAD_CLAUDE_USER_SKILLS,
        });
        if (!cancelled) {
          const t = (v ?? "").trim().toLowerCase();
          setLoadClaudeUserSkillsAtRuntime(
            t === "true" || t === "1" || t === "yes",
          );
          setLoadClaudeSettingReady(true);
        }
      } catch {
        if (!cancelled) {
          setLoadClaudeUserSkillsAtRuntime(false);
          setLoadClaudeSettingReady(true);
        }
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  const runImportMcp = useCallback(async () => {
    setMessage(null);
    const picked = await open({
      multiple: false,
      directory: false,
      filters: [{ name: "JSON", extensions: ["json"] }],
      title: "选择 Claude Code 形式的 MCP 配置（含 mcpServers）",
    });
    if (picked == null || Array.isArray(picked)) return;
    const sourcePath = picked;
    setBusy("mcp");
    try {
      const res = await invoke<ImportMcpResult>("import_merge_project_mcp_json", {
        projectRoot: root,
        sourcePath,
      });
      setMessage({
        kind: "success",
        text: `已合并到 ${res.wrotePath}（共 ${res.serverCount} 个 MCP 服务名，后导入的同名项覆盖已有）。`,
      });
    } catch (e) {
      setMessage({
        kind: "error",
        text: e instanceof Error ? e.message : String(e),
      });
    } finally {
      setBusy(null);
    }
  }, [root]);

  const runImportGlobalClaudeMcp = useCallback(async () => {
    if (!defaults?.globalClaudeConfig) return;
    setMessage(null);
    setBusy("mcpGlobal");
    try {
      const res = await invoke<ImportMcpResult>("import_merge_project_mcp_json", {
        projectRoot: root,
        sourcePath: defaults.globalClaudeConfig,
      });
      setMessage({
        kind: "success",
        text: `已将 ~/.claude.json 中的 MCP 合并到 ${res.wrotePath}（共 ${res.serverCount} 个服务）。`,
      });
    } catch (e) {
      setMessage({
        kind: "error",
        text: e instanceof Error ? e.message : String(e),
      });
    } finally {
      setBusy(null);
    }
  }, [root, defaults?.globalClaudeConfig]);

  const runImportSkillsFromFolder = useCallback(
    async (target: SkillsImportTarget) => {
      setMessage(null);
      const picked = await open({
        multiple: false,
        directory: true,
        title: "选择 skills 根目录（其下为 skill-name/SKILL.md）",
      });
      if (picked == null || Array.isArray(picked)) return;
      const key: BusyKey =
        target === "userOmiga" ? "skillsFolderUser" : "skillsFolderProject";
      setBusy(key);
      try {
        const res = await invoke<ImportSkillsResult>("import_skills_from_directory", {
          projectRoot: root,
          sourceSkillsDir: picked,
          target,
        });
        const n = res.importedSkillDirs.length;
        setMessage({
          kind: "success",
          text:
            n === 0
              ? `未找到含 SKILL.md 的子目录。目标目录：${res.destSkillsRoot}`
              : `已导入 ${n} 个技能到 ${res.destSkillsRoot}：${res.importedSkillDirs.join(", ")}`,
        });
      } catch (e) {
        setMessage({
          kind: "error",
          text: e instanceof Error ? e.message : String(e),
        });
      } finally {
        setBusy(null);
      }
    },
    [root],
  );

  const runImportClaudeDefaultSkills = useCallback(
    async (target: SkillsImportTarget) => {
      setMessage(null);
      const key: BusyKey =
        target === "userOmiga" ? "skillsClaudeUser" : "skillsClaudeProject";
      setBusy(key);
      try {
        const res = await invoke<ImportSkillsResult>("import_claude_default_user_skills", {
          projectRoot: root,
          target,
        });
        const n = res.importedSkillDirs.length;
        setMessage({
          kind: "success",
          text:
            n === 0
              ? `未在 Claude 默认目录找到含 SKILL.md 的子目录。目标：${res.destSkillsRoot}`
              : `已从 Claude 默认目录导入 ${n} 个技能到 ${res.destSkillsRoot}：${res.importedSkillDirs.join(", ")}`,
        });
      } catch (e) {
        setMessage({
          kind: "error",
          text: e instanceof Error ? e.message : String(e),
        });
      } finally {
        setBusy(null);
      }
    },
    [root],
  );

  const onToggleLoadClaudeUserSkills = useCallback(
    async (_: unknown, checked: boolean) => {
      setLoadClaudeUserSkillsAtRuntime(checked);
      try {
        await invoke("set_setting", {
          key: SETTING_LOAD_CLAUDE_USER_SKILLS,
          value: checked ? "true" : "false",
        });
      } catch (e) {
        setLoadClaudeUserSkillsAtRuntime(!checked);
        setMessage({
          kind: "error",
          text: e instanceof Error ? e.message : String(e),
        });
      }
    },
    [],
  );

  const showMcp = mode === "mcp" || mode === "both";
  const showSkills = mode === "skills" || mode === "both";
  const busyAny = busy != null;

  return (
    <Box sx={{ mt: 2 }}>
      {noWorkspace && showMcp && (
        <Alert severity="warning" sx={{ mb: 2, borderRadius: 2 }}>
          当前会话未绑定工作区路径，无法将 MCP / 技能导入到当前项目。用户级 ~/.omiga 导入仍可使用。
        </Alert>
      )}
      <Typography variant="subtitle2" fontWeight={600} sx={{ mb: 1 }}>
        导入 Claude Code 形式
      </Typography>
      <Typography variant="body2" color="text.secondary" sx={{ mb: 2 }}>
        MCP 合并到项目{" "}
        <Typography component="span" fontFamily="monospace" fontSize="0.85em">
          .omiga/mcp.json
        </Typography>
        。技能目录：会话始终加载{" "}
        <Typography component="span" fontFamily="monospace" fontSize="0.85em">
          ~/.omiga/skills
        </Typography>
        与项目{" "}
        <Typography component="span" fontFamily="monospace" fontSize="0.85em">
          .omiga/skills
        </Typography>
        ；<Typography component="span" fontFamily="monospace" fontSize="0.85em">
          ~/.claude/skills
        </Typography>
        仅在下方开关开启时参与会话（默认关闭）。下方「复制到 Omiga」可把技能复制到
        <Typography component="span" fontFamily="monospace" fontSize="0.85em">
          .omiga/skills
        </Typography>
        。
      </Typography>

      {defaultsError && (
        <Alert severity="warning" sx={{ mb: 2, borderRadius: 2 }}>
          无法读取默认 Claude 路径：{defaultsError}
        </Alert>
      )}
      {defaults && showMcp && (
        <Typography variant="caption" color="text.secondary" display="block" sx={{ mb: 1 }}>
          Claude Code 全局 MCP 配置：{defaults.globalClaudeConfig}
          {defaults.globalClaudeConfigExists ? " ✓" : "（文件不存在）"}
        </Typography>
      )}
      {defaults && showSkills && (
        <Typography variant="caption" color="text.secondary" display="block" sx={{ mb: 1 }}>
          Claude Code 用户 skills 目录（开启下方开关后由会话加载；一键导入亦从此目录复制）
          {defaults.envClaudeConfigDirSet ? "（已设置 $CLAUDE_CONFIG_DIR）" : ""}：
          {defaults.defaultUserSkillsDir}
        </Typography>
      )}

      <Box sx={{ display: "flex", flexWrap: "wrap", gap: 1, alignItems: "center" }}>
        {showMcp && (
          <>
            <Button
              variant="outlined"
              size="small"
              startIcon={
                busy === "mcpGlobal" ? (
                  <CircularProgress size={16} />
                ) : (
                  <ContentCopyIcon fontSize="small" />
                )
              }
              disabled={
                busyAny ||
                noWorkspace ||
                !defaults?.globalClaudeConfigExists
              }
              onClick={() => void runImportGlobalClaudeMcp()}
            >
              从 ~/.claude.json 导入 MCP
            </Button>
            <Button
              variant="outlined"
              size="small"
              startIcon={
                busy === "mcp" ? (
                  <CircularProgress size={16} />
                ) : (
                  <UploadFileIcon fontSize="small" />
                )
              }
              disabled={busyAny || noWorkspace}
              onClick={() => void runImportMcp()}
            >
              从 JSON 文件合并 MCP
            </Button>
          </>
        )}
      </Box>

      {showSkills && (
        <>
          <FormControlLabel
            sx={{ mt: 2, mb: 1, alignItems: "flex-start", ml: 0 }}
            control={
              <Switch
                checked={loadClaudeUserSkillsAtRuntime}
                disabled={!loadClaudeSettingReady || busyAny}
                onChange={onToggleLoadClaudeUserSkills}
                color="primary"
              />
            }
            label={
              <Box>
                <Typography variant="body2" component="span" fontWeight={600}>
                  在会话中加载 ~/.claude/skills
                </Typography>
                <Typography variant="caption" color="text.secondary" display="block">
                  关闭时聊天与技能工具不会读取 Claude 用户目录（默认关闭）；仍可使用 ~/.omiga/skills
                  与项目 .omiga/skills。
                </Typography>
              </Box>
            }
          />
          <Typography variant="subtitle2" fontWeight={600} sx={{ mt: 1, mb: 1 }}>
            从 Claude 默认目录导入到 Omiga（一键）
          </Typography>
          <Typography variant="caption" color="text.secondary" display="block" sx={{ mb: 1 }}>
            来源为{" "}
            <Typography component="span" fontFamily="monospace" fontSize="0.85em">
              {defaults?.defaultUserSkillsDir ?? "~/.claude/skills"}
            </Typography>
            （与 Claude Code 用户级 skills 目录一致）。
          </Typography>
          <Box sx={{ display: "flex", flexWrap: "wrap", gap: 1, alignItems: "center", mb: 2 }}>
            <Button
              variant="contained"
              size="small"
              color="primary"
              startIcon={
                busy === "skillsClaudeUser" ? (
                  <CircularProgress size={16} color="inherit" />
                ) : (
                  <CloudDownloadIcon fontSize="small" />
                )
              }
              disabled={busyAny}
              onClick={() => void runImportClaudeDefaultSkills("userOmiga")}
            >
              Claude 默认目录 → 用户 ~/.omiga/skills
            </Button>
            <Button
              variant="contained"
              size="small"
              color="secondary"
              startIcon={
                busy === "skillsClaudeProject" ? (
                  <CircularProgress size={16} color="inherit" />
                ) : (
                  <CloudDownloadIcon fontSize="small" />
                )
              }
              disabled={busyAny || noWorkspace}
              onClick={() => void runImportClaudeDefaultSkills("projectOmiga")}
            >
              Claude 默认目录 → 项目 .omiga/skills
            </Button>
          </Box>

          <Typography variant="subtitle2" fontWeight={600} sx={{ mt: 1, mb: 1 }}>
            从任意文件夹复制到 Omiga
          </Typography>
          <Box sx={{ display: "flex", flexWrap: "wrap", gap: 1, alignItems: "center", mb: 2 }}>
            <Button
              variant="outlined"
              size="small"
              color="primary"
              startIcon={
                busy === "skillsFolderUser" ? (
                  <CircularProgress size={16} />
                ) : (
                  <FolderOpenIcon fontSize="small" />
                )
              }
              disabled={busyAny}
              onClick={() => void runImportSkillsFromFolder("userOmiga")}
            >
              选择文件夹 → 用户 ~/.omiga/skills
            </Button>
            <Button
              variant="outlined"
              size="small"
              color="secondary"
              startIcon={
                busy === "skillsFolderProject" ? (
                  <CircularProgress size={16} />
                ) : (
                  <FolderOpenIcon fontSize="small" />
                )
              }
              disabled={busyAny || noWorkspace}
              onClick={() => void runImportSkillsFromFolder("projectOmiga")}
            >
              选择文件夹 → 项目 .omiga/skills
            </Button>
          </Box>
          <Typography variant="caption" color="text.secondary" display="block" sx={{ mt: 1 }}>
            删除 ~/.omiga/skills 或项目 .omiga/skills 下的副本：请在下方「当前已加载项」→ Skills 卡片上使用「卸载副本」。
          </Typography>
        </>
      )}

      {message && (
        <Alert
          severity={message.kind === "success" ? "success" : "error"}
          sx={{ mt: 2, borderRadius: 2 }}
        >
          {message.text}
        </Alert>
      )}
    </Box>
  );
}
