/**
 * Maps `window` event `openSettings` → `detail.tab` string to Settings sidebar index.
 * Keep in sync with `SETTINGS_SECTIONS` in `Settings/index.tsx` (0–15). Language is not a tab — use profile menu + locale store.
 * Optional `detail.executionSubTab`: legacy value; Execution currently exposes SSH only.
 */
export type OpenSettingsEventDetail = {
  tab?: string;
  executionSubTab?: number;
};

export type ResolvedOpenSettingsTarget = {
  tabIndex: number;
  executionSubTab: number;
};

export const OPEN_SETTINGS_TAB_DETAIL: Record<string, number> = {
  /** LLM provider & keys */
  provider: 0,
  advanced: 1,
  search: 13,
  "web-search": 13,
  "search-settings": 13,
  permissions: 2,
  theme: 3,
  harness: 10,
  "runtime-constraints": 10,
  trace: 10,
  /** Integrations — Plugins / MCP / Skills */
  plugins: 4,
  extensions: 4,
  "computer-use": 4,
  computer: 4,
  connectors: 14,
  connector: 14,
  apps: 14,
  /** @deprecated use `plugins` — left nav Customize opens Plugins */
  customize: 4,
  mcp: 5,
  skills: 6,
  /** Jupyter / .ipynb viewer settings live in Plugins → Notebook Helper */
  notebook: 4,
  jupyter: 4,
  ipynb: 4,
  /** Browser Operator setup lives in Execution; runtime access is handled through plugins. */
  "browser-operator": 9,
  browser_operator: 9,
  "browser-use": 9,
  browser: 9,
  /** Profile files: ~/.omiga/SOUL.md, USER.md, MEMORY.md */
  profile: 12,
  soul: 12,
  user: 12,
  preferences: 12,
  "user-profile": 12,
  "agent-profile": 12,
  /** Unified Memory system (Wiki + PageIndex) */
  memory: 8,
  wiki: 8,
  knowledge: 8,
  "memory-v2": 8,
  unified: 8,
  /** Execution environments (SSH) — `omiga.yaml` + ~/.ssh/config */
  execution: 9,
  ssh: 9,
  "execution-env": 9,
  "execution-envs": 9,
  /** Cron / scheduled jobs surface */
  schedule: 15,
  cron: 15,
  "cron-jobs": 15,
  "scheduled-jobs": 15,
};

const OPEN_SETTINGS_EXECUTION_SUBTAB_DETAIL: Record<string, number> = {
  ssh: 0,
};

function clampExecutionSubTab(value: unknown): number {
  void value;
  return 0;
}

export function resolveOpenSettingsTarget(
  detail?: OpenSettingsEventDetail,
): ResolvedOpenSettingsTarget {
  const key = detail?.tab;
  const tabIndex =
    key != null && OPEN_SETTINGS_TAB_DETAIL[key] !== undefined
      ? OPEN_SETTINGS_TAB_DETAIL[key]
      : 0;
  const defaultExecutionSubTab =
    key != null ? OPEN_SETTINGS_EXECUTION_SUBTAB_DETAIL[key] : undefined;

  return {
    tabIndex,
    executionSubTab: clampExecutionSubTab(
      detail?.executionSubTab ?? defaultExecutionSubTab,
    ),
  };
}
