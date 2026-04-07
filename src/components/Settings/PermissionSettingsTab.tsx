import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  Alert,
  Box,
  Button,
  Checkbox,
  CircularProgress,
  FormControlLabel,
  FormGroup,
  TextField,
  Typography,
} from "@mui/material";
import { Save as SaveIcon } from "@mui/icons-material";
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

export function PermissionSettingsTab({ projectPath }: PermissionSettingsTabProps) {
  const [presetChecked, setPresetChecked] = useState<Record<string, boolean>>({});
  const [customBlock, setCustomBlock] = useState("");
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [message, setMessage] = useState<{
    type: "success" | "error";
    text: string;
  } | null>(null);

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
    <Box sx={{ mt: 1 }}>
      <Typography variant="body2" color="text.secondary" sx={{ mb: 2 }}>
        勾选要禁止 AI 使用的工具。规则写入当前工作区{" "}
        <Typography component="span" variant="body2" fontFamily="monospace">
          .omiga/permissions.json
        </Typography>
        ，与 Claude Code 的{" "}
        <Typography component="span" variant="body2" fontFamily="monospace">
          permissions.deny
        </Typography>{" "}
        合并生效。
      </Typography>

      {unset && (
        <Alert severity="warning" sx={{ mb: 2 }}>
          当前会话未选择工作区目录，无法读写权限文件。请在聊天侧选择项目文件夹。
        </Alert>
      )}

      {loading ? (
        <Box sx={{ display: "flex", justifyContent: "center", py: 4 }}>
          <CircularProgress size={32} />
        </Box>
      ) : (
        <>
          <Typography variant="subtitle2" fontWeight={600} sx={{ mb: 1 }}>
            常用工具
          </Typography>
          <FormGroup
            sx={{
              display: "grid",
              gridTemplateColumns: { xs: "1fr", sm: "1fr 1fr" },
              gap: 0.5,
            }}
          >
            {PERMISSION_PRESETS.map((p: PermissionPreset) => (
              <FormControlLabel
                key={p.rule}
                control={
                  <Checkbox
                    size="small"
                    checked={Boolean(presetChecked[p.rule])}
                    onChange={() => togglePreset(p.rule)}
                    disabled={unset}
                  />
                }
                label={p.label}
              />
            ))}
          </FormGroup>

          <Typography variant="subtitle2" fontWeight={600} sx={{ mt: 2, mb: 1 }}>
            自定义规则（每行一条）
          </Typography>
          <Typography variant="caption" color="text.secondary" sx={{ display: "block", mb: 1 }}>
            例如禁用整个 MCP 服务：<code>mcp__server-name</code>，或{" "}
            <code>mcp__server__*</code>
          </Typography>
          <TextField
            fullWidth
            multiline
            minRows={4}
            value={customBlock}
            onChange={(e) => setCustomBlock(e.target.value)}
            disabled={unset}
            placeholder={'mcp__user-Figma\nBash(rm:*)'}
            sx={{ mb: 2, fontFamily: "monospace" }}
          />

          <Button
            variant="contained"
            startIcon={saving ? <CircularProgress size={16} color="inherit" /> : <SaveIcon />}
            onClick={() => void handleSave()}
            disabled={unset || saving}
          >
            {saving ? "保存中…" : "保存权限"}
          </Button>
        </>
      )}

      {message && (
        <Alert severity={message.type} sx={{ mt: 2 }}>
          {message.text}
        </Alert>
      )}
    </Box>
  );
}
