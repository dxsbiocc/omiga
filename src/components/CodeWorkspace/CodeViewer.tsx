import { useRef, useCallback } from "react";
import Editor, { type OnMount, type OnChange } from "@monaco-editor/react";
import type * as Monaco from "monaco-editor";
import { Box, CircularProgress, Typography } from "@mui/material";
import { useTheme } from "@mui/material/styles";

// ─── Language maps ────────────────────────────────────────────────────────────

export function extToLanguage(ext: string): string {
  const map: Record<string, string> = {
    py: "python", pyw: "python",
    rs: "rust",
    js: "javascript", jsx: "javascript",
    ts: "typescript", tsx: "typescript",
    go: "go",
    java: "java",
    c: "c", h: "c",
    cpp: "cpp", cc: "cpp", cxx: "cpp", hpp: "cpp",
    cs: "csharp",
    swift: "swift",
    kt: "kotlin", kts: "kotlin",
    r: "r",
    rb: "ruby",
    php: "php",
    scala: "scala",
    lua: "lua",
    pl: "perl", pm: "perl",
    dart: "dart",
    sh: "shell", bash: "shell", zsh: "shell", fish: "shell",
    ps1: "powershell", psm1: "powershell",
    json: "json",
    yaml: "yaml", yml: "yaml",
    toml: "ini",
    xml: "xml",
    html: "html", htm: "html",
    css: "css", scss: "scss", sass: "scss", less: "less",
    md: "markdown", markdown: "markdown", rmd: "markdown", qmd: "markdown",
    sql: "sql",
    graphql: "graphql", gql: "graphql",
    tf: "hcl", hcl: "hcl",
    dockerfile: "dockerfile",
    makefile: "makefile",
  };
  return map[ext.toLowerCase()] ?? "plaintext";
}

export function extToLabel(ext: string): string {
  const map: Record<string, string> = {
    py: "Python", pyw: "Python",
    rs: "Rust",
    js: "JavaScript", jsx: "JavaScript JSX",
    ts: "TypeScript", tsx: "TypeScript JSX",
    go: "Go",
    java: "Java",
    c: "C", h: "C Header",
    cpp: "C++", cc: "C++", cxx: "C++", hpp: "C++ Header",
    cs: "C#",
    swift: "Swift",
    kt: "Kotlin", kts: "Kotlin Script",
    r: "R",
    rb: "Ruby",
    php: "PHP",
    scala: "Scala",
    lua: "Lua",
    pl: "Perl",
    dart: "Dart",
    sh: "Shell", bash: "Bash", zsh: "Zsh", fish: "Fish",
    ps1: "PowerShell",
    json: "JSON",
    yaml: "YAML", yml: "YAML",
    toml: "TOML",
    xml: "XML",
    html: "HTML", htm: "HTML",
    css: "CSS", scss: "SCSS", sass: "Sass", less: "Less",
    md: "Markdown", markdown: "Markdown", rmd: "R Markdown", qmd: "Quarto",
    sql: "SQL",
    graphql: "GraphQL", gql: "GraphQL",
    csv: "CSV", tsv: "TSV",
    ipynb: "Jupyter Notebook",
    txt: "Plain Text",
    tf: "Terraform HCL", hcl: "HCL",
  };
  return map[ext.toLowerCase()] ?? ext.toUpperCase();
}

// ─── Component ────────────────────────────────────────────────────────────────

interface CodeViewerProps {
  content: string;
  language: string;
  onChange?: (value: string) => void;
}

export function CodeViewer({ content, language, onChange }: CodeViewerProps) {
  const theme = useTheme();
  const editorRef = useRef<Monaco.editor.IStandaloneCodeEditor | null>(null);

  const handleMount: OnMount = useCallback(
    (editor) => {
      editorRef.current = editor;

      // JSON: auto-format once on open (runs in the JSON worker thread)
      if (language === "json") {
        setTimeout(() => {
          editor
            .getAction("editor.action.formatDocument")
            ?.run()
            .catch(() => {/* ignore */});
        }, 100);
      }
    },
    [language],
  );

  const handleChange: OnChange = useCallback(
    (value) => onChange?.(value ?? ""),
    [onChange],
  );

  return (
    <Box sx={{ flex: 1, minHeight: 0, overflow: "hidden" }}>
      <Editor
        height="100%"
        language={language}
        value={content}
        theme={theme.palette.mode === "dark" ? "vs-dark" : "vs"}
        onMount={handleMount}
        onChange={handleChange}
        loading={
          <Box
            sx={{
              display: "flex",
              flexDirection: "column",
              alignItems: "center",
              justifyContent: "center",
              height: "100%",
              gap: 1.5,
            }}
          >
            <CircularProgress size={24} />
            <Typography variant="caption" color="text.secondary">
              加载编辑器…
            </Typography>
          </Box>
        }
        options={{
          fontFamily: '"JetBrains Mono", "Fira Code", ui-monospace, monospace',
          fontSize: 12,
          lineHeight: 19,
          fontLigatures: true,
          minimap: { enabled: false },
          scrollBeyondLastLine: false,
          renderLineHighlight: "line",
          smoothScrolling: true,
          cursorBlinking: "smooth",
          wordWrap: "off",
          padding: { top: 16, bottom: 16 },
          automaticLayout: true,
        }}
      />
    </Box>
  );
}
