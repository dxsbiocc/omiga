import {
  Alert,
  Box,
  FormControlLabel,
  FormGroup,
  Switch,
  Typography,
} from "@mui/material";
import { useNotebookViewerStore } from "../../state/notebookViewerStore";

export function NotebookSettingsTab() {
  const virtualizeCells = useNotebookViewerStore((s) => s.virtualizeCells);
  const htmlSandboxAllowScripts = useNotebookViewerStore((s) => s.htmlSandboxAllowScripts);
  const enablePythonShellMagic = useNotebookViewerStore((s) => s.enablePythonShellMagic);
  const enableNotebookShortcuts = useNotebookViewerStore((s) => s.enableNotebookShortcuts);
  const setVirtualizeCells = useNotebookViewerStore((s) => s.setVirtualizeCells);
  const setHtmlSandboxAllowScripts = useNotebookViewerStore((s) => s.setHtmlSandboxAllowScripts);
  const setEnablePythonShellMagic = useNotebookViewerStore((s) => s.setEnablePythonShellMagic);
  const setEnableNotebookShortcuts = useNotebookViewerStore((s) => s.setEnableNotebookShortcuts);

  return (
    <Box>
      <Typography variant="body2" color="text.secondary" sx={{ mb: 2 }}>
        控制内置 .ipynb 查看器的行为。设置保存在本机浏览器存储中。
      </Typography>

      <Alert severity="warning" sx={{ mb: 2, borderRadius: 2 }}>
        若开启「HTML 内允许脚本」，仅打开你信任的 notebook；恶意 HTML 可能执行脚本。
      </Alert>

      <FormGroup>
        <FormControlLabel
          control={
            <Switch
              checked={virtualizeCells}
              onChange={(_, c) => setVirtualizeCells(c)}
              color="primary"
            />
          }
          label="长笔记本虚拟滚动"
        />
        <Typography variant="caption" color="text.secondary" sx={{ display: "block", ml: 4.5, mb: 1.5 }}>
          关闭后所有单元格同时渲染，小文件更直观，超大 notebook 可能变慢。
        </Typography>

        <FormControlLabel
          control={
            <Switch
              checked={htmlSandboxAllowScripts}
              onChange={(_, c) => setHtmlSandboxAllowScripts(c)}
              color="primary"
            />
          }
          label="HTML 输出 iframe 允许脚本"
        />
        <Typography variant="caption" color="text.secondary" sx={{ display: "block", ml: 4.5, mb: 1.5 }}>
          关闭后 HTML 输出更安全，但部分图表/交互可能无法显示。
        </Typography>

        <FormControlLabel
          control={
            <Switch
              checked={enablePythonShellMagic}
              onChange={(_, c) => setEnablePythonShellMagic(c)}
              color="primary"
            />
          }
          label="Python 行首 ! shell 魔法"
        />
        <Typography variant="caption" color="text.secondary" sx={{ display: "block", ml: 4.5, mb: 1.5 }}>
          关闭后 <code>!ls</code> 等行将按普通 Python 解析（可能语法错误）。
        </Typography>

        <FormControlLabel
          control={
            <Switch
              checked={enableNotebookShortcuts}
              onChange={(_, c) => setEnableNotebookShortcuts(c)}
              color="primary"
            />
          }
          label="代码单元快捷键"
        />
        <Typography variant="caption" color="text.secondary" sx={{ display: "block", ml: 4.5, mb: 1.5 }}>
          Shift+Enter 运行并跳到下一格；Ctrl/Cmd+Enter 仅运行。关闭后可用工具栏「运行」。
        </Typography>
      </FormGroup>
    </Box>
  );
}
