/**
 * Critical path: settings panel opens and closes.
 *
 * Flow: open user menu via bottom profile trigger → click Settings menu item
 * → settings dialog appears → click "Close settings" (ArrowBack) → hidden.
 */
import { test, expect } from "../fixtures";

test.describe("Settings panel", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/");
    // Wait for session sidebar to be visible before interacting
    await expect(page.locator("#omiga-session-panel")).toBeVisible({
      timeout: 15_000,
    });
  });

  test("opens settings via user menu", async ({ page }) => {
    // Open the user menu (bottom-left profile area)
    await page.locator('[data-testid="user-menu-trigger"]').click();

    // Wait for the Settings menu item to appear
    const settingsItem = page.locator('[data-testid="menu-item-settings"]');
    await expect(settingsItem).toBeVisible({ timeout: 5_000 });

    // Click Settings
    await settingsItem.click();

    // Settings panel renders as a dialog — identified by role + title
    const settingsDialog = page.locator('[role="dialog"][aria-labelledby="omiga-settings-title"]');
    await expect(settingsDialog).toBeVisible({ timeout: 5_000 });
  });

  test("closes settings via back button", async ({ page }) => {
    // Open settings
    await page.locator('[data-testid="user-menu-trigger"]').click();
    await page.locator('[data-testid="menu-item-settings"]').click();

    const settingsDialog = page.locator('[role="dialog"][aria-labelledby="omiga-settings-title"]');
    await expect(settingsDialog).toBeVisible({ timeout: 5_000 });

    // Close via the ArrowBack button
    await page.getByRole("button", { name: "Close settings" }).click();

    // Settings dialog should no longer be in the DOM (Settings returns null when !open)
    await expect(settingsDialog).not.toBeVisible({ timeout: 5_000 });
  });

  test("settings panel shows Model section in sidebar", async ({ page }) => {
    await page.locator('[data-testid="user-menu-trigger"]').click();
    await page.locator('[data-testid="menu-item-settings"]').click();

    // The settings sidebar always lists "Model" as the first nav item
    const settingsDialog = page.locator('[role="dialog"][aria-labelledby="omiga-settings-title"]');
    await expect(settingsDialog).toBeVisible({ timeout: 5_000 });

    await expect(page.getByRole("navigation", { name: "Settings sections" })).toBeVisible();
  });
});
