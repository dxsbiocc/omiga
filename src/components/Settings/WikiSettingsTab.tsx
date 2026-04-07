/**
 * WikiSettingsTab — Settings panel for the Omiga Wiki Agent feature.
 *
 * Dispatches wiki agent prompts via window event "wikiSendMessage" so that
 * the active Chat session picks them up without needing prop drilling.
 */

import { useState } from "react";
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
} from "@mui/material";
import {
  MenuBook as WikiIcon,
  Search as SearchIcon,
  ContentPaste as IngestIcon,
  HealthAndSafety as LintIcon,
  Refresh as RefreshIcon,
  FolderOpen as FolderIcon,
  CheckCircle as OkIcon,
  Cancel as MissingIcon,
  AutoAwesome as AgentIcon,
} from "@mui/icons-material";
import { useWikiHook, WikiPageExcerpt } from "../../hooks/useWikiHook";

interface WikiSettingsTabProps {
  projectPath: string;
}

/** Dispatch a prompt to the active chat session via window event. */
function dispatchToChatSession(content: string) {
  window.dispatchEvent(new CustomEvent("wikiSendMessage", { detail: { content } }));
}

export function WikiSettingsTab({ projectPath }: WikiSettingsTabProps) {
  const wiki = useWikiHook(projectPath);
  const [searchQuery, setSearchQuery] = useState("");
  const [ingestText, setIngestText] = useState("");
  const [ingestTitle, setIngestTitle] = useState("");
  const [activeSection, setActiveSection] = useState<
    "overview" | "search" | "ingest" | "lint"
  >("overview");
  const [toast, setToast] = useState<string | null>(null);
  const [savingPage, setSavingPage] = useState(false);

  const handleSearch = async () => {
    if (searchQuery.trim()) {
      await wiki.search(searchQuery.trim());
    }
  };

  /** Direct ingest: write content as a raw wiki page (no LLM, instant). */
  const handleDirectIngest = async () => {
    if (!ingestText.trim()) return;
    setSavingPage(true);
    try {
      const slug = ingestTitle.trim()
        ? ingestTitle.trim().toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "")
        : `ingest-${Date.now()}`;
      const pageContent = [
        `# ${ingestTitle.trim() || slug}`,
        "",
        `> 摄入于 ${new Date().toLocaleString("zh-CN")}`,
        "",
        ingestText.trim(),
      ].join("\n");
      await wiki.writePage(slug, pageContent);
      // Update index.md with a new entry
      const existing = (await wiki.readIndex()) ?? "# Wiki Index\n\n";
      const entry = `- [${ingestTitle.trim() || slug}](${slug}.md) — ${ingestText.trim().slice(0, 80).replace(/\n/g, " ")}...\n`;
      if (!existing.includes(`(${slug}.md)`)) {
        await wiki.writeIndex(existing.trimEnd() + "\n" + entry);
      }
      await wiki.appendLog(`ingest: created page "${slug}"`);
      await wiki.refresh();
      setIngestText("");
      setIngestTitle("");
      setToast(`已保存页面 "${slug}"（直接写入，未经 AI 提炼）`);
    } catch (e) {
      setToast(`写入失败: ${String(e)}`);
    } finally {
      setSavingPage(false);
    }
  };

  /** AI ingest: dispatch to wiki-agent via chat session. */
  const handleAgentIngest = () => {
    if (!ingestText.trim()) return;
    dispatchToChatSession(wiki.buildIngestPrompt(ingestText, ingestTitle || undefined));
    setIngestText("");
    setIngestTitle("");
    setToast("已发送到 Wiki Agent，请切换到对话窗口查看进度");
  };

  return (
    <Box sx={{ display: "flex", flexDirection: "column", gap: 2 }}>
      {/* Header */}
      <Alert severity="info" icon={<WikiIcon />} sx={{ borderRadius: 2 }}>
        <Typography variant="body2">
          Wiki Agent 基于{" "}
          <Typography component="span" fontWeight={600}>
            Karpathy LLM Wiki 模式
          </Typography>
          ，将项目知识持久化为 Markdown 文件，存储于{" "}
          <Typography component="span" fontWeight={600}>
            .omiga/wiki/
          </Typography>
          。系统会在每次对话前自动注入相关 Wiki 上下文（透明 Hook）。
        </Typography>
      </Alert>

      {/* Wiki Status */}
      <Paper variant="outlined" sx={{ p: 2, borderRadius: 2 }}>
        <Stack direction="row" alignItems="center" justifyContent="space-between" mb={1}>
          <Typography variant="subtitle2" fontWeight={600}>
            Wiki 状态
          </Typography>
          <Tooltip title="刷新">
            <Button
              size="small"
              startIcon={
                wiki.loading ? <CircularProgress size={14} /> : <RefreshIcon />
              }
              onClick={wiki.refresh}
              disabled={wiki.loading}
            >
              刷新
            </Button>
          </Tooltip>
        </Stack>

        {wiki.status ? (
          <Stack spacing={1}>
            <Stack direction="row" spacing={1} alignItems="center">
              {wiki.status.exists ? (
                <OkIcon color="success" fontSize="small" />
              ) : (
                <MissingIcon color="disabled" fontSize="small" />
              )}
              <Typography variant="body2">
                {wiki.status.exists ? "Wiki 已初始化" : "Wiki 尚未创建"}
              </Typography>
              {wiki.status.exists && (
                <Chip
                  label={`${wiki.status.page_count} 页`}
                  size="small"
                  color="primary"
                  variant="outlined"
                />
              )}
            </Stack>

            <Stack direction="row" spacing={1} alignItems="center">
              <FolderIcon fontSize="small" color="action" />
              <Typography
                variant="caption"
                color="text.secondary"
                sx={{ fontFamily: "monospace", wordBreak: "break-all" }}
              >
                {wiki.status.wiki_dir}
              </Typography>
            </Stack>

            {wiki.status.last_log_entry && (
              <Typography variant="caption" color="text.secondary">
                最近操作：{wiki.status.last_log_entry}
              </Typography>
            )}

            {wiki.status.index_summary && (
              <Paper
                variant="outlined"
                sx={{
                  p: 1,
                  mt: 0.5,
                  bgcolor: "action.hover",
                  borderRadius: 1,
                  maxHeight: 100,
                  overflow: "auto",
                }}
              >
                <Typography
                  variant="caption"
                  component="pre"
                  sx={{ fontFamily: "monospace", m: 0, whiteSpace: "pre-wrap" }}
                >
                  {wiki.status.index_summary}
                </Typography>
              </Paper>
            )}
          </Stack>
        ) : (
          <Typography variant="body2" color="text.secondary">
            加载中...
          </Typography>
        )}
      </Paper>

      {/* Operation Tabs */}
      <Stack direction="row" spacing={1}>
        {(
          [
            { key: "overview", label: "概览" },
            { key: "search", label: "搜索" },
            { key: "ingest", label: "摄入" },
            { key: "lint", label: "健检" },
          ] as const
        ).map((tab) => (
          <Button
            key={tab.key}
            size="small"
            variant={activeSection === tab.key ? "contained" : "outlined"}
            onClick={() => setActiveSection(tab.key)}
            sx={{ borderRadius: 2 }}
          >
            {tab.label}
          </Button>
        ))}
      </Stack>

      <Divider />

      {/* Overview section */}
      {activeSection === "overview" && (
        <Box>
          <Typography variant="body2" color="text.secondary" gutterBottom>
            透明 Hook 工作原理：
          </Typography>
          <Box
            component="ol"
            sx={{ pl: 2, m: 0, "& li": { mb: 0.5 }, typography: "body2" }}
          >
            <li>每次发送消息前，系统自动读取 wiki/index.md</li>
            <li>关键词匹配提取最多 3 个相关页面的摘要（≤400 字符）</li>
            <li>将摘要注入到 LLM 系统提示的「Project Knowledge Base」段落</li>
            <li>无 Wiki 或无匹配时零开销，不注入任何内容</li>
          </Box>
          <Typography variant="body2" color="text.secondary" sx={{ mt: 1.5, mb: 0.5 }}>
            Wiki Agent（subagent_type: &quot;wiki-agent&quot;）具备以下能力：
          </Typography>
          <Box
            component="ul"
            sx={{ pl: 2, m: 0, "& li": { mb: 0.5 }, typography: "body2" }}
          >
            <li>
              <strong>Ingest</strong>：从文章/代码/文档提取知识，创建/更新页面
            </li>
            <li>
              <strong>Query</strong>：搜索 Wiki 并综合回答，结果可成为新页面
            </li>
            <li>
              <strong>Lint</strong>：检查矛盾、过时声明、孤立页面、缺失引用
            </li>
          </Box>
          <Typography variant="caption" color="text.secondary" sx={{ mt: 1, display: "block" }}>
            也可直接在对话中使用：
            <code style={{ marginLeft: 4 }}>
              Agent(subagent_type: &quot;wiki-agent&quot;, prompt: &quot;ingest ...&quot;)
            </code>
          </Typography>
        </Box>
      )}

      {/* Search section */}
      {activeSection === "search" && (
        <Box>
          <Stack direction="row" spacing={1}>
            <TextField
              size="small"
              placeholder="输入关键词搜索 Wiki..."
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && handleSearch()}
              fullWidth
              sx={{ "& .MuiInputBase-root": { borderRadius: 2 } }}
            />
            <Button
              variant="contained"
              size="small"
              startIcon={<SearchIcon />}
              onClick={handleSearch}
              disabled={!searchQuery.trim() || wiki.operation === "query"}
              sx={{ whiteSpace: "nowrap", borderRadius: 2 }}
            >
              搜索
            </Button>
          </Stack>

          {wiki.operation === "query" && (
            <Box sx={{ mt: 2, textAlign: "center" }}>
              <CircularProgress size={24} />
            </Box>
          )}

          {wiki.lastQueryResult && (
            <Box sx={{ mt: 2 }}>
              <Typography variant="caption" color="text.secondary">
                找到 {wiki.lastQueryResult.matched_slugs.length} 个匹配页面
              </Typography>
              <Stack spacing={1.5} mt={1}>
                {wiki.lastQueryResult.excerpts.map((ex: WikiPageExcerpt) => (
                  <Paper
                    key={ex.slug}
                    variant="outlined"
                    sx={{ p: 1.5, borderRadius: 2 }}
                  >
                    <Typography
                      variant="caption"
                      fontWeight={600}
                      sx={{ fontFamily: "monospace" }}
                    >
                      {ex.slug}
                    </Typography>
                    <Typography
                      variant="caption"
                      display="block"
                      color="text.secondary"
                      sx={{ mt: 0.5, whiteSpace: "pre-wrap" }}
                    >
                      {ex.excerpt}
                    </Typography>
                  </Paper>
                ))}
                {wiki.lastQueryResult.matched_slugs.length === 0 && (
                  <Typography variant="body2" color="text.secondary">
                    未找到相关页面
                  </Typography>
                )}
              </Stack>

              {wiki.lastQueryResult.matched_slugs.length > 0 && (
                <Button
                  size="small"
                  variant="outlined"
                  startIcon={<AgentIcon />}
                  sx={{ mt: 1.5, borderRadius: 2 }}
                  onClick={() =>
                    dispatchToChatSession(wiki.buildQueryPrompt(searchQuery))
                  }
                >
                  通过 Wiki Agent 深度回答此问题
                </Button>
              )}
            </Box>
          )}
        </Box>
      )}

      {/* Ingest section */}
      {activeSection === "ingest" && (
        <Box>
          <Typography variant="body2" color="text.secondary" gutterBottom>
            将外部内容摄入到 Wiki。
          </Typography>
          <Stack spacing={1.5}>
            <TextField
              size="small"
              label="来源标题（可选）"
              value={ingestTitle}
              onChange={(e) => setIngestTitle(e.target.value)}
              fullWidth
              sx={{ "& .MuiInputBase-root": { borderRadius: 2 } }}
            />
            <TextField
              size="small"
              label="摄入内容"
              multiline
              minRows={4}
              maxRows={10}
              value={ingestText}
              onChange={(e) => setIngestText(e.target.value)}
              placeholder="粘贴文章、代码注释、文档或任何需要摄入知识库的内容..."
              fullWidth
              sx={{ "& .MuiInputBase-root": { borderRadius: 2 } }}
            />
            <Stack direction="row" spacing={1}>
              <Tooltip title="直接将内容作为 Markdown 页面保存，无需 AI 处理">
                <Button
                  variant="outlined"
                  startIcon={
                    savingPage ? <CircularProgress size={16} /> : <IngestIcon />
                  }
                  disabled={!ingestText.trim() || savingPage}
                  onClick={handleDirectIngest}
                  sx={{ borderRadius: 2 }}
                >
                  直接保存
                </Button>
              </Tooltip>
              <Tooltip title="发送给 Wiki Agent，由 AI 智能提炼知识点后写入 Wiki">
                <Button
                  variant="contained"
                  startIcon={<AgentIcon />}
                  disabled={!ingestText.trim()}
                  onClick={handleAgentIngest}
                  sx={{ borderRadius: 2 }}
                >
                  AI 智能摄入
                </Button>
              </Tooltip>
            </Stack>
            <Typography variant="caption" color="text.secondary">
              「直接保存」立即写入；「AI 智能摄入」将提示发送到对话，由 wiki-agent 提炼知识点。
            </Typography>
          </Stack>
        </Box>
      )}

      {/* Lint section */}
      {activeSection === "lint" && (
        <Box>
          <Typography variant="body2" color="text.secondary" gutterBottom>
            对 Wiki 进行健康检查：发现矛盾、过时声明、孤立页面和缺失引用。
          </Typography>
          <Button
            variant="outlined"
            startIcon={<LintIcon />}
            disabled={!wiki.status?.exists}
            onClick={() => dispatchToChatSession(wiki.buildLintPrompt())}
            sx={{ borderRadius: 2 }}
          >
            运行 Wiki 健检
          </Button>
          {!wiki.status?.exists && (
            <Typography variant="caption" color="text.secondary" display="block" mt={1}>
              Wiki 尚未创建。先摄入一些内容。
            </Typography>
          )}
        </Box>
      )}

      {wiki.error && (
        <Alert severity="error" sx={{ borderRadius: 2 }}>
          {wiki.error}
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
