import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  Alert,
  Avatar,
  Box,
  Button,
  Card,
  CardContent,
  Chip,
  CircularProgress,
  Collapse,
  Divider,
  Dialog,
  DialogActions,
  DialogContent,
  DialogTitle,
  FormControlLabel,
  IconButton,
  List,
  ListItem,
  ListItemText,
  Stack,
  Switch,
  Tab,
  Tabs,
  TextField,
  ToggleButton,
  ToggleButtonGroup,
  Tooltip,
  Typography,
} from "@mui/material";
import { alpha } from "@mui/material/styles";
import type { Theme } from "@mui/material/styles";
import AddIcon from "@mui/icons-material/Add";
import EditOutlinedIcon from "@mui/icons-material/EditOutlined";
import ExpandMoreIcon from "@mui/icons-material/ExpandMore";
import RefreshIcon from "@mui/icons-material/Refresh";
import DeleteOutlineIcon from "@mui/icons-material/DeleteOutline";
import { SkillPreviewDialog } from "./SkillPreviewDialog";
import { extractErrorMessage } from "../../utils/errorMessage";

type McpToolCatalogEntry = {
  wireName: string;
  description: string;
};

type McpServerConfigCatalogEntry = {
  kind: McpProtocol;
  command: string | null;
  args: string[];
  env: Record<string, string>;
  headers: Record<string, string>;
  url: string | null;
  cwd: string | null;
};

type McpServerCatalogEntry = {
  configKey: string;
  normalizedKey: string;
  enabled: boolean;
  config: McpServerConfigCatalogEntry;
  toolListChecked: boolean;
  oauthAuthenticated: boolean;
  listToolsError: string | null;
  tools: McpToolCatalogEntry[];
};

type SkillSource = "claudeUser" | "omigaUser" | "omigaProject" | "omigaPlugin";

const SKILL_SOURCE_LABEL: Record<SkillSource, string> = {
  claudeUser: "Claude ~/.claude",
  omigaUser: "用户 ~/.omiga",
  omigaProject: "项目 .omiga",
  omigaPlugin: "插件",
};

type SkillCatalogEntry = {
  name: string;
  description: string;
  enabled: boolean;
  source: SkillSource;
  directoryName: string;
  skillMdPath: string;
  /** YAML frontmatter `tags` */
  tags: string[];
  canUninstallOmigaCopy: boolean;
};

type IntegrationsCatalog = {
  mcpServers: McpServerCatalogEntry[];
  skills: SkillCatalogEntry[];
};

const integrationsCatalogMemoryCache = new Map<string, IntegrationsCatalog>();

type PanelMode = "mcp" | "skills" | "both";

type McpProtocol = "stdio" | "http";

type McpServerFormState = {
  name: string;
  kind: McpProtocol;
  command: string;
  argsText: string;
  envText: string;
  bearerEnvName: string;
  headersText: string;
  url: string;
  cwd: string;
};

type ProjectMcpServerInput = {
  name: string;
  kind: McpProtocol;
  command?: string;
  args?: string[];
  env?: Record<string, string>;
  headers?: Record<string, string>;
  url?: string;
  cwd?: string;
};

type ImportMcpMergeResult = {
  wrotePath: string;
  serverCount: number;
};

type VerifyMcpServerResult = {
  configKey: string;
  normalizedKey: string;
  ok: boolean;
  toolListChecked: boolean;
  oauthAuthenticated: boolean;
  listToolsError: string | null;
  tools: McpToolCatalogEntry[];
};

type StartMcpOAuthLoginResult = {
  configKey: string;
  normalizedKey: string;
  loginSessionId: string;
  authorizationUrl: string;
  expiresIn: number;
  intervalSecs: number;
  expiresAt: string;
  message: string;
};

type McpOAuthPollStatus =
  | "pending"
  | "complete"
  | "expired"
  | "denied"
  | "error";

type PollMcpOAuthLoginResult = {
  configKey: string;
  normalizedKey: string;
  status: McpOAuthPollStatus;
  message: string;
  intervalSecs: number;
  ok: boolean;
  toolListChecked: boolean;
  oauthAuthenticated: boolean;
  listToolsError: string | null;
  tools: McpToolCatalogEntry[];
};

type LogoutMcpOAuthServerResult = {
  configKey: string;
  normalizedKey: string;
};

type McpOAuthFlowStatus = "opening" | "waiting" | "exchanging" | "verifying";

type McpOAuthFlow = {
  loginSessionId: string;
  status: McpOAuthFlowStatus;
  intervalSecs: number;
  message: string;
};

function emptyMcpServerForm(): McpServerFormState {
  return {
    name: "",
    kind: "stdio",
    command: "",
    argsText: "",
    envText: "",
    bearerEnvName: "",
    headersText: "",
    url: "",
    cwd: "",
  };
}

function authorizationBearerEnvName(headers: Record<string, string>): string {
  const authorization = Object.entries(headers).find(([key]) =>
    key.toLowerCase() === "authorization",
  )?.[1];
  const match = authorization
    ?.trim()
    .match(/^Bearer\s+\$\{([A-Za-z_][A-Za-z0-9_]*)\}$/i);
  return match?.[1] ?? "";
}

function mcpServerFormFromCatalogEntry(
  srv: McpServerCatalogEntry,
): McpServerFormState {
  const envText = Object.entries(srv.config.env ?? {})
    .map(([key, value]) => `${key}=${value}`)
    .join("\n");
  const bearerEnvName = authorizationBearerEnvName(srv.config.headers ?? {});
  const headersText = Object.entries(srv.config.headers ?? {})
    .filter(
      ([key]) => !(bearerEnvName && key.toLowerCase() === "authorization"),
    )
    .map(([key, value]) => `${key}=${value}`)
    .join("\n");
  return {
    name: srv.configKey,
    kind: srv.config.kind === "http" ? "http" : "stdio",
    command: srv.config.command ?? "",
    argsText: (srv.config.args ?? []).join("\n"),
    envText,
    bearerEnvName,
    headersText,
    url: srv.config.url ?? "",
    cwd: srv.config.cwd ?? "",
  };
}

function splitMultilineValues(raw: string): string[] {
  return raw
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean);
}

function parseKeyValueLines(raw: string, label: string): Record<string, string> {
  const values: Record<string, string> = {};
  for (const line of splitMultilineValues(raw)) {
    const idx = line.indexOf("=");
    if (idx <= 0) {
      throw new Error(`${label}必须使用 KEY=value 格式：${line}`);
    }
    const key = line.slice(0, idx).trim();
    const value = line.slice(idx + 1);
    if (!key) {
      throw new Error(`${label}名称不能为空。`);
    }
    values[key] = value;
  }
  return values;
}

function buildProjectMcpServerInput(
  form: McpServerFormState,
): ProjectMcpServerInput {
  const name = form.name.trim();
  if (!name) {
    throw new Error("请填写 MCP 服务名称。");
  }

  if (form.kind === "http") {
    const url = form.url.trim();
    if (!/^https?:\/\//i.test(url)) {
      throw new Error("HTTP MCP 地址必须以 http:// 或 https:// 开头。");
    }
    const headers = parseKeyValueLines(form.headersText, "请求头");
    const bearerEnvName = form.bearerEnvName.trim();
    if (bearerEnvName) {
      if (!/^[A-Za-z_][A-Za-z0-9_]*$/.test(bearerEnvName)) {
        throw new Error("Bearer 令牌环境变量必须是有效的环境变量名，例如 PAPERCLIP_TOKEN。");
      }
      if (
        Object.keys(headers).some((key) => key.toLowerCase() === "authorization")
      ) {
        throw new Error("已填写 Bearer 令牌环境变量时，请不要在额外标头中重复 Authorization。");
      }
      headers.Authorization = "Bearer ${" + bearerEnvName + "}";
    }
    return {
      name,
      kind: "http",
      url,
      headers,
    };
  }

  const command = form.command.trim();
  if (!command) {
    throw new Error("STDIO MCP 需要启动命令。");
  }
  const cwd = form.cwd.trim();
  return {
    name,
    kind: "stdio",
    command,
    args: splitMultilineValues(form.argsText),
    env: parseKeyValueLines(form.envText, "环境变量"),
    cwd: cwd || undefined,
  };
}

function resolveProjectPath(raw: string): string {
  const t = raw.trim();
  return t.length > 0 ? t : ".";
}

function isSkillSource(s: string): s is SkillSource {
  return s === "claudeUser" || s === "omigaUser" || s === "omigaProject" || s === "omigaPlugin";
}

type SkillFilterTab = "all" | "user" | "project";

function normalizeSkillSource(sk: SkillCatalogEntry): SkillSource {
  return isSkillSource(sk.source) ? sk.source : "omigaProject";
}

function skillMatchesFilter(
  sk: SkillCatalogEntry,
  tab: SkillFilterTab,
): boolean {
  const src = normalizeSkillSource(sk);
  if (tab === "all") return true;
  if (tab === "user") return src === "claudeUser" || src === "omigaUser";
  return src === "omigaProject" || src === "omigaPlugin";
}

