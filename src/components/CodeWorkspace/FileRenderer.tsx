import { CsvViewer } from "./CsvViewer";
import { CodeViewer, extToLanguage } from "./CodeViewer";
import { ImageViewer } from "./ImageViewer";
import { IpynbViewer } from "./IpynbViewer";
import { PdfViewer } from "./PdfViewer";

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
  const ext = fileName.split(".").pop()?.toLowerCase() ?? "";

  if (IMAGE_EXTS.has(ext)) {
    return <ImageViewer filePath={filePath} />;
  }

  if (ext === "pdf") {
    return <PdfViewer filePath={filePath} />;
  }

  if (ext === "csv" || ext === "tsv") {
    return <CsvViewer content={content} onChange={onChange} />;
  }

  if (ext === "ipynb") {
    return <IpynbViewer filePath={filePath} content={content} onChange={onChange} />;
  }

  const language = CODE_EXTS.has(ext) ? extToLanguage(ext) : "plaintext";
  return <CodeViewer content={content} language={language} onChange={onChange} />;
}
