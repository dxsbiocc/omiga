/**
 * UnifiedMemoryTab — Unified settings panel for the Memory system
 *
 * Manages both:
 * - Explicit Memory (Wiki): External knowledge base documents (PDF, MD, TXT, etc.)
 * - Implicit Memory (PageIndex): Auto-indexed chat history
 */

import { useState } from "react";
import { open as openFileDialog } from "@tauri-apps/plugin-dialog";
import {
  Box,
  Typography,
  Alert,
  Button,
  Chip,
  CircularProgress,
  Divider,
  Stack,
  TextField,
  Paper,
  Tooltip,
  Snackbar,
  LinearProgress,
  Tabs,
  Tab,
  Switch,
  FormControlLabel,
  Dialog,
  DialogTitle,
  DialogContent,
  DialogActions,
  Card,
  CardContent,
  Grid,
  useTheme,
} from "@mui/material";
import { alpha } from "@mui/material/styles";
import {
  Storage as MemoryIcon,
  Search as SearchIcon,
  Refresh as RefreshIcon,
  Build as BuildIcon,
  Update as UpdateIcon,
  DeleteForever as ClearIcon,
  Upgrade as MigrateIcon,
  FolderOpen as FolderIcon,
  CheckCircle as OkIcon,
  Cancel as MissingIcon,
  Settings as ConfigIcon,
  Book as WikiIcon,
  AutoFixHigh as ImplicitIcon,
  FileUpload as UploadIcon,
  TextFields as TextIcon,
  Folder as FolderUploadIcon,
} from "@mui/icons-material";
import { useUnifiedMemory } from "../../hooks/useUnifiedMemory";

interface UnifiedMemoryTabProps {
  projectPath: string;
}

