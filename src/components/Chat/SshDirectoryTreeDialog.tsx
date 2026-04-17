import { useState, useEffect, useCallback, useRef, useMemo } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  Box,
  Button,
  Dialog,
  DialogActions,
  DialogContent,
  DialogTitle,
  Typography,
  CircularProgress,
  List,
  ListItem,
  ListItemButton,
  ListItemIcon,
  ListItemText,
  Collapse,
  Alert,
  TextField,
  IconButton,
} from "@mui/material";
import {
  Folder as FolderIcon,
  ExpandMore,
  ChevronRight,
  CreateNewFolder,
} from "@mui/icons-material";

interface DirectoryEntry {
  name: string;
  path: string;
  is_directory: boolean;
  size?: number | null;
  modified?: string | null;
}

interface DirectoryListResponse {
  directory: string;
  entries: DirectoryEntry[];
  total: number;
  has_more: boolean;
}

function formatError(e: unknown): string {
  if (e instanceof Error) return e.message;
  if (typeof e === "string") return e;
  if (
    e &&
    typeof e === "object" &&
    "message" in e &&
    typeof (e as { message?: unknown }).message === "string"
  ) {
    return (e as { message: string }).message;
  }
  try {
    return JSON.stringify(e);
  } catch {
    return String(e);
  }
}

interface SshDirectoryTreeDialogProps {
  open: boolean;
  onClose: () => void;
  sshProfileName: string;
  defaultPath?: string;
  onConfirm: (path: string) => void;
}

interface TreeNodeProps {
  entry: DirectoryEntry;
  selectedPath: string;
  expandedPaths: Set<string>;
  loadingPaths: Set<string>;
  directoryCache: Record<string, DirectoryEntry[]>;
  filterQuery: string;
  onToggleExpand: (path: string) => void;
  onSelect: (path: string) => void;
  level: number;
}

function TreeNode({
  entry,
  selectedPath,
  expandedPaths,
  loadingPaths,
  directoryCache,
  filterQuery,
  onToggleExpand,
  onSelect,
  level,
}: TreeNodeProps) {
  const isExpanded = expandedPaths.has(entry.path);
  const isLoading = loadingPaths.has(entry.path);
  const allChildren = (directoryCache[entry.path] ?? []).filter(
    (e) => e.is_directory,
  );
  const children = useMemo(() => {
    if (!filterQuery) return allChildren;
    const q = filterQuery.toLowerCase();
    return allChildren.filter((c) => c.name.toLowerCase().includes(q));
  }, [allChildren, filterQuery]);
  const isSelected = selectedPath === entry.path;

  const handleToggle = (e: React.MouseEvent) => {
    e.stopPropagation();
    onToggleExpand(entry.path);
  };

  const handleSelect = () => {
    onSelect(entry.path);
  };

  return (
    <>
      <ListItem disablePadding dense sx={{ py: 0, my: 0 }}>
        <ListItemButton
          selected={isSelected}
          onClick={handleSelect}
          dense
          sx={{
            pl: 0.75 + level * 0.875,
            py: 0,
            minHeight: 28,
            borderRadius: 0.5,
            mx: 0.25,
            my: 0,
          }}
        >
          <ListItemIcon
            onClick={handleToggle}
            sx={{
              minWidth: 20,
              mr: 0.25,
              justifyContent: "center",
            }}
          >
            {isLoading ? (
              <CircularProgress size={16} thickness={5} />
            ) : isExpanded ? (
              <ExpandMore sx={{ fontSize: 18 }} />
            ) : allChildren.length > 0 || !directoryCache[entry.path] ? (
              <ChevronRight sx={{ fontSize: 18 }} />
            ) : null}
          </ListItemIcon>
          <ListItemIcon sx={{ minWidth: 20, mr: 0.5 }}>
            <FolderIcon sx={{ fontSize: 18, color: "primary.main" }} />
          </ListItemIcon>
          <ListItemText
            primary={entry.name}
            primaryTypographyProps={{
              variant: "body2",
              noWrap: true,
              fontSize: 14,
              lineHeight: 1.3,
            }}
          />
        </ListItemButton>
      </ListItem>
      {isExpanded && (
        <Collapse in={isExpanded} timeout="auto" unmountOnExit sx={{ my: 0 }}>
          <List disablePadding dense sx={{ py: 0 }}>
            {children.map((child) => (
              <TreeNode
                key={child.path}
                entry={child}
                selectedPath={selectedPath}
                expandedPaths={expandedPaths}
                loadingPaths={loadingPaths}
                directoryCache={directoryCache}
                filterQuery={filterQuery}
                onToggleExpand={onToggleExpand}
                onSelect={onSelect}
                level={level + 1}
              />
            ))}
          </List>
        </Collapse>
      )}
    </>
  );
}

