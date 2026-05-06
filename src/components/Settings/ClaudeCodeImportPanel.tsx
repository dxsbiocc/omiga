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
import CloudDownloadIcon from "@mui/icons-material/CloudDownload";
import { extractErrorMessage } from "../../utils/errorMessage";

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
  const showMcp = mode === "mcp" || mode === "both";
  const showSkills = mode === "skills" || mode === "both";

  useEffect(() => {
    if (!showSkills) return;
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
          setDefaultsError(extractErrorMessage(e));
        }
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [showSkills]);

  const runImportMcp = useCallback(async () => {
    setMessage(null);
    const picked = await open({
      multiple: false,
      directory: false,
      filters: [{ name: "JSON", extensions: ["json"] }],
      title: "选择 MCP JSON 配置（含 mcpServers）",
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
        text: extractErrorMessage(e),
      });
    } finally {
      setBusy(null);
    }
  }, [root]);

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
          text: extractErrorMessage(e),
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
          text: extractErrorMessage(e),
        });
      } finally {
        setBusy(null);
      }
    },
    [root],
  );

  const busyAny = busy != null;
  const claudeSkillsFrom = defaults?.defaultUserSkillsDir ?? "~/.claude/skills";
  const skillImportBtnSx = {
    textTransform: "none" as const,
    whiteSpace: "normal" as const,
    lineHeight: 1.35,
    py: 1.1,
  };

  return (
    <Box sx={{ mt: 2 }}>
      {noWorkspace && showMcp && (
        <Alert severity="warning" sx={{ mb: 2, borderRadius: 2 }}>
          当前会话未绑定工作区路径，无法将 MCP 配置保存到当前项目
          .omiga/mcp.json。
        </Alert>
      )}

      {defaultsError && showSkills && (
        <Alert severity="warning" sx={{ mb: 2, borderRadius: 2 }}>
          无法读取默认 Claude 路径：{defaultsError}
        </Alert>
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
                （只读复制到 Omiga，不启用对 Claude 目录的运行时引用）。
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
                    sx={skillImportBtnSx}
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
                    从 {claudeSkillsFrom} 导入
                  </Button>
                  <Button
                    fullWidth
                    variant="contained"
                    size="small"
                    color="secondary"
                    sx={skillImportBtnSx}
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
                    从 {claudeSkillsFrom} 导入
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
                    sx={skillImportBtnSx}
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
                    从所选文件夹导入到 ~/.omiga/skills
                  </Button>
                  <Button
                    fullWidth
                    variant="outlined"
                    size="small"
                    color="secondary"
                    sx={skillImportBtnSx}
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
                    从所选文件夹导入到 项目 .omiga/skills
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
