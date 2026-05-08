import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Prism as SyntaxHighlighter } from "react-syntax-highlighter";
import {
  oneDark,
  oneLight,
} from "react-syntax-highlighter/dist/esm/styles/prism";
import {
  Alert,
  Box,
  Chip,
  CircularProgress,
  Collapse,
  Dialog,
  DialogContent,
  DialogTitle,
  IconButton,
  ListItemButton,
  ListItemText,
  Typography,
} from "@mui/material";
import { alpha, useTheme, type Theme } from "@mui/material/styles";
import CloseIcon from "@mui/icons-material/Close";
import KeyboardArrowRightIcon from "@mui/icons-material/KeyboardArrowRight";
import { FileIcon, FolderIcon } from "react-material-icon-theme";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { materialIconFileExtension } from "../../utils/materialIconTheme";
import { extractErrorMessage } from "../../utils/errorMessage";
import { useSessionStore } from "../../state/sessionStore";

export type SkillPreviewTarget = {
  name: string;
  skillMdPath: string;
  /** From catalog / frontmatter `tags`; shown as chips when set */
  tags?: string[];
};

type DirectoryListResponse = {
  directory: string;
  entries: Array<{
    name: string;
    path: string;
    is_directory: boolean;
    size?: number | null;
    modified?: string | null;
  }>;
  total: number;
  has_more: boolean;
};

type TreeNode = {
  name: string;
  path: string;
  isDirectory: boolean;
  children?: TreeNode[];
};

/** Parent directory of a file path (handles `/` and `\`). */
function dirnamePath(filePath: string): string {
  const t = filePath.replace(/[/\\]+$/, "");
  const li = Math.max(t.lastIndexOf("/"), t.lastIndexOf("\\"));
  if (li <= 0) return t;
  return t.slice(0, li);
}

function normPathKey(p: string): string {
  return p.replace(/[/\\]+$/, "");
}

/** Uncollapse every directory on the path from skill root to `filePath` (inclusive parent dirs). */
function expandAncestorsToFile(
  collapsedPaths: Set<string>,
  filePath: string,
  skillRoot: string,
): Set<string> {
  const next = new Set(collapsedPaths);
  const rootN = normPathKey(skillRoot);
  let dir = dirnamePath(filePath);
  for (let i = 0; i < 128; i++) {
    const d = normPathKey(dir);
    if (!d) break;
    next.delete(d);
    if (d === rootN) break;
    const parent = dirnamePath(dir);
    if (parent === dir) break;
    dir = parent;
  }
  return next;
}

async function loadSkillFileTree(
  dirPath: string,
  skillRoot: string,
  sessionId?: string,
): Promise<TreeNode[]> {
  const res = await invoke<DirectoryListResponse>("list_directory", {
    path: dirPath,
    offset: null,
    limit: null,
    sessionId,
    workspaceRoot: skillRoot,
  });
  const nodes: TreeNode[] = [];
  for (const e of res.entries) {
    if (e.is_directory) {
      const children = await loadSkillFileTree(e.path, skillRoot, sessionId);
      nodes.push({
        name: e.name,
        path: e.path,
        isDirectory: true,
        children,
      });
    } else {
      nodes.push({
        name: e.name,
        path: e.path,
        isDirectory: false,
      });
    }
  }
  return nodes;
}

function isMarkdownPath(p: string): boolean {
  const lower = p.toLowerCase();
  return (
    lower.endsWith(".md") ||
    lower.endsWith(".mdx") ||
    lower.endsWith(".markdown") ||
    lower.endsWith(".qmd") ||
    lower.endsWith(".rmd")
  );
}

