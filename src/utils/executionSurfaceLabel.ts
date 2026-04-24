import type { ExecutionStep } from "../state/activityStore";

/**
 * Short, fixed category labels (legacy / non–tool-name contexts).
 */
export function toolCategoryLabel(toolName: string): string {
  const n = (toolName || "").toLowerCase();
  if (!n) return "调用工具";
  if (n.includes("web_search")) return "搜索内容";
  if (n.includes("web_fetch") || (n.includes("fetch") && n.includes("web")))
    return "获取网页";
  if (n === "taskcreate" || n.includes("task_create")) return "创建任务";
  if (n.includes("taskstop") || n.includes("taskoutput")) return "任务输出";
  if (
    n === "taskget" ||
    n === "tasklist" ||
    n === "taskupdate" ||
    (n.includes("task") && !n.includes("bash"))
  )
    return "任务操作";
  if (n.includes("todo_write") || n.includes("todowrite")) return "更新清单";
  if (n.includes("bash") || n.includes("shell")) return "运行命令";
  if (n.includes("glob")) return "搜索文件";
  if (n.includes("ripgrep") || n.includes("grep")) return "代码搜索";
  if (n.includes("notebook")) return "编辑笔记";
  if (n.includes("file_read") || n === "read_file") return "读取文件";
  if (n.includes("file_write") || n.includes("write")) return "写入文件";
  if (n.includes("file_edit") || n.includes("edit")) return "编辑文件";
  if (n === "skill" || n.includes("skilltool")) return "技能";
  if (n.includes("ask_user") || n.includes("askuser")) return "询问用户";
  return "调用工具";
}

/**
 * Show the actual tool identifier in the UI (header / task strip).
 * MCP-style names become `server.tool` for readability.
 */
export function formatToolDisplayName(name: string | undefined | null): string {
  const raw = (name ?? "").trim();
  if (!raw) return "调用工具";
  if (raw.includes("mcp__")) {
    const parts = raw.split("__").filter(Boolean);
    if (parts[0] === "mcp" && parts.length >= 3) {
      const server = parts[1];
      const tool = parts.slice(2).join("__");
      const joined = `${server}.${tool}`;
      return joined.length > 72 ? `${joined.slice(0, 70)}…` : joined;
    }
  }
  if (raw.length > 72) return `${raw.slice(0, 70)}…`;
  return raw;
}

export interface ExecutionSurfaceContext {
  isConnecting: boolean;
  isStreaming: boolean;
  waitingFirstChunk: boolean;
  /** Raw tool name from stream when step row not yet committed */
  toolHintFallback: string | null;
}

/** Drives header icon / animation — not only idle vs busy. */
export type ExecutionSurfaceKind =
  | "idle"
  | "finished"
  | "waiting"
  | "thinking"
  | "generating"
  | "tool";

export function getExecutionSurfaceView(
  steps: ExecutionStep[],
  ctx: ExecutionSurfaceContext,
): { label: string; kind: ExecutionSurfaceKind; toolName: string | null } {
  if (ctx.isConnecting) {
    const connectStep = steps.find(
      (step) => step.id === "connect" && step.status === "running",
    );
    return {
      label: connectStep?.title || "等待响应",
      kind: "waiting",
      toolName: null,
    };
  }

  const run = steps.find((s) => s.status === "running");
  if (run) {
    if (run.id === "connect") {
      // Stream is live but the connect row was not cleared (e.g. `Start` event not handled).
      // Do not keep showing「等待响应」— fall through to streaming / step logic below.
      if (!(ctx.isStreaming && !ctx.isConnecting)) {
        return { label: "等待响应", kind: "waiting", toolName: null };
      }
    } else if (run.id === "think") {
      return { label: "推理中", kind: "thinking", toolName: null };
    } else if (run.id === "reply" || run.id.startsWith("reply-")) {
      return { label: "解析输出", kind: "generating", toolName: null };
    } else if (run.id.startsWith("tool-")) {
      const tn = run.toolName?.trim() ?? null;
      const label = tn
        ? formatToolDisplayName(tn)
        : run.title || "调用工具";
      return { label, kind: "tool", toolName: tn };
    } else {
      return { label: run.title, kind: "generating", toolName: null };
    }
  }

  if (ctx.isStreaming && ctx.waitingFirstChunk) {
    return { label: "推理中", kind: "thinking", toolName: null };
  }
  if (ctx.isStreaming && ctx.toolHintFallback) {
    const tn = ctx.toolHintFallback.trim();
    return {
      label: formatToolDisplayName(tn),
      kind: "tool",
      toolName: tn || null,
    };
  }
  if (ctx.isStreaming) {
    return { label: "解析输出", kind: "generating", toolName: null };
  }

  if (steps.length > 0 && steps.every((s) => s.status === "done")) {
    return { label: "已完成", kind: "finished", toolName: null };
  }

  return { label: "就绪", kind: "idle", toolName: null };
}

/**
 * Single line of text for: chat header pill, task panel title row, etc.
 */
export function getExecutionSurfacePrimaryLabel(
  steps: ExecutionStep[],
  ctx: ExecutionSurfaceContext,
): string {
  return getExecutionSurfaceView(steps, ctx).label;
}
