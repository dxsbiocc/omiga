/**
 * Playwright global setup.
 * Nothing async is needed here — the mock is injected per-page via
 * `page.addInitScript` in the shared fixture (see e2e/fixtures.ts).
 */
export default async function globalSetup() {
  // No-op: Tauri mock is applied per-page in fixtures.ts
}
