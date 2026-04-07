import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import {
  Alert,
  Box,
  Button,
  CircularProgress,
  Divider,
  Stack,
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
      const res = await invoke<ImportMcpResult>(
        "import_merge_project_mcp_json",
        {
          projectRoot: root,
          sourcePath,
        },
      );
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
      const res = await invoke<ImportMcpResult>(
        "import_merge_project_mcp_json",
        {
          projectRoot: root,
          sourcePath: defaults.globalClaudeConfig,
        },
      );
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
        const res = await invoke<ImportSkillsResult>(
          "import_skills_from_directory",
          {
            projectRoot: root,
            sourceSkillsDir: picked,
            target,
          },
        );
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
        const res = await invoke<ImportSkillsResult>(
          "import_claude_default_user_skills",
          {
            projectRoot: root,
            target,
          },
        );
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

  const showMcp = mode === "mcp" || mode === "both";
  const showSkills = mode === "skills" || mode === "both";
  const busyAny = busy != null;

  return (
    <Box sx={{ mt: 2 }}>
      {noWorkspace && showMcp && (
        <Alert severity="warning" sx={{ mb: 2, borderRadius: 2 }}>
          当前会话未绑定工作区路径，无法将 MCP / 技能导入到当前项目。用户级
          ~/.omiga 导入仍可使用。
        </Alert>
      )}

      {defaultsError && (
        <Alert severity="warning" sx={{ mb: 2, borderRadius: 2 }}>
          无法读取默认 Claude 路径：{defaultsError}
        </Alert>
      )}
      {defaults && showMcp && (
        <Typography
          variant="caption"
          color="text.secondary"
          display="block"
          sx={{ mb: 1 }}
        >
          Claude Code 全局 MCP 配置：{defaults.globalClaudeConfig}
          {defaults.globalClaudeConfigExists ? " ✓" : "（文件不存在）"}
        </Typography>
      )}

      <Box
        sx={{
          display: "flex",
          flexWrap: "wrap",
          gap: 3,
          alignItems: "center",
          justifyContent: "center",
        }}
      >
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
                busyAny || noWorkspace || !defaults?.globalClaudeConfigExists
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
          <Typography
            variant="subtitle1"
            fontWeight={600}
            sx={{ mt: 2, mb: 1.5 }}
          >
            导入技能到 Omiga
          </Typography>
          <Stack spacing={2.5}>
            <Box>
              <Typography variant="body2" fontWeight={600} sx={{ mb: 0.5 }}>
                从 Claude 默认目录导入
              </Typography>
              <Typography
                variant="caption"
                color="text.secondary"
                display="block"
                sx={{ mb: 1.25 }}
              >
                来源{" "}
                <Typography
                  component="span"
                  fontFamily="monospace"
                  fontSize="0.85em"
                >
                  {defaults?.defaultUserSkillsDir ?? "~/.claude/skills"}
                </Typography>
              </Typography>
              <Box
                sx={{
                  display: "flex",
                  justifyContent: "center",
                  width: "100%",
                }}
              >
                <Stack
                  direction={{ xs: "column", sm: "row" }}
                  spacing={3}
                  useFlexGap
                  sx={{ width: "100%", maxWidth: "80%" }}
                >
                  <Button
                    fullWidth
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
                    onClick={() =>
                      void runImportClaudeDefaultSkills("userOmiga")
                    }
                  >
                    用户 ~/.omiga/skills
                  </Button>
                  <Button
                    fullWidth
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
                    onClick={() =>
                      void runImportClaudeDefaultSkills("projectOmiga")
                    }
                  >
                    项目 .omiga/skills
                  </Button>
                </Stack>
              </Box>
            </Box>

            <Divider flexItem sx={{ borderStyle: "dashed" }} />

            <Box>
              <Typography variant="body2" fontWeight={600} sx={{ mb: 1 }}>
                从任意文件夹导入
              </Typography>
              <Box
                sx={{
                  display: "flex",
                  justifyContent: "center",
                  width: "100%",
                }}
              >
                <Stack
                  direction={{ xs: "column", sm: "row" }}
                  spacing={3}
                  useFlexGap
                  sx={{ width: "100%", maxWidth: "80%" }}
                >
                  <Button
                    fullWidth
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
                    用户 ~/.omiga/skills
                  </Button>
                  <Button
                    fullWidth
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
                    onClick={() =>
                      void runImportSkillsFromFolder("projectOmiga")
                    }
                  >
                    项目 .omiga/skills
                  </Button>
                </Stack>
              </Box>
            </Box>
          </Stack>
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