/** Map file path → Prism `language` prop; `null` → plain text preview (no highlighting). */
function pathToPrismLanguage(path: string): string | null {
  const base =
    path.split(/[/\\]/).pop() ?? path;
  const lowerBase = base.toLowerCase();
  if (lowerBase === "dockerfile" || lowerBase.startsWith("dockerfile.")) {
    return "docker";
  }
  if (lowerBase === "makefile" || lowerBase === "gnumakefile") {
    return "makefile";
  }
  const dot = base.lastIndexOf(".");
  if (dot < 0) {
    return null;
  }
  const ext = base.slice(dot + 1).toLowerCase();

  const map: Record<string, string> = {
    sh: "bash",
    bash: "bash",
    zsh: "bash",
    fish: "bash",
    ksh: "bash",
    ps1: "powershell",
    psm1: "powershell",
    bat: "bash",
    cmd: "bash",
    py: "python",
    pyw: "python",
    pyi: "python",
    rb: "ruby",
    php: "php",
    pl: "perl",
    pm: "perl",
    lua: "lua",
    jl: "julia",
    rs: "rust",
    go: "go",
    java: "java",
    kt: "kotlin",
    kts: "kotlin",
    scala: "scala",
    swift: "swift",
    c: "c",
    h: "c",
    cpp: "cpp",
    cc: "cpp",
    cxx: "cpp",
    hpp: "cpp",
    hh: "cpp",
    cs: "csharp",
    fs: "fsharp",
    mjs: "javascript",
    cjs: "javascript",
    js: "javascript",
    jsx: "jsx",
    ts: "typescript",
    tsx: "tsx",
    json: "json",
    jsonc: "json",
    yml: "yaml",
    yaml: "yaml",
    toml: "toml",
    ini: "ini",
    cfg: "ini",
    conf: "ini",
    xml: "markup",
    html: "markup",
    htm: "markup",
    svg: "markup",
    vue: "markup",
    svelte: "markup",
    css: "css",
    scss: "scss",
    sass: "scss",
    less: "less",
    sql: "sql",
    graphql: "graphql",
    gql: "graphql",
    r: "r",
    ex: "elixir",
    exs: "elixir",
    erl: "erlang",
    groovy: "groovy",
    gradle: "groovy",
    clj: "clojure",
    cljs: "clojure",
    edn: "clojure",
  };

  return map[ext] ?? null;
}

function isRasterImagePath(p: string): boolean {
  return /\.(png|jpe?g|gif|webp|bmp|ico)$/i.test(p);
}

/** Readable as UTF-8 text in `read_file` (not binary). */
function isTextLikePath(p: string): boolean {
  if (!p.includes(".")) return true;
  if (isMarkdownPath(p)) return true;
  if (pathToPrismLanguage(p) !== null) return true;
  return /\.(txt|csv|log|plist|gitignore|env|lock|toml|ya?ml)$/i.test(p);
}

