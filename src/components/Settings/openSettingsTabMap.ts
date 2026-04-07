/**
 * Maps `window` event `openSettings` → `detail.tab` string to Settings sidebar index.
 * Keep in sync with `SETTINGS_SECTIONS` in `Settings/index.tsx` (0–7). Language is not a tab — use profile menu + locale store.
 */
export const OPEN_SETTINGS_TAB_DETAIL: Record<string, number> = {
  /** LLM provider & keys */
  provider: 0,
  advanced: 1,
  permissions: 2,
  theme: 3,
  /** Integrations — Plugins / MCP / Skills */
  plugins: 4,
  extensions: 4,
  /** @deprecated use `plugins` — left nav Customize opens Plugins */
  customize: 4,
  mcp: 5,
  skills: 6,
  /** Jupyter / .ipynb viewer */
  notebook: 7,
  jupyter: 7,
  ipynb: 7,
  /** LLM Wiki knowledge base */
  wiki: 8,
  knowledge: 8,
};