function mcpInitialLetter(name: string): string {
  const c = name.trim().charAt(0);
  return c ? c.toUpperCase() : "?";
}

function mcpRowSubtitle(
  srv: McpServerCatalogEntry,
  options?: {
    pendingVerification?: boolean;
    verifying?: boolean;
    authFlowStatus?: McpOAuthFlowStatus;
  },
): string {
  if (!srv.enabled) return "已禁用";
  if (options?.verifying) return "验证中…";
  if (options?.authFlowStatus === "opening") return "准备 OAuth 授权…";
  if (options?.authFlowStatus === "waiting") return "待验证 · 请在浏览器完成授权";
  if (options?.authFlowStatus === "exchanging") return "交换 token…";
  if (options?.authFlowStatus === "verifying") return "验证工具列表…";
  if (options?.pendingVerification) return "待验证 · 完成登录后点击验证";
  if (srv.listToolsError) return "连接失败 · 展开查看详情";
  if (!srv.toolListChecked && srv.oauthAuthenticated) {
    return "OAuth token 已保存 · 点击验证工具";
  }
  if (!srv.toolListChecked) return "未检测 · 点击验证或刷新";
  if (srv.tools.length === 0) return "未发现可用工具";
  return `${srv.tools.length} 个工具已启用`;
}

function mcpAuthRequired(srv: McpServerCatalogEntry): boolean {
  const err = srv.listToolsError?.toLowerCase() ?? "";
  return (
    err.includes("auth required") ||
    err.includes("401") ||
    err.includes("403") ||
    err.includes("unauthorized") ||
    err.includes("forbidden") ||
    err.includes("oauth") ||
    err.includes("login")
  );
}

type McpErrorAdvice = {
  title: string;
  detail: string;
  actions: string[];
};

function mcpErrorAdvice(srv: McpServerCatalogEntry): McpErrorAdvice | null {
  const raw = srv.listToolsError?.trim();
  if (!raw) return null;

  const err = raw.toLowerCase();
  const isPaperclip = srv.configKey.toLowerCase() === "paperclip";
  const endpoint = srv.config.kind === "http" ? srv.config.url : srv.config.command;
  const proxyRelated =
    err.includes("proxy") ||
    err.includes("127.0.0.1") ||
    err.includes("localhost") ||
    err.includes("connection refused");
  const authRelated =
    err.includes("401") ||
    err.includes("403") ||
    err.includes("unauthorized") ||
    err.includes("forbidden") ||
    err.includes("oauth") ||
    err.includes("auth") ||
    err.includes("login");
  const dnsRelated =
    err.includes("dns") ||
    err.includes("resolve") ||
    err.includes("could not resolve") ||
    err.includes("name or service not known");
  const timeoutRelated = err.includes("timeout") || err.includes("timed out");
  const stdioRelated =
    srv.config.kind === "stdio" &&
    (err.includes("spawn") ||
      err.includes("no such file") ||
      err.includes("permission denied"));

  if (proxyRelated) {
    return {
      title: "可能是本地代理不可用",
      detail:
        "系统代理环境变量指向本机端口，但该代理没有响应。Omiga 会自动重试直连；如果仍失败，需要修复代理或网络。",
      actions: [
        "确认代理客户端已启动，或清理 http_proxy / https_proxy / all_proxy 环境变量。",
        "点击右上角刷新，重新执行 MCP tools/list 检测。",
        ...(isPaperclip
          ? ["Paperclip 是远程 HTTP MCP；代理修复后仍失败时，再检查 Paperclip 登录/授权状态。"]
          : []),
      ],
    };
  }

  if (dnsRelated || timeoutRelated) {
    return {
      title: dnsRelated ? "DNS 或网络不可达" : "连接超时",
      detail: `Omiga 无法在超时时间内连接到 ${endpoint ?? "该 MCP 服务"}。`,
      actions: [
        "确认当前网络能访问该域名或本地服务。",
        "如果使用代理，请确认代理可用且不会拦截 MCP POST/SSE 流式请求。",
        "点击刷新重试；远程服务偶发慢启动时可能需要再次检测。",
      ],
    };
  }

  if (authRelated || isPaperclip) {
    return {
      title: isPaperclip ? "Paperclip 可能需要登录或授权" : "MCP 服务可能需要认证",
      detail: `当前端点：${endpoint ?? "未配置"}。网络可达但服务端拒绝握手时，通常需要先完成登录、OAuth 或配置访问令牌。`,
      actions: [
        isPaperclip
          ? "点击“连接”，由 Omiga 发起 MCP OAuth 授权并自动交换 token；不要只打开普通网页登录。"
          : "点击“连接”尝试 MCP OAuth；如果服务不支持 OAuth discovery，再按服务文档配置 token 或 header。",
        "如果服务提供 Bearer token，可编辑该服务并填写“Bearer 令牌环境变量”；API key 则放到额外请求头。",
        "确认 MCP 地址是 Streamable HTTP 的 /mcp 端点，而不是普通网页地址。",
      ],
    };
  }

  if (stdioRelated) {
    return {
      title: "本地 STDIO 服务启动失败",
      detail: "启动命令、参数、工作目录或权限可能不正确。",
      actions: [
        "检查命令是否存在，并确认可在终端中直接运行。",
        "检查工作目录是否存在；相对路径会按当前项目解析。",
        "如果依赖包未安装，请用该项目允许的包管理器安装后再刷新。",
      ],
    };
  }

  return {
    title: "MCP 握手失败",
    detail: "服务返回了非预期错误，展开的原始错误可用于进一步诊断。",
    actions: [
      "确认配置的协议、URL/命令和参数正确。",
      "在终端中单独运行该 MCP 服务，查看是否能正常响应 tools/list。",
      "修正后点击刷新重新检测。",
    ],
  };
}