function SkillDirTree({
  nodes,
  depth,
  selectedPath,
  onSelectFile,
  collapsedPaths,
  onToggleFolder,
}: {
  nodes: TreeNode[];
  depth: number;
  selectedPath: string | null;
  onSelectFile: (path: string) => void;
  collapsedPaths: Set<string>;
  onToggleFolder: (dirPath: string) => void;
}) {
  const theme = useTheme();
  const iconLight = theme.palette.mode === "dark";

  return (
    <>
      {nodes.map((n) => {
        if (!n.isDirectory) {
          return (
            <ListItemButton
              key={n.path}
              selected={selectedPath === n.path}
              onClick={() => onSelectFile(n.path)}
              sx={(theme) => ({
                py: 0.5,
                pl: 0.5 + depth * 1.5,
                pr: 1,
                borderRadius: 1.25,
                minHeight: 36,
                transition: "background-color 0.15s ease",
                "&:hover": {
                  bgcolor: alpha(theme.palette.text.primary, 0.05),
                },
                "&.Mui-selected": {
                  bgcolor: alpha(theme.palette.text.primary, 0.08),
                },
                "&.Mui-selected:hover": {
                  bgcolor: alpha(theme.palette.text.primary, 0.11),
                },
              })}
            >
              <Box
                component="div"
                sx={{
                  display: "inline-flex",
                  alignItems: "center",
                  mr: 0.75,
                  flexShrink: 0,
                  opacity: 0.92,
                }}
              >
                <FileIcon
                  fileName={n.name}
                  fileExtension={materialIconFileExtension(n.name)}
                  size={18}
                  light={iconLight}
                />
              </Box>
              <ListItemText
                primary={n.name}
                primaryTypographyProps={{
                  variant: "body2",
                  noWrap: true,
                  title: n.name,
                }}
              />
            </ListItemButton>
          );
        }

        const hasChildren = Boolean(n.children && n.children.length > 0);
        const isExpanded = !collapsedPaths.has(n.path);

        return (
          <Box key={n.path}>
            <ListItemButton
              dense
              disabled={!hasChildren}
              aria-expanded={hasChildren ? isExpanded : undefined}
              onClick={() => hasChildren && onToggleFolder(n.path)}
              sx={(theme) => ({
                py: 0.45,
                pl: 0.25 + depth * 1.5,
                pr: 0.75,
                borderRadius: 1.25,
                minHeight: 32,
                opacity: hasChildren ? 1 : 0.88,
                transition: "background-color 0.15s ease",
                "&:hover": {
                  bgcolor: hasChildren
                    ? alpha(theme.palette.text.primary, 0.05)
                    : undefined,
                },
              })}
            >
              <Box
                sx={{
                  width: 22,
                  display: "flex",
                  alignItems: "center",
                  justifyContent: "center",
                  flexShrink: 0,
                }}
              >
                {hasChildren ? (
                  <KeyboardArrowRightIcon
                    sx={{
                      fontSize: 20,
                      transition: "transform 0.18s ease",
                      transform: isExpanded ? "rotate(90deg)" : "rotate(0deg)",
                      opacity: 0.85,
                    }}
                  />
                ) : (
                  <Box sx={{ width: 20 }} />
                )}
              </Box>
              <Box
                component="div"
                sx={{
                  display: "inline-flex",
                  alignItems: "center",
                  mr: 0.5,
                  flexShrink: 0,
                  opacity: 0.95,
                }}
              >
                <FolderIcon
                  folderName={n.name}
                  isOpen={isExpanded}
                  isRoot={depth === 0}
                  size={18}
                  light={iconLight}
                  theme="specific"
                />
              </Box>
              <Typography
                variant="caption"
                color="text.secondary"
                fontWeight={600}
                noWrap
                title={n.name}
              >
                {n.name}
              </Typography>
            </ListItemButton>
            {hasChildren ? (
              <Collapse in={isExpanded} timeout="auto" unmountOnExit>
                <Box sx={{ pl: 0 }}>
                  <SkillDirTree
                    nodes={n.children!}
                    depth={depth + 1}
                    selectedPath={selectedPath}
                    onSelectFile={onSelectFile}
                    collapsedPaths={collapsedPaths}
                    onToggleFolder={onToggleFolder}
                  />
                </Box>
              </Collapse>
            ) : null}
          </Box>
        );
      })}
    </>
  );
}

const markdownSx = (theme: Theme) => ({
  color: "text.primary",
  fontSize: "0.9375rem",
  lineHeight: 1.65,
  "& :first-of-type": { mt: 0 },
  "& h1": {
    fontSize: "1.35rem",
    fontWeight: 700,
    mt: 2,
    mb: 1,
    lineHeight: 1.3,
  },
  "& h2": {
    fontSize: "1.15rem",
    fontWeight: 700,
    mt: 2,
    mb: 1,
    lineHeight: 1.35,
  },
  "& h3": {
    fontSize: "1.05rem",
    fontWeight: 600,
    mt: 1.5,
    mb: 0.75,
  },
  "& h4, & h5, & h6": {
    fontSize: "1rem",
    fontWeight: 600,
    mt: 1.25,
    mb: 0.5,
  },
  "& p": { mb: 1.25 },
  "& ul, & ol": { pl: 2.5, my: 1 },
  "& li": { mb: 0.5 },
  "& a": {
    color: "primary.main",
    textDecoration: "underline",
    textUnderlineOffset: 2,
  },
  "& code": {
    fontFamily:
      "ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace",
    fontSize: "0.85em",
    bgcolor: "action.hover",
    px: 0.5,
    py: 0.15,
    borderRadius: 0.5,
  },
  "& pre": {
    bgcolor: theme.palette.mode === "dark" ? "grey.900" : "grey.100",
    p: 1.5,
    borderRadius: 1,
    overflow: "auto",
    my: 1.5,
    border: 1,
    borderColor: "divider",
  },
  "& pre code": {
    bgcolor: "transparent",
    p: 0,
    fontSize: "0.8rem",
    lineHeight: 1.55,
    display: "block",
    whiteSpace: "pre-wrap",
    wordBreak: "break-word",
  },
  "& blockquote": {
    borderLeft: "2px solid",
    borderColor: "divider",
    pl: 2,
    my: 1.5,
    py: 0.5,
    color: "text.secondary",
    fontStyle: "italic",
    bgcolor: alpha(theme.palette.text.primary, 0.03),
    borderRadius: 1,
  },
  "& table": {
    width: "100%",
    borderCollapse: "collapse",
    my: 2,
    fontSize: "0.875rem",
  },
  "& th, & td": {
    border: 1,
    borderColor: "divider",
    p: 1,
    textAlign: "left",
    verticalAlign: "top",
  },
  "& th": { bgcolor: "action.hover", fontWeight: 600 },
  "& hr": { my: 2, borderColor: "divider" },
  "& img": { maxWidth: "100%", height: "auto", borderRadius: 1 },
});

