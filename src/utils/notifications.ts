import { isPermissionGranted, requestPermission, sendNotification } from "@tauri-apps/plugin-notification";
import { invoke } from "@tauri-apps/api/core";

let permissionGranted: boolean | null = null;

const MAX_TITLE_LENGTH = 100;
const MAX_BODY_LENGTH = 200;

type PermissionState = "granted" | "denied" | "prompt" | "unknown" | "error";

async function getNotificationPermissionStatus(): Promise<PermissionState> {
  try {
    return await invoke<PermissionState>("get_notification_permission_status");
  } catch {
    return "error";
  }
}

async function requestNotificationPermissionViaBackend(): Promise<PermissionState> {
  try {
    return await invoke<PermissionState>("request_notification_permission");
  } catch {
    return "error";
  }
}

async function sendNotificationViaBackend(title: string, body: string): Promise<boolean> {
  try {
    await invoke<string>("send_notification", { title, body });
    return true;
  } catch {
    return false;
  }
}

function sanitizeNotificationText(text: string, maxLength: number): string {
  if (!text) return "";

  const sanitized = text
    .replace(/[\x00-\x08\x0B\x0C\x0E-\x1F\x7F-\x9F]/g, "")
    .replace(/\s+/g, " ")
    .trim();

  return sanitized.length > maxLength
    ? sanitized.slice(0, maxLength - 3) + "..."
    : sanitized;
}

export async function initNotifications(): Promise<boolean> {
  if (permissionGranted !== null) {
    return permissionGranted;
  }

  try {
    const [frontendGranted, backendStatus] = await Promise.all([
      isPermissionGranted(),
      getNotificationPermissionStatus(),
    ]);

    let granted = frontendGranted;

    if (!granted) {
      const permission = await requestPermission();
      granted = permission === "granted";

      if (backendStatus === "prompt") {
        await requestNotificationPermissionViaBackend();
      }
    }

    permissionGranted = granted;
    return granted;
  } catch {
    permissionGranted = false;
    return false;
  }
}

export async function showNotification(options: {
  title: string;
  body: string;
  icon?: string;
}): Promise<void> {
  const sanitizedTitle = sanitizeNotificationText(options.title, MAX_TITLE_LENGTH);
  const sanitizedBody = sanitizeNotificationText(options.body, MAX_BODY_LENGTH);

  const backendSuccess = await sendNotificationViaBackend(sanitizedTitle, sanitizedBody);
  if (backendSuccess) return;

  const granted = await initNotifications();
  if (granted) {
    sendNotification({
      title: sanitizedTitle,
      body: sanitizedBody,
      icon: options.icon,
    });
  }
}

export async function notifyTaskCompleted(taskName?: string): Promise<void> {
  await showNotification({
    title: "✅ 任务完成",
    body: taskName ? `"${taskName}" 已完成` : "所有任务已完成",
  });
}

export async function notifyTaskFailed(taskName?: string, error?: string): Promise<void> {
  await showNotification({
    title: "❌ 任务失败",
    body: taskName
      ? `"${taskName}" 执行失败${error ? `: ${error}` : ""}`
      : `任务执行失败${error ? `: ${error}` : ""}`,
  });
}

export async function notifyPermissionRequest(toolName: string, riskLevel: string): Promise<void> {
  const riskLabels: Record<string, string> = {
    safe: "安全",
    low: "低风险",
    medium: "中等风险",
    high: "高风险",
    critical: "严重风险",
  };

  await showNotification({
    title: "🔒 需要权限确认",
    body: `工具 "${toolName}" 请求执行操作（风险等级: ${riskLabels[riskLevel] || riskLevel}）`,
  });
}

export async function notifyUserInteraction(questionTitle?: string): Promise<void> {
  await showNotification({
    title: "❓ 需要您的回答",
    body: questionTitle || "AI 需要您回答一个问题才能继续",
  });
}

export async function notifyBackgroundTaskCompleted(label: string): Promise<void> {
  await showNotification({
    title: "🎉 后台任务完成",
    body: `"${label}" 已完成`,
  });
}

export async function notifyBackgroundTaskFailed(label: string, error?: string): Promise<void> {
  await showNotification({
    title: "💥 后台任务失败",
    body: `"${label}" 执行失败${error ? `: ${error}` : ""}`,
  });
}
