/**
 * 当前文件夹扁平列表（非树形）：TanStack Table + MUI Table；
 * 路径导航：MUI Breadcrumbs；类型图标按扩展名区分。
 */
import { useState, useEffect, useMemo, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  Box,
  Typography,
  IconButton,
  CircularProgress,
  Checkbox,
  Tooltip,
  Fade,
  Table,
  TableBody,
  TableCell,
  TableContainer,
  TableHead,
  TableRow,
  Breadcrumbs,
  Link,
} from "@mui/material";
import { useTheme } from "@mui/material/styles";
import {
  Folder,
  Refresh,
  CreateNewFolder,
  PostAdd,
  DeleteOutline,
  DriveFileRenameOutline,
  Settings,
  NavigateNext,
  ArrowBack,
} from "@mui/icons-material";
import { FileIcon, FolderIcon } from "react-material-icon-theme";
import { materialIconFileExtension } from "../../utils/materialIconTheme";
import { extractErrorMessage } from "../../utils/errorMessage";
import {
  createColumnHelper,
  flexRender,
  getCoreRowModel,
  useReactTable,
  type RowSelectionState,
} from "@tanstack/react-table";
import { useWorkspaceStore } from "../../state/workspaceStore";
import { useSessionStore } from "../../state/sessionStore";
import { useChatComposerStore } from "../../state/chatComposerStore";
import { usePencilPalette } from "../../theme";

export interface FileNode {
  name: string;
  path: string;
  isDirectory: boolean;
  size?: number | null;
  modified?: string | null;
}

interface DirectoryListResponse {
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
}

function parseListResult(data: unknown): { directory: string; files: FileNode[] } {
  if (data && typeof data === "object" && "entries" in data) {
    const r = data as DirectoryListResponse;
    const entries = r.entries;
    if (!Array.isArray(entries)) return { directory: r.directory ?? "", files: [] };
    return {
      directory: typeof r.directory === "string" ? r.directory : "",
      files: entries.map((e) => ({
        name: e.name,
        path: e.path,
        isDirectory: e.is_directory,
        size: e.size ?? null,
        modified: e.modified ?? null,
      })),
    };
  }
  return { directory: "", files: [] };
}

/** 一位小数；数值满 1024 则进 KB/MB/GB/TB */
function formatBytes(n?: number | null): string {
  if (n == null || n === undefined) return "—";
  if (!Number.isFinite(n) || n < 0) return "—";
  if (n < 1024) return `${Math.round(n)} B`;

  const units = ["KB", "MB", "GB", "TB", "PB"] as const;
  let v = n / 1024;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i += 1;
  }
  return `${v.toFixed(1)} ${units[i]}`;
}

function formatModified(iso: string | null | undefined): string {
  if (!iso?.trim()) return "—";
  try {
    const d = new Date(iso);
    if (Number.isNaN(d.getTime())) return "—";
    return d.toLocaleString(undefined, {
      month: "short",
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit",
    });
  } catch {
    return "—";
  }
}

/** 规范化路径（POSIX，Tauri 桌面端以 / 为主） */
function normalizePath(p: string): string {
  const t = p.trim().replace(/\/+$/, "");
  return t || "/";
}

function dirnamePath(p: string): string {
  const n = normalizePath(p);
  if (n === "/") return "/";
  const i = n.lastIndexOf("/");
  if (i <= 0) return "/";
  return n.slice(0, i) || "/";
}

function isUnderOrEqual(child: string, root: string): boolean {
  const c = normalizePath(child);
  const r = normalizePath(root);
  return c === r || c.startsWith(r + "/");
}

/** 在会话根内返回上一级路径，否则 null */
function parentWithinRoot(current: string, root: string): string | null {
  const c = normalizePath(current);
  const r = normalizePath(root);
  if (c === r) return null;
  const p = dirnamePath(c);
  if (!isUnderOrEqual(p, r)) return r;
  return p;
}