type Props = {
  open: boolean;
  skill: SkillPreviewTarget | null;
  onClose: () => void;
};

export function SkillPreviewDialog({ open, skill, onClose }: Props) {
  const currentSession = useSessionStore((s) => s.currentSession);
  const skillRoot = useMemo(
    () => (skill ? dirnamePath(skill.skillMdPath) : ""),
    [skill],
  );
  const previewSessionId = useMemo(() => {
    const projectPath = currentSession?.projectPath?.trim();
    if (!currentSession?.id || !projectPath || projectPath === "." || !skillRoot) {
      return undefined;
    }
    const root = normPathKey(skillRoot);
    const project = normPathKey(projectPath);
    return root === project || root.startsWith(`${project}/`)
      ? currentSession.id
      : undefined;
  }, [currentSession?.id, currentSession?.projectPath, skillRoot]);

  const [tree, setTree] = useState<TreeNode[]>([]);
  const [treeLoading, setTreeLoading] = useState(false);
  const [treeError, setTreeError] = useState<string | null>(null);
  /** Paths of folders whose children are hidden (collapsed). Empty = all expanded. */
  const [collapsedPaths, setCollapsedPaths] = useState<Set<string>>(
    () => new Set(),
  );

  const [selectedPath, setSelectedPath] = useState<string | null>(null);

  const [fileLoading, setFileLoading] = useState(false);
  const [fileError, setFileError] = useState<string | null>(null);
  const [textContent, setTextContent] = useState<string | null>(null);
  const [imageDataUrl, setImageDataUrl] = useState<string | null>(null);
  const [scriptLanguage, setScriptLanguage] = useState<string | null>(null);
  const [previewKind, setPreviewKind] = useState<
    "empty" | "markdown" | "script" | "text" | "image" | "unsupported"
  >("empty");

  const themeMui = useTheme();

  useEffect(() => {
    if (!open || !skill) {
      setTree([]);
      setTreeError(null);
      setCollapsedPaths(new Set());
      setSelectedPath(null);
      setTextContent(null);
      setImageDataUrl(null);
      setScriptLanguage(null);
      setPreviewKind("empty");
      setFileError(null);
      return;
    }

    let cancelled = false;
    setTreeLoading(true);
    setTreeError(null);
    setCollapsedPaths(new Set());
    setSelectedPath(skill.skillMdPath);

    void loadSkillFileTree(skillRoot, skillRoot, previewSessionId)
      .then((nodes) => {
        if (!cancelled) setTree(nodes);
      })
      .catch((e) => {
        if (!cancelled) {
          setTreeError(extractErrorMessage(e));
          setTree([]);
        }
      })
      .finally(() => {
        if (!cancelled) setTreeLoading(false);
      });

    return () => {
      cancelled = true;
    };
  }, [open, skill, previewSessionId, skillRoot]);

  const loadFile = useCallback(async (path: string) => {
    setFileLoading(true);
    setFileError(null);
    setTextContent(null);
    setImageDataUrl(null);
    setScriptLanguage(null);

    if (isRasterImagePath(path)) {
      try {
        const res = await invoke<{ data: string; mime_type: string }>(
          "read_image_base64",
          { path, sessionId: previewSessionId, workspaceRoot: skillRoot },
        );
        setImageDataUrl(`data:${res.mime_type};base64,${res.data}`);
        setPreviewKind("image");
      } catch (e) {
        setFileError(extractErrorMessage(e));
        setPreviewKind("empty");
      } finally {
        setFileLoading(false);
      }
      return;
    }

    if (!isTextLikePath(path)) {
      setPreviewKind("unsupported");
      setFileLoading(false);
      return;
    }

    try {
      const res = await invoke<{
        content: string;
        total_lines: number;
        has_more: boolean;
      }>("read_file", {
        path,
        offset: null,
        limit: null,
        sessionId: previewSessionId,
        workspaceRoot: skillRoot,
      });
      const body = res.content;
      setTextContent(body);

      if (isMarkdownPath(path)) {
        setPreviewKind("markdown");
        return;
      }

      const lang = pathToPrismLanguage(path);
      if (lang !== null) {
        setScriptLanguage(lang);
        setPreviewKind("script");
        return;
      }

      setPreviewKind("text");
    } catch (e) {
      setFileError(extractErrorMessage(e));
      setPreviewKind("empty");
      setScriptLanguage(null);
    } finally {
      setFileLoading(false);
    }
  }, [previewSessionId, skillRoot]);

  useEffect(() => {
    if (!open || !selectedPath) {
      return;
    }
    void loadFile(selectedPath);
  }, [open, selectedPath, loadFile]);

  const handleSelectFile = useCallback((path: string) => {
    setSelectedPath(path);
  }, []);

  const handleToggleFolder = useCallback((dirPath: string) => {
    setCollapsedPaths((prev) => {
      const next = new Set(prev);
      if (next.has(dirPath)) {
        next.delete(dirPath);
      } else {
        next.add(dirPath);
      }
      return next;
    });
  }, []);

  useEffect(() => {
    if (!open || !selectedPath || !skillRoot) {
      return;
    }
    setCollapsedPaths((prev) =>
      expandAncestorsToFile(prev, selectedPath, skillRoot),
    );
  }, [open, selectedPath, skillRoot]);

  if (!skill) return null;

  return (
    <Dialog
      open={open}
      onClose={onClose}
      maxWidth="lg"
      fullWidth
      scroll="paper"
      aria-labelledby="skill-preview-title"
      PaperProps={{
        elevation: 0,
        sx: (theme) => ({
          borderRadius: 3,
          overflow: "hidden",
          border: `1px solid ${alpha(
            theme.palette.divider,
            theme.palette.mode === "dark" ? 0.55 : 0.88,
          )}`,
          backgroundImage: "none",
          boxShadow:
            theme.palette.mode === "dark"
              ? "0 24px 56px rgba(0,0,0,0.48)"
              : "0 22px 48px rgba(15, 23, 42, 0.1)",
        }),
      }}
    >
      <DialogTitle
        id="skill-preview-title"
        sx={(theme) => ({
          display: "flex",
          alignItems: "flex-start",
          justifyContent: "space-between",
          gap: 1.5,
          pt: 2.75,
          pb: 2,
          px: 3,
          pr: 2,
          borderBottom: `1px solid ${alpha(theme.palette.divider, 0.88)}`,
        })}
      >
        <Box sx={{ minWidth: 0, pr: 1 }}>
          <Typography
            component="span"
            variant="h6"
            fontWeight={650}
            sx={{ letterSpacing: "-0.025em", lineHeight: 1.25 }}
            noWrap
          >
            {skill.name}
          </Typography>
          <Typography
            variant="caption"
            color="text.secondary"
            display="block"
            sx={{
              mt: 0.75,
              fontFamily:
                "ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace",
              fontSize: "0.7rem",
              lineHeight: 1.45,
              wordBreak: "break-all",
              opacity: 0.92,
            }}
          >
            {skill.skillMdPath}
          </Typography>
          {skill.tags && skill.tags.length > 0 && (
            <Box
              sx={{
                display: "flex",
                flexWrap: "wrap",
                gap: 0.5,
                mt: 1,
              }}
            >
              {skill.tags.map((tag) => (
                <Chip
                  key={tag}
                  size="small"
                  label={tag}
                  variant="outlined"
                  sx={(theme) => ({
                    height: 22,
                    fontSize: "0.68rem",
                    fontWeight: 500,
                    borderColor: alpha(theme.palette.primary.main, 0.35),
                    color: "text.secondary",
                  })}
                />
              ))}
            </Box>
          )}
        </Box>
        <IconButton
          aria-label="关闭"
          onClick={onClose}
          size="small"
          sx={(theme) => ({
            color: "text.secondary",
            mt: -0.25,
            bgcolor: alpha(theme.palette.text.primary, 0.06),
            "&:hover": {
              bgcolor: alpha(theme.palette.text.primary, 0.11),
            },
          })}
        >
          <CloseIcon fontSize="small" />
        </IconButton>
      </DialogTitle>
      <DialogContent
        dividers={false}
        sx={{
          display: "flex",
          p: 0,
          gap: 0,
          minHeight: 380,
          maxHeight: "min(85vh, 760px)",
          overflow: "hidden",
        }}
      >
        <Box
          sx={(theme) => ({
            width: { xs: 212, sm: 256 },
            flexShrink: 0,
            borderRight: `1px solid ${alpha(theme.palette.divider, 0.88)}`,
            overflow: "auto",
            px: 1.75,
            py: 2.25,
            bgcolor: alpha(
              theme.palette.text.primary,
              theme.palette.mode === "dark" ? 0.045 : 0.028,
            ),
          })}
        >
          <Typography
            variant="overline"
            color="text.secondary"
            sx={{
              display: "block",
              mb: 1.35,
              letterSpacing: "0.14em",
              fontWeight: 700,
              fontSize: "0.65rem",
              opacity: 0.9,
            }}
          >
            技能目录
          </Typography>
          {treeLoading && (
            <Box sx={{ py: 2, display: "flex", justifyContent: "center" }}>
              <CircularProgress size={22} />
            </Box>
          )}
          {treeError && !treeLoading && (
            <Alert
              severity="warning"
              variant="outlined"
              sx={(theme) => ({
                py: 0.75,
                fontSize: "0.75rem",
                borderColor: alpha(theme.palette.warning.main, 0.35),
                bgcolor: alpha(theme.palette.warning.main, 0.06),
              })}
            >
              {treeError}
            </Alert>
          )}
          {!treeLoading && !treeError && tree.length === 0 && (
            <Typography variant="caption" color="text.secondary">
              目录为空
            </Typography>
          )}
          {!treeLoading && !treeError && tree.length > 0 && (
            <Box component="nav" aria-label="技能文件">
              <SkillDirTree
                nodes={tree}
                depth={0}
                selectedPath={selectedPath}
                onSelectFile={handleSelectFile}
                collapsedPaths={collapsedPaths}
                onToggleFolder={handleToggleFolder}
              />
            </Box>
          )}
        </Box>

        <Box
          sx={(theme) => ({
            flex: 1,
            minWidth: 0,
            display: "flex",
            flexDirection: "column",
            overflow: "hidden",
            p: 2.5,
            pt: 2.25,
            bgcolor: theme.palette.background.paper,
          })}
        >
          {selectedPath && (
            <Typography
              variant="caption"
              color="text.secondary"
              sx={{
                fontFamily:
                  "ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace",
                fontSize: "0.7rem",
                lineHeight: 1.5,
                wordBreak: "break-all",
                mb: 1.5,
                display: "block",
                flexShrink: 0,
                opacity: 0.88,
              }}
            >
              {selectedPath}
            </Typography>
          )}

          {fileLoading && (
            <Box
              sx={{
                py: 4,
                display: "flex",
                justifyContent: "center",
                flex: 1,
                alignItems: "center",
              }}
            >
              <CircularProgress size={32} />
            </Box>
          )}

          {fileError && !fileLoading && (
            <Alert
              severity="error"
              variant="outlined"
              sx={(theme) => ({
                borderRadius: 2,
                borderColor: alpha(theme.palette.error.main, 0.4),
                bgcolor: alpha(theme.palette.error.main, 0.05),
              })}
            >
              {fileError}
            </Alert>
          )}

          {!fileLoading &&
            !fileError &&
            previewKind === "unsupported" &&
            selectedPath && (
              <Typography color="text.secondary" variant="body2">
                该文件类型暂不支持在此预览，请用外部编辑器打开。
              </Typography>
            )}

          {!fileLoading &&
            !fileError &&
            previewKind === "image" &&
            imageDataUrl && (
              <Box
                component="img"
                src={imageDataUrl}
                alt=""
                sx={{
                  maxWidth: "100%",
                  maxHeight: "min(65vh, 600px)",
                  objectFit: "contain",
                  borderRadius: 1,
                  alignSelf: "flex-start",
                }}
              />
            )}

          {!fileLoading &&
            !fileError &&
            previewKind === "markdown" &&
            textContent !== null && (
              <Box
                sx={(theme) => ({
                  ...markdownSx(theme),
                  overflow: "auto",
                  flex: 1,
                  minHeight: 0,
                  pr: 0.5,
                })}
              >
                <ReactMarkdown remarkPlugins={[remarkGfm]}>
                  {textContent || " "}
                </ReactMarkdown>
              </Box>
            )}

          {!fileLoading &&
            !fileError &&
            previewKind === "script" &&
            textContent !== null &&
            scriptLanguage && (
              <Box
                sx={{
                  flex: 1,
                  minHeight: 0,
                  display: "flex",
                  flexDirection: "column",
                  overflow: "hidden",
                  borderRadius: 2,
                  border: 1,
                  borderColor: "divider",
                }}
              >
                <Box
                  sx={(t) => ({
                    px: 1.5,
                    py: 0.65,
                    borderBottom: `1px solid ${alpha(t.palette.divider, 0.85)}`,
                    bgcolor: alpha(t.palette.text.primary, 0.03),
                  })}
                >
                  <Typography
                    variant="caption"
                    color="text.secondary"
                    component="span"
                    sx={{
                      fontFamily:
                        "ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace",
                      fontSize: "0.7rem",
                      letterSpacing: "0.06em",
                      textTransform: "lowercase",
                    }}
                  >
                    {scriptLanguage}
                  </Typography>
                </Box>
                <Box sx={{ overflow: "auto", flex: 1, minHeight: 0 }}>
                  <SyntaxHighlighter
                    style={
                      themeMui.palette.mode === "dark" ? oneDark : oneLight
                    }
                    language={scriptLanguage}
                    PreTag="div"
                    customStyle={{
                      margin: 0,
                      padding: "1rem 1.125rem",
                      fontSize: "0.8125rem",
                      lineHeight: 1.62,
                    }}
                  >
                    {textContent}
                  </SyntaxHighlighter>
                </Box>
              </Box>
            )}

          {!fileLoading &&
            !fileError &&
            previewKind === "text" &&
            textContent !== null && (
              <Box
                component="pre"
                sx={(theme) => ({
                  m: 0,
                  p: 1.5,
                  borderRadius: 1,
                  border: 1,
                  borderColor: "divider",
                  bgcolor:
                    theme.palette.mode === "dark" ? "grey.900" : "grey.50",
                  color: "text.primary",
                  fontFamily:
                    "ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace",
                  fontSize: "0.8rem",
                  lineHeight: 1.55,
                  whiteSpace: "pre-wrap",
                  wordBreak: "break-word",
                  overflow: "auto",
                  flex: 1,
                  minHeight: 0,
                  maxHeight: "min(65vh, 600px)",
                })}
              >
                {textContent}
              </Box>
            )}
        </Box>
      </DialogContent>
    </Dialog>
  );
}