export function SshDirectoryTreeDialog({
  open,
  onClose,
  sshProfileName,
  defaultPath,
  onConfirm,
}: SshDirectoryTreeDialogProps) {
  const [pathInput, setPathInput] = useState<string>("");
  const [treeRootPath, setTreeRootPath] = useState<string>("");
  const [directoryCache, setDirectoryCache] = useState<
    Record<string, DirectoryEntry[]>
  >({});
  const [expandedPaths, setExpandedPaths] = useState<Set<string>>(new Set());
  const [loadingPaths, setLoadingPaths] = useState<Set<string>>(new Set());
  const [isInitializing, setIsInitializing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [newFolderName, setNewFolderName] = useState("");
  const [isCreatingFolder, setIsCreatingFolder] = useState(false);

  const loadingPathsRef = useRef<Set<string>>(new Set());
  const isTypingRef = useRef(false);

  useEffect(() => {
    loadingPathsRef.current = loadingPaths;
  }, [loadingPaths]);

  const filterQuery = useMemo(() => {
    const trimmed = pathInput.trim();
    if (trimmed === treeRootPath) return "";
    if (trimmed.endsWith("/")) return "";
    const lastSegment = trimmed.replace(/.*\//, "");
    return lastSegment;
  }, [pathInput, treeRootPath]);

  const loadDirectory = useCallback(
    async (path: string) => {
      if (loadingPathsRef.current.has(path)) return;
      setLoadingPaths((prev) => {
        if (prev.has(path)) return prev;
        const next = new Set(prev);
        next.add(path);
        return next;
      });
      try {
        const result = (await invoke("ssh_list_directory", {
          sshProfileName,
          path,
        })) as DirectoryListResponse;
        setDirectoryCache((prev) => ({
          ...prev,
          [path]: result.entries,
        }));
        setError(null);
      } catch (e) {
        setError(formatError(e));
      } finally {
        setLoadingPaths((prev) => {
          if (!prev.has(path)) return prev;
          const next = new Set(prev);
          next.delete(path);
          return next;
        });
      }
    },
    [sshProfileName],
  );

  const navigateToPath = useCallback(
    async (path: string) => {
      const trimmed = path.trim();
      if (!trimmed || (!trimmed.startsWith("/") && !trimmed.startsWith("~"))) {
        setError("路径须为绝对路径（以 / 或 ~ 开头）");
        return;
      }
      isTypingRef.current = false;
      setPathInput(trimmed);
      setTreeRootPath(trimmed);
      setExpandedPaths((prev) => {
        const next = new Set(prev);
        next.add(trimmed);
        return next;
      });
      await loadDirectory(trimmed);
    },
    [loadDirectory],
  );

  const handleToggleExpand = useCallback(
    (path: string) => {
      setExpandedPaths((prev) => {
        const next = new Set(prev);
        if (next.has(path)) {
          next.delete(path);
        } else {
          next.add(path);
        }
        return next;
      });
      loadDirectory(path);
    },
    [loadDirectory],
  );

  const handleSelect = useCallback(
    (path: string) => {
      isTypingRef.current = false;
      setPathInput(path);
      setTreeRootPath(path);
      setExpandedPaths((prev) => {
        const next = new Set(prev);
        next.add(path);
        return next;
      });
      loadDirectory(path);
    },
    [loadDirectory],
  );

  useEffect(() => {
    if (!open || !sshProfileName) return;
    if (!isTypingRef.current) return;

    const trimmed = pathInput.trim();
    if (
      !trimmed ||
      (!trimmed.startsWith("/") && !trimmed.startsWith("~"))
    ) {
      return;
    }

    let newRoot = trimmed;
    if (!trimmed.endsWith("/")) {
      const lastSlash = trimmed.lastIndexOf("/");
      if (lastSlash >= 0) {
        newRoot = trimmed.slice(0, lastSlash + 1);
      }
    }

    if (newRoot === treeRootPath) return;

    const timer = setTimeout(() => {
      setTreeRootPath(newRoot);
      setExpandedPaths((prev) => {
        const next = new Set(prev);
        next.add(newRoot);
        return next;
      });
      loadDirectory(newRoot);
    }, 300);

    return () => clearTimeout(timer);
  }, [pathInput, treeRootPath, loadDirectory, open, sshProfileName]);

  useEffect(() => {
    if (!open || !sshProfileName) return;

    let cancelled = false;

    const init = async () => {
      setIsInitializing(true);
      setError(null);
      setDirectoryCache({});
      setExpandedPaths(new Set());
      setNewFolderName("");
      setIsCreatingFolder(false);

      let targetPath = defaultPath?.trim();
      if (
        !targetPath ||
        targetPath === "." ||
        (!targetPath.startsWith("/") && !targetPath.startsWith("~"))
      ) {
        try {
          targetPath = (await invoke("ssh_get_home_directory", {
            sshProfileName,
          })) as string;
        } catch (e) {
          if (!cancelled) {
            setError(formatError(e));
          }
          setIsInitializing(false);
          return;
        }
      }

      if (cancelled) return;
      setPathInput(targetPath);
      setTreeRootPath(targetPath);
      setExpandedPaths((prev) => {
        const next = new Set(prev);
        next.add(targetPath);
        return next;
      });

      setLoadingPaths((prev) => {
        const next = new Set(prev);
        next.add(targetPath);
        return next;
      });
      try {
        const result = (await invoke("ssh_list_directory", {
          sshProfileName,
          path: targetPath,
        })) as DirectoryListResponse;
        if (!cancelled) {
          setDirectoryCache((prev) => ({
            ...prev,
            [targetPath]: result.entries,
          }));
          setError(null);
        }
      } catch (e) {
        if (!cancelled) {
          setError(formatError(e));
        }
      } finally {
        if (!cancelled) {
          setLoadingPaths((prev) => {
            const next = new Set(prev);
            next.delete(targetPath);
            return next;
          });
          setIsInitializing(false);
        }
      }
    };

    void init();
    return () => {
      cancelled = true;
    };
  }, [open, sshProfileName, defaultPath]);

  const handleCreateFolder = async () => {
    const name = newFolderName.trim();
    const parentPath = treeRootPath;
    if (!name || !parentPath) return;
    const parent = parentPath.endsWith("/") ? parentPath : parentPath + "/";
    const newPath = parent + name;
    try {
      await invoke("ssh_create_directory", {
        sshProfileName,
        path: newPath,
      });
      setNewFolderName("");
      setIsCreatingFolder(false);
      await loadDirectory(parentPath);
      setExpandedPaths((prev) => {
        const next = new Set(prev);
        next.add(parentPath);
        return next;
      });
    } catch (e) {
      setError(formatError(e));
    }
  };

  const rootEntry = treeRootPath
    ? {
        name:
          treeRootPath === "/"
            ? "/"
            : treeRootPath.replace(/.*\//, "") || treeRootPath,
        path: treeRootPath,
        is_directory: true,
      }
    : null;

  const confirmPath = pathInput.trim();

  return (
    <Dialog
      open={open}
      onClose={onClose}
      maxWidth="sm"
      fullWidth
      PaperProps={{ sx: { minHeight: 360 } }}
    >
      <DialogTitle sx={{ px: 2, py: 1.5, fontSize: "1rem", fontWeight: 600 }}>
        选择远程工作目录
      </DialogTitle>
      <DialogContent sx={{ px: 2, py: 0 }}>
        <Box
          sx={{ display: "flex", alignItems: "center", gap: 1, mt: 2, mb: 1 }}
        >
          <TextField
            fullWidth
            size="small"
            label="远程路径"
            value={pathInput}
            onChange={(e) => {
              setPathInput(e.target.value);
              isTypingRef.current = true;
            }}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                e.preventDefault();
                void navigateToPath(pathInput);
              }
            }}
            placeholder="/home/username/project"
            InputProps={{ sx: { fontFamily: "monospace", fontSize: 13 } }}
          />
          <IconButton
            size="medium"
            color="primary"
            title="新建文件夹"
            onClick={() => setIsCreatingFolder((v) => !v)}
          >
            <CreateNewFolder fontSize="medium" />
          </IconButton>
        </Box>

        {isCreatingFolder && (
          <Box sx={{ display: "flex", gap: 1, mb: 1 }}>
            <TextField
              fullWidth
              size="small"
              label="新文件夹名称"
              value={newFolderName}
              onChange={(e) => setNewFolderName(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") {
                  e.preventDefault();
                  void handleCreateFolder();
                }
              }}
              autoFocus
              placeholder="newfolder"
              InputProps={{ sx: { fontSize: 13 } }}
            />
            <Button
              size="small"
              variant="contained"
              onClick={() => void handleCreateFolder()}
              disabled={!newFolderName.trim() || !treeRootPath}
            >
              创建
            </Button>
            <Button
              size="small"
              variant="outlined"
              color="inherit"
              onClick={() => {
                setIsCreatingFolder(false);
                setNewFolderName("");
              }}
            >
              取消
            </Button>
          </Box>
        )}

        {error && (
          <Alert severity="error" sx={{ mb: 1, py: 0.5, px: 1, fontSize: 12 }}>
            {error}
          </Alert>
        )}

        <Box
          sx={{
            border: 1,
            borderColor: "divider",
            borderRadius: 1,
            height: 260,
            overflow: "auto",
            bgcolor: "background.paper",
          }}
        >
          {isInitializing ? (
            <Box sx={{ p: 3, textAlign: "center" }}>
              <CircularProgress size={20} sx={{ mb: 0.75 }} />
              <Typography variant="caption" color="text.secondary">
                正在加载远程目录...
              </Typography>
            </Box>
          ) : rootEntry ? (
            <List disablePadding dense sx={{ py: 0 }}>
              <TreeNode
                entry={rootEntry}
                selectedPath={confirmPath}
                expandedPaths={expandedPaths}
                loadingPaths={loadingPaths}
                directoryCache={directoryCache}
                filterQuery={filterQuery}
                onToggleExpand={handleToggleExpand}
                onSelect={handleSelect}
                level={0}
              />
            </List>
          ) : (
            <Box sx={{ p: 2, textAlign: "center" }}>
              <Typography variant="caption" color="text.secondary">
                无法获取远程目录
              </Typography>
            </Box>
          )}
        </Box>
      </DialogContent>
      <DialogActions sx={{ px: 2, py: 1 }}>
        <Button
          size="small"
          variant="outlined"
          color="inherit"
          onClick={onClose}
          sx={{ textTransform: "none" }}
        >
          取消
        </Button>
        <Button
          size="small"
          variant="contained"
          onClick={() => onConfirm(confirmPath)}
          disabled={
            !confirmPath ||
            (!confirmPath.startsWith("/") && !confirmPath.startsWith("~"))
          }
          sx={{ textTransform: "none" }}
        >
          确定
        </Button>
      </DialogActions>
    </Dialog>
  );
}