export function UnifiedMemoryTab({ projectPath }: UnifiedMemoryTabProps) {
  const theme = useTheme();
  const memory = useUnifiedMemory(projectPath);
  const [toast, setToast] = useState<string | null>(null);
  const [showPathDialog, setShowPathDialog] = useState(false);
  const [newPath, setNewPath] = useState("");
  const [rawDirInput, setRawDirInput] = useState("");
  
  // Import states
  const [importSourcePath, setImportSourcePath] = useState("");
  const [importTextTitle, setImportTextTitle] = useState("");
  const [importTextContent, setImportTextContent] = useState("");
  const [importTags, setImportTags] = useState("");
  const [showImportDialog, setShowImportDialog] = useState(false);
  const [importType, setImportType] = useState<"file" | "directory" | "text">("file");
  // Explicit memory is always user-level (global knowledge base)
  const importMemoryLevel = "user" as const;

  const handleSearch = async () => {
    if (memory.searchQuery.trim()) {
      await memory.search(memory.searchQuery.trim(), 5);
    }
  };

  const handleBuild = async () => {
    await memory.buildIndex();
    if (!memory.error) {
      setToast("索引构建完成");
    }
  };

  const handleUpdate = async () => {
    await memory.updateIndex();
    if (!memory.error) {
      setToast("索引更新完成");
    }
  };

  const handleMigrate = async () => {
    const migrated = await memory.migrate();
    if (migrated) {
      setToast("迁移完成");
    } else {
      setToast("无需迁移");
    }
  };

  const handleSavePath = async () => {
    if (!memory.isValidPath(newPath)) {
      setToast("无效的路径");
      return;
    }
    const success = await memory.updateConfig({ root_dir: newPath });
    if (success) {
      setToast("路径已更新");
      setShowPathDialog(false);
    }
  };

  const formatBytes = (bytes: number): string => {
    if (bytes === 0) return "0 B";
    const k = 1024;
    const sizes = ["B", "KB", "MB", "GB"];
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    return parseFloat((bytes / Math.pow(k, i)).toFixed(2)) + " " + sizes[i];
  };

  const handleImport = async () => {
    const tags = importTags.split(",").map(t => t.trim()).filter(Boolean);
    const options = { include_content: true, tags, memory_level: importMemoryLevel };
    
    let result;
    if (importType === "text") {
      result = await memory.importToWiki("text", undefined, importTextTitle, importTextContent, options);
    } else {
      result = await memory.importToWiki(importType, importSourcePath, undefined, undefined, options);
    }
    
    if (result) {
      if (result.success) {
        setToast(`导入成功: ${result.imported_count} 个页面`);
        setShowImportDialog(false);
        setImportSourcePath("");
        setImportTextTitle("");
        setImportTextContent("");
        setImportTags("");
      } else {
        setToast(`导入失败: ${result.errors.join(", ")}`);
      }
    }
  };



  const handleTabChange = (_: React.SyntheticEvent, value: string) => {
    memory.setActiveTab(value as typeof memory.activeTab);
  };

  const glassSurface = {
    borderRadius: 2.5,
    border: `1px solid ${alpha(theme.palette.divider, 0.55)}`,
    bgcolor: alpha(theme.palette.background.paper, 0.72),
    backdropFilter: "blur(16px) saturate(150%)",
    WebkitBackdropFilter: "blur(16px) saturate(150%)",
    boxShadow: `0 1px 0 ${alpha(theme.palette.common.black, 0.04)}, 0 16px 44px ${alpha(theme.palette.common.black, 0.07)}`,
  } as const;

  return (
    <Box
      sx={{
        display: "flex",
        flexDirection: "column",
        gap: 2.5,
        maxWidth: 920,
        mx: "auto",
        pb: 1,
      }}
    >
      {/* Hero */}
      <Box
        sx={{
          position: "relative",
          overflow: "hidden",
          p: { xs: 2, sm: 2.5 },
          ...glassSurface,
          borderRadius: 3,
          border: `1px solid ${alpha(theme.palette.primary.main, 0.22)}`,
          background: `linear-gradient(125deg, ${alpha(theme.palette.primary.main, 0.14)} 0%, ${alpha(theme.palette.secondary.main, 0.09)} 42%, ${alpha(theme.palette.background.paper, 0.85)} 100%)`,
        }}
      >
        <Box
          aria-hidden
          sx={{
            position: "absolute",
            right: -40,
            top: -60,
            width: 220,
            height: 220,
            borderRadius: "50%",
            background: `radial-gradient(circle, ${alpha(theme.palette.primary.main, 0.18)} 0%, transparent 70%)`,
            pointerEvents: "none",
          }}
        />
        <Stack direction={{ xs: "column", sm: "row" }} spacing={2} alignItems={{ sm: "flex-start" }}>
          <Box
            sx={{
              width: 52,
              height: 52,
              borderRadius: 2,
              flexShrink: 0,
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              bgcolor: alpha(theme.palette.primary.main, 0.15),
              color: "primary.main",
              border: `1px solid ${alpha(theme.palette.primary.main, 0.28)}`,
            }}
          >
            <MemoryIcon sx={{ fontSize: 28 }} />
          </Box>
          <Box sx={{ minWidth: 0, position: "relative", zIndex: 1 }}>
            <Typography
              variant="overline"
              sx={{
                letterSpacing: 0.14,
                fontWeight: 700,
                color: "text.secondary",
                display: "block",
                mb: 0.5,
              }}
            >
              Knowledge · 统一记忆
            </Typography>
            <Typography variant="body2" color="text.primary" sx={{ lineHeight: 1.65 }}>
              <strong>全局知识库</strong>是用户级全局存储，保存在{" "}
              <strong>~/.omiga/memory/permanent/wiki</strong>，跨所有项目可用。
              <strong>隐性记忆</strong>（PageIndex）自动索引聊天历史，按项目存储在{" "}
              <strong>~/.omiga/memory/projects/&lt;项目键&gt;/</strong>。
              所有路径登记在 <strong>~/.omiga/memory/registry.json</strong>。
            </Typography>
          </Box>
        </Stack>
      </Box>

      {/* Migration Warning */}
      {memory.status?.needs_migration && (
        <Alert
          severity="warning"
          action={
            <Button
              color="inherit"
              size="small"
              variant="outlined"
              sx={{ borderColor: alpha(theme.palette.warning.main, 0.45) }}
              startIcon={<MigrateIcon />}
              onClick={handleMigrate}
              disabled={memory.loading}
            >
              迁移
            </Button>
          }
          sx={{
            borderRadius: 2.5,
            border: `1px solid ${alpha(theme.palette.warning.main, 0.35)}`,
            bgcolor: alpha(theme.palette.warning.main, 0.06),
          }}
        >
          检测到旧版记忆结构，需要迁移到新版统一结构。
        </Alert>
      )}

      {/* Overview — bento */}
      <Paper elevation={0} sx={{ p: 2.25, ...glassSurface }}>
        <Stack direction="row" alignItems="center" justifyContent="space-between" mb={2}>
          <Box>
            <Typography variant="overline" sx={{ letterSpacing: 0.12, color: "text.secondary", fontWeight: 700 }}>
              概览
            </Typography>
            <Typography variant="subtitle1" fontWeight={700} sx={{ mt: 0.25 }}>
              记忆概览
            </Typography>
          </Box>
          <Tooltip title="刷新状态">
            <Button
              size="small"
              variant="outlined"
              startIcon={memory.loading ? <CircularProgress size={14} /> : <RefreshIcon />}
              onClick={memory.refresh}
              disabled={memory.loading}
              sx={{ borderRadius: 2, textTransform: "none", fontWeight: 600 }}
            >
              刷新
            </Button>
          </Tooltip>
        </Stack>

        {memory.status ? (
          <Stack spacing={2}>
            <Grid container spacing={1.5}>
              <Grid item xs={12} sm={6}>
                <Card
                  elevation={0}
                  sx={{
                    height: "100%",
                    borderRadius: 2,
                    border: `1px solid ${alpha(theme.palette.primary.main, 0.2)}`,
                    bgcolor: alpha(theme.palette.primary.main, 0.04),
                    transition: "transform 0.2s ease, box-shadow 0.2s ease",
                    "&:hover": {
                      boxShadow: `0 8px 24px ${alpha(theme.palette.primary.main, 0.12)}`,
                    },
                  }}
                >
                  <CardContent sx={{ p: 2, "&:last-child": { pb: 2 } }}>
                    <Stack direction="row" alignItems="center" justifyContent="space-between" mb={1}>
                      <Stack direction="row" alignItems="center" spacing={1}>
                        <WikiIcon sx={{ color: "primary.main", fontSize: 22 }} />
                        <Typography variant="subtitle2" fontWeight={700}>
                          显性记忆
                        </Typography>
                      </Stack>
                      {memory.status.explicit.enabled ? (
                        <Chip size="small" icon={<OkIcon />} label="已启用" color="success" variant="outlined" />
                      ) : (
                        <Chip size="small" icon={<MissingIcon />} label="未启用" variant="outlined" />
                      )}
                    </Stack>
                    <Typography variant="h4" fontWeight={800} sx={{ fontFeatureSettings: '"tnum"', color: "text.primary" }}>
                      {memory.status.explicit.enabled ? memory.status.explicit.document_count : "—"}
                    </Typography>
                    <Typography variant="caption" color="text.secondary">
                      知识库页面数
                    </Typography>
                  </CardContent>
                </Card>
              </Grid>
              <Grid item xs={12} sm={6}>
                <Card
                  elevation={0}
                  sx={{
                    height: "100%",
                    borderRadius: 2,
                    border: `1px solid ${alpha(theme.palette.secondary.main, 0.22)}`,
                    bgcolor: alpha(theme.palette.secondary.main, 0.05),
                    transition: "transform 0.2s ease, box-shadow 0.2s ease",
                    "&:hover": {
                      boxShadow: `0 8px 24px ${alpha(theme.palette.secondary.main, 0.12)}`,
                    },
                  }}
                >
                  <CardContent sx={{ p: 2, "&:last-child": { pb: 2 } }}>
                    <Stack direction="row" alignItems="center" justifyContent="space-between" mb={1}>
                      <Stack direction="row" alignItems="center" spacing={1}>
                        <ImplicitIcon sx={{ color: "secondary.main", fontSize: 22 }} />
                        <Typography variant="subtitle2" fontWeight={700}>
                          隐性记忆
                        </Typography>
                      </Stack>
                      {memory.status.implicit.enabled && memory.status.implicit.document_count > 0 ? (
                        <Chip size="small" icon={<OkIcon />} label="已索引" color="success" variant="outlined" />
                      ) : (
                        <Chip size="small" icon={<MissingIcon />} label="未构建" variant="outlined" />
                      )}
                    </Stack>
                    <Typography variant="h4" fontWeight={800} sx={{ fontFeatureSettings: '"tnum"', color: "text.primary" }}>
                      {memory.status.implicit.enabled && memory.status.implicit.document_count > 0
                        ? memory.status.implicit.document_count
                        : "—"}
                    </Typography>
                    <Typography variant="caption" color="text.secondary" display="block">
                      {memory.status.implicit.enabled && memory.status.implicit.document_count > 0
                        ? `${memory.status.implicit.section_count} 章节 · ${formatBytes(memory.status.implicit.total_bytes)}`
                        : "对话后自动索引聊天记录"}
                    </Typography>
                  </CardContent>
                </Card>
              </Grid>
            </Grid>

            <Box
              sx={{
                p: 1.5,
                borderRadius: 2,
                border: `1px dashed ${alpha(theme.palette.divider, 0.9)}`,
                bgcolor: alpha(theme.palette.action.hover, 0.04),
              }}
            >
              <Typography variant="caption" color="text.secondary" display="block" sx={{ mb: 1 }}>
                模式：{memory.config?.memory_mode === "project_relative" ? "项目内 .omiga/memory" : "用户目录 ~/.omiga/memory/projects/…"}
              </Typography>
              <Stack direction={{ xs: "column", sm: "row" }} spacing={1} alignItems={{ sm: "center" }}>
                <Stack direction="row" spacing={1} alignItems="center" sx={{ minWidth: 0, flex: 1 }}>
                  <FolderIcon sx={{ color: "action.active", flexShrink: 0 }} />
                  <Typography
                    variant="body2"
                    color="text.secondary"
                    sx={{ fontFamily: "ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace", wordBreak: "break-all" }}
                  >
                    {memory.status.paths.root}
                  </Typography>
                </Stack>
                <Button
                  size="small"
                  variant="contained"
                  onClick={() => {
                    setNewPath(memory.config?.root_dir || ".omiga/memory");
                    setShowPathDialog(true);
                  }}
                  sx={{ alignSelf: { xs: "stretch", sm: "center" }, borderRadius: 2, textTransform: "none", fontWeight: 600 }}
                >
                  修改路径
                </Button>
              </Stack>
            </Box>
          </Stack>
        ) : (
          <Stack direction="row" alignItems="center" spacing={1.5} sx={{ py: 2 }}>
            <CircularProgress size={22} />
            <Typography variant="body2" color="text.secondary">
              加载状态中…
            </Typography>
          </Stack>
        )}
      </Paper>

      {/* Tabs */}
      <Tabs
        value={memory.activeTab}
        onChange={handleTabChange}
        variant="scrollable"
        scrollButtons="auto"
        sx={{
          minHeight: 48,
          px: 0.5,
          py: 0.5,
          borderRadius: 2,
          bgcolor: alpha(theme.palette.action.hover, 0.12),
          border: `1px solid ${alpha(theme.palette.divider, 0.35)}`,
          "& .MuiTabs-flexContainer": { gap: 0.25 },
          "& .MuiTab-root": {
            minHeight: 44,
            borderRadius: 1.5,
            textTransform: "none",
            fontWeight: 600,
            fontSize: "0.8125rem",
          },
          "& .Mui-selected": {
            bgcolor: alpha(theme.palette.background.paper, 0.95),
            boxShadow: `0 1px 4px ${alpha(theme.palette.common.black, 0.08)}`,
          },
          "& .MuiTabs-indicator": { display: "none" },
        }}
      >
        <Tab value="overview" label="概览" />
        <Tab value="knowledge" label="知识库" icon={<WikiIcon />} iconPosition="start" />
        <Tab value="implicit" label="隐性记忆" icon={<ImplicitIcon />} iconPosition="start" />
        <Tab value="config" label="配置" icon={<ConfigIcon />} iconPosition="start" />
      </Tabs>

      {/* Tab Content */}
      <Box sx={{ py: 1.5 }}>
        {/* Overview */}
        {memory.activeTab === "overview" && (
          <Paper elevation={0} sx={{ p: 2.25, ...glassSurface }}>
            <Typography variant="overline" sx={{ letterSpacing: 0.12, color: "text.secondary", fontWeight: 700 }}>
              工作原理
            </Typography>
            <Typography variant="subtitle1" fontWeight={700} sx={{ mb: 1.5 }}>
              记忆如何进入对话
            </Typography>
            <Box
              component="ol"
              sx={{
                pl: 2.25,
                m: 0,
                "& li": { mb: 1 },
                typography: "body2",
                lineHeight: 1.65,
                "& code": {
                  px: 0.75,
                  py: 0.15,
                  borderRadius: 1,
                  fontSize: "0.8em",
                  bgcolor: alpha(theme.palette.primary.main, 0.08),
                  border: `1px solid ${alpha(theme.palette.primary.main, 0.15)}`,
                },
              }}
            >
              <li>
                <strong>全局知识库</strong>：用户级全局知识库（PDF、MD、TXT 等），位于{" "}
                <code>{memory.status?.paths.permanent_wiki || "~/.omiga/memory/permanent/wiki"}</code>
              </li>
              <li>
                <strong>长期记忆</strong>：项目级与全局级可召回总结，位于{" "}
                <code>{memory.status?.paths.long_term || "~/.omiga/memory/projects/<key>/long_term"}</code> /{" "}
                <code>{memory.status?.paths.permanent_long_term || "~/.omiga/memory/permanent/long_term"}</code>
              </li>
              <li>
                <strong>工作记忆</strong>：session scratchpad，保存在 SQLite，会在当前轮按需注入
              </li>
              <li>
                <strong>隐性记忆</strong>：自动索引的聊天历史，每次对话后更新
              </li>
              <li>每次对话前自动检索相关上下文</li>
              <li>显性记忆优先于隐性记忆</li>
            </Box>

            <Divider sx={{ my: 2 }} />

            <Typography variant="body2" color="text.secondary" fontWeight={600} sx={{ mb: 1 }}>
              快速操作
            </Typography>
            <Stack direction="row" spacing={1} flexWrap="wrap" useFlexGap>
              <Button
                variant="contained"
                color="secondary"
                size="small"
                startIcon={<BuildIcon />}
                onClick={handleBuild}
                disabled={memory.building}
                sx={{ borderRadius: 2, textTransform: "none", fontWeight: 600 }}
              >
                构建索引
              </Button>
              <Button
                variant="outlined"
                size="small"
                startIcon={<UpdateIcon />}
                onClick={handleUpdate}
                disabled={memory.building}
                sx={{ borderRadius: 2, textTransform: "none", fontWeight: 600 }}
              >
                增量更新
              </Button>
              <Button
                variant="outlined"
                size="small"
                startIcon={<WikiIcon />}
                onClick={() => memory.setActiveTab("knowledge")}
                sx={{ borderRadius: 2, textTransform: "none", fontWeight: 600 }}
              >
                知识库
              </Button>
              <Button
                variant="outlined"
                size="small"
                startIcon={<SearchIcon />}
                onClick={() => memory.setActiveTab("implicit")}
                sx={{ borderRadius: 2, textTransform: "none", fontWeight: 600 }}
              >
                去搜索
              </Button>
            </Stack>
          </Paper>
        )}

        {/* Knowledge Base (Explicit Memory + Import) */}
        {memory.activeTab === "knowledge" && (
          <Stack spacing={2}>
            <Alert
              severity="info"
              sx={{
                borderRadius: 2.5,
                border: `1px solid ${alpha(theme.palette.info.main, 0.25)}`,
                bgcolor: alpha(theme.palette.info.main, 0.06),
              }}
            >
              全局知识库是<strong>用户级全局存储</strong>，跨所有项目可用。导入的文档将保存到{" "}
              <strong>~/.omiga/memory/permanent/wiki</strong>，对话时自动检索提供上下文。
            </Alert>

            <Paper elevation={0} sx={{ p: 2, ...glassSurface }}>
              <Stack direction="row" alignItems="center" spacing={1} mb={1.5}>
                <WikiIcon sx={{ color: "primary.main", fontSize: 20 }} />
                <Box>
                  <Typography variant="subtitle2" fontWeight={700}>
                    全局知识库路径
                  </Typography>
                  <Typography variant="caption" color="text.secondary">
                    跨项目共享 · {memory.status?.knowledge_base.global_page_count ?? memory.status?.explicit.document_count ?? 0} 个页面
                  </Typography>
                </Box>
              </Stack>
              <Typography
                variant="body2"
                sx={{
                  fontFamily: "ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace",
                  wordBreak: "break-all",
                  color: "text.primary",
                  p: 1,
                  borderRadius: 1.5,
                  bgcolor: alpha(theme.palette.action.hover, 0.06),
                  border: `1px solid ${alpha(theme.palette.divider, 0.4)}`,
                }}
              >
                {memory.status?.paths.permanent_wiki || "~/.omiga/memory/permanent/wiki"}
              </Typography>
            </Paper>

            <Box>
              <Typography variant="overline" sx={{ letterSpacing: 0.12, color: "text.secondary", fontWeight: 700, display: "block", mb: 1 }}>
                导入文档到知识库
              </Typography>
              <Alert
                severity="warning"
                sx={{
                  mb: 1.5,
                  borderRadius: 2.5,
                  border: `1px solid ${alpha(theme.palette.warning.main, 0.3)}`,
                  bgcolor: alpha(theme.palette.warning.main, 0.05),
                }}
              >
                <strong>支持格式：</strong>Markdown、TXT、PDF、HTML、JSON/YAML/TOML 等文档。
                <br />
                <em>聊天记录会自动索引到隐性记忆，无需手动导入。</em>
              </Alert>

              <Grid container spacing={1.5}>
                <Grid item xs={12} sm={4}>
                  <Card
                    elevation={0}
                    sx={{
                      height: "100%",
                      cursor: "pointer",
                      borderRadius: 2,
                      border: `1px solid ${alpha(theme.palette.divider, 0.55)}`,
                      bgcolor: alpha(theme.palette.background.paper, 0.55),
                      transition: "all 0.2s ease",
                      "&:hover": {
                        borderColor: alpha(theme.palette.primary.main, 0.45),
                        boxShadow: `0 8px 28px ${alpha(theme.palette.primary.main, 0.1)}`,
                      },
                    }}
                    onClick={() => {
                      setImportType("file");
                      setImportSourcePath("");
                      setShowImportDialog(true);
                    }}
                  >
                    <CardContent sx={{ py: 2, "&:last-child": { pb: 2 } }}>
                      <UploadIcon color="primary" sx={{ mb: 1 }} />
                      <Typography variant="subtitle2" fontWeight={700}>
                        导入文件
                      </Typography>
                      <Typography variant="caption" color="text.secondary">
                        单文件导入
                      </Typography>
                    </CardContent>
                  </Card>
                </Grid>
                <Grid item xs={12} sm={4}>
                  <Card
                    elevation={0}
                    sx={{
                      height: "100%",
                      cursor: "pointer",
                      borderRadius: 2,
                      border: `1px solid ${alpha(theme.palette.divider, 0.55)}`,
                      bgcolor: alpha(theme.palette.background.paper, 0.55),
                      transition: "all 0.2s ease",
                      "&:hover": {
                        borderColor: alpha(theme.palette.secondary.main, 0.45),
                        boxShadow: `0 8px 28px ${alpha(theme.palette.secondary.main, 0.1)}`,
                      },
                    }}
                    onClick={() => {
                      setImportType("directory");
                      setImportSourcePath("");
                      setShowImportDialog(true);
                    }}
                  >
                    <CardContent sx={{ py: 2, "&:last-child": { pb: 2 } }}>
                      <FolderUploadIcon color="secondary" sx={{ mb: 1 }} />
                      <Typography variant="subtitle2" fontWeight={700}>
                        导入文件夹
                      </Typography>
                      <Typography variant="caption" color="text.secondary">
                        批量目录
                      </Typography>
                    </CardContent>
                  </Card>
                </Grid>
                <Grid item xs={12} sm={4}>
                  <Card
                    elevation={0}
                    sx={{
                      height: "100%",
                      cursor: "pointer",
                      borderRadius: 2,
                      border: `1px solid ${alpha(theme.palette.divider, 0.55)}`,
                      bgcolor: alpha(theme.palette.background.paper, 0.55),
                      transition: "all 0.2s ease",
                      "&:hover": {
                        borderColor: alpha(theme.palette.info.main, 0.45),
                        boxShadow: `0 8px 28px ${alpha(theme.palette.info.main, 0.12)}`,
                      },
                    }}
                    onClick={() => {
                      setImportType("text");
                      setImportSourcePath("");
                      setShowImportDialog(true);
                    }}
                  >
                    <CardContent sx={{ py: 2, "&:last-child": { pb: 2 } }}>
                      <TextIcon sx={{ mb: 1, color: "info.main" }} />
                      <Typography variant="subtitle2" fontWeight={700}>
                        导入文本
                      </Typography>
                      <Typography variant="caption" color="text.secondary">
                        粘贴正文
                      </Typography>
                    </CardContent>
                  </Card>
                </Grid>
              </Grid>
            </Box>

            {memory.importResult && (
              <Paper elevation={0} sx={{ p: 2, ...glassSurface }}>
                <Typography variant="subtitle2" fontWeight={700} gutterBottom>
                  上次导入结果
                </Typography>
                <Stack spacing={0.5}>
                  <Typography variant="body2">
                    成功导入: {memory.importResult.imported_count} 个页面
                  </Typography>
                  <Typography variant="body2">
                    跳过: {memory.importResult.skipped_count} 个文件
                  </Typography>
                  {memory.importResult.created_pages.length > 0 && (
                    <Typography variant="body2">
                      创建页面: {memory.importResult.created_pages.join(", ")}
                    </Typography>
                  )}
                  {memory.importResult.errors.length > 0 && (
                    <Alert severity="error" sx={{ mt: 1, borderRadius: 2 }}>
                      {memory.importResult.errors.join("; ")}
                    </Alert>
                  )}
                </Stack>
              </Paper>
            )}

            <Paper elevation={0} sx={{ p: 2, ...glassSurface }}>
              <Typography variant="subtitle2" fontWeight={700} gutterBottom>
                支持的文件类型
              </Typography>
              <Stack direction="row" spacing={0.5} flexWrap="wrap" useFlexGap>
                {memory.supportedExtensions.map((ext) => (
                  <Chip
                    key={ext}
                    label={`.${ext}`}
                    size="small"
                    variant="outlined"
                    sx={{ fontFamily: "ui-monospace, monospace", fontWeight: 600 }}
                  />
                ))}
              </Stack>
            </Paper>
          </Stack>
        )}

        {/* Implicit Memory */}
        {memory.activeTab === "implicit" && (
          <Stack spacing={2}>
            <Paper elevation={0} sx={{ p: 2.25, ...glassSurface }}>
              <Stack direction="row" alignItems="center" spacing={1} mb={1.5}>
                <SearchIcon sx={{ color: "secondary.main", fontSize: 22 }} />
                <Box>
                  <Typography variant="subtitle2" fontWeight={700}>
                    搜索聊天历史
                  </Typography>
                  <Typography variant="caption" color="text.secondary">
                    在隐性记忆中查找过往对话内容
                  </Typography>
                </Box>
              </Stack>
              <Stack direction={{ xs: "column", sm: "row" }} spacing={1}>
                <TextField
                  size="small"
                  placeholder="输入关键词…"
                  value={memory.searchQuery}
                  onChange={(e) => memory.setSearchQuery(e.target.value)}
                  onKeyDown={(e) => e.key === "Enter" && handleSearch()}
                  fullWidth
                  sx={{
                    "& .MuiOutlinedInput-root": { borderRadius: 2 },
                  }}
                />
                <Button
                  variant="contained"
                  color="secondary"
                  size="medium"
                  startIcon={<SearchIcon />}
                  onClick={handleSearch}
                  disabled={!memory.searchQuery.trim() || memory.loading}
                  sx={{ borderRadius: 2, textTransform: "none", fontWeight: 700, minWidth: { sm: 108 } }}
                >
                  搜索
                </Button>
              </Stack>

              {memory.loading && (
                <LinearProgress
                  sx={{
                    mt: 1.5,
                    height: 3,
                    borderRadius: 1,
                    bgcolor: alpha(theme.palette.secondary.main, 0.12),
                    "& .MuiLinearProgress-bar": { borderRadius: 1 },
                  }}
                />
              )}

              {memory.searchResults && (
                <Box sx={{ mt: 2 }}>
                  <Typography variant="caption" color="text.secondary" fontWeight={600}>
                    找到 {memory.searchResults.total_matches} 条匹配
                  </Typography>
                  <Stack spacing={1} mt={1}>
                    {memory.searchResults.results.map((result, idx) => (
                      <Paper
                        key={idx}
                        elevation={0}
                        sx={{
                          p: 1.5,
                          borderRadius: 2,
                          border: `1px solid ${alpha(theme.palette.divider, 0.55)}`,
                          bgcolor: alpha(theme.palette.background.paper, 0.5),
                          transition: "box-shadow 0.2s ease, border-color 0.2s ease, transform 0.2s ease",
                          "&:hover": {
                            borderColor: alpha(theme.palette.secondary.main, 0.35),
                            boxShadow: `0 6px 20px ${alpha(theme.palette.common.black, 0.06)}`,
                          },
                        }}
                      >
                        <Stack direction="row" justifyContent="space-between" alignItems="flex-start" gap={1}>
                          <Typography variant="caption" fontWeight={700} color="text.primary">
                            {result.title}
                          </Typography>
                          <Chip
                            label={`${result.source_type} · ${result.match_type}`}
                            size="small"
                            sx={{ fontWeight: 600 }}
                          />
                        </Stack>
                        <Typography variant="caption" color="text.secondary" display="block" sx={{ mt: 0.5 }}>
                          {result.path}
                        </Typography>
                        <Typography variant="body2" display="block" sx={{ mt: 0.75, fontSize: "0.8rem", lineHeight: 1.5 }}>
                          {result.excerpt}
                        </Typography>
                      </Paper>
                    ))}
                  </Stack>
                </Box>
              )}
            </Paper>

            <Paper elevation={0} sx={{ p: 2.25, ...glassSurface }}>
              <Typography variant="subtitle2" fontWeight={700} gutterBottom>
                聊天历史索引管理
              </Typography>
              <Typography variant="caption" color="text.secondary" display="block" sx={{ mb: 1.5 }}>
                聊天历史会在每次对话后自动索引。如出现问题可手动重建。
              </Typography>
              <Stack direction="row" spacing={1} flexWrap="wrap" useFlexGap>
                <Button
                  variant="outlined"
                  size="small"
                  startIcon={memory.building ? <CircularProgress size={16} /> : <BuildIcon />}
                  onClick={handleBuild}
                  disabled={memory.building}
                  sx={{ borderRadius: 2, textTransform: "none", fontWeight: 600 }}
                >
                  重建索引
                </Button>
                <Button
                  variant="outlined"
                  size="small"
                  startIcon={memory.building ? <CircularProgress size={16} /> : <UpdateIcon />}
                  onClick={handleUpdate}
                  disabled={memory.building}
                  sx={{ borderRadius: 2, textTransform: "none", fontWeight: 600 }}
                >
                  增量更新
                </Button>
                <Button
                  variant="outlined"
                  color="error"
                  size="small"
                  startIcon={<ClearIcon />}
                  onClick={memory.clearIndex}
                  disabled={memory.loading}
                  sx={{ borderRadius: 2, textTransform: "none", fontWeight: 600 }}
                >
                  清除索引
                </Button>
              </Stack>
            </Paper>
          </Stack>
        )}


        {/* Config */}
        {memory.activeTab === "config" && (
          <Stack spacing={2}>
            <Paper elevation={0} sx={{ p: 2.25, ...glassSurface }}>
              <Typography variant="subtitle2" fontWeight={700} gutterBottom>
                路径配置
              </Typography>
              <Typography variant="caption" color="text.secondary" display="block" sx={{ mb: 1.5 }}>
                根目录在概览卡片中修改；子目录为相对根目录的名称。
              </Typography>
              <Stack spacing={1.25}>
                <TextField
                  size="small"
                  label="记忆根目录"
                  value={memory.config?.root_dir || ""}
                  disabled
                  helperText='在「记忆概览」中点击「修改路径」'
                  sx={{ "& .MuiOutlinedInput-root": { borderRadius: 2 } }}
                />
                <TextField
                  size="small"
                  label="显性记忆子目录"
                  value={memory.config?.wiki_dir || "wiki"}
                  disabled
                  sx={{ "& .MuiOutlinedInput-root": { borderRadius: 2 } }}
                />
                <TextField
                  size="small"
                  label="隐性记忆子目录"
                  value={memory.config?.implicit_dir || "implicit"}
                  disabled
                  sx={{ "& .MuiOutlinedInput-root": { borderRadius: 2 } }}
                />
                <TextField
                  size="small"
                  label="原始文件存储目录"
                  value={rawDirInput || memory.config?.raw_dir || ""}
                  onChange={(e) => setRawDirInput(e.target.value)}
                  placeholder={memory.config?.raw_dir || "~/.omiga/memory/raw"}
                  helperText="导入到知识库时原始文件的备份目录（绝对路径）。留空使用默认值 ~/.omiga/memory/raw"
                  InputProps={{
                    endAdornment: (
                      <Button
                        size="small"
                        sx={{ ml: 0.5, whiteSpace: "nowrap", textTransform: "none" }}
                        onClick={async () => {
                          await memory.updateConfig({ raw_dir: rawDirInput });
                          setRawDirInput("");
                        }}
                        disabled={!rawDirInput}
                      >
                        保存
                      </Button>
                    ),
                  }}
                  sx={{ "& .MuiOutlinedInput-root": { borderRadius: 2 } }}
                />
              </Stack>
            </Paper>

            <Paper elevation={0} sx={{ p: 2.25, ...glassSurface }}>
              <Typography variant="subtitle2" fontWeight={700} gutterBottom>
                索引设置
              </Typography>
              <FormControlLabel
                sx={{ mt: 0.5 }}
                control={
                  <Switch
                    checked={memory.config?.auto_build_index ?? true}
                    onChange={(e) =>
                      memory.updateConfig({ auto_build_index: e.target.checked })
                    }
                  />
                }
                label={<Typography fontWeight={600}>启用自动索引</Typography>}
              />
              <TextField
                size="small"
                label="最大文件大小 (MB)"
                type="number"
                value={Math.floor((memory.config?.max_file_size || 10485760) / 1048576)}
                onChange={(e) =>
                  memory.updateConfig({
                    max_file_size: parseInt(e.target.value, 10) * 1048576,
                  })
                }
                sx={{ mt: 1.5, maxWidth: 280, "& .MuiOutlinedInput-root": { borderRadius: 2 } }}
              />
            </Paper>
          </Stack>
        )}
      </Box>

      {/* Path Dialog */}
      <Dialog
        open={showPathDialog}
        onClose={() => setShowPathDialog(false)}
        maxWidth="sm"
        fullWidth
        PaperProps={{
          elevation: 0,
          sx: {
            ...glassSurface,
            borderRadius: 3,
            border: `1px solid ${alpha(theme.palette.divider, 0.45)}`,
          },
        }}
      >
        <DialogTitle sx={{ pb: 1 }}>
          <Stack direction="row" alignItems="center" spacing={1.25}>
            <Box
              sx={{
                width: 40,
                height: 40,
                borderRadius: 1.5,
                display: "flex",
                alignItems: "center",
                justifyContent: "center",
                bgcolor: alpha(theme.palette.primary.main, 0.12),
                color: "primary.main",
              }}
            >
              <FolderIcon />
            </Box>
            <Box>
              <Typography variant="h6" component="span" fontWeight={800}>
                修改记忆路径
              </Typography>
              <Typography variant="caption" color="text.secondary" display="block">
                相对项目根或绝对路径
              </Typography>
            </Box>
          </Stack>
        </DialogTitle>
        <DialogContent>
          <Typography variant="body2" color="text.secondary" sx={{ mb: 2 }}>
            支持相对路径（如 .omiga/memory）或绝对路径（如 ~/.my-memory）。
            路径不能包含 ".." 或指向系统目录。
          </Typography>
          <TextField
            fullWidth
            label="新路径"
            value={newPath}
            onChange={(e) => setNewPath(e.target.value)}
            error={!memory.isValidPath(newPath)}
            helperText={
              !memory.isValidPath(newPath)
                ? "路径无效"
                : "相对路径基于项目根目录"
            }
          />
        </DialogContent>
        <DialogActions sx={{ px: 3, pb: 2, gap: 1 }}>
          <Button onClick={() => setShowPathDialog(false)} sx={{ borderRadius: 2, textTransform: "none" }}>
            取消
          </Button>
          <Button
            variant="contained"
            onClick={handleSavePath}
            disabled={!memory.isValidPath(newPath)}
            sx={{ borderRadius: 2, textTransform: "none", fontWeight: 700 }}
          >
            保存
          </Button>
        </DialogActions>
      </Dialog>

      {/* Import Dialog */}
      <Dialog
        open={showImportDialog}
        onClose={() => setShowImportDialog(false)}
        maxWidth="sm"
        fullWidth
        PaperProps={{
          elevation: 0,
          sx: {
            ...glassSurface,
            borderRadius: 3,
            border: `1px solid ${alpha(theme.palette.divider, 0.45)}`,
          },
        }}
      >
        <DialogTitle sx={{ pb: 1 }}>
          <Stack direction="row" alignItems="center" spacing={1.25}>
            <Box
              sx={{
                width: 40,
                height: 40,
                borderRadius: 1.5,
                display: "flex",
                alignItems: "center",
                justifyContent: "center",
                bgcolor: alpha(theme.palette.primary.main, 0.12),
                color: "primary.main",
              }}
            >
              <UploadIcon />
            </Box>
            <Box>
              <Typography variant="h6" component="span" fontWeight={800}>
                {importType === "file" && "导入文件到知识库"}
                {importType === "directory" && "导入文件夹到知识库"}
                {importType === "text" && "导入文本到知识库"}
              </Typography>
              <Typography variant="caption" color="text.secondary" display="block">
                全局知识库 · ~/.omiga/memory/permanent/wiki
              </Typography>
            </Box>
          </Stack>
        </DialogTitle>
        <DialogContent>
          <Stack spacing={2} sx={{ mt: 1 }}>
            {(importType === "file" || importType === "directory") && (
              <Box>
                <Typography variant="body2" fontWeight={600} gutterBottom>
                  {importType === "file" ? "选择文件" : "选择文件夹"}
                </Typography>
                <Stack direction="row" spacing={1} alignItems="center">
                  <Button
                    variant="outlined"
                    startIcon={<FolderIcon />}
                    onClick={async () => {
                      try {
                        const selected = await openFileDialog({
                          directory: importType === "directory",
                          multiple: false,
                          title: importType === "file" ? "选择要导入的文件" : "选择要导入的文件夹",
                        });
                        if (selected == null) return;
                        const path = Array.isArray(selected) ? selected[0] : selected;
                        if (path) setImportSourcePath(path);
                      } catch (e) {
                        console.error("[UnifiedMemoryTab] file dialog failed", e);
                      }
                    }}
                    sx={{ borderRadius: 2, textTransform: "none", fontWeight: 600, flexShrink: 0 }}
                  >
                    {importType === "file" ? "浏览文件…" : "浏览文件夹…"}
                  </Button>
                  {importSourcePath && (
                    <Typography
                      variant="body2"
                      color="text.secondary"
                      noWrap
                      sx={{
                        fontFamily: "ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace",
                        flex: 1,
                        minWidth: 0,
                      }}
                      title={importSourcePath}
                    >
                      {importSourcePath}
                    </Typography>
                  )}
                  {!importSourcePath && (
                    <Typography variant="body2" color="text.disabled" sx={{ fontStyle: "italic" }}>
                      未选择
                    </Typography>
                  )}
                </Stack>
              </Box>
            )}
            
            {importType === "text" && (
              <>
                <TextField
                  fullWidth
                  label="标题"
                  value={importTextTitle}
                  onChange={(e) => setImportTextTitle(e.target.value)}
                  placeholder="My Notes"
                />
                <TextField
                  fullWidth
                  label="内容"
                  multiline
                  rows={6}
                  value={importTextContent}
                  onChange={(e) => setImportTextContent(e.target.value)}
                  placeholder="输入要导入的文本内容..."
                />
              </>
            )}
            
            <TextField
              fullWidth
              label="标签（可选，用逗号分隔）"
              value={importTags}
              onChange={(e) => setImportTags(e.target.value)}
              placeholder="important, reference, todo"
            />
            
            <Typography variant="caption" color="text.secondary">
              使用 PageIndex 算法自动解析文档结构，提取标题和章节层次。
              <br />
              支持格式：Markdown、PDF、TXT、HTML、JSON、YAML、TOML
            </Typography>
          </Stack>
        </DialogContent>
        <DialogActions sx={{ px: 3, pb: 2, gap: 1 }}>
          <Button onClick={() => setShowImportDialog(false)} sx={{ borderRadius: 2, textTransform: "none" }}>
            取消
          </Button>
          <Button
            variant="contained"
            onClick={handleImport}
            disabled={
              memory.importing ||
              (importType === "file" && !importSourcePath) ||
              (importType === "directory" && !importSourcePath) ||
              (importType === "text" && (!importTextTitle || !importTextContent))
            }
            startIcon={memory.importing ? <CircularProgress size={16} /> : <UploadIcon />}
            sx={{ borderRadius: 2, textTransform: "none", fontWeight: 700 }}
          >
            导入到知识库
          </Button>
        </DialogActions>
      </Dialog>

      {memory.error && (
        <Alert
          severity="error"
          sx={{
            borderRadius: 2.5,
            border: `1px solid ${alpha(theme.palette.error.main, 0.35)}`,
            bgcolor: alpha(theme.palette.error.main, 0.06),
          }}
        >
          {memory.error}
        </Alert>
      )}

      <Snackbar
        open={!!toast}
        autoHideDuration={4000}
        onClose={() => setToast(null)}
        message={toast}
        anchorOrigin={{ vertical: "bottom", horizontal: "center" }}
      />
    </Box>
  );
}
