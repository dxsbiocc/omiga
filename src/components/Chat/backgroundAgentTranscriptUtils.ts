import { stringifyUnknown } from "../../utils/stringifyUnknown";

export type SidechainValueKind = "message" | "toolOutput" | "toolArguments";

export type BackgroundTaskSummary = {
  agent_type?: string | null;
  description?: string | null;
  status?: string | null;
  result_summary?: unknown;
  error_message?: unknown;
};

const OPAQUE_OBJECT_TEXT = "[object Object]";

export function isOpaqueObjectText(value: unknown): boolean {
  return typeof value === "string" && value.trim() === OPAQUE_OBJECT_TEXT;
}

function prettyPrintJsonString(value: string): string | null {
  const trimmed = value.trim();
  if (
    !(
      (trimmed.startsWith("{") && trimmed.endsWith("}")) ||
      (trimmed.startsWith("[") && trimmed.endsWith("]"))
    )
  ) {
    return null;
  }

  try {
    return JSON.stringify(JSON.parse(trimmed), null, 2);
  } catch {
    return null;
  }
}

export function buildOpaqueSidechainFallback(args: {
  kind: SidechainValueKind;
  task?: BackgroundTaskSummary | null;
  taskLabel?: string;
}): string {
  const subject = args.task?.description?.trim() || args.taskLabel?.trim();
  const status = args.task?.status?.trim();
  const summary = normalizeSidechainValue(args.task?.result_summary, undefined, {
    allowOpaqueFallback: false,
  }).trim();
  const error = normalizeSidechainValue(args.task?.error_message, undefined, {
    allowOpaqueFallback: false,
  }).trim();
  const noun =
    args.kind === "toolArguments"
      ? "工具参数"
      : args.kind === "toolOutput"
        ? "工具输出"
        : "消息内容";
  const lines = [`${noun}历史记录保存成了未序列化对象，已改为显示可用上下文。`];

  if (subject) lines.push(`任务：${subject}`);
  if (status) lines.push(`状态：${status}`);
  if (error && !isOpaqueObjectText(error)) lines.push(`错误：${error}`);
  if (summary && !isOpaqueObjectText(summary)) lines.push(`摘要：${summary}`);
  if (lines.length === 1) lines.push("请查看上方任务详情或 Trace 面板获取上下文。");

  return lines.join("\n");
}

export function normalizeSidechainValue(
  value: unknown,
  opaqueFallback?: string,
  options: { allowOpaqueFallback?: boolean } = {},
): string {
  const allowOpaqueFallback = options.allowOpaqueFallback ?? true;

  if (typeof value === "string") {
    if (value.trim() === OPAQUE_OBJECT_TEXT) {
      return allowOpaqueFallback
        ? (opaqueFallback ?? "未序列化对象")
        : OPAQUE_OBJECT_TEXT;
    }
    return prettyPrintJsonString(value) ?? value;
  }
  if (value == null) return "";

  const normalized = stringifyUnknown(value);
  if (normalized.trim() === OPAQUE_OBJECT_TEXT) {
    return allowOpaqueFallback
      ? (opaqueFallback ?? "未序列化对象")
      : OPAQUE_OBJECT_TEXT;
  }
  return normalized;
}
