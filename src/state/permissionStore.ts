import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import { useSessionStore } from "./sessionStore";

export type RiskLevel = "safe" | "low" | "medium" | "high" | "critical";

// 前端友好的 PermissionMode，与后端 PermissionModeInput 对应
export type ToolPermissionMode =
  | "AskEveryTime"
  | "Session"
  | { TimeWindow: { minutes: number } }
  | "Plan"
  | "Auto";

export interface RiskInfo {
  category: string;
  severity: RiskLevel;
  description: string;
  mitigation?: string;
}

export interface PermissionCheckResult {
  allowed: boolean;
  requires_approval: boolean;
  request_id?: string;
  tool_name: string;
  risk_level: RiskLevel;
  risk_description: string;
  detected_risks: RiskInfo[];
  recommendations: string[];
  arguments?: Record<string, unknown>; // 原始参数
  /** 与后端 `check_tool` 一致的会话 id，批准时必须回传 */
  session_id?: string;
}

/** Rust `PermissionModeInput` 为 externally tagged enum，需序列化为 `{"Session":null}` 等形式 */
export function permissionModeToRustJson(
  mode: ToolPermissionMode,
): Record<string, unknown> {
  if (
    typeof mode === "object" &&
    mode !== null &&
    "TimeWindow" in mode &&
    typeof (mode as { TimeWindow?: { minutes?: number } }).TimeWindow ===
      "object"
  ) {
    const tw = (mode as { TimeWindow: { minutes: number } }).TimeWindow;
    return { TimeWindow: tw };
  }
  const unit = mode as string;
  return { [unit]: null };
}

export interface PermissionRule {
  id: string;
  name: string;
  description?: string;
  tool_matcher: { type: "Exact" | "Wildcard" | "Regex" | "Any"; pattern?: string };
  path_matcher?: { type: string; pattern?: string };
  mode: ToolPermissionMode;
  priority: number;
}

export interface DenialRecord {
  id: string;
  timestamp: string;
  tool_name: string;
  reason: string;
}

interface PermissionState {
  // 当前待处理的权限请求
  pendingRequest: PermissionCheckResult | null;
  // 权限规则列表
  rules: PermissionRule[];
  // 最近拒绝记录
  recentDenials: DenialRecord[];
  // 是否正在检查
  checking: boolean;
  // 错误信息
  error: string | null;

  // Actions
  checkPermission: (
    sessionId: string,
    toolName: string,
    args: Record<string, unknown>
  ) => Promise<boolean>;
  setPendingRequest: (request: PermissionCheckResult | null) => void;
  approveRequest: (mode: ToolPermissionMode) => Promise<void>;
  denyRequest: (reason?: string) => Promise<void>;
  clearPending: () => void;
  clearError: () => void;
  loadRules: () => Promise<void>;
  addRule: (rule: Omit<PermissionRule, "id">) => Promise<void>;
  deleteRule: (id: string) => Promise<void>;
  loadRecentDenials: () => Promise<void>;
}

export const usePermissionStore = create<PermissionState>((set, get) => ({
  pendingRequest: null,
  rules: [],
  recentDenials: [],
  checking: false,
  error: null,

  checkPermission: async (sessionId, toolName, args) => {
    set({ checking: true, error: null });
    try {
      const result = await invoke<PermissionCheckResult>("permission_check", {
        request: {
          sessionId,
          toolName,
          arguments: args,
        },
      });

      if (result.allowed) {
        return true;
      }

      if (result.requires_approval) {
        set({ pendingRequest: result });
        return false;
      }

      // 被拒绝
      return false;
    } catch (err) {
      const errorMsg = err instanceof Error ? err.message : String(err);
      console.error("Permission check failed:", errorMsg);
      set({ error: `权限检查失败: ${errorMsg}` });
      return false;
    } finally {
      set({ checking: false });
    }
  },

  setPendingRequest: (request) => {
    set({ pendingRequest: request });
  },

  approveRequest: async (mode) => {
    const { pendingRequest } = get();
    if (!pendingRequest) return;

    // Get actual session ID from session store
    const sessionId =
      pendingRequest.session_id ??
      useSessionStore.getState().currentSession?.id ??
      "default";

    set({ error: null });
    try {
      await invoke("permission_approve", {
        request: {
          sessionId,
          toolName: pendingRequest.tool_name,
          arguments: pendingRequest.arguments ?? {},
          mode: permissionModeToRustJson(mode),
          requestId: pendingRequest.request_id ?? null,
        },
      });

      set({ pendingRequest: null });
    } catch (err) {
      const errorMsg = err instanceof Error ? err.message : String(err);
      console.error("Approve failed:", errorMsg);
      set({ error: `批准失败: ${errorMsg}` });
      throw err;
    }
  },

  denyRequest: async (reason = "用户拒绝") => {
    const { pendingRequest } = get();
    if (!pendingRequest) return;

    // Get actual session ID from session store
    const sessionId =
      pendingRequest.session_id ??
      useSessionStore.getState().currentSession?.id ??
      "default";

    set({ error: null });
    try {
      await invoke("permission_deny", {
        request: {
          sessionId,
          toolName: pendingRequest.tool_name,
          arguments: pendingRequest.arguments ?? {},
          reason,
          requestId: pendingRequest.request_id ?? null,
        },
      });

      set({ pendingRequest: null });
    } catch (err) {
      const errorMsg = err instanceof Error ? err.message : String(err);
      console.error("Deny failed:", errorMsg);
      set({ error: `拒绝失败: ${errorMsg}` });
      throw err;
    }
  },

  clearPending: () => set({ pendingRequest: null }),
  clearError: () => set({ error: null }),

  loadRules: async () => {
    try {
      const rules = await invoke<PermissionRule[]>("permission_list_rules");
      set({ rules });
    } catch (err) {
      console.error("Failed to load rules:", err);
      set({ error: "加载规则失败" });
    }
  },

  addRule: async (rule) => {
    try {
      await invoke("permission_add_rule", {
        request: {
          rule: { ...rule, id: crypto.randomUUID() },
        },
      });
      await get().loadRules();
    } catch (err) {
      console.error("Failed to add rule:", err);
      set({ error: "添加规则失败" });
      throw err;
    }
  },

  deleteRule: async (id) => {
    try {
      await invoke("permission_delete_rule", { id });
      await get().loadRules();
    } catch (err) {
      console.error("Failed to delete rule:", err);
      set({ error: "删除规则失败" });
      throw err;
    }
  },

  loadRecentDenials: async () => {
    try {
      const denials = await invoke<DenialRecord[]>("permission_get_recent_denials", {
        limit: 50,
      });
      set({ recentDenials: denials });
    } catch (err) {
      console.error("Failed to load denials:", err);
    }
  },
}));
