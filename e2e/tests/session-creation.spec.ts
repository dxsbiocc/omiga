/**
 * Critical path: new session creation.
 *
 * Flow: click "New session" nav item → mock returns a new session object →
 * verify a session item appears in the session list.
 */
import { test, expect } from "../fixtures";

test.describe("Session creation", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/");
    await expect(page.locator("#omiga-session-panel")).toBeVisible({
      timeout: 15_000,
    });
  });

  test("new-session button is visible", async ({ page }) => {
    await expect(
      page.locator('[data-testid="new-session-btn"]'),
    ).toBeVisible();
  });

  test("clicking new-session populates the session list", async ({ page }) => {
    const sessionList = page.locator('[data-testid="session-list"]');
    await expect(sessionList).toBeVisible();

    // Click "New session"
    await page.locator('[data-testid="new-session-btn"]').click();

    // After creation the Zustand store adds the session — the list should show
    // at least one item. We do not assert the exact name since the mock returns
    // a generated session and the UI may show a placeholder label.
    await expect(sessionList).toBeVisible();

    // The list container should contain at least one child Box (session row)
    // Wait for any text to appear inside the list
    await expect(sessionList).not.toBeEmpty();
  });

  test("new session does not crash the app", async ({ page }) => {
    const pageErrors: Error[] = [];
    page.on("pageerror", (err) => pageErrors.push(err));

    await page.locator('[data-testid="new-session-btn"]').click();
    // Brief wait for any async effects to settle
    await page.waitForTimeout(500);

    expect(pageErrors).toHaveLength(0);
  });
});