/** VS Code Material Icon Theme 风格（react-material-icon-theme）。 */
function FileTypeIcon({ node }: { node: FileNode }) {
  const theme = useTheme();
  const pen = usePencilPalette();
  const light = theme.palette.mode === "dark";
  const size = 18;

  return (
    <Box
      component="div"
      sx={{
        display: "inline-flex",
        alignItems: "center",
        justifyContent: "center",
        width: 24,
        height: 24,
        borderRadius: "6px",
        bgcolor: pen.iconChipBg,
        flexShrink: 0,
        verticalAlign: "middle",
      }}
    >
      {node.isDirectory ? (
        <FolderIcon
          folderName={node.name}
          isOpen={false}
          isRoot={false}
          size={size}
          light={light}
          theme="specific"
        />
      ) : (
        <FileIcon
          fileName={node.name}
          fileExtension={materialIconFileExtension(node.name)}
          size={size}
          light={light}
        />
      )}
    </Box>
  );
}

/** 当前 session 工作区文件夹显示名（面包屑首段）；`.` 或空为「工作区」 */
function sessionWorkspaceBasename(projectPath: string): string {
  const t = projectPath.trim();
  if (!t || t === ".") return "工作区";
  const parts = t.split(/[/\\]/u).filter(Boolean);
  return parts[parts.length - 1] ?? t;
}

/** 会话根 → 当前目录 的面包屑（首段为 session 工作区文件夹；否则退化为绝对路径分段） */
function workspaceBreadcrumbs(
  sessionRoot: string,
  currentDir: string,
  opts?: { sessionRootLabel?: string },
): { label: string; path: string }[] {
  const root = normalizePath(sessionRoot);
  const cur = normalizePath(currentDir);
  const rootLabel =
    opts?.sessionRootLabel?.trim() ||
    root.split("/").filter(Boolean).pop() ||
    root;

  if (cur === root) return [{ label: rootLabel, path: root }];

  if (cur.startsWith(root + "/")) {
    const suffix = cur.slice(root.length + 1);
    const parts = suffix.split("/").filter(Boolean);
    const crumbs: { label: string; path: string }[] = [
      { label: rootLabel, path: root },
    ];
    let acc = root;
    for (const part of parts) {
      acc = `${acc}/${part}`;
      crumbs.push({ label: part, path: acc });
    }
    return crumbs;
  }

  const segments = cur.split("/").filter(Boolean);
  const crumbs: { label: string; path: string }[] = [];
  let acc = cur.startsWith("/") ? "" : "";
  for (const seg of segments) {
    acc = acc ? `${acc}/${seg}` : `/${seg}`;
    crumbs.push({ label: seg, path: acc });
  }
  return crumbs.length ? crumbs : [{ label: rootLabel, path: root }];
}

const columnHelper = createColumnHelper<FileNode>();

