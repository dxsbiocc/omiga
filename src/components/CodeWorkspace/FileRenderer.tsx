import { useMemo } from "react";
import { Alert, Box, Typography } from "@mui/material";
import { CsvViewer } from "./CsvViewer";
import { CodeViewer, extToLanguage } from "./CodeViewer";
import { ImageViewer } from "./ImageViewer";
import { IpynbViewer } from "./IpynbViewer";
import { PdfViewer } from "./PdfViewer";
import { useExtensionStore } from "../../state/extensionStore";
import {
  findCustomEditorForFile,
  languageForFile,
  type CustomEditorContribution,
} from "../../utils/vscodeExtensions";

const IMAGE_EXTS = new Set([
  "png", "jpg", "jpeg", "gif", "webp", "svg", "bmp", "ico", "tiff", "tif", "avif",
]);

const CODE_EXTS = new Set([
  "py", "pyw", "rs", "js", "jsx", "ts", "tsx", "go", "java",
  "c", "h", "cpp", "cc", "cxx", "hpp", "cs", "swift", "kt", "kts",
  "r", "rb", "php", "scala", "lua", "pl", "pm", "dart",
  "sh", "bash", "zsh", "fish", "ps1", "psm1",
  "json", "yaml", "yml", "toml", "xml",
  "html", "htm", "css", "scss", "sass", "less",
  "md", "markdown", "rmd", "qmd", "sql", "graphql", "gql",
  "tf", "hcl", "vim", "nix", "cmake", "makefile", "dockerfile",
  "txt", "log", "env",
]);

interface FileRendererProps {
  fileName: string;
  /** Full path — needed by ImageViewer to invoke the Tauri backend. */
  filePath: string;
  content: string;
  onChange?: (value: string) => void;
}

export function FileRenderer({ fileName, filePath, content, onChange }: FileRendererProps) {
  const installedExtensions = useExtensionStore((s) => s.installedExtensions);
  const ext = useMemo(() => fileName.split(".").pop()?.toLowerCase() ?? "", [fileName]);
  const customEditor = useMemo(
    () => findCustomEditorForFile(fileName, filePath, installedExtensions),
    [fileName, filePath, installedExtensions],
  );

  // Determine viewer type - stable across renders
  const viewerType = useMemo(() => {
    if (IMAGE_EXTS.has(ext)) return "image";
    if (ext === "pdf") return "pdf";
    if (ext === "csv" || ext === "tsv") return "csv";
    if (ext === "ipynb") return "ipynb";
    return "code";
  }, [ext]);

  // Memoize language to prevent unnecessary re-renders
  const language = useMemo(() =>
    languageForFile(fileName, installedExtensions) ??
    (CODE_EXTS.has(ext) ? extToLanguage(ext) : "plaintext"),
    [ext, fileName, installedExtensions]
  );

  // For empty filePath (file closed), just show empty code viewer
  // This keeps the Monaco instance alive for fast file switching
  if (!filePath) {
    return <CodeViewer content="" language="plaintext" onChange={onChange} />;
  }

  // Use key based on filePath to ensure clean state when switching files
  // but keep editor instance alive for code files
  const viewer = (() => {
    switch (viewerType) {
    case "image":
      return <ImageViewer key={filePath} filePath={filePath} />;
    case "pdf":
      return <PdfViewer key={filePath} filePath={filePath} />;
    case "csv":
      return <CsvViewer key={filePath} content={content} onChange={onChange} />;
    case "ipynb":
      return <IpynbViewer key={filePath} filePath={filePath} content={content} onChange={onChange} />;
    case "code":
    default:
      // Don't use filePath as key for code viewer to keep editor instance alive
      // The CodeViewer uses theme as key to prevent re-mount on dark/light toggle
      return <CodeViewer content={content} language={language} onChange={onChange} />;
    }
  })();

  if (!customEditor) return viewer;

  return (
    <Box sx={{ flex: 1, minHeight: 0, display: "flex", flexDirection: "column" }}>
      <PluginRendererNotice customEditor={customEditor} />
      <Box sx={{ flex: 1, minHeight: 0, display: "flex", flexDirection: "column" }}>
        {viewer}
      </Box>
    </Box>
  );
}

function PluginRendererNotice({
  customEditor,
}: {
  customEditor: CustomEditorContribution;
}) {
  return (
    <Alert
      severity="info"
      variant="outlined"
      sx={{
        flexShrink: 0,
        borderRadius: 0,
        borderLeft: 0,
        borderRight: 0,
        borderTop: 0,
        py: 0.75,
        "& .MuiAlert-message": { width: "100%" },
      }}
    >
      <Typography variant="caption" color="text.secondary">
        已匹配 VS Code 渲染插件{" "}
        <Typography component="span" variant="caption" fontWeight={700}>
          {customEditor.displayName}
        </Typography>
        {" "}({customEditor.viewType})，来自 {customEditor.extensionName}。当前版本先启用
        VSIX 贡献点识别并回退到内置查看器；完整 Webview/custom editor runtime
        将在扩展主机阶段接入。
      </Typography>
    </Alert>
  );
}
