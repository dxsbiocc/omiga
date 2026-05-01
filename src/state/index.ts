/**
 * Omiga client state — Zustand stores and shared layout constants.
 *
 * - `useSessionStore` — sessions, messages, Tauri chat
 * - `useWorkspaceStore` — open file in editor
 * - `useUiStore` — layout dimensions, settings dialog (layout persisted in localStorage)
 * - `useLocaleStore` — UI locale `en` | `zh-CN` (persisted)
 * - `useColorModeStore` — light / dark / system appearance (persisted)
 */

export {
  useSessionStore,
  type Session,
  type Message,
  type RoundStatus,
  PLACEHOLDER_SESSION_TITLE_PREFIX,
  UNUSED_SESSION_LABEL,
  isPlaceholderSessionTitle,
  shouldShowNewSessionPlaceholder,
  titleFromFirstUserMessage,
  isUnsetWorkspacePath,
} from "./sessionStore";
export {
  useActivityStore,
  type BackgroundJob,
  type ExecutionStep,
  type ToolUseStepDetail,
  type ToolResultStepDetail,
} from "./activityStore";
export { useWorkspaceStore } from "./workspaceStore";
export { useUiStore, type UiState } from "./uiStore";
export { useLocaleStore, type AppLocale } from "./localeStore";
export {
  useColorModeStore,
  type ColorModePreference,
} from "./themeStore";
export type { AccentPresetId } from "../theme/accentPresets";
export {
  LAYOUT_LEFT_MIN,
  LAYOUT_LEFT_MAX,
  LAYOUT_RIGHT_MIN,
  LAYOUT_RIGHT_MAX,
  LAYOUT_PANEL_MIN,
} from "./constants";
export {
  useChatComposerStore,
  type PermissionMode,
  type SandboxBackend,
  type ExecutionEnvironment,
  type LocalVenvType,
} from "./chatComposerStore";
export {
  useNotebookViewerStore,
  type NotebookViewerState,
} from "./notebookViewerStore";
export {
  useAgentStore,
  type BackgroundAgentTask,
  type BackgroundAgentStatus,
  getAgentTypeDisplayName,
  STATUS_COLORS,
  STATUS_ICONS,
  STATUS_LABELS,
} from "./agentStore";
export {
  usePermissionStore,
  type RiskLevel,
  type ToolPermissionMode,
  type PermissionCheckResult,
  type RiskInfo,
  type PermissionRule,
} from "./permissionStore";
export { useExtensionStore } from "./extensionStore";
export { usePluginStore } from "./pluginStore";
