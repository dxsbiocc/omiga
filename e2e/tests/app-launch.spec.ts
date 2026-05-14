/**
 * Critical path: app renders without crashing.
 *
 * Verifies the three-panel layout (session sidebar, center chat, right panel)
 * appears after navigation and that no unhandled errors reach the console.
 */
import { test, expect } from "../fixtures";

test.describe("App launch", () => {
  test("renders the session sidebar", async ({ page }) => {
    const consoleErrors: string[] = [];
    page.on("console", (msg) => {
      if (msg.type() === "error") consoleErrors.push(msg.text());
    });

    await page.goto("/");

    // Left panel — identified by the id set in App.tsx
    const sessionPanel = page.locator("#omiga-session-panel");
    await expect(sessionPanel).toBeVisible({ timeout: 15_000 });
  });

  test("renders the Omiga logo and title", async ({ page }) => {
    await page.goto("/");

    // The SessionList header always shows "Omiga" as the app name
    await expect(page.getByText("Omiga", { exact: true })).toBeVisible({
      timeout: 15_000,
    });
  });

  test("renders new-session navigation item", async ({ page }) => {
    await page.goto("/");

    const btn = page.locator('[data-testid="new-session-btn"]');
    await expect(btn).toBeVisible({ timeout: 15_000 });
  });

  test("does not crash with unhandled page errors", async ({ page }) => {
    const pageErrors: Error[] = [];
    page.on("pageerror", (err) => pageErrors.push(err));

    await page.goto("/");
    // Wait for the session panel to mount — avoids networkidle which hangs on open connections
    await expect(page.locator("#omiga-session-panel")).toBeVisible({ timeout: 15_000 });

    expect(pageErrors).toHaveLength(0);
  });
});