export function IntegrationsCatalogPanel({
  projectPath,
  mode,
}: {
  projectPath: string;
  mode: PanelMode;
}) {
  const root = resolveProjectPath(projectPath);
  const [catalog, setCatalog] = useState<IntegrationsCatalog | null>(
    () => integrationsCatalogMemoryCache.get(root) ?? null,
  );
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [message, setMessage] = useState<{
    kind: "success" | "error";
    text: string;
  } | null>(null);
  const [removingKey, setRemovingKey] = useState<string | null>(null);
  const [skillPreview, setSkillPreview] = useState<SkillCatalogEntry | null>(
    null,
  );
  const [skillFilterTab, setSkillFilterTab] = useState<SkillFilterTab>("all");
  const [expandedMcp, setExpandedMcp] = useState<Record<string, boolean>>({});
  const [addMcpOpen, setAddMcpOpen] = useState(false);
  const [mcpForm, setMcpForm] = useState<McpServerFormState>(
    emptyMcpServerForm,
  );
  const [mcpFormError, setMcpFormError] = useState<string | null>(null);
  const [addingMcp, setAddingMcp] = useState(false);
  const [editingMcpName, setEditingMcpName] = useState<string | null>(null);
  const [deletingMcpKey, setDeletingMcpKey] = useState<string | null>(null);
  const [verifyingMcpKey, setVerifyingMcpKey] = useState<string | null>(null);
  const [loggingOutMcpKey, setLoggingOutMcpKey] = useState<string | null>(null);
  const [pendingMcpVerification, setPendingMcpVerification] = useState<
    Record<string, boolean>
  >({});
  const [mcpOAuthFlows, setMcpOAuthFlows] = useState<Record<string, McpOAuthFlow>>(
    {},
  );
  const isEditingMcp = editingMcpName !== null;
  const noWorkspace = projectPath.trim().length === 0;

  const setCatalogSnapshot = useCallback(
    (
      nextValue:
        | IntegrationsCatalog
        | null
        | ((
            prev: IntegrationsCatalog | null,
          ) => IntegrationsCatalog | null),
    ) => {
      setCatalog((prev) => {
        const next =
          typeof nextValue === "function" ? nextValue(prev) : nextValue;
        if (next) {
          integrationsCatalogMemoryCache.set(root, next);
        } else {
          integrationsCatalogMemoryCache.delete(root);
        }
        return next;
      });
    },
    [root],
  );

  const load = useCallback(
    async (options?: {
      ignoreCache?: boolean;
      preserveMessage?: boolean;
      probeTools?: boolean;
      silent?: boolean;
    }) => {
      const cached = integrationsCatalogMemoryCache.get(root);
      const showSpinner =
        !options?.silent &&
        (!cached || Boolean(options?.ignoreCache) || Boolean(options?.probeTools));
      if (showSpinner) {
        setLoading(true);
      }
      if (!options?.preserveMessage) {
        setMessage(null);
      }
      try {
        const c = await invoke<IntegrationsCatalog>(
          "get_integrations_catalog",
          {
            projectRoot: root,
            ignoreCache: options?.ignoreCache ?? false,
            probeTools: options?.probeTools ?? false,
          },
        );
        setCatalogSnapshot(c);
        setPendingMcpVerification((prev) => {
          const next = { ...prev };
          for (const srv of c.mcpServers) {
            delete next[srv.normalizedKey];
          }
          return next;
        });
        setExpandedMcp((prev) => {
          const errored = c.mcpServers.filter(
            (srv) => srv.enabled && srv.listToolsError,
          );
          if (errored.length === 0) return prev;
          const next = { ...prev };
          for (const srv of errored) {
            next[srv.normalizedKey] = true;
          }
          return next;
        });
      } catch (e) {
        if (!cached) {
          setCatalogSnapshot(null);
        }
        setMessage({
          kind: "error",
          text: extractErrorMessage(e),
        });
      } finally {
        setLoading(false);
      }
    },
    [root, setCatalogSnapshot],
  );

  useEffect(() => {
    const cached = integrationsCatalogMemoryCache.get(root);
    if (cached) {
      setCatalog(cached);
    } else {
      setCatalog(null);
    }
    void load({ silent: Boolean(cached), probeTools: false });
  }, [load, root]);

  const persist = useCallback(
    async (next: IntegrationsCatalog) => {
      setSaving(true);
      setMessage(null);
      try {
        const disabledMcpServers = next.mcpServers
          .filter((s) => !s.enabled)
          .map((s) => s.normalizedKey);
        const disabledSkills = next.skills
          .filter((s) => !s.enabled)
          .map((s) => s.name);
        await invoke("save_integrations_state", {
          projectRoot: root,
          disabledMcpServers,
          disabledSkills,
        });
        setMessage({
          kind: "success",
          text: "已保存到 .omiga/integrations.json，新对话将生效。",
        });
        setCatalogSnapshot(next);
      } catch (e) {
        setMessage({
          kind: "error",
          text: extractErrorMessage(e),
        });
      } finally {
        setSaving(false);
      }
    },
    [root, setCatalogSnapshot],
  );

  const setMcpEnabled = (normalizedKey: string, enabled: boolean) => {
    if (!catalog) return;
    setPendingMcpVerification((prev) => {
      const next = { ...prev };
      delete next[normalizedKey];
      return next;
    });
    setMcpOAuthFlows((prev) => {
      const next = { ...prev };
      delete next[normalizedKey];
      return next;
    });
    const mcpServers = catalog.mcpServers.map((s) =>
      s.normalizedKey === normalizedKey ? { ...s, enabled } : s,
    );
    void persist({ ...catalog, mcpServers });
  };

  const setSkillEnabled = (name: string, enabled: boolean) => {
    if (!catalog) return;
    const skills = catalog.skills.map((s) =>
      s.name === name ? { ...s, enabled } : s,
    );
    void persist({ ...catalog, skills });
  };

  const openAddMcpDialog = () => {
    setMcpForm(emptyMcpServerForm());
    setEditingMcpName(null);
    setMcpFormError(null);
    setAddMcpOpen(true);
  };

  const openEditMcpDialog = (srv: McpServerCatalogEntry) => {
    setMcpForm(mcpServerFormFromCatalogEntry(srv));
    setEditingMcpName(srv.configKey);
    setMcpFormError(null);
    setAddMcpOpen(true);
  };

  const submitMcpServer = useCallback(async () => {
    if (noWorkspace) {
      setMcpFormError("当前会话未绑定工作区，无法写入项目 .omiga/mcp.json。");
      return;
    }
    setAddingMcp(true);
    setMcpFormError(null);
    try {
      const server = buildProjectMcpServerInput(mcpForm);
      const res = await invoke<ImportMcpMergeResult>(
        "upsert_project_mcp_server",
        {
          projectRoot: root,
          server,
        },
      );
      setAddMcpOpen(false);
      setMessage({
        kind: "success",
        text: `${editingMcpName ? "已更新" : "已保存"}「${server.name}」到 ${res.wrotePath}（当前项目共 ${res.serverCount} 个 MCP 配置项）。新对话将加载最新配置。`,
      });
      await load({ ignoreCache: true, preserveMessage: true });
    } catch (e) {
      setMcpFormError(extractErrorMessage(e));
    } finally {
      setAddingMcp(false);
    }
  }, [editingMcpName, load, mcpForm, noWorkspace, root]);

  const deleteMcpServer = useCallback(
    async (srv: McpServerCatalogEntry) => {
      if (noWorkspace) return;
      if (
        !window.confirm(
          `确定移除 MCP 服务「${srv.configKey}」？\n\n这会在当前项目 .omiga/mcp.json 中写入隐藏规则；不会修改用户级或内置配置。`,
        )
      ) {
        return;
      }
      setDeletingMcpKey(srv.normalizedKey);
      setMessage(null);
      try {
        const res = await invoke<ImportMcpMergeResult>(
          "delete_project_mcp_server",
          {
            projectRoot: root,
            name: srv.configKey,
          },
        );
        setMessage({
          kind: "success",
          text: `已从当前项目移除「${srv.configKey}」（写入 ${res.wrotePath}）。新对话将加载最新配置。`,
        });
        await load({ ignoreCache: true, preserveMessage: true });
      } catch (e) {
        setMessage({
          kind: "error",
          text: extractErrorMessage(e),
        });
      } finally {
        setDeletingMcpKey(null);
      }
    },
    [load, noWorkspace, root],
  );

  const verifyMcpServer = useCallback(
    async (srv: McpServerCatalogEntry) => {
      setVerifyingMcpKey(srv.normalizedKey);
      setPendingMcpVerification((prev) => {
        const next = { ...prev };
        delete next[srv.normalizedKey];
        return next;
      });
      setMcpOAuthFlows((prev) => {
        const next = { ...prev };
        delete next[srv.normalizedKey];
        return next;
      });
      setMessage(null);
      try {
        const res = await invoke<VerifyMcpServerResult>("verify_mcp_server", {
          projectRoot: root,
          serverName: srv.configKey,
        });
        setCatalogSnapshot((prev) => {
          if (!prev) return prev;
          return {
            ...prev,
            mcpServers: prev.mcpServers.map((entry) =>
              entry.normalizedKey === srv.normalizedKey
                ? {
                    ...entry,
                    toolListChecked: res.toolListChecked ?? true,
                    oauthAuthenticated:
                      res.oauthAuthenticated ?? entry.oauthAuthenticated,
                    listToolsError: res.ok
                      ? null
                      : res.listToolsError ?? "MCP 连接验证失败。",
                    tools: res.tools,
                  }
                : entry,
            ),
          };
        });
        setExpandedMcp((prev) => ({
          ...prev,
          [srv.normalizedKey]: !res.ok || res.tools.length > 0,
        }));
        setMessage({
          kind: res.ok ? "success" : "error",
          text: res.ok
            ? `「${srv.configKey}」验证通过，发现 ${res.tools.length} 个工具。`
            : `「${srv.configKey}」验证失败：${res.listToolsError ?? "未知错误"}`,
        });
      } catch (e) {
        setExpandedMcp((prev) => ({
          ...prev,
          [srv.normalizedKey]: true,
        }));
        setMessage({
          kind: "error",
          text: extractErrorMessage(e),
        });
      } finally {
        setVerifyingMcpKey(null);
      }
    },
    [root, setCatalogSnapshot],
  );

  const pollMcpOAuthLogin = useCallback(
    async (normalizedKey: string, flow: McpOAuthFlow) => {
      if (!flow.loginSessionId) return;
      setMcpOAuthFlows((prev) => {
        const current = prev[normalizedKey];
        if (!current || current.loginSessionId !== flow.loginSessionId) return prev;
        return {
          ...prev,
          [normalizedKey]: {
            ...current,
            status: "exchanging",
            message: "正在交换 token…",
          },
        };
      });

      try {
        const res = await invoke<PollMcpOAuthLoginResult>("poll_mcp_oauth_login", {
          projectRoot: root,
          loginSessionId: flow.loginSessionId,
        });
        const rowKey = res.normalizedKey || normalizedKey;

        if (res.status === "pending") {
          setMcpOAuthFlows((prev) => {
            const current = prev[rowKey];
            if (!current || current.loginSessionId !== flow.loginSessionId) return prev;
            return {
              ...prev,
              [rowKey]: {
                ...current,
                status: "waiting",
                intervalSecs: Math.max(1, res.intervalSecs || current.intervalSecs),
                message: res.message,
              },
            };
          });
          return;
        }

        const terminalError =
          res.status === "complete"
            ? (res.listToolsError ?? null)
            : res.message || "MCP OAuth 登录失败。";
        setCatalogSnapshot((prev) => {
          if (!prev) return prev;
          return {
            ...prev,
            mcpServers: prev.mcpServers.map((entry) =>
              entry.normalizedKey === rowKey
                ? {
                    ...entry,
                    toolListChecked: res.toolListChecked ?? entry.toolListChecked,
                    oauthAuthenticated:
                      res.oauthAuthenticated ?? entry.oauthAuthenticated,
                    listToolsError:
                      res.status === "complete" && res.ok ? null : terminalError,
                    tools: res.tools,
                  }
                : entry,
            ),
          };
        });
        setExpandedMcp((prev) => ({
          ...prev,
          [rowKey]: true,
        }));
        setPendingMcpVerification((prev) => {
          const next = { ...prev };
          delete next[rowKey];
          return next;
        });
        setMcpOAuthFlows((prev) => {
          const next = { ...prev };
          delete next[rowKey];
          return next;
        });
        const success = res.status === "complete" && res.ok;
        setMessage({
          kind: success ? "success" : "error",
          text: success
            ? `「${res.configKey}」授权完成，发现 ${res.tools.length} 个工具。`
            : `「${res.configKey}」${res.status === "complete" ? "授权成功但验证失败" : "授权失败"}：${terminalError ?? res.message}`,
        });
      } catch (e) {
        setMcpOAuthFlows((prev) => {
          const next = { ...prev };
          delete next[normalizedKey];
          return next;
        });
        setPendingMcpVerification((prev) => {
          const next = { ...prev };
          delete next[normalizedKey];
          return next;
        });
        setMessage({
          kind: "error",
          text: `MCP OAuth 状态检查失败：${extractErrorMessage(e)}`,
        });
      }
    },
    [root, setCatalogSnapshot],
  );

  useEffect(() => {
    const timers = Object.entries(mcpOAuthFlows)
      .filter(([, flow]) => flow.status === "waiting" || flow.status === "verifying")
      .map(([normalizedKey, flow]) =>
        window.setTimeout(
          () => void pollMcpOAuthLogin(normalizedKey, flow),
          Math.max(1, flow.intervalSecs || 2) * 1000,
        ),
      );
    return () => {
      timers.forEach((timer) => window.clearTimeout(timer));
    };
  }, [mcpOAuthFlows, pollMcpOAuthLogin]);

  const startMcpOAuthLogin = useCallback(
    async (srv: McpServerCatalogEntry) => {
      setMessage(null);
      setExpandedMcp((prev) => ({
        ...prev,
        [srv.normalizedKey]: true,
      }));
      setPendingMcpVerification((prev) => ({
        ...prev,
        [srv.normalizedKey]: true,
      }));
      setMcpOAuthFlows((prev) => ({
        ...prev,
        [srv.normalizedKey]: {
          loginSessionId: "",
          status: "opening",
          intervalSecs: 2,
          message: "正在准备 OAuth 授权…",
        },
      }));
      try {
        const res = await invoke<StartMcpOAuthLoginResult>(
          "start_mcp_oauth_login",
          {
            projectRoot: root,
            serverName: srv.configKey,
          },
        );
        setMcpOAuthFlows((prev) => ({
          ...prev,
          [res.normalizedKey]: {
            loginSessionId: res.loginSessionId,
            status: "waiting",
            intervalSecs: Math.max(1, res.intervalSecs || 2),
            message: res.message,
          },
        }));
        await openUrl(res.authorizationUrl);
        setMessage({
          kind: "success",
          text: `已打开「${srv.configKey}」授权页。完成登录后 Omiga 会自动获取 token 并验证工具列表。`,
        });
      } catch (e) {
        setPendingMcpVerification((prev) => {
          const next = { ...prev };
          delete next[srv.normalizedKey];
          return next;
        });
        setMcpOAuthFlows((prev) => {
          const next = { ...prev };
          delete next[srv.normalizedKey];
          return next;
        });
        setMessage({
          kind: "error",
          text: `启动 MCP OAuth 登录失败：${extractErrorMessage(e)}`,
        });
      }
    },
    [root],
  );

  const logoutMcpOAuthServer = useCallback(
    async (srv: McpServerCatalogEntry) => {
      setLoggingOutMcpKey(srv.normalizedKey);
      setMessage(null);
      try {
        const res = await invoke<LogoutMcpOAuthServerResult>(
          "logout_mcp_oauth_server",
          {
            projectRoot: root,
            serverName: srv.configKey,
          },
        );
        setCatalogSnapshot((prev) => {
          if (!prev) return prev;
          return {
            ...prev,
            mcpServers: prev.mcpServers.map((entry) =>
              entry.normalizedKey === res.normalizedKey
                ? {
                    ...entry,
                    oauthAuthenticated: false,
                    toolListChecked: false,
                    listToolsError: null,
                    tools: [],
                  }
                : entry,
            ),
          };
        });
        setExpandedMcp((prev) => {
          const next = { ...prev };
          delete next[res.normalizedKey];
          return next;
        });
        setMessage({
          kind: "success",
          text: `已退出「${res.configKey}」OAuth 登录，并清除本机安全存储中的 token。`,
        });
      } catch (e) {
        setMessage({
          kind: "error",
          text: `退出 MCP OAuth 登录失败：${extractErrorMessage(e)}`,
        });
      } finally {
        setLoggingOutMcpKey(null);
      }
    },
    [root, setCatalogSnapshot],
  );

  const uninstallOmigaSkillCopy = useCallback(
    async (sk: SkillCatalogEntry) => {
      if (!sk.canUninstallOmigaCopy || !sk.directoryName) return;
      const src: SkillSource = isSkillSource(sk.source)
        ? sk.source
        : "omigaProject";
      if (src === "omigaProject" && noWorkspace) return;
      const target = src === "omigaUser" ? "userOmiga" : "projectOmiga";
      const rk = `${target}:${sk.directoryName}`;
      if (
        !window.confirm(
          `确定删除 Omiga 目录下的技能副本「${sk.directoryName}」？\n（不会删除 ~/.claude/skills 中的文件）`,
        )
      ) {
        return;
      }
      setMessage(null);
      setRemovingKey(rk);
      try {
        await invoke("remove_omiga_imported_skill", {
          projectRoot: root,
          directoryName: sk.directoryName,
          target,
        });
        setMessage({
          kind: "success",
          text: `已卸载：${sk.directoryName}`,
        });
        await load();
      } catch (e) {
        setMessage({
          kind: "error",
          text: extractErrorMessage(e),
        });
      } finally {
        setRemovingKey(null);
      }
    },
    [root, load, noWorkspace],
  );

  const showMcp = mode === "mcp" || mode === "both";
  const showSkills = mode === "skills" || mode === "both";
  const mcpServers = catalog?.mcpServers ?? [];
  const enabledMcpCount = mcpServers.filter((srv) => srv.enabled).length;
  const mcpToolCount = mcpServers.reduce(
    (sum, srv) => sum + srv.tools.length,
    0,
  );
  const mcpErrorCount = mcpServers.filter((srv) => srv.listToolsError).length;
  const mcpAuthedCount = mcpServers.filter(
    (srv) => srv.oauthAuthenticated,
  ).length;
  const mcpOAuthBusy = Object.keys(mcpOAuthFlows).length > 0;
  const mcpPendingCount = mcpServers.filter(
    (srv) =>
      srv.enabled &&
      (pendingMcpVerification[srv.normalizedKey] ||
        Boolean(mcpOAuthFlows[srv.normalizedKey])),
  ).length;

  return (
    <Box sx={{ mt: 2 }}>
      {showMcp && (
        <Card
          elevation={0}
          sx={(theme) => ({
            mb: 2,
            overflow: "hidden",
            borderRadius: 4,
            border: `1px solid ${alpha(theme.palette.divider, theme.palette.mode === "dark" ? 0.65 : 1)}`,
            background:
              theme.palette.mode === "dark"
                ? `linear-gradient(135deg, ${alpha(theme.palette.primary.main, 0.14)} 0%, ${alpha(theme.palette.background.paper, 0.92)} 46%, ${alpha(theme.palette.success.main, 0.1)} 100%)`
                : `linear-gradient(135deg, ${alpha(theme.palette.primary.light, 0.18)} 0%, ${theme.palette.background.paper} 54%, ${alpha(theme.palette.success.light, 0.18)} 100%)`,
            boxShadow:
              theme.palette.mode === "dark"
                ? "0 22px 56px rgba(0,0,0,0.32)"
                : "0 18px 48px rgba(15, 23, 42, 0.08)",
          })}
        >
          <CardContent sx={{ p: { xs: 2, sm: 2.5 } }}>
            <Stack
              direction={{ xs: "column", sm: "row" }}
              spacing={2}
              alignItems={{ xs: "stretch", sm: "flex-start" }}
              justifyContent="space-between"
            >
              <Box sx={{ minWidth: 0 }}>
                <Typography variant="h6" fontWeight={750} letterSpacing="-0.03em">
                  MCP 服务器
                </Typography>
                <Typography
                  variant="body2"
                  color="text.secondary"
                  sx={{ mt: 0.5, lineHeight: 1.65, maxWidth: 720 }}
                >
                  连接外部工具和数据源。新增服务会写入当前项目的 .omiga/mcp.json；新对话将读取最新配置。
                </Typography>
              </Box>
              <Stack direction="row" spacing={1} sx={{ flexShrink: 0 }}>
                <Tooltip title="重新检测 MCP 服务">
                  <span>
                    <IconButton
                      size="small"
                      disabled={
                        loading ||
                        saving ||
                        addingMcp ||
                        deletingMcpKey !== null ||
                        verifyingMcpKey !== null ||
                        loggingOutMcpKey !== null ||
                        mcpOAuthBusy
                      }
                      onClick={() =>
                        void load({ ignoreCache: true, probeTools: true })
                      }
                      sx={(theme) => ({
                        border: `1px solid ${alpha(theme.palette.divider, 0.65)}`,
                        borderRadius: 2,
                      })}
                    >
                      {loading ? <CircularProgress size={16} /> : <RefreshIcon fontSize="small" />}
                    </IconButton>
                  </span>
                </Tooltip>
                <Button
                  size="small"
                  variant="contained"
                  startIcon={<AddIcon />}
                  disabled={
                    noWorkspace ||
                    addingMcp ||
                    deletingMcpKey !== null ||
                    verifyingMcpKey !== null ||
                    loggingOutMcpKey !== null ||
                    mcpOAuthBusy
                  }
                  onClick={openAddMcpDialog}
                >
                  添加服务器
                </Button>
              </Stack>
            </Stack>

            <Stack
              direction="row"
              spacing={1}
              useFlexGap
              flexWrap="wrap"
              sx={{ mt: 2 }}
            >
              <Chip size="small" label={`${mcpServers.length} 个服务`} />
              <Chip size="small" color="success" variant="outlined" label={`${enabledMcpCount} 个启用`} />
              <Chip size="small" color="primary" variant="outlined" label={`${mcpToolCount} 个工具`} />
              {mcpAuthedCount > 0 && (
                <Chip size="small" color="secondary" variant="outlined" label={`${mcpAuthedCount} 个 OAuth 已登录`} />
              )}
              {mcpErrorCount > 0 && (
                <Chip size="small" color="error" variant="outlined" label={`${mcpErrorCount} 个连接异常`} />
              )}
              {mcpPendingCount > 0 && (
                <Chip size="small" color="warning" variant="outlined" label={`${mcpPendingCount} 个待验证`} />
              )}
            </Stack>

            <Box
              sx={(theme) => ({
                mt: 2,
                px: 1.5,
                py: 1.25,
                borderRadius: 2.5,
                border: `1px solid ${alpha(theme.palette.info.main, 0.18)}`,
                bgcolor: alpha(theme.palette.info.main, theme.palette.mode === "dark" ? 0.08 : 0.06),
              })}
            >
              <Typography variant="caption" color="text.secondary" sx={{ lineHeight: 1.6 }}>
                合并顺序：内置 bundled_mcp.json → 用户 ~/.omiga/mcp.json → 插件提供的 MCP → 当前项目 .omiga/mcp.json（同名以后者为准）。
              </Typography>
            </Box>
          </CardContent>
        </Card>
      )}

      {!showMcp && (
        <Box
          sx={{
            display: "flex",
            alignItems: "center",
            justifyContent: "space-between",
            gap: 1,
            mb: 1,
          }}
        >
          <Typography variant="subtitle1" fontWeight={600}>
            当前已加载技能（启用 / 禁用）
          </Typography>
        </Box>
      )}

      {message && (
        <Alert
          severity={message.kind === "success" ? "success" : "error"}
          sx={{ mb: 2, borderRadius: 2 }}
          onClose={() => setMessage(null)}
        >
          {message.text}
        </Alert>
      )}

      {loading && !catalog && (
        <Box sx={{ py: 2, display: "flex", justifyContent: "center" }}>
          <CircularProgress size={28} />
        </Box>
      )}

      {catalog && showMcp && (
        <Box sx={{ mb: showSkills ? 3 : 0 }}>
          {mcpServers.length === 0 ? (
            <Card
              elevation={0}
              sx={(theme) => ({
                borderRadius: 3,
                border: `1px dashed ${alpha(theme.palette.divider, 0.8)}`,
                bgcolor: alpha(theme.palette.background.paper, theme.palette.mode === "dark" ? 0.48 : 0.82),
              })}
            >
              <CardContent
                sx={{
                  minHeight: 220,
                  display: "flex",
                  flexDirection: "column",
                  alignItems: "center",
                  justifyContent: "center",
                  textAlign: "center",
                  gap: 1.5,
                }}
              >
                <Avatar
                  sx={(theme) => ({
                    width: 56,
                    height: 56,
                    bgcolor: alpha(theme.palette.success.main, 0.14),
                    color: "success.main",
                    border: `1px solid ${alpha(theme.palette.success.main, 0.28)}`,
                  })}
                >
                  <AddIcon />
                </Avatar>
                <Box>
                  <Typography fontWeight={750}>还没有 MCP 服务器</Typography>
                  <Typography variant="body2" color="text.secondary" sx={{ mt: 0.5 }}>
                    添加本地 STDIO 服务或远程 Streamable HTTP MCP，让对话可以调用外部工具。
                  </Typography>
                </Box>
                <Typography variant="caption" color="text.secondary">
                  点击右上角“添加服务器”开始配置。
                </Typography>
              </CardContent>
            </Card>
          ) : (
            <Box
              sx={(theme) => ({
                borderRadius: 3,
                border: `1px solid ${alpha(theme.palette.divider, theme.palette.mode === "dark" ? 0.75 : 1)}`,
                overflow: "hidden",
                bgcolor:
                  theme.palette.mode === "dark"
                    ? alpha(theme.palette.background.paper, 0.58)
                    : alpha(theme.palette.background.paper, 0.94),
              })}
            >
              {mcpServers.map((srv, idx) => {
                const expanded = expandedMcp[srv.normalizedKey] ?? false;
                const oauthFlow = mcpOAuthFlows[srv.normalizedKey] ?? null;
                const hasExpand =
                  (srv.tools.length > 0 || Boolean(srv.listToolsError)) &&
                  srv.enabled;
                const isVerifying = verifyingMcpKey === srv.normalizedKey;
                const isLoggingOut = loggingOutMcpKey === srv.normalizedKey;
                const pendingVerification =
                  srv.enabled &&
                  (Boolean(pendingMcpVerification[srv.normalizedKey]) ||
                    Boolean(oauthFlow));
                const statusDot = (theme: Theme) => {
                  if (!srv.enabled) return theme.palette.action.disabled;
                  if (isVerifying || pendingVerification) return theme.palette.warning.main;
                  if (srv.listToolsError) return theme.palette.error.main;
                  return theme.palette.success.main;
                };
                const advice = mcpErrorAdvice(srv);
                const needsBrowserLogin =
                  srv.enabled && mcpAuthRequired(srv) && !pendingVerification;
                const showVerifyButton =
                  srv.enabled &&
                  !oauthFlow &&
                  !needsBrowserLogin &&
                  (!srv.toolListChecked ||
                    pendingVerification ||
                    Boolean(srv.listToolsError) ||
                    srv.tools.length === 0);
                const oauthButtonLabel = (() => {
                  if (!oauthFlow) return "";
                  if (oauthFlow.status === "opening") return "准备授权…";
                  if (oauthFlow.status === "waiting") return "待验证";
                  if (oauthFlow.status === "exchanging") return "交换 token…";
                  return "验证工具…";
                })();
                return (
                  <Box key={srv.configKey}>
                    {idx > 0 && <Divider sx={{ opacity: 0.65 }} />}
                    <Box
                      sx={(theme) => ({
                        display: "flex",
                        alignItems: "center",
                        gap: 1.5,
                        px: 2,
                        py: 1.5,
                        minHeight: 72,
                        transition: "background-color 0.18s ease",
                        "&:hover": {
                          bgcolor: alpha(theme.palette.text.primary, theme.palette.mode === "dark" ? 0.045 : 0.025),
                        },
                      })}
                    >
                      <Box sx={{ position: "relative", flexShrink: 0 }}>
                        <Avatar
                          variant="rounded"
                          sx={(theme) => ({
                            width: 44,
                            height: 44,
                            fontSize: "1rem",
                            fontWeight: 750,
                            bgcolor: alpha(theme.palette.primary.main, 0.11),
                            color: "text.primary",
                            border: `1px solid ${alpha(theme.palette.divider, 0.64)}`,
                          })}
                        >
                          {mcpInitialLetter(srv.configKey)}
                        </Avatar>
                        <Box
                          sx={(theme) => ({
                            position: "absolute",
                            right: -1,
                            bottom: -1,
                            width: 11,
                            height: 11,
                            borderRadius: "50%",
                            bgcolor: statusDot(theme),
                            border: `2px solid ${theme.palette.background.paper}`,
                            boxSizing: "border-box",
                          })}
                        />
                      </Box>
                      <Box sx={{ minWidth: 0, flex: 1 }}>
                        <Box
                          sx={{
                            display: "flex",
                            alignItems: "center",
                            gap: 0.75,
                            minWidth: 0,
                          }}
                        >
                          <Typography
                            fontWeight={750}
                            fontSize={15}
                            lineHeight={1.3}
                            noWrap
                            title={srv.configKey}
                            sx={{ color: "text.primary", minWidth: 0 }}
                          >
                            {srv.configKey}
                          </Typography>
                          {srv.oauthAuthenticated && (
                            <Chip
                              size="small"
                              label="OAuth 已保存"
                              color="success"
                              variant="outlined"
                              sx={{ height: 20, fontSize: 11, flexShrink: 0 }}
                            />
                          )}
                        </Box>
                        <Box
                          sx={{
                            display: "flex",
                            alignItems: "center",
                            gap: 0.5,
                            mt: 0.35,
                          }}
                        >
                          <Typography
                            variant="caption"
                            color="text.secondary"
                            sx={{
                              fontSize: 12,
                              lineHeight: 1.35,
                              overflow: "hidden",
                              textOverflow: "ellipsis",
                              whiteSpace: "nowrap",
                            }}
                            title={srv.normalizedKey}
                          >
                            {mcpRowSubtitle(srv, {
                              pendingVerification,
                              verifying: isVerifying,
                              authFlowStatus: oauthFlow?.status,
                            })}
                          </Typography>
                          {hasExpand && (
                            <IconButton
                              size="small"
                              aria-expanded={expanded}
                              aria-label={expanded ? "收起详情" : "展开详情"}
                              onClick={(e) => {
                                e.stopPropagation();
                                setExpandedMcp((p) => ({
                                  ...p,
                                  [srv.normalizedKey]: !expanded,
                                }));
                              }}
                              sx={{
                                p: 0.25,
                                color: "text.secondary",
                                transform: expanded ? "rotate(180deg)" : "none",
                                transition: "transform 0.2s ease",
                              }}
                            >
                              <ExpandMoreIcon sx={{ fontSize: 18 }} />
                            </IconButton>
                          )}
                        </Box>
                      </Box>
                      <Box
                        sx={{
                          display: "flex",
                          alignItems: "center",
                          gap: 0.25,
                          flexShrink: 0,
                        }}
                      >
                        {srv.oauthAuthenticated && !oauthFlow && (
                          <Button
                            size="small"
                            color="inherit"
                            disabled={
                              saving ||
                              addingMcp ||
                              deletingMcpKey !== null ||
                              verifyingMcpKey !== null ||
                              loggingOutMcpKey !== null ||
                              mcpOAuthBusy
                            }
                            onClick={() => void logoutMcpOAuthServer(srv)}
                            startIcon={
                              isLoggingOut ? (
                                <CircularProgress color="inherit" size={13} />
                              ) : undefined
                            }
                            sx={{
                              minWidth: 54,
                              mr: 0.5,
                              textTransform: "none",
                            }}
                          >
                            {isLoggingOut ? "退出中…" : "退出"}
                          </Button>
                        )}
                        {oauthFlow && (
                          <Button
                            size="small"
                            variant="contained"
                            color="primary"
                            disabled
                            startIcon={<CircularProgress color="inherit" size={14} />}
                            sx={{
                              minWidth: 96,
                              mr: 0.5,
                              textTransform: "none",
                              "&.Mui-disabled": {
                                color: "primary.contrastText",
                                bgcolor: "primary.main",
                                opacity: 0.72,
                              },
                            }}
                          >
                            {oauthButtonLabel}
                          </Button>
                        )}
                        {needsBrowserLogin && (
                          <Button
                            size="small"
                            variant="contained"
                            color="primary"
                            disabled={
                              saving ||
                              addingMcp ||
                              deletingMcpKey !== null ||
                              verifyingMcpKey !== null ||
                              loggingOutMcpKey !== null ||
                              mcpOAuthBusy
                            }
                            onClick={() => void startMcpOAuthLogin(srv)}
                            sx={{
                              minWidth: 72,
                              mr: 0.5,
                              textTransform: "none",
                            }}
                          >
                            连接
                          </Button>
                        )}
                        {showVerifyButton && (
                          <Button
                            size="small"
                            variant={
                              srv.listToolsError && !needsBrowserLogin
                                ? "contained"
                                : "outlined"
                            }
                            color={
                              srv.listToolsError && !needsBrowserLogin
                                ? "primary"
                                : "inherit"
                            }
                            disabled={
                              saving ||
                              addingMcp ||
                              deletingMcpKey !== null ||
                              verifyingMcpKey !== null ||
                              loggingOutMcpKey !== null ||
                              mcpOAuthBusy
                            }
                            onClick={() => void verifyMcpServer(srv)}
                            startIcon={
                              isVerifying ? (
                                <CircularProgress color="inherit" size={14} />
                              ) : undefined
                            }
                            sx={{
                              minWidth: 72,
                              mr: 0.5,
                              textTransform: "none",
                            }}
                          >
                            {isVerifying
                              ? "验证中…"
                              : srv.listToolsError && !pendingVerification
                                ? "连接"
                                : "验证"}
                          </Button>
                        )}
                        <Tooltip title="编辑配置（保存为项目级覆盖）">
                          <span>
                            <IconButton
                              size="small"
                              disabled={
                                saving ||
                                addingMcp ||
                                deletingMcpKey !== null ||
                                verifyingMcpKey !== null ||
                                loggingOutMcpKey !== null ||
                                mcpOAuthBusy
                              }
                              onClick={() => openEditMcpDialog(srv)}
                              aria-label={`编辑 MCP 服务 ${srv.configKey}`}
                            >
                              <EditOutlinedIcon sx={{ fontSize: 18 }} />
                            </IconButton>
                          </span>
                        </Tooltip>
                        <Tooltip title="从当前项目移除 / 隐藏">
                          <span>
                            <IconButton
                              size="small"
                              color="error"
                              disabled={
                                noWorkspace ||
                                saving ||
                                addingMcp ||
                                deletingMcpKey !== null ||
                                verifyingMcpKey !== null ||
                                loggingOutMcpKey !== null ||
                                mcpOAuthBusy
                              }
                              onClick={() => void deleteMcpServer(srv)}
                              aria-label={`移除 MCP 服务 ${srv.configKey}`}
                              sx={{ opacity: deletingMcpKey ? 0.7 : 1 }}
                            >
                              {deletingMcpKey === srv.normalizedKey ? (
                                <CircularProgress size={16} />
                              ) : (
                                <DeleteOutlineIcon sx={{ fontSize: 18 }} />
                              )}
                            </IconButton>
                          </span>
                        </Tooltip>
                        <Switch
                          size="small"
                          color="success"
                          checked={srv.enabled}
                          disabled={
                            saving ||
                            deletingMcpKey !== null ||
                            verifyingMcpKey !== null ||
                            loggingOutMcpKey !== null ||
                            mcpOAuthBusy
                          }
                          onChange={(_, v) => setMcpEnabled(srv.normalizedKey, v)}
                          inputProps={{
                            "aria-label": srv.enabled ? "禁用 MCP 服务" : "启用 MCP 服务",
                          }}
                          sx={{ ml: 0.5 }}
                        />
                      </Box>
                    </Box>
                    <Collapse in={expanded && hasExpand} timeout="auto" unmountOnExit>
                      <Box
                        sx={(theme) => ({
                          px: 2,
                          pb: 1.5,
                          pl: { xs: 2, sm: 9 },
                          borderTop: `1px solid ${alpha(theme.palette.divider, 0.5)}`,
                          bgcolor: alpha(theme.palette.common.black, theme.palette.mode === "dark" ? 0.18 : 0.04),
                        })}
                      >
                        {srv.listToolsError && (
                          <Stack spacing={1} sx={{ mb: srv.tools.length > 0 ? 1 : 0 }}>
                            {advice && (
                              <Box
                                sx={(theme) => ({
                                  display: "grid",
                                  gridTemplateColumns: {
                                    xs: "24px 1fr",
                                    sm: needsBrowserLogin ? "24px 1fr auto" : "24px 1fr",
                                  },
                                  gap: 1.25,
                                  alignItems: "flex-start",
                                  px: 1.5,
                                  py: 1.35,
                                  borderRadius: 2,
                                  border: `1px solid ${alpha(theme.palette.warning.main, 0.38)}`,
                                  bgcolor: alpha(
                                    theme.palette.warning.main,
                                    theme.palette.mode === "dark" ? 0.08 : 0.04,
                                  ),
                                })}
                              >
                                <Box
                                  sx={(theme) => ({
                                    width: 18,
                                    height: 18,
                                    mt: 0.15,
                                    borderRadius: "50%",
                                    color: "warning.main",
                                    display: "flex",
                                    alignItems: "center",
                                    justifyContent: "center",
                                    border: `1px solid ${alpha(theme.palette.warning.main, 0.55)}`,
                                    fontSize: 12,
                                    fontWeight: 800,
                                  })}
                                >
                                  !
                                </Box>
                                <Box sx={{ minWidth: 0 }}>
                                  <Typography
                                    variant="caption"
                                    component="div"
                                    fontWeight={750}
                                    sx={{ lineHeight: 1.45 }}
                                  >
                                    {advice.title}
                                  </Typography>
                                  <Typography
                                    variant="caption"
                                    component="div"
                                    color="text.secondary"
                                    sx={{ mt: 0.25, lineHeight: 1.6 }}
                                  >
                                    {advice.detail}
                                  </Typography>
                                  <Box component="ul" sx={{ m: 0.5, mb: 0, pl: 2.2 }}>
                                    {advice.actions.map((action) => (
                                      <Box
                                        key={action}
                                        component="li"
                                        sx={(theme) => ({
                                          color: theme.palette.text.secondary,
                                          fontSize: 12,
                                          lineHeight: 1.65,
                                        })}
                                      >
                                        {action}
                                      </Box>
                                    ))}
                                  </Box>
                                </Box>
                                {needsBrowserLogin && (
                                  <Button
                                    color="warning"
                                    size="small"
                                    variant="outlined"
                                    disabled={mcpOAuthBusy || loggingOutMcpKey !== null}
                                    onClick={() => void startMcpOAuthLogin(srv)}
                                    sx={{
                                      alignSelf: "flex-start",
                                      whiteSpace: "nowrap",
                                      gridColumn: { xs: "2 / 3", sm: "auto" },
                                    }}
                                  >
                                    连接
                                  </Button>
                                )}
                              </Box>
                            )}
                            <Box
                              sx={(theme) => ({
                                borderRadius: 2,
                                px: 1.25,
                                py: 1,
                                bgcolor: alpha(theme.palette.error.main, 0.08),
                                border: `1px solid ${alpha(theme.palette.error.main, 0.22)}`,
                              })}
                            >
                              <Typography
                                variant="caption"
                                color="error"
                                sx={{
                                  display: "block",
                                  fontFamily: "ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace",
                                  whiteSpace: "pre-wrap",
                                  wordBreak: "break-word",
                                }}
                              >
                                {srv.listToolsError}
                              </Typography>
                            </Box>
                          </Stack>
                        )}
                        {srv.tools.length > 0 && (
                          <List dense disablePadding sx={{ maxHeight: 220, overflow: "auto" }}>
                            {srv.tools.map((t) => (
                              <ListItem key={t.wireName} sx={{ py: 0.35, alignItems: "flex-start", px: 0 }}>
                                <ListItemText
                                  primary={
                                    <Typography variant="caption" fontFamily="monospace" component="span">
                                      {t.wireName}
                                    </Typography>
                                  }
                                  secondary={t.description}
                                />
                              </ListItem>
                            ))}
                          </List>
                        )}
                      </Box>
                    </Collapse>
                  </Box>
                );
              })}
            </Box>
          )}
        </Box>
      )}

      {catalog && showSkills && (
        <Box>
          <Typography variant="body2" fontWeight={600} sx={{ mb: 0.5 }}>
            Skills
          </Typography>

          <Box
            sx={{
              display: "flex",
              alignItems: "center",
              gap: 1,
              mb: 2,
              flexWrap: "wrap",
            }}
          >
            <Tabs
              value={skillFilterTab}
              onChange={(_, v) => setSkillFilterTab(v as SkillFilterTab)}
              aria-label="按技能来源筛选"
              sx={{
                flex: 1,
                minWidth: 0,
                minHeight: 40,
                "& .MuiTab-root": {
                  textTransform: "none",
                  fontWeight: 600,
                  fontSize: "0.875rem",
                },
              }}
            >
              <Tab label="全部" value="all" />
              <Tab label="用户级" value="user" />
              <Tab label="项目级" value="project" />
            </Tabs>
            <Button
              size="small"
              startIcon={
                loading ? <CircularProgress size={14} /> : <RefreshIcon />
              }
              disabled={loading || saving}
              onClick={() => void load({ ignoreCache: true })}
              sx={{ flexShrink: 0, alignSelf: "center" }}
            >
              刷新
            </Button>
          </Box>
          {catalog.skills.length === 0 ? (
            <Typography variant="body2" color="text.secondary">
              暂无技能。可经上方从 Claude 目录或任意文件夹导入到 Omiga，或手动放入
              ~/.omiga/skills、项目 .omiga/skills。
            </Typography>
          ) : (
            (() => {
              const visibleSkills = catalog.skills.filter((sk) =>
                skillMatchesFilter(sk, skillFilterTab),
              );
              if (visibleSkills.length === 0) {
                return (
                  <Typography variant="body2" color="text.secondary">
                    当前分类下暂无技能。
                  </Typography>
                );
              }
              return (
                <Box
                  sx={{
                    display: "grid",
                    gridTemplateColumns: {
                      xs: "1fr",
                      sm: "repeat(2, minmax(0, 1fr))",
                      md: "repeat(3, minmax(0, 1fr))",
                    },
                    gap: 1.5,
                  }}
                >
                  {visibleSkills.map((sk) => {
                    const src = normalizeSkillSource(sk);
                    const showUninstall =
                      sk.canUninstallOmigaCopy &&
                      sk.directoryName &&
                      (src !== "omigaProject" || !noWorkspace);
                    const rk =
                      src === "omigaUser"
                        ? `userOmiga:${sk.directoryName}`
                        : `projectOmiga:${sk.directoryName}`;
                    const busyRm = removingKey === rk;
                    return (
                      <Card
                        key={sk.skillMdPath}
                        elevation={0}
                        sx={(theme) => ({
                          display: "flex",
                          flexDirection: "column",
                          height: "100%",
                          borderRadius: 3,
                          border: `1px solid ${alpha(
                            theme.palette.divider,
                            theme.palette.mode === "dark" ? 0.55 : 1,
                          )}`,
                          background:
                            theme.palette.mode === "dark"
                              ? alpha(theme.palette.background.paper, 0.85)
                              : theme.palette.background.paper,
                          boxShadow:
                            theme.palette.mode === "dark"
                              ? "0 2px 14px rgba(0,0,0,0.28)"
                              : "0 2px 14px rgba(15, 23, 42, 0.05)",
                          transition:
                            "transform 0.22s ease, box-shadow 0.22s ease",
                          "&:hover": {
                            transform: "translateY(-3px)",
                            boxShadow:
                              theme.palette.mode === "dark"
                                ? "0 14px 32px rgba(0,0,0,0.4)"
                                : "0 14px 36px rgba(15, 23, 42, 0.09)",
                          },
                        })}
                      >
                        <CardContent
                          onClick={() => setSkillPreview(sk)}
                          role="button"
                          tabIndex={0}
                          onKeyDown={(ev) => {
                            if (ev.key === "Enter" || ev.key === " ") {
                              ev.preventDefault();
                              setSkillPreview(sk);
                            }
                          }}
                          sx={(theme) => ({
                            flex: 1,
                            pb: 1.5,
                            pt: 2,
                            px: 2,
                            "&:last-child": { pb: 1.5 },
                            cursor: "pointer",
                            "&:hover": {
                              bgcolor: alpha(
                                theme.palette.text.primary,
                                theme.palette.mode === "dark" ? 0.05 : 0.03,
                              ),
                            },
                            "&:focus-visible": {
                              outline: `2px solid ${alpha(theme.palette.text.primary, 0.35)}`,
                              outlineOffset: 2,
                            },
                          })}
                        >
                          <Box
                            sx={{
                              display: "flex",
                              alignItems: "flex-start",
                              justifyContent: "space-between",
                              gap: 1.25,
                              mb: 1.25,
                            }}
                          >
                            <Typography
                              variant="subtitle1"
                              fontWeight={650}
                              sx={{
                                lineHeight: 1.35,
                                letterSpacing: "-0.02em",
                                wordBreak: "break-word",
                                fontSize: "1.02rem",
                              }}
                            >
                              {sk.name}
                            </Typography>
                            <Chip
                              size="small"
                              label={SKILL_SOURCE_LABEL[src]}
                              variant="outlined"
                              sx={(theme) => ({
                                flexShrink: 0,
                                maxWidth: "52%",
                                height: 24,
                                fontSize: "0.65rem",
                                fontWeight: 600,
                                letterSpacing: "0.06em",
                                textTransform: "uppercase",
                                borderColor: alpha(
                                  theme.palette.text.secondary,
                                  0.35,
                                ),
                                color: "text.secondary",
                                bgcolor: alpha(
                                  theme.palette.text.primary,
                                  0.02,
                                ),
                              })}
                            />
                          </Box>
                          <Typography
                            variant="body2"
                            color="text.secondary"
                            sx={{
                              display: "-webkit-box",
                              WebkitLineClamp: 4,
                              WebkitBoxOrient: "vertical",
                              overflow: "hidden",
                              minHeight: "4.5em",
                              lineHeight: 1.65,
                              fontSize: "0.875rem",
                            }}
                          >
                            {sk.description || "—"}
                          </Typography>
                          {sk.tags && sk.tags.length > 0 && (
                            <Box
                              sx={{
                                display: "flex",
                                flexWrap: "wrap",
                                gap: 0.5,
                                mt: 1.25,
                              }}
                            >
                              {sk.tags.map((tag) => (
                                <Chip
                                  key={tag}
                                  size="small"
                                  label={tag}
                                  variant="outlined"
                                  sx={(theme) => ({
                                    height: 22,
                                    fontSize: "0.68rem",
                                    fontWeight: 500,
                                    borderColor: alpha(
                                      theme.palette.primary.main,
                                      0.35,
                                    ),
                                    color: "text.secondary",
                                  })}
                                />
                              ))}
                            </Box>
                          )}
                        </CardContent>
                        <Box
                          onClick={(e) => e.stopPropagation()}
                          onKeyDown={(e) => e.stopPropagation()}
                          sx={(theme) => ({
                            px: 2,
                            py: 1.25,
                            borderTop: `1px solid ${alpha(theme.palette.divider, 0.9)}`,
                            display: "flex",
                            alignItems: "center",
                            justifyContent: "space-between",
                            gap: 1,
                          })}
                        >
                          {showUninstall ? (
                            <Button
                              size="small"
                              color="error"
                              variant="text"
                              disabled={saving || busyRm}
                              startIcon={
                                busyRm ? (
                                  <CircularProgress size={14} />
                                ) : (
                                  <DeleteOutlineIcon fontSize="small" />
                                )
                              }
                              onClick={() => void uninstallOmigaSkillCopy(sk)}
                              sx={{ flexShrink: 0 }}
                            >
                              卸载
                            </Button>
                          ) : (
                            <Box sx={{ minWidth: 0 }} />
                          )}
                          <FormControlLabel
                            sx={{ m: 0, flexShrink: 0 }}
                            control={
                              <Switch
                                size="small"
                                checked={sk.enabled}
                                disabled={saving}
                                onChange={(_, v) => setSkillEnabled(sk.name, v)}
                              />
                            }
                            label={sk.enabled ? "启用" : "禁用"}
                          />
                        </Box>
                      </Card>
                    );
                  })}
                </Box>
              );
            })()
          )}
        </Box>
      )}

      <Dialog
        open={addMcpOpen}
        onClose={() => {
          if (!addingMcp) setAddMcpOpen(false);
        }}
        fullWidth
        maxWidth="sm"
        PaperProps={{
          sx: (theme) => ({
            borderRadius: 4,
            border: `1px solid ${alpha(theme.palette.divider, theme.palette.mode === "dark" ? 0.72 : 1)}`,
            background:
              theme.palette.mode === "dark"
                ? alpha(theme.palette.background.paper, 0.96)
                : theme.palette.background.paper,
          }),
        }}
      >
        <DialogTitle sx={{ pb: 1.25 }}>
          <Typography variant="h6" fontWeight={750} letterSpacing="-0.02em">
            {isEditingMcp ? "编辑 MCP 服务" : "连接至自定义 MCP"}
          </Typography>
          <Typography variant="body2" color="text.secondary" sx={{ mt: 0.5 }}>
            {isEditingMcp
              ? "修改会保存为当前项目覆盖配置；不会改动内置或用户级 MCP 文件。"
              : "配置会保存到当前项目 .omiga/mcp.json，新对话将自动加载。"}
          </Typography>
        </DialogTitle>
        <DialogContent dividers sx={{ pt: 2.25 }}>
          <Stack spacing={2.25}>
            {mcpFormError && (
              <Alert
                severity="error"
                sx={{ borderRadius: 2 }}
                onClose={() => setMcpFormError(null)}
              >
                {mcpFormError}
              </Alert>
            )}

            <TextField
              label="名称"
              placeholder="例如 paperclip 或 github"
              value={mcpForm.name}
              onChange={(e) =>
                setMcpForm((f) => ({ ...f, name: e.target.value }))
              }
              disabled={addingMcp || isEditingMcp}
              autoFocus
              fullWidth
              required
              helperText={isEditingMcp ? "编辑时不支持重命名；如需改名请新增服务后删除旧服务。" : undefined}
            />

            <Box>
              <Typography variant="caption" color="text.secondary" fontWeight={650}>
                连接方式
              </Typography>
              <ToggleButtonGroup
                exclusive
                fullWidth
                value={mcpForm.kind}
                onChange={(_, value: McpProtocol | null) => {
                  if (value) setMcpForm((f) => ({ ...f, kind: value }));
                }}
                disabled={addingMcp}
                sx={{ mt: 0.75 }}
              >
                <ToggleButton value="stdio" sx={{ textTransform: "none" }}>
                  STDIO
                </ToggleButton>
                <ToggleButton value="http" sx={{ textTransform: "none" }}>
                  流式 HTTP
                </ToggleButton>
              </ToggleButtonGroup>
            </Box>

            {mcpForm.kind === "stdio" ? (
              <>
                <TextField
                  label="启动命令"
                  placeholder="例如 npx、uvx、python 或 /path/to/server"
                  value={mcpForm.command}
                  onChange={(e) =>
                    setMcpForm((f) => ({ ...f, command: e.target.value }))
                  }
                  disabled={addingMcp}
                  fullWidth
                  required
                />
                <TextField
                  label="参数"
                  placeholder={"每行一个参数，例如：\n-y\n@modelcontextprotocol/server-filesystem\n."}
                  value={mcpForm.argsText}
                  onChange={(e) =>
                    setMcpForm((f) => ({ ...f, argsText: e.target.value }))
                  }
                  disabled={addingMcp}
                  fullWidth
                  multiline
                  minRows={3}
                  helperText="按行填写，保存时会转为 args 数组。"
                />
                <TextField
                  label="环境变量"
                  placeholder={"每行一个 KEY=value，例如：\nAPI_KEY=..."}
                  value={mcpForm.envText}
                  onChange={(e) =>
                    setMcpForm((f) => ({ ...f, envText: e.target.value }))
                  }
                  disabled={addingMcp}
                  fullWidth
                  multiline
                  minRows={2}
                  helperText="可选；不会写入空行。"
                />
                <TextField
                  label="工作目录"
                  placeholder="默认当前项目；也可填写 ./tools 或 ~/code/server"
                  value={mcpForm.cwd}
                  onChange={(e) =>
                    setMcpForm((f) => ({ ...f, cwd: e.target.value }))
                  }
                  disabled={addingMcp}
                  fullWidth
                  helperText="可选；相对路径会按当前项目解析。"
                />
              </>
            ) : (
              <>
                <TextField
                  label="HTTP 地址"
                  placeholder="https://example.com/mcp"
                  value={mcpForm.url}
                  onChange={(e) =>
                    setMcpForm((f) => ({ ...f, url: e.target.value }))
                  }
                  disabled={addingMcp}
                  fullWidth
                  required
                  helperText="支持 Streamable HTTP MCP 端点。"
                />
                <TextField
                  label="Bearer 令牌环境变量"
                  placeholder="例如 PAPERCLIP_TOKEN"
                  value={mcpForm.bearerEnvName}
                  onChange={(e) =>
                    setMcpForm((f) => ({
                      ...f,
                      bearerEnvName: e.target.value,
                    }))
                  }
                  disabled={addingMcp}
                  fullWidth
                  helperText="可选；保存后写入 Authorization=Bearer ${ENV_NAME}，token 值只从运行环境读取，不明文写入配置。"
                />
                <TextField
                  label="额外请求头"
                  placeholder={"每行一个 KEY=value，例如：\nX-API-Key=${PAPERCLIP_API_KEY}\nX-Workspace=default"}
                  value={mcpForm.headersText}
                  onChange={(e) =>
                    setMcpForm((f) => ({ ...f, headersText: e.target.value }))
                  }
                  disabled={addingMcp}
                  fullWidth
                  multiline
                  minRows={3}
                  helperText="可选；同样支持 ${ENV_NAME}。如果上面已填写 Bearer 变量，这里不要再写 Authorization。"
                />
              </>
            )}
          </Stack>
        </DialogContent>
        <DialogActions sx={{ px: 3, py: 2 }}>
          <Button disabled={addingMcp} onClick={() => setAddMcpOpen(false)}>
            取消
          </Button>
          <Button
            variant="contained"
            startIcon={addingMcp ? <CircularProgress size={16} /> : <AddIcon />}
            disabled={addingMcp || noWorkspace}
            onClick={() => void submitMcpServer()}
          >
            {isEditingMcp ? "保存修改" : "保存服务器"}
          </Button>
        </DialogActions>
      </Dialog>

      <SkillPreviewDialog
        key={skillPreview?.skillMdPath ?? "closed"}
        open={skillPreview !== null}
        skill={skillPreview}
        onClose={() => setSkillPreview(null)}
      />
    </Box>
  );
}