export function FileTree() {
  const pen = usePencilPalette();
  const headerLabelSx = useMemo(
    () => ({
      fontSize: 10,
      fontWeight: 600,
      letterSpacing: "0.06em",
      textTransform: "uppercase" as const,
      color: pen.textHeader,
    }),
    [pen.textHeader],
  );
  const openFile = useWorkspaceStore((s) => s.openFile);
  const currentSession = useSessionStore((s) => s.currentSession);
  const sessionId = currentSession?.id;
  const projectPath = (currentSession?.projectPath ?? ".").trim() || ".";
  const environment = useChatComposerStore((s) => s.environment);
  const sshServer = useChatComposerStore((s) => s.sshServer);
  const sandboxBackend = useChatComposerStore((s) => s.sandboxBackend);

  const [files, setFiles] = useState<FileNode[]>([]);
  const [sessionRoot, setSessionRoot] = useState<string | null>(null);
  const [currentDir, setCurrentDir] = useState<string | null>(null);
  const [isLoading, setIsLoading] = useState(false);
  const [loadingMessage, setLoadingMessage] = useState<string>("加载文件夹…");
  const [error, setError] = useState<string | null>(null);
  const [rowSelection, setRowSelection] = useState<RowSelectionState>({});

  const loadDirectory = useCallback(
    async (path: string) => {
      if (!sessionId) {
        setFiles([]);
        setSessionRoot(null);
        setCurrentDir(null);
        setRowSelection({});
        return;
      }

      setIsLoading(true);
      setError(null);

      try {
        const useSsh = environment === "ssh" && Boolean(sshServer?.trim());
        const useSandbox = environment === "sandbox" && Boolean(sandboxBackend?.trim());

        if (useSsh) {
          setLoadingMessage(`正在连接到 ${sshServer!.trim()}…`);
        } else if (useSandbox) {
          setLoadingMessage(`正在连接到沙箱 (${sandboxBackend})…`);
        } else {
          setLoadingMessage("加载文件夹…");
        }

        if (environment === "ssh" && !sshServer?.trim()) {
          setFiles([]);
          setCurrentDir(null);
          setSessionRoot(null);
          setError("请先在聊天输入区选择 SSH 服务器");
          return;
        }

        // Remote envs require absolute paths; fall back to sensible defaults.
        let remotePath = path;
        if (useSsh && !path.startsWith("/") && !path.startsWith("~/")) {
          remotePath = "~";
        } else if (useSandbox && !path.startsWith("/")) {
          remotePath = "/workspace";
        }

        let result: unknown;
        if (useSsh) {
          result = await invoke("ssh_list_directory", { sshProfileName: sshServer!.trim(), path: remotePath });
        } else if (useSandbox) {
          result = await invoke("sandbox_list_directory", { sessionId, sandboxBackend: sandboxBackend!.trim(), path: remotePath });
        } else {
          result = await invoke("list_directory", { path });
        }
        const parsed = parseListResult(result);
        setFiles(parsed.files);
        setCurrentDir(parsed.directory);
        setSessionRoot((prev) => prev ?? parsed.directory);
        setRowSelection({});
      } catch (err) {
        setError(`无法加载目录: ${extractErrorMessage(err)}`);
      } finally {
        setIsLoading(false);
      }
    },
    [sessionId, environment, sshServer, sandboxBackend],
  );

  useEffect(() => {
    if (!sessionId) {
      setFiles([]);
      setSessionRoot(null);
      setCurrentDir(null);
      setError(null);
      setRowSelection({});
      return;
    }
    setSessionRoot(null);
    setCurrentDir(null);
    void loadDirectory(projectPath);
  }, [sessionId, projectPath, loadDirectory]);

  const [pendingOpen, setPendingOpen] = useState<string | null>(null);
  
  const handleOpenFile = useCallback(async (path: string) => {
    if (pendingOpen) return; // Prevent double-clicks
    setPendingOpen(path);
    try {
      await openFile(path);
    } finally {
      setPendingOpen(null);
    }
  }, [openFile, pendingOpen]);

  const fileList = Array.isArray(files) ? files : [];

  const sessionFolderLabel = useMemo(
    () => sessionWorkspaceBasename(projectPath),
    [projectPath],
  );

  /** 有 currentDir 即可显示面包屑；锚定 session 工作区根，首段为当前会话文件夹名 */
  const crumbs = useMemo(() => {
    if (!currentDir?.trim()) return [];
    const root = sessionRoot?.trim() ? sessionRoot : currentDir;
    return workspaceBreadcrumbs(root, currentDir, {
      sessionRootLabel: sessionFolderLabel,
    });
  }, [sessionRoot, currentDir, sessionFolderLabel]);

  const canGoUp =
    sessionRoot &&
    currentDir &&
    normalizePath(currentDir) !== normalizePath(sessionRoot);

  const parentPath =
    sessionRoot && currentDir ? parentWithinRoot(currentDir, sessionRoot) : null;

  /** 首行「..」：与会话根内「上级目录」一致，点击即 loadDirectory(parentPath) */
  const parentDotDot = useMemo((): FileNode | null => {
    if (parentPath == null) return null;
    return {
      name: "..",
      path: parentPath,
      isDirectory: true,
      size: null,
      modified: null,
    };
  }, [parentPath]);

  const tableData = useMemo(
    () => (parentDotDot ? [parentDotDot, ...fileList] : fileList),
    [parentDotDot, fileList],
  );

  const columns = useMemo(
    () => [
      columnHelper.display({
        id: "select",
        header: ({ table }) => (
          <Box sx={{ display: "flex", justifyContent: "center" }}>
            <Checkbox
              size="small"
              disableRipple
              sx={{ p: 0 }}
              indeterminate={
                table.getIsSomeRowsSelected() && !table.getIsAllRowsSelected()
              }
              checked={table.getIsAllRowsSelected()}
              disabled={table.getRowModel().rows.length === 0}
              onChange={table.getToggleAllRowsSelectedHandler()}
            />
          </Box>
        ),
        cell: ({ row }) =>
          row.getCanSelect() ? (
            <Box
              sx={{ display: "flex", justifyContent: "center" }}
              onClick={(e) => e.stopPropagation()}
            >
              <Checkbox
                size="small"
                disableRipple
                sx={{ p: 0 }}
                checked={row.getIsSelected()}
                onChange={row.getToggleSelectedHandler()}
              />
            </Box>
          ) : (
            <Box sx={{ display: "flex", justifyContent: "center", minHeight: 32 }} />
          ),
      }),
      columnHelper.accessor("name", {
        header: () => (
          <Typography component="span" sx={headerLabelSx}>
            Name
          </Typography>
        ),
        cell: ({ row }) => {
          const node = row.original;
          return (
            <Box
              sx={{
                display: "flex",
                alignItems: "center",
                gap: 0.5,
                minWidth: 0,
                maxWidth: "100%",
                overflow: "hidden",
                pr: 1,
              }}
            >
              <FileTypeIcon node={node} />
              <Typography
                noWrap
                title={node.name}
                sx={{
                  minWidth: 0,
                  flex: 1,
                  fontSize: 13,
                  fontWeight: node.isDirectory ? 500 : 400,
                  letterSpacing: "-0.01em",
                  color: node.isDirectory ? pen.textTitle : pen.textFilename,
                }}
              >
                {node.name}
              </Typography>
            </Box>
          );
        },
      }),
      columnHelper.display({
        id: "size",
        header: () => (
          <Typography
            component="span"
            sx={{
              ...headerLabelSx,
              display: "block",
              textAlign: "right",
              pr: 0.25,
            }}
          >
            Size
          </Typography>
        ),
        cell: ({ row }) => {
          const node = row.original;
          return (
            <Typography
              component="div"
              sx={{
                fontSize: 12,
                fontVariantNumeric: "tabular-nums",
                color: pen.textSize,
                textAlign: "right",
                width: "100%",
                minWidth: 0,
                overflow: "hidden",
                textOverflow: "ellipsis",
                whiteSpace: "nowrap",
                pr: 0.25,
                pl: 0.25,
                boxSizing: "border-box",
              }}
            >
              {node.isDirectory ? "—" : formatBytes(node.size)}
            </Typography>
          );
        },
      }),
      columnHelper.display({
        id: "modified",
        header: () => (
          <Typography
            component="span"
            sx={{
              ...headerLabelSx,
              display: "block",
              textAlign: "right",
              pl: 0.25,
            }}
          >
            Modified
          </Typography>
        ),
        cell: ({ row }) => {
          const node = row.original;
          return (
            <Typography
              sx={{
                fontSize: 11,
                fontVariantNumeric: "tabular-nums",
                color: pen.textModified,
                textAlign: "right",
                pl: 0.25,
                minWidth: 0,
                overflow: "hidden",
                textOverflow: "ellipsis",
                whiteSpace: "nowrap",
              }}
              title={node.modified ?? undefined}
            >
              {formatModified(node.modified)}
            </Typography>
          );
        },
      }),
    ],
    [headerLabelSx, pen],
  );

  const table = useReactTable({
    data: tableData,
    columns,
    state: { rowSelection },
    onRowSelectionChange: setRowSelection,
    getCoreRowModel: getCoreRowModel(),
    getRowId: (row) => row.path,
    enableRowSelection: (row) => row.original.name !== "..",
    enableMultiRowSelection: true,
  });

  if (!sessionId) {
    return (
      <Box
        sx={{
          height: "100%",
          display: "flex",
          flexDirection: "column",
          alignItems: "center",
          justifyContent: "center",
          p: 3,
          textAlign: "center",
          bgcolor: "background.paper",
        }}
      >
        <Folder sx={{ fontSize: 40, color: pen.emptyStateIcon, mb: 1.5, opacity: 0.6 }} />
        <Typography sx={{ fontSize: 13, color: pen.textTitle, fontWeight: 600 }}>
          未选择会话
        </Typography>
        <Typography sx={{ fontSize: 12, color: pen.textHeader, mt: 0.5, maxWidth: 260 }}>
          在左侧选择或创建一个会话后，将显示该会话工作区下的文件列表。
        </Typography>
      </Box>
    );
  }

  if (isLoading && fileList.length === 0 && !currentDir) {
    return (
      <Box
        sx={{
          display: "flex",
          flexDirection: "column",
          alignItems: "center",
          justifyContent: "center",
          height: "100%",
          gap: 2,
          bgcolor: "background.paper",
        }}
      >
        <CircularProgress size={28} thickness={4} sx={{ color: pen.loadingSpinner }} />
        <Typography variant="body2" sx={{ color: pen.textLoading, fontSize: 13 }}>
          {loadingMessage}
        </Typography>
      </Box>
    );
  }

  if (error && !currentDir) {
    return (
      <Box
        sx={{
          display: "flex",
          flexDirection: "column",
          alignItems: "center",
          justifyContent: "center",
          height: "100%",
          p: 3,
          textAlign: "center",
          bgcolor: "background.paper",
        }}
      >
        <Typography variant="body2" color="error" sx={{ mb: 2, maxWidth: 280, fontSize: 13 }}>
          {error}
        </Typography>
        <IconButton
          onClick={() => void loadDirectory(projectPath)}
          size="small"
          sx={{
            bgcolor: pen.errorRetryBg,
            "&:hover": { bgcolor: pen.errorRetryHoverBg },
          }}
        >
          <Refresh sx={{ fontSize: 18 }} />
        </IconButton>
      </Box>
    );
  }

  const toolbarBtn = {
    size: "small" as const,
    disabled: true,
    sx: {
      p: 0.75,
      color: pen.toolbarIconMuted,
      "&.Mui-disabled": { opacity: 0.5 },
    },
  };

  const refresh = () => void loadDirectory(currentDir ?? projectPath);

  return (
    <Fade in>
      <Box
        sx={{
          height: "100%",
          display: "flex",
          flexDirection: "column",
          bgcolor: "background.paper",
          overflow: "hidden",
        }}
      >
        <Box sx={{ px: 2, pt: 1.5, pb: 1 }}>
          <Box
            sx={{
              display: "flex",
              alignItems: "center",
              gap: 0.5,
              minWidth: 0,
            }}
          >
            {canGoUp && parentPath != null && (
              <Tooltip title="上级目录">
                <IconButton
                  size="small"
                  onClick={() => void loadDirectory(parentPath)}
                  sx={{
                    p: 0.5,
                    color: pen.toolbarIconAccent,
                    flexShrink: 0,
                  }}
                >
                  <ArrowBack sx={{ fontSize: 18 }} />
                </IconButton>
              </Tooltip>
            )}
            <Breadcrumbs
              maxItems={6}
              separator={<NavigateNext sx={{ fontSize: 14, color: pen.textHeader }} />}
              sx={{
                flex: 1,
                minWidth: 0,
                "& .MuiBreadcrumbs-ol": {
                  flexWrap: "nowrap",
                },
              }}
            >
              {crumbs.map((c, i) => {
                const last = i === crumbs.length - 1;
                const isWorkspaceRoot =
                  i === 0 &&
                  ((!sessionRoot && crumbs.length === 1) ||
                    (sessionRoot != null &&
                      normalizePath(c.path) === normalizePath(sessionRoot)));
                const crumbTitle = isWorkspaceRoot
                  ? `${sessionFolderLabel} · ${c.path}`
                  : c.path;
                return last ? (
                  <Typography
                    key={c.path}
                    component="span"
                    color="text.primary"
                    title={crumbTitle}
                    sx={{
                      display: "inline-flex",
                      alignItems: "center",
                      gap: 0.35,
                      minWidth: 0,
                      fontSize: 11,
                      fontWeight: 600,
                      fontFamily:
                        "ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace",
                      maxWidth: 200,
                      overflow: "hidden",
                      textOverflow: "ellipsis",
                      whiteSpace: "nowrap",
                    }}
                  >
                    {isWorkspaceRoot && (
                      <Folder
                        sx={{
                          fontSize: 13,
                          flexShrink: 0,
                          color: pen.fileIconFolder,
                          opacity: 0.9,
                        }}
                      />
                    )}
                    <Box component="span" sx={{ minWidth: 0, overflow: "hidden", textOverflow: "ellipsis" }}>
                      {c.label}
                    </Box>
                  </Typography>
                ) : (
                  <Link
                    key={c.path}
                    component="button"
                    type="button"
                    underline="hover"
                    color="inherit"
                    onClick={() => void loadDirectory(c.path)}
                    title={crumbTitle}
                    sx={{
                      display: "inline-flex",
                      alignItems: "center",
                      gap: 0.35,
                      minWidth: 0,
                      fontSize: 11,
                      cursor: "pointer",
                      fontFamily:
                        "ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace",
                      maxWidth: 160,
                      overflow: "hidden",
                      textOverflow: "ellipsis",
                      whiteSpace: "nowrap",
                      border: "none",
                      background: "none",
                      p: 0,
                      textAlign: "left",
                      color: pen.textPath,
                      "&:hover": { color: pen.textTitle },
                    }}
                  >
                    {isWorkspaceRoot && (
                      <Folder
                        sx={{
                          fontSize: 13,
                          flexShrink: 0,
                          color: pen.fileIconFolder,
                          opacity: 0.9,
                        }}
                      />
                    )}
                    <Box component="span" sx={{ minWidth: 0, overflow: "hidden", textOverflow: "ellipsis" }}>
                      {c.label}
                    </Box>
                  </Link>
                );
              })}
            </Breadcrumbs>
          </Box>
        </Box>

        <Box sx={{ px: 2, pb: 1.5 }}>
          <Box
            sx={{
              display: "flex",
              alignItems: "center",
              gap: 0.25,
              px: 0.5,
              py: 0.35,
              borderRadius: 2,
              bgcolor: pen.toolbarSurface,
              border: `1px solid ${pen.toolbarBorder}`,
            }}
          >
            <Tooltip title="New folder (soon)">
              <span>
                <IconButton {...toolbarBtn}>
                  <CreateNewFolder sx={{ fontSize: 18 }} />
                </IconButton>
              </span>
            </Tooltip>
            <Tooltip title="New file (soon)">
              <span>
                <IconButton {...toolbarBtn}>
                  <PostAdd sx={{ fontSize: 18 }} />
                </IconButton>
              </span>
            </Tooltip>
            <Tooltip title="Delete (soon)">
              <span>
                <IconButton {...toolbarBtn}>
                  <DeleteOutline sx={{ fontSize: 18 }} />
                </IconButton>
              </span>
            </Tooltip>
            <Tooltip title="Rename (soon)">
              <span>
                <IconButton {...toolbarBtn}>
                  <DriveFileRenameOutline sx={{ fontSize: 18 }} />
                </IconButton>
              </span>
            </Tooltip>
            <Tooltip title="Options (soon)">
              <span>
                <IconButton {...toolbarBtn}>
                  <Settings sx={{ fontSize: 18 }} />
                </IconButton>
              </span>
            </Tooltip>
            <Box sx={{ flex: 1, minWidth: 8 }} />
            <Tooltip title="刷新当前文件夹">
              <IconButton
                size="small"
                onClick={refresh}
                disabled={isLoading}
                sx={{
                  p: 0.75,
                  color: pen.toolbarIconAccent,
                  "&:hover": {
                    bgcolor: pen.toolbarIconHoverBg,
                    color: pen.textTitle,
                  },
                }}
              >
                <Refresh sx={{ fontSize: 18 }} />
              </IconButton>
            </Tooltip>
          </Box>
        </Box>

        <Box
          sx={{
            borderTop: `1px solid ${pen.borderSubtle}`,
            flex: 1,
            minHeight: 0,
            display: "flex",
            flexDirection: "column",
            px: 1,
            pb: 1,
          }}
        >
          {error && currentDir && (
            <Typography variant="caption" color="error" sx={{ px: 1, py: 0.5 }}>
              {error}
            </Typography>
          )}
          {fileList.length === 0 && !isLoading && parentPath == null ? (
            <Box
              sx={{
                display: "flex",
                flexDirection: "column",
                alignItems: "center",
                justifyContent: "center",
                py: 6,
                px: 2,
                flex: 1,
              }}
            >
              <Box
                sx={{
                  width: 48,
                  height: 48,
                  borderRadius: 2,
                  bgcolor: pen.emptyStateIconBg,
                  display: "flex",
                  alignItems: "center",
                  justifyContent: "center",
                  mb: 1.5,
                }}
              >
                <Folder sx={{ fontSize: 24, color: pen.emptyStateIcon }} />
              </Box>
              <Typography sx={{ fontSize: 13, color: pen.textPath, fontWeight: 500 }}>
                此文件夹为空
              </Typography>
            </Box>
          ) : (
            <TableContainer
              sx={{
                flex: 1,
                overflow: "auto",
                borderRadius: 1,
                opacity: isLoading ? 0.65 : 1,
                pointerEvents: isLoading ? "none" : "auto",
              }}
            >
              <Table
                stickyHeader
                size="small"
                sx={{
                  tableLayout: "fixed",
                  width: "100%",
                  borderCollapse: "separate",
                  borderSpacing: 0,
                }}
              >
                <colgroup>
                  <col style={{ width: 34 }} />
                  <col style={{ minWidth: 160 }} />
                  <col style={{ width: 76 }} />
                  <col style={{ width: 96 }} />
                </colgroup>
                <TableHead>
                  {table.getHeaderGroups().map((headerGroup) => (
                    <TableRow key={headerGroup.id}>
                      {headerGroup.headers.map((header) => (
                        <TableCell
                          key={header.id}
                          padding="none"
                          sx={{
                            bgcolor: "background.paper",
                            borderBottom: `1px solid ${pen.borderSubtle}`,
                            py: 0.75,
                            px: 1,
                            verticalAlign: "middle",
                            position: "sticky",
                            top: 0,
                            zIndex: 2,
                            backgroundClip: "padding-box",
                            ...(header.column.id === "select" && {
                              width: 34,
                              maxWidth: 34,
                              textAlign: "center",
                              pl: 0.75,
                              pr: 0.25,
                            }),
                            ...(header.column.id === "name" && {
                              pl: 0.25,
                            }),
                            ...(header.column.id === "size" && {
                              width: 76,
                              maxWidth: 76,
                              minWidth: 76,
                              px: 0.5,
                            }),
                          }}
                        >
                          {header.isPlaceholder
                            ? null
                            : flexRender(header.column.columnDef.header, header.getContext())}
                        </TableCell>
                      ))}
                    </TableRow>
                  ))}
                </TableHead>
                <TableBody>
                  {table.getRowModel().rows.map((row) => {
                    const node = row.original;
                    const isSel = row.getIsSelected();
                    return (
                      <TableRow
                        key={row.id}
                        hover
                        selected={isSel}
                        onClick={() => {
                          if (node.isDirectory) void loadDirectory(node.path);
                          else if (!pendingOpen) void handleOpenFile(node.path);
                        }}
                        sx={{
                          cursor: pendingOpen ? "wait" : "pointer",
                          transition: "background-color 0.12s ease",
                          opacity: pendingOpen === node.path ? 0.6 : 1,
                          "&.Mui-selected": {
                            bgcolor: `${pen.rowSelected} !important`,
                          },
                          "&:hover": {
                            bgcolor: isSel
                              ? pen.rowSelected
                              : node.isDirectory
                                ? pen.rowHoverDir
                                : pen.rowHover,
                          },
                        }}
                      >
                        {row.getVisibleCells().map((cell) => (
                          <TableCell
                            key={cell.id}
                            padding="none"
                            sx={{
                              borderBottom: `1px solid ${pen.borderSubtle}`,
                              py: 0.75,
                              px: 1,
                              verticalAlign: "middle",
                              ...(cell.column.id === "select" && {
                                width: 34,
                                maxWidth: 34,
                                pl: 0.75,
                                pr: 0.25,
                              }),
                              ...(cell.column.id === "name" && {
                                pl: 0.25,
                              }),
                              ...(cell.column.id === "size" && {
                                width: 76,
                                maxWidth: 76,
                                minWidth: 76,
                                px: 0.5,
                              }),
                            }}
                          >
                            {flexRender(cell.column.columnDef.cell, cell.getContext())}
                          </TableCell>
                        ))}
                      </TableRow>
                    );
                  })}
                </TableBody>
              </Table>
            </TableContainer>
          )}
        </Box>
      </Box>
    </Fade>
  );
}
