/**
 * Shared Playwright fixture that injects the Tauri IPC mock before every test.
 *
 * Import `{ test, expect }` from this file instead of `@playwright/test` in
 * every spec so the mock is always active.
 */
import { test as base, expect, Page } from "@playwright/test";
import { TAURI_MOCK_SCRIPT } from "./setup/tauri-mock";

type OmigaFixtures = {
  /** A page with the Tauri mock pre-injected. */
  page: Page;
};

export const test = base.extend<OmigaFixtures>({
  page: async ({ page }, use) => {
    // Mark onboarding as completed so the blocking Dialog doesn't cover the UI.
    // zustand-persist stores { state: {...}, version: N } under the store name.
    await page.addInitScript(() => {
      const stored = localStorage.getItem("omiga-ui");
      const parsed = stored ? JSON.parse(stored) : { state: {}, version: 0 };
      parsed.state = { ...parsed.state, onboardingCompleted: true };
      localStorage.setItem("omiga-ui", JSON.stringify(parsed));
    });
    await page.addInitScript(TAURI_MOCK_SCRIPT);
    await use(page);
  },
});

export { expect };
